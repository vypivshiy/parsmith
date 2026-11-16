//! Rust `JsonDef` struct/type-alias generation and `from_value` allowlist JSON projector emission.

use crate::ast::{BaseType, JsonDef, JsonFieldDef, Module, TypeInfo};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::rust::{
    RustEmitter, emit_rust_doc, rust_ident, rust_key_type, rust_literal, rust_pascal, rust_str,
};
use crate::codegen::writer::{CodeWriter, wln};
use crate::gjson::classify_gjson_path;

impl RustEmitter {
    /// Resolves the Rust type of a [`JsonFieldDef`], accounting for inline `(dict)` fields and `@omitempty`.
    pub fn resolve_json_field_type(&self, field: &JsonFieldDef, module: &Module) -> String {
        let optional = field.is_optional
            || field.omitempty
            || field.type_info.is_optional
            || field.type_info.omitempty;
        if field.is_dict && !field.type_info.is_dict {
            let key_type = rust_key_type(
                field
                    .key_type
                    .as_ref()
                    .or(field.type_info.key_type.as_deref()),
            );
            let val_type = field
                .value_type
                .as_ref()
                .or(field.type_info.value_type.as_deref())
                .map(|vt| Self::resolve_type(vt, module))
                .unwrap_or_else(|| "String".to_owned());
            let mut ty = format!("std::collections::HashMap<{key_type}, {val_type}>");
            if field.type_info.is_array {
                ty = format!("Vec<{ty}>");
            }
            if optional {
                ty = format!("Option<{ty}>");
            }
            return ty;
        }
        let mut info = field.type_info.clone();
        if optional {
            info.is_optional = true;
        }
        Self::resolve_type(&info, module)
    }

    /// Emits a `JsonDef` struct or dictionary type alias with `from_value` and `impl rt::Decode`.
    pub(crate) fn emit_json_def_impl(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let schema_name = rust_pascal(&json_def.name);
        let name = format!("{schema_name}Json");

        if let Some(ref doc) = json_def.doc {
            emit_rust_doc(doc, "///", writer);
        }

        if json_def.is_dict {
            let key_type = rust_key_type(json_def.key_type.as_ref());
            let val_type = json_def
                .value_type
                .as_ref()
                .map(|vt| Self::resolve_type(vt, module))
                .unwrap_or_else(|| "String".to_owned());
            wln!(
                writer,
                "pub type {name} = std::collections::HashMap<{key_type}, {val_type}>;"
            );
            writer.blankline();
            return;
        }

        builder.require_import("use serde_json::Value;");
        builder.require_import("use serde::Deserialize;");

        writer.writeln("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]");
        wln!(writer, "pub struct {name} {{");
        writer.indented(|writer| {
            for field in &json_def.fields {
                if field.skip || field.type_info.skip {
                    continue;
                }
                if let Some(ref fdoc) = field.doc {
                    emit_rust_doc(fdoc, "///", writer);
                }
                let typ = self.resolve_json_field_type(field, module);
                let mut serde_items: Vec<String> = Vec::new();
                if let Some(ref alias) = field.alias {
                    let (is_direct_key, unescaped_key) = classify_gjson_path(alias);
                    if is_direct_key {
                        serde_items.push(format!("alias = {}", rust_str(&unescaped_key)));
                    }
                }
                if field.omitempty || field.type_info.omitempty {
                    serde_items.push("skip_serializing_if = \"Option::is_none\"".to_owned());
                }
                if !serde_items.is_empty() {
                    wln!(writer, "#[serde({})]", serde_items.join(", "));
                }
                let f_ident = rust_ident(&field.name);
                wln!(writer, "pub {f_ident}: {typ},");
            }
        });
        writer.writeln("}");
        writer.blankline();

        let schema_lit = rust_str(&schema_name);
        wln!(writer, "impl {name} {{");
        writer.indented(|writer| {
            writer.writeln("pub fn from_value(value: Value) -> Result<Self, rt::RtError> {");
            writer.indented(|writer| {
                wln!(writer,
                    "value.as_object().ok_or_else(|| rt::RtError::new({schema_lit}, \"expected JSON object\"))?;"
                );
                for field in &json_def.fields {
                    if field.skip || field.type_info.skip {
                        continue;
                    }
                    self.emit_json_field_decode(field, &schema_name, module, writer);
                }
                if json_def
                    .fields
                    .iter()
                    .all(|f| f.skip || f.type_info.skip)
                {
                    writer.writeln("Ok(Self {})");
                } else {
                    writer.writeln("Ok(Self {");
                    writer.indented(|writer| {
                        for field in &json_def.fields {
                            if field.skip || field.type_info.skip {
                                continue;
                            }
                            let f_ident = rust_ident(&field.name);
                            wln!(writer, "{f_ident},");
                        }
                    });
                    writer.writeln("})");
                }
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();

        wln!(writer, "impl rt::Decode for {name} {{");
        writer.indented(|writer| {
            writer
                .writeln("fn decode(value: Value, _context: &str) -> Result<Self, rt::RtError> {");
            writer.indented(|writer| {
                writer.writeln("Self::from_value(value)");
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_json_field_decode(
        &self,
        field: &JsonFieldDef,
        schema_name: &str,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let info: &TypeInfo = &field.type_info;
        let name = rust_ident(&field.name);
        let typ = self.resolve_json_field_type(field, module);
        if let Some(ref lit) = field.value {
            let lit_expr = rust_literal(lit, &typ);
            wln!(writer, "let {name}: {typ} = {lit_expr};");
            return;
        }
        let wire = field.alias.as_deref().unwrap_or(&field.name);
        let optional = field.is_optional || field.omitempty || info.is_optional || info.omitempty;
        let inner_type = typ
            .strip_prefix("Option<")
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(&typ);
        let path_lit = rust_str(wire);
        let ctx_lit = rust_str(&format!("{schema_name}.{}", field.name));

        let is_dict = field.is_dict
            || info.is_dict
            || info
                .ref_name()
                .and_then(|r| module.json_def(r))
                .is_some_and(|j| j.is_dict);

        wln!(
            writer,
            "let {name} = match rt::json_path(&value, {path_lit}, {ctx_lit}) {{"
        );
        writer.indented(|writer| {
            writer.writeln("Ok(raw) if !raw.is_null() => {");
            writer.indented(|writer| {
                if !is_dict
                    && let BaseType::Json(ref ref_name) = info.base
                {
                    let child = format!("{}Json", rust_pascal(ref_name));
                    let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
                    if info.is_array || def_is_arr {
                        wln!(writer,
                            "let values = raw.as_array().ok_or_else(|| rt::RtError::new({ctx_lit}, \"expected JSON array\"))?;"
                        );
                        let decoded = format!(
                            "values.iter().cloned().map({child}::from_value).collect::<Result<Vec<_>, _>>()?"
                        );
                        if optional {
                            wln!(writer, "Some({decoded})");
                        } else {
                            writer.writeln(decoded);
                        }
                    } else {
                        let decoded = format!("{child}::from_value(raw.clone())?");
                        if optional {
                            wln!(writer, "Some({decoded})");
                        } else {
                            writer.writeln(decoded);
                        }
                    }
                } else {
                    let decoded = format!("rt::decode::<{inner_type}>(raw.clone(), {ctx_lit})?");
                    if optional {
                        wln!(writer, "Some({decoded})");
                    } else {
                        writer.writeln(decoded);
                    }
                }
            });
            writer.writeln("},");
            if optional {
                writer.writeln("Ok(_) => None,");
                writer.writeln("Err(_) => None,");
            } else {
                wln!(writer,
                    "Ok(_) => return Err(rt::RtError::new({ctx_lit}, \"required field is null\")),"
                );
                writer.writeln("Err(error) => return Err(error),");
            }
        });
        writer.writeln("};");
    }
}

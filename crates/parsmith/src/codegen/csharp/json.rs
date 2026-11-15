//! C# (.NET 8+) `JsonDef` record generation and allowlist JSON projector emission (`FromNode` / `Rt.Project<Name>`).

use crate::ast::{BaseType, JsonDef, JsonFieldDef, Module, TypeInfo};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::csharp::{
    CSharpEmitter, csharp_ident, csharp_key_type, csharp_pascal, csharp_str, emit_csharp_doc,
};
use crate::codegen::writer::{CodeWriter, wln};
use crate::gjson::classify_gjson_path;
use crate::linter::symbols::to_snake_case;

impl CSharpEmitter {
    /// Resolves the C# type of a [`JsonFieldDef`], accounting for inline `(dict)` fields, `?` nullability, and `@omitempty`.
    pub fn resolve_json_field_type(&self, field: &JsonFieldDef, module: &Module) -> String {
        let optional = field.is_optional
            || field.omitempty
            || field.type_info.is_optional
            || field.type_info.omitempty;
        if field.is_dict && !field.type_info.is_dict {
            let key_type = csharp_key_type(
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
                .unwrap_or_else(|| "string".to_owned());
            let mut ty = format!("Dictionary<{key_type}, {val_type}>");
            if field.type_info.is_array {
                ty = format!("List<{ty}>");
            }
            if optional {
                ty.push('?');
            }
            return ty;
        }
        let mut info = field.type_info.clone();
        if optional {
            info.is_optional = true;
        }
        Self::resolve_type(&info, module)
    }

    /// Renders a strict C# JSON decoder expression for a non-null `JsonNode` (`node_expr`) into `info`.
    pub(crate) fn render_json_decode_expr(
        info: &TypeInfo,
        node_expr: &str,
        ctx_expr: &str,
        module: &Module,
    ) -> String {
        if info.is_dict {
            if info.is_array {
                let elem_ti = TypeInfo {
                    is_array: false,
                    is_optional: false,
                    omitempty: false,
                    ..info.clone()
                };
                let elem_dec = Self::render_json_decode_expr(&elem_ti, "_item", ctx_expr, module);
                return format!("Rt.DecodeList({node_expr}, {ctx_expr}, _item => {elem_dec})");
            }
            let key_parser = match info.key_type.as_deref().map(|k| &k.base) {
                Some(BaseType::Int) => "Rt.ParseIntKey",
                Some(BaseType::Float) => "Rt.ParseFloatKey",
                Some(BaseType::Bool) => "Rt.ParseBoolKey",
                _ => "Rt.ParseStringKey",
            };
            let val_ti = info
                .value_type
                .as_deref()
                .cloned()
                .unwrap_or_else(TypeInfo::string);
            let val_dec = if val_ti.is_optional || val_ti.omitempty {
                let non_opt = TypeInfo {
                    is_optional: false,
                    omitempty: false,
                    ..val_ti.clone()
                };
                let inner_dec = Self::render_json_decode_expr(&non_opt, "_v", ctx_expr, module);
                let opt_cs_ty = Self::resolve_type(&val_ti, module);
                format!("(_v is null ? ({opt_cs_ty})null : {inner_dec})")
            } else {
                Self::render_json_decode_expr(&val_ti, "_v", ctx_expr, module)
            };
            return format!(
                "Rt.DecodeDict({node_expr}, {ctx_expr}, {key_parser}, _v => {val_dec})"
            );
        }

        if let BaseType::Json(ref ref_name) = info.base {
            let child = format!("{}Json", csharp_pascal(ref_name));
            let target_def = module.json_def(ref_name);
            let is_dict_def = target_def.is_some_and(|j| j.is_dict);
            let def_is_arr = target_def.is_some_and(|j| j.is_array);
            if is_dict_def {
                if info.is_array {
                    return format!(
                        "Rt.DecodeList({node_expr}, {ctx_expr}, _item => {child}.FromNode(_item, {ctx_expr}))"
                    );
                }
                return format!("{child}.FromNode({node_expr}, {ctx_expr})");
            }
            if info.is_array || def_is_arr {
                return format!(
                    "Rt.DecodeList({node_expr}, {ctx_expr}, _item => {child}.FromNode(_item, {ctx_expr}))"
                );
            }
            return format!("{child}.FromNode({node_expr}, {ctx_expr})");
        }

        if info.is_array {
            let elem_ti = TypeInfo {
                is_array: false,
                is_optional: false,
                omitempty: false,
                ..info.clone()
            };
            let elem_dec = Self::render_json_decode_expr(&elem_ti, "_item", ctx_expr, module);
            return format!("Rt.DecodeList({node_expr}, {ctx_expr}, _item => {elem_dec})");
        }

        match info.base {
            BaseType::String | BaseType::Auto | BaseType::DictValue => {
                format!("Rt.DecodeString({node_expr}, {ctx_expr})")
            }
            BaseType::Int => format!("Rt.DecodeInt({node_expr}, {ctx_expr})"),
            BaseType::Float => format!("Rt.DecodeFloat({node_expr}, {ctx_expr})"),
            BaseType::Bool => format!("Rt.DecodeBool({node_expr}, {ctx_expr})"),
            BaseType::Null => "null".to_owned(),
            _ => format!("Rt.DecodeString({node_expr}, {ctx_expr})"),
        }
    }

    /// Emits a `JsonDef` record/static projector class and its `Rt.Project<Name>` allowlist projector.
    pub(crate) fn emit_json_def_impl(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let schema_pascal = to_snake_case(&json_def.name);
        let _ = schema_pascal;
        let clean_pascal = crate::json_to_kdl::to_pascal_case(&json_def.name).replace('-', "");
        let name = format!("{clean_pascal}Json");
        let schema_lit = csharp_str(&clean_pascal);

        if let Some(ref doc) = json_def.doc {
            emit_csharp_doc(doc, "///", writer);
        }

        if json_def.is_dict {
            let key_type = csharp_key_type(json_def.key_type.as_ref());
            let val_ti = json_def.value_type.clone().unwrap_or_else(TypeInfo::string);
            let val_type = Self::resolve_type(&val_ti, module);
            let dict_type = format!("Dictionary<{key_type}, {val_type}>");
            let dict_ti = TypeInfo::dict(
                json_def.key_type.clone().unwrap_or_else(TypeInfo::string),
                val_ti,
            );
            let decode_expr = Self::render_json_decode_expr(&dict_ti, "node", "context", module);

            wln!(writer, "public static class {name}");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(writer,
                    "public static {dict_type} FromNode(JsonNode? node, string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    wln!(writer, "{decode_expr};");
                });
                writer.blankline();
                wln!(writer,
                    "public static {dict_type} FromJson(string raw, string path = \"\", string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    writer.writeln("FromNode(Rt.ParseJson(raw, path, context), context);");
                });
            });
            writer.writeln("}");
            writer.blankline();

            writer.writeln("public static partial class Rt");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(writer,
                    "public static {dict_type} Project{clean_pascal}(JsonNode? node, string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    wln!(writer, "{name}.FromNode(node, context);");
                });
            });
            writer.writeln("}");
            writer.blankline();
            return;
        }

        let active_fields: Vec<&JsonFieldDef> = json_def
            .fields
            .iter()
            .filter(|f| !f.skip && !f.type_info.skip)
            .collect();

        if active_fields.is_empty() {
            wln!(writer, "public sealed record {name}()");
        } else {
            wln!(writer, "public sealed record {name}(");
            writer.indented(|writer| {
                for (idx, field) in active_fields.iter().enumerate() {
                    let typ = self.resolve_json_field_type(field, module);
                    let wire_prop = to_snake_case(&field.name);
                    let wire_lit = csharp_str(&wire_prop);
                    let prop_ident = csharp_pascal(&field.name);
                    let comma = if idx + 1 < active_fields.len() {
                        ","
                    } else {
                        ")"
                    };
                    if field.omitempty || field.type_info.omitempty {
                        wln!(writer,
                            "[property: JsonPropertyName({wire_lit}), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] {typ} {prop_ident}{comma}"
                        );
                    } else {
                        wln!(writer,
                            "[property: JsonPropertyName({wire_lit})] {typ} {prop_ident}{comma}"
                        );
                    }
                }
            });
        }

        writer.writeln("{");
        writer.indented(|writer| {
            wln!(writer,
                "public static {name} FromNode(JsonNode? node, string context = {schema_lit})"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("Rt.ExpectObject(node, context);");
                for field in &active_fields {
                    self.emit_json_field_decode(field, &clean_pascal, module, writer);
                }
                if active_fields.is_empty() {
                    wln!(writer, "return new {name}();");
                } else {
                    let args: Vec<String> = active_fields
                        .iter()
                        .map(|f| format!("_f_{}", to_snake_case(&f.name)))
                        .collect();
                    wln!(writer, "return new {name}({});", args.join(", "));
                }
            });
            writer.writeln("}");
            writer.blankline();

            wln!(writer,
                "public static {name} FromJson(string raw, string path = \"\", string context = {schema_lit}) =>"
            );
            writer.indented(|writer| {
                writer.writeln("FromNode(Rt.ParseJson(raw, path, context), context);");
            });

            if json_def.is_array {
                writer.blankline();
                wln!(writer,
                    "public static List<{name}> ListFromNode(JsonNode? node, string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    writer.writeln("Rt.DecodeList(node, context, _item => FromNode(_item, context));");
                });
                writer.blankline();
                wln!(writer,
                    "public static List<{name}> ListFromJson(string raw, string path = \"\", string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    writer.writeln("ListFromNode(Rt.ParseJson(raw, path, context), context);");
                });
            }
        });
        writer.writeln("}");
        writer.blankline();

        writer.writeln("public static partial class Rt");
        writer.writeln("{");
        writer.indented(|writer| {
            if json_def.is_array {
                wln!(writer,
                    "public static List<{name}> Project{clean_pascal}(JsonNode? node, string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    wln!(writer, "{name}.ListFromNode(node, context);");
                });
                writer.blankline();
                wln!(writer,
                    "public static {name} Project{clean_pascal}Item(JsonNode? node, string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    wln!(writer, "{name}.FromNode(node, context);");
                });
            } else {
                wln!(writer,
                    "public static {name} Project{clean_pascal}(JsonNode? node, string context = {schema_lit}) =>"
                );
                writer.indented(|writer| {
                    wln!(writer, "{name}.FromNode(node, context);");
                });
            }
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
        let snake = to_snake_case(&field.name);
        let local_var = format!("_f_{snake}");
        let raw_var = format!("_raw_{snake}");
        let miss_var = format!("_miss_{snake}");
        let wire = field.alias.as_deref().unwrap_or(&field.name);
        let (is_direct_key, _) = classify_gjson_path(wire);
        let optional = field.is_optional
            || field.omitempty
            || field.type_info.is_optional
            || field.type_info.omitempty;
        let full_typ = self.resolve_json_field_type(field, module);

        let mut decode_ti = field.type_info.clone();
        if field.is_dict && !decode_ti.is_dict {
            decode_ti = TypeInfo::dict(
                field.key_type.clone().unwrap_or_else(TypeInfo::string),
                field.value_type.clone().unwrap_or_else(TypeInfo::string),
            )
            .with_array(field.type_info.is_array);
        }
        decode_ti.is_optional = false;
        decode_ti.omitempty = false;

        let path_lit = csharp_str(wire);
        let missing_field_msg_lit = csharp_str(&format!(
            "Required JSON field '{wire}' (mapped to '{}') is missing",
            field.name
        ));
        let null_field_msg_lit = csharp_str(&format!(
            "Field '{wire}' is null, but '{}' is not nullable",
            field.name
        ));
        let path_err_prefix_lit = csharp_str("Missing key '");
        let path_err_suffix_lit = csharp_str(&format!("' in path '{wire}'"));
        let ctx_lit = csharp_str(&format!("{schema_name}.{}", field.name));
        let decode_expr = Self::render_json_decode_expr(&decode_ti, &raw_var, &ctx_lit, module);

        let _ = csharp_ident;

        if optional {
            wln!(writer, "{full_typ} {local_var} = default;");
            wln!(
                writer,
                "if (Rt.TryJsonPath(node, {path_lit}, {ctx_lit}, out var {raw_var}, out _) && {raw_var} is not null)"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(writer, "{local_var} = {decode_expr};");
            });
            writer.writeln("}");
        } else {
            wln!(
                writer,
                "if (!Rt.TryJsonPath(node, {path_lit}, {ctx_lit}, out var {raw_var}, out var {miss_var}))"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                if is_direct_key {
                    wln!(writer,
                        "throw new Rt.RtJsonFieldMissingError({ctx_lit}, {missing_field_msg_lit});"
                    );
                } else {
                    wln!(writer,
                        "throw new Rt.RtJsonPathError({ctx_lit}, {path_err_prefix_lit} + ({miss_var} ?? {path_lit}) + {path_err_suffix_lit});"
                    );
                }
            });
            writer.writeln("}");
            wln!(writer, "if ({raw_var} is null)");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(
                    writer,
                    "throw new Rt.RtJsonFieldMissingError({ctx_lit}, {null_field_msg_lit});"
                );
            });
            writer.writeln("}");
            wln!(writer, "{full_typ} {local_var} = {decode_expr};");
        }
    }
}

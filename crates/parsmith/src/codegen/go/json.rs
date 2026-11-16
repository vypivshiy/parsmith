//! Go `JsonDef` struct/alias emission and strict `UnmarshalJSON` projector generation.

use crate::ast::{BaseType, JsonDef, JsonFieldDef, Module, TypeInfo};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::go::runtime::go_str;
use crate::codegen::go::{GoEmitter, emit_aligned_struct_fields, emit_go_doc, go_literal};
use crate::codegen::writer::{CodeWriter, wln};
use crate::gjson::GjsonStep;
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::to_snake_case;

impl GoEmitter {
    /// Emits a `JsonDef` type declaration (`struct` or `map` alias) and its strict `UnmarshalJSON` projector.
    pub(crate) fn emit_json_def_impl(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = to_pascal_case(&json_def.name);
        if let Some(ref doc) = json_def.doc {
            emit_go_doc(doc, writer);
        }

        if json_def.is_dict {
            let key_type = resolve_dict_key_type(json_def.key_type.as_ref());
            let val_type = match json_def.value_type.as_ref() {
                Some(vt) => Self::resolve_type(vt, module),
                None => "any".to_owned(),
            };
            wln!(writer, "type {name}Json = map[{key_type}]{val_type}");
            writer.blankline();
            return;
        }

        let active_fields: Vec<&JsonFieldDef> =
            json_def.fields.iter().filter(|f| !f.skip).collect();

        let mut struct_fields: Vec<(String, String, Option<String>)> =
            Vec::with_capacity(active_fields.len());
        for field in &active_fields {
            let field_name = to_pascal_case(&field.name);
            let go_type = Self::resolve_type(&field.type_info, module);
            let raw_tag = to_snake_case(&field.name);
            let escaped_tag = raw_tag.replace('\\', "\\\\").replace('"', "\\\"");
            let omitempty = if field.omitempty { ",omitempty" } else { "" };
            let tag = format!("`json:\"{escaped_tag}{omitempty}\"`");
            struct_fields.push((field_name, go_type, Some(tag)));
        }

        wln!(writer, "type {name}Json struct {{");
        writer.indented(|writer| {
            writer.writeln("// JSON schema");
            emit_aligned_struct_fields(&struct_fields, writer);
        });
        writer.writeln("}");
        writer.blankline();

        // Always emit UnmarshalJSON so root object validation and strict field projection apply
        builder.require_import("\"fmt\"");
        builder.require_import("\"github.com/tidwall/gjson\"");

        let needs_encoding_json = active_fields.iter().any(|f| {
            f.is_dict || f.type_info.is_dict || matches!(f.type_info.base, BaseType::Json(_))
        });
        if needs_encoding_json {
            builder.require_import("\"encoding/json\"");
        }

        self.emit_unmarshal_json(&name, &active_fields, module, writer);
    }

    fn emit_unmarshal_json(
        &self,
        name: &str,
        active_fields: &[&JsonFieldDef],
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let root_err_msg = go_str(&format!("{name}Json: expected JSON object"));
        wln!(
            writer,
            "func (res *{name}Json) UnmarshalJSON(raw []byte) error {{"
        );
        writer.indented(|writer| {
            writer.writeln("g := gjson.ParseBytes(raw)");
            writer.writeln("if !g.IsObject() {");
            writer.indented(|writer| {
                wln!(writer, "return fmt.Errorf({root_err_msg})");
            });
            writer.writeln("}");

            for field in active_fields {
                self.emit_unmarshal_field(name, field, module, writer);
            }

            writer.writeln("return nil");
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_unmarshal_field(
        &self,
        schema_name: &str,
        field: &JsonFieldDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let field_name = to_pascal_case(&field.name);
        if let Some(ref lit) = field.value {
            let lit_expr = go_literal(lit);
            wln!(writer, "res.{field_name} = {lit_expr}");
            return;
        }
        let default_wire = to_snake_case(&field.name);
        let wire_path = field.alias.as_deref().unwrap_or(&default_wire);
        let go_wire_path = go_str(wire_path);
        let missing_err_msg = go_str(&format!(
            "{schema_name}.{}: required JSON field '{wire_path}' missing",
            field.name
        ));
        let res_var = format!("res{field_name}");
        let is_opt = field.is_optional || field.omitempty;
        let is_array = field.type_info.is_array;

        // 1. Inline `(dict)` field or reference to a top-level `(dict)json` schema
        let is_ref_dict = match &field.type_info.base {
            BaseType::Json(ref_name) => module.json_def(ref_name).is_some_and(|j| j.is_dict),
            _ => false,
        };
        if field.is_dict || field.type_info.is_dict || is_ref_dict {
            let map_type = if is_ref_dict {
                if let BaseType::Json(ref ref_name) = field.type_info.base {
                    let base = format!("{}Json", to_pascal_case(ref_name));
                    if is_array { format!("[]{base}") } else { base }
                } else {
                    "map[string]any".to_owned()
                }
            } else {
                let key_info = field
                    .key_type
                    .as_ref()
                    .or(field.type_info.key_type.as_deref());
                let val_info = field
                    .value_type
                    .as_ref()
                    .or(field.type_info.value_type.as_deref());
                let key_type = resolve_dict_key_type(key_info);
                let val_type = match val_info {
                    Some(vt) => Self::resolve_type(vt, module),
                    None => "any".to_owned(),
                };
                if is_array {
                    format!("[]map[{key_type}]{val_type}")
                } else {
                    format!("map[{key_type}]{val_type}")
                }
            };

            let container_check = if is_array { "IsArray" } else { "IsObject" };
            let container_err = go_str(&format!(
                "{schema_name}.{}: expected JSON {}",
                field.name,
                if is_array { "array" } else { "object" }
            ));

            if is_opt {
                wln!(
                    writer,
                    "if {res_var} := gjson.GetBytes(raw, {go_wire_path}); {res_var}.Exists() && {res_var}.Type != gjson.Null {{"
                );
                writer.indented(|writer| {
                    wln!(writer, "if !{res_var}.{container_check}() {{");
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({container_err})");
                    });
                    writer.writeln("}");
                    wln!(writer, "var child {map_type}");
                    wln!(
                        writer,
                        "if err := json.Unmarshal([]byte({res_var}.Raw), &child); err != nil {{"
                    );
                    writer.indented(|writer| {
                        writer.writeln("return err");
                    });
                    writer.writeln("}");
                    wln!(writer, "res.{field_name} = &child");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "{res_var} := gjson.GetBytes(raw, {go_wire_path})");
                wln!(
                    writer,
                    "if !{res_var}.Exists() || {res_var}.Type == gjson.Null {{"
                );
                writer.indented(|writer| {
                    wln!(writer, "return fmt.Errorf({missing_err_msg})");
                });
                writer.writeln("}");
                wln!(writer, "if !{res_var}.{container_check}() {{");
                writer.indented(|writer| {
                    wln!(writer, "return fmt.Errorf({container_err})");
                });
                writer.writeln("}");
                wln!(
                    writer,
                    "if err := json.Unmarshal([]byte({res_var}.Raw), &res.{field_name}); err != nil {{"
                );
                writer.indented(|writer| {
                    writer.writeln("return err");
                });
                writer.writeln("}");
            }
            return;
        }

        // 2. Nested `JsonDef` object or array reference
        if let BaseType::Json(ref ref_name) = field.type_info.base {
            let child_type = format!("{}Json", to_pascal_case(ref_name));
            let effective_is_array =
                is_array || module.json_def(ref_name).is_some_and(|j| j.is_array);

            if effective_is_array {
                let arr_err = go_str(&format!(
                    "{schema_name}.{}: expected JSON array",
                    field.name
                ));
                if is_opt {
                    wln!(
                        writer,
                        "if {res_var} := gjson.GetBytes(raw, {go_wire_path}); {res_var}.Exists() && {res_var}.Type != gjson.Null {{"
                    );
                    writer.indented(|writer| {
                        wln!(writer, "if !{res_var}.IsArray() {{");
                        writer.indented(|writer| {
                            wln!(writer, "return fmt.Errorf({arr_err})");
                        });
                        writer.writeln("}");
                        wln!(writer, "items := make([]{child_type}, 0)");
                        wln!(writer, "for _, item := range {res_var}.Array() {{");
                        writer.indented(|writer| {
                            wln!(writer, "var child {child_type}");
                            writer.writeln(
                                "if err := json.Unmarshal([]byte(item.Raw), &child); err != nil {",
                            );
                            writer.indented(|writer| {
                                writer.writeln("return err");
                            });
                            writer.writeln("}");
                            writer.writeln("items = append(items, child)");
                        });
                        writer.writeln("}");
                        wln!(writer, "res.{field_name} = &items");
                    });
                    writer.writeln("}");
                } else {
                    wln!(writer, "{res_var} := gjson.GetBytes(raw, {go_wire_path})");
                    wln!(
                        writer,
                        "if !{res_var}.Exists() || {res_var}.Type == gjson.Null {{"
                    );
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({missing_err_msg})");
                    });
                    writer.writeln("}");
                    wln!(writer, "if !{res_var}.IsArray() {{");
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({arr_err})");
                    });
                    writer.writeln("}");
                    wln!(writer, "res.{field_name} = make([]{child_type}, 0)");
                    wln!(writer, "for _, item := range {res_var}.Array() {{");
                    writer.indented(|writer| {
                        wln!(writer, "var child {child_type}");
                        writer.writeln(
                            "if err := json.Unmarshal([]byte(item.Raw), &child); err != nil {",
                        );
                        writer.indented(|writer| {
                            writer.writeln("return err");
                        });
                        writer.writeln("}");
                        wln!(writer, "res.{field_name} = append(res.{field_name}, child)");
                    });
                    writer.writeln("}");
                }
            } else {
                let obj_err = go_str(&format!(
                    "{schema_name}.{}: expected JSON object",
                    field.name
                ));
                if is_opt {
                    wln!(
                        writer,
                        "if {res_var} := gjson.GetBytes(raw, {go_wire_path}); {res_var}.Exists() && {res_var}.Type != gjson.Null {{"
                    );
                    writer.indented(|writer| {
                        wln!(writer, "if !{res_var}.IsObject() {{");
                        writer.indented(|writer| {
                            wln!(writer, "return fmt.Errorf({obj_err})");
                        });
                        writer.writeln("}");
                        wln!(writer, "var child {child_type}");
                        wln!(
                            writer,
                            "if err := json.Unmarshal([]byte({res_var}.Raw), &child); err != nil {{"
                        );
                        writer.indented(|writer| {
                            writer.writeln("return err");
                        });
                        writer.writeln("}");
                        wln!(writer, "res.{field_name} = &child");
                    });
                    writer.writeln("}");
                } else {
                    wln!(writer, "{res_var} := gjson.GetBytes(raw, {go_wire_path})");
                    wln!(
                        writer,
                        "if !{res_var}.Exists() || {res_var}.Type == gjson.Null {{"
                    );
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({missing_err_msg})");
                    });
                    writer.writeln("}");
                    wln!(writer, "if !{res_var}.IsObject() {{");
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({obj_err})");
                    });
                    writer.writeln("}");
                    wln!(
                        writer,
                        "if err := json.Unmarshal([]byte({res_var}.Raw), &res.{field_name}); err != nil {{"
                    );
                    writer.indented(|writer| {
                        writer.writeln("return err");
                    });
                    writer.writeln("}");
                }
            }
            return;
        }

        // 3. Primitive scalar or primitive array (STRING, INT, FLOAT, BOOL)
        let (getter, cast_elem, expected_label) = match field.type_info.base {
            BaseType::String => (".String()", "string", "string"),
            BaseType::Int => (".Int()", "int64", "integer"),
            BaseType::Float => (".Float()", "float64", "number"),
            BaseType::Bool => (".Bool()", "bool", "boolean"),
            _ => (".Value()", "any", "value"),
        };

        let has_array_count = field
            .gjson_path
            .steps
            .iter()
            .any(|s| matches!(s, GjsonStep::ArrayCount));
        let has_array_projection = field
            .gjson_path
            .steps
            .iter()
            .any(|s| matches!(s, GjsonStep::ArrayProjection(_)));
        let count_prefix = if has_array_count {
            wire_path.strip_suffix(".#")
        } else {
            None
        };
        let proj_prefix = if has_array_projection {
            wire_path.split_once(".#.").map(|(p, _)| p)
        } else {
            None
        };

        let type_err_msg = go_str(&format!(
            "{schema_name}.{}: expected JSON {expected_label}",
            field.name
        ));

        if is_array {
            let arr_err_msg = go_str(&format!(
                "{schema_name}.{}: expected JSON array",
                field.name
            ));
            let item_cond = primitive_invalid_cond("item", &field.type_info.base);

            if is_opt {
                if let Some(prefix) = proj_prefix {
                    let go_prefix = go_str(prefix);
                    let hash_err = go_str(&format!(
                        "{schema_name}.{}: expected list for '#'",
                        field.name
                    ));
                    wln!(
                        writer,
                        "if _arr := gjson.GetBytes(raw, {go_prefix}); _arr.Exists() && _arr.Type != gjson.Null && !_arr.IsArray() {{"
                    );
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({hash_err})");
                    });
                    writer.writeln("}");
                }
                wln!(
                    writer,
                    "if {res_var} := gjson.GetBytes(raw, {go_wire_path}); {res_var}.Exists() && {res_var}.Type != gjson.Null {{"
                );
                writer.indented(|writer| {
                    wln!(writer, "if !{res_var}.IsArray() {{");
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({arr_err_msg})");
                    });
                    writer.writeln("}");
                    wln!(writer, "items := make([]{cast_elem}, 0)");
                    wln!(writer, "for _, item := range {res_var}.Array() {{");
                    writer.indented(|writer| {
                        if let Some(cond) = &item_cond {
                            wln!(writer, "if {cond} {{");
                            writer.indented(|writer| {
                                wln!(writer, "return fmt.Errorf({type_err_msg})");
                            });
                            writer.writeln("}");
                        }
                        wln!(writer, "items = append(items, item{getter})");
                    });
                    writer.writeln("}");
                    wln!(writer, "res.{field_name} = &items");
                });
                writer.writeln("}");
            } else {
                if let Some(prefix) = proj_prefix {
                    let go_prefix = go_str(prefix);
                    let hash_err = go_str(&format!(
                        "{schema_name}.{}: expected list for '#'",
                        field.name
                    ));
                    wln!(writer, "if !gjson.GetBytes(raw, {go_prefix}).IsArray() {{");
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({hash_err})");
                    });
                    writer.writeln("}");
                }
                wln!(writer, "{res_var} := gjson.GetBytes(raw, {go_wire_path})");
                wln!(
                    writer,
                    "if !{res_var}.Exists() || {res_var}.Type == gjson.Null {{"
                );
                writer.indented(|writer| {
                    wln!(writer, "return fmt.Errorf({missing_err_msg})");
                });
                writer.writeln("}");
                wln!(writer, "if !{res_var}.IsArray() {{");
                writer.indented(|writer| {
                    wln!(writer, "return fmt.Errorf({arr_err_msg})");
                });
                writer.writeln("}");
                wln!(writer, "res.{field_name} = make([]{cast_elem}, 0)");
                wln!(writer, "for _, item := range {res_var}.Array() {{");
                writer.indented(|writer| {
                    if let Some(cond) = &item_cond {
                        wln!(writer, "if {cond} {{");
                        writer.indented(|writer| {
                            wln!(writer, "return fmt.Errorf({type_err_msg})");
                        });
                        writer.writeln("}");
                    }
                    wln!(
                        writer,
                        "res.{field_name} = append(res.{field_name}, item{getter})"
                    );
                });
                writer.writeln("}");
            }
        } else {
            let val_cond = primitive_invalid_cond(&res_var, &field.type_info.base);
            if is_opt {
                if let Some(prefix) = count_prefix {
                    let go_prefix = go_str(prefix);
                    let hash_err = go_str(&format!(
                        "{schema_name}.{}: expected list for '#'",
                        field.name
                    ));
                    wln!(
                        writer,
                        "if _arr := gjson.GetBytes(raw, {go_prefix}); _arr.Exists() && _arr.Type != gjson.Null && !_arr.IsArray() {{"
                    );
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({hash_err})");
                    });
                    writer.writeln("}");
                }
                wln!(
                    writer,
                    "if {res_var} := gjson.GetBytes(raw, {go_wire_path}); {res_var}.Exists() && {res_var}.Type != gjson.Null {{"
                );
                writer.indented(|writer| {
                    if let Some(cond) = &val_cond {
                        wln!(writer, "if {cond} {{");
                        writer.indented(|writer| {
                            wln!(writer, "return fmt.Errorf({type_err_msg})");
                        });
                        writer.writeln("}");
                    }
                    wln!(writer, "v := {res_var}{getter}");
                    wln!(writer, "res.{field_name} = &v");
                });
                writer.writeln("}");
            } else {
                if let Some(prefix) = count_prefix {
                    let go_prefix = go_str(prefix);
                    let hash_err = go_str(&format!(
                        "{schema_name}.{}: expected list for '#'",
                        field.name
                    ));
                    wln!(writer, "if !gjson.GetBytes(raw, {go_prefix}).IsArray() {{");
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({hash_err})");
                    });
                    writer.writeln("}");
                }
                wln!(writer, "{res_var} := gjson.GetBytes(raw, {go_wire_path})");
                wln!(
                    writer,
                    "if !{res_var}.Exists() || {res_var}.Type == gjson.Null {{"
                );
                writer.indented(|writer| {
                    wln!(writer, "return fmt.Errorf({missing_err_msg})");
                });
                writer.writeln("}");
                if let Some(cond) = &val_cond {
                    wln!(writer, "if {cond} {{");
                    writer.indented(|writer| {
                        wln!(writer, "return fmt.Errorf({type_err_msg})");
                    });
                    writer.writeln("}");
                }
                wln!(writer, "res.{field_name} = {res_var}{getter}");
            }
        }
    }
}

pub(crate) fn resolve_dict_key_type(key_info: Option<&TypeInfo>) -> &'static str {
    match key_info.map(|k| &k.base) {
        Some(BaseType::Int) => "int64",
        Some(BaseType::Float) => "float64",
        Some(BaseType::Bool) => "bool",
        _ => "string",
    }
}

fn primitive_invalid_cond(var: &str, base: &BaseType) -> Option<String> {
    match base {
        BaseType::String => Some(format!("{var}.Type != gjson.String")),
        BaseType::Int | BaseType::Float => Some(format!("{var}.Type != gjson.Number")),
        BaseType::Bool => Some(format!(
            "{var}.Type != gjson.True && {var}.Type != gjson.False"
        )),
        _ => None,
    }
}

//! Kotlin `JsonDef` `data class` / `typealias` generation, `toJsonElement()` serialization,
//! and `Rt.project<Name>` allowlist JSON projector emission.

use crate::ast::{BaseType, JsonDef, JsonFieldDef, Module, TypeInfo};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::kotlin::{
    KotlinEmitter, emit_kotlin_doc, kotlin_ident, kotlin_key_type, kotlin_literal, kotlin_pascal,
    kotlin_str,
};
use crate::codegen::writer::{CodeWriter, wln};
use crate::linter::symbols::to_snake_case;

fn render_map_key_parse(key_info: Option<&TypeInfo>, key_var: &str, ctx_lit: &str) -> String {
    match key_info.map(|k| &k.base) {
        Some(BaseType::Int) => format!("Rt.parseMapKeyLong({key_var}, {ctx_lit})"),
        Some(BaseType::Float) => format!("Rt.parseMapKeyDouble({key_var}, {ctx_lit})"),
        Some(BaseType::Bool) => format!("Rt.parseMapKeyBoolean({key_var}, {ctx_lit})"),
        _ => format!("Rt.parseMapKeyString({key_var}, {ctx_lit})"),
    }
}

impl KotlinEmitter {
    /// Resolves the Kotlin type of a [`JsonFieldDef`], accounting for inline `(dict)` fields and `@omitempty`.
    pub fn resolve_json_field_type(&self, field: &JsonFieldDef, module: &Module) -> String {
        let optional = field.is_optional
            || field.omitempty
            || field.type_info.is_optional
            || field.type_info.omitempty;
        if field.is_dict && !field.type_info.is_dict {
            let key_type = kotlin_key_type(
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
            let mut ty = format!("Map<{key_type}, {val_type}>");
            if field.type_info.is_array {
                ty = format!("List<{ty}>");
            }
            if optional {
                ty = format!("{ty}?");
            }
            return ty;
        }
        let mut info = field.type_info.clone();
        if optional {
            info.is_optional = true;
        }
        Self::resolve_type(&info, module)
    }

    /// Renders a Kotlin expression that decodes `val_expr` (`JsonElement`) into `info`.
    pub(crate) fn render_decode_type_info(
        info: &TypeInfo,
        val_expr: &str,
        ctx_lit: &str,
        module: &Module,
    ) -> String {
        let optional = info.is_optional || info.omitempty;
        if optional {
            let mut non_opt = info.clone();
            non_opt.is_optional = false;
            non_opt.omitempty = false;
            let inner = Self::render_decode_type_info(&non_opt, val_expr, ctx_lit, module);
            return format!("if ({val_expr} is JsonNull) null else {inner}");
        }

        if info.is_dict {
            if info.is_array {
                let mut elem_dict = info.clone();
                elem_dict.is_array = false;
                let inner = Self::render_decode_type_info(&elem_dict, "item", ctx_lit, module);
                return format!("Rt.expectArray({val_expr}, {ctx_lit}).map {{ item -> {inner} }}");
            }
            let key_parse = render_map_key_parse(info.key_type.as_deref(), "k", ctx_lit);
            let val_decode = info
                .value_type
                .as_deref()
                .map(|vt| Self::render_decode_type_info(vt, "v", ctx_lit, module))
                .unwrap_or_else(|| format!("Rt.decodeString(v, {ctx_lit})"));
            return format!(
                "Rt.expectObject({val_expr}, {ctx_lit}).entries.associate {{ (k, v) -> {key_parse} to {val_decode} }}"
            );
        }

        if let BaseType::Json(ref ref_name) = info.base {
            let child_pascal = kotlin_pascal(ref_name);
            let target_def = module.json_def(ref_name);
            let def_is_arr = target_def.is_some_and(|j| j.is_array);
            let def_is_dict = target_def.is_some_and(|j| j.is_dict);
            if info.is_array && !def_is_arr {
                if def_is_dict {
                    return format!(
                        "Rt.expectArray({val_expr}, {ctx_lit}).map {{ item -> Rt.project{child_pascal}(item) }}"
                    );
                }
                return format!(
                    "Rt.expectArray({val_expr}, {ctx_lit}).map {{ item -> {child_pascal}Json.fromElement(item) }}"
                );
            }
            return format!("Rt.project{child_pascal}({val_expr})");
        }

        if info.is_array {
            let mut elem_info = info.clone();
            elem_info.is_array = false;
            let inner = Self::render_decode_type_info(&elem_info, "item", ctx_lit, module);
            return format!("Rt.expectArray({val_expr}, {ctx_lit}).map {{ item -> {inner} }}");
        }

        match &info.base {
            BaseType::String | BaseType::Auto | BaseType::DictValue => {
                format!("Rt.decodeString({val_expr}, {ctx_lit})")
            }
            BaseType::Int => format!("Rt.decodeLong({val_expr}, {ctx_lit})"),
            BaseType::Float => format!("Rt.decodeDouble({val_expr}, {ctx_lit})"),
            BaseType::Bool => format!("Rt.decodeBoolean({val_expr}, {ctx_lit})"),
            BaseType::Null => "null".to_owned(),
            BaseType::Document | BaseType::Nested(_) | BaseType::Json(_) => val_expr.to_owned(),
        }
    }

    /// Emits a `JsonDef` `data class` or `typealias` along with `toJsonElement()` and `Rt.project<Name>`.
    pub(crate) fn emit_json_def_impl(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let schema_name = kotlin_pascal(&json_def.name);
        let name = format!("{schema_name}Json");
        let schema_lit = kotlin_str(&schema_name);

        if let Some(ref doc) = json_def.doc {
            emit_kotlin_doc(doc, "//", writer);
        }

        if json_def.is_dict {
            let key_type = kotlin_key_type(json_def.key_type.as_ref());
            let val_type = json_def
                .value_type
                .as_ref()
                .map(|vt| Self::resolve_type(vt, module))
                .unwrap_or_else(|| "String".to_owned());
            wln!(writer, "typealias {name} = Map<{key_type}, {val_type}>");
            writer.blankline();

            let key_parse = render_map_key_parse(json_def.key_type.as_ref(), "k", &schema_lit);
            let val_decode = json_def
                .value_type
                .as_ref()
                .map(|vt| Self::render_decode_type_info(vt, "v", &schema_lit, module))
                .unwrap_or_else(|| format!("Rt.decodeString(v, {schema_lit})"));

            wln!(
                writer,
                "fun Rt.project{schema_name}(el: JsonElement): {name} {{"
            );
            writer.indented(|writer| {
                wln!(writer, "val obj = Rt.expectObject(el, {schema_lit})");
                wln!(
                    writer,
                    "return obj.entries.associate {{ (k, v) -> {key_parse} to {val_decode} }}"
                );
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
            wln!(writer, "class {name} : Rt.JsonEncodable {{");
        } else {
            wln!(writer, "data class {name}(");
            writer.indented(|writer| {
                for field in &active_fields {
                    if let Some(ref fdoc) = field.doc {
                        emit_kotlin_doc(fdoc, "//", writer);
                    }
                    let typ = self.resolve_json_field_type(field, module);
                    let f_ident = kotlin_ident(&field.name);
                    let optional = field.is_optional
                        || field.omitempty
                        || field.type_info.is_optional
                        || field.type_info.omitempty;
                    if optional {
                        wln!(writer, "val {f_ident}: {typ} = null,");
                    } else {
                        wln!(writer, "val {f_ident}: {typ},");
                    }
                }
            });
            writer.writeln(") : Rt.JsonEncodable {");
        }

        writer.indented(|writer| {
            writer.writeln("override fun toJsonElement(): JsonElement {");
            writer.indented(|writer| {
                writer.writeln("val map = LinkedHashMap<String, JsonElement>()");
                for field in &active_fields {
                    let f_ident = kotlin_ident(&field.name);
                    let snake_key = kotlin_str(&to_snake_case(&field.name));
                    if field.omitempty || field.type_info.omitempty {
                        wln!(writer, "if (this.{f_ident} != null) {{");
                        writer.indented(|writer| {
                            wln!(writer,
                                "map[{snake_key}] = Rt.toJsonElement(this.{f_ident})"
                            );
                        });
                        writer.writeln("}");
                    } else {
                        wln!(writer,
                            "map[{snake_key}] = Rt.toJsonElement(this.{f_ident})"
                        );
                    }
                }
                writer.writeln("return JsonObject(map)");
            });
            writer.writeln("}");
            writer.blankline();

            if active_fields.is_empty() {
                wln!(writer,
                    "override fun equals(other: Any?): Boolean = other is {name}"
                );
                writer.writeln("override fun hashCode(): Int = 0");
                wln!(writer,
                    "override fun toString(): String = \"{}()\"",
                    name
                );
                writer.blankline();
            }

            writer.writeln("companion object {");
            writer.indented(|writer| {
                wln!(writer,
                    "fun fromElement(el: JsonElement): {name} {{"
                );
                writer.indented(|writer| {
                    wln!(writer, "Rt.expectObject(el, {schema_lit})");
                    for field in &active_fields {
                        self.emit_json_field_decode(field, &schema_name, module, writer);
                    }
                    if active_fields.is_empty() {
                        wln!(writer, "return {name}()");
                    } else {
                        wln!(writer, "return {name}(");
                        writer.indented(|writer| {
                            for field in &active_fields {
                                let f_ident = kotlin_ident(&field.name);
                                wln!(writer, "{f_ident} = {f_ident},");
                            }
                        });
                        writer.writeln(")");
                    }
                });
                writer.writeln("}");
                writer.blankline();

                wln!(writer,
                    "fun fromValue(el: JsonElement): {name} = fromElement(el)"
                );
                writer.blankline();

                wln!(writer,
                    "fun fromJson(raw: String): {name} = fromElement(Rt.parseJson(raw, \"\", {schema_lit}))"
                );
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();

        if json_def.is_array {
            wln!(
                writer,
                "fun Rt.project{schema_name}(el: JsonElement): List<{name}> ="
            );
            writer.indented(|writer| {
                wln!(
                    writer,
                    "Rt.expectArray(el, {schema_lit}).map {{ {name}.fromElement(it) }}"
                );
            });
        } else {
            wln!(
                writer,
                "fun Rt.project{schema_name}(el: JsonElement): {name} = {name}.fromElement(el)"
            );
        }
        writer.blankline();
    }

    fn emit_json_field_decode(
        &self,
        field: &JsonFieldDef,
        schema_name: &str,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let f_ident = kotlin_ident(&field.name);
        let wire = field.alias.as_deref().unwrap_or(&field.name);
        let optional = field.is_optional
            || field.omitempty
            || field.type_info.is_optional
            || field.type_info.omitempty;
        let typ = self.resolve_json_field_type(field, module);
        if let Some(ref lit) = field.value {
            let lit_expr = kotlin_literal(lit);
            wln!(writer, "val {f_ident}: {typ} = {lit_expr}");
            return;
        }
        let path_lit = kotlin_str(wire);
        let ctx_lit = kotlin_str(&format!("{schema_name}.{}", field.name));

        let non_opt_info = if field.is_dict && !field.type_info.is_dict {
            let kt = field
                .key_type
                .clone()
                .or_else(|| field.type_info.key_type.as_deref().cloned())
                .unwrap_or_else(TypeInfo::string);
            let vt = field
                .value_type
                .clone()
                .or_else(|| field.type_info.value_type.as_deref().cloned())
                .unwrap_or_else(TypeInfo::string);
            TypeInfo::dict(kt, vt).with_array(field.type_info.is_array)
        } else {
            let mut info = field.type_info.clone();
            info.is_optional = false;
            info.omitempty = false;
            info
        };

        let decoded_expr = Self::render_decode_type_info(&non_opt_info, "raw", &ctx_lit, module);

        if optional {
            wln!(writer, "val {f_ident}: {typ} = try {{");
            writer.indented(|writer| {
                wln!(writer, "val raw = Rt.jsonPath(el, {path_lit}, {ctx_lit})");
                wln!(writer, "if (raw is JsonNull) null else {decoded_expr}");
            });
            writer.writeln("} catch (_: Rt.RtMissingPathError) {");
            writer.indented(|writer| {
                writer.writeln("null");
            });
            writer.writeln("}");
        } else {
            wln!(writer, "val {f_ident}: {typ} = run {{");
            writer.indented(|writer| {
                wln!(writer, "val raw = Rt.jsonPath(el, {path_lit}, {ctx_lit})");
                wln!(
                    writer,
                    "if (raw is JsonNull) throw Rt.RtError({ctx_lit}, \"required field is null\")"
                );
                writer.writeln(decoded_expr);
            });
            writer.writeln("}");
        }
    }
}

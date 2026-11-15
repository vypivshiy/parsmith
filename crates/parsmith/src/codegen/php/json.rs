//! PHP 8.1+ `JsonDef` model generation (`readonly class <Name>Json implements \JsonSerializable, \ArrayAccess`)
//! and allowlist JSON projector emission (`Rt::project<Name>` / `<Name>Json::fromValue`).

use crate::ast::{BaseType, JsonDef, JsonFieldDef, Module, TypeInfo};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::php::dom::PhpDomSpelling;
use crate::codegen::php::{PhpEmitter, emit_php_doc, php_ident, php_pascal, php_str};
use crate::codegen::writer::{CodeWriter, wln};
use crate::linter::symbols::to_snake_case;

/// Formats the canonical PHP JSON projector method name on `Rt` (`project<PascalName>`).
pub fn php_json_projector_method(name: &str) -> String {
    format!("project{}", php_pascal(name))
}

/// Formats the top-level PHP JSON projector function name (`rt_project_<snake_name>`).
pub fn php_json_projector_fn(name: &str) -> String {
    let clean = to_snake_case(name);
    let trimmed = clean.trim_start_matches('_');
    format!("rt_project_{trimmed}")
}

/// Returns `true` if `info` represents a dictionary/map at runtime (either inline `(dict)` or referencing a `(dict)json`).
pub fn is_dict_type_info(info: &TypeInfo, module: &Module) -> bool {
    info.is_dict
        || match &info.base {
            BaseType::Json(ref_name) => module.json_def(ref_name).is_some_and(|j| j.is_dict),
            BaseType::Nested(ref_name) => module.struct_decl(ref_name).is_some_and(|s| {
                matches!(
                    s.kind,
                    crate::ast::StructKind::Dict | crate::ast::StructKind::Table
                )
            }),
            _ => false,
        }
}

impl<D: PhpDomSpelling> PhpEmitter<D> {
    /// Resolves the PHP property type hint for a [`JsonFieldDef`].
    pub fn resolve_json_field_type(&self, field: &JsonFieldDef, module: &Module) -> String {
        let optional = field.is_optional
            || field.omitempty
            || field.type_info.is_optional
            || field.type_info.omitempty;
        if field.is_dict || field.type_info.is_dict {
            return if optional {
                "?array".to_owned()
            } else {
                "array".to_owned()
            };
        }
        let mut info = field.type_info.clone();
        if optional {
            info.is_optional = true;
        }
        self.resolve_type(&info, module)
    }

    /// Emits a `JsonDef` `readonly class <Name>Json` and its projector wrappers.
    pub(crate) fn emit_json_def_impl(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let schema_name = php_pascal(&json_def.name);
        let model_name = format!("{schema_name}Json");
        let proj_method = php_json_projector_method(&json_def.name);
        let proj_fn = php_json_projector_fn(&json_def.name);

        if !json_def.is_dict {
            if let Some(ref doc) = json_def.doc {
                emit_php_doc(doc, writer);
            }
            let active_fields: Vec<&JsonFieldDef> = json_def
                .fields
                .iter()
                .filter(|f| !f.skip && !f.type_info.skip)
                .collect();

            wln!(
                writer,
                "readonly class {model_name} implements \\JsonSerializable, \\ArrayAccess"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                if active_fields.is_empty() {
                    writer.writeln("public function __construct()");
                    writer.writeln("{");
                    writer.writeln("}");
                } else {
                    writer.writeln("public function __construct(");
                    writer.indented(|writer| {
                        for field in &active_fields {
                            let f_ident = php_ident(&field.name);
                            let f_type = self.resolve_json_field_type(field, module);
                            wln!(writer, "public {f_type} ${f_ident},");
                        }
                    });
                    writer.writeln(") {");
                    writer.writeln("}");
                }
                writer.blankline();

                let from_ret = if json_def.is_array { "array" } else { "self" };
                wln!(writer,
                    "public static function fromValue(mixed $data): {from_ret}"
                );
                writer.writeln("{");
                writer.indented(|writer| {
                    wln!(writer, "return self::{proj_method}($data);");
                });
                writer.writeln("}");
                writer.blankline();

                Self::emit_json_projector_method_on_class(
                    self,
                    json_def,
                    &schema_name,
                    &model_name,
                    &proj_method,
                    module,
                    writer,
                );
                writer.blankline();

                writer.writeln("public function jsonSerialize(): mixed");
                writer.writeln("{");
                writer.indented(|writer| {
                    if active_fields.is_empty() {
                        writer.writeln("return new \\stdClass();");
                    } else {
                        writer.writeln("$out = [];");
                        for field in &active_fields {
                            let f_ident = php_ident(&field.name);
                            let key_lit = php_str(&to_snake_case(&field.name));
                            let is_dict = field.is_dict || is_dict_type_info(&field.type_info, module);
                            let val_expr = if is_dict {
                                format!("Rt::serializeDict($this->{f_ident})")
                            } else {
                                format!("$this->{f_ident}")
                            };
                            if field.omitempty || field.type_info.omitempty {
                                wln!(writer, "if ($this->{f_ident} !== null) {{");
                                writer.indented(|writer| {
                                    wln!(writer, "$out[{key_lit}] = {val_expr};");
                                });
                                writer.writeln("}");
                            } else {
                                wln!(writer, "$out[{key_lit}] = {val_expr};");
                            }
                        }
                        writer.writeln("return empty($out) ? new \\stdClass() : $out;");
                    }
                });
                writer.writeln("}");
                writer.blankline();

                writer.writeln("public function offsetExists(mixed $offset): bool");
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln(
                        "return is_string($offset) && property_exists($this, $offset) && $this->{$offset} !== null;",
                    );
                });
                writer.writeln("}");
                writer.blankline();

                writer.writeln("public function offsetGet(mixed $offset): mixed");
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("return $this->{$offset};");
                });
                writer.writeln("}");
                writer.blankline();

                writer.writeln("public function offsetSet(mixed $offset, mixed $value): void");
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("throw new \\BadMethodCallException('Cannot modify readonly object');");
                });
                writer.writeln("}");
                writer.blankline();

                writer.writeln("public function offsetUnset(mixed $offset): void");
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("throw new \\BadMethodCallException('Cannot modify readonly object');");
                });
                writer.writeln("}");
            });
            writer.writeln("}");
            writer.blankline();
        } else {
            if let Some(ref doc) = json_def.doc {
                emit_php_doc(doc, writer);
            }
            wln!(writer, "final class {model_name}");
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("public static function fromValue(mixed $data): array");
                writer.writeln("{");
                writer.indented(|writer| {
                    wln!(writer, "return self::{proj_method}($data);");
                });
                writer.writeln("}");
                writer.blankline();

                Self::emit_json_projector_method_on_class(
                    self,
                    json_def,
                    &schema_name,
                    &model_name,
                    &proj_method,
                    module,
                    writer,
                );
            });
            writer.writeln("}");
            writer.blankline();
        }

        let fn_ret = if json_def.is_dict || json_def.is_array {
            "array".to_owned()
        } else {
            model_name.clone()
        };
        wln!(writer, "function {proj_fn}(mixed $data): {fn_ret}");
        writer.writeln("{");
        writer.indented(|writer| {
            wln!(writer, "return {model_name}::{proj_method}($data);");
        });
        writer.writeln("}");
        writer.blankline();
    }

    pub(crate) fn emit_json_projector_method_on_class(
        &self,
        json_def: &JsonDef,
        schema_name: &str,
        model_name: &str,
        method_name: &str,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let schema_lit = php_str(schema_name);

        if json_def.is_dict {
            wln!(
                writer,
                "public static function {method_name}(mixed $data): array"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                if let Some(ref root_path) = json_def.path {
                    let path_lit = php_str(root_path);
                    wln!(
                        writer,
                        "$data = Rt::resolveDotpath($data, {path_lit}, false, {schema_lit});"
                    );
                }
                wln!(writer, "$_d = Rt::expectDict($data, {schema_lit});");
                writer.writeln("$_out = [];");
                writer.writeln("foreach ($_d as $_k => $_v) {");
                writer.indented(|writer| {
                    let val_expr = if let Some(ref vi) = json_def.value_type {
                        Self::transform_json_type_expr("$_v", vi, &schema_lit, module)
                    } else {
                        "Rt::toPlainPhp($_v)".to_owned()
                    };
                    wln!(
                        writer,
                        "$_out[(string) $_k] = $_v !== null ? {val_expr} : null;"
                    );
                });
                writer.writeln("}");
                writer.writeln("return $_out;");
            });
            writer.writeln("}");
            return;
        }

        let ret_type = if json_def.is_array {
            "array".to_owned()
        } else {
            model_name.to_owned()
        };

        wln!(
            writer,
            "public static function {method_name}(mixed $data): {ret_type}"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            if let Some(ref root_path) = json_def.path {
                let path_lit = php_str(root_path);
                wln!(
                    writer,
                    "$data = Rt::resolveDotpath($data, {path_lit}, false, {schema_lit});"
                );
            }

            if json_def.is_array {
                writer.writeln("$_out = [];");
                wln!(
                    writer,
                    "foreach (Rt::expectList($data, {schema_lit}) as $_item) {{"
                );
                writer.indented(|writer| {
                    wln!(writer, "$_d = Rt::expectDict($_item, {schema_lit});");
                    self.emit_json_object_fields(
                        json_def,
                        schema_name,
                        model_name,
                        true,
                        module,
                        writer,
                    );
                });
                writer.writeln("}");
                writer.writeln("return $_out;");
            } else {
                wln!(writer, "$_d = Rt::expectDict($data, {schema_lit});");
                self.emit_json_object_fields(
                    json_def,
                    schema_name,
                    model_name,
                    false,
                    module,
                    writer,
                );
            }
        });
        writer.writeln("}");
    }

    fn emit_json_object_fields(
        &self,
        json_def: &JsonDef,
        schema_name: &str,
        model_name: &str,
        in_array_loop: bool,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let active_fields: Vec<&JsonFieldDef> = json_def
            .fields
            .iter()
            .filter(|f| !f.skip && !f.type_info.skip)
            .collect();

        let schema_lit = php_str(schema_name);

        for field in &active_fields {
            let f_ident = php_ident(&field.name);
            let wire_path = field.alias.as_deref().unwrap_or(&field.name);
            let target = format!("{schema_name}.{}", field.name);
            let target_lit = php_str(&target);
            let is_direct = field.gjson_path.is_direct_key();
            let direct_key = field.gjson_path.as_direct_key().unwrap_or(wire_path);
            let optional = field.is_optional
                || field.omitempty
                || field.type_info.is_optional
                || field.type_info.omitempty;

            if !optional {
                let raw_expr = if is_direct {
                    let req_target = if direct_key == field.name {
                        schema_lit.clone()
                    } else {
                        target_lit.clone()
                    };
                    format!("Rt::jsonReq($_d, {}, {req_target})", php_str(direct_key))
                } else {
                    format!("Rt::jsonReqPath($_d, {}, {target_lit})", php_str(wire_path))
                };
                let val_expr =
                    Self::transform_json_field_expr(&raw_expr, field, &target_lit, module);
                wln!(writer, "$_f_{f_ident} = {val_expr};");
            } else {
                let raw_expr = if is_direct {
                    format!("Rt::jsonGet($_d, {})", php_str(direct_key))
                } else {
                    format!(
                        "Rt::resolveDotpath($_d, {}, true, {target_lit})",
                        php_str(wire_path)
                    )
                };
                wln!(writer, "$_raw_{f_ident} = {raw_expr};");
                let val_expr = Self::transform_json_field_expr(
                    &format!("$_raw_{f_ident}"),
                    field,
                    &target_lit,
                    module,
                );
                wln!(
                    writer,
                    "$_f_{f_ident} = $_raw_{f_ident} !== null ? {val_expr} : null;"
                );
            }
        }

        let args = active_fields
            .iter()
            .map(|f| format!("$_f_{}", php_ident(&f.name)))
            .collect::<Vec<_>>()
            .join(", ");

        if in_array_loop {
            wln!(writer, "$_out[] = new {model_name}({args});");
        } else {
            wln!(writer, "return new {model_name}({args});");
        }
    }

    fn transform_json_field_expr(
        raw_expr: &str,
        field: &JsonFieldDef,
        target_lit: &str,
        module: &Module,
    ) -> String {
        if field.is_dict || field.type_info.is_dict {
            let f_val_info = field
                .value_type
                .as_ref()
                .or(field.type_info.value_type.as_deref());
            if let Some(vi) = f_val_info {
                let item_expr = Self::transform_json_type_expr("$_dv", vi, target_lit, module);
                return format!(
                    "Rt::mapDict(Rt::expectDict({raw_expr}, {target_lit}), static fn(mixed $_dv): mixed => $_dv !== null ? {item_expr} : null)"
                );
            }
            return format!("Rt::expectDict({raw_expr}, {target_lit})");
        }

        Self::transform_json_type_expr(raw_expr, &field.type_info, target_lit, module)
    }

    fn transform_json_type_expr(
        raw_expr: &str,
        info: &TypeInfo,
        target_lit: &str,
        module: &Module,
    ) -> String {
        if info.is_dict {
            if let Some(ref vi) = info.value_type {
                let item_expr = Self::transform_json_type_expr("$_dv", vi, target_lit, module);
                return format!(
                    "Rt::mapDict(Rt::expectDict({raw_expr}, {target_lit}), static fn(mixed $_dv): mixed => $_dv !== null ? {item_expr} : null)"
                );
            }
            return format!("Rt::expectDict({raw_expr}, {target_lit})");
        }

        if let BaseType::Json(ref ref_name) = info.base {
            let child_cls = format!("{}Json", php_pascal(ref_name));
            let child_proj = php_json_projector_method(ref_name);
            let child_is_arr_def = module.json_def(ref_name).is_some_and(|j| j.is_array);
            if info.is_array && !child_is_arr_def {
                return format!(
                    "array_values(array_map(static fn(mixed $_x): {child_cls} => {child_cls}::{child_proj}($_x), array_filter(Rt::expectList({raw_expr}, {target_lit}), static fn(mixed $_x): bool => $_x !== null)))"
                );
            }
            return format!("{child_cls}::{child_proj}({raw_expr})");
        }

        if info.is_array {
            let elem_info = TypeInfo {
                is_array: false,
                is_optional: false,
                omitempty: false,
                ..info.clone()
            };
            let elem_expr = Self::transform_json_type_expr("$_x", &elem_info, target_lit, module);
            return format!(
                "array_map(static fn(mixed $_x): mixed => {elem_expr}, Rt::expectList({raw_expr}, {target_lit}))"
            );
        }

        match info.base {
            BaseType::String => format!("Rt::expectStr({raw_expr}, {target_lit})"),
            BaseType::Int => format!("Rt::expectInt({raw_expr}, {target_lit})"),
            BaseType::Float => format!("Rt::expectFloat({raw_expr}, {target_lit})"),
            BaseType::Bool => format!("Rt::expectBool({raw_expr}, {target_lit})"),
            _ => format!("Rt::toPlainPhp({raw_expr})"),
        }
    }
}

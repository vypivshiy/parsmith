//! Python 3.10+ `JsonDef` model generation (`TypedDict` / `@dataclass(slots=True)`)
//! and allowlist JSON projector functions (`rt_project_<schema>`).

use crate::ast::{BaseType, JsonDef, JsonFieldDef, Module, TypeInfo};
use crate::codegen::PythonModels;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::python::dom::PyDomSpelling;
use crate::codegen::python::{PythonEmitter, emit_docstring, py_literal, py_str};
use crate::codegen::writer::{CodeWriter, wln};
use crate::json_to_kdl::to_pascal_case;

/// Key used to register the JSON projection runtime helpers in [`ModuleBuilder`].
pub const STD_JSON_PROJECT_KEY: &str = "rt_json_project";

/// Symbol names exported by [`STD_JSON_PROJECT_CODE`] when `-R / --separate-runtime` is active.
pub const JSON_PROJECT_RUNTIME_SYMBOLS: &[&str] = &[
    "RtJsonError",
    "RtJsonPathError",
    "RtJsonFieldMissingError",
    "RtJsonSchemaError",
    "rt_expect_dict",
    "rt_expect_list",
    "rt_resolve_dotpath",
    "rt_json_req",
    "rt_json_req_path",
];

/// Standard Python runtime helpers for allowlist JSON projection and GJSON dot-path resolution.
pub const STD_JSON_PROJECT_CODE: &str = include_str!("runtime/json_project.py");

/// Formats the canonical Python allowlist JSON projector function name (`rt_project_<snake_name>`).
pub use crate::linter::symbols::python_json_projector_name;

/// Registers the standard JSON projection runtime helper in `builder`.
pub fn require_json_project_std(builder: &mut ModuleBuilder) {
    builder
        .require_import("from typing import Any")
        .require_std(STD_JSON_PROJECT_KEY, STD_JSON_PROJECT_CODE);
}

impl<D: PyDomSpelling> PythonEmitter<D> {
    /// Emits a `JsonDef` model (`TypedDict`, `@dataclass(slots=True)`, or `TypeAlias`) and its
    /// schema-specific allowlist JSON projector function (`rt_project_<name>`).
    pub(crate) fn emit_json_def_impl(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        require_json_project_std(builder);
        let schema_name = to_pascal_case(&json_def.name);
        let model_name = format!("{schema_name}Json");
        let use_dataclass = self.options.models == PythonModels::Dataclass;

        // 1. Emit model type declaration
        if json_def.is_dict {
            builder.require_import("from typing import TypeAlias");
            let key_type = super::py_key_type(json_def.key_type.as_ref());
            let val_type = match json_def.value_type.as_ref() {
                Some(vt) => self.resolve_type(vt, module, builder),
                None => {
                    builder.require_import("from typing import Any");
                    "Any".to_owned()
                }
            };
            wln!(
                writer,
                "{model_name}: TypeAlias = dict[{key_type}, {val_type}]"
            );
            writer.blankline();
        } else {
            let active_fields: Vec<&JsonFieldDef> =
                json_def.fields.iter().filter(|f| !f.skip).collect();

            match self.options.models {
                PythonModels::TypedDict => {
                    builder.require_import("from typing import TypedDict");
                    wln!(writer, "class {model_name}(TypedDict):");
                    writer.indented(|writer| {
                        if let Some(ref doc) = json_def.doc {
                            emit_docstring(doc, writer);
                        }
                        if active_fields.is_empty() {
                            if json_def.doc.is_none() {
                                writer.writeln("pass");
                            }
                        } else {
                            for field in &active_fields {
                                let fname = &field.name;
                                let mut ftype =
                                    self.resolve_type(&field.type_info, module, builder);
                                if field.omitempty {
                                    builder.require_import("from typing import NotRequired");
                                    ftype = format!("NotRequired[{ftype}]");
                                }
                                wln!(writer, "{fname}: {ftype}");
                            }
                        }
                    });
                }
                PythonModels::Dataclass => {
                    builder.require_import("from dataclasses import dataclass");
                    writer.writeln("@dataclass(slots=True)");
                    wln!(writer, "class {model_name}:");
                    writer.indented(|writer| {
                        if let Some(ref doc) = json_def.doc {
                            emit_docstring(doc, writer);
                        }
                        if active_fields.is_empty() {
                            if json_def.doc.is_none() {
                                writer.writeln("pass");
                            }
                        } else {
                            // Ensure fields without defaults precede `@omitempty` fields with `= None` defaults
                            let mut ordered_fields = active_fields.clone();
                            ordered_fields.sort_by_key(|f| f.omitempty);
                            for field in ordered_fields {
                                let fname = &field.name;
                                let mut ftype =
                                    self.resolve_type(&field.type_info, module, builder);
                                if field.omitempty {
                                    if !field.is_optional && ftype != "None" {
                                        ftype = format!("{ftype} | None");
                                    }
                                    wln!(writer, "{fname}: {ftype} = None");
                                } else {
                                    wln!(writer, "{fname}: {ftype}");
                                }
                            }
                        }
                    });
                }
            }
            writer.blankline();
        }

        // 2. Emit allowlist JSON projector function `rt_project_<name>`
        self.emit_json_projector_fn(
            json_def,
            &schema_name,
            &model_name,
            use_dataclass,
            module,
            builder,
            writer,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_json_projector_fn(
        &self,
        json_def: &JsonDef,
        schema_name: &str,
        model_name: &str,
        use_dataclass: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_import("from typing import Any");
        let fn_name = python_json_projector_name(&json_def.name);
        let schema_lit = py_str(schema_name);

        if json_def.is_dict {
            builder.require_import("from typing import cast");
            wln!(writer, "def {fn_name}(data: Any) -> {model_name}:");
            writer.indented(|writer| {
                if let Some(ref root_path) = json_def.path {
                    let path_lit = py_str(root_path);
                    wln!(writer,
                        "data = rt_resolve_dotpath(data, {path_lit}, False, {schema_lit})"
                    );
                }
                wln!(writer, "_d = rt_expect_dict(data, {schema_lit})");

                let val_info = json_def.value_type.as_ref();
                let dict_expr = if let Some(vi) = val_info
                    && let BaseType::Json(ref ref_name) = vi.base
                {
                    let child_proj = python_json_projector_name(ref_name);
                    let child_is_arr_def =
                        module.json_def(ref_name).is_some_and(|j| j.is_array);
                    if vi.is_array && !child_is_arr_def {
                        format!(
                            "{{_k: ([{child_proj}(_x) for _x in rt_expect_list(_v, {schema_lit}) if _x is not None] if _v is not None else None) for _k, _v in _d.items()}}"
                        )
                    } else {
                        format!(
                            "{{_k: ({child_proj}(_v) if _v is not None else None) for _k, _v in _d.items()}}"
                        )
                    }
                } else if val_info.is_some_and(|vi| vi.is_array) {
                    format!(
                        "{{_k: (rt_expect_list(_v, {schema_lit}) if _v is not None else None) for _k, _v in _d.items()}}"
                    )
                } else {
                    "dict(_d)".to_owned()
                };

                wln!(writer, "return cast({model_name}, {dict_expr})");
            });
            writer.blankline();
            return;
        }

        let ret_type = if json_def.is_array {
            format!("list[{model_name}]")
        } else {
            model_name.to_owned()
        };

        let mut field_entries: Vec<(String, String)> = Vec::new();
        let mut omitempty_entries: Vec<(String, String, String)> = Vec::new();

        for field in &json_def.fields {
            if field.skip {
                continue;
            }
            let fname = &field.name;
            if let Some(ref lit) = field.value {
                field_entries.push((fname.clone(), py_literal(lit)));
                continue;
            }
            let wire_path = field.alias.as_deref().unwrap_or(fname.as_str());
            let target = format!("{schema_name}.{fname}");
            let target_lit = py_str(&target);
            let is_direct = field.gjson_path.is_direct_key();
            let direct_key = field.gjson_path.as_direct_key().unwrap_or(wire_path);

            if !field.is_optional && !field.omitempty {
                let raw_expr = if is_direct {
                    let req_target = if direct_key == fname {
                        schema_lit.clone()
                    } else {
                        target_lit.clone()
                    };
                    format!("rt_json_req(_d, {}, {req_target})", py_str(direct_key))
                } else {
                    format!("rt_json_req_path(_d, {}, {target_lit})", py_str(wire_path))
                };
                let val_expr =
                    self.transform_json_field_value(&raw_expr, field, &target, module, builder);
                field_entries.push((fname.clone(), val_expr));
            } else if field.omitempty && !use_dataclass {
                let raw_expr = if is_direct {
                    format!("_d.get({})", py_str(direct_key))
                } else {
                    format!(
                        "rt_resolve_dotpath(_d, {}, True, {target_lit})",
                        py_str(wire_path)
                    )
                };
                let val_expr =
                    self.transform_json_field_value("_v", field, &target, module, builder);
                omitempty_entries.push((fname.clone(), raw_expr, val_expr));
            } else {
                // Nullable (`?`) field, or `@omitempty` in dataclass mode
                let raw_expr = if is_direct {
                    format!("_d.get({})", py_str(direct_key))
                } else {
                    format!(
                        "rt_resolve_dotpath(_d, {}, True, {target_lit})",
                        py_str(wire_path)
                    )
                };
                if !json_field_needs_transform(field) {
                    field_entries.push((fname.clone(), raw_expr));
                } else {
                    let val_expr =
                        self.transform_json_field_value("_v", field, &target, module, builder);
                    field_entries.push((
                        fname.clone(),
                        format!("{val_expr} if (_v := {raw_expr}) is not None else None"),
                    ));
                }
            }
        }

        if !use_dataclass {
            builder.require_import("from typing import cast");
        }

        wln!(writer, "def {fn_name}(data: Any) -> {ret_type}:");
        writer.indented(|writer| {
            if let Some(ref root_path) = json_def.path {
                let path_lit = py_str(root_path);
                wln!(
                    writer,
                    "data = rt_resolve_dotpath(data, {path_lit}, False, {schema_lit})"
                );
            }

            if json_def.is_array {
                wln!(writer, "_out: list[{model_name}] = []");
                wln!(writer, "for _item in rt_expect_list(data, {schema_lit}):");
                writer.indented(|writer| {
                    wln!(writer, "_d = rt_expect_dict(_item, {schema_lit})");
                    self.emit_projector_item_construction(
                        model_name,
                        &field_entries,
                        &omitempty_entries,
                        use_dataclass,
                        true,
                        writer,
                    );
                });
                writer.writeln("return _out");
            } else {
                wln!(writer, "_d = rt_expect_dict(data, {schema_lit})");
                self.emit_projector_item_construction(
                    model_name,
                    &field_entries,
                    &omitempty_entries,
                    use_dataclass,
                    false,
                    writer,
                );
            }
        });
        writer.blankline();
    }

    fn emit_projector_item_construction(
        &self,
        model_name: &str,
        field_entries: &[(String, String)],
        omitempty_entries: &[(String, String, String)],
        use_dataclass: bool,
        in_array_loop: bool,
        writer: &mut CodeWriter,
    ) {
        if use_dataclass {
            if field_entries.is_empty() {
                if in_array_loop {
                    wln!(writer, "_out.append({model_name}())");
                } else {
                    wln!(writer, "return {model_name}()");
                }
            } else {
                if in_array_loop {
                    wln!(writer, "_out.append({model_name}(");
                } else {
                    wln!(writer, "return {model_name}(");
                }
                writer.indented(|writer| {
                    for (fname, expr) in field_entries {
                        wln!(writer, "{fname}={expr},");
                    }
                });
                if in_array_loop {
                    writer.writeln("))");
                } else {
                    writer.writeln(")");
                }
            }
            return;
        }

        // TypedDict mode
        if !omitempty_entries.is_empty() {
            if field_entries.is_empty() {
                writer.writeln("_res: dict[str, Any] = {}");
            } else {
                writer.writeln("_res: dict[str, Any] = {");
                writer.indented(|writer| {
                    for (fname, expr) in field_entries {
                        let k_lit = py_str(fname);
                        wln!(writer, "{k_lit}: {expr},");
                    }
                });
                writer.writeln("}");
            }
            for (fname, raw_expr, val_expr) in omitempty_entries {
                let k_lit = py_str(fname);
                wln!(writer, "if (_v := {raw_expr}) is not None:");
                writer.indented(|writer| {
                    wln!(writer, "_res[{k_lit}] = {val_expr}");
                });
            }
            if in_array_loop {
                wln!(writer, "_out.append(cast({model_name}, _res))");
            } else {
                wln!(writer, "return cast({model_name}, _res)");
            }
        } else if field_entries.is_empty() {
            if in_array_loop {
                wln!(writer, "_out.append(cast({model_name}, {{}}))");
            } else {
                wln!(writer, "return cast({model_name}, {{}})");
            }
        } else {
            if in_array_loop {
                wln!(writer, "_out.append(cast({model_name}, {{");
            } else {
                wln!(writer, "return cast({model_name}, {{");
            }
            writer.indented(|writer| {
                for (fname, expr) in field_entries {
                    let k_lit = py_str(fname);
                    wln!(writer, "{k_lit}: {expr},");
                }
            });
            if in_array_loop {
                writer.writeln("}))");
            } else {
                writer.writeln("})");
            }
        }
    }

    fn transform_json_field_value(
        &self,
        v_expr: &str,
        field: &JsonFieldDef,
        target: &str,
        module: &Module,
        builder: &mut ModuleBuilder,
    ) -> String {
        let target_lit = py_str(target);
        if field.is_dict || field.type_info.is_dict {
            let f_val_info: Option<&TypeInfo> = field
                .value_type
                .as_ref()
                .or(field.type_info.value_type.as_deref());
            let key_is_non_str = field
                .key_type
                .as_ref()
                .or(field.type_info.key_type.as_deref())
                .is_some_and(|k| !matches!(k.base, BaseType::String));

            if let Some(vi) = f_val_info
                && let BaseType::Json(ref ref_name) = vi.base
            {
                let child_proj = python_json_projector_name(ref_name);
                let child_is_arr_def = module.json_def(ref_name).is_some_and(|j| j.is_array);
                let comp = if vi.is_array && !child_is_arr_def {
                    format!(
                        "{{_k: ([{child_proj}(_x) for _x in rt_expect_list(_dv, {target_lit}) if _x is not None] if _dv is not None else None) for _k, _dv in rt_expect_dict({v_expr}, {target_lit}).items()}}"
                    )
                } else {
                    format!(
                        "{{_k: ({child_proj}(_dv) if _dv is not None else None) for _k, _dv in rt_expect_dict({v_expr}, {target_lit}).items()}}"
                    )
                };
                if self.options.models == PythonModels::Dataclass || key_is_non_str {
                    builder.require_import("from typing import Any, cast");
                    return format!("cast(Any, {comp})");
                }
                return comp;
            }

            if f_val_info.is_some_and(|vi| vi.is_array) {
                let comp = format!(
                    "{{_k: (rt_expect_list(_dv, {target_lit}) if _dv is not None else None) for _k, _dv in rt_expect_dict({v_expr}, {target_lit}).items()}}"
                );
                if self.options.models == PythonModels::Dataclass || key_is_non_str {
                    builder.require_import("from typing import Any, cast");
                    return format!("cast(Any, {comp})");
                }
                return comp;
            }

            return format!("dict(rt_expect_dict({v_expr}, {target_lit}))");
        }

        if let BaseType::Json(ref ref_name) = field.type_info.base {
            let child_proj = python_json_projector_name(ref_name);
            let child_is_arr_def = module.json_def(ref_name).is_some_and(|j| j.is_array);
            if field.type_info.is_array && !child_is_arr_def {
                return format!(
                    "[{child_proj}(_x) for _x in rt_expect_list({v_expr}, {target_lit}) if _x is not None]"
                );
            }
            return format!("{child_proj}({v_expr})");
        }

        if field.type_info.is_array {
            return format!("rt_expect_list({v_expr}, {target_lit})");
        }

        v_expr.to_owned()
    }
}

pub(crate) fn json_field_needs_transform(field: &JsonFieldDef) -> bool {
    if field.is_dict || field.type_info.is_dict {
        return true;
    }
    if matches!(field.type_info.base, BaseType::Json(_)) {
        return true;
    }
    field.type_info.is_array
}

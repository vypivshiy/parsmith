//! Elixir `JsonDef` module generation (`@derive JSON.Encoder` / `defimpl JSON.Encoder`, `defstruct`)
//! and allowlist JSON projector emission (`project/1`, `from_value/1`, `from_json/1`).

use crate::ast::{BaseType, JsonDef, JsonFieldDef, Module, TypeInfo};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::elixir::{
    ElixirEmitter, elixir_field_name, elixir_key_type, elixir_pascal, elixir_str, emit_elixir_doc,
    format_elixir_module_prefix,
};
use crate::codegen::writer::{CodeWriter, wln};

impl ElixirEmitter {
    /// Resolves the Elixir `@type` specification of a [`JsonFieldDef`], accounting for inline `(dict)` fields and `@omitempty`.
    pub fn resolve_json_field_type(&self, field: &JsonFieldDef, module: &Module) -> String {
        let optional = field.is_optional
            || field.omitempty
            || field.type_info.is_optional
            || field.type_info.omitempty;
        if field.is_dict && !field.type_info.is_dict {
            let key_type = elixir_key_type(
                field
                    .key_type
                    .as_ref()
                    .or(field.type_info.key_type.as_deref()),
            );
            let val_type = field
                .value_type
                .as_ref()
                .or(field.type_info.value_type.as_deref())
                .map(|vt| self.resolve_type(vt, module))
                .unwrap_or_else(|| "String.t()".to_owned());
            let mut ty = format!("%{{optional({key_type}) => {val_type}}}");
            if field.type_info.is_array {
                ty = format!("[{ty}]");
            }
            if optional {
                ty = format!("{ty} | nil");
            }
            return ty;
        }
        let mut info = field.type_info.clone();
        if optional {
            info.is_optional = true;
        }
        self.resolve_type(&info, module)
    }

    /// Emits a `JsonDef` module (`defstruct` or `@type t` map alias) and its allowlist projector functions.
    pub(crate) fn emit_json_def_impl(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let schema_name = elixir_pascal(&json_def.name);
        let mod_name = format!("{prefix}{schema_name}Json");
        let schema_lit = elixir_str(&schema_name);

        wln!(writer, "defmodule {mod_name} do");
        writer.indented(|writer| {
            self.emit_rt_alias(writer);
            if let Some(ref doc) = json_def.doc {
                emit_elixir_doc(doc, "@moduledoc", writer);
            }

            if json_def.is_dict {
                let key_type = elixir_key_type(json_def.key_type.as_ref());
                let val_type = json_def
                    .value_type
                    .as_ref()
                    .map(|vt| self.resolve_type(vt, module))
                    .unwrap_or_else(|| "String.t()".to_owned());
                wln!(writer, "@type t :: %{{optional({key_type}) => {val_type}}}");
                writer.blankline();

                writer.writeln("def project(data) do");
                writer.indented(|writer| {
                    if let Some(ref root_path) = json_def.path {
                        wln!(
                            writer,
                            "data = Rt.json_path!(data, {}, {schema_lit})",
                            elixir_str(root_path)
                        );
                    }
                    let decode_expr = self.render_dict_decode_expr(
                        "data",
                        json_def.key_type.as_ref(),
                        json_def.value_type.as_ref(),
                        &schema_name,
                        module,
                    );
                    writer.writeln(decode_expr);
                });
                writer.writeln("end");
                writer.blankline();

                Self::emit_json_convenience_fns(&schema_lit, writer);
                return;
            }

            let active_fields: Vec<&JsonFieldDef> = json_def
                .fields
                .iter()
                .filter(|f| !f.skip && !f.type_info.skip)
                .collect();

            let omitempty_fields: Vec<String> = active_fields
                .iter()
                .filter(|f| f.omitempty || f.type_info.omitempty)
                .map(|f| format!(":{}", elixir_field_name(&f.name)))
                .collect();

            if omitempty_fields.is_empty() {
                writer.writeln("@derive JSON.Encoder");
            }

            let field_atoms: Vec<String> = active_fields
                .iter()
                .map(|f| format!(":{}", elixir_field_name(&f.name)))
                .collect();
            wln!(writer, "defstruct [{}]", field_atoms.join(", "));
            writer.blankline();

            let type_name = if json_def.is_array { "item_t" } else { "t" };
            if active_fields.is_empty() {
                wln!(writer, "@type {type_name} :: %__MODULE__{{}}");
            } else {
                wln!(writer, "@type {type_name} :: %__MODULE__{{");
                writer.indented(|writer| {
                    for (idx, field) in active_fields.iter().enumerate() {
                        let f_atom = elixir_field_name(&field.name);
                        let f_ty = self.resolve_json_field_type(field, module);
                        let comma = if idx + 1 < active_fields.len() {
                            ","
                        } else {
                            ""
                        };
                        wln!(writer, "{f_atom}: {f_ty}{comma}");
                    }
                });
                writer.writeln("}");
            }
            if json_def.is_array {
                writer.writeln("@type t :: [item_t()]");
            }
            writer.blankline();

            if !omitempty_fields.is_empty() {
                writer.writeln("defimpl JSON.Encoder do");
                writer.indented(|writer| {
                    writer.writeln("def encode(value, encoder) do");
                    writer.indented(|writer| {
                        writer.writeln("value");
                        writer.writeln("|> Map.from_struct()");
                        wln!(
                            writer,
                            "|> Map.reject(fn {{k, v}} -> k in [{}] and is_nil(v) end)",
                            omitempty_fields.join(", ")
                        );
                        writer.writeln("|> JSON.Encoder.Map.encode(encoder)");
                    });
                    writer.writeln("end");
                });
                writer.writeln("end");
                writer.blankline();
            }

            if json_def.is_array {
                writer.writeln("def project(value) do");
                writer.indented(|writer| {
                    if let Some(ref root_path) = json_def.path {
                        wln!(
                            writer,
                            "value = Rt.json_path!(value, {}, {schema_lit})",
                            elixir_str(root_path)
                        );
                    }
                    wln!(
                        writer,
                        "value |> Rt.expect_array!({schema_lit}) |> Enum.map(&project_item/1)"
                    );
                });
                writer.writeln("end");
                writer.blankline();

                writer.writeln("def project_item(value) do");
                writer.indented(|writer| {
                    self.emit_json_object_projection(&active_fields, &schema_name, module, writer);
                });
                writer.writeln("end");
                writer.blankline();
            } else {
                writer.writeln("def project(value) do");
                writer.indented(|writer| {
                    if let Some(ref root_path) = json_def.path {
                        wln!(
                            writer,
                            "value = Rt.json_path!(value, {}, {schema_lit})",
                            elixir_str(root_path)
                        );
                    }
                    self.emit_json_object_projection(&active_fields, &schema_name, module, writer);
                });
                writer.writeln("end");
                writer.blankline();
            }

            Self::emit_json_convenience_fns(&schema_lit, writer);
        });
        writer.writeln("end");
        writer.blankline();
    }

    fn emit_json_convenience_fns(schema_lit: &str, writer: &mut CodeWriter) {
        writer.writeln("def from_value(value), do: Rt.wrap_result(fn -> project(value) end)");
        writer.writeln("def from_value!(value), do: project(value)");
        writer.writeln(
            "def from_json(raw) when is_binary(raw), do: Rt.wrap_result(fn -> from_json!(raw) end)",
        );
        wln!(
            writer,
            "def from_json!(raw) when is_binary(raw), do: raw |> Rt.decode_json!({schema_lit}) |> project()"
        );
    }

    fn emit_json_object_projection(
        &self,
        active_fields: &[&JsonFieldDef],
        schema_name: &str,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let schema_lit = elixir_str(schema_name);
        if active_fields.is_empty() {
            wln!(writer, "_ = Rt.expect_object!(value, {schema_lit})");
            writer.writeln("%__MODULE__{}");
            return;
        }

        wln!(writer, "obj = Rt.expect_object!(value, {schema_lit})");
        for field in active_fields {
            self.emit_json_field_decode(field, schema_name, module, writer);
        }

        writer.writeln("%__MODULE__{");
        writer.indented(|writer| {
            for (idx, field) in active_fields.iter().enumerate() {
                let f_name = elixir_field_name(&field.name);
                let comma = if idx + 1 < active_fields.len() {
                    ","
                } else {
                    ""
                };
                wln!(writer, "{f_name}: f_{f_name}{comma}");
            }
        });
        writer.writeln("}");
    }

    fn emit_json_field_decode(
        &self,
        field: &JsonFieldDef,
        schema_name: &str,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let info = &field.type_info;
        let f_name = elixir_field_name(&field.name);
        let var_name = format!("f_{f_name}");
        let wire = field.alias.as_deref().unwrap_or(&field.name);
        let optional = field.is_optional || field.omitempty || info.is_optional || info.omitempty;
        let path_lit = elixir_str(wire);
        let ctx = format!("{schema_name}.{}", field.name);
        let ctx_lit = elixir_str(&ctx);

        let decode_raw = self.render_field_value_decode("raw", field, &ctx, module);

        wln!(
            writer,
            "{var_name} = case Rt.json_path(obj, {path_lit}, {ctx_lit}) do"
        );
        writer.indented(|writer| {
            if optional {
                wln!(writer, "{{:ok, raw}} when not is_nil(raw) -> {decode_raw}");
                writer.writeln("_ -> nil");
            } else {
                wln!(writer, "{{:ok, raw}} when not is_nil(raw) -> {decode_raw}");
                wln!(
                    writer,
                    "{{:ok, nil}} -> raise Rt.JsonError.new({ctx_lit}, \"required field is null\")"
                );
                wln!(
                    writer,
                    ":missing -> raise Rt.JsonError.new({ctx_lit}, \"missing required field\")"
                );
            }
        });
        writer.writeln("end");
    }

    fn render_field_value_decode(
        &self,
        raw_var: &str,
        field: &JsonFieldDef,
        ctx: &str,
        module: &Module,
    ) -> String {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let info = &field.type_info;
        let ctx_lit = elixir_str(ctx);

        let is_ref_dict = match &info.base {
            BaseType::Json(ref_name) => module.json_def(ref_name).is_some_and(|j| j.is_dict),
            _ => false,
        };

        if is_ref_dict {
            if let BaseType::Json(ref ref_name) = info.base {
                let child_mod = format!("{prefix}{}Json", elixir_pascal(ref_name));
                if info.is_array {
                    return format!("Rt.decode_list({raw_var}, &{child_mod}.project/1, {ctx_lit})");
                }
                return format!("{child_mod}.project({raw_var})");
            }
        }

        if field.is_dict || info.is_dict {
            let key_info = field.key_type.as_ref().or(info.key_type.as_deref());
            let val_info = field.value_type.as_ref().or(info.value_type.as_deref());
            if info.is_array {
                let item_dict =
                    self.render_dict_decode_expr("item", key_info, val_info, ctx, module);
                return format!("Rt.decode_list({raw_var}, fn item -> {item_dict} end, {ctx_lit})");
            }
            return self.render_dict_decode_expr(raw_var, key_info, val_info, ctx, module);
        }

        if let BaseType::Json(ref ref_name) = info.base {
            let child_mod = format!("{prefix}{}Json", elixir_pascal(ref_name));
            let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
            if def_is_arr {
                return format!("{child_mod}.project({raw_var})");
            } else if info.is_array {
                return format!("Rt.decode_list({raw_var}, &{child_mod}.project/1, {ctx_lit})");
            } else {
                return format!("{child_mod}.project({raw_var})");
            }
        }

        if info.is_array {
            let elem_decode = Self::render_primitive_decode("item", &info.base, &ctx_lit);
            return format!("Rt.decode_list({raw_var}, fn item -> {elem_decode} end, {ctx_lit})");
        }

        Self::render_primitive_decode(raw_var, &info.base, &ctx_lit)
    }

    fn render_dict_decode_expr(
        &self,
        raw_var: &str,
        key_info: Option<&TypeInfo>,
        val_info: Option<&TypeInfo>,
        ctx: &str,
        module: &Module,
    ) -> String {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let ctx_lit = elixir_str(ctx);

        let key_fn = match key_info.map(|k| &k.base) {
            Some(BaseType::Int) => format!("fn k -> Rt.decode_int_key(k, {ctx_lit}) end"),
            Some(BaseType::Float) => format!("fn k -> Rt.decode_float_key(k, {ctx_lit}) end"),
            Some(BaseType::Bool) => format!("fn k -> Rt.decode_bool_key(k, {ctx_lit}) end"),
            _ => "fn k -> k end".to_owned(),
        };

        let inner_val = match val_info {
            Some(vt) if vt.is_dict => self.render_dict_decode_expr(
                "dv",
                vt.key_type.as_deref(),
                vt.value_type.as_deref(),
                ctx,
                module,
            ),
            Some(vt) => match &vt.base {
                BaseType::Json(ref_name) => {
                    let child_mod = format!("{prefix}{}Json", elixir_pascal(ref_name));
                    let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
                    if vt.is_array && !def_is_arr {
                        format!("Rt.decode_list(dv, &{child_mod}.project/1, {ctx_lit})")
                    } else {
                        format!("{child_mod}.project(dv)")
                    }
                }
                base if vt.is_array => {
                    let elem = Self::render_primitive_decode("item", base, &ctx_lit);
                    format!("Rt.decode_list(dv, fn item -> {elem} end, {ctx_lit})")
                }
                base => Self::render_primitive_decode("dv", base, &ctx_lit),
            },
            None => "dv".to_owned(),
        };

        let val_fn = if val_info.is_some_and(|vt| vt.is_optional || vt.omitempty) {
            format!("fn dv -> if is_nil(dv), do: nil, else: {inner_val} end")
        } else {
            format!("fn dv -> {inner_val} end")
        };

        format!("Rt.decode_map({raw_var}, {key_fn}, {val_fn}, {ctx_lit})")
    }

    fn render_primitive_decode(var: &str, base: &BaseType, ctx_lit: &str) -> String {
        match base {
            BaseType::String => format!("Rt.decode_string({var}, {ctx_lit})"),
            BaseType::Int => format!("Rt.decode_int({var}, {ctx_lit})"),
            BaseType::Float => format!("Rt.decode_float({var}, {ctx_lit})"),
            BaseType::Bool => format!("Rt.decode_bool({var}, {ctx_lit})"),
            _ => var.to_owned(),
        }
    }
}

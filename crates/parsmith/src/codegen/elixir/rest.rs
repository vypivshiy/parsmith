//! Elixir `@request` HTML/Raw fetch and `(rest)struct` client code generation.

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultVariantDef,
    StructDecl,
};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::elixir::{
    ElixirEmitter, elixir_ident, elixir_pascal, elixir_str, elixir_str_inner, emit_elixir_doc,
    format_elixir_module_prefix,
};
use crate::codegen::writer::{CodeWriter, wln};
use crate::linter::symbols::to_snake_case;
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderStyle,
    PlaceholderTemplate, RequestBodyKind, RequestPayload, RequestSpec,
};

/// Renders a [`PlaceholderTemplate`] as an Elixir double-quoted string literal or interpolated string.
pub fn render_template(tmpl: &PlaceholderTemplate) -> String {
    if !tmpl.has_placeholders() {
        return elixir_str(&tmpl.source());
    }

    let mut out = String::from("\"");
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Literal(lit) => {
                out.push_str(&elixir_str_inner(lit));
            }
            PlaceholderPart::Placeholder(ph) => {
                let ident = elixir_ident(&to_snake_case(&ph.name));
                out.push_str(&format!("#{{{ident}}}"));
            }
        }
    }
    out.push('"');
    out
}

fn render_json_template_value(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "nil".to_owned(),
        JsonTemplateValue::Bool(b) => b.to_string(),
        JsonTemplateValue::Number(n) => n.clone(),
        JsonTemplateValue::String(s) => elixir_str(s),
        JsonTemplateValue::Placeholder(ph) => elixir_ident(&to_snake_case(&ph.name)),
        JsonTemplateValue::Template(tmpl) => render_template(tmpl),
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template_value).collect();
            format!("[{}]", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} => {}", elixir_str(k), render_json_template_value(v)))
                .collect();
            format!("%{{{}}}", inner.join(", "))
        }
    }
}

/// Renders a JSON request body as an Elixir map/list expression.
pub fn render_json_body(spec: &RequestSpec) -> Option<String> {
    spec.json_template.as_ref().map(render_json_template_value)
}

fn emit_kv_pair(var_name: &str, key: &str, tmpl: &PlaceholderTemplate, writer: &mut CodeWriter) {
    if let Some(ph) = tmpl.single_placeholder() {
        let var = elixir_ident(&to_snake_case(&ph.name));
        let effective_key = if ph.is_array && ph.style == Some(PlaceholderStyle::Bracket) {
            format!("{key}[]")
        } else {
            key.to_owned()
        };
        let key_lit = elixir_str(&effective_key);

        if ph.is_array {
            if let Some(sep) = ph.array_join_sep() {
                let sep_lit = elixir_str(sep);
                if ph.is_optional {
                    wln!(
                        writer,
                        "{var_name} = if is_nil({var}), do: {var_name}, else: {var_name} ++ [{{{key_lit}, Enum.map_join({var}, {sep_lit}, &to_string/1)}}]"
                    );
                } else {
                    wln!(
                        writer,
                        "{var_name} = {var_name} ++ [{{{key_lit}, Enum.map_join({var}, {sep_lit}, &to_string/1)}}]"
                    );
                }
            } else if ph.is_optional {
                wln!(
                    writer,
                    "{var_name} = if is_nil({var}), do: {var_name}, else: {var_name} ++ Enum.map({var}, fn item -> {{{key_lit}, to_string(item)}} end)"
                );
            } else {
                wln!(
                    writer,
                    "{var_name} = {var_name} ++ Enum.map({var}, fn item -> {{{key_lit}, to_string(item)}} end)"
                );
            }
        } else if ph.is_optional {
            wln!(
                writer,
                "{var_name} = if is_nil({var}), do: {var_name}, else: {var_name} ++ [{{{key_lit}, to_string({var})}}]"
            );
        } else {
            wln!(
                writer,
                "{var_name} = {var_name} ++ [{{{key_lit}, to_string({var})}}]"
            );
        }
    } else {
        let val_expr = render_template(tmpl);
        wln!(
            writer,
            "{var_name} = {var_name} ++ [{{{}, {val_expr}}}]",
            elixir_str(key)
        );
    }
}

fn emit_request_builder(spec: &RequestSpec, writer: &mut CodeWriter) {
    let url_expr = render_template(&spec.url);
    let method_lit = elixir_str(&spec.method.to_ascii_uppercase());

    if !spec.params.is_empty() {
        writer.writeln("query_pairs = []");
        for (key, tmpls) in &spec.params {
            for tmpl in tmpls {
                emit_kv_pair("query_pairs", key, tmpl, writer);
            }
        }
    }

    if !spec.headers.is_empty() {
        writer.writeln("headers = %{}");
        for (k, tmpl) in &spec.headers {
            let k_lit = elixir_str(k);
            if let Some(ph) = tmpl.single_placeholder() {
                let var = elixir_ident(&to_snake_case(&ph.name));
                if ph.is_optional {
                    wln!(
                        writer,
                        "headers = if is_nil({var}), do: headers, else: Map.put(headers, {k_lit}, to_string({var}))"
                    );
                } else {
                    wln!(
                        writer,
                        "headers = Map.put(headers, {k_lit}, to_string({var}))"
                    );
                }
            } else {
                wln!(
                    writer,
                    "headers = Map.put(headers, {k_lit}, {})",
                    render_template(tmpl)
                );
            }
        }
    }

    if !spec.cookies.is_empty() {
        writer.writeln("cookies = %{}");
        for (k, tmpl) in &spec.cookies {
            let k_lit = elixir_str(k);
            if let Some(ph) = tmpl.single_placeholder() {
                let var = elixir_ident(&to_snake_case(&ph.name));
                if ph.is_optional {
                    wln!(
                        writer,
                        "cookies = if is_nil({var}), do: cookies, else: Map.put(cookies, {k_lit}, to_string({var}))"
                    );
                } else {
                    wln!(
                        writer,
                        "cookies = Map.put(cookies, {k_lit}, to_string({var}))"
                    );
                }
            } else {
                wln!(
                    writer,
                    "cookies = Map.put(cookies, {k_lit}, {})",
                    render_template(tmpl)
                );
            }
        }
    }

    let (body_expr, body_kind) = match spec.body_kind {
        RequestBodyKind::Empty => ("nil".to_owned(), ":empty"),
        RequestBodyKind::Json => (
            render_json_body(spec).unwrap_or_else(|| "nil".to_owned()),
            ":json",
        ),
        RequestBodyKind::Form => {
            if let RequestPayload::Form(ref form_map) = spec.payload {
                writer.writeln("form_pairs = []");
                for (k, tmpl) in form_map {
                    emit_kv_pair("form_pairs", k, tmpl, writer);
                }
                ("form_pairs".to_owned(), ":form")
            } else {
                ("nil".to_owned(), ":empty")
            }
        }
        RequestBodyKind::Raw => {
            if let RequestPayload::Template(ref tmpl) = spec.payload {
                (render_template(tmpl), ":raw")
            } else {
                ("nil".to_owned(), ":empty")
            }
        }
    };

    let params_expr = if spec.params.is_empty() {
        "[]"
    } else {
        "query_pairs"
    };
    let headers_expr = if spec.headers.is_empty() {
        "%{}"
    } else {
        "headers"
    };
    let cookies_expr = if spec.cookies.is_empty() {
        "%{}"
    } else {
        "cookies"
    };

    writer.writeln("req = %Rt.HttpRequest{");
    writer.indented(|writer| {
        wln!(writer, "method: {method_lit},");
        wln!(writer, "url: {url_expr},");
        wln!(writer, "headers: {headers_expr},");
        wln!(writer, "params: {params_expr},");
        wln!(writer, "cookies: {cookies_expr},");
        wln!(writer, "body: {body_expr},");
        wln!(writer, "body_kind: {body_kind}");
    });
    writer.writeln("}");
}

fn placeholder_params(placeholders: &[PlaceholderSpec]) -> (Vec<String>, Vec<String>) {
    if placeholders.is_empty() {
        return (
            vec![
                "client \\\\ Rt.via_httpc()".to_owned(),
                "opts \\\\ []".to_owned(),
            ],
            vec!["client".to_owned(), "opts".to_owned()],
        );
    }

    let ordered = PlaceholderSpec::sort_required_first(placeholders);
    let mut def_params = Vec::with_capacity(ordered.len() + 2);
    let mut call_args = Vec::with_capacity(ordered.len() + 2);
    def_params.push("client".to_owned());
    call_args.push("client".to_owned());

    for ph in ordered {
        let name = elixir_ident(&to_snake_case(&ph.name));
        call_args.push(name.clone());
        if ph.is_optional {
            def_params.push(format!("{name} \\\\ nil"));
        } else {
            def_params.push(name);
        }
    }

    def_params.push("opts \\\\ []".to_owned());
    call_args.push("opts".to_owned());
    (def_params, call_args)
}

fn render_error_cond(path: &str, val: &ErrorConditionValue) -> String {
    let path_lit = elixir_str(path);
    match val {
        ErrorConditionValue::Null => format!("{{:null, {path_lit}}}"),
        ErrorConditionValue::Bool(b) => format!("{{:bool, {path_lit}, {b}}}"),
        ErrorConditionValue::Int(n) => format!("{{:int, {path_lit}, {n}}}"),
        ErrorConditionValue::Float(f) => format!("{{:float, {path_lit}, {f:?}}}"),
        ErrorConditionValue::String(s) => format!("{{:str, {path_lit}, {}}}", elixir_str(s)),
    }
}

impl ElixirEmitter {
    /// Emits `fetch` and `fetch!` (or `fetch_<suffix>` and `fetch_<suffix>!`) functions on an HTML or Raw struct module.
    pub(crate) fn emit_request_impl(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let struct_name = elixir_pascal(&struct_decl.name);
        let raw_method_name = if request.name.is_empty() || request.name == "fetch" {
            "fetch".to_owned()
        } else {
            let snake = to_snake_case(&request.name);
            let clean = snake.strip_prefix("fetch_").unwrap_or(&snake);
            format!("fetch_{clean}")
        };
        let method_ident = elixir_ident(&raw_method_name);
        let bang_ident = format!("{method_ident}!");
        let context = format!("{struct_name}.{raw_method_name}");
        let (def_params, call_args) = placeholder_params(&request.placeholders);

        if let Some(ref doc) = request.doc {
            emit_elixir_doc(doc, "@doc", writer);
        }
        wln!(writer, "def {method_ident}({}) do", def_params.join(", "));
        writer.indented(|writer| {
            emit_request_builder(&request.spec, writer);
            let path_opt = request
                .response_path
                .as_deref()
                .map(elixir_str)
                .unwrap_or_else(|| "nil".to_owned());
            let join_opt = request
                .response_join
                .as_deref()
                .map(elixir_str)
                .unwrap_or_else(|| "nil".to_owned());
            wln!(
                writer,
                "case Rt.fetch_html(client, req, {}, {path_opt}, {join_opt}, opts) do",
                elixir_str(&context)
            );
            writer.indented(|writer| {
                writer.writeln("{:ok, body} -> {:ok, new(body)}");
                writer.writeln("{:error, _} = err -> err");
            });
            writer.writeln("end");
        });
        writer.writeln("end");
        writer.blankline();

        wln!(writer, "def {bang_ident}({}) do", def_params.join(", "));
        writer.indented(|writer| {
            wln!(writer, "case {method_ident}({}) do", call_args.join(", "));
            writer.indented(|writer| {
                writer.writeln("{:ok, parser} -> parser");
                writer.writeln("{:error, exc} -> raise exc");
            });
            writer.writeln("end");
        });
        writer.writeln("end");
        writer.blankline();
    }

    /// Emits a synthesized [`RestArtifact`] (`ErrorVariant` module).
    pub(crate) fn emit_rest_artifact_impl(
        &mut self,
        artifact: &RestArtifact,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let RestArtifact::ErrorVariant(variant) = artifact {
            let prefix = format_elixir_module_prefix(&self.options.package);
            let mod_name = format!("{prefix}{}", elixir_pascal(&variant.name));
            wln!(writer, "defmodule {mod_name} do");
            writer.indented(|writer| {
                writer.writeln("@derive JSON.Encoder");
                writer.writeln("defstruct [:status, :body, :value, headers: %{}, ok: false]");
            });
            writer.writeln("end");
            writer.blankline();
        }
    }

    /// Emits `def matchers do ... end` inside a `(rest)struct` module.
    pub(crate) fn emit_struct_matchers(
        &self,
        struct_decl: &StructDecl,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let matchers_opt: Option<&MatcherListDef> = module.rest_artifacts.iter().find_map(|a| {
            if let RestArtifact::MatcherList(m) = a
                && m.struct_name == struct_decl.name
            {
                Some(m)
            } else {
                None
            }
        });

        let Some(matchers) = matchers_opt else {
            writer.writeln("def matchers, do: []");
            writer.blankline();
            return;
        };

        if matchers.entries.is_empty() {
            writer.writeln("def matchers, do: []");
            writer.blankline();
            return;
        }

        let variants_by_name: IndexMap<&str, &ResultVariantDef> = module
            .rest_artifacts
            .iter()
            .filter_map(|a| match a {
                RestArtifact::ErrorVariant(v) => Some((v.name.as_str(), v)),
                _ => None,
            })
            .collect();

        writer.writeln("def matchers do");
        writer.indented(|writer| {
            writer.writeln("[");
            writer.indented(|writer| {
                for (idx, entry) in matchers.entries.iter().enumerate() {
                    let v_mod = format!("{prefix}{}", elixir_pascal(&entry.factory_name));
                    let req_keys: Vec<String> =
                        entry.required_keys.iter().map(|k| elixir_str(k)).collect();
                    let conds: Vec<String> = entry
                        .conditions
                        .iter()
                        .map(|(p, v)| render_error_cond(p, v))
                        .collect();
                    let has_schema = !entry.error_schema.is_empty();

                    let decode_fn = if has_schema {
                        let schema_mod =
                            format!("{prefix}{}Json", elixir_pascal(&entry.error_schema));
                        let var_def = variants_by_name.get(entry.factory_name.as_str()).copied();
                        let is_array = var_def.is_some_and(|v| v.schema_is_array)
                            || module
                                .json_def(&entry.error_schema)
                                .is_some_and(|j| j.is_array);
                        let def_is_arr = module
                            .json_def(&entry.error_schema)
                            .is_some_and(|j| j.is_array);
                        let proj_expr = if is_array && !def_is_arr {
                            format!(
                                "Rt.decode_list(val, &{schema_mod}.project/1, {})",
                                elixir_str(&entry.factory_name)
                            )
                        } else {
                            format!("{schema_mod}.project(val)")
                        };
                        format!(
                            "fn status, val, headers -> case Rt.wrap_result(fn -> {proj_expr} end) do {{:ok, body}} -> {{:ok, %{v_mod}{{status: status, body: body, value: body, headers: headers}}}}; {{:error, err}} -> {{:error, err}} end end"
                        )
                    } else {
                        format!(
                            "fn status, val, headers -> {{:ok, %{v_mod}{{status: status, body: val, value: val, headers: headers}}}} end"
                        )
                    };

                    let comma = if idx + 1 < matchers.entries.len() {
                        ","
                    } else {
                        ""
                    };
                    writer.writeln("%Rt.ErrorMatcher{");
                    writer.indented(|writer| {
                        wln!(writer, "status: {},", entry.status);
                        wln!(writer, "required_keys: [{}],", req_keys.join(", "));
                        wln!(writer, "conditions: [{}],", conds.join(", "));
                        wln!(writer, "has_schema: {has_schema},");
                        wln!(writer, "decode: {decode_fn}");
                    });
                    wln!(writer, "}}{comma}");
                }
            });
            writer.writeln("]");
        });
        writer.writeln("end");
        writer.blankline();
    }

    /// Emits a REST endpoint function on a `(rest)struct` module returning `{:ok, value} | {:error, error_struct}`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let struct_pascal = elixir_pascal(&struct_decl.name);
        let raw_method_name = if endpoint.name.is_empty() {
            "fetch".to_owned()
        } else {
            to_snake_case(&endpoint.name)
        };
        let method_ident = elixir_ident(&raw_method_name);
        let context = format!("{struct_pascal}.{raw_method_name}");
        let (def_params, _call_args) = placeholder_params(&endpoint.placeholders);

        if let Some(ref doc) = endpoint.doc {
            emit_elixir_doc(doc, "@doc", writer);
        }

        wln!(writer, "def {method_ident}({}) do", def_params.join(", "));
        writer.indented(|writer| {
            emit_request_builder(&endpoint.spec, writer);
            let ctx_lit = elixir_str(&context);
            let path_opt = endpoint
                .response_path
                .as_deref()
                .map(elixir_str)
                .unwrap_or_else(|| "nil".to_owned());

            if let Some(ref schema_name) = endpoint.response_schema {
                let schema_mod = format!("{prefix}{}Json", elixir_pascal(schema_name));
                let def_is_arr = module.json_def(schema_name).is_some_and(|j| j.is_array);
                let decode_fn = if endpoint.response_is_array && !def_is_arr {
                    format!(
                        "fn val -> Rt.wrap_result(fn -> Rt.decode_list(val, &{schema_mod}.project/1, {ctx_lit}) end) end"
                    )
                } else {
                    format!("&{schema_mod}.from_value/1")
                };
                wln!(writer,
                    "Rt.rest_call(client, req, {path_opt}, {ctx_lit}, matchers(), {decode_fn}, opts)"
                );
            } else if endpoint.response_path.is_some() {
                wln!(writer,
                    "Rt.rest_call(client, req, {path_opt}, {ctx_lit}, matchers(), fn val -> {{:ok, val}} end, opts)"
                );
            } else {
                wln!(writer,
                    "Rt.rest_call_void(client, req, {ctx_lit}, matchers(), opts)"
                );
            }
        });
        writer.writeln("end");
        writer.blankline();
    }
}

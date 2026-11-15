//! Rust `reqwest` async `@request` fetch and `(rest)struct` client code generation.

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultAliasDef,
    ResultVariantDef, StructDecl, StructKind,
};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::rust::{RustEmitter, emit_rust_doc, rust_ident, rust_pascal, rust_str};
use crate::codegen::writer::{CodeWriter, wln};
use crate::linter::symbols::to_snake_case;
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderStyle,
    PlaceholderTemplate, PlaceholderType, RequestBodyKind, RequestPayload, RequestSpec,
};

/// Maps a [`PlaceholderSpec`] to its idiomatic Rust parameter type annotation.
pub fn rust_ph_type(ph: &PlaceholderSpec) -> String {
    if ph.is_array {
        let elem = match ph.type_name {
            PlaceholderType::Int => "i64",
            PlaceholderType::Float => "f64",
            PlaceholderType::Bool => "bool",
            PlaceholderType::Str => "&str",
        };
        let base = format!("&[{elem}]");
        return if ph.is_optional {
            format!("Option<{base}>")
        } else {
            base
        };
    }

    if ph.is_optional {
        return match ph.type_name {
            PlaceholderType::Int => "Option<i64>".to_owned(),
            PlaceholderType::Float => "Option<f64>".to_owned(),
            PlaceholderType::Bool => "Option<bool>".to_owned(),
            PlaceholderType::Str => "Option<&str>".to_owned(),
        };
    }

    match ph.type_name {
        PlaceholderType::Int => "i64".to_owned(),
        PlaceholderType::Float => "f64".to_owned(),
        PlaceholderType::Bool => "bool".to_owned(),
        PlaceholderType::Str => "&str".to_owned(),
    }
}

/// Renders a [`PlaceholderTemplate`] as a Rust string literal or `format!(...)` expression.
pub fn render_template(tmpl: &PlaceholderTemplate) -> String {
    if !tmpl.has_placeholders() {
        return rust_str(&tmpl.source());
    }

    let mut fmt_str = String::new();
    let mut args: Vec<String> = Vec::new();
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Placeholder(ph) => {
                fmt_str.push_str("{}");
                let ident = rust_ident(&to_snake_case(&ph.name));
                if ph.is_optional {
                    if ph.type_name == PlaceholderType::Str && !ph.is_array {
                        args.push(format!("{ident}.unwrap_or_default()"));
                    } else {
                        args.push(format!(
                            "{ident}.map(|v| v.to_string()).unwrap_or_default()"
                        ));
                    }
                } else {
                    args.push(ident);
                }
            }
            PlaceholderPart::Literal(lit) => {
                fmt_str.push_str(&lit.replace('{', "{{").replace('}', "}}"));
            }
        }
    }
    format!("format!({}, {})", rust_str(&fmt_str), args.join(", "))
}

fn render_json_template_value(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "null".to_owned(),
        JsonTemplateValue::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        JsonTemplateValue::Number(n) => n.clone(),
        JsonTemplateValue::String(s) => rust_str(s),
        JsonTemplateValue::Placeholder(ph) => {
            format!("({})", rust_ident(&to_snake_case(&ph.name)))
        }
        JsonTemplateValue::Template(tmpl) => {
            if tmpl.has_placeholders() {
                format!("({})", render_template(tmpl))
            } else {
                rust_str(&tmpl.source())
            }
        }
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template_value).collect();
            format!("[{}]", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", rust_str(k), render_json_template_value(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Renders a JSON request body as a `serde_json::json!(...)` macro invocation.
pub fn render_json_body(spec: &RequestSpec) -> Option<String> {
    spec.json_template
        .as_ref()
        .map(|jt| format!("serde_json::json!({})", render_json_template_value(jt)))
}

fn emit_kv_pair(var_name: &str, key: &str, tmpl: &PlaceholderTemplate, writer: &mut CodeWriter) {
    if let Some(ph) = tmpl.single_placeholder() {
        let var = rust_ident(&to_snake_case(&ph.name));
        let effective_key = if ph.is_array && ph.style == Some(PlaceholderStyle::Bracket) {
            format!("{key}[]")
        } else {
            key.to_owned()
        };
        let key_lit = rust_str(&effective_key);

        if ph.is_array {
            if let Some(sep) = ph.array_join_sep() {
                let sep_lit = rust_str(sep);
                if ph.is_optional {
                    wln!(writer, "if let Some(items) = {var} {{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "let val = items.iter().map(|x| x.to_string()).collect::<Vec<_>>().join({sep_lit});"
                        );
                        wln!(writer, "{var_name}.push(({key_lit}, val));");
                    });
                    writer.writeln("}");
                } else {
                    wln!(
                        writer,
                        "let val = {var}.iter().map(|x| x.to_string()).collect::<Vec<_>>().join({sep_lit});"
                    );
                    wln!(writer, "{var_name}.push(({key_lit}, val));");
                }
            } else if ph.is_optional {
                wln!(writer, "if let Some(items) = {var} {{");
                writer.indented(|writer| {
                    writer.writeln("for item in items {");
                    writer.indented(|writer| {
                        wln!(writer, "{var_name}.push(({key_lit}, item.to_string()));");
                    });
                    writer.writeln("}");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "for item in {var} {{");
                writer.indented(|writer| {
                    wln!(writer, "{var_name}.push(({key_lit}, item.to_string()));");
                });
                writer.writeln("}");
            }
        } else if ph.is_optional {
            wln!(writer, "if let Some(val) = {var} {{");
            writer.indented(|writer| {
                wln!(writer, "{var_name}.push(({key_lit}, val.to_string()));");
            });
            writer.writeln("}");
        } else {
            wln!(writer, "{var_name}.push(({key_lit}, {var}.to_string()));");
        }
    } else if tmpl.has_placeholders() {
        let val_expr = render_template(tmpl);
        wln!(writer, "{var_name}.push(({}, {val_expr}));", rust_str(key));
    } else {
        wln!(
            writer,
            "{var_name}.push(({}, {}.to_string()));",
            rust_str(key),
            rust_str(&tmpl.source())
        );
    }
}

fn emit_request_builder(spec: &RequestSpec, context: &str, writer: &mut CodeWriter) {
    let url_expr = render_template(&spec.url);
    let method_upper = spec.method.to_ascii_uppercase();
    let method_expr = if matches!(
        method_upper.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
    ) {
        format!("reqwest::Method::{method_upper}")
    } else if !context.is_empty() {
        format!(
            "reqwest::Method::from_bytes({}.as_bytes()).map_err(|e| rt::RtError::new({}, e.to_string()))?",
            rust_str(&method_upper),
            rust_str(context)
        )
    } else {
        format!(
            "reqwest::Method::from_bytes({}.as_bytes()).expect(\"valid HTTP method\")",
            rust_str(&method_upper)
        )
    };

    wln!(
        writer,
        "let mut req = client.request({method_expr}, {url_expr});"
    );
    writer.writeln("let _ = &mut req;");

    if !spec.params.is_empty() {
        writer.writeln("let mut query_pairs: Vec<(&str, String)> = Vec::new();");
        for (key, tmpls) in &spec.params {
            for tmpl in tmpls {
                emit_kv_pair("query_pairs", key, tmpl, writer);
            }
        }
        writer.writeln("req = req.query(&query_pairs);");
    }

    for (k, tmpl) in &spec.headers {
        let k_lit = rust_str(k);
        if let Some(header_ph) = tmpl.single_placeholder() {
            let var = rust_ident(&to_snake_case(&header_ph.name));
            if header_ph.is_optional {
                wln!(writer, "if let Some(val) = {var} {{");
                writer.indented(|writer| {
                    wln!(writer, "req = req.header({k_lit}, val.to_string());");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "req = req.header({k_lit}, {var}.to_string());");
            }
        } else if tmpl.has_placeholders() {
            wln!(
                writer,
                "req = req.header({k_lit}, {});",
                render_template(tmpl)
            );
        } else {
            wln!(
                writer,
                "req = req.header({k_lit}, {});",
                rust_str(&tmpl.source())
            );
        }
    }

    if !spec.cookies.is_empty() {
        writer.writeln("let mut cookies: Vec<String> = Vec::new();");
        for (k, tmpl) in &spec.cookies {
            if let Some(cookie_ph) = tmpl.single_placeholder() {
                let var = rust_ident(&to_snake_case(&cookie_ph.name));
                if cookie_ph.is_optional {
                    wln!(writer, "if let Some(val) = {var} {{");
                    writer.indented(|writer| {
                        wln!(writer, "cookies.push(format!(\"{k}={{val}}\"));");
                    });
                    writer.writeln("}");
                } else {
                    wln!(writer, "cookies.push(format!(\"{k}={{{var}}}\"));");
                }
            } else if tmpl.has_placeholders() {
                wln!(
                    writer,
                    "cookies.push(format!(\"{k}={{}}\", {}));",
                    render_template(tmpl)
                );
            } else {
                let kv_lit = rust_str(&format!("{k}={}", tmpl.source()));
                wln!(writer, "cookies.push({kv_lit}.to_string());");
            }
        }
        writer.writeln("if !cookies.is_empty() {");
        writer.indented(|writer| {
            writer.writeln("req = req.header(\"Cookie\", cookies.join(\"; \"));");
        });
        writer.writeln("}");
    }

    match spec.body_kind {
        RequestBodyKind::Empty => {}
        RequestBodyKind::Json => {
            if let Some(json_expr) = render_json_body(spec) {
                wln!(writer, "req = req.json(&{json_expr});");
            }
        }
        RequestBodyKind::Form => {
            if let RequestPayload::Form(ref form_map) = spec.payload {
                writer.writeln("let mut form_pairs: Vec<(&str, String)> = Vec::new();");
                for (k, tmpl) in form_map {
                    emit_kv_pair("form_pairs", k, tmpl, writer);
                }
                writer.writeln("req = req.form(&form_pairs);");
            }
        }
        RequestBodyKind::Raw => {
            if let RequestPayload::Template(ref tmpl) = spec.payload {
                wln!(writer, "req = req.body({});", render_template(tmpl));
            }
        }
    }
}

fn placeholder_params(placeholders: &[PlaceholderSpec]) -> Vec<String> {
    let ordered = PlaceholderSpec::sort_required_first(placeholders);
    let mut params = Vec::with_capacity(ordered.len() + 1);
    params.push("client: &reqwest::Client".to_owned());
    for ph in ordered {
        let name = rust_ident(&to_snake_case(&ph.name));
        let ty = rust_ph_type(ph);
        params.push(format!("{name}: {ty}"));
    }
    params
}

/// Derives an idiomatic Rust enum variant name from a synthesized error factory name.
pub fn rust_variant_name(struct_name: &str, factory_name: &str) -> String {
    let pascal = rust_pascal(struct_name);
    if let Some(tail) = factory_name.strip_prefix(&pascal)
        && !tail.is_empty()
    {
        return tail.to_owned();
    }
    factory_name.to_owned()
}

fn render_error_cond(path: &str, val: &ErrorConditionValue) -> String {
    let path_lit = rust_str(path);
    match val {
        ErrorConditionValue::Null => format!("rt::ErrorCond::Null({path_lit})"),
        ErrorConditionValue::Bool(b) => format!("rt::ErrorCond::Bool({path_lit}, {b})"),
        ErrorConditionValue::Int(n) => format!("rt::ErrorCond::Int({path_lit}, {n})"),
        ErrorConditionValue::Float(f) => format!("rt::ErrorCond::Float({path_lit}, {f:?})"),
        ErrorConditionValue::String(s) => {
            format!("rt::ErrorCond::Str({path_lit}, {})", rust_str(s))
        }
    }
}

impl RustEmitter {
    /// Emits an asynchronous `fetch` associated method on an HTML or Raw parser struct.
    pub(crate) fn emit_request_impl(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_import("use serde_json::Value;");
        let struct_name = rust_pascal(&struct_decl.name);
        let raw_method_name = if request.name.is_empty() || request.name == "fetch" {
            "fetch".to_owned()
        } else {
            let snake = to_snake_case(&request.name);
            let clean = snake.strip_prefix("fetch_").unwrap_or(&snake);
            format!("fetch_{clean}")
        };
        let method_ident = rust_ident(&raw_method_name);
        let context = format!("{struct_name}.{raw_method_name}");
        let params = placeholder_params(&request.placeholders);

        if let Some(ref doc) = request.doc {
            emit_rust_doc(doc, "//", writer);
        }
        wln!(
            writer,
            "pub async fn {method_ident}({}) -> Result<Self, rt::RtError> {{",
            params.join(", ")
        );
        writer.indented(|writer| {
            emit_request_builder(&request.spec, &context, writer);
            let path_opt = request
                .response_path
                .as_deref()
                .map(|p| format!("Some({})", rust_str(p)))
                .unwrap_or_else(|| "None".to_owned());
            let join_opt = request
                .response_join
                .as_deref()
                .map(|j| format!("Some({})", rust_str(j)))
                .unwrap_or_else(|| "None".to_owned());
            wln!(
                writer,
                "let body = rt::fetch_html(req, {}, {path_opt}, {join_opt}).await?;",
                rust_str(&context)
            );
            writer.writeln("Self::new(body)");
        });
        writer.writeln("}");
        writer.blankline();
    }

    /// Emits a synthesized [`RestArtifact`] (`ErrorVariant`, `ResultAlias`, or `MatcherList`).
    pub(crate) fn emit_rest_artifact_impl(
        &mut self,
        artifact: &RestArtifact,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_import("use serde_json::Value;");
        match artifact {
            RestArtifact::ErrorVariant(_) => {}
            RestArtifact::ResultAlias(alias) => {
                self.emit_result_alias_def(alias, module, writer);
            }
            RestArtifact::MatcherList(matchers) => {
                self.emit_matcher_list_def(matchers, module, writer);
            }
        }
    }

    fn emit_result_alias_def(
        &mut self,
        alias: &ResultAliasDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let struct_pascal = rust_pascal(&alias.struct_name);
        let enum_name = format!("{struct_pascal}Error");
        let ok_type = if let Some(ref schema_name) = alias.response_schema {
            let s_pascal = format!("{}Json", rust_pascal(schema_name));
            let is_arr =
                alias.response_is_array || module.json_def(schema_name).is_some_and(|j| j.is_array);
            if is_arr {
                format!("Vec<{s_pascal}>")
            } else {
                s_pascal
            }
        } else if alias.has_response_path(module) {
            "Value".to_owned()
        } else {
            "()".to_owned()
        };
        let alias_str = format!("Result<{ok_type}, {enum_name}>");

        let effective_name =
            module.resolve_effective_result_alias_name(&alias.struct_name, &alias.name);
        let qualified = format!("{struct_pascal}{}", alias.name);

        let mut emitted_any = false;
        if self.emitted_aliases.insert(effective_name.clone()) {
            wln!(writer, "pub type {effective_name} = {alias_str};");
            emitted_any = true;
        }
        if qualified != effective_name && self.emitted_aliases.insert(qualified.clone()) {
            wln!(writer, "pub type {qualified} = {alias_str};");
            emitted_any = true;
        }
        if emitted_any {
            writer.blankline();
        }
    }

    fn emit_matcher_list_def(
        &self,
        matchers: &MatcherListDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let struct_name = &matchers.struct_name;
        let struct_pascal = rust_pascal(struct_name);
        let struct_upper = to_snake_case(struct_name).to_ascii_uppercase();
        let enum_name = format!("{struct_pascal}Error");

        let variants_by_name: IndexMap<&str, &ResultVariantDef> = module
            .rest_artifacts
            .iter()
            .filter_map(|a| match a {
                RestArtifact::ErrorVariant(v) => Some((v.name.as_str(), v)),
                _ => None,
            })
            .collect();

        // Map: v_name -> (rust_type, status, is_array)
        let mut variant_info: IndexMap<String, (String, u16, bool)> = IndexMap::new();
        for entry in &matchers.entries {
            let v_name = rust_variant_name(struct_name, &entry.factory_name);
            if variant_info.contains_key(&v_name) {
                continue;
            }
            let var_def = variants_by_name.get(entry.factory_name.as_str()).copied();
            if !entry.error_schema.is_empty() {
                let s_pascal = format!("{}Json", rust_pascal(&entry.error_schema));
                let is_array = var_def.is_some_and(|v| v.schema_is_array)
                    || module
                        .json_def(&entry.error_schema)
                        .is_some_and(|j| j.is_array);
                let typ = if is_array {
                    format!("Vec<{s_pascal}>")
                } else {
                    s_pascal
                };
                variant_info.insert(v_name, (typ, entry.status, is_array));
            } else {
                variant_info.insert(
                    v_name,
                    ("serde_json::Value".to_owned(), entry.status, false),
                );
            }
        }

        writer.writeln("#[derive(Debug)]");
        wln!(writer, "pub enum {enum_name} {{");
        writer.indented(|writer| {
            for (v_name, (typ, _, _)) in &variant_info {
                wln!(writer, "{v_name}({typ}),");
            }
            writer.writeln("Transport(reqwest::Error),");
            writer.writeln("Decode(u16, rt::RtError),");
            writer.writeln("Unknown(u16, serde_json::Value),");
        });
        writer.writeln("}");
        writer.blankline();

        wln!(writer, "impl {enum_name} {{");
        writer.indented(|writer| {
            writer.writeln("pub fn status(&self) -> Option<u16> {");
            writer.indented(|writer| {
                writer.writeln("match self {");
                writer.indented(|writer| {
                    for (v_name, (_, status, _)) in &variant_info {
                        wln!(writer, "Self::{v_name}(_) => Some({status}),");
                    }
                    writer.writeln("Self::Decode(status, _) => Some(*status),");
                    writer.writeln("Self::Unknown(status, _) => Some(*status),");
                    writer.writeln("Self::Transport(_) => None,");
                });
                writer.writeln("}");
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();

        wln!(writer, "impl std::fmt::Display for {enum_name} {{");
        writer.indented(|writer| {
            writer.writeln("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {");
            writer.indented(|writer| {
                writer.writeln("match self {");
                writer.indented(|writer| {
                    for (v_name, (_, status, _)) in &variant_info {
                        wln!(writer,
                            "Self::{v_name}(body) => write!(f, \"HTTP {status}: {{:?}}\", body),"
                        );
                    }
                    writer.writeln(
                        "Self::Transport(err) => write!(f, \"transport error: {}\", err),",
                    );
                    writer.writeln(
                        "Self::Decode(status, err) => write!(f, \"HTTP {} decode error: {}\", status, err),",
                    );
                    writer.writeln(
                        "Self::Unknown(status, body) => write!(f, \"HTTP {}: {}\", status, body),",
                    );
                });
                writer.writeln("}");
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();

        wln!(writer, "impl std::error::Error for {enum_name} {{");
        writer.indented(|writer| {
            writer.writeln("fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {");
            writer.indented(|writer| {
                writer.writeln("match self {");
                writer.indented(|writer| {
                    writer.writeln("Self::Transport(err) => Some(err),");
                    writer.writeln("Self::Decode(_, err) => Some(err),");
                    writer.writeln("_ => None,");
                });
                writer.writeln("}");
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();

        wln!(writer, "impl From<reqwest::Error> for {enum_name} {{");
        writer.indented(|writer| {
            writer.writeln("fn from(err: reqwest::Error) -> Self {");
            writer.indented(|writer| {
                writer.writeln("Self::Transport(err)");
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();

        let rest_count = module
            .structs
            .iter()
            .filter(|s| s.kind == StructKind::Rest)
            .count();
        let is_single_rest = rest_count <= 1;

        wln!(
            writer,
            "pub type {struct_pascal}EndpointError = {enum_name};"
        );
        if is_single_rest {
            wln!(writer, "pub type EndpointError = {enum_name};");
        }
        let mut emitted_factory_aliases = std::collections::HashSet::new();
        for entry in &matchers.entries {
            let v_name = rust_variant_name(struct_name, &entry.factory_name);
            if entry.factory_name != v_name
                && emitted_factory_aliases.insert(entry.factory_name.clone())
                && let Some((typ, _, _)) = variant_info.get(&v_name)
            {
                wln!(writer, "pub type {} = {typ};", entry.factory_name);
            }
        }
        writer.blankline();

        let matchers_const = format!("RT_{struct_upper}_MATCHERS");
        if matchers.entries.is_empty() {
            wln!(
                writer,
                "const {matchers_const}: &[rt::ErrorMatcher<{enum_name}>] = &[];"
            );
            writer.blankline();
            return;
        }

        wln!(
            writer,
            "const {matchers_const}: &[rt::ErrorMatcher<{enum_name}>] = &["
        );
        writer.indented(|writer| {
            for entry in &matchers.entries {
                let v_name = rust_variant_name(struct_name, &entry.factory_name);
                let req_keys: Vec<String> =
                    entry.required_keys.iter().map(|k| rust_str(k)).collect();
                let conds: Vec<String> = entry
                    .conditions
                    .iter()
                    .map(|(p, v)| render_error_cond(p, v))
                    .collect();
                let decode_fn = if !entry.error_schema.is_empty() {
                    let (typ, _, _) = &variant_info[&v_name];
                    let ctx_lit = rust_str(&format!("{enum_name}::{v_name}"));
                    format!("|v| Ok({enum_name}::{v_name}(rt::decode::<{typ}>(v, {ctx_lit})?))")
                } else {
                    format!("|v| Ok({enum_name}::{v_name}(v))")
                };

                writer.writeln("rt::ErrorMatcher {");
                writer.indented(|writer| {
                    wln!(writer, "status: {},", entry.status);
                    wln!(writer, "required_keys: &[{}],", req_keys.join(", "));
                    wln!(writer, "conditions: &[{}],", conds.join(", "));
                    wln!(writer, "decode: {decode_fn},");
                });
                writer.writeln("},");
            }
        });
        writer.writeln("];");
        writer.blankline();
    }

    /// Emits an asynchronous REST endpoint associated method on a `(rest)struct`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_import("use serde_json::Value;");
        let struct_name = &struct_decl.name;
        let struct_pascal = rust_pascal(struct_name);
        let struct_upper = to_snake_case(struct_name).to_ascii_uppercase();
        let enum_name = format!("{struct_pascal}Error");
        let raw_method_name = if endpoint.name.is_empty() {
            "fetch".to_owned()
        } else {
            to_snake_case(&endpoint.name)
        };
        let method_ident = rust_ident(&raw_method_name);
        let context = format!("{struct_pascal}.{raw_method_name}");
        let params = placeholder_params(&endpoint.placeholders);

        let ok_type = if let Some(ref schema_name) = endpoint.response_schema {
            let s_pascal = format!("{}Json", rust_pascal(schema_name));
            let is_array = endpoint.response_is_array
                || module.json_def(schema_name).is_some_and(|j| j.is_array);
            if is_array {
                format!("Vec<{s_pascal}>")
            } else {
                s_pascal
            }
        } else if endpoint.response_path.is_some() {
            "Value".to_owned()
        } else {
            "()".to_owned()
        };

        if let Some(ref doc) = endpoint.doc {
            emit_rust_doc(doc, "//", writer);
        }
        wln!(
            writer,
            "pub async fn {method_ident}({}) -> Result<{ok_type}, {enum_name}> {{",
            params.join(", ")
        );
        writer.indented(|writer| {
            emit_request_builder(&endpoint.spec, "", writer);
            let matchers_const = format!("RT_{struct_upper}_MATCHERS");
            if ok_type == "()" {
                wln!(writer,
                    "rt::rest_call_void(req, {}, {matchers_const}, {enum_name}::Transport, {enum_name}::Decode, {enum_name}::Unknown).await",
                    rust_str(&context)
                );
            } else {
                let path_opt = endpoint
                    .response_path
                    .as_deref()
                    .map(|p| format!("Some({})", rust_str(p)))
                    .unwrap_or_else(|| "None".to_owned());
                wln!(writer,
                    "rt::rest_call::<{ok_type}, {enum_name}>(req, {path_opt}, {}, {matchers_const}, {enum_name}::Transport, {enum_name}::Decode, {enum_name}::Unknown).await",
                    rust_str(&context)
                );
            }
        });
        writer.writeln("}");
        writer.blankline();
    }
}

//! Go `@request` HTML/Raw fetch constructors and `(rest)struct` client and `@error` matcher emission.

use std::collections::BTreeSet;

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultVariantDef,
    StructDecl, StructKind,
};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::go::runtime::go_str;
use crate::codegen::go::{GoEmitter, emit_go_doc, receiver};
use crate::codegen::writer::{CodeWriter, wln};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::to_camel_case;
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderStyle,
    PlaceholderTemplate, PlaceholderType, RequestBodyKind, RequestPayload, RequestSpec,
};

impl GoEmitter {
    /// Emits a synthesized [`RestArtifact`] (`ErrorVariant`, `ResultAlias`, or `MatcherList`).
    pub(crate) fn emit_rest_artifact_impl(
        &mut self,
        artifact: &RestArtifact,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        match artifact {
            RestArtifact::ErrorVariant(variant) => {
                builder.require_import("\"fmt\"");
                emit_result_variant_def(variant, writer);
            }
            RestArtifact::ResultAlias(_) => {
                // Go REST endpoints return `(T, error)` tuples directly.
            }
            RestArtifact::MatcherList(matchers) => {
                let has_checks = matchers
                    .entries
                    .iter()
                    .any(|e| !e.required_keys.is_empty() || !e.conditions.is_empty());
                let has_array_schema = matchers.entries.iter().any(|e| {
                    !e.error_schema.is_empty()
                        && module.json_def(&e.error_schema).is_some_and(|j| j.is_array)
                });
                let has_schema_body = matchers.entries.iter().any(|e| !e.error_schema.is_empty());
                if has_schema_body {
                    builder.require_import("\"encoding/json\"");
                    builder.require_import("\"github.com/tidwall/gjson\"");
                }
                if has_checks || has_array_schema {
                    builder.require_import("\"github.com/tidwall/gjson\"");
                }
                emit_matcher_list_def(matchers, module, writer);
            }
        }
    }

    /// Emits a `New<Struct><Suffix>` HTTP fetch constructor function for an HTML or Raw struct.
    pub(crate) fn emit_request_impl(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_import("\"context\"");
        builder.require_import("\"net/http\"");
        if request.spec.body_kind == RequestBodyKind::Json {
            self.require("rtJSONBody", builder);
        }
        for pkg in go_method_imports(&request.spec) {
            builder.require_import(format!("\"{pkg}\""));
        }

        let struct_name = to_pascal_case(&struct_decl.name);
        let method_suffix = if request.name.is_empty() {
            "Fetch".to_owned()
        } else {
            to_pascal_case(&request.name)
        };
        let func_name = format!("New{struct_name}{method_suffix}");
        let ph_params = placeholder_params(&request.placeholders);
        let url_expr = render_full_url(&request.spec);
        let path_expr = request
            .response_path
            .as_deref()
            .map_or_else(|| "\"\"".to_owned(), go_str);
        let join_expr = request
            .response_join
            .as_deref()
            .map_or_else(|| "\"\"".to_owned(), go_str);
        let opts_expr = build_opts_inline(&request.spec);

        if let Some(ref doc) = request.doc {
            emit_go_doc(doc, writer);
        }

        wln!(
            writer,
            "func {func_name}(ctx context.Context, client *http.Client{ph_params}, opts ...rtReqOpt) (*{struct_name}, error) {{"
        );
        writer.indented(|writer| {
            let fetch_line = format!(
                "body, err := rtFetchHTML(ctx, client, {}, {}, {url_expr}, {path_expr}, {join_expr}, {opts_expr}, opts...)",
                go_str(&struct_name),
                go_str(&request.spec.method),
            );
            writer.write_block(&fetch_line);
            writer.writeln("if err != nil {");
            writer.indented(|writer| {
                writer.writeln("return nil, err");
            });
            writer.writeln("}");
            if struct_decl.kind == StructKind::Raw {
                wln!(writer, "return New{struct_name}(body), nil");
            } else {
                wln!(writer, "return New{struct_name}(body)");
            }
        });
        writer.writeln("}");
        writer.blankline();
    }

    /// Emits a typed REST endpoint receiver method on a `(rest)struct`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_import("\"net/http\"");
        if endpoint.spec.body_kind == RequestBodyKind::Json {
            self.require("rtJSONBody", builder);
        }
        for pkg in go_method_imports(&endpoint.spec) {
            builder.require_import(format!("\"{pkg}\""));
        }

        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let method_name = if endpoint.name.is_empty() {
            "Fetch".to_owned()
        } else {
            to_pascal_case(&endpoint.name)
        };
        let ph_params = placeholder_params(&endpoint.placeholders);
        let matchers_var = format!("rt{struct_name}Matchers");
        let url_expr = render_full_url(&endpoint.spec);
        let opts_expr = build_opts_inline(&endpoint.spec);
        let path_expr = endpoint
            .response_path
            .as_deref()
            .map_or_else(|| "\"\"".to_owned(), go_str);
        let method_lit = go_str(&endpoint.spec.method);

        if let Some(ref doc) = endpoint.doc {
            emit_go_doc(doc, writer);
        }

        let (ret_type, call_expr) = if let Some(ref schema_name) = endpoint.response_schema {
            let schema_go = format!("{}Json", to_pascal_case(schema_name));
            let is_array = endpoint.response_is_array
                || module.json_def(schema_name).is_some_and(|j| j.is_array);
            if is_array {
                (
                    format!("[]{schema_go}"),
                    format!(
                        "rtRestCallSlice[{schema_go}](client, {matchers_var}, {method_lit}, {url_expr}, {path_expr}, {opts_expr}, opts...)"
                    ),
                )
            } else {
                (
                    format!("*{schema_go}"),
                    format!(
                        "rtRestCallValue[{schema_go}](client, {matchers_var}, {method_lit}, {url_expr}, {path_expr}, {opts_expr}, opts...)"
                    ),
                )
            }
        } else {
            (
                "struct{}".to_owned(),
                format!(
                    "rtRestCallVoid(client, {matchers_var}, {method_lit}, {url_expr}, {opts_expr}, opts...)"
                ),
            )
        };

        wln!(
            writer,
            "func ({rcv} {struct_name}) {method_name}(client *http.Client{ph_params}, opts ...rtReqOpt) ({ret_type}, error) {{"
        );
        writer.indented(|writer| {
            let ret_line = format!("return {call_expr}");
            writer.write_block(&ret_line);
        });
        writer.writeln("}");
        writer.blankline();
    }
}

fn emit_result_variant_def(variant: &ResultVariantDef, writer: &mut CodeWriter) {
    let vname = &variant.name;
    if !variant.schema_name.is_empty() {
        let base = format!("{}Json", to_pascal_case(&variant.schema_name));
        let body_type = if variant.schema_is_array {
            format!("[]{base}")
        } else {
            base
        };
        wln!(writer, "type {vname} struct {{");
        writer.indented(|writer| {
            writer.writeln("Status int");
            wln!(writer, "Body   {body_type}");
        });
        writer.writeln("}");
        writer.blankline();
        wln!(writer, "func (e *{vname}) Error() string {{");
        writer.indented(|writer| {
            writer.writeln("return fmt.Sprintf(\"HTTP %d: %v\", e.Status, e.Body)");
        });
        writer.writeln("}");
    } else {
        wln!(writer, "type {vname} struct {{");
        writer.indented(|writer| {
            writer.writeln("Status int");
            writer.writeln("Body   string");
        });
        writer.writeln("}");
        writer.blankline();
        wln!(writer, "func (e *{vname}) Error() string {{");
        writer.indented(|writer| {
            writer.writeln("return fmt.Sprintf(\"HTTP %d: %s\", e.Status, e.Body)");
        });
        writer.writeln("}");
    }
    writer.blankline();
    wln!(writer, "func (e *{vname}) IsError() bool {{");
    writer.indented(|writer| {
        writer.writeln("return true");
    });
    writer.writeln("}");
    writer.blankline();
}

fn emit_matcher_list_def(matchers: &MatcherListDef, module: &Module, writer: &mut CodeWriter) {
    let var = format!("rt{}Matchers", to_pascal_case(&matchers.struct_name));
    if matchers.entries.is_empty() {
        wln!(writer, "var {var} = []rtErrMatcher{{}}");
        writer.blankline();
        return;
    }

    wln!(writer, "var {var} = []rtErrMatcher{{");
    writer.indented(|writer| {
        for entry in &matchers.entries {
            let mut checks: Vec<String> = entry
                .required_keys
                .iter()
                .map(|k| format!("gjson.GetBytes(_b, {}).Exists()", go_str(k)))
                .collect();

            for (path, val) in &entry.conditions {
                let go_path = go_str(path);
                let cond = match val {
                    ErrorConditionValue::Bool(b) => {
                        format!("gjson.GetBytes(_b, {go_path}).Bool() == {b}")
                    }
                    ErrorConditionValue::Int(n) => {
                        format!("gjson.GetBytes(_b, {go_path}).Int() == {n}")
                    }
                    ErrorConditionValue::Float(f) => {
                        format!("gjson.GetBytes(_b, {go_path}).Float() == {f:?}")
                    }
                    ErrorConditionValue::Null => {
                        format!("!gjson.GetBytes(_b, {go_path}).Exists()")
                    }
                    ErrorConditionValue::String(s) => {
                        format!("gjson.GetBytes(_b, {go_path}).String() == {}", go_str(s))
                    }
                };
                checks.push(cond);
            }

            let check_expr = checks.join(" && ");
            if !check_expr.is_empty() {
                wln!(writer,
                    "{{Status: {}, Check: func(_b []byte) bool {{",
                    entry.status
                );
                writer.indented(|writer| {
                    wln!(writer, "return {check_expr}");
                });
                writer.writeln("}, Factory: func(_s int, _b []byte) error {");
            } else {
                wln!(writer,
                    "{{Status: {}, Factory: func(_s int, _b []byte) error {{",
                    entry.status
                );
            }

            writer.indented(|writer| {
                if !entry.error_schema.is_empty() {
                    let base_type = format!("{}Json", to_pascal_case(&entry.error_schema));
                    let is_arr = module
                        .json_def(&entry.error_schema)
                        .is_some_and(|j| j.is_array);
                    if is_arr {
                        writer.writeln("if !gjson.ValidBytes(_b) {");
                        writer.indented(|writer| {
                            writer.writeln("return nil");
                        });
                        writer.writeln("}");
                        writer.writeln("if !gjson.ParseBytes(_b).IsArray() {");
                        writer.indented(|writer| {
                            writer.writeln("return &DecodeErr{Status: _s, Body: string(_b), Cause: \"expected JSON array\"}");
                        });
                        writer.writeln("}");
                        wln!(writer, "_body := make([]{base_type}, 0)");
                    } else {
                        writer.writeln("if !gjson.ValidBytes(_b) {");
                        writer.indented(|writer| {
                            writer.writeln("return nil");
                        });
                        writer.writeln("}");
                        wln!(writer, "var _body {base_type}");
                    }
                    writer.writeln("if _err := json.Unmarshal(_b, &_body); _err != nil {");
                    writer.indented(|writer| {
                        writer.writeln(
                            "return &DecodeErr{Status: _s, Body: string(_b), Cause: _err.Error()}",
                        );
                    });
                    writer.writeln("}");
                    wln!(writer,
                        "return &{}{{Status: _s, Body: _body}}",
                        entry.factory_name
                    );
                } else {
                    wln!(writer, "return &{}{{Status: _s}}", entry.factory_name);
                }
            });
            writer.writeln("}},");
        }
    });
    writer.writeln("}");
    writer.blankline();
}

fn go_fmt_placeholder(ph: &PlaceholderSpec) -> &'static str {
    match ph.type_name {
        PlaceholderType::Int => "%d",
        PlaceholderType::Float => "%f",
        PlaceholderType::Bool => "%t",
        PlaceholderType::Str => "%s",
    }
}

fn render_url(tmpl: &PlaceholderTemplate) -> String {
    if !tmpl.has_placeholders() {
        return go_str(&tmpl.source());
    }
    let mut fmt_str = String::new();
    let mut args = Vec::new();
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Placeholder(ph) => {
                fmt_str.push_str(go_fmt_placeholder(ph));
                args.push(to_camel_case(&ph.name));
            }
            PlaceholderPart::Literal(lit) => {
                fmt_str.push_str(&lit.replace('%', "%%"));
            }
        }
    }
    format!("fmt.Sprintf({}, {})", go_str(&fmt_str), args.join(", "))
}

fn render_json_template_value(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "nil".to_owned(),
        JsonTemplateValue::Bool(b) => b.to_string(),
        JsonTemplateValue::Number(n) => n.clone(),
        JsonTemplateValue::String(s) => go_str(s),
        JsonTemplateValue::Placeholder(ph) => to_camel_case(&ph.name),
        JsonTemplateValue::Template(tmpl) => render_url(tmpl),
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template_value).collect();
            format!("[]any{{{}}}", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", go_str(k), render_json_template_value(v)))
                .collect();
            format!("map[string]any{{{}}}", inner.join(", "))
        }
    }
}

fn render_url_values_multi(map: &IndexMap<String, Vec<PlaceholderTemplate>>) -> String {
    if map.is_empty() {
        return "\"\"".to_owned();
    }
    let has_array = map
        .values()
        .flatten()
        .any(|tmpl| tmpl.single_placeholder().is_some_and(|ph| ph.is_array));

    if has_array {
        let mut w = CodeWriter::with_indent("\t");
        w.writeln("func() string {");
        w.indented(|w| {
            w.writeln("_values := url.Values{}");
            for (key, tmpls) in map {
                for tmpl in tmpls {
                    if let Some(ph) = tmpl.single_placeholder()
                        && ph.is_array
                    {
                        let name = to_camel_case(&ph.name);
                        let effective_key = if ph.style == Some(PlaceholderStyle::Bracket) {
                            format!("{key}[]")
                        } else {
                            key.clone()
                        };
                        if let Some(sep) = ph.array_join_sep() {
                            let parts_name = format!("_{name}Parts");
                            wln!(w, "{parts_name} := make([]string, len({name}))");
                            wln!(w, "for _i, _value := range {name} {{");
                            w.indented(|w| {
                                wln!(w, "{parts_name}[_i] = fmt.Sprint(_value)");
                            });
                            w.writeln("}");
                            wln!(
                                w,
                                "_values.Set({}, strings.Join({parts_name}, {}))",
                                go_str(&effective_key),
                                go_str(sep)
                            );
                        } else {
                            wln!(w, "for _, _value := range {name} {{");
                            w.indented(|w| {
                                wln!(
                                    w,
                                    "_values.Add({}, fmt.Sprint(_value))",
                                    go_str(&effective_key)
                                );
                            });
                            w.writeln("}");
                        }
                    } else if tmpls.len() == 1 {
                        wln!(
                            w,
                            "_values.Set({}, fmt.Sprint({}))",
                            go_str(key),
                            render_url(tmpl)
                        );
                    } else {
                        wln!(
                            w,
                            "_values.Add({}, fmt.Sprint({}))",
                            go_str(key),
                            render_url(tmpl)
                        );
                    }
                }
            }
            w.writeln("return _values.Encode()");
        });
        w.writeln("}()");
        return w.finish().trim_end().to_owned();
    }

    let mut parts = Vec::with_capacity(map.len());
    for (k, tmpls) in map {
        let vals: Vec<String> = tmpls.iter().map(render_url).collect();
        parts.push(format!("{}: []string{{{}}}", go_str(k), vals.join(", ")));
    }
    format!("url.Values{{{}}}.Encode()", parts.join(", "))
}

fn render_form_values(map: &IndexMap<String, PlaceholderTemplate>) -> String {
    let mut multi = IndexMap::with_capacity(map.len());
    for (k, v) in map {
        multi.insert(k.clone(), vec![v.clone()]);
    }
    render_url_values_multi(&multi)
}

fn render_full_url(spec: &RequestSpec) -> String {
    let base = render_url(&spec.url);
    if spec.params.is_empty() {
        base
    } else {
        format!(
            "({base} + \"?\" + {})",
            render_url_values_multi(&spec.params)
        )
    }
}

fn placeholder_params(placeholders: &[PlaceholderSpec]) -> String {
    if placeholders.is_empty() {
        return String::new();
    }
    let sorted = PlaceholderSpec::sort_required_first(placeholders);
    let mut parts = Vec::with_capacity(sorted.len());
    for ph in sorted {
        let mut go_type = match ph.type_name {
            PlaceholderType::Str => "string".to_owned(),
            PlaceholderType::Int => "int64".to_owned(),
            PlaceholderType::Float => "float64".to_owned(),
            PlaceholderType::Bool => "bool".to_owned(),
        };
        if ph.is_array {
            go_type = format!("[]{go_type}");
        }
        let name = to_camel_case(&ph.name);
        parts.push(format!("{name} {go_type}"));
    }
    format!(", {}", parts.join(", "))
}

fn render_rt_headers(headers: &IndexMap<String, PlaceholderTemplate>) -> String {
    let parts: Vec<String> = headers
        .iter()
        .map(|(k, tmpl)| format!("{}: []string{{{}}}", go_str(k), render_url(tmpl)))
        .collect();
    format!("rtHeaders{{{}}}", parts.join(", "))
}

fn build_opts_inline(spec: &RequestSpec) -> String {
    let has_headers = !spec.headers.is_empty();
    let has_cookies = !spec.cookies.is_empty();
    let has_body =
        spec.body_kind != RequestBodyKind::Empty && !matches!(spec.payload, RequestPayload::Empty);

    if !has_headers && !has_cookies && !has_body {
        return "nil".to_owned();
    }

    let mut fields = Vec::new();
    if has_body {
        match (&spec.body_kind, &spec.payload) {
            (RequestBodyKind::Json, _) => {
                if let Some(ref jt) = spec.json_template {
                    fields.push(format!(
                        "Body: rtJSONBody({})",
                        render_json_template_value(jt)
                    ));
                }
            }
            (RequestBodyKind::Form, RequestPayload::Form(form_map)) => {
                fields.push(format!("Body: {}", render_form_values(form_map)));
            }
            (_, RequestPayload::Template(tmpl)) => {
                fields.push(format!("Body: {}", render_url(tmpl)));
            }
            _ => {}
        }
    }

    if has_headers {
        fields.push(format!("Headers: {}", render_rt_headers(&spec.headers)));
    }

    if has_cookies {
        fields.push(format!("Cookies: {}", render_rt_headers(&spec.cookies)));
    }

    format!("&rtReqOpts{{{}}}", fields.join(", "))
}

fn json_tree_uses_fmt(val: &JsonTemplateValue) -> bool {
    match val {
        JsonTemplateValue::Template(tmpl) => tmpl.has_placeholders(),
        JsonTemplateValue::Array(items) => items.iter().any(json_tree_uses_fmt),
        JsonTemplateValue::Object(map) => map.values().any(json_tree_uses_fmt),
        _ => false,
    }
}

fn url_values_imports(map: &IndexMap<String, Vec<PlaceholderTemplate>>) -> BTreeSet<&'static str> {
    let mut imports = BTreeSet::new();
    if map.is_empty() {
        return imports;
    }
    imports.insert("net/url");
    let has_array = map
        .values()
        .flatten()
        .any(|tmpl| tmpl.single_placeholder().is_some_and(|ph| ph.is_array));
    if has_array {
        imports.insert("fmt");
        for tmpl in map.values().flatten() {
            if let Some(ph) = tmpl.single_placeholder()
                && ph.is_array
                && ph.array_join_sep().is_some()
            {
                imports.insert("strings");
            }
        }
    } else if map
        .values()
        .flatten()
        .any(PlaceholderTemplate::has_placeholders)
    {
        imports.insert("fmt");
    }
    imports
}

fn go_method_imports(spec: &RequestSpec) -> BTreeSet<&'static str> {
    let mut imports = BTreeSet::new();
    if spec.url.has_placeholders() {
        imports.insert("fmt");
    }
    if spec
        .headers
        .values()
        .any(PlaceholderTemplate::has_placeholders)
    {
        imports.insert("fmt");
    }
    if spec
        .cookies
        .values()
        .any(PlaceholderTemplate::has_placeholders)
    {
        imports.insert("fmt");
    }
    imports.extend(url_values_imports(&spec.params));

    if spec.body_kind != RequestBodyKind::Empty {
        match &spec.payload {
            RequestPayload::Empty => {}
            RequestPayload::Form(form_map) => {
                let mut multi = IndexMap::with_capacity(form_map.len());
                for (k, v) in form_map {
                    multi.insert(k.clone(), vec![v.clone()]);
                }
                imports.extend(url_values_imports(&multi));
            }
            RequestPayload::Template(tmpl) => {
                if spec.body_kind == RequestBodyKind::Json {
                    if let Some(ref jt) = spec.json_template
                        && json_tree_uses_fmt(jt)
                    {
                        imports.insert("fmt");
                    }
                } else if tmpl.has_placeholders() {
                    imports.insert("fmt");
                }
            }
        }
    }

    imports
}

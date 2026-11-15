//! Kotlin `java.net.http.HttpClient` `@request` fetch and `(rest)struct` client code generation.

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultAliasDef,
    ResultVariantDef, StructDecl,
};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::kotlin::{
    KotlinEmitter, emit_kotlin_doc, kotlin_ident, kotlin_pascal, kotlin_str,
};
use crate::codegen::writer::{CodeWriter, wln};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::to_snake_case;
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderStyle,
    PlaceholderTemplate, PlaceholderType, RequestBodyKind, RequestPayload, RequestSpec,
};

/// Maps a [`PlaceholderSpec`] to its idiomatic Kotlin parameter type annotation.
pub fn kotlin_ph_type(ph: &PlaceholderSpec) -> String {
    let elem = match ph.type_name {
        PlaceholderType::Int => "Long",
        PlaceholderType::Float => "Double",
        PlaceholderType::Bool => "Boolean",
        PlaceholderType::Str => "String",
    };
    let base = if ph.is_array {
        format!("List<{elem}>")
    } else {
        elem.to_owned()
    };
    if ph.is_optional {
        format!("{base}?")
    } else {
        base
    }
}

/// Renders a [`PlaceholderTemplate`] as a Kotlin string expression.
pub fn render_template(tmpl: &PlaceholderTemplate) -> String {
    if !tmpl.has_placeholders() {
        return kotlin_str(&tmpl.source());
    }

    let mut parts: Vec<String> = Vec::with_capacity(tmpl.parts.len());
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Literal(lit) => {
                if !lit.is_empty() {
                    parts.push(kotlin_str(lit));
                }
            }
            PlaceholderPart::Placeholder(ph) => {
                let ident = kotlin_ident(&ph.name);
                if ph.is_optional {
                    if ph.type_name == PlaceholderType::Str && !ph.is_array {
                        parts.push(format!("({ident} ?: \"\")"));
                    } else {
                        parts.push(format!("({ident}?.toString() ?: \"\")"));
                    }
                } else if ph.type_name == PlaceholderType::Str && !ph.is_array {
                    parts.push(ident);
                } else {
                    parts.push(format!("{ident}.toString()"));
                }
            }
        }
    }
    if parts.is_empty() {
        "\"\"".to_owned()
    } else {
        parts.join(" + ")
    }
}

fn render_json_template_value(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "JsonNull".to_owned(),
        JsonTemplateValue::Bool(b) => format!("JsonPrimitive({b})"),
        JsonTemplateValue::Number(n) => {
            if n.contains('.') || n.contains('e') || n.contains('E') {
                format!("JsonPrimitive({n})")
            } else {
                format!("JsonPrimitive({n}L)")
            }
        }
        JsonTemplateValue::String(s) => format!("JsonPrimitive({})", kotlin_str(s)),
        JsonTemplateValue::Placeholder(ph) => {
            format!("Rt.toJsonElement({})", kotlin_ident(&ph.name))
        }
        JsonTemplateValue::Template(tmpl) => format!("JsonPrimitive({})", render_template(tmpl)),
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template_value).collect();
            format!("JsonArray(listOf({}))", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} to {}", kotlin_str(k), render_json_template_value(v)))
                .collect();
            format!("JsonObject(mapOf({}))", inner.join(", "))
        }
    }
}

/// Renders a JSON request body as a `kotlinx.serialization.json.JsonElement` construction expression.
pub fn render_json_body(spec: &RequestSpec) -> Option<String> {
    spec.json_template.as_ref().map(render_json_template_value)
}

fn emit_param_pair(key: &str, tmpl: &PlaceholderTemplate, writer: &mut CodeWriter) {
    if let Some(ph) = tmpl.single_placeholder() {
        let var = kotlin_ident(&ph.name);
        let effective_key = if ph.is_array && ph.style == Some(PlaceholderStyle::Bracket) {
            format!("{key}[]")
        } else {
            key.to_owned()
        };
        let key_lit = kotlin_str(&effective_key);

        if ph.is_array {
            if let Some(sep) = ph.array_join_sep() {
                let sep_lit = kotlin_str(sep);
                if ph.is_optional {
                    wln!(writer, "if ({var} != null) {{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "reqOpts.addParam({key_lit}, {var}.joinToString({sep_lit}) {{ it.toString() }})"
                        );
                    });
                    writer.writeln("}");
                } else {
                    wln!(
                        writer,
                        "reqOpts.addParam({key_lit}, {var}.joinToString({sep_lit}) {{ it.toString() }})"
                    );
                }
            } else if ph.is_optional {
                wln!(writer, "if ({var} != null) {{");
                writer.indented(|writer| {
                    wln!(writer, "for (item in {var}) {{");
                    writer.indented(|writer| {
                        wln!(writer, "reqOpts.addParam({key_lit}, item.toString())");
                    });
                    writer.writeln("}");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "for (item in {var}) {{");
                writer.indented(|writer| {
                    wln!(writer, "reqOpts.addParam({key_lit}, item.toString())");
                });
                writer.writeln("}");
            }
        } else if ph.is_optional {
            wln!(writer, "if ({var} != null) {{");
            writer.indented(|writer| {
                wln!(writer, "reqOpts.addParam({key_lit}, {var}.toString())");
            });
            writer.writeln("}");
        } else {
            wln!(writer, "reqOpts.addParam({key_lit}, {var}.toString())");
        }
    } else if tmpl.has_placeholders() {
        let val_expr = render_template(tmpl);
        wln!(writer, "reqOpts.addParam({}, {val_expr})", kotlin_str(key));
    } else {
        wln!(
            writer,
            "reqOpts.addParam({}, {})",
            kotlin_str(key),
            kotlin_str(&tmpl.source())
        );
    }
}

fn emit_form_pair(list_var: &str, key: &str, tmpl: &PlaceholderTemplate, writer: &mut CodeWriter) {
    if let Some(ph) = tmpl.single_placeholder() {
        let var = kotlin_ident(&ph.name);
        let effective_key = if ph.is_array && ph.style == Some(PlaceholderStyle::Bracket) {
            format!("{key}[]")
        } else {
            key.to_owned()
        };
        let key_lit = kotlin_str(&effective_key);

        if ph.is_array {
            if let Some(sep) = ph.array_join_sep() {
                let sep_lit = kotlin_str(sep);
                if ph.is_optional {
                    wln!(writer, "if ({var} != null) {{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "{list_var}.add({key_lit} to {var}.joinToString({sep_lit}) {{ it.toString() }})"
                        );
                    });
                    writer.writeln("}");
                } else {
                    wln!(
                        writer,
                        "{list_var}.add({key_lit} to {var}.joinToString({sep_lit}) {{ it.toString() }})"
                    );
                }
            } else if ph.is_optional {
                wln!(writer, "if ({var} != null) {{");
                writer.indented(|writer| {
                    wln!(writer, "for (item in {var}) {{");
                    writer.indented(|writer| {
                        wln!(writer, "{list_var}.add({key_lit} to item.toString())");
                    });
                    writer.writeln("}");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "for (item in {var}) {{");
                writer.indented(|writer| {
                    wln!(writer, "{list_var}.add({key_lit} to item.toString())");
                });
                writer.writeln("}");
            }
        } else if ph.is_optional {
            wln!(writer, "if ({var} != null) {{");
            writer.indented(|writer| {
                wln!(writer, "{list_var}.add({key_lit} to {var}.toString())");
            });
            writer.writeln("}");
        } else {
            wln!(writer, "{list_var}.add({key_lit} to {var}.toString())");
        }
    } else if tmpl.has_placeholders() {
        let val_expr = render_template(tmpl);
        wln!(
            writer,
            "{list_var}.add({} to ({val_expr}))",
            kotlin_str(key)
        );
    } else {
        wln!(
            writer,
            "{list_var}.add({} to {})",
            kotlin_str(key),
            kotlin_str(&tmpl.source())
        );
    }
}

fn emit_request_builder(spec: &RequestSpec, writer: &mut CodeWriter) {
    writer.writeln("val reqOpts = Rt.ReqOpts()");

    for (key, tmpls) in &spec.params {
        for tmpl in tmpls {
            emit_param_pair(key, tmpl, writer);
        }
    }

    for (k, tmpl) in &spec.headers {
        let k_lit = kotlin_str(k);
        if let Some(header_ph) = tmpl.single_placeholder() {
            let var = kotlin_ident(&header_ph.name);
            if header_ph.is_optional {
                wln!(writer, "if ({var} != null) {{");
                writer.indented(|writer| {
                    wln!(writer, "reqOpts.addHeader({k_lit}, {var}.toString())");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "reqOpts.addHeader({k_lit}, {var}.toString())");
            }
        } else if tmpl.has_placeholders() {
            wln!(
                writer,
                "reqOpts.addHeader({k_lit}, {})",
                render_template(tmpl)
            );
        } else {
            wln!(
                writer,
                "reqOpts.addHeader({k_lit}, {})",
                kotlin_str(&tmpl.source())
            );
        }
    }

    for (k, tmpl) in &spec.cookies {
        let k_lit = kotlin_str(k);
        if let Some(cookie_ph) = tmpl.single_placeholder() {
            let var = kotlin_ident(&cookie_ph.name);
            if cookie_ph.is_optional {
                wln!(writer, "if ({var} != null) {{");
                writer.indented(|writer| {
                    wln!(writer, "reqOpts.addCookie({k_lit}, {var}.toString())");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "reqOpts.addCookie({k_lit}, {var}.toString())");
            }
        } else if tmpl.has_placeholders() {
            wln!(
                writer,
                "reqOpts.addCookie({k_lit}, {})",
                render_template(tmpl)
            );
        } else {
            wln!(
                writer,
                "reqOpts.addCookie({k_lit}, {})",
                kotlin_str(&tmpl.source())
            );
        }
    }

    match spec.body_kind {
        RequestBodyKind::Empty => {}
        RequestBodyKind::Json => {
            if let Some(json_expr) = render_json_body(spec) {
                writer.writeln("reqOpts.ensureHeader(\"Content-Type\", \"application/json\")");
                wln!(writer, "reqOpts.body = {json_expr}.toString()");
            }
        }
        RequestBodyKind::Form => {
            if let RequestPayload::Form(ref form_map) = spec.payload {
                writer.writeln("val formPairs = ArrayList<Pair<String, String>>()");
                for (k, tmpl) in form_map {
                    emit_form_pair("formPairs", k, tmpl, writer);
                }
                writer.writeln(
                    "reqOpts.ensureHeader(\"Content-Type\", \"application/x-www-form-urlencoded\")",
                );
                writer.writeln("reqOpts.body = Rt.encodeForm(formPairs)");
            }
        }
        RequestBodyKind::Raw => {
            if let RequestPayload::Template(ref tmpl) = spec.payload {
                wln!(writer, "reqOpts.body = {}", render_template(tmpl));
            }
        }
    }
}

fn placeholder_params(placeholders: &[PlaceholderSpec]) -> Vec<String> {
    let ordered = PlaceholderSpec::sort_required_first(placeholders);
    let mut params = Vec::with_capacity(ordered.len() + 2);
    params.push("client: HttpClient = Rt.defaultHttpClient".to_owned());
    for ph in ordered {
        let name = kotlin_ident(&ph.name);
        let ty = kotlin_ph_type(ph);
        if ph.is_optional {
            params.push(format!("{name}: {ty} = null"));
        } else {
            params.push(format!("{name}: {ty}"));
        }
    }
    params.push("vararg opts: Rt.HttpOption".to_owned());
    params
}

fn placeholder_call_args(placeholders: &[PlaceholderSpec]) -> Vec<String> {
    let ordered = PlaceholderSpec::sort_required_first(placeholders);
    let mut args = Vec::with_capacity(ordered.len() + 2);
    args.push("client".to_owned());
    for ph in ordered {
        args.push(kotlin_ident(&ph.name));
    }
    args.push("*opts".to_owned());
    args
}

/// Derives an idiomatic Kotlin nested `data class` variant name from a synthesized error factory name.
pub fn kotlin_variant_name(struct_name: &str, factory_name: &str) -> String {
    let pascal = kotlin_pascal(struct_name);
    if let Some(tail) = factory_name.strip_prefix(&pascal)
        && !tail.is_empty()
    {
        return tail.to_owned();
    }
    factory_name.to_owned()
}

fn render_error_cond(path: &str, val: &ErrorConditionValue) -> String {
    let path_lit = kotlin_str(path);
    match val {
        ErrorConditionValue::Null => format!("Rt.ErrorCond.Null({path_lit})"),
        ErrorConditionValue::Bool(b) => format!("Rt.ErrorCond.Bool({path_lit}, {b})"),
        ErrorConditionValue::Int(n) => format!("Rt.ErrorCond.IntVal({path_lit}, {n}L)"),
        ErrorConditionValue::Float(f) => format!("Rt.ErrorCond.FloatVal({path_lit}, {f:?})"),
        ErrorConditionValue::String(s) => {
            format!("Rt.ErrorCond.Str({path_lit}, {})", kotlin_str(s))
        }
    }
}

impl KotlinEmitter {
    /// Emits a `fetch` method on the `companion object` of an HTML or Raw parser class.
    pub(crate) fn emit_request_impl(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let struct_name = kotlin_pascal(&struct_decl.name);
        let (camel_name, snake_name) = if request.name.is_empty() || request.name == "fetch" {
            ("fetch".to_owned(), "fetch".to_owned())
        } else {
            let snake = to_snake_case(&request.name);
            let clean = snake.strip_prefix("fetch_").unwrap_or(&snake);
            (
                format!("fetch{}", to_pascal_case(clean)),
                format!("fetch_{clean}"),
            )
        };
        let method_ident = if camel_name == "fetch" {
            "fetch".to_owned()
        } else {
            camel_name.clone()
        };
        let context = format!("{struct_name}.{snake_name}");
        let params = placeholder_params(&request.placeholders);
        let method_lit = kotlin_str(&request.spec.method);
        let url_expr = render_template(&request.spec.url);
        let path_opt = request
            .response_path
            .as_deref()
            .map(kotlin_str)
            .unwrap_or_else(|| "null".to_owned());
        let join_opt = request
            .response_join
            .as_deref()
            .map(kotlin_str)
            .unwrap_or_else(|| "null".to_owned());

        if let Some(ref doc) = request.doc {
            emit_kotlin_doc(doc, "//", writer);
        }
        wln!(
            writer,
            "fun {method_ident}({}): {struct_name} {{",
            params.join(", ")
        );
        writer.indented(|writer| {
            emit_request_builder(&request.spec, writer);
            wln!(writer,
                "val body = Rt.fetchHtml(client, {method_lit}, {url_expr}, reqOpts, opts, {}, {path_opt}, {join_opt})",
                kotlin_str(&context)
            );
            wln!(writer, "return {struct_name}(body)");
        });
        writer.writeln("}");
        writer.blankline();

        if snake_name != camel_name {
            let call_args = placeholder_call_args(&request.placeholders);
            wln!(
                writer,
                "fun {snake_name}({}): {struct_name} = {method_ident}({})",
                params.join(", "),
                call_args.join(", ")
            );
            writer.blankline();
        }
    }

    /// Emits a synthesized [`RestArtifact`] (`ErrorVariant`, `ResultAlias`, or `MatcherList`).
    pub(crate) fn emit_rest_artifact_impl(
        &mut self,
        artifact: &RestArtifact,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let RestArtifact::ResultAlias(alias) = artifact {
            self.emit_result_alias_def(alias, module, writer);
        }
    }

    fn emit_result_alias_def(
        &mut self,
        alias: &ResultAliasDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let struct_name = &alias.struct_name;
        let struct_pascal = kotlin_pascal(struct_name);
        let effective_name =
            module.resolve_effective_result_alias_name(&alias.struct_name, &alias.name);
        let qualified = format!("{struct_pascal}{}", alias.name);

        if !self.emitted_aliases.insert(effective_name.clone()) {
            if qualified != effective_name && self.emitted_aliases.insert(qualified.clone()) {
                wln!(writer, "typealias {qualified} = {effective_name}");
                writer.blankline();
            }
            return;
        }

        let (ok_type, is_unit) = if let Some(ref schema_name) = alias.response_schema {
            let s_pascal = format!("{}Json", kotlin_pascal(schema_name));
            let is_arr =
                alias.response_is_array || module.json_def(schema_name).is_some_and(|j| j.is_array);
            if is_arr {
                (format!("List<{s_pascal}>"), false)
            } else {
                (s_pascal, false)
            }
        } else if alias.has_response_path(module) {
            ("JsonElement".to_owned(), false)
        } else {
            ("Unit".to_owned(), true)
        };

        let variants_by_name: IndexMap<&str, &ResultVariantDef> = module
            .rest_artifacts
            .iter()
            .filter_map(|a| match a {
                RestArtifact::ErrorVariant(v) => Some((v.name.as_str(), v)),
                _ => None,
            })
            .collect();

        let matchers_def: Option<&MatcherListDef> =
            module.rest_artifacts.iter().find_map(|a| match a {
                RestArtifact::MatcherList(m) if m.struct_name == *struct_name => Some(m),
                _ => None,
            });

        // Map: short_v_name -> (full_factory_name, body_type, status, schema_name, is_array)
        let mut variant_info: IndexMap<String, (String, String, u16, String, bool)> =
            IndexMap::new();
        if let Some(m_def) = matchers_def {
            for entry in &m_def.entries {
                let short_name = kotlin_variant_name(struct_name, &entry.factory_name);
                if variant_info.contains_key(&short_name) {
                    continue;
                }
                let var_def = variants_by_name.get(entry.factory_name.as_str()).copied();
                if !entry.error_schema.is_empty() {
                    let s_pascal = format!("{}Json", kotlin_pascal(&entry.error_schema));
                    let is_array = var_def.is_some_and(|v| v.schema_is_array)
                        || module
                            .json_def(&entry.error_schema)
                            .is_some_and(|j| j.is_array);
                    let typ = if is_array {
                        format!("List<{s_pascal}>")
                    } else {
                        s_pascal
                    };
                    variant_info.insert(
                        short_name,
                        (
                            entry.factory_name.clone(),
                            typ,
                            entry.status,
                            entry.error_schema.clone(),
                            is_array,
                        ),
                    );
                } else {
                    variant_info.insert(
                        short_name,
                        (
                            entry.factory_name.clone(),
                            "JsonElement".to_owned(),
                            entry.status,
                            String::new(),
                            false,
                        ),
                    );
                }
            }
        }

        wln!(writer, "sealed interface {effective_name} {{");
        writer.indented(|writer| {
            if is_unit {
                wln!(writer,
                    "data class Ok(val value: {ok_type} = Unit) : {effective_name}"
                );
            } else {
                wln!(writer,
                    "data class Ok(val value: {ok_type}) : {effective_name}"
                );
            }
            wln!(writer,
                "data class DecodeErr(val status: Int, val body: String, val cause: String) : {effective_name}"
            );
            wln!(writer,
                "data class TransportErr(val cause: Throwable) : {effective_name}"
            );
            wln!(writer,
                "data class UnknownErr(val status: Int, val body: JsonElement?) : {effective_name}"
            );
            for (short_name, (_, body_type, _, _, _)) in &variant_info {
                wln!(writer,
                    "data class {short_name}(val status: Int, val body: {body_type}) : {effective_name}"
                );
            }
        });
        writer.writeln("}");
        writer.blankline();

        if qualified != effective_name && self.emitted_aliases.insert(qualified.clone()) {
            wln!(writer, "typealias {qualified} = {effective_name}");
        }
        for (short_name, (full_name, _, _, _, _)) in &variant_info {
            if full_name != short_name && self.emitted_aliases.insert(full_name.clone()) {
                wln!(
                    writer,
                    "typealias {full_name} = {effective_name}.{short_name}"
                );
            }
        }

        let effective_upper = to_snake_case(&effective_name).to_ascii_uppercase();
        let matchers_const = format!("RT_{effective_upper}_MATCHERS");
        let entries = matchers_def.map(|m| m.entries.as_slice()).unwrap_or(&[]);

        if entries.is_empty() {
            wln!(
                writer,
                "private val {matchers_const}: List<Rt.ErrorMatcher<{effective_name}>> = emptyList()"
            );
            writer.blankline();
            return;
        }

        wln!(
            writer,
            "private val {matchers_const}: List<Rt.ErrorMatcher<{effective_name}>> = listOf("
        );
        writer.indented(|writer| {
            for entry in entries {
                let short_name = kotlin_variant_name(struct_name, &entry.factory_name);
                let req_keys: Vec<String> =
                    entry.required_keys.iter().map(|k| kotlin_str(k)).collect();
                let conds: Vec<String> = entry
                    .conditions
                    .iter()
                    .map(|(p, v)| render_error_cond(p, v))
                    .collect();
                let decode_fn = if !entry.error_schema.is_empty() {
                    let s_pascal = kotlin_pascal(&entry.error_schema);
                    let (_, _, _, _, is_array) = &variant_info[&short_name];
                    let def_is_arr = module
                        .json_def(&entry.error_schema)
                        .is_some_and(|j| j.is_array);
                    let proj_expr = if *is_array && !def_is_arr {
                        let ctx_lit = kotlin_str(&format!("{effective_name}.{short_name}"));
                        format!("Rt.expectArray(v, {ctx_lit}).map {{ Rt.project{s_pascal}(it) }}")
                    } else {
                        format!("Rt.project{s_pascal}(v)")
                    };
                    format!("{{ status, v -> {effective_name}.{short_name}(status, {proj_expr}) }}")
                } else {
                    format!("{{ status, v -> {effective_name}.{short_name}(status, v) }}")
                };

                writer.writeln("Rt.ErrorMatcher(");
                writer.indented(|writer| {
                    wln!(writer, "status = {},", entry.status);
                    wln!(writer, "requiredKeys = listOf({}),", req_keys.join(", "));
                    wln!(writer, "conditions = listOf({}),", conds.join(", "));
                    wln!(writer, "decode = {decode_fn},");
                });
                writer.writeln("),");
            }
        });
        writer.writeln(")");
        writer.blankline();
    }

    /// Emits a REST endpoint method on a `(rest)struct` `companion object`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let struct_name = &struct_decl.name;
        let struct_pascal = kotlin_pascal(struct_name);
        let raw_name = if endpoint.name.is_empty() {
            "fetch".to_owned()
        } else {
            endpoint.name.clone()
        };
        let camel_ident = kotlin_ident(&raw_name);
        let snake_name = to_snake_case(&raw_name);
        let context = format!("{struct_pascal}.{snake_name}");
        let params = placeholder_params(&endpoint.placeholders);

        let effective_result = module
            .resolve_effective_result_alias_name(&struct_decl.name, &endpoint.result_alias_name);
        let effective_upper = to_snake_case(&effective_result).to_ascii_uppercase();
        let matchers_const = format!("RT_{effective_upper}_MATCHERS");
        let method_lit = kotlin_str(&endpoint.spec.method);
        let url_expr = render_template(&endpoint.spec.url);

        if let Some(ref doc) = endpoint.doc {
            emit_kotlin_doc(doc, "//", writer);
        }
        wln!(
            writer,
            "fun {camel_ident}({}): {effective_result} {{",
            params.join(", ")
        );
        writer.indented(|writer| {
            emit_request_builder(&endpoint.spec, writer);
            if let Some(ref schema_name) = endpoint.response_schema {
                let s_pascal = kotlin_pascal(schema_name);
                let is_array = endpoint.response_is_array
                    || module.json_def(schema_name).is_some_and(|j| j.is_array);
                let def_is_arr = module.json_def(schema_name).is_some_and(|j| j.is_array);
                let decode_ok = if is_array && !def_is_arr {
                    let ctx_lit = kotlin_str(&context);
                    format!(
                        "{{ v -> {effective_result}.Ok(Rt.expectArray(v, {ctx_lit}).map {{ Rt.project{s_pascal}(it) }}) }}"
                    )
                } else {
                    format!("{{ v -> {effective_result}.Ok(Rt.project{s_pascal}(v)) }}")
                };
                let path_opt = endpoint
                    .response_path
                    .as_deref()
                    .map(kotlin_str)
                    .unwrap_or_else(|| "null".to_owned());
                wln!(writer,
                    "return Rt.restCall(client, {method_lit}, {url_expr}, reqOpts, opts, {path_opt}, {}, {matchers_const}, {decode_ok}, {effective_result}::TransportErr, {effective_result}::DecodeErr, {effective_result}::UnknownErr)",
                    kotlin_str(&context)
                );
            } else if let Some(ref rp) = endpoint.response_path {
                let path_lit = kotlin_str(rp);
                wln!(writer,
                    "return Rt.restCall(client, {method_lit}, {url_expr}, reqOpts, opts, {path_lit}, {}, {matchers_const}, {{ v -> {effective_result}.Ok(v) }}, {effective_result}::TransportErr, {effective_result}::DecodeErr, {effective_result}::UnknownErr)",
                    kotlin_str(&context)
                );
            } else {
                wln!(writer,
                    "return Rt.restCallVoid(client, {method_lit}, {url_expr}, reqOpts, opts, {matchers_const}, {{ {effective_result}.Ok(Unit) }}, {effective_result}::TransportErr, {effective_result}::DecodeErr, {effective_result}::UnknownErr)"
                );
            }
        });
        writer.writeln("}");
        writer.blankline();

        let snake_ident = if crate::linter::symbols::KOTLIN_RESERVED.contains(&snake_name.as_str())
        {
            format!("`{snake_name}`")
        } else {
            snake_name
        };
        if snake_ident != camel_ident {
            let call_args = placeholder_call_args(&endpoint.placeholders);
            wln!(
                writer,
                "fun {snake_ident}({}): {effective_result} = {camel_ident}({})",
                params.join(", "),
                call_args.join(", ")
            );
            writer.blankline();
        }
    }
}

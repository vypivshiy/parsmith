//! C# (.NET 8+) `HttpClient` `@request` fetch and `(rest)struct` client code generation.

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultAliasDef,
    ResultVariantDef, StructDecl,
};
use crate::codegen::HttpIo;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::csharp::{
    CSharpEmitter, csharp_ident, csharp_pascal, csharp_str, emit_csharp_doc,
};
use crate::codegen::writer::{CodeWriter, wln};
use crate::linter::symbols::to_snake_case;
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderStyle,
    PlaceholderTemplate, PlaceholderType, RequestBodyKind, RequestPayload, RequestSpec,
};

/// Maps a [`PlaceholderSpec`] to its idiomatic C# parameter type annotation.
pub fn csharp_ph_type(ph: &PlaceholderSpec) -> String {
    let elem = match ph.type_name {
        PlaceholderType::Int => "long",
        PlaceholderType::Float => "double",
        PlaceholderType::Bool => "bool",
        PlaceholderType::Str => "string",
    };
    if ph.is_array {
        let base = format!("IEnumerable<{elem}>");
        if ph.is_optional {
            format!("{base}?")
        } else {
            base
        }
    } else if ph.is_optional {
        format!("{elem}?")
    } else {
        elem.to_owned()
    }
}

fn format_scalar_ph_expr(var_expr: &str, ty: PlaceholderType, is_nullable_val: bool) -> String {
    let val_access = if is_nullable_val
        && matches!(
            ty,
            PlaceholderType::Int | PlaceholderType::Float | PlaceholderType::Bool
        ) {
        format!("{var_expr}.Value")
    } else {
        var_expr.to_owned()
    };
    match ty {
        PlaceholderType::Bool => format!("({val_access} ? \"true\" : \"false\")"),
        PlaceholderType::Int | PlaceholderType::Float => {
            format!("{val_access}.ToString(CultureInfo.InvariantCulture)")
        }
        PlaceholderType::Str => val_access,
    }
}

/// Renders a [`PlaceholderTemplate`] as a C# string literal or `string.Concat(...)` expression.
pub fn render_template(tmpl: &PlaceholderTemplate) -> String {
    if !tmpl.has_placeholders() {
        return csharp_str(&tmpl.source());
    }

    let mut parts_exprs: Vec<String> = Vec::new();
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Placeholder(ph) => {
                let ident = csharp_ident(&to_snake_case(&ph.name));
                if ph.is_optional {
                    if ph.type_name == PlaceholderType::Str && !ph.is_array {
                        parts_exprs.push(format!("({ident} ?? string.Empty)"));
                    } else {
                        let formatted = format_scalar_ph_expr(&ident, ph.type_name, true);
                        parts_exprs
                            .push(format!("({ident}.HasValue ? {formatted} : string.Empty)"));
                    }
                } else {
                    parts_exprs.push(format_scalar_ph_expr(&ident, ph.type_name, false));
                }
            }
            PlaceholderPart::Literal(lit) => {
                if !lit.is_empty() {
                    parts_exprs.push(csharp_str(lit));
                }
            }
        }
    }

    if parts_exprs.is_empty() {
        "string.Empty".to_owned()
    } else if parts_exprs.len() == 1 {
        parts_exprs.remove(0)
    } else {
        format!("string.Concat({})", parts_exprs.join(", "))
    }
}

fn render_json_template_value(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "((JsonNode?)null)".to_owned(),
        JsonTemplateValue::Bool(b) => format!("JsonValue.Create({b})"),
        JsonTemplateValue::Number(n) => format!("JsonNode.Parse({})", csharp_str(n)),
        JsonTemplateValue::String(s) => format!("JsonValue.Create({})", csharp_str(s)),
        JsonTemplateValue::Placeholder(ph) => {
            let ident = csharp_ident(&to_snake_case(&ph.name));
            if ph.is_array {
                format!("Rt.ToJsonArray({ident})")
            } else {
                format!("JsonValue.Create({ident})")
            }
        }
        JsonTemplateValue::Template(tmpl) => {
            format!("JsonValue.Create({})", render_template(tmpl))
        }
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template_value).collect();
            format!("new JsonArray({})", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            if map.is_empty() {
                "new JsonObject()".to_owned()
            } else {
                let inner: Vec<String> = map
                    .iter()
                    .map(|(k, v)| {
                        format!("[{}] = {}", csharp_str(k), render_json_template_value(v))
                    })
                    .collect();
                format!("new JsonObject {{ {} }}", inner.join(", "))
            }
        }
    }
}

/// Renders a JSON request body template into a C# `JsonNode.ToJsonString()` expression.
pub fn render_json_body(spec: &RequestSpec) -> Option<String> {
    spec.json_template
        .as_ref()
        .map(|jt| format!("{}.ToJsonString()", render_json_template_value(jt)))
}

fn emit_kv_pair(list_var: &str, key: &str, tmpl: &PlaceholderTemplate, writer: &mut CodeWriter) {
    if let Some(ph) = tmpl.single_placeholder() {
        let var = csharp_ident(&to_snake_case(&ph.name));
        let effective_key = if ph.is_array && ph.style == Some(PlaceholderStyle::Bracket) {
            format!("{key}[]")
        } else {
            key.to_owned()
        };
        let key_lit = csharp_str(&effective_key);

        if ph.is_array {
            let item_fmt = format_scalar_ph_expr("_item", ph.type_name, false);
            if let Some(sep) = ph.array_join_sep() {
                let sep_lit = csharp_str(sep);
                if ph.is_optional {
                    wln!(writer, "if ({var} is not null)");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "{list_var}.Add(new KeyValuePair<string, string>({key_lit}, string.Join({sep_lit}, {var}.Select(_item => {item_fmt}))));"
                        );
                    });
                    writer.writeln("}");
                } else {
                    wln!(
                        writer,
                        "{list_var}.Add(new KeyValuePair<string, string>({key_lit}, string.Join({sep_lit}, {var}.Select(_item => {item_fmt}))));"
                    );
                }
            } else if ph.is_optional {
                wln!(writer, "if ({var} is not null)");
                writer.writeln("{");
                writer.indented(|writer| {
                    wln!(writer, "foreach (var _item in {var})");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "{list_var}.Add(new KeyValuePair<string, string>({key_lit}, {item_fmt}));"
                        );
                    });
                    writer.writeln("}");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "foreach (var _item in {var})");
                writer.writeln("{");
                writer.indented(|writer| {
                    wln!(
                        writer,
                        "{list_var}.Add(new KeyValuePair<string, string>({key_lit}, {item_fmt}));"
                    );
                });
                writer.writeln("}");
            }
        } else if ph.is_optional {
            let is_val_ty = matches!(
                ph.type_name,
                PlaceholderType::Int | PlaceholderType::Float | PlaceholderType::Bool
            );
            let cond = if is_val_ty {
                format!("{var}.HasValue")
            } else {
                format!("{var} is not null")
            };
            let val_expr = format_scalar_ph_expr(&var, ph.type_name, is_val_ty);
            wln!(writer, "if ({cond})");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(
                    writer,
                    "{list_var}.Add(new KeyValuePair<string, string>({key_lit}, {val_expr}));"
                );
            });
            writer.writeln("}");
        } else {
            let val_expr = format_scalar_ph_expr(&var, ph.type_name, false);
            wln!(
                writer,
                "{list_var}.Add(new KeyValuePair<string, string>({key_lit}, {val_expr}));"
            );
        }
    } else if tmpl.has_placeholders() {
        let val_expr = render_template(tmpl);
        wln!(
            writer,
            "{list_var}.Add(new KeyValuePair<string, string>({}, {val_expr}));",
            csharp_str(key)
        );
    } else {
        wln!(
            writer,
            "{list_var}.Add(new KeyValuePair<string, string>({}, {}));",
            csharp_str(key),
            csharp_str(&tmpl.source())
        );
    }
}

fn emit_request_setup(spec: &RequestSpec, writer: &mut CodeWriter) -> (String, String, String) {
    let url_expr = render_template(&spec.url);
    wln!(writer, "var _baseUrl = {url_expr};");
    writer.writeln("var _queryPairs = new List<KeyValuePair<string, string>>();");
    for (key, tmpls) in &spec.params {
        for tmpl in tmpls {
            emit_kv_pair("_queryPairs", key, tmpl, writer);
        }
    }

    writer.writeln("var _headers = new List<KeyValuePair<string, string>>();");
    for (k, tmpl) in &spec.headers {
        emit_kv_pair("_headers", k, tmpl, writer);
    }

    writer.writeln("var _cookies = new List<KeyValuePair<string, string>>();");
    for (k, tmpl) in &spec.cookies {
        emit_kv_pair("_cookies", k, tmpl, writer);
    }

    let mut body_arg = "null".to_owned();
    let mut form_arg = "null".to_owned();
    match spec.body_kind {
        RequestBodyKind::Empty => {}
        RequestBodyKind::Json => {
            if let Some(json_expr) = render_json_body(spec) {
                wln!(writer, "string? _body = {json_expr};");
                body_arg = "_body".to_owned();
            }
        }
        RequestBodyKind::Form => {
            if let RequestPayload::Form(ref form_map) = spec.payload {
                writer.writeln("var _formPairs = new List<KeyValuePair<string, string>>();");
                for (k, tmpl) in form_map {
                    emit_kv_pair("_formPairs", k, tmpl, writer);
                }
                form_arg = "_formPairs".to_owned();
            }
        }
        RequestBodyKind::Raw => {
            if let RequestPayload::Template(ref tmpl) = spec.payload {
                wln!(writer, "string? _body = {};", render_template(tmpl));
                body_arg = "_body".to_owned();
            }
        }
    }

    ("_baseUrl".to_owned(), body_arg, form_arg)
}

fn placeholder_params(placeholders: &[PlaceholderSpec]) -> (Vec<String>, Vec<String>) {
    let ordered = PlaceholderSpec::sort_required_first(placeholders);
    let mut decls = Vec::with_capacity(ordered.len() + 2);
    let mut args = Vec::with_capacity(ordered.len() + 2);
    decls.push("HttpClient client".to_owned());
    args.push("client".to_owned());

    for ph in ordered {
        let name = csharp_ident(&to_snake_case(&ph.name));
        let ty = csharp_ph_type(ph);
        if ph.is_optional {
            decls.push(format!("{ty} {name} = null"));
        } else {
            decls.push(format!("{ty} {name}"));
        }
        args.push(name);
    }

    decls.push("params Rt.HttpOption[] options".to_owned());
    args.push("options".to_owned());
    (decls, args)
}

/// Derives an idiomatic C# record variant name from a synthesized error factory name.
pub fn csharp_variant_name(struct_name: &str, factory_name: &str) -> String {
    let pascal = crate::json_to_kdl::to_pascal_case(struct_name).replace('-', "");
    if let Some(tail) = factory_name.strip_prefix(&pascal)
        && !tail.is_empty()
    {
        return tail.to_owned();
    }
    factory_name.to_owned()
}

fn render_error_cond(path: &str, val: &ErrorConditionValue) -> String {
    let path_lit = csharp_str(path);
    match val {
        ErrorConditionValue::Null => format!("Rt.ErrorCond.Null({path_lit})"),
        ErrorConditionValue::Bool(b) => format!("Rt.ErrorCond.Bool({path_lit}, {b})"),
        ErrorConditionValue::Int(n) => format!("Rt.ErrorCond.Int({path_lit}, {n}L)"),
        ErrorConditionValue::Float(f) => format!("Rt.ErrorCond.Float({path_lit}, {f:?})"),
        ErrorConditionValue::String(s) => {
            format!("Rt.ErrorCond.Str({path_lit}, {})", csharp_str(s))
        }
    }
}

impl CSharpEmitter {
    /// Emits `FetchAsync` and `Fetch` static methods on an HTML or `raw` parser class.
    pub(crate) fn emit_request_impl(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let struct_name = csharp_pascal(&struct_decl.name);
        let raw_pascal = if request.name.is_empty() || request.name == "fetch" {
            "Fetch".to_owned()
        } else {
            let snake = to_snake_case(&request.name);
            let clean = snake.strip_prefix("fetch_").unwrap_or(&snake);
            format!(
                "Fetch{}",
                crate::json_to_kdl::to_pascal_case(clean).replace('-', "")
            )
        };
        let async_name = format!("{raw_pascal}Async");
        let sync_name = raw_pascal.clone();
        let context = format!("{struct_name}.{raw_pascal}");
        let (decls, forward_args) = placeholder_params(&request.placeholders);
        let decls_str = decls.join(", ");
        let forward_str = forward_args.join(", ");

        let path_arg = request
            .response_path
            .as_deref()
            .map(csharp_str)
            .unwrap_or_else(|| "null".to_owned());
        let join_arg = request
            .response_join
            .as_deref()
            .map(csharp_str)
            .unwrap_or_else(|| "null".to_owned());
        let method_lit = csharp_str(&request.spec.method);
        let ctx_lit = csharp_str(&context);

        if let Some(ref doc) = request.doc {
            emit_csharp_doc(doc, "///", writer);
        }

        let vis_async = if self.options.http_io == HttpIo::Sync {
            "private"
        } else {
            "public"
        };

        wln!(
            writer,
            "{vis_async} static async Task<{struct_name}> {async_name}({decls_str})"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            let (base_url, body_arg, form_arg) = emit_request_setup(&request.spec, writer);
            wln!(writer,
                "var _html = await Rt.FetchHtmlAsync(client, {ctx_lit}, {method_lit}, {base_url}, _queryPairs, _headers, _cookies, {body_arg}, {form_arg}, {path_arg}, {join_arg}, options).ConfigureAwait(false);"
            );
            wln!(writer, "return new {struct_name}(_html);");
        });
        writer.writeln("}");
        writer.blankline();

        if self.options.http_io != HttpIo::Async {
            wln!(
                writer,
                "public static {struct_name} {sync_name}({decls_str}) =>"
            );
            writer.indented(|writer| {
                wln!(
                    writer,
                    "{async_name}({forward_str}).GetAwaiter().GetResult();"
                );
            });
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
        match artifact {
            RestArtifact::ErrorVariant(_) | RestArtifact::MatcherList(_) => {}
            RestArtifact::ResultAlias(alias) => {
                self.emit_result_alias_def(alias, module, writer);
            }
        }
    }

    fn emit_result_alias_def(
        &mut self,
        alias: &ResultAliasDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let effective_name =
            module.resolve_effective_result_alias_name(&alias.struct_name, &alias.name);
        if !self.emitted_aliases.insert(effective_name.clone()) {
            return;
        }

        let ok_type_opt = if let Some(ref schema_name) = alias.response_schema {
            let clean_s = crate::json_to_kdl::to_pascal_case(schema_name).replace('-', "");
            let target_def = module.json_def(schema_name);
            let is_dict_def = target_def.is_some_and(|j| j.is_dict);
            if is_dict_def {
                let info = crate::ast::TypeInfo::json(schema_name, alias.response_is_array);
                Some(Self::resolve_type(&info, module))
            } else {
                let s_pascal = format!("{clean_s}Json");
                let is_arr = alias.response_is_array || target_def.is_some_and(|j| j.is_array);
                if is_arr {
                    Some(format!("List<{s_pascal}>"))
                } else {
                    Some(s_pascal)
                }
            }
        } else if alias.has_response_path(module) {
            Some("JsonNode?".to_owned())
        } else {
            None
        };

        let variants_by_name: IndexMap<&str, &ResultVariantDef> = module
            .rest_artifacts
            .iter()
            .filter_map(|a| match a {
                RestArtifact::ErrorVariant(v) if v.struct_name == alias.struct_name => {
                    Some((v.name.as_str(), v))
                }
                _ => None,
            })
            .collect();

        wln!(writer, "public abstract record {effective_name}");
        writer.writeln("{");
        writer.indented(|writer| {
            writer.writeln("public virtual bool IsOk => false;");
            writer.writeln("public virtual string Kind => \"error\";");
            writer.writeln("public virtual string Variant => string.Empty;");
            writer.blankline();

            if let Some(ref ok_ty) = ok_type_opt {
                wln!(writer,
                    "public sealed record Ok({ok_ty} Value) : {effective_name}"
                );
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("public int Status { get; init; } = 200;");
                    writer.writeln("public Dictionary<string, string> Headers { get; init; } = new(StringComparer.OrdinalIgnoreCase);");
                    writer.writeln("public override bool IsOk => true;");
                    writer.writeln("public override string Kind => \"ok\";");
                });
                writer.writeln("}");
            } else {
                wln!(writer, "public sealed record Ok() : {effective_name}");
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("public object? Value => null;");
                    writer.writeln("public int Status { get; init; } = 200;");
                    writer.writeln("public Dictionary<string, string> Headers { get; init; } = new(StringComparer.OrdinalIgnoreCase);");
                    writer.writeln("public override bool IsOk => true;");
                    writer.writeln("public override string Kind => \"ok\";");
                });
                writer.writeln("}");
            }
            writer.blankline();

            wln!(writer,
                "public sealed record DecodeErr(int Status, string Body, string Cause) : {effective_name}"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("public Dictionary<string, string> Headers { get; init; } = new(StringComparer.OrdinalIgnoreCase);");
                writer.writeln("public override string Kind => \"decode\";");
            });
            writer.writeln("}");
            writer.blankline();

            wln!(writer,
                "public sealed record TransportErr(Exception Cause) : {effective_name}"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("public int Status => 0;");
                writer.writeln("public override string Kind => \"transport\";");
            });
            writer.writeln("}");
            writer.blankline();

            wln!(writer,
                "public sealed record UnknownErr(int Status, JsonNode? Body) : {effective_name}"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("public JsonNode? Value => this.Body;");
                writer.writeln("public Dictionary<string, string> Headers { get; init; } = new(StringComparer.OrdinalIgnoreCase);");
                writer.writeln("public override string Kind => \"unknown\";");
            });
            writer.writeln("}");

            let mut seen_short = std::collections::HashSet::new();
            for err_var_name in &alias.err_variants {
                let Some(&var_def) = variants_by_name.get(err_var_name.as_str()) else {
                    continue;
                };
                let short_name = csharp_variant_name(&alias.struct_name, &var_def.name);
                if !seen_short.insert(short_name.clone()) {
                    continue;
                }
                let body_type = if !var_def.schema_name.is_empty() {
                    let clean_s =
                        crate::json_to_kdl::to_pascal_case(&var_def.schema_name).replace('-', "");
                    let target_def = module.json_def(&var_def.schema_name);
                    if target_def.is_some_and(|j| j.is_dict) {
                        let info =
                            crate::ast::TypeInfo::json(&var_def.schema_name, var_def.schema_is_array);
                        Self::resolve_type(&info, module)
                    } else {
                        let s_pascal = format!("{clean_s}Json");
                        let is_arr = var_def.schema_is_array
                            || target_def.is_some_and(|j| j.is_array);
                        if is_arr {
                            format!("List<{s_pascal}>")
                        } else {
                            s_pascal
                        }
                    }
                } else {
                    "JsonNode?".to_owned()
                };
                let full_lit = csharp_str(&var_def.name);
                writer.blankline();
                wln!(writer,
                    "public sealed record {short_name}(int Status, {body_type} Body) : {effective_name}"
                );
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("public Dictionary<string, string> Headers { get; init; } = new(StringComparer.OrdinalIgnoreCase);");
                    wln!(writer, "public {body_type} Value => this.Body;");
                    wln!(writer, "public override string Variant => {full_lit};");
                });
                writer.writeln("}");
            }
        });
        writer.writeln("}");
        writer.blankline();
    }

    /// Emits `<Endpoint>Async` and `<Endpoint>` REST methods on a `(rest)struct`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let struct_name = &struct_decl.name;
        let struct_pascal = csharp_pascal(struct_name);
        let method_pascal = if endpoint.name.is_empty() || endpoint.name == "fetch" {
            "Fetch".to_owned()
        } else {
            csharp_pascal(&endpoint.name)
        };
        let async_name = format!("{method_pascal}Async");
        let sync_name = method_pascal.clone();
        let result_type =
            module.resolve_effective_result_alias_name(struct_name, &endpoint.result_alias_name);
        let context = format!("{struct_pascal}.{method_pascal}");
        let ctx_lit = csharp_str(&context);
        let method_lit = csharp_str(&endpoint.spec.method);
        let (decls, forward_args) = placeholder_params(&endpoint.placeholders);
        let decls_str = decls.join(", ");
        let forward_str = forward_args.join(", ");

        let matcher_list: Option<&MatcherListDef> =
            module.rest_artifacts.iter().find_map(|a| match a {
                RestArtifact::MatcherList(m) if m.struct_name == *struct_name => Some(m),
                _ => None,
            });

        let variants_by_name: IndexMap<&str, &ResultVariantDef> = module
            .rest_artifacts
            .iter()
            .filter_map(|a| match a {
                RestArtifact::ErrorVariant(v) if v.struct_name == *struct_name => {
                    Some((v.name.as_str(), v))
                }
                _ => None,
            })
            .collect();

        if let Some(ref doc) = endpoint.doc {
            emit_csharp_doc(doc, "///", writer);
        }

        let vis_async = if self.options.http_io == HttpIo::Sync {
            "private"
        } else {
            "public"
        };

        wln!(
            writer,
            "{vis_async} static async Task<{result_type}> {async_name}({decls_str})"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            let (base_url, body_arg, form_arg) = emit_request_setup(&endpoint.spec, writer);
            writer.writeln("int _status;");
            writer.writeln("string _respText;");
            writer.writeln("Dictionary<string, string> _respHeaders;");
            writer.writeln("try");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(writer,
                    "(_status, _respText, _respHeaders) = await Rt.ExecuteHttpAsync(client, {method_lit}, {base_url}, _queryPairs, _headers, _cookies, {body_arg}, {form_arg}, options).ConfigureAwait(false);"
                );
            });
            writer.writeln("}");
            writer.writeln("catch (Exception _ex)");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(writer,
                    "return new {result_type}.TransportErr(_ex);"
                );
            });
            writer.writeln("}");
            writer.blankline();

            writer.writeln("bool _hasJson = Rt.TryParseJsonNode(_respText, out var _parsedNode);");
            writer.writeln("Rt.RtError? _lastDecodeErr = null;");

            if let Some(matchers) = matcher_list {
                for entry in &matchers.entries {
                    let short_name = csharp_variant_name(struct_name, &entry.factory_name);
                    let req_keys: Vec<String> =
                        entry.required_keys.iter().map(|k| csharp_str(k)).collect();
                    let conds: Vec<String> = entry
                        .conditions
                        .iter()
                        .map(|(p, v)| render_error_cond(p, v))
                        .collect();
                    let keys_expr = if req_keys.is_empty() {
                        "Array.Empty<string>()".to_owned()
                    } else {
                        format!("new[] {{ {} }}", req_keys.join(", "))
                    };
                    let conds_expr = if conds.is_empty() {
                        "Array.Empty<Rt.ErrorCond>()".to_owned()
                    } else {
                        format!("new[] {{ {} }}", conds.join(", "))
                    };

                    wln!(writer, "if (_status == {})", entry.status);
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "if (_hasJson && Rt.MatchErrorJson(_parsedNode, {keys_expr}, {conds_expr}))"
                        );
                        writer.writeln("{");
                        writer.indented(|writer| {
                            if !entry.error_schema.is_empty() {
                                let var_def =
                                    variants_by_name.get(entry.factory_name.as_str()).copied();
                                let is_arr = var_def.is_some_and(|v| v.schema_is_array)
                                    || module
                                        .json_def(&entry.error_schema)
                                        .is_some_and(|j| j.is_array);
                                let err_ti =
                                    crate::ast::TypeInfo::json(&entry.error_schema, is_arr);
                                let err_ctx_lit =
                                    csharp_str(&format!("{result_type}.{short_name}"));
                                let dec_expr = Self::render_json_decode_expr(
                                    &err_ti,
                                    "_parsedNode",
                                    &err_ctx_lit,
                                    module,
                                );
                                writer.writeln("try");
                                writer.writeln("{");
                                writer.indented(|writer| {
                                    wln!(writer,
                                        "var _errBody = {dec_expr};"
                                    );
                                    wln!(writer,
                                        "return new {result_type}.{short_name}(_status, _errBody) {{ Headers = _respHeaders }};"
                                    );
                                });
                                writer.writeln("}");
                                writer.writeln("catch (Rt.RtError _decEx)");
                                writer.writeln("{");
                                writer.indented(|writer| {
                                    writer.writeln("_lastDecodeErr = _decEx;");
                                });
                                writer.writeln("}");
                            } else {
                                wln!(writer,
                                    "return new {result_type}.{short_name}(_status, _parsedNode) {{ Headers = _respHeaders }};"
                                );
                            }
                        });
                        writer.writeln("}");
                    });
                    writer.writeln("}");
                }
            }

            writer.blankline();
            writer.writeln("if (_status >= 200 && _status < 300)");
            writer.writeln("{");
            writer.indented(|writer| {
                if let Some(ref schema_name) = endpoint.response_schema {
                    let is_arr = endpoint.response_is_array
                        || module.json_def(schema_name).is_some_and(|j| j.is_array);
                    let ok_ti = crate::ast::TypeInfo::json(schema_name, is_arr);
                    let dec_expr =
                        Self::render_json_decode_expr(&ok_ti, "_targetNode", &ctx_lit, module);
                    let path_arg = endpoint
                        .response_path
                        .as_deref()
                        .map(csharp_str)
                        .unwrap_or_else(|| "null".to_owned());

                    writer.writeln("if (!_hasJson)");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "return new {result_type}.DecodeErr(_status, _respText, \"Invalid JSON response\") {{ Headers = _respHeaders }};"
                        );
                    });
                    writer.writeln("}");
                    writer.writeln("try");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "var _targetNode = Rt.ExtractJsonValue(_parsedNode, {path_arg}, {ctx_lit});"
                        );
                        wln!(writer, "var _okVal = {dec_expr};");
                        wln!(writer,
                            "return new {result_type}.Ok(_okVal) {{ Status = _status, Headers = _respHeaders }};"
                        );
                    });
                    writer.writeln("}");
                    writer.writeln("catch (Rt.RtError _err)");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "return new {result_type}.DecodeErr(_status, _respText, _err.Message) {{ Headers = _respHeaders }};"
                        );
                    });
                    writer.writeln("}");
                } else if let Some(ref resp_path) = endpoint.response_path {
                    let path_lit = csharp_str(resp_path);
                    writer.writeln("if (!_hasJson)");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "return new {result_type}.DecodeErr(_status, _respText, \"Invalid JSON response\") {{ Headers = _respHeaders }};"
                        );
                    });
                    writer.writeln("}");
                    writer.writeln("try");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "var _targetNode = Rt.ExtractJsonValue(_parsedNode, {path_lit}, {ctx_lit});"
                        );
                        wln!(writer,
                            "return new {result_type}.Ok(_targetNode) {{ Status = _status, Headers = _respHeaders }};"
                        );
                    });
                    writer.writeln("}");
                    writer.writeln("catch (Rt.RtError _err)");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        wln!(writer,
                            "return new {result_type}.DecodeErr(_status, _respText, _err.Message) {{ Headers = _respHeaders }};"
                        );
                    });
                    writer.writeln("}");
                } else {
                    wln!(writer,
                        "return new {result_type}.Ok() {{ Status = _status, Headers = _respHeaders }};"
                    );
                }
            });
            writer.writeln("}");
            writer.blankline();

            writer.writeln("if (_lastDecodeErr is not null)");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(writer,
                    "return new {result_type}.DecodeErr(_status, _respText, _lastDecodeErr.Message) {{ Headers = _respHeaders }};"
                );
            });
            writer.writeln("}");
            writer.writeln(
                "JsonNode? _unkBody = _hasJson ? _parsedNode : JsonValue.Create(_respText);",
            );
            wln!(writer,
                "return new {result_type}.UnknownErr(_status, _unkBody) {{ Headers = _respHeaders }};"
            );
        });
        writer.writeln("}");
        writer.blankline();

        if self.options.http_io != HttpIo::Async {
            wln!(
                writer,
                "public static {result_type} {sync_name}({decls_str}) =>"
            );
            writer.indented(|writer| {
                wln!(
                    writer,
                    "{async_name}({forward_str}).GetAwaiter().GetResult();"
                );
            });
            writer.blankline();
        }
    }
}

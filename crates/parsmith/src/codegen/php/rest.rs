//! PHP 8.1+ `@request` HTML/raw fetch methods and `(rest)struct` client code generation.

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultVariantDef,
    StructDecl,
};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::php::dom::PhpDomSpelling;
use crate::codegen::php::json::php_json_projector_method;
use crate::codegen::php::{PhpEmitter, emit_php_doc, php_ident, php_pascal, php_str};
use crate::codegen::writer::{CodeWriter, wln};
use crate::linter::symbols::to_snake_case;
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderTemplate,
    PlaceholderType, RequestBodyKind, RequestPayload, RequestSpec,
};

/// Maps a [`PlaceholderSpec`] to its PHP parameter type hint and default value suffix.
pub fn format_php_placeholder_params(placeholders: &[PlaceholderSpec]) -> String {
    let mut parts = Vec::with_capacity(placeholders.len() + 2);
    parts.push("SyncTransport $transport".to_owned());

    let sorted = PlaceholderSpec::sort_required_first(placeholders);
    for ph in sorted {
        let name = php_ident(&ph.name);
        let base_ty = if ph.is_array {
            "array"
        } else {
            match ph.type_name {
                PlaceholderType::Str => "string",
                PlaceholderType::Int => "int",
                PlaceholderType::Float => "float",
                PlaceholderType::Bool => "bool",
            }
        };
        if ph.is_optional {
            parts.push(format!("?{base_ty} ${name} = null"));
        } else {
            parts.push(format!("{base_ty} ${name}"));
        }
    }

    parts.push("mixed ...$__extra".to_owned());
    parts.join(", ")
}

fn render_template_concat(tmpl: &PlaceholderTemplate) -> String {
    if !tmpl.has_placeholders() {
        return php_str(&tmpl.source());
    }
    let mut parts: Vec<String> = Vec::new();
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Literal(lit) => {
                if !lit.is_empty() {
                    parts.push(php_str(lit));
                }
            }
            PlaceholderPart::Placeholder(ph) => {
                let ident = php_ident(&ph.name);
                if ph.type_name == PlaceholderType::Bool {
                    if ph.is_optional {
                        parts.push(format!(
                            "(${ident} === null ? '' : (${ident} ? 'true' : 'false'))"
                        ));
                    } else {
                        parts.push(format!("(${ident} ? 'true' : 'false')"));
                    }
                } else {
                    parts.push(format!("((string) (${ident} ?? ''))"));
                }
            }
        }
    }
    if parts.is_empty() {
        "''".to_owned()
    } else if parts.len() == 1 {
        parts.remove(0)
    } else {
        format!("({})", parts.join(" . "))
    }
}

fn render_template_expr(tmpl: &PlaceholderTemplate) -> String {
    if let Some(ph) = tmpl.single_placeholder() {
        let ident = php_ident(&ph.name);
        if let Some(sep) = ph.array_join_sep() {
            let sep_lit = php_str(sep);
            let join_expr = format!(
                "implode({sep_lit}, array_map(static fn(mixed $_x): string => (string) $_x, ${ident}))"
            );
            if ph.is_optional {
                return format!("(${ident} !== null ? {join_expr} : null)");
            }
            return join_expr;
        }
        return format!("${ident}");
    }
    render_template_concat(tmpl)
}

fn render_template_map(map: &IndexMap<String, PlaceholderTemplate>) -> String {
    if map.is_empty() {
        return "[]".to_owned();
    }
    let items: Vec<String> = map
        .iter()
        .map(|(k, v)| format!("{} => {}", php_str(k), render_template_expr(v)))
        .collect();
    format!("[{}]", items.join(", "))
}

fn render_params_map(map: &IndexMap<String, Vec<PlaceholderTemplate>>) -> String {
    if map.is_empty() {
        return "[]".to_owned();
    }
    let mut items = Vec::with_capacity(map.len());
    for (k, vals) in map {
        if vals.len() == 1 {
            let v = &vals[0];
            let effective_key = v.effective_param_key(k);
            items.push(format!(
                "{} => {}",
                php_str(&effective_key),
                render_template_expr(v)
            ));
        } else {
            let list_items: Vec<String> = vals.iter().map(render_template_expr).collect();
            items.push(format!("{} => [{}]", php_str(k), list_items.join(", ")));
        }
    }
    format!("[{}]", items.join(", "))
}

fn render_json_template(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "null".to_owned(),
        JsonTemplateValue::Bool(true) => "true".to_owned(),
        JsonTemplateValue::Bool(false) => "false".to_owned(),
        JsonTemplateValue::Number(n) => n.clone(),
        JsonTemplateValue::String(s) => php_str(s),
        JsonTemplateValue::Placeholder(ph) => format!("${}", php_ident(&ph.name)),
        JsonTemplateValue::Template(tmpl) => render_template_expr(tmpl),
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template).collect();
            format!("[{}]", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            if map.is_empty() {
                "new \\stdClass()".to_owned()
            } else {
                let inner: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{} => {}", php_str(k), render_json_template(v)))
                    .collect();
                format!("[{}]", inner.join(", "))
            }
        }
    }
}

fn render_build_request_expr(spec: &RequestSpec) -> String {
    let method_lit = php_str(&spec.method);
    let url_expr = render_template_concat(&spec.url);
    let headers_expr = render_template_map(&spec.headers);
    let params_expr = render_params_map(&spec.params);
    let cookies_expr = render_template_map(&spec.cookies);

    let (body_expr, body_kind_lit) = match spec.body_kind {
        RequestBodyKind::Empty => ("null".to_owned(), "'empty'".to_owned()),
        RequestBodyKind::Json => {
            if let Some(ref jt) = spec.json_template {
                (render_json_template(jt), "'json'".to_owned())
            } else {
                ("null".to_owned(), "'empty'".to_owned())
            }
        }
        RequestBodyKind::Form => {
            if let RequestPayload::Form(ref form_map) = spec.payload {
                (render_template_map(form_map), "'form'".to_owned())
            } else {
                ("null".to_owned(), "'empty'".to_owned())
            }
        }
        RequestBodyKind::Raw => {
            if let RequestPayload::Template(ref tmpl) = spec.payload {
                (render_template_expr(tmpl), "'raw'".to_owned())
            } else {
                ("null".to_owned(), "'empty'".to_owned())
            }
        }
    };

    format!(
        "Rt::buildRequest({method_lit}, {url_expr}, {headers_expr}, {params_expr}, {cookies_expr}, {body_expr}, {body_kind_lit}, $__extra)"
    )
}

fn render_php_condition_closure(
    required_keys: &[String],
    conditions: &IndexMap<String, ErrorConditionValue>,
) -> String {
    if required_keys.is_empty() && conditions.is_empty() {
        return "null".to_owned();
    }
    let mut checks: Vec<String> = Vec::new();
    for key in required_keys {
        checks.push(format!("Rt::hasErrorKey($_b, {})", php_str(key)));
    }
    for (path, val) in conditions {
        let path_lit = php_str(path);
        let cond_expr = match val {
            ErrorConditionValue::Null => {
                format!("Rt::resolveDotpath($_b, {path_lit}, true) === null")
            }
            ErrorConditionValue::Bool(b) => {
                format!("Rt::resolveDotpath($_b, {path_lit}, true) === {b}")
            }
            ErrorConditionValue::Int(n) => {
                format!("Rt::resolveDotpath($_b, {path_lit}, true) === {n}")
            }
            ErrorConditionValue::Float(f) => {
                format!(
                    "(is_numeric($_v = Rt::resolveDotpath($_b, {path_lit}, true)) && (float) $_v === {f:?})"
                )
            }
            ErrorConditionValue::String(s) => {
                format!(
                    "Rt::resolveDotpath($_b, {path_lit}, true) === {}",
                    php_str(s)
                )
            }
        };
        checks.push(cond_expr);
    }
    format!("static fn(mixed $_b): bool => {}", checks.join(" && "))
}

impl<D: PhpDomSpelling> PhpEmitter<D> {
    /// Emits a synthesized [`RestArtifact`] (`ErrorVariant`, `ResultAlias`, or `MatcherList`).
    pub(crate) fn emit_rest_artifact_impl(
        &mut self,
        artifact: &RestArtifact,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        match artifact {
            RestArtifact::ErrorVariant(variant) => {
                Self::emit_error_variant_def(variant, module, writer);
            }
            RestArtifact::ResultAlias(_) => {}
            RestArtifact::MatcherList(_) => {}
        }
    }

    fn emit_error_variant_def(
        variant: &ResultVariantDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let val_type = if variant.schema_name.is_empty() {
            "mixed".to_owned()
        } else {
            let def_is_arr_or_dict = module
                .json_def(&variant.schema_name)
                .is_some_and(|j| j.is_array || j.is_dict);
            if variant.schema_is_array || def_is_arr_or_dict {
                "array".to_owned()
            } else {
                format!("{}Json", php_pascal(&variant.schema_name))
            }
        };
        let status = variant.status;
        let v_name = &variant.name;
        let v_lit = php_str(v_name);

        wln!(
            writer,
            "readonly class {v_name} implements \\JsonSerializable"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            writer.writeln("/** @param array<string, string> $headers */");
            writer.writeln("public function __construct(");
            writer.indented(|writer| {
                writer.writeln("public array $headers,");
                wln!(writer, "public {val_type} $value,");
                wln!(writer, "public int $status = {status},");
                writer.writeln("public bool $ok = false,");
                writer.writeln("public bool $is_ok = false,");
                wln!(writer, "public string $variant = {v_lit},");
            });
            writer.writeln(") {");
            writer.writeln("}");
            writer.blankline();

            writer.writeln("public function isOk(): bool");
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("return false;");
            });
            writer.writeln("}");
            writer.blankline();

            writer.writeln("public function jsonSerialize(): mixed");
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("return [");
                writer.indented(|writer| {
                    writer.writeln("'ok' => false,");
                    writer.writeln("'kind' => 'http',");
                    writer.writeln("'variant' => $this->variant,");
                    writer.writeln("'status' => $this->status,");
                    writer.writeln("'headers' => $this->headers,");
                    writer.writeln("'value' => $this->value,");
                });
                writer.writeln("];");
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();
    }

    /// Emits `private static function _matchers(): array` inside a `(rest)struct` class.
    pub(crate) fn emit_struct_matchers_method(
        struct_decl: &StructDecl,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let matchers_opt = module.rest_artifacts.iter().find_map(|a| match a {
            RestArtifact::MatcherList(m) if m.struct_name == struct_decl.name => Some(m),
            _ => None,
        });

        writer.writeln("/** @return list<ErrMatcher> */");
        writer.writeln("private static function _matchers(): array");
        writer.writeln("{");
        writer.indented(|writer| {
            let Some(MatcherListDef { entries, .. }) = matchers_opt else {
                writer.writeln("return [];");
                return;
            };
            if entries.is_empty() {
                writer.writeln("return [];");
                return;
            }
            writer.writeln("return [");
            writer.indented(|writer| {
                for entry in entries {
                    let check_closure =
                        render_php_condition_closure(&entry.required_keys, &entry.conditions);
                    let factory_closure = if !entry.error_schema.is_empty()
                        && module.json_def(&entry.error_schema).is_some()
                    {
                        let s_cls = format!("{}Json", php_pascal(&entry.error_schema));
                        let proj = php_json_projector_method(&entry.error_schema);
                        format!(
                            "static fn(array $headers, mixed $value): {} => new {}(headers: $headers, value: {s_cls}::{proj}($value))",
                            entry.factory_name, entry.factory_name
                        )
                    } else {
                        format!(
                            "static fn(array $headers, mixed $value): {} => new {}(headers: $headers, value: Rt::toPlainPhp($value))",
                            entry.factory_name, entry.factory_name
                        )
                    };
                    wln!(writer,
                        "new ErrMatcher({}, {check_closure}, {factory_closure}),",
                        entry.status
                    );
                }
            });
            writer.writeln("];");
        });
        writer.writeln("}");
        writer.blankline();
    }

    /// Emits `@request` `fetch` / `fetch_<name>` static methods on an HTML or Raw struct.
    pub(crate) fn emit_request_impl(
        &mut self,
        _struct_decl: &StructDecl,
        request: &RequestDecl,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let method_name = if request.name.is_empty() || request.name == "fetch" {
            "fetch".to_owned()
        } else {
            let suffix = to_snake_case(&request.name);
            let clean = suffix.strip_prefix("fetch_").unwrap_or(&suffix);
            format!("fetch_{clean}")
        };
        let params = format_php_placeholder_params(&request.placeholders);
        let req_expr = render_build_request_expr(&request.spec);
        let rp_lit = php_str(request.response_path.as_deref().unwrap_or(""));
        let rj_lit = php_str(request.response_join.as_deref().unwrap_or(""));

        if let Some(ref doc) = request.doc {
            emit_php_doc(doc, writer);
        }
        wln!(
            writer,
            "public static function {method_name}({params}): static"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            wln!(writer, "$_req = {req_expr};");
            wln!(
                writer,
                "return new static(Rt::fetchHtml($transport, $_req, {rp_lit}, {rj_lit}));"
            );
        });
        writer.writeln("}");
        writer.blankline();
    }

    /// Emits a typed REST endpoint static method on a `(rest)struct`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let method_name = if endpoint.name.is_empty() {
            "fetch".to_owned()
        } else {
            to_snake_case(&endpoint.name)
        };
        let params = format_php_placeholder_params(&endpoint.placeholders);
        let req_expr = render_build_request_expr(&endpoint.spec);

        let mut union_types: Vec<String> = vec!["Ok".to_owned()];
        if let Some(MatcherListDef { entries, .. }) =
            module.rest_artifacts.iter().find_map(|a| match a {
                RestArtifact::MatcherList(m) if m.struct_name == struct_decl.name => Some(m),
                _ => None,
            })
        {
            for entry in entries {
                if !union_types.contains(&entry.factory_name) {
                    union_types.push(entry.factory_name.clone());
                }
            }
        }
        union_types.push("UnknownErr".to_owned());
        union_types.push("TransportErr".to_owned());
        union_types.push("DecodeErr".to_owned());
        let ret_union = union_types.join("|");

        let has_response_schema = endpoint
            .response_schema
            .as_deref()
            .and_then(|s| module.json_def(s))
            .is_some();

        let value_fn_expr = match (&endpoint.response_path, &endpoint.response_schema) {
            (Some(rp), Some(schema_name)) if has_response_schema => {
                let s_cls = format!("{}Json", php_pascal(schema_name));
                let proj = php_json_projector_method(schema_name);
                let rp_lit = php_str(rp);
                format!(
                    "static fn(mixed $_b): mixed => {s_cls}::{proj}(Rt::resolveDotpath($_b, {rp_lit}, false))"
                )
            }
            (Some(rp), _) => {
                let rp_lit = php_str(rp);
                format!(
                    "static fn(mixed $_b): mixed => Rt::toPlainPhp(Rt::resolveDotpath($_b, {rp_lit}, false))"
                )
            }
            (None, Some(schema_name)) if has_response_schema => {
                let s_cls = format!("{}Json", php_pascal(schema_name));
                let proj = php_json_projector_method(schema_name);
                format!("static fn(mixed $_b): mixed => {s_cls}::{proj}($_b)")
            }
            (None, _) => "null".to_owned(),
        };

        if let Some(ref doc) = endpoint.doc {
            emit_php_doc(doc, writer);
        }
        wln!(
            writer,
            "public static function {method_name}({params}): {ret_union}"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            wln!(writer, "$_req = {req_expr};");
            wln!(
                writer,
                "return Rt::restCall($transport, $_req, self::_matchers(), {value_fn_expr});"
            );
        });
        writer.writeln("}");
        writer.blankline();
    }
}

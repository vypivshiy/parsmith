//! Unified HTTP transport runtime (`SyncTransport` / `AsyncTransport`, `via_*` factories),
//! `@request` fetch methods, and monadic `(rest)struct` client emission for Python 3.10+.

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultAliasDef,
    ResultVariantDef, StructDecl,
};
use crate::codegen::HttpIo;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::python::dom::PyDomSpelling;
use crate::codegen::python::json::python_json_projector_name;
use crate::codegen::python::{PythonEmitter, emit_docstring, py_str};
use crate::codegen::writer::{CodeWriter, wln};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::to_snake_case;
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderTemplate,
    PlaceholderType, RequestBodyKind, RequestPayload, RequestSpec,
};

/// Key used to register the unified HTTP transport runtime in [`ModuleBuilder`].
pub const STD_HTTP_TRANSPORT_KEY: &str = "rt_http_transport";

/// Key used to register synchronous `rt_fetch` in [`ModuleBuilder`].
pub const STD_FETCH_KEY: &str = "rt_fetch";

/// Alias for [`STD_FETCH_KEY`].
pub const STD_FETCH_SYNC_KEY: &str = STD_FETCH_KEY;

/// Key used to register asynchronous `rt_fetch_async` in [`ModuleBuilder`].
pub const STD_FETCH_ASYNC_KEY: &str = "rt_fetch_async";

/// Key used to register core REST result dataclasses (`Ok`, `UnknownErr`, `TransportErr`, `DecodeErr`, `ErrMatcher`) in [`ModuleBuilder`].
pub const STD_REST_CORE_KEY: &str = "rt_rest_core";

/// Key used to register synchronous `rt_rest_call` in [`ModuleBuilder`].
pub const STD_REST_CALL_KEY: &str = "rt_rest_call";

/// Alias for [`STD_REST_CALL_KEY`].
pub const STD_REST_SYNC_KEY: &str = STD_REST_CALL_KEY;

/// Key used to register asynchronous `rt_rest_call_async` in [`ModuleBuilder`].
pub const STD_REST_CALL_ASYNC_KEY: &str = "rt_rest_call_async";

/// Alias for [`STD_REST_CALL_ASYNC_KEY`].
pub const STD_REST_ASYNC_KEY: &str = STD_REST_CALL_ASYNC_KEY;

/// Symbols exported by [`STD_HTTP_TRANSPORT_CODE`] when `-R / --separate-runtime` is active.
pub const HTTP_TRANSPORT_RUNTIME_SYMBOLS: &[&str] = &[
    "HttpRequest",
    "RawResponse",
    "TransportError",
    "SyncTransport",
    "AsyncTransport",
    "HttpxTransport",
    "AsyncHttpxTransport",
    "RequestsTransport",
    "AiohttpTransport",
    "UrllibTransport",
    "via_httpx",
    "via_httpx_async",
    "via_requests",
    "via_aiohttp",
    "via_urllib",
    "rt_build_request",
];

/// Symbols exported by [`STD_REST_CORE_CODE`] when `-R / --separate-runtime` is active.
pub const REST_CORE_RUNTIME_SYMBOLS: &[&str] = &[
    "Ok",
    "UnknownErr",
    "TransportErr",
    "DecodeErr",
    "ErrMatcher",
    "rt_dispatch_response",
];

/// Unified HTTP transport contract (`HttpRequest`, `RawResponse`, `TransportError`, `SyncTransport`,
/// `AsyncTransport`), lazy-imported built-in adapters, `via_*` factory functions, and `rt_build_request`.
pub const STD_HTTP_TRANSPORT_CODE: &str = include_str!("runtime/http_transport.py");

/// Synchronous HTML/raw `@request` fetch helper (`rt_fetch`).
pub const STD_FETCH_SYNC_CODE: &str = r#"
def rt_fetch(
    transport: SyncTransport,
    req: HttpRequest,
    *,
    response_path: str = "",
    response_join: str = "",
) -> str:
    resp = transport.send(req)
    if response_path:
        data: Any = json.loads(resp.text)
        for part in response_path.split("."):
            data = data[part]
        if response_join and isinstance(data, list):
            return response_join.join(str(x) for x in data)
        return str(data)
    return resp.text
"#;

/// Asynchronous HTML/raw `@request` fetch helper (`rt_fetch_async`).
pub const STD_FETCH_ASYNC_CODE: &str = r#"
async def rt_fetch_async(
    transport: AsyncTransport,
    req: HttpRequest,
    *,
    response_path: str = "",
    response_join: str = "",
) -> str:
    resp = await transport.send(req)
    if response_path:
        data: Any = json.loads(resp.text)
        for part in response_path.split("."):
            data = data[part]
        if response_join and isinstance(data, list):
            return response_join.join(str(x) for x in data)
        return str(data)
    return resp.text
"#;

/// Core monadic REST result dataclasses (`Ok`, `UnknownErr`, `TransportErr`, `DecodeErr`, `ErrMatcher`)
/// and response dispatcher (`rt_dispatch_response`).
pub const STD_REST_CORE_CODE: &str = include_str!("runtime/rest_core.py");

/// Synchronous REST endpoint call helper (`rt_rest_call`).
pub const STD_REST_SYNC_CODE: &str = r#"
def rt_rest_call(
    transport: SyncTransport,
    req: HttpRequest,
    matchers: list[ErrMatcher],
    value_fn: Callable[[Any], Any] | None = None,
) -> Any:
    try:
        resp = transport.send(req)
    except TransportError as e:
        return TransportErr(status=0, cause=repr(e.cause), exception=e.cause)
    return rt_dispatch_response(matchers, resp, value_fn)
"#;

/// Asynchronous REST endpoint call helper (`rt_rest_call_async`).
pub const STD_REST_ASYNC_CODE: &str = r#"
async def rt_rest_call_async(
    transport: AsyncTransport,
    req: HttpRequest,
    matchers: list[ErrMatcher],
    value_fn: Callable[[Any], Any] | None = None,
) -> Any:
    try:
        resp = await transport.send(req)
    except TransportError as e:
        return TransportErr(status=0, cause=repr(e.cause), exception=e.cause)
    return rt_dispatch_response(matchers, resp, value_fn)
"#;

pub use crate::codegen::{module_has_fetch, module_has_rest, module_uses_http};

/// Registers the unified HTTP transport runtime in `builder`.
pub fn require_http_transport_std(builder: &mut ModuleBuilder, separate_runtime: bool) {
    builder.require_import("from typing import Any");
    if !separate_runtime {
        builder.require_import("from typing import Literal, Protocol");
    }
    builder.require_std_with_imports(
        STD_HTTP_TRANSPORT_KEY,
        STD_HTTP_TRANSPORT_CODE,
        [
            "from collections.abc import Mapping",
            "from dataclasses import dataclass",
            "from dataclasses import field",
            "import importlib",
            "import json",
            "import urllib.error",
            "import urllib.parse",
            "import urllib.request",
        ],
    );
}

/// Registers `rt_fetch` and/or `rt_fetch_async` in `builder` according to `http_io`.
pub fn require_fetch_std(builder: &mut ModuleBuilder, http_io: HttpIo, separate_runtime: bool) {
    require_http_transport_std(builder, separate_runtime);
    if matches!(http_io, HttpIo::Both | HttpIo::Sync) {
        builder.require_std_with_imports(STD_FETCH_SYNC_KEY, STD_FETCH_SYNC_CODE, ["import json"]);
    }
    if matches!(http_io, HttpIo::Both | HttpIo::Async) {
        builder.require_std_with_imports(
            STD_FETCH_ASYNC_KEY,
            STD_FETCH_ASYNC_CODE,
            ["import json"],
        );
    }
}

/// Registers the REST monad runtime in `builder` according to `http_io`.
pub fn require_rest_std(builder: &mut ModuleBuilder, http_io: HttpIo, separate_runtime: bool) {
    require_http_transport_std(builder, separate_runtime);
    builder
        .require_import("from dataclasses import dataclass")
        .require_import("from typing import Any, Literal, TypeAlias, cast");
    if !separate_runtime {
        builder.require_import("from typing import Generic, TypeVar");
    }
    builder.require_std_with_imports(
        STD_REST_CORE_KEY,
        STD_REST_CORE_CODE,
        [
            "from collections.abc import Callable",
            "from dataclasses import dataclass",
            "from dataclasses import field",
            "import json",
        ],
    );
    if matches!(http_io, HttpIo::Both | HttpIo::Sync) {
        builder.require_std(STD_REST_SYNC_KEY, STD_REST_SYNC_CODE);
    }
    if matches!(http_io, HttpIo::Both | HttpIo::Async) {
        builder.require_std(STD_REST_ASYNC_KEY, STD_REST_ASYNC_CODE);
    }
}

impl<D: PyDomSpelling> PythonEmitter<D> {
    /// Emits a synthesized [`RestArtifact`] (`ErrorVariant`, `ResultAlias`, or `MatcherList`).
    pub(crate) fn emit_rest_artifact_impl(
        &mut self,
        artifact: &RestArtifact,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        require_rest_std(builder, self.options.http_io, self.options.separate_runtime);
        match artifact {
            RestArtifact::ErrorVariant(variant) => {
                Self::emit_error_variant_def(variant, writer);
            }
            RestArtifact::ResultAlias(alias) => {
                self.emit_result_alias_def(alias, module, builder, writer);
            }
            RestArtifact::MatcherList(matchers) => {
                Self::emit_matcher_list_def(matchers, module, writer);
            }
        }
    }

    fn emit_error_variant_def(variant: &ResultVariantDef, writer: &mut CodeWriter) {
        let val_type = if variant.schema_name.is_empty() {
            "Any".to_owned()
        } else {
            let base = format!("{}Json", to_pascal_case(&variant.schema_name));
            if variant.schema_is_array {
                format!("list[{base}]")
            } else {
                base
            }
        };
        let status = variant.status;
        writer.writeln("@dataclass(slots=True, frozen=True)");
        wln!(writer, "class {}:", variant.name);
        writer.indented(|writer| {
            writer.writeln("headers: dict[str, str]");
            wln!(writer, "value: {val_type}");
            wln!(writer, "status: Literal[{status}] = {status}");
            writer.writeln("ok: Literal[False] = False");
            writer.blankline();
            writer.writeln("@property");
            writer.writeln("def is_ok(self) -> Literal[False]:");
            writer.indented(|writer| {
                writer.writeln("return False");
            });
        });
        writer.blankline();
    }

    fn emit_result_alias_def(
        &self,
        alias: &ResultAliasDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let ok_type = if let Some(ref schema_name) = alias.response_schema {
            let base = format!("{}Json", to_pascal_case(schema_name));
            if alias.response_is_array {
                format!("list[{base}]")
            } else {
                base
            }
        } else if alias.has_response_path(module) {
            builder.require_import("from typing import Any");
            "Any".to_owned()
        } else {
            "None".to_owned()
        };

        let mut parts = Vec::with_capacity(alias.err_variants.len() + 4);
        parts.push(format!("Ok[{ok_type}]"));
        for err_var in &alias.err_variants {
            parts.push(err_var.clone());
        }
        parts.push("UnknownErr".to_owned());
        parts.push("TransportErr".to_owned());
        parts.push("DecodeErr".to_owned());
        let union_rhs = parts.join(" | ");

        let effective_name =
            module.resolve_effective_result_alias_name(&alias.struct_name, &alias.name);
        wln!(writer, "{effective_name}: TypeAlias = {union_rhs}");
        let struct_qualified = format!("{}{}", to_pascal_case(&alias.struct_name), alias.name);
        if struct_qualified != effective_name {
            wln!(writer, "{struct_qualified}: TypeAlias = {effective_name}");
        }
        writer.blankline();
    }

    fn emit_matcher_list_def(matchers: &MatcherListDef, module: &Module, writer: &mut CodeWriter) {
        let var_name = format!(
            "RT_{}_MATCHERS",
            to_snake_case(&matchers.struct_name).to_uppercase()
        );
        if matchers.entries.is_empty() {
            wln!(writer, "{var_name}: list[ErrMatcher] = []");
            writer.blankline();
            return;
        }

        wln!(writer, "{var_name}: list[ErrMatcher] = [");
        writer.indented(|writer| {
            for entry in &matchers.entries {
                let check_arg = render_py_condition_lambda(&entry.required_keys, &entry.conditions)
                    .unwrap_or_else(|| "None".to_owned());
                let factory_arg = if module.json_def(&entry.error_schema).is_some() {
                    let proj = python_json_projector_name(&entry.error_schema);
                    format!(
                        "lambda headers, value: {}(headers=headers, value={proj}(value))",
                        entry.factory_name
                    )
                } else {
                    format!(
                        "lambda headers, value: {}(headers=headers, value=value)",
                        entry.factory_name
                    )
                };
                wln!(
                    writer,
                    "ErrMatcher({}, {check_arg}, {factory_arg}),",
                    entry.status
                );
            }
        });
        writer.writeln("]");
        writer.blankline();
    }

    /// Emits `@request` `fetch` and/or `async_fetch` classmethods on an HTML or Raw struct.
    pub(crate) fn emit_request_impl(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let http_io = self.options.http_io;
        require_fetch_std(builder, http_io, self.options.separate_runtime);
        let struct_name = to_pascal_case(&struct_decl.name);

        let (sync_name, async_name) = if request.name.is_empty() || request.name == "fetch" {
            ("fetch".to_owned(), "async_fetch".to_owned())
        } else {
            let suffix = to_snake_case(&request.name);
            let clean = suffix.strip_prefix("fetch_").unwrap_or(&suffix);
            (format!("fetch_{clean}"), format!("async_fetch_{clean}"))
        };

        let ph_params = format_placeholder_params(&request.placeholders);
        let req_call_lines = render_rt_build_request_lines(&request.spec);

        let mut extra_fetch_args = String::new();
        if let Some(ref rp) = request.response_path {
            extra_fetch_args.push_str(&format!(", response_path={}", py_str(rp)));
        }
        if let Some(ref rj) = request.response_join {
            extra_fetch_args.push_str(&format!(", response_join={}", py_str(rj)));
        }

        let has_sync = matches!(http_io, HttpIo::Both | HttpIo::Sync);
        let has_async = matches!(http_io, HttpIo::Both | HttpIo::Async);

        if has_sync {
            writer.blankline();
            writer.writeln("@classmethod");
            wln!(
                writer,
                "def {sync_name}(cls, transport: SyncTransport{ph_params}, **kwargs: Any) -> {struct_name}:"
            );
            writer.indented(|writer| {
                if let Some(ref doc) = request.doc {
                    emit_docstring(doc, writer);
                }
                for line in &req_call_lines {
                    writer.writeln(line);
                }
                wln!(
                    writer,
                    "return cls(rt_fetch(transport, _req{extra_fetch_args}))"
                );
            });
        }

        if has_async {
            writer.blankline();
            writer.writeln("@classmethod");
            wln!(
                writer,
                "async def {async_name}(cls, transport: AsyncTransport{ph_params}, **kwargs: Any) -> {struct_name}:"
            );
            writer.indented(|writer| {
                if let Some(ref doc) = request.doc {
                    emit_docstring(doc, writer);
                }
                for line in &req_call_lines {
                    writer.writeln(line);
                }
                wln!(
                    writer,
                    "return cls(await rt_fetch_async(transport, _req{extra_fetch_args}))"
                );
            });
        }
    }

    /// Emits a typed REST endpoint (`<name>` / `async_<name>`) on a `(rest)struct`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let http_io = self.options.http_io;
        require_rest_std(builder, http_io, self.options.separate_runtime);

        let method_name = if endpoint.name.is_empty() {
            "fetch".to_owned()
        } else {
            to_snake_case(&endpoint.name)
        };
        let async_method_name = format!("async_{method_name}");

        let ret_type = module
            .resolve_effective_result_alias_name(&struct_decl.name, &endpoint.result_alias_name);
        let ph_params = format_placeholder_params(&endpoint.placeholders);
        let matchers_var = format!(
            "RT_{}_MATCHERS",
            to_snake_case(&struct_decl.name).to_uppercase()
        );
        let req_call_lines = render_rt_build_request_lines(&endpoint.spec);

        let has_response_schema = endpoint
            .response_schema
            .as_deref()
            .and_then(|s| module.json_def(s))
            .is_some();

        let value_fn_arg: Option<String> = if let Some(ref rp) = endpoint.response_path {
            let accessor = rp
                .split('.')
                .map(|seg| format!("[{}]", py_str(seg)))
                .collect::<Vec<_>>()
                .join("");
            if let Some(ref schema_name) = endpoint.response_schema
                && has_response_schema
            {
                let proj = python_json_projector_name(schema_name);
                Some(format!("lambda _b: {proj}(_b{accessor})"))
            } else {
                Some(format!("lambda _b: _b{accessor}"))
            }
        } else if let Some(ref schema_name) = endpoint.response_schema
            && has_response_schema
        {
            Some(python_json_projector_name(schema_name))
        } else {
            None
        };

        let is_first_endpoint = struct_decl
            .rest_endpoints
            .first()
            .is_some_and(|first| first.name == endpoint.name && first.span == endpoint.span);
        let has_sync = matches!(http_io, HttpIo::Both | HttpIo::Sync);
        let has_async = matches!(http_io, HttpIo::Both | HttpIo::Async);

        if has_sync {
            if !is_first_endpoint {
                writer.blankline();
            }
            writer.writeln("@classmethod");
            wln!(
                writer,
                "def {method_name}(cls, transport: SyncTransport{ph_params}, **kwargs: Any) -> {ret_type}:"
            );
            writer.indented(|writer| {
                if let Some(ref doc) = endpoint.doc {
                    emit_docstring(doc, writer);
                }
                for line in &req_call_lines {
                    writer.writeln(line);
                }
                if let Some(ref vf) = value_fn_arg {
                    wln!(writer,
                        "return cast({ret_type}, rt_rest_call(transport, _req, {matchers_var}, value_fn={vf}))"
                    );
                } else {
                    wln!(writer,
                        "return cast({ret_type}, rt_rest_call(transport, _req, {matchers_var}))"
                    );
                }
            });
        }

        if has_async {
            if has_sync || !is_first_endpoint {
                writer.blankline();
            }
            writer.writeln("@classmethod");
            wln!(
                writer,
                "async def {async_method_name}(cls, transport: AsyncTransport{ph_params}, **kwargs: Any) -> {ret_type}:"
            );
            writer.indented(|writer| {
                if let Some(ref doc) = endpoint.doc {
                    emit_docstring(doc, writer);
                }
                for line in &req_call_lines {
                    writer.writeln(line);
                }
                if let Some(ref vf) = value_fn_arg {
                    wln!(writer,
                        "return cast({ret_type}, await rt_rest_call_async(transport, _req, {matchers_var}, value_fn={vf}))"
                    );
                } else {
                    wln!(writer,
                        "return cast({ret_type}, await rt_rest_call_async(transport, _req, {matchers_var}))"
                    );
                }
            });
        }
    }
}

fn format_placeholder_params(placeholders: &[PlaceholderSpec]) -> String {
    if placeholders.is_empty() {
        return String::new();
    }
    let sorted = PlaceholderSpec::sort_required_first(placeholders);

    let parts: Vec<String> = sorted
        .into_iter()
        .map(|ph| {
            let py_name = to_snake_case(&ph.name);
            let base_ty = match ph.type_name {
                PlaceholderType::Str => "str",
                PlaceholderType::Int => "int",
                PlaceholderType::Float => "float",
                PlaceholderType::Bool => "bool",
            };
            let mut ty = if ph.is_array {
                format!("list[{base_ty}]")
            } else {
                base_ty.to_owned()
            };
            if ph.is_optional {
                ty = format!("{ty} | None = None");
            }
            format!("{py_name}: {ty}")
        })
        .collect();

    format!(", {}", parts.join(", "))
}

fn render_rt_build_request_lines(spec: &RequestSpec) -> Vec<String> {
    let method_lit = py_str(&spec.method);
    let url_expr = render_url_template(&spec.url);

    let mut kw_parts: Vec<String> = Vec::new();
    if !spec.headers.is_empty() {
        kw_parts.push(format!("headers={}", render_template_map(&spec.headers)));
    }
    if !spec.params.is_empty() {
        kw_parts.push(format!("params={}", render_params_map(&spec.params)));
    }
    if !spec.cookies.is_empty() {
        kw_parts.push(format!("cookies={}", render_template_map(&spec.cookies)));
    }

    match spec.body_kind {
        RequestBodyKind::Empty => {}
        RequestBodyKind::Json => {
            if let Some(ref jt) = spec.json_template {
                let body_expr = render_json_template(jt);
                kw_parts.push(format!("body={body_expr}"));
                kw_parts.push("body_kind='json'".to_owned());
            }
        }
        RequestBodyKind::Form => {
            if let RequestPayload::Form(ref form_map) = spec.payload {
                kw_parts.push(format!("body={}", render_template_map(form_map)));
                kw_parts.push("body_kind='form'".to_owned());
            }
        }
        RequestBodyKind::Raw => {
            if let RequestPayload::Template(ref tmpl) = spec.payload {
                kw_parts.push(format!("body={}", render_template_expr(tmpl)));
                kw_parts.push("body_kind='raw'".to_owned());
            }
        }
    }

    kw_parts.push("extra=kwargs".to_owned());
    let joined_kw = kw_parts.join(", ");
    vec![format!(
        "_req = rt_build_request({method_lit}, {url_expr}, {joined_kw})"
    )]
}

fn render_url_template(tmpl: &PlaceholderTemplate) -> String {
    if tmpl.has_placeholders() {
        format!("f\"{}\"", escape_fstring(tmpl))
    } else {
        py_str(&tmpl.source())
    }
}

fn render_template_expr(tmpl: &PlaceholderTemplate) -> String {
    if let Some(ph) = tmpl.single_placeholder() {
        let py_name = to_snake_case(&ph.name);
        if let Some(sep) = ph.array_join_sep() {
            let join_expr = format!("{}.join(str(_x) for _x in {py_name})", py_str(sep));
            if ph.is_optional {
                return format!("({join_expr} if {py_name} is not None else None)");
            }
            return join_expr;
        }
        return py_name;
    }
    if tmpl.has_placeholders() {
        format!("f\"{}\"", escape_fstring(tmpl))
    } else {
        py_str(&tmpl.source())
    }
}

fn escape_fstring(tmpl: &PlaceholderTemplate) -> String {
    let mut out = String::new();
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Placeholder(ph) => {
                out.push('{');
                out.push_str(&to_snake_case(&ph.name));
                out.push('}');
            }
            PlaceholderPart::Literal(lit) => {
                for ch in lit.chars() {
                    match ch {
                        '{' => out.push_str("{{"),
                        '}' => out.push_str("}}"),
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
            }
        }
    }
    out
}

fn render_template_map(map: &IndexMap<String, PlaceholderTemplate>) -> String {
    if map.is_empty() {
        return "{}".to_owned();
    }
    let items: Vec<String> = map
        .iter()
        .map(|(k, v)| format!("{}: {}", py_str(k), render_template_expr(v)))
        .collect();
    format!("{{{}}}", items.join(", "))
}

fn render_params_map(map: &IndexMap<String, Vec<PlaceholderTemplate>>) -> String {
    if map.is_empty() {
        return "{}".to_owned();
    }
    let mut items = Vec::with_capacity(map.len());
    for (k, vals) in map {
        if vals.len() == 1 {
            let v = &vals[0];
            let effective_key = v.effective_param_key(k);
            items.push(format!(
                "{}: {}",
                py_str(&effective_key),
                render_template_expr(v)
            ));
        } else {
            let list_items: Vec<String> = vals.iter().map(render_template_expr).collect();
            items.push(format!("{}: [{}]", py_str(k), list_items.join(", ")));
        }
    }
    format!("{{{}}}", items.join(", "))
}

fn render_json_template(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "None".to_owned(),
        JsonTemplateValue::Bool(true) => "True".to_owned(),
        JsonTemplateValue::Bool(false) => "False".to_owned(),
        JsonTemplateValue::Number(n) => n.clone(),
        JsonTemplateValue::String(s) => py_str(s),
        JsonTemplateValue::Placeholder(ph) => to_snake_case(&ph.name),
        JsonTemplateValue::Template(tmpl) => render_template_expr(tmpl),
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template).collect();
            format!("[{}]", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_str(k), render_json_template(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn render_py_condition_lambda(
    required_keys: &[String],
    conditions: &IndexMap<String, ErrorConditionValue>,
) -> Option<String> {
    if required_keys.is_empty() && conditions.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    let mut var_counter = 1usize;

    for key in required_keys {
        let segs = crate::gjson::parse_gjson_segments(key).unwrap_or_else(|_| {
            vec![crate::gjson::GjsonSegment {
                kind: crate::gjson::GjsonSegmentKind::Key,
                key: key.clone(),
                index: None,
            }]
        });
        if segs.len() == 1 && segs[0].kind == crate::gjson::GjsonSegmentKind::Key {
            parts.push(format!("{} in _b", py_str(&segs[0].key)));
        } else {
            let expr = py_path_expr("_b", key, &mut var_counter);
            parts.push(format!("{expr} is not None"));
        }
    }

    for (path, val) in conditions {
        let lhs = py_path_expr("_b", path, &mut var_counter);
        let cond = match val {
            ErrorConditionValue::Bool(true) => format!("{lhs} is True"),
            ErrorConditionValue::Bool(false) => format!("{lhs} is False"),
            ErrorConditionValue::Null => format!("{lhs} is None"),
            ErrorConditionValue::Int(n) => format!("{lhs} == {n}"),
            ErrorConditionValue::Float(f) => format!("{lhs} == {f:?}"),
            ErrorConditionValue::String(s) => format!("{lhs} == {}", py_str(s)),
        };
        parts.push(cond);
    }

    Some(format!("lambda _b: {}", parts.join(" and ")))
}

fn py_path_expr(body_var: &str, path: &str, var_counter: &mut usize) -> String {
    let segs = crate::gjson::parse_gjson_segments(path).unwrap_or_else(|_| {
        vec![crate::gjson::GjsonSegment {
            kind: crate::gjson::GjsonSegmentKind::Key,
            key: path.to_owned(),
            index: None,
        }]
    });
    if segs.len() == 1 && segs[0].kind == crate::gjson::GjsonSegmentKind::Key {
        return format!("{body_var}.get({})", py_str(&segs[0].key));
    }
    let mut expr = body_var.to_owned();
    for (i, seg) in segs.iter().enumerate() {
        if let Some(idx) = seg.index {
            let v = *var_counter;
            *var_counter += 1;
            expr = format!(
                "(_v{v}[{idx}] if isinstance(_v{v} := {expr}, list) and 0 <= {idx} < len(_v{v}) else None)"
            );
        } else if i == 0 {
            expr = format!(
                "({expr}.get({}) if isinstance({expr}, dict) else None)",
                py_str(&seg.key)
            );
        } else {
            let v = *var_counter;
            *var_counter += 1;
            expr = format!(
                "(_v{v}.get({}) if isinstance(_v{v} := {expr}, dict) else None)",
                py_str(&seg.key)
            );
        }
    }
    expr
}

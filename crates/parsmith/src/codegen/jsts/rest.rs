//! Unified HTTP transport runtime (`HttpTransport`, `FetchTransport`, `AxiosTransport`,
//! `viaFetch`, `viaAxios`), `@request` fetch methods, and monadic `(rest)struct` client
//! emission for TypeScript (`.ts`) and JavaScript (`.js` + `.d.ts`).

use indexmap::IndexMap;

use crate::ast::{
    MatcherListDef, Module, RequestDecl, RestArtifact, RestEndpointDecl, ResultAliasDef,
    ResultVariantDef, StructDecl,
};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::jsts::helpers::{
    STD_JSON_PROJECT_JS, STD_JSON_PROJECT_TS, emit_jsdoc, is_js_ident, js_str,
};
use crate::codegen::jsts::{JsTsEmitter, JsTsMode};
use crate::codegen::writer::{CodeWriter, wln};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::{js_json_projector_name, to_camel_case, to_snake_case};
use crate::request::{
    ErrorConditionValue, JsonTemplateValue, PlaceholderPart, PlaceholderSpec, PlaceholderTemplate,
    PlaceholderType, RequestBodyKind, RequestPayload, RequestSpec,
};

/// Key used to register the unified HTTP transport runtime in [`ModuleBuilder`].
pub const STD_HTTP_TRANSPORT_KEY: &str = "rtHttpTransport";

/// Key used to register `rtFetch` in [`ModuleBuilder`].
pub const STD_FETCH_KEY: &str = "rtFetch";

/// Key used to register core REST result types and `rtRestCall` in [`ModuleBuilder`].
pub const STD_REST_CORE_KEY: &str = "rtRestCore";

/// Unified HTTP transport contract (`HttpRequest`, `RawResponse`, `TransportError`, `HttpTransport`,
/// `AxiosLike`), built-in adapters (`FetchTransport`, `AxiosTransport`), `viaFetch` / `viaAxios`
/// factory helpers, and `rtBuildRequest` in TypeScript (`.ts`).
pub const STD_HTTP_TRANSPORT_TS: &str = include_str!("runtime/http_transport.ts");

/// Unified HTTP transport runtime in clean ESM JavaScript (`.js`).
pub const STD_HTTP_TRANSPORT_JS: &str = include_str!("runtime/http_transport.js");

/// Unified HTTP transport declarations in `.d.ts` mode.
pub const STD_HTTP_TRANSPORT_DTS: &str = include_str!("runtime/http_transport.d.ts");

/// HTML/raw `@request` fetch helper (`rtFetch`) in TypeScript (`.ts`).
pub const STD_FETCH_TS: &str = r##"
export function rtExtractPath(data: any, path: string): any {
  const parts = path.split(".");
  let cur = data;
  for (let i = 0; i < parts.length; i++) {
    if (cur == null) return undefined;
    const part = parts[i];
    if (part === "#" && Array.isArray(cur)) {
      if (i === parts.length - 1) return cur.length;
      const rest = parts.slice(i + 1).join(".");
      return cur.map((item) => rtExtractPath(item, rest)).filter((v) => v != null);
    }
    cur = cur[part];
  }
  return cur;
}

export async function rtFetch(
  transport: HttpTransport,
  req: HttpRequest,
  responsePath: string = "",
  responseJoin: string = "",
): Promise<string> {
  const resp = await transport.send(req);
  if (responsePath) {
    const data: any = rtExtractPath(JSON.parse(resp.text), responsePath);
    if (responseJoin && Array.isArray(data)) {
      return data.map((x) => String(x)).join(responseJoin);
    }
    return String(data ?? "");
  }
  return resp.text;
}
"##;

/// HTML/raw `@request` fetch helper (`rtFetch`) in JavaScript (`.js`).
pub const STD_FETCH_JS: &str = r##"
export function rtExtractPath(data, path) {
  const parts = path.split(".");
  let cur = data;
  for (let i = 0; i < parts.length; i++) {
    if (cur == null) return undefined;
    const part = parts[i];
    if (part === "#" && Array.isArray(cur)) {
      if (i === parts.length - 1) return cur.length;
      const rest = parts.slice(i + 1).join(".");
      return cur.map((item) => rtExtractPath(item, rest)).filter((v) => v != null);
    }
    cur = cur[part];
  }
  return cur;
}

export async function rtFetch(transport, req, responsePath = "", responseJoin = "") {
  const resp = await transport.send(req);
  if (responsePath) {
    const data = rtExtractPath(JSON.parse(resp.text), responsePath);
    if (responseJoin && Array.isArray(data)) {
      return data.map((x) => String(x)).join(responseJoin);
    }
    return String(data ?? "");
  }
  return resp.text;
}
"##;

/// HTML/raw `@request` fetch helper (`rtFetch`) declaration in `.d.ts`.
pub const STD_FETCH_DTS: &str = r#"
export declare function rtExtractPath(data: any, path: string): any;

export declare function rtFetch(
  transport: HttpTransport,
  req: HttpRequest,
  responsePath?: string,
  responseJoin?: string,
): Promise<string>;
"#;

/// Core monadic REST result types (`Ok<T>`, `UnknownErr`, `TransportErr`, `DecodeErr`, `ErrMatcher`)
/// and response dispatcher (`rtDispatchResponse`, `rtRestCall`) in TypeScript (`.ts`).
pub const STD_REST_CORE_TS: &str = include_str!("runtime/rest_core.ts");

/// Core monadic REST dispatcher (`rtDispatchResponse`, `rtRestCall`) in JavaScript (`.js`).
pub const STD_REST_CORE_JS: &str = include_str!("runtime/rest_core.js");

/// Core monadic REST result type declarations (`Ok<T>`, `UnknownErr`, `TransportErr`, `DecodeErr`, `ErrMatcher`)
/// in `.d.ts` mode.
pub const STD_REST_CORE_DTS: &str = include_str!("runtime/rest_core.d.ts");

impl JsTsEmitter {
    /// Registers the unified HTTP transport runtime in `builder`.
    pub(crate) fn require_http_transport_std(&self, builder: &mut ModuleBuilder) {
        self.require_std_helper(
            STD_HTTP_TRANSPORT_KEY,
            STD_HTTP_TRANSPORT_TS,
            STD_HTTP_TRANSPORT_JS,
            builder,
        );
    }

    /// Registers `rtFetch` (and the unified HTTP transport runtime) in `builder`.
    pub(crate) fn require_fetch_std(&self, builder: &mut ModuleBuilder) {
        self.require_http_transport_std(builder);
        self.require_std_helper(STD_FETCH_KEY, STD_FETCH_TS, STD_FETCH_JS, builder);
    }

    /// Registers the REST monad runtime (and the unified HTTP transport runtime) in `builder`.
    pub(crate) fn require_rest_std(&self, builder: &mut ModuleBuilder) {
        self.require_http_transport_std(builder);
        self.require_std_helper(
            STD_REST_CORE_KEY,
            STD_REST_CORE_TS,
            STD_REST_CORE_JS,
            builder,
        );
    }

    /// Emits a synthesized [`RestArtifact`] (`ErrorVariant`, `ResultAlias`, or `MatcherList`).
    pub(crate) fn emit_rest_artifact_impl(
        &mut self,
        artifact: &RestArtifact,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.require_rest_std(builder);
        match artifact {
            RestArtifact::ErrorVariant(variant) => {
                self.emit_error_variant_def(variant, writer);
            }
            RestArtifact::ResultAlias(alias) => {
                self.emit_result_alias_def(alias, module, writer);
            }
            RestArtifact::MatcherList(matchers) => {
                self.emit_matcher_list_def(matchers, module, writer);
            }
        }
    }

    fn emit_error_variant_def(&self, variant: &ResultVariantDef, writer: &mut CodeWriter) {
        if !self.mode.is_ts() {
            return;
        }
        let val_type = if variant.schema_name.is_empty() {
            "any".to_owned()
        } else {
            let base = format!("{}Json", to_pascal_case(&variant.schema_name));
            if variant.schema_is_array {
                format!("{base}[]")
            } else {
                base
            }
        };
        let variant_lit = js_str(&variant.name);
        wln!(writer, "export interface {} {{", variant.name);
        writer.indented(|writer| {
            writer.writeln("readonly ok: false;");
            writer.writeln("readonly kind: \"error\";");
            wln!(writer, "readonly variant: {variant_lit};");
            writer.writeln("readonly status: number;");
            writer.writeln("readonly headers: Record<string, string>;");
            wln!(writer, "readonly value: {val_type};");
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_result_alias_def(
        &self,
        alias: &ResultAliasDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        if !self.mode.is_ts() {
            return;
        }
        let ok_type = if let Some(ref schema_name) = alias.response_schema {
            let base = format!("{}Json", to_pascal_case(schema_name));
            if alias.response_is_array {
                format!("{base}[]")
            } else {
                base
            }
        } else if alias.has_response_path(module) {
            "any".to_owned()
        } else {
            "null".to_owned()
        };

        let mut parts = Vec::with_capacity(alias.err_variants.len() + 4);
        parts.push(format!("Ok<{ok_type}>"));
        for err_var in &alias.err_variants {
            parts.push(err_var.clone());
        }
        parts.push("UnknownErr".to_owned());
        parts.push("TransportErr".to_owned());
        parts.push("DecodeErr".to_owned());
        let union_rhs = parts.join(" | ");

        let struct_qualified = format!("{}{}", to_pascal_case(&alias.struct_name), alias.name);
        let effective_name =
            module.resolve_effective_result_alias_name(&alias.struct_name, &alias.name);

        wln!(writer, "export type {struct_qualified} = {union_rhs};");
        if effective_name != struct_qualified {
            wln!(writer, "export type {effective_name} = {struct_qualified};");
        }
        writer.blankline();
    }

    fn emit_matcher_list_def(
        &self,
        matchers: &MatcherListDef,
        module: &Module,
        writer: &mut CodeWriter,
    ) {
        let var_name = format!(
            "RT_{}_MATCHERS",
            to_snake_case(&matchers.struct_name).to_uppercase()
        );
        if self.mode.is_dts() {
            wln!(writer, "export declare const {var_name}: ErrMatcher[];");
            writer.blankline();
            return;
        }
        let is_ts = self.mode == JsTsMode::TypeScript;

        if matchers.entries.is_empty() {
            if is_ts {
                wln!(writer, "export const {var_name}: ErrMatcher[] = [];");
            } else {
                wln!(writer, "export const {var_name} = [];");
            }
            writer.blankline();
            return;
        }

        if is_ts {
            wln!(writer, "export const {var_name}: ErrMatcher[] = [");
        } else {
            wln!(writer, "export const {var_name} = [");
        }
        writer.indented(|writer| {
            for entry in &matchers.entries {
                let check_arg =
                    render_js_condition_arrow(&entry.required_keys, &entry.conditions, is_ts)
                        .unwrap_or_else(|| "null".to_owned());
                let val_expr = if module.json_def(&entry.error_schema).is_some() {
                    let proj = js_json_projector_name(&entry.error_schema);
                    format!("{proj}(value)")
                } else {
                    "value".to_owned()
                };
                let variant_lit = js_str(&entry.factory_name);
                let status = entry.status;
                if is_ts {
                    wln!(writer,
                        "{{ status: {status}, check: {check_arg}, factory: (headers: Record<string, string>, value: any): {} => ({{ ok: false, kind: \"error\", variant: {variant_lit}, status: {status}, headers, value: {val_expr} }}) }},",
                        entry.factory_name
                    );
                } else {
                    wln!(writer,
                        "{{ status: {status}, check: {check_arg}, factory: (headers, value) => ({{ ok: false, kind: \"error\", variant: {variant_lit}, status: {status}, headers, value: {val_expr} }}) }},",
                    );
                }
            }
        });
        writer.writeln("];");
        writer.blankline();
    }

    /// Emits `@request` `fetch` (and named `fetch<PascalName>` / `fetch_<snake_name>`) static async methods
    /// on an HTML or Raw struct.
    pub(crate) fn emit_request_impl(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.require_fetch_std(builder);
        let struct_name = to_pascal_case(&struct_decl.name);

        let (camel_name, snake_name) = if request.name.is_empty() || request.name == "fetch" {
            ("fetch".to_owned(), "fetch".to_owned())
        } else {
            let suffix = to_snake_case(&request.name);
            let clean = suffix.strip_prefix("fetch_").unwrap_or(&suffix);
            (
                format!("fetch{}", to_pascal_case(clean)),
                format!("fetch_{clean}"),
            )
        };

        let ret_type = struct_name.clone();
        let sorted_phs = PlaceholderSpec::sort_required_first(&request.placeholders);

        let mut extra_fetch_args = String::new();
        if let Some(ref rp) = request.response_path {
            extra_fetch_args.push_str(&format!(", {}", js_str(rp)));
            if let Some(ref rj) = request.response_join {
                extra_fetch_args.push_str(&format!(", {}", js_str(rj)));
            }
        } else if let Some(ref rj) = request.response_join {
            extra_fetch_args.push_str(&format!(", \"\", {}", js_str(rj)));
        }

        if self.mode.is_dts() {
            emit_method_overloads(&camel_name, &sorted_phs, &ret_type, writer);
            if snake_name != camel_name {
                emit_method_overloads(&snake_name, &sorted_phs, &ret_type, writer);
            }
            return;
        }

        writer.blankline();
        if let Some(ref doc) = request.doc {
            emit_jsdoc(doc, writer);
        }

        let is_ts = self.mode == JsTsMode::TypeScript;
        if sorted_phs.is_empty() {
            if is_ts {
                wln!(
                    writer,
                    "static async {camel_name}(transport: HttpTransport, extra?: Record<string, unknown>): Promise<{ret_type}> {{"
                );
            } else {
                wln!(
                    writer,
                    "static async {camel_name}(transport, extra = {{}}) {{"
                );
            }
            writer.indented(|writer| {
                let req_expr = render_rt_build_request_expr(&request.spec, "extra");
                wln!(writer, "const _req = {req_expr};");
                wln!(
                    writer,
                    "return new {struct_name}(await rtFetch(transport, _req{extra_fetch_args}));"
                );
            });
            writer.writeln("}");
        } else {
            if is_ts {
                emit_method_overloads(&camel_name, &sorted_phs, &ret_type, writer);
                wln!(
                    writer,
                    "static async {camel_name}(transport: HttpTransport, ..._args: any[]): Promise<{ret_type}> {{"
                );
            } else {
                wln!(writer, "static async {camel_name}(transport, ..._args) {{");
            }
            writer.indented(|writer| {
                let unpack_line = render_extract_params_line(&sorted_phs);
                writer.writeln(&unpack_line);
                let req_expr = render_rt_build_request_expr(&request.spec, "_extra");
                wln!(writer, "const _req = {req_expr};");
                wln!(
                    writer,
                    "return new {struct_name}(await rtFetch(transport, _req{extra_fetch_args}));"
                );
            });
            writer.writeln("}");
        }

        if snake_name != camel_name {
            writer.blankline();
            if is_ts {
                emit_method_overloads(&snake_name, &sorted_phs, &ret_type, writer);
                wln!(
                    writer,
                    "static async {snake_name}(transport: HttpTransport, ..._args: any[]): Promise<{ret_type}> {{"
                );
            } else {
                wln!(writer, "static async {snake_name}(transport, ..._args) {{");
            }
            writer.indented(|writer| {
                if is_ts {
                    wln!(
                        writer,
                        "return (this.{camel_name} as any)(transport, ..._args);"
                    );
                } else {
                    wln!(writer, "return this.{camel_name}(transport, ..._args);");
                }
            });
            writer.writeln("}");
        }
    }

    /// Emits a typed REST endpoint (`<camelName>` and `<snake_name>`) on a `(rest)struct`.
    pub(crate) fn emit_rest_endpoint_impl(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.require_rest_std(builder);
        if endpoint.response_path.is_some() {
            self.require_std_helper(
                "rtResolveDotpath",
                STD_JSON_PROJECT_TS,
                STD_JSON_PROJECT_JS,
                builder,
            );
        }

        let (camel_name, snake_name) = if endpoint.name.is_empty() || endpoint.name == "fetch" {
            ("fetch".to_owned(), "fetch".to_owned())
        } else {
            let snake = to_snake_case(&endpoint.name);
            (to_camel_case(&snake), snake)
        };

        let ret_type = format!(
            "{}{}",
            to_pascal_case(&struct_decl.name),
            endpoint.result_alias_name
        );
        let sorted_phs = PlaceholderSpec::sort_required_first(&endpoint.placeholders);

        if self.mode.is_dts() {
            emit_method_overloads(&camel_name, &sorted_phs, &ret_type, writer);
            if snake_name != camel_name {
                emit_method_overloads(&snake_name, &sorted_phs, &ret_type, writer);
            }
            return;
        }

        let is_first_endpoint = struct_decl
            .rest_endpoints
            .first()
            .is_some_and(|first| first.name == endpoint.name && first.span == endpoint.span);
        if !is_first_endpoint {
            writer.blankline();
        }
        if let Some(ref doc) = endpoint.doc {
            emit_jsdoc(doc, writer);
        }

        let matchers_var = format!(
            "RT_{}_MATCHERS",
            to_snake_case(&struct_decl.name).to_uppercase()
        );
        let is_ts = self.mode == JsTsMode::TypeScript;

        let has_response_schema = endpoint
            .response_schema
            .as_deref()
            .and_then(|s| module.json_def(s))
            .is_some();

        let value_fn_arg: Option<String> = if let Some(ref rp) = endpoint.response_path {
            let rp_lit = js_str(rp);
            let b_param = if is_ts { "(_b: any)" } else { "(_b)" };
            if let Some(ref schema_name) = endpoint.response_schema
                && has_response_schema
            {
                let proj = js_json_projector_name(schema_name);
                Some(format!(
                    "{b_param} => {proj}(rtResolveDotpath(_b, {rp_lit}, false))"
                ))
            } else {
                Some(format!(
                    "{b_param} => rtResolveDotpath(_b, {rp_lit}, false)"
                ))
            }
        } else if let Some(ref schema_name) = endpoint.response_schema
            && has_response_schema
        {
            Some(js_json_projector_name(schema_name))
        } else {
            None
        };

        let rest_call_expr = match value_fn_arg {
            Some(vf) => format!("rtRestCall(transport, _req, {matchers_var}, {vf})"),
            None => format!("rtRestCall(transport, _req, {matchers_var})"),
        };

        if sorted_phs.is_empty() {
            if is_ts {
                wln!(
                    writer,
                    "static async {camel_name}(transport: HttpTransport, extra?: Record<string, unknown>): Promise<{ret_type}> {{"
                );
            } else {
                wln!(
                    writer,
                    "static async {camel_name}(transport, extra = {{}}) {{"
                );
            }
            writer.indented(|writer| {
                let req_expr = render_rt_build_request_expr(&endpoint.spec, "extra");
                wln!(writer, "const _req = {req_expr};");
                wln!(writer, "return {rest_call_expr};");
            });
            writer.writeln("}");
        } else {
            if is_ts {
                emit_method_overloads(&camel_name, &sorted_phs, &ret_type, writer);
                wln!(
                    writer,
                    "static async {camel_name}(transport: HttpTransport, ..._args: any[]): Promise<{ret_type}> {{"
                );
            } else {
                wln!(writer, "static async {camel_name}(transport, ..._args) {{");
            }
            writer.indented(|writer| {
                let unpack_line = render_extract_params_line(&sorted_phs);
                writer.writeln(&unpack_line);
                let req_expr = render_rt_build_request_expr(&endpoint.spec, "_extra");
                wln!(writer, "const _req = {req_expr};");
                wln!(writer, "return {rest_call_expr};");
            });
            writer.writeln("}");
        }

        if snake_name != camel_name {
            writer.blankline();
            if is_ts {
                emit_method_overloads(&snake_name, &sorted_phs, &ret_type, writer);
                wln!(
                    writer,
                    "static async {snake_name}(transport: HttpTransport, ..._args: any[]): Promise<{ret_type}> {{"
                );
            } else {
                wln!(writer, "static async {snake_name}(transport, ..._args) {{");
            }
            writer.indented(|writer| {
                if is_ts {
                    wln!(
                        writer,
                        "return (this.{camel_name} as any)(transport, ..._args);"
                    );
                } else {
                    wln!(writer, "return this.{camel_name}(transport, ..._args);");
                }
            });
            writer.writeln("}");
        }
    }
}

fn placeholder_ts_type(ph: &PlaceholderSpec) -> String {
    let base_ty = match ph.type_name {
        PlaceholderType::Str => "string",
        PlaceholderType::Int | PlaceholderType::Float => "number",
        PlaceholderType::Bool => "boolean",
    };
    let mut ty = if ph.is_array {
        format!("{base_ty}[]")
    } else {
        base_ty.to_owned()
    };
    if ph.is_optional {
        ty = format!("{ty} | null");
    }
    ty
}

fn emit_method_overloads(
    method_name: &str,
    sorted_phs: &[&PlaceholderSpec],
    ret_type: &str,
    writer: &mut CodeWriter,
) {
    if sorted_phs.is_empty() {
        wln!(
            writer,
            "static {method_name}(transport: HttpTransport, extra?: Record<string, unknown>): Promise<{ret_type}>;"
        );
        return;
    }

    // 1. Positional overload
    let mut pos_params = Vec::with_capacity(sorted_phs.len() + 2);
    pos_params.push("transport: HttpTransport".to_owned());
    for ph in sorted_phs {
        let camel = to_camel_case(&to_snake_case(&ph.name));
        let ty = placeholder_ts_type(ph);
        let opt_q = if ph.is_optional { "?" } else { "" };
        pos_params.push(format!("{camel}{opt_q}: {ty}"));
    }
    pos_params.push("extra?: Record<string, unknown>".to_owned());
    wln!(
        writer,
        "static {method_name}({}): Promise<{ret_type}>;",
        pos_params.join(", ")
    );

    // 2. Named params object overload (supporting both camelCase and snake_case keys)
    let all_optional = sorted_phs.iter().all(|p| p.is_optional);
    let any_required_differs = sorted_phs.iter().any(|p| {
        !p.is_optional && {
            let snake = to_snake_case(&p.name);
            let camel = to_camel_case(&snake);
            camel != snake
        }
    });

    let build_obj_type = |use_snake_for_required: bool| -> String {
        let mut fields = Vec::new();
        for ph in sorted_phs {
            let snake = to_snake_case(&ph.name);
            let camel = to_camel_case(&snake);
            let ty = placeholder_ts_type(ph);
            if ph.is_optional {
                fields.push(format!("{camel}?: {ty}"));
                if snake != camel {
                    fields.push(format!("{snake}?: {ty}"));
                }
            } else {
                let key = if use_snake_for_required {
                    &snake
                } else {
                    &camel
                };
                fields.push(format!("{key}: {ty}"));
            }
        }
        fields.push("[key: string]: unknown".to_owned());
        format!("{{ {} }}", fields.join("; "))
    };

    let obj_type = if any_required_differs {
        format!("{} | {}", build_obj_type(false), build_obj_type(true))
    } else {
        build_obj_type(false)
    };
    let params_opt = if all_optional { "?" } else { "" };
    wln!(
        writer,
        "static {method_name}(transport: HttpTransport, params{params_opt}: {obj_type}, extra?: Record<string, unknown>): Promise<{ret_type}>;"
    );
}

fn render_extract_params_line(sorted_phs: &[&PlaceholderSpec]) -> String {
    let mut destructured = Vec::with_capacity(sorted_phs.len());
    let mut key_tuples = Vec::with_capacity(sorted_phs.len());
    for ph in sorted_phs {
        let snake = to_snake_case(&ph.name);
        let camel = to_camel_case(&snake);
        destructured.push(camel.clone());
        let mut aliases = vec![js_str(&camel)];
        if snake != camel {
            aliases.push(js_str(&snake));
        }
        if ph.name != camel && ph.name != snake {
            aliases.push(js_str(&ph.name));
        }
        key_tuples.push(format!("[{}]", aliases.join(", ")));
    }
    format!(
        "const [{{ {} }}, _extra] = rtExtractParams(_args, [{}]);",
        destructured.join(", "),
        key_tuples.join(", ")
    )
}

fn render_rt_build_request_expr(spec: &RequestSpec, extra_var: &str) -> String {
    let method_lit = js_str(&spec.method);
    let url_expr = render_url_template(&spec.url);

    let mut opt_parts: Vec<String> = Vec::new();
    if !spec.headers.is_empty() {
        opt_parts.push(format!("headers: {}", render_template_map(&spec.headers)));
    }
    if !spec.params.is_empty() {
        opt_parts.push(format!("params: {}", render_params_map(&spec.params)));
    }
    if !spec.cookies.is_empty() {
        opt_parts.push(format!("cookies: {}", render_template_map(&spec.cookies)));
    }

    match spec.body_kind {
        RequestBodyKind::Empty => {}
        RequestBodyKind::Json => {
            if let Some(ref jt) = spec.json_template {
                opt_parts.push(format!("body: {}", render_json_template(jt)));
                opt_parts.push("bodyKind: \"json\"".to_owned());
            }
        }
        RequestBodyKind::Form => {
            if let RequestPayload::Form(ref form_map) = spec.payload {
                opt_parts.push(format!("body: {}", render_template_map(form_map)));
                opt_parts.push("bodyKind: \"form\"".to_owned());
            }
        }
        RequestBodyKind::Raw => {
            if let RequestPayload::Template(ref tmpl) = spec.payload {
                opt_parts.push(format!("body: {}", render_template_expr(tmpl, false)));
                opt_parts.push("bodyKind: \"raw\"".to_owned());
            }
        }
    }

    if extra_var == "extra" {
        opt_parts.push("extra".to_owned());
    } else {
        opt_parts.push(format!("extra: {extra_var}"));
    }

    format!(
        "rtBuildRequest({method_lit}, {url_expr}, {{ {} }})",
        opt_parts.join(", ")
    )
}

fn render_url_template(tmpl: &PlaceholderTemplate) -> String {
    if tmpl.has_placeholders() {
        format!("`{}`", escape_template_literal(tmpl, true))
    } else {
        js_str(&tmpl.source())
    }
}

fn render_template_expr(tmpl: &PlaceholderTemplate, encode_uri: bool) -> String {
    if let Some(ph) = tmpl.single_placeholder() {
        let js_name = to_camel_case(&to_snake_case(&ph.name));
        if let Some(sep) = ph.array_join_sep() {
            let join_expr = format!("{js_name}.map(String).join({})", js_str(sep));
            if ph.is_optional {
                return format!("({js_name} != null ? {join_expr} : undefined)");
            }
            return join_expr;
        }
        return js_name;
    }
    if tmpl.has_placeholders() {
        format!("`{}`", escape_template_literal(tmpl, encode_uri))
    } else {
        js_str(&tmpl.source())
    }
}

fn escape_template_literal(tmpl: &PlaceholderTemplate, encode_uri: bool) -> String {
    let mut out = String::new();
    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Placeholder(ph) => {
                let js_name = to_camel_case(&to_snake_case(&ph.name));
                out.push_str("${");
                if encode_uri {
                    out.push_str(&format!("encodeURIComponent(String({js_name}))"));
                } else {
                    out.push_str(&js_name);
                }
                out.push('}');
            }
            PlaceholderPart::Literal(lit) => {
                for ch in lit.chars() {
                    match ch {
                        '\\' => out.push_str("\\\\"),
                        '`' => out.push_str("\\`"),
                        '$' => out.push_str("\\$"),
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

fn format_obj_key(k: &str) -> String {
    if is_js_ident(k) {
        k.to_owned()
    } else {
        js_str(k)
    }
}

fn render_template_map(map: &IndexMap<String, PlaceholderTemplate>) -> String {
    if map.is_empty() {
        return "{}".to_owned();
    }
    let items: Vec<String> = map
        .iter()
        .map(|(k, v)| {
            let key_s = format_obj_key(k);
            let val_s = render_template_expr(v, false);
            if key_s == val_s {
                key_s
            } else {
                format!("{key_s}: {val_s}")
            }
        })
        .collect();
    format!("{{ {} }}", items.join(", "))
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
            let key_s = format_obj_key(&effective_key);
            let val_s = render_template_expr(v, false);
            if key_s == val_s {
                items.push(key_s);
            } else {
                items.push(format!("{key_s}: {val_s}"));
            }
        } else {
            let list_items: Vec<String> = vals
                .iter()
                .map(|v| render_template_expr(v, false))
                .collect();
            items.push(format!(
                "{}: [{}]",
                format_obj_key(k),
                list_items.join(", ")
            ));
        }
    }
    format!("{{ {} }}", items.join(", "))
}

fn render_json_template(val: &JsonTemplateValue) -> String {
    match val {
        JsonTemplateValue::Null => "null".to_owned(),
        JsonTemplateValue::Bool(true) => "true".to_owned(),
        JsonTemplateValue::Bool(false) => "false".to_owned(),
        JsonTemplateValue::Number(n) => n.clone(),
        JsonTemplateValue::String(s) => js_str(s),
        JsonTemplateValue::Placeholder(ph) => {
            let js_name = to_camel_case(&to_snake_case(&ph.name));
            if ph.is_optional {
                format!("{js_name} ?? null")
            } else {
                js_name
            }
        }
        JsonTemplateValue::Template(tmpl) => render_template_expr(tmpl, false),
        JsonTemplateValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(render_json_template).collect();
            format!("[{}]", inner.join(", "))
        }
        JsonTemplateValue::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let key_s = format_obj_key(k);
                    let val_s = render_json_template(v);
                    if key_s == val_s {
                        key_s
                    } else {
                        format!("{key_s}: {val_s}")
                    }
                })
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

fn render_js_condition_arrow(
    required_keys: &[String],
    conditions: &IndexMap<String, ErrorConditionValue>,
    is_ts: bool,
) -> Option<String> {
    if required_keys.is_empty() && conditions.is_empty() {
        return None;
    }
    let mut parts = Vec::new();

    for key in required_keys {
        let segs = crate::gjson::parse_gjson_segments(key).unwrap_or_else(|_| {
            vec![crate::gjson::GjsonSegment {
                kind: crate::gjson::GjsonSegmentKind::Key,
                key: key.clone(),
                index: None,
            }]
        });
        if segs.len() > 1 {
            let expr = js_path_expr("_b", key);
            parts.push(format!("{expr} != null"));
        } else {
            parts.push(format!("{} in _b", js_str(&segs[0].key)));
        }
    }

    for (path, val) in conditions {
        let lhs = js_path_expr("_b", path);
        let cond = match val {
            ErrorConditionValue::Bool(true) => format!("{lhs} === true"),
            ErrorConditionValue::Bool(false) => format!("{lhs} === false"),
            ErrorConditionValue::Null => format!("{lhs} === null"),
            ErrorConditionValue::Int(n) => format!("{lhs} === {n}"),
            ErrorConditionValue::Float(f) => format!("{lhs} === {f:?}"),
            ErrorConditionValue::String(s) => format!("{lhs} === {}", js_str(s)),
        };
        parts.push(cond);
    }

    let param = if is_ts {
        "(_b: Record<string, any>)"
    } else {
        "(_b)"
    };
    Some(format!("{param} => {}", parts.join(" && ")))
}

fn js_path_expr(body_var: &str, path: &str) -> String {
    let mut expr = body_var.to_owned();
    let segs = crate::gjson::parse_gjson_segments(path).unwrap_or_else(|_| {
        vec![crate::gjson::GjsonSegment {
            kind: crate::gjson::GjsonSegmentKind::Key,
            key: path.to_owned(),
            index: None,
        }]
    });
    for seg in segs {
        if let Some(idx) = seg.index {
            expr.push_str(&format!("?.[{idx}]"));
        } else {
            expr.push_str(&format!("?.[{}]", js_str(&seg.key)));
        }
    }
    expr
}

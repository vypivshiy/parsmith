# HTTP Requests (`@request`) & Typed REST Clients (`(rest)struct`)

`parsmith` compiles raw HTTP requests and POSIX `curl` commands directly into typed network constructors and monadic REST API clients across Python, TypeScript, JavaScript, Go, Rust, Elixir, PHP 8.1+, C# (.NET 8+), and Kotlin (JVM).

---

## 1. Overview: `@request` in HTML/`(raw)struct` vs `(rest)struct`

The `@request` directive operates in two distinct contexts depending on the enclosing `struct` kind:

| Aspect | HTML / `(raw)struct` | `(rest)struct` |
| :--- | :--- | :--- |
| **Primary Role** | Optional convenience factory (`fetch` / `async_fetch`) that fetches the page and instantiates the parser | Dedicated multi-endpoint REST API client |
| **Parser Coupling** | Parser class remains transport-agnostic (can always be instantiated directly from an HTML/text string) | Struct consists exclusively of `@doc`, `@request`, and `@error[:<suffix>]` directives (`E203` on regular fields) |
| **`name=` & `response=`** | Optional on a single `@request` (required when declaring multiple `@request` blocks on the same struct) | **Mandatory** on every `@request`: `name="<kebab-id>"` and `response="<SchemaName>"` (or `response=#null`) |
| **Return Value** | Instantiated parser (`<Struct>` in Python/TS/JS/Go, `Result<<Struct>Parser, rt::RtError>` in Rust, `{:ok, parser}` in Elixir) | Monadic `Result` union (`Ok`, `<Struct>Err<Status>[<Suffix>]`, `DecodeErr`, `UnknownErr`, `TransportErr`) |
| **Error Behavior** | Raises/returns transport or HTTP $\ge 400$ errors | **Never throws** on HTTP, network, or JSON decode failures; all outcomes are represented as typed `Result` variants |

### Why `(rest)struct` & `@request` Instead of OpenAPI?

OpenAPI (Swagger) and `parsmith` address opposite sides of HTTP client generation:

1. **Producer-First (OpenAPI) vs. Trace-First (`parsmith`)**:
   OpenAPI is designed for backend teams who author or auto-generate specifications from server framework annotations. When building clients for external, undocumented, or reverse-engineered APIs (mobile backends, internal BFFs, e-commerce AJAX routes), you rarely have a formal specification—you have a series of captured **raw HTTP traces or `curl` commands** (from browser DevTools, Mitmproxy, or Charles) and a few sample JSON responses. `parsmith` works directly from that minimal empirical input: paste the `curl` or raw HTTP request into `@request`, mark variable segments with typed placeholders (`{{id:int}}`), infer the response contract with `parsmith json-to-kdl`, and compile a working client.
2. **Isolating Consumers from Stale, Bloated, or Inaccurate Third-Party Specs**:
   Even when an external vendor publishes an OpenAPI file, generating code directly from it inherits every upstream defect: 200-field models when your service only reads 4 fields, fields marked non-nullable in the spec that return `null` in production, and undocumented error envelopes (such as HTTP `200 OK` responses carrying `{"success": false, "reason": "..."}`). In `parsmith`, the consumer controls the contract via strict **allowlist JSON projection** (including GJSON flattening `from="data.items.#.id"`), strips undeclared keys, and routes real-world failure shapes into typed `@error` variants or `DecodeErr`.
3. **Minimal, Unified Codegen Across 9 Ecosystems**:
   Because OpenAPI supports unbounded schema composition (`oneOf`, `anyOf`, `allOf`, discriminator polymorphism, cyclic `$ref`), there is no universal, minimal OpenAPI code generator across languages—teams either rely on heavy multi-file generators (`openapi-generator`) or write custom generators per stack. By constraining `(rest)struct` to concrete wire templates, allowlist `json` schemas, and deterministic `@error` rules, `parsmith` emits a **single, zero-dependency module** with an identical monadic `Result` and transport adapter (`via_*`) contract across all 9 targets.

>[!tip]
>**Using OpenAPI as a Reference for AI Coding Agents**
>If an external service provides an OpenAPI / Swagger specification, use it as **read-only reference context** for an LLM agent rather than feeding the raw spec into an OpenAPI code generator. Pass the relevant OpenAPI endpoint slice together with [`docs/llm.txt`](llm.txt) to the agent to generate a focused `.kdl` `(rest)struct` containing only the endpoints, GJSON field projections, and `@error` discriminators your application requires, then verify it with `parsmith check`.

### `@request` Properties

| Property | Context | Description |
| :--- | :--- | :--- |
| `name="<kebab-id>"` | Required in `(rest)struct` or multi-request structs | Method identifier (`get-product` $\to$ `get_product` / `getProduct` / `GetProduct`). |
| `response="<SchemaName>"` | Required in `(rest)struct` | `json` schema used to project `2xx` responses, or `response=#null` for empty/`204` responses. |
| `doc="..."` | Optional | Method documentation emitted as a docstring, JSDoc, Go doc, or Rust doc comment. |
| `response-path="<gjson>"` | Optional | Extracts a nested JSON field before passing HTML/text to the parser or projecting the REST response. |
| `response-join="<sep>"` | Optional (HTML/`raw`) | Joins a `list[str]` extracted by `response-path` using `<sep>` (e.g. `response-join="\n"`). |

### Unwrapping HTML Inside JSON Envelopes (`response-path` & `response-join`)

Many AJAX endpoints return HTML fragments wrapped inside a JSON object or an array of HTML chunks. Use `response-path` and `response-join` on `@request` so the generated `fetch` method extracts and joins the HTML before initializing the DOM parser:

```kdl
struct AjaxProductList {
    @request response-path="payload.html_chunks" response-join="\n" #"""
    POST /api/v1/catalog/render HTTP/1.1
    Host: shop.example.com
    Content-Type: application/json

    {"category_id": {{category_id:int}}, "page": {{page:int}}}
    """#

    titles { css-all ".product-card h3"; text; trim }
}
```

---

## 2. Request Formats & Compile-Time vs Runtime Placeholders

### Supported `@request` Formats

The compiler normalizes three `@request` representations into the same internal `RequestSpec`:

#### 1. POSIX `curl` Command

Supports `-X` / `--request`, `-H` / `--header`, `-d` / `--data` / `--data-raw` / `--data-binary`, `--json`, `-F` / `--form`, `--data-urlencode`, `-u` / `--user` (Basic auth), `--url`, and automatic `Cookie` header extraction:

```kdl
struct BookPage {
    @request #"""
    curl 'https://books.example.com/catalogue/{{slug}}/index.html' \
      -H 'Accept: text/html' \
      -H 'Cookie: session={{session_id}}'
    """#

    title { css "h1"; text; trim }
}
```

#### 2. Raw `HTTP/1.1` or `HTTP/2` Request

Written as a standard HTTP request line (`METHOD /path HTTP/1.1`), headers (including `Host:`), a blank line, and an optional body:

```kdl
struct SearchPage {
    @request #"""
    GET /search?q={{query}}&page={{page:int?}} HTTP/1.1
    Host: books.example.com
    Accept: text/html
    """#

    results { css-all ".result-item a"; text; trim }
}
```

#### 3. Scalar `define` Reference

Long request templates can be declared at the top of the module with a **Scalar define** and referenced by name:

```kdl
define BASE-URL="https://api.example.com/v2"
define SEARCH-REQ="curl '{{BASE-URL}}/search?q={{query}}&limit={{limit:int?}}'"

struct SearchSnippet {
    @request SEARCH-REQ
    items { css-all "li.hit"; text; trim }
}
```

>[!warning]
> **KDL `\` Line Continuations and `//` Comments**:
> In KDL 2.0, a single-line `//` comment consumes the trailing newline. Placing a `//` comment immediately after a `\` line continuation breaks the continuation, causing subsequent request lines (`-H`, `GET`, `POST`, `curl`) to be parsed as stray struct fields (`E000` / `E200`). Always place comments **outside** multi-line `@request` continuations.

### Typed Placeholders (`{{name[:type][[]][?][|style]}}`)

Runtime request parameters are written in lowercase inside `{{...}}`:

```text
{{ name [:type] [[]] [?] [|style] }}
```

- **`name`**: Lowercase identifier matching `[a-z][a-z0-9_-]*`. Hyphens are automatically converted to `snake_case` in Python/Rust (`page-num` $\to$ `page_num`) and `camelCase` in TypeScript/JavaScript/Go (`pageNum`).
- **`type`**: Scalar primitive type: `str` (default), `int`, `float`, or `bool`.
- **`[]`**: Marks the parameter as an array (`list[T]`, `T[]`, `[]T`, `&[T]`).
- **`?`**: Marks the parameter as optional. When omitted or `None`/`null`/`undefined`, optional query parameters, headers, and cookies are omitted from the outgoing request.
- **`|style`**: Array serialization style for query and form parameters (valid only when `[]` is present; defaults to `repeat`).

#### Compile-Time `{{UPPER_NAME}}` vs Runtime `{{lower_name}}`

`parsmith` distinguishes compile-time constants from runtime method parameters strictly by case:

- **`{{UPPER_NAME}}` (`UPPER_CASE`)**: **Scalar define** interpolation evaluated at compile time inside `define` strings.
- **`{{lower_name}}` (`lowercase`)**: **Typed placeholder** turned into a runtime parameter on the generated `fetch` or REST endpoint method.

```kdl
define API-HOST="https://api.example.com"
define USER-REQ="curl '{{API-HOST}}/users/{{user-id:int}}?include={{fields:str[]?|csv}}'"
//                     ^^^^^^^^^^^^         ^^^^^^^^^^^^^           ^^^^^^^^^^^^^^^^^^^
//                     compile-time         runtime param           optional runtime array
```

>[!note]
> Compile-time `{{UPPER_NAME}}` interpolation is resolved inside `define` declarations. Referencing an `{{UPPER_NAME}}` token directly inside an inline `@request "..."` literal without composing it in a `define` first is rejected with `E002`.

#### Array Serialization Styles & Target Types

| Placeholder | Array Style | Wire Query Output (`tags = [1, 2]`) | Python Signature | TypeScript Signature | Go Signature | Rust Signature |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `{{id}}` | — | `/items/abc` | `id: str` | `id: string` | `id string` | `id: &str` |
| `{{id:int}}` | — | `/items/42` | `id: int` | `id: number` | `id int64` | `id: i64` |
| `{{q:str?}}` | — | `?q=text` *(omitted if unset)* | `q: str \| None = None` | `q?: string \| null` | `q string` | `q: Option<&str>` |
| `{{tags:int[]}}` | `repeat` *(default)* | `?tags=1&tags=2` | `tags: list[int]` | `tags: number[]` | `tags []int64` | `tags: &[i64]` |
| `{{tags:int[]\|csv}}` | `csv` | `?tags=1,2` | `tags: list[int]` | `tags: number[]` | `tags []int64` | `tags: &[i64]` |
| `{{tags:int[]\|bracket}}` | `bracket` | `?tags[]=1&tags[]=2` | `tags: list[int]` | `tags: number[]` | `tags []int64` | `tags: &[i64]` |
| `{{tags:int[]\|pipe}}` | `pipe` | `?tags=1\|2` | `tags: list[int]` | `tags: number[]` | `tags []int64` | `tags: &[i64]` |
| `{{tags:int[]?\|space}}` | `space` | `?tags=1%202` | `tags: list[int] \| None = None` | `tags?: number[] \| null` | `tags []int64` | `tags: Option<&[i64]>` |

In generated method signatures across all targets, **required parameters are always ordered before optional (`?`) parameters**, regardless of the order in which they appear in the URL, headers, or body.

#### JSON Body Placeholders & Linter Constraints (`E002`)

Inside JSON request bodies, standalone placeholders (`{"page": {{page:int}}, "tags": {{tags:str[]}}}`) preserve their native JSON types (numbers, booleans, arrays, or `null`), while placeholders embedded inside strings (`{"cursor": "page_{{page:int}}"}`) interpolate as strings.

The linter enforces the following `@request` placeholder rules (`E002`):

1. **URL path segments cannot use `[]`, `?`, or `|style`**: Path placeholders must be required scalars (`{{id}}`, `{{id:int}}`). Move optional or array parameters to the query string, headers, or body.
2. **`|style` requires `[]`**: Specifying a serialization style on a scalar placeholder (`{{id:int|csv}}`) is an error.
3. **Consistent re-use**: If the same placeholder name appears multiple times in a single `@request`, every occurrence must declare the exact same type and modifiers.
4. **JSON object keys and `curl -u`**: Placeholders are not allowed in JSON object keys (`{{{key}}: "val"}`) or inside `curl -u user:pass` flags (use an explicit `Authorization` header instead).

---

## 3. HTTP Transport Architecture (`SyncTransport` / `AsyncTransport` & `via_*` Adapters)

`parsmith` does not hardcode or branch on third-party HTTP libraries at compile time. Instead:

- **Python** and **TypeScript/JavaScript** emit a lightweight transport protocol (`HttpRequest` $\to$ `RawResponse`, raising `TransportError` on network failure) alongside zero-overhead `via_*` adapter factories that lazily import the chosen HTTP library at runtime.
- **Go** and **Rust** bind directly to their ecosystem-standard clients (`*http.Client` in Go and `&reqwest::Client` in Rust).

| Target | Transport Parameter | Built-in `via_*` Adapters / Client Type | Per-Call Request Overrides | HTML / `raw` Method Name | `(rest)struct` Method Name |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Python** *(sync)* | `transport: SyncTransport` | `via_httpx(client=None)`, `via_requests(session=None)`, `via_urllib(timeout=None)` | `**kwargs: Any` (`headers`, `params`, `cookies`, client kwargs) | `Struct.fetch(transport, ...)` | `Api.<snake_name>(transport, ...)` |
| **Python** *(async)* | `transport: AsyncTransport` | `via_httpx_async(client=None)`, `via_aiohttp(session=None)` | `**kwargs: Any` | `Struct.async_fetch(transport, ...)` | `Api.async_<snake_name>(transport, ...)` |
| **TypeScript / JS** | `transport: HttpTransport` | `viaFetch(fetchFn?)`, `viaAxios(axiosInstance)` | `extra?: Record<string, unknown>` / `RtRequestOptions` | `Struct.fetch(transport, ...)` | `Api.<camelName>(transport, ...)` & `Api.<snake_name>(...)` |
| **Go** | `client *http.Client` (`nil` $\to$ `http.DefaultClient`) | Standard `*http.Client` | `opts ...RtHttpOption` (`WithHeader`, `WithParams`, `WithTimeout`, ...) | `New<Struct>Fetch(ctx, client, ..., opts...)` | `New<Struct>().<PascalName>(client, ..., opts...)` |
| **Rust** | `client: &reqwest::Client` | Standard `&reqwest::Client` | Configured on `reqwest::Client` (or `_with_opts` extensions) | `<Struct>Parser::fetch(client, ...)` | `<Struct>::<snake_name>(client, ...)` / `<Struct>::<snake_name>_with_opts(client, ..., opts)` |

### Python Transports & `--http-io`

In Python, `parsmith generate python` accepts `--http-io both|sync|async` (default `both`):

- `via_httpx(client=None)`: Wraps an `httpx.Client` (or creates a per-request client when `None`).
- `via_httpx_async(client=None)`: Wraps an `httpx.AsyncClient` (or creates a per-request async client when `None`).
- `via_requests(session=None)`: Wraps a `requests.Session` (or creates a per-request session when `None`).
- `via_aiohttp(session=None)`: Wraps an `aiohttp.ClientSession` (or creates a per-request session when `None`).
- `via_urllib(timeout=None)`: Uses Python's standard library `urllib.request` with zero third-party dependencies.

Custom transports can be implemented by providing any object with `send(self, req: HttpRequest) -> RawResponse` (or `async def send(self, req: HttpRequest) -> RawResponse`) that raises `TransportError(cause)` on network failure.

### TypeScript / JavaScript Transports & Calling Conventions

In TypeScript and JavaScript:

- `viaFetch(fetchFn?)`: Uses `globalThis.fetch` by default, or a custom fetch implementation (such as `undici.fetch` or a mocked test function).
- `viaAxios(axiosInstance)`: Wraps any `AxiosInstance` (`AxiosLike`), automatically setting `validateStatus: () => true` and `responseType: "text"` so status codes and raw response bodies pass cleanly to the `parsmith` dispatcher.

Generated TS/JS methods support **two overload styles** for endpoints with placeholders:

```ts
// 1. Positional arguments + optional extra overrides
const r1 = await CatalogApi.searchItems(viaFetch(), 42, "laptop", ["sale"], {
  headers: { "X-Trace-Id": "abc" },
});

// 2. Named parameter object (accepts both camelCase and snake_case keys)
const r2 = await CatalogApi.searchItems(
  viaFetch(),
  { itemId: 42, query: "laptop", tags: ["sale"] },
  { headers: { "X-Trace-Id": "abc" } },
);
```

### Go `*http.Client` & `RtHttpOption` Functional Options

In Go, passing `nil` as `client` automatically uses `http.DefaultClient`. Per-call overrides are passed via variadic `RtHttpOption` functions without mutating the DSL defaults:

- `WithHeader(key, val string) RtHttpOption` and `WithHeaders(headers map[string]string) RtHttpOption`
- `WithParam(key, val string) RtHttpOption` and `WithParams(params map[string]string) RtHttpOption`
- `WithCookie(name, val string) RtHttpOption` and `WithCookies(cookies map[string]string) RtHttpOption`
- `WithTimeout(d time.Duration) RtHttpOption`

Note the Go constructor convention:

- **HTML / `(raw)struct`**: Emits a package-level constructor `New<Struct>Fetch(ctx, client, ..., opts...)` (or `New<Struct><PascalName>` when `name="<kebab-id>"` is set on `@request`).
- **`(rest)struct`**: Emits a stateless client struct `New<Struct>()` with receiver methods `<PascalName>(client, ..., opts...)`.

### Rust `&reqwest::Client`

In Rust, generated async methods accept `client: &reqwest::Client`:

- **HTML / `(raw)struct`**: `<Struct>Parser::fetch(client, ...)` (or `<Struct>Parser::fetch_<snake_name>(client, ...)`) fetches the body, extracts `response-path` if configured, and calls `<Struct>Parser::new(body)`.
- **`(rest)struct`**: `<Struct>::<snake_name>(client, ...)` (and `<Struct>::<snake_name>_with_opts(client, ..., opts)` when per-call options are used) executes the request and dispatches the response into `Result<T, <Struct>Error>`.

---

## 4. `(rest)struct`, `@error[:<suffix>]` Discriminators & Monadic `Result` Contract

A `(rest)struct` models a typed REST API client whose methods never throw exceptions on HTTP status errors, network failures, or JSON schema mismatches. Every call returns a discriminated `Result` value.

### Monadic `Result` Variants Across Targets

| Outcome | When Returned | Python | TypeScript / JavaScript | Go (`(T, error)`) | Rust (`Result<T, <Struct>Error>`) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`Ok[T]`** | HTTP `200..299` (and no `@error 200` rule matched), projected through `<Schema>Json` | `Ok(status, headers, value, ok=True)` (`r.is_ok == True`) | `Ok<T>` (`{ ok: true, kind: "ok", status, headers, value }`) | `(*<Schema>Json, nil)` / `([]<Schema>Json, nil)` / `(struct{}{}, nil)` | `Ok(T)` (`<Schema>Json`, `Vec<<Schema>Json>`, or `()`) |
| **`<Struct>Err<Status>[<Suffix>]`** | Matched an `@error[:<suffix>]` rule and projected through `<ErrSchema>Json` | `<Struct>Err<Status>[<Suffix>](headers, value, status, ok=False)` | `<Struct>Err<Status>[<Suffix>]` (`{ ok: false, kind: "error", variant: "...", status, headers, value }`) | `*<Struct>Err<Status>[<Suffix>]{Status, Body}` | `Err(<Struct>Error::Err<Status>[<Suffix>](<ErrSchema>Json))` |
| **`DecodeErr`** | HTTP response arrived (`2xx` or matched `@error`), but body is invalid JSON or failed its `JsonDef` projector (`RtJsonError` / `rt::RtError`) | `DecodeErr(status, headers, body, cause, exception, ok=False)` | `DecodeErr` (`{ ok: false, kind: "decode", status, headers, body, cause, exception }`) | `*DecodeErr{Status, Body, Cause}` | `Err(<Struct>Error::Decode(u16, rt::RtError))` |
| **`UnknownErr`** | Unmatched non-`2xx` HTTP status code | `UnknownErr(status, headers, body, ok=False)` (`r.value` aliases `r.body`) | `UnknownErr` (`{ ok: false, kind: "unknown", status, headers, body }`) | `*UnknownErr{Status, Body}` | `Err(<Struct>Error::Unknown(u16, serde_json::Value))` |
| **`TransportErr`** | Network, DNS, TLS, or timeout error before receiving an HTTP response | `TransportErr(status=0, headers={}, cause, exception, ok=False)` | `TransportErr` (`{ ok: false, kind: "transport", status: 0, cause, exception }`) | `*TransportErr{Cause}` | `Err(<Struct>Error::Transport(reqwest::Error))` |

### Error Discriminator (`@error[:<suffix>]`) Syntax & Semantics

```kdl
@error[:<suffix>] <status:100..599> <SchemaName> ["pos.key"...] [prop.key=val ...]
```

- **`<status>`**: Integer HTTP status code in `100..599` (`E002` if out of range).
- **`<SchemaName>`**: Reference to a `json` schema declared above (`E300` / `E302`).
- **Positional GJSON path existence checks (`"pos.key"`)**: Strings after `<SchemaName>` verify that the given flat key or nested GJSON dot-path (including array indices) exists and is non-null in the response JSON object:
  ```kdl
  @error 404 ApiError "error.detail.0.code"
  ```
- **Property equality checks (`prop.key=val`)**: KDL key-value properties verify that a flat key or nested dot-path equals a specific literal (`#true`, `#false`, `#null`, `int`, `float`, or `"string"`):
  ```kdl
  @error 200 ActionError success=#false "detail.reason"
  ```

All positional existence checks and property equality checks on a single `@error` rule combine via logical `AND`. Specifying the same key as both a positional argument and a property on the same `@error` is rejected with `E400`.

#### Automatic Variant Naming vs Explicit `@error:<suffix>`

1. **Automatic Naming (no `:suffix`)**:
   The compiler extracts the last non-numeric leaf segment from each positional path and property key in declaration order, converts up to the **first 2 leaf keys** to `PascalCase`, and appends them to `<Struct>Err<Status>`:
   - `@error 404 ApiError` in `CatalogApi` $\to$ `CatalogApiErr404` (`CatalogApiError::Err404` in Rust)
   - `@error 200 ActionError success=#false` $\to$ `CatalogApiErr200Success`
   - `@error 404 ApiError "error.detail.0.code"` $\to$ `CatalogApiErr404Code`
   - `@error 400 ApiError "a.first" "b.second" third="x"` $\to$ `CatalogApiErr400FirstSecond` (capped at 2 leaf keys)

2. **Explicit Suffix (`@error:<suffix>`)**:
   When `:suffix` is provided (matching `^[A-Za-z_][A-Za-z0-9_-]*$`, `E002` otherwise), the variant name is `<Struct>Err<Status><PascalSuffix>` regardless of how many key checks are present:
   - `@error:not-found 404 ApiError error="not_found"` $\to$ `CatalogApiErr404NotFound` (`CatalogApiError::Err404NotFound` in Rust)

3. **Collision Detection (`E400`)**:
   If two `@error` rules in the same `(rest)struct` resolve to the same variant name (for example, `@error 404 ApiError "error"` and `@error 404 ApiError error="missing"` both resolving to `<Struct>Err404Error`), `parsmith check` emits `E400` and prompts you to disambiguate them with `@error:<suffix>`.

#### Safe Runtime Fallthrough & `DecodeErr`

`@error` rules are evaluated in exact declaration order—including `@error 200 ...` rules, which run **before** `2xx` success projection so APIs that return HTTP `200` with `{"success": false}` are routed to the error variant first.

If an incoming response matches a rule's HTTP status and key/value conditions, the runtime attempts to project the body through `<SchemaName>`:

- If projection succeeds, the typed `<Struct>Err<Status>[<Suffix>]` variant is returned immediately.
- If the body is non-JSON (such as an upstream CDN returning an HTML `502`/`404` gateway page) or fails `<SchemaName>` projection, the dispatcher **does not raise an uncaught exception**; it records the decode failure and falls through to test subsequent `@error` rules.
- If a rule matched by status and conditions but all matching rules failed schema projection, `DecodeErr` is returned with the underlying `RtJsonError` / `rt::RtError` cause.
- If no `@error` rule matched a non-`2xx` status, `UnknownErr` is returned.

### End-to-End `(rest)struct` Example Across Targets

Consider the following KDL schema (`catalog_api.kdl`):

```kdl
define API-BASE="https://api.example.com/v1"
define GET-PRODUCT-REQ="curl '{{API-BASE}}/products/{{id:int}}?currency={{currency:str?}}' -H 'Accept: application/json'"

json Product {
    id int
    title str
    price float
}

json ApiError {
    code str from="error.code"
    message str from="error.message"
}

json ActionError {
    success bool
    reason str
}

(rest)struct CatalogApi {
    @doc "Typed client for the Catalog service."

    @request name="get-product" response="Product" GET-PRODUCT-REQ

    @request name="delete-product" response=#null #"""
    DELETE /v1/products/{{id:int}} HTTP/1.1
    Host: api.example.com
    """#

    @error:not-found 404 ApiError "error.code"
    @error 200 ActionError success=#false
}
```

#### Python

```python
import httpx
from catalog_api import (
    CatalogApi,
    CatalogApiErr200Success,
    CatalogApiErr404NotFound,
    DecodeErr,
    TransportErr,
    UnknownErr,
    via_httpx,
)

with httpx.Client() as client:
    transport = via_httpx(client)
    res = CatalogApi.get_product(transport, id=101, currency="USD")

    if res.is_ok:
        print(res.value["title"], res.value["price"])
    elif isinstance(res, CatalogApiErr404NotFound):
        print("Not found:", res.value["code"], res.value["message"])
    elif isinstance(res, CatalogApiErr200Success):
        print("Rejected:", res.value["reason"])
    elif isinstance(res, DecodeErr):
        print("Schema mismatch on HTTP", res.status, res.cause)
    elif isinstance(res, TransportErr):
        print("Network error:", res.cause)
    elif isinstance(res, UnknownErr):
        print("Unexpected HTTP", res.status, res.body)
```

#### TypeScript

```ts
import { CatalogApi, viaFetch } from "./catalog_api.ts";

const res = await CatalogApi.getProduct(viaFetch(), { id: 101, currency: "USD" });

if (res.ok) {
  console.log(res.value.title, res.value.price);
} else {
  switch (res.kind) {
    case "error":
      if (res.variant === "CatalogApiErr404NotFound") {
        console.error("Not found:", res.value.code, res.value.message);
      } else if (res.variant === "CatalogApiErr200Success") {
        console.error("Rejected:", res.value.reason);
      }
      break;
    case "decode":
      console.error("Schema mismatch on HTTP", res.status, res.cause);
      break;
    case "transport":
      console.error("Network error:", res.cause);
      break;
    case "unknown":
      console.error("Unexpected HTTP", res.status, res.body);
      break;
  }
}
```

#### Go

```go
package main

import (
	"errors"
	"fmt"
	"time"
)

func main() {
	api := NewCatalogApi()
	product, err := api.GetProduct(nil, 101, "USD", WithTimeout(5*time.Second))
	if err != nil {
		var notFound *CatalogApiErr404NotFound
		var actionErr *CatalogApiErr200Success
		var decodeErr *DecodeErr
		var transportErr *TransportErr
		var unknownErr *UnknownErr

		switch {
		case errors.As(err, &notFound):
			fmt.Println("Not found:", notFound.Body.Code, notFound.Body.Message)
		case errors.As(err, &actionErr):
			fmt.Println("Rejected:", actionErr.Body.Reason)
		case errors.As(err, &decodeErr):
			fmt.Println("Decode error:", decodeErr.Status, decodeErr.Cause)
		case errors.As(err, &transportErr):
			fmt.Println("Transport error:", transportErr.Cause)
		case errors.As(err, &unknownErr):
			fmt.Println("Unexpected status:", unknownErr.Status, unknownErr.Body)
		}
		return
	}
	fmt.Println(product.Title, product.Price)
}
```

#### Rust

```rust
use reqwest::Client;

#[tokio::main]
async fn main() {
    let client = Client::new();
    match CatalogApi::get_product(&client, 101, Some("USD")).await {
        Ok(product) => println!("{} (${})", product.title, product.price),
        Err(CatalogApiError::Err404NotFound(err)) => {
            eprintln!("Not found: {} - {}", err.code, err.message);
        }
        Err(CatalogApiError::Err200Success(err)) => {
            eprintln!("Rejected: {}", err.reason);
        }
        Err(CatalogApiError::Decode(status, err)) => {
            eprintln!("Decode error on HTTP {status}: {err}");
        }
        Err(CatalogApiError::Transport(err)) => {
            eprintln!("Transport error: {err}");
        }
        Err(CatalogApiError::Unknown(status, body)) => {
            eprintln!("Unexpected HTTP {status}: {body}");
        }
    }
}
```

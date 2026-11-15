# `@request`, `@error[:<suffix>]`, Monadic `Result` & 9-Target Codegen (DSL v2.1)

## 1. `@request` Formats & Properties

`@request` accepts a raw `HTTP/1.1` / `HTTP/2` request string, a POSIX `curl` command, or a scalar `define` constant name.
- **On HTML / `(raw)struct`**: emits transport-agnostic `fetch` / `async_fetch` constructors that execute the HTTP request, optionally extract `response-path` (joining `list[str]` via `response-join`), and instantiate the parser.
- **On `(rest)struct`**: emits typed endpoint methods that **never throw** on HTTP, network, or JSON projection failures, returning a monadic `Result` (`Ok`, `<Struct>Err<Status>[<Suffix>]`, `DecodeErr`, `UnknownErr`, or `TransportErr`).

| Property | In `(rest)struct` | In HTML / `(raw)struct` | Semantics |
| :--- | :--- | :--- | :--- |
| `name="<kebab-id>"` | Mandatory (`E401`) | Optional on single `@request` (default `fetch`); required if $>1$ | Emits `snake_case` (`py`/`rs`/`ex`), `camelCase` (`ts`/`js`/`php`/`kt`), `PascalCase` (`go`/`cs`) |
| `response="<Schema>"\|#null` | Mandatory (`E401`) | Forbidden (`E203`) | `2xx` JSON schema (`Ok[<Schema>Json]`), or `response=#null` for empty `204` (`Ok[None]` / `()`) |
| `doc="<text>"` | Optional | Optional | Method docstring / JSDoc / Go doc / Rust doc / ExDoc |
| `response-path="<gjson>"` | Optional | Optional | Extracts nested GJSON path (e.g. `"data.items"`) from JSON response envelope before parsing/projection |
| `response-join="<sep>"` | **Forbidden (`E001`)** | Optional (requires `response-path`) | Joins `list[str]` extracted by `response-path` into a single HTML/text string |

Supported `curl` flags: `-X/--request`, `-H/--header`, `-d/--data/--data-raw/--data-binary`, `--json`, `-u/--user`, `-F/--form`, `--data-urlencode`, `--url`, `--compressed` (ignored).

### Scalar `define` Composition & Line Continuation Rules
- Scalar defines may interpolate earlier scalar defines via `{{UPPER_NAME}}` while keeping lowercase `{{param}}` as runtime parameters:
  ```kdl
  define API-HOST="https://api.example.com"
  define GET-USER="curl '{{API-HOST}}/users/{{id:int}}?fields={{fields:str[]?|csv}}'"
  ```
- **`E002` rule**: Uppercase `{{UPPER_NAME}}` is **forbidden** directly inside inline `@request #"""..."""#` string literals — compose in a top-level `define` first and pass the define identifier to `@request`.
- **`E000`/`E203` rule**: Keep `//` single-line comments **outside** multi-line `\` continuations—in KDL 2.0, `//` consumes the trailing newline and breaks continuation.

---

## 2. Typed Placeholders (`{{name[:type][[]][?][|style]}}`)

- `name`: lowercase `^[a-z][a-z0-9_-]*$` (`-` converts to `_` in `py`/`rs`/`ex`/`php` and `camelCase` in `ts`/`js`/`go`/`cs`/`kt`).
- `type`: `str` (default) | `int` | `float` | `bool`.
- `[]`: array parameter (forbidden in URL path, `E002`).
- `?`: optional parameter, omitted from `params`, `headers`, and `cookies` when `None`/`null`/`undefined` (forbidden in URL path, `E002`). Inside JSON request bodies, `None`/`null` is preserved as `"key": null`.
- `|style` (arrays only, requires `[]`, `E002`; default `repeat`):
  - `repeat`: `?k=1&k=2`
  - `csv`: `?k=1,2`
  - `bracket`: `?k[]=1&k[]=2`
  - `pipe`: `?k=1|2`
  - `space`: `?k=1%202`
- Repeated occurrences of the same `name` in one `@request` must use the **exact same full spec** (`E402`), and must not collide with un-escapable target language keywords (`E403`).

---

## 3. `@error[:<suffix>]` Dispatch & Safe Fallthrough (`(rest)struct` only)

Syntax: `@error[:<suffix>] <status:100..599> <SchemaName> ["pos.key"...] [prop.key=val ...]`

- **Explicit suffix (`@error:<suffix>`)**: `<suffix>` must match `^[A-Za-z_][A-Za-z0-9_-]*$` (`E002`). Generates `<PascalStruct>Err<Status><PascalSuffix>` (or `<PascalStruct>Error::Err<Status><PascalSuffix>` in Rust). Prefer explicit `:suffix` (`@error:rate-limit 429 RateLimitError`, `@error:not-found 404 ApiError`) for readable variant names and to prevent `E400` collisions.
- **Auto-naming (no `:suffix`)**: extracts the last non-numeric leaf segment from at most the first **2** keys (`required_keys` then `conditions`), e.g. `"data.errors.0.detail"` $\to$ `<Struct>Err404Detail`.
- **Collision & duplicate key lint (`E400`)**: duplicate resolved variant names in the same `(rest)struct` or repeating the same key in both positional args and properties fails `parsmith check` with `E400`.
- **Conditions, `@error 200`, & `DecodeErr` fallthrough**: positional args check GJSON path existence (non-null); properties check literal equality (`#true`, `#false`, `#null`, `int`, `float`, `"str"`), combined via logical `AND`. Rules evaluate in exact declaration order—including `@error 200 ...` rules **before** `2xx` success projection. If a response matches a rule's status and conditions but fails `<SchemaName>` projection, the dispatcher records the cause and tests subsequent `@error` rules; if all matching rules fail projection (or a `2xx` response fails its `response=` schema), it returns **`DecodeErr`** with the underlying `RtJsonError` / `rt::RtError` cause.

---

## 4. Monadic `Result` Contract & 9-Target Transport Adapters

`parsmith` does **not** use a compile-time `--http-client` flag. All `@request` and `(rest)struct` methods are emitted unconditionally and accept a lightweight `SyncTransport` / `AsyncTransport` adapter (`via_*`) or native client:

| Outcome | When Returned | Python | TS / JS | Go (`(T, error)`) | Rust (`Result<T, <Struct>Error>`) | Elixir (`{:ok, T} \| {:error, E}`) | PHP / C# / Kotlin |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`Ok[T]`** | HTTP `200..299` (no `@error 200` matched), projected via `<Schema>Json` | `Ok(status, headers, value, ok=True)` (`r.is_ok`) | `Ok<T>` (`{ ok: true, kind: "ok", status, headers, value }`) | `(*<Schema>Json, nil)` / `([]<Schema>Json, nil)` | `Ok(T)` | `{:ok, %<Schema>Json{}}` | `Ok` (`$res->isOk()`, `res.IsOk`, `res.isOk`) |
| **`<Struct>Err<Status>[<Suffix>]`** | Matched `@error[:<suffix>]` and projected via `<ErrSchema>Json` | `<Struct>Err<Status>[<Suffix>](headers, value, status, ok=False)` | `{ ok: false, kind: "error", variant: "...", status, headers, value }` | `*<Struct>Err<Status>[<Suffix>]{Status, Body}` | `Err(<Struct>Error::Err<Status>[<Suffix>](<ErrSchema>Json))` | `{:error, %<Struct>Err<Status>[<Suffix>]{value: ...}}` | `<Struct>Err<Status>[<Suffix>]` |
| **`DecodeErr`** | HTTP response arrived (`2xx` or matched `@error`), but JSON parse or `JsonDef` projector failed | `DecodeErr(status, headers, body, cause, exception, ok=False)` | `{ ok: false, kind: "decode", status, headers, body, cause, exception }` | `*DecodeErr{Status, Body, Cause}` | `Err(<Struct>Error::Decode(u16, rt::RtError))` | `{:error, %Rt.DecodeErr{status, body, cause}}` | `DecodeErr` (`Rt.DecodeErr`) |
| **`UnknownErr`** | Unmatched non-`2xx` HTTP status | `UnknownErr(status, headers, body, ok=False)` | `{ ok: false, kind: "unknown", status, headers, body }` | `*UnknownErr{Status, Body}` | `Err(<Struct>Error::Unknown(u16, Value))` | `{:error, %Rt.UnknownErr{status, body}}` | `UnknownErr` (`Rt.UnknownErr`) |
| **`TransportErr`** | Network, DNS, TLS, or timeout failure | `TransportErr(status=0, headers={}, cause, exception, ok=False)` | `{ ok: false, kind: "transport", status: 0, cause, exception }` | `*TransportErr{Cause}` | `Err(<Struct>Error::Transport(reqwest::Error))` | `{:error, %Rt.TransportErr{cause}}` | `TransportErr` (`Rt.TransportErr`) |

### Target CLI Commands & Transport Factories

| Target | CLI Subcommand | Transport Factory / Client Argument | Key Target Flags |
| :--- | :--- | :--- | :--- |
| **Python** | `parsmith generate python` (`py`) | `via_httpx(client)`, `via_httpx_async(client)`, `via_requests(session)`, `via_aiohttp(session)`, `via_urllib()` | `-L bs4\|lxml\|parsel\|selectolax`, `--models typeddict\|dataclass`, `--http-io both\|sync\|async`, `-R` |
| **TypeScript** | `parsmith generate ts` (`typescript`) | `viaFetch(fetchFn?)`, `viaAxios(instance)` | `-o <file.ts\|dir>` |
| **JavaScript** | `parsmith generate js` (`javascript`) | `viaFetch(fetchFn?)`, `viaAxios(instance)` | `-o <file.js\|dir>` (emits `.js` + `.d.ts`) |
| **Go** | `parsmith generate go` (`golang`) | `*http.Client` (`nil` uses `http.DefaultClient`) + `WithHeader`, `WithParam`, `WithCookie`, `WithTimeout` | `--package <pkg>` (default `main`), `-R` |
| **Rust** | `parsmith generate rust` (`rs`) | `&reqwest::Client` + `rt::HttpOption` (`_with_opts`) | `-R`, `--runtime-name <stem>` |
| **Elixir** | `parsmith generate elixir` (`ex`) | `Rt.via_httpc(opts \\ [])` (OTP `:httpc`), `Rt.via_req(req_opts \\ [])` | `--package <ModulePrefix>`, `-R` |
| **PHP 8.1+** | `parsmith generate php` | `Rt::viaCurl()`, `Rt::viaStream()`, `Rt::viaGuzzle($client)` | `-L native\|dom-crawler`, `--package <Namespace>`, `-R` |
| **C# (.NET 8+)** | `parsmith generate csharp` (`cs`) | `System.Net.Http.HttpClient` + `Rt.WithHeader`, `Rt.WithParam`, `Rt.WithCookie`, `Rt.WithTimeout` | `--package <Namespace>`, `-R` |
| **Kotlin (JVM)** | `parsmith generate kotlin` (`kt`) | `java.net.http.HttpClient` + `Rt.withHeader`, `Rt.withParam`, `Rt.withCookie`, `Rt.withTimeout` | `--package <pkg>`, `-R` |

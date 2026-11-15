# parsmith Documentation

This directory contains the user guide, language reference, runtime specification, and architectural documentation for **`parsmith`** and the **KDL 2.0 Schema DSL (v2.1)**.

## Recommended Reading Order

1. **Onboarding**
   - Start with [Quickstart (`quickstart.md`)](quickstart.md) to understand the pipeline mental model, compile your first `.kdl` schema across all nine targets, and use the Decision Matrix to pick the right schema construct.
   - Read [Syntax & File Structure (`syntax.md`)](syntax.md) for a KDL 2.0 primer, all seven struct kinds (`item`, `list`, `dict`, `table`, `flat`, `raw`, `rest`), module-level functions (`fn`, `(raw)fn`), struct directives, `define` reuse, `import` modularity, and the top-down declaration order rule (`E302`).
2. **Core Language Reference**
   - [Type System & Portable Regex Subset (`types.md`)](types.md) — `TypeInfo` base types and modifiers (`T[]`, `T?`, `dict[K, V]`), implicit array mapping, `fallback` type rules, and cross-target regular expression rules.
   - [Pipeline Operations (`operations.md`)](operations.md) — Complete reference for CSS/XPath selectors, block pattern-matching selectors, DOM extraction, string/regex/array transforms, conversions, `nested`, `jsonify`, and control steps.
   - [Condition Expressions (`conditions.md`)](conditions.md) — `Cond` containers (`filter`, `assert`, `match`), boolean combinators (`and`, `or`, `not`), leaf condition operators, and `RtAssertionError` runtime behavior.
   - [Typed User Extensions (`extensions.md`)](extensions.md) — Custom target-native pipeline operations (`extension`, `!Namespace.op`, `sig`, `helper`, `emit`).
3. **JSON & HTTP/REST**
   - [JSON Schemas & GJSON Projection (`json.md`)](json.md) — `json`, `(array)json`, `(dict)json`, inline JSON schemas, `@value` blocks, synthesized schema names, GJSON path expressions (`\.`, `#`, `#.subpath`), allowlist JSON projectors, and `parsmith json-to-kdl`.
   - [HTTP Requests & REST Clients (`http-rest.md`)](http-rest.md) — `@request` templates (`curl` and raw HTTP), typed placeholders (`{{name:type[]?|style}}`), `(rest)struct`, error discriminators (`@error[:<suffix>]`), monadic `Result` & `DecodeErr`, and `via_*` transport adapters.
4. **Tooling, Recipes & Diagnostics**
   - [CLI & Host SDKs (`cli-and-sdk.md`)](cli-and-sdk.md) — Native CLI (`check`, `generate`, `json-to-kdl`, `version`) and Python/JS Host SDKs (`parsmith-py`, `parsmith-js`) with in-memory VFS (`sources`) and `inspect_schema` / `inspectSchema`.
   - [End-to-End Extraction Recipes (`recipes.md`)](recipes.md) — Self-contained examples (`Input -> KDL -> Typed Output`) for every schema kind.
   - [Compiler & Linter Diagnostics (`diagnostics.md`)](diagnostics.md) — Complete reference for error codes `E000`–`E403` and warnings `W011`–`W041`.
5. **Agent & Domain Docs**
   - [Single-File LLM Reference (`llm.txt`)](llm.txt) — Compact specification of DSL v2.1, CLI, SDK, and 9-target runtime contracts designed for AI coding agents.
   - [Domain Context & Glossary (`../CONTEXT.md`)](../CONTEXT.md) — Canonical ubiquitous language for the `parsmith` compiler and runtime.
   - [Architecture Decision Records (`adr/`)](adr/) — Design rationales for CSS/regex validation, DOM spellings, REST lowering, Host SDKs, WASM playground, CSS-to-XPath lowering, and the `rt` runtime namespace.

## Documentation Map

| File | Category | Scope |
| :--- | :--- | :--- |
| [`quickstart.md`](quickstart.md) | Onboarding | Pipeline mental model, first schema walkthrough (`check` $\to$ `generate` $\to$ run across Python, TS, JS, Go, Rust, Elixir, PHP, C#, and Kotlin), and construct decision matrix. |
| [`syntax.md`](syntax.md) | Onboarding | KDL 2.0 crash course, struct kinds (`item`, `list`, `dict`, `table`, `flat`, `raw`, `rest`), `fn`/`(raw)fn`, struct directives, `Scalar define` & `Block define`, `import`, and `E302` declaration order. |
| [`types.md`](types.md) | Language Reference | `TypeInfo` (`BaseType`, `T[]`, `T?`, `dict[K, V]`, `AUTO`), implicit array mapping, `fallback` rules, and Portable regex subset. |
| [`operations.md`](operations.md) | Language Reference | Pipeline operations reference (selectors, block pattern-matching, extract, string, regex, array, conversions, `nested`, `jsonify`, control ops) and backend rules (`E203` XPath). |
| [`conditions.md`](conditions.md) | Language Reference | `Cond` containers (`filter`, `assert`, `match`), combinators (`and`, `or`, `not`), string/attr/text/DOM/length/regex-list/numeric conditions, and `RtAssertionError`. |
| [`json.md`](json.md) | JSON & HTTP/REST | `json`, `(array)json`, `(dict)json`, inline JSON schemas, inline `@value` blocks, synthesized schema names, GJSON paths (`\.`, `#`, `#.subpath`), allowlist JSON projectors, `RtJson*Error`, and `json-to-kdl`. |
| [`http-rest.md`](http-rest.md) | JSON & HTTP/REST | `@request` (curl & raw HTTP, `response-path`, `response-join`), typed placeholders (`{{name:type[]?\|style}}`), `(rest)struct`, `@error[:<suffix>]`, monadic `Result` & `DecodeErr`, and `via_*` transport adapters across all 9 targets. |
| [`extensions.md`](extensions.md) | Language Reference | `extension` declarations, `!Namespace.op` calls, `sig` types & `T`, `emit` placeholders, lazy `helper` & `import` blocks across targets. |
| [`cli-and-sdk.md`](cli-and-sdk.md) | Tooling | `parsmith` CLI (`check`, `generate`, `json-to-kdl`, `version`) and Host SDKs (`parsmith-py` and `parsmith-js` with `MemorySourceLoader` VFS and `inspect_schema` / `inspectSchema`). |
| [`recipes.md`](recipes.md) | Recipes | Self-contained end-to-end recipes (`Input -> KDL -> Output`) for `item`, `list`, `table`, `dict`, `flat`, `(raw)struct`, embedded `jsonify`, and `(rest)struct`. |
| [`diagnostics.md`](diagnostics.md) | Diagnostics | Complete reference for `E000`–`E403` and `W011`–`W041` with causes, broken examples, and fixes. |
| [`llm.txt`](llm.txt) | Agent Reference | Compact, single-file specification of `parsmith` DSL v2.1, CLI, SDK, and 9-target runtime contracts for LLM context windows. |
| [`../CONTEXT.md`](../CONTEXT.md) | Domain Model | Canonical vocabulary for KDL schemas, compiler passes, codegen abstractions, and runtime contracts. |
| [`adr/`](adr/) | Architecture | Architecture Decision Records (`0001` through `0011`). |

## Target Capabilities Overview

| Capability | Python (`python`) | TypeScript (`ts`) | JavaScript (`js`) | Go (`go`) | Rust (`rust`) | Elixir (`elixir`) | PHP 8.1+ (`php`) | C# (`csharp`) | Kotlin (`kotlin`) |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **DOM Backends** | `bs4` (default), `lxml`, `parsel`, `selectolax` | Web DOM (`DOMParser`, `querySelectorAll`) | Web DOM (`DOMParser`, `querySelectorAll`) | `goquery` | `dom_query` | `Floki` | `native` (`\DOMDocument` + `\DOMXPath`, default) or `dom-crawler` | `AngleSharp` + `AngleSharp.XPath` | `org.jsoup:jsoup` |
| **CSS3/4 Selectors** | Supported (all 4 backends) | Supported | Supported | Supported | Supported | Supported (CSS3) | CSS3 lowered to XPath 1.0 in `native` (`E002` on CSS4); CSS3/4 in `dom-crawler` | Supported | Supported |
| **XPath 1.0 (`xpath*`)** | Supported in `lxml` and `parsel`; rejected in `bs4` and `selectolax` (`E203`) | Supported (`document.evaluate`) | Supported (`document.evaluate`) | Rejected at compile time (`E203`) | Rejected at compile time (`E203`) | Rejected at compile time (`E203`) | Supported in both `native` and `dom-crawler` | Supported (`AngleSharp.XPath`) | Supported (`Jsoup` XPath) |
| **Output Models** | `TypedDict` (default) or `dataclass` (`--models`) | `export interface <Name>Type` | Plain objects + `.d.ts` `interface <Name>Type` | `type <Name>Type struct` | `pub struct <Name>Type` (`#[derive(Serialize)]`) | `defmodule <Name>Type` (`defstruct`) | `readonly class <Name>Type` (`\JsonSerializable`) | `public sealed record <Name>Type` | `data class <Name>Type` |
| **JSON Projection** | `rt_project_<schema>` allowlist projector | `rtProject<Schema>` allowlist projector | `rtProject<Schema>` + `.d.ts` | `UnmarshalJSON` via `gjson` allowlist projector | `rt::project_<schema>` via `serde_json` | `Rt.project_<schema>` / `<Schema>Json.project/1` | `Rt::project<Schema>` allowlist projector | `Rt.Project<Schema>` via `System.Text.Json` | `Rt.project<Schema>` via `kotlinx.serialization` |
| **HTTP / REST Client** | `SyncTransport` / `AsyncTransport` (`via_httpx`, `via_httpx_async`, `via_requests`, `via_aiohttp`, `via_urllib`) | `SyncTransport` / `AsyncTransport` (`viaFetch`, `viaAxios`) | `SyncTransport` / `AsyncTransport` (`viaFetch`, `viaAxios`) | `*http.Client` (`net/http`) + `With*` options | `&reqwest::Client` (async) + `rt::HttpOption` | OTP `:httpc` (`Rt.via_httpc/1`) + `Rt.via_req/1` | `SyncTransport` (`Rt::viaCurl()`, `Rt::viaStream()`, `Rt::viaGuzzle()`) | `HttpClient` + `Rt.HttpOption` (`Rt.With*`) | `java.net.http.HttpClient` + `Rt.HttpOption` (`Rt.with*`) |
| **Runtime Namespace** | `rt_*`, `Rt*Error`, `RT_*` (inline or `-R` separate module) | Exported `rt*`, `Rt*Error`, `RT_*` | Exported `rt*`, `Rt*Error`, `RT_*` + `.d.ts` | `rt*`, `Rt*` (inline or `-R` separate package file) | `pub mod rt` / `rt::RtError` (inline or `-R` separate module) | `defmodule Rt` (inline or `-R` separate module) | `final class Rt` (inline or `-R` separate file) | `public static partial class Rt` (inline or `-R` separate file) | `object Rt` (inline or `-R` separate file) |

## Architecture Decision Records (`docs/adr/`)

- [ADR-0001: Formal CSS Grammar Validation and Portable Regex Subset](adr/0001-formal-css-grammar-and-portable-regex-subset.md)
- [ADR-0002: Reject XPath Operations in BeautifulSoup (`Bs4Spelling`)](adr/0002-reject-xpath-in-bs4-dom-spelling.md)
- [ADR-0003: Language-Agnostic `ModuleBuilder`, Hybrid Runtime Templates, and Direct REST Lowering](adr/0003-language-agnostic-module-builder-and-direct-rest-lowering.md)
- [ADR-0004: Unified Runtime and Type Contracts Across Python `DomSpelling` Backends](adr/0004-unified-python-dom-spelling-runtime-contracts.md)
- [ADR-0005: Loopback HTTP Server and Structural HTML Fixtures for Cross-Target E2E Testing](adr/0005-loopback-http-e2e-and-structural-fixtures.md)
- [ADR-0006: Go and Rust Target Codegen, Inline vs Separate Runtime, and Native REST Contracts](adr/0006-go-and-rust-codegen-runtime-and-rest-contracts.md)
- [ADR-0007: JS/TS Node-API (`napi-rs`) Host SDK, Hybrid Native Loader, and Dual Serialization Contracts](adr/0007-js-ts-napi-host-sdk-and-dual-serialization-contracts.md)
- [ADR-0008: Browser Playground via Rust WASM (`parsmith-wasm`) and Native Browser DOM Execution](adr/0008-wasm-browser-playground-and-native-dom-health.md)
- [ADR-0009: Unified `rt` Runtime Prefix Across Python, TypeScript/JavaScript, Go, and Rust Targets](adr/0009-unified-rt-runtime-prefix-across-targets.md)
- [ADR-0010: Elixir, PHP, C#, and Kotlin Target Backends and Compile-Time CSS-to-XPath Lowering](adr/0010-elixir-php-csharp-kotlin-targets-and-css-to-xpath-lowering.md)
- [ADR-0011: Explicit-Delimiter `CodeWriter`, Unified `ModuleBuilder`, and Shared Extension Lowering](adr/0011-explicit-indented-code-writer-and-unified-module-builder.md)

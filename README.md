# parsmith

`parsmith` is a universal Rust compiler, linter, and 9-target code generation engine for the **KDL 2.0 Schema DSL (v2.1)** covering HTML DOM scraping, plain-text extraction, embedded JSON projection, and typed REST/JSON HTTP clients.

It compiles declarative `.kdl` schemas through a 5-pass pure-Rust frontend (`SourceLoader` filesystem and in-memory VFS, explicit `import` resolution, `E000`–`E403` / `W011`–`W041` static linting, Algebraic AST lowering, and pipeline type checking) into standalone, zero-framework source code across 8 ecosystems: **Python**, **TypeScript**, **JavaScript (ESM + `.d.ts`)**, **Go**, **Rust**, **Elixir**, **PHP 8.1+ (`native` & `dom-crawler`)**, **C# (.NET 8+)**, and **Kotlin (JVM)**.

## Workspace Architecture

`parsmith` is organized as a modular Cargo workspace:

| Crate | Role |
| :--- | :--- |
| `crates/parsmith` | Standalone native CLI (`parsmith`) and pure-Rust 5-pass compiler (`parsmith::kdl` KDL 2.0 document tree, selector engine, and serializer; `SourceLoader` FS + in-memory VFS; explicit imports; `E000`–`E403` / `W011`–`W041` linters; CSS3/4 + XPath 1.0 + CSS3-to-XPath 1.0 lowering + Portable regex subset + GJSON path validators; `json-to-kdl`; Algebraic AST; 2-pass `CodeWriter` + `ModuleBuilder` + `LanguageEmitter`; and 9 target emitters). |
| `crates/parsmith-py` | `PyO3` (`abi3-py310`) / `maturin` Python Host SDK (`check`, `generate`, `inspect_schema`, `json_to_kdl`, VFS `sources`, paired with `python/`). |
| `crates/parsmith-js` | `napi-rs` Node-API JS/TS Host SDK (`check`, `generate`, `inspectSchema`, `jsonToKdl`, VFS `sources`, paired with `js/`). |
| `crates/parsmith-wasm` | `wasm-bindgen` WebAssembly browser compiler, `json-to-kdl`, and AST selector health extractor (paired with `playground/`). |

## Target Matrix

A single `.kdl` schema compiles to idiomatic, type-checked code across nine targets:

| Target | CLI Subcommand | HTML / DOM Backend | Models & JSON Projectors | HTTP & REST Transport | Runtime Options |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Python** | `parsmith generate python` | `bs4` (default; `--bs4-parser lxml\|html.parser\|html5lib`), `lxml`, `parsel`, `selectolax` | `TypedDict` (default) or `dataclass` (`--models`); `rt_project_*` allowlist projectors | `SyncTransport` / `AsyncTransport` (`via_httpx`, `via_httpx_async`, `via_requests`, `via_aiohttp`, `via_urllib`); `--http-io both\|sync\|async` | Single file (default) or `-R` / `--separate-runtime` |
| **TypeScript** | `parsmith generate ts` | Standard Web DOM (`DOMParser`, `querySelectorAll`, `document.evaluate`) | Exported `interface` definitions + `rtProject*` allowlist projectors | `SyncTransport` / `AsyncTransport` (`viaFetch`, `viaAxios`) | Standalone `.ts` module |
| **JavaScript** | `parsmith generate js` | Standard Web DOM (`DOMParser`, `querySelectorAll`, `document.evaluate`) | ESM `.js` + companion `.d.ts` TypeScript declarations + `rtProject*` projectors | `SyncTransport` / `AsyncTransport` (`viaFetch`, `viaAxios`) | Dual `.js` + `.d.ts` files |
| **Go** | `parsmith generate go` | `goquery` (`github.com/PuerkitoBio/goquery`) | Typed `struct` + `gjson` allowlist `UnmarshalJSON` | Standard `*http.Client` (`net/http`) + `WithHeader` / `WithParam` / `WithCookie` / `WithTimeout` | Single file (default) or `-R` / `--separate-runtime` (`--package <pkg>`) |
| **Rust** | `parsmith generate rust` | `dom_query` | `#[derive(Serialize)]` `struct` + `serde_json` `rt::project_*` allowlist projectors | `&reqwest::Client` (async `reqwest`) + `rt::HttpOption` | Single file with `pub mod rt` (default) or `-R` / `--separate-runtime` |
| **Elixir** | `parsmith generate elixir` | `Floki` (`Floki.parse_document!/1`, `Floki.find/2`) | `defstruct` + `@derive JSON.Encoder` + `Rt.project_*` allowlist projectors | Built-in OTP `:httpc` (`Rt.via_httpc/1`) + `Rt.via_req/1` | Single file with `defmodule Rt` (default) or `-R` / `--separate-runtime` (`--package <ModulePrefix>`) |
| **PHP 8.1+** | `parsmith generate php` | `--html-lib native` (default; zero-Composer `\DOMDocument` + `\DOMXPath` via compile-time CSS3-to-XPath 1.0 lowering) or `dom-crawler` (`Symfony\Component\DomCrawler\Crawler`) | `readonly class` (`\JsonSerializable`) + `Rt::project*` allowlist projectors | `SyncTransport` (`Rt::viaCurl()`, `Rt::viaStream()`, `Rt::viaGuzzle($client)`) | Single file with `final class Rt` (default) or `-R` / `--separate-runtime` (`--package <Namespace>`) |
| **C# (.NET 8+)** | `parsmith generate csharp` | `AngleSharp` + conditional `AngleSharp.XPath` | `public sealed record` + `System.Text.Json` `Rt.Project*` allowlist projectors | `System.Net.Http.HttpClient` + `Rt.HttpOption` (`Rt.WithHeader`, `Rt.WithParam`, `Rt.WithCookie`, `Rt.WithTimeout`) | Single file with `public static partial class Rt` (default) or `-R` / `--separate-runtime` (`--package <Namespace>`) |
| **Kotlin (JVM)** | `parsmith generate kotlin` | `org.jsoup:jsoup` (CSS & XPath) | `data class` (`Rt.JsonEncodable`) + `kotlinx.serialization.json` `Rt.project*` projectors | `java.net.http.HttpClient` + `Rt.HttpOption` (`Rt.withHeader`, `Rt.withParam`, `Rt.withCookie`, `Rt.withTimeout`) | Single file with `object Rt` (default) or `-R` / `--separate-runtime` (`--package <pkg>`) |

>[!note]
>Generated parsers do not depend on `parsmith` at runtime. All required helpers, error types (`RtAssertionError`, `RtRegexError`, `RtJsonError`, `rt::RtError`), and allowlist JSON projectors (`rt_project_*` / `rtProject*`) are emitted directly into the generated module under the unified `rt` runtime namespace.

## Quick Example

### 1. Write a Schema (`book.kdl`)

```kdl
(list)struct BookCard {
    @split-doc { css-all "article.book-card" }

    title { css "h3 a"; attr "title"; trim }
    price { css ".price"; text; re #"(\d+\.\d{2})"#; to-float }
    in_stock { css ".availability"; text; trim; to-bool; fallback #false }
}

struct CatalogPage {
    heading { css "h1"; text; trim }
    books { nested BookCard }
}
```

### 2. Validate and Generate

```bash
# Validate syntax, selectors, regexes, declaration order, and pipeline types
parsmith check book.kdl

# Generate target parsers
parsmith generate python book.kdl -o catalog.py
parsmith generate ts book.kdl -o catalog.ts
parsmith generate js book.kdl -o catalog.js
parsmith generate go book.kdl -o catalog.go
parsmith generate rust book.kdl -o catalog.rs
parsmith generate elixir book.kdl -o catalog.ex
parsmith generate php book.kdl -o catalog.php
parsmith generate csharp book.kdl -o Catalog.cs
parsmith generate kotlin book.kdl -o Catalog.kt
```

### 3. Run the Generated Parser

**Python:**

```python
from catalog import CatalogPage

data = CatalogPage(html).parse()
print(data["heading"], len(data["books"]))
```

**TypeScript / JavaScript:**

```typescript
import { CatalogPage } from "./catalog";

const data = new CatalogPage(html).parse();
console.log(data.heading, data.books.length);
```

## Documentation

Full user guides and technical references live in [`docs/`](docs/README.md):

- **Onboarding**
  - [Documentation Index & Reading Order](docs/README.md)
  - [Quickstart & Decision Matrix](docs/quickstart.md)
  - [Syntax & File Structure (`syntax.md`)](docs/syntax.md)
- **Language Reference**
  - [Type System & Portable Regex Subset (`types.md`)](docs/types.md)
  - [Pipeline Operations (`operations.md`)](docs/operations.md)
  - [Condition Expressions (`conditions.md`)](docs/conditions.md)
  - [JSON Schemas & GJSON Projection (`json.md`)](docs/json.md)
  - [HTTP Requests & REST Clients (`http-rest.md`)](docs/http-rest.md)
  - [Typed User Extensions (`extensions.md`)](docs/extensions.md)
- **Tooling, Recipes & Diagnostics**
  - [CLI & Host SDKs (`cli-and-sdk.md`)](docs/cli-and-sdk.md)
  - [End-to-End Extraction Recipes (`recipes.md`)](docs/recipes.md)
  - [Linter & Compiler Diagnostics (`diagnostics.md`)](docs/diagnostics.md)
  - [Single-File LLM Reference (`llm.txt`)](docs/llm.txt)
- **Architecture & Domain Model**
  - [Domain Context & Glossary (`CONTEXT.md`)](CONTEXT.md)
  - [Architecture Decision Records (`docs/adr/`)](docs/adr/)

## Development Workflow

```bash
# 1. Format, lint, and test Rust workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2. Build and test Python Host SDK (via uv + maturin)
uv run maturin develop -m crates/parsmith-py/Cargo.toml
uv run pytest
uv run mypy --strict

# 3. Build and test JS/TS Host SDK (via N-API + Node.js)
cargo build -p parsmith-js
node --test js/tests/*.test.mjs

# 4. Build and test Web Playground (via wasm-bindgen + Vite/Vitest)
cd playground && npm run build:wasm && npm test -- --run && npm run build
```

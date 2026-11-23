# parsmith

`parsmith` is a multi-target **code generator and static linter** (written in Rust) for the **KDL 2.0 Schema DSL (v2.1)** covering HTML DOM scraping, plain-text extraction, embedded JSON projection, and typed REST/JSON HTTP clients.

It validates declarative `.kdl` schemas (`SourceLoader` filesystem and in-memory VFS, explicit `import` resolution, `E000`–`E403` / `W011`–`W041` static linting, **Algebraic AST** lowering, and pipeline type checking) and generates standalone, zero-framework source code across 9 targets: **Python**, **TypeScript**, **JavaScript (ESM + `.d.ts`)**, **Go**, **Rust**, **Elixir 1.18+ (Erlang/OTP 27–29)**, **PHP 8.1+ (`native` & `dom-crawler`)**, **C# (.NET 8+)**, and **Kotlin (JVM)**.

[**Interactive Playground**](https://vypivshiy.github.io/parsmith/) · [Why `parsmith`?](#why-parsmith) · [End-to-End Showcase](#end-to-end-showcase) · [Target Matrix & Ecosystem Fit](#target-matrix--ecosystem-fit) · [Tooling, SDKs & Selector Health](#tooling-host-sdks--selector-health-check) · [Documentation](#documentation) · [Architecture & Development](#workspace-architecture--development-workflow)

>[!tip]
>**Try `parsmith` in the Browser**: Open the [**Interactive WebAssembly Playground**](https://vypivshiy.github.io/parsmith/) to edit `.kdl` schemas with live `E000`–`E403` / `W011`–`W041` diagnostics, inspect generated code across all 9 target languages in real time, infer schemas from raw JSON (`json-to-kdl`), and run live CSS/XPath selector health checks against HTML snapshots—zero installation required.

---

## Why `parsmith`?

### Why Imperative & Raw LLM-Generated Parsers Break at Scale

Writing scrapers or API extractors directly in imperative code—or asking an LLM to generate raw Python, TypeScript, Go, or Rust parsers—hits four structural failure modes in production:

1. **Contract Drift & Inconsistent Edge-Case Handling**
   Ask an engineer or an LLM to write 30 parsers and you get 30 incompatible implementations. One crashes with `AttributeError` when `.select_one()` returns `None` on a card without a discount badge; a second swallows failures via `except Exception: pass`; a third returns `"19.99"` instead of `float`; a fourth passes undeclared keys from raw JSON straight into the database.
2. **Token Budget & Signal-to-Noise Ratio (`20 lines of KDL` vs. `600 lines of code`)**
   A 20-line `.kdl` schema contains 100% domain logic: selectors, transformations, target types, fallbacks, and JSON wire contracts. The generated module—complete with static model types, `rt_project_*` allowlist JSON projectors, monadic REST error variants (`Ok`, `DecodeErr`, `TransportErr`, `@error`), and HTTP transport adapters—spans 300–700 lines. For both human reviewers and AI coding agents, reading and patching 20 lines of declarative KDL is 15–20x cheaper, faster, and less error-prone than maintaining 600 lines of imperative Go or Rust.
3. **Library-Specific Hallucinations Across Stacks**
   In Python alone, `bs4`, `lxml`, `parsel`, and `selectolax` differ in text concatenation, attribute lookup, and in-place DOM removal semantics (`css-remove`). Go (`goquery`), Rust (`dom_query`), Elixir (`Floki`), C# (`AngleSharp`), Kotlin (`Jsoup`), and PHP (`\DOMXPath` vs. `DomCrawler`) each have their own DOM quirks, while HTTP clients (`httpx`, `requests`, `aiohttp`, `fetch`, `net/http`, `reqwest`, `:httpc`) raise incompatible exceptions. LLMs routinely hallucinate method signatures on non-mainstream DOM libraries and cannot maintain behavioral parity across languages by hand.
4. **Inability to Statically Inspect Imperative Code**
   When a parser is written as arbitrary Python or JS code, you cannot reliably extract all selectors without executing it. Because a `parsmith` schema lowers to a typed **Algebraic AST**, `inspect_schema` and the `parsmith-wasm` **Selector health check** statically extract every CSS/XPath step and its terminal `fallback` clause, validating selector health against a fresh HTML snapshot in milliseconds.

### Three Core Guarantees

Every constraint in `parsmith`—static validation (`E000`–`E403`), strict pipeline type checking, the **Portable regex subset**, and deterministic **GJSON path expressions**—serves three concrete guarantees:

1. **Instant Static Oracle (`parsmith check`)**
   Type mismatches in imperative scrapers (such as calling string `.strip()` on an element list before extracting text, or omitting a default value on a nullable field) only surface at runtime on live data. `parsmith check` statically verifies pipeline step types (`DOCUMENT`, `STRING`, `INT`, `FLOAT`, `BOOL`, arrays), CSS3/4 selector syntax, XPath 1.0 queries, GJSON paths, and regex capture group counts before generating code. For AI agents, this provides a deterministic feedback loop: *generate `.kdl` $\to$ run `parsmith check` $\to$ fix exact line/column diagnostic in one iteration*.
2. **Bitwise Cross-Target Parity**
   `parsmith` restricts regular expressions to a **Portable regex subset** (forbidding lookarounds `(?=...)` and named capture groups, requiring exactly one capture group in `re` / `re-all` steps so patterns execute in linear $O(N)$ time on Go `regexp` and Rust `regex` as well as PCRE/ECMA engines) and uses deterministic **GJSON path expressions** instead of arbitrary script filters. Any schema that passes `parsmith check` generates code across all 9 targets and produces identical extracted data structures on the same input fixture—letting you prototype in Python or the browser WASM playground and deploy native Rust, Go, or Elixir code to production.
3. **Zero-Runtime Dependency & Sandboxed Execution**
   - **Safe AI self-healing (No RCE)**: If an AI agent automatically generates or repairs parsers when a target website changes its layout, executing LLM-generated Python or JS code in production is a direct Remote Code Execution (RCE) vector. A `.kdl` schema (without explicit user `extension` hooks) is declarative and Turing-incomplete: it cannot execute system calls, access the filesystem, or enter infinite loops.
   - **Self-contained output (`rt` namespace)**: Generated code does not require `parsmith` or a heavy scraping framework at runtime. Two-pass code generation (`DryRun` + `ModuleBuilder` + `CodeWriter`) emits only the exact subset of lightweight `rt` helpers and allowlist JSON projectors actually used by the schema.

>[!tip]
>**When `parsmith` Is (and Isn't) the Right Tool**
>- **When plain `bs4` or `cheerio` is enough**: If you are writing a one-off, throwaway script in 30 lines of Python or JavaScript to scrape a single page once, a schema code generator is overkill—write imperative code directly.
>- **When `parsmith` is built for the job**: Long-term support (**LTS**) data pipelines where contracts must stay stable across dozens or hundreds of evolving sources; AI-driven scraper generation and automated selector repair; strict allowlist projection of noisy embedded/REST JSON; or executing parsers natively inside **Go, Rust, Elixir, C#, Kotlin, or PHP** services without running a separate Python microservice sidecar.

---

## End-to-End Showcase

### 1. HTML DOM Scraping + Embedded JSON Projection (`catalog.kdl`)

Modern web pages mix server-rendered HTML with JSON blobs inside `<script>` tags or `data-*` attributes. `parsmith` handles both in a single typed pipeline:

```kdl
// 1. Strict allowlist contract for embedded JSON (undeclared wire keys are stripped)
json SellerMeta {
    id int
    store_name str from="store.name"
    rating float? from="store.rating"
}

// 2. Repeated HTML card extraction
(list)struct ProductCard {
    @split-doc { css-all "article.product-card" }

    title {
        css "h3.title"
        css-remove ".promo-badge"
        text
        trim
    }
    price {
        css ".price"
        text
        re #"(\d+\.\d{2})"#
        to-float
        fallback 0.0
    }
    seller { css "script.seller-data"; text; jsonify SellerMeta }
}

// 3. Page-level container
struct CatalogPage {
    heading  { css "h1"; text; trim }
    products { nested ProductCard }
}
```

Validate and generate code for any target:

```bash
parsmith check catalog.kdl

# Full target names (`python`, `rust`, `elixir`, `csharp`, `kotlin`) and short aliases (`py`, `rs`, `ex`, `cs`, `kt`) both work:
parsmith generate py catalog.kdl -o catalog.py
parsmith generate ts catalog.kdl -o catalog.ts
parsmith generate go catalog.kdl -o catalog.go
parsmith generate rs catalog.kdl -o catalog.rs
parsmith generate ex catalog.kdl -o catalog.ex
```

Call the generated parser in your target runtime:

**Python (`bs4` / `lxml` / `parsel` / `selectolax`):**

```python
from catalog import CatalogPage

data = CatalogPage(html).parse()
print(data["heading"], data["products"][0]["seller"]["store_name"])
```

**TypeScript / JavaScript:**

```typescript
import { CatalogPage } from "./catalog";

const data = new CatalogPage(html).parse();
console.log(data.heading, data.products[0]?.seller.store_name);
```

**Go (`goquery` + `gjson`):**

```go
parser, err := NewCatalogPage(html)
data, err := parser.Parse()
fmt.Println(data.Heading, data.Products[0].Seller.StoreName)
```

**Rust (`dom_query` + `serde_json`):**

```rust
let parser = CatalogPageParser::new(&html);
let data = parser.parse()?;
println!("{} ({})", data.heading, data.products[0].seller.store_name);
```

**Elixir (`Floki`):**

```elixir
data = html |> CatalogPage.new() |> CatalogPage.parse()
IO.puts("#{data.heading}: #{hd(data.products).seller.store_name}")
```

#### Under the Hood: What `parsmith` Synthesizes

From the 20-line `.kdl` schema above, the 2-pass code generator (`DryRun` + `Emit`) produces a self-contained module with four layers of boilerplate handled automatically:

```text
// 1. Static Output & Wire Models (TypedDict / dataclass / interface / struct / record / data class)
type SellerMetaJson  = { id: Int, store_name: String, rating: Float? }
type ProductCardType = { title: String, price: Float, seller: SellerMetaJson }
type CatalogPageType = { heading: String, products: List<ProductCardType> }

// 2. Strict Allowlist JSON Projector (drops undeclared wire keys, enforces types & nullability)
fn rt_project_seller_meta(raw_json) -> SellerMetaJson:
    obj = expect_object(raw_json)
    return {
        id:         expect_int(obj["id"], required: true),
        store_name: expect_str(gjson_get(obj, "store.name"), required: true),
        rating:     expect_float_or_null(gjson_get(obj, "store.rating"), required: false),
    }

// 3. Scoped Field Pipelines (immutable DOM removal, regex group 1, fallback recovery)
struct ProductCard(doc):
    fn parse_title(card) -> String:
        node = css_one(card, "h3.title")
        clean = css_remove_clone(node, ".promo-badge")   // never mutates shared DOM in-place
        return trim(text(clean))

    fn parse_price(card) -> Float:
        try:
            raw = text(css_one(card, ".price"))
            match = regex_group_1(raw, /(\d+\.\d{2})/)
            return to_float(match)
        catch MissingNode | RegexMismatch | ParseError:
            return 0.0                                   // terminal `fallback 0.0`

    fn parse_seller(card) -> SellerMetaJson:
        raw = text(css_one(card, "script.seller-data"))
        return rt_project_seller_meta(json_decode(raw))
```

---

### 2. Typed REST Clients & Error Discriminators (`catalog_api.kdl`)

A `(rest)struct` turns POSIX `curl` commands or raw HTTP templates into a strongly-typed REST client whose methods **never throw uncaught exceptions** on HTTP errors, network failures, or JSON schema mismatches. Every call resolves to a discriminated `Result` (`Ok`, typed `<Struct>Err<Status>[<Suffix>]`, `DecodeErr`, `TransportErr`, or `UnknownErr`):

```kdl
define API-BASE="https://api.example.com/v1"

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
    @request name="get-product" response="Product" #"""
    curl '{{API-BASE}}/products/{{id:int}}?currency={{currency:str?}}' \
      -H 'Accept: application/json'
    """#

    // Discriminate HTTP 404 with required GJSON path "error.code"
    @error:not-found 404 ApiError "error.code"

    // Discriminate HTTP 200 responses that actually encode a business error (`{"success": false}`)
    @error 200 ActionError success=#false
}
```

**Python (`via_httpx(client)` / `via_httpx_async(client)` / `via_requests(session)` / `via_aiohttp(session)` / `via_urllib(timeout=None)`):**

Third-party adapters accept explicit, pre-configured clients via structural `*Like` `typing.Protocol` interfaces (`httpx` / `httpx2`, `requests` / `curl_cffi` / `niquests`, `aiohttp`); `via_urllib(timeout=None)` uses stdlib `urllib.request`. HTML and `(raw)struct` `fetch` / `async_fetch` raise `HttpStatusError` on HTTP `>= 400`.

```python
import httpx
from catalog_api import (
    CatalogApi,
    CatalogApiErr200Success,
    CatalogApiErr404NotFound,
    DecodeErr,
    TransportErr,
    via_httpx,
)

with httpx.Client() as client:
    res = CatalogApi.get_product(via_httpx(client), id=101, currency="USD")
    if res.is_ok:
        print(res.value["title"], res.value["price"])
    elif isinstance(res, CatalogApiErr404NotFound):
        print("404 Not Found:", res.value["code"], res.value["message"])
    elif isinstance(res, CatalogApiErr200Success):
        print("Business rejection on HTTP 200:", res.value["reason"])
    elif isinstance(res, DecodeErr):
        print("Wire contract violation on HTTP", res.status, res.cause)
    elif isinstance(res, TransportErr):
        print("Network failure:", res.cause)
```

**Rust (`&reqwest::Client`):**

```rust
match CatalogApi::get_product(&client, 101, Some("USD")).await {
    Ok(product) => println!("{} (${})", product.title, product.price),
    Err(CatalogApiError::Err404NotFound(err)) => eprintln!("Not found: {}", err.code),
    Err(CatalogApiError::Err200Success(err)) => eprintln!("Rejected: {}", err.reason),
    Err(CatalogApiError::Decode(status, err)) => eprintln!("Decode error on {status}: {err}"),
    Err(CatalogApiError::Transport(err)) => eprintln!("Network error: {err}"),
    Err(CatalogApiError::Unknown(status, body)) => eprintln!("HTTP {status}: {body}"),
}
```

**Elixir (`Rt.via_httpc()` / `Rt.via_req()`):**

```elixir
case CatalogApi.get_product(Rt.via_httpc(), 101, "USD") do
  {:ok, %ProductJson{title: title, price: price}} ->
    IO.puts("#{title} ($#{price})")
  {:error, %CatalogApiErr404NotFound{value: err}} ->
    IO.puts("Not found: #{err.code}")
  {:error, %CatalogApiErr200Success{value: err}} ->
    IO.puts("Rejected: #{err.reason}")
  {:error, %Rt.DecodeErr{status: status, cause: cause}} ->
    IO.puts("Schema mismatch on HTTP #{status}: #{cause}")
end
```

### Why `(rest)struct` Instead of OpenAPI?

OpenAPI (Swagger) and `parsmith` solve opposite ends of the API lifecycle:

1. **Producer-First (OpenAPI) vs. Trace-First (`parsmith`)**
   OpenAPI works well when you *own* the backend and generate specifications from server framework annotations (FastAPI, NestJS, ASP.NET, Spring). When integrating with external, undocumented, or reverse-engineered endpoints, you rarely have a formal spec—you have a handful of **raw HTTP traces or `curl` commands** copied from browser DevTools / Mitmproxy and a few sample JSON responses. `parsmith` builds directly from those empirical traces: paste the `curl` or raw HTTP request into `@request`, parameterize dynamic inputs with `{{name:type}}`, bootstrap the response contract via `parsmith json-to-kdl`, and compile a typed client immediately.
2. **Protection Against Stale, Bloated, or Inaccurate Vendor Specs**
   Generating client code directly from a third-party OpenAPI spec pulls in every upstream flaw: 200-field response models when your worker only needs 3 fields, inaccurate nullability flags that crash decoders in production, and undocumented error payloads (such as APIs returning HTTP `200 OK` with `{"success": false}`). In `parsmith`, the consumer defines a minimal **allowlist JSON projector** (with GJSON path flattening `from="data.items"`), drops undeclared wire noise, and routes real-world failure shapes through `@error` discriminators and `DecodeErr` without uncaught exceptions.
3. **No Universal, Zero-Boilerplate OpenAPI Codegen Across 9 Languages**
   Because the OpenAPI specification allows unbounded schema polymorphism (`oneOf`, `anyOf`, `allOf`, cyclic `$ref`), off-the-shelf generators are either language-specific or emit sprawling multi-file directories with heavy runtime dependencies and incompatible error models. By scoping `(rest)struct` to concrete wire templates, allowlist projectors, and monadic error tables, `parsmith` emits a **single, dependency-free file** with an identical `Result` (`Ok`, `<Struct>Err*`, `DecodeErr`, `TransportErr`, `UnknownErr`) and transport (`via_*`) contract across all 9 targets.
4. **OpenAPI as an Upstream Reference for LLM Agents**
   When an external API *does* publish an OpenAPI spec (often thousands of lines of YAML/JSON), treat it as **read-only reference context** for an AI agent rather than a direct codegen input: pass the relevant OpenAPI endpoint slice + [`docs/llm.txt`](docs/llm.txt) to the LLM to synthesize a compact 25-line `.kdl` `(rest)struct` tailored to the exact fields and error variants your application needs, validate it with `parsmith check`, and compile it to your target language.

---

## Target Matrix & Ecosystem Fit

A single `.kdl` schema generates idiomatic, type-checked source code across nine targets:

| Target | CLI Subcommand (& Alias) | HTML / DOM Backend | Models & JSON Projectors | HTTP & REST Transport | Runtime Options |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Python** | `parsmith generate python` (`py`) | `bs4` (default; `--bs4-parser lxml\|html.parser\|html5lib`), `lxml`, `parsel`, `selectolax` | `TypedDict` (default) or `dataclass` (`--models`); `rt_project_*` allowlist projectors | `SyncTransport` / `AsyncTransport` (`via_httpx(client)` for `httpx`/`httpx2`, `via_httpx_async(client)`, `via_requests(session)`, `via_aiohttp(session)`, `via_urllib(timeout=None)`); `*Like` `Protocol` types & `HttpStatusError`; `--http-io both\|sync\|async` | Single file (default) or `-R` / `--separate-runtime` |
| **TypeScript** | `parsmith generate ts` (`typescript`) | Standard Web DOM (`DOMParser`, `querySelectorAll`, `document.evaluate`) | Exported `interface` definitions + `rtProject*` allowlist projectors | `SyncTransport` / `AsyncTransport` (`viaFetch`, `viaAxios`) | Standalone `.ts` module |
| **JavaScript** | `parsmith generate js` (`javascript`) | Standard Web DOM (`DOMParser`, `querySelectorAll`, `document.evaluate`) | ESM `.js` + companion `.d.ts` TypeScript declarations + `rtProject*` projectors | `SyncTransport` / `AsyncTransport` (`viaFetch`, `viaAxios`) | Dual `.js` + `.d.ts` files |
| **Go** | `parsmith generate go` (`golang`) | `goquery` (`github.com/PuerkitoBio/goquery`) | Typed `struct` + `gjson` allowlist `UnmarshalJSON` | Standard `*http.Client` (`net/http`) + `WithHeader` / `WithParam` / `WithCookie` / `WithTimeout` | Single file (default) or `-R` / `--separate-runtime` (`--package <pkg>`) |
| **Rust** | `parsmith generate rust` (`rs`) | `dom_query` | `#[derive(Serialize)]` `struct` + `serde_json` `rt::project_*` allowlist projectors | `&reqwest::Client` (async `reqwest`) + `rt::HttpOption` | Single file with `pub mod rt` (default) or `-R` / `--separate-runtime` |
| **Elixir 1.18+ (OTP 27–29)** | `parsmith generate elixir` (`ex`) | `Floki` (`Floki.parse_document!/1`, `Floki.find/2`) | `defstruct` + `@derive JSON.Encoder` + `Rt.project_*` allowlist projectors (built-in Elixir 1.18+ `JSON`) | Built-in OTP `:httpc` (`Rt.via_httpc/1`) + `Rt.via_req/1` | Single file with `defmodule Rt` (default) or `-R` / `--separate-runtime` (`--package <ModulePrefix>`) |
| **PHP 8.1+** | `parsmith generate php` | `--html-lib native` (default; zero-Composer `\DOMDocument` + `\DOMXPath` via compile-time CSS3-to-XPath 1.0 lowering) or `dom-crawler` (`Symfony\Component\DomCrawler\Crawler`) | `readonly class` (`\JsonSerializable`) + `Rt::project*` allowlist projectors | `SyncTransport` (`Rt::viaCurl()`, `Rt::viaStream()`, `Rt::viaGuzzle($client)`) | Single file with `final class Rt` (default) or `-R` / `--separate-runtime` (`--package <Namespace>`) |
| **C# (.NET 8+)** | `parsmith generate csharp` (`cs`) | `AngleSharp` + conditional `AngleSharp.XPath` | `public sealed record` + `System.Text.Json` `Rt.Project*` allowlist projectors | `System.Net.Http.HttpClient` + `Rt.HttpOption` (`Rt.WithHeader`, `Rt.WithParam`, `Rt.WithCookie`, `Rt.WithTimeout`) | Single file with `public static partial class Rt` (default) or `-R` / `--separate-runtime` (`--package <Namespace>`) |
| **Kotlin (JVM)** | `parsmith generate kotlin` (`kt`) | `org.jsoup:jsoup` (CSS & XPath) | `data class` (`Rt.JsonEncodable`) + `kotlinx.serialization.json` `Rt.project*` projectors | `java.net.http.HttpClient` + `Rt.HttpOption` (`Rt.withHeader`, `Rt.withParam`, `Rt.withCookie`, `Rt.withTimeout`) | Single file with `object Rt` (default) or `-R` / `--separate-runtime` (`--package <pkg>`) |

### Eliminating the "Python Sidecar" in Backend Ecosystems

In Python and JavaScript, scraping libraries are ubiquitous. In compiled and enterprise server ecosystems, teams face two acute problems: (1) maintaining a separate Python microservice sidecar just to parse a few dozen HTML/REST sources is operationally expensive, and (2) LLMs produce brittle code for less common DOM libraries (`Floki`, `dom_query`, `AngleSharp`, `\DOMXPath`).

Because `parsmith`'s core pipeline operations form a closed **Algebraic AST**, the code generator translates a single `.kdl` schema into idiomatic native code for each runtime:

- **Elixir 1.18–1.20+ (`Floki` + Erlang/OTP 27–29 BEAM)**: Built for high-concurrency data pipelines (Oban, Broadway, GenStage) using Elixir's standard `JSON` module (`>= 1.18`) and OTP `:httpc` (`>= 27`, verified through Elixir 1.20.x and Erlang/OTP 29). `Floki`'s HTML tree is immutable (`Floki.filter_out/2` handles `css-remove` cleanly without mutating shared DOM nodes in place), REST endpoints return native `{:ok, %Struct{}} | {:error, %Rt.DecodeErr{}}` tuples for pattern matching, and `@type` specifications integrate directly with Dialyzer and Elixir's type checker.
- **PHP 8.1+ (Compile-Time CSS3-to-XPath 1.0 Lowering)**: PHP's standard `ext-dom` (`\DOMDocument` + `\DOMXPath`) on PHP 8.1–8.3 natively evaluates XPath 1.0 but lacks CSS selector queries. `parsmith`'s default `php --html-lib native` backend compiles CSS3 selectors into deterministic XPath 1.0 expressions at compile time—enabling zero-Composer HTML and JSON extraction out of the box (or targeting `symfony/dom-crawler` via `--html-lib dom-crawler` when CSS4 selectors are needed).
- **Go (`goquery` + `gjson`) & Rust (`dom_query` + `reqwest`)**: High-throughput native parsers with zero dynamic reflection in hot paths, functional `With*` / `rt::HttpOption` request modifiers, and strongly-typed error hierarchies (`errors.As` in Go; exhaustive enum matching in Rust).
- **C# (.NET 8+ `AngleSharp`) & Kotlin (JVM `Jsoup`)**: First-class `sealed record` and `data class` models with `System.Text.Json` and `kotlinx.serialization.json` allowlist projectors for enterprise `.NET` and JVM backends.

---

## Tooling, Host SDKs & Selector Health Check

### 1. Bootstrap Schemas from Live JSON (`parsmith json-to-kdl`)

Instead of writing `json` schemas by hand, pipe any sample JSON file or API response into `parsmith json-to-kdl` to infer a KDL v2.1 `json` or `(array)json` schema with nested inline blocks:

```bash
# Convert a local JSON fixture into a KDL schema
parsmith json-to-kdl fixtures/user.json --name UserProfile -o schemas/user.kdl

# Pipe a live API payload directly from curl
curl -s https://api.example.com/v1/items/42 | parsmith json-to-kdl --name ItemPayload
```

### 2. Static Validation & Machine-Readable Diagnostics (`parsmith check`)

Run `parsmith check` in CI or inside an AI agent's tool loop to validate syntax, imports, target reserved keywords (`E403`), selector grammars, and pipeline types in milliseconds:

```bash
# Annotated Rust-style console diagnostics
parsmith check schemas/catalog.kdl

# Structured JSON diagnostics for CI or LLM repair agents
parsmith check schemas/catalog.kdl -f json
```

### 3. Static AST Introspection & Selector Health Check (`inspect_schema` & `parsmith-wasm`)

When a target site changes its markup, `parsmith` does not require executing target parsers to diagnose broken selectors:

- **`inspect_schema` / `inspectSchema`**: Runs the 5-pass frontend and returns a structured `ModuleInfo` tree exposing all structs, inferred field types (`TypeInfo`), `json` definitions, REST endpoints, and `@error` matchers.
- **Selector Health Check (`parsmith-wasm`)**: Statically walks a `struct` and all transitive `nested` references to extract every `css`, `css-all`, `css-remove`, `xpath`, `xpath-all`, and `xpath-remove` step along with its terminal `fallback` clause, evaluating each selector against a sample HTML document in the browser to classify it as `ok`, `warn` (matched 0 elements but protected by `fallback`), or `fail`.

### 4. Embedded Host SDKs & In-Memory VFS (`parsmith-py` & `parsmith-js`)

Embed the linter and code generator directly in Python (`PyO3` `abi3`) or Node.js/TypeScript (`napi-rs`) without spawning CLI subprocesses or touching the filesystem (`MemorySourceLoader`):

```python
import parsmith

vfs = {
    "shared/common.kdl": """
        define PRICE-RE=#"(\\d+\\.\\d{2})"#
    """,
    "scrapers/card.kdl": """
        import "../shared/common.kdl" { (define)PRICE-RE }
        struct Card {
            price { css ".price"; text; re PRICE-RE; to-float; fallback 0.0 }
        }
    """,
}

# Validate and compile purely in memory
res = parsmith.generate("scrapers/card.kdl", target="rust", sources=vfs)
assert res.ok
rust_source: str = res["card.rs"]
```

### 5. Bundled Skill for AI Coding Agents (`.agents/skills/parsmith-dsl`)

`parsmith` ships an agent skill directly in the repository at [`.agents/skills/parsmith-dsl/`](.agents/skills/parsmith-dsl/SKILL.md) (including progressive-disclosure reference guides in `references/` and verified `.kdl` examples in `references/examples/`).

To enable `parsmith-dsl` in your own project for Claude Code, OpenCode, Cursor, or Codex agents, copy the skill directory into your workspace (or global skills folder):

```bash
# Copy into your project's agent skills directory (.agents/skills/ or .claude/skills/)
mkdir -p .agents/skills
cp -r path/to/parsmith/.agents/skills/parsmith-dsl .agents/skills/
```

For single-file context injection (system prompts, custom GPTs, or API tool loops without skill directories), pass [`docs/llm.txt`](docs/llm.txt) directly.

---

## Documentation

Full user guides, language specifications, and architectural references live in [`docs/`](docs/README.md):

- **Onboarding**
  - [Documentation Index & Reading Order](docs/README.md)
  - [Quickstart & Decision Matrix (`quickstart.md`)](docs/quickstart.md)
  - [Syntax & File Structure (`syntax.md`)](docs/syntax.md)
- **Language Reference**
  - [Type System & Portable Regex Subset (`types.md`)](docs/types.md)
  - [Pipeline Operations (`operations.md`)](docs/operations.md)
  - [Condition Expressions (`conditions.md`)](docs/conditions.md)
  - [Typed User Extensions (`extensions.md`)](docs/extensions.md)
- **JSON & HTTP/REST**
  - [JSON Schemas & GJSON Projection (`json.md`)](docs/json.md)
  - [HTTP Requests & REST Clients (`http-rest.md`)](docs/http-rest.md)
- **Tooling, Recipes & Diagnostics**
  - [CLI & Host SDKs (`cli-and-sdk.md`)](docs/cli-and-sdk.md)
  - [End-to-End Extraction Recipes (`recipes.md`)](docs/recipes.md)
  - [Linter & Compiler Diagnostics (`diagnostics.md`)](docs/diagnostics.md)
  - [Bundled Agent Skill (`.agents/skills/parsmith-dsl/`)](.agents/skills/parsmith-dsl/SKILL.md)
  - [Single-File LLM Reference (`llm.txt`)](docs/llm.txt)
- **Architecture & Domain Model**
  - [Domain Context & Glossary (`CONTEXT.md`)](CONTEXT.md)
  - [Architecture Decision Records (`docs/adr/`)](docs/adr/)

---

## Workspace Architecture & Development Workflow

`parsmith` is organized as a modular Cargo workspace:

| Crate | Role |
| :--- | :--- |
| `crates/parsmith` | Standalone native CLI (`parsmith`) and pure-Rust 5-pass validation and codegen library (`parsmith::kdl` KDL 2.0 document tree, selector engine, and serializer; `SourceLoader` FS + in-memory VFS; explicit imports; `E000`–`E403` / `W011`–`W041` linters; CSS3/4 + XPath 1.0 + CSS3-to-XPath 1.0 lowering + Portable regex subset + GJSON path validators; `json-to-kdl`; Algebraic AST; 2-pass `CodeWriter` + `ModuleBuilder` + `LanguageEmitter`; and 9 target emitters). |
| `crates/parsmith-py` | `PyO3` (`abi3-py310`) / `maturin` Python Host SDK (`check`, `generate`, `inspect_schema`, `json_to_kdl`, VFS `sources`, paired with `python/`). |
| `crates/parsmith-js` | `napi-rs` Node-API JS/TS Host SDK (`check`, `generate`, `inspectSchema`, `jsonToKdl`, VFS `sources`, paired with `js/`). |
| `crates/parsmith-wasm` | `wasm-bindgen` WebAssembly browser code generator, `json-to-kdl`, and AST selector health extractor (paired with `playground/`). |

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

---
name: parsmith-dsl
version: "1.1.0"
dsl_version: "2.1"
description: >
  Author, combine, validate, and compile KDL 2.0 Schema DSL (v2.1) files with `parsmith` for HTML DOM scraping, plain-text extraction, embedded JSON projection, and typed REST/JSON HTTP clients across 9 targets (`python`/`py`, `ts`, `js`, `go`, `rust`/`rs`, `elixir`/`ex`, `php`, `csharp`/`cs`, `kotlin`/`kt`). Covers `(item|list|table|dict|flat|raw|rest)struct`, `fn`, `(raw)fn`, `json`, `(array)json`, `(dict)json`, `@request`, `@error[:<suffix>]`, `css`/`xpath`/`re`/`jsonify`, `parsmith check`, `parsmith generate`, and `parsmith json-to-kdl`. Use whenever designing, refactoring, or debugging `.kdl` schemas or generated `parsmith` parsers/clients.
---

# parsmith-dsl — KDL 2.0 Schema DSL (v2.1)

Compile `.kdl` schemas into standalone, zero-framework HTML/text parsers and typed REST HTTP clients via `parsmith generate <py|ts|js|go|rs|ex|php|cs|kt>`. HTML structs, `(raw)struct`, `fn`, `json` schemas, and `(rest)struct` compose freely in a single `.kdl` file.

---

## Core Invariants (Always Apply)

1. **CLI subcommands**:
   - Version: `parsmith version [-f text|json]` (subcommand).
   - Bootstrap JSON schema: `parsmith json-to-kdl [<input.json>] --name <SchemaName> [-o <out.kdl>]` (accepts file path or `stdin`).
   - Validate schema(s): `parsmith check <PATHS>... [-f text|json] [-t <target>...]` (accepts 1+ `.kdl` files or directories).
   - Generate code: `parsmith generate <py|ts|js|go|rs|ex|php|cs|kt> <PATHS>... [-o <file_or_dir>] [TARGET_OPTIONS]` (accepts 1+ `.kdl` files or directories; skips `_*.kdl` helper files during directory scan and merges shared runtime helpers across the batch when `-R / --separate-runtime` is used; Python `-R` defaults `--runtime-name` to `.parsmith_runtime` emitting `from .parsmith_runtime import ...`).
2. **Top-down declaration order (`E302`)**: declare helper `struct` and `json` schemas **above** any struct, `json` field, `nested`, `jsonify`, `@request response=`, or `@error` that references them.
3. **Inline `json` schemas by default**: nest single-use child objects (`field { ... }` / `field Model { ... }`), object arrays (`field (array)ItemModel { ... }` — explicit `ItemModel` name is mandatory, `E001`), and maps (`field (dict) { @key str; @value ... }`) **inline** inside their parent `json` schema. Declare a standalone top-level `json` schema only when referenced directly by `@request response=`, `@error`, or `jsonify`, or when shared across 2+ schemas (declared above callers, `E302`).
4. **Portable `json` field names (`E403`) & GJSON `from="..."` (`E040`, `E041`)**: `json` field names (after stripping `?`) must match `^[A-Za-z_][A-Za-z0-9_]*$` (`snake_case` or `camelCase`; no `kebab-case`, no colons `url: str`, no target keywords like `from`, `type`, `class`, `in`). Map reserved wire keys or nested GJSON paths via `from="..."` (e.g. `sender str from="from"`, `tag_names (array)str from="tags.#.name"`). Duplicate field or wire keys in one schema emit `E041`.
5. **Selectors & Portable regex subset (`E002`, `E203`)**:
   - Default to **CSS3** (`[attr^=val]`, `:nth-of-type(N)`, `:first-child`, combinators). Prefer `css "sel:nth-of-type(N)"` over `css-all "sel"; index N` for single-element selection.
   - **XPath (`xpath`, `xpath-all`, `xpath-remove`)** is supported in `py -L lxml`, `py -L parsel`, `ts`, `js`, `php`, `cs`, and `kt`, and rejected at compile time (`E203`) in `py -L bs4`, `py -L selectolax`, `go`, `rs`, and `ex`.
   - **PHP `native` CSS3-to-XPath lowering**: `parsmith generate php` defaults to `--html-lib native` (zero-Composer `\DOMDocument` + `\DOMXPath` via compile-time CSS3 $\to$ XPath 1.0 lowering; emits `E002` on non-lowerable CSS4 selectors—use `--html-lib dom-crawler` when CSS4 is required).
   - **Portable regex subset**: `re #"..."#` and `re-all #"..."#` pipeline steps forbid lookarounds (`(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`) and named groups (`(?P<name>...)`), and require **exactly 1** capturing group `(...)` (`E002`; catch no-match `RtRegexError` with `fallback`). Inside `assert {}` / `filter {}` / `match {}`, `re` is a boolean condition and requires 0 capture groups.
6. **Struct invariants (`E203`, `E401`)**:
   - `(rest)struct` allows **only** `@doc`, `@request`, and `@error[:<suffix>]` (`E203`) and requires $\ge 1$ `@request` (`E401`). `response-join` is forbidden on `(rest)struct` (`E001`; use `response-path` alone).
   - `(raw)struct` / `(raw)fn` start with `STRING` and **forbid** all HTML ops (`css*`, `xpath*`, `text`, `attr`, `raw`).
   - `(table)struct` requires `@table`, `@rows`, `@match`, `@value`, and every regular field **must** start with `match { <string-conditions> }`.
   - `(dict)struct` requires `@split-doc`, `@key`, `@value` and forbids regular fields.
7. **Pipeline layout for line-accurate diagnostics (`1–3 flat ops` vs. `4+ ops / nested blocks`)**:
   - Write **1–3 flat steps** (no nested `{}` blocks) inline on a single line separated by `;` (e.g. `title { css "h3"; text; trim }`).
   - Write **4+ steps**, or any pipeline containing nested `{ ... }` blocks (`repl {}`, `filter {}`, `assert {}`, multi-step `match {}`) or `re` + type conversion + `fallback`, **one step per indented line** (no `;`). This avoids missing-`;` / brace-matching errors (`E000`/`E001`) and ensures `parsmith check` line numbers point to the exact offending step.
8. **KDL 2.0 `\` line continuation (`E000`/`E203`)**: keep `//` comments **outside** multi-line `\` continuations—in KDL 2.0, `//` consumes the trailing newline and breaks continuation, leaking subsequent HTTP lines (`GET`, `POST`, `curl`, `-H`) as stray struct fields.
9. **`@doc` semantics & hygiene**: document base URL, auth mechanics, rate limits, and domain constants; never hardcode live credentials, cookies, or session tokens into `.kdl` files.

---

## Workflow

### Step 1 — Map Input Signals to KDL Constructs

Select the construct matching the source payload. Combine multiple structs in one `.kdl` file when an extraction workflow spans HTML + plain text/JS + REST endpoints:

| Source Signal | Construct in `.kdl` | Required Directives & Key Rules | Reference to Load |
| :--- | :--- | :--- | :--- |
| **JSON HTTP endpoint** (`application/json`, root `{` or `[`, cURL/HAR trace) | `(rest)struct Name` + `json` schemas | 1+ `@request` (`name="<kebab-id>"`, `response="<Schema>"\|#null`); `@error[:<suffix>] <status> <Schema> ["pos.key"...] [k=v ...]` | `references/rest.md` + `references/json.md` |
| **HTML with embedded JSON** (`application/ld+json`, `__NEXT_DATA__`, inline JS var / `JSON.parse`, `data-*` JSON attribute) | `struct Name` + `json` schemas | Extract raw string (`css ...; raw inner` / `attr ...; unescape` / `re`) $\to$ `jsonify <Schema> [path="<gjson>"]` | `references/html-raw.md` + `references/json.md` |
| **HTML single detail object** | `struct Name` (`(item)struct`) | 1+ regular fields (`<name> [<type>] [value=<literal>] [{ <pipeline> }]`) starting with `css`/`css-all`/`xpath`, `@<init_field>`, or `value=<literal>` | `references/html-raw.md` |
| **HTML repeating cards or horizontal table rows** | `(list)struct Name` | `@split-doc { css-all "<selector>" }` + item fields (`<name> [<type>] [value=<literal>] [{ <pipeline> }]`; append `fallback #null` on optional elements; `css-all` for list fields) | `references/html-raw.md` |
| **HTML key-value table / `<dl>` / label-value rows** | `(table)struct Name` | `@table { css "<container>" }`, `@rows { css-all "<rows>" }`, `@match { css "<key>"; ... }`, `@value { css "<val>"; ... }`; every field starts with `match { eq\|starts\|contains\|re ... }` | `references/html-raw.md` |
| **HTML dynamic map** (`<meta>` tags, open-ended key-value pairs) | `(dict)struct Name` | `@split-doc`, `@key`, `@value` (no regular fields) | `references/html-raw.md` |
| **HTML flat string list** | `(flat)struct Name [keep-order=#true]` | Every field pipeline must return `STRING` or `STRING[]` | `references/html-raw.md` |
| **Plain text** (JS bundle, URL query string, `.m3u8` playlist, CSV) | `(raw)struct Name` | Starts with `STRING`; optional `@split-doc` switches output from single object to list; HTML ops forbidden (`E203`) | `references/html-raw.md` |
| **Single extracted value helper** | `fn name` / `(raw)fn name` | Standalone module function; only `@doc` + pipeline steps allowed | `references/html-raw.md` |

> **Bootstrap from Sample JSON:** Pipe or pass any sample JSON file to generate hierarchical inline `json` schemas automatically:
> `parsmith json-to-kdl sample.json --name ResponseName -o schema.kdl`

### Step 2 — Core Syntax Summary

#### 2.1 `json` Schemas (used by both `(rest)struct` and `jsonify`) & `struct` Field Grammar
- **Top-level `json` kinds**: `json Name { ... }`, `(array)json Name { ... }`, `(dict)json Name { [@key str|int|float|bool;] @value <TypeOrBlock>; }`.
- **`json` field syntax**: `<name> <type>[?] [from="<gjson_path>" | value=<literal>] [@omitempty] [@skip]`
  - `?`: value may be `null` (`T | None` / `T | null` / `*T` / `Option<T>`).
  - `@omitempty`: key may be absent from JSON object (`NotRequired[T]` / `?:` / `,omitempty` / `#[serde(default)]`). Combine `T? @omitempty` when a key can be either absent or `null`.
  - `@skip`: validated by linter, omitted from generated types & allowlist projector (`type` defaults to `str`).
  - `from="<gjson_path>"`: maps wire keys (`from="from"`, `from=#"os\.version"#`), nested paths (`from="data.user.0.name"`), array count (`from="items.#"` $\to$ `int`), or array projection (`from="tags.#.name"` $\to$ `(array)str`).
  - `value=<str|int|float|bool>`: injects a constant scalar into the projected model without reading wire JSON (supports `{{DEFINE}}` expansion in strings; cannot combine with `?`, `(array)`, `(dict)`, `from=`, `@omitempty`, `@skip`, or inline `{}`).
  - **Inline blocks (hoisted post-order ahead of parent)**:
    - `field[?] { ... }` $\to$ synthesizes `{Parent}{FieldPascal}`
    - `field ModelName[?] { ... }` $\to$ `ModelName`
    - `field (array)ItemModel[?] { ... }` $\to$ `ItemModel` (explicit `ItemModel` is **mandatory**, `E001`)
    - `field (dict)[DictName] { [@key str;] @value [ValModel][?] { ... } }` $\to$ `{DictName}Value` / `{Parent}{FieldPascal}Value`
- **`struct` & `@init` field syntax**: `<name> [<type>] [value=<literal>] [{ <pipeline> }]`
  - Optional postfix `<type>` (`str`, `int`, `float`, `bool`, `?`, `(array)`): keeps automatic inference when omitted; when specified on a `{ <pipeline> }` field, auto-inserts `to-bool` (`Any -> BOOL`), `to-int` (`STR -> INT`), or `to-float` (`STR -> FLOAT`) before any terminal `fallback` and enforces `E100` (never implicitly extracts `text` from `DOC`). Prefix field annotations like `(str)field` are forbidden (`E001`).
  - `value=<str|int|float|bool>` (supports `{{DEFINE}}` expansion in strings; forbidden in `(table)struct`, `E203`): defines a standalone constant field when `{}` is omitted (`source str value="example"`), or seeds step 0 of `{ <pipeline> }` (`fetched_at int value=0 { !Feed.now_ts }`).

#### 2.2 `@request` & `@error[:<suffix>]`
- **`@request` formats**: raw `HTTP/1.1` / `HTTP/2`, POSIX `curl`, or scalar `define` constant (`define BASE="https://..."`, `define REQ="curl '{{BASE}}/items/{{id:int}}'"`).
- **Typed placeholders (`{{name[:type][[]][?][|style]}}`)**: `name` must be **lowercase** `^[a-z][a-z0-9_-]*$` (`E002`; uppercase `{{UPPER}}` is only expanded inside top-level `define`, never inside inline `@request #"""..."""#` literals). `type`: `str|int|float|bool`. `[]` (array) and `?` (optional) are forbidden in URL path (`E002`). `|style` (`repeat|csv|bracket|pipe|space`) requires `[]` (`E002`).
- **`@error[:<suffix>] <status:100..599> <Schema> ["pos.key"...] [prop.key=val ...]`** (`(rest)struct` only):
  - Prefer explicit `:suffix` (`@error:rate-limit 429 RateLimitError` $\to$ `<Struct>Err429RateLimit`) for semantic variant names and to prevent `E400` collisions.
  - `@error` on `2xx` requires $\ge 1$ condition (e.g. `@error 200 ActionError success=#false`). Runtime matchers evaluate in declaration order and safely fall through on non-JSON bodies or failed `<Schema>` projection, returning `DecodeErr` if all matching rules fail projection or `UnknownErr` for unmatched non-`2xx` statuses.

#### 2.3 Unified Hybrid Example (HTML + Embedded JSON + Raw + REST in one `.kdl`)

```kdl
@doc "Unified catalog scraper + player parser + REST API client (https://example.com)."

define BASE-URL="https://example.com"

json HydratedItem {
    id int
    title str
    tag_names (array)str from="tags.#.name"
}

json InboxResponse {
    messages (array)MessageItem {
        id str
        sender str from="from"
        subject str
    }
    errors (dict) @omitempty {
        @key str
        @value (array)str
    }
}

json ApiError { message str }

// 1. Plain-text (raw)struct for JS player config
(raw)struct PlayerConfig {
    stream_url { re #"file:\s*[\"']([^\"']+)[\"']"# }
}

// 2. HTML (list)struct + embedded JSON via jsonify
(list)struct CatalogPage {
    @split-doc { css-all ".product-card" }
    source  str  value="example"
    title   str  { css "h3"; text; normalize-space }
    views   int  { css ".views"; text; fallback 0 }
    in_stock bool { css ".badge-in-stock" }
    url     { css "a.card-link"; attr "href"; fmt "{{BASE-URL}}{{}}" }
    payload { css "script.item-json"; raw inner; jsonify HydratedItem }
}

// 3. Typed REST API client
(rest)struct ExampleApi {
    @request name="get-inbox" response="InboxResponse" #"""
    POST /api/inbox?limit={{limit:int?}} HTTP/1.1
    Host: api.example.com
    Content-Type: application/json

    {"email": "{{email}}"}
    """#

    @error:not-found 404 ApiError
    @error:rate-limit 429 ApiError
}
```

### Step 3 — Iterative Lint & Code Generation

```bash
# 1. Validate schema or directory of schemas (MANDATORY before returning .kdl or generating code; cap 5 retries per line)
parsmith check schema.kdl -f json
parsmith check schemas/ -f json

# 2. Generate standalone parser / REST client for any of the 9 targets (single file, multiple files, or directory)
parsmith generate py schema.kdl -L bs4 -o out/
parsmith generate py schemas/ -R -o out/         # batch 2-pass shared runtime (from .parsmith_runtime import ...)
parsmith generate ts schema.kdl -o out/
parsmith generate go schema.kdl -o out/
parsmith generate rs schema.kdl -o out/
parsmith generate ex schema.kdl -o out/
```

**Completion criteria before returning `.kdl`:**
1. `parsmith check <schema.kdl>` exits `0` with zero errors.
2. Top-down declaration order (`E302`): `@doc` $\to$ `import`/`define`/`extension` $\to$ shared `json` & helper `struct` $\to$ endpoint `json` (with single-use nested models inlined) $\to$ entrypoint `struct` / `(rest)struct`.

---

## Reference Pointers (Load on Demand)

- **HTML/Raw pipelines, DOM pattern mapping (`(list)struct`, `(table)struct`, `(dict)struct`, `jsonify` extraction patterns), `Cond` condition matrix (`match {}` vs `filter {}` vs `assert {}`), XPath/CSS3/CSS4 rules, and `import`/`define`/`extension` across 9 targets** $\to$ read `references/html-raw.md`
- **REST `@request` formats, `define` URL composition, typed placeholder matrix, `@error[:<suffix>]` dispatch, 5-variant monadic `Result` (`Ok`, `<Struct>Err*`, `DecodeErr`, `UnknownErr`, `TransportErr`), `via_*` transport adapters & 9-target CLI flags** $\to$ read `references/rest.md`
- **Deep `json` / `(dict)json` / inline `@value` post-order hoisting rules, allowlist `rt_project_*` projectors, GJSON path grammar, and `parsmith json-to-kdl`** $\to$ read `references/json.md`
- **Compiler & Linter `DiagnosticCode` (`E000`–`E403`, `W011`–`W041`) fix table, CLI/SDK cheat sheet, and safe repair loop** $\to$ read `references/linter-errors.md`
- **Complete working `.kdl` examples** $\to$ read `references/examples/` (`booksToScrape.kdl`, `hackernews.kdl`, `imdbcom.kdl`, `quotesToScrape.kdl`, `rawParser.kdl`, `regexFallback.kdl`, `dummyjson-rest.kdl`)

# Compiler & Linter Diagnostics Reference

`parsmith` validates `.kdl` schemas ahead of code generation using a 5-pass compiler pipeline. Every diagnostic has a stable code (`E000`–`E403` for fatal compilation errors, `W011`–`W041` for non-fatal lint warnings), source coordinates (`path`, `line`, `col`, `end_line`, `end_col`), and an actionable `hint`.

---

## 1. The 5-Pass Validation Pipeline

When you run `parsmith check <path.kdl>` (or `parsmith.check()` in the Python/JS SDKs), `parsmith` executes five sequential passes:

| Pass | Responsibility | Diagnostic Codes Emitted |
| :--- | :--- | :--- |
| **Pass 1: Import Resolution & KDL 2.0 Parse** | Parses `.kdl` files with `parsmith::kdl`, validates explicit `import` blocks, resolves transitive module graphs via `SourceLoader` (`FsSourceLoader` or `MemorySourceLoader`), and detects missing files or import cycles. | `E000`, `E001`, `E003`, `E200` |
| **Pass 2: Structural & Grammar Lint** | Validates top-level declarations, struct kinds and required directives, CSS3/4 selectors, XPath 1.0 expressions, the **Portable regex subset**, `@request` templates and **Typed placeholders**, `@error[:<suffix>]` discriminators, and `json` / **GJSON path expressions**. | `E001`, `E002`, `E040`, `E041`, `E200`, `E203`, `E400`, `E401`, `E402`, `W011`, `W040`, `W041` |
| **Pass 3: Cross-Reference & Declaration Order Lint** | Verifies intra-file top-down declaration order (`nested`, `jsonify`, JSON schema refs, `@request` `response=`), `@init` precomputed field references (`@<name>`), `define` interpolations, and `extension` calls (`!Namespace.op`). | `E300`, `E301`, `E302` |
| **Pass 4: Target Symbol & Reserved Word Lint** | Checks all schema, struct, field, function, and endpoint identifiers against target language lexical rules, reserved keywords, and case-folded collisions across Python, JavaScript/TypeScript, Go, Rust, Elixir, PHP, C#, and Kotlin. | `E402`, `E403` |
| **Pass 5: Hoisting & Pipeline Type Checking** | Performs post-order hoisting of **Inline JSON schemas** and `@value` blocks into top-level `JsonDef` nodes, binds generic `T` in `extension` signatures, and verifies input/output `TypeInfo` compatibility across every pipeline step and `fallback` literal. | `E001`, `E002`, `E100`, `E203` |

>[!note]
> If Passes 1–4 emit any fatal structural or unresolved-reference errors, `parsmith` halts before Pass 5 AST lowering so cascading false-positive type errors are suppressed.

---

## 2. Running Diagnostics via CLI & JSON Output

```bash
# Human-readable Rust-style annotated source snippets (default)
parsmith check schemas/catalog.kdl

# Scope Pass 4 reserved-keyword checks (E403) to specific target(s)
parsmith check schemas/catalog.kdl -t python -t rust

# Machine-readable JSON payload for CI, editors, and LLM repair loops
parsmith check schemas/catalog.kdl -f json
```

### Text Format (`-f text`)

```text
error[E100]: operation 'to-float' does not accept DOCUMENT; expected STRING
 --> catalog.kdl:3:25
  |
3 |     price { css ".price"; to-float }
  |                           ^^^^^^^^
  |
  = hint: extract a string first with 'text', 'attr', or 'raw'
```

### JSON Format (`-f json`)

```json
{
  "diagnostics": [
    {
      "code": "E100",
      "severity": "error",
      "message": "operation 'to-float' does not accept DOCUMENT; expected STRING",
      "path": "catalog.kdl",
      "line": 3,
      "col": 27,
      "end_line": 3,
      "end_col": 35,
      "span": {
        "offset": 46,
        "len": 8,
        "line": 3,
        "col": 27,
        "end_line": 3,
        "end_col": 35
      },
      "hint": "extract a string first with 'text', 'attr', or 'raw'",
      "notes": []
    }
  ]
}
```

---

## 3. Diagnostic Summary Table

| Code | Symbolic Name | Severity | Primary Cause |
| :--- | :--- | :--- | :--- |
| [`E000`](#e000--syntax_error) | `SYNTAX_ERROR` | Error | Invalid KDL 2.0 syntax (unclosed braces/quotes, bare `true`/`null`, or `//` after `\`). |
| [`E001`](#e001--invalid_argument_count) | `INVALID_ARGUMENT_COUNT` | Error | Wrong positional argument count, `re`/`re-all` without 1 capture group, malformed `import` block syntax (missing `(kind)` tag or empty block), empty `{}` block, anonymous `(array)` inline JSON, or hoisted name collision. |
| [`E002`](#e002--invalid_argument_value) | `INVALID_ARGUMENT_VALUE` | Error | Invalid CSS/XPath/regex syntax (lookarounds, named groups), CSS4 construct not representable in XPath 1.0 when compiling for `php-native`, invalid HTTP status, or invalid placeholder. |
| [`E003`](#e003--invalid_import) | `INVALID_IMPORT` | Error | Missing imported `.kdl` file, circular import chain (`A -> B -> A`), or `import` in inline source without a base file path. |
| [`E040`](#e040--malformed_dotpath) | `MALFORMED_DOTPATH` | Error | Malformed GJSON path in `from="..."` or `jsonify path="..."` (`..`, leading/trailing `.`, wildcards, filters). |
| [`E041`](#e041--duplicate_json_key) | `DUPLICATE_JSON_KEY` | Error | Duplicate field name or duplicate wire path (`from="..."`) in the same `json` schema. |
| [`E100`](#e100--type_mismatch) | `TYPE_MISMATCH` | Error | Pipeline operation input incompatible with previous step output, or `fallback` literal type mismatch. |
| [`E200`](#e200--unknown_node) | `UNKNOWN_NODE` | Error | Unrecognized top-level node, pipeline operation, or condition keyword. |
| [`E203`](#e203--disallowed_directive) | `DISALLOWED_DIRECTIVE` | Error | Operation or directive used in a forbidden context (e.g. HTML ops in `(raw)struct`, XPath in `bs4`/`selectolax`/`go`/`rust`/`elixir`, `len-*` outside `assert`). |
| [`E300`](#e300--undefined_symbol) | `UNDEFINED_SYMBOL` | Error | Reference to an undeclared `struct`, `json`, `define`, `fn`, or `!Namespace.op` extension, or dependency cycle. |
| [`E301`](#e301--undefined_init_field) | `UNDEFINED_INIT_FIELD` | Error | `@<name>` references a field not declared in the struct's `@init` block. |
| [`E302`](#e302--invalid_declaration_order) | `INVALID_DECLARATION_ORDER` | Error | Intra-file forward reference to a `struct` or `json` declared below its caller. |
| [`E400`](#e400--invalid_type_declaration) | `INVALID_TYPE_DECLARATION` | Error | Invalid struct/json type tag, invalid `extension` `sig`, or colliding `@error` variant names. |
| [`E401`](#e401--missing_required_field) | `MISSING_REQUIRED_FIELD` | Error | Struct missing required structural directives (`@split-doc`, `@key`/`@value`, `@table`/`@rows`/`@match`, `@request`). |
| [`E402`](#e402--duplicate_declaration) | `DUPLICATE_DECLARATION` | Error | Duplicate top-level symbol, duplicate struct field/check/request name, or case-folded target collision. |
| [`E403`](#e403--invalid_identifier) | `INVALID_IDENTIFIER` | Error | Invalid identifier syntax or collision with a target language reserved keyword. |
| [`W011`](#w011--deprecated_json_alias) | `DEPRECATED_JSON_ALIAS` | Warning | Deprecated positional wire-key alias on a `json` field (`field str "wire_key"`). |
| [`W040`](#w040--whitespace_in_path) | `WHITESPACE_IN_PATH` | Warning | Leading or trailing whitespace inside a `from="..."` GJSON path. |
| [`W041`](#w041--path_synonym_in_json) | `PATH_SYNONYM_IN_JSON` | Warning | `path="..."` used instead of `from="..."` on a `json` field declaration. |

---

## 4. Detailed Error & Warning Reference

### `E000` — `SYNTAX_ERROR`

**Cause:** The file is not valid KDL 2.0 syntax. Common triggers include unquoted CSS selectors containing special characters, KDL 1.0 booleans/nulls (`true`, `false`, `null` without `#`), missing `;` between single-line operations, or a `//` comment placed immediately after a `\` line continuation.

**Bad KDL:**
```kdl
struct Card {
    title { css "h1" text }
    optional-field { css ".opt"; text; fallback null }
}
```

**Fixed KDL:**
```kdl
struct Card {
    title { css "h1"; text }
    optional-field { css ".opt"; text; fallback #null }
}
```

---

### `E001` — `INVALID_ARGUMENT_COUNT`

**Cause:**
- A pipeline operation or directive received too few or too many positional arguments (e.g., `attr` with 0 arguments, `re-sub` with 1 argument, or an empty `{}` block on a selector `css {}`, condition container, or `@value {}`).
- Pipeline `re` or `re-all` does not contain **exactly 1** unnamed capture group `(...)` (`ADR-0001`).
- Malformed `import` syntax: `import` without a `{ ... }` block, an empty `import` block, a duplicate import path, or an imported symbol missing its mandatory `(kind)` tag (`(struct)`, `(json)`, `(define)`, `(fn)`, `(extension)`).
- An anonymous `(array)` inline JSON field (`items (array) { ... }`) was declared without an explicit item model name.
- Two inline JSON schemas hoist to the same **Synthesized schema name**.

**Bad KDL:**
```kdl
json Order {
    items (array) {
        sku str
    }
}

struct Page {
    title { css {}; text }
    price { css ".price"; text; re #"\d+\.\d+"#; to-float }
}
```

**Fixed KDL:**
```kdl
json Order {
    items (array)OrderItem {
        sku str
    }
}

struct Page {
    title { css { "h1.main"; "h1" }; text }
    price { css ".price"; text; re #"(\d+\.\d+)"#; to-float }
}
```

---

### `E002` — `INVALID_ARGUMENT_VALUE`

**Cause:**
- **Regex validation (Portable regex subset)**:
  - A regex has invalid syntax or uses forbidden lookarounds (`(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`) or named capture groups (`(?P<name>...)`, `(?<name>...)`).
- **Selector validation**: Unbalanced brackets, quotes, or invalid combinators in `css` / `xpath`, or a CSS4 pseudo-class/construct that cannot be lowered to XPath 1.0 when generating for `php` with `--html-lib native` (hint: use an `xpath` step or compile with `--html-lib dom-crawler`).
- **HTTP / REST validation**:
  - `@error` status code is outside `100..599`.
  - `@error:<suffix>` uses an invalid suffix identifier.
  - A **Typed placeholder** uses `[]` or `?` inside a URL path segment (`/items/{{id:int?}}`), or uses `|style` on a non-array placeholder.
- **Naming**: A `define` name is not `UPPER_CASE` (`^[A-Z_][A-Z0-9_-]*$`).

**Bad KDL:**
```kdl
define price_re=#"\d+\.\d+"#

struct Item {
    price { css ".price"; text; re #"(?P<val>\d+\.\d+)"#; to-float }
}
```

**Fixed KDL:**
```kdl
define PRICE-RE=#"(?:\$|USD\s*)(\d+\.\d+)"#

struct Item {
    price { css ".price"; text; re PRICE-RE; to-float }
}
```

---

### `E003` — `INVALID_IMPORT`

**Cause:**
- The `.kdl` file referenced by `import "<path.kdl>"` does not exist in the `SourceLoader` (`FsSourceLoader` or `MemorySourceLoader`).
- Two or more `.kdl` files form a circular `import` chain (`a.kdl -> b.kdl -> a.kdl`).
- An in-memory KDL source string uses `import` without providing a base file `path` or `MemorySourceLoader`.

**Bad KDL:**
```kdl
import "./nonexistent.kdl" {
    (define)PRICE-REGEX
    (json)SellerJson
}
```

**Fixed KDL:**
```kdl
import "./shared.kdl" {
    (define)PRICE-REGEX
    (json)SellerJson
}
```

---

### `E040` — `MALFORMED_DOTPATH`

**Cause:** A `from="..."` attribute on a `json` field or `path="..."` on `jsonify` is not a valid **GJSON path expression** (e.g., empty path, leading or trailing `.`, consecutive `..`, negative array index, wildcard `*`, or GJSON filter `#[...]`).

**Bad KDL:**
```kdl
json Feed {
    first_title str from=".items..0.title"
}
```

**Fixed KDL:**
```kdl
json Feed {
    first_title str from="items.0.title"
    all_titles (array)str from="items.#.title"
    literal_dot str from=#"meta\.version"#
}
```

---

### `E041` — `DUPLICATE_JSON_KEY`

**Cause:** A `json` schema declares two fields with the same field name or maps two non-skipped fields to the same wire key (`from="..."`).

**Bad KDL:**
```kdl
json User {
    id int from="user_id"
    legacy_id int from="user_id"
}
```

**Fixed KDL:**
```kdl
json User {
    id int from="user_id"
    legacy_id int from="legacy_user_id"
}
```

---

### `E100` — `TYPE_MISMATCH`

**Cause:**
- A pipeline step expects an input `TypeInfo` that does not match the output of the preceding step (for example, calling `to-float` or `trim` directly on `DOCUMENT` without `text` or `attr`).
- A string/conversion operation that requires a preceding step is placed at the start of an HTML pipeline.
- `join`, `filter`, `unique`, `index`, `first`, `last`, `slice`, or `len` is called on a scalar instead of an array `T[]`.
- `fallback` literal does not match the pipeline output type (e.g., `fallback 0` on a `STRING` pipeline, `fallback #null` on `BOOL`/`DOCUMENT` or an array pipeline, or `fallback {}` on a scalar pipeline).

**Bad KDL:**
```kdl
struct Product {
    // css returns DOCUMENT, but to-float requires STRING or STRING[]
    price { css ".price"; to-float }
    // text returns STRING, but fallback 0 is INT
    sku { css ".sku"; text; fallback 0 }
}
```

**Fixed KDL:**
```kdl
struct Product {
    price { css ".price"; text; to-float }
    sku { css ".sku"; text; fallback #null }
}
```

---

### `E200` — `UNKNOWN_NODE`

**Cause:** An unrecognized keyword is used at the module top level, inside a pipeline, or inside a `filter` / `assert` / `match` condition block, or an `extension` operation is called without its mandatory `!` prefix (`Utils.slugify` instead of `!Utils.slugify`).

**Bad KDL:**
```kdl
struct Article {
    title { csss "h1"; text }
}
```

**Fixed KDL:**
```kdl
struct Article {
    title { css "h1"; text }
}
```

---

### `E203` — `DISALLOWED_DIRECTIVE`

**Cause:** A valid DSL construct is used in a context or target backend where it is forbidden:
- **HTML operations in raw context**: `css`, `css-all`, `css-remove`, `xpath`, `xpath-all`, `xpath-remove`, `text`, `attr`, or `raw` used inside `(raw)struct` or `(raw)fn`.
- **XPath backend restrictions**: `xpath`, `xpath-all`, `xpath-remove`, or `xpath` condition used when generating for `python` with `-L bs4` or `-L selectolax` (`slax`), `go`, `rust`, or `elixir` (`ex`). (`xpath` is supported in `python` with `-L lxml`/`-L parsel`, `ts`, `js`, `php` (`native` and `dom-crawler`), `csharp`, and `kotlin`).
- **Struct kind restrictions**:
  - Regular fields inside `(dict)struct` or `(rest)struct`.
  - `(table)struct` field not starting with `match { ... }`.
  - `@error` used outside `(rest)struct`.
  - Struct directives (`@init`, `@pre-validate`, `@check`, `@split-doc`, `@request`) used inside `fn`.
- **Condition container restrictions**:
  - `len-*`, `gt`, `lt`, `ge`, `le`, `re-any`, or `re-all` used inside `filter` or `match` (allowed in `assert` only).
  - DOM/attribute/text conditions (`css`, `has-attr`, `attr-*`, `text-*`) used inside `match`.

**Bad KDL:**
```kdl
(table)struct Specs {
    @table { css "table" }
    @rows { css-all "tr" }
    @match { css "th"; text }
    @value { css "td"; text }

    // Missing required leading 'match { ... }' in (table)struct field
    weight { trim }
}
```

**Fixed KDL:**
```kdl
(table)struct Specs {
    @table { css "table" }
    @rows { css-all "tr" }
    @match { css "th"; text }
    @value { css "td"; text }

    weight {
        match { eq "Weight" }
        trim
    }
}
```

---

### `E300` — `UNDEFINED_SYMBOL`

**Cause:** A pipeline or field references a `struct` (`nested`), `json` schema (`jsonify`, `response=`, `@error`), `define`, or `!Namespace.op` extension that has not been declared or imported, or structs/defines form a dependency cycle.

**Bad KDL:**
```kdl
struct Page {
    user { css "#user"; text; jsonify UndeclaredUser }
}
```

**Fixed KDL:**
```kdl
json UserSchema {
    id int
    name str
}

struct Page {
    user { css "#user"; text; jsonify UserSchema }
}
```

---

### `E301` — `UNDEFINED_INIT_FIELD`

**Cause:** A field pipeline starts with `@<name>`, but `<name>` is not declared in the enclosing struct's `@init { ... }` block.

**Bad KDL:**
```kdl
struct Page {
    @init {
        raw-box { css ".box"; text }
    }
    title { @missing-box; trim }
}
```

**Fixed KDL:**
```kdl
struct Page {
    @init {
        raw-box { css ".box"; text }
    }
    title { @raw-box; trim }
}
```

---

### `E302` — `INVALID_DECLARATION_ORDER`

**Cause:** A `struct` or `json` schema is referenced before it is declared in the same `.kdl` file. Intra-file declarations must be ordered top-down (helper schemas and child structs above parent structs).

**Bad KDL:**
```kdl
struct Catalog {
    items { nested ProductCard }
}

(list)struct ProductCard {
    @split-doc { css-all ".card" }
    title { css "h3"; text }
}
```

**Fixed KDL:**
```kdl
(list)struct ProductCard {
    @split-doc { css-all ".card" }
    title { css "h3"; text }
}

struct Catalog {
    items { nested ProductCard }
}
```

---

### `E400` — `INVALID_TYPE_DECLARATION`

**Cause:**
- Unknown type tag on `struct` (expected `item`, `list`, `dict`, `table`, `flat`, `raw`, `rest`) or `json` (expected `array`, `dict`).
- Invalid `sig <In> <Out>` declaration inside an `extension` operation, or missing target implementation block when generating for a target.
- Two `@error` rules on a `(rest)struct` resolve to the same variant name, or the same key is listed in both positional arguments and `key=val` properties on an `@error` rule.

**Bad KDL:**
```kdl
json ErrBody {
    detail str
}

(rest)struct Api {
    @request name="get-item" response=#null "curl https://api.example.com/item"
    // Both rules resolve to ApiErr400Detail
    @error 400 ErrBody "detail"
    @error 400 ErrBody detail="invalid"
}
```

**Fixed KDL:**
```kdl
json ErrBody {
    detail str
}

(rest)struct Api {
    @request name="get-item" response=#null "curl https://api.example.com/item"
    @error:missing-detail 400 ErrBody "detail"
    @error:invalid-detail 400 ErrBody detail="invalid"
}
```

---

### `E401` — `MISSING_REQUIRED_FIELD`

**Cause:** A struct is missing the structural directives required for its kind:
- `struct` / `(item)struct`, `(flat)struct`, `(raw)struct`: at least 1 regular field.
- `(list)struct`: `@split-doc` and at least 1 regular field.
- `(dict)struct`: `@split-doc`, `@key`, and `@value`.
- `(table)struct`: `@table`, `@rows`, `@match`, `@value`, and at least 1 regular field.
- `(rest)struct`: at least 1 `@request` directive.

**Bad KDL:**
```kdl
(list)struct Cards {
    title { css "h2"; text }
}
```

**Fixed KDL:**
```kdl
(list)struct Cards {
    @split-doc { css-all ".card" }
    title { css "h2"; text }
}
```

---

### `E402` — `DUPLICATE_DECLARATION`

**Cause:** Duplicate top-level declaration names (`struct`, `json`, `fn`, `define`, `extension`), duplicate field/`@init`/`@check`/`@request` names inside the same struct, or two identifiers that collide after target-language case conversion (such as `user-id` and `user_id`, both mapping to `user_id` in Python/Rust and `userId` in JS/TS).

**Bad KDL:**
```kdl
struct Profile {
    user-id { css ".id"; text }
    user_id { css ".legacy-id"; text }
}
```

**Fixed KDL:**
```kdl
struct Profile {
    user-id { css ".id"; text }
    legacy-user-id { css ".legacy-id"; text }
}
```

---

### `E403` — `INVALID_IDENTIFIER`

**Cause:**
- A symbol or field name is not a valid identifier (note: `json` field names must match `^[A-Za-z_][A-Za-z0-9_]*$` without hyphens).
- A struct, field, function, or schema name collides with a reserved keyword in one of the checked target languages (`from`, `class`, `def`, `async`, `type`, `match`, `func`, `package`, `fn`, `impl`, etc.).

>[!tip]
> When a JSON wire payload uses a reserved keyword or hyphenated key (such as `"from"`, `"type"`, or `"content-type"`), give the KDL field a safe identifier (`sender`, `kind`, `content_type`) and map the wire key with `from="..."`.

**Bad KDL:**
```kdl
json Message {
    from str
    content-type str
}
```

**Fixed KDL:**
```kdl
json Message {
    sender str from="from"
    content_type str from="content-type"
}
```

---

### `W011` — `DEPRECATED_JSON_ALIAS`

**Cause:** A `json` field uses a legacy positional string argument (`field str "wire_key"`) instead of the `from="wire_key"` property.

**Bad KDL:**
```kdl
json Schema {
    context str "@context"
}
```

**Fixed KDL:**
```kdl
json Schema {
    context str from="@context"
}
```

---

### `W040` — `WHITESPACE_IN_PATH`

**Cause:** A `from="..."` path on a `json` field contains leading or trailing whitespace (`from=" data.id "`).

**Bad KDL:**
```kdl
json Item {
    id int from=" data.id "
}
```

**Fixed KDL:**
```kdl
json Item {
    id int from="data.id"
}
```

---

### `W041` — `PATH_SYNONYM_IN_JSON`

**Cause:** A `json` field uses `path="..."` instead of `from="..."` (`path="..."` is reserved for `jsonify <Schema> path="..."` and top-level `json <Schema> path="..."`).

**Bad KDL:**
```kdl
json Item {
    id int path="data.id"
}
```

**Fixed KDL:**
```kdl
json Item {
    id int from="data.id"
}
```

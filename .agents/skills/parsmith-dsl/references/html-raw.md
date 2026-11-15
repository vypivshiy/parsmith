# HTML & Plain-Text (`(raw)struct`) Reference (DSL v2.1)

## 1. Unified `TypeInfo` Model & Directives

- **Scalar Base Types**: `DOCUMENT` (`DOC`), `STRING` (`STR`), `INT` (64-bit), `FLOAT` (64-bit), `BOOL`, `NULL`, `NESTED` (terminal), `JSON` (terminal).
- **Modifiers**: `T[]` (`is_array=true`), `T?` (`is_optional=true`, produced by `fallback #null` on `STRING`/`INT`/`FLOAT` or `?` in `json`), `dict[K, V]` (in `json`).
- **Pipeline Start Type**: `DOCUMENT` in HTML structs/`fn`; `STRING` in `(raw)struct`/`(raw)fn`; or `TypeOf(name)` when starting with `@<name>`.
- **Struct Directives** (for `item`, `list`, `table`, `dict`, `flat`, `raw`):
  - `@init { <name> { <pipeline> }; ... }` — precomputes & caches values on the parser instance; referenced as **first op** in fields via `@<name>` (`E301` if undeclared).
  - `@pre-validate { assert ["<msg>"] { <conditions> } }` — validates input before `parse()`; raises `RtAssertionError` (`rt::RtError` in Rust).
  - `@check <name> { <pipeline>; to-bool; [fallback #false] }` — emits public boolean method (`is_ok()` / `isOk()` / `IsOk()`); pipeline must end in `BOOL` (`E100`).
  - `@request [name="<kebab-id>"] [response-path="<gjson>"] [response-join="\n"] #"<curl_or_http>"#` — emits transport-agnostic `fetch` / `async_fetch` constructor.

---

## 2. Complete Pipeline Operations Reference

| Category | Signature (`InType -> OutType`) | Rules & Map Semantics |
| :--- | :--- | :--- |
| **Selectors** | `css "<q>" \| css { "<q1>"; "<q2>"; ... } : DOC -> DOC` | Block form requires $\ge 2$ queries (no positional arg); returns first match |
| | `css-all "<q>" \| css-all { "<q1>"; "<q2>"; ... } : DOC -> DOC[]` | Block form ($\ge 2$ queries) returns first non-empty element list |
| | `css-remove "<q>" : DOC -> DOC` | Clones/filters subtree without mutating shared DOM nodes; single positional arg only |
| | `xpath "<q>" \| xpath { "<q1>"; "<q2>"; ... } : DOC -> DOC` | Block form $\ge 2$ queries; rejected (`E203`) in `py -L bs4`, `py -L selectolax`, `go`, `rs`, `ex` |
| | `xpath-all "<q>" \| xpath-all { "<q1>"; "<q2>"; ... } : DOC -> DOC[]` | Block form $\ge 2$ queries; rejected (`E203`) in `py -L bs4`, `py -L selectolax`, `go`, `rs`, `ex` |
| | `xpath-remove "<q>" : DOC -> DOC` | Single arg only; rejected (`E203`) in `py -L bs4`, `py -L selectolax`, `go`, `rs`, `ex` |
| **Extract** | `text : DOC -> STR, DOC[] -> STR[]` | Element text content |
| | `raw [outer\|inner] : DOC -> STR, DOC[] -> STR[]` | Default `outer` (includes element tag); `inner` extracts inner HTML only |
| | `attr "<k>" : DOC -> STR, DOC[] -> STR[]` | Single attribute value |
| | `attr "<k1>" "<k2>" ... : DOC -> STR[], DOC[] -> STR[]` | Multi-attribute extraction ($\ge 2$ args always returns `STR[]`) |
| **String** *(maps over `STR` & `STR[]`)* | `trim ["<chars>"] \| ltrim ["<chars>"] \| rtrim ["<chars>"] : STR -> STR, STR[] -> STR[]` | Strips whitespace or custom character set |
| | `normalize-space : STR -> STR, STR[] -> STR[]` | Collapses whitespace runs to single space and trims |
| | `fmt "<template>" : STR -> STR, STR[] -> STR[]` | Replaces `{{}}` placeholder with current string (and `{{DEFINE_NAME}}`) |
| | `repl "<old>" "<new>" \| repl { "<k1>" "<v1>"; ... } : STR -> STR, STR[] -> STR[]` | Single replacement or ordered multi-pair replacement block |
| | `lower \| upper \| unescape : STR -> STR, STR[] -> STR[]` | Case conversion / HTML entity decoding |
| | `rm-prefix "<s>" \| rm-suffix "<s>" \| rm-prefix-suffix "<s>" : STR -> STR, STR[] -> STR[]` | Strips literal prefix and/or suffix |
| | `split "<sep>" : STR -> STR[], STR[] -> STR[]` | Splits scalar `STR` into `STR[]` or flat-maps over `STR[]` |
| | `join "<sep>" : STR[] -> STR` | Array `STR[]` input only (`E100` on scalar `STR`) |
| **Regex** | `re "<pattern>" : STR -> STR, STR[] -> STR[]` | Requires **exactly 1** capture group (`E001`/`E002`). Raises `RtRegexError` on no-match (caught by `fallback`). Portable flags: `(?i)`, `(?m)`, `(?s)`, `(?x)`, `(?xs)` |
| | `re-all "<pattern>" : STR -> STR[], STR[] -> STR[]` | Requires **exactly 1** capture group (`E001`/`E002`); flat-maps over `STR[]` |
| | `re-sub "<pattern>" "<repl>" : STR -> STR, STR[] -> STR[]` | Regex substitution (0+ capture groups) |
| | `re-split "<pattern>" : STR -> STR[], STR[] -> STR[]` | Splits string by regex pattern (flat-maps over `STR[]`) |
| **Array** | `index <i:int> \| first \| last : T[] -> T` | Requires `T[]` (`E100` on scalars); supports negative indices (`first` = `0`, `last` = `-1`) |
| | `slice <start:int> <end:int> : T[] -> T[]` | Requires `T[]` (`E100` on scalars); sub-list slice supporting negative bounds |
| | `len : T[] -> INT` | Requires `T[]` (`E100` on scalar `STR`; for string length checks in `assert`, use `len-ge` / `len-eq`) |
| | `unique [keep-order=#true] : STR[] -> STR[]` | Deduplicates string list |
| **Conversions** | `to-int : STR -> INT, STR[] -> INT[]` | 64-bit integer |
| | `to-float : STR -> FLOAT, STR[] -> FLOAT[]` | 64-bit float |
| | `to-bool : Any -> BOOL` | Scalar truthiness (mandatory in `@check`) |
| **Structured** | `nested <StructName> : DOC -> NESTED, DOC[] -> NESTED[], STR -> NESTED` | Terminal op. Target must be declared above caller (`E302`). `STR` input valid only when target is `(raw)struct` |
| | `jsonify <SchemaName> [path="<gjson>"] : STR -> JSON, STR[] -> JSON[]` | Terminal op. Projects JSON via `<SchemaName>` (declared above caller, `E302`) |
| **Control** | `filter { <conditions> } : T[] -> T[]` | Retains elements matching all conditions (implicit `AND`) |
| | `assert ["<msg>"] { <conditions> } : T -> T` | Raises `RtAssertionError` on failure (survives `python -O`; caught by `fallback`) |
| | `match { <conditions> } : DOC -> STR` | **Must be first op** in `(table)struct` fields (`E203`) |
| | `fallback <literal \| #null \| {}> : T -> T \| T?` | **Must be last op**. `#null` marks scalar `STR`/`INT`/`FLOAT` optional (`T?`); `{}` requires `T[]` |
| | `@<name> : -> TypeOf(name)` | **Must be first op**; reads cached `@init` field (`E301` if undeclared) |
| | `!<Namespace>.<op> : sig In -> Out` | Invokes typed user extension (`E300` if unknown, `E100` on type mismatch) |

---

## 3. Conditions (`Cond`) & Context Matrix (`match {}` vs `filter {}` vs `assert {}`)

Top-level children of `filter`, `assert`, `match`, `and`, and `not` combine via implicit `AND`; children of `or` combine via `OR`. Multi-value args (`eq "a" "b"`) match if **any** value matches.

| Category | Condition Signatures | `match {}` | `filter {}` | `assert {}` | Input Type |
| :--- | :--- | :---: | :---: | :---: | :--- |
| **String** | `eq <v...>`, `ne <v...>`, `starts <v...>`, `ends <v...>`, `contains <v...>`, `in <v...>`, `re "<pat>"` (0 capture groups required) | ✓ | ✓ | ✓ | `STR` (`eq`/`ne` also `INT`/`FLOAT`/`DOC`/`T[]`) |
| **Attribute** | `has-attr <a...>`, `attr-eq <a> <v...>`, `attr-ne <a> <v...>`, `attr-starts <a> <v...>`, `attr-ends <a> <v...>`, `attr-contains <a> <v...>`, `attr-in <a> <v...>`, `attr-re <a> "<pat>"` | ✗ | ✓ | ✓ | `DOC` / `DOC[]` |
| **Text / DOM** | `text-eq <v...>`, `text-ne <v...>`, `text-starts <v...>`, `text-ends <v...>`, `text-contains <v...>`, `text-in <v...>`, `text-re "<pat>"`, `css "<sel>"`, `xpath "<sel>"` | ✗ | ✓ | ✓ | `DOC` / `DOC[]` |
| **Length** | `len-eq <n...>`, `len-ne <n...>`, `len-gt <n>`, `len-lt <n>`, `len-ge <n>`, `len-le <n>`, `len-range <min> <max>` | ✗ | ✗ | ✓ (`E203` elsewhere) | `STR` or `T[]` |
| **Numeric** | `gt <n>`, `lt <n>`, `ge <n>`, `le <n>` | ✗ | ✗ | ✓ (`E203` elsewhere) | `INT`, `FLOAT`, `STR`, or `T[]` |
| **List Regex** | `re-any "<pat>"` (at least one item matches), `re-all "<pat>"` (every item matches) | ✗ | ✗ | ✓ (`E203` elsewhere) | `STR[]` |

---

## 4. Structural Recipes (`(list)struct`, `(table)struct`, `(dict)struct`, `jsonify`, `(raw)struct`)

### 4.1 Mapping Page Patterns to KDL Constructs

| Page Pattern | Target KDL Construct | Implementation Recipe |
| :--- | :--- | :--- |
| **Repeating cards / product grids / horizontal `<th>` table rows** | `(list)struct Name` | `@split-doc { css-all "<item_selector>" }`. Use relative `css` for scalar fields, `css "td:nth-of-type(k)"` for table columns, `css-all` for list fields, and append `fallback #null` on optional elements. |
| **Key-value table / `<dl>` / label-value rows (fixed keys)** | `(table)struct Name` | `@table { css "<container>" }`, `@rows { css-all "<rows>" }`, `@match { css "<key>"; text; trim; lower }`, `@value { css "<val>"; text; trim }`. Start every field with `match { eq "<key>" }` (or `starts`/`contains`). |
| **Key-value rows or `<meta>` tags (open-ended dynamic keys)** | `(dict)struct Name` | `@split-doc { css-all "<rows>" }`, `@key { css "<key>"; text; trim }`, `@value { css "<val>"; text; trim }` (no regular fields). |
| **Single detail container** | `struct Name` (`(item)struct`) | Select from document root or cache a container node in `@init` and query relative fields via `@container`. |
| **Embedded JSON in `<script>` or `data-*` attribute** | `struct Name` + `json Schema` (`jsonify`) | Extract raw string (`raw inner` / `attr` + `unescape` / `re`) and pipe into `jsonify Schema [path="<gjson>"]`. |

### 4.2 Extracting Embedded JSON (`jsonify`)

| Source Pattern | Extraction Pipeline (`jsonify`) |
| :--- | :--- |
| `<script type="application/ld+json">` | `css "script[type='application/ld+json']"; raw inner; jsonify SchemaName` |
| `<script id="__NEXT_DATA__" type="application/json">` | `css "script#__NEXT_DATA__"; raw inner; jsonify SchemaName path="props.pageProps.items"` |
| `var data = {...};` / `.push({...})` / `JSON.parse("...")` in `<script>` | `css "script..."; raw inner; re #"(?s)..."#; [unescape;] jsonify SchemaName [path="..."]` |
| `<div id="app" data-state="{...}">` | `css "#app"; attr "data-state"; unescape; jsonify SchemaName [path="..."]` |

### 4.3 `(raw)struct` — Plain-Text Parsing (JS, URLs, m3u8, CSV)
Starts with `STRING` (HTML ops forbidden, `E203`). Returns a single object by default, or a list of objects when `@split-doc` is present:

```kdl
(raw)struct PlayerScript {
    @request "curl '{{script_url}}'"
    stream_url { re #"file:\s*[\"']([^\"']+)[\"']"# }
}

(raw)struct CsvRows {
    @split-doc { split "\n" }
    @init { cols { split "," } }
    col_0 { @cols; index 0 }
    col_1 { @cols; index 1 }
}
```

### 4.4 `(table)struct` on Non-Table Label+Value Containers (`kv_rows` / `dl`)

```kdl
(table)struct ProfileInfo {
    @table { css ".profile-data" }
    @rows  { css-all ".info-row" }
    @match { css "strong"; text; trim; lower }
    @value { text; re-sub #"^[^:]+:\s*"# ""; trim }

    registered   { match { starts "registered" } }
    publications { match { starts "publications" }; re #"(\d+)"#; to-int; fallback 0 }
}
```

---

## 5. Module Declarations (`import`, `define`, `extension`) Across 9 Targets

```kdl
import "./shared.kdl" { (extension)Utils; (struct)BookCard; (json)OfferPayload; (define)BASE-URL; (fn)clean-title }
define BASE-URL="https://example.com"
define EXTRACT-HREF { css "a"; attr "href"; trim }

extension Utils {
    slugify {
        sig str str
        py   { emit #"{{out}} = {{in}}.lower().replace(' ', '-')"# }
        js   { emit #"const {{out}} = {{in}}.toLowerCase().replace(/ /g, '-');"# }
        go   { import "strings"; emit #"{{out}} := strings.ToLower(strings.ReplaceAll({{in}}, \" \", \"-\"))"# }
        rust { emit #"let {{out}}: String = {{in}}.to_lowercase().replace(' ', \"-\");"# }
        ex   { emit #"{{out}} = {{in}} |> String.downcase() |> String.replace(\" \", \"-\")"# }
        php  { emit #"${{out}} = str_replace(' ', '-', strtolower(${{in}}));"# }
        cs   { emit #"var {{out}} = {{in}}.ToLowerInvariant().Replace(\" \", \"-\");"# }
        kt   { emit #"val {{out}} = {{in}}.lowercase().replace(\" \", \"-\")"# }
    }
}
```

- **Target blocks in `extension`**: `py`, `ts` (falls back to `js` if omitted), `js`, `go`, `rust`, `ex`, `php`, `cs`, `kt`. Supports `sig <In> <Out>` (including whole-type generic `sig T T` or `sig T bool`), `import "<stmt>"`, lazy deduplicated `helper <name> { [import "...";] source #"..."# }`, and `emit #"<template>"#` (`{{in}}`, `{{out}}`, `{{in_type}}`, `{{out_type}}`).
- **CSS3 (default)**: `[attr]`, `[attr=val]`, `[attr^=val]`, `[attr$=val]`, `[attr*=val]`, `[attr~=word]`, `A > B`, `A + B`, `A ~ B`, `:first-child`, `:last-child`, `:nth-child(n)`, `:nth-of-type(n)`, `:empty`.
- **CSS4 (opt-in)**: `:not(...)`, `:is(...)`, `:where(...)`, `:has(...)` (note: `php --html-lib native` lowers CSS3 to XPath 1.0 and rejects non-lowerable CSS4 with `E002`; pass `--html-lib dom-crawler` for CSS4 in PHP).

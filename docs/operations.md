# Pipeline Operations Reference

A **Pipeline** transforms an initial input (`DOC` in HTML schemas, `STR` in `(raw)` schemas, or `@<name>` from `@init`) through a sequence of strongly-typed operations into a final output value.

Operations can be written on a single line separated by `;` or across multiple lines inside `{ ... }`:

```kdl
struct Product {
    // Single-line pipeline
    title { css "h1.title"; text; trim }

    // Multi-line pipeline
    price {
        css ".price-box"
        text
        re #"(\d+\.\d{2})"#
        to-float
        fallback #null
    }
}
```

---

## 1. Selectors (`DOC -> DOC` / `DOC[]`)

Selector operations query or mutate an HTML DOM tree. They require a scalar `DOC` (`DOCUMENT`) input and are forbidden in `(raw)struct` and `(raw)fn` (`E203`).

| Operation | Signature | Description |
| :--- | :--- | :--- |
| `css "<query>"` | `DOC -> DOC` | Selects the first descendant element matching the CSS selector. Raises a runtime error if no element matches (unless followed by `to-bool` or caught by `fallback`). |
| `css { "<q1>"; "<q2>"; ... }` | `DOC -> DOC` | Block pattern-matching CSS selector (non-empty block, `E001` if empty). Evaluates queries in order and returns the first matching element. |
| `css-all "<query>"` | `DOC -> DOC[]` | Selects all descendant elements matching the CSS selector (returns an empty list if none match). |
| `css-all { "<q1>"; "<q2>"; ... }` | `DOC -> DOC[]` | Block pattern-matching list selector (non-empty block, `E001` if empty). Evaluates queries in order and returns the first non-empty element list (or `[]` if all are empty). |
| `css-remove "<query>"` | `DOC -> DOC` | Removes all descendant elements matching `<query>` from the current DOM subtree and returns the mutated `DOC`. |
| `xpath "<expr>"` | `DOC -> DOC` | Selects the first node matching the XPath 1.0 expression. |
| `xpath { "<q1>"; "<q2>"; ... }` | `DOC -> DOC` | Block pattern-matching XPath selector (non-empty block, `E001` if empty). Returns the first matching node across the listed expressions. |
| `xpath-all "<expr>"` | `DOC -> DOC[]` | Selects all nodes matching the XPath 1.0 expression. |
| `xpath-all { "<q1>"; "<q2>"; ... }` | `DOC -> DOC[]` | Block pattern-matching XPath list selector (non-empty block, `E001` if empty). Returns the first non-empty node list. |
| `xpath-remove "<expr>"` | `DOC -> DOC` | Removes all nodes matching `<expr>` from the current DOM subtree and returns the mutated `DOC`. |

### Block Pattern-Matching Selectors

`css`, `css-all`, `xpath`, and `xpath-all` accept a `{ ... }` children block containing fallback selector alternatives instead of a positional argument:

```kdl
struct Article {
    title {
        css {
            "h1.article-headline"
            "header h1"
            "meta[property='og:title']"
        }
        text
        trim
    }

    paragraphs {
        css-all {
            "article.post-body > p"
            "div.entry-content > p"
        }
        text
    }
}
```

**Rules for block selectors:**
- Specify **either** a single positional query (`css ".a"`) **or** a non-empty children block (`css { ".a"; ".b" }`), never both (`E001`).
- Each entry in the block is a bare string node (`".a"`, `"//h1"`) or a scalar `define` identifier without arguments, properties, or child blocks (`E001`).
- Queries are tried sequentially at runtime; the first non-empty match wins.
- `css-remove` and `xpath-remove` accept only a single positional argument (`E001` on blocks).

### Target Backend Support for XPath (`ADR-0002`)

Not all HTML DOM libraries support XPath evaluation. `parsmith` enforces XPath capability at compile time (`E203`) rather than emulating XPath via lossy HTML re-serialization:

| Target / Flag | DOM Engine | CSS Support | XPath Support |
| :--- | :--- | :--- | :--- |
| `python -L bs4` *(default)* | `beautifulsoup4` | Yes | **Rejected (`E203`)** — use `-L lxml` or `-L parsel` |
| `python -L selectolax` | `selectolax` (`Lexbor`) | Yes | **Rejected (`E203`)** — use `-L lxml` or `-L parsel` |
| `python -L lxml` | `lxml.html` | Yes (`cssselect`) | **Yes** |
| `python -L parsel` | `parsel` | Yes | **Yes** |
| `ts` / `js` | Standard DOM (`querySelector` / `document.evaluate`) | Yes | **Yes** |
| `go` | `goquery` | Yes | **Rejected (`E203`)** |
| `rust` | `dom_query` | Yes | **Rejected (`E203`)** |

>[!warning]
>When targeting Python, `parsmith generate python` defaults to `-L bs4` (`Bs4Spelling`), which rejects `xpath`, `xpath-all`, `xpath-remove`, and `xpath` conditions with `E203` (`ADR-0002`). Pass `-L lxml` or `-L parsel` when compiling schemas that use XPath.

### Existence Checks with `css`/`xpath` + `to-bool` (`ADR-0004`)

When a single-element `css` or `xpath` operation is **immediately followed** by `to-bool`, the generated code checks whether the element exists (`el is not None` / `el !== null` / `sel.Length() > 0` / `!nodes.is_empty()`) without raising a missing-element exception:

```kdl
struct ProductCard {
    @check is-available {
        css ".in-stock-badge"
        to-bool
    }
    has_video {
        css "video.preview"
        to-bool
    }
}
```

>[!tip]
>In `lxml`, `parsmith` explicitly emits `v is not None` for `to-bool` on `DOCUMENT` nodes rather than `bool(v)`, avoiding `lxml.html.HtmlElement.__bool__` (which checks `len(children) > 0` and fails on leaf elements like `<img/>` or `<br/>`).

---

## 2. Extract Operations (`DOC -> STR` / `STR[]`)

Extract operations convert DOM elements (`DOC` or `DOC[]`) into strings (`STR` or `STR[]`). They are forbidden in `(raw)struct` and `(raw)fn` (`E203`).

| Operation | Signature | Description |
| :--- | :--- | :--- |
| `text` | `DOC -> STR`<br>`DOC[] -> STR[]` | Extracts the concatenated text content of the element and its descendants. |
| `raw` / `raw outer` | `DOC -> STR`<br>`DOC[] -> STR[]` | Serializes the outer HTML of the element (including the element's own tag). |
| `raw inner` | `DOC -> STR`<br>`DOC[] -> STR[]` | Serializes the inner HTML of the element's children (excluding the element's own outer tag). |
| `attr "<key>"` | `DOC -> STR`<br>`DOC[] -> STR[]` | Extracts the value of attribute `<key>`. Raises a runtime error if `<key>` is missing on any target element (caught by `fallback`). |
| `attr "<k1>" "<k2>" ...` | `DOC -> STR[]`<br>`DOC[] -> STR[]` | Multi-attribute extraction ($\ge 2$ keys). Always returns `STR[]`, collecting present attributes in key order and skipping missing attributes. |

```kdl
struct MediaCard {
    // Single-key attr: DOC -> STR (falls back to #null if data-id is absent)
    card_id {
        attr "data-id"
        fallback #null
    }

    // Multi-key attr: DOC -> STR[] (collects whichever attributes exist)
    image_candidates {
        css "img.cover"
        attr "data-hi-res" "data-src" "src"
    }

    // Inner HTML markup: DOC -> STR
    description_html {
        css ".description"
        raw inner
        trim
    }
}
```

>[!note]
>Across all Python `DomSpelling` backends (`bs4`, `lxml`, `parsel`, `selectolax`), single-key `attr "<key>"` normalizes multi-valued attributes such as `class` into a single space-joined `str` and raises `KeyError` when the attribute is absent so `fallback` triggers identically across all targets (`ADR-0004`).

---

## 3. String Operations (`STR -> STR` / `STR[]`)

All string operations (except `split` and `join`) automatically map over `STR[]` when the pipeline holds an array of strings.

| Operation | Signature | Description |
| :--- | :--- | :--- |
| `trim ["<chars>"]` | `STR -> STR`<br>`STR[] -> STR[]` | Strips leading and trailing whitespace, or any character in `<chars>` when an argument is provided. |
| `ltrim ["<chars>"]` | `STR -> STR`<br>`STR[] -> STR[]` | Strips leading whitespace (or characters in `<chars>`). |
| `rtrim ["<chars>"]` | `STR -> STR`<br>`STR[] -> STR[]` | Strips trailing whitespace (or characters in `<chars>`). |
| `normalize-space` | `STR -> STR`<br>`STR[] -> STR[]` | Collapses all runs of whitespace (`\s+`) into a single space `" "` and trims leading/trailing whitespace. |
| `fmt "<template>"` | `STR -> STR`<br>`STR[] -> STR[]` | Formats the current string into `<template>`, replacing every `{{}}` placeholder with the current value (`E001` if `{{}}` is missing). |
| `repl "<old>" "<new>"` | `STR -> STR`<br>`STR[] -> STR[]` | Replaces all occurrences of literal substring `<old>` with `<new>`. |
| `repl { "<old>" "<new>"; ... }` | `STR -> STR`<br>`STR[] -> STR[]` | Block replacement map: applies each `"<old>" "<new>"` replacement pair sequentially in declaration order. |
| `lower` | `STR -> STR`<br>`STR[] -> STR[]` | Converts characters to lowercase. |
| `upper` | `STR -> STR`<br>`STR[] -> STR[]` | Converts characters to uppercase. |
| `rm-prefix "<prefix>"` | `STR -> STR`<br>`STR[] -> STR[]` | Strips literal `<prefix>` from the start of the string if present. |
| `rm-suffix "<suffix>"` | `STR -> STR`<br>`STR[] -> STR[]` | Strips literal `<suffix>` from the end of the string if present. |
| `rm-prefix-suffix "<str>"` | `STR -> STR`<br>`STR[] -> STR[]` | Strips literal `<str>` from both the start and end of the string if present. |
| `unescape` | `STR -> STR`<br>`STR[] -> STR[]` | Decodes HTML entities (`&amp;`, `&#39;`, `&quot;`) and Unicode/hex escape sequences (`\uXXXX`, `\xXX`). |
| `split "<sep>"` | `STR -> STR[]`<br>`STR[] -> STR[]` | Splits a string by literal delimiter `<sep>` into a list of strings (flat-maps when input is `STR[]`). |
| `join "<sep>"` | `STR[] -> STR` | Joins an array of strings into a single string separated by `<sep>`. Requires `STR[]` input (`E100` on scalar `STR`). |

```kdl
define BASE-URL="https://shop.example.com"

struct Offer {
    url {
        css "a.offer-link"
        attr "href"
        trim
        fmt "{{BASE-URL}}{{}}"
    }

    sku {
        css ".sku-label"
        text
        rm-prefix "SKU:"
        trim " #-"
        upper
    }

    clean_summary {
        css ".summary"
        text
        unescape
        repl {
            "\u{00a0}" " "
            "N/A" ""
        }
        normalize-space
    }
}
```

---

## 4. Regex Operations & `RtRegexError`

All regex patterns are validated at compile time against the **Portable regex subset** (`E002`). Use KDL raw strings (`#"..."#`) so regex backslashes (`\d`, `\s`, `\.`) do not need double escaping.

| Operation | Signature | Capture Group Rule & Behavior |
| :--- | :--- | :--- |
| `re "<pattern>"` | `STR -> STR`<br>`STR[] -> STR[]` | **Requires exactly 1 capturing group `(...)` (`E001`).** Extracts the substring matched by group `1` on the first match. Raises `RtRegexError` / `rt::RtError` if the pattern does not match. |
| `re-all "<pattern>"` | `STR -> STR[]`<br>`STR[] -> STR[]` | **Requires exactly 1 capturing group `(...)` (`E001`).** Extracts group `1` from all non-overlapping matches (returns `[]` if none match; never raises on no-match). |
| `re-sub "<pattern>" "<repl>"` | `STR -> STR`<br>`STR[] -> STR[]` | Replaces all matches of `<pattern>` with `<repl>`. Supports numbered capture group references (`\1` or `$1`) in `<repl>`. |
| `re-split "<pattern>"` | `STR -> STR[]`<br>`STR[] -> STR[]` | Splits the string by matches of `<pattern>` into `STR[]`. |

```kdl
struct ReleaseInfo {
    // Extracts capture group 1 ("2.4.1")
    version {
        css ".release-header"
        text
        re #"v(\d+\.\d+\.\d+)"#
    }

    // Extracts all @mentions as STR[]
    mentions {
        css ".release-notes"
        text
        re-all #"@([a-zA-Z0-9_-]+)"#
    }

    // Normalizes phone digits using group references
    phone {
        css ".contact"
        text
        re-sub #"[^\d]+"# ""
    }
}
```

### Runtime Contract: `RtRegexError` (`ADR-0009`)

When a pipeline `re "<pattern>"` operation fails to match at runtime, `parsmith`'s runtime helper (`rt_re_search` in Python, `rtReSearch` in TypeScript/JavaScript/Go, `rt::re_search` in Rust) reports a structured failure with source location context:

```text
<file.kdl>:<line>:<col> re-match failed at <Struct>.<field>: pattern=<pattern>
```

- **With `fallback`**: If the pipeline ends with `fallback <default>`, the no-match error is caught automatically and the fallback value is returned.
- **Without `fallback`**:
  - **Python**: Raises `RtRegexError` (a subclass of `ValueError`).
  - **TypeScript / JavaScript**: Throws `RtRegexError` (a subclass of `Error`).
  - **Go**: Panics with `"parsmith: <diagnostic>"` (recovered automatically when `fallback` is present via `rtFallback`).
  - **Rust**: Returns `Err(rt::RtError)` from `.parse()`.

---

## 5. Array Operations (`T[] -> T` / `T[]` / `INT`)

Array operations require a list input (`is_array = true`, `E100` on scalars).

| Operation | Signature | Description |
| :--- | :--- | :--- |
| `index <i:int>` | `T[] -> T` | Extracts the element at 0-based index `<i>`. Supports negative indices (`-1` is the last element). Raises a runtime out-of-bounds error if the list is too short (caught by `fallback`). |
| `first` | `T[] -> T` | Shorthand for `index 0`. Extracts the first element of the list. |
| `last` | `T[] -> T` | Shorthand for `index -1`. Extracts the last element of the list. |
| `slice <start:int> <end:int>` | `T[] -> T[]` | Returns the sub-list from `<start>` (inclusive) to `<end>` (exclusive). Supports negative indices on both bounds. |
| `len` | `T[] -> INT` | Returns the number of elements in the list as a 64-bit integer. |
| `unique [keep-order=#true]` | `STR[] -> STR[]` | Deduplicates a list of strings (`E100` if input is not `STR[]`). Preserves first-seen order when `keep-order=#true` is specified (or by default in Go/Rust/JS). |

```kdl
struct SearchResults {
    total_shown {
        css-all ".result-item"
        len
    }

    top_three_titles {
        css-all ".result-item h3"
        text
        trim
        slice 0 3
    }

    distinct_authors {
        css-all ".result-item .author"
        text
        trim
        unique keep-order=#true
    }
}
```

---

## 6. Conversions (`STR -> INT` / `FLOAT`, `Any -> BOOL`)

| Operation | Signature | Description |
| :--- | :--- | :--- |
| `to-int` | `STR -> INT`<br>`STR[] -> INT[]` | Parses a base-10 integer string into a 64-bit signed integer (`INT`). Raises a runtime parse error on invalid input (caught by `fallback`). |
| `to-float` | `STR -> FLOAT`<br>`STR[] -> FLOAT[]` | Parses a floating-point string into a 64-bit float (`FLOAT`). Raises a runtime parse error on invalid input (caught by `fallback`). |
| `to-bool` | `Any -> BOOL` | Converts the current pipeline value into a scalar `BOOL`. Mandatory as the final conversion in `@check` pipelines (`E100`). |

**`to-bool` truthiness rules across pipeline types:**
- After `css "<q>"` or `xpath "<q>"` (`DOC`): `true` if the element exists (`not None` / `!== null`), `false` if absent (never raises on missing element).
- On `DOC[]` or `T[]`: `true` if the list is non-empty (`len > 0`).
- On `STR`: `true` if the string is non-empty (`s != ""`).
- On `INT` / `FLOAT`: `true` if non-zero (`n != 0` / `n != 0.0`).

---

## 7. Structured Terminal Operations (`nested`, `jsonify`)

`nested` and `jsonify` delegate extraction to another declared `struct` or `json` schema. Both are **terminal operations** (no transformation steps may follow them except a terminal `fallback`).

| Operation | Signature | Description |
| :--- | :--- | :--- |
| `nested <StructName>` | `DOC -> NESTED(<StructName>)`<br>`STR -> NESTED(<StructName>)` | Instantiates `<StructName>` on the current `DOC` (or `STR` when `<StructName>` is a `(raw)struct`) and returns its parsed output. If `<StructName>` is a `(list)struct` or `(flat)struct`, the resulting `TypeInfo` has `is_array = true`. |
| `jsonify <SchemaName> [path="<gjson>"]` | `STR -> JSON(<SchemaName>)`<br>`STR[] -> JSON(<SchemaName>)[]` | Parses the current string (or list of strings) as JSON, optionally navigates to `path="<gjson>"` using a **GJSON path expression**, and projects the value through `<SchemaName>`'s strict allowlist **JSON projector**. |

### Intra-File Declaration Order Invariant (`E302`)

Within a single `.kdl` file, any `struct` referenced by `nested <StructName>` or `json` schema referenced by `jsonify <SchemaName>` **must be declared above** the struct or function that references it (`E302`):

```kdl
// 1. Declare referenced json and helper struct FIRST
json PriceMeta {
    amount float
    currency str
}

struct AuthorBadge {
    name { css ".name"; text; trim }
    profile_url { css "a"; attr "href" }
}

// 2. Reference them in the parent struct BELOW
struct BookPage {
    author {
        css ".author-box"
        nested AuthorBadge
    }
    pricing {
        css "script#pricing-data"
        text
        jsonify PriceMeta path="offer.pricing"
    }
}
```

>[!note]
>When `nested` targets a standard HTML `struct`, the current pipeline type must be scalar `DOC` (`E100` on `DOC[]` or `STR`). When `nested` targets a `(raw)struct`, the current pipeline type must be scalar `STR` (`E100` on `DOC` — extract text with `text`, `raw`, or `attr` first).

---

## 8. Control, Reference & Extension Operations

| Operation | Signature | Rules & Placement Constraints |
| :--- | :--- | :--- |
| `filter { <conds> }` | `T[] -> T[]` | Retains elements of `DOC[]` or `STR[]` that satisfy the inner `Cond` block (`E100` if input is not an array). See [Condition Expressions (`Cond`)](conditions.md). |
| `assert ["<msg>"] { <conds> }` | `T -> T` | Pass-through validation step. Raises `RtAssertionError` / `rt::RtError` if the inner `Cond` block evaluates to `false`. See [Condition Expressions (`Cond`)](conditions.md). |
| `match { <conds> }` | `DOC -> STR` | **Mandatory first step** in regular fields of a `(table)struct` (`E203` elsewhere or if not first). Matches the table row's `@match` key against `<conds>` and yields the row's extracted `@value` string. |
| `fallback <val \| #null \| {}>` | `T -> T \| T?`<br>`T[] -> T[]` | **Must be the last operation** in a pipeline (`E203`). Catches runtime extraction/assertion/regex errors and returns the default value. |
| `@<name>` | `-> TypeOf(<name>)` | **Must be the first operation** in a field pipeline (`E203` if placed later or inside `fn`). Loads the cached value of `<name>` from the struct's `@init` block (`E301` if `<name>` is not declared in `@init`). |
| `<BLOCK-DEFINE>` | `In -> Out` | Inline expansion of a `define NAME { ... }` block. Spliced before type checking. |
| `!<Namespace>.<op>` | `sig In -> Out` | Invokes a user-defined **Extension operation** declared in `extension <Namespace> { ... }`. The leading `!` prefix is mandatory (`E001` if omitted, `E300` if undeclared, `E100` on input type mismatch). |

>[!warning]
>The legacy `self <name>` syntax from DSL v1 is rejected with `E203`. Always reference `@init` precomputed fields with `@<name>`.

---

## 9. Central Compiler Guards (`E001` & `E100`)

The compiler enforces two central structural guards across all pipeline operations:

1. **Argument & Block Arity (`E001` — `INVALID_ARGUMENT_COUNT`)**:
   - No-argument operations (`first`, `last`, `len`, `lower`, `normalize-space`, `text`, `to-bool`, `to-float`, `to-int`, `unescape`, `unique`, `upper`) reject any positional arguments or child blocks (`unique` accepts only the `keep-order=#true|#false` property).
   - Fixed-arity operations (`css-remove`, `xpath-remove`, `rm-prefix`, `rm-suffix`, `rm-prefix-suffix`, `fmt`, `split`, `join`, `re`, `re-all`, `re-split`, `index`, `jsonify`, `nested` require `1` arg; `re-sub` and `slice` require `2` args) reject missing or extra positional arguments.
   - Dual-form operations (`css`, `css-all`, `xpath`, `xpath-all`, `repl`) require either positional argument(s) or a non-empty `{ ... }` block, never both.
2. **Missing Producer on HTML Pipelines (`E100` — `TYPE_MISMATCH`)**:
   - In HTML schemas (`struct` and `fn`), the pipeline starts with `DOC`. Placing a string, regex, array, conversion, or `fallback` operation (`trim`, `lower`, `fmt`, `re`, `split`, `to-int`, `to-float`, `to-bool`, `fallback`, etc.) at the start of a pipeline without first extracting a string or selecting a node raises `E100` with the hint:
     `add 'text', 'raw', or 'attr' before this operation to extract a string` (or `add a producer first, e.g. 'css ".x"; text'`).

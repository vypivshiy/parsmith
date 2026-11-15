# Type System, Pipeline Flow & Validation Rules

`parsmith` enforces a static type system (`TypeInfo`) across all field pipelines, `@init` precomputed fields, `@check` methods, standalone functions (`fn` and `(raw)fn`), and JSON schemas. During Pass 5 of compilation (`parsmith check`), the compiler infers the output type of every pipeline step and verifies type compatibility (`E100`) before any target code is generated.

---

## 1. Unified `TypeInfo` Model

Every pipeline step, struct field, function return value, and JSON field is represented in the compiler's **Algebraic AST** by a unified `TypeInfo` descriptor combining a scalar or structural `BaseType` with cardinality (`is_array`), nullability (`is_optional`), and dictionary (`is_dict`) modifiers.

### Base Scalar Types (`BaseType`)

| DSL Name | Shorthand | Description |
| :--- | :--- | :--- |
| `DOCUMENT` | `DOC` | An HTML/XML DOM document or element node. |
| `STRING` | `STR` | A UTF-8 text string. |
| `INT` | `INT` | A 64-bit signed integer (`i64` / `int64` / `int` / `number`). |
| `FLOAT` | `FLOAT` | A 64-bit IEEE-754 floating-point number (`f64` / `float64` / `float` / `number`). |
| `BOOL` | `BOOL` | A boolean truth value (`#true` or `#false`). |
| `NULL` | `NULL` | A null/empty value (`#null`), used in void REST responses or `@pre-validate`. |
| `NESTED(<Struct>)` | `NESTED` | **Terminal type.** The parsed output of a nested `struct` referenced via `nested <StructName>`. |
| `JSON(<Schema>)` | `JSON` | **Terminal type.** The projected output of a `json` schema referenced via `jsonify <SchemaName>`. |
| `AUTO` | `AUTO` | Compile-time placeholder used during type inference for **Block defines** and unresolved `@init` references. |

### `TypeInfo` Modifiers

Rather than defining separate `LIST_*` or `OPT_*` enum variants, `TypeInfo` composes orthogonally over any `BaseType`:

- **Array (`T[]`, `is_array = true`)**: A homogeneous list of elements (`DOC[]`, `STR[]`, `INT[]`, `FLOAT[]`, `NESTED[]`, `JSON[]`). Produced by array-introducing pipeline steps (`css-all`, `xpath-all`, `split`, `re-split`, `re-all`, multi-argument `attr`) or declared in `json` schemas with `(array)<type>`.
- **Optional / Nullable (`T?`, `is_optional = true`)**: A value that may resolve to `null` / `None` / `nil`. Produced in pipelines by a terminal `fallback #null` step or declared in `json` schemas with a trailing `?` (`str?`, `int?`) or `@omitempty`.
- **Dictionary (`dict[K, V]`, `is_dict = true`)**: A homogeneous key-value map carrying `key_type` (`K`, defaulting to `STR`) and `value_type` (`V`). Produced by `(dict)struct` parsers or declared in `json` schemas via `(dict)json` and inline `(dict)` fields.

### Cross-Target Type Mapping

`parsmith` maps each resolved `TypeInfo` to idiomatic static types across all five target backends:

| `TypeInfo` | Python (`typeddict` / `dataclass`) | TypeScript (`.ts`) | JavaScript (`.js` + `.d.ts`) | Go (`go`) | Rust (`rust`) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `DOC` | `Tag` / `HtmlElement` / `Selector` / `LexborNode` | `Document \| Element` | `Document \| Element` | `*goquery.Selection` | `rt::Nodes` |
| `DOC[]` | `list[Tag]` / `list[HtmlElement]` / `SelectorList` / `list[LexborNode]` | `Element[]` | `Element[]` | `*goquery.Selection` | `rt::Nodes` |
| `STR` | `str` | `string` | `string` | `string` | `String` |
| `INT` | `int` | `number` | `number` | `int64` | `i64` |
| `FLOAT` | `float` | `number` | `number` | `float64` | `f64` |
| `BOOL` | `bool` | `boolean` | `boolean` | `bool` | `bool` |
| `NULL` | `None` | `null` | `null` | `any` | `()` |
| `T[]` | `list[T]` | `T[]` | `T[]` | `[]T` | `Vec<T>` |
| `T?` | `T \| None` | `T \| null` | `T \| null` | `*T` | `Option<T>` |
| `T[]?` | `list[T] \| None` | `T[] \| null` | `T[] \| null` | `*[]T` | `Option<Vec<T>>` |
| `NESTED(S)` *(item/table/dict)* | `<S>Type` | `<S>Type` | `<S>Type` | `<S>Type` | `<S>Type` |
| `NESTED(S)` *(list)* | `list[<S>Type]` | `<S>Type[]` | `<S>Type[]` | `[]<S>Type` | `Vec<<S>Type>` |
| `NESTED(S)` *(flat)* | `list[str]` | `string[]` | `string[]` | `[]string` | `Vec<String>` |
| `JSON(J)` *(object)* | `<J>Json` | `<J>Json` | `<J>Json` | `<J>Json` | `<J>Json` |
| `JSON(J)` *((array)json)* | `list[<J>Json]` | `<J>Json[]` | `<J>Json[]` | `[]<J>Json` | `Vec<<J>Json>` |
| `dict[K, V]` | `dict[K, V]` | `Record<K, V>` | `Record<K, V>` | `map[K]V` | `std::collections::HashMap<K, V>` |

>[!note]
>In JavaScript (`parsmith generate js`), `parsmith` emits a clean ES module (`.js`) paired with a companion TypeScript declaration file (`.d.ts`) containing the exact TypeScript interfaces and type aliases shown above.

---

## 2. Pipeline Type Flow & Implicit Array Mapping

A **Pipeline** is a left-to-right (or top-to-bottom) sequence of operations where the output `TypeInfo` of step $i$ becomes the input `TypeInfo` of step $i + 1$.

### Initial Pipeline Input Type

Every pipeline starts with a well-defined input type determined by its enclosing construct:

1. **HTML schemas (`struct`, `(item|list|dict|table|flat)struct`, `fn`)**: Fields and directives start with scalar `DOC` (`DOCUMENT`). In `(list)struct` and `(dict)struct`, `@split-doc` transforms the root `DOC` into `DOC[]`, and each regular field or `@key`/`@value` pipeline executes once per split item with scalar `DOC` input.
2. **Raw schemas (`(raw)struct`, `(raw)fn`)**: Fields and directives start with scalar `STR` (`STRING`). HTML DOM operations (`css*`, `xpath*`, `text`, `raw`, `attr`) are rejected at compile time with `E203`.
3. **Precomputed field reference (`@<name>`)**: When a field pipeline begins with `@<name>`, its initial type is replaced by the resolved output `TypeInfo` of the `<name>` pipeline declared in the struct's `@init` block.
4. **Block `define` (`define NAME { ... }`)**: Starts with `AUTO` until spliced into a concrete caller pipeline, where it is type-checked against the caller's current `TypeInfo`.

### Implicit Array Mapping (Lifting `T -> U` to `T[] -> U[]`)

Most extraction, string transformation, regex, and type conversion operations propagate list cardinality automatically (`list_propagates = true`). When applied to an array `T[]`, the operation maps element-wise over the list and returns `U[]`:

```kdl
struct Article {
    // Scalar flow: DOC -> DOC -> STR -> STR -> INT
    views {
        css ".view-count"       // DOC -> DOC
        text                    // DOC -> STR
        re #"(\d+)"#            // STR -> STR
        to-int                  // STR -> INT
    }

    // Implicit array mapping: DOC -> DOC[] -> STR[] -> STR[] -> INT[]
    milestone_years {
        css-all "ul.timeline li.year" // DOC   -> DOC[]
        text                          // DOC[] -> STR[]
        trim                          // STR[] -> STR[]
        to-int                        // STR[] -> INT[]
    }
}
```

### Cardinality Transitions: Producers, Preservers, and Reducers

Pipeline operations fall into four cardinality categories:

| Category | Operations | Cardinality Behavior |
| :--- | :--- | :--- |
| **Scalar-only DOM selectors** | `css`, `xpath`, `css-remove`, `xpath-remove` | Require scalar `DOC` input (`DOC -> DOC`). Applying a selector directly to `DOC[]` raises `E100`. |
| **Array-introducing (Producers)** | `css-all`, `xpath-all` (`DOC -> DOC[]`)<br>`split`, `re-split`, `re-all` (`STR -> STR[]`)<br>`attr "<k1>" "<k2>" ...` ($\ge 2$ keys: `DOC -> STR[]`, `DOC[] -> STR[]`) | Always produce an array (`is_array = true`). Note that `split`, `re-split`, and `re-all` accept scalar `STR` (`STR -> STR[]`) or map over `STR[]` (`STR[] -> STR[]` via flat-mapping). |
| **Array-preserving (List filters/modifiers)** | `filter { ... }` (`T[] -> T[]`)<br>`slice <start> <end>` (`T[] -> T[]`)<br>`unique` (`STR[] -> STR[]`) | Require an array input (`T[]`, `E100` on scalars) and return an array of the same element type. |
| **Array-reducing (Reducers)** | `index <i>`, `first`, `last` (`T[] -> T`)<br>`len` (`T[] -> INT`)<br>`join "<sep>"` (`STR[] -> STR`)<br>`to-bool` (`Any -> BOOL`) | Collapse an array into a scalar (`is_array = false`). `index`, `first`, `last`, `len`, and `join` require an array input (`E100` on scalars). |

```kdl
struct Breadcrumbs {
    // DOC -> DOC[] -> STR[] -> STR[] -> STR
    category_path {
        css-all "nav.breadcrumb a"  // DOC   -> DOC[]
        text                        // DOC[] -> STR[]
        trim                        // STR[] -> STR[]
        slice 1 -1                  // STR[] -> STR[] (drop home and current page)
        join " > "                  // STR[] -> STR   (reduce list to scalar string)
    }

    // DOC -> DOC[] -> DOC -> STR
    last_crumb {
        css-all "nav.breadcrumb li" // DOC   -> DOC[]
        last                        // DOC[] -> DOC   (reduce list to last element)
        text                        // DOC   -> STR
        trim                        // STR   -> STR
    }
}
```

---

## 3. Optional Types & `fallback` Rules

By default, every pipeline step is strict: if a `css` or `xpath` selector matches no element, `attr` is missing, `re` fails to match, `index` is out of bounds, `to-int`/`to-float` fails to parse, or `assert` fails, the pipeline aborts with a runtime error.

Adding a terminal `fallback` step wraps the pipeline in an error handler that catches any runtime extraction failure and returns the specified default value.

### Rule 1: `fallback` Must Be Terminal and Preceded by a Producer

- `fallback` **must** be the final operation in a pipeline. Placing any operation after `fallback` raises `E203` (`'fallback' must be the last operation in a pipeline`).
- In HTML schemas (`DOC` input), placing `fallback` as the only operation in an empty pipeline raises `E100` (`'fallback' requires a preceding operation`).

### Rule 2: Typed Scalar Literals (`T -> T`)

When `fallback` is given a concrete scalar literal (`"..."`, `0`, `0.0`, `#true`, `#false`), its type must match the pipeline's current scalar output type (`E100`), and the field's output type remains non-nullable (`T`):

| Current Pipeline Type | Valid Literal Fallbacks | Resulting Output Type |
| :--- | :--- | :--- |
| `STR` | `fallback ""`, `fallback "N/A"`, `fallback MY_DEFINE` | `STR` (non-null) |
| `INT` | `fallback 0`, `fallback -1` | `INT` (non-null) |
| `FLOAT` | `fallback 0.0`, `fallback -1.0` | `FLOAT` (non-null) |
| `BOOL` | `fallback #false`, `fallback #true` | `BOOL` (non-null) |

Passing a scalar literal to an array pipeline (e.g. `css-all ".tag"; text; fallback ""`) or passing a mismatched literal type (e.g. `to-int; fallback "0"`) raises `E100`.

### Rule 3: Nullable Scalar Fallback (`fallback #null` $\to$ `T?`)

Passing `#null` to `fallback` marks the pipeline output as optional (`is_optional = true`, `T?`):

```kdl
struct Product {
    subtitle {
        css "p.subtitle"
        text
        trim
        fallback #null      // STR -> STR? (str | None, string | null, *string, Option<String>)
    }
    discount_price {
        css ".price-sale"
        text
        re #"(\d+\.\d{2})"#
        to-float
        fallback #null      // FLOAT -> FLOAT?
    }
}
```

- `fallback #null` is valid on scalar `STR`, `INT`, `FLOAT`, `NESTED`, and `JSON` pipelines.
- Using `fallback #null` on `BOOL` or `DOCUMENT` raises `E100` (`'fallback #null' only valid for STRING/INT/FLOAT`). For boolean pipelines, use `fallback #false` or `fallback #true`.

### Rule 4: Empty Array Fallback (`fallback {}` $\to$ `T[]`)

For array pipelines (`T[]`), `fallback {}` provides an empty list (`[]` / `Vec::new()`) if an intermediate step fails:

```kdl
struct Article {
    // Splits a comma-separated tag string if the container exists; otherwise returns []
    tags {
        css ".tags-line"    // DOC -> DOC (raises if .tags-line is missing)
        text                // DOC -> STR
        split ","           // STR -> STR[]
        trim                // STR[] -> STR[]
        fallback {}         // STR[] -> STR[] (catches missing .tags-line and returns [])
    }
}
```

- `fallback {}` is valid **only** when the current pipeline type is an array `T[]`.
- Using `fallback {}` on a scalar pipeline raises `E100` (`'fallback {}' is only valid for list types`).
- The braces in `fallback {}` must be empty and take no positional arguments (`E001`).

---

## 4. Portable Regex Subset & Selector Validation

`parsmith` validates all CSS selectors, XPath expressions, and regular expressions at compile time (`E001` / `E002`) using pure-Rust parsers with zero C/`libxml2` dependencies.

### Formal CSS3/CSS4 Selector Validation (`ADR-0001`)

Every selector passed to `css`, `css-all`, `css-remove`, or `css` conditions is validated against formal CSS3/CSS4 syntax:

- **Balanced delimiters & strings**: Checks balanced `[...]`, `(...)`, `/* ... */` comments, and quoted string literals.
- **Combinators**: Validates descendant whitespace, child `>`, next-sibling `+`, and subsequent-sibling `~` combinators. Dangling combinators (`div >`, `.a, > .b`) and leading combinators outside `:has(...)` are rejected with `E002`.
- **Compound selector ordering**: Type (`div`) or universal (`*`) selectors and namespace pipes (`svg|path`) must appear at the start of a compound selector before `#id`, `.class`, `[attr]`, or `:pseudo` selectors.
- **Attribute selectors**: Validates `[attr]`, `[attr=val]`, `~=`, `|=`, `^=`, `$=`, `*=`, and optional case-sensitivity modifiers (`i` / `s`).
- **Functional pseudo-classes**: Recursively validates selector lists inside `:not(...)`, `:is(...)`, `:where(...)`, `:matches(...)`, and `:has(...)` (which permits relative leading combinators such as `:has(> img)`), and validates `An+B` microsyntax (`odd`, `even`, `2n+1`, `-n+3`, including `of <selector-list>`) inside `:nth-child(...)`, `:nth-last-child(...)`, `:nth-of-type(...)`, and `:nth-last-of-type(...)`.

>[!note]
>`parsmith` validates formal CSS grammar without allowlisting or denylisting specific `:pseudo-class` or `::pseudo-element` identifier names, allowing target-specific extensions (such as `:-soup-contains(...)` in BeautifulSoup) when they are syntactically well-formed.

### W3C XPath 1.0 Validation

Every expression passed to `xpath`, `xpath-all`, `xpath-remove`, or `xpath` conditions is parsed against the full W3C XPath 1.0 grammar (`E002`), covering:

- Absolute (`/`, `//`) and relative location paths, `.` and `..` abbreviations, and `@attr` steps.
- All 13 standard axes (`ancestor::`, `ancestor-or-self::`, `attribute::`, `child::`, `descendant::`, `descendant-or-self::`, `following::`, `following-sibling::`, `namespace::`, `parent::`, `preceding::`, `preceding-sibling::`, `self::`).
- Node tests (`*`, QNames `prefix:local`, `text()`, `node()`, `comment()`, `processing-instruction()`).
- Predicates (`[...]`), union (`|`), boolean/relational/arithmetic operators (`or`, `and`, `=`, `!=`, `<`, `<=`, `>`, `>=`, `+`, `-`, `*`, `div`, `mod`), and function calls (`contains(...)`, `normalize-space(...)`, `position()`, etc.).

### Portable Regex Subset (`ADR-0001`)

Because `parsmith` compiles schemas to linear-time NFA/DFA regular expression engines in Go (`regexp` / RE2) and Rust (`regex`) alongside Python (`re`) and JavaScript/TypeScript (`RegExp`), all regular expressions in pipeline steps (`re`, `re-all`, `re-sub`, `re-split`) and conditions (`re`, `re-any`, `re-all`, `attr-re`, `text-re`) must conform to the **Portable regex subset**:

1. **Forbidden constructs (`E002`)**:
   - **Lookaround assertions**: Positive/negative lookahead (`(?=...)`, `(?!...)`) and lookbehind (`(?<=...)`, `(?<!...)`) are rejected with `E002`.
   - **Named capture groups & backreferences**: `(?P<name>...)`, `(?<name>...)`, and `(?P=name)` are rejected with `E002`. Use unnamed capturing groups `(...)` or non-capturing groups `(?:...)` instead.
2. **Single capture group rule in pipeline `re` and `re-all` (`E001`)**:
   - In pipeline extraction steps, `re "<pattern>"` and `re-all "<pattern>"` must contain **exactly one** unnamed capturing group `(...)`.
   - Patterns with `0` capturing groups or $\ge 2$ capturing groups are rejected at compile time with `E001`.
   - If you need grouping for alternation or repetition alongside your extraction group, use non-capturing groups `(?:...)`:
     ```kdl
     struct OfferItem {
         // Valid: 1 non-capturing group (?:USD|EUR) and 1 capturing group (\d+\.\d{2})
         price {
             css ".price"
             text
             re #"(?:USD|EUR)\s*(\d+\.\d{2})"#
             to-float
         }
     }
     ```
   - Inside `Cond` blocks (`filter`, `assert`, `match`), `re`, `re-any`, `re-all`, `attr-re`, and `text-re` test for a boolean match and allow **any** number of unnamed capturing groups (`0`, `1`, or more).
3. **Inline flags & automatic `(?x)` verbose normalization**:
   - Leading inline flags `(?i)` (case-insensitive), `(?s)` (dot matches newline), `(?m)` (multi-line anchors), and `(?x)` (verbose mode) are supported across all 5 targets.
   - When `(?x)` is present, the compiler automatically strips unescaped whitespace and `# ...` end-of-line comments (while preserving whitespace and `#` inside `[...]` character classes or escaped as `\ ` and `\#`), normalizing the expression into a portable single-line pattern before emitting target code:
     ```kdl
     define ORDER-ID-RE=#"""
         (?xi)
         order[\s_-]*   # prefix
         \#?            # optional literal hash
         ([a-z0-9]{8})  # 1 capture group: 8-char alphanumeric ID
     """#

     struct Receipt {
         order_id { css ".summary"; text; re ORDER-ID-RE }
     }
     ```

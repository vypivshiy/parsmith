# Condition Expressions (`Cond`): `filter`, `assert`, and `match`

**Cond (condition expressions)** are declarative boolean trees (`Cond::And`, `Cond::Or`, `Cond::Not`, `Cond::Atom(CondOp)`) evaluated inside three pipeline containers:

- `filter { ... }` — filters elements of a list (`DOC[] -> DOC[]` or `STR[] -> STR[]`).
- `assert ["<msg>"] { ... }` — validates a pipeline value (`T -> T`) or document (`@pre-validate`), raising a runtime error if the condition fails.
- `match { ... }` — selects a table row by matching its header key in `(table)struct` fields (`DOC -> STR`).

---

## 1. Condition Containers

### `filter { <conds> }` — List Filtering (`T[] -> T[]`)

`filter` evaluates its inner condition tree against each element of an array (`DOC[]` or `STR[]`) and retains only the elements for which the condition evaluates to `true`.

- **Input requirement**: The current pipeline type must be an array (`DOC[]` or `STR[]`, produced by `css-all`, `xpath-all`, `split`, `re-split`, `re-all`, or multi-key `attr`). Applying `filter` to a scalar raises `E100`.
- **Output type**: Unchanged (`DOC[] -> DOC[]`, `STR[] -> STR[]`).
- **Arguments**: `filter` accepts no positional arguments or properties (`E001`); all conditions go inside `{ ... }`.

```kdl
struct Navigation {
    // Filter DOM elements (DOC[] -> DOC[]) before extracting href
    external_links {
        css-all "nav a"
        filter {
            has-attr "href"
            attr-starts "href" "https://"
            not { attr-contains "rel" "nofollow" "sponsored" }
        }
        attr "href"
    }

    // Filter extracted strings (STR[] -> STR[])
    valid_codes {
        css ".codes-csv"
        text
        split ","
        trim
        filter {
            ne "" "N/A"
            re #"^[A-Z]{3}-\d{4}$"#
        }
        fallback {}
    }
}
```

### `assert ["<message>"] { <conds> }` — Pass-Through Validation (`T -> T`)

`assert` validates the current pipeline value without modifying it. If the condition tree evaluates to `true`, the value passes through unchanged (`T -> T`). If it evaluates to `false`, execution aborts with `RtAssertionError` (or `rt::RtError` in Rust).

`assert` can be used in two places:

1. **Inside any field, `@init`, or `fn` pipeline**: Validates the intermediate value at that exact point in the pipeline (on `DOC`, `DOC[]`, `STR`, `STR[]`, `INT`, `FLOAT`, etc.).
2. **Inside `@pre-validate { ... }` on a `struct`**: Runs at the beginning of `.parse()` against the root document (`DOC` in HTML structs, `STR` in `(raw)struct`) before any field pipelines execute.

```kdl
struct ProductPage {
    // Document-level guard executed before field extraction
    @pre-validate {
        assert "Expected a product detail page" {
            css "main.product-detail"
            not { css ".captcha-challenge" }
        }
    }

    title {
        css "h1.product-title"
        text
        trim
        assert "Product title must be between 3 and 200 characters" {
            len-range 3 200
        }
    }

    price {
        css ".price-value"
        text
        re #"(\d+\.\d{2})"#
        to-float
        assert { gt 0 }
    }
}
```

### `match { <conds> }` — Table Row Key Matching (`DOC -> STR`)

In a `(table)struct`, the parser iterates over each row selected by `@rows`, extracts a string key using `@match`, and extracts a cell string using `@value`. Every regular field in a `(table)struct` (unless it is a single `nested` call) **must** begin with `match { ... }` (`E203`):

```kdl
(table)struct TechSpecs {
    @table { css "table.specs" }
    @rows  { css-all "tr" }
    @match { css "th"; text; trim; lower }
    @value { css "td"; text; trim }

    weight {
        match { eq "weight" "net weight" }
    }
    dimensions {
        match { starts "dimensions" "size" }
        fallback #null
    }
    battery_mah {
        match { contains "battery" }
        re #"(\d+)\s*mAh"#
        to-int
        fallback #null
    }
}
```

**Rules for `match`:**
- Only valid inside `(table)struct` fields (`E203` if used in any other struct kind or `fn`).
- Must be the **first** operation in the field pipeline (`E203` / `E100` if preceded by another step).
- Evaluates against the `STR` produced by `@match` and outputs the `STR` produced by `@value` (`DOC -> STR`), allowing subsequent string/regex/conversion steps on the matched cell value.
- Only **String conditions** (`eq`, `ne`, `in`, `starts`, `ends`, `contains`, `re`) and logical combinators (`and`, `or`, `not`) are permitted inside `match` (`E203` on DOM or `assert`-only conditions).

---

## 2. Logical Combinators & Multi-Argument Semantics

### Implicit `AND` vs Explicit `and`, `or`, `not`

Condition blocks compose recursively using three logical combinators:

| Combinator | Semantics |
| :--- | :--- |
| Top-level block (`filter { A; B }`, `assert { A; B }`, `match { A; B }`) | **Implicit `AND`**: Every top-level child condition must evaluate to `true`. |
| `and { A; B; ... }` | **Conjunction (`AND`)**: Every child condition inside the block must evaluate to `true`. |
| `or { A; B; ... }` | **Disjunction (`OR`)**: At least one child condition inside the block must evaluate to `true`. |
| `not { A; B; ... }` | **Negation (`NOT`)**: Negates the conjunction of its child conditions (`!(A && B)`). |

```kdl
struct ReleaseFeed {
    stable_links {
        css-all "a.release-download"
        filter {
            // Top-level conditions combine via AND:
            has-attr "href"
            or {
                attr-ends "href" ".tar.gz" ".zip"
                attr-contains "data-format" "archive"
            }
            not {
                attr-contains "href" "-rc" "-beta" "-alpha"
            }
        }
        attr "href"
    }
}
```

### Multi-Argument Leaf Conditions

Many condition operations accept one or more target values in a single call (written `op <v1> <v2> ...`):

- **Positive conditions (`eq`, `in`, `starts`, `ends`, `contains`, `has-attr`, `attr-eq`, `attr-in`, `attr-starts`, `attr-ends`, `attr-contains`, `text-eq`, `text-in`, `text-starts`, `text-ends`, `text-contains`, `len-eq`)**: Match if **any** listed argument matches (**implicit `OR`** across arguments).
  - Example: `eq "active" "featured"` checks `val == "active" || val == "featured"`.
  - Example: `has-attr "href" "data-href"` checks that the element has `"href"` **or** `"data-href"`.
- **Negative conditions (`ne`, `attr-ne`, `text-ne`, `len-ne`)**: Match only if **none** of the listed arguments match (**implicit `AND` of `!=`** across arguments).
  - Example: `ne "" "null" "N/A"` checks `val != "" && val != "null" && val != "N/A"`.

>[!tip]
>You can also reuse condition sequences across multiple fields by declaring a **Block define** (`define VALID-LINK { has-attr "href"; not { attr-starts "href" "#" "javascript:" } }`) and referencing `VALID-LINK` inside `filter { VALID-LINK }`.

---

## 3. Condition Operations Reference

### 3.1 String Conditions (`filter`, `assert`, `match`)

Operate on `STR` values (or `DOC` elements by inspecting their text when used on `DOC`, and `INT`/`FLOAT`/`BOOL` scalars for `eq`/`ne`/`in`).

| Condition | Arguments | Description |
| :--- | :--- | :--- |
| `eq <v1> [v2 ...]` | $1+$ (`str\|int\|float\|bool`) | Value equals any of `<v1>, <v2>, ...`. |
| `ne <v1> [v2 ...]` | $1+$ (`str\|int\|float\|bool`) | Value does not equal any of `<v1>, <v2>, ...`. |
| `in <v1> [v2 ...]` | $1+$ (`str\|int\|float\|bool`) | Value is a member of `{<v1>, <v2>, ...}` (equivalent to multi-arg `eq`). |
| `starts "<s1>" ["<s2>" ...]` | $1+$ (`str`) | String starts with any of the given prefixes. |
| `ends "<s1>" ["<s2>" ...]` | $1+$ (`str`) | String ends with any of the given suffixes. |
| `contains "<s1>" ["<s2>" ...]` | $1+$ (`str`) | String contains any of the given substrings. |
| `re "<pattern>"` | `1` (`str` regex) | String matches `<pattern>` anywhere (boolean regex search). |

>[!note]
>Unlike the pipeline `re` operation (which extracts a substring and requires **1 capture group**), the `re` condition inside `filter`, `assert`, or `match` only tests whether the pattern matches and requires **zero** capture groups (`re #"^\d{4}-\d{2}-\d{2}$"#`). It still enforces the **Portable regex subset** (`E002`).

### 3.2 Attribute Conditions (`filter`, `assert` on `DOC` / `DOC[]`)

Inspect HTML attributes on a DOM element. Forbidden in `match` and in `(raw)struct` / `(raw)fn` (`E203`).

| Condition | Arguments | Description |
| :--- | :--- | :--- |
| `has-attr "<a1>" ["<a2>" ...]` | $1+$ (`str`) | Element has at least one of the specified attributes. |
| `attr-eq "<attr>" "<v1>" ["<v2>" ...]` | $2+$ (`str`) | Attribute `<attr>` equals any of `<v1>, <v2>, ...`. |
| `attr-ne "<attr>" "<v1>" ["<v2>" ...]` | $2+$ (`str`) | Attribute `<attr>` does not equal any of `<v1>, <v2>, ...` (or is absent). |
| `attr-in "<attr>" "<v1>" ["<v2>" ...]` | $2+$ (`str`) | Attribute `<attr>` is one of `<v1>, <v2>, ...`. |
| `attr-starts "<attr>" "<v1>" ["<v2>" ...]` | $2+$ (`str`) | Attribute `<attr>` starts with any of `<v1>, <v2>, ...`. |
| `attr-ends "<attr>" "<v1>" ["<v2>" ...]` | $2+$ (`str`) | Attribute `<attr>` ends with any of `<v1>, <v2>, ...`. |
| `attr-contains "<attr>" "<v1>" ["<v2>" ...]` | $2+$ (`str`) | Attribute `<attr>` contains any of `<v1>, <v2>, ...`. |
| `attr-re "<attr>" "<pattern>"` | `2` (`str`, regex) | Attribute `<attr>` matches regex `<pattern>`. |

### 3.3 Text Conditions (`filter`, `assert` on `DOC` / `DOC[]`)

Inspect the text content of a DOM element without converting the pipeline from `DOC` to `STR`. Forbidden in `match` and in `(raw)struct` / `(raw)fn` (`E203`).

| Condition | Arguments | Description |
| :--- | :--- | :--- |
| `text-eq "<v1>" ["<v2>" ...]` | $1+$ (`str`) | Element text equals any of `<v1>, <v2>, ...`. |
| `text-ne "<v1>" ["<v2>" ...]` | $1+$ (`str`) | Element text does not equal any of `<v1>, <v2>, ...`. |
| `text-in "<v1>" ["<v2>" ...]` | $1+$ (`str`) | Element text is one of `<v1>, <v2>, ...`. |
| `text-starts "<v1>" ["<v2>" ...]` | $1+$ (`str`) | Element text starts with any of `<v1>, <v2>, ...`. |
| `text-ends "<v1>" ["<v2>" ...]` | $1+$ (`str`) | Element text ends with any of `<v1>, <v2>, ...`. |
| `text-contains "<v1>" ["<v2>" ...]` | $1+$ (`str`) | Element text contains any of `<v1>, <v2>, ...`. |
| `text-re "<pattern>"` | `1` (regex) | Element text matches regex `<pattern>`. |

### 3.4 Document Conditions (`filter`, `assert` on `DOC` / `DOC[]`)

Check whether a DOM element contains a matching descendant node. Forbidden in `match` and in `(raw)struct` / `(raw)fn` (`E203`).

| Condition | Arguments | Description |
| :--- | :--- | :--- |
| `css "<selector>"` | `1` (CSS selector) | Element has at least one descendant matching `<selector>`. |
| `xpath "<expr>"` | `1` (XPath 1.0 expr) | Element has at least one node matching `<expr>` (rejected with `E203` on backends without XPath support: `python-bs4`, `python-selectolax`, `go`, `rust`). |

### 3.5 Length Conditions (`assert` **only**)

Validate the length of a string (`STR`, measured in Unicode characters/code points) or the element count of a list (`T[]`). Using any `len-*` condition outside `assert { ... }` raises `E203`.

| Condition | Arguments | Description |
| :--- | :--- | :--- |
| `len-eq <n1> [n2 ...]` | $1+$ (non-negative `int`) | Length equals any of `<n1>, <n2>, ...` (`E001` if negative). |
| `len-ne <n1> [n2 ...]` | $1+$ (non-negative `int`) | Length does not equal any of `<n1>, <n2>, ...` (`E001` if negative). |
| `len-gt <n>` | `1` (`int`) | Length is strictly greater than `<n>` (`len > n`). |
| `len-lt <n>` | `1` (`int`) | Length is strictly less than `<n>` (`len < n`). |
| `len-ge <n>` | `1` (`int`) | Length is greater than or equal to `<n>` (`len >= n`). |
| `len-le <n>` | `1` (`int`) | Length is less than or equal to `<n>` (`len <= n`). |
| `len-range <min> <max>` | `2` (`int`, `int`) | Length is strictly between `<min>` and `<max>` (`min < len < max`). |

### 3.6 Regex List Conditions (`assert` **only**)

Validate elements of a string list (`STR[]`) against a regular expression. Using `re-any` or `re-all` inside `filter` or `match` raises `E203`.

| Condition | Arguments | Description |
| :--- | :--- | :--- |
| `re-any "<pattern>"` | `1` (regex) | At least one string in the `STR[]` list matches `<pattern>`. |
| `re-all "<pattern>"` | `1` (regex) | Every string in the `STR[]` list matches `<pattern>`. |

### 3.7 Numeric Conditions (`assert` **only**)

Compare a numeric value (`INT` or `FLOAT`) or the length of a `STR` / `T[]` against an integer threshold. Using `gt`, `lt`, `ge`, or `le` outside `assert { ... }` raises `E203`.

| Condition | Arguments | Description |
| :--- | :--- | :--- |
| `gt <n>` | `1` (`int`) | Value is strictly greater than `<n>` (`v > n`). |
| `lt <n>` | `1` (`int`) | Value is strictly less than `<n>` (`v < n`). |
| `ge <n>` | `1` (`int`) | Value is greater than or equal to `<n>` (`v >= n`). |
| `le <n>` | `1` (`int`) | Value is less than or equal to `<n>` (`v <= n`). |

```kdl
struct OrderSummary {
    items {
        css-all "ul.cart-items li.sku"
        text
        trim
        assert "Cart must contain between 1 and 50 valid SKUs" {
            len-ge 1
            len-le 50
            re-all #"^SKU-[0-9]{6}$"#
        }
    }

    total_cents {
        css ".order-total"
        attr "data-cents"
        to-int
        assert "Order total must be positive" {
            gt 0
        }
    }
}
```

---

## 4. Runtime Contract: `RtAssertionError` & `fallback` Interaction

### Why `parsmith` Never Emits Bare `assert` Statements (`ADR-0009`)

In Python, built-in `assert cond` statements are silently stripped by the interpreter when running in optimized mode (`python -O` or `PYTHONOPTIMIZE=1`). To guarantee that schema validations and `@pre-validate` guards always execute in production, `parsmith` emits a dedicated runtime helper in the unified `rt` namespace:

| Target | Runtime Helper | Error Type / Mechanism |
| :--- | :--- | :--- |
| **Python** | `rt_assert(cond, msg)` | Raises `RtAssertionError(ValueError)` |
| **TypeScript** | `rtAssert(cond, msg)` | Throws `RtAssertionError extends Error` |
| **JavaScript** | `rtAssert(cond, msg)` | Throws `RtAssertionError extends Error` |
| **Go** | `rtAssert(cond, msg)` | Panics with `"parsmith: " + msg` (recovered by `rtFallback`) |
| **Rust** | `if !(cond) { return Err(rt::RtError::new(...)); }` | Returns `Err(rt::RtError)` |

### Diagnostic Message Format

When an `assert { ... }` block has no custom message argument, `parsmith` synthesizes a default diagnostic containing the exact KDL source file, 1-based line and column, struct name, and field name:

```text
catalog.kdl:14:9 assertion failed at ProductPage.price
```

For `@pre-validate` blocks, the location is reported as `<Struct>.@pre-validate`:

```text
catalog.kdl:4:9 assertion failed at ProductPage.@pre-validate
```

When a custom message string is supplied (`assert "Custom error message" { ... }`), that message is passed directly to `RtAssertionError` / `rt::RtError`.

### Interaction with `fallback`

When a field pipeline includes a terminal `fallback` step after `assert`, any `RtAssertionError` raised by `assert` is caught by the field's `fallback` handler, returning the fallback value instead of aborting `.parse()`:

```kdl
struct Listing {
    // Returns #null if .rating is missing, non-numeric, or outside 1..5
    rating {
        css ".rating"
        text
        to-float
        assert {
            ge 1
            le 5
        }
        fallback #null
    }
}
```

>[!note]
>`@pre-validate` runs at the struct level before field extraction and does not use `fallback` — a failed `@pre-validate` assertion always aborts `.parse()` immediately.

### Handling Assertion & Regex Errors Across Targets

#### Python

```python
from catalog_parser import ProductPage, RtAssertionError, RtRegexError

try:
    data = ProductPage(html).parse()
except RtAssertionError as err:
    print(f"Validation failed: {err}")
except RtRegexError as err:
    print(f"Pattern extraction failed: {err}")
```

#### TypeScript / JavaScript

```typescript
import { ProductPage, RtAssertionError, RtRegexError } from "./catalog_parser.js";

try {
  const data = new ProductPage(html).parse();
} catch (err) {
  if (err instanceof RtAssertionError) {
    console.error("Validation failed:", err.message);
  } else if (err instanceof RtRegexError) {
    console.error("Pattern extraction failed:", err.message);
  } else {
    throw err;
  }
}
```

#### Go

In Go, field pipelines with a `fallback` recover automatically via `rtFallback`. Uncaught assertion or selector failures in `Parse()` panic with a `"parsmith: ..."` prefix:

```go
parser, err := NewProductPage(html)
if err != nil {
    log.Fatal(err)
}
data := parser.Parse()
```

#### Rust

In Rust, `new(...)`, `parse()`, `@check` methods, and standalone functions return `Result<T, rt::RtError>` (re-exported at the module root as `pub use rt::RtError;`):

```rust
use crate::catalog_parser::{ProductPage, RtError};

fn extract(html: &str) -> Result<(), RtError> {
    let mut parser = ProductPage::new(html)?;
    let data = parser.parse()?;
    println!("Title: {}", data.title);
    Ok(())
}
```

---

## 5. Comparison Matrix: `filter` vs `assert` vs `match`

| Feature | `filter { ... }` | `assert ["msg"] { ... }` | `match { ... }` |
| :--- | :--- | :--- | :--- |
| **Purpose** | Keep matching list elements | Validate value or abort | Match row key in `(table)struct` |
| **Input `TypeInfo`** | `DOC[]` or `STR[]` (`E100` on scalars) | Any type (`DOC`, `STR`, `INT`, `FLOAT`, `T[]`) | `DOC` (table row element) |
| **Output `TypeInfo`** | Same list type (`T[] -> T[]`) | Unchanged (`T -> T`, pass-through) | `STR` (extracted `@value` cell string) |
| **On `false` condition** | Drops the non-matching element | Raises `RtAssertionError` / `rt::RtError` | Skips row; falls back if no row matches |
| **Placement in pipeline** | Any step where current type is `T[]` | Any step in field/`fn`, or in `@pre-validate` | **Must be first step** in `(table)struct` field (`E203`) |
| **Custom error message** | Not allowed (`E001`) | Optional string argument (`assert "msg" { ... }`) | Not allowed (`E001`) |
| **String conditions** (`eq`, `starts`, `re`, ...) | Yes | Yes | Yes |
| **DOM conditions** (`css`, `xpath`, `has-attr`, `attr-*`, `text-*`) | Yes (on `DOC[]`) | Yes (on `DOC` / `DOC[]`) | **Forbidden (`E203`)** |
| **Length, Numeric & Regex-List conditions** (`len-*`, `gt`/`lt`/`ge`/`le`, `re-any`, `re-all`) | **Forbidden (`E203`)** | **Yes (`assert` only)** | **Forbidden (`E203`)** |

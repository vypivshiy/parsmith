# JSON Schemas, GJSON Paths & `jsonify`

`json` declarations define typed wire contracts for JSON payloads. They are used in two places:

1. **Pipeline extraction (`jsonify`)**: Parse and project JSON strings embedded inside HTML `<script>` tags, element attributes, or plain-text `(raw)struct` sources.
2. **REST API clients (`(rest)struct`)**: Validate and project HTTP `2xx` responses (`response=Schema`) and discriminated error responses (`@error <status> Schema`).

---

## 1. Strict Allowlist Projection & Top-Level `json` Kinds

Every `json` schema in `parsmith` compiles into both a native static type declaration and a **JSON projector** function that enforces **strict allowlist projection** at runtime:

- **Allowlist extraction**: Only fields explicitly declared in the KDL schema are extracted into the output structure.
- **Wire noise stripping**: Undeclared keys present in the incoming JSON payload are dropped.
- **Fail-fast contract validation**: Container types (`object` vs `array`), required keys, non-nullable constraints, and primitive value types are validated during projection. Any mismatch raises a schema-qualified error (`RtJsonError` in Python/TS/JS, `rt::RtError` in Rust, or an `UnmarshalJSON` error in Go).

| Target | Generated Model Type (`json User`) | Generated JSON Projector |
| :--- | :--- | :--- |
| **Python** (`typeddict`) | `class UserJson(TypedDict)` | `rt_project_user(data: Any) -> UserJson` |
| **Python** (`dataclass`) | `@dataclass(slots=True) class UserJson` | `rt_project_user(data: Any) -> UserJson` |
| **TypeScript** (`ts`) | `export interface UserJson` | `export function rtProjectUser(data: unknown): UserJson` |
| **JavaScript** (`js` + `.d.ts`) | JSDoc `@typedef` + `.d.ts` `interface UserJson` | `export function rtProjectUser(data)` |
| **Go** (`go`) | `type UserJson struct` | `func (res *UserJson) UnmarshalJSON(raw []byte) error` |
| **Rust** (`rust`) | `pub struct UserJson` (`Serialize, Deserialize`) | `UserJson::from_value(value: Value) -> Result<Self, rt::RtError>` |

### Top-Level `json` Kinds

`parsmith` supports three top-level `json` schema kinds:

#### 1. Object Schema (`json <Name>`)

Models a JSON object (`{ ... }`) with a fixed set of named fields:

```kdl
json Author {
    id int
    name str
    bio str?
}
```

#### 2. Top-Level Array Schema (`(array)json <Name>`)

Models a JSON array of homogeneous objects (`[ { ... }, ... ]`). The generated model type `QuoteJson` represents a single item, while the projector and `jsonify Quote` / `response=Quote` return an array of items (`list[QuoteJson]`, `QuoteJson[]`, `[]QuoteJson`, `Vec<QuoteJson>`):

```kdl
json Author {
    name str
    slug str
}

(array)json Quote {
    text str
    tags (array)str
    author Author
}
```

#### 3. Dictionary Schema (`(dict)json <Name>`)

Models a JSON object used as a homogeneous key-value map (`{ "<dynamic_key>": <value>, ... }`), such as localization tables, episode maps, or resource catalogs indexed by ID:

```kdl
(dict)json EpisodeTranslations {
    @key int
    @value (array)str
}

json ResourceItem {
    title str
    url str
}

(dict)json ResourceCatalog {
    @key str
    @value ResourceItem
}
```

Inside a `(dict)json` block, only `@key` and `@value` directives are permitted (`E203` on regular fields):

- `@key <ScalarType>`: Optional key type (`str`, `int`, `float`, `bool`). Defaults to `str` when omitted.
- `@value <ValueType>`: Mandatory value type (`E401` if missing). Accepts any scalar (`str`, `int`, `float`, `bool`), array `(array)<Type>`, reference to a `json` schema declared above, or an inline `@value { ... }` block.

| Target | Generated Type for `(dict)json EpisodeTranslations` |
| :--- | :--- |
| **Python** | `EpisodeTranslationsJson: TypeAlias = dict[int, list[str]]` |
| **TypeScript** | `export type EpisodeTranslationsJson = Record<number, string[]>;` |
| **JavaScript** | `/** @typedef {Record<number, string[]>} EpisodeTranslationsJson */` |
| **Go** | `type EpisodeTranslationsJson = map[int64][]string` |
| **Rust** | `pub type EpisodeTranslationsJson = std::collections::HashMap<i64, Vec<String>>;` |

---

## 2. Field Types & Modifiers

Inside `json` and `(array)json` blocks, each child node declares a field:

```kdl
<field_name> <type>[?] [from="<gjson_path>"] [@omitempty] [@skip]
```

### Field Types

| DSL Type | Python | TypeScript / JS | Go | Rust |
| :--- | :--- | :--- | :--- | :--- |
| `str` | `str` | `string` | `string` | `String` |
| `int` | `int` | `number` | `int64` | `i64` |
| `float` | `float` | `number` | `float64` | `f64` |
| `bool` | `bool` | `boolean` | `bool` | `bool` |
| `null` / `nil` | `None` | `null` | `any` | `()` |
| `<SchemaName>` | `<SchemaName>Json` | `<SchemaName>Json` | `<SchemaName>Json` | `<SchemaName>Json` |
| `(array)<type>` | `list[T]` | `T[]` | `[]T` | `Vec<T>` |
| `(dict)[DictName]` | `dict[K, V]` | `Record<K, V>` | `map[K]V` | `HashMap<K, V>` |

>[!note]
> Within a single `.kdl` file, any referenced `<SchemaName>` must be declared **above** the schema that references it (`E302`).

### Field Modifiers

- **Nullable (`?`)**: Placed at the end of the type token (`str?`, `(array)str?`, `Author?`) or field name on an inline block (`profile? { ... }`). Allows the wire value to be `null` or the path to be absent, resolving to `None` in Python, `null` in TS/JS, a nil pointer `*T` in Go, and `Option<T>` in Rust.
- **Omit-Empty (`@omitempty`)**: Marks a key that may be absent from the wire payload:
  - **Python (`typeddict`)**: Wraps the field type in `NotRequired[T]` and omits the key from the output dictionary when absent or `null` on the wire.
  - **Python (`dataclass`)**: Types the field as `T | None = None` (ordered after required fields).
  - **TypeScript / JavaScript**: Emits an optional interface property (`field?: T`) and omits the property when absent on the wire.
  - **Go**: Emits a pointer field (`*T`) with struct tag `` `json:"field,omitempty"` ``.
  - **Rust**: Emits `Option<T>` with `#[serde(skip_serializing_if = "Option::is_none")]`.
- **Skip (`@skip`)**: Excludes the field from the generated type definition and runtime projector. Useful when keeping a full wire schema documented in KDL while omitting unused heavy fields from generated code.
- **Wire Key Alias & Path (`from="<gjson_path>"`)**: Maps a wire property name or nested **GJSON path expression** to `<field_name>`. Use `from=` when a wire key is `camelCase`, contains hyphens/dots/colons, collides with a target language reserved keyword (`type`, `class`, `match`), or lives inside a nested wrapper object.

```kdl
json ItemDetail {
    id int
    item_type str from="type"
    created_at str @omitempty from="meta.createdAt"
    discount_pct float? from="pricing.discount"
    debug_trace @skip
}
```

>[!warning]
> Always use `from="..."` for JSON field aliases. Writing `path="..."` on a `json` field triggers compiler warning `W041` (`path=` is reserved for the `jsonify` pipeline operation). Using a positional string literal (`item_type str "type"`) triggers deprecation warning `W011`, and accidental leading/trailing whitespace inside `from=" ... "` triggers `W040`.

### Block `define` Expansion in `json`

Reusable field groups can be declared with a **Block define** (`define UPPER_NAME { ... }`) and spliced inline into one or more `json` schemas by writing the bare `UPPER_CASE` identifier:

```kdl
define TIMESTAMPS {
    created_at str
    updated_at str? @omitempty
    internal_rev int @skip
}

json Post {
    id int
    title str
    TIMESTAMPS
}

json Comment {
    id int
    body str
    TIMESTAMPS
}
```

---

## 3. Inline JSON Schemas, Inline `@value` Blocks & Synthesized Schema Names

Instead of flattening deeply nested API responses into dozens of separate top-level `json` blocks, you can declare **Inline JSON schemas** directly inside parent fields and dictionary `@value` directives.

### Inline Object & Array Forms

```kdl
json AnimeResponse {
    id str

    // 1. Anonymous inline object -> synthesizes AnimeResponseMaterialDataJson
    material_data {
        anime_title str
        year int
    }

    // 2. Explicitly named inline object -> emits FranchiseJson
    franchise Franchise {
        id str
        name str
    }

    // 3. Inline array of objects -> explicit item model name is required (NodeItemJson)
    nodes (array)NodeItem {
        id int
        label str
    }

    // 4. Named inline dict with scalar or array values
    episode_tags (dict)EpisodeTags {
        @key int
        @value (array)str
    }

    // 5. Prefix-annotated anonymous inline dict
    (dict)external_ids {
        @key str
        @value int?
    }
}
```

>[!warning]
> Inline object arrays **require** an explicit item model name (`nodes (array)NodeItem { ... }` or `(array)@value NodeItem { ... }`). Writing an anonymous array block such as `nodes (array) { ... }` or `(array)@value { ... }` is rejected by the linter with `E001`.

### Inline `@value` Blocks in Dictionaries

The `@value` directive in both top-level `(dict)json` schemas and inline `(dict)` fields accepts a child block `{ ... }` to define structured dictionary values inline:

```kdl
// Top-level dict with anonymous @value block -> synthesizes TranslationsValueJson
(dict)json Translations {
    @key str
    @value {
        title str from="wire_title"
        active bool
    }
}

json MediaCatalog {
    // Named inline dict with named @value block -> emits StudioInfoJson
    studios (dict)StudioMap {
        @key str
        @value StudioInfo {
            name str
            founded int?
        }
    }

    // Anonymous inline dict with nullable anonymous @value block
    // -> synthesizes MediaCatalogLocalizedValueJson
    (dict)localized {
        @key str
        @value? {
            label str
            summary str? @omitempty
        }
    }
}
```

`@value` blocks support all four naming and modifier combinations:

1. **Explicit model name**: `@value ModelName { ... }` or `(ModelName)@value { ... }`
2. **Array of inline objects**: `@value (array)ItemModel { ... }` or `(array)@value ItemModel { ... }` (`ItemModel` is mandatory; `E001` if omitted)
3. **Anonymous object block**: `@value { ... }` (uses a deterministic **Synthesized schema name**)
4. **Nullable value block**: `@value ModelName? { ... }`, `(ModelName?)@value { ... }`, `@value? ModelName { ... }`, or `@value? { ... }`

### Synthesized Schema Name Rules

When an inline object or `@value` block omits an explicit model name, the compiler synthesizes a deterministic `PascalCase` `JsonDef` name:

| Inline Declaration Form | Synthesized `JsonDef` Name | Generated Model Type (`<Name>Json`) |
| :--- | :--- | :--- |
| `field { ... }` inside `json Parent` | `{Parent}{FieldPascal}` | `ParentFieldJson` |
| `field ModelName { ... }` | `ModelName` | `ModelNameJson` |
| `field (array)ItemModel { ... }` | `ItemModel` | `ItemModelJson` |
| `(dict)json DictSchema { @value { ... } }` | `{DictSchema}Value` | `DictSchemaValueJson` |
| `field (dict)DictName { @value { ... } }` | `{DictName}Value` | `DictNameValueJson` |
| `field (dict) { @value { ... } }` or `(dict)field { @value { ... } }` inside `json Parent` | `{Parent}{FieldPascal}Value` | `ParentFieldValueJson` |
| `@value ModelName { ... }` / `(ModelName)@value { ... }` | `ModelName` | `ModelNameJson` |
| `@value (array)ItemModel { ... }` / `(array)@value ItemModel { ... }` | `ItemModel` | `ItemModelJson` |

>[!note]
> If an explicit or synthesized inline schema name collides with another `json` schema in the module, `parsmith check` reports `E001`. Empty inline `@value {}` blocks also report `E001`, and combining `@skip` with an inline `@value { ... }` block reports `E002`.

### Recursive Post-Order Compiler Hoisting

Inline schemas can nest arbitrarily deeply—including objects inside `@value` blocks that themselves contain nested `(dict)` fields. During AST lowering, the compiler performs **post-order hoisting**:

```kdl
json AnimeResponse {
    translations (dict)Translation {
        @key str
        @value TranslationValue {
            is_active bool
            episodes (dict)EpisodeMap {
                @key int
                @value EpisodeValue {
                    link str from="stream_url"
                    bitrate int?
                }
            }
        }
    }
}
```

1. The deepest leaf schema (`EpisodeValue`) is hoisted and emitted first.
2. The enclosing intermediate schema (`TranslationValue`) is emitted second.
3. The root schema (`AnimeResponse`) is emitted last.

This guarantees that generated `TypedDict` classes, TypeScript interfaces, Go structs, and Rust structs are always emitted in valid dependency order.

---

## 4. GJSON Path Expressions (`from="..."` and `jsonify path="..."`)

Both the `from="..."` property on `json` fields and the `path="..."` property on `jsonify` use `parsmith`'s deterministic, filter-free **GJSON path expression** subset:

| Syntax | Segment Kind | Example | Behavior |
| :--- | :--- | :--- | :--- |
| `a.b.c` | Object key lookup | `from="profile.avatar.url"` | Traverses nested object keys `data["profile"]["avatar"]["url"]` |
| `\.`, `\\`, `\#` | Escaped literal char | `from=#"meta.item\.id"#` | Looks up a single key containing a literal `.`, `\`, or `#` (`data["meta"]["item.id"]`) |
| `0`, `1`, `42` | Array index (0-based) | `from="badges.0.icon"` | Indexes into a JSON array at the given non-negative integer offset |
| `#` *(terminal)* | Array count | `from="tags.#"` | Returns the integer length of the array (`int`) |
| `#.subpath` | Array element projection | `from="tags.#.name"` | Maps `subpath` over every element of the array, filtering out `null` / missing results (`(array)T`) |

```kdl
json ArticleSummary {
    id int from=#"meta.article\.id"#
    author_name str from="author.profile.displayName"
    primary_category str? from="categories.0.slug"
    comment_count int from="comments.#"
    reviewer_names (array)str from="reviews.#.author.name"
}
```

>[!tip]
> Use KDL raw strings (`from=#"meta.item\.id"#`) when escaping literal dots, backslashes, or hashes in GJSON paths so you do not need to double-escape the backslash (`from="meta.item\\.id"`).

### Forbidden GJSON Syntax (`E040`)

To guarantee identical, linear-time behavior across Python, TypeScript/JavaScript, Go, and Rust, non-deterministic or query-filter GJSON features are rejected at compile time with `E040`:

- Empty paths (`""`), leading dots (`".a"`), trailing dots (`"a."`), or consecutive unescaped dots (`"a..b"`).
- Negative array indices (`"items.-1"`).
- Wildcards (`*`, `?`) and pipeline/alternation operators (`|`, `!`).
- Filter expressions (`"items.#[price>10]"` or `"items.#(active==true)"`).
- GJSON `@` modifiers (`@reverse`, `@this`, `@valid`, `@flatten`, `@keys`, `@values`, etc.).

*(Note: Normal object keys starting with `@`, such as JSON-LD `@context` or `@type`, are valid object key segments).*

---

## 5. Runtime Error Hierarchy (`RtJson*Error`) & `jsonify` Usage

### Runtime Error Hierarchy

When a JSON payload violates a `json` schema contract, the generated projector fails fast with a structured runtime error:

| Error Condition | Python (`ValueError` subclass) | TypeScript / JavaScript (`Error` subclass) | Go | Rust |
| :--- | :--- | :--- | :--- | :--- |
| Base JSON projection error | `RtJsonError` | `RtJsonError` | `error` | `rt::RtError` |
| Expected `dict`/`object` or `list`/`array`, got another type | `RtJsonSchemaError` | `RtJsonSchemaError` | `error` (`UnmarshalJSON`) | `rt::RtError` |
| Required key is missing or its value is `null` on a non-nullable field | `RtJsonFieldMissingError` | `RtJsonFieldMissingError` | `error` (`UnmarshalJSON`) | `rt::RtError` |
| Intermediate GJSON dot-path segment is missing, `null`, non-container, or array index is out of bounds | `RtJsonPathError` | `RtJsonPathError` | `error` (`UnmarshalJSON`) | `rt::RtError` |

- **Required fields** (neither `?` nor `@omitempty`): Raise `RtJsonFieldMissingError` (or `RtJsonPathError` on a broken multi-segment `from=` path) if the key is absent or `null`.
- **Nullable fields (`?`)**: Return `None` / `null` / `nil` when the key or intermediate path is missing or `null`.
- **Omit-empty fields (`@omitempty`)**: Omit the key (or return `None` / `nil` / `Option::None`) when absent or `null`.

### Using `jsonify` in Pipelines

`jsonify <SchemaName> [path="<gjson_path>"]` is a **terminal** pipeline operation (`STR -> JSON`, `STR[] -> JSON[]`) that parses a JSON string and projects it through `<SchemaName>`.

#### Extracting Subtrees with `path="..."`

When an HTML page embeds a large state blob (such as `__NEXT_DATA__`), you can extract the raw JSON text once in `@init` and project specific subtrees in each field using `jsonify <Schema> path="<gjson_path>"`:

```kdl
json Author {
    name str
    slug str
}

(array)json Quote {
    text str
    author Author
}

struct QuotesPage {
    @init {
        state_json { css "script#__NEXT_DATA__"; text }
    }

    featured_author {
        @state_json
        jsonify Author path="props.pageProps.featuredAuthor"
    }

    all_quotes {
        @state_json
        jsonify Quote path="props.pageProps.quotes"
    }

    first_quote_author {
        @state_json
        jsonify Author path="props.pageProps.quotes.0.author"
    }
}
```

>[!note]
> The `path="..."` argument on `jsonify` selects a **subtree of the input JSON payload** before projection. The output cardinality is determined by the target schema declaration (`json Author` produces a single `AuthorJson`, whereas `(array)json Quote` produces an array of `QuoteJson`).

#### Unescaping JSON Inside HTML Attributes

When JSON is stored inside an HTML attribute (e.g. `data-state="{&quot;id&quot;: 42}"`), apply `unescape` before `jsonify` so HTML entities are decoded prior to JSON parsing:

```kdl
json WidgetState {
    id int
    active bool
}

struct WidgetCard {
    state {
        css "div.widget"
        attr "data-state"
        unescape
        jsonify WidgetState
    }
}
```

---

## 6. Bootstrapping Schemas from Sample JSON (`parsmith json-to-kdl`)

You can generate a complete, lint-clean KDL 2.0 `json` schema directly from a captured API response or sample JSON file using `parsmith json-to-kdl`:

```bash
# From a sample JSON file to a .kdl file
parsmith json-to-kdl sample.json --name ApiResponse -o schema.kdl

# From stdin to stdout
cat sample.json | parsmith json-to-kdl --name ProductCatalog
```

The same functionality is available programmatically in the Host SDKs via `parsmith.json_to_kdl(json_str, name="ApiResponse")` in Python and `jsonToKdl(jsonStr, "ApiResponse")` in TypeScript/JavaScript.

### Synthesis & Inference Rules

1. **Hierarchical inline objects**: Nested JSON objects are emitted as anonymous inline blocks (`field { ... }`) inside the root `json <Name> { ... }` schema.
2. **Object arrays & top-level arrays**:
   - An array field containing objects is emitted with an explicit item model name: `items (array)ItemsItem { ... }` (prepending ancestor field names if needed to avoid model name collisions, e.g. `OrderItemsItem`).
   - If the root JSON value is a non-empty array of objects, the root node is emitted as `(array)json <Name>Item { ... }`.
3. **Multi-sample recursive merging**: When an array contains multiple object elements, `json-to-kdl` merges all elements recursively:
   - Fields present in only a subset of elements are marked `@omitempty`.
   - Fields that are `null` in some elements and non-null in others are marked nullable (`?`).
   - Numeric fields mixing `int` and `float` across elements widen to `float`.
4. **Identifier normalization & `from="..."` aliasing**:
   - Wire keys containing hyphens, dots, spaces, or special characters are normalized to `snake_case` with `from="<wire_key>"` attached automatically.
   - Keys starting with a digit or matching a reserved keyword in KDL 2.0, Python, JS/TS, Go, or Rust are prefixed with `k_` (e.g. `k_type str from="type"`, `k_0 str from="0"`).
5. **Empty containers & `null`-only fields**:
   - Empty objects (`{}`) emit `<field> @skip // empty object` (avoiding `E001` empty block errors).
   - Empty arrays (`[]`) emit `<field> (array)nil @skip // empty array`.
   - Fields whose sample values are exclusively `null` emit `<field> nil // unknown real type`.

### Example

Given `sample.json`:

```json
{
  "item-id": 101,
  "type": "book",
  "author": {
    "display_name": "Ursula K. Le Guin",
    "website": null
  },
  "editions": [
    { "year": 1968, "isbn": "978-0553383041" },
    { "year": 2004 }
  ],
  "metadata": {}
}
```

Running `parsmith json-to-kdl sample.json --name BookPayload` produces:

```kdl
json BookPayload {
    item_id int from="item-id"
    k_type str from="type"
    author {
        display_name str
        website nil // unknown real type
    }
    editions (array)EditionsItem {
        year int
        isbn str @omitempty
    }
    metadata @skip // empty object
}
```

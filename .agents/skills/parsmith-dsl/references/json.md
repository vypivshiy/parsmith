# `json` Schemas, Inline Hoisting, `(dict)json` & GJSON Paths (DSL v2.1)

Used for both REST endpoint responses (`@request response="<Schema>"`, `@error <status> <Schema>`) and HTML/raw JSON extraction (`jsonify <Schema> [path="<gjson>"]`). Every `json` schema compiles into both a native static type and an **allowlist JSON projector** (`rt_project_<schema>` / `rtProject<Schema>` / `UnmarshalJSON` / `from_value` / `Rt.project_*`) that strips undeclared wire keys and raises a schema-qualified error (`RtJsonError` / `rt::RtError`) on contract mismatch.

## 1. Top-Level & Inline `json` Declarations

- `json <Name> [path="<gjson>"] { <fields...> }` — JSON object schema (optional `path=` pre-extracts a root GJSON path before field projection).
- `(array)json <Name> [path="<gjson>"] { <fields...> }` — top-level JSON array of homogeneous objects.
- `(dict)json <Name> { [@key <ScalarType>;] @value <ValueType>; }` — top-level homogeneous map with dynamic keys (`@key`: `str` default | `int` | `float` | `bool`).

**Default Idiom — Inline Single-Use Nested Models:**
Define nested objects, object arrays, and dynamic maps **inline** inside their parent `json` schema unless the nested model is shared across 2+ parent schemas/endpoints (in which case declare it at top-level **above** its callers, `E302`).

### Recursive Compiler Hoisting & Synthesized Names

Inline object blocks and `@value` blocks are hoisted post-order (innermost leaves first) into top-level `JsonDef` nodes ahead of their parent schema:

| Inline Form | Synthesized `JsonDef` Name | Rules & Validation |
| :--- | :--- | :--- |
| `field[?] { ... }` | `{Parent}{FieldPascal}` | Anonymous inline object (e.g. `Product` + `dimensions` $\to$ `ProductDimensions`) |
| `field ModelName[?] { ... }` | `ModelName` | Explicitly named inline object |
| `field (array)ItemModel[?] { ... }` | `ItemModel` | Inline object array; `ItemModel` name is **mandatory** (anonymous `(array)` raises `E001`) |
| `(dict)json DictSchema { @value { ... } }` | `{DictSchema}Value` | Top-level dict with anonymous inline `@value` block |
| `field (dict)DictName { @value { ... } }` | `{DictName}Value` | Named inline dict field with anonymous `@value` block |
| `field (dict) { @value { ... } }` or `(dict)field { @value { ... } }` | `{Parent}{FieldPascal}Value` | Anonymous inline dict field with anonymous `@value` block |
| `@value ModelName[?] { ... }` / `(ModelName[?])@value { ... }` | `ModelName` | Explicitly named `@value` object block (nullable `@value ? { ... }` also valid) |
| `@value (array)ItemModel[?] { ... }` / `(array)@value ItemModel[?] { ... }` | `ItemModel` | Array of inline objects in `@value`; `ItemModel` name is **mandatory** (`E001`) |

**Validation constraints:**
- Empty `{}` in an inline field block or `@value` block raises `E001`.
- `@skip` on an inline field block or `@value` block raises `E002`.
- Colliding explicit model names within the module raise `E001`.

```kdl
json AnimeResponse {
    id int
    material_data {
        anime_title str
        year int?
    }
    episodes (array)EpisodeItem {
        num int
        air_date str? @omitempty
    }
    translations (dict)Translation {
        @key str
        @value {
            is_active bool
            streams (dict) {
                @key int
                @value StreamEntry {
                    url str from="stream_url"
                    bitrate int?
                }
            }
        }
    }
}
```

---

## 2. Field Modifiers (`?`, `@omitempty`, `@skip`, `(array)`, `value=`) Across Targets

Syntax: `<field_name> <type>[?] [from="<gjson_path>" | value=<literal>] [@omitempty] [@skip]`

| Modifier | Wire Semantics | Python | TS / JS | Go | Rust | Elixir / PHP / C# / Kotlin |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `(array)T` | JSON array `[...]` | `list[T]` | `T[]` | `[]T` | `Vec<T>` | `[T]` / `array` / `List<T>` |
| `T?` | Key required, value may be `null` | `T \| None` | `T \| null` | `*T` | `Option<T>` | `T \| nil` / `?T` / `T?` |
| `@omitempty` | Key may be absent from JSON object | `NotRequired[T]` | `field?: T` | `*T` + `,omitempty` | `Option<T>` + `#[serde(default)]` | `T \| nil` / `?T` / `T?` |
| `T? @omitempty` | Key may be absent OR `null` | `NotRequired[T \| None]` | `field?: T \| null` | `*T` + `,omitempty` | `Option<T>` + `#[serde(default)]` | `T \| nil` / `?T` / `T?` |
| `value=<literal>` | Scalar constant (`str` with `{{DEFINE}}`, `int`, `float`, `bool`) injected directly by projector without reading wire JSON (cannot combine with `?`, `(array)`, `(dict)`, `from=`, `@omitempty`, `@skip`, or inline `{}`) | `T` (constant) | `T` (constant) | `T` (constant) | `T` (constant) | `T` (constant) |
| `@skip` | Validated by linter; omitted from output types & projector (`type` defaults to `str`) | omitted | omitted | omitted | omitted | omitted |

---

## 3. Portable Field Names (`E403`) & GJSON `from="..."` / `jsonify path="..."` (`E040`, `E041`)

- Every `json` field name (after stripping `?`) must match `^[A-Za-z_][A-Za-z0-9_]*$` and avoid un-escapable target language reserved words (`from`, `type`, `class`, `in`, `return`, etc.) (`E403`).
- `from="<gjson_path>"` on `json` fields and `path="<gjson_path>"` on `jsonify` evaluate a deterministic GJSON path subset:
  - Direct wire key remap: `sender str from="from"`, `track_id str from="track-id"`, `ctx str from="@context"`.
  - Escaped literal dot in flat wire key: `os_ver str from=#"os\.version"#` (or `from="os\\.version"`).
  - Nested object path & non-negative array index: `author_name str from="data.authors.0.name"`.
  - Array count (`#` $\to$ `int`): `total_items int from="items.#"`.
  - Array element projection (`#.subpath` $\to$ `(array)T`): `tag_names (array)str from="tags.#.name"`.
- **Diagnostics**: `E040` (malformed path, `*`, `#[...]`, negative index, `..`), `E041` (duplicate field or wire key), `W011` (deprecated positional alias), `W040` (whitespace in `from=`), `W041` (`path=` used on a `json` field instead of `from=`).

---

## 4. Block `define` Reuse & `parsmith json-to-kdl`

- **Block `define` in `json`**: when multiple `json` schemas share common fields, extract `define SHARED-FIELDS { ... }` and place the bare identifier `SHARED-FIELDS` inside each `json` block.
- **Bootstrap from JSON payload**:
  ```bash
  parsmith json-to-kdl sample.json --name ResponseSchema -o schema.kdl
  curl -s https://api.example.com/v1/items/1 | parsmith json-to-kdl --name ItemResponse
  ```
  Infers a lint-validated hierarchical inline `json` schema with `from="..."` key aliases and `@omitempty` / nullable `?` inference across array items.

# CLI & Host SDK Reference

`parsmith` can be used as a standalone native CLI binary (`parsmith`) or embedded directly into Python (`parsmith-py`) and JavaScript/TypeScript (`parsmith-js`) applications via native Rust bindings. All interfaces share the same 5-pass `parsmith` compiler, linter, and code generation engine.

---

## 1. CLI Reference (`parsmith`)

### Overview

```text
parsmith <COMMAND>

Commands:
  check        Validate a KDL 2.0 schema file and its imports without generating code
  generate     Generate typed parser code from a KDL 2.0 schema
  json-to-kdl  Bootstrap a KDL v2.1 `json` schema from a sample JSON payload
  version      Print the parsmith version
```

Running `parsmith` without a subcommand prints `parsmith <version>` and exits with code `0`.

---

### `parsmith check`

Runs the 5-pass schema validation pipeline (KDL 2.0 parsing, transitive `import` resolution, structural linting, cross-target identifier checks, and pipeline type checking) without emitting target code.

```bash
parsmith check <PATHS>... [-f text|json] [-t <target>...]
```

| Flag / Argument | Default | Description |
| :--- | :--- | :--- |
| `<PATHS>...` | *(required)* | One or more `.kdl` schema files or directories to validate (along with all transitive `import` files). |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format: `text` renders Rust-style annotated source snippets; `json` outputs a structured `{"diagnostics": [...]}` payload. |
| `-t`, `--target <target>` | *(all 8 ecosystems)* | Target language(s) for Pass 4 identifier and reserved-keyword validation (`E403`, `E402`). Accepts `python`, `javascript`, `typescript`, `go`, `rust`, `elixir` (`ex`), `php`, `csharp` (`cs`), `kotlin` (`kt`). Repeatable. |

**Exit codes:**
- `0` — Validation succeeded with zero error diagnostics (warnings may still be printed).
- `1` — One or more `E000`–`E403` error diagnostics were emitted.

**Examples:**

```bash
# Validate against all target ecosystems (Python, JS/TS, Go, Rust)
parsmith check schemas/catalog.kdl

# Scope reserved-keyword checks (E403) to Python and TypeScript only
parsmith check schemas/catalog.kdl -t python -t typescript

# Emit machine-readable JSON diagnostics for CI or editor integrations
parsmith check schemas/catalog.kdl -f json
```

---

### `parsmith generate`

Compiles one or more `.kdl` schemas (or directories of `.kdl` schemas) and emits standalone target code for `python`, `ts`, `js`, `go`, `rust`, `elixir` (`ex`), `php`, `csharp` (`cs`), or `kotlin` (`kt`). During directory scans, `_*.kdl` helper files are skipped as root entrypoints, and `-R / --separate-runtime` aggregates runtime helpers across all compiled modules in a two-pass batch build.

```bash
parsmith generate <target> <PATHS>... [-o <file_or_dir>] [TARGET_OPTIONS]
```

If compilation produces any `E000`–`E403` errors, diagnostics are printed to `stderr` (in `text` or `json` format) and `parsmith` exits with code `1` without writing output files.

#### Output Path Resolution (`-o / --output`)

Every `parsmith generate <target>` subcommand accepts `-o <OUTPUT>` (`--output <OUTPUT>`):

1. **Omitted or `-` (stdout)**: Writes the primary generated module directly to `stdout`.
2. **Explicit file path** (a path ending in `.py`, `.ts`, `.js`, `.go`, `.rs`, `.ex`, `.php`, `.cs`, or `.kt` that does not end with a path separator and is not an existing directory):
   - Writes the primary generated module to that exact file path.
   - Uses the file stem (for example, `custom_parser` from `-o src/custom_parser.py`) as the generated module stem.
   - Writes any companion files (`.d.ts` for `generate js`, or `<runtime_name>.{py,go,rs,ex,php,cs,kt}` when `-R / --separate-runtime` is enabled) into the same parent directory (`src/`).
3. **Directory path** (for example, `-o ./generated` or `-o ./generated/`):
   - Creates the directory if it does not exist.
   - Writes all generated files (`<schema_stem>.<ext>`, plus `.d.ts` or `<runtime_name>.<ext>`) into that directory.

---

#### `parsmith generate python` (alias: `py`)

Generates Python 3.10+ parsers and REST clients strictly typed for `mypy --strict`.

```bash
parsmith generate python <path.kdl> [OPTIONS]
```

| Flag | Default | Allowed Values & Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.py` file path, directory path, or `-` for `stdout`. |
| `-L`, `--html-lib <LIB>` (alias: `--lib`) | `bs4` | Python HTML DOM backend: `bs4` (`BeautifulSoup4`), `lxml` (`lxml.html`), `parsel` (`parsel.Selector`), `selectolax` (alias `slax`, `selectolax.lexbor`). |
| `--models <MODE>` | `typeddict` | Output model flavor: `typeddict` (zero-overhead `TypedDict` classes returning `dict`) or `dataclass` (`@dataclass(slots=True)` classes). |
| `--bs4-parser <PARSER>` | `lxml` | Underlying parser passed to `BeautifulSoup(..., "<PARSER>")` when `-L bs4`: `lxml`, `html.parser`, or `html5lib`. |
| `--http-io <MODE>` | `both` | HTTP method emission mode for `@request` and `(rest)struct`: `both` (sync + `async_*`), `sync` only, or `async` only. |
| `-R`, `--separate-runtime` | `false` | Emit shared `rt_*` / `Rt*` / `RT_*` helpers and HTTP transport adapters into `<runtime_name>.py` and import them from the parser module. |
| `--runtime-name <NAME>` | `.parsmith_runtime` | Module import path for the separate runtime file when `-R` is active (`.parsmith_runtime` emits `from .parsmith_runtime import ...` and writes `parsmith_runtime.py`; `parsmith_runtime` without a leading dot emits `from parsmith_runtime import ...`). |
| `--prologue <CODE>` | *(none)* | Custom Python code injected after imports and runtime utilities. |
| `--epilogue <CODE>` | *(none)* | Custom Python code appended at the end of the generated module. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

>[!warning]
> The `bs4` and `selectolax` (`slax`) backends do not support XPath (`E203`). If your schema uses `xpath`, `xpath-all`, `xpath-remove`, or `xpath` conditions, pass `-L lxml` or `-L parsel`.

**Examples:**

```bash
# Generate a single-file Python parser using selectolax and dataclasses
parsmith generate python schemas/product.kdl -L selectolax --models dataclass -o src/product_parser.py

# Generate a REST/HTML module with a shared runtime file (src/catalog.py + src/parsmith_runtime.py)
parsmith generate py schemas/catalog.kdl -R --http-io async -o src/catalog.py
```

---

#### `parsmith generate ts` (alias: `typescript`)

Generates a standalone TypeScript (`.ts`) module containing exported interfaces (`<Struct>Type`, `<Schema>Json`), parser classes, and `rt*` runtime helpers over standard Web/DOM APIs (`DOMParser`, `Document`, `Element`, `fetch`).

```bash
parsmith generate ts <path.kdl> [-o <file_or_dir>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.ts` file path, directory path, or `-` for `stdout`. |
| `--prologue <CODE>` | *(none)* | Custom TypeScript code injected after imports and runtime utilities. |
| `--epilogue <CODE>` | *(none)* | Custom TypeScript code appended at the end of the generated module. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

---

#### `parsmith generate js` (alias: `javascript`)

Generates a clean ES module (`.js`) paired with a companion TypeScript declaration file (`.d.ts`).

```bash
parsmith generate js <path.kdl> [-o <file_or_dir>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.js` file path (writes `<stem>.js` and `<stem>.d.ts` side by side), directory path, or `-` (prints `.js` to `stdout`). |
| `--prologue <CODE>` | *(none)* | Custom JavaScript code injected after runtime utilities. |
| `--epilogue <CODE>` | *(none)* | Custom JavaScript code appended at the end of the generated module. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

**Example:**

```bash
# Writes dist/catalog.js and dist/catalog.d.ts
parsmith generate js schemas/catalog.kdl -o dist/catalog.js
```

---

#### `parsmith generate go` (alias: `golang`)

Generates `gofmt`-compliant Go 1.26+ parsers (`github.com/PuerkitoBio/goquery`) and REST clients (`net/http` + `github.com/tidwall/gjson`).

```bash
parsmith generate go <path.kdl> [-o <file_or_dir>] [--package <PKG>] [-R] [--runtime-name <NAME>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.go` file path, directory path, or `-` for `stdout`. |
| `--package <PKG>` | `main` | Go package identifier emitted at the top of generated `.go` file(s). |
| `-R`, `--separate-runtime` | `false` | Emit shared `rt*` / `Rt*` runtime helpers into `<runtime_name>.go` in the same Go package. |
| `--runtime-name <NAME>` | `parsmith_runtime` | File stem for the separate Go runtime file when `-R` is active. |
| `--prologue <CODE>` | *(none)* | Custom Go code injected after `import (...)` and runtime utilities. |
| `--epilogue <CODE>` | *(none)* | Custom Go code appended at the end of the generated file. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

>[!tip]
> When compiling multiple `.kdl` schemas into the same Go package, pass `-R --package parsers` so `rt*` runtime helpers are emitted once in `parsmith_runtime.go` instead of being duplicated across files.

---

#### `parsmith generate rust` (alias: `rs`)

Generates `rustfmt`-compliant Rust 2021/2024 parsers (`dom_query`, `regex`, `serde`) and async REST clients (`reqwest`). By default, emits a single self-contained `.rs` file with an inline `pub mod rt { ... }` that automatically omits `dom_query` when the schema has no HTML constructs and omits `reqwest` when the schema has no HTTP constructs.

```bash
parsmith generate rust <path.kdl> [-o <file_or_dir>] [-R] [--runtime-name <NAME>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.rs` file path, directory path, or `-` for `stdout`. |
| `-R`, `--separate-runtime` | `false` | Emit shared runtime helpers into `<runtime_name>.rs` and import them via `use super::<runtime_name> as rt;`. |
| `--runtime-name <NAME>` | `parsmith_runtime` | Module stem for the separate Rust runtime file when `-R` is active. |
| `--prologue <CODE>` | *(none)* | Custom Rust code injected after `use` imports and `pub mod rt`. |
| `--epilogue <CODE>` | *(none)* | Custom Rust code appended at the end of the generated module. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

---

#### `parsmith generate elixir` (alias: `ex`)

Generates Elixir 1.18–1.20+ parsers (`Floki` + built-in standard `JSON`) and REST clients (Erlang/OTP 27–29 `:httpc` + `Rt.via_req/1`). Automatically omits `Floki` helpers when the schema has no HTML constructs.

```bash
parsmith generate elixir <path.kdl> [-o <file_or_dir>] [--package <PREFIX>] [-R] [--runtime-name <NAME>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.ex` file path, directory path, or `-` for `stdout`. |
| `--package <PREFIX>` (alias: `--namespace`) | *(none)* | Elixir module namespace prefix (e.g. `MyApp.Parsers`). |
| `-R`, `--separate-runtime` | `false` | Emit shared runtime helpers into `<runtime_name>.ex`. |
| `--runtime-name <NAME>` | `parsmith_runtime` | File stem for the separate Elixir runtime file when `-R` is active. |
| `--prologue <CODE>` | *(none)* | Custom Elixir code injected before module definitions. |
| `--epilogue <CODE>` | *(none)* | Custom Elixir code appended at the end of the generated file. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

---

#### `parsmith generate php`

Generates PHP 8.1+ `declare(strict_types=1);` `readonly class` models (`\JsonSerializable`), HTML/XPath parsers, and `SyncTransport` REST clients (`Rt::viaCurl()`, `Rt::viaStream()`, `Rt::viaGuzzle()`).

```bash
parsmith generate php <path.kdl> [-o <file_or_dir>] [-L native|dom-crawler] [--package <NS>] [-R] [--runtime-name <NAME>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.php` file path, directory path, or `-` for `stdout`. |
| `-L`, `--html-lib <LIB>` (alias: `--lib`) | `native` | PHP HTML DOM backend: `native` (zero-Composer `ext-dom` `\DOMDocument` + `\DOMXPath` via compile-time CSS3-to-XPath 1.0 lowering) or `dom-crawler` (`Symfony\Component\DomCrawler\Crawler`). |
| `--package <NS>` (alias: `--namespace`) | *(none)* | PHP `namespace` declaration (`App\Parsers` or `App.Parsers`). |
| `-R`, `--separate-runtime` | `false` | Emit `final class Rt` and shared runtime classes into `<runtime_name>.php` and load via `require_once`. |
| `--runtime-name <NAME>` | `parsmith_runtime` | File stem for the separate PHP runtime file when `-R` is active. |
| `--prologue <CODE>` | *(none)* | Custom PHP code injected after `declare(strict_types=1);` / `namespace`. |
| `--epilogue <CODE>` | *(none)* | Custom PHP code appended at the end of the generated file. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

---

#### `parsmith generate csharp` (alias: `cs`)

Generates C# 12 / .NET 8+ parsers (`AngleSharp` + conditional `AngleSharp.XPath`), `sealed record` models (`System.Text.Json`), and `HttpClient` REST clients.

```bash
parsmith generate csharp <path.kdl> [-o <file_or_dir>] [--package <NS>] [-R] [--runtime-name <NAME>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.cs` file path, directory path, or `-` for `stdout`. |
| `--package <NS>` (alias: `--namespace`) | *(none)* | C# file-scoped `namespace` declaration (e.g. `MyApp.Parsers`). |
| `-R`, `--separate-runtime` | `false` | Emit `public static partial class Rt` into `<runtime_name>.cs`. |
| `--runtime-name <NAME>` | `parsmith_runtime` | File stem for the separate C# runtime file when `-R` is active. |
| `--prologue <CODE>` | *(none)* | Custom C# code injected after `using` / `namespace` declarations. |
| `--epilogue <CODE>` | *(none)* | Custom C# code appended at the end of the generated file. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

---

#### `parsmith generate kotlin` (alias: `kt`)

Generates Kotlin (JVM 11+) `data class` models (`kotlinx.serialization.json`), HTML/XPath parsers (`org.jsoup:jsoup`), and `java.net.http.HttpClient` REST clients.

```bash
parsmith generate kotlin <path.kdl> [-o <file_or_dir>] [--package <PKG>] [-R] [--runtime-name <NAME>] [--prologue <CODE>] [--epilogue <CODE>] [-f text|json]
```

| Flag | Default | Description |
| :--- | :--- | :--- |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.kt` file path, directory path, or `-` for `stdout`. |
| `--package <PKG>` (alias: `--namespace`) | *(none)* | Kotlin `package` declaration (e.g. `com.example.parsers`). |
| `-R`, `--separate-runtime` | `false` | Emit `object Rt` into `<runtime_name>.kt`. |
| `--runtime-name <NAME>` | `parsmith_runtime` | File stem for the separate Kotlin runtime file when `-R` is active. |
| `--prologue <CODE>` | *(none)* | Custom Kotlin code injected after imports. |
| `--epilogue <CODE>` | *(none)* | Custom Kotlin code appended at the end of the generated file. |
| `-f`, `--format <text\|json>` | `text` | Diagnostic output format on `stderr`. |

---

### `parsmith json-to-kdl`

Infers a KDL v2.1 `json` (or `(array)json`) schema with nested inline JSON blocks from a sample JSON document.

```bash
parsmith json-to-kdl [<INPUT>] [--name <NAME>] [-o <OUTPUT>]
```

| Argument / Flag | Default | Description |
| :--- | :--- | :--- |
| `<INPUT>` | `stdin` | Path to a sample `.json` file, or `-` (or omitted) to read JSON from `stdin`. |
| `--name <NAME>` | `JsonResponse` | Root `json` schema identifier. |
| `-o`, `--output <OUTPUT>` | `stdout` | Output `.kdl` file path, or `-` (or omitted) to print to `stdout`. |

**Examples:**

```bash
# Convert a local JSON fixture into a KDL schema file
parsmith json-to-kdl fixtures/user.json --name UserProfile -o schemas/user.kdl

# Pipe a live API response directly into parsmith json-to-kdl
curl -s https://api.example.com/v1/items/42 | parsmith json-to-kdl --name ItemPayload
```

---

### `parsmith version`

Prints the `parsmith` compiler version.

```bash
parsmith version [-f text|json]
```

- `parsmith version` (or `-f text`): `parsmith 1.0.0`
- `parsmith version -f json`: `{"name":"parsmith","version":"1.1.0"}`

---

## 2. Python Host SDK (`parsmith-py`)

The `parsmith` Python package (`crates/parsmith-py`, built on `PyO3` `abi3`) exposes the Rust compiler, code generator, and schema inspector directly in Python without spawning subprocesses.

### Top-Level Functions

```python
import parsmith

# 1. Validate a schema (file path, PathLike, or inline KDL string)
check_res: parsmith.CheckResult = parsmith.check(
    source_or_path,
    *,
    sources=None,   # Optional Mapping[str, str] for in-memory VFS compilation
    targets=None,   # Optional str | Sequence[str] ("python", "javascript", "go", "rust")
    path=None,      # Optional logical file path when source_or_path is an inline KDL string
)

# 2. Generate target code
gen_res: parsmith.GenerateResult = parsmith.generate(
    source_or_path,
    target="python",          # "python" | "py-bs4" | "py-lxml" | "py-parsel" | "py-selectolax" | "ts" | "js" | "go" | "rust" | "elixir" | "php" | "php-native" | "php-dom-crawler" | "csharp" | "kotlin"
    *,
    sources=None,             # Optional Mapping[str, str] VFS map
    options=None,             # Optional CodegenOptions or Mapping[str, Any]
    path=None,                # Optional logical filename for inline KDL
    html_lib=None,            # "bs4" | "lxml" | "parsel" | "selectolax" | "slax" (or "native" | "dom-crawler" when target="php")
    php_html_lib=None,        # "native" | "dom-crawler"
    models=None,              # "typeddict" | "dataclass"
    bs4_parser=None,          # "lxml" | "html.parser" | "html5lib"
    http_io=None,             # "both" | "sync" | "async"
    separate_runtime=None,    # bool
    runtime_name=None,        # str (default "parsmith_runtime")
    package=None,             # str (default "main" for Go; namespace/package for Elixir/PHP/C#/Kotlin)
    module_stem=None,         # str (overrides output filename stem)
    prologue=None,            # str
    epilogue=None,            # str
)

# 3. Introspect compiled AST metadata
mod_info: parsmith.ModuleInfo = parsmith.inspect_schema(
    source_or_path,
    *,
    sources=None,
    path=None,
)

# 4. Bootstrap KDL from sample JSON
kdl_text: str = parsmith.json_to_kdl(json_input, *, name="JsonResponse")

# 5. Version query
ver: str = parsmith.version()  # Also available as parsmith.__version__
```

>[!note]
> **How `source_or_path` is resolved:**
> - If `source_or_path` is an `os.PathLike` (such as `pathlib.Path`), it is always treated as a file path.
> - If `sources` is provided and `source_or_path` matches a key in `sources` (or is a single-line `.kdl` filename), it resolves that entry from the in-memory `MemorySourceLoader`.
> - Otherwise, if `source_or_path` is a single-line string pointing to an existing file on disk or ending in `.kdl`, it loads from the filesystem (`FsSourceLoader`); multiline strings or strings containing `{` are compiled directly as inline KDL source.

### Validation & Code Generation Example

```python
import parsmith

schema = """
struct ProductCard {
    title { css "h2.title"; text; trim }
    price { css ".price"; text; re #"(\\d+\\.\\d{2})"#; to-float; fallback #null }
}
"""

# Validate across Python and Rust targets
res = parsmith.check(schema, targets=["python", "rust"], path="product.kdl")
if not res.ok:
    print(res.format_text())
    for err in res.errors:
        print(err.code, err.code_name, err.line, err.col, err.message, err.hint)

# Generate Python (selectolax + dataclass) and TypeScript from the same schema
py_out = parsmith.generate(
    schema,
    target="py-selectolax",
    models="dataclass",
    module_stem="product",
)
assert py_out.ok
python_code: str = py_out["product.py"]  # or py_out.code / py_out.primary_code()

ts_out = parsmith.generate(schema, target="ts", module_stem="product")
typescript_code: str = ts_out["product.ts"]
```

### In-Memory VFS Compilation (`sources`)

When building web services, playgrounds, or database-backed scraping pipelines, pass a dictionary of virtual files to `sources`. Relative `import` statements are resolved purely in memory via `MemorySourceLoader` without touching the filesystem:

```python
import parsmith

vfs = {
    "shared/common.kdl": """
        define PRICE-RE=#"(\\d+\\.\\d{2})"#
        json SellerInfo {
            id int
            name str
        }
    """,
    "scrapers/listing.kdl": """
        import "../shared/common.kdl" {
            (define)PRICE-RE
            (json)SellerInfo
        }

        struct ListingPage {
            price { css ".price"; text; re PRICE-RE; to-float }
            seller { css "script#seller"; text; jsonify SellerInfo }
        }
    """,
}

result = parsmith.generate(
    "scrapers/listing.kdl",
    target="python",
    sources=vfs,
    separate_runtime=True,
)
assert result.ok
print(list(result.files.keys()))  # ['listing.py', 'parsmith_runtime.py']
```

### Schema Introspection (`inspect_schema`)

`parsmith.inspect_schema()` runs the 5-pass compiler and returns a `ModuleInfo` object exposing the complete typed structure of the module:

| Class | Key Properties & Methods |
| :--- | :--- |
| `ModuleInfo` | `ok`, `doc`, `source_file`, `struct_kinds` (`dict[str, str]`), `structs` (`list[StructInfo]`), `functions` (`list[FunctionInfo]`), `json_defs` (`list[JsonDefInfo]`), `extensions` (`list[ExtensionInfo]`), `diagnostics`, `errors`, `warnings`, `to_dict()`, `to_json()`, `format_text()`, `format_json()` |
| `StructInfo` | `name`, `kind` (`"item"`, `"list"`, `"dict"`, `"table"`, `"flat"`, `"raw"`, `"rest"`), `doc`, `keep_order`, `fields` (`list[FieldInfo]`), `init_fields` (`list[InitFieldInfo]`), `checks` (`list[CheckMethodInfo]`), `has_pre_validate`, `key_type`, `value_type`, `requests` (`list[RequestInfo]`), `rest_endpoints` (`list[RestEndpointInfo]`), `errors` (`list[ErrorVariantInfo]`), `to_dict()` |
| `FieldInfo` | `name`, `type_info` (`TypeInfo`), `has_fallback`, `to_dict()` |
| `TypeInfo` | `base` (`"string"`, `"int"`, `"float"`, `"bool"`, `"document"`, `"nested"`, `"json"`, `"dict"`, ...), `dsl_name` (`"STRING"`, `"INT"`, ...), `is_array`, `is_optional`, `is_dict`, `reference`, `key_type`, `value_type`, `omitempty`, `skip`, `to_dict()` |
| `JsonDefInfo` | `name`, `kind` (`"object"`, `"array"`, `"dict"`), `doc`, `is_array`, `is_dict`, `is_hoisted`, `path`, `fields` (`list[JsonFieldInfo]`), `key_type`, `value_type`, `to_dict()` |
| `JsonFieldInfo` | `name`, `type_info`, `alias` (`from=` path), `doc`, `is_optional`, `omitempty`, `skip`, `is_dict`, `to_dict()` |
| `RequestInfo` / `RestEndpointInfo` | `name`, `method`, `url`, `doc`, `response_schema`, `response_path`, `result_alias_name`, `body_kind`, `placeholders` (`list[PlaceholderInfo]`), `to_dict()` |
| `ErrorVariantInfo` | `variant_name`, `name`, `status`, `schema_name`, `suffix`, `required_keys`, `conditions`, `to_dict()` |

```python
import parsmith

info = parsmith.inspect_schema("scrapers/listing.kdl", sources=vfs)
assert info.ok

for s in info.structs:
    print(f"Struct {s.name} (kind={s.kind}):")
    for f in s.fields:
        t = f.type_info
        print(f"  - {f.name}: {t.dsl_name} (array={t.is_array}, optional={t.is_optional}, ref={t.reference})")

# Export as a Python dict or JSON string
schema_dict = info.to_dict()
diagnostics_json = info.format_json()
```

---

## 3. JavaScript / TypeScript Host SDK (`parsmith-js`)

The `parsmith` Node-API package (`crates/parsmith-js` + `js/`, built on `napi-rs`) provides synchronous native bindings for Node.js, Bun, and Deno with full TypeScript declarations (`js/index.d.ts`).

### Top-Level Functions

```ts
import {
  check,
  generate,
  inspectSchema,
  jsonToKdl,
  version,
  CodegenOptions,
} from "parsmith";

// 1. Validate a schema (string path, URL, or inline KDL string)
const checkRes = check(sourceOrPath, {
  sources, // Optional Record<string, string> | Map<string | URL, string>
  targets: ["typescript", "python"],
  path: "inline.kdl",
});

// 2. Generate target code (defaults to "ts" when target is omitted)
const genRes = generate(sourceOrPath, {
  target: "ts",             // "ts" | "js" | "python" | "py-lxml" | "go" | "rust"
  sources,                  // Optional in-memory VFS
  separateRuntime: false,   // Accepts camelCase, snake_case, or kebab-case option keys
  moduleStem: "catalog",
});

// 3. Introspect schema AST metadata
const modInfo = inspectSchema(sourceOrPath, { sources });

// 4. Bootstrap KDL from sample JSON
const kdlText = jsonToKdl('{"id": 1, "tags": ["a", "b"]}', "ItemPayload");

// 5. Version query
console.log(version());
```

### In-Memory VFS (`Record` or `Map`)

`sources` accepts either a plain JavaScript object (`Record<string, string>`) or a `Map<string | URL, string>`:

```ts
import { generate, inspectSchema } from "parsmith";

const sources = new Map<string, string>([
  [
    "models.kdl",
    `json Author {
        id int
        handle str
    }`,
  ],
  [
    "post.kdl",
    `import "./models.kdl" { (json)Author }
    struct PostCard {
        author { css "script.author-json"; text; jsonify Author }
    }`,
  ],
]);

const res = generate("post.kdl", { target: "js", sources });
if (!res.ok) {
  console.error(res.formatText());
} else {
  const jsCode = res.getFile("post.js");
  const dtsCode = res.getFile("post.d.ts");
}
```

### Dual Serialization Contract (`toObject()` vs `toDict()` vs `formatJson()`)

Every class in `parsmith-js` (`Diagnostic`, `CheckResult`, `CodegenOptions`, `GenerateResult`, `ModuleInfo`, `StructInfo`, `FieldInfo`, `JsonDefInfo`, `RequestInfo`, `RestEndpointInfo`, `ErrorVariantInfo`, `TypeInfo`, etc.) implements a dual serialization contract:

| Method | Key Casing | Purpose |
| :--- | :--- | :--- |
| `toObject()` / `toJSON()` | `camelCase` | Idiomatic JavaScript/TypeScript plain objects matching `js/index.d.ts` (automatically used by `JSON.stringify(instance)`). Properties include `codeName`, `errorCount`, `htmlLib`, `separateRuntime`, `typeInfo`, `jsonDefs`, `restEndpoints`, `isHoisted`. |
| `toDict()` | `snake_case` | Cross-language dictionary representation matching `parsmith-py`'s `.to_dict()` contract (`code_name`, `error_count`, `html_lib`, `separate_runtime`, `type_info`, `json_defs`, `rest_endpoints`, `is_hoisted`). |
| `formatJson()` / `toJson()` | `snake_case` JSON string | Pretty-printed canonical `parsmith` JSON string (`CheckResult.formatJson()`, `GenerateResult.formatJson()`, `ModuleInfo.toJson()`, `ModuleInfo.formatJson()`). |
| `formatText()` / `render()` | Annotated text | Human-readable Rust-style source snippet with line numbers, `^^^` carets, and `= hint:` footers. |

```ts
import { inspectSchema } from "parsmith";

const info = inspectSchema(`
  struct Article {
    reading-time { css ".meta"; text; re #"(\\d+) min"#; to-int; fallback 0 }
  }
`);

// 1. camelCase JS object (or JSON.stringify(info))
const jsObj = info.toObject();
console.log(jsObj.structKinds); // { Article: "item" }
console.log(jsObj.structs[0].fields[0].hasFallback); // true

// 2. snake_case dict matching Python's info.to_dict()
const pyDict = info.toDict();
console.log(pyDict.struct_kinds); // { Article: "item" }
console.log(pyDict.structs[0].fields[0].has_fallback); // true

// 3. Formatted JSON string of diagnostics
console.log(info.formatJson());
```

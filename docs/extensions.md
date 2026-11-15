# User Extension Operations (`extension`)

An **Extension operation** declares a typed, target-specific custom pipeline operation in KDL 2.0. Extensions let you embed custom decoding, cryptographic hashing, date normalization, or domain-specific transformations into a pipeline while preserving static type checking and lazy code emission across Python, TypeScript, JavaScript, Go, Rust, Elixir, PHP, C#, and Kotlin.

---

## 1. Declaring & Calling Extension Operations

Each top-level `extension <Namespace> { ... }` block declares one or more operations as direct child nodes:

```kdl
extension Utils {
    to-base64 {
        sig str str

        py {
            import "from base64 import b64encode"
            emit #"{{out}} = b64encode({{in}}.encode("utf-8")).decode("ascii")"#
        }

        js {
            emit #"const {{out}} = btoa({{in}});"#
        }

        go {
            import "encoding/base64"
            emit #"{{out}} := base64.StdEncoding.EncodeToString([]byte({{in}}))"#
        }

        rust {
            emit #"let {{out}}: String = {{in}}.as_bytes().iter().map(|b| format!("{b:02x}")).collect();"#
        }
    }
}
```

>[!note]
> Every direct child of `extension <Namespace>` is an operation name (`to-base64` above). There is no intermediate `op` wrapper node.

### Invoking with `!Namespace.op`

Inside any field, `@init`, `@check`, or `fn` pipeline, invoke an extension operation with a mandatory `!` prefix:

```kdl
struct TokenCard {
    encoded_id {
        css "span.raw-id"
        text
        trim
        !Utils.to-base64
    }
}
```

The `!` prefix separates user-defined **Extension operations** (`!Utils.to-base64`) from built-in DSL operations (`trim`, `text`) and **Precomputed field** references (`@raw_id`). Referencing an undeclared namespace or operation raises `E300`.

---

## 2. Type Signatures (`sig <In> <Out>`)

Every extension operation must declare exactly one `sig <InType> <OutType>` node (`E001`):

```kdl
sig str str
sig (array)str str
sig str int?
sig T T
sig T bool
```

### Concrete Types & Modifiers

- **Base types**: `doc`, `str`, `int`, `float`, `bool`.
- **Array modifier**: `(array)<type>` prefix annotation (e.g. `(array)str`, `(array)int`).
- **Nullable modifier**: `?` suffix (e.g. `str?`, `(array)str?`).

### Whole-`TypeInfo` Generic `T`

The type variable `T` binds the complete incoming `TypeInfo` (including its `[]` array and `?` optionality flags) at each call site:

- **`sig T T`**: Identity or pass-through signature (useful for logging, tracing, or in-place assertions). Preserves the exact input `TypeInfo`.
- **`sig T bool`**: Accepts any pipeline value and returns `bool`.

Constraints on `T`:
- `T` cannot be combined with `(array)` or `?` modifiers (`(array)T` and `T?` raise `E400`).
- Generic output `T` requires generic input `T` (`sig str T` raises `E100`).

### No Implicit Array Mapping

Unlike built-in string operations (`trim`, `lower`, `repl`), **Extension operations do not implicitly map over arrays**:

- Calling an operation declared with `sig str str` on a `STR[]` pipeline value raises `E100`.
- To process an array, declare `sig (array)str (array)str` (or `sig (array)str str`) explicitly.

---

## 3. Target Blocks (`py`, `js`, `ts`, `go`, `rust`, `ex`, `php`, `cs`, `kt`)

Inside an operation block, target implementations are grouped by language tag:

| Target Tag | Applies To CLI / SDK Target | Notes |
| :--- | :--- | :--- |
| `py` | `python` (`bs4`, `lxml`, `parsel`, `selectolax`) | Emits Python 3.10+ statements |
| `js` | `js` **and** `ts` | Shared implementation for both JavaScript (`.js`) and TypeScript (`.ts`) |
| `ts` | `ts` *(optional override)* | When present, overrides `js` for the `ts` target; when omitted, `ts` automatically falls back to the `js` block |
| `go` | `go` | Emits Go statements; leading indentation is automatically normalized to tabs |
| `rust` | `rust` | Emits Rust statements |
| `ex` / `elixir` | `elixir` (`ex`) | Emits Elixir statements (helpers emitted inside `Rt`) |
| `php` | `php` (`native`, `dom-crawler`) | Emits PHP 8.1+ statements (helpers emitted as static methods on `Rt`) |
| `cs` / `csharp` | `csharp` (`cs`) | Emits C# 12 statements (helpers emitted inside `public static partial class Rt`) |
| `kt` / `kotlin` | `kotlin` (`kt`) | Emits Kotlin statements (helpers emitted inside `object Rt`) |

>[!tip]
> You only need to declare target blocks for the languages you compile to. `parsmith check` validates all declared target blocks, while `parsmith generate <target>` reports `E400` only if a referenced extension operation lacks a block for the requested target.

---

## 4. `emit` Templates & Placeholders

Each target block must contain exactly one `emit` directive (`E001`) whose template string assigns to `{{out}}` (`E002`).

| Placeholder | Meaning | Example Expansion |
| :--- | :--- | :--- |
| `{{in}}` | Identifier of the previous pipeline step variable | `v1` |
| `{{out}}` | Identifier of the current step's output variable | `v2` |
| `{{in_type}}` | Target-language type annotation for the bound input `TypeInfo` | `list[str]`, `string[]`, `[]string`, `Vec<String>` |
| `{{out_type}}` | Target-language type annotation for the bound output `TypeInfo` | `str`, `string`, `int64`, `i64` |

### Single-Line & Multi-Line Raw Strings

Use KDL raw strings (`#"..."#`) for single-line statements and multi-line raw strings (`#"""..."""#`) for multi-statement blocks. The compiler automatically strips common leading whitespace (dedent) and indents every line to match the surrounding function body:

```kdl
extension Text {
    clean-slug {
        sig T T

        py {
            emit #"""
                # Type flow: {{in_type}} -> {{out_type}}
                {{out}} = {{in}}
                """#
        }

        js {
            emit #"""
                const {{out}} = {{in}};
                """#
        }

        go {
            emit #"""
                var {{out}} {{out_type}} = {{in}}
                """#
        }

        rust {
            emit #"""
                let {{out}}: {{out_type}} = {{in}};
                """#
        }
    }
}
```

---

## 5. Target `import` Directives & Lazy Deduplicated `helper` Blocks

### Target-Level `import`

An `import "<statement>"` inside a target block registers a top-level import with `ModuleBuilder` whenever the operation is referenced:

- **Python**: Full import statement (`import "import re"` or `import "from urllib.parse import urljoin"`). `PythonEmitter` automatically merges and deduplicates `from <mod> import ...` statements.
- **TypeScript / JavaScript**: Full ESM import statement (`import "import { createHash } from 'node:crypto';"`).
- **Go**: Package import path (`import "strings"` or `import "encoding/hex"`), with an optional `alias="<ident>"` property for aliased package imports:
  ```kdl
  go {
      import "example.com/project/normalizer" alias="norm"
      emit #"{{out}} := norm.Clean({{in}})"#
  }
  ```
- **Rust**: Use declaration (`import "use std::collections::BTreeSet;"`).

>[!note]
> The `alias=` property on `import` is supported for Go package imports (and Rust). Using `alias=` inside `py` or `js` target blocks raises `E203`.

### Lazy, Deduplicated `helper` Functions

When an operation requires multi-line logic, declare a `helper <name> { ... }` block with optional `import` nodes and a `source` string:

```kdl
extension Utils {
    slugify {
        sig str str

        py {
            helper normalize_slug {
                import "import re"
                source #"""
                    def normalize_slug(value: str) -> str:
                        return re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")
                    """#
            }
            emit #"{{out}} = normalize_slug({{in}})"#
        }

        js {
            helper normalizeSlug {
                source #"""
                    function normalizeSlug(value) {
                      return value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
                    }
                    """#
            }
            emit #"const {{out}} = normalizeSlug({{in}});"#
        }

        go {
            helper normalizeSlug {
                import "strings"
                source #"""
                    func normalizeSlug(value string) string {
                    	return strings.Trim(strings.ToLower(value), "- ")
                    }
                    """#
            }
            emit #"{{out}} := normalizeSlug({{in}})"#
        }

        rust {
            helper normalize_slug {
                source #"""
                    pub fn normalize_slug(value: &str) -> String {
                        value.trim().to_lowercase().replace(' ', "-")
                    }
                    """#
            }
            emit #"let {{out}} = rt::normalize_slug(&{{in}});"#
        }
    }
}
```

#### Lazy Emission & Deduplication Rules

1. **Lazy emission**: If an extension operation is declared (or imported) but never called in any pipeline, its `import` statements and `helper` functions are not emitted.
2. **Idempotent deduplication**: Multiple operations may declare a `helper` with the same identifier and identical `source` text; `ModuleBuilder` emits the helper once. Declaring two helpers with the same name but different `source` bodies in the same target raises `E400`.
3. **Placement in Single-File Mode vs `-R / --separate-runtime`**:

| Target | Single-File Mode *(default)* | Separate Runtime Mode (`-R / --separate-runtime`, default `parsmith_runtime`) |
| :--- | :--- | :--- |
| **Python** | Emitted in the module utility section alongside `rt_*` helpers | Emitted in `<runtime_name>.py` and imported into the parser module |
| **TypeScript / JS** | Emitted in the module utility section | Emitted in the module utility section |
| **Go** | Emitted in `<stem>.go` alongside `rt*` helpers | Emitted in `<runtime_name>.go` within the same Go package |
| **Rust** | Emitted inside `pub mod rt { ... }` (functions starting with `fn` are automatically promoted to `pub fn` and called as `rt::<helper>`) | Emitted in `<runtime_name>.rs` and accessed via `use super::<runtime_name> as rt;` (`rt::<helper>`) |

---

## 6. Sharing Extension Libraries (`import`)

Common extensions can be stored in a dedicated `.kdl` file (e.g. `extensions.kdl`) and imported into scraper schemas using the `(extension)` symbol tag:

```kdl
import "./extensions.kdl" {
    (extension)Utils
    (extension)Text
}

struct Article {
    slug {
        css "h1"
        text
        !Utils.slugify
    }
}
```

- **Extension-only files**: A `.kdl` file containing only `extension` and `define` declarations passes `parsmith check` and can be imported from disk or via an in-memory `MemorySourceLoader` VFS map, without producing empty standalone parser classes.
- **Private imports**: Imported extensions are scoped to the importing file and are not implicitly re-exported to downstream importers.

>[!warning]
> `emit` and `helper { source ... }` blocks inject verbatim target-language source code into generated parsers. Only compile `.kdl` schemas containing `extension` blocks from trusted sources.

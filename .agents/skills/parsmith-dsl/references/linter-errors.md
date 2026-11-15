# Compiler & Linter `DiagnosticCode` Reference, CLI/SDK Cheat Sheet & Repair Loop (DSL v2.1)

## 1. CLI & Host SDK Cheat Sheet

```bash
# 1. Print version
parsmith version [-f text|json]

# 2. Bootstrap hierarchical inline `json` schema from sample JSON (file or stdin)
parsmith json-to-kdl sample.json --name ResponseName -o schema.kdl
curl -s https://api.example.com/v1/items/42 | parsmith json-to-kdl --name ItemPayload

# 3. Validate schema (5-pass compiler: syntax, imports, selectors, regex, symbols, pipeline types)
parsmith check schema.kdl -f json
parsmith check schema.kdl -t py -t rs -t ex

# 4. Generate standalone parser / REST client across 9 targets
parsmith generate py schema.kdl [-L bs4|lxml|parsel|selectolax] [--models typeddict|dataclass] [--http-io both|sync|async] [-R] -o out/
parsmith generate ts schema.kdl -o out/
parsmith generate js schema.kdl -o out/          # emits <stem>.js + <stem>.d.ts
parsmith generate go schema.kdl [--package main] [-R] -o out/
parsmith generate rs schema.kdl [-R] -o out/
parsmith generate ex schema.kdl [--package MyApp] [-R] -o out/
parsmith generate php schema.kdl [-L native|dom-crawler] [--package MyApp] [-R] -o out/
parsmith generate cs schema.kdl [--package MyApp] [-R] -o out/
parsmith generate kt schema.kdl [--package com.example] [-R] -o out/
```

**Python (`parsmith-py`) & Node.js (`parsmith-js`) Host SDKs (with in-memory VFS `sources`):**
- Python: `parsmith.check(src, *, sources=..., targets=...)`, `parsmith.generate(src, target="py", *, sources=...)`, `parsmith.inspect_schema(src, *, sources=...)`, `parsmith.json_to_kdl(json_str, name=...)`.
- JS/TS: `check(src, { sources, targets })`, `generate(src, { target: "ts", sources })`, `inspectSchema(src, { sources })`, `jsonToKdl(jsonStr, name)`.

---

## 2. Canonical Compiler & Linter `DiagnosticCode` Fixes (`parsmith check -f json`)

Filter `"severity": "error"`, sort by line ascending, and fix top-to-bottom. Never return a `.kdl` file until `parsmith check` exits `0`.

| Code | Name | Cause | Canonical Fix |
| :--- | :--- | :--- | :--- |
| `E000` | `SYNTAX_ERROR` | Invalid KDL 2.0 syntax (unquoted strings, `true`/`null` without `#`, unclosed `{`, or `//` comment immediately after `\` continuation) | Quote strings, use `#true`/`#false`/`#null`, move `//` comments outside `\` continuations |
| `E001` | `INVALID_ARGUMENT_COUNT` | Wrong positional arg count on op/directive, empty `{}` in selector/conditions/`@value`, anonymous `field (array) { ... }`, colliding inline model name, `response-join` on `(rest)struct`, or missing `match` at start of `(table)struct` field | Supply required args; separate same-line ops with `;`; name item model in `(array)ItemModel { ... }`; remove `response-join` from `(rest)struct`; start `(table)struct` fields with `match { ... }` |
| `E002` | `INVALID_ARGUMENT_VALUE` | `re`/`re-all` lacks 1 capture group or uses lookarounds/named groups, invalid CSS/XPath, non-lowerable CSS4 in `php -L native`, `fmt` missing `{{}}`, HTTP status outside `100..599`, `[]`/`?` in URL path placeholder, uppercase `{{NAME}}` in `@request` literal, `@skip` on inline/`@value` block, or invalid `define` / `@error:<suffix>` | Add one `(...)` group to `re`/`re-all` (`(?:...)` for non-capturing, no lookarounds); use lowercase `{{param}}` in `@request` (compose `{{UPPER_NAME}}` in a `define` first); move `[]`/`?` to query string; use valid ASCII `:suffix` |
| `E003` | `INVALID_IMPORT` | Empty `import` block, missing `(kind)` tag, missing file, duplicate import path, or circular import | Use `import "./shared.kdl" { (struct)Name; (json)Model; (define)CONST; (extension)Utils; (fn)func }` |
| `E040` | `MALFORMED_DOTPATH` | Leading/trailing `.`, `..`, negative index, `*`, or `#[...]` filter in `from="..."` or `jsonify path="..."` | Use valid GJSON path (`a.b.0.c`, `#"a\.b"#`, `items.#`, `tags.#.name`) |
| `E041` | `DUPLICATE_JSON_KEY` | Duplicate field name or duplicate wire key (`from=`) in the same `json` schema | Rename duplicate field or disambiguate `from="..."` wire paths |
| `E100` | `TYPE_MISMATCH` | Op input incompatible with previous step (`css-all` into scalar op without extract, `trim` on `DOC`, `re` after `to-int`), `@check` not ending in `BOOL`, or `fallback` literal mismatch | Add `text`/`attr` after `css`/`css-all`; use `css` or `first`/`last` when scalar needed; match `fallback` literal to pipeline type (or `#null` / `{}`) |
| `E200` | `UNKNOWN_NODE` | Unrecognized top-level declaration, pipeline op, or condition operator | Use valid DSL keywords or prefix custom extension calls with `!Namespace.op` |
| `E203` | `DISALLOWED_DIRECTIVE` | HTML op inside `(raw)struct`/`(raw)fn`, XPath op in `bs4`/`selectolax`/`go`/`rs`/`ex`, struct directive inside `fn`, regular field in `(rest)struct`/`(dict)struct`, `has-attr`/`css` inside `match {}`, or `len-*`/`gt`/`re-any` outside `assert {}` | Keep `(rest)struct` to `@doc`/`@request`/`@error`; use CSS selectors or an XPath-capable backend (`lxml`/`parsel`/`ts`/`js`/`php`/`cs`/`kt`); keep `match {}` to string conditions; keep `len-*`/`gt`/`re-any` inside `assert {}` |
| `E300` | `UNDEFINED_SYMBOL` | Reference to undeclared `struct`, `json`, `define`, `!Namespace.op`, or circular reference | Declare the referenced symbol or import it |
| `E301` | `UNDEFINED_INIT_FIELD` | `@<name>` references a field not declared in the struct's `@init` block | Declare `<name> { ... }` inside `@init { ... }` |
| `E302` | `INVALID_DECLARATION_ORDER` | Local `struct` or `json` referenced via `nested`, `jsonify`, `@request`, `@error`, or schema field before its declaration | Move shared `json` and helper `struct` definitions **above** the caller (or inline single-use `json` blocks) |
| `E400` | `INVALID_TYPE_DECLARATION` | Invalid struct/json/fn type annotation, invalid `extension` `sig`, duplicate resolved `@error` variant names in `(rest)struct`, or same key in `@error` positional args and properties | Use valid `(item\|list\|dict\|table\|flat\|rest\|raw)struct`; disambiguate colliding `@error` rules with `@error:<suffix>` |
| `E401` | `MISSING_REQUIRED_FIELD` | Missing `@split-doc` (`list`), `@split-doc`+`@key`+`@value` (`dict`), `@table`+`@rows`+`@match`+`@value` (`table`), or `@request` (`rest`) | Add all required structural directives for the struct kind |
| `E402` | `DUPLICATE_DECLARATION` | Duplicate symbol/field/`@init`/`@check`/`@request name=`, or conflicting specs for the same placeholder in one `@request` | Rename colliding symbols/methods; use identical spec for repeated `{{param:type}}` |
| `E403` | `INVALID_IDENTIFIER` | Symbol or `json` field name has `-` (`kebab-case`), trailing colon (`url: str`), `@`/`$`, or collides with an un-escapable target reserved word (`from`, `type`, `class`, `in`) | Use a portable `snake_case` identifier + `from="wire-key"` (e.g. `sender str from="from"`) |
| `W011` | `DEPRECATED_JSON_ALIAS` | Positional string alias on `json` field (`field str "wire_key"`) | Replace with `from="wire_key"` |
| `W040` | `WHITESPACE_IN_PATH` | Leading/trailing whitespace in `from="..."` | Strip accidental spaces |
| `W041` | `PATH_SYNONYM_IN_JSON` | `path="..."` used on a `json` field | Replace `path="..."` with `from="..."` (`path=` is for `jsonify`) |

---

## 3. Safe Syntax & Repair Loop

- Keep the last known-good schema text; fix one primary diagnostic at a time top-to-bottom.
- After two identical errors on the same line, switch syntax form (e.g. inline vs block, CSS3 `:nth-of-type(N)` vs `css-all`, explicit `@error:<suffix>`).
- Stop after 5 iterations on the same line and report the minimal conflicting fragment to the user.

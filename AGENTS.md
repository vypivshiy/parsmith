# parsmith Agent Guide

`parsmith` is a multi-target code generator and static linter (written in Rust) for the KDL 2.0 Schema DSL (v2.1) covering HTML scraping, plain-text extraction, and typed REST/JSON HTTP clients.

It is organized as a modular Cargo Workspace:

- `crates/parsmith` — Standalone CLI (`parsmith`) and pure-Rust 5-pass validation and codegen library (`parsmith::kdl` KDL 2.0 document tree, selector engine, and serializer; `SourceLoader` FS + in-memory VFS; explicit imports; `E000`–`E403` / `W011`–`W041` linters; CSS3/4 + XPath 1.0 + CSS3-to-XPath 1.0 lowering + Regex + GJSON validators; `json-to-kdl`; Algebraic AST; 2-pass `CodeWriter` + `LanguageEmitter`; and 9 target backends: Python, TS, JS, Go, Rust, Elixir, PHP, C#, Kotlin).
- `crates/parsmith-py` — `PyO3` / `maturin` Python SDK bindings (`check`, `generate`, `inspect_schema`, `json_to_kdl`, VFS support).
- `crates/parsmith-js` — `napi-rs` Node-API JS/TS SDK bindings (`check`, `generate`, `inspectSchema`, `jsonToKdl`, VFS support, paired with `js/`).
- `crates/parsmith-wasm` — `wasm-bindgen` WebAssembly browser code generator, `json-to-kdl`, and AST selector health extractor (paired with `playground/`).

## Development Workflow

```bash
# 1. Build & test Rust workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2. Build & test Python bindings (via uv + maturin)
uv run maturin develop -m crates/parsmith-py/Cargo.toml
uv run pytest
uv run mypy --strict

# 3. Build & test JS/TS bindings (via N-API + Node.js)
cargo build -p parsmith-js
node --test js/tests/*.test.mjs

# 4. Build & test Web Playground (via wasm-bindgen + Vite/Vitest)
cd playground && npm run build:wasm && npm test -- --run && npm run build
```

## Agent skills

### KDL Schema DSL (`parsmith-dsl`)

Authoring, combining, validating, and compiling `.kdl` (KDL 2.0 Schema DSL v2.1) files across all 9 targets. See `.agents/skills/parsmith-dsl/SKILL.md`.

### Issue tracker

Issues and specs live as local markdown files under `.scratch/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role triage vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout (`CONTEXT.md` and `docs/adr/` at the repo root). See `docs/agents/domain.md`.

//! Modular Rust runtime (`rt`) for `RustEmitter`.
//!
//! Splits runtime definitions into:
//! 1. Core / string / regex / collection / JSON helpers (always emitted).
//! 2. HTML DOM helpers (`dom_query`, emitted when `module_has_html(module)` is `true`).
//! 3. HTTP / REST helpers (`reqwest`, emitted when `module_uses_http(module)` is `true`).

use indexmap::IndexSet;

use crate::ast::Module;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::writer::CodeWriter;
use crate::codegen::{CodegenOptions, module_has_html, module_uses_http};

/// Core error, string, regex, collection, and JSON projection runtime helpers (always included in `rt`).
pub const RUST_CORE_RUNTIME: &str = include_str!("runtime/core.rs.inc");

/// HTML DOM extraction runtime helpers (`dom_query`), emitted when `module_has_html(module)` is `true`.
pub const RUST_HTML_RUNTIME: &str = include_str!("runtime/html.rs.inc");

/// HTTP & REST transport runtime helpers (`reqwest`), emitted when `module_uses_http(module)` is `true`.
pub const RUST_HTTP_RUNTIME: &str = include_str!("runtime/http.rs.inc");

fn ensure_pub_helper(code: &str) -> String {
    let trimmed = code.trim_start();
    if trimmed.starts_with("pub ") {
        code.to_owned()
    } else if trimmed.starts_with("fn ") || trimmed.starts_with("async fn ") {
        let prefix_len = code.len() - trimmed.len();
        format!("{}pub {trimmed}", &code[..prefix_len])
    } else {
        code.to_owned()
    }
}

/// Writes the body of `rt` (used both inside `pub mod rt { ... }` and in `<runtime_name>.rs`) to `writer`.
pub fn write_rust_runtime_body(module: &Module, builder: &ModuleBuilder, writer: &mut CodeWriter) {
    let include_html = module_has_html(module);
    let include_http = module_uses_http(module);

    let mut imports: IndexSet<&str> = IndexSet::new();
    if include_html {
        imports.insert("use dom_query::{Document, NodeId, NodeRef, Selection};");
    }
    imports.insert("use regex::Regex;");
    imports.insert("use serde_json::Value;");
    if include_html {
        imports.insert("use std::cell::RefCell;");
    }
    imports.insert("use std::collections::{HashMap, HashSet};");
    imports.insert("use std::fmt;");
    if include_html {
        imports.insert("use std::rc::Rc;");
    }
    for imp in builder.runtime_imports() {
        imports.insert(imp.as_str());
    }

    for imp in &imports {
        writer.write_block(imp);
    }
    writer.blankline();

    writer.write_block(RUST_CORE_RUNTIME.trim_end());

    if include_html {
        writer.blankline();
        writer.write_block(RUST_HTML_RUNTIME.trim_end());
    }

    if include_http {
        writer.blankline();
        writer.write_block(RUST_HTTP_RUNTIME.trim_end());
    }

    for def in builder.runtime_defs().values() {
        writer.blankline();
        let pub_code = ensure_pub_helper(&def.code);
        writer.write_block(&pub_code);
    }
}

/// Generates the standalone Rust shared runtime module (`<runtime_name>.rs`) for `-R / --separate-runtime`.
pub fn emit_rust_runtime_module(
    module: &Module,
    builder: &ModuleBuilder,
    _options: &CodegenOptions,
) -> String {
    let mut writer = CodeWriter::with_indent("    ");
    writer.writeln("// Auto-generated runtime helpers by parsmith. DO NOT EDIT.");
    writer.writeln("#![allow(dead_code, unused_imports)]");
    writer.blankline();
    write_rust_runtime_body(module, builder, &mut writer);
    writer.finish()
}

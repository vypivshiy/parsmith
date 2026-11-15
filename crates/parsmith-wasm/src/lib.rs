//! WebAssembly (`wasm-bindgen`) browser bindings and AST selector health extractor for `parsmith`.

pub mod node_sdk;

use std::str::FromStr;

pub use node_sdk::*;

use indexmap::{IndexMap, IndexSet};
use parsmith::kdl::{KdlDocument, LineIndex};
use parsmith::{
    Bs4Parser, CodegenOptions, Cond, CondOp, Diagnostic, HttpIo, LiteralValue, Module, PhpHtmlLib,
    Pipeline, PythonHtmlLib, PythonModels, Severity, StepOp, StructDecl, StructKind, Target,
    check_source, compile_source, generate_module, generate_source, parse_target_with_libs,
    to_pascal_case, validate_csharp_namespace, validate_elixir_package_name,
    validate_go_package_name, validate_kotlin_package_name, validate_php_namespace,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// 1-based line/column position with 0-based byte offset for Monaco editor markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticPosition {
    /// 0-based byte offset in the KDL source text.
    pub offset: usize,
    /// 1-based line number (`>= 1`).
    pub line: usize,
    /// 1-based column number (`>= 1`).
    pub column: usize,
}

/// Source span with 1-based `start` and `end` positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSpan {
    /// Start position of the diagnostic span.
    pub start: DiagnosticPosition,
    /// End position of the diagnostic span.
    pub end: DiagnosticPosition,
}

/// Playground wire format for a compiler or linter diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SscDiagnostic {
    /// Diagnostic code (`E000`–`E403`, `W011`–`W041`).
    pub code: String,
    /// Severity string (`"error"` or `"warning"`).
    pub severity: String,
    /// Human-readable message.
    pub message: String,
    /// Actionable hint (empty string when none).
    pub hint: String,
    /// Originating logical file path (defaults to `"schema.kdl"`).
    pub path: String,
    /// Optional inline span label.
    pub label: Option<String>,
    /// Additional context notes.
    pub notes: Vec<String>,
    /// 1-based start/end span with byte offsets.
    pub span: DiagnosticSpan,
}

/// Struct metadata entry returned by `compile_and_lint` for playground struct pills and dropdowns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaygroundStructInfo {
    /// Declared KDL struct name.
    pub name: String,
    /// Generated `PascalCase` class name.
    pub class_name: String,
    /// Struct strategy kind (`"item"`, `"list"`, `"table"`, `"dict"`, `"flat"`, `"raw"`, `"rest"`).
    #[serde(rename = "type")]
    pub struct_type: String,
    /// Whether this struct is a `(rest)struct`.
    pub is_rest: bool,
    /// Names of parent structs in the module that reference this struct via `nested`.
    pub referenced_by: Vec<String>,
}

/// JSON result payload returned by [`compile_and_lint`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileAndLintResult {
    /// `true` when validation and code generation succeeded without errors.
    pub ok: bool,
    /// Compiler, linter, and codegen diagnostics.
    pub diagnostics: Vec<SscDiagnostic>,
    /// Declared structs in the schema (from AST or fallback CST scan).
    pub structs: Vec<PlaygroundStructInfo>,
    /// Emitted target source files keyed by filename (`parser.py`, `parsmith_runtime.py`, etc.).
    pub files: IndexMap<String, String>,
    /// Target option validation error or codegen error message, if any.
    pub target_error: Option<String>,
}

/// Static selector check descriptor extracted from a compiled [`StructDecl`] AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorCheckSpec {
    /// Dot-separated AST path (e.g. `"Book.title"`, `"Book.@split-doc"`, `"Catalogue.items[Item].name"`).
    pub path: String,
    /// Selector operation kind (`"css"`, `"css-all"`, `"css-remove"`, `"xpath"`, `"xpath-all"`, `"xpath-remove"`).
    pub selector_type: String,
    /// Raw selector queries in fallback evaluation order.
    pub queries: Vec<String>,
    /// Display query string (joined with `" || "` for multi-selector fallback blocks).
    pub report_query: String,
    /// Formatted representation of the enclosing pipeline's terminal `fallback` literal, if present.
    pub fallback_value: Option<String>,
}

/// JSON result payload returned by [`prepare_test_html`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareTestHtmlResult {
    /// `true` when the schema compiled cleanly to JavaScript and the target struct is testable.
    pub ok: bool,
    /// Error message when `ok == false`.
    pub error: Option<String>,
    /// Resolved target struct name.
    pub struct_name: Option<String>,
    /// Generated `PascalCase` JavaScript parser class name.
    pub class_name: Option<String>,
    /// Struct kind (`"item"`, `"list"`, `"table"`, `"dict"`, `"flat"`, `"raw"`, `"rest"`).
    pub struct_kind: Option<String>,
    /// Generated standalone ESM JavaScript parser module (`parser.js`).
    pub js_code: Option<String>,
    /// Extracted CSS and XPath selector descriptors in deterministic AST order.
    pub selectors: Vec<SelectorCheckSpec>,
}

/// JSON result payload returned by [`json_to_kdl`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonToKdlResult {
    /// `true` when the JSON sample was converted into KDL 2.0 `json` blocks.
    pub ok: bool,
    /// Generated KDL 2.0 schema text.
    pub kdl: String,
    /// Error message when `ok == false`.
    pub error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TargetSpecPayload {
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    lib: Option<String>,
    #[serde(default)]
    html_lib: Option<String>,
    #[serde(default)]
    php_html_lib: Option<String>,
    #[serde(default)]
    models: Option<String>,
    #[serde(default)]
    bs4_parser: Option<String>,
    #[serde(default)]
    http_io: Option<String>,
    #[serde(default)]
    separate_runtime: Option<bool>,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    go_package: Option<String>,
}

/// Computes a 0-based byte offset from 1-based `(line, col)` in `src` (returning `0` if out of range).
fn line_col_to_offset(src: &str, line: usize, col: usize) -> usize {
    if src.is_empty() || line == 0 || col == 0 {
        return 0;
    }
    let line_index = LineIndex::new(src);
    let line_starts = line_index.line_starts();
    let Some(&line_start) = line_starts.get(line - 1) else {
        return 0;
    };
    let line_end = line_starts.get(line).copied().unwrap_or(src.len());
    let Some(line_slice) = src.get(line_start..line_end) else {
        return 0;
    };
    let char_offset: usize = line_slice
        .chars()
        .take(col.saturating_sub(1))
        .map(char::len_utf8)
        .sum();
    (line_start + char_offset).min(src.len())
}

/// Converts a [`parsmith::Diagnostic`] into the playground [`SscDiagnostic`] wire format
/// with guaranteed 1-based line/column coordinates and byte offsets computed from `kdl_src`.
fn to_ssc_diagnostic(diag: &Diagnostic, kdl_src: &str) -> SscDiagnostic {
    let start_line = diag.line.max(1);
    let start_col = diag.col.max(1);
    let end_line = diag.end_line.max(start_line);
    let end_col = diag.end_col.max(1);

    let start_offset = line_col_to_offset(kdl_src, start_line, start_col);
    let end_offset = line_col_to_offset(kdl_src, end_line, end_col).max(start_offset);

    let path = if diag.path.is_empty() {
        "schema.kdl".to_owned()
    } else {
        diag.path.clone()
    };

    SscDiagnostic {
        code: diag.code.as_str().to_owned(),
        severity: diag.severity.as_str().to_owned(),
        message: diag.message.clone(),
        hint: diag.hint.clone().unwrap_or_default(),
        path,
        label: diag.label.clone(),
        notes: diag.notes.clone(),
        span: DiagnosticSpan {
            start: DiagnosticPosition {
                offset: start_offset,
                line: start_line,
                column: start_col,
            },
            end: DiagnosticPosition {
                offset: end_offset,
                line: end_line,
                column: end_col,
            },
        },
    }
}

fn record_pipeline_nested_refs(
    parent_name: &str,
    pipeline: &Pipeline,
    ref_map: &mut IndexMap<String, Vec<String>>,
) {
    for step in &pipeline.steps {
        if let StepOp::Nested { ref struct_name } = step.op {
            let parents = ref_map.entry(struct_name.clone()).or_default();
            if !parents.iter().any(|p| p == parent_name) {
                parents.push(parent_name.to_owned());
            }
        }
    }
}

/// Extracts [`PlaygroundStructInfo`] descriptors (including `referenced_by`) from a compiled [`Module`].
fn collect_module_structs(module: &Module) -> Vec<PlaygroundStructInfo> {
    let mut ref_map: IndexMap<String, Vec<String>> = IndexMap::new();

    for s in &module.structs {
        if let Some(ref p) = s.split_doc {
            record_pipeline_nested_refs(&s.name, p, &mut ref_map);
        }
        if let Some(ref p) = s.pre_validate {
            record_pipeline_nested_refs(&s.name, p, &mut ref_map);
        }
        for init in &s.init_fields {
            record_pipeline_nested_refs(&s.name, &init.pipeline, &mut ref_map);
        }
        for check in &s.checks {
            record_pipeline_nested_refs(&s.name, &check.pipeline, &mut ref_map);
        }
        if let Some(ref p) = s.key_pipeline {
            record_pipeline_nested_refs(&s.name, p, &mut ref_map);
        }
        if let Some(ref p) = s.value_pipeline {
            record_pipeline_nested_refs(&s.name, p, &mut ref_map);
        }
        if let Some(ref p) = s.table_pipeline {
            record_pipeline_nested_refs(&s.name, p, &mut ref_map);
        }
        if let Some(ref p) = s.rows_pipeline {
            record_pipeline_nested_refs(&s.name, p, &mut ref_map);
        }
        if let Some(ref p) = s.match_pipeline {
            record_pipeline_nested_refs(&s.name, p, &mut ref_map);
        }
        for field in &s.fields {
            record_pipeline_nested_refs(&s.name, &field.pipeline, &mut ref_map);
        }
    }

    module
        .structs
        .iter()
        .map(|s| PlaygroundStructInfo {
            name: s.name.clone(),
            class_name: to_pascal_case(&s.name),
            struct_type: s.kind.as_str().to_owned(),
            is_rest: s.kind == StructKind::Rest,
            referenced_by: ref_map.get(&s.name).cloned().unwrap_or_default(),
        })
        .collect()
}

/// Scans top-level `struct` nodes from a KDL CST document as a fallback when compilation errors
/// prevent AST lowering.
fn extract_cst_structs(kdl_src: &str) -> Vec<PlaygroundStructInfo> {
    let Ok(doc) = KdlDocument::parse(kdl_src) else {
        return Vec::new();
    };

    let mut ref_map: IndexMap<String, Vec<String>> = IndexMap::new();
    for node in doc.roots() {
        if node.name() != "struct" {
            continue;
        }
        let Some(parent_name) = node.arg_value(0).and_then(|v| v.as_str()) else {
            continue;
        };
        if parent_name.is_empty() {
            continue;
        }
        for desc in node.iter_descendants() {
            if desc.name() == "nested"
                && let Some(target_name) = desc.arg_value(0).and_then(|v| v.as_str())
                && !target_name.is_empty()
            {
                let parents = ref_map.entry(target_name.to_owned()).or_default();
                if !parents.iter().any(|p| p == parent_name) {
                    parents.push(parent_name.to_owned());
                }
            }
        }
    }

    let mut seen = IndexSet::new();
    let mut structs = Vec::new();
    for node in doc.roots() {
        if node.name() != "struct" {
            continue;
        }
        let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) else {
            continue;
        };
        if name.is_empty() || !seen.insert(name.to_owned()) {
            continue;
        }
        let raw_kind = node
            .ty()
            .or_else(|| node.get("type").and_then(|v| v.as_str()))
            .unwrap_or("item");
        let kind = StructKind::from_str(raw_kind).unwrap_or(StructKind::Item);
        structs.push(PlaygroundStructInfo {
            name: name.to_owned(),
            class_name: to_pascal_case(name),
            struct_type: kind.as_str().to_owned(),
            is_rest: kind == StructKind::Rest,
            referenced_by: ref_map.get(name).cloned().unwrap_or_default(),
        });
    }

    structs
}

fn apply_parsed_opt<T, F>(
    raw: Option<&str>,
    parse: F,
    target_error: &mut Option<String>,
    apply: impl FnOnce(T),
) where
    F: FnOnce(&str) -> Result<T, String>,
{
    if let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) {
        match parse(trimmed) {
            Ok(val) => apply(val),
            Err(err) if target_error.is_none() => *target_error = Some(err),
            Err(_) => {}
        }
    }
}

/// Parses the playground `target_json` payload into [`CodegenOptions`] and an optional target error message.
fn parse_target_options(target_json: &str) -> (CodegenOptions, Option<String>) {
    let mut opts = CodegenOptions {
        module_stem: Some("parser".to_owned()),
        ..CodegenOptions::default()
    };

    if target_json.trim().is_empty() {
        return (opts, None);
    }

    let payload: TargetSpecPayload = match serde_json::from_str(target_json) {
        Ok(p) => p,
        Err(err) => {
            return (opts, Some(format!("Invalid target JSON: {err}")));
        }
    };

    let mut target_error: Option<String> = None;

    apply_parsed_opt(
        payload.lang.as_deref().or(payload.target.as_deref()),
        parse_target_with_libs,
        &mut target_error,
        |(target, shorthand_py_lib, shorthand_php_lib)| {
            opts.target = target;
            if let Some(lib) = shorthand_py_lib {
                opts.html_lib = lib;
            }
            if let Some(php_lib) = shorthand_php_lib {
                opts.php_html_lib = php_lib;
            }
        },
    );

    apply_parsed_opt(
        payload.php_html_lib.as_deref(),
        PhpHtmlLib::from_str,
        &mut target_error,
        |php_lib| opts.php_html_lib = php_lib,
    );

    if opts.target == Target::Php {
        if payload.php_html_lib.is_none() {
            apply_parsed_opt(
                payload.lib.as_deref().or(payload.html_lib.as_deref()),
                PhpHtmlLib::from_str,
                &mut target_error,
                |php_lib| opts.php_html_lib = php_lib,
            );
        }
    } else {
        apply_parsed_opt(
            payload.lib.as_deref().or(payload.html_lib.as_deref()),
            PythonHtmlLib::from_str,
            &mut target_error,
            |lib| opts.html_lib = lib,
        );
    }

    apply_parsed_opt(
        payload.models.as_deref(),
        PythonModels::from_str,
        &mut target_error,
        |models| opts.models = models,
    );

    apply_parsed_opt(
        payload.bs4_parser.as_deref(),
        Bs4Parser::from_str,
        &mut target_error,
        |parser| opts.bs4_parser = parser,
    );

    apply_parsed_opt(
        payload.http_io.as_deref(),
        HttpIo::from_str,
        &mut target_error,
        |http_io| opts.http_io = http_io,
    );

    if let Some(sep) = payload.separate_runtime {
        opts.separate_runtime = sep;
    }

    if let Some(pkg) = payload.package.or(payload.namespace).or(payload.go_package) {
        let trimmed = pkg.trim();
        if !trimmed.is_empty() {
            opts.package = trimmed.to_owned();
        } else if opts.target == Target::Go {
            opts.package = String::new();
        } else {
            opts.package = "main".to_owned();
        }
    }

    match opts.target {
        Target::Go if !validate_go_package_name(&opts.package) && target_error.is_none() => {
            target_error = Some(format!(
                "Invalid Go package name '{}'. Use a valid non-keyword Go package identifier (e.g. main, parsers)",
                opts.package
            ));
        }
        Target::Elixir
            if !validate_elixir_package_name(&opts.package) && target_error.is_none() =>
        {
            target_error = Some(format!(
                "Invalid Elixir module prefix '{}'. Use valid dot-separated Elixir module segments (e.g. MyApp.Parsers)",
                opts.package
            ));
        }
        Target::Php if !validate_php_namespace(&opts.package) && target_error.is_none() => {
            target_error = Some(format!(
                "Invalid PHP namespace '{}'. Use a valid PHP namespace (e.g. App\\Parsers or App.Parsers)",
                opts.package
            ));
        }
        Target::CSharp if !validate_csharp_namespace(&opts.package) && target_error.is_none() => {
            target_error = Some(format!(
                "Invalid C# namespace '{}'. Use valid dot-separated C# identifiers (e.g. MyApp.Parsers)",
                opts.package
            ));
        }
        Target::Kotlin
            if !validate_kotlin_package_name(&opts.package) && target_error.is_none() =>
        {
            target_error = Some(format!(
                "Invalid Kotlin package name '{}'. Use a valid dot-separated Kotlin package name (e.g. com.example.parsers)",
                opts.package
            ));
        }
        _ => {}
    }

    (opts, target_error)
}

/// Validates and compiles `kdl_src` for the target configuration in `target_json`, returning a
/// structured [`CompileAndLintResult`].
pub fn compile_and_lint_result(kdl_src: &str, target_json: &str) -> CompileAndLintResult {
    let (opts, target_error) = parse_target_options(target_json);

    let check_res = check_source(kdl_src, Some("schema.kdl"));
    let mut diagnostics: Vec<SscDiagnostic> = check_res
        .diagnostics
        .iter()
        .map(|d| to_ssc_diagnostic(d, kdl_src))
        .collect();

    let structs = match check_res.module {
        Some(ref module) if !module.structs.is_empty() => collect_module_structs(module),
        _ => extract_cst_structs(kdl_src),
    };

    if target_error.is_some() || check_res.has_errors() {
        return CompileAndLintResult {
            ok: false,
            diagnostics,
            structs,
            files: IndexMap::new(),
            target_error,
        };
    }

    let gen_res = match check_res.module {
        Some(ref module) => generate_module(module, &opts),
        None => generate_source(kdl_src, Some("schema.kdl"), &opts),
    };

    if gen_res.has_errors() {
        let first_err = gen_res
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
            .map(|d| d.message.clone());
        for d in &gen_res.diagnostics {
            diagnostics.push(to_ssc_diagnostic(d, kdl_src));
        }
        return CompileAndLintResult {
            ok: false,
            diagnostics,
            structs,
            files: IndexMap::new(),
            target_error: first_err,
        };
    }

    for d in &gen_res.diagnostics {
        diagnostics.push(to_ssc_diagnostic(d, kdl_src));
    }

    CompileAndLintResult {
        ok: true,
        diagnostics,
        structs,
        files: gen_res.files,
        target_error: None,
    }
}

/// Formats a terminal [`LiteralValue`] fallback into the representation used in `Selector Health` reports.
fn format_literal_fallback(lit: &LiteralValue) -> String {
    match lit {
        LiteralValue::Null => "#null".to_owned(),
        LiteralValue::Bool(b) => if *b { "#true" } else { "#false" }.to_owned(),
        LiteralValue::Int(i) => i.to_string(),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => format!("{s:?}"),
        LiteralValue::EmptyArray => "[]".to_owned(),
    }
}

fn collect_cond_selectors(
    cond: &Cond,
    current_path: &str,
    fallback_value: Option<&str>,
    out: &mut Vec<SelectorCheckSpec>,
) {
    match cond {
        Cond::And(children) | Cond::Or(children) => {
            for child in children {
                collect_cond_selectors(child, current_path, fallback_value, out);
            }
        }
        Cond::Not(inner) => {
            collect_cond_selectors(inner, current_path, fallback_value, out);
        }
        Cond::Atom(CondOp::Css { query }) => {
            out.push(SelectorCheckSpec {
                path: current_path.to_owned(),
                selector_type: "css".to_owned(),
                queries: vec![query.clone()],
                report_query: query.clone(),
                fallback_value: fallback_value.map(str::to_owned),
            });
        }
        Cond::Atom(CondOp::Xpath { query }) => {
            out.push(SelectorCheckSpec {
                path: current_path.to_owned(),
                selector_type: "xpath".to_owned(),
                queries: vec![query.clone()],
                report_query: query.clone(),
                fallback_value: fallback_value.map(str::to_owned),
            });
        }
        Cond::Atom(_) => {}
    }
}

fn collect_pipeline_selectors(
    pipeline: &Pipeline,
    current_path: &str,
    out: &mut Vec<SelectorCheckSpec>,
    nested_refs: &mut Vec<(String, String)>,
) {
    let fallback_value = pipeline.fallback.as_ref().map(format_literal_fallback);
    for step in &pipeline.steps {
        match &step.op {
            StepOp::Css { queries } => {
                out.push(SelectorCheckSpec {
                    path: current_path.to_owned(),
                    selector_type: "css".to_owned(),
                    queries: queries.clone(),
                    report_query: queries.join(" || "),
                    fallback_value: fallback_value.clone(),
                });
            }
            StepOp::CssAll { queries } => {
                out.push(SelectorCheckSpec {
                    path: current_path.to_owned(),
                    selector_type: "css-all".to_owned(),
                    queries: queries.clone(),
                    report_query: queries.join(" || "),
                    fallback_value: fallback_value.clone(),
                });
            }
            StepOp::CssRemove { query } => {
                out.push(SelectorCheckSpec {
                    path: current_path.to_owned(),
                    selector_type: "css-remove".to_owned(),
                    queries: vec![query.clone()],
                    report_query: query.clone(),
                    fallback_value: fallback_value.clone(),
                });
            }
            StepOp::Xpath { queries } => {
                out.push(SelectorCheckSpec {
                    path: current_path.to_owned(),
                    selector_type: "xpath".to_owned(),
                    queries: queries.clone(),
                    report_query: queries.join(" || "),
                    fallback_value: fallback_value.clone(),
                });
            }
            StepOp::XpathAll { queries } => {
                out.push(SelectorCheckSpec {
                    path: current_path.to_owned(),
                    selector_type: "xpath-all".to_owned(),
                    queries: queries.clone(),
                    report_query: queries.join(" || "),
                    fallback_value: fallback_value.clone(),
                });
            }
            StepOp::XpathRemove { query } => {
                out.push(SelectorCheckSpec {
                    path: current_path.to_owned(),
                    selector_type: "xpath-remove".to_owned(),
                    queries: vec![query.clone()],
                    report_query: query.clone(),
                    fallback_value: fallback_value.clone(),
                });
            }
            StepOp::Filter { cond } | StepOp::Assert { cond, .. } | StepOp::Match { cond } => {
                collect_cond_selectors(cond, current_path, fallback_value.as_deref(), out);
            }
            StepOp::Nested { struct_name } => {
                nested_refs.push((current_path.to_owned(), struct_name.clone()));
            }
            _ => {}
        }
    }
}

fn collect_struct_selectors_recursive(
    module: &Module,
    struct_decl: &StructDecl,
    prefix: &str,
    visited: &mut IndexSet<String>,
    out: &mut Vec<SelectorCheckSpec>,
) {
    if !visited.insert(struct_decl.name.clone()) {
        return;
    }

    let mut nested_refs: Vec<(String, String)> = Vec::new();

    if let Some(ref p) = struct_decl.split_doc {
        collect_pipeline_selectors(p, &format!("{prefix}.@split-doc"), out, &mut nested_refs);
    }

    if let Some(ref p) = struct_decl.pre_validate {
        collect_pipeline_selectors(p, &format!("{prefix}.@pre-validate"), out, &mut nested_refs);
    }

    for init in &struct_decl.init_fields {
        collect_pipeline_selectors(
            &init.pipeline,
            &format!("{prefix}.@init.{}", init.name),
            out,
            &mut nested_refs,
        );
    }

    for check in &struct_decl.checks {
        collect_pipeline_selectors(
            &check.pipeline,
            &format!("{prefix}.@check:{}", check.name),
            out,
            &mut nested_refs,
        );
    }

    if let Some(ref p) = struct_decl.key_pipeline {
        collect_pipeline_selectors(p, &format!("{prefix}.@key"), out, &mut nested_refs);
    }

    if let Some(ref p) = struct_decl.value_pipeline {
        collect_pipeline_selectors(p, &format!("{prefix}.@value"), out, &mut nested_refs);
    }

    if let Some(ref p) = struct_decl.table_pipeline {
        collect_pipeline_selectors(p, &format!("{prefix}.@table"), out, &mut nested_refs);
    }

    if let Some(ref p) = struct_decl.rows_pipeline {
        collect_pipeline_selectors(p, &format!("{prefix}.@rows"), out, &mut nested_refs);
    }

    if let Some(ref p) = struct_decl.match_pipeline {
        collect_pipeline_selectors(p, &format!("{prefix}.@match"), out, &mut nested_refs);
    }

    for field in &struct_decl.fields {
        collect_pipeline_selectors(
            &field.pipeline,
            &format!("{prefix}.{}", field.name),
            out,
            &mut nested_refs,
        );
    }

    for (ref_path, nested_name) in nested_refs {
        if let Some(nested_struct) = module.struct_decl(&nested_name) {
            let nested_prefix = format!("{ref_path}[{nested_name}]");
            collect_struct_selectors_recursive(module, nested_struct, &nested_prefix, visited, out);
        }
    }
}

/// Compiles `kdl_src` to standalone JavaScript (`Target::JavaScript`) and extracts AST selector
/// health descriptors for `struct_name` (or the default primary non-REST struct).
pub fn prepare_test_html_result(kdl_src: &str, struct_name: Option<&str>) -> PrepareTestHtmlResult {
    let requested_name = struct_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let module = match compile_source(kdl_src, Some("schema.kdl")) {
        Ok(m) => m,
        Err(check_res) => {
            let joined = check_res
                .errors()
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            return PrepareTestHtmlResult {
                ok: false,
                error: Some(joined),
                struct_name: requested_name,
                class_name: None,
                struct_kind: None,
                js_code: None,
                selectors: Vec::new(),
            };
        }
    };

    if module.structs.is_empty() {
        return PrepareTestHtmlResult {
            ok: false,
            error: Some("No structs declared in schema.".to_owned()),
            struct_name: requested_name,
            class_name: None,
            struct_kind: None,
            js_code: None,
            selectors: Vec::new(),
        };
    }

    let struct_infos = collect_module_structs(&module);

    let target_struct: &StructDecl = if let Some(ref req_name) = requested_name {
        match module.struct_decl(req_name) {
            Some(s) => s,
            None => {
                let available = module
                    .structs
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return PrepareTestHtmlResult {
                    ok: false,
                    error: Some(format!(
                        "Struct '{req_name}' not found in schema. Available: {available}"
                    )),
                    struct_name: Some(req_name.clone()),
                    class_name: None,
                    struct_kind: None,
                    js_code: None,
                    selectors: Vec::new(),
                };
            }
        }
    } else {
        let root_non_rest_name = struct_infos
            .iter()
            .rev()
            .find(|info| !info.is_rest && info.referenced_by.is_empty())
            .map(|info| info.name.as_str());

        if let Some(name) = root_non_rest_name
            && let Some(s) = module.struct_decl(name)
        {
            s
        } else if let Some(s) = module
            .structs
            .iter()
            .rev()
            .find(|s| s.kind != StructKind::Rest)
        {
            s
        } else {
            module.structs.last().unwrap()
        }
    };

    let class_name = to_pascal_case(&target_struct.name);
    let struct_kind = target_struct.kind.as_str().to_owned();

    if target_struct.kind == StructKind::Rest {
        return PrepareTestHtmlResult {
            ok: false,
            error: Some(format!(
                "Struct '{}' is a (rest)struct. REST schemas support linting and code generation only; HTML parser execution and selector health checks are not supported.",
                target_struct.name
            )),
            struct_name: Some(target_struct.name.clone()),
            class_name: Some(class_name),
            struct_kind: Some(struct_kind),
            js_code: None,
            selectors: Vec::new(),
        };
    }

    let mut visited = IndexSet::new();
    let mut selectors = Vec::new();
    collect_struct_selectors_recursive(
        &module,
        target_struct,
        &target_struct.name,
        &mut visited,
        &mut selectors,
    );

    let js_opts = CodegenOptions {
        target: Target::JavaScript,
        module_stem: Some("parser".to_owned()),
        ..CodegenOptions::default()
    };
    let gen_res = generate_module(&module, &js_opts);
    if gen_res.has_errors() {
        let err_msg = gen_res
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
            .map_or_else(
                || "Failed to generate JavaScript parser.".to_owned(),
                |d| d.message.clone(),
            );
        return PrepareTestHtmlResult {
            ok: false,
            error: Some(err_msg),
            struct_name: Some(target_struct.name.clone()),
            class_name: Some(class_name),
            struct_kind: Some(struct_kind),
            js_code: None,
            selectors,
        };
    }

    let Some(js_code) = gen_res.files.get("parser.js").cloned() else {
        return PrepareTestHtmlResult {
            ok: false,
            error: Some("Generated 'parser.js' not found.".to_owned()),
            struct_name: Some(target_struct.name.clone()),
            class_name: Some(class_name),
            struct_kind: Some(struct_kind),
            js_code: None,
            selectors,
        };
    };

    PrepareTestHtmlResult {
        ok: true,
        error: None,
        struct_name: Some(target_struct.name.clone()),
        class_name: Some(class_name),
        struct_kind: Some(struct_kind),
        js_code: Some(js_code),
        selectors,
    }
}

/// Returns the `parsmith` compiler version string.
#[wasm_bindgen]
pub fn version() -> String {
    parsmith::version().to_owned()
}

/// Validates and compiles a KDL 2.0 schema for the target configuration in `target_json`,
/// returning a JSON-serialized [`CompileAndLintResult`].
#[wasm_bindgen]
pub fn compile_and_lint(kdl_src: &str, target_json: &str) -> String {
    let res = compile_and_lint_result(kdl_src, target_json);
    serde_json::to_string(&res).unwrap_or_else(|e| {
        format!(
            r#"{{"ok":false,"diagnostics":[],"structs":[],"files":{{}},"target_error":{:?}}}"#,
            e.to_string()
        )
    })
}

/// Compiles `kdl_src` to standalone JavaScript (`parser.js`) and extracts AST selector check
/// descriptors for `struct_name`, returning a JSON-serialized [`PrepareTestHtmlResult`].
#[wasm_bindgen]
pub fn prepare_test_html(kdl_src: &str, struct_name: Option<String>) -> String {
    let res = prepare_test_html_result(kdl_src, struct_name.as_deref());
    serde_json::to_string(&res).unwrap_or_else(|e| {
        format!(
            r#"{{"ok":false,"error":{:?},"struct_name":null,"class_name":null,"struct_kind":null,"js_code":null,"selectors":[]}}"#,
            e.to_string()
        )
    })
}

/// Infers a KDL 2.0 `json` schema from `json_src` using `schema_name` (defaulting to `"JsonResponse"`),
/// returning a JSON-serialized [`JsonToKdlResult`].
#[wasm_bindgen]
pub fn json_to_kdl(json_src: &str, schema_name: Option<String>) -> String {
    let name = schema_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("JsonResponse");

    let res = match parsmith::json_to_kdl(json_src, name) {
        Ok(kdl) => JsonToKdlResult {
            ok: true,
            kdl,
            error: None,
        },
        Err(err) => JsonToKdlResult {
            ok: false,
            kdl: String::new(),
            error: Some(err.to_string()),
        },
    };
    serde_json::to_string(&res)
        .unwrap_or_else(|e| format!(r#"{{"ok":false,"kdl":"","error":{:?}}}"#, e.to_string()))
}

//! High-level schema validation (`check`) and compilation (`compile`) entrypoints.
#![allow(clippy::result_large_err)]

use std::path::Path;

use crate::ast::Module;
use crate::diagnostic::{CheckResult, Severity};
use crate::imports::{resolve_source_with_loader, resolve_with_loader};
use crate::linter::lint_program;
use crate::linter::symbols::lint_pass4_symbols;
use crate::loader::{FsSourceLoader, SourceLoader};
use crate::lowering::lower_program;

/// Options controlling schema validation (`parsmith check`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Optional target ecosystem filter (`"python"`, `"javascript"`, `"typescript"`, `"go"`, `"rust"`).
    ///
    /// When `None` or empty, all four ecosystems are validated by default.
    pub targets: Option<Vec<String>>,
}

impl CheckOptions {
    /// Creates default `CheckOptions` validating all 4 target ecosystems.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the target ecosystem filter.
    pub fn with_targets<I, S>(mut self, targets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let vec: Vec<String> = targets.into_iter().map(Into::into).collect();
        if !vec.is_empty() {
            self.targets = Some(vec);
        }
        self
    }
}

/// Validates a KDL 2.0 schema file on disk (and all its transitive `import` dependencies)
/// across all 4 target ecosystems.
pub fn check_path(path: impl AsRef<Path>) -> CheckResult {
    let loader = FsSourceLoader::new();
    let entry_path = path.as_ref().to_string_lossy();
    check_with_loader(&entry_path, &loader)
}

/// Validates a KDL 2.0 schema file on disk with an optional target ecosystem filter.
pub fn check_path_with_targets<P, I, S>(path: P, targets: Option<I>) -> CheckResult
where
    P: AsRef<Path>,
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let loader = FsSourceLoader::new();
    let entry_path = path.as_ref().to_string_lossy();
    check_with_loader_and_targets(&entry_path, &loader, targets)
}

/// Validates an in-memory KDL 2.0 source string across all 4 target ecosystems.
///
/// If `path` is provided, relative `import` statements are resolved relative to `path`'s parent
/// directory via [`FsSourceLoader`]. If `path` is `None`, any `import` statement reports `E003`.
pub fn check_source(source: &str, path: Option<&str>) -> CheckResult {
    let loader = FsSourceLoader::new();
    check_source_with_loader(source, path, &loader)
}

/// Validates an in-memory KDL 2.0 source string with an optional target ecosystem filter.
pub fn check_source_with_targets<I, S>(
    source: &str,
    path: Option<&str>,
    targets: Option<I>,
) -> CheckResult
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let loader = FsSourceLoader::new();
    check_source_with_loader_and_targets(source, path, &loader, targets)
}

/// Validates a KDL 2.0 schema starting from `entry_path` using a custom [`SourceLoader`]
/// (such as [`FsSourceLoader`] or [`crate::loader::MemorySourceLoader`]).
pub fn check_with_loader(entry_path: &str, loader: &dyn SourceLoader) -> CheckResult {
    check_with_options(entry_path, loader, &CheckOptions::default())
}

/// Validates a KDL 2.0 schema starting from `entry_path` using `loader` and an optional target filter.
pub fn check_with_loader_and_targets<I, S>(
    entry_path: &str,
    loader: &dyn SourceLoader,
    targets: Option<I>,
) -> CheckResult
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let opts = match targets {
        Some(iter) => CheckOptions::new().with_targets(iter),
        None => CheckOptions::default(),
    };
    check_with_options(entry_path, loader, &opts)
}

/// Validates an in-memory KDL 2.0 `source` string at logical `path`, resolving any relative
/// `import` blocks through `loader`.
pub fn check_source_with_loader(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
) -> CheckResult {
    check_source_with_options(source, path, loader, &CheckOptions::default())
}

/// Validates an in-memory KDL 2.0 `source` string at logical `path` with `loader` and an optional target filter.
pub fn check_source_with_loader_and_targets<I, S>(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
    targets: Option<I>,
) -> CheckResult
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let opts = match targets {
        Some(iter) => CheckOptions::new().with_targets(iter),
        None => CheckOptions::default(),
    };
    check_source_with_options(source, path, loader, &opts)
}

/// Runs all 5 compiler passes starting from `entry_path` using `loader` and `options`.
pub fn check_with_options(
    entry_path: &str,
    loader: &dyn SourceLoader,
    options: &CheckOptions,
) -> CheckResult {
    let (mut program, sources, mut diagnostics) = resolve_with_loader(entry_path, loader);
    let mut compiled_module = None;
    if let Some(ref mut prog) = program {
        compiled_module = run_passes_3_to_5(prog, options, &mut diagnostics);
    }
    CheckResult::new(diagnostics)
        .with_sources(sources)
        .with_program(program)
        .with_module(compiled_module)
}

/// Runs all 5 compiler passes starting from an in-memory `source` string at logical `path`
/// using `loader` and `options`.
pub fn check_source_with_options(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
    options: &CheckOptions,
) -> CheckResult {
    let (mut program, sources, mut diagnostics) = resolve_source_with_loader(source, path, loader);
    let mut compiled_module = None;
    if let Some(ref mut prog) = program {
        compiled_module = run_passes_3_to_5(prog, options, &mut diagnostics);
    }
    CheckResult::new(diagnostics)
        .with_sources(sources)
        .with_program(program)
        .with_module(compiled_module)
}

fn run_passes_3_to_5(
    prog: &mut crate::imports::ResolvedProgram,
    options: &CheckOptions,
    diagnostics: &mut Vec<crate::diagnostic::Diagnostic>,
) -> Option<Module> {
    // Pass 3: Structural, selector, @request, @error, GJSON, and intra-file top-down order lint
    diagnostics.extend(lint_program(prog));

    let has_pass1_to_3_errors = diagnostics.iter().any(|d| d.severity == Severity::Error);

    // Pass 4: Cross-reference, cycle detection, and 4-ecosystem symbol lint
    let pass4_diags = lint_pass4_symbols(prog, options.targets.as_deref(), diagnostics);
    let has_undef_or_cycle_in_pass4 = pass4_diags
        .iter()
        .any(|d| d.code == crate::diagnostic::DiagnosticCode::E300);
    diagnostics.extend(pass4_diags);

    // Halt before Pass 5 AST lowering when fatal structural/reference errors exist
    if has_pass1_to_3_errors || has_undef_or_cycle_in_pass4 {
        return None;
    }

    // Pass 5: Post-order inline JSON hoisting, extension generic binding, OpSig type checking, and AST lowering
    let module = lower_program(prog, diagnostics);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        None
    } else {
        Some(module)
    }
}

/// Compiles a KDL 2.0 schema file on disk into an immutable [`Module`] AST, returning
/// `Err(CheckResult)` if any compilation errors occurred.
pub fn compile_path(path: impl AsRef<Path>) -> Result<Module, CheckResult> {
    let loader = FsSourceLoader::new();
    let entry_path = path.as_ref().to_string_lossy();
    compile_with_loader(&entry_path, &loader)
}

/// Compiles an in-memory KDL 2.0 source string into an immutable [`Module`] AST, returning
/// `Err(CheckResult)` if any compilation errors occurred.
pub fn compile_source(source: &str, path: Option<&str>) -> Result<Module, CheckResult> {
    let loader = FsSourceLoader::new();
    compile_source_with_loader(source, path, &loader)
}

/// Compiles a KDL 2.0 schema starting from `entry_path` using `loader` into an immutable [`Module`] AST.
pub fn compile_with_loader(
    entry_path: &str,
    loader: &dyn SourceLoader,
) -> Result<Module, CheckResult> {
    compile_with_options(entry_path, loader, &CheckOptions::default())
}

/// Compiles an in-memory KDL 2.0 `source` string at logical `path` using `loader` into an immutable [`Module`] AST.
pub fn compile_source_with_loader(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
) -> Result<Module, CheckResult> {
    compile_source_with_options(source, path, loader, &CheckOptions::default())
}

/// Compiles a KDL 2.0 schema starting from `entry_path` using `loader` and `options`.
pub fn compile_with_options(
    entry_path: &str,
    loader: &dyn SourceLoader,
    options: &CheckOptions,
) -> Result<Module, CheckResult> {
    let mut result = check_with_options(entry_path, loader, options);
    if result.has_errors() {
        return Err(result);
    }
    result.module.take().ok_or(result)
}

/// Compiles an in-memory KDL 2.0 `source` string at logical `path` using `loader` and `options`.
pub fn compile_source_with_options(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
    options: &CheckOptions,
) -> Result<Module, CheckResult> {
    let mut result = check_source_with_options(source, path, loader, options);
    if result.has_errors() {
        return Err(result);
    }
    result.module.take().ok_or(result)
}

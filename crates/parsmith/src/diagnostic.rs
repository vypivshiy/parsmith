//! Compiler diagnostics, error codes, severity levels, and Rust-style snippet rendering.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::kdl::{KdlError, ParseDiagnostic, Span};
use serde::{Deserialize, Serialize};

use crate::imports::ResolvedProgram;

/// Canonical KDL Schema DSL v2.1 diagnostic codes (`E000`–`E403`, `W011`–`W041`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DiagnosticCode {
    /// `E000` (`SYNTAX_ERROR`): Invalid KDL 2.0 syntax.
    E000,
    /// `E001` (`INVALID_ARGUMENT_COUNT`): Invalid argument count, malformed block/import syntax, or inline schema collision.
    E001,
    /// `E002` (`INVALID_ARGUMENT_VALUE`): Invalid selector, regex, HTTP status, placeholder, or argument value.
    E002,
    /// `E003` (`INVALID_IMPORT`): Missing imported file, circular import chain, or unresolvable module import.
    E003,
    /// `E040` (`MALFORMED_DOTPATH`): Malformed GJSON dot-path expression.
    E040,
    /// `E041` (`DUPLICATE_JSON_KEY`): Duplicate JSON field name or wire key (`from=`).
    E041,
    /// `E100` (`TYPE_MISMATCH`): Pipeline operation or `fallback` type mismatch.
    E100,
    /// `E200` (`UNKNOWN_NODE`): Unrecognized top-level declaration, pipeline operation, or predicate keyword.
    E200,
    /// `E203` (`DISALLOWED_DIRECTIVE`): Disallowed directive or operation in the current context.
    E203,
    /// `E300` (`UNDEFINED_SYMBOL`): Reference to an undeclared or non-visible `struct`, `json`, `define`, `fn`, or `extension`, or dependency cycle.
    E300,
    /// `E301` (`UNDEFINED_INIT_FIELD`): Reference to an undeclared `@init` (`@name`) precomputed field.
    E301,
    /// `E302` (`INVALID_DECLARATION_ORDER`): Intra-file forward reference before declaration.
    E302,
    /// `E400` (`INVALID_TYPE_DECLARATION`): Invalid struct/json type tag, extension signature, or `@error` collision.
    E400,
    /// `E401` (`MISSING_REQUIRED_FIELD`): Struct missing required structural directives.
    E401,
    /// `E402` (`DUPLICATE_DECLARATION`): Duplicate symbol/field declaration or case-folded target collision.
    E402,
    /// `E403` (`INVALID_IDENTIFIER`): Invalid identifier or reserved keyword in a target language.
    E403,
    /// `W011` (`DEPRECATED_JSON_ALIAS`): Deprecated positional string alias on a `json` field.
    W011,
    /// `W040` (`WHITESPACE_IN_PATH`): Leading or trailing whitespace in a `from="..."` path.
    W040,
    /// `W041` (`PATH_SYNONYM_IN_JSON`): `path="..."` used instead of `from="..."` on a `json` field.
    W041,
}

impl DiagnosticCode {
    /// Returns the canonical string code (e.g. `"E000"`, `"W011"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::E000 => "E000",
            Self::E001 => "E001",
            Self::E002 => "E002",
            Self::E003 => "E003",
            Self::E040 => "E040",
            Self::E041 => "E041",
            Self::E100 => "E100",
            Self::E200 => "E200",
            Self::E203 => "E203",
            Self::E300 => "E300",
            Self::E301 => "E301",
            Self::E302 => "E302",
            Self::E400 => "E400",
            Self::E401 => "E401",
            Self::E402 => "E402",
            Self::E403 => "E403",
            Self::W011 => "W011",
            Self::W040 => "W040",
            Self::W041 => "W041",
        }
    }

    /// Returns the symbolic name of the diagnostic code (e.g. `"SYNTAX_ERROR"`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::E000 => "SYNTAX_ERROR",
            Self::E001 => "INVALID_ARGUMENT_COUNT",
            Self::E002 => "INVALID_ARGUMENT_VALUE",
            Self::E003 => "INVALID_IMPORT",
            Self::E040 => "MALFORMED_DOTPATH",
            Self::E041 => "DUPLICATE_JSON_KEY",
            Self::E100 => "TYPE_MISMATCH",
            Self::E200 => "UNKNOWN_NODE",
            Self::E203 => "DISALLOWED_DIRECTIVE",
            Self::E300 => "UNDEFINED_SYMBOL",
            Self::E301 => "UNDEFINED_INIT_FIELD",
            Self::E302 => "INVALID_DECLARATION_ORDER",
            Self::E400 => "INVALID_TYPE_DECLARATION",
            Self::E401 => "MISSING_REQUIRED_FIELD",
            Self::E402 => "DUPLICATE_DECLARATION",
            Self::E403 => "INVALID_IDENTIFIER",
            Self::W011 => "DEPRECATED_JSON_ALIAS",
            Self::W040 => "WHITESPACE_IN_PATH",
            Self::W041 => "PATH_SYNONYM_IN_JSON",
        }
    }

    /// Returns the default [`Severity`] associated with this diagnostic code.
    pub const fn default_severity(self) -> Severity {
        match self {
            Self::W011 | Self::W040 | Self::W041 => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// Returns `true` if this code is an error (`E*`).
    pub const fn is_error(self) -> bool {
        matches!(self.default_severity(), Severity::Error)
    }

    /// Returns `true` if this code is a warning (`W*`).
    pub const fn is_warning(self) -> bool {
        matches!(self.default_severity(), Severity::Warning)
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DiagnosticCode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "E000" => Ok(Self::E000),
            "E001" => Ok(Self::E001),
            "E002" => Ok(Self::E002),
            "E003" => Ok(Self::E003),
            "E040" => Ok(Self::E040),
            "E041" => Ok(Self::E041),
            "E100" => Ok(Self::E100),
            "E200" => Ok(Self::E200),
            "E203" => Ok(Self::E203),
            "E300" => Ok(Self::E300),
            "E301" => Ok(Self::E301),
            "E302" => Ok(Self::E302),
            "E400" => Ok(Self::E400),
            "E401" => Ok(Self::E401),
            "E402" => Ok(Self::E402),
            "E403" => Ok(Self::E403),
            "W011" => Ok(Self::W011),
            "W040" => Ok(Self::W040),
            "W041" => Ok(Self::W041),
            other => Err(format!("unknown diagnostic code '{other}'")),
        }
    }
}

impl PartialEq<&str> for DiagnosticCode {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<str> for DiagnosticCode {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

/// Severity level of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Fatal compilation or validation error.
    Error,
    /// Non-fatal lint warning.
    Warning,
}

impl Severity {
    /// Returns `"error"` or `"warning"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }

    /// Returns `true` if this severity is [`Severity::Error`].
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }

    /// Returns `true` if this severity is [`Severity::Warning`].
    pub const fn is_warning(self) -> bool {
        matches!(self, Self::Warning)
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for Severity {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// A structured compiler or linter diagnostic with source span and actionable hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Canonical diagnostic code (`E000`–`E403`, `W011`–`W041`).
    pub code: DiagnosticCode,
    /// Severity level (`Error` or `Warning`).
    pub severity: Severity,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Logical or filesystem path of the file where the diagnostic originated.
    pub path: String,
    /// 1-based starting line number (`0` when not attached to a specific line).
    pub line: usize,
    /// 1-based starting column number (`0` when not attached to a specific column).
    pub col: usize,
    /// 1-based ending line number.
    pub end_line: usize,
    /// 1-based ending column number.
    pub end_col: usize,
    /// Full source span with byte offsets and line/column coordinates.
    pub span: Span,
    /// Optional actionable hint explaining how to fix the issue.
    pub hint: Option<String>,
    /// Additional context notes attached to this diagnostic.
    pub notes: Vec<String>,
    /// Optional short inline label for the underlined span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Diagnostic {
    /// Creates a new [`Diagnostic`] with the default severity for `code`.
    pub fn new(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: code.default_severity(),
            message: message.into(),
            path: String::new(),
            line: span.line,
            col: span.col,
            end_line: span.end_line,
            end_col: span.end_col,
            span,
            hint: None,
            notes: Vec::new(),
            label: None,
        }
    }

    /// Creates a new [`Severity::Error`] diagnostic.
    pub fn error(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        let mut diag = Self::new(code, message, span);
        diag.severity = Severity::Error;
        diag
    }

    /// Creates a new [`Severity::Warning`] diagnostic.
    pub fn warning(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        let mut diag = Self::new(code, message, span);
        diag.severity = Severity::Warning;
        diag
    }

    /// Converts a [`crate::kdl::ParseDiagnostic`] into an `E000` (`SYNTAX_ERROR`) [`Diagnostic`].
    pub fn from_parse_diagnostic(parse_diag: &ParseDiagnostic, path: impl Into<String>) -> Self {
        let mut diag = Self::error(
            DiagnosticCode::E000,
            parse_diag.message.clone(),
            parse_diag.span,
        )
        .with_path(path);
        diag.label = parse_diag.label.clone();
        diag.hint = parse_diag.hint.clone();
        diag
    }

    /// Converts a [`crate::kdl::KdlError`] into its primary `E000` (`SYNTAX_ERROR`) [`Diagnostic`].
    pub fn from_kdl_error(err: &KdlError, path: impl Into<String>) -> Self {
        Self::from_parse_diagnostic(err.primary(), path)
    }

    /// Sets the file path on this diagnostic.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Sets the actionable hint on this diagnostic.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        let h = hint.into();
        if !h.is_empty() {
            self.hint = Some(h);
        }
        self
    }

    /// Sets the inline span label on this diagnostic.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        let l = label.into();
        if !l.is_empty() {
            self.label = Some(l);
        }
        self
    }

    /// Appends a note to this diagnostic.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Renders this diagnostic as a human-readable Rust-style annotated source snippet.
    ///
    /// Includes `error[EXXX]: message`, `--> path:line:col`, the source line preview,
    /// `^^^` caret underline, and `= hint: ...` / `= note: ...` footer lines.
    pub fn render(&self, source: Option<&str>) -> String {
        let owned_fallback;
        let resolved_source = match source {
            Some(s) => Some(s),
            None if !self.path.is_empty() => {
                owned_fallback = std::fs::read_to_string(&self.path).ok();
                owned_fallback
                    .as_deref()
                    .map(|s| s.strip_prefix('\u{feff}').unwrap_or(s))
            }
            None => None,
        };

        let gutter_width = if self.line > 0 {
            self.line.to_string().len()
        } else {
            1
        };
        let pad = " ".repeat(gutter_width);

        let mut lines_out = Vec::new();

        // 1. Header: error[E000]: message
        lines_out.push(format!(
            "{}[{}]: {}",
            self.severity.as_str(),
            self.code.as_str(),
            self.message
        ));

        // 2. Location arrow: --> path:line:col
        if self.line > 0 {
            let col = self.col.max(1);
            if !self.path.is_empty() {
                lines_out.push(format!("{pad}--> {}:{}:{}", self.path, self.line, col));
            } else {
                lines_out.push(format!("{pad}--> line {}:{}", self.line, col));
            }
        } else if !self.path.is_empty() {
            lines_out.push(format!("{pad}--> {}", self.path));
        }

        // 3. Source snippet & caret underline
        let src_line = resolved_source.and_then(|src| {
            if self.line == 0 {
                return None;
            }
            let clean = src.strip_prefix('\u{feff}').unwrap_or(src);
            clean.lines().nth(self.line - 1)
        });

        if let Some(line_text) = src_line {
            lines_out.push(format!("{pad} |"));
            lines_out.push(format!(
                "{:>width$} | {}",
                self.line,
                line_text,
                width = gutter_width
            ));

            let underline =
                build_underline(self.col, self.end_col, self.line, self.end_line, line_text);
            if let Some(label) = self.label.as_deref().filter(|l| !l.is_empty()) {
                lines_out.push(format!("{pad} | {underline} {label}"));
            } else {
                lines_out.push(format!("{pad} | {underline}"));
            }
        }

        // 4. Footer annotations (= hint: ..., = note: ...)
        let has_hint = self.hint.as_deref().is_some_and(|h| !h.is_empty());
        if (has_hint || !self.notes.is_empty()) && src_line.is_some() {
            lines_out.push(format!("{pad} |"));
        }

        if let Some(hint) = self.hint.as_deref().filter(|h| !h.is_empty()) {
            lines_out.push(format!("{pad} = hint: {hint}"));
        }

        for note in &self.notes {
            lines_out.push(format!("{pad} = note: {note}"));
        }

        lines_out.join("\n")
    }
}

fn build_underline(
    col: usize,
    end_col: usize,
    line: usize,
    end_line: usize,
    src_line: &str,
) -> String {
    let char_count = src_line.chars().count();
    let col_idx = col.saturating_sub(1);
    let caret_len = if end_line == line && end_col > col {
        let clamped_end = (end_col - 1).min(char_count.max(col_idx + 1));
        clamped_end.saturating_sub(col_idx).max(1)
    } else {
        token_length_at(src_line, col_idx)
    };
    format!("{}{}", " ".repeat(col_idx), "^".repeat(caret_len))
}

fn token_length_at(line: &str, col_idx: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    if col_idx >= chars.len() {
        return 1;
    }
    let mut end = col_idx;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    (end - col_idx).max(1)
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(None))
    }
}

/// Result of running schema validation (`parsmith check`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckResult {
    /// Collected errors and warnings in deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// Loaded source texts keyed by path for snippet rendering (not serialized to JSON).
    #[serde(skip)]
    pub sources: BTreeMap<String, String>,
    /// Resolved module graph and topological dependency closure (not serialized to JSON).
    #[serde(skip)]
    pub program: Option<ResolvedProgram>,
    /// Compiled `Algebraic AST` module when validation succeeded without fatal errors (not serialized to JSON).
    #[serde(skip)]
    pub module: Option<crate::ast::Module>,
}

impl PartialEq for CheckResult {
    fn eq(&self, other: &Self) -> bool {
        self.diagnostics == other.diagnostics
    }
}

impl Eq for CheckResult {}

impl CheckResult {
    /// Creates a new `CheckResult` from a list of diagnostics.
    pub fn new(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics,
            sources: BTreeMap::new(),
            program: None,
            module: None,
        }
    }

    /// Attaches a map of loaded source files for snippet rendering.
    pub fn with_sources(mut self, sources: BTreeMap<String, String>) -> Self {
        self.sources = sources;
        self
    }

    /// Attaches the resolved module graph.
    pub fn with_program(mut self, program: Option<ResolvedProgram>) -> Self {
        self.program = program;
        self
    }

    /// Attaches the compiled [`crate::ast::Module`] AST.
    pub fn with_module(mut self, module: Option<crate::ast::Module>) -> Self {
        self.module = module;
        self
    }

    /// Returns `true` if at least one [`Severity::Error`] diagnostic was emitted.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Returns `true` if at least one [`Severity::Warning`] diagnostic was emitted.
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning)
    }

    /// Returns `true` if there are no error diagnostics (`!self.has_errors()`).
    pub fn is_ok(&self) -> bool {
        !self.has_errors()
    }

    /// Returns the number of [`Severity::Error`] diagnostics.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// Returns the number of [`Severity::Warning`] diagnostics.
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    /// Iterates over all [`Severity::Error`] diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
    }

    /// Iterates over all [`Severity::Warning`] diagnostics.
    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    ///
    /// Returns an empty string if `self.diagnostics` is empty.
    pub fn format_text(&self) -> String {
        if self.diagnostics.is_empty() {
            return String::new();
        }
        self.diagnostics
            .iter()
            .map(|d| {
                let src = self.sources.get(&d.path).map(String::as_str);
                d.render(src)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Formats this `CheckResult` as a pretty-printed JSON string.
    pub fn format_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{\"diagnostics\":[]}".to_string())
    }
}

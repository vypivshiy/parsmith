use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::kdl::selector::SelectorError;
use crate::kdl::span::{LineIndex, Span};

/// A single syntax error diagnostic reported during KDL 2.0 parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseDiagnostic {
    /// Human-readable description of the syntax error.
    pub message: String,
    /// Exact source span where the error occurred.
    pub span: Span,
    /// Optional short label for the underlined span.
    pub label: Option<String>,
    /// Actionable hint for resolving the error.
    pub hint: Option<String>,
}

/// Structured KDL 2.0 parse error carrying source spans and diagnostic hints.
///
/// Dereferences to the primary [`ParseDiagnostic`] (`err.message`, `err.span`, `err.label`, `err.hint`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdlError {
    primary_index: usize,
    /// All individual diagnostics collected by the parser.
    pub diagnostics: Vec<ParseDiagnostic>,
    /// Present when this error originated from a malformed KDL selector expression.
    pub selector_error: Option<Box<SelectorError>>,
}

/// Type alias for [`KdlError`].
pub type ParseError = KdlError;

impl Deref for KdlError {
    type Target = ParseDiagnostic;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.diagnostics[self.primary_index]
    }
}

impl KdlError {
    /// Hint message for `escline` (`\`) followed by a `//` single-line comment on a subsequent line.
    pub const ESCLINE_COMMENT_HINT: &'static str = "Single-line comment '//' cannot follow an escline '\\' across lines because it silently terminates the node in KDL. Use block comments '/* ... */', or place comments before the node or after its definition.";

    /// Constructs a `KdlError` for an `escline` (`\`) followed across lines by a `//` comment.
    pub(crate) fn escline_comment(line_index: &LineIndex, offset: usize, len: usize) -> Self {
        let span = line_index.span(offset, len);
        let diag = ParseDiagnostic {
            message: "Single-line comment '//' cannot follow an escline '\\' across lines"
                .to_string(),
            span,
            label: Some("escline comment continuation".to_string()),
            hint: Some(Self::ESCLINE_COMMENT_HINT.to_string()),
        };
        Self {
            primary_index: 0,
            diagnostics: vec![diag],
            selector_error: None,
        }
    }

    /// Constructs a `KdlError` from a [`SelectorError`].
    pub fn from_selector_error(err: SelectorError) -> Self {
        let diag = ParseDiagnostic {
            message: err.message.clone(),
            span: err.span(),
            label: Some("invalid selector".to_string()),
            hint: None,
        };
        Self {
            primary_index: 0,
            diagnostics: vec![diag],
            selector_error: Some(Box::new(err)),
        }
    }

    /// Constructs a `KdlError` from a [`SelectorError`].
    #[inline]
    pub fn selector_error(err: SelectorError) -> Self {
        Self::from_selector_error(err)
    }

    /// Variant-style constructor wrapping a [`SelectorError`] into [`KdlError`].
    #[allow(non_snake_case)]
    #[inline]
    pub fn SelectorError(err: SelectorError) -> Self {
        Self::from_selector_error(err)
    }

    /// Returns `true` if this error originated from a malformed KDL selector expression.
    #[inline]
    pub fn is_selector_error(&self) -> bool {
        self.selector_error.is_some()
    }

    /// Returns the underlying [`SelectorError`] if this error originated from selector parsing.
    #[inline]
    pub fn as_selector_error(&self) -> Option<&SelectorError> {
        self.selector_error.as_deref()
    }

    /// Constructs a structured `KdlError` from a `kdl::KdlError` and precomputed `LineIndex`.
    pub(crate) fn from_kdl_error(err: kdl::KdlError, line_index: &LineIndex) -> Self {
        let source = line_index.source();
        let mut diagnostics = Vec::with_capacity(err.diagnostics.len().max(1));

        for diag in err.diagnostics {
            let offset = diag.span.offset();
            let len = diag.span.len();
            let span = line_index.span(offset, len);
            let slice = source.get(span.byte_range()).unwrap_or("").trim();

            let message = diag
                .message
                .clone()
                .or_else(|| {
                    diag.label
                        .as_ref()
                        .map(|l| format!("Unexpected token ({l})"))
                })
                .unwrap_or_else(|| "Invalid KDL 2.0 syntax".to_string());

            let label = diag.label.clone();
            let hint = diag
                .help
                .clone()
                .or_else(|| infer_syntax_hint(source, &span, slice, &message, label.as_deref()));

            diagnostics.push(ParseDiagnostic {
                message,
                span,
                label,
                hint,
            });
        }

        if diagnostics.is_empty() {
            let span = line_index.span(0, 0);
            diagnostics.push(ParseDiagnostic {
                message: "Invalid KDL 2.0 syntax".to_string(),
                span,
                label: Some("syntax error".to_string()),
                hint: Some("Fix the KDL 2.0 syntax error and try again.".to_string()),
            });
        }

        let primary_index = diagnostics
            .iter()
            .position(|d| d.message != "Expected end of document")
            .unwrap_or(0);

        Self {
            primary_index,
            diagnostics,
            selector_error: None,
        }
    }

    /// Returns a reference to the primary [`ParseDiagnostic`].
    #[inline]
    pub fn primary(&self) -> &ParseDiagnostic {
        &self.diagnostics[self.primary_index]
    }

    /// Returns the primary error message.
    #[inline]
    pub fn message(&self) -> &str {
        &self.primary().message
    }

    /// Returns the primary error span.
    #[inline]
    pub fn span(&self) -> Span {
        self.primary().span
    }

    /// Returns the 1-based line number of the primary error.
    #[inline]
    pub fn line(&self) -> usize {
        self.primary().span.line
    }

    /// Returns the 1-based column number of the primary error.
    #[inline]
    pub fn col(&self) -> usize {
        self.primary().span.col
    }

    /// Returns the 0-based byte offset of the primary error.
    #[inline]
    pub fn offset(&self) -> usize {
        self.primary().span.offset
    }

    /// Returns the byte length of the primary error span.
    #[inline]
    pub fn len(&self) -> usize {
        self.primary().span.len
    }

    /// Returns `true` if the primary error span has zero byte length.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.primary().span.len == 0
    }

    /// Returns the optional short label for the primary span.
    #[inline]
    pub fn label(&self) -> Option<&str> {
        self.primary().label.as_deref()
    }

    /// Returns the optional actionable hint for the primary error.
    #[inline]
    pub fn hint(&self) -> Option<&str> {
        self.primary().hint.as_deref()
    }

    /// Returns all parse diagnostics collected for the document.
    #[inline]
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }
}

fn infer_syntax_hint(
    source: &str,
    span: &Span,
    slice: &str,
    message: &str,
    label: Option<&str>,
) -> Option<String> {
    if matches!(slice, "true" | "false" | "null" | "inf" | "-inf" | "nan") {
        return Some(format!(
            "In KDL 2.0, `{slice}` is a reserved keyword literal and must be prefixed with `#` (`#{slice}`) or quoted (`\"{slice}\"`)."
        ));
    }
    let msg_lower = message.to_ascii_lowercase();
    if msg_lower.contains("closing '}'") || label == Some("not closed") {
        return Some("Add a matching closing `}` for the children block.".to_string());
    }
    if msg_lower.contains("type annotation") {
        return Some(
            "Type annotations must contain a valid identifier or quoted string inside `(...)`."
                .to_string(),
        );
    }
    let prefix = source.get(span.offset..).unwrap_or("");
    if prefix.starts_with('#') && prefix.contains('"') {
        return Some(
            "KDL 2.0 raw strings use `#\"...\"#` (without a leading `r`) and require matching `#` delimiters."
                .to_string(),
        );
    }
    if prefix.starts_with('"') {
        return Some(
            "Ensure the quoted string is closed with `\"` on the same line, or use `\"\"\"\\n...\\n\"\"\"` for multi-line strings."
                .to_string(),
        );
    }
    Some("Fix the KDL 2.0 syntax error and try again.".to_string())
}

impl fmt::Display for KdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (at line {}, column {})",
            self.message(),
            self.line(),
            self.col()
        )
    }
}

impl std::error::Error for KdlError {}

impl From<SelectorError> for KdlError {
    #[inline]
    fn from(err: SelectorError) -> Self {
        Self::from_selector_error(err)
    }
}

//! GJSON path expression parser, validator, and AST segments (`E040`, `E041`, `W011`, `W040`, `W041`).
//!
//! Supports the deterministic, filter-free GJSON subset used in KDL Schema DSL v2.1:
//! - Dot-separated object property keys (`a.b.c`, `@context`, `dc:creator`).
//! - Escaped literal characters (`\.`, `\\`, `\#`, `\*`, `\?`).
//! - Non-negative integer array indices (`0`, `1`, `42`).
//! - Array count (`#` as a terminal segment).
//! - Array element projection (`#.subpath...` mapping remaining segments over each element).
//! - Rejects empty paths, leading/trailing/consecutive unescaped dots, negative array indices,
//!   wildcards (`*`, `?`), filter expressions (`#[...]`, `#(...)`), and GJSON `@` modifiers.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error returned when a GJSON path expression violates the supported DSL v2.1 grammar (`E040`).
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct GjsonPathError {
    /// Human-readable description of why the GJSON path is malformed.
    pub message: String,
}

impl GjsonPathError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Kind of a flat segment in a [`GjsonPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GjsonSegmentKind {
    /// Object key lookup (`key` contains the unescaped property name).
    Key,
    /// Non-negative array element index (`index` contains the parsed `usize`).
    Index,
    /// Array operator `#` (terminal count or intermediate element projection).
    Hash,
}

/// A single flat segment of a parsed GJSON path expression (matching `ssc_codegen.traversal.utils.GjsonSegment`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GjsonSegment {
    /// Segment category (`Key`, `Index`, or `Hash`).
    pub kind: GjsonSegmentKind,
    /// Unescaped key text (`"#"` for `Hash`, digit string for `Index`).
    pub key: String,
    /// Parsed non-negative array index when `kind == GjsonSegmentKind::Index`.
    pub index: Option<usize>,
}

/// High-level algebraic step of a [`GjsonPath`] for codegen lowers in Python and TS/JS.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GjsonStep {
    /// Object property lookup (`data[key]`).
    Key(String),
    /// Non-negative array index lookup (`data[index]`).
    Index(usize),
    /// Array length (`len(data)` / `data.length`), valid when `#` is the final segment.
    ArrayCount,
    /// Array element projection (`#.subpath`), mapping `subpath` over each element of the array
    /// and filtering out `null`/`undefined` results.
    ArrayProjection(Vec<GjsonStep>),
}

/// A parsed and validated GJSON path expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GjsonPath {
    /// Original raw path string as written in the schema.
    pub raw: String,
    /// Flat sequence of segments (`Key`, `Index`, `Hash`).
    pub segments: Vec<GjsonSegment>,
    /// Algebraic traversal steps (with `#` grouped into `ArrayCount` or `ArrayProjection`).
    pub steps: Vec<GjsonStep>,
}

const GJSON_BUILTIN_MODIFIERS: &[&str] = &[
    "@reverse", "@ugly", "@pretty", "@this", "@valid", "@flatten", "@join", "@keys", "@values",
    "@tostr", "@fromstr", "@group", "@dig",
];

impl GjsonPath {
    /// Parses and validates a GJSON path expression.
    pub fn parse(path: &str) -> Result<Self, GjsonPathError> {
        let segments = parse_gjson_segments(path)?;
        let steps = build_steps(&segments);
        Ok(Self {
            raw: path.to_owned(),
            segments,
            steps,
        })
    }

    /// If this path resolves to a single flat object key lookup (`O(1)` dictionary access),
    /// returns `Some(unescaped_key)`. Returns `None` if it requires multi-segment, index, or `#` traversal.
    pub fn as_direct_key(&self) -> Option<&str> {
        if self.segments.len() == 1 && self.segments[0].kind == GjsonSegmentKind::Key {
            Some(self.segments[0].key.as_str())
        } else {
            None
        }
    }

    /// Returns `true` if this path is a single flat object key lookup.
    pub fn is_direct_key(&self) -> bool {
        self.as_direct_key().is_some()
    }
}

impl fmt::Display for GjsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

fn build_steps(segments: &[GjsonSegment]) -> Vec<GjsonStep> {
    let mut out = Vec::new();
    for (idx, seg) in segments.iter().enumerate() {
        match seg.kind {
            GjsonSegmentKind::Key => out.push(GjsonStep::Key(seg.key.clone())),
            GjsonSegmentKind::Index => {
                out.push(GjsonStep::Index(seg.index.unwrap_or(0)));
            }
            GjsonSegmentKind::Hash => {
                if idx + 1 == segments.len() {
                    out.push(GjsonStep::ArrayCount);
                } else {
                    let sub_steps = build_steps(&segments[idx + 1..]);
                    out.push(GjsonStep::ArrayProjection(sub_steps));
                }
                break;
            }
        }
    }
    out
}

/// Parses and validates a GJSON path expression into flat [`GjsonSegment`]s.
pub fn parse_gjson_segments(path: &str) -> Result<Vec<GjsonSegment>, GjsonPathError> {
    if path.is_empty() {
        return Err(GjsonPathError::new("Empty GJSON path expression"));
    }

    let chars: Vec<char> = path.chars().collect();
    let n = chars.len();

    let mut raw_chars = String::new();
    let mut unescaped_chars = String::new();
    let mut has_escape = false;
    let mut has_unescaped_hash = false;
    let mut raw_segments: Vec<(String, String, bool, bool)> = Vec::new();

    let mut i = 0;
    while i < n {
        let ch = chars[i];
        if ch == '\\' {
            if i + 1 >= n {
                return Err(GjsonPathError::new(format!(
                    "Trailing unescaped backslash in GJSON path '{path}'"
                )));
            }
            let nxt = chars[i + 1];
            if !matches!(nxt, '.' | '\\' | '#') {
                return Err(GjsonPathError::new(format!(
                    "Invalid escape sequence '\\{nxt}' in GJSON path '{path}'"
                )));
            }
            raw_chars.push(ch);
            raw_chars.push(nxt);
            unescaped_chars.push(nxt);
            has_escape = true;
            i += 2;
        } else if ch == '.' {
            if raw_chars.is_empty() {
                return Err(GjsonPathError::new(format!(
                    "Empty segment in GJSON path '{path}'"
                )));
            }
            raw_segments.push((
                std::mem::take(&mut raw_chars),
                std::mem::take(&mut unescaped_chars),
                has_escape,
                has_unescaped_hash,
            ));
            has_escape = false;
            has_unescaped_hash = false;
            i += 1;
        } else {
            if matches!(ch, '*' | '?' | '[' | ']' | '(' | ')' | '!' | '|') {
                return Err(GjsonPathError::new(format!(
                    "Unsupported GJSON wildcard or filter character '{ch}' in '{path}'"
                )));
            }
            if ch == '#' {
                has_unescaped_hash = true;
            }
            raw_chars.push(ch);
            unescaped_chars.push(ch);
            i += 1;
        }
    }

    if raw_chars.is_empty() {
        return Err(GjsonPathError::new(format!(
            "Empty trailing segment in GJSON path '{path}'"
        )));
    }
    raw_segments.push((raw_chars, unescaped_chars, has_escape, has_unescaped_hash));

    let mut segments = Vec::with_capacity(raw_segments.len());
    for (raw_seg, unescaped_seg, seg_has_escape, seg_has_hash) in raw_segments {
        if raw_seg == "#" {
            segments.push(GjsonSegment {
                kind: GjsonSegmentKind::Hash,
                key: "#".to_string(),
                index: None,
            });
        } else if seg_has_hash {
            return Err(GjsonPathError::new(format!(
                "Unsupported GJSON filter or hash expression '{raw_seg}' in '{path}'"
            )));
        } else if !seg_has_escape
            && (raw_seg == "@"
                || GJSON_BUILTIN_MODIFIERS.contains(&raw_seg.as_str())
                || raw_seg.starts_with("@") && raw_seg.contains(':'))
        {
            return Err(GjsonPathError::new(format!(
                "Unsupported GJSON modifier '{raw_seg}' in '{path}'"
            )));
        } else if !seg_has_escape
            && raw_seg.starts_with('-')
            && raw_seg.len() > 1
            && raw_seg[1..].chars().all(|c| c.is_ascii_digit())
        {
            return Err(GjsonPathError::new(format!(
                "Negative array index '{raw_seg}' is not supported in GJSON path '{path}'"
            )));
        } else if !seg_has_escape && raw_seg.chars().all(|c| c.is_ascii_digit()) {
            let idx = raw_seg.parse::<usize>().map_err(|_| {
                GjsonPathError::new(format!(
                    "Invalid array index '{raw_seg}' in GJSON path '{path}'"
                ))
            })?;
            segments.push(GjsonSegment {
                kind: GjsonSegmentKind::Index,
                key: raw_seg,
                index: Some(idx),
            });
        } else {
            segments.push(GjsonSegment {
                kind: GjsonSegmentKind::Key,
                key: unescaped_seg,
                index: None,
            });
        }
    }

    Ok(segments)
}

/// Checks whether a GJSON path expression violates the supported DSL v2.1 subset grammar.
pub fn is_malformed_gjson_path(path: &str) -> bool {
    parse_gjson_segments(path).is_err()
}

/// Classifies a GJSON path into a direct `O(1)` dictionary key `(true, unescaped_key)`
/// or a multi-segment/index/projection query `(false, raw_path)`.
pub fn classify_gjson_path(path: &str) -> (bool, String) {
    match GjsonPath::parse(path) {
        Ok(parsed) => {
            if let Some(direct) = parsed.as_direct_key() {
                (true, direct.to_owned())
            } else {
                (false, path.to_owned())
            }
        }
        Err(_) => (false, path.to_owned()),
    }
}

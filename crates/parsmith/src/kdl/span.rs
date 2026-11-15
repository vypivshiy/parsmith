use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Source location span with byte offsets and 1-based line/column coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Span {
    /// 0-based starting byte offset in the source text.
    pub offset: usize,
    /// Length of the span in bytes.
    pub len: usize,
    /// 1-based starting line number (`0` for synthetic/unattached spans).
    pub line: usize,
    /// 1-based starting column number in characters (`0` for synthetic/unattached spans).
    pub col: usize,
    /// 1-based ending line number (`0` for synthetic/unattached spans).
    pub end_line: usize,
    /// 1-based ending column number in characters (`0` for synthetic/unattached spans).
    pub end_col: usize,
}

impl Span {
    /// Sentinel empty span for synthetic or unattached nodes and values.
    pub const EMPTY: Self = Self {
        offset: 0,
        len: 0,
        line: 0,
        col: 0,
        end_line: 0,
        end_col: 0,
    };

    /// Returns `true` if this span is empty (`len == 0 && line == 0`).
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0 && self.line == 0
    }

    /// Returns the exclusive ending byte offset (`offset + len`).
    #[inline]
    pub const fn end_offset(&self) -> usize {
        self.offset + self.len
    }

    /// Returns the byte range `offset..offset + len`.
    #[inline]
    pub const fn byte_range(&self) -> std::ops::Range<usize> {
        self.offset..(self.offset + self.len)
    }
}

/// Precomputed line-offset lookup table for `O(log N)` byte-offset to `1-based` `(line, col)` conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    source: Arc<str>,
    line_starts: Vec<usize>,
    is_ascii: bool,
}

impl Default for LineIndex {
    fn default() -> Self {
        Self::new("")
    }
}

impl LineIndex {
    /// Builds a `LineIndex` from a source string.
    pub fn new(source: impl Into<Arc<str>>) -> Self {
        let source: Arc<str> = source.into();
        let is_ascii = source.is_ascii();
        let mut line_starts = vec![0usize];

        let mut chars = source.char_indices().peekable();
        while let Some((idx, ch)) = chars.next() {
            match ch {
                '\r' => {
                    if let Some(&(next_idx, '\n')) = chars.peek() {
                        chars.next();
                        line_starts.push(next_idx + 1);
                    } else {
                        line_starts.push(idx + 1);
                    }
                }
                '\n' | '\u{0085}' | '\u{000B}' | '\u{000C}' | '\u{2028}' | '\u{2029}' => {
                    line_starts.push(idx + ch.len_utf8());
                }
                _ => {}
            }
        }

        Self {
            source,
            line_starts,
            is_ascii,
        }
    }

    /// Returns the underlying source text.
    #[inline]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the number of lines in the source text.
    #[inline]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Returns the byte offsets of each line start.
    #[inline]
    pub fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }

    /// Converts a 0-based byte offset into 1-based `(line, col)` coordinates in `O(log N)`.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let clamped = offset.min(self.source.len());
        let line_idx = self
            .line_starts
            .partition_point(|&start| start <= clamped)
            .saturating_sub(1);
        let line = line_idx + 1;
        let line_start = self.line_starts[line_idx];
        let col = if self.is_ascii {
            clamped - line_start + 1
        } else {
            self.source
                .get(line_start..clamped)
                .map_or(clamped - line_start + 1, |s| s.chars().count() + 1)
        };
        (line, col)
    }

    /// Constructs a [`Span`] with precomputed 1-based `(line, col)` and `(end_line, end_col)` from `(offset, len)`.
    pub fn span(&self, offset: usize, len: usize) -> Span {
        let start = offset.min(self.source.len());
        let end = offset.saturating_add(len).min(self.source.len());
        let actual_len = end.saturating_sub(start);
        let (line, col) = self.line_col(start);
        let (end_line, end_col) = self.line_col(end);
        Span {
            offset: start,
            len: actual_len,
            line,
            col,
            end_line,
            end_col,
        }
    }

    /// Returns the text slice for a given 1-based line number (excluding trailing line break).
    pub fn line_text(&self, line: usize) -> Option<&str> {
        if line == 0 || line > self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[line - 1];
        let end = if line < self.line_starts.len() {
            self.line_starts[line]
        } else {
            self.source.len()
        };
        let slice = self.source.get(start..end)?;
        Some(slice.trim_end_matches([
            '\r', '\n', '\u{0085}', '\u{000B}', '\u{000C}', '\u{2028}', '\u{2029}',
        ]))
    }
}

//! Two-pass indentation-aware `CodeWriter` supporting `DryRun` (Pass 1) and `Emit` (Pass 2) modes.

use std::borrow::Cow;
use std::fmt::{self, Write as _};

/// Writes a formatted line into a [`CodeWriter`] at the current indentation level (`writer.writeln(format_args!(...))`),
/// performing zero buffer allocation in [`WriteMode::DryRun`] mode.
#[macro_export]
macro_rules! wln {
    ($writer:expr, $($arg:tt)*) => {
        $writer.writeln(::core::format_args!($($arg)*))
    };
}
pub use crate::wln;

/// Trait implemented by line arguments accepted by [`CodeWriter::writeln`], supporting both
/// string slices (`&str`, `String`, `&String`, `Cow<str>`) and deferred [`fmt::Arguments`]
/// (`format_args!(...)`) with zero buffer allocation in [`WriteMode::DryRun`] mode.
pub trait IntoCodeLine {
    /// Writes this line's content into `buf` (without leading indentation or trailing newline).
    fn write_line_into(self, buf: &mut String);
}

impl IntoCodeLine for &str {
    #[inline]
    fn write_line_into(self, buf: &mut String) {
        buf.push_str(self);
    }
}

impl IntoCodeLine for String {
    #[inline]
    fn write_line_into(self, buf: &mut String) {
        buf.push_str(&self);
    }
}

impl IntoCodeLine for &String {
    #[inline]
    fn write_line_into(self, buf: &mut String) {
        buf.push_str(self);
    }
}

impl IntoCodeLine for Cow<'_, str> {
    #[inline]
    fn write_line_into(self, buf: &mut String) {
        buf.push_str(&self);
    }
}

impl IntoCodeLine for fmt::Arguments<'_> {
    #[inline]
    fn write_line_into(self, buf: &mut String) {
        let _ = buf.write_fmt(self);
    }
}

/// Execution pass mode for [`CodeWriter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteMode {
    /// Pass 1: discards all emitted source text with zero buffer allocation while
    /// visitors register required imports, standard helpers, and extension helpers in `ModuleBuilder`.
    DryRun,
    /// Pass 2: formats and appends indented lines into the output string buffer.
    Emit,
}

/// Indentation-aware two-pass source code writer with scoped local variable allocation (`v0`, `v1`, ...).
#[derive(Debug, Clone)]
pub struct CodeWriter {
    mode: WriteMode,
    buf: String,
    indent_level: usize,
    indent_unit: &'static str,
    var_counter: usize,
}

impl CodeWriter {
    /// Creates a Pass 1 `DryRun` writer that performs zero output buffer allocations.
    pub const fn dry_run() -> Self {
        Self {
            mode: WriteMode::DryRun,
            buf: String::new(),
            indent_level: 0,
            indent_unit: "    ",
            var_counter: 0,
        }
    }

    /// Creates a Pass 2 `Emit` writer with default 4-space indentation (`"    "`).
    pub fn emit() -> Self {
        Self::with_indent("    ")
    }

    /// Creates a Pass 2 `Emit` writer with a custom indentation unit (e.g. `"    "` or `"  "`).
    pub fn with_indent(indent_unit: &'static str) -> Self {
        Self {
            mode: WriteMode::Emit,
            buf: String::with_capacity(4096),
            indent_level: 0,
            indent_unit,
            var_counter: 0,
        }
    }

    /// Returns the current [`WriteMode`].
    #[inline]
    pub const fn mode(&self) -> WriteMode {
        self.mode
    }

    /// Returns `true` if this writer is in [`WriteMode::DryRun`] mode.
    #[inline]
    pub const fn is_dry_run(&self) -> bool {
        matches!(self.mode, WriteMode::DryRun)
    }

    /// Returns `true` if this writer is in [`WriteMode::Emit`] mode.
    #[inline]
    pub const fn is_emit(&self) -> bool {
        matches!(self.mode, WriteMode::Emit)
    }

    /// Increases the current block indentation level by 1.
    #[inline]
    pub fn indent(&mut self) -> &mut Self {
        self.indent_level += 1;
        self
    }

    /// Decreases the current block indentation level by 1 (saturating at 0).
    #[inline]
    pub fn dedent(&mut self) -> &mut Self {
        self.indent_level = self.indent_level.saturating_sub(1);
        self
    }

    /// Returns the current nesting depth (0, 1, 2, ...).
    #[inline]
    pub const fn indent_level(&self) -> usize {
        self.indent_level
    }

    /// Returns the configured single-level indentation unit string.
    #[inline]
    pub const fn indent_unit(&self) -> &'static str {
        self.indent_unit
    }

    /// Executes `f` inside an indented block (`indent()` -> `f(self)` -> `dedent()`).
    #[inline]
    pub fn indented<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut Self),
    {
        self.indent();
        f(self);
        self.dedent()
    }

    /// Resets the scoped local variable counter to `0` at the start of a function or method.
    #[inline]
    pub fn reset_vars(&mut self) -> &mut Self {
        self.var_counter = 0;
        self
    }

    /// Allocates and returns the next scoped local variable identifier (`"v0"`, `"v1"`, `"v2"`, ...).
    #[inline]
    pub fn next_var(&mut self) -> String {
        let idx = self.var_counter;
        self.var_counter += 1;
        format!("v{idx}")
    }

    /// Writes a single line at the current indentation level followed by `\n`.
    ///
    /// Accepts both string slices (`&str`, `String`, `Cow<str>`) and deferred
    /// [`fmt::Arguments`] (`format_args!(...)`).
    /// In [`WriteMode::DryRun`] mode, this is a no-op with zero buffer allocations.
    #[inline]
    pub fn writeln(&mut self, line: impl IntoCodeLine) -> &mut Self {
        if matches!(self.mode, WriteMode::DryRun) {
            return self;
        }
        let indent_start = self.buf.len();
        for _ in 0..self.indent_level {
            self.buf.push_str(self.indent_unit);
        }
        let content_start = self.buf.len();
        line.write_line_into(&mut self.buf);
        if self.buf.len() == content_start {
            self.buf.truncate(indent_start);
        }
        self.buf.push('\n');
        self
    }

    /// Splits a multi-line block on line breaks and writes each line at the current indentation level.
    ///
    /// Blank lines within `block` are emitted as empty `\n` lines without trailing indentation spaces.
    pub fn write_block(&mut self, block: &str) -> &mut Self {
        if matches!(self.mode, WriteMode::DryRun) {
            return self;
        }
        for line in block.lines() {
            if line.trim().is_empty() {
                self.buf.push('\n');
            } else {
                self.writeln(line);
            }
        }
        self
    }

    /// Writes an empty newline `\n`.
    ///
    /// In [`WriteMode::DryRun`] mode, this is a no-op.
    #[inline]
    pub fn blankline(&mut self) -> &mut Self {
        if matches!(self.mode, WriteMode::DryRun) {
            return self;
        }
        self.buf.push('\n');
        self
    }

    /// Returns a view of the emitted buffer so far (empty in `DryRun` mode).
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.buf
    }

    /// Returns the allocated byte capacity of the underlying buffer (`0` in `DryRun` mode).
    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Consumes the writer and returns the final emitted source string.
    #[inline]
    pub fn finish(self) -> String {
        self.buf
    }
}

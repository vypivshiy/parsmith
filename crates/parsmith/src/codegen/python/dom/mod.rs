//! `PyDomSpelling` trait and `PythonDom` backend dispatcher (`bs4`, `lxml`, `parsel`, `selectolax`).

pub mod bs4;
pub mod lxml;
pub mod parsel;
pub mod selectolax;

use crate::ast::RawMode;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, PythonHtmlLib};

pub use bs4::Bs4Spelling;
pub use lxml::LxmlSpelling;
pub use parsel::ParselSpelling;
pub use selectolax::SelectolaxSpelling;

/// Backend sub-trait defining HTML-library-specific DOM query statements and predicate expressions (`DomSpelling`).
pub trait PyDomSpelling {
    /// Canonical backend identifier (`"python-bs4"`, `"python-lxml"`, `"python-parsel"`, `"python-selectolax"`).
    fn backend_name(&self) -> &'static str;

    /// Returns `true` if this backend supports native XPath 1.0 queries (`lxml`, `parsel`).
    fn supports_xpath(&self) -> bool;

    /// Registers required DOM parser and typing imports in `builder`.
    fn register_imports(&self, builder: &mut ModuleBuilder);

    /// Emits module-level DOM constants and type aliases (e.g. `RT_BS4_FEATURES`, `RtBs4Html`).
    fn emit_utilities(&self, writer: &mut CodeWriter);

    /// Python type annotation for a single DOM document/element.
    fn document_type(&self) -> &'static str;

    /// Python type annotation for a list of DOM elements.
    fn document_array_type(&self) -> &'static str;

    /// Python type annotation accepted by `__init__` and `fn`.
    fn init_arg_type(&self) -> &'static str;

    /// Extra keyword parameter signature fragment for `__init__` and `fn`.
    fn extra_params(&self) -> &'static str;

    /// Python expression parsing a `document: str` into a DOM tree.
    fn init_from_str_expr(&self) -> &'static str;

    /// Emits document initialization / type narrowing in `__init__` or `fn`.
    fn emit_doc_init(&self, target_var: &str, writer: &mut CodeWriter) {
        let doc_ty = self.document_type();
        let from_str = self.init_from_str_expr();
        writer.writeln("if isinstance(document, str):");
        writer.indented(|writer| {
            wln!(writer, "{target_var}: {doc_ty} = {from_str}");
        });
        writer.writeln("else:");
        writer.indented(|writer| {
            wln!(writer, "{target_var} = document");
        });
    }

    /// Emits a `css` single-element selection statement.
    fn emit_css_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    );

    /// Emits a `css-all` multi-element selection statement.
    fn emit_css_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    );

    /// Emits a `css-remove` element decomposition statement.
    fn emit_css_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    );

    /// Emits an `xpath` single-element selection statement.
    fn emit_xpath_select(
        &self,
        _prv: &str,
        _nxt: &str,
        _queries: &[String],
        _next_is_to_bool: bool,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits an `xpath-all` multi-element selection statement.
    fn emit_xpath_select_all(
        &self,
        _prv: &str,
        _nxt: &str,
        _queries: &[String],
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits an `xpath-remove` element removal statement.
    fn emit_xpath_remove(
        &self,
        _prv: &str,
        _nxt: &str,
        _query: &str,
        _is_array: bool,
        _builder: &mut ModuleBuilder,
        _writer: &mut CodeWriter,
    ) {
    }

    /// Emits a `text` extraction statement.
    fn emit_text(
        &self,
        prv: &str,
        nxt: &str,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    );

    /// Emits a `raw [outer|inner]` HTML serialization statement.
    fn emit_raw(
        &self,
        prv: &str,
        nxt: &str,
        mode: RawMode,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    );

    /// Emits an `attr <key1> [key2 ...]` attribute extraction statement.
    fn emit_attr(
        &self,
        prv: &str,
        nxt: &str,
        keys: &[String],
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    );

    /// Formats a `css <query>` predicate expression over `i`.
    fn pred_css(&self, query: &str, builder: &mut ModuleBuilder) -> String;

    /// Formats an `xpath <query>` predicate expression over `i`.
    fn pred_xpath(&self, _query: &str, _builder: &mut ModuleBuilder) -> String {
        "False".to_owned()
    }

    /// Formats a `has-attr <a1> [a2 ...]` predicate expression over `i`.
    fn pred_has_attr(&self, attrs: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats an `attr-eq` / `attr-in` predicate expression over `i`.
    fn pred_attr_eq(&self, name: &str, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats an `attr-ne` predicate expression over `i`.
    fn pred_attr_ne(&self, name: &str, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats an `attr-starts` predicate expression over `i`.
    fn pred_attr_starts(
        &self,
        name: &str,
        values: &[String],
        builder: &mut ModuleBuilder,
    ) -> String;

    /// Formats an `attr-ends` predicate expression over `i`.
    fn pred_attr_ends(&self, name: &str, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats an `attr-contains` predicate expression over `i`.
    fn pred_attr_contains(
        &self,
        name: &str,
        values: &[String],
        builder: &mut ModuleBuilder,
    ) -> String;

    /// Formats an `attr-re` predicate expression over `i`.
    fn pred_attr_re(&self, name: &str, pattern: &str, builder: &mut ModuleBuilder) -> String;

    /// Formats a `text-eq` / `text-in` predicate expression over `i`.
    fn pred_text_eq(&self, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats a `text-ne` predicate expression over `i`.
    fn pred_text_ne(&self, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats a `text-starts` predicate expression over `i`.
    fn pred_text_starts(&self, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats a `text-ends` predicate expression over `i`.
    fn pred_text_ends(&self, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats a `text-contains` predicate expression over `i`.
    fn pred_text_contains(&self, values: &[String], builder: &mut ModuleBuilder) -> String;

    /// Formats a `text-re` predicate expression over `i`.
    fn pred_text_re(&self, pattern: &str, builder: &mut ModuleBuilder) -> String;
}

/// Dynamic Python DOM backend dispatcher wrapping [`Bs4Spelling`], [`LxmlSpelling`],
/// [`ParselSpelling`], and [`SelectolaxSpelling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonDom {
    /// `BeautifulSoup4` (`bs4`).
    Bs4(Bs4Spelling),
    /// `lxml.html` (`lxml`).
    Lxml(LxmlSpelling),
    /// `parsel` (`Selector` / `SelectorList`).
    Parsel(ParselSpelling),
    /// `selectolax.lexbor` (`LexborHTMLParser` / `LexborNode`).
    Selectolax(SelectolaxSpelling),
}

impl Default for PythonDom {
    fn default() -> Self {
        Self::Bs4(Bs4Spelling::default())
    }
}

impl PythonDom {
    /// Constructs a `PythonDom` from [`CodegenOptions`].
    pub const fn from_options(options: &CodegenOptions) -> Self {
        match options.html_lib {
            PythonHtmlLib::Bs4 => Self::Bs4(Bs4Spelling::new(options.bs4_parser)),
            PythonHtmlLib::Lxml => Self::Lxml(LxmlSpelling::new()),
            PythonHtmlLib::Parsel => Self::Parsel(ParselSpelling::new()),
            PythonHtmlLib::Selectolax => Self::Selectolax(SelectolaxSpelling::new()),
        }
    }
}

macro_rules! delegate_dom {
    ($self:ident, $method:ident ( $($arg:ident),* )) => {
        match $self {
            Self::Bs4(s) => s.$method($($arg),*),
            Self::Lxml(s) => s.$method($($arg),*),
            Self::Parsel(s) => s.$method($($arg),*),
            Self::Selectolax(s) => s.$method($($arg),*),
        }
    };
}

impl PyDomSpelling for PythonDom {
    fn backend_name(&self) -> &'static str {
        delegate_dom!(self, backend_name())
    }

    fn supports_xpath(&self) -> bool {
        delegate_dom!(self, supports_xpath())
    }

    fn register_imports(&self, builder: &mut ModuleBuilder) {
        delegate_dom!(self, register_imports(builder))
    }

    fn emit_utilities(&self, writer: &mut CodeWriter) {
        delegate_dom!(self, emit_utilities(writer))
    }

    fn document_type(&self) -> &'static str {
        delegate_dom!(self, document_type())
    }

    fn document_array_type(&self) -> &'static str {
        delegate_dom!(self, document_array_type())
    }

    fn init_arg_type(&self) -> &'static str {
        delegate_dom!(self, init_arg_type())
    }

    fn extra_params(&self) -> &'static str {
        delegate_dom!(self, extra_params())
    }

    fn init_from_str_expr(&self) -> &'static str {
        delegate_dom!(self, init_from_str_expr())
    }

    fn emit_doc_init(&self, target_var: &str, writer: &mut CodeWriter) {
        delegate_dom!(self, emit_doc_init(target_var, writer))
    }

    fn emit_css_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(
            self,
            emit_css_select(prv, nxt, queries, next_is_to_bool, builder, writer)
        )
    }

    fn emit_css_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(
            self,
            emit_css_select_all(prv, nxt, queries, builder, writer)
        )
    }

    fn emit_css_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(
            self,
            emit_css_remove(prv, nxt, query, is_array, builder, writer)
        )
    }

    fn emit_xpath_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(
            self,
            emit_xpath_select(prv, nxt, queries, next_is_to_bool, builder, writer)
        )
    }

    fn emit_xpath_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(
            self,
            emit_xpath_select_all(prv, nxt, queries, builder, writer)
        )
    }

    fn emit_xpath_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(
            self,
            emit_xpath_remove(prv, nxt, query, is_array, builder, writer)
        )
    }

    fn emit_text(
        &self,
        prv: &str,
        nxt: &str,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(self, emit_text(prv, nxt, is_array, builder, writer))
    }

    fn emit_raw(
        &self,
        prv: &str,
        nxt: &str,
        mode: RawMode,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(self, emit_raw(prv, nxt, mode, is_array, builder, writer))
    }

    fn emit_attr(
        &self,
        prv: &str,
        nxt: &str,
        keys: &[String],
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        delegate_dom!(self, emit_attr(prv, nxt, keys, is_array, builder, writer))
    }

    fn pred_css(&self, query: &str, builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_css(query, builder))
    }

    fn pred_xpath(&self, query: &str, builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_xpath(query, builder))
    }

    fn pred_has_attr(&self, attrs: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_has_attr(attrs, builder))
    }

    fn pred_attr_eq(&self, name: &str, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_attr_eq(name, values, builder))
    }

    fn pred_attr_ne(&self, name: &str, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_attr_ne(name, values, builder))
    }

    fn pred_attr_starts(
        &self,
        name: &str,
        values: &[String],
        builder: &mut ModuleBuilder,
    ) -> String {
        delegate_dom!(self, pred_attr_starts(name, values, builder))
    }

    fn pred_attr_ends(&self, name: &str, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_attr_ends(name, values, builder))
    }

    fn pred_attr_contains(
        &self,
        name: &str,
        values: &[String],
        builder: &mut ModuleBuilder,
    ) -> String {
        delegate_dom!(self, pred_attr_contains(name, values, builder))
    }

    fn pred_attr_re(&self, name: &str, pattern: &str, builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_attr_re(name, pattern, builder))
    }

    fn pred_text_eq(&self, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_text_eq(values, builder))
    }

    fn pred_text_ne(&self, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_text_ne(values, builder))
    }

    fn pred_text_starts(&self, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_text_starts(values, builder))
    }

    fn pred_text_ends(&self, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_text_ends(values, builder))
    }

    fn pred_text_contains(&self, values: &[String], builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_text_contains(values, builder))
    }

    fn pred_text_re(&self, pattern: &str, builder: &mut ModuleBuilder) -> String {
        delegate_dom!(self, pred_text_re(pattern, builder))
    }
}

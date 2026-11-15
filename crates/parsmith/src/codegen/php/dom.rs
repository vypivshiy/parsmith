//! PHP DOM extraction spelling strategies (`PhpDomSpelling`, `PhpNativeSpelling`, `PhpDomCrawlerSpelling`).
//!
//! - [`PhpNativeSpelling`] (`--html-lib native`, default): Zero-Composer built-in `ext-dom`
//!   (`\DOMDocument` + `\DOMXPath` + `\DOMElement`). Lowers CSS3/CSS4 selectors at compile time
//!   via [`crate::validators::css_to_xpath`] and evaluates both CSS and XPath 1.0 queries natively.
//! - [`PhpDomCrawlerSpelling`] (`--html-lib dom-crawler`): `Symfony\Component\DomCrawler\Crawler`
//!   evaluating CSS selectors via `->filter()` and XPath queries via `->filterXPath()`.

use crate::kdl::Span;

use crate::ast::{RawMode, TypeInfo};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::php::{php_str, php_str_array};
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, PhpHtmlLib};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::validators::css_to_xpath;

/// Strategy protocol for PHP HTML DOM extraction backends (`native` vs `dom-crawler`).
pub trait PhpDomSpelling {
    /// Returns the canonical backend identifier (`"php-native"` or `"php-dom-crawler"`).
    fn backend_name(&self) -> &'static str;

    /// Registers any top-level `use` imports required by this DOM backend.
    fn register_imports(&self, builder: &mut ModuleBuilder);

    /// Returns the PHP type hint for a single DOM element/node (`\DOMElement` or `Crawler`).
    fn document_type(&self) -> &'static str;

    /// Returns the PHP type hint for an array of DOM elements (`array`).
    fn document_array_type(&self) -> &'static str {
        "array"
    }

    /// Returns the PHP constructor parameter type hint for an HTML parser struct.
    fn init_arg_type(&self) -> &'static str;

    /// Emits a `css` single-element selection step (`$nxt = ...;`).
    #[allow(clippy::too_many_arguments)]
    fn emit_css_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        context: &str,
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    );

    /// Emits a `css-all` multi-element selection step (`$nxt = ...;`).
    #[allow(clippy::too_many_arguments)]
    fn emit_css_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    );

    /// Emits a `css-remove` step (`$nxt = ...;`).
    #[allow(clippy::too_many_arguments)]
    fn emit_css_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    );

    /// Emits an `xpath` single-element selection step (`$nxt = ...;`).
    fn emit_xpath_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        context: &str,
        writer: &mut CodeWriter,
    ) {
        let q_arr = php_str_array(queries);
        if next_is_to_bool {
            wln!(writer, "${nxt} = Rt::xpathFirstOpt(${prv}, {q_arr});");
        } else {
            let ctx_lit = php_str(context);
            wln!(
                writer,
                "${nxt} = Rt::xpathFirst(${prv}, {q_arr}, {ctx_lit});"
            );
        }
    }

    /// Emits an `xpath-all` multi-element selection step (`$nxt = ...;`).
    fn emit_xpath_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        writer: &mut CodeWriter,
    ) {
        let q_arr = php_str_array(queries);
        wln!(writer, "${nxt} = Rt::xpathAll(${prv}, {q_arr});");
    }

    /// Emits an `xpath-remove` step (`$nxt = ...;`).
    fn emit_xpath_remove(&self, prv: &str, nxt: &str, query: &str, writer: &mut CodeWriter) {
        let q_lit = php_str(query);
        wln!(writer, "${nxt} = Rt::xpathRemove(${prv}, {q_lit});");
    }

    /// Emits a `text` extraction step (`$nxt = ...;`).
    fn emit_text(&self, prv: &str, nxt: &str, is_array: bool, writer: &mut CodeWriter) {
        if is_array {
            wln!(writer, "${nxt} = Rt::textAll(${prv});");
        } else {
            wln!(writer, "${nxt} = Rt::text(${prv});");
        }
    }

    /// Emits a `raw` (`inner` or `outer`) HTML serialization step (`$nxt = ...;`).
    fn emit_raw(
        &self,
        prv: &str,
        nxt: &str,
        mode: RawMode,
        is_array: bool,
        writer: &mut CodeWriter,
    ) {
        let inner = if mode == RawMode::Inner {
            "true"
        } else {
            "false"
        };
        if is_array {
            wln!(writer, "${nxt} = Rt::rawAll(${prv}, {inner});");
        } else {
            wln!(writer, "${nxt} = Rt::raw(${prv}, {inner});");
        }
    }

    /// Emits an `attr` extraction step (`$nxt = ...;`).
    fn emit_attr(
        &self,
        prv: &str,
        nxt: &str,
        keys: &[String],
        is_array_in: bool,
        context: &str,
        writer: &mut CodeWriter,
    ) {
        let k_arr = php_str_array(keys);
        if keys.len() > 1 {
            wln!(writer, "${nxt} = Rt::attrMulti(${prv}, {k_arr});");
        } else if is_array_in {
            let ctx_lit = php_str(context);
            wln!(writer, "${nxt} = Rt::attrList(${prv}, {k_arr}, {ctx_lit});");
        } else {
            let ctx_lit = php_str(context);
            wln!(writer, "${nxt} = Rt::attr(${prv}, {k_arr}, {ctx_lit});");
        }
    }

    /// Emits a `to-bool` conversion step (`$nxt = ...;`).
    fn emit_to_bool(&self, prv: &str, nxt: &str, input_type: &TypeInfo, writer: &mut CodeWriter) {
        use crate::ast::BaseType;
        let expr = if input_type.base == BaseType::Document {
            format!("Rt::nodeBool(${prv})")
        } else if input_type.is_array {
            format!("!empty(${prv})")
        } else if input_type.base == BaseType::Int {
            format!("(${prv} !== 0)")
        } else if input_type.base == BaseType::Float {
            format!("(${prv} !== 0.0 && !is_nan(${prv}))")
        } else if input_type.base == BaseType::Bool {
            format!("(bool) ${prv}")
        } else {
            format!("(${prv} !== '')")
        };
        wln!(writer, "${nxt} = {expr};");
    }

    /// Renders a `css` predicate condition on `$target`.
    fn pred_css(
        &self,
        target: &str,
        query: &str,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> String;

    /// Renders an `xpath` predicate condition on `$target`.
    fn pred_xpath(&self, target: &str, query: &str) -> String {
        format!("Rt::predXpath({target}, {})", php_str(query))
    }
}

fn lower_css_or_diag(
    query: &str,
    span: Span,
    source_file: &str,
    is_dry_run: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    match css_to_xpath(query) {
        Ok(xp) => xp,
        Err(err) => {
            if is_dry_run {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E002,
                        format!(
                            "CSS selector '{query}' cannot be lowered to XPath 1.0 for php-native: {err}"
                        ),
                        span,
                    )
                    .with_path(source_file.to_owned())
                    .with_hint("use an 'xpath' step or compile with '--html-lib dom-crawler'"),
                );
            }
            ".//*".to_owned()
        }
    }
}

/// Zero-Composer built-in PHP `ext-dom` (`\DOMDocument` + `\DOMXPath`) spelling (`--html-lib native`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhpNativeSpelling;

impl PhpNativeSpelling {
    /// Creates a new `PhpNativeSpelling`.
    pub const fn new() -> Self {
        Self
    }
}

impl PhpDomSpelling for PhpNativeSpelling {
    fn backend_name(&self) -> &'static str {
        "php-native"
    }

    fn register_imports(&self, _builder: &mut ModuleBuilder) {}

    fn document_type(&self) -> &'static str {
        "\\DOMElement"
    }

    fn init_arg_type(&self) -> &'static str {
        "string|\\DOMNode"
    }

    fn emit_css_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        context: &str,
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        let is_dry = writer.is_dry_run();
        let lowered: Vec<String> = queries
            .iter()
            .map(|q| lower_css_or_diag(q, span, source_file, is_dry, diagnostics))
            .collect();
        self.emit_xpath_select(prv, nxt, &lowered, next_is_to_bool, context, writer);
    }

    fn emit_css_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        let is_dry = writer.is_dry_run();
        let lowered: Vec<String> = queries
            .iter()
            .map(|q| lower_css_or_diag(q, span, source_file, is_dry, diagnostics))
            .collect();
        self.emit_xpath_select_all(prv, nxt, &lowered, writer);
    }

    fn emit_css_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        let is_dry = writer.is_dry_run();
        let xp = lower_css_or_diag(query, span, source_file, is_dry, diagnostics);
        self.emit_xpath_remove(prv, nxt, &xp, writer);
    }

    fn pred_css(
        &self,
        target: &str,
        query: &str,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> String {
        let xp = lower_css_or_diag(query, span, source_file, is_dry_run, diagnostics);
        self.pred_xpath(target, &xp)
    }
}

/// `Symfony\Component\DomCrawler\Crawler` spelling (`--html-lib dom-crawler`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhpDomCrawlerSpelling;

impl PhpDomCrawlerSpelling {
    /// Creates a new `PhpDomCrawlerSpelling`.
    pub const fn new() -> Self {
        Self
    }
}

impl PhpDomSpelling for PhpDomCrawlerSpelling {
    fn backend_name(&self) -> &'static str {
        "php-dom-crawler"
    }

    fn register_imports(&self, builder: &mut ModuleBuilder) {
        builder.require_import("use Symfony\\Component\\DomCrawler\\Crawler;");
    }

    fn document_type(&self) -> &'static str {
        "Crawler"
    }

    fn init_arg_type(&self) -> &'static str {
        "string|Crawler|\\DOMNode"
    }

    fn emit_css_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        context: &str,
        _span: Span,
        _source_file: &str,
        _diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        let q_arr = php_str_array(queries);
        if next_is_to_bool {
            wln!(writer, "${nxt} = Rt::cssFirstOpt(${prv}, {q_arr});");
        } else {
            let ctx_lit = php_str(context);
            wln!(writer, "${nxt} = Rt::cssFirst(${prv}, {q_arr}, {ctx_lit});");
        }
    }

    fn emit_css_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        _span: Span,
        _source_file: &str,
        _diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        let q_arr = php_str_array(queries);
        wln!(writer, "${nxt} = Rt::cssAll(${prv}, {q_arr});");
    }

    fn emit_css_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        _span: Span,
        _source_file: &str,
        _diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        let q_lit = php_str(query);
        wln!(writer, "${nxt} = Rt::cssRemove(${prv}, {q_lit});");
    }

    fn pred_css(
        &self,
        target: &str,
        query: &str,
        _span: Span,
        _source_file: &str,
        _is_dry_run: bool,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> String {
        format!("Rt::predCss({target}, {})", php_str(query))
    }
}

/// Dynamic dispatch wrapper over [`PhpNativeSpelling`] and [`PhpDomCrawlerSpelling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhpDom {
    /// Built-in `ext-dom` (`\DOMDocument` + `\DOMXPath` with CSS-to-XPath lowering).
    Native(PhpNativeSpelling),
    /// `Symfony\Component\DomCrawler\Crawler`.
    DomCrawler(PhpDomCrawlerSpelling),
}

impl Default for PhpDom {
    fn default() -> Self {
        Self::Native(PhpNativeSpelling)
    }
}

impl PhpDom {
    /// Constructs a `PhpDom` from [`CodegenOptions`].
    pub const fn from_options(options: &CodegenOptions) -> Self {
        match options.php_html_lib {
            PhpHtmlLib::Native => Self::Native(PhpNativeSpelling),
            PhpHtmlLib::DomCrawler => Self::DomCrawler(PhpDomCrawlerSpelling),
        }
    }
}

impl PhpDomSpelling for PhpDom {
    fn backend_name(&self) -> &'static str {
        match self {
            Self::Native(s) => s.backend_name(),
            Self::DomCrawler(s) => s.backend_name(),
        }
    }

    fn register_imports(&self, builder: &mut ModuleBuilder) {
        match self {
            Self::Native(s) => s.register_imports(builder),
            Self::DomCrawler(s) => s.register_imports(builder),
        }
    }

    fn document_type(&self) -> &'static str {
        match self {
            Self::Native(s) => s.document_type(),
            Self::DomCrawler(s) => s.document_type(),
        }
    }

    fn init_arg_type(&self) -> &'static str {
        match self {
            Self::Native(s) => s.init_arg_type(),
            Self::DomCrawler(s) => s.init_arg_type(),
        }
    }

    fn emit_css_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        context: &str,
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        match self {
            Self::Native(s) => s.emit_css_select(
                prv,
                nxt,
                queries,
                next_is_to_bool,
                context,
                span,
                source_file,
                diagnostics,
                writer,
            ),
            Self::DomCrawler(s) => s.emit_css_select(
                prv,
                nxt,
                queries,
                next_is_to_bool,
                context,
                span,
                source_file,
                diagnostics,
                writer,
            ),
        }
    }

    fn emit_css_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        match self {
            Self::Native(s) => {
                s.emit_css_select_all(prv, nxt, queries, span, source_file, diagnostics, writer)
            }
            Self::DomCrawler(s) => {
                s.emit_css_select_all(prv, nxt, queries, span, source_file, diagnostics, writer)
            }
        }
    }

    fn emit_css_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        span: Span,
        source_file: &str,
        diagnostics: &mut Vec<Diagnostic>,
        writer: &mut CodeWriter,
    ) {
        match self {
            Self::Native(s) => {
                s.emit_css_remove(prv, nxt, query, span, source_file, diagnostics, writer)
            }
            Self::DomCrawler(s) => {
                s.emit_css_remove(prv, nxt, query, span, source_file, diagnostics, writer)
            }
        }
    }

    fn pred_css(
        &self,
        target: &str,
        query: &str,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> String {
        match self {
            Self::Native(s) => {
                s.pred_css(target, query, span, source_file, is_dry_run, diagnostics)
            }
            Self::DomCrawler(s) => {
                s.pred_css(target, query, span, source_file, is_dry_run, diagnostics)
            }
        }
    }
}

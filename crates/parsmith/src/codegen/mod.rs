//! Multi-target code generation core (`CodeWriter`, `ModuleBuilder`, `LanguageEmitter`)
//! and target backends (`PythonEmitter`, etc.).

pub mod builder;
pub mod csharp;
pub mod elixir;
pub mod emitter;
pub mod go;
pub mod jsts;
pub mod kotlin;
pub mod php;
pub mod python;
pub mod rust;
pub mod writer;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use crate::kdl::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::ast::{Module, StructKind};
use crate::check::{CheckOptions, check_source_with_options, check_with_options};
use crate::diagnostic::{CheckResult, Diagnostic, DiagnosticCode, Severity};
use crate::linter::symbols::is_valid_symbol;
use crate::loader::{FsSourceLoader, SourceLoader};

pub use builder::{HelperDef, ModuleBuilder, emit_extension_call};
pub use csharp::{
    CSharpEmitter, emit_csharp_module, emit_csharp_runtime_module, module_has_xpath,
    validate_csharp_namespace,
};
pub use elixir::{
    ElixirEmitter, emit_elixir_module, emit_elixir_runtime_module, format_elixir_module_prefix,
    validate_elixir_package_name,
};
pub use emitter::{LanguageEmitter, drive_module, drive_single_pass, drive_struct};
pub use go::{GoEmitter, emit_go_module, emit_go_runtime_module, validate_go_package_name};
pub use jsts::{JsTsEmitter, JsTsMode, emit_dts_module, emit_js_module, emit_ts_module};
pub use kotlin::{
    KotlinEmitter, emit_kotlin_module, emit_kotlin_runtime_module, validate_kotlin_package_name,
};
pub use php::dom::{PhpDom, PhpDomCrawlerSpelling, PhpDomSpelling, PhpNativeSpelling};
pub use php::runtime::emit_php_runtime_module;
pub use php::{PhpEmitter, emit_php_module, normalize_php_namespace, validate_php_namespace};
pub use python::{
    Bs4Spelling, DomSpelling, LxmlSpelling, ParselSpelling, PyDomSpelling, PythonDom,
    PythonEmitter, SelectolaxSpelling, emit_python_module, emit_python_runtime_module,
};
pub use rust::{RustEmitter, emit_rust_module, emit_rust_runtime_module};
pub use writer::{CodeWriter, IntoCodeLine, WriteMode, wln};

/// Checks whether a compiled [`Module`] contains at least one HTML-parsing `struct` or `fn`.
pub fn module_has_html(module: &Module) -> bool {
    module
        .structs
        .iter()
        .any(|s| !matches!(s.kind, StructKind::Raw | StructKind::Rest))
        || module.functions.iter().any(|f| !f.is_raw)
}

/// Checks whether a compiled [`Module`] contains at least one `(table)struct`.
pub fn module_has_table(module: &Module) -> bool {
    module.structs.iter().any(|s| s.kind == StructKind::Table)
}

/// Checks whether a [`Module`] contains at least one `@request` or `(rest)struct`.
pub fn module_uses_http(module: &Module) -> bool {
    module
        .structs
        .iter()
        .any(|s| s.kind == StructKind::Rest || !s.requests.is_empty())
}

/// Checks whether a [`Module`] contains at least one `(rest)struct`.
pub fn module_has_rest(module: &Module) -> bool {
    module.structs.iter().any(|s| s.kind == StructKind::Rest)
}

/// Checks whether a [`Module`] contains at least one non-REST struct with `@request`.
pub fn module_has_fetch(module: &Module) -> bool {
    module
        .structs
        .iter()
        .any(|s| s.kind != StructKind::Rest && !s.requests.is_empty())
}

/// Default Python shared runtime module name for `-R / --separate-runtime`.
pub const DEFAULT_RUNTIME_NAME: &str = "parsmith_runtime";

/// Code generation target language (`python`, `ts`, `js`, `go`, `rust`, `elixir`, `php`, `csharp`, `kotlin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// Python 3.10+ (`bs4`, `TypedDict` / `dataclass`).
    #[default]
    #[serde(alias = "py")]
    Python,
    /// Standalone TypeScript (`.ts`).
    #[serde(rename = "ts", alias = "typescript")]
    TypeScript,
    /// Clean ESM JavaScript + companion TypeScript declarations (`.js` + `.d.ts`).
    #[serde(rename = "js", alias = "javascript")]
    JavaScript,
    /// Go 1.26+ (`goquery`, `net/http`, `gjson`).
    #[serde(rename = "go", alias = "golang")]
    Go,
    /// Rust (`dom_query` + `serde` + `reqwest`).
    #[serde(rename = "rust", alias = "rs")]
    Rust,
    /// Elixir 1.18+ (`Floki` + built-in `JSON` + OTP `:httpc` / `Req`).
    #[serde(rename = "elixir", alias = "ex")]
    Elixir,
    /// PHP 8.1+ (`ext-dom` / `symfony/dom-crawler` + `JsonSerializable` + `ext-curl` / `Guzzle`).
    #[serde(rename = "php")]
    Php,
    /// C# / .NET 8+ (`AngleSharp` + `System.Text.Json` + `HttpClient`).
    #[serde(rename = "csharp", alias = "cs")]
    CSharp,
    /// Kotlin / JVM (`org.jsoup:jsoup` + `kotlinx.serialization.json` + `java.net.http.HttpClient`).
    #[serde(rename = "kotlin", alias = "kt")]
    Kotlin,
}

impl Target {
    /// Returns `"python"`, `"ts"`, `"js"`, `"go"`, `"rust"`, `"elixir"`, `"php"`, `"csharp"`, or `"kotlin"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::TypeScript => "ts",
            Self::JavaScript => "js",
            Self::Go => "go",
            Self::Rust => "rust",
            Self::Elixir => "elixir",
            Self::Php => "php",
            Self::CSharp => "csharp",
            Self::Kotlin => "kotlin",
        }
    }

    /// Returns the canonical Pass 4 ecosystem name (`"python"`, `"javascript"`, `"go"`, `"rust"`, `"elixir"`, `"php"`, `"csharp"`, or `"kotlin"`).
    pub const fn ecosystem_name(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::TypeScript | Self::JavaScript => "javascript",
            Self::Go => "go",
            Self::Rust => "rust",
            Self::Elixir => "elixir",
            Self::Php => "php",
            Self::CSharp => "csharp",
            Self::Kotlin => "kotlin",
        }
    }

    /// Returns the primary file extension (without leading dot: `"py"`, `"ts"`, `"js"`, `"go"`, `"rs"`, `"ex"`, `"php"`, `"cs"`, `"kt"`).
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::TypeScript => "ts",
            Self::JavaScript => "js",
            Self::Go => "go",
            Self::Rust => "rs",
            Self::Elixir => "ex",
            Self::Php => "php",
            Self::CSharp => "cs",
            Self::Kotlin => "kt",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "python" | "py" => Ok(Self::Python),
            "typescript" | "ts" => Ok(Self::TypeScript),
            "javascript" | "js" => Ok(Self::JavaScript),
            "go" | "golang" => Ok(Self::Go),
            "rust" | "rs" => Ok(Self::Rust),
            "elixir" | "ex" => Ok(Self::Elixir),
            "php" => Ok(Self::Php),
            "csharp" | "cs" | "c#" => Ok(Self::CSharp),
            "kotlin" | "kt" => Ok(Self::Kotlin),
            other => Err(format!(
                "Invalid target '{other}'. Expected one of: python, ts, js, go, rust, elixir, php, csharp, kotlin"
            )),
        }
    }
}

/// Python HTML DOM extraction library (`--html-lib bs4|lxml|parsel|selectolax`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PythonHtmlLib {
    /// `BeautifulSoup4` (`bs4`, default).
    #[default]
    Bs4,
    /// `lxml.html` (`lxml`).
    Lxml,
    /// `parsel` (`Selector` / `SelectorList`).
    Parsel,
    /// `selectolax.lexbor` (`LexborHTMLParser` / `LexborNode`).
    #[serde(alias = "slax")]
    Selectolax,
}

impl PythonHtmlLib {
    /// Returns `"bs4"`, `"lxml"`, `"parsel"`, or `"selectolax"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bs4 => "bs4",
            Self::Lxml => "lxml",
            Self::Parsel => "parsel",
            Self::Selectolax => "selectolax",
        }
    }

    /// Returns `"python-bs4"`, `"python-lxml"`, `"python-parsel"`, or `"python-selectolax"`.
    pub const fn backend_name(self) -> &'static str {
        match self {
            Self::Bs4 => "python-bs4",
            Self::Lxml => "python-lxml",
            Self::Parsel => "python-parsel",
            Self::Selectolax => "python-selectolax",
        }
    }
}

impl fmt::Display for PythonHtmlLib {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PythonHtmlLib {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bs4" | "beautifulsoup" | "beautifulsoup4" => Ok(Self::Bs4),
            "lxml" => Ok(Self::Lxml),
            "parsel" => Ok(Self::Parsel),
            "selectolax" | "slax" => Ok(Self::Selectolax),
            other => Err(format!(
                "Invalid --html-lib '{other}'. Expected one of: bs4, lxml, parsel, selectolax, slax"
            )),
        }
    }
}

/// PHP HTML DOM extraction library (`--html-lib native|dom-crawler`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PhpHtmlLib {
    /// Zero-Composer built-in `ext-dom` (`\DOMDocument` + `\DOMXPath` via compile-time CSS-to-XPath lowering, default).
    #[default]
    #[serde(alias = "ext-dom", alias = "dom")]
    Native,
    /// `Symfony\Component\DomCrawler\Crawler` (`symfony/dom-crawler` + `symfony/css-selector`).
    #[serde(
        rename = "dom-crawler",
        alias = "dom_crawler",
        alias = "crawler",
        alias = "symfony"
    )]
    DomCrawler,
}

impl PhpHtmlLib {
    /// Returns `"native"` or `"dom-crawler"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::DomCrawler => "dom-crawler",
        }
    }

    /// Returns `"php-native"` or `"php-dom-crawler"`.
    pub const fn backend_name(self) -> &'static str {
        match self {
            Self::Native => "php-native",
            Self::DomCrawler => "php-dom-crawler",
        }
    }
}

impl fmt::Display for PhpHtmlLib {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PhpHtmlLib {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "native" | "ext-dom" | "ext_dom" | "dom" => Ok(Self::Native),
            "dom-crawler" | "dom_crawler" | "domcrawler" | "crawler" | "symfony" => {
                Ok(Self::DomCrawler)
            }
            other => Err(format!(
                "Invalid PHP --html-lib '{other}'. Expected one of: native, dom-crawler"
            )),
        }
    }
}

/// Parses a target name or HTML library shorthand into `(Target, Option<PythonHtmlLib>, Option<PhpHtmlLib>)`.
pub fn parse_target_with_libs(
    s: &str,
) -> Result<(Target, Option<PythonHtmlLib>, Option<PhpHtmlLib>), String> {
    let lower = s.trim().to_ascii_lowercase();
    match lower.as_str() {
        "python" | "py" => Ok((Target::Python, None, None)),
        "typescript" | "ts" => Ok((Target::TypeScript, None, None)),
        "javascript" | "js" => Ok((Target::JavaScript, None, None)),
        "go" | "golang" => Ok((Target::Go, None, None)),
        "rust" | "rs" => Ok((Target::Rust, None, None)),
        "elixir" | "ex" => Ok((Target::Elixir, None, None)),
        "php" => Ok((Target::Php, None, None)),
        "php-native" | "php-ext-dom" | "php-dom" => {
            Ok((Target::Php, None, Some(PhpHtmlLib::Native)))
        }
        "php-dom-crawler" | "php-dom_crawler" | "php-crawler" | "php-symfony" => {
            Ok((Target::Php, None, Some(PhpHtmlLib::DomCrawler)))
        }
        "csharp" | "cs" | "c#" => Ok((Target::CSharp, None, None)),
        "kotlin" | "kt" => Ok((Target::Kotlin, None, None)),
        "py-bs4"
        | "python-bs4"
        | "py-beautifulsoup"
        | "python-beautifulsoup"
        | "py-beautifulsoup4"
        | "python-beautifulsoup4" => Ok((Target::Python, Some(PythonHtmlLib::Bs4), None)),
        "py-lxml" | "python-lxml" => Ok((Target::Python, Some(PythonHtmlLib::Lxml), None)),
        "py-parsel" | "python-parsel" => Ok((Target::Python, Some(PythonHtmlLib::Parsel), None)),
        "py-selectolax" | "python-selectolax" | "py-slax" | "python-slax" => {
            Ok((Target::Python, Some(PythonHtmlLib::Selectolax), None))
        }
        other => Err(format!(
            "Invalid target '{other}'. Expected one of: python, ts, js, go, rust, elixir, php, csharp, kotlin, py-bs4, py-lxml, py-parsel, py-selectolax, py-slax, php-native, php-dom-crawler"
        )),
    }
}

/// Parses a target name (`"python"`, `"ts"`, `"js"`, `"go"`, `"rust"`, `"elixir"`, `"php"`, `"csharp"`, `"kotlin"`)
/// or HTML library shorthand (`"py-bs4"`, `"py-lxml"`, `"py-parsel"`, `"py-selectolax"`, `"php-native"`, `"php-dom-crawler"`, etc.).
pub fn parse_target_or_shorthand(s: &str) -> Result<(Target, Option<PythonHtmlLib>), String> {
    let (target, py_lib, _php_lib) = parse_target_with_libs(s)?;
    Ok((target, py_lib))
}

/// Python output model representation (`--models typeddict|dataclass`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PythonModels {
    /// Zero-overhead `TypedDict` classes returning plain `dict` values (default).
    #[default]
    TypedDict,
    /// `@dataclass(slots=True)` classes returning typed dataclass instances.
    Dataclass,
}

impl PythonModels {
    /// Returns `"typeddict"` or `"dataclass"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TypedDict => "typeddict",
            Self::Dataclass => "dataclass",
        }
    }
}

impl fmt::Display for PythonModels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PythonModels {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "typeddict" | "typed-dict" | "typed_dict" => Ok(Self::TypedDict),
            "dataclass" | "data-class" | "data_class" => Ok(Self::Dataclass),
            other => Err(format!(
                "Invalid --models '{other}'. Expected 'typeddict' or 'dataclass'"
            )),
        }
    }
}

/// Underlying HTML parser engine for `BeautifulSoup4` (`--bs4-parser`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Bs4Parser {
    /// `lxml` HTML parser (default).
    #[default]
    #[serde(rename = "lxml")]
    Lxml,
    /// Python standard library `html.parser`.
    #[serde(rename = "html.parser", alias = "html_parser", alias = "html-parser")]
    HtmlParser,
    /// `html5lib` parser.
    #[serde(rename = "html5lib")]
    Html5lib,
}

impl Bs4Parser {
    /// Returns `"lxml"`, `"html.parser"`, or `"html5lib"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lxml => "lxml",
            Self::HtmlParser => "html.parser",
            Self::Html5lib => "html5lib",
        }
    }
}

impl fmt::Display for Bs4Parser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Bs4Parser {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "lxml" => Ok(Self::Lxml),
            "html.parser" => Ok(Self::HtmlParser),
            "html5lib" => Ok(Self::Html5lib),
            other => Err(format!(
                "Invalid --bs4-parser '{other}'. Expected one of: lxml, html.parser, html5lib"
            )),
        }
    }
}

/// HTTP method generation mode (`--http-io both|sync|async`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpIo {
    /// Emit both synchronous (`fetch`) and asynchronous (`async_fetch`) HTTP methods (default).
    #[default]
    Both,
    /// Emit only synchronous (`fetch`) HTTP methods.
    Sync,
    /// Emit only asynchronous (`async_fetch`) HTTP methods.
    Async,
}

impl HttpIo {
    /// Returns `"both"`, `"sync"`, or `"async"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Sync => "sync",
            Self::Async => "async",
        }
    }
}

impl fmt::Display for HttpIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HttpIo {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "both" => Ok(Self::Both),
            "sync" => Ok(Self::Sync),
            "async" => Ok(Self::Async),
            other => Err(format!(
                "Invalid --http-io '{other}'. Expected one of: both, sync, async"
            )),
        }
    }
}

/// Configuration options for [`generate_path`], [`generate_source`], and [`generate_with_loader`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodegenOptions {
    /// Target language (`Python`, `TypeScript`, `JavaScript`, `Go`, `Rust`, `Elixir`, `Php`, `CSharp`, `Kotlin`).
    #[serde(default)]
    pub target: Target,
    /// Python HTML DOM extraction library (`Bs4`, `Lxml`, `Parsel`, `Selectolax`).
    #[serde(default)]
    pub html_lib: PythonHtmlLib,
    /// PHP HTML DOM extraction library (`Native` or `DomCrawler`).
    #[serde(default)]
    pub php_html_lib: PhpHtmlLib,
    /// Python model flavor (`TypedDict` or `Dataclass`).
    #[serde(default)]
    pub models: PythonModels,
    /// BeautifulSoup4 HTML parser feature engine (`Lxml`, `HtmlParser`, `Html5lib`).
    #[serde(default)]
    pub bs4_parser: Bs4Parser,
    /// HTTP I/O method emission mode (`Both`, `Sync`, `Async`).
    #[serde(default)]
    pub http_io: HttpIo,
    /// Whether to emit shared runtime helpers into a separate `<runtime_name>.py` file (`-R / --separate-runtime`).
    #[serde(default)]
    pub separate_runtime: bool,
    /// Module name for the separated runtime file (defaults to `"parsmith_runtime"`).
    #[serde(default = "default_runtime_name")]
    pub runtime_name: String,
    /// Go package name (defaults to `"main"`).
    #[serde(default = "default_go_package")]
    pub package: String,
    /// Optional explicit output file stem (e.g. `"my_parser"` -> `"my_parser.py"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_stem: Option<String>,
    /// Optional custom code block injected after imports and runtime utilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prologue: Option<String>,
    /// Optional custom code block appended at the end of the generated module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epilogue: Option<String>,
}

fn default_runtime_name() -> String {
    DEFAULT_RUNTIME_NAME.to_owned()
}

fn default_go_package() -> String {
    "main".to_owned()
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            target: Target::Python,
            html_lib: PythonHtmlLib::Bs4,
            php_html_lib: PhpHtmlLib::Native,
            models: PythonModels::TypedDict,
            bs4_parser: Bs4Parser::Lxml,
            http_io: HttpIo::Both,
            separate_runtime: false,
            runtime_name: DEFAULT_RUNTIME_NAME.to_owned(),
            package: "main".to_owned(),
            module_stem: None,
            prologue: None,
            epilogue: None,
        }
    }
}

impl CodegenOptions {
    /// Creates default `CodegenOptions` for `target`.
    pub fn new(target: Target) -> Self {
        Self {
            target,
            ..Self::default()
        }
    }

    /// Sets the target language.
    pub fn with_target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    /// Sets the Python HTML DOM extraction library (`Bs4`, `Lxml`, `Parsel`, `Selectolax`).
    pub fn with_html_lib(mut self, html_lib: PythonHtmlLib) -> Self {
        self.html_lib = html_lib;
        self
    }

    /// Sets the PHP HTML DOM extraction library (`Native` or `DomCrawler`).
    pub fn with_php_html_lib(mut self, php_html_lib: PhpHtmlLib) -> Self {
        self.php_html_lib = php_html_lib;
        self
    }

    /// Sets the Python model representation (`TypedDict` or `Dataclass`).
    pub fn with_models(mut self, models: PythonModels) -> Self {
        self.models = models;
        self
    }

    /// Sets the `bs4` HTML parser feature engine.
    pub fn with_bs4_parser(mut self, bs4_parser: Bs4Parser) -> Self {
        self.bs4_parser = bs4_parser;
        self
    }

    /// Sets the HTTP I/O emission mode (`Both`, `Sync`, `Async`).
    pub fn with_http_io(mut self, http_io: HttpIo) -> Self {
        self.http_io = http_io;
        self
    }

    /// Enables or disables separate runtime emission (`-R / --separate-runtime`).
    pub fn with_separate_runtime(mut self, separate_runtime: bool) -> Self {
        self.separate_runtime = separate_runtime;
        self
    }

    /// Sets the separate runtime module name (and enables `separate_runtime = true`).
    pub fn with_runtime_name(mut self, runtime_name: impl Into<String>) -> Self {
        self.runtime_name = runtime_name.into();
        self
    }

    /// Sets the Go package name (defaults to `"main"`).
    pub fn with_package(mut self, package: impl Into<String>) -> Self {
        self.package = package.into();
        self
    }

    /// Sets the output file stem (without extension).
    pub fn with_module_stem(mut self, stem: impl Into<String>) -> Self {
        self.module_stem = Some(stem.into());
        self
    }

    /// Sets the custom user prologue block.
    pub fn with_prologue(mut self, prologue: impl Into<String>) -> Self {
        self.prologue = Some(prologue.into());
        self
    }

    /// Sets the custom user epilogue block.
    pub fn with_epilogue(mut self, epilogue: impl Into<String>) -> Self {
        self.epilogue = Some(epilogue.into());
        self
    }
}

/// Result of a code generation run (`generate_path`, `generate_source`, `generate_with_loader`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerateResult {
    /// Generated output files keyed by relative filename (e.g. `"schema.py"`, `"parsmith_runtime.py"`).
    pub files: IndexMap<String, String>,
    /// Compiler and codegen diagnostics (`E000`–`E403`, `W011`–`W041`).
    pub diagnostics: Vec<Diagnostic>,
    /// Loaded KDL source texts keyed by path for human-readable diagnostic rendering.
    #[serde(skip)]
    pub sources: BTreeMap<String, String>,
}

impl GenerateResult {
    /// Returns `true` if any diagnostic has `Severity::Error`.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Returns `true` if any diagnostic has `Severity::Warning`.
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning)
    }

    /// Returns the source content of the primary generated parser file, if present.
    pub fn primary_code(&self) -> Option<&str> {
        self.files.values().next().map(String::as_str)
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    pub fn format_text(&self) -> String {
        CheckResult::new(self.diagnostics.clone())
            .with_sources(self.sources.clone())
            .format_text()
    }

    /// Formats all diagnostics as a JSON string.
    pub fn format_json(&self) -> String {
        CheckResult::new(self.diagnostics.clone()).format_json()
    }
}

/// Compiles and generates target code for a KDL 2.0 schema file on disk.
pub fn generate_path(path: impl AsRef<Path>, options: &CodegenOptions) -> GenerateResult {
    let loader = FsSourceLoader::new();
    let entry_path = path.as_ref().to_string_lossy();
    generate_with_loader(&entry_path, &loader, options)
}

/// Compiles and generates target code for an in-memory KDL 2.0 source string.
pub fn generate_source(
    source: &str,
    path: Option<&str>,
    options: &CodegenOptions,
) -> GenerateResult {
    let loader = FsSourceLoader::new();
    generate_source_with_loader(source, path, &loader, options)
}

/// Compiles and generates target code starting from `entry_path` using a custom [`SourceLoader`].
pub fn generate_with_loader(
    entry_path: &str,
    loader: &dyn SourceLoader,
    options: &CodegenOptions,
) -> GenerateResult {
    let check_opts = CheckOptions::new().with_targets([options.target.ecosystem_name()]);
    let check_res = check_with_options(entry_path, loader, &check_opts);
    finish_generation(check_res, Some(entry_path), options)
}

/// Compiles and generates target code for an in-memory `source` string at logical `path` using `loader`.
pub fn generate_source_with_loader(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
    options: &CodegenOptions,
) -> GenerateResult {
    let check_opts = CheckOptions::new().with_targets([options.target.ecosystem_name()]);
    let check_res = check_source_with_options(source, path, loader, &check_opts);
    finish_generation(check_res, path, options)
}

/// Generates target code directly from a pre-compiled [`Module`] AST.
pub fn generate_module(module: &Module, options: &CodegenOptions) -> GenerateResult {
    let mut diagnostics = Vec::new();
    let files = emit_files_for_module(module, None, options, &mut diagnostics);
    GenerateResult {
        files,
        diagnostics,
        sources: BTreeMap::new(),
    }
}

fn finish_generation(
    mut check_res: CheckResult,
    path_hint: Option<&str>,
    options: &CodegenOptions,
) -> GenerateResult {
    let sources = std::mem::take(&mut check_res.sources);
    let mut diagnostics = std::mem::take(&mut check_res.diagnostics);

    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return GenerateResult {
            files: IndexMap::new(),
            diagnostics,
            sources,
        };
    }

    let Some(module) = check_res.module.take() else {
        return GenerateResult {
            files: IndexMap::new(),
            diagnostics,
            sources,
        };
    };

    let files = emit_files_for_module(&module, path_hint, options, &mut diagnostics);
    GenerateResult {
        files,
        diagnostics,
        sources,
    }
}

fn resolve_module_stem(
    module: &Module,
    path_hint: Option<&str>,
    options: &CodegenOptions,
) -> String {
    if let Some(ref stem) = options.module_stem
        && !stem.is_empty()
    {
        return stem.clone();
    }
    let raw_path = path_hint.unwrap_or(&module.source_file);
    if !raw_path.is_empty()
        && let Some(stem) = Path::new(raw_path).file_stem().and_then(|s| s.to_str())
        && !stem.is_empty()
        && stem != "<memory>"
    {
        return stem.to_owned();
    }
    "schema".to_owned()
}

fn resolve_clean_runtime_name<'a>(
    ecosystem: &str,
    lang_label: &str,
    noun: &str,
    ext: &str,
    options: &'a CodegenOptions,
    source_file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a str> {
    let clean_rt = options
        .runtime_name
        .trim_start_matches('.')
        .trim_end_matches(ext);
    if options.separate_runtime && (clean_rt.is_empty() || !is_valid_symbol(ecosystem, clean_rt)) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E002,
                format!(
                    "invalid {lang_label} {noun} name '{}'",
                    options.runtime_name
                ),
                Span::EMPTY,
            )
            .with_path(source_file.to_owned())
            .with_hint(format!(
                "use a valid {lang_label} identifier (e.g. parsmith_runtime)"
            )),
        );
        return None;
    }
    Some(clean_rt)
}

fn emit_files_for_module(
    module: &Module,
    path_hint: Option<&str>,
    options: &CodegenOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> IndexMap<String, String> {
    let mut files = IndexMap::new();
    let stem = resolve_module_stem(module, path_hint, options);

    match options.target {
        Target::Python => {
            let Some(clean_rt) = resolve_clean_runtime_name(
                "python",
                "Python",
                "runtime module",
                ".py",
                options,
                &module.source_file,
                diagnostics,
            ) else {
                return files;
            };

            match emit_python_module(module, options) {
                Ok((parser_code, runtime_code, _builder)) => {
                    files.insert(format!("{stem}.py"), parser_code);
                    if let Some(rt_code) = runtime_code {
                        files.insert(format!("{clean_rt}.py"), rt_code);
                    }
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }
        Target::TypeScript => match emit_ts_module(module, options) {
            Ok((ts_code, _builder)) => {
                files.insert(format!("{stem}.ts"), ts_code);
            }
            Err(errs) => {
                diagnostics.extend(errs);
            }
        },
        Target::JavaScript => match emit_js_module(module, options) {
            Ok((js_code, dts_code, _builder)) => {
                files.insert(format!("{stem}.js"), js_code);
                files.insert(format!("{stem}.d.ts"), dts_code);
            }
            Err(errs) => {
                diagnostics.extend(errs);
            }
        },
        Target::Go => {
            if !validate_go_package_name(&options.package) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E002,
                        format!("invalid Go package name '{}'", options.package),
                        Span::EMPTY,
                    )
                    .with_path(module.source_file.clone())
                    .with_hint(
                        "use a valid non-keyword Go package identifier (e.g. main, parsers)",
                    ),
                );
                return files;
            }
            let Some(clean_rt) = resolve_clean_runtime_name(
                "go",
                "Go",
                "runtime",
                ".go",
                options,
                &module.source_file,
                diagnostics,
            ) else {
                return files;
            };

            match emit_go_module(module, options) {
                Ok((parser_code, runtime_code, _builder)) => {
                    files.insert(format!("{stem}.go"), parser_code);
                    if let Some(rt_code) = runtime_code {
                        files.insert(format!("{clean_rt}.go"), rt_code);
                    }
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }
        Target::Rust => {
            let Some(clean_rt) = resolve_clean_runtime_name(
                "rust",
                "Rust",
                "runtime module",
                ".rs",
                options,
                &module.source_file,
                diagnostics,
            ) else {
                return files;
            };

            match emit_rust_module(module, options) {
                Ok((parser_code, runtime_code, _builder)) => {
                    files.insert(format!("{stem}.rs"), parser_code);
                    if let Some(rt_code) = runtime_code {
                        files.insert(format!("{clean_rt}.rs"), rt_code);
                    }
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }
        Target::Elixir => {
            if !validate_elixir_package_name(&options.package) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E002,
                        format!("invalid Elixir module prefix '{}'", options.package),
                        Span::EMPTY,
                    )
                    .with_path(module.source_file.clone())
                    .with_hint(
                        "use valid dot-separated Elixir module segments (e.g. MyApp.Parsers)",
                    ),
                );
                return files;
            }
            let Some(clean_rt) = resolve_clean_runtime_name(
                "elixir",
                "Elixir",
                "runtime module",
                ".ex",
                options,
                &module.source_file,
                diagnostics,
            ) else {
                return files;
            };

            match emit_elixir_module(module, options) {
                Ok((parser_code, runtime_code, _builder)) => {
                    files.insert(format!("{stem}.ex"), parser_code);
                    if let Some(rt_code) = runtime_code {
                        files.insert(format!("{clean_rt}.ex"), rt_code);
                    }
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }
        Target::Php => {
            if !validate_php_namespace(&options.package) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E002,
                        format!("invalid PHP namespace '{}'", options.package),
                        Span::EMPTY,
                    )
                    .with_path(module.source_file.clone())
                    .with_hint("use a valid PHP namespace (e.g. App\\Parsers or App.Parsers)"),
                );
                return files;
            }
            let Some(clean_rt) = resolve_clean_runtime_name(
                "php",
                "PHP",
                "runtime module",
                ".php",
                options,
                &module.source_file,
                diagnostics,
            ) else {
                return files;
            };

            match emit_php_module(module, options) {
                Ok((parser_code, runtime_code, _builder)) => {
                    files.insert(format!("{stem}.php"), parser_code);
                    if let Some(rt_code) = runtime_code {
                        files.insert(format!("{clean_rt}.php"), rt_code);
                    }
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }
        Target::CSharp => {
            if !validate_csharp_namespace(&options.package) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E002,
                        format!("invalid C# namespace '{}'", options.package),
                        Span::EMPTY,
                    )
                    .with_path(module.source_file.clone())
                    .with_hint("use valid dot-separated C# identifiers (e.g. MyApp.Parsers)"),
                );
                return files;
            }
            let Some(clean_rt) = resolve_clean_runtime_name(
                "csharp",
                "C#",
                "runtime",
                ".cs",
                options,
                &module.source_file,
                diagnostics,
            ) else {
                return files;
            };

            match emit_csharp_module(module, options) {
                Ok((parser_code, runtime_code, _builder)) => {
                    files.insert(format!("{stem}.cs"), parser_code);
                    if let Some(rt_code) = runtime_code {
                        files.insert(format!("{clean_rt}.cs"), rt_code);
                    }
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }
        Target::Kotlin => {
            if !validate_kotlin_package_name(&options.package) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E002,
                        format!("invalid Kotlin package name '{}'", options.package),
                        Span::EMPTY,
                    )
                    .with_path(module.source_file.clone())
                    .with_hint(
                        "use a valid dot-separated Kotlin package name (e.g. com.example.parsers)",
                    ),
                );
                return files;
            }
            let Some(clean_rt) = resolve_clean_runtime_name(
                "kotlin",
                "Kotlin",
                "runtime",
                ".kt",
                options,
                &module.source_file,
                diagnostics,
            ) else {
                return files;
            };

            match emit_kotlin_module(module, options) {
                Ok((parser_code, runtime_code, _builder)) => {
                    files.insert(format!("{stem}.kt"), parser_code);
                    if let Some(rt_code) = runtime_code {
                        files.insert(format!("{clean_rt}.kt"), rt_code);
                    }
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }
    }

    files
}

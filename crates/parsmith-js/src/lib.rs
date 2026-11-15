//! Node-API (`napi-rs`) JavaScript/TypeScript bindings for `parsmith`.

pub mod inspect;

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

pub use inspect::*;

use napi::bindgen_prelude::*;
use napi::{Env, JsFunction, JsObject, JsUnknown, NapiRaw};
use napi_derive::napi;
use parsmith::{
    Bs4Parser, CodegenOptions as CoreCodegenOptions, HttpIo, MemorySourceLoader, PhpHtmlLib,
    PythonHtmlLib, PythonModels, Target, check_path_with_targets,
    check_source_with_loader_and_targets, check_source_with_targets, check_with_loader_and_targets,
    generate_path, generate_source, generate_source_with_loader, generate_with_loader,
    parse_target_with_libs,
};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

pub(crate) fn value_err(msg: impl Into<String>) -> Error {
    Error::new(Status::InvalidArg, msg.into())
}

pub(crate) fn type_err(env: &Env, msg: impl AsRef<str>) -> Error {
    let _ = env.throw_type_error(msg.as_ref(), None);
    Error::from_status(Status::PendingException)
}

pub(crate) fn parse_target_shorthand(
    s: &str,
) -> Result<(Target, Option<PythonHtmlLib>, Option<PhpHtmlLib>)> {
    parse_target_with_libs(s).map_err(value_err)
}

pub(crate) fn parse_html_lib(s: &str) -> Result<PythonHtmlLib> {
    PythonHtmlLib::from_str(s).map_err(value_err)
}

pub(crate) fn parse_php_html_lib(s: &str) -> Result<PhpHtmlLib> {
    PhpHtmlLib::from_str(s).map_err(value_err)
}

pub(crate) fn parse_models(s: &str) -> Result<PythonModels> {
    PythonModels::from_str(s).map_err(value_err)
}

pub(crate) fn parse_bs4_parser(s: &str) -> Result<Bs4Parser> {
    Bs4Parser::from_str(s).map_err(value_err)
}

pub(crate) fn parse_http_io(s: &str) -> Result<HttpIo> {
    HttpIo::from_str(s).map_err(value_err)
}

pub(crate) fn serialize_to_json_value<T: serde::Serialize>(value: &T) -> Result<JsonValue> {
    serde_json::to_value(value).map_err(|e| value_err(e.to_string()))
}

// ============================================================================
// Diagnostic & CheckResult
// ============================================================================

/// A structured compiler or linter diagnostic (`E000`–`E403`, `W011`–`W041`).
#[napi]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub(crate) inner: parsmith::Diagnostic,
    pub(crate) cached_source: Option<String>,
}

impl Diagnostic {
    pub(crate) fn from_core(
        diag: parsmith::Diagnostic,
        sources: &BTreeMap<String, String>,
    ) -> Self {
        let cached_source = sources.get(&diag.path).cloned();
        Self {
            inner: diag,
            cached_source,
        }
    }

    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "code": self.code(),
            "codeName": self.code_name(),
            "severity": self.severity(),
            "message": self.message(),
            "path": self.path(),
            "line": self.line(),
            "col": self.col(),
            "endLine": self.end_line(),
            "endCol": self.end_col(),
            "hint": self.hint(),
            "notes": self.notes(),
            "label": self.label(),
        })
    }

    pub(crate) fn to_snake_json(&self) -> JsonValue {
        json!({
            "code": self.code(),
            "code_name": self.code_name(),
            "severity": self.severity(),
            "message": self.message(),
            "path": self.path(),
            "line": self.line(),
            "col": self.col(),
            "end_line": self.end_line(),
            "end_col": self.end_col(),
            "hint": self.hint(),
            "notes": self.notes(),
            "label": self.label(),
        })
    }
}

#[napi]
impl Diagnostic {
    /// Canonical diagnostic code (e.g. `"E000"`, `"W011"`).
    #[napi(getter)]
    pub fn code(&self) -> String {
        self.inner.code.as_str().to_owned()
    }

    /// Symbolic name of the diagnostic code (e.g. `"SYNTAX_ERROR"`).
    #[napi(getter, js_name = "codeName")]
    pub fn code_name(&self) -> String {
        self.inner.code.name().to_owned()
    }

    /// Severity level (`"error"` or `"warning"`).
    #[napi(getter)]
    pub fn severity(&self) -> String {
        self.inner.severity.as_str().to_owned()
    }

    /// Human-readable diagnostic message.
    #[napi(getter)]
    pub fn message(&self) -> String {
        self.inner.message.clone()
    }

    /// Logical or filesystem path where the diagnostic originated (`null` if unattached).
    #[napi(getter)]
    pub fn path(&self) -> Option<String> {
        if self.inner.path.is_empty() {
            None
        } else {
            Some(self.inner.path.clone())
        }
    }

    /// 1-based starting line number (`0` when unattached).
    #[napi(getter)]
    pub fn line(&self) -> u32 {
        self.inner.line as u32
    }

    /// 1-based starting column number (`0` when unattached).
    #[napi(getter)]
    pub fn col(&self) -> u32 {
        self.inner.col as u32
    }

    /// 1-based ending line number.
    #[napi(getter, js_name = "endLine")]
    pub fn end_line(&self) -> u32 {
        self.inner.end_line as u32
    }

    /// 1-based ending column number.
    #[napi(getter, js_name = "endCol")]
    pub fn end_col(&self) -> u32 {
        self.inner.end_col as u32
    }

    /// Optional actionable hint explaining how to fix the issue.
    #[napi(getter)]
    pub fn hint(&self) -> Option<String> {
        self.inner.hint.clone()
    }

    /// Additional context notes attached to this diagnostic.
    #[napi(getter)]
    pub fn notes(&self) -> Vec<String> {
        self.inner.notes.clone()
    }

    /// Optional inline span label.
    #[napi(getter)]
    pub fn label(&self) -> Option<String> {
        self.inner.label.clone()
    }

    /// Returns `true` if this diagnostic has `"error"` severity.
    #[napi(getter, js_name = "isError")]
    pub fn is_error(&self) -> bool {
        self.inner.severity.is_error()
    }

    /// Returns `true` if this diagnostic has `"warning"` severity.
    #[napi(getter, js_name = "isWarning")]
    pub fn is_warning(&self) -> bool {
        self.inner.severity.is_warning()
    }

    /// Renders this diagnostic as a Rust-style annotated source snippet.
    #[napi]
    pub fn render(&self, source: Option<String>) -> String {
        let effective = source.as_deref().or(self.cached_source.as_deref());
        self.inner.render(effective)
    }

    /// Converts this diagnostic into a plain JS object with `camelCase` keys.
    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    /// Serializes this diagnostic for `JSON.stringify()`.
    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    /// Converts this diagnostic into a plain dictionary with `snake_case` keys matching Python's `to_dict()`.
    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> JsonValue {
        self.to_snake_json()
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        self.render(None)
    }
}

/// Result of running schema validation (`parsmith.check()`).
#[napi]
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub(crate) inner: parsmith::CheckResult,
}

#[napi]
impl CheckResult {
    /// All collected error and warning diagnostics in deterministic order.
    #[napi(getter)]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.inner
            .diagnostics
            .iter()
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// All error diagnostics (`severity === "error"`).
    #[napi(getter)]
    pub fn errors(&self) -> Vec<Diagnostic> {
        self.inner
            .errors()
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// All warning diagnostics (`severity === "warning"`).
    #[napi(getter)]
    pub fn warnings(&self) -> Vec<Diagnostic> {
        self.inner
            .warnings()
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Returns `true` if validation succeeded with zero error diagnostics.
    #[napi(getter)]
    pub fn ok(&self) -> bool {
        !self.inner.has_errors()
    }

    /// Number of error diagnostics.
    #[napi(getter, js_name = "errorCount")]
    pub fn error_count(&self) -> u32 {
        self.inner.error_count() as u32
    }

    /// Number of warning diagnostics.
    #[napi(getter, js_name = "warningCount")]
    pub fn warning_count(&self) -> u32 {
        self.inner.warning_count() as u32
    }

    /// Returns `true` if at least one error diagnostic was emitted.
    #[napi(js_name = "hasErrors")]
    pub fn has_errors(&self) -> bool {
        self.inner.has_errors()
    }

    /// Returns `true` if at least one warning diagnostic was emitted.
    #[napi(js_name = "hasWarnings")]
    pub fn has_warnings(&self) -> bool {
        self.inner.has_warnings()
    }

    /// Returns `true` if there are no error diagnostics (`!hasErrors()`).
    #[napi(js_name = "isOk")]
    pub fn is_ok(&self) -> bool {
        self.inner.is_ok()
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    #[napi(js_name = "formatText")]
    pub fn format_text(&self) -> String {
        self.inner.format_text()
    }

    /// Formats all diagnostics as a pretty-printed JSON string.
    #[napi(js_name = "formatJson")]
    pub fn format_json(&self) -> String {
        self.inner.format_json()
    }

    /// Converts this `CheckResult` into a plain JS object with `camelCase` keys.
    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        let diags: Vec<JsonValue> = self
            .diagnostics()
            .into_iter()
            .map(|d| d.to_camel_json())
            .collect();
        json!({
            "ok": self.ok(),
            "diagnostics": diags,
        })
    }

    /// Serializes this `CheckResult` for `JSON.stringify()`.
    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_object()
    }

    /// Converts this `CheckResult` into a plain dictionary with `snake_case` keys matching Python's `to_dict()`.
    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> JsonValue {
        let diags: Vec<JsonValue> = self
            .diagnostics()
            .into_iter()
            .map(|d| d.to_snake_json())
            .collect();
        json!({
            "ok": self.ok(),
            "diagnostics": diags,
        })
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "CheckResult(ok={}, errors={}, warnings={})",
            self.ok(),
            self.error_count(),
            self.warning_count()
        )
    }
}

// ============================================================================
// CodegenOptions & GenerateResult
// ============================================================================

/// Configuration options for `parsmith.generate()`.
#[napi]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOptions {
    pub(crate) inner: CoreCodegenOptions,
}

impl CodegenOptions {
    pub(crate) fn default_ts() -> CoreCodegenOptions {
        CoreCodegenOptions::new(Target::TypeScript)
    }

    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "target": self.target(),
            "htmlLib": self.html_lib(),
            "phpHtmlLib": self.php_html_lib(),
            "models": self.models(),
            "bs4Parser": self.bs4_parser(),
            "httpIo": self.http_io(),
            "separateRuntime": self.separate_runtime(),
            "runtimeName": self.runtime_name(),
            "package": self.package(),
            "moduleStem": self.module_stem(),
            "prologue": self.prologue(),
            "epilogue": self.epilogue(),
        })
    }
}

#[napi]
impl CodegenOptions {
    #[napi(constructor)]
    pub fn new(
        env: Env,
        target_or_options: Option<JsUnknown>,
        options: Option<JsUnknown>,
    ) -> Result<Self> {
        let parsed = parse_codegen_args(&env, target_or_options, options, false)?;
        Ok(Self { inner: parsed.opts })
    }

    #[napi(getter)]
    pub fn target(&self) -> String {
        self.inner.target.as_str().to_owned()
    }

    #[napi(setter)]
    pub fn set_target(&mut self, value: String) -> Result<()> {
        let (target, shorthand_py_lib, shorthand_php_lib) = parse_target_shorthand(&value)?;
        self.inner.target = target;
        if let Some(lib) = shorthand_py_lib {
            self.inner.html_lib = lib;
        }
        if let Some(php_lib) = shorthand_php_lib {
            self.inner.php_html_lib = php_lib;
        }
        Ok(())
    }

    #[napi(getter, js_name = "htmlLib")]
    pub fn html_lib(&self) -> String {
        self.inner.html_lib.as_str().to_owned()
    }

    #[napi(setter, js_name = "htmlLib")]
    pub fn set_html_lib(&mut self, value: String) -> Result<()> {
        self.inner.html_lib = parse_html_lib(&value)?;
        Ok(())
    }

    #[napi(getter, js_name = "phpHtmlLib")]
    pub fn php_html_lib(&self) -> String {
        self.inner.php_html_lib.as_str().to_owned()
    }

    #[napi(setter, js_name = "phpHtmlLib")]
    pub fn set_php_html_lib(&mut self, value: String) -> Result<()> {
        self.inner.php_html_lib = parse_php_html_lib(&value)?;
        Ok(())
    }

    #[napi(getter, js_name = "php_html_lib")]
    pub fn php_html_lib_snake(&self) -> String {
        self.inner.php_html_lib.as_str().to_owned()
    }

    #[napi(setter, js_name = "php_html_lib")]
    pub fn set_php_html_lib_snake(&mut self, value: String) -> Result<()> {
        self.inner.php_html_lib = parse_php_html_lib(&value)?;
        Ok(())
    }

    #[napi(getter)]
    pub fn models(&self) -> String {
        self.inner.models.as_str().to_owned()
    }

    #[napi(setter)]
    pub fn set_models(&mut self, value: String) -> Result<()> {
        self.inner.models = parse_models(&value)?;
        Ok(())
    }

    #[napi(getter, js_name = "bs4Parser")]
    pub fn bs4_parser(&self) -> String {
        self.inner.bs4_parser.as_str().to_owned()
    }

    #[napi(setter, js_name = "bs4Parser")]
    pub fn set_bs4_parser(&mut self, value: String) -> Result<()> {
        self.inner.bs4_parser = parse_bs4_parser(&value)?;
        Ok(())
    }

    #[napi(getter, js_name = "httpIo")]
    pub fn http_io(&self) -> String {
        self.inner.http_io.as_str().to_owned()
    }

    #[napi(setter, js_name = "httpIo")]
    pub fn set_http_io(&mut self, value: String) -> Result<()> {
        self.inner.http_io = parse_http_io(&value)?;
        Ok(())
    }

    #[napi(getter, js_name = "separateRuntime")]
    pub fn separate_runtime(&self) -> bool {
        self.inner.separate_runtime
    }

    #[napi(setter, js_name = "separateRuntime")]
    pub fn set_separate_runtime(&mut self, value: bool) {
        self.inner.separate_runtime = value;
    }

    #[napi(getter, js_name = "runtimeName")]
    pub fn runtime_name(&self) -> String {
        self.inner.runtime_name.clone()
    }

    #[napi(setter, js_name = "runtimeName")]
    pub fn set_runtime_name(&mut self, value: String) {
        self.inner.runtime_name = value;
    }

    #[napi(getter)]
    pub fn package(&self) -> String {
        self.inner.package.clone()
    }

    #[napi(setter)]
    pub fn set_package(&mut self, value: String) {
        self.inner.package = value;
    }

    #[napi(getter, js_name = "moduleStem")]
    pub fn module_stem(&self) -> Option<String> {
        self.inner.module_stem.clone()
    }

    #[napi(setter, js_name = "moduleStem")]
    pub fn set_module_stem(&mut self, value: Option<String>) {
        self.inner.module_stem = value;
    }

    #[napi(getter)]
    pub fn prologue(&self) -> Option<String> {
        self.inner.prologue.clone()
    }

    #[napi(setter)]
    pub fn set_prologue(&mut self, value: Option<String>) {
        self.inner.prologue = value;
    }

    #[napi(getter)]
    pub fn epilogue(&self) -> Option<String> {
        self.inner.epilogue.clone()
    }

    #[napi(setter)]
    pub fn set_epilogue(&mut self, value: Option<String>) {
        self.inner.epilogue = value;
    }

    /// Converts these options into a plain JS object with `camelCase` keys.
    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    /// Serializes these options for `JSON.stringify()`.
    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    /// Converts these options into a plain dictionary with `snake_case` keys matching Python's `to_dict()`.
    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "CodegenOptions(target={:?}, htmlLib={:?}, phpHtmlLib={:?}, models={:?}, bs4Parser={:?}, httpIo={:?}, separateRuntime={}, runtimeName={:?}, package={:?})",
            self.target(),
            self.html_lib(),
            self.php_html_lib(),
            self.models(),
            self.bs4_parser(),
            self.http_io(),
            self.separate_runtime(),
            self.runtime_name(),
            self.package(),
        )
    }
}

/// Result of running code generation (`parsmith.generate()`).
#[napi]
#[derive(Debug, Clone)]
pub struct GenerateResult {
    pub(crate) inner: parsmith::GenerateResult,
}

#[napi]
impl GenerateResult {
    /// Generated files keyed by relative filename (e.g. `{"schema.ts": "..."}`).
    #[napi(getter)]
    pub fn files(&self) -> JsonValue {
        let mut map = JsonMap::new();
        for (k, v) in &self.inner.files {
            map.insert(k.clone(), JsonValue::String(v.clone()));
        }
        JsonValue::Object(map)
    }

    /// Compiler and codegen diagnostics (`E000`–`E403`, `W011`–`W041`).
    #[napi(getter)]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.inner
            .diagnostics
            .iter()
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Error diagnostics (`severity === "error"`).
    #[napi(getter)]
    pub fn errors(&self) -> Vec<Diagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_error())
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Warning diagnostics (`severity === "warning"`).
    #[napi(getter)]
    pub fn warnings(&self) -> Vec<Diagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_warning())
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Returns `true` if generation succeeded with zero error diagnostics.
    #[napi(getter)]
    pub fn ok(&self) -> bool {
        !self.inner.has_errors()
    }

    /// Primary generated file content, if any.
    #[napi(getter)]
    pub fn code(&self) -> Option<String> {
        self.inner.primary_code().map(str::to_owned)
    }

    /// Returns the source content of the primary generated file, if present.
    #[napi(js_name = "primaryCode")]
    pub fn primary_code(&self) -> Option<String> {
        self.inner.primary_code().map(str::to_owned)
    }

    /// Looks up a generated file by relative name (returns `null` if not found).
    #[napi(js_name = "getFile")]
    pub fn get_file(&self, name: String) -> Option<String> {
        self.inner.files.get(&name).cloned()
    }

    /// Returns `true` if a generated file with `name` exists in `files`.
    #[napi(js_name = "hasFile")]
    pub fn has_file(&self, name: String) -> bool {
        self.inner.files.contains_key(&name)
    }

    /// Returns `true` if any diagnostic has `"error"` severity.
    #[napi(js_name = "hasErrors")]
    pub fn has_errors(&self) -> bool {
        self.inner.has_errors()
    }

    /// Returns `true` if any diagnostic has `"warning"` severity.
    #[napi(js_name = "hasWarnings")]
    pub fn has_warnings(&self) -> bool {
        self.inner.has_warnings()
    }

    /// Returns `true` if there are no error diagnostics (`!hasErrors()`).
    #[napi(js_name = "isOk")]
    pub fn is_ok(&self) -> bool {
        !self.inner.has_errors()
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    #[napi(js_name = "formatText")]
    pub fn format_text(&self) -> String {
        self.inner.format_text()
    }

    /// Formats all diagnostics as a pretty-printed JSON string.
    #[napi(js_name = "formatJson")]
    pub fn format_json(&self) -> String {
        self.inner.format_json()
    }

    /// Converts this `GenerateResult` into a plain JS object with `camelCase` keys.
    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        let diags: Vec<JsonValue> = self
            .diagnostics()
            .into_iter()
            .map(|d| d.to_camel_json())
            .collect();
        json!({
            "ok": self.ok(),
            "files": self.files(),
            "diagnostics": diags,
        })
    }

    /// Serializes this `GenerateResult` for `JSON.stringify()`.
    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_object()
    }

    /// Converts this `GenerateResult` into a plain dictionary with `snake_case` keys matching Python's `to_dict()`.
    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> JsonValue {
        let diags: Vec<JsonValue> = self
            .diagnostics()
            .into_iter()
            .map(|d| d.to_snake_json())
            .collect();
        json!({
            "ok": self.ok(),
            "files": self.files(),
            "diagnostics": diags,
        })
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        let file_names: Vec<&str> = self.inner.files.keys().map(String::as_str).collect();
        format!(
            "GenerateResult(ok={}, files={:?}, diagnostics={})",
            self.ok(),
            file_names,
            self.inner.diagnostics.len()
        )
    }
}

// ============================================================================
// Input Resolution Helpers (`sourceOrPath` & `sources` VFS & Options)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOrPath {
    Path(String),
    Source(String),
}

pub fn is_single_line_kdl_filename(s: &str) -> bool {
    !s.contains('\n') && !s.contains('\r') && !s.contains('{') && s.trim().ends_with(".kdl")
}

pub fn resolve_input_mode(
    raw_str: &str,
    is_pathlike: bool,
    has_vfs: Option<&MemorySourceLoader>,
) -> SourceOrPath {
    if let Some(loader) = has_vfs {
        if loader.get(raw_str).is_some() || is_pathlike || is_single_line_kdl_filename(raw_str) {
            SourceOrPath::Path(raw_str.to_owned())
        } else {
            SourceOrPath::Source(raw_str.to_owned())
        }
    } else if is_pathlike
        || (!raw_str.contains('\n')
            && !raw_str.contains('\r')
            && !raw_str.contains('{')
            && (Path::new(raw_str).exists() || raw_str.trim().ends_with(".kdl")))
    {
        SourceOrPath::Path(raw_str.to_owned())
    } else {
        SourceOrPath::Source(raw_str.to_owned())
    }
}

pub(crate) fn extract_raw_source_or_path(env: &Env, val: JsUnknown) -> Result<(String, bool)> {
    match val.get_type()? {
        ValueType::String => {
            let s: String = unsafe { String::from_napi_value(env.raw(), val.raw())? };
            Ok((s, false))
        }
        ValueType::Object => {
            let obj = val.coerce_to_object()?;
            if obj.has_named_property("__parsmithPath__")? {
                let p: String = obj.get_named_property("__parsmithPath__")?;
                return Ok((p, true));
            }
            Err(type_err(
                env,
                "expected string or file: URL for sourceOrPath",
            ))
        }
        _ => Err(type_err(
            env,
            "expected string or file: URL for sourceOrPath",
        )),
    }
}

pub(crate) fn extract_sources_map(
    env: &Env,
    val: Option<JsUnknown>,
) -> Result<Option<MemorySourceLoader>> {
    let Some(src_val) = val else {
        return Ok(None);
    };
    match src_val.get_type()? {
        ValueType::Undefined | ValueType::Null => Ok(None),
        ValueType::Object => {
            let obj = src_val.coerce_to_object()?;
            let mut loader = MemorySourceLoader::new();

            // Check if it is a JS Map (has callable `entries` and `get` methods)
            let is_map = obj.get_named_property::<JsFunction>("entries").is_ok()
                && obj.get_named_property::<JsFunction>("get").is_ok();

            if is_map {
                let entries_fn: JsFunction = obj.get_named_property("entries")?;
                let iter: JsObject = entries_fn
                    .call_without_args(Some(&obj))?
                    .coerce_to_object()?;
                let next_fn: JsFunction = iter.get_named_property("next")?;
                loop {
                    let step: JsObject =
                        next_fn.call_without_args(Some(&iter))?.coerce_to_object()?;
                    let done: bool = step.get_named_property("done")?;
                    if done {
                        break;
                    }
                    let pair: JsObject = step.get_named_property("value")?;
                    let k_unk: JsUnknown = pair.get_element(0)?;
                    let v_unk: JsUnknown = pair.get_element(1)?;
                    let (k_str, _) = extract_raw_source_or_path(env, k_unk)
                        .map_err(|_| type_err(env, "sources keys must be string or file: URL"))?;
                    if v_unk.get_type()? != ValueType::String {
                        return Err(type_err(env, "sources values must be strings"));
                    }
                    let v_str: String = unsafe { String::from_napi_value(env.raw(), v_unk.raw())? };
                    loader.insert(k_str, v_str);
                }
                return Ok(Some(loader));
            }

            let keys = JsObject::keys(&obj)?;
            for key in keys {
                let v_unk: JsUnknown = obj.get_named_property(&key)?;
                if v_unk.get_type()? != ValueType::String {
                    return Err(type_err(env, "sources values must be strings"));
                }
                let v_str: String = unsafe { String::from_napi_value(env.raw(), v_unk.raw())? };
                loader.insert(key, v_str);
            }
            Ok(Some(loader))
        }
        _ => Err(type_err(
            env,
            "sources must be a Record<string, string> or Map<string, string>",
        )),
    }
}

pub(crate) fn extract_targets(env: &Env, val: Option<JsUnknown>) -> Result<Option<Vec<String>>> {
    let Some(t_val) = val else {
        return Ok(None);
    };
    match t_val.get_type()? {
        ValueType::Undefined | ValueType::Null => Ok(None),
        ValueType::String => {
            let s: String = unsafe { String::from_napi_value(env.raw(), t_val.raw())? };
            Ok(Some(vec![s]))
        }
        ValueType::Object => {
            if !t_val.is_array()? {
                return Err(type_err(
                    env,
                    "targets must be a string or an array of strings",
                ));
            }
            let arr = t_val.coerce_to_object()?;
            let len = arr.get_array_length()?;
            let mut vec = Vec::with_capacity(len as usize);
            for i in 0..len {
                let item: JsUnknown = arr.get_element(i)?;
                if item.get_type()? != ValueType::String {
                    return Err(type_err(env, "targets elements must be strings"));
                }
                let s: String = unsafe { String::from_napi_value(env.raw(), item.raw())? };
                vec.push(s);
            }
            if vec.is_empty() {
                Ok(None)
            } else {
                Ok(Some(vec))
            }
        }
        _ => Err(type_err(
            env,
            "targets must be a string or an array of strings",
        )),
    }
}

pub(crate) fn extract_opt_string(
    env: &Env,
    val: JsUnknown,
    field_name: &str,
) -> Result<Option<String>> {
    match val.get_type()? {
        ValueType::Undefined | ValueType::Null => Ok(None),
        ValueType::String => {
            let s: String = unsafe { String::from_napi_value(env.raw(), val.raw())? };
            Ok(Some(s))
        }
        _ => Err(type_err(env, format!("'{field_name}' must be a string"))),
    }
}

fn extract_req_string(env: &Env, val: JsUnknown, field_name: &str) -> Result<String> {
    match val.get_type()? {
        ValueType::String => unsafe { String::from_napi_value(env.raw(), val.raw()) },
        _ => Err(type_err(env, format!("'{field_name}' must be a string"))),
    }
}

fn extract_req_bool(env: &Env, val: JsUnknown, field_name: &str) -> Result<bool> {
    match val.get_type()? {
        ValueType::Boolean => unsafe { bool::from_napi_value(env.raw(), val.raw()) },
        _ => Err(type_err(env, format!("'{field_name}' must be a boolean"))),
    }
}

pub(crate) struct ParsedCodegenCall {
    pub(crate) opts: CoreCodegenOptions,
    pub(crate) options_had_html_lib: bool,
    pub(crate) options_had_php_html_lib: bool,
    pub(crate) pending_html_lib: Option<String>,
    pub(crate) vfs_loader: Option<MemorySourceLoader>,
    pub(crate) logical_path: Option<String>,
}

impl Default for ParsedCodegenCall {
    fn default() -> Self {
        Self {
            opts: CodegenOptions::default_ts(),
            options_had_html_lib: false,
            options_had_php_html_lib: false,
            pending_html_lib: None,
            vfs_loader: None,
            logical_path: None,
        }
    }
}

pub(crate) fn apply_options_from_js_object(
    env: &Env,
    obj: &JsObject,
    state: &mut ParsedCodegenCall,
    allow_call_context_keys: bool,
) -> Result<()> {
    if let Ok(cg_ref) =
        unsafe { <&CodegenOptions as FromNapiValue>::from_napi_value(env.raw(), obj.raw()) }
    {
        state.opts = cg_ref.inner.clone();
        state.options_had_html_lib = true;
        state.options_had_php_html_lib = true;
        return Ok(());
    }

    let keys = JsObject::keys(obj)?;
    for key in keys {
        let v_unk: JsUnknown = obj.get_named_property(&key)?;
        if matches!(v_unk.get_type()?, ValueType::Undefined) {
            continue;
        }
        match key.as_str() {
            "target" => {
                let s = extract_req_string(env, v_unk, "target")?;
                let (parsed_target, shorthand_py_lib, shorthand_php_lib) =
                    parse_target_shorthand(&s)?;
                state.opts.target = parsed_target;
                if let Some(lib) = shorthand_py_lib
                    && !state.options_had_html_lib
                {
                    state.opts.html_lib = lib;
                }
                if let Some(php_lib) = shorthand_php_lib
                    && !state.options_had_php_html_lib
                {
                    state.opts.php_html_lib = php_lib;
                }
            }
            "htmlLib" | "html_lib" | "html-lib" | "lib" => {
                let s = extract_req_string(env, v_unk, &key)?;
                state.pending_html_lib = Some(s);
                state.options_had_html_lib = true;
            }
            "phpHtmlLib" | "php_html_lib" | "php-html-lib" => {
                let s = extract_req_string(env, v_unk, &key)?;
                state.opts.php_html_lib = parse_php_html_lib(&s)?;
                state.options_had_php_html_lib = true;
            }
            "models" => {
                let s = extract_req_string(env, v_unk, "models")?;
                state.opts.models = parse_models(&s)?;
            }
            "bs4Parser" | "bs4_parser" | "bs4-parser" => {
                let s = extract_req_string(env, v_unk, &key)?;
                state.opts.bs4_parser = parse_bs4_parser(&s)?;
            }
            "httpIo" | "http_io" | "http-io" => {
                let s = extract_req_string(env, v_unk, &key)?;
                state.opts.http_io = parse_http_io(&s)?;
            }
            "separateRuntime" | "separate_runtime" | "separate-runtime" => {
                state.opts.separate_runtime = extract_req_bool(env, v_unk, &key)?;
            }
            "runtimeName" | "runtime_name" | "runtime-name" => {
                state.opts.runtime_name = extract_req_string(env, v_unk, &key)?;
            }
            "package" | "namespace" => {
                state.opts.package = extract_req_string(env, v_unk, &key)?;
            }
            "moduleStem" | "module_stem" | "module-stem" => {
                state.opts.module_stem = extract_opt_string(env, v_unk, &key)?;
            }
            "prologue" => {
                state.opts.prologue = extract_opt_string(env, v_unk, "prologue")?;
            }
            "epilogue" => {
                state.opts.epilogue = extract_opt_string(env, v_unk, "epilogue")?;
            }
            "options" if allow_call_context_keys => {
                if !matches!(v_unk.get_type()?, ValueType::Null | ValueType::Undefined) {
                    if v_unk.get_type()? != ValueType::Object {
                        return Err(type_err(
                            env,
                            "options must be a CodegenOptions instance or an object",
                        ));
                    }
                    let nested_obj = v_unk.coerce_to_object()?;
                    apply_options_from_js_object(env, &nested_obj, state, false)?;
                }
            }
            "sources" if allow_call_context_keys => {
                state.vfs_loader = extract_sources_map(env, Some(v_unk))?;
            }
            "path" if allow_call_context_keys => {
                state.logical_path = extract_opt_string(env, v_unk, "path")?;
            }
            other => {
                return Err(value_err(format!("Unknown codegen option '{other}'")));
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_codegen_args(
    env: &Env,
    target_or_options: Option<JsUnknown>,
    options: Option<JsUnknown>,
    allow_call_context_keys: bool,
) -> Result<ParsedCodegenCall> {
    let mut state = ParsedCodegenCall::default();
    let mut explicit_target: Option<String> = None;
    let mut first_was_object = false;

    if let Some(first) = target_or_options {
        match first.get_type()? {
            ValueType::Undefined | ValueType::Null => {}
            ValueType::String => {
                let s: String = unsafe { String::from_napi_value(env.raw(), first.raw())? };
                explicit_target = Some(s);
            }
            ValueType::Object => {
                first_was_object = true;
                let obj = first.coerce_to_object()?;
                apply_options_from_js_object(env, &obj, &mut state, allow_call_context_keys)?;
            }
            _ => {
                return Err(type_err(
                    env,
                    "targetOrOptions must be a target string, CodegenOptions, or options object",
                ));
            }
        }
    }

    if let Some(second) = options {
        match second.get_type()? {
            ValueType::Undefined | ValueType::Null => {}
            ValueType::Object => {
                if first_was_object && !allow_call_context_keys {
                    return Err(type_err(
                        env,
                        "cannot pass two options objects to CodegenOptions",
                    ));
                }
                let obj = second.coerce_to_object()?;
                apply_options_from_js_object(env, &obj, &mut state, allow_call_context_keys)?;
            }
            _ => {
                return Err(type_err(
                    env,
                    "options must be a CodegenOptions instance or an object",
                ));
            }
        }
    }

    if let Some(t_str) = explicit_target {
        let (parsed_target, shorthand_py_lib, shorthand_php_lib) = parse_target_shorthand(&t_str)?;
        state.opts.target = parsed_target;
        if let Some(lib) = shorthand_py_lib
            && !state.options_had_html_lib
        {
            state.opts.html_lib = lib;
        }
        if let Some(php_lib) = shorthand_php_lib
            && !state.options_had_php_html_lib
        {
            state.opts.php_html_lib = php_lib;
        }
    }

    if let Some(ref hl) = state.pending_html_lib {
        if state.opts.target == Target::Php && !state.options_had_php_html_lib {
            if let Ok(pl) = parse_php_html_lib(hl) {
                state.opts.php_html_lib = pl;
            } else {
                state.opts.html_lib = parse_html_lib(hl)?;
            }
        } else {
            state.opts.html_lib = parse_html_lib(hl)?;
        }
    }

    Ok(state)
}

// ============================================================================
// Top-Level SDK Functions (`version`, `check`, `generate`, `jsonToKdl`)
// ============================================================================

/// Returns the version of the `parsmith` engine.
#[napi]
pub fn version() -> String {
    parsmith::version().to_owned()
}

/// Validates a KDL 2.0 schema (from a file path, `file:` URL, inline KDL string, or in-memory VFS `sources` map).
#[napi]
pub fn check(
    env: Env,
    source_or_path: JsUnknown,
    options: Option<JsUnknown>,
) -> Result<CheckResult> {
    let (raw_str, is_pathlike) = extract_raw_source_or_path(&env, source_or_path)?;
    let mut vfs_loader: Option<MemorySourceLoader> = None;
    let mut targets_vec: Option<Vec<String>> = None;
    let mut logical_path: Option<String> = None;

    if let Some(opt_val) = options {
        match opt_val.get_type()? {
            ValueType::Undefined | ValueType::Null => {}
            ValueType::Object => {
                let obj = opt_val.coerce_to_object()?;
                let keys = JsObject::keys(&obj)?;
                for key in keys {
                    let v_unk: JsUnknown = obj.get_named_property(&key)?;
                    if matches!(v_unk.get_type()?, ValueType::Undefined) {
                        continue;
                    }
                    match key.as_str() {
                        "sources" => {
                            vfs_loader = extract_sources_map(&env, Some(v_unk))?;
                        }
                        "targets" => {
                            targets_vec = extract_targets(&env, Some(v_unk))?;
                        }
                        "path" => {
                            logical_path = extract_opt_string(&env, v_unk, "path")?;
                        }
                        other => {
                            return Err(value_err(format!("Unknown check option '{other}'")));
                        }
                    }
                }
            }
            _ => return Err(type_err(&env, "check options must be an object")),
        }
    }

    let mode = resolve_input_mode(&raw_str, is_pathlike, vfs_loader.as_ref());
    let res = match (mode, vfs_loader) {
        (SourceOrPath::Path(entry), Some(loader)) => {
            check_with_loader_and_targets(&entry, &loader, targets_vec)
        }
        (SourceOrPath::Source(src), Some(loader)) => {
            let lp = logical_path.as_deref().unwrap_or("<memory>.kdl");
            check_source_with_loader_and_targets(&src, Some(lp), &loader, targets_vec)
        }
        (SourceOrPath::Path(entry), None) => check_path_with_targets(&entry, targets_vec),
        (SourceOrPath::Source(src), None) => {
            check_source_with_targets(&src, logical_path.as_deref(), targets_vec)
        }
    };

    Ok(CheckResult { inner: res })
}

/// Compiles a KDL 2.0 schema and generates target code (`"ts"` default, `"js"`, `"python"`, `"go"`, or `"rust"`).
#[napi]
pub fn generate(
    env: Env,
    source_or_path: JsUnknown,
    target_or_options: Option<JsUnknown>,
    options: Option<JsUnknown>,
) -> Result<GenerateResult> {
    let (raw_str, is_pathlike) = extract_raw_source_or_path(&env, source_or_path)?;
    let parsed = parse_codegen_args(&env, target_or_options, options, true)?;
    let opts = parsed.opts;
    let vfs_loader = parsed.vfs_loader;
    let logical_path = parsed.logical_path;

    let mode = resolve_input_mode(&raw_str, is_pathlike, vfs_loader.as_ref());
    let res = match (mode, vfs_loader) {
        (SourceOrPath::Path(entry), Some(loader)) => generate_with_loader(&entry, &loader, &opts),
        (SourceOrPath::Source(src), Some(loader)) => {
            let lp = logical_path.as_deref().unwrap_or("<memory>.kdl");
            generate_source_with_loader(&src, Some(lp), &loader, &opts)
        }
        (SourceOrPath::Path(entry), None) => generate_path(&entry, &opts),
        (SourceOrPath::Source(src), None) => generate_source(&src, logical_path.as_deref(), &opts),
    };

    Ok(GenerateResult { inner: res })
}

/// Bootstraps a KDL v2.1 `json` schema string from a sample JSON payload.
#[napi(js_name = "jsonToKdl")]
pub fn json_to_kdl(
    env: Env,
    json_input: String,
    name_or_options: Option<JsUnknown>,
) -> Result<String> {
    let mut name = "JsonResponse".to_owned();
    if let Some(arg) = name_or_options {
        match arg.get_type()? {
            ValueType::Undefined | ValueType::Null => {}
            ValueType::String => {
                name = unsafe { String::from_napi_value(env.raw(), arg.raw())? };
            }
            ValueType::Object => {
                let obj = arg.coerce_to_object()?;
                let keys = JsObject::keys(&obj)?;
                for key in keys {
                    let v_unk: JsUnknown = obj.get_named_property(&key)?;
                    if matches!(v_unk.get_type()?, ValueType::Undefined) {
                        continue;
                    }
                    match key.as_str() {
                        "name" => {
                            if let Some(s) = extract_opt_string(&env, v_unk, "name")? {
                                name = s;
                            }
                        }
                        other => {
                            return Err(value_err(format!("Unknown jsonToKdl option '{other}'")));
                        }
                    }
                }
            }
            _ => {
                return Err(type_err(
                    &env,
                    "second argument to jsonToKdl must be a string or an options object",
                ));
            }
        }
    }
    parsmith::json_to_kdl(&json_input, &name).map_err(|e| value_err(e.to_string()))
}

/// Snake_case alias for `jsonToKdl`.
#[napi(js_name = "json_to_kdl")]
pub fn json_to_kdl_alias(
    env: Env,
    json_input: String,
    name_or_options: Option<JsUnknown>,
) -> Result<String> {
    json_to_kdl(env, json_input, name_or_options)
}

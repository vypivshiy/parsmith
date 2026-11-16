//! PyO3 Python bindings for `parsmith`.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use parsmith::{
    Bs4Parser, CheckOptions, CodegenOptions, ErrorConditionValue, FsSourceLoader, HttpIo,
    MemorySourceLoader, PhpHtmlLib, PythonHtmlLib, PythonModels, Target, check_path_with_targets,
    check_paths_with_targets, check_source_with_loader_and_targets, check_source_with_options,
    check_source_with_targets, check_with_loader_and_targets, check_with_options, generate_path,
    generate_paths, generate_source, generate_source_with_loader, generate_with_loader,
    inspect_module, parse_target_with_libs,
};
use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyList, PyMapping, PySequence, PyString};

fn json_value_to_py(py: Python<'_>, val: &serde_json::Value) -> PyResult<PyObject> {
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.to_owned().into_any().unbind())
            } else if let Some(u) = n.as_u64() {
                Ok(u.into_pyobject(py)?.to_owned().into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.to_owned().into_any().unbind())
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Array(arr) => {
            let mut items = Vec::with_capacity(arr.len());
            for item in arr {
                items.push(json_value_to_py(py, item)?);
            }
            Ok(PyList::new(py, items)?.into_any().unbind())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_value_to_py(py, v)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

fn serialize_to_py_dict<T: serde::Serialize>(py: Python<'_>, value: &T) -> PyResult<PyObject> {
    let val = serde_json::to_value(value).map_err(|e| PyValueError::new_err(e.to_string()))?;
    json_value_to_py(py, &val)
}

fn parse_target_shorthand(
    s: &str,
) -> PyResult<(Target, Option<PythonHtmlLib>, Option<PhpHtmlLib>)> {
    parse_target_with_libs(s).map_err(PyValueError::new_err)
}

fn parse_html_lib(s: &str) -> PyResult<PythonHtmlLib> {
    PythonHtmlLib::from_str(s).map_err(PyValueError::new_err)
}

fn parse_php_html_lib(s: &str) -> PyResult<PhpHtmlLib> {
    PhpHtmlLib::from_str(s).map_err(PyValueError::new_err)
}

fn parse_models(s: &str) -> PyResult<PythonModels> {
    PythonModels::from_str(s).map_err(PyValueError::new_err)
}

fn parse_bs4_parser(s: &str) -> PyResult<Bs4Parser> {
    Bs4Parser::from_str(s).map_err(PyValueError::new_err)
}

fn parse_http_io(s: &str) -> PyResult<HttpIo> {
    HttpIo::from_str(s).map_err(PyValueError::new_err)
}

// ============================================================================
// Diagnostic & CheckResult
// ============================================================================

/// A structured compiler or linter diagnostic (`E000`–`E403`, `W011`–`W041`).
#[pyclass(name = "Diagnostic")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyDiagnostic {
    pub(crate) inner: parsmith::Diagnostic,
    pub(crate) cached_source: Option<String>,
}

impl PyDiagnostic {
    fn from_core(diag: parsmith::Diagnostic, sources: &BTreeMap<String, String>) -> PyDiagnostic {
        let cached_source = sources.get(&diag.path).cloned();
        Self {
            inner: diag,
            cached_source,
        }
    }
}

#[pymethods]
impl PyDiagnostic {
    /// Canonical diagnostic code (e.g. `"E000"`, `"W011"`).
    #[getter]
    pub fn code(&self) -> &str {
        self.inner.code.as_str()
    }

    /// Symbolic name of the diagnostic code (e.g. `"SYNTAX_ERROR"`).
    #[getter]
    pub fn code_name(&self) -> &str {
        self.inner.code.name()
    }

    /// Severity level (`"error"` or `"warning"`).
    #[getter]
    pub fn severity(&self) -> &str {
        self.inner.severity.as_str()
    }

    /// Human-readable diagnostic message.
    #[getter]
    pub fn message(&self) -> &str {
        &self.inner.message
    }

    /// Logical or filesystem path where the diagnostic originated (`None` if unattached).
    #[getter]
    pub fn path(&self) -> Option<&str> {
        if self.inner.path.is_empty() {
            None
        } else {
            Some(&self.inner.path)
        }
    }

    /// 1-based starting line number (`0` when unattached).
    #[getter]
    pub fn line(&self) -> usize {
        self.inner.line
    }

    /// 1-based starting column number (`0` when unattached).
    #[getter]
    pub fn col(&self) -> usize {
        self.inner.col
    }

    /// 1-based ending line number.
    #[getter]
    pub fn end_line(&self) -> usize {
        self.inner.end_line
    }

    /// 1-based ending column number.
    #[getter]
    pub fn end_col(&self) -> usize {
        self.inner.end_col
    }

    /// Optional actionable hint explaining how to fix the issue.
    #[getter]
    pub fn hint(&self) -> Option<&str> {
        self.inner.hint.as_deref()
    }

    /// Additional context notes attached to this diagnostic.
    #[getter]
    pub fn notes(&self) -> Vec<String> {
        self.inner.notes.clone()
    }

    /// Optional inline span label.
    #[getter]
    pub fn label(&self) -> Option<&str> {
        self.inner.label.as_deref()
    }

    /// Returns `True` if this diagnostic has `"error"` severity.
    #[getter]
    pub fn is_error(&self) -> bool {
        self.inner.severity.is_error()
    }

    /// Returns `True` if this diagnostic has `"warning"` severity.
    #[getter]
    pub fn is_warning(&self) -> bool {
        self.inner.severity.is_warning()
    }

    /// Renders this diagnostic as a Rust-style annotated source snippet.
    #[pyo3(signature = (source = None))]
    pub fn render(&self, source: Option<&str>) -> String {
        let effective = source.or(self.cached_source.as_deref());
        self.inner.render(effective)
    }

    /// Converts this diagnostic into a plain Python `dict`.
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("code", self.code())?;
        dict.set_item("code_name", self.code_name())?;
        dict.set_item("severity", self.severity())?;
        dict.set_item("message", self.message())?;
        dict.set_item("path", self.path())?;
        dict.set_item("line", self.line())?;
        dict.set_item("col", self.col())?;
        dict.set_item("end_line", self.end_line())?;
        dict.set_item("end_col", self.end_col())?;
        dict.set_item("hint", self.hint())?;
        dict.set_item("notes", self.notes())?;
        dict.set_item("label", self.label())?;
        Ok(dict.into_any().unbind())
    }

    pub fn __repr__(&self) -> String {
        format!(
            "Diagnostic(code={:?}, severity={:?}, line={}, col={}, message={:?})",
            self.code(),
            self.severity(),
            self.line(),
            self.col(),
            self.message()
        )
    }

    pub fn __str__(&self) -> String {
        self.render(None)
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Result of running schema validation (`parsmith.check()`).
#[pyclass(name = "CheckResult")]
#[derive(Debug, Clone)]
pub struct PyCheckResult {
    pub(crate) inner: parsmith::CheckResult,
}

#[pymethods]
impl PyCheckResult {
    /// All collected error and warning diagnostics in deterministic order.
    #[getter]
    pub fn diagnostics(&self) -> Vec<PyDiagnostic> {
        self.inner
            .diagnostics
            .iter()
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// All error diagnostics (`severity == "error"`).
    #[getter]
    pub fn errors(&self) -> Vec<PyDiagnostic> {
        self.inner
            .errors()
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// All warning diagnostics (`severity == "warning"`).
    #[getter]
    pub fn warnings(&self) -> Vec<PyDiagnostic> {
        self.inner
            .warnings()
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Returns `True` if validation succeeded with zero error diagnostics.
    #[getter]
    pub fn ok(&self) -> bool {
        !self.inner.has_errors()
    }

    /// Number of error diagnostics.
    #[getter]
    pub fn error_count(&self) -> usize {
        self.inner.error_count()
    }

    /// Number of warning diagnostics.
    #[getter]
    pub fn warning_count(&self) -> usize {
        self.inner.warning_count()
    }

    /// Returns `True` if at least one error diagnostic was emitted.
    pub fn has_errors(&self) -> bool {
        self.inner.has_errors()
    }

    /// Returns `True` if at least one warning diagnostic was emitted.
    pub fn has_warnings(&self) -> bool {
        self.inner.has_warnings()
    }

    /// Returns `True` if there are no error diagnostics (`!has_errors()`).
    pub fn is_ok(&self) -> bool {
        self.inner.is_ok()
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    pub fn format_text(&self) -> String {
        self.inner.format_text()
    }

    /// Formats all diagnostics as a pretty-printed JSON string.
    pub fn format_json(&self) -> String {
        self.inner.format_json()
    }

    /// Converts this `CheckResult` into a plain Python `dict`.
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("ok", self.ok())?;
        let diags = PyList::empty(py);
        for d in self.diagnostics() {
            diags.append(d.to_dict(py)?)?;
        }
        dict.set_item("diagnostics", diags)?;
        Ok(dict.into_any().unbind())
    }

    pub fn __bool__(&self) -> bool {
        !self.inner.has_errors()
    }

    pub fn __repr__(&self) -> String {
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
#[pyclass(name = "CodegenOptions")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyCodegenOptions {
    pub(crate) inner: CodegenOptions,
}

#[pymethods]
impl PyCodegenOptions {
    #[new]
    #[pyo3(signature = (
        target = "python",
        *,
        html_lib = None,
        php_html_lib = None,
        models = "typeddict",
        bs4_parser = "lxml",
        http_io = "both",
        separate_runtime = false,
        runtime_name = None,
        package = "main",
        module_stem = None,
        prologue = None,
        epilogue = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: &str,
        html_lib: Option<&str>,
        php_html_lib: Option<&str>,
        models: &str,
        bs4_parser: &str,
        http_io: &str,
        separate_runtime: bool,
        runtime_name: Option<&str>,
        package: &str,
        module_stem: Option<String>,
        prologue: Option<String>,
        epilogue: Option<String>,
    ) -> PyResult<Self> {
        let (parsed_target, shorthand_py_lib, shorthand_php_lib) = parse_target_shorthand(target)?;
        let mut resolved_html_lib = shorthand_py_lib.unwrap_or(PythonHtmlLib::Bs4);
        let mut resolved_php_html_lib = shorthand_php_lib.unwrap_or(PhpHtmlLib::Native);

        if let Some(phl) = php_html_lib {
            resolved_php_html_lib = parse_php_html_lib(phl)?;
        }
        if let Some(hl) = html_lib {
            if parsed_target == Target::Php && php_html_lib.is_none() {
                if let Ok(pl) = parse_php_html_lib(hl) {
                    resolved_php_html_lib = pl;
                } else {
                    resolved_html_lib = parse_html_lib(hl)?;
                }
            } else {
                resolved_html_lib = parse_html_lib(hl)?;
            }
        }

        let mut inner = CodegenOptions::new(parsed_target)
            .with_html_lib(resolved_html_lib)
            .with_php_html_lib(resolved_php_html_lib)
            .with_models(parse_models(models)?)
            .with_bs4_parser(parse_bs4_parser(bs4_parser)?)
            .with_http_io(parse_http_io(http_io)?)
            .with_separate_runtime(separate_runtime)
            .with_package(package);
        if let Some(rn) = runtime_name {
            inner = inner.with_runtime_name(rn);
        }
        inner.module_stem = module_stem;
        inner.prologue = prologue;
        inner.epilogue = epilogue;
        Ok(Self { inner })
    }

    #[getter]
    pub fn target(&self) -> &str {
        self.inner.target.as_str()
    }

    #[setter]
    pub fn set_target(&mut self, value: &str) -> PyResult<()> {
        let (target, shorthand_py_lib, shorthand_php_lib) = parse_target_shorthand(value)?;
        self.inner = self.inner.clone().with_target(target);
        if let Some(lib) = shorthand_py_lib {
            self.inner.html_lib = lib;
        }
        if let Some(php_lib) = shorthand_php_lib {
            self.inner.php_html_lib = php_lib;
        }
        Ok(())
    }

    #[getter]
    pub fn html_lib(&self) -> &str {
        self.inner.html_lib.as_str()
    }

    #[setter]
    pub fn set_html_lib(&mut self, value: &str) -> PyResult<()> {
        self.inner.html_lib = parse_html_lib(value)?;
        Ok(())
    }

    #[getter]
    pub fn php_html_lib(&self) -> &str {
        self.inner.php_html_lib.as_str()
    }

    #[setter]
    pub fn set_php_html_lib(&mut self, value: &str) -> PyResult<()> {
        self.inner.php_html_lib = parse_php_html_lib(value)?;
        Ok(())
    }

    #[getter]
    pub fn models(&self) -> &str {
        self.inner.models.as_str()
    }

    #[setter]
    pub fn set_models(&mut self, value: &str) -> PyResult<()> {
        self.inner.models = parse_models(value)?;
        Ok(())
    }

    #[getter]
    pub fn bs4_parser(&self) -> &str {
        self.inner.bs4_parser.as_str()
    }

    #[setter]
    pub fn set_bs4_parser(&mut self, value: &str) -> PyResult<()> {
        self.inner.bs4_parser = parse_bs4_parser(value)?;
        Ok(())
    }

    #[getter]
    pub fn http_io(&self) -> &str {
        self.inner.http_io.as_str()
    }

    #[setter]
    pub fn set_http_io(&mut self, value: &str) -> PyResult<()> {
        self.inner.http_io = parse_http_io(value)?;
        Ok(())
    }

    #[getter]
    pub fn separate_runtime(&self) -> bool {
        self.inner.separate_runtime
    }

    #[setter]
    pub fn set_separate_runtime(&mut self, value: bool) {
        self.inner.separate_runtime = value;
    }

    #[getter]
    pub fn runtime_name(&self) -> &str {
        &self.inner.runtime_name
    }

    #[setter]
    pub fn set_runtime_name(&mut self, value: String) {
        self.inner.runtime_name = value;
    }

    #[getter]
    pub fn package(&self) -> &str {
        &self.inner.package
    }

    #[setter]
    pub fn set_package(&mut self, value: String) {
        self.inner.package = value;
    }

    #[getter]
    pub fn module_stem(&self) -> Option<&str> {
        self.inner.module_stem.as_deref()
    }

    #[setter]
    pub fn set_module_stem(&mut self, value: Option<String>) {
        self.inner.module_stem = value;
    }

    #[getter]
    pub fn prologue(&self) -> Option<&str> {
        self.inner.prologue.as_deref()
    }

    #[setter]
    pub fn set_prologue(&mut self, value: Option<String>) {
        self.inner.prologue = value;
    }

    #[getter]
    pub fn epilogue(&self) -> Option<&str> {
        self.inner.epilogue.as_deref()
    }

    #[setter]
    pub fn set_epilogue(&mut self, value: Option<String>) {
        self.inner.epilogue = value;
    }

    /// Converts these options into a plain Python `dict`.
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "CodegenOptions(target={:?}, html_lib={:?}, php_html_lib={:?}, models={:?}, bs4_parser={:?}, http_io={:?}, separate_runtime={}, runtime_name={:?}, package={:?})",
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

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Result of running code generation (`parsmith.generate()`).
#[pyclass(name = "GenerateResult")]
#[derive(Debug, Clone)]
pub struct PyGenerateResult {
    pub(crate) inner: parsmith::GenerateResult,
}

#[pymethods]
impl PyGenerateResult {
    /// Generated files keyed by relative filename (e.g. `{"schema.py": "..."}`).
    #[getter]
    pub fn files(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        for (k, v) in &self.inner.files {
            dict.set_item(k, v)?;
        }
        Ok(dict.into_any().unbind())
    }

    /// Compiler and codegen diagnostics (`E000`–`E403`, `W011`–`W041`).
    #[getter]
    pub fn diagnostics(&self) -> Vec<PyDiagnostic> {
        self.inner
            .diagnostics
            .iter()
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Error diagnostics (`severity == "error"`).
    #[getter]
    pub fn errors(&self) -> Vec<PyDiagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_error())
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Warning diagnostics (`severity == "warning"`).
    #[getter]
    pub fn warnings(&self) -> Vec<PyDiagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_warning())
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.inner.sources))
            .collect()
    }

    /// Returns `True` if generation succeeded with zero error diagnostics.
    #[getter]
    pub fn ok(&self) -> bool {
        !self.inner.has_errors()
    }

    /// Primary generated file content, if any.
    #[getter]
    pub fn code(&self) -> Option<&str> {
        self.inner.primary_code()
    }

    /// Returns the source content of the primary generated file, if present.
    pub fn primary_code(&self) -> Option<&str> {
        self.inner.primary_code()
    }

    /// Returns `True` if any diagnostic has `"error"` severity.
    pub fn has_errors(&self) -> bool {
        self.inner.has_errors()
    }

    /// Returns `True` if any diagnostic has `"warning"` severity.
    pub fn has_warnings(&self) -> bool {
        self.inner.has_warnings()
    }

    /// Returns `True` if there are no error diagnostics (`!has_errors()`).
    pub fn is_ok(&self) -> bool {
        !self.inner.has_errors()
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    pub fn format_text(&self) -> String {
        self.inner.format_text()
    }

    /// Formats all diagnostics as a pretty-printed JSON string.
    pub fn format_json(&self) -> String {
        self.inner.format_json()
    }

    /// Converts this `GenerateResult` into a plain Python `dict`.
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("ok", self.ok())?;
        dict.set_item("files", self.files(py)?)?;
        let diags = PyList::empty(py);
        for d in self.diagnostics() {
            diags.append(d.to_dict(py)?)?;
        }
        dict.set_item("diagnostics", diags)?;
        Ok(dict.into_any().unbind())
    }

    pub fn __bool__(&self) -> bool {
        !self.inner.has_errors()
    }

    pub fn __getitem__(&self, key: &str) -> PyResult<String> {
        self.inner
            .files
            .get(key)
            .cloned()
            .ok_or_else(|| PyKeyError::new_err(format!("file '{key}' not found in GenerateResult")))
    }

    pub fn __contains__(&self, key: &str) -> bool {
        self.inner.files.contains_key(key)
    }

    pub fn __repr__(&self) -> String {
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
// Schema Introspection (`ModuleInfo` & Structured Child Classes)
// ============================================================================

/// Complete type descriptor for struct fields, function signatures, and JSON fields.
#[pyclass(name = "TypeInfo")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyTypeInfo {
    pub(crate) inner: parsmith::TypeInfo,
}

#[pymethods]
impl PyTypeInfo {
    /// Lowercase base type category (`"auto"`, `"document"`, `"string"`, `"int"`, `"float"`, `"bool"`, `"null"`, `"nested"`, `"json"`, `"dict"`).
    #[getter]
    pub fn base(&self) -> String {
        self.inner.base.dsl_name().to_ascii_lowercase()
    }

    /// Uppercase DSL type category (`"AUTO"`, `"DOCUMENT"`, `"STRING"`, `"INT"`, `"FLOAT"`, `"BOOL"`, `"NULL"`, `"NESTED"`, `"JSON"`, `"DICT"`).
    #[getter]
    pub fn dsl_name(&self) -> &str {
        self.inner.base.dsl_name()
    }

    /// Whether this type represents a list/array (`list[T]` / `T[]`).
    #[getter]
    pub fn is_array(&self) -> bool {
        self.inner.is_array
    }

    /// Whether this type is nullable/optional (`T | None` / `T | null`).
    #[getter]
    pub fn is_optional(&self) -> bool {
        self.inner.is_optional
    }

    /// Whether this type represents a dynamic dictionary (`dict[K, V]` / `Record<K, V>`).
    #[getter]
    pub fn is_dict(&self) -> bool {
        self.inner.is_dict
    }

    /// Referenced `struct` or `json` schema name, if applicable.
    #[getter]
    pub fn reference(&self) -> Option<&str> {
        self.inner.ref_name()
    }

    /// Dictionary key type when `is_dict` is `True`.
    #[getter]
    pub fn key_type(&self) -> Option<PyTypeInfo> {
        self.inner
            .key_type
            .as_deref()
            .cloned()
            .map(|inner| PyTypeInfo { inner })
    }

    /// Dictionary value type when `is_dict` is `True`.
    #[getter]
    pub fn value_type(&self) -> Option<PyTypeInfo> {
        self.inner
            .value_type
            .as_deref()
            .cloned()
            .map(|inner| PyTypeInfo { inner })
    }

    /// Whether the JSON field carries `@omitempty`.
    #[getter]
    pub fn omitempty(&self) -> bool {
        self.inner.omitempty
    }

    /// Whether the JSON field carries `@skip`.
    #[getter]
    pub fn skip(&self) -> bool {
        self.inner.skip
    }

    /// Converts this `TypeInfo` into a plain Python `dict`.
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "TypeInfo(base={:?}, is_array={}, is_optional={}, reference={:?})",
            self.base(),
            self.is_array(),
            self.is_optional(),
            self.reference()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a struct output field.
#[pyclass(name = "FieldInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyFieldInfo {
    pub(crate) inner: parsmith::FieldInfo,
}

#[pymethods]
impl PyFieldInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn type_info(&self) -> PyTypeInfo {
        PyTypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    #[getter]
    pub fn has_fallback(&self) -> bool {
        self.inner.has_fallback
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "FieldInfo(name={:?}, type_info={}, has_fallback={})",
            self.name(),
            self.type_info().__repr__(),
            self.has_fallback()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a precomputed `@init` field.
#[pyclass(name = "InitFieldInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyInitFieldInfo {
    pub(crate) inner: parsmith::InitFieldInfo,
}

#[pymethods]
impl PyInitFieldInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn type_info(&self) -> PyTypeInfo {
        PyTypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "InitFieldInfo(name={:?}, type_info={})",
            self.name(),
            self.type_info().__repr__()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a `@check` validation method.
#[pyclass(name = "CheckMethodInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyCheckMethodInfo {
    pub(crate) inner: parsmith::CheckMethodInfo,
}

#[pymethods]
impl PyCheckMethodInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn return_type(&self) -> PyTypeInfo {
        PyTypeInfo {
            inner: self.inner.return_type.clone(),
        }
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "CheckMethodInfo(name={:?}, return_type={})",
            self.name(),
            self.return_type().__repr__()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a `@request` parameter placeholder (`{{name:type[]?|style}}`).
#[pyclass(name = "PlaceholderInfo")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyPlaceholderInfo {
    pub(crate) inner: parsmith::PlaceholderSpec,
}

#[pymethods]
impl PyPlaceholderInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn type_name(&self) -> &str {
        self.inner.type_name.as_str()
    }

    #[getter]
    pub fn is_array(&self) -> bool {
        self.inner.is_array
    }

    #[getter]
    pub fn is_optional(&self) -> bool {
        self.inner.is_optional
    }

    #[getter]
    pub fn style(&self) -> Option<&str> {
        self.inner.style.map(|s| s.as_str())
    }

    #[getter]
    pub fn token(&self) -> String {
        self.inner.to_token()
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "PlaceholderInfo(name={:?}, type_name={:?}, is_array={}, is_optional={}, style={:?})",
            self.name(),
            self.type_name(),
            self.is_array(),
            self.is_optional(),
            self.style()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for an HTTP `@request` declaration.
#[pyclass(name = "RequestInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyRequestInfo {
    pub(crate) inner: parsmith::RequestInfo,
}

#[pymethods]
impl PyRequestInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn method(&self) -> &str {
        &self.inner.method
    }

    #[getter]
    pub fn url(&self) -> &str {
        &self.inner.url
    }

    #[getter]
    pub fn doc(&self) -> Option<&str> {
        self.inner.doc.as_deref()
    }

    #[getter]
    pub fn response_schema(&self) -> Option<&str> {
        self.inner.response_schema.as_deref()
    }

    #[getter]
    pub fn response_path(&self) -> Option<&str> {
        self.inner.response_path.as_deref()
    }

    #[getter]
    pub fn response_join(&self) -> Option<&str> {
        self.inner.response_join.as_deref()
    }

    #[getter]
    pub fn result_alias_name(&self) -> Option<&str> {
        self.inner.result_alias_name.as_deref()
    }

    #[getter]
    pub fn body_kind(&self) -> &str {
        self.inner.body_kind.as_str()
    }

    #[getter]
    pub fn placeholders(&self) -> Vec<PyPlaceholderInfo> {
        self.inner
            .placeholders
            .iter()
            .cloned()
            .map(|inner| PyPlaceholderInfo { inner })
            .collect()
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "RequestInfo(name={:?}, method={:?}, url={:?}, body_kind={:?})",
            self.name(),
            self.method(),
            self.url(),
            self.body_kind()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a typed `(rest)struct` endpoint declaration.
#[pyclass(name = "RestEndpointInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyRestEndpointInfo {
    pub(crate) inner: parsmith::RestEndpointDecl,
}

#[pymethods]
impl PyRestEndpointInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn method(&self) -> &str {
        &self.inner.spec.method
    }

    #[getter]
    pub fn url(&self) -> String {
        self.inner.spec.url.source()
    }

    #[getter]
    pub fn doc(&self) -> Option<&str> {
        self.inner.doc.as_deref()
    }

    #[getter]
    pub fn response_schema(&self) -> Option<&str> {
        self.inner.response_schema.as_deref()
    }

    #[getter]
    pub fn response_path(&self) -> Option<&str> {
        self.inner.response_path.as_deref()
    }

    #[getter]
    pub fn response_is_array(&self) -> bool {
        self.inner.response_is_array
    }

    #[getter]
    pub fn result_alias_name(&self) -> &str {
        &self.inner.result_alias_name
    }

    #[getter]
    pub fn body_kind(&self) -> &str {
        self.inner.spec.body_kind.as_str()
    }

    #[getter]
    pub fn placeholders(&self) -> Vec<PyPlaceholderInfo> {
        self.inner
            .placeholders
            .iter()
            .cloned()
            .map(|inner| PyPlaceholderInfo { inner })
            .collect()
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("name", self.name())?;
        dict.set_item("method", self.method())?;
        dict.set_item("url", self.url())?;
        dict.set_item("doc", self.doc())?;
        dict.set_item("response_schema", self.response_schema())?;
        dict.set_item("response_path", self.response_path())?;
        dict.set_item("response_is_array", self.response_is_array())?;
        dict.set_item("result_alias_name", self.result_alias_name())?;
        dict.set_item("body_kind", self.body_kind())?;
        let ph_list = PyList::empty(py);
        for ph in self.placeholders() {
            ph_list.append(ph.to_dict(py)?)?;
        }
        dict.set_item("placeholders", ph_list)?;
        Ok(dict.into_any().unbind())
    }

    pub fn __repr__(&self) -> String {
        format!(
            "RestEndpointInfo(name={:?}, method={:?}, url={:?}, result_alias_name={:?})",
            self.name(),
            self.method(),
            self.url(),
            self.result_alias_name()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for an `@error` variant on a `(rest)struct`.
#[pyclass(name = "ErrorVariantInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyErrorVariantInfo {
    pub(crate) inner: parsmith::ErrorVariantInfo,
}

#[pymethods]
impl PyErrorVariantInfo {
    #[getter]
    pub fn variant_name(&self) -> &str {
        &self.inner.variant_name
    }

    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.variant_name
    }

    #[getter]
    pub fn status(&self) -> u16 {
        self.inner.status
    }

    #[getter]
    pub fn schema_name(&self) -> &str {
        &self.inner.schema_name
    }

    #[getter]
    pub fn suffix(&self) -> &str {
        &self.inner.suffix
    }

    #[getter]
    pub fn required_keys(&self) -> Vec<String> {
        self.inner.required_keys.clone()
    }

    #[getter]
    pub fn conditions(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        for (k, v) in &self.inner.conditions {
            let py_val: PyObject = match v {
                ErrorConditionValue::String(s) => {
                    s.into_pyobject(py)?.to_owned().into_any().unbind()
                }
                ErrorConditionValue::Int(i) => i.into_pyobject(py)?.to_owned().into_any().unbind(),
                ErrorConditionValue::Float(f) => {
                    f.into_pyobject(py)?.to_owned().into_any().unbind()
                }
                ErrorConditionValue::Bool(b) => b.into_pyobject(py)?.to_owned().into_any().unbind(),
                ErrorConditionValue::Null => py.None(),
            };
            dict.set_item(k, py_val)?;
        }
        Ok(dict.into_any().unbind())
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "ErrorVariantInfo(variant_name={:?}, status={}, schema_name={:?}, suffix={:?})",
            self.variant_name(),
            self.status(),
            self.schema_name(),
            self.suffix()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a single `struct` declaration.
#[pyclass(name = "StructInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyStructInfo {
    pub(crate) inner: parsmith::StructInfo,
    pub(crate) rest_endpoints: Vec<parsmith::RestEndpointDecl>,
}

#[pymethods]
impl PyStructInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn kind(&self) -> &str {
        self.inner.kind.as_str()
    }

    #[getter]
    pub fn doc(&self) -> Option<&str> {
        self.inner.doc.as_deref()
    }

    #[getter]
    pub fn keep_order(&self) -> bool {
        self.inner.keep_order
    }

    #[getter]
    pub fn fields(&self) -> Vec<PyFieldInfo> {
        self.inner
            .fields
            .iter()
            .cloned()
            .map(|inner| PyFieldInfo { inner })
            .collect()
    }

    #[getter]
    pub fn init_fields(&self) -> Vec<PyInitFieldInfo> {
        self.inner
            .init_fields
            .iter()
            .cloned()
            .map(|inner| PyInitFieldInfo { inner })
            .collect()
    }

    #[getter]
    pub fn checks(&self) -> Vec<PyCheckMethodInfo> {
        self.inner
            .checks
            .iter()
            .cloned()
            .map(|inner| PyCheckMethodInfo { inner })
            .collect()
    }

    #[getter]
    pub fn has_pre_validate(&self) -> bool {
        self.inner.has_pre_validate
    }

    #[getter]
    pub fn key_type(&self) -> Option<PyTypeInfo> {
        self.inner
            .key_type
            .clone()
            .map(|inner| PyTypeInfo { inner })
    }

    #[getter]
    pub fn value_type(&self) -> Option<PyTypeInfo> {
        self.inner
            .value_type
            .clone()
            .map(|inner| PyTypeInfo { inner })
    }

    #[getter]
    pub fn requests(&self) -> Vec<PyRequestInfo> {
        self.inner
            .requests
            .iter()
            .cloned()
            .map(|inner| PyRequestInfo { inner })
            .collect()
    }

    #[getter]
    pub fn rest_endpoints(&self) -> Vec<PyRestEndpointInfo> {
        self.rest_endpoints
            .iter()
            .cloned()
            .map(|inner| PyRestEndpointInfo { inner })
            .collect()
    }

    #[getter]
    pub fn errors(&self) -> Vec<PyErrorVariantInfo> {
        self.inner
            .errors
            .iter()
            .cloned()
            .map(|inner| PyErrorVariantInfo { inner })
            .collect()
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "StructInfo(name={:?}, kind={:?}, fields={}, requests={}, errors={})",
            self.name(),
            self.kind(),
            self.inner.fields.len(),
            self.inner.requests.len(),
            self.inner.errors.len()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a standalone function parameter.
#[pyclass(name = "FnParamInfo")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyFnParamInfo {
    pub(crate) inner: parsmith::FnParam,
}

#[pymethods]
impl PyFnParamInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn type_info(&self) -> PyTypeInfo {
        PyTypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "FnParamInfo(name={:?}, type_info={})",
            self.name(),
            self.type_info().__repr__()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a standalone `fn` or `(raw)fn` declaration.
#[pyclass(name = "FunctionInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyFunctionInfo {
    pub(crate) inner: parsmith::FunctionInfo,
}

#[pymethods]
impl PyFunctionInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn is_raw(&self) -> bool {
        self.inner.is_raw
    }

    #[getter]
    pub fn doc(&self) -> Option<&str> {
        self.inner.doc.as_deref()
    }

    #[getter]
    pub fn params(&self) -> Vec<PyFnParamInfo> {
        self.inner
            .params
            .iter()
            .cloned()
            .map(|inner| PyFnParamInfo { inner })
            .collect()
    }

    #[getter]
    pub fn return_type(&self) -> PyTypeInfo {
        PyTypeInfo {
            inner: self.inner.return_type.clone(),
        }
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "FunctionInfo(name={:?}, is_raw={}, return_type={})",
            self.name(),
            self.is_raw(),
            self.return_type().__repr__()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a field inside a `JsonDefInfo`.
#[pyclass(name = "JsonFieldInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyJsonFieldInfo {
    pub(crate) inner: parsmith::JsonFieldInfo,
}

#[pymethods]
impl PyJsonFieldInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn type_info(&self) -> PyTypeInfo {
        PyTypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    #[getter]
    pub fn alias(&self) -> Option<&str> {
        self.inner.alias.as_deref()
    }

    #[getter]
    pub fn doc(&self) -> Option<&str> {
        self.inner.doc.as_deref()
    }

    #[getter]
    pub fn is_optional(&self) -> bool {
        self.inner.is_optional
    }

    #[getter]
    pub fn omitempty(&self) -> bool {
        self.inner.omitempty
    }

    #[getter]
    pub fn skip(&self) -> bool {
        self.inner.skip
    }

    #[getter]
    pub fn is_dict(&self) -> bool {
        self.inner.is_dict
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "JsonFieldInfo(name={:?}, type_info={}, alias={:?}, is_optional={}, omitempty={}, skip={})",
            self.name(),
            self.type_info().__repr__(),
            self.alias(),
            self.is_optional(),
            self.omitempty(),
            self.skip()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a top-level or hoisted `json` schema.
#[pyclass(name = "JsonDefInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyJsonDefInfo {
    pub(crate) inner: parsmith::JsonDefInfo,
}

#[pymethods]
impl PyJsonDefInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn kind(&self) -> &str {
        self.inner.kind.as_str()
    }

    #[getter]
    pub fn doc(&self) -> Option<&str> {
        self.inner.doc.as_deref()
    }

    #[getter]
    pub fn is_array(&self) -> bool {
        self.inner.is_array
    }

    #[getter]
    pub fn is_dict(&self) -> bool {
        self.inner.is_dict
    }

    #[getter]
    pub fn is_hoisted(&self) -> bool {
        self.inner.is_hoisted
    }

    #[getter]
    pub fn path(&self) -> Option<&str> {
        self.inner.path.as_deref()
    }

    #[getter]
    pub fn fields(&self) -> Vec<PyJsonFieldInfo> {
        self.inner
            .fields
            .iter()
            .cloned()
            .map(|inner| PyJsonFieldInfo { inner })
            .collect()
    }

    #[getter]
    pub fn key_type(&self) -> Option<PyTypeInfo> {
        self.inner
            .key_type
            .clone()
            .map(|inner| PyTypeInfo { inner })
    }

    #[getter]
    pub fn value_type(&self) -> Option<PyTypeInfo> {
        self.inner
            .value_type
            .clone()
            .map(|inner| PyTypeInfo { inner })
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "JsonDefInfo(name={:?}, kind={:?}, is_hoisted={}, fields={})",
            self.name(),
            self.kind(),
            self.is_hoisted(),
            self.inner.fields.len()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a single operation inside an `ExtensionInfo`.
#[pyclass(name = "ExtensionOpInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyExtensionOpInfo {
    pub(crate) inner: parsmith::ExtensionOpInfo,
}

#[pymethods]
impl PyExtensionOpInfo {
    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn qualified_name(&self) -> &str {
        &self.inner.qualified_name
    }

    #[getter]
    pub fn input_type(&self) -> String {
        self.inner.input.expected_name()
    }

    #[getter]
    pub fn output_type(&self) -> String {
        self.inner.output.expected_name()
    }

    #[getter]
    pub fn is_generic(&self) -> bool {
        self.inner.input.is_generic()
    }

    #[getter]
    pub fn targets(&self) -> Vec<String> {
        self.inner.targets.clone()
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "ExtensionOpInfo(qualified_name={:?}, input={:?}, output={:?}, targets={:?})",
            self.qualified_name(),
            self.input_type(),
            self.output_type(),
            self.targets()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Introspection metadata for a top-level `extension` declaration.
#[pyclass(name = "ExtensionInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyExtensionInfo {
    pub(crate) inner: parsmith::ExtensionInfo,
}

#[pymethods]
impl PyExtensionInfo {
    #[getter]
    pub fn namespace(&self) -> &str {
        &self.inner.namespace
    }

    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.namespace
    }

    #[getter]
    pub fn operations(&self) -> Vec<PyExtensionOpInfo> {
        self.inner
            .operations
            .iter()
            .cloned()
            .map(|inner| PyExtensionOpInfo { inner })
            .collect()
    }

    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        serialize_to_py_dict(py, &self.inner)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "ExtensionInfo(namespace={:?}, operations={})",
            self.namespace(),
            self.inner.operations.len()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// Top-level serializable schema introspection metadata returned by `parsmith.inspect_schema()`.
#[pyclass(name = "ModuleInfo")]
#[derive(Debug, Clone, PartialEq)]
pub struct PyModuleInfo {
    pub(crate) inner: parsmith::ModuleInfo,
    pub(crate) rest_endpoints_by_struct: BTreeMap<String, Vec<parsmith::RestEndpointDecl>>,
    pub(crate) sources: BTreeMap<String, String>,
}

impl PyModuleInfo {
    fn from_check_result(mut result: parsmith::CheckResult) -> Self {
        let sources = std::mem::take(&mut result.sources);
        let diagnostics = std::mem::take(&mut result.diagnostics);
        if let Some(module) = result.module.take() {
            let mut rest_endpoints_by_struct = BTreeMap::new();
            for s in &module.structs {
                if !s.rest_endpoints.is_empty() {
                    rest_endpoints_by_struct.insert(s.name.clone(), s.rest_endpoints.clone());
                }
            }
            let inner = inspect_module(&module, diagnostics);
            Self {
                inner,
                rest_endpoints_by_struct,
                sources,
            }
        } else {
            let inner = parsmith::ModuleInfo {
                diagnostics,
                ..Default::default()
            };
            Self {
                inner,
                rest_endpoints_by_struct: BTreeMap::new(),
                sources,
            }
        }
    }
}

#[pymethods]
impl PyModuleInfo {
    #[getter]
    pub fn doc(&self) -> Option<&str> {
        self.inner.doc.as_deref()
    }

    #[getter]
    pub fn source_file(&self) -> &str {
        &self.inner.source_file
    }

    #[getter]
    pub fn struct_kinds(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        for (name, kind) in &self.inner.struct_kinds {
            dict.set_item(name, kind.as_str())?;
        }
        Ok(dict.into_any().unbind())
    }

    #[getter]
    pub fn structs(&self) -> Vec<PyStructInfo> {
        self.inner
            .structs
            .iter()
            .cloned()
            .map(|s| {
                let eps = self
                    .rest_endpoints_by_struct
                    .get(&s.name)
                    .cloned()
                    .unwrap_or_default();
                PyStructInfo {
                    inner: s,
                    rest_endpoints: eps,
                }
            })
            .collect()
    }

    #[getter]
    pub fn functions(&self) -> Vec<PyFunctionInfo> {
        self.inner
            .functions
            .iter()
            .cloned()
            .map(|inner| PyFunctionInfo { inner })
            .collect()
    }

    #[getter]
    pub fn json_defs(&self) -> Vec<PyJsonDefInfo> {
        self.inner
            .json_defs
            .iter()
            .cloned()
            .map(|inner| PyJsonDefInfo { inner })
            .collect()
    }

    #[getter]
    pub fn extensions(&self) -> Vec<PyExtensionInfo> {
        self.inner
            .extensions
            .iter()
            .cloned()
            .map(|inner| PyExtensionInfo { inner })
            .collect()
    }

    #[getter]
    pub fn diagnostics(&self) -> Vec<PyDiagnostic> {
        self.inner
            .diagnostics
            .iter()
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.sources))
            .collect()
    }

    #[getter]
    pub fn errors(&self) -> Vec<PyDiagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_error())
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.sources))
            .collect()
    }

    #[getter]
    pub fn warnings(&self) -> Vec<PyDiagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_warning())
            .cloned()
            .map(|d| PyDiagnostic::from_core(d, &self.sources))
            .collect()
    }

    #[getter]
    pub fn ok(&self) -> bool {
        !self.has_errors()
    }

    pub fn has_errors(&self) -> bool {
        self.inner.diagnostics.iter().any(|d| d.severity.is_error())
    }

    pub fn has_warnings(&self) -> bool {
        self.inner
            .diagnostics
            .iter()
            .any(|d| d.severity.is_warning())
    }

    pub fn is_ok(&self) -> bool {
        !self.has_errors()
    }

    /// Converts this `ModuleInfo` into a plain Python `dict`.
    pub fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let obj = serialize_to_py_dict(py, &self.inner)?;
        let bound = obj.bind(py);
        if let Ok(dict) = bound.downcast::<PyDict>() {
            dict.set_item("ok", self.ok())?;
        }
        Ok(obj)
    }

    /// Serializes this `ModuleInfo` as a pretty-printed JSON string.
    pub fn to_json(&self) -> PyResult<String> {
        serde_json::to_string_pretty(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    pub fn format_text(&self) -> String {
        parsmith::CheckResult::new(self.inner.diagnostics.clone())
            .with_sources(self.sources.clone())
            .format_text()
    }

    /// Formats all diagnostics as a pretty-printed JSON string.
    pub fn format_json(&self) -> String {
        parsmith::CheckResult::new(self.inner.diagnostics.clone()).format_json()
    }

    pub fn __bool__(&self) -> bool {
        !self.has_errors()
    }

    pub fn __repr__(&self) -> String {
        format!(
            "ModuleInfo(ok={}, structs={}, functions={}, json_defs={}, extensions={})",
            self.ok(),
            self.inner.structs.len(),
            self.inner.functions.len(),
            self.inner.json_defs.len(),
            self.inner.extensions.len()
        )
    }

    pub fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

// ============================================================================
// Input Resolution Helpers (`source_or_path` & `sources` VFS)
// ============================================================================

enum SourceOrPath {
    Path(String),
    Paths(Vec<String>),
    Source(String),
}

fn extract_raw_source_or_path(obj: &Bound<'_, PyAny>) -> PyResult<(String, bool)> {
    if let Ok(py_str) = obj.downcast::<PyString>() {
        return Ok((py_str.to_str()?.to_owned(), false));
    }
    let py = obj.py();
    let os = py.import("os")?;
    let fspath = os
        .call_method1("fspath", (obj,))
        .map_err(|_| PyTypeError::new_err("expected str or os.PathLike[str] for source_or_path"))?;
    if let Ok(py_str) = fspath.downcast::<PyString>() {
        return Ok((py_str.to_str()?.to_owned(), true));
    }
    Err(PyTypeError::new_err(
        "expected str or os.PathLike[str] for source_or_path",
    ))
}

fn extract_source_or_paths(
    obj: &Bound<'_, PyAny>,
    has_vfs: Option<&MemorySourceLoader>,
) -> PyResult<SourceOrPath> {
    if let Ok((raw_str, is_pathlike)) = extract_raw_source_or_path(obj) {
        return Ok(resolve_input_mode(&raw_str, is_pathlike, has_vfs));
    }
    if obj.is_instance_of::<PyBytes>()
        || obj.is_instance_of::<PyByteArray>()
        || obj.downcast::<PyMapping>().is_ok()
    {
        return Err(PyTypeError::new_err(
            "expected str, os.PathLike[str], or Sequence[str | os.PathLike[str]] for source_or_path",
        ));
    }
    if let Ok(seq) = obj.downcast::<PySequence>() {
        let len = seq.len()?;
        let mut paths = Vec::with_capacity(len);
        for i in 0..len {
            let item = seq.get_item(i)?;
            let (raw_str, _) = extract_raw_source_or_path(&item).map_err(|_| {
                PyTypeError::new_err(
                    "sequence elements for source_or_path must be str or os.PathLike[str]",
                )
            })?;
            paths.push(raw_str);
        }
        return Ok(SourceOrPath::Paths(paths));
    }
    Err(PyTypeError::new_err(
        "expected str, os.PathLike[str], or Sequence[str | os.PathLike[str]] for source_or_path",
    ))
}

fn extract_sources_map(sources: Option<&Bound<'_, PyAny>>) -> PyResult<Option<MemorySourceLoader>> {
    let Some(src_obj) = sources else {
        return Ok(None);
    };
    if src_obj.is_none() {
        return Ok(None);
    }
    let mapping = src_obj
        .downcast::<PyMapping>()
        .map_err(|_| PyTypeError::new_err("sources must be a mapping of str to str"))?;
    let items = mapping.items()?;
    let mut loader = MemorySourceLoader::new();
    for item in items.iter() {
        let (k_obj, v_obj): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item.extract()?;
        let (k_str, _) = extract_raw_source_or_path(&k_obj)
            .map_err(|_| PyTypeError::new_err("sources keys must be str or PathLike[str]"))?;
        let v_str: String = v_obj
            .extract()
            .map_err(|_| PyTypeError::new_err("sources values must be str"))?;
        loader.insert(k_str, v_str);
    }
    Ok(Some(loader))
}

fn extract_targets(targets: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Vec<String>>> {
    let Some(t_obj) = targets else {
        return Ok(None);
    };
    if t_obj.is_none() {
        return Ok(None);
    }
    if let Ok(s) = t_obj.downcast::<PyString>() {
        return Ok(Some(vec![s.to_str()?.to_owned()]));
    }
    let seq = t_obj
        .downcast::<PySequence>()
        .map_err(|_| PyTypeError::new_err("targets must be a sequence of str"))?;
    let mut vec = Vec::with_capacity(seq.len()?);
    for i in 0..seq.len()? {
        let item = seq.get_item(i)?;
        let s: String = item
            .extract()
            .map_err(|_| PyTypeError::new_err("targets elements must be str"))?;
        vec.push(s);
    }
    if vec.is_empty() {
        Ok(None)
    } else {
        Ok(Some(vec))
    }
}

fn is_single_line_kdl_filename(s: &str) -> bool {
    !s.contains('\n') && !s.contains('\r') && !s.contains('{') && s.trim().ends_with(".kdl")
}

fn resolve_input_mode(
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

struct RawCodegenKwargs<'a> {
    html_lib: Option<&'a str>,
    php_html_lib: Option<&'a str>,
    models: Option<&'a str>,
    bs4_parser: Option<&'a str>,
    http_io: Option<&'a str>,
    separate_runtime: Option<bool>,
    runtime_name: Option<&'a str>,
    package: Option<&'a str>,
    module_stem: Option<&'a str>,
    prologue: Option<&'a str>,
    epilogue: Option<&'a str>,
}

fn resolve_codegen_options(
    target: Option<&str>,
    options: Option<&Bound<'_, PyAny>>,
    kwargs: RawCodegenKwargs<'_>,
) -> PyResult<CodegenOptions> {
    let mut opts = CodegenOptions::default();
    let mut options_had_target = false;
    let mut options_had_html_lib = false;
    let mut options_had_php_html_lib = false;
    let mut options_had_runtime_name = false;
    let mut pending_html_lib: Option<String> = None;

    if let Some(opt_obj) = options
        && !opt_obj.is_none()
    {
        if let Ok(py_opts) = opt_obj.extract::<PyRef<'_, PyCodegenOptions>>() {
            opts = py_opts.inner.clone();
            options_had_target = true;
            options_had_runtime_name = true;
        } else if let Ok(mapping) = opt_obj.downcast::<PyMapping>() {
            let items = mapping.items()?;
            for item in items.iter() {
                let (k_obj, v_obj): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item.extract()?;
                let key: String = k_obj
                    .extract()
                    .map_err(|_| PyTypeError::new_err("options keys must be str"))?;
                match key.as_str() {
                    "target" => {
                        let s: String = v_obj.extract()?;
                        let (parsed_target, shorthand_py_lib, shorthand_php_lib) =
                            parse_target_shorthand(&s)?;
                        if !options_had_runtime_name {
                            opts = opts.with_target(parsed_target);
                        } else {
                            opts.target = parsed_target;
                        }
                        if let Some(lib) = shorthand_py_lib
                            && !options_had_html_lib
                        {
                            opts.html_lib = lib;
                        }
                        if let Some(php_lib) = shorthand_php_lib
                            && !options_had_php_html_lib
                        {
                            opts.php_html_lib = php_lib;
                        }
                        options_had_target = true;
                    }
                    "html_lib" | "html-lib" | "lib" => {
                        let s: String = v_obj.extract()?;
                        pending_html_lib = Some(s);
                        options_had_html_lib = true;
                    }
                    "php_html_lib" | "php-html-lib" => {
                        let s: String = v_obj.extract()?;
                        opts.php_html_lib = parse_php_html_lib(&s)?;
                        options_had_php_html_lib = true;
                    }
                    "models" => {
                        let s: String = v_obj.extract()?;
                        opts.models = parse_models(&s)?;
                    }
                    "bs4_parser" | "bs4-parser" => {
                        let s: String = v_obj.extract()?;
                        opts.bs4_parser = parse_bs4_parser(&s)?;
                    }
                    "http_io" | "http-io" => {
                        let s: String = v_obj.extract()?;
                        opts.http_io = parse_http_io(&s)?;
                    }
                    "separate_runtime" | "separate-runtime" => {
                        opts.separate_runtime = v_obj.extract()?;
                    }
                    "runtime_name" | "runtime-name" => {
                        opts.runtime_name = v_obj.extract()?;
                        options_had_runtime_name = true;
                    }
                    "package" | "namespace" => {
                        opts.package = v_obj.extract()?;
                    }
                    "module_stem" | "module-stem" => {
                        opts.module_stem = v_obj.extract()?;
                    }
                    "prologue" => {
                        opts.prologue = v_obj.extract()?;
                    }
                    "epilogue" => {
                        opts.epilogue = v_obj.extract()?;
                    }
                    other => {
                        return Err(PyValueError::new_err(format!(
                            "Unknown codegen option '{other}'"
                        )));
                    }
                }
            }
        } else {
            return Err(PyTypeError::new_err(
                "options must be a CodegenOptions instance or a mapping",
            ));
        }
    }

    if let Some(t_str) = target {
        let (parsed_target, shorthand_py_lib, shorthand_php_lib) = parse_target_shorthand(t_str)?;
        if !options_had_runtime_name {
            opts = opts.with_target(parsed_target);
        } else {
            opts.target = parsed_target;
        }
        if let Some(lib) = shorthand_py_lib
            && !options_had_html_lib
        {
            opts.html_lib = lib;
        }
        if let Some(php_lib) = shorthand_php_lib
            && !options_had_php_html_lib
        {
            opts.php_html_lib = php_lib;
        }
    } else if !options_had_target {
        if !options_had_runtime_name {
            opts = opts.with_target(Target::Python);
        } else {
            opts.target = Target::Python;
        }
    }

    if let Some(ref hl) = pending_html_lib {
        if opts.target == Target::Php && !options_had_php_html_lib {
            if let Ok(pl) = parse_php_html_lib(hl) {
                opts.php_html_lib = pl;
            } else {
                opts.html_lib = parse_html_lib(hl)?;
            }
        } else {
            opts.html_lib = parse_html_lib(hl)?;
        }
    }

    if let Some(phl) = kwargs.php_html_lib {
        opts.php_html_lib = parse_php_html_lib(phl)?;
    }
    if let Some(hl) = kwargs.html_lib {
        if opts.target == Target::Php && kwargs.php_html_lib.is_none() {
            if let Ok(pl) = parse_php_html_lib(hl) {
                opts.php_html_lib = pl;
            } else {
                opts.html_lib = parse_html_lib(hl)?;
            }
        } else {
            opts.html_lib = parse_html_lib(hl)?;
        }
    }
    if let Some(m) = kwargs.models {
        opts.models = parse_models(m)?;
    }
    if let Some(b) = kwargs.bs4_parser {
        opts.bs4_parser = parse_bs4_parser(b)?;
    }
    if let Some(h) = kwargs.http_io {
        opts.http_io = parse_http_io(h)?;
    }
    if let Some(sr) = kwargs.separate_runtime {
        opts.separate_runtime = sr;
    }
    if let Some(rn) = kwargs.runtime_name {
        opts.runtime_name = rn.to_owned();
    }
    if let Some(pkg) = kwargs.package {
        opts.package = pkg.to_owned();
    }
    if let Some(ms) = kwargs.module_stem {
        opts.module_stem = Some(ms.to_owned());
    }
    if let Some(pr) = kwargs.prologue {
        opts.prologue = Some(pr.to_owned());
    }
    if let Some(ep) = kwargs.epilogue {
        opts.epilogue = Some(ep.to_owned());
    }

    Ok(opts)
}

// ============================================================================
// Top-Level SDK Functions (`check`, `generate`, `inspect_schema`, `json_to_kdl`)
// ============================================================================

/// Returns the version of the `parsmith` engine.
#[pyfunction]
pub fn version() -> &'static str {
    parsmith::version()
}

/// Validates a KDL 2.0 schema (from a file/directory path, sequence of paths, inline KDL string, or in-memory VFS `sources` map).
#[pyfunction]
#[pyo3(signature = (source_or_path, *, sources = None, targets = None, path = None))]
pub fn check(
    source_or_path: &Bound<'_, PyAny>,
    sources: Option<&Bound<'_, PyAny>>,
    targets: Option<&Bound<'_, PyAny>>,
    path: Option<&str>,
) -> PyResult<PyCheckResult> {
    let vfs_loader = extract_sources_map(sources)?;
    let targets_vec = extract_targets(targets)?;
    let mode = extract_source_or_paths(source_or_path, vfs_loader.as_ref())?;

    let res = match (mode, vfs_loader) {
        (SourceOrPath::Path(entry), Some(loader)) => {
            check_with_loader_and_targets(&entry, &loader, targets_vec)
        }
        (SourceOrPath::Paths(entries), Some(loader)) => {
            if entries.len() == 1 {
                check_with_loader_and_targets(&entries[0], &loader, targets_vec)
            } else {
                let check_opts = match targets_vec {
                    Some(iter) => CheckOptions::new().with_targets(iter),
                    None => CheckOptions::default(),
                };
                let mut all_diagnostics = Vec::new();
                let mut all_sources = BTreeMap::new();
                let mut last_program = None;
                let mut last_module = None;
                for entry in &entries {
                    let r = check_with_options(entry, &loader, &check_opts);
                    for (k, v) in r.sources {
                        all_sources.insert(k, v);
                    }
                    for d in r.diagnostics {
                        if !all_diagnostics.contains(&d) {
                            all_diagnostics.push(d);
                        }
                    }
                    if r.program.is_some() {
                        last_program = r.program;
                    }
                    if r.module.is_some() {
                        last_module = r.module;
                    }
                }
                parsmith::CheckResult::new(all_diagnostics)
                    .with_sources(all_sources)
                    .with_program(last_program)
                    .with_module(last_module)
            }
        }
        (SourceOrPath::Source(src), Some(loader)) => {
            let logical_path = path.unwrap_or("<memory>.kdl");
            check_source_with_loader_and_targets(&src, Some(logical_path), &loader, targets_vec)
        }
        (SourceOrPath::Path(entry), None) => check_path_with_targets(&entry, targets_vec),
        (SourceOrPath::Paths(entries), None) => check_paths_with_targets(&entries, targets_vec),
        (SourceOrPath::Source(src), None) => check_source_with_targets(&src, path, targets_vec),
    };

    Ok(PyCheckResult { inner: res })
}

/// Compiles a KDL 2.0 schema and generates target code (`"python"`, `"ts"`, `"js"`, `"go"`, `"rust"`, `"elixir"`, `"php"`, `"csharp"`, or `"kotlin"`).
#[pyfunction]
#[pyo3(signature = (
    source_or_path,
    target = None,
    *,
    sources = None,
    options = None,
    path = None,
    html_lib = None,
    php_html_lib = None,
    models = None,
    bs4_parser = None,
    http_io = None,
    separate_runtime = None,
    runtime_name = None,
    package = None,
    module_stem = None,
    prologue = None,
    epilogue = None,
))]
#[allow(clippy::too_many_arguments)]
pub fn generate(
    source_or_path: &Bound<'_, PyAny>,
    target: Option<&str>,
    sources: Option<&Bound<'_, PyAny>>,
    options: Option<&Bound<'_, PyAny>>,
    path: Option<&str>,
    html_lib: Option<&str>,
    php_html_lib: Option<&str>,
    models: Option<&str>,
    bs4_parser: Option<&str>,
    http_io: Option<&str>,
    separate_runtime: Option<bool>,
    runtime_name: Option<&str>,
    package: Option<String>,
    module_stem: Option<&str>,
    prologue: Option<&str>,
    epilogue: Option<&str>,
) -> PyResult<PyGenerateResult> {
    let vfs_loader = extract_sources_map(sources)?;
    let opts = resolve_codegen_options(
        target,
        options,
        RawCodegenKwargs {
            html_lib,
            php_html_lib,
            models,
            bs4_parser,
            http_io,
            separate_runtime,
            runtime_name,
            package: package.as_deref(),
            module_stem,
            prologue,
            epilogue,
        },
    )?;
    let mode = extract_source_or_paths(source_or_path, vfs_loader.as_ref())?;

    let res = match (mode, vfs_loader) {
        (SourceOrPath::Path(entry), Some(loader)) => generate_with_loader(&entry, &loader, &opts),
        (SourceOrPath::Paths(entries), Some(loader)) => {
            if entries.len() == 1 {
                generate_with_loader(&entries[0], &loader, &opts)
            } else {
                let mut combined = parsmith::GenerateResult::default();
                for entry in &entries {
                    let r = generate_with_loader(entry, &loader, &opts);
                    for (k, v) in r.files {
                        combined.files.insert(k, v);
                    }
                    for (k, v) in r.sources {
                        combined.sources.insert(k, v);
                    }
                    for d in r.diagnostics {
                        if !combined.diagnostics.contains(&d) {
                            combined.diagnostics.push(d);
                        }
                    }
                }
                combined
            }
        }
        (SourceOrPath::Source(src), Some(loader)) => {
            let logical_path = path.unwrap_or("<memory>.kdl");
            generate_source_with_loader(&src, Some(logical_path), &loader, &opts)
        }
        (SourceOrPath::Path(entry), None) => generate_path(&entry, &opts),
        (SourceOrPath::Paths(entries), None) => generate_paths(&entries, &opts),
        (SourceOrPath::Source(src), None) => generate_source(&src, path, &opts),
    };

    Ok(PyGenerateResult { inner: res })
}

/// Compiles and introspects a KDL 2.0 schema into structured `ModuleInfo` metadata.
#[pyfunction]
#[pyo3(signature = (source_or_path, *, sources = None, path = None))]
pub fn inspect_schema(
    source_or_path: &Bound<'_, PyAny>,
    sources: Option<&Bound<'_, PyAny>>,
    path: Option<&str>,
) -> PyResult<PyModuleInfo> {
    let (raw_str, is_pathlike) = extract_raw_source_or_path(source_or_path)?;
    let vfs_loader = extract_sources_map(sources)?;
    let mode = resolve_input_mode(&raw_str, is_pathlike, vfs_loader.as_ref());
    let check_opts = CheckOptions::default();

    let check_res = match (mode, vfs_loader) {
        (SourceOrPath::Path(entry), Some(loader)) => {
            check_with_options(&entry, &loader, &check_opts)
        }
        (SourceOrPath::Source(src), Some(loader)) => {
            let logical_path = path.unwrap_or("<memory>.kdl");
            check_source_with_options(&src, Some(logical_path), &loader, &check_opts)
        }
        (SourceOrPath::Path(entry), None) => {
            let fs_loader = FsSourceLoader::new();
            check_with_options(&entry, &fs_loader, &check_opts)
        }
        (SourceOrPath::Source(src), None) => {
            let fs_loader = FsSourceLoader::new();
            check_source_with_options(&src, path, &fs_loader, &check_opts)
        }
        (SourceOrPath::Paths(_), _) => unreachable!(),
    };

    Ok(PyModuleInfo::from_check_result(check_res))
}

/// Bootstraps a KDL v2.1 `json` schema string from a sample JSON payload.
#[pyfunction]
#[pyo3(signature = (json_input, *, name = "JsonResponse"))]
pub fn json_to_kdl(json_input: &str, name: &str) -> PyResult<String> {
    parsmith::json_to_kdl(json_input, name).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// Native `_parsmith` Python extension module.
#[pymodule]
fn _parsmith(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", parsmith::version())?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(check, m)?)?;
    m.add_function(wrap_pyfunction!(generate, m)?)?;
    m.add_function(wrap_pyfunction!(inspect_schema, m)?)?;
    m.add_function(wrap_pyfunction!(json_to_kdl, m)?)?;

    m.add_class::<PyDiagnostic>()?;
    m.add_class::<PyCheckResult>()?;
    m.add_class::<PyCodegenOptions>()?;
    m.add_class::<PyGenerateResult>()?;
    m.add_class::<PyTypeInfo>()?;
    m.add_class::<PyFieldInfo>()?;
    m.add_class::<PyInitFieldInfo>()?;
    m.add_class::<PyCheckMethodInfo>()?;
    m.add_class::<PyPlaceholderInfo>()?;
    m.add_class::<PyRequestInfo>()?;
    m.add_class::<PyRestEndpointInfo>()?;
    m.add_class::<PyErrorVariantInfo>()?;
    m.add_class::<PyStructInfo>()?;
    m.add_class::<PyFnParamInfo>()?;
    m.add_class::<PyFunctionInfo>()?;
    m.add_class::<PyJsonFieldInfo>()?;
    m.add_class::<PyJsonDefInfo>()?;
    m.add_class::<PyExtensionOpInfo>()?;
    m.add_class::<PyExtensionInfo>()?;
    m.add_class::<PyModuleInfo>()?;

    Ok(())
}

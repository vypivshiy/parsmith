//! Schema introspection (`inspect_schema() -> ModuleInfo`).
//!
//! Provides serializable structural metadata (`structs`, `struct_kinds`, `fields` with resolved
//! [`TypeInfo`], `init_fields`, `requests` with [`PlaceholderSpec`]s, `errors`, `functions`,
//! `json_defs`, `extensions`, `diagnostics`) for UI form builders, IDE plugins, and host SDKs.
#![allow(clippy::result_large_err)]

use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::ast::{ExtensionTypeSig, FnParam, JsonKind, Module, StructKind, TypeInfo};
use crate::check::{CheckOptions, check_source_with_options, check_with_options};
use crate::diagnostic::{CheckResult, Diagnostic};
use crate::loader::{FsSourceLoader, SourceLoader};
use crate::request::{ErrorConditionValue, PlaceholderSpec, RequestBodyKind, err_subclass_name};

/// Top-level serializable schema introspection metadata returned by [`inspect_schema_path`],
/// [`inspect_schema_source`], and [`inspect_schema_with_loader`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Optional module-level `@doc` description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Source file name or logical path.
    #[serde(default)]
    pub source_file: String,
    /// Map of struct names to their [`StructKind`].
    pub struct_kinds: IndexMap<String, StructKind>,
    /// Declared `struct` definitions in topological order.
    pub structs: Vec<StructInfo>,
    /// Standalone `fn` and `(raw)fn` definitions.
    pub functions: Vec<FunctionInfo>,
    /// Declared and hoisted `json` schema definitions in post-order.
    pub json_defs: Vec<JsonDefInfo>,
    /// Declared `extension` namespaces and operations.
    pub extensions: Vec<ExtensionInfo>,
    /// Non-fatal warnings (or collected diagnostics) emitted during compilation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

/// Introspection metadata for a single `struct` declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructInfo {
    /// Struct name.
    pub name: String,
    /// Struct parsing or REST client kind (`item`, `list`, `dict`, `table`, `flat`, `raw`, `rest`).
    pub kind: StructKind,
    /// Optional `@doc` docstring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Whether `keep-order=#true` is set on `(flat)struct`.
    pub keep_order: bool,
    /// Extracted output fields with resolved [`TypeInfo`].
    pub fields: Vec<FieldInfo>,
    /// Precomputed `@init` fields with resolved [`TypeInfo`].
    pub init_fields: Vec<InitFieldInfo>,
    /// `@check` boolean validation methods.
    pub checks: Vec<CheckMethodInfo>,
    /// Whether `@pre-validate` is declared on this struct.
    pub has_pre_validate: bool,
    /// Resolved `@key` output type for `(dict)struct`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_type: Option<TypeInfo>,
    /// Resolved `@value` output type for `(dict)struct` or `(table)struct`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<TypeInfo>,
    /// `@request` HTTP methods declared on this struct.
    pub requests: Vec<RequestInfo>,
    /// `@error` status variants declared on `(rest)struct`.
    pub errors: Vec<ErrorVariantInfo>,
}

/// Introspection metadata for a struct output field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldInfo {
    /// Field name.
    pub name: String,
    /// Resolved output [`TypeInfo`].
    pub type_info: TypeInfo,
    /// Whether the field pipeline has a terminal `fallback` clause.
    pub has_fallback: bool,
}

/// Introspection metadata for a precomputed `@init` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitFieldInfo {
    /// Precomputed field name (referenced as `@<name>`).
    pub name: String,
    /// Resolved output [`TypeInfo`].
    pub type_info: TypeInfo,
}

/// Introspection metadata for a `@check` validation method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckMethodInfo {
    /// Check method name.
    pub name: String,
    /// Resolved return [`TypeInfo`] (`BOOL`).
    pub return_type: TypeInfo,
}

/// Introspection metadata for an HTTP `@request` declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestInfo {
    /// Method name (defaults to `"fetch"`).
    pub name: String,
    /// HTTP method verb (`"GET"`, `"POST"`, etc.).
    pub method: String,
    /// Canonical URL template string.
    pub url: String,
    /// Optional method docstring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Optional 2xx response `json` schema name (`response="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<String>,
    /// Optional dot-notation response extraction path (`response-path="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_path: Option<String>,
    /// Optional list join separator (`response-join="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_join: Option<String>,
    /// Synthesized result union type alias name for `(rest)struct` endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_alias_name: Option<String>,
    /// Request body kind (`empty`, `json`, `form`, `raw`).
    pub body_kind: RequestBodyKind,
    /// Deduplicated parameter placeholders in order of first appearance.
    pub placeholders: Vec<PlaceholderSpec>,
}

/// Introspection metadata for an `@error` variant on a `(rest)struct`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorVariantInfo {
    /// Synthesized error variant class name (e.g. `"APIErr404NotFound"`).
    pub variant_name: String,
    /// HTTP status code (`100..599`).
    pub status: u16,
    /// Error payload `json` schema name.
    pub schema_name: String,
    /// Optional `@error:<suffix>` disambiguator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    /// Required JSON keys/dot-paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_keys: Vec<String>,
    /// JSON key/dot-path value conditions.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub conditions: IndexMap<String, ErrorConditionValue>,
}

/// Introspection metadata for a standalone `fn` or `(raw)fn` declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionInfo {
    /// Function name.
    pub name: String,
    /// Whether this function is `(raw)fn` (`STRING` input) rather than `fn` (`DOCUMENT` input).
    pub is_raw: bool,
    /// Optional `@doc` docstring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Function parameters.
    pub params: Vec<FnParam>,
    /// Resolved return [`TypeInfo`].
    pub return_type: TypeInfo,
}

/// Introspection metadata for a top-level or hoisted `json` schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonDefInfo {
    /// Schema name.
    pub name: String,
    /// Schema container kind (`object`, `array`, `dict`).
    pub kind: JsonKind,
    /// Optional `doc` description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Whether this schema is `(array)json`.
    pub is_array: bool,
    /// Whether this schema is `(dict)json`.
    pub is_dict: bool,
    /// Whether this schema was hoisted from an inline JSON or `@value` block.
    pub is_hoisted: bool,
    /// Optional root extraction dot-path (`path="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Declared fields for `object` and `array` schemas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<JsonFieldInfo>,
    /// Key type for `(dict)json` schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_type: Option<TypeInfo>,
    /// Value type for `(dict)json` schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<TypeInfo>,
}

/// Introspection metadata for a field inside a [`JsonDefInfo`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonFieldInfo {
    /// Canonical field name.
    pub name: String,
    /// Resolved field [`TypeInfo`].
    pub type_info: TypeInfo,
    /// Optional wire key or GJSON dot-path alias (`from="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Optional field documentation (`doc="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Whether the field is nullable (`?`).
    pub is_optional: bool,
    /// Whether the field carries `@omitempty`.
    pub omitempty: bool,
    /// Whether the field carries `@skip`.
    pub skip: bool,
    /// Whether this field is an inline `(dict)` field.
    pub is_dict: bool,
}

/// Introspection metadata for a top-level `extension` declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionInfo {
    /// Extension namespace name.
    pub namespace: String,
    /// Operations declared in this namespace.
    pub operations: Vec<ExtensionOpInfo>,
}

/// Introspection metadata for a single operation inside an [`ExtensionInfo`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionOpInfo {
    /// Operation name.
    pub name: String,
    /// Qualified `"Namespace.op"` name.
    pub qualified_name: String,
    /// Input type signature.
    pub input: ExtensionTypeSig,
    /// Output type signature.
    pub output: ExtensionTypeSig,
    /// Target languages supported by this operation (including `"ts"` when `"js"` is present).
    pub targets: Vec<String>,
}

/// Extracts serializable [`ModuleInfo`] metadata from a compiled [`Module`] and its diagnostics.
pub fn inspect_module(module: &Module, diagnostics: Vec<Diagnostic>) -> ModuleInfo {
    let mut struct_kinds = IndexMap::new();
    let mut structs = Vec::with_capacity(module.structs.len());

    for s in &module.structs {
        struct_kinds.insert(s.name.clone(), s.kind);

        let fields = s
            .fields
            .iter()
            .map(|f| FieldInfo {
                name: f.name.clone(),
                type_info: f.type_info.clone(),
                has_fallback: f.pipeline.fallback.is_some(),
            })
            .collect();

        let init_fields = s
            .init_fields
            .iter()
            .map(|i| InitFieldInfo {
                name: i.name.clone(),
                type_info: i.type_info.clone(),
            })
            .collect();

        let checks = s
            .checks
            .iter()
            .map(|c| CheckMethodInfo {
                name: c.name.clone(),
                return_type: c.pipeline.output.clone(),
            })
            .collect();

        let requests = if s.kind == StructKind::Rest {
            s.rest_endpoints
                .iter()
                .map(|ep| RequestInfo {
                    name: ep.name.clone(),
                    method: ep.spec.method.clone(),
                    url: ep.spec.url.source(),
                    doc: ep.doc.clone(),
                    response_schema: ep.response_schema.clone(),
                    response_path: ep.response_path.clone(),
                    response_join: None,
                    result_alias_name: Some(ep.result_alias_name.clone()),
                    body_kind: ep.spec.body_kind,
                    placeholders: ep.placeholders.clone(),
                })
                .collect()
        } else {
            s.requests
                .iter()
                .map(|req| RequestInfo {
                    name: req.name.clone(),
                    method: req.spec.method.clone(),
                    url: req.spec.url.source(),
                    doc: req.doc.clone(),
                    response_schema: req.response_schema.clone(),
                    response_path: req.response_path.clone(),
                    response_join: req.response_join.clone(),
                    result_alias_name: None,
                    body_kind: req.spec.body_kind,
                    placeholders: req.placeholders.clone(),
                })
                .collect()
        };

        let errors = s
            .errors
            .iter()
            .map(|err| ErrorVariantInfo {
                variant_name: err_subclass_name(&s.name, err),
                status: err.status,
                schema_name: err.schema_name.clone(),
                suffix: err.suffix.clone(),
                required_keys: err.required_keys.clone(),
                conditions: err.conditions.clone(),
            })
            .collect();

        structs.push(StructInfo {
            name: s.name.clone(),
            kind: s.kind,
            doc: s.doc.clone(),
            keep_order: s.keep_order,
            fields,
            init_fields,
            checks,
            has_pre_validate: s.pre_validate.is_some(),
            key_type: s.key_pipeline.as_ref().map(|p| p.output.clone()),
            value_type: s.value_pipeline.as_ref().map(|p| p.output.clone()),
            requests,
            errors,
        });
    }

    let functions = module
        .functions
        .iter()
        .map(|f| FunctionInfo {
            name: f.name.clone(),
            is_raw: f.is_raw,
            doc: f.doc.clone(),
            params: f.params.clone(),
            return_type: f.return_type.clone(),
        })
        .collect();

    let json_defs = module
        .json_defs
        .iter()
        .map(|j| JsonDefInfo {
            name: j.name.clone(),
            kind: j.kind,
            doc: j.doc.clone(),
            is_array: j.is_array,
            is_dict: j.is_dict,
            is_hoisted: j.is_hoisted,
            path: j.path.clone(),
            fields: j
                .fields
                .iter()
                .map(|f| JsonFieldInfo {
                    name: f.name.clone(),
                    type_info: f.type_info.clone(),
                    alias: f.alias.clone(),
                    doc: f.doc.clone(),
                    is_optional: f.is_optional,
                    omitempty: f.omitempty,
                    skip: f.skip,
                    is_dict: f.is_dict,
                })
                .collect(),
            key_type: j.key_type.clone(),
            value_type: j.value_type.clone(),
        })
        .collect();

    let extensions = module
        .extensions
        .iter()
        .map(|ext| ExtensionInfo {
            namespace: ext.namespace.clone(),
            operations: ext
                .operations
                .iter()
                .map(|op| ExtensionOpInfo {
                    name: op.name.clone(),
                    qualified_name: op.qualified_name.clone(),
                    input: op.input.clone(),
                    output: op.output.clone(),
                    targets: op.targets.keys().cloned().collect(),
                })
                .collect(),
        })
        .collect();

    ModuleInfo {
        doc: module.doc.clone(),
        source_file: module.source_file.clone(),
        struct_kinds,
        structs,
        functions,
        json_defs,
        extensions,
        diagnostics,
    }
}

/// Compiles and introspects a KDL 2.0 schema file on disk, returning [`ModuleInfo`] on success
/// or [`CheckResult`] if compilation errors occurred.
pub fn inspect_schema_path(path: impl AsRef<Path>) -> Result<ModuleInfo, CheckResult> {
    let loader = FsSourceLoader::new();
    let entry_path = path.as_ref().to_string_lossy();
    inspect_schema_with_loader(&entry_path, &loader)
}

/// Compiles and introspects an in-memory KDL 2.0 source string, returning [`ModuleInfo`] on success
/// or [`CheckResult`] if compilation errors occurred.
pub fn inspect_schema_source(source: &str, path: Option<&str>) -> Result<ModuleInfo, CheckResult> {
    let loader = FsSourceLoader::new();
    inspect_schema_source_with_loader(source, path, &loader)
}

/// Alias for [`inspect_schema_source`].
pub fn inspect_schema(source: &str, path: Option<&str>) -> Result<ModuleInfo, CheckResult> {
    inspect_schema_source(source, path)
}

/// Compiles and introspects a KDL 2.0 schema starting from `entry_path` using `loader`.
pub fn inspect_schema_with_loader(
    entry_path: &str,
    loader: &dyn SourceLoader,
) -> Result<ModuleInfo, CheckResult> {
    let result = check_with_options(entry_path, loader, &CheckOptions::default());
    if result.has_errors() {
        return Err(result);
    }
    let module = result.module.clone().unwrap_or_default();
    Ok(inspect_module(&module, result.diagnostics))
}

/// Compiles and introspects an in-memory KDL 2.0 `source` string at logical `path`, resolving any
/// relative `import` blocks via `loader`.
pub fn inspect_schema_source_with_loader(
    source: &str,
    path: Option<&str>,
    loader: &dyn SourceLoader,
) -> Result<ModuleInfo, CheckResult> {
    let result = check_source_with_options(source, path, loader, &CheckOptions::default());
    if result.has_errors() {
        return Err(result);
    }
    let module = result.module.clone().unwrap_or_default();
    Ok(inspect_module(&module, result.diagnostics))
}

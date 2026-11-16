//! Schema introspection classes (`ModuleInfo` and 15 child `*Info` classes) for `parsmith-js`.

use std::collections::BTreeMap;

use napi::bindgen_prelude::*;
use napi::{Env, JsObject, JsUnknown};
use napi_derive::napi;
use parsmith::{
    CheckOptions, ErrorConditionValue, FsSourceLoader, MemorySourceLoader,
    check_source_with_options, check_with_options, inspect_module,
};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use crate::{
    Diagnostic, SourceOrPath, extract_opt_string, extract_raw_source_or_path, extract_sources_map,
    resolve_input_mode, serialize_to_json_value, type_err, value_err,
};

/// Complete type descriptor for struct fields, function signatures, and JSON fields.
#[napi]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    pub(crate) inner: parsmith::TypeInfo,
}

impl TypeInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "base": self.base(),
            "dslName": self.dsl_name(),
            "isArray": self.is_array(),
            "isOptional": self.is_optional(),
            "isDict": self.is_dict(),
            "reference": self.reference(),
            "keyType": self.key_type().map(|k| k.to_camel_json()),
            "valueType": self.value_type().map(|v| v.to_camel_json()),
            "omitempty": self.omitempty(),
            "skip": self.skip(),
        })
    }
}

#[napi]
impl TypeInfo {
    /// Lowercase base type category (`"auto"`, `"document"`, `"string"`, `"int"`, `"float"`, `"bool"`, `"null"`, `"nested"`, `"json"`, `"dict"`).
    #[napi(getter)]
    pub fn base(&self) -> String {
        self.inner.base.dsl_name().to_ascii_lowercase()
    }

    /// Uppercase DSL type category (`"AUTO"`, `"DOCUMENT"`, `"STRING"`, `"INT"`, `"FLOAT"`, `"BOOL"`, `"NULL"`, `"NESTED"`, `"JSON"`, `"DICT"`).
    #[napi(getter, js_name = "dslName")]
    pub fn dsl_name(&self) -> String {
        self.inner.base.dsl_name().to_owned()
    }

    /// Whether this type represents a list/array (`list[T]` / `T[]`).
    #[napi(getter, js_name = "isArray")]
    pub fn is_array(&self) -> bool {
        self.inner.is_array
    }

    /// Whether this type is nullable/optional (`T | None` / `T | null`).
    #[napi(getter, js_name = "isOptional")]
    pub fn is_optional(&self) -> bool {
        self.inner.is_optional
    }

    /// Whether this type represents a dynamic dictionary (`dict[K, V]` / `Record<K, V>`).
    #[napi(getter, js_name = "isDict")]
    pub fn is_dict(&self) -> bool {
        self.inner.is_dict
    }

    /// Referenced `struct` or `json` schema name, if applicable.
    #[napi(getter)]
    pub fn reference(&self) -> Option<String> {
        self.inner.ref_name().map(str::to_owned)
    }

    /// Dictionary key type when `isDict` is `true`.
    #[napi(getter, js_name = "keyType")]
    pub fn key_type(&self) -> Option<TypeInfo> {
        self.inner
            .key_type
            .as_deref()
            .cloned()
            .map(|inner| TypeInfo { inner })
    }

    /// Dictionary value type when `isDict` is `true`.
    #[napi(getter, js_name = "valueType")]
    pub fn value_type(&self) -> Option<TypeInfo> {
        self.inner
            .value_type
            .as_deref()
            .cloned()
            .map(|inner| TypeInfo { inner })
    }

    /// Whether the JSON field carries `@omitempty`.
    #[napi(getter)]
    pub fn omitempty(&self) -> bool {
        self.inner.omitempty
    }

    /// Whether the JSON field carries `@skip`.
    #[napi(getter)]
    pub fn skip(&self) -> bool {
        self.inner.skip
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "TypeInfo(base={:?}, isArray={}, isOptional={}, reference={:?})",
            self.base(),
            self.is_array(),
            self.is_optional(),
            self.reference()
        )
    }
}

/// Introspection metadata for a struct output field.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub(crate) inner: parsmith::FieldInfo,
}

impl FieldInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "name": self.name(),
            "typeInfo": self.type_info().to_camel_json(),
            "hasFallback": self.has_fallback(),
        })
    }
}

#[napi]
impl FieldInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "typeInfo")]
    pub fn type_info(&self) -> TypeInfo {
        TypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    #[napi(getter, js_name = "hasFallback")]
    pub fn has_fallback(&self) -> bool {
        self.inner.has_fallback
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "FieldInfo(name={:?}, typeInfo={}, hasFallback={})",
            self.name(),
            self.type_info().to_string_repr(),
            self.has_fallback()
        )
    }
}

/// Introspection metadata for a precomputed `@init` field.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct InitFieldInfo {
    pub(crate) inner: parsmith::InitFieldInfo,
}

impl InitFieldInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "name": self.name(),
            "typeInfo": self.type_info().to_camel_json(),
        })
    }
}

#[napi]
impl InitFieldInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "typeInfo")]
    pub fn type_info(&self) -> TypeInfo {
        TypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "InitFieldInfo(name={:?}, typeInfo={})",
            self.name(),
            self.type_info().to_string_repr()
        )
    }
}

/// Introspection metadata for a `@check` validation method.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct CheckMethodInfo {
    pub(crate) inner: parsmith::CheckMethodInfo,
}

impl CheckMethodInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "name": self.name(),
            "returnType": self.return_type().to_camel_json(),
        })
    }
}

#[napi]
impl CheckMethodInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "returnType")]
    pub fn return_type(&self) -> TypeInfo {
        TypeInfo {
            inner: self.inner.return_type.clone(),
        }
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "CheckMethodInfo(name={:?}, returnType={})",
            self.name(),
            self.return_type().to_string_repr()
        )
    }
}

/// Introspection metadata for a `@request` parameter placeholder (`{{name:type[]?|style}}`).
#[napi]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderInfo {
    pub(crate) inner: parsmith::PlaceholderSpec,
}

impl PlaceholderInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "name": self.name(),
            "typeName": self.type_name(),
            "isArray": self.is_array(),
            "isOptional": self.is_optional(),
            "style": self.style(),
            "token": self.token(),
        })
    }
}

#[napi]
impl PlaceholderInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "typeName")]
    pub fn type_name(&self) -> String {
        self.inner.type_name.as_str().to_owned()
    }

    #[napi(getter, js_name = "isArray")]
    pub fn is_array(&self) -> bool {
        self.inner.is_array
    }

    #[napi(getter, js_name = "isOptional")]
    pub fn is_optional(&self) -> bool {
        self.inner.is_optional
    }

    #[napi(getter)]
    pub fn style(&self) -> Option<String> {
        self.inner.style.map(|s| s.as_str().to_owned())
    }

    #[napi(getter)]
    pub fn token(&self) -> String {
        self.inner.to_token()
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "PlaceholderInfo(name={:?}, typeName={:?}, isArray={}, isOptional={}, style={:?})",
            self.name(),
            self.type_name(),
            self.is_array(),
            self.is_optional(),
            self.style()
        )
    }
}

/// Introspection metadata for an HTTP `@request` declaration.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct RequestInfo {
    pub(crate) inner: parsmith::RequestInfo,
}

impl RequestInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        let phs: Vec<JsonValue> = self
            .placeholders()
            .into_iter()
            .map(|p| p.to_camel_json())
            .collect();
        json!({
            "name": self.name(),
            "method": self.method(),
            "url": self.url(),
            "doc": self.doc(),
            "responseSchema": self.response_schema(),
            "responsePath": self.response_path(),
            "responseJoin": self.response_join(),
            "resultAliasName": self.result_alias_name(),
            "bodyKind": self.body_kind(),
            "placeholders": phs,
        })
    }
}

#[napi]
impl RequestInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter)]
    pub fn method(&self) -> String {
        self.inner.method.clone()
    }

    #[napi(getter)]
    pub fn url(&self) -> String {
        self.inner.url.clone()
    }

    #[napi(getter)]
    pub fn doc(&self) -> Option<String> {
        self.inner.doc.clone()
    }

    #[napi(getter, js_name = "responseSchema")]
    pub fn response_schema(&self) -> Option<String> {
        self.inner.response_schema.clone()
    }

    #[napi(getter, js_name = "responsePath")]
    pub fn response_path(&self) -> Option<String> {
        self.inner.response_path.clone()
    }

    #[napi(getter, js_name = "responseJoin")]
    pub fn response_join(&self) -> Option<String> {
        self.inner.response_join.clone()
    }

    #[napi(getter, js_name = "resultAliasName")]
    pub fn result_alias_name(&self) -> Option<String> {
        self.inner.result_alias_name.clone()
    }

    #[napi(getter, js_name = "bodyKind")]
    pub fn body_kind(&self) -> String {
        self.inner.body_kind.as_str().to_owned()
    }

    #[napi(getter)]
    pub fn placeholders(&self) -> Vec<PlaceholderInfo> {
        self.inner
            .placeholders
            .iter()
            .cloned()
            .map(|inner| PlaceholderInfo { inner })
            .collect()
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "RequestInfo(name={:?}, method={:?}, url={:?}, bodyKind={:?})",
            self.name(),
            self.method(),
            self.url(),
            self.body_kind()
        )
    }
}

/// Introspection metadata for a typed `(rest)struct` endpoint declaration.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct RestEndpointInfo {
    pub(crate) inner: parsmith::RestEndpointDecl,
}

impl RestEndpointInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        let phs: Vec<JsonValue> = self
            .placeholders()
            .into_iter()
            .map(|p| p.to_camel_json())
            .collect();
        json!({
            "name": self.name(),
            "method": self.method(),
            "url": self.url(),
            "doc": self.doc(),
            "responseSchema": self.response_schema(),
            "responsePath": self.response_path(),
            "responseIsArray": self.response_is_array(),
            "resultAliasName": self.result_alias_name(),
            "bodyKind": self.body_kind(),
            "placeholders": phs,
        })
    }
}

#[napi]
impl RestEndpointInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter)]
    pub fn method(&self) -> String {
        self.inner.spec.method.clone()
    }

    #[napi(getter)]
    pub fn url(&self) -> String {
        self.inner.spec.url.source()
    }

    #[napi(getter)]
    pub fn doc(&self) -> Option<String> {
        self.inner.doc.clone()
    }

    #[napi(getter, js_name = "responseSchema")]
    pub fn response_schema(&self) -> Option<String> {
        self.inner.response_schema.clone()
    }

    #[napi(getter, js_name = "responsePath")]
    pub fn response_path(&self) -> Option<String> {
        self.inner.response_path.clone()
    }

    #[napi(getter, js_name = "responseIsArray")]
    pub fn response_is_array(&self) -> bool {
        self.inner.response_is_array
    }

    #[napi(getter, js_name = "resultAliasName")]
    pub fn result_alias_name(&self) -> String {
        self.inner.result_alias_name.clone()
    }

    #[napi(getter, js_name = "bodyKind")]
    pub fn body_kind(&self) -> String {
        self.inner.spec.body_kind.as_str().to_owned()
    }

    #[napi(getter)]
    pub fn placeholders(&self) -> Vec<PlaceholderInfo> {
        self.inner
            .placeholders
            .iter()
            .cloned()
            .map(|inner| PlaceholderInfo { inner })
            .collect()
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        let mut ph_list = Vec::new();
        for ph in self.placeholders() {
            ph_list.push(ph.to_dict()?);
        }
        Ok(json!({
            "name": self.name(),
            "method": self.method(),
            "url": self.url(),
            "doc": self.doc(),
            "response_schema": self.response_schema(),
            "response_path": self.response_path(),
            "response_is_array": self.response_is_array(),
            "result_alias_name": self.result_alias_name(),
            "body_kind": self.body_kind(),
            "placeholders": ph_list,
        }))
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "RestEndpointInfo(name={:?}, method={:?}, url={:?}, resultAliasName={:?})",
            self.name(),
            self.method(),
            self.url(),
            self.result_alias_name()
        )
    }
}

/// Introspection metadata for an `@error` variant on a `(rest)struct`.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorVariantInfo {
    pub(crate) inner: parsmith::ErrorVariantInfo,
}

impl ErrorVariantInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "variantName": self.variant_name(),
            "name": self.name(),
            "status": self.status(),
            "schemaName": self.schema_name(),
            "suffix": self.suffix(),
            "requiredKeys": self.required_keys(),
            "conditions": self.conditions(),
        })
    }
}

#[napi]
impl ErrorVariantInfo {
    #[napi(getter, js_name = "variantName")]
    pub fn variant_name(&self) -> String {
        self.inner.variant_name.clone()
    }

    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.variant_name.clone()
    }

    #[napi(getter)]
    pub fn status(&self) -> u16 {
        self.inner.status
    }

    #[napi(getter, js_name = "schemaName")]
    pub fn schema_name(&self) -> String {
        self.inner.schema_name.clone()
    }

    #[napi(getter)]
    pub fn suffix(&self) -> String {
        self.inner.suffix.clone()
    }

    #[napi(getter, js_name = "requiredKeys")]
    pub fn required_keys(&self) -> Vec<String> {
        self.inner.required_keys.clone()
    }

    #[napi(getter)]
    pub fn conditions(&self) -> JsonValue {
        let mut map = JsonMap::new();
        for (k, v) in &self.inner.conditions {
            let jv = match v {
                ErrorConditionValue::String(s) => JsonValue::String(s.clone()),
                ErrorConditionValue::Int(i) => json!(*i),
                ErrorConditionValue::Float(f) => json!(*f),
                ErrorConditionValue::Bool(b) => JsonValue::Bool(*b),
                ErrorConditionValue::Null => JsonValue::Null,
            };
            map.insert(k.clone(), jv);
        }
        JsonValue::Object(map)
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "ErrorVariantInfo(variantName={:?}, status={}, schemaName={:?}, suffix={:?})",
            self.variant_name(),
            self.status(),
            self.schema_name(),
            self.suffix()
        )
    }
}

/// Introspection metadata for a single `struct` declaration.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct StructInfo {
    pub(crate) inner: parsmith::StructInfo,
    pub(crate) rest_endpoints: Vec<parsmith::RestEndpointDecl>,
}

impl StructInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        let fields: Vec<JsonValue> = self
            .fields()
            .into_iter()
            .map(|f| f.to_camel_json())
            .collect();
        let init_fields: Vec<JsonValue> = self
            .init_fields()
            .into_iter()
            .map(|i| i.to_camel_json())
            .collect();
        let checks: Vec<JsonValue> = self
            .checks()
            .into_iter()
            .map(|c| c.to_camel_json())
            .collect();
        let requests: Vec<JsonValue> = self
            .requests()
            .into_iter()
            .map(|r| r.to_camel_json())
            .collect();
        let rest_endpoints: Vec<JsonValue> = self
            .rest_endpoints()
            .into_iter()
            .map(|e| e.to_camel_json())
            .collect();
        let errors: Vec<JsonValue> = self
            .errors()
            .into_iter()
            .map(|e| e.to_camel_json())
            .collect();
        json!({
            "name": self.name(),
            "kind": self.kind(),
            "doc": self.doc(),
            "keepOrder": self.keep_order(),
            "fields": fields,
            "initFields": init_fields,
            "checks": checks,
            "hasPreValidate": self.has_pre_validate(),
            "keyType": self.key_type().map(|k| k.to_camel_json()),
            "valueType": self.value_type().map(|v| v.to_camel_json()),
            "requests": requests,
            "restEndpoints": rest_endpoints,
            "errors": errors,
        })
    }
}

#[napi]
impl StructInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter)]
    pub fn kind(&self) -> String {
        self.inner.kind.as_str().to_owned()
    }

    #[napi(getter)]
    pub fn doc(&self) -> Option<String> {
        self.inner.doc.clone()
    }

    #[napi(getter, js_name = "keepOrder")]
    pub fn keep_order(&self) -> bool {
        self.inner.keep_order
    }

    #[napi(getter)]
    pub fn fields(&self) -> Vec<FieldInfo> {
        self.inner
            .fields
            .iter()
            .cloned()
            .map(|inner| FieldInfo { inner })
            .collect()
    }

    #[napi(getter, js_name = "initFields")]
    pub fn init_fields(&self) -> Vec<InitFieldInfo> {
        self.inner
            .init_fields
            .iter()
            .cloned()
            .map(|inner| InitFieldInfo { inner })
            .collect()
    }

    #[napi(getter)]
    pub fn checks(&self) -> Vec<CheckMethodInfo> {
        self.inner
            .checks
            .iter()
            .cloned()
            .map(|inner| CheckMethodInfo { inner })
            .collect()
    }

    #[napi(getter, js_name = "hasPreValidate")]
    pub fn has_pre_validate(&self) -> bool {
        self.inner.has_pre_validate
    }

    #[napi(getter, js_name = "keyType")]
    pub fn key_type(&self) -> Option<TypeInfo> {
        self.inner.key_type.clone().map(|inner| TypeInfo { inner })
    }

    #[napi(getter, js_name = "valueType")]
    pub fn value_type(&self) -> Option<TypeInfo> {
        self.inner
            .value_type
            .clone()
            .map(|inner| TypeInfo { inner })
    }

    #[napi(getter)]
    pub fn requests(&self) -> Vec<RequestInfo> {
        self.inner
            .requests
            .iter()
            .cloned()
            .map(|inner| RequestInfo { inner })
            .collect()
    }

    #[napi(getter, js_name = "restEndpoints")]
    pub fn rest_endpoints(&self) -> Vec<RestEndpointInfo> {
        self.rest_endpoints
            .iter()
            .cloned()
            .map(|inner| RestEndpointInfo { inner })
            .collect()
    }

    #[napi(getter)]
    pub fn errors(&self) -> Vec<ErrorVariantInfo> {
        self.inner
            .errors
            .iter()
            .cloned()
            .map(|inner| ErrorVariantInfo { inner })
            .collect()
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "StructInfo(name={:?}, kind={:?}, fields={}, requests={}, errors={})",
            self.name(),
            self.kind(),
            self.inner.fields.len(),
            self.inner.requests.len(),
            self.inner.errors.len()
        )
    }
}

/// Introspection metadata for a standalone function parameter.
#[napi]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnParamInfo {
    pub(crate) inner: parsmith::FnParam,
}

impl FnParamInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "name": self.name(),
            "typeInfo": self.type_info().to_camel_json(),
        })
    }
}

#[napi]
impl FnParamInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "typeInfo")]
    pub fn type_info(&self) -> TypeInfo {
        TypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "FnParamInfo(name={:?}, typeInfo={})",
            self.name(),
            self.type_info().to_string_repr()
        )
    }
}

/// Introspection metadata for a standalone `fn` or `(raw)fn` declaration.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionInfo {
    pub(crate) inner: parsmith::FunctionInfo,
}

impl FunctionInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        let params: Vec<JsonValue> = self
            .params()
            .into_iter()
            .map(|p| p.to_camel_json())
            .collect();
        json!({
            "name": self.name(),
            "isRaw": self.is_raw(),
            "doc": self.doc(),
            "params": params,
            "returnType": self.return_type().to_camel_json(),
        })
    }
}

#[napi]
impl FunctionInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "isRaw")]
    pub fn is_raw(&self) -> bool {
        self.inner.is_raw
    }

    #[napi(getter)]
    pub fn doc(&self) -> Option<String> {
        self.inner.doc.clone()
    }

    #[napi(getter)]
    pub fn params(&self) -> Vec<FnParamInfo> {
        self.inner
            .params
            .iter()
            .cloned()
            .map(|inner| FnParamInfo { inner })
            .collect()
    }

    #[napi(getter, js_name = "returnType")]
    pub fn return_type(&self) -> TypeInfo {
        TypeInfo {
            inner: self.inner.return_type.clone(),
        }
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "FunctionInfo(name={:?}, isRaw={}, returnType={})",
            self.name(),
            self.is_raw(),
            self.return_type().to_string_repr()
        )
    }
}

/// Introspection metadata for a field inside a `JsonDefInfo`.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct JsonFieldInfo {
    pub(crate) inner: parsmith::JsonFieldInfo,
}

impl JsonFieldInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "name": self.name(),
            "typeInfo": self.type_info().to_camel_json(),
            "alias": self.alias(),
            "doc": self.doc(),
            "isOptional": self.is_optional(),
            "omitempty": self.omitempty(),
            "skip": self.skip(),
            "isDict": self.is_dict(),
        })
    }
}

#[napi]
impl JsonFieldInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "typeInfo")]
    pub fn type_info(&self) -> TypeInfo {
        TypeInfo {
            inner: self.inner.type_info.clone(),
        }
    }

    #[napi(getter)]
    pub fn alias(&self) -> Option<String> {
        self.inner.alias.clone()
    }

    #[napi(getter)]
    pub fn doc(&self) -> Option<String> {
        self.inner.doc.clone()
    }

    #[napi(getter, js_name = "isOptional")]
    pub fn is_optional(&self) -> bool {
        self.inner.is_optional
    }

    #[napi(getter)]
    pub fn omitempty(&self) -> bool {
        self.inner.omitempty
    }

    #[napi(getter)]
    pub fn skip(&self) -> bool {
        self.inner.skip
    }

    #[napi(getter, js_name = "isDict")]
    pub fn is_dict(&self) -> bool {
        self.inner.is_dict
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "JsonFieldInfo(name={:?}, typeInfo={}, alias={:?}, isOptional={}, omitempty={}, skip={})",
            self.name(),
            self.type_info().to_string_repr(),
            self.alias(),
            self.is_optional(),
            self.omitempty(),
            self.skip()
        )
    }
}

/// Introspection metadata for a top-level or hoisted `json` schema.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct JsonDefInfo {
    pub(crate) inner: parsmith::JsonDefInfo,
}

impl JsonDefInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        let fields: Vec<JsonValue> = self
            .fields()
            .into_iter()
            .map(|f| f.to_camel_json())
            .collect();
        json!({
            "name": self.name(),
            "kind": self.kind(),
            "doc": self.doc(),
            "isArray": self.is_array(),
            "isDict": self.is_dict(),
            "isHoisted": self.is_hoisted(),
            "path": self.path(),
            "fields": fields,
            "keyType": self.key_type().map(|k| k.to_camel_json()),
            "valueType": self.value_type().map(|v| v.to_camel_json()),
        })
    }
}

#[napi]
impl JsonDefInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter)]
    pub fn kind(&self) -> String {
        self.inner.kind.as_str().to_owned()
    }

    #[napi(getter)]
    pub fn doc(&self) -> Option<String> {
        self.inner.doc.clone()
    }

    #[napi(getter, js_name = "isArray")]
    pub fn is_array(&self) -> bool {
        self.inner.is_array
    }

    #[napi(getter, js_name = "isDict")]
    pub fn is_dict(&self) -> bool {
        self.inner.is_dict
    }

    #[napi(getter, js_name = "isHoisted")]
    pub fn is_hoisted(&self) -> bool {
        self.inner.is_hoisted
    }

    #[napi(getter)]
    pub fn path(&self) -> Option<String> {
        self.inner.path.clone()
    }

    #[napi(getter)]
    pub fn fields(&self) -> Vec<JsonFieldInfo> {
        self.inner
            .fields
            .iter()
            .cloned()
            .map(|inner| JsonFieldInfo { inner })
            .collect()
    }

    #[napi(getter, js_name = "keyType")]
    pub fn key_type(&self) -> Option<TypeInfo> {
        self.inner.key_type.clone().map(|inner| TypeInfo { inner })
    }

    #[napi(getter, js_name = "valueType")]
    pub fn value_type(&self) -> Option<TypeInfo> {
        self.inner
            .value_type
            .clone()
            .map(|inner| TypeInfo { inner })
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "JsonDefInfo(name={:?}, kind={:?}, isHoisted={}, fields={})",
            self.name(),
            self.kind(),
            self.is_hoisted(),
            self.inner.fields.len()
        )
    }
}

/// Introspection metadata for a single operation inside an `ExtensionInfo`.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionOpInfo {
    pub(crate) inner: parsmith::ExtensionOpInfo,
}

impl ExtensionOpInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        json!({
            "name": self.name(),
            "qualifiedName": self.qualified_name(),
            "inputType": self.input_type(),
            "outputType": self.output_type(),
            "isGeneric": self.is_generic(),
            "targets": self.targets(),
        })
    }
}

#[napi]
impl ExtensionOpInfo {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[napi(getter, js_name = "qualifiedName")]
    pub fn qualified_name(&self) -> String {
        self.inner.qualified_name.clone()
    }

    #[napi(getter, js_name = "inputType")]
    pub fn input_type(&self) -> String {
        self.inner.input.expected_name()
    }

    #[napi(getter, js_name = "outputType")]
    pub fn output_type(&self) -> String {
        self.inner.output.expected_name()
    }

    #[napi(getter, js_name = "isGeneric")]
    pub fn is_generic(&self) -> bool {
        self.inner.input.is_generic()
    }

    #[napi(getter)]
    pub fn targets(&self) -> Vec<String> {
        self.inner.targets.clone()
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "ExtensionOpInfo(qualifiedName={:?}, input={:?}, output={:?}, targets={:?})",
            self.qualified_name(),
            self.input_type(),
            self.output_type(),
            self.targets()
        )
    }
}

/// Introspection metadata for a top-level `extension` declaration.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionInfo {
    pub(crate) inner: parsmith::ExtensionInfo,
}

impl ExtensionInfo {
    pub(crate) fn to_camel_json(&self) -> JsonValue {
        let ops: Vec<JsonValue> = self
            .operations()
            .into_iter()
            .map(|o| o.to_camel_json())
            .collect();
        json!({
            "namespace": self.namespace(),
            "name": self.name(),
            "operations": ops,
        })
    }
}

#[napi]
impl ExtensionInfo {
    #[napi(getter)]
    pub fn namespace(&self) -> String {
        self.inner.namespace.clone()
    }

    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.namespace.clone()
    }

    #[napi(getter)]
    pub fn operations(&self) -> Vec<ExtensionOpInfo> {
        self.inner
            .operations
            .iter()
            .cloned()
            .map(|inner| ExtensionOpInfo { inner })
            .collect()
    }

    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        serialize_to_json_value(&self.inner)
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "ExtensionInfo(namespace={:?}, operations={})",
            self.namespace(),
            self.inner.operations.len()
        )
    }
}

/// Top-level serializable schema introspection metadata returned by `parsmith.inspectSchema()`.
#[napi]
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleInfo {
    pub(crate) inner: parsmith::ModuleInfo,
    pub(crate) rest_endpoints_by_struct: BTreeMap<String, Vec<parsmith::RestEndpointDecl>>,
    pub(crate) sources: BTreeMap<String, String>,
}

impl ModuleInfo {
    pub(crate) fn from_check_result(mut result: parsmith::CheckResult) -> Self {
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

    pub(crate) fn to_camel_json(&self) -> JsonValue {
        let structs: Vec<JsonValue> = self
            .structs()
            .into_iter()
            .map(|s| s.to_camel_json())
            .collect();
        let functions: Vec<JsonValue> = self
            .functions()
            .into_iter()
            .map(|f| f.to_camel_json())
            .collect();
        let json_defs: Vec<JsonValue> = self
            .json_defs()
            .into_iter()
            .map(|j| j.to_camel_json())
            .collect();
        let extensions: Vec<JsonValue> = self
            .extensions()
            .into_iter()
            .map(|e| e.to_camel_json())
            .collect();
        let diagnostics: Vec<JsonValue> = self
            .diagnostics()
            .into_iter()
            .map(|d| d.to_camel_json())
            .collect();
        json!({
            "ok": self.ok(),
            "doc": self.doc(),
            "sourceFile": self.source_file(),
            "structKinds": self.struct_kinds(),
            "structs": structs,
            "functions": functions,
            "jsonDefs": json_defs,
            "extensions": extensions,
            "diagnostics": diagnostics,
        })
    }
}

#[napi]
impl ModuleInfo {
    #[napi(getter)]
    pub fn doc(&self) -> Option<String> {
        self.inner.doc.clone()
    }

    #[napi(getter, js_name = "sourceFile")]
    pub fn source_file(&self) -> String {
        self.inner.source_file.clone()
    }

    #[napi(getter, js_name = "structKinds")]
    pub fn struct_kinds(&self) -> JsonValue {
        let mut map = JsonMap::new();
        for (name, kind) in &self.inner.struct_kinds {
            map.insert(name.clone(), JsonValue::String(kind.as_str().to_owned()));
        }
        JsonValue::Object(map)
    }

    #[napi(getter)]
    pub fn structs(&self) -> Vec<StructInfo> {
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
                StructInfo {
                    inner: s,
                    rest_endpoints: eps,
                }
            })
            .collect()
    }

    #[napi(getter)]
    pub fn functions(&self) -> Vec<FunctionInfo> {
        self.inner
            .functions
            .iter()
            .cloned()
            .map(|inner| FunctionInfo { inner })
            .collect()
    }

    #[napi(getter, js_name = "jsonDefs")]
    pub fn json_defs(&self) -> Vec<JsonDefInfo> {
        self.inner
            .json_defs
            .iter()
            .cloned()
            .map(|inner| JsonDefInfo { inner })
            .collect()
    }

    #[napi(getter)]
    pub fn extensions(&self) -> Vec<ExtensionInfo> {
        self.inner
            .extensions
            .iter()
            .cloned()
            .map(|inner| ExtensionInfo { inner })
            .collect()
    }

    #[napi(getter)]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.inner
            .diagnostics
            .iter()
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.sources))
            .collect()
    }

    #[napi(getter)]
    pub fn errors(&self) -> Vec<Diagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_error())
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.sources))
            .collect()
    }

    #[napi(getter)]
    pub fn warnings(&self) -> Vec<Diagnostic> {
        self.inner
            .diagnostics
            .iter()
            .filter(|d| d.severity.is_warning())
            .cloned()
            .map(|d| Diagnostic::from_core(d, &self.sources))
            .collect()
    }

    #[napi(getter)]
    pub fn ok(&self) -> bool {
        !self.has_errors()
    }

    #[napi(js_name = "hasErrors")]
    pub fn has_errors(&self) -> bool {
        self.inner.diagnostics.iter().any(|d| d.severity.is_error())
    }

    #[napi(js_name = "hasWarnings")]
    pub fn has_warnings(&self) -> bool {
        self.inner
            .diagnostics
            .iter()
            .any(|d| d.severity.is_warning())
    }

    #[napi(js_name = "isOk")]
    pub fn is_ok(&self) -> bool {
        !self.has_errors()
    }

    /// Converts this `ModuleInfo` into a plain JS object with `camelCase` keys.
    #[napi(js_name = "toObject")]
    pub fn to_object(&self) -> JsonValue {
        self.to_camel_json()
    }

    /// Serializes this `ModuleInfo` for `JSON.stringify()`.
    #[napi(js_name = "toJSON")]
    pub fn to_json_val(&self) -> JsonValue {
        self.to_camel_json()
    }

    /// Converts this `ModuleInfo` into a plain dictionary with `snake_case` keys matching Python's `to_dict()`.
    #[napi(js_name = "toDict")]
    pub fn to_dict(&self) -> Result<JsonValue> {
        let mut val = serialize_to_json_value(&self.inner)?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert("ok".to_owned(), JsonValue::Bool(self.ok()));
        }
        Ok(val)
    }

    /// Serializes this `ModuleInfo` as a pretty-printed canonical JSON string.
    #[napi(js_name = "toJson")]
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.inner).map_err(|e| value_err(e.to_string()))
    }

    /// Formats all diagnostics as human-readable Rust-style annotated source snippets.
    #[napi(js_name = "formatText")]
    pub fn format_text(&self) -> String {
        parsmith::CheckResult::new(self.inner.diagnostics.clone())
            .with_sources(self.sources.clone())
            .format_text()
    }

    /// Formats all diagnostics as a pretty-printed JSON string.
    #[napi(js_name = "formatJson")]
    pub fn format_json(&self) -> String {
        parsmith::CheckResult::new(self.inner.diagnostics.clone()).format_json()
    }

    #[napi(js_name = "toString")]
    pub fn to_string_repr(&self) -> String {
        format!(
            "ModuleInfo(ok={}, structs={}, functions={}, jsonDefs={}, extensions={})",
            self.ok(),
            self.inner.structs.len(),
            self.inner.functions.len(),
            self.inner.json_defs.len(),
            self.inner.extensions.len()
        )
    }
}

/// Compiles and introspects a KDL 2.0 schema into structured `ModuleInfo` metadata.
#[napi(js_name = "inspectSchema")]
pub fn inspect_schema(
    env: Env,
    source_or_path: JsUnknown,
    options: Option<JsUnknown>,
) -> Result<ModuleInfo> {
    let (raw_str, is_pathlike) = extract_raw_source_or_path(&env, source_or_path)?;
    let mut vfs_loader: Option<MemorySourceLoader> = None;
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
                        "path" => {
                            logical_path = extract_opt_string(&env, v_unk, "path")?;
                        }
                        other => {
                            return Err(value_err(format!(
                                "Unknown inspectSchema option '{other}'"
                            )));
                        }
                    }
                }
            }
            _ => return Err(type_err(&env, "inspectSchema options must be an object")),
        }
    }

    let mode = resolve_input_mode(&raw_str, is_pathlike, vfs_loader.as_ref());
    let check_opts = CheckOptions::default();

    let check_res = match (mode, vfs_loader) {
        (SourceOrPath::Path(entry), Some(loader)) => {
            check_with_options(&entry, &loader, &check_opts)
        }
        (SourceOrPath::Source(src), Some(loader)) => {
            let lp = logical_path.as_deref().unwrap_or("<memory>.kdl");
            check_source_with_options(&src, Some(lp), &loader, &check_opts)
        }
        (SourceOrPath::Path(entry), None) => {
            let fs_loader = FsSourceLoader::new();
            check_with_options(&entry, &fs_loader, &check_opts)
        }
        (SourceOrPath::Source(src), None) => {
            let fs_loader = FsSourceLoader::new();
            check_source_with_options(&src, logical_path.as_deref(), &fs_loader, &check_opts)
        }
        (SourceOrPath::Paths(_), _) => unreachable!(),
    };

    Ok(ModuleInfo::from_check_result(check_res))
}

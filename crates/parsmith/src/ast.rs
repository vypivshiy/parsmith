//! Immutable `Algebraic AST` representation for compiled KDL Schema DSL v2.1 modules.
//!
//! Free of parent back-pointers and synthetic marker hook nodes (`Utilities`, `StartParse`,
//! `CodeStartHook`, `CodeEndHook`, `InitFieldCall`).

use std::fmt;
use std::str::FromStr;

use crate::kdl::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::gjson::GjsonPath;
use crate::request::{ErrorConditionValue, ErrorResponseSpec, PlaceholderSpec, RequestSpec};

/// Primitive and structural base types in the KDL Schema DSL v2.1 type system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "ref", rename_all = "lowercase")]
pub enum BaseType {
    /// Type inferred automatically or unresolved (`AUTO`).
    #[default]
    Auto,
    /// HTML/XML DOM document or element (`DOCUMENT`).
    Document,
    /// UTF-8 string (`STRING`).
    String,
    /// 64-bit signed integer (`INT`).
    Int,
    /// 64-bit IEEE-754 floating-point number (`FLOAT`).
    Float,
    /// Boolean (`BOOL`).
    Bool,
    /// Null / None / nil (`NULL`).
    Null,
    /// Parsed result of a nested `struct` (`NESTED`), carrying the referenced struct name.
    Nested(String),
    /// Projected result of a `json` schema (`JSON`), carrying the referenced `JsonDef` name.
    Json(String),
    /// Inline dictionary value (`(dict)` JSON field).
    DictValue,
}

impl BaseType {
    /// Returns the uppercase DSL type category name used in compiler diagnostics
    /// (`"AUTO"`, `"DOCUMENT"`, `"STRING"`, `"INT"`, `"FLOAT"`, `"BOOL"`, `"NULL"`, `"NESTED"`, `"JSON"`, `"DICT"`).
    pub const fn dsl_name(&self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::Document => "DOCUMENT",
            Self::String => "STRING",
            Self::Int => "INT",
            Self::Float => "FLOAT",
            Self::Bool => "BOOL",
            Self::Null => "NULL",
            Self::Nested(_) => "NESTED",
            Self::Json(_) => "JSON",
            Self::DictValue => "DICT",
        }
    }

    /// Returns the referenced `struct` or `json` schema name if this is [`BaseType::Nested`] or [`BaseType::Json`].
    pub fn ref_name(&self) -> Option<&str> {
        match self {
            Self::Nested(name) | Self::Json(name) => Some(name.as_str()),
            _ => None,
        }
    }

    /// Checks whether two base types belong to the same category (ignoring inner `Nested`/`Json` reference names).
    pub const fn same_category(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Auto, Self::Auto)
                | (Self::Document, Self::Document)
                | (Self::String, Self::String)
                | (Self::Int, Self::Int)
                | (Self::Float, Self::Float)
                | (Self::Bool, Self::Bool)
                | (Self::Null, Self::Null)
                | (Self::Nested(_), Self::Nested(_))
                | (Self::Json(_), Self::Json(_))
                | (Self::DictValue, Self::DictValue)
        )
    }
}

impl fmt::Display for BaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nested(name) => write!(f, "NESTED({name})"),
            Self::Json(name) => write!(f, "JSON({name})"),
            other => f.write_str(other.dsl_name()),
        }
    }
}

/// Complete type descriptor for pipeline steps, struct fields, function signatures, and JSON fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct TypeInfo {
    /// Primary base type category.
    pub base: BaseType,
    /// Whether this type represents a list/array (`list[T]` / `T[]`).
    #[serde(default)]
    pub is_array: bool,
    /// Whether this type is nullable/optional (`T | None` / `T | null`).
    #[serde(default)]
    pub is_optional: bool,
    /// Whether this type represents a dynamic dictionary (`dict[K, V]` / `Record<K, V>`).
    #[serde(default)]
    pub is_dict: bool,
    /// Referenced `struct` or `json` name (mirrors [`BaseType::Nested`] / [`BaseType::Json`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Dictionary key type when `is_dict == true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_type: Option<Box<TypeInfo>>,
    /// Dictionary value type when `is_dict == true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<Box<TypeInfo>>,
    /// Whether the JSON field carries `@omitempty` (key may be absent from JSON payload).
    #[serde(default)]
    pub omitempty: bool,
    /// Whether the JSON field carries `@skip` (field omitted from public model output).
    #[serde(default)]
    pub skip: bool,
}

impl TypeInfo {
    /// Creates an unresolved `AUTO` type.
    pub const fn auto() -> Self {
        Self {
            base: BaseType::Auto,
            is_array: false,
            is_optional: false,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a scalar `DOCUMENT` type.
    pub const fn document() -> Self {
        Self {
            base: BaseType::Document,
            is_array: false,
            is_optional: false,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a scalar `STRING` type.
    pub const fn string() -> Self {
        Self {
            base: BaseType::String,
            is_array: false,
            is_optional: false,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a scalar `INT` type.
    pub const fn int() -> Self {
        Self {
            base: BaseType::Int,
            is_array: false,
            is_optional: false,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a scalar `FLOAT` type.
    pub const fn float() -> Self {
        Self {
            base: BaseType::Float,
            is_array: false,
            is_optional: false,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a scalar `BOOL` type.
    pub const fn bool() -> Self {
        Self {
            base: BaseType::Bool,
            is_array: false,
            is_optional: false,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a `NULL` type.
    pub const fn null() -> Self {
        Self {
            base: BaseType::Null,
            is_array: false,
            is_optional: false,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a `NESTED` struct reference type.
    pub fn nested(struct_name: impl Into<String>, is_array: bool) -> Self {
        let name = struct_name.into();
        Self {
            base: BaseType::Nested(name.clone()),
            is_array,
            is_optional: false,
            is_dict: false,
            reference: Some(name),
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a `JSON` schema reference type.
    pub fn json(schema_name: impl Into<String>, is_array: bool) -> Self {
        let name = schema_name.into();
        Self {
            base: BaseType::Json(name.clone()),
            is_array,
            is_optional: false,
            is_dict: false,
            reference: Some(name),
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Creates a dictionary type (`dict[K, V]` / `Record<K, V>`).
    pub fn dict(key_type: TypeInfo, value_type: TypeInfo) -> Self {
        Self {
            base: BaseType::DictValue,
            is_array: false,
            is_optional: false,
            is_dict: true,
            reference: None,
            key_type: Some(Box::new(key_type)),
            value_type: Some(Box::new(value_type)),
            omitempty: false,
            skip: false,
        }
    }

    /// Sets `is_array` on this `TypeInfo`.
    pub fn with_array(mut self, is_array: bool) -> Self {
        self.is_array = is_array;
        self
    }

    /// Sets `is_optional` on this `TypeInfo`.
    pub fn with_optional(mut self, is_optional: bool) -> Self {
        self.is_optional = is_optional;
        self
    }

    /// Sets `omitempty` on this `TypeInfo`.
    pub fn with_omitempty(mut self, omitempty: bool) -> Self {
        self.omitempty = omitempty;
        self
    }

    /// Sets `skip` on this `TypeInfo`.
    pub fn with_skip(mut self, skip: bool) -> Self {
        self.skip = skip;
        self
    }

    /// Returns the referenced `struct` or `json` name if present.
    pub fn ref_name(&self) -> Option<&str> {
        self.reference.as_deref().or_else(|| self.base.ref_name())
    }
}

/// Struct parsing strategy (`item`, `list`, `dict`, `table`, `flat`, `raw`, `rest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StructKind {
    /// Single object parser (`(item)struct` or `struct`).
    #[default]
    Item,
    /// Repeating item list parser (`(list)struct`).
    List,
    /// Key-value dictionary parser (`(dict)struct`).
    Dict,
    /// Tabular key-matching row parser (`(table)struct`).
    Table,
    /// Deduplicated flat list parser (`(flat)struct`).
    Flat,
    /// Plain-text parser (`(raw)struct`).
    Raw,
    /// Monadic HTTP REST client (`(rest)struct`).
    Rest,
}

impl StructKind {
    /// Returns the lowercase DSL annotation name (`"item"`, `"list"`, `"dict"`, `"table"`, `"flat"`, `"raw"`, `"rest"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::List => "list",
            Self::Dict => "dict",
            Self::Table => "table",
            Self::Flat => "flat",
            Self::Raw => "raw",
            Self::Rest => "rest",
        }
    }

    /// Returns `true` if `nested <ThisStruct>` produces an array of items (`List` or `Flat`).
    pub const fn produces_array(self) -> bool {
        matches!(self, Self::List | Self::Flat)
    }

    /// Returns the initial pipeline input [`TypeInfo`] for fields in this struct kind.
    pub const fn start_type(self) -> TypeInfo {
        match self {
            Self::Raw => TypeInfo::string(),
            _ => TypeInfo::document(),
        }
    }
}

impl fmt::Display for StructKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StructKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "item" => Ok(Self::Item),
            "list" => Ok(Self::List),
            "dict" => Ok(Self::Dict),
            "table" => Ok(Self::Table),
            "flat" => Ok(Self::Flat),
            "raw" => Ok(Self::Raw),
            "rest" => Ok(Self::Rest),
            other => Err(format!("unknown struct kind '{other}'")),
        }
    }
}

/// Container flavor of a top-level [`JsonDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JsonKind {
    /// Standard JSON object (`json Name { ... }`).
    #[default]
    Object,
    /// Array of JSON objects (`(array)json Name { ... }`).
    Array,
    /// Dynamic key-value dictionary (`(dict)json Name { @key ...; @value ... }`).
    Dict,
}

impl JsonKind {
    /// Returns `"object"`, `"array"`, or `"dict"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::Array => "array",
            Self::Dict => "dict",
        }
    }
}

/// Top-level or hoisted JSON schema definition (`json`, `(array)json`, `(dict)json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonDef {
    /// Schema identifier (e.g. `"User"`, or hoisted `"AnimeResponseTranslationsValue"`).
    pub name: String,
    /// Container kind (`Object`, `Array`, `Dict`).
    pub kind: JsonKind,
    /// Optional documentation comment (`doc="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Whether this schema is `(array)json`.
    pub is_array: bool,
    /// Whether this schema is `(dict)json`.
    pub is_dict: bool,
    /// Optional root GJSON extraction dot-path (`path="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Pre-parsed [`GjsonPath`] when `path` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsed_path: Option<GjsonPath>,
    /// Field declarations for `Object` and `Array` JSON schemas.
    #[serde(default)]
    pub fields: Vec<JsonFieldDef>,
    /// Key type for `(dict)json` schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_type: Option<TypeInfo>,
    /// Value type for `(dict)json` schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<TypeInfo>,
    /// Whether this `JsonDef` was hoisted from an inline JSON or `@value` block.
    #[serde(default)]
    pub is_hoisted: bool,
    /// Source span of the declaration.
    pub span: Span,
}

/// Field declaration inside a [`JsonDef`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonFieldDef {
    /// Canonical field name in the generated model (without trailing `?`).
    pub name: String,
    /// Resolved field [`TypeInfo`].
    pub type_info: TypeInfo,
    /// Optional wire key or GJSON dot-path alias (`from="..."` or positional alias).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Pre-parsed [`GjsonPath`] for `alias` (or direct key `name` when `alias` is `None`).
    pub gjson_path: GjsonPath,
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
    /// Key type when `is_dict == true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_type: Option<TypeInfo>,
    /// Value type when `is_dict == true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<TypeInfo>,
    /// Optional constant literal value (`value=<literal>`) injected directly by the projector without reading wire JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<LiteralValue>,
    /// Source span of the field node.
    pub span: Span,
}

/// Type signature pattern for an `extension` operation's input or output (`str`, `(array)int`, `str?`, `T`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExtensionTypeSig {
    /// Concrete base type when not generic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<BaseType>,
    /// Generic type parameter name (e.g. `Some("T")`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generic: Option<String>,
    /// Whether the signature specifies `(array)`.
    #[serde(default)]
    pub is_array: bool,
    /// Whether the signature specifies `?`.
    #[serde(default)]
    pub is_optional: bool,
}

impl ExtensionTypeSig {
    /// Returns `true` if this signature is generic (`T`).
    pub fn is_generic(&self) -> bool {
        self.generic.is_some()
    }

    /// Checks whether an incoming pipeline [`TypeInfo`] satisfies this signature constraint.
    pub fn accepts(&self, value: &TypeInfo) -> bool {
        if self.generic.is_some() {
            return true;
        }
        let Some(ref expected_base) = self.base else {
            return true;
        };
        let base_ok = value.base == BaseType::Auto || value.base.same_category(expected_base);
        base_ok && value.is_array == self.is_array && value.is_optional == self.is_optional
    }

    /// Resolves this signature into a concrete [`TypeInfo`], substituting `binding` when generic.
    pub fn resolve(&self, binding: Option<&TypeInfo>) -> TypeInfo {
        if self.generic.is_some() {
            return binding.cloned().unwrap_or_else(TypeInfo::auto);
        }
        TypeInfo {
            base: self.base.clone().unwrap_or(BaseType::Auto),
            is_array: self.is_array,
            is_optional: self.is_optional,
            is_dict: false,
            reference: None,
            key_type: None,
            value_type: None,
            omitempty: false,
            skip: false,
        }
    }

    /// Returns a human-readable expected type name for diagnostics.
    pub fn expected_name(&self) -> String {
        if let Some(ref g) = self.generic {
            return g.clone();
        }
        let base_name = self.base.as_ref().map_or("AUTO", BaseType::dsl_name);
        let mut out = if self.is_array {
            format!("LIST_{base_name}")
        } else {
            base_name.to_string()
        };
        if self.is_optional {
            out.push('?');
        }
        out
    }
}

/// Import statement inside an `extension` target block or helper.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExtensionImport {
    /// Import path or statement string.
    pub value: String,
    /// Optional package alias (`alias="..."` for Go/Rust).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

/// Helper function definition inside an `extension` target block.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExtensionHelper {
    /// Helper function identifier.
    pub name: String,
    /// Helper function source code.
    pub source: String,
    /// Imports required by this helper.
    #[serde(default)]
    pub imports: Vec<ExtensionImport>,
}

/// Target-language implementation block (`py`, `js`, `ts`, `go`, `rust`) inside an [`ExtensionOp`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionTargetBlock {
    /// Target language key (`"py"`, `"js"`, `"ts"`, `"go"`, `"rust"`).
    pub language: String,
    /// Code emission template containing `{{in}}` and `{{out}}` placeholders.
    pub emit: String,
    /// Required imports for this target.
    #[serde(default)]
    pub imports: Vec<ExtensionImport>,
    /// Required helper functions for this target.
    #[serde(default)]
    pub helpers: Vec<ExtensionHelper>,
}

/// A single operation inside an [`ExtensionDecl`] (`extension Namespace { op { ... } }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionOp {
    /// Enclosing extension namespace.
    pub namespace: String,
    /// Operation name.
    pub name: String,
    /// Qualified `"Namespace.op"` identifier.
    pub qualified_name: String,
    /// Input type constraint (`sig <input> <output>`).
    pub input: ExtensionTypeSig,
    /// Output type signature (`sig <input> <output>`).
    pub output: ExtensionTypeSig,
    /// Target-language code blocks keyed by language (`"py"`, `"js"`, `"ts"`, `"go"`, `"rust"`).
    ///
    /// When a `"js"` block is declared and `"ts"` is omitted, `"ts"` automatically resolves to `"js"`.
    pub targets: IndexMap<String, ExtensionTargetBlock>,
    /// Source span of the operation node.
    pub span: Span,
}

impl ExtensionOp {
    /// Resolves the output [`TypeInfo`] for a given `input_type`, binding generic `T` per call site.
    pub fn resolve_return(&self, input_type: &TypeInfo) -> TypeInfo {
        if !self.input.accepts(input_type) {
            return TypeInfo::auto();
        }
        let binding = if self.input.is_generic() {
            Some(input_type)
        } else {
            None
        };
        self.output.resolve(binding)
    }

    /// Looks up a target block by language name (`"py"`/`"python"`, `"ts"`/`"typescript"`, `"js"`/`"javascript"`, `"go"`, `"rust"`, `"ex"`/`"elixir"`, `"php"`, `"cs"`/`"csharp"`, `"kt"`/`"kotlin"`),
    /// automatically falling back from `"ts"` to `"js"` when `"ts"` is omitted.
    pub fn target(&self, lang: &str) -> Option<&ExtensionTargetBlock> {
        match lang {
            "py" | "python" => self.targets.get("py"),
            "ts" | "typescript" => self.targets.get("ts").or_else(|| self.targets.get("js")),
            "js" | "javascript" => self.targets.get("js"),
            "go" | "golang" => self.targets.get("go"),
            "rust" | "rs" => self.targets.get("rust"),
            "ex" | "elixir" => self
                .targets
                .get("ex")
                .or_else(|| self.targets.get("elixir")),
            "php" => self.targets.get("php"),
            "cs" | "csharp" => self
                .targets
                .get("cs")
                .or_else(|| self.targets.get("csharp")),
            "kt" | "kotlin" => self
                .targets
                .get("kt")
                .or_else(|| self.targets.get("kotlin")),
            other => self.targets.get(other),
        }
    }
}

/// Top-level `extension Namespace { ... }` declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionDecl {
    /// Extension namespace identifier.
    pub namespace: String,
    /// Declared operations in this namespace.
    pub operations: Vec<ExtensionOp>,
    /// Source span of the `extension` declaration.
    pub span: Span,
}

/// Terminal fallback literal value in a [`Pipeline`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum LiteralValue {
    /// `fallback #null`.
    Null,
    /// `fallback #true` or `fallback #false`.
    Bool(bool),
    /// `fallback 0`, `fallback 42`, etc.
    Int(i64),
    /// `fallback 0.0`, `fallback 3.14`, etc.
    Float(f64),
    /// `fallback ""`, `fallback "default"`, etc.
    String(String),
    /// `fallback {}` (empty list fallback).
    EmptyArray,
}

/// Extraction mode for the `raw` DOM operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RawMode {
    /// Outer HTML including the element's own tag (`raw` or `raw outer`).
    #[default]
    Outer,
    /// Inner HTML of the element's children (`raw inner`).
    Inner,
}

impl RawMode {
    /// Returns `"outer"` or `"inner"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Outer => "outer",
            Self::Inner => "inner",
        }
    }
}

/// Scalar literal value inside a comparison condition (`eq`, `ne`, `in`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CondValue {
    /// Integer comparison value.
    Int(i64),
    /// Float comparison value.
    Float(f64),
    /// Boolean comparison value.
    Bool(bool),
    /// String comparison value.
    String(String),
}

impl CondValue {
    /// Returns the string representation of this condition value.
    pub fn as_str_cow(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::String(s) => std::borrow::Cow::Borrowed(s.as_str()),
            Self::Int(i) => std::borrow::Cow::Owned(i.to_string()),
            Self::Float(f) => std::borrow::Cow::Owned(f.to_string()),
            Self::Bool(b) => std::borrow::Cow::Owned(b.to_string()),
        }
    }
}

/// Leaf condition operation inside `filter`, `assert`, or `match`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum CondOp {
    /// `eq <v1> [v2 ...]`.
    Eq { values: Vec<CondValue> },
    /// `ne <v1> [v2 ...]`.
    Ne { values: Vec<CondValue> },
    /// `in <v1> [v2 ...]`.
    In { values: Vec<CondValue> },
    /// `gt <n>` (`assert` only).
    Gt { value: i64 },
    /// `lt <n>` (`assert` only).
    Lt { value: i64 },
    /// `ge <n>` (`assert` only).
    Ge { value: i64 },
    /// `le <n>` (`assert` only).
    Le { value: i64 },
    /// `starts <s1> [s2 ...]`.
    Starts { values: Vec<String> },
    /// `ends <s1> [s2 ...]`.
    Ends { values: Vec<String> },
    /// `contains <s1> [s2 ...]`.
    Contains { values: Vec<String> },
    /// `re <pattern>`.
    Re { pattern: String },
    /// `re-any <pattern>` (`assert` only).
    ReAny { pattern: String },
    /// `re-all <pattern>` (`assert` only).
    ReAll { pattern: String },
    /// `css <selector>`.
    Css { query: String },
    /// `xpath <expr>`.
    Xpath { query: String },
    /// `has-attr <a1> [a2 ...]`.
    HasAttr { attrs: Vec<String> },
    /// `attr-eq <name> <v1> [v2 ...]`.
    AttrEq { name: String, values: Vec<String> },
    /// `attr-ne <name> <v1> [v2 ...]`.
    AttrNe { name: String, values: Vec<String> },
    /// `attr-in <name> <v1> [v2 ...]`.
    AttrIn { name: String, values: Vec<String> },
    /// `attr-starts <name> <v1> [v2 ...]`.
    AttrStarts { name: String, values: Vec<String> },
    /// `attr-ends <name> <v1> [v2 ...]`.
    AttrEnds { name: String, values: Vec<String> },
    /// `attr-contains <name> <v1> [v2 ...]`.
    AttrContains { name: String, values: Vec<String> },
    /// `attr-re <name> <pattern>`.
    AttrRe { name: String, pattern: String },
    /// `text-eq <v1> [v2 ...]`.
    TextEq { values: Vec<String> },
    /// `text-ne <v1> [v2 ...]`.
    TextNe { values: Vec<String> },
    /// `text-in <v1> [v2 ...]`.
    TextIn { values: Vec<String> },
    /// `text-starts <v1> [v2 ...]`.
    TextStarts { values: Vec<String> },
    /// `text-ends <v1> [v2 ...]`.
    TextEnds { values: Vec<String> },
    /// `text-contains <v1> [v2 ...]`.
    TextContains { values: Vec<String> },
    /// `text-re <pattern>`.
    TextRe { pattern: String },
    /// `len-eq <n1> [n2 ...]` (`assert` only).
    LenEq { values: Vec<i64> },
    /// `len-ne <n1> [n2 ...]` (`assert` only).
    LenNe { values: Vec<i64> },
    /// `len-gt <n>` (`assert` only).
    LenGt { value: i64 },
    /// `len-lt <n>` (`assert` only).
    LenLt { value: i64 },
    /// `len-ge <n>` (`assert` only).
    LenGe { value: i64 },
    /// `len-le <n>` (`assert` only).
    LenLe { value: i64 },
    /// `len-range <start> <end>` (`assert` only).
    LenRange { start: i64, end: i64 },
}

/// Recursive algebraic condition expression tree (`And`, `Or`, `Not`, `Atom`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "expr", rename_all = "lowercase")]
pub enum Cond {
    /// Conjunction (`and { ... }` or multiple child conditions in a block).
    And(Vec<Cond>),
    /// Disjunction (`or { ... }`).
    Or(Vec<Cond>),
    /// Negation (`not { ... }`).
    Not(Box<Cond>),
    /// Atomic condition operation.
    Atom(CondOp),
}

impl Cond {
    /// Combines a list of condition expressions using conjunction (`And`) if `exprs.len() > 1`,
    /// or unwraps the single expression if `exprs.len() == 1`.
    pub fn all(mut exprs: Vec<Cond>) -> Self {
        if exprs.len() == 1 {
            exprs.pop().unwrap()
        } else {
            Self::And(exprs)
        }
    }
}

/// Strongly-typed pipeline operation in a [`TypedStep`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum StepOp {
    /// `value=<literal>` constant or initial pipeline seed value.
    Value { value: LiteralValue },
    /// `@<name>` load from cached `@init` precomputed field.
    LoadInit { name: String },
    /// `css <query>` or `css { <q1>; <q2> }`.
    Css { queries: Vec<String> },
    /// `css-all <query>` or `css-all { <q1>; <q2> }`.
    CssAll { queries: Vec<String> },
    /// `xpath <query>` or `xpath { <q1>; <q2> }`.
    Xpath { queries: Vec<String> },
    /// `xpath-all <query>` or `xpath-all { <q1>; <q2> }`.
    XpathAll { queries: Vec<String> },
    /// `css-remove <query>`.
    CssRemove { query: String },
    /// `xpath-remove <query>`.
    XpathRemove { query: String },
    /// `text`.
    Text,
    /// `raw [outer|inner]`.
    Raw { mode: RawMode },
    /// `attr <key1> [key2 ...]`.
    Attr { keys: Vec<String> },
    /// `trim [chars]`.
    Trim { chars: String },
    /// `ltrim [chars]`.
    Ltrim { chars: String },
    /// `rtrim [chars]`.
    Rtrim { chars: String },
    /// `normalize-space`.
    NormalizeSpace,
    /// `rm-prefix <substr>`.
    RmPrefix { substr: String },
    /// `rm-suffix <substr>`.
    RmSuffix { substr: String },
    /// `rm-prefix-suffix <substr>`.
    RmPrefixSuffix { substr: String },
    /// `fmt <template>`.
    Fmt { template: String },
    /// `repl <old> <new>`.
    Repl { old: String, new: String },
    /// `repl { <k1> <v1>; <k2> <v2> }`.
    ReplMap {
        replacements: IndexMap<String, String>,
    },
    /// `lower`.
    Lower,
    /// `upper`.
    Upper,
    /// `split <sep>`.
    Split { sep: String },
    /// `join <sep>`.
    Join { sep: String },
    /// `unescape`.
    Unescape,
    /// `re <pattern>`.
    Re { pattern: String },
    /// `re-all <pattern>`.
    ReAll { pattern: String },
    /// `re-sub <pattern> <repl>`.
    ReSub { pattern: String, repl: String },
    /// `re-split <pattern>`.
    ReSplit { pattern: String },
    /// `index <i>`, `first` (`i = 0`), or `last` (`i = -1`).
    Index { index: i64 },
    /// `slice <start> <end>`.
    Slice { start: i64, end: i64 },
    /// `len`.
    Len,
    /// `unique [keep-order=#true]`.
    Unique { keep_order: bool },
    /// `to-int`.
    ToInt,
    /// `to-float`.
    ToFloat,
    /// `to-bool`.
    ToBool,
    /// `jsonify <SchemaName> [path="..."]`.
    Jsonify {
        schema_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parsed_path: Option<GjsonPath>,
    },
    /// `nested <StructName>`.
    Nested { struct_name: String },
    /// `filter { ... }`.
    Filter { cond: Cond },
    /// `assert ["message"] { ... }`.
    Assert {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        cond: Cond,
    },
    /// `match { ... }` (inside `(table)struct` fields).
    Match { cond: Cond },
    /// `!Namespace.op` user-defined extension call with bound call-site types.
    ExtensionCall {
        namespace: String,
        name: String,
        qualified_name: String,
        bound_input: TypeInfo,
        bound_output: TypeInfo,
    },
}

/// A single type-annotated step in a [`Pipeline`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedStep {
    /// Operation payload.
    pub op: StepOp,
    /// Input [`TypeInfo`] entering this step.
    pub input: TypeInfo,
    /// Output [`TypeInfo`] produced by this step.
    pub output: TypeInfo,
    /// Source span of the KDL operation node.
    pub span: Span,
}

/// Ordered sequence of typed pipeline operations with an optional terminal `fallback` literal.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Pipeline {
    /// Input [`TypeInfo`] entering the pipeline.
    pub input: TypeInfo,
    /// Final output [`TypeInfo`] produced by the pipeline (including `is_optional = true` if `fallback #null`).
    pub output: TypeInfo,
    /// Ordered transformation steps (excluding terminal `fallback`).
    pub steps: Vec<TypedStep>,
    /// Optional terminal `fallback` literal value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<LiteralValue>,
}

/// Precomputed `@init` field declaration inside a [`StructDecl`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitFieldDecl {
    /// Precomputed field name (referenced as `@<name>`).
    pub name: String,
    /// Pipeline computing the cached value.
    pub pipeline: Pipeline,
    /// Resolved output [`TypeInfo`] of the pipeline.
    pub type_info: TypeInfo,
    /// Source span.
    pub span: Span,
}

/// Boolean validation method (`@check <name> { ... }`) inside a [`StructDecl`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckDecl {
    /// Check method name.
    pub name: String,
    /// Pipeline returning `BOOL`.
    pub pipeline: Pipeline,
    /// Source span.
    pub span: Span,
}

/// Extracted output field declaration inside a [`StructDecl`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDecl {
    /// Field name.
    pub name: String,
    /// Extraction pipeline.
    pub pipeline: Pipeline,
    /// Resolved output [`TypeInfo`] of the field.
    pub type_info: TypeInfo,
    /// Source span.
    pub span: Span,
}

/// `@request` HTTP fetch method declaration on a [`StructDecl`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestDecl {
    /// Method name (`name="..."`, defaults to `"fetch"`).
    pub name: String,
    /// Optional method docstring (`doc="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Optional dot-notation response extraction path (`response-path="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_path: Option<String>,
    /// Optional join separator for list-of-string responses (`response-join="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_join: Option<String>,
    /// Optional response JSON schema name (`response="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<String>,
    /// Parsed HTTP request specification.
    pub spec: RequestSpec,
    /// Deduplicated parameter placeholders in order of first appearance.
    pub placeholders: Vec<PlaceholderSpec>,
    /// Source span.
    pub span: Span,
}

/// Typed REST endpoint declaration on a `(rest)struct` [`StructDecl`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestEndpointDecl {
    /// Endpoint method name (`name="..."`, defaults to `"fetch"`).
    pub name: String,
    /// Optional endpoint docstring (`doc="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Optional dot-notation response extraction path (`response-path="..."`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_path: Option<String>,
    /// Optional response JSON schema name (`response="..."`, `None` for void `Ok[None]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<String>,
    /// Whether `response_schema` refers to an `(array)json` schema.
    pub response_is_array: bool,
    /// Synthesized result union type alias name (e.g. `"GetUserResult"` or `"FetchResult"`).
    pub result_alias_name: String,
    /// Parsed HTTP request specification.
    pub spec: RequestSpec,
    /// Deduplicated parameter placeholders in order of first appearance.
    pub placeholders: Vec<PlaceholderSpec>,
    /// Source span.
    pub span: Span,
}

/// Top-level `struct` declaration (`item`, `list`, `dict`, `table`, `flat`, `raw`, `rest`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDecl {
    /// Struct parsing or client kind.
    pub kind: StructKind,
    /// Declared struct name.
    pub name: String,
    /// Optional `@doc` docstring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Whether `keep-order=#true` is set on `(flat)struct`.
    pub keep_order: bool,
    /// Precomputed `@init` fields.
    #[serde(default)]
    pub init_fields: Vec<InitFieldDecl>,
    /// Optional `@pre-validate` pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_validate: Option<Pipeline>,
    /// Primary `@check` pipeline (if at least one `@check` is declared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_pipeline: Option<Pipeline>,
    /// All named `@check <name> { ... }` methods on this struct.
    #[serde(default)]
    pub checks: Vec<CheckDecl>,
    /// `@split-doc` pipeline for `List`, `Dict`, or `Raw` structs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_doc: Option<Pipeline>,
    /// `@key` pipeline for `Dict` structs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_pipeline: Option<Pipeline>,
    /// `@value` pipeline for `Dict` or `Table` structs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_pipeline: Option<Pipeline>,
    /// `@table` pipeline for `Table` structs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_pipeline: Option<Pipeline>,
    /// `@rows` pipeline for `Table` structs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows_pipeline: Option<Pipeline>,
    /// `@match` pipeline for `Table` structs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_pipeline: Option<Pipeline>,
    /// Extracted output fields.
    #[serde(default)]
    pub fields: Vec<FieldDecl>,
    /// `@request` declarations on this struct.
    #[serde(default)]
    pub requests: Vec<RequestDecl>,
    /// `@error` declarations on `(rest)struct`.
    #[serde(default)]
    pub errors: Vec<ErrorResponseSpec>,
    /// Typed REST endpoints on `(rest)struct`.
    #[serde(default)]
    pub rest_endpoints: Vec<RestEndpointDecl>,
    /// Source span of the `struct` node.
    pub span: Span,
}

/// Field inside a synthesized [`StructTypeDef`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDefField {
    /// Field name (`"key"` / `"value"` for `Dict` structs, or regular field name).
    pub name: String,
    /// Resolved field [`TypeInfo`].
    pub type_info: TypeInfo,
}

/// Synthesized output data model (`{Name}Type`) for a non-`Rest` [`StructDecl`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructTypeDef {
    /// Base struct name (emitted as `{name}Type`).
    pub name: String,
    /// Associated struct kind (`Item`, `List`, `Dict`, `Table`, `Flat`, `Raw`).
    pub struct_kind: StructKind,
    /// Output model fields.
    pub fields: Vec<TypeDefField>,
}

/// Synthesized error variant class (`{Struct}Err{Status}{Suffix}`) for a `(rest)struct`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultVariantDef {
    /// Enclosing `(rest)struct` name.
    pub struct_name: String,
    /// Synthesized error class name (e.g. `"UsersClientErr404NotFound"`).
    pub name: String,
    /// HTTP status code (`100..599`).
    pub status: u16,
    /// Error payload `JsonDef` name.
    pub schema_name: String,
    /// Whether `schema_name` is an `(array)json` schema.
    pub schema_is_array: bool,
}

/// Synthesized result union type alias (`{Method}Result`) for a `(rest)struct` endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultAliasDef {
    /// Enclosing `(rest)struct` name.
    pub struct_name: String,
    /// Result alias identifier (e.g. `"GetUserResult"` or `"FetchResult"`).
    pub name: String,
    /// Optional 2xx response `JsonDef` name (`None` for void/empty response).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<String>,
    /// Whether `response_schema` is an `(array)json` schema.
    pub response_is_array: bool,
    /// Error variant class names included in the union.
    pub err_variants: Vec<String>,
}

impl ResultAliasDef {
    /// Returns `true` if any REST endpoint on the enclosing struct using this result alias
    /// declares a `response-path`.
    pub fn has_response_path(&self, module: &Module) -> bool {
        module.struct_decl(&self.struct_name).is_some_and(|s| {
            s.rest_endpoints
                .iter()
                .any(|ep| ep.result_alias_name == self.name && ep.response_path.is_some())
        })
    }
}

/// Single HTTP error matcher entry in a [`MatcherListDef`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatcherEntry {
    /// HTTP status code (`100..599`).
    pub status: u16,
    /// Keys or dot-paths that must be present in the error JSON body.
    pub required_keys: Vec<String>,
    /// Key/dot-path value conditions on the error JSON body.
    pub conditions: IndexMap<String, ErrorConditionValue>,
    /// Error variant class name (`ResultVariantDef::name`) constructed on match.
    pub factory_name: String,
    /// Error payload `JsonDef` name.
    pub error_schema: String,
}

/// Ordered registry of HTTP error matchers synthesized for a `(rest)struct`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatcherListDef {
    /// Enclosing `(rest)struct` name.
    pub struct_name: String,
    /// Ordered matcher entries.
    pub entries: Vec<MatcherEntry>,
}

/// Companion REST artifact synthesized for a `(rest)struct`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestArtifact {
    /// Typed error variant class definition.
    ErrorVariant(ResultVariantDef),
    /// Endpoint result union type alias.
    ResultAlias(ResultAliasDef),
    /// Per-struct ordered error matcher list.
    MatcherList(MatcherListDef),
}

/// Parameter descriptor for a standalone [`FunctionDef`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FnParam {
    /// Parameter name (e.g. `"document"`).
    pub name: String,
    /// Parameter [`TypeInfo`] (`DOCUMENT` for `fn`, `STRING` for `(raw)fn`).
    pub type_info: TypeInfo,
}

/// Top-level standalone function declaration (`fn` or `(raw)fn`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDef {
    /// Function identifier.
    pub name: String,
    /// Whether this function is `(raw)fn` (`STRING` input) rather than `fn` (`DOCUMENT` input).
    pub is_raw: bool,
    /// Input parameters (defaults to `[FnParam { name: "document", type_info }]`).
    pub params: Vec<FnParam>,
    /// Resolved return [`TypeInfo`] of the function's pipeline.
    pub return_type: TypeInfo,
    /// Function transformation pipeline.
    pub pipeline: Pipeline,
    /// Optional `@doc` docstring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Source span of the `fn` node.
    pub span: Span,
}

/// Root immutable `Algebraic AST` representing a compiled KDL Schema DSL v2.1 module.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Module {
    /// Optional module-level `@doc` description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Originating source file name or logical path.
    #[serde(default)]
    pub source_file: String,
    /// User-defined `extension` declarations.
    #[serde(default)]
    pub extensions: Vec<ExtensionDecl>,
    /// Top-level and post-order hoisted `json` schema definitions.
    #[serde(default)]
    pub json_defs: Vec<JsonDef>,
    /// Synthesized `StructTypeDef` models for non-`Rest` structs.
    #[serde(default)]
    pub typedefs: Vec<StructTypeDef>,
    /// Synthesized REST error variants, result aliases, and matcher tables for `Rest` structs.
    #[serde(default)]
    pub rest_artifacts: Vec<RestArtifact>,
    /// Top-level `struct` declarations in dependency order.
    #[serde(default)]
    pub structs: Vec<StructDecl>,
    /// Top-level `fn` / `(raw)fn` declarations.
    #[serde(default)]
    pub functions: Vec<FunctionDef>,
}

impl Module {
    /// Looks up a [`JsonDef`] by name.
    pub fn json_def(&self, name: &str) -> Option<&JsonDef> {
        self.json_defs.iter().find(|j| j.name == name)
    }

    /// Looks up a [`StructDecl`] by name.
    pub fn struct_decl(&self, name: &str) -> Option<&StructDecl> {
        self.structs.iter().find(|s| s.name == name)
    }

    /// Looks up a [`FunctionDef`] by name.
    pub fn function_def(&self, name: &str) -> Option<&FunctionDef> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// Looks up an [`ExtensionOp`] by qualified `"Namespace.op"` name.
    pub fn extension_op(&self, qualified_name: &str) -> Option<&ExtensionOp> {
        self.extensions
            .iter()
            .flat_map(|ext| ext.operations.iter())
            .find(|op| op.qualified_name == qualified_name)
    }

    /// Resolves the effective result union type alias name for `alias_name` on `struct_name`,
    /// prefixing with `to_pascal_case(struct_name)` if multiple `(rest)struct`s in the module
    /// synthesize a result alias with the same `alias_name`.
    pub fn resolve_effective_result_alias_name(
        &self,
        struct_name: &str,
        alias_name: &str,
    ) -> String {
        let collision_count = self
            .rest_artifacts
            .iter()
            .filter(|a| matches!(a, RestArtifact::ResultAlias(ra) if ra.name == alias_name))
            .count();
        if collision_count > 1 {
            format!(
                "{}{alias_name}",
                crate::json_to_kdl::to_pascal_case(struct_name)
            )
        } else {
            alias_name.to_owned()
        }
    }
}

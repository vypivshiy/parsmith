use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::kdl::serializer::{arg_to_kdl, normalize_ty_input, prop_to_kdl, value_to_kdl};
use crate::kdl::span::Span;

/// A typed KDL 2.0 scalar value.
#[derive(Debug, Clone, PartialOrd, Serialize, Deserialize)]
pub enum KdlValue {
    /// A KDL string (bare identifier, quoted, raw, or multi-line).
    String(String),
    /// A KDL integer (`i128`).
    Int(i128),
    /// A KDL floating-point number (`f64`, including `#inf`, `#-inf`, `#nan`).
    Float(f64),
    /// A KDL boolean (`#true` or `#false`).
    Bool(bool),
    /// The KDL null value (`#null`).
    Null,
}

impl PartialEq for KdlValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => (a.is_nan() && b.is_nan()) || a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Null, Self::Null) => true,
            _ => false,
        }
    }
}

impl Eq for KdlValue {}

impl Hash for KdlValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            Self::String(s) => s.hash(state),
            Self::Int(i) => i.hash(state),
            Self::Float(f) => {
                let bits = if f.is_nan() {
                    f64::NAN.to_bits()
                } else if *f == 0.0 {
                    0.0f64.to_bits()
                } else {
                    f.to_bits()
                };
                bits.hash(state);
            }
            Self::Bool(b) => b.hash(state),
            Self::Null => {}
        }
    }
}

impl KdlValue {
    /// Converts a `kdl::KdlValue` into a `crate::kdl::KdlValue`.
    pub(crate) fn from_kdl(val: &kdl::KdlValue) -> Self {
        match val {
            kdl::KdlValue::String(s) => Self::String(s.clone()),
            kdl::KdlValue::Integer(i) => Self::Int(*i),
            kdl::KdlValue::Float(f) => Self::Float(*f),
            kdl::KdlValue::Bool(b) => Self::Bool(*b),
            kdl::KdlValue::Null => Self::Null,
        }
    }

    /// Returns `true` if this value is a [`KdlValue::String`].
    #[inline]
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Returns `true` if this value is a [`KdlValue::Int`].
    #[inline]
    pub fn is_int(&self) -> bool {
        matches!(self, Self::Int(_))
    }

    /// Alias for [`KdlValue::is_int`].
    #[inline]
    pub fn is_integer(&self) -> bool {
        self.is_int()
    }

    /// Returns `true` if this value is a [`KdlValue::Float`].
    #[inline]
    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float(_))
    }

    /// Returns `true` if this value is a [`KdlValue::Bool`].
    #[inline]
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Returns `true` if this value is [`KdlValue::Null`].
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the string slice if this value is [`KdlValue::String`].
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Alias for [`KdlValue::as_str`].
    #[inline]
    pub fn as_string(&self) -> Option<&str> {
        self.as_str()
    }

    /// Returns the `i128` value if this value is [`KdlValue::Int`].
    #[inline]
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Alias for [`KdlValue::as_int`].
    #[inline]
    pub fn as_integer(&self) -> Option<i128> {
        self.as_int()
    }

    /// Returns the `i64` value if this value is [`KdlValue::Int`] and fits in `i64`.
    #[inline]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => i64::try_from(*i).ok(),
            _ => None,
        }
    }

    /// Returns the `f64` value if this value is [`KdlValue::Float`].
    #[inline]
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Returns the `bool` value if this value is [`KdlValue::Bool`].
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Serializes this value to KDL 2.0 text.
    pub fn to_kdl(&self) -> String {
        value_to_kdl(self)
    }
}

impl fmt::Display for KdlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_kdl())
    }
}

impl From<&str> for KdlValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_owned())
    }
}

impl From<String> for KdlValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&String> for KdlValue {
    fn from(s: &String) -> Self {
        Self::String(s.clone())
    }
}

impl From<i128> for KdlValue {
    fn from(n: i128) -> Self {
        Self::Int(n)
    }
}

impl From<i64> for KdlValue {
    fn from(n: i64) -> Self {
        Self::Int(i128::from(n))
    }
}

impl From<i32> for KdlValue {
    fn from(n: i32) -> Self {
        Self::Int(i128::from(n))
    }
}

impl From<usize> for KdlValue {
    fn from(n: usize) -> Self {
        Self::Int(n as i128)
    }
}

impl From<isize> for KdlValue {
    fn from(n: isize) -> Self {
        Self::Int(n as i128)
    }
}

impl From<f64> for KdlValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<bool> for KdlValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl<T: Into<KdlValue>> From<Option<T>> for KdlValue {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => v.into(),
            None => Self::Null,
        }
    }
}

/// A positional argument on a KDL node, preserving value and type annotation spans.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KdlArg {
    /// The parsed value of the argument.
    pub value: KdlValue,
    /// Optional type annotation identifier (without parentheses, e.g. `"u8"`).
    pub ty: Option<String>,
    /// Full span of the argument entry (including any type annotation).
    pub span: Span,
    /// Span of the argument's literal value.
    pub value_span: Span,
    /// Span of the argument's `(type)` annotation, if present.
    pub type_span: Option<Span>,
}

impl KdlArg {
    /// Creates a new synthetic positional argument with an empty span.
    pub fn new(value: impl Into<KdlValue>) -> Self {
        Self {
            value: value.into(),
            ty: None,
            span: Span::EMPTY,
            value_span: Span::EMPTY,
            type_span: None,
        }
    }

    /// Creates a new synthetic positional argument with a type annotation.
    pub fn typed(ty: impl AsRef<str>, value: impl Into<KdlValue>) -> Self {
        Self {
            value: value.into(),
            ty: Some(normalize_ty_input(ty.as_ref())),
            span: Span::EMPTY,
            value_span: Span::EMPTY,
            type_span: None,
        }
    }

    /// Attaches a type annotation to this argument.
    pub fn with_ty(mut self, ty: impl AsRef<str>) -> Self {
        self.ty = Some(normalize_ty_input(ty.as_ref()));
        self
    }

    /// Returns a reference to the argument's [`KdlValue`].
    #[inline]
    pub fn value(&self) -> &KdlValue {
        &self.value
    }

    /// Returns the argument's type annotation identifier (without parentheses), if present.
    #[inline]
    pub fn ty(&self) -> Option<&str> {
        self.ty.as_deref()
    }

    /// Alias for [`KdlArg::ty`].
    #[inline]
    pub fn type_annotation(&self) -> Option<&str> {
        self.ty()
    }

    /// Returns the full span of the argument entry.
    #[inline]
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the span of the argument's value literal.
    #[inline]
    pub fn value_span(&self) -> Span {
        self.value_span
    }

    /// Returns the span of the argument's `(type)` annotation, if present.
    #[inline]
    pub fn type_span(&self) -> Option<Span> {
        self.type_span
    }

    /// Serializes this positional argument to KDL 2.0 text.
    pub fn to_kdl(&self) -> String {
        arg_to_kdl(self)
    }
}

impl Deref for KdlArg {
    type Target = KdlValue;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl PartialEq<KdlValue> for KdlArg {
    fn eq(&self, other: &KdlValue) -> bool {
        &self.value == other
    }
}

impl<T: Into<KdlValue>> From<T> for KdlArg {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

/// A named `key=value` property on a KDL node, preserving key, value, and type annotation spans.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KdlProp {
    /// Property key identifier.
    pub key: String,
    /// Property value.
    pub value: KdlValue,
    /// Optional type annotation identifier on the property value (without parentheses).
    pub ty: Option<String>,
    /// Full span of the `key=(type)value` property entry.
    pub span: Span,
    /// Span of the property key identifier.
    pub key_span: Span,
    /// Span of the property's literal value.
    pub value_span: Span,
    /// Span of the property's `(type)` annotation, if present.
    pub type_span: Option<Span>,
}

impl KdlProp {
    /// Creates a new synthetic named property with an empty span.
    pub fn new(key: impl Into<String>, value: impl Into<KdlValue>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ty: None,
            span: Span::EMPTY,
            key_span: Span::EMPTY,
            value_span: Span::EMPTY,
            type_span: None,
        }
    }

    /// Creates a new synthetic named property with a type annotation.
    pub fn typed(key: impl Into<String>, ty: impl AsRef<str>, value: impl Into<KdlValue>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ty: Some(normalize_ty_input(ty.as_ref())),
            span: Span::EMPTY,
            key_span: Span::EMPTY,
            value_span: Span::EMPTY,
            type_span: None,
        }
    }

    /// Attaches a type annotation to this property's value.
    pub fn with_ty(mut self, ty: impl AsRef<str>) -> Self {
        self.ty = Some(normalize_ty_input(ty.as_ref()));
        self
    }

    /// Returns the property key name.
    #[inline]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns a reference to the property's [`KdlValue`].
    #[inline]
    pub fn value(&self) -> &KdlValue {
        &self.value
    }

    /// Returns the property's type annotation identifier (without parentheses), if present.
    #[inline]
    pub fn ty(&self) -> Option<&str> {
        self.ty.as_deref()
    }

    /// Alias for [`KdlProp::ty`].
    #[inline]
    pub fn type_annotation(&self) -> Option<&str> {
        self.ty()
    }

    /// Returns the full span of the `key=value` entry.
    #[inline]
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the span of the property key.
    #[inline]
    pub fn key_span(&self) -> Span {
        self.key_span
    }

    /// Returns the span of the property value literal.
    #[inline]
    pub fn value_span(&self) -> Span {
        self.value_span
    }

    /// Returns the span of the property value's `(type)` annotation, if present.
    #[inline]
    pub fn type_span(&self) -> Option<Span> {
        self.type_span
    }

    /// Serializes this named property to KDL 2.0 text (`key=val` or `key=(type)val`).
    pub fn to_kdl(&self) -> String {
        prop_to_kdl(self)
    }
}

impl Deref for KdlProp {
    type Target = KdlValue;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl PartialEq<KdlValue> for KdlProp {
    fn eq(&self, other: &KdlValue) -> bool {
        &self.value == other
    }
}

/// A KDL node entry (either a positional argument or a named property).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KdlEntry {
    /// Positional argument entry.
    Arg(KdlArg),
    /// Named property entry.
    Prop(KdlProp),
}

impl KdlEntry {
    /// Returns the full span of this entry.
    #[inline]
    pub fn span(&self) -> Span {
        match self {
            Self::Arg(a) => a.span,
            Self::Prop(p) => p.span,
        }
    }

    /// Returns the underlying value of this entry.
    #[inline]
    pub fn value(&self) -> &KdlValue {
        match self {
            Self::Arg(a) => &a.value,
            Self::Prop(p) => &p.value,
        }
    }
}

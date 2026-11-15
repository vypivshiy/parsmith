//! Pass 5 `OpSig` pipeline type inference and checking (`E100`).
//!
//! Matches `ssc_codegen/core/type_checking.py` and `ssc_codegen/core/expressions.py`.

use crate::kdl::{KdlValue, NodeRef};

use crate::ast::{BaseType, LiteralValue, TypeInfo};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::linter::{UnitEnv, make_error};

/// Static type signature for a built-in pipeline operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpSig {
    /// Expected input base type (`None` means any base type is accepted).
    pub accept: Option<BaseType>,
    /// Produced output base type (`None` means unchanged input base type).
    pub ret: Option<BaseType>,
    /// Whether list cardinality of the input carries over to the output.
    pub list_propagates: bool,
    /// Whether this operation always forces the output to be a list (`is_array = true`).
    pub force_list: bool,
    /// Whether this operation always forces the output to be a scalar (`is_array = false`).
    pub force_scalar: bool,
    /// Whether this operation requires a list input (`is_array == true`).
    pub requires_list: bool,
}

impl OpSig {
    const fn new(accept: Option<BaseType>, ret: Option<BaseType>) -> Self {
        Self {
            accept,
            ret,
            list_propagates: true,
            force_list: false,
            force_scalar: false,
            requires_list: false,
        }
    }

    const fn with_force_list(mut self) -> Self {
        self.force_list = true;
        self
    }

    const fn with_force_scalar(mut self) -> Self {
        self.force_scalar = true;
        self
    }

    const fn with_requires_list(mut self) -> Self {
        self.requires_list = true;
        self
    }
}

/// Returns the static [`OpSig`] for a built-in pipeline operation name.
pub fn op_signature(op_name: &str) -> Option<OpSig> {
    match op_name {
        "css" | "xpath" | "css-remove" | "xpath-remove" => Some(OpSig::new(
            Some(BaseType::Document),
            Some(BaseType::Document),
        )),
        "css-all" | "xpath-all" => {
            Some(OpSig::new(Some(BaseType::Document), Some(BaseType::Document)).with_force_list())
        }
        "text" | "raw" | "attr" => {
            Some(OpSig::new(Some(BaseType::Document), Some(BaseType::String)))
        }
        "trim" | "ltrim" | "rtrim" | "normalize-space" | "fmt" | "repl" | "lower" | "upper"
        | "rm-prefix" | "rm-suffix" | "rm-prefix-suffix" | "unescape" | "re" | "re-sub" => {
            Some(OpSig::new(Some(BaseType::String), Some(BaseType::String)))
        }
        "split" | "re-split" | "re-all" => {
            Some(OpSig::new(Some(BaseType::String), Some(BaseType::String)).with_force_list())
        }
        "join" => Some(
            OpSig::new(Some(BaseType::String), Some(BaseType::String))
                .with_force_scalar()
                .with_requires_list(),
        ),
        "index" | "first" | "last" => Some(
            OpSig::new(None, None)
                .with_force_scalar()
                .with_requires_list(),
        ),
        "slice" => Some(OpSig::new(None, None).with_requires_list()),
        "len" => Some(
            OpSig::new(None, Some(BaseType::Int))
                .with_force_scalar()
                .with_requires_list(),
        ),
        "unique" => {
            Some(OpSig::new(Some(BaseType::String), Some(BaseType::String)).with_requires_list())
        }
        "to-int" => Some(OpSig::new(Some(BaseType::String), Some(BaseType::Int))),
        "to-float" => Some(OpSig::new(Some(BaseType::String), Some(BaseType::Float))),
        "to-bool" => Some(OpSig::new(None, Some(BaseType::Bool)).with_force_scalar()),
        "jsonify" => Some(OpSig::new(
            Some(BaseType::String),
            Some(BaseType::Json(String::new())),
        )),
        "nested" => Some(
            OpSig::new(
                Some(BaseType::Document),
                Some(BaseType::Nested(String::new())),
            )
            .with_force_scalar(),
        ),
        "match" => {
            Some(OpSig::new(Some(BaseType::Document), Some(BaseType::String)).with_force_scalar())
        }
        _ => None,
    }
}

/// Checks whether `got` base type is compatible with `expected`.
pub fn base_type_compatible(got: &BaseType, expected: Option<&BaseType>) -> bool {
    let Some(exp) = expected else {
        return true;
    };
    if *got == BaseType::Auto || *exp == BaseType::Auto {
        return true;
    }
    got.same_category(exp)
}

/// Resolves the output [`TypeInfo`] after applying `sig` to `current`.
pub fn apply_op_sig(sig: &OpSig, current: &TypeInfo) -> TypeInfo {
    let new_base = sig.ret.clone().unwrap_or_else(|| current.base.clone());
    let new_is_array = if sig.force_list {
        true
    } else if sig.force_scalar {
        false
    } else {
        sig.list_propagates && current.is_array
    };

    let reference = match &new_base {
        BaseType::Nested(r) | BaseType::Json(r) if !r.is_empty() => Some(r.clone()),
        _ => {
            if sig.ret.is_none() {
                current.reference.clone()
            } else {
                None
            }
        }
    };

    TypeInfo {
        base: new_base,
        is_array: new_is_array,
        is_optional: false,
        is_dict: false,
        reference,
        key_type: None,
        value_type: None,
        omitempty: false,
        skip: false,
    }
}

/// Computes an actionable diagnostic hint for an `E100` pipeline type mismatch.
pub fn type_mismatch_hint(op_name: &str, got_base: &BaseType, got_is_array: bool) -> String {
    const NEEDS_TEXT: &[&str] = &[
        "fmt",
        "trim",
        "ltrim",
        "rtrim",
        "lower",
        "upper",
        "re",
        "re-sub",
        "re-all",
        "re-split",
        "to-int",
        "to-float",
        "split",
        "join",
        "normalize-space",
        "unescape",
        "rm-prefix",
        "rm-suffix",
        "rm-prefix-suffix",
    ];
    if *got_base == BaseType::Document && NEEDS_TEXT.contains(&op_name) {
        return "add 'text', 'raw', or 'attr' before this operation to extract a string".to_owned();
    }
    if got_is_array && matches!(op_name, "css" | "xpath" | "css-all" | "xpath-all") {
        return "selectors work on a single DOCUMENT, not a list".to_owned();
    }
    if matches!(op_name, "index" | "first" | "last" | "slice") && !got_is_array {
        return format!(
            "'{op_name}' requires a LIST type, got {}",
            got_base.dsl_name()
        );
    }
    if matches!(op_name, "unique" | "join") && (*got_base != BaseType::String || !got_is_array) {
        return format!(
            "'{op_name}' requires LIST_STRING, got {}",
            got_base.dsl_name()
        );
    }
    if !got_is_array && op_name == "len" {
        return "'len' counts elements of any list — produce a list first".to_owned();
    }
    if matches!(op_name, "split" | "re-split") && *got_base != BaseType::String {
        return format!("'{op_name}' requires STRING, got {}", got_base.dsl_name());
    }
    if let Some(sig) = op_signature(op_name) {
        if let Some(ref accept) = sig.accept {
            return format!("'{op_name}' accepts: {}", accept.dsl_name());
        }
    }
    format!("unexpected type {} for '{op_name}'", got_base.dsl_name())
}

/// Validates and resolves a terminal `fallback` node against the current pipeline [`TypeInfo`].
///
/// Returns `(Option<LiteralValue>, updated_output_type)`.
pub fn check_and_lower_fallback(
    unit_path: &str,
    node: NodeRef<'_>,
    env: &UnitEnv,
    has_preceding_steps: bool,
    current: &TypeInfo,
    diags: &mut Vec<Diagnostic>,
) -> (Option<LiteralValue>, TypeInfo) {
    if !has_preceding_steps && current.base == BaseType::Document {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E100,
            node.span(),
            "'fallback' requires a preceding operation",
            "add a producer first, e.g. `css '.x'; text`",
        ));
    }

    if node.has_children_block() {
        if !current.is_array && current.base != BaseType::Auto {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E100,
                node.span(),
                format!(
                    "'fallback {{}}' is only valid for list types, got {}",
                    current.base.dsl_name()
                ),
                "use 'css-all' or 'xpath-all' to produce a list",
            ));
        }
        return (Some(LiteralValue::EmptyArray), current.clone());
    }

    let Some(arg) = node.arg(0) else {
        return (None, current.clone());
    };

    match &arg.value {
        KdlValue::Null => {
            if !matches!(
                current.base,
                BaseType::String
                    | BaseType::Int
                    | BaseType::Float
                    | BaseType::Nested(_)
                    | BaseType::Json(_)
                    | BaseType::Auto
            ) {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E100,
                    node.span(),
                    format!(
                        "'fallback #null' only valid for STRING/INT/FLOAT, got {}",
                        current.base.dsl_name()
                    ),
                    "",
                ));
            }
            let mut updated = current.clone();
            updated.is_optional = true;
            (Some(LiteralValue::Null), updated)
        }
        KdlValue::Bool(b) => {
            check_scalar_fallback_compat(unit_path, node, current, &BaseType::Bool, diags);
            (Some(LiteralValue::Bool(*b)), current.clone())
        }
        KdlValue::Int(n) => {
            check_scalar_fallback_compat(unit_path, node, current, &BaseType::Int, diags);
            (Some(LiteralValue::Int(*n as i64)), current.clone())
        }
        KdlValue::Float(f) => {
            check_scalar_fallback_compat(unit_path, node, current, &BaseType::Float, diags);
            (Some(LiteralValue::Float(*f)), current.clone())
        }
        KdlValue::String(s) => {
            let resolved = env.resolve_str_arg(s);
            check_scalar_fallback_compat(unit_path, node, current, &BaseType::String, diags);
            (Some(LiteralValue::String(resolved)), current.clone())
        }
    }
}

fn check_scalar_fallback_compat(
    unit_path: &str,
    node: NodeRef<'_>,
    current: &TypeInfo,
    fb_base: &BaseType,
    diags: &mut Vec<Diagnostic>,
) {
    if current.base == BaseType::Auto {
        return;
    }
    if current.is_array {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E100,
            node.span(),
            format!(
                "'fallback' type {} does not match pipeline LIST_{}",
                fb_base.dsl_name(),
                current.base.dsl_name()
            ),
            "use 'fallback {}' for list types",
        ));
    } else if !current.base.same_category(fb_base) {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E100,
            node.span(),
            format!(
                "'fallback' type {} does not match pipeline {}",
                fb_base.dsl_name(),
                current.base.dsl_name()
            ),
            format!(
                "use a {} literal or #null",
                current.base.dsl_name().to_ascii_lowercase()
            ),
        ));
    }
}

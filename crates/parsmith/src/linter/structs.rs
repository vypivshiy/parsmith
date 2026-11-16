//! `struct` and `fn` shell/directive validation (`@init`, `@check`, `@pre-validate`, `@split-doc`,
//! `@key`, `@value`, `@table`, `@rows`, `@match`) and `define` block expansion.

use std::collections::HashSet;

use crate::ast::{BaseType, TypeInfo};
use crate::kdl::{KdlValue, NodeId, NodeRef};
use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::imports::{ResolvedProgram, SourceUnit};
pub use crate::linter::http::*;
pub use crate::linter::order::*;
pub use crate::linter::pipeline::*;
pub use crate::linter::predicates::*;
use crate::linter::{DefineEntry, UnitEnv, did_you_mean_hint, make_error};
use crate::request::{
    ErrorResponseSpec, REQUEST_LINE_CONTINUATION_HINT, RequestSpec, is_error_directive,
    starts_with_http_signature,
};

const VALID_STRUCT_TYPES: &[&str] = &["dict", "flat", "item", "list", "raw", "rest", "table"];

const KNOWN_STRUCT_DIRECTIVES: &[&str] = &[
    "@check",
    "@doc",
    "@error",
    "@init",
    "@key",
    "@match",
    "@pre-validate",
    "@request",
    "@rows",
    "@split-doc",
    "@table",
    "@value",
];

/// Validates all top-level `struct` declarations in `unit`.
pub fn lint_structs(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    diags: &mut Vec<Diagnostic>,
    parsed_requests: &mut IndexMap<NodeId, RequestSpec>,
    parsed_errors: &mut IndexMap<NodeId, ErrorResponseSpec>,
) {
    let Ok(nodes) = unit.document.select("struct:root") else {
        return;
    };

    let mut seen_names = HashSet::new();
    for node in nodes {
        let name = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
        if !name.is_empty() && !seen_names.insert(name.to_owned()) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E402,
                node.span(),
                format!("duplicate struct definition '{name}'"),
                format!("remove or rename duplicate struct '{name}'"),
            ));
        }

        lint_single_struct(
            unit,
            program,
            env,
            node,
            diags,
            parsed_requests,
            parsed_errors,
        );
    }
}

fn lint_single_struct(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    node: NodeRef<'_>,
    diags: &mut Vec<Diagnostic>,
    parsed_requests: &mut IndexMap<NodeId, RequestSpec>,
    parsed_errors: &mut IndexMap<NodeId, ErrorResponseSpec>,
) {
    let struct_name = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
    if node.args().is_empty() || struct_name.is_empty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'struct' requires a name",
            "example: struct MyStruct { ... }",
        ));
        return;
    }
    if node.args().len() > 1 {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'struct' accepts only 1 name argument",
            "example: struct MyStruct { ... }",
        ));
    }

    let struct_type = if let Some(ty) = node.ty() {
        ty
    } else if let Some(tp) = node.prop("type") {
        tp.value.as_str().unwrap_or("invalid")
    } else {
        "item"
    };

    if !VALID_STRUCT_TYPES.contains(&struct_type) {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E400,
            node.type_span().unwrap_or_else(|| node.span()),
            format!("unknown struct type '{struct_type}'"),
            format!("valid types: {}", VALID_STRUCT_TYPES.join(", ")),
        ));
        return;
    }

    for prop in node.props() {
        match prop.key.as_str() {
            "type" => {}
            "keep-order" => {
                if struct_type != "flat" {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E203,
                        prop.span,
                        format!(
                            "'keep-order' is only allowed on (flat)struct, not '{struct_type}'"
                        ),
                        "use `(flat)struct Name keep-order=#true { ... }`",
                    ));
                } else if !prop.value.is_bool() {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E002,
                        prop.value_span,
                        "'keep-order' property must be a boolean (#true or #false)",
                        "example: (flat)struct Tags keep-order=#true { ... }",
                    ));
                }
            }
            other => {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    prop.span,
                    format!("unknown property '{other}' on 'struct'"),
                    "allowed properties on 'struct': type, keep-order",
                ));
            }
        }
    }

    // Check required directives using KDL selectors on `node`
    let required: &[&str] = match struct_type {
        "list" => &["@split-doc"],
        "dict" => &["@split-doc", "@key", "@value"],
        "table" => &["@table", "@rows", "@match", "@value"],
        "rest" => &["@request"],
        _ => &[],
    };

    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|&sel| node.select_first(sel).ok().flatten().is_none())
        .collect();

    if !missing.is_empty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E401,
            node.span(),
            format!(
                "struct type='{struct_type}' missing required field(s) {}",
                missing.join(", ")
            ),
            format!("add: {}", missing.join(", ")),
        ));
    }

    // Collect `@init` field names first so field pipelines can validate `@<name>` (`E301`)
    let mut init_fields: HashSet<String> = HashSet::new();
    for child in node.children() {
        if child.name() == "@init" {
            for init_child in child.children() {
                init_fields.insert(init_child.name().to_owned());
            }
        }
    }

    let is_raw = struct_type == "raw";
    let mut seen_err_variants: HashSet<String> = HashSet::new();
    let mut seen_field_names: HashSet<String> = HashSet::new();
    let mut seen_singleton_directives: HashSet<String> = HashSet::new();
    let mut seen_check_names: HashSet<String> = HashSet::new();
    let mut seen_request_names: HashSet<String> = HashSet::new();
    let mut regular_field_count = 0usize;
    let mut has_validation_directive = false;

    for child in node.children() {
        let field_name = child.name();
        if field_name.is_empty() {
            continue;
        }

        if field_name.starts_with('@') {
            if matches!(field_name, "@pre-validate" | "@check") {
                has_validation_directive = true;
            }
            if matches!(
                field_name,
                "@doc"
                    | "@init"
                    | "@pre-validate"
                    | "@split-doc"
                    | "@key"
                    | "@value"
                    | "@table"
                    | "@rows"
                    | "@match"
            ) && !seen_singleton_directives.insert(field_name.to_owned())
            {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E402,
                    child.span(),
                    format!("duplicate '{field_name}' directive in struct '{struct_name}'"),
                    format!("remove duplicate '{field_name}'"),
                ));
            }

            lint_reserved_directive(
                unit,
                program,
                env,
                child,
                field_name,
                struct_name,
                struct_type,
                is_raw,
                &init_fields,
                &mut seen_check_names,
                &mut seen_request_names,
                &mut seen_err_variants,
                diags,
                parsed_requests,
                parsed_errors,
            );
        } else {
            regular_field_count += 1;
            if struct_type == "rest" {
                let hint = if starts_with_http_signature(field_name) {
                    REQUEST_LINE_CONTINUATION_HINT
                } else {
                    ""
                };
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E203,
                    child.span(),
                    format!("regular field '{field_name}' not allowed in struct type='rest'"),
                    hint,
                ));
            } else if struct_type == "dict" {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E203,
                    child.span(),
                    format!(
                        "regular field '{field_name}' not allowed in struct type='dict'; use '@key' and '@value'"
                    ),
                    "use '@split-doc', '@key', and '@value' in (dict)struct",
                ));
            } else {
                if !seen_field_names.insert(field_name.to_owned()) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E402,
                        child.span(),
                        format!("duplicate field '{field_name}' in struct '{struct_name}'"),
                        format!("rename or remove duplicate field '{field_name}'"),
                    ));
                }
                lint_regular_field(
                    unit,
                    program,
                    env,
                    child,
                    field_name,
                    struct_type,
                    is_raw,
                    &init_fields,
                    diags,
                );
            }
        }
    }

    if matches!(struct_type, "item" | "list" | "table" | "flat" | "raw")
        && regular_field_count == 0
        && !has_validation_directive
    {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E401,
            node.span(),
            format!(
                "struct '{struct_name}' (type='{struct_type}') must contain at least one field"
            ),
            format!("add at least one field inside 'struct {struct_name} {{ ... }}'"),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn lint_reserved_directive(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    node: NodeRef<'_>,
    field_name: &str,
    struct_name: &str,
    struct_type: &str,
    is_raw: bool,
    init_fields: &HashSet<String>,
    seen_check_names: &mut HashSet<String>,
    seen_request_names: &mut HashSet<String>,
    seen_err_variants: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
    parsed_requests: &mut IndexMap<NodeId, RequestSpec>,
    parsed_errors: &mut IndexMap<NodeId, ErrorResponseSpec>,
) {
    let base_name = if is_error_directive(field_name) {
        "@error"
    } else {
        field_name
    };

    let allowed_in: Option<&[&str]> = match base_name {
        "@request" | "@doc" => None,
        "@pre-validate" | "@check" | "@init" => {
            Some(&["dict", "flat", "item", "list", "raw", "table"])
        }
        "@split-doc" => Some(&["dict", "list", "raw"]),
        "@key" => Some(&["dict"]),
        "@value" => Some(&["dict", "table"]),
        "@table" | "@rows" | "@match" => Some(&["table"]),
        "@error" => Some(&["rest"]),
        _ => {
            let hint = did_you_mean_hint(
                field_name,
                KNOWN_STRUCT_DIRECTIVES.iter().copied(),
                "valid directives: @doc, @init, @pre-validate, @check, @split-doc, @key, @value, @table, @rows, @match, @request, @error",
            );
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E200,
                node.name_span(),
                format!("Unknown directive: {field_name}"),
                hint,
            ));
            return;
        }
    };

    if let Some(allowed) = allowed_in {
        if !allowed.contains(&struct_type) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E203,
                node.span(),
                format!("'{field_name}' not allowed in struct type='{struct_type}'"),
                format!("'{field_name}' only valid in: {}", allowed.join(", ")),
            ));
            return;
        }
    }

    let pctx = PipelineLintCtx {
        unit,
        program,
        env,
        is_raw,
        is_fn: false,
        is_table_field: false,
        init_fields,
    };

    match base_name {
        "@doc" => {
            if node.args().len() != 1
                || node
                    .arg_value(0)
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty)
            {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'@doc' requires a description string",
                    "example: @doc \"Struct description\"",
                ));
            }
            if !node.props().is_empty() || node.has_children_block() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'@doc' does not accept properties or children",
                    "",
                ));
            }
        }
        "@request" => {
            lint_request_directive(
                unit,
                env,
                node,
                struct_type,
                seen_request_names,
                diags,
                parsed_requests,
            );
        }
        "@init" => {
            if !node.args().is_empty() || !node.props().is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'@init' does not accept arguments or properties",
                    "",
                ));
            }
            let children = node.children();
            if children.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'@init' block must contain at least one named pipeline",
                    "@init {\n    my-field { css \".x\"; text }\n}",
                ));
            }
            let mut seen_init = HashSet::new();
            let empty_init_set = HashSet::new();
            let init_pctx = PipelineLintCtx {
                init_fields: &empty_init_set,
                ..pctx
            };
            for init_field in children {
                let init_name = init_field.name();
                if !seen_init.insert(init_name.to_owned()) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E402,
                        init_field.span(),
                        format!("duplicate @init field '{init_name}' in struct '{struct_name}'"),
                        "",
                    ));
                }
                let ops = expand_defines(init_field.children(), program, env);
                lint_field_header_and_value(
                    unit,
                    env,
                    init_field,
                    init_name,
                    true,
                    false,
                    !ops.is_empty(),
                    diags,
                );
                if !ops.is_empty() {
                    lint_pipeline_ops(&init_pctx, &ops, diags);
                }
            }
        }
        "@check" => {
            let check_name = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
            if node.args().len() != 1 || check_name.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "@check requires a name: @check <name> { ... }",
                    "example: @check is-valid { css \".ok\"; to-bool }",
                ));
            } else if !seen_check_names.insert(check_name.to_owned()) {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E402,
                    node.span(),
                    format!("duplicate @check method '{check_name}' in struct '{struct_name}'"),
                    "",
                ));
            }
            if !node.props().is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'@check' does not accept properties",
                    "",
                ));
            }
            let ops = expand_defines(node.children(), program, env);
            if ops.is_empty() {
                let label = if check_name.is_empty() {
                    String::new()
                } else {
                    format!("{check_name} ")
                };
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("@check {label}block must contain at least one operation"),
                    "",
                ));
            } else {
                lint_pipeline_ops(&pctx, &ops, diags);
            }
        }
        "@pre-validate" | "@split-doc" | "@key" | "@value" | "@table" | "@rows" | "@match" => {
            if !node.args().is_empty() || !node.props().is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{field_name}' does not accept arguments or properties"),
                    "",
                ));
            }
            let ops = expand_defines(node.children(), program, env);
            if ops.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{field_name}' block must contain at least one operation"),
                    "",
                ));
            } else {
                lint_pipeline_ops(&pctx, &ops, diags);
            }
        }
        "@error" => {
            lint_error_directive(
                unit,
                env,
                node,
                field_name,
                struct_name,
                seen_err_variants,
                diags,
                parsed_errors,
            );
        }
        _ => {}
    }
}

/// Parses an optional postfix field type argument (`str`, `int`, `float`, `bool`, with optional `?` and `(array)`)
/// on a `struct` or `@init` field node.
pub fn parse_postfix_field_type(
    arg: &crate::kdl::KdlArg,
    field_name: &str,
) -> Result<TypeInfo, (DiagnosticCode, crate::kdl::Span, String, String)> {
    let Some(s) = arg.value.as_str() else {
        return Err((
            DiagnosticCode::E001,
            arg.span,
            format!(
                "field '{field_name}' type argument must be a type name (str, int, float, bool)"
            ),
            format!("example: {field_name} str {{ css \"...\"; text }}"),
        ));
    };

    let (without_arr, is_array) = if let Some(ann) = arg.ty() {
        if ann != "array" {
            return Err((
                DiagnosticCode::E001,
                arg.type_span.unwrap_or(arg.span),
                format!("unsupported type annotation '({ann})' on field '{field_name}'"),
                "only '(array)' is supported on field types".to_owned(),
            ));
        }
        (s, true)
    } else if let Some(stripped) = s.strip_prefix("(array)") {
        (stripped, true)
    } else {
        (s, false)
    };

    let (base_str, is_optional) = match without_arr.strip_suffix('?') {
        Some(b) => (b, true),
        None => (without_arr, false),
    };

    let base_ti = match base_str {
        "str" => TypeInfo::string(),
        "int" => TypeInfo::int(),
        "float" => TypeInfo::float(),
        "bool" => TypeInfo::bool(),
        _ => {
            return Err((
                DiagnosticCode::E001,
                arg.span,
                format!(
                    "unknown field type '{s}' on field '{field_name}' (valid types: str, int, float, bool)"
                ),
                format!(
                    "use a valid field type (str, int, float, bool) or move operations inside braces: {field_name} {{ css \"...\"; text }}"
                ),
            ));
        }
    };

    Ok(base_ti.with_array(is_array).with_optional(is_optional))
}

#[allow(clippy::too_many_arguments)]
fn lint_field_header_and_value(
    unit: &SourceUnit,
    env: &UnitEnv,
    field_node: NodeRef<'_>,
    field_name: &str,
    is_init: bool,
    is_table_field: bool,
    has_ops: bool,
    diags: &mut Vec<Diagnostic>,
) {
    let field_label = if is_init {
        format!("@init field '{field_name}'")
    } else {
        format!("field '{field_name}'")
    };

    // 1. Reject prefix type annotations on fields (`(str)title`)
    if let Some(ann) = field_node.ty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            field_node.type_span().unwrap_or_else(|| field_node.span()),
            format!(
                "prefix type annotation '({ann})' is not allowed on {field_label}; place the type after the field name"
            ),
            format!("use postfix type syntax: {field_name} {ann}"),
        ));
    }

    // 2. Validate optional postfix type argument (at most 1 positional argument)
    let mut declared_type: Option<TypeInfo> = None;
    if field_node.args().len() > 1 {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            field_node.span(),
            format!("{field_label} accepts at most 1 type argument"),
            format!("move operations inside braces: {field_name} {{ css \"...\"; text }}"),
        ));
    } else if let Some(arg) = field_node.arg(0) {
        match parse_postfix_field_type(arg, field_name) {
            Ok(ti) => declared_type = Some(ti),
            Err((code, span, msg, hint)) => {
                diags.push(make_error(&unit.path, code, span, msg, hint));
            }
        }
    }

    // 3. Validate properties (only `value=<literal>` is allowed)
    for prop in field_node.props() {
        if prop.key != "value" {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                prop.span,
                format!("unknown property '{}' on {field_label}", prop.key),
                "only 'value=<literal>' is supported on fields",
            ));
        }
    }

    if let Some(val_prop) = field_node.prop("value") {
        if is_table_field {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E203,
                val_prop.span,
                format!(
                    "table field '{field_name}' must start with 'match {{ ... }}' (cannot use 'value=')"
                ),
                format!("example: {field_name} {{ match {{ eq \"Header\" }} }}"),
            ));
        }

        let lit_base = match &val_prop.value {
            KdlValue::String(s) => {
                for token in crate::linter::top_level::extract_double_brace_tokens(s) {
                    if crate::linter::is_define_name(token) {
                        if !env.defines.contains_key(token)
                            && !env.all_program_defines.contains(token)
                        {
                            diags.push(make_error(
                                &unit.path,
                                DiagnosticCode::E300,
                                val_prop.value_span,
                                format!("undefined define '{token}' in value=\"{s}\""),
                                format!("declare 'define {token}=\"...\"' before use"),
                            ));
                        } else if matches!(env.defines.get(token), Some(DefineEntry::Block { .. }))
                        {
                            diags.push(make_error(
                                &unit.path,
                                DiagnosticCode::E002,
                                val_prop.value_span,
                                format!(
                                    "cannot interpolate block define '{token}' in value=\"{s}\""
                                ),
                                format!("use a scalar define: define {token}=\"...\""),
                            ));
                        }
                    }
                }
                Some(BaseType::String)
            }
            KdlValue::Int(_) => Some(BaseType::Int),
            KdlValue::Float(_) => Some(BaseType::Float),
            KdlValue::Bool(_) => Some(BaseType::Bool),
            KdlValue::Null => {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E002,
                    val_prop.value_span,
                    "'value' property cannot be #null; use a scalar str, int, float, or bool literal",
                    "example: value=\"default\" or value=0 or value=#false",
                ));
                None
            }
        };

        if field_node.has_children_block() && !has_ops {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                field_node.span(),
                format!("empty pipeline block '{{}}' alongside 'value=' on {field_label}"),
                "remove the empty '{}' block or add pipeline operations inside it",
            ));
        }

        if !has_ops {
            if let (Some(target_ti), Some(actual_base)) = (&declared_type, &lit_base) {
                if target_ti.is_array || target_ti.is_optional || &target_ti.base != actual_base {
                    let mut target_repr = if target_ti.is_array {
                        format!("LIST_{}", target_ti.base.dsl_name())
                    } else {
                        target_ti.base.dsl_name().to_owned()
                    };
                    if target_ti.is_optional {
                        target_repr.push('?');
                    }
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        val_prop.span,
                        format!(
                            "{field_label} declared as {target_repr} but 'value=' literal has type {}",
                            actual_base.dsl_name()
                        ),
                        "ensure the field type matches the 'value=' literal type, or add a conversion pipeline '{ ... }'",
                    ));
                }
            }
        }
    } else if !has_ops {
        let hint = if !is_init && starts_with_http_signature(field_name) {
            REQUEST_LINE_CONTINUATION_HINT.to_owned()
        } else {
            format!("add at least one operation: {field_name} {{ css \".item\"; text }}")
        };
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            field_node.span(),
            format!("{field_label} has no operations"),
            hint,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn lint_regular_field(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    field_node: NodeRef<'_>,
    field_name: &str,
    struct_type: &str,
    is_raw: bool,
    init_fields: &HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    let expanded = expand_defines(field_node.children(), program, env);
    lint_field_header_and_value(
        unit,
        env,
        field_node,
        field_name,
        false,
        struct_type == "table",
        !expanded.is_empty(),
        diags,
    );
    if expanded.is_empty() {
        return;
    }

    let is_single_nested = expanded.len() == 1 && expanded[0].name() == "nested";
    if struct_type == "table"
        && field_node.prop("value").is_none()
        && !is_single_nested
        && expanded[0].name() != "match"
    {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E203,
            field_node.span(),
            format!("table field '{field_name}' must start with 'match {{ ... }}'"),
            format!("example: {field_name} {{ match {{ eq \"Header\" }} }}"),
        ));
    }

    let pctx = PipelineLintCtx {
        unit,
        program,
        env,
        is_raw,
        is_fn: false,
        is_table_field: struct_type == "table",
        init_fields,
    };
    lint_pipeline_ops(&pctx, &expanded, diags);
}

/// Validates all top-level `fn` and `(raw)fn` declarations in `unit`.
pub fn lint_fns(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    diags: &mut Vec<Diagnostic>,
) {
    let Ok(nodes) = unit.document.select("fn:root") else {
        return;
    };

    let mut seen_names = HashSet::new();
    for node in nodes {
        let name = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
        if node.args().is_empty() || name.is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                node.span(),
                "'fn' requires a name",
                "example: fn page_title { css \"h1\"; text }",
            ));
            continue;
        }
        if node.args().len() > 1 || !node.props().is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                node.span(),
                "'fn' accepts only 1 name argument and no properties",
                "example: fn page_title { css \"h1\"; text }",
            ));
        }

        if !seen_names.insert(name.to_owned()) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E402,
                node.span(),
                format!("duplicate fn definition '{name}'"),
                format!("remove or rename duplicate fn '{name}'"),
            ));
        }

        let is_raw = if let Some(ty) = node.ty() {
            if ty != "raw" {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E400,
                    node.type_span().unwrap_or_else(|| node.span()),
                    format!("unsupported fn annotation '({ty})' — only '(raw)' is recognized"),
                    "use 'fn { ... }' (HTML) or '(raw)fn { ... }' (plain text)",
                ));
                false
            } else {
                true
            }
        } else {
            false
        };

        let mut pipeline_children = Vec::new();
        for child in node.children() {
            if child.name() == "@doc" {
                if child.args().len() != 1
                    || child
                        .arg_value(0)
                        .and_then(|v| v.as_str())
                        .is_none_or(str::is_empty)
                {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E001,
                        child.span(),
                        "'@doc' requires a description string",
                        "",
                    ));
                }
            } else if child.name().starts_with('@') {
                if KNOWN_STRUCT_DIRECTIVES.contains(&child.name())
                    || is_error_directive(child.name())
                {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E203,
                        child.span(),
                        format!(
                            "'{}' is not allowed inside fn — use a struct for init/check/request/etc.",
                            child.name()
                        ),
                        "",
                    ));
                } else {
                    // Might be `@init_field` reference in `fn` -> forbidden in `fn` (`E203`)
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E203,
                        child.span(),
                        format!(
                            "'{}' is not allowed inside fn — 'fn' has no '@init' block",
                            child.name()
                        ),
                        "use a struct if you need '@init' precomputed fields",
                    ));
                }
            } else {
                pipeline_children.push(child);
            }
        }

        let expanded = expand_defines(pipeline_children, program, env);
        if expanded.is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                node.span(),
                format!("fn '{name}' must contain at least one pipeline operation"),
                "example: fn title { css \"h1\"; text }",
            ));
            continue;
        }

        let empty_init = HashSet::new();
        let pctx = PipelineLintCtx {
            unit,
            program,
            env,
            is_raw,
            is_fn: true,
            is_table_field: false,
            init_fields: &empty_init,
        };
        lint_pipeline_ops(&pctx, &expanded, diags);
    }
}

/// Expands block `define` references inside a sequence of pipeline or condition child nodes.
pub fn expand_defines<'a>(
    ops: Vec<NodeRef<'a>>,
    program: &'a ResolvedProgram,
    env: &UnitEnv,
) -> Vec<NodeRef<'a>> {
    let mut visited = HashSet::new();
    expand_defines_inner(ops, program, env, &mut visited)
}

fn expand_defines_inner<'a>(
    ops: Vec<NodeRef<'a>>,
    program: &'a ResolvedProgram,
    env: &UnitEnv,
    visited: &mut HashSet<(String, NodeId)>,
) -> Vec<NodeRef<'a>> {
    let mut out = Vec::new();
    for op in ops {
        if op.args().is_empty() && !op.has_children_block() {
            if let Some(&DefineEntry::Block {
                ref unit_path,
                node_id,
            }) = env.defines.get(op.name())
            {
                let key = (unit_path.clone(), node_id);
                if visited.insert(key.clone()) {
                    if let Some(def_unit) = program.units.get(unit_path) {
                        let def_node = def_unit.document.node(node_id);
                        out.extend(expand_defines_inner(
                            def_node.children(),
                            program,
                            env,
                            visited,
                        ));
                    }
                    visited.remove(&key);
                    continue;
                }
            }
        }
        out.push(op);
    }
    out
}

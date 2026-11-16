//! Condition container (`filter`, `assert`, `match`), combinator (`and`, `or`, `not`), and leaf `CondOp` validation.

use crate::kdl::NodeRef;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::linter::pipeline::{
    PipelineLintCtx, arg_as_int, require_arg_count, require_int_args, require_str_args,
    validate_selector_query,
};
use crate::linter::structs::expand_defines;
use crate::linter::{did_you_mean_hint, make_error};
use crate::validators::validate_regex;

const DOM_PREDICATE_OPS: &[&str] = &[
    "attr-contains",
    "attr-ends",
    "attr-eq",
    "attr-in",
    "attr-ne",
    "attr-re",
    "attr-starts",
    "css",
    "has-attr",
    "text-contains",
    "text-ends",
    "text-eq",
    "text-in",
    "text-ne",
    "text-re",
    "text-starts",
    "xpath",
];

pub(crate) const PREDICATE_ONLY_OPS: &[&str] = &[
    "and",
    "attr-contains",
    "attr-ends",
    "attr-eq",
    "attr-in",
    "attr-ne",
    "attr-re",
    "attr-starts",
    "contains",
    "ends",
    "eq",
    "ge",
    "gt",
    "has-attr",
    "in",
    "le",
    "len-eq",
    "len-ge",
    "len-gt",
    "len-le",
    "len-lt",
    "len-ne",
    "len-range",
    "lt",
    "ne",
    "not",
    "or",
    "re-any",
    "starts",
    "text-contains",
    "text-ends",
    "text-eq",
    "text-in",
    "text-ne",
    "text-re",
    "text-starts",
];

pub(crate) const ASSERT_ONLY_PREDICATES: &[&str] = &[
    "ge",
    "gt",
    "le",
    "len-eq",
    "len-ge",
    "len-gt",
    "len-le",
    "len-lt",
    "len-ne",
    "len-range",
    "lt",
    "re-all",
    "re-any",
];

const ALL_PREDICATE_OPS: &[&str] = &[
    "and",
    "attr-contains",
    "attr-ends",
    "attr-eq",
    "attr-in",
    "attr-ne",
    "attr-re",
    "attr-starts",
    "contains",
    "css",
    "ends",
    "eq",
    "ge",
    "gt",
    "has-attr",
    "in",
    "le",
    "len-eq",
    "len-ge",
    "len-gt",
    "len-le",
    "len-lt",
    "len-ne",
    "len-range",
    "lt",
    "ne",
    "not",
    "or",
    "re",
    "re-all",
    "re-any",
    "starts",
    "text-contains",
    "text-ends",
    "text-eq",
    "text-in",
    "text-ne",
    "text-re",
    "text-starts",
    "xpath",
];

/// Enclosing condition container (`filter`, `assert`, or `match`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateContainer {
    /// `filter { ... }`
    Filter,
    /// `assert ["msg"] { ... }`
    Assert,
    /// `match { ... }`
    Match,
}

/// Validates a condition container (`filter`, `assert`, `match`) and its inner condition tree.
pub fn lint_predicate_container<'a>(
    pctx: &PipelineLintCtx<'a>,
    node: NodeRef<'a>,
    idx: usize,
    diags: &mut Vec<Diagnostic>,
) {
    let name = node.name();
    let unit_path = &pctx.unit.path;

    if name == "match" {
        if !pctx.is_table_field {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E203,
                node.span(),
                "'match' is only allowed inside (table)struct fields",
                "use 'filter' or 'assert' outside (table)struct",
            ));
        } else if idx != 0 {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E203,
                node.span(),
                "'match' must be the first operation in a table field pipeline",
                "",
            ));
        }
    }

    if !node.props().is_empty() {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E001,
            node.props()[0].span,
            format!("'{name}' does not accept properties"),
            "",
        ));
    }

    if name == "assert" {
        if node.args().len() > 1 {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.span(),
                "'assert' accepts at most one argument",
                "example: assert \"msg\" { ... }  or  assert { ... }",
            ));
        } else if let Some(arg) = node.arg(0) {
            if !arg.value.is_string() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    arg.span,
                    "'assert' message argument must be a string",
                    "example: assert \"expected non-empty\" { ... }",
                ));
            }
        }
    } else if !node.args().is_empty() {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E001,
            node.span(),
            format!("'{name}' does not accept arguments"),
            format!("move expressions into the children block: {name} {{ ... }}"),
        ));
    }

    let preds = expand_defines(node.children(), pctx.program, pctx.env);
    if preds.is_empty() {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E001,
            node.span(),
            format!("'{name}' block must contain at least one predicate expression"),
            format!("example: {name} {{ css \".item\"; has-attr \"href\" }}"),
        ));
    } else {
        let container = match name {
            "filter" => PredicateContainer::Filter,
            "assert" => PredicateContainer::Assert,
            _ => PredicateContainer::Match,
        };
        lint_predicate_list(pctx, container, &preds, diags);
    }
}

/// Validates a list of condition expressions inside `container`.
pub fn lint_predicate_list<'a>(
    pctx: &PipelineLintCtx<'a>,
    container: PredicateContainer,
    preds: &[NodeRef<'a>],
    diags: &mut Vec<Diagnostic>,
) {
    let unit_path = &pctx.unit.path;

    for &node in preds {
        let name = node.name();
        if name.is_empty() {
            continue;
        }

        // Check if unknown predicate op (`E200`)
        if !ALL_PREDICATE_OPS.contains(&name) {
            if pctx.env.all_program_defines.contains(name) {
                continue;
            }
            let hint = did_you_mean_hint(
                name,
                ALL_PREDICATE_OPS.iter().copied(),
                "valid predicates: eq, ne, contains, starts, ends, re, css, xpath, has-attr, attr-*, text-*, len-*, and, or, not",
            );
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E200,
                node.span(),
                format!("unknown operation '{name}'"),
                hint,
            ));
            continue;
        }

        // Check DOM predicates in `(raw)struct` or `(raw)fn` (`E203`)
        if pctx.is_raw && DOM_PREDICATE_OPS.contains(&name) {
            let target_kind = if pctx.is_fn { "(raw)fn" } else { "(raw)struct" };
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E203,
                node.span(),
                format!(
                    "HTML predicate '{name}' is forbidden in {target_kind} — the input is a plain string, not a DOM"
                ),
                "use string or regex predicates (eq, contains, starts, ends, re, len-*) in (raw) schemas",
            ));
        }

        // Check DOM predicates inside `match` (`E203`)
        if container == PredicateContainer::Match && DOM_PREDICATE_OPS.contains(&name) {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E203,
                node.span(),
                format!(
                    "'{name}' is not allowed inside 'match' block — 'match' evaluates the extracted string key"
                ),
                "use string predicates in 'match': eq, ne, starts, ends, contains, in, re, and, or, not",
            ));
        }

        // Check assert-only predicates outside `assert` (`E203`)
        if ASSERT_ONLY_PREDICATES.contains(&name) && container != PredicateContainer::Assert {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E203,
                node.span(),
                format!("'{name}' is only valid inside an assert block"),
                format!("example: assert {{ {name} ... }}"),
            ));
            continue;
        }

        if !node.props().is_empty() {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.props()[0].span,
                format!("'{name}' does not accept properties"),
                "",
            ));
        }

        // Logic combinators (`not`, `and`, `or`)
        if matches!(name, "not" | "and" | "or") {
            if !node.args().is_empty() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{name}' does not accept arguments"),
                    format!("move expressions into the children block: {name} {{ ... }}"),
                ));
            }
            let sub_preds = expand_defines(node.children(), pctx.program, pctx.env);
            if sub_preds.is_empty() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{name}' block must contain at least one predicate expression"),
                    format!("example: {name} {{ starts \"foo\" }}"),
                ));
            } else {
                lint_predicate_list(pctx, container, &sub_preds, diags);
            }
            continue;
        }

        if node.has_children_block() {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.span(),
                format!("'{name}' does not accept a children block"),
                "",
            ));
        }

        match name {
            "eq" | "ne" | "in" => {
                require_arg_count(
                    unit_path,
                    node,
                    None,
                    Some(1),
                    &format!("{name} \"value\""),
                    diags,
                );
            }
            "starts" | "ends" | "contains" | "text-eq" | "text-ne" | "text-in" | "text-starts"
            | "text-ends" | "text-contains" | "has-attr" => {
                if require_arg_count(
                    unit_path,
                    node,
                    None,
                    Some(1),
                    &format!("{name} \"value\""),
                    diags,
                ) {
                    require_str_args(
                        unit_path,
                        node,
                        &format!("example: {name} \"value\""),
                        diags,
                    );
                }
            }
            "attr-eq" | "attr-ne" | "attr-in" | "attr-starts" | "attr-ends" | "attr-contains" => {
                if require_arg_count(
                    unit_path,
                    node,
                    None,
                    Some(2),
                    &format!("{name} \"href\" \"value\""),
                    diags,
                ) {
                    require_str_args(
                        unit_path,
                        node,
                        &format!("example: {name} \"href\" \"value\""),
                        diags,
                    );
                }
            }
            "attr-re" => {
                if require_arg_count(
                    unit_path,
                    node,
                    Some(2),
                    None,
                    "attr-re \"href\" #\".*\\.com$\"#",
                    diags,
                ) && require_str_args(
                    unit_path,
                    node,
                    "example: attr-re \"href\" #\".*\\.com$\"#",
                    diags,
                ) {
                    let raw_pat = node.arg_value(1).and_then(|v| v.as_str()).unwrap_or("");
                    let pat = pctx.env.resolve_str_arg(raw_pat);
                    if let Err(err_msg) = validate_regex(&pat) {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E002,
                            node.span(),
                            format!("invalid regex pattern: {err_msg}"),
                            "check regex syntax",
                        ));
                    }
                }
            }
            "text-re" | "re" | "re-any" | "re-all" => {
                if require_arg_count(
                    unit_path,
                    node,
                    Some(1),
                    None,
                    &format!("{name} #\"\\d+\"#"),
                    diags,
                ) && require_str_args(
                    unit_path,
                    node,
                    &format!("example: {name} #\"\\d+\"#"),
                    diags,
                ) {
                    let raw_pat = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                    let pat = pctx.env.resolve_str_arg(raw_pat);
                    if let Err(err_msg) = validate_regex(&pat) {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E002,
                            node.span(),
                            format!("invalid regex pattern: {err_msg}"),
                            "check regex syntax",
                        ));
                    }
                }
            }
            "css" | "xpath" => {
                let is_css = name == "css";
                if require_arg_count(
                    unit_path,
                    node,
                    Some(1),
                    None,
                    &format!("{name} \"...\""),
                    diags,
                ) && require_str_args(
                    unit_path,
                    node,
                    &format!("example: {name} \"...\""),
                    diags,
                ) {
                    let raw_q = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                    let q = pctx.env.resolve_str_arg(raw_q);
                    validate_selector_query(unit_path, node.span(), is_css, &q, diags);
                }
            }
            "len-eq" | "len-ne" => {
                if require_arg_count(unit_path, node, None, Some(1), &format!("{name} 5"), diags) {
                    for arg in node.args() {
                        match arg_as_int(arg, pctx.env) {
                            Some(val) if val < 0 => {
                                diags.push(make_error(
                                    unit_path,
                                    DiagnosticCode::E001,
                                    node.span(),
                                    format!("'{name}' argument must be non-negative, got {val}"),
                                    format!("example: {name} 5"),
                                ));
                                break;
                            }
                            Some(_) => {}
                            None => {
                                diags.push(make_error(
                                    unit_path,
                                    DiagnosticCode::E001,
                                    node.span(),
                                    format!(
                                        "'{name}' argument must be integer, got '{}'",
                                        arg.value.to_kdl()
                                    ),
                                    format!("example: {name} 5"),
                                ));
                                break;
                            }
                        }
                    }
                }
            }
            "len-gt" | "len-lt" | "len-ge" | "len-le" | "gt" | "lt" | "ge" | "le" => {
                if require_arg_count(unit_path, node, Some(1), None, &format!("{name} 10"), diags) {
                    require_int_args(unit_path, node, diags);
                }
            }
            "len-range"
                if require_arg_count(unit_path, node, Some(2), None, "len-range 1 100", diags) =>
            {
                require_int_args(unit_path, node, diags);
            }
            _ => {}
        }
    }
}

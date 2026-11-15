//! Pipeline operation validation (`StepOp` arity, argument types, and context rules).

use std::collections::HashSet;

use crate::kdl::{KdlValue, NodeRef};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::gjson::is_malformed_gjson_path;
use crate::imports::{ResolvedProgram, SourceUnit};
use crate::linter::predicates::{
    ASSERT_ONLY_PREDICATES, PREDICATE_ONLY_OPS, lint_predicate_container,
};
use crate::linter::{
    DefineEntry, UnitEnv, did_you_mean_hint, is_define_name, make_error, make_warning,
};
use crate::validators::{validate_css_selector, validate_regex, validate_xpath};

const NO_ARGS_OPS: &[&str] = &[
    "first",
    "last",
    "len",
    "lower",
    "normalize-space",
    "text",
    "to-bool",
    "to-float",
    "to-int",
    "unescape",
    "unique",
    "upper",
];

const TRIM_OPS: &[&str] = &["ltrim", "rtrim", "trim"];
const RM_OPS: &[&str] = &["rm-prefix", "rm-prefix-suffix", "rm-suffix"];

const DOM_PIPELINE_OPS: &[&str] = &[
    "attr",
    "css",
    "css-all",
    "css-remove",
    "raw",
    "remove",
    "rows",
    "table",
    "text",
    "xpath",
    "xpath-all",
    "xpath-remove",
];

const BUILTIN_PIPELINE_OPS: &[&str] = &[
    "assert",
    "attr",
    "css",
    "css-all",
    "css-remove",
    "fallback",
    "filter",
    "first",
    "fmt",
    "index",
    "join",
    "jsonify",
    "last",
    "len",
    "lower",
    "ltrim",
    "match",
    "nested",
    "normalize-space",
    "raw",
    "re",
    "re-all",
    "re-split",
    "re-sub",
    "repl",
    "rm-prefix",
    "rm-prefix-suffix",
    "rm-suffix",
    "rtrim",
    "slice",
    "split",
    "text",
    "to-bool",
    "to-float",
    "to-int",
    "trim",
    "unescape",
    "unique",
    "upper",
    "xpath",
    "xpath-all",
    "xpath-remove",
];

/// Contextual environment for validating a field, directive, or `fn` pipeline.
#[derive(Debug, Clone)]
pub struct PipelineLintCtx<'a> {
    /// Current source unit.
    pub unit: &'a SourceUnit,
    /// Full resolved program (for expanding imported block `define`s).
    pub program: &'a ResolvedProgram,
    /// Unit semantic environment (`define`s and `extension`s).
    pub env: &'a UnitEnv,
    /// Whether the enclosing container is `(raw)struct` or `(raw)fn`.
    pub is_raw: bool,
    /// Whether the enclosing container is a standalone `fn`.
    pub is_fn: bool,
    /// Whether this pipeline is a regular field inside a `(table)struct`.
    pub is_table_field: bool,
    /// Names of precomputed `@init` fields declared on the enclosing `struct`.
    pub init_fields: &'a HashSet<String>,
}

/// Validates a sequence of expanded pipeline operation nodes.
pub fn lint_pipeline_ops<'a>(
    pctx: &PipelineLintCtx<'a>,
    ops: &[NodeRef<'a>],
    diags: &mut Vec<Diagnostic>,
) {
    for (idx, &node) in ops.iter().enumerate() {
        let name = node.name();
        if name.is_empty() {
            continue;
        }

        // 1. Check DOM operations forbidden in `(raw)struct` or `(raw)fn` (`E203`)
        if pctx.is_raw && DOM_PIPELINE_OPS.contains(&name) {
            let target_kind = if pctx.is_fn { "(raw)fn" } else { "(raw)struct" };
            diags.push(make_error(
                &pctx.unit.path,
                DiagnosticCode::E203,
                node.span(),
                format!(
                    "HTML operation '{name}' is forbidden in {target_kind} — the document is a plain string, not a DOM"
                ),
                "use string or regex operations (re, split, trim, etc.) in (raw) schemas",
            ));
        }

        // 2. `@<name>` precomputed `@init` field reference
        if let Some(init_name) = name.strip_prefix('@') {
            if pctx.is_fn {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E203,
                    node.span(),
                    format!("'{name}' is not allowed inside fn"),
                    "",
                ));
            } else if !pctx.init_fields.contains(init_name) {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E301,
                    node.span(),
                    format!("'@{init_name}': field '{init_name}' not found in @init block"),
                    format!("declare it in @init: @init {{ {init_name} {{ ... }} }}"),
                ));
            }
            if !node.args().is_empty() || !node.props().is_empty() || node.has_children_block() {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{name}' does not accept arguments, properties, or children"),
                    "",
                ));
            }
            if idx > 0 {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E203,
                    node.span(),
                    format!("'{name}' must be the first operation in a field pipeline"),
                    "",
                ));
            }
            continue;
        }

        // 3. Legacy `self <name>`
        if name == "self" {
            let ref_name = node
                .arg_value(0)
                .and_then(|v| v.as_str())
                .unwrap_or("<name>");
            diags.push(make_error(
                &pctx.unit.path,
                DiagnosticCode::E203,
                node.span(),
                format!(
                    "'self {ref_name}' syntax is no longer supported; use '@{ref_name}' instead"
                ),
                format!("replace 'self {ref_name}' with '@{ref_name}'"),
            ));
            continue;
        }

        // 4. Custom extension call `!Namespace.op`
        if let Some(ext_ref) = name.strip_prefix('!') {
            if !node.args().is_empty() || !node.props().is_empty() || node.has_children_block() {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("custom operation '!{ext_ref}' does not accept entries"),
                    "",
                ));
            }
            let Some((ns, _op)) = ext_ref.split_once('.') else {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("custom operation '!{ext_ref}' must use '!<Namespace>.<op>' syntax"),
                    "",
                ));
                continue;
            };
            if pctx.env.extension_namespaces.contains(ns)
                && !pctx.env.extension_ops.contains(ext_ref)
            {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E300,
                    node.span(),
                    format!("unknown extension operation '{ext_ref}'"),
                    "check the operation name in the extension declaration",
                ));
            }
            continue;
        }

        // 5. Predicate-only operations used directly in a pipeline (`E203`)
        if PREDICATE_ONLY_OPS.contains(&name) {
            if ASSERT_ONLY_PREDICATES.contains(&name) {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E203,
                    node.span(),
                    format!("'{name}' is only valid inside an assert block"),
                    format!("example: assert {{ {name} ... }}"),
                ));
            } else {
                diags.push(make_error(
                    &pctx.unit.path,
                    DiagnosticCode::E203,
                    node.span(),
                    format!("'{name}' is only valid inside a predicate block"),
                    format!(
                        "wrap it in one of: and, assert, filter, match, not, or. Example: filter {{ {name} ... }}"
                    ),
                ));
            }
            continue;
        }

        // 6. Un-prefixed extension call `Namespace.op`
        if pctx.env.extension_ops.contains(name)
            || pctx.env.all_program_extension_ops.contains(name)
        {
            diags.push(make_error(
                &pctx.unit.path,
                DiagnosticCode::E001,
                node.span(),
                format!("custom operation '{name}' requires the '!' prefix"),
                format!("use '!{name}'"),
            ));
            continue;
        }

        // 7. Scalar `define` used as a pipeline operation
        if let Some(DefineEntry::Scalar(_)) = pctx.env.defines.get(name) {
            diags.push(make_error(
                &pctx.unit.path,
                DiagnosticCode::E001,
                node.span(),
                format!("'{name}' is a scalar define — cannot be used as a pipeline operation"),
                format!("use a block define: define {name} {{ ... }}"),
            ));
            continue;
        }

        // 8. Check if `name` is an unimported `Define` in another file (Pass 2 already reported `E300`)
        if !BUILTIN_PIPELINE_OPS.contains(&name) {
            if pctx.env.all_program_defines.contains(name) {
                continue;
            }
            let candidates = BUILTIN_PIPELINE_OPS
                .iter()
                .copied()
                .chain(PREDICATE_ONLY_OPS.iter().copied())
                .chain(pctx.env.defines.iter().filter_map(|(k, v)| match v {
                    DefineEntry::Block { .. } => Some(k.as_str()),
                    DefineEntry::Scalar(_) => None,
                }));
            let hint = did_you_mean_hint(
                name,
                candidates,
                &format!("check spelling or declare it: define {name} {{ ... }}"),
            );
            diags.push(make_error(
                &pctx.unit.path,
                DiagnosticCode::E200,
                node.span(),
                format!("unknown operation '{name}'"),
                hint,
            ));
            continue;
        }

        // 9. Validate built-in pipeline operation
        lint_single_pipeline_op(pctx, node, idx, ops.len(), diags);
    }
}

fn lint_single_pipeline_op<'a>(
    pctx: &PipelineLintCtx<'a>,
    node: NodeRef<'a>,
    idx: usize,
    total_ops: usize,
    diags: &mut Vec<Diagnostic>,
) {
    let name = node.name();
    let unit_path = &pctx.unit.path;

    if NO_ARGS_OPS.contains(&name) {
        if !node.args().is_empty() {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.span(),
                format!("'{name}' does not accept arguments"),
                format!("remove arguments: use just '{name}'"),
            ));
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
        if name == "unique" {
            for prop in node.props() {
                if prop.key != "keep-order" {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E001,
                        prop.span,
                        format!("unknown property '{}' on 'unique'", prop.key),
                        "only 'keep-order=#true|#false' is supported on 'unique'",
                    ));
                } else if !prop.value.is_bool() {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E002,
                        prop.value_span,
                        "'keep-order' property on 'unique' must be a boolean (#true or #false)",
                        "example: unique keep-order=#true",
                    ));
                }
            }
        } else if !node.props().is_empty() {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.props()[0].span,
                format!("'{name}' does not accept properties"),
                "",
            ));
        }
        return;
    }

    match name {
        "css" | "css-all" | "xpath" | "xpath-all" => {
            let is_css = name.starts_with("css");
            let noun = if is_css {
                "a selector"
            } else {
                "an expression"
            };
            if !node.props().is_empty() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.props()[0].span,
                    format!("'{name}' does not accept properties"),
                    "",
                ));
            }
            if node.args().is_empty() && !node.has_children_block() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{name}' requires {noun}"),
                    format!("example: {name} \"...\""),
                ));
                return;
            }
            if !node.args().is_empty() && node.has_children_block() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{name}' cannot have both positional arguments and a children block"),
                    "",
                ));
                return;
            }
            if node.has_children_block() {
                let children = node.children();
                if children.is_empty() {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E001,
                        node.span(),
                        format!("'{name}' requires {noun}"),
                        "",
                    ));
                    return;
                }
                for child in children {
                    if !child.args().is_empty()
                        || !child.props().is_empty()
                        || child.has_children_block()
                    {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E001,
                            child.span(),
                            format!(
                                "selector entry inside '{name} {{ ... }}' cannot have arguments, properties, or children"
                            ),
                            "",
                        ));
                    }
                    let q = pctx.env.resolve_str_arg(child.name());
                    validate_selector_query(unit_path, child.span(), is_css, &q, diags);
                }
            } else {
                if node.args().len() > 1 {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E001,
                        node.span(),
                        format!(
                            "'{name}' requires exactly 1 argument, got {}",
                            node.args().len()
                        ),
                        format!("example: {name} \"...\""),
                    ));
                    return;
                }
                if !require_str_args(unit_path, node, &format!("example: {name} \"...\""), diags) {
                    return;
                }
                let raw_q = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                let q = pctx.env.resolve_str_arg(raw_q);
                validate_selector_query(unit_path, node.span(), is_css, &q, diags);
            }
        }
        "css-remove" | "xpath-remove" => {
            let is_css = name == "css-remove";
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(
                unit_path,
                node,
                Some(1),
                None,
                &format!("{name} \"...\""),
                diags,
            ) && require_str_args(unit_path, node, &format!("example: {name} \"...\""), diags)
            {
                let raw_q = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                let q = pctx.env.resolve_str_arg(raw_q);
                validate_selector_query(unit_path, node.span(), is_css, &q, diags);
            }
        }
        "attr" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(unit_path, node, None, Some(1), "attr \"href\"", diags) {
                require_str_args(unit_path, node, "example: attr \"href\"", diags);
            }
        }
        "raw" => {
            reject_props_and_children(unit_path, node, diags);
            if node.args().len() > 1 {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'raw' accepts at most 1 argument",
                    "example: raw  or  raw outer  or  raw inner",
                ));
            } else if let Some(arg) = node.arg(0) {
                match arg.value.as_str() {
                    Some("outer" | "inner") => {}
                    Some(other) => {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E001,
                            arg.span,
                            format!("invalid 'raw' mode '{other}' — expected 'outer' or 'inner'"),
                            "example: raw  or  raw outer  or  raw inner",
                        ));
                    }
                    None => {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E001,
                            arg.span,
                            "'raw' mode argument must be 'outer' or 'inner'",
                            "example: raw  or  raw outer  or  raw inner",
                        ));
                    }
                }
            }
        }
        _ if TRIM_OPS.contains(&name) => {
            reject_props_and_children(unit_path, node, diags);
            if node.args().len() > 1 {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    format!("'{name}' accepts at most 1 argument"),
                    format!("example: {name}  or  {name} \"chars\""),
                ));
            } else if !node.args().is_empty() {
                require_str_args(
                    unit_path,
                    node,
                    &format!("example: {name}  or  {name} \"chars\""),
                    diags,
                );
            }
        }
        _ if RM_OPS.contains(&name) => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(
                unit_path,
                node,
                Some(1),
                None,
                &format!("{name} \"substring\""),
                diags,
            ) {
                require_str_args(
                    unit_path,
                    node,
                    &format!("example: {name} \"substring\""),
                    diags,
                );
            }
        }
        "fmt" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(
                unit_path,
                node,
                Some(1),
                None,
                "fmt \"prefix-{{}}-suffix\"",
                diags,
            ) && require_str_args(
                unit_path,
                node,
                "example: fmt \"prefix-{{}}-suffix\"",
                diags,
            ) {
                let raw_tmpl = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                if !(is_define_name(raw_tmpl)
                    || raw_tmpl.contains("{{}}")
                    || raw_tmpl.contains("{}"))
                {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E001,
                        node.span(),
                        "'fmt' template is missing the '{{}}' placeholder",
                        format!("add placeholder to template, example: fmt \"{raw_tmpl}{{{{}}}}\""),
                    ));
                }
            }
        }
        "repl" => {
            if !node.props().is_empty() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.props()[0].span,
                    "'repl' does not accept properties",
                    "",
                ));
            }
            let has_args = !node.args().is_empty();
            let has_children = node.has_children();
            if !has_args && !has_children {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'repl' requires 2 arguments or a children block",
                    "example: repl \"old\" \"new\"  or  repl { \"old\" \"new\"; \"foo\" \"bar\" }",
                ));
            } else if has_args && node.has_children_block() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'repl' cannot have both positional arguments and a children block",
                    "",
                ));
            } else if has_args {
                if require_arg_count(
                    unit_path,
                    node,
                    Some(2),
                    None,
                    "repl \"old\" \"new\"",
                    diags,
                ) {
                    require_str_args(unit_path, node, "example: repl \"old\" \"new\"", diags);
                }
            } else {
                for pair in node.children() {
                    if pair.args().len() != 1
                        || pair.arg_value(0).and_then(|v| v.as_str()).is_none()
                        || !pair.props().is_empty()
                        || pair.has_children_block()
                    {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E001,
                            pair.span(),
                            "each replacement entry inside 'repl { ... }' requires 1 replacement string argument",
                            "example: repl { \"old\" \"new\" }",
                        ));
                    }
                }
            }
        }
        "split" | "join" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(
                unit_path,
                node,
                Some(1),
                None,
                &format!("{name} \" \""),
                diags,
            ) {
                require_str_args(unit_path, node, &format!("example: {name} \" \""), diags);
            }
        }
        "re" | "re-all" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(
                unit_path,
                node,
                Some(1),
                None,
                &format!("{name} #\"(\\d+)\"#"),
                diags,
            ) && require_str_args(
                unit_path,
                node,
                &format!("example: {name} #\"(\\d+)\"#"),
                diags,
            ) {
                let raw_pat = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                let pat = pctx.env.resolve_str_arg(raw_pat);
                match validate_regex(&pat) {
                    Ok(groups) => {
                        if groups == 0 {
                            diags.push(make_error(
                                unit_path,
                                DiagnosticCode::E001,
                                node.span(),
                                format!("'{name}' pattern must have exactly one capture group"),
                                format!("wrap the match in a group: {name} #\"({pat})\"#"),
                            ));
                        } else if groups > 1 {
                            diags.push(make_error(
                                unit_path,
                                DiagnosticCode::E001,
                                node.span(),
                                format!(
                                    "'{name}' pattern must have exactly one capture group, got {groups}"
                                ),
                                "use a non-capturing group (?:...) for grouping without capturing",
                            ));
                        }
                    }
                    Err(err_msg) => {
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
        }
        "re-sub" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(
                unit_path,
                node,
                Some(2),
                None,
                "re-sub #\"\\D\"# \"\"",
                diags,
            ) && require_str_args(unit_path, node, "example: re-sub #\"\\D\"# \"\"", diags)
            {
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
        "re-split" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(unit_path, node, Some(1), None, "re-split #\"\\s+\"#", diags)
                && require_str_args(unit_path, node, "example: re-split #\"\\s+\"#", diags)
            {
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
        "index" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(unit_path, node, Some(1), None, "index 0", diags) {
                require_int_args(unit_path, node, diags);
            }
        }
        "slice" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(unit_path, node, Some(2), None, "slice 0 10", diags) {
                require_int_args(unit_path, node, diags);
            }
        }
        "jsonify" => {
            if node.has_children_block() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'jsonify' does not accept a children block",
                    "",
                ));
            }
            if require_arg_count(unit_path, node, Some(1), None, "jsonify MySchema", diags) {
                require_str_args(unit_path, node, "example: jsonify MySchema", diags);
            }
            for prop in node.props() {
                if prop.key != "path" {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E001,
                        prop.span,
                        format!("unknown property '{}' on 'jsonify'", prop.key),
                        "only 'path=\"...\"' is supported on 'jsonify'",
                    ));
                }
            }
            if let Some(path_prop) = node.prop("path") {
                match path_prop.value.as_str() {
                    None => {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E001,
                            path_prop.span,
                            "'path' property must be a non-empty string",
                            "example: jsonify MySchema path=\"response.data\"",
                        ));
                    }
                    Some("") => {
                        diags.push(make_error(
                            unit_path,
                            DiagnosticCode::E040,
                            path_prop.span,
                            "'path' property must be a non-empty string (malformed dot-path '')",
                            "example: jsonify MySchema path=\"response.data\"",
                        ));
                    }
                    Some(raw_path) => {
                        let path_val = pctx.env.resolve_str_arg(raw_path);
                        if path_val != path_val.trim() {
                            diags.push(make_warning(
                                unit_path,
                                DiagnosticCode::W040,
                                path_prop.span,
                                format!("leading or trailing whitespace in 'path' '{path_val}'"),
                                format!("remove whitespace: path=\"{}\"", path_val.trim()),
                            ));
                        }
                        if is_malformed_gjson_path(path_val.trim()) {
                            diags.push(make_error(
                                unit_path,
                                DiagnosticCode::E040,
                                path_prop.span,
                                format!("malformed dot-path '{path_val}'"),
                                "example: jsonify MySchema path=\"response.data\"",
                            ));
                        }
                    }
                }
            }
        }
        "nested" => {
            reject_props_and_children(unit_path, node, diags);
            if require_arg_count(unit_path, node, Some(1), None, "nested MyStruct", diags) {
                require_str_args(unit_path, node, "example: nested MyStruct", diags);
            }
        }
        "fallback" => {
            if !node.props().is_empty() {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.props()[0].span,
                    "'fallback' does not accept properties",
                    "",
                ));
            }
            if node.has_children_block() {
                if !node.args().is_empty() || node.has_children() {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E001,
                        node.span(),
                        "array 'fallback {}' must be an empty braces block without arguments",
                        "example: fallback {}",
                    ));
                }
            } else if node.args().len() != 1 {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'fallback' requires 1 literal argument or '{}'",
                    "example: fallback \"\"  or  fallback #null  or  fallback {}",
                ));
            }
            if idx + 1 < total_ops {
                diags.push(make_error(
                    unit_path,
                    DiagnosticCode::E203,
                    node.span(),
                    "'fallback' must be the last operation in a pipeline",
                    "move 'fallback' to the end of the pipeline",
                ));
            }
        }
        "filter" | "assert" | "match" => {
            lint_predicate_container(pctx, node, idx, diags);
        }
        _ => {}
    }
}

pub(crate) fn validate_selector_query(
    unit_path: &str,
    span: crate::kdl::Span,
    is_css: bool,
    query: &str,
    diags: &mut Vec<Diagnostic>,
) {
    if is_css {
        if let Err(err_msg) = validate_css_selector(query) {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E002,
                span,
                format!("invalid CSS selector: {err_msg}"),
                "check selector syntax",
            ));
        }
    } else if let Err(err_msg) = validate_xpath(query) {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E002,
            span,
            format!("invalid XPath expression: {err_msg}"),
            "check XPath syntax",
        ));
    }
}

pub(crate) fn reject_props_and_children(
    unit_path: &str,
    node: NodeRef<'_>,
    diags: &mut Vec<Diagnostic>,
) {
    if !node.props().is_empty() {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E001,
            node.props()[0].span,
            format!("'{}' does not accept properties", node.name()),
            "",
        ));
    }
    if node.has_children_block() {
        diags.push(make_error(
            unit_path,
            DiagnosticCode::E001,
            node.span(),
            format!("'{}' does not accept a children block", node.name()),
            "",
        ));
    }
}

pub(crate) fn require_arg_count(
    unit_path: &str,
    node: NodeRef<'_>,
    exact: Option<usize>,
    min_count: Option<usize>,
    example: &str,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let count = node.args().len();
    let name = node.name();

    if let Some(ex) = exact {
        if count != ex {
            let noun = if ex == 1 { "argument" } else { "arguments" };
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.span(),
                format!("'{name}' requires exactly {ex} {noun}, got {count}"),
                example,
            ));
            return false;
        }
    }

    if let Some(min) = min_count {
        if count < min {
            let noun = if min == 1 { "argument" } else { "arguments" };
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.span(),
                format!("'{name}' requires at least {min} {noun}, got {count}"),
                example,
            ));
            return false;
        }
    }

    true
}

pub(crate) fn require_str_args(
    unit_path: &str,
    node: NodeRef<'_>,
    example: &str,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let name = node.name();
    for arg in node.args() {
        if !arg.value.is_string() {
            let val_type = match arg.value {
                KdlValue::Null => "null",
                KdlValue::Bool(_) => "bool",
                KdlValue::Int(_) => "int",
                KdlValue::Float(_) => "float",
                KdlValue::String(_) => "str",
            };
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.span(),
                format!("'{name}' argument must be a string, got {val_type}"),
                example,
            ));
            return false;
        }
    }
    true
}

pub(crate) fn require_int_args(
    unit_path: &str,
    node: NodeRef<'_>,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let name = node.name();
    for arg in node.args() {
        let ok = match &arg.value {
            KdlValue::Int(_) => true,
            KdlValue::String(s) => s.parse::<i64>().is_ok(),
            _ => false,
        };
        if !ok {
            diags.push(make_error(
                unit_path,
                DiagnosticCode::E001,
                node.span(),
                format!(
                    "'{name}' arguments must be integers, got '{}'",
                    arg.value.to_kdl()
                ),
                format!("example: {name} 0"),
            ));
            return false;
        }
    }
    true
}

pub(crate) fn arg_as_int(arg: &crate::kdl::KdlArg, env: &UnitEnv) -> Option<i128> {
    match &arg.value {
        KdlValue::Int(n) => Some(*n),
        KdlValue::String(s) => {
            let resolved = env.resolve_str_arg(s);
            resolved.parse::<i128>().ok()
        }
        _ => None,
    }
}

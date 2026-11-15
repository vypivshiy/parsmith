//! Top-level declaration validation (`E200`), `@doc`, `define`, and `extension` validation.

use std::collections::HashSet;

use crate::kdl::NodeRef;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::imports::SourceUnit;
use crate::linter::naming::is_valid_symbol;
pub use crate::linter::order::lint_topdown_order;
use crate::linter::{did_you_mean_hint, is_define_name, is_kebab_or_snake_ident, make_error};

const TOP_LEVEL_NODES: &[&str] = &[
    "@doc",
    "define",
    "extension",
    "fn",
    "import",
    "json",
    "struct",
];

/// Validates top-level nodes using KDL selector `:root:not(@doc, json, struct, fn, define, extension, import)`.
pub fn lint_top_level(unit: &SourceUnit, diags: &mut Vec<Diagnostic>) {
    if let Ok(nodes) = unit
        .document
        .select(":root:not(@doc, json, struct, fn, define, extension, import)")
    {
        for node in nodes {
            let hint = did_you_mean_hint(
                node.name(),
                TOP_LEVEL_NODES.iter().copied(),
                "valid top-level declarations: @doc, import, define, extension, json, struct, fn",
            );
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E200,
                node.name_span(),
                format!("Unknown node: {}", node.name()),
                hint,
            ));
        }
    }
}

/// Validates top-level `@doc` declarations.
pub fn lint_doc_nodes(unit: &SourceUnit, diags: &mut Vec<Diagnostic>) {
    if let Ok(nodes) = unit.document.select("@doc:root") {
        for node in nodes {
            if let Some(ty) = node.ty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E400,
                    node.type_span().unwrap_or_else(|| node.span()),
                    format!("'@doc' cannot have a type annotation '({ty})'"),
                    "remove type annotation from '@doc'",
                ));
            }
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
                    "example: @doc \"Module description\"",
                ));
            }
            if !node.props().is_empty() || node.has_children_block() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'@doc' does not accept properties or children",
                    "example: @doc \"Module description\"",
                ));
            }
        }
    }
}

/// Validates `extension` blocks (`E001`, `E002`, `E100`, `E200`, `E203`, `E400`, `E402`, `E403`).
pub fn lint_extensions(unit: &SourceUnit, diags: &mut Vec<Diagnostic>) {
    let Ok(nodes) = unit.document.select("extension:root") else {
        return;
    };

    let mut seen_namespaces = HashSet::new();
    for node in nodes {
        if let Some(ty) = node.ty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E400,
                node.type_span().unwrap_or_else(|| node.span()),
                format!("'extension' cannot have a type annotation '({ty})'"),
                "remove the type annotation from 'extension'",
            ));
        }

        let namespace = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
        if node.args().len() != 1 || !node.props().is_empty() || !is_kebab_or_snake_ident(namespace)
        {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                node.span(),
                "'extension' requires one portable namespace",
                "example: extension Utils { ... }",
            ));
        } else if !seen_namespaces.insert(namespace.to_owned()) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E402,
                node.span(),
                format!("duplicate extension namespace '{namespace}'"),
                "",
            ));
        }

        let operations = node.children();
        if operations.is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                node.span(),
                format!("extension '{namespace}' must declare at least one operation"),
                "",
            ));
        }

        let mut seen_operations = HashSet::new();
        for op in operations {
            if !is_kebab_or_snake_ident(op.name())
                || !op.args().is_empty()
                || !op.props().is_empty()
                || op.ty().is_some()
            {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    op.span(),
                    "extension operation must be a portable child node without entries",
                    "",
                ));
            }

            if !seen_operations.insert(op.name().to_owned()) {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E402,
                    op.span(),
                    format!("duplicate extension operation '{namespace}.{}'", op.name()),
                    "",
                ));
            }

            let op_children = op.children();
            let mut sigs = Vec::new();
            let mut targets = Vec::new();

            for child in op_children {
                match child.name() {
                    "sig" => sigs.push(child),
                    "py" | "js" | "ts" | "go" | "rust" | "ex" | "elixir" | "php" | "cs"
                    | "csharp" | "kt" | "kotlin" => targets.push(child),
                    other => {
                        let hint = did_you_mean_hint(
                            other,
                            [
                                "sig", "py", "js", "ts", "go", "rust", "ex", "elixir", "php", "cs",
                                "csharp", "kt", "kotlin",
                            ],
                            "valid entries: sig, py, js, ts, go, rust, ex, php, cs, kt",
                        );
                        diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E200,
                            child.span(),
                            format!("unknown extension operation entry '{other}'"),
                            hint,
                        ));
                    }
                }
            }

            if sigs.len() != 1
                || sigs[0].args().len() != 2
                || !sigs[0].props().is_empty()
                || sigs[0].has_children_block()
            {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    op.span(),
                    "extension operation requires exactly one `sig <input> <output>`",
                    "example: sig str str",
                ));
            } else {
                let sig = sigs[0];
                lint_extension_type(unit, sig, 0, diags);
                lint_extension_type(unit, sig, 1, diags);

                let in_str = sig
                    .arg_value(0)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_end_matches('?');
                let out_str = sig
                    .arg_value(1)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_end_matches('?');
                if out_str == "T" && in_str != "T" {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        sig.span(),
                        "generic output T must be bound by generic input T",
                        "",
                    ));
                }
            }

            if targets.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    op.span(),
                    "extension operation requires at least one target",
                    "valid targets: py, js, ts, go, rust, ex, php, cs, kt",
                ));
            }

            let mut seen_targets = HashSet::new();
            for target in targets {
                let canon = canonical_extension_target_key(target.name());
                if !seen_targets.insert(canon.to_owned()) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E402,
                        target.span(),
                        format!("duplicate extension target '{}'", target.name()),
                        "",
                    ));
                }
                lint_extension_target(unit, target, diags);
            }
        }
    }
}

pub(crate) fn canonical_extension_target_key(name: &str) -> &str {
    match name {
        "elixir" => "ex",
        "csharp" => "cs",
        "kotlin" => "kt",
        other => other,
    }
}

fn lint_extension_type(
    unit: &SourceUnit,
    sig_node: NodeRef<'_>,
    arg_idx: usize,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(arg) = sig_node.arg(arg_idx) else {
        return;
    };
    let Some(raw_val) = arg.value.as_str() else {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E400,
            arg.span,
            "extension signature type must be a type name",
            "valid types: T, bool, doc, float, int, str",
        ));
        return;
    };

    let name = raw_val.trim_end_matches('?');
    let ann = arg.ty();

    if !matches!(name, "T" | "bool" | "doc" | "float" | "int" | "str") {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E400,
            sig_node.span(),
            format!("unknown extension signature type '{name}'"),
            "valid types: T, bool, doc, float, int, str",
        ));
    }

    if let Some(a) = ann {
        if a != "array" {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E400,
                sig_node.span(),
                format!("unsupported extension type annotation '({a})'"),
                "only '(array)' is supported",
            ));
        }
    }

    if name == "T" && (ann.is_some() || raw_val.ends_with('?')) {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E400,
            sig_node.span(),
            "generic type T cannot have modifiers",
            "",
        ));
    }
}

fn lint_extension_target(unit: &SourceUnit, target_node: NodeRef<'_>, diags: &mut Vec<Diagnostic>) {
    let target_name = target_node.name();
    let mut emits = Vec::new();

    for child in target_node.children() {
        match child.name() {
            "emit" => emits.push(child),
            "import" => lint_extension_import(unit, child, target_name, diags),
            "helper" => {
                let helper_name = child.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                if child.args().len() != 1 || !is_valid_symbol(target_name, helper_name) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E403,
                        child.span(),
                        "helper requires one portable identifier",
                        "",
                    ));
                }

                let mut sources = Vec::new();
                for entry in child.children() {
                    match entry.name() {
                        "import" => lint_extension_import(unit, entry, target_name, diags),
                        "source" => sources.push(entry),
                        other => {
                            let hint = did_you_mean_hint(
                                other,
                                ["import", "source"],
                                "valid entries: import, source",
                            );
                            diags.push(make_error(
                                &unit.path,
                                DiagnosticCode::E200,
                                entry.span(),
                                format!("unknown helper entry '{other}'"),
                                hint,
                            ));
                        }
                    }
                }
                if sources.len() != 1
                    || sources[0].args().len() != 1
                    || sources[0].arg_value(0).and_then(|v| v.as_str()).is_none()
                {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E001,
                        child.span(),
                        "helper requires exactly one `source <code>` entry",
                        "",
                    ));
                }
            }
            other => {
                let hint = did_you_mean_hint(
                    other,
                    ["emit", "helper", "import"],
                    "valid entries: emit, helper, import",
                );
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E200,
                    child.span(),
                    format!("unknown '{target_name}' extension entry '{other}'"),
                    hint,
                ));
            }
        }
    }

    if emits.len() != 1
        || emits[0].args().len() != 1
        || emits[0].arg_value(0).and_then(|v| v.as_str()).is_none()
    {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            target_node.span(),
            format!("extension target '{target_name}' requires exactly one `emit <template>`"),
            "",
        ));
    } else {
        let emit_node = emits[0];
        let template = emit_node
            .arg_value(0)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let placeholders = extract_double_brace_tokens(template);
        let mut unknown: Vec<&str> = placeholders
            .iter()
            .copied()
            .filter(|&p| !matches!(p, "in" | "in_type" | "out" | "out_type"))
            .collect();
        unknown.sort();
        unknown.dedup();
        if !unknown.is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E002,
                emit_node.span(),
                format!(
                    "unknown extension template placeholders: {}",
                    unknown.join(", ")
                ),
                "",
            ));
        }
        if !placeholders.contains(&"out") {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E002,
                emit_node.span(),
                "extension emit template must assign '{{out}}'",
                "",
            ));
        }
    }
}

fn lint_extension_import(
    unit: &SourceUnit,
    node: NodeRef<'_>,
    target: &str,
    diags: &mut Vec<Diagnostic>,
) {
    if node.args().len() != 1
        || node.arg_value(0).and_then(|v| v.as_str()).is_none()
        || node.has_children_block()
    {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "extension import requires one string argument and no children",
            "",
        ));
    }

    let mut unknown_props: Vec<&str> = node
        .props()
        .iter()
        .map(|p| p.key.as_str())
        .filter(|&k| k != "alias")
        .collect();
    unknown_props.sort();
    unknown_props.dedup();
    if !unknown_props.is_empty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E002,
            node.span(),
            format!(
                "unknown extension import properties: {}",
                unknown_props.join(", ")
            ),
            "",
        ));
    }

    if node.has_prop("alias") && !matches!(target, "go" | "rust") {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E203,
            node.span(),
            "import alias is only supported for Go package imports",
            "",
        ));
    }
}

pub(crate) fn extract_double_brace_tokens(template: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let inner = &after[..end];
        if !inner.contains('{') && !inner.contains('}') {
            out.push(inner);
        }
        rest = &after[end + 2..];
    }
    out
}

/// Validates `define` declarations (`E001`, `E002`, `E400`, `E402`).
pub fn lint_defines(unit: &SourceUnit, diags: &mut Vec<Diagnostic>) {
    let Ok(nodes) = unit.document.select("define:root") else {
        return;
    };

    let mut seen_defines = HashSet::new();

    for node in nodes {
        if let Some(ty) = node.ty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E400,
                node.type_span().unwrap_or_else(|| node.span()),
                format!("'define' cannot have a type annotation '({ty})'"),
                "remove the type annotation from 'define'",
            ));
        }

        let args = node.args();
        let has_children = node.has_children();

        if !args.is_empty() {
            if args.len() > 1 {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "'define' accepts at most 1 argument",
                    "use `define NAME { ... }` or `define KEY=value`",
                ));
            }
            if let Some(name) = args[0].value.as_str() {
                if !is_define_name(name) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E002,
                        args[0].span,
                        format!("define name '{name}' must be UPPER_CASE ([A-Z_][A-Z0-9_-]*)"),
                        "use UPPER_CASE: define MY-VAR=... or define MY_BLOCK { ... }",
                    ));
                } else if has_children && !seen_defines.insert(name.to_owned()) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E402,
                        args[0].span,
                        format!("duplicate define '{name}'"),
                        format!("remove or rename duplicate define '{name}'"),
                    ));
                }
            } else {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    args[0].span,
                    "block 'define' name must be a string identifier",
                    "example: define EXTRACT-HREF { css \"a\"; attr \"href\" }",
                ));
            }
        }

        if has_children {
            if args.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "block 'define' requires a name",
                    "example: define EXTRACT-HREF { css \"a\"; attr \"href\" }",
                ));
            }
            if !node.props().is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.props()[0].span,
                    "block 'define' cannot have properties",
                    "place scalar defines in a separate `define KEY=value` statement",
                ));
            }
        } else if node.has_children_block() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                node.span(),
                "block 'define' must contain at least one operation or field",
                "example: define EXTRACT-HREF { css \"a\"; attr \"href\" }",
            ));
        } else if node.props().is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                node.span(),
                "'define' must be scalar (NAME=value) or block (NAME { ... })",
                "example: define BASE-URL=\"https://example.com\"",
            ));
        } else {
            if !args.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    node.span(),
                    "scalar 'define' cannot have positional arguments",
                    "use `define NAME=\"value\"`",
                ));
            }
            for prop in node.props() {
                if !is_define_name(&prop.key) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E002,
                        prop.key_span,
                        format!(
                            "define name '{}' must be UPPER_CASE ([A-Z_][A-Z0-9_-]*)",
                            prop.key
                        ),
                        "use UPPER_CASE: define MY-VAR=... or define MY_BLOCK { ... }",
                    ));
                } else if !seen_defines.insert(prop.key.clone()) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E402,
                        prop.key_span,
                        format!("duplicate define '{}'", prop.key),
                        format!("remove or rename duplicate define '{}'", prop.key),
                    ));
                }
            }
        }
    }
}

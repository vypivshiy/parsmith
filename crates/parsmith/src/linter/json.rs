//! `json`, `(array)json`, `(dict)json`, inline JSON & `@value` blocks, and GJSON path validation (`E040`, `E041`, `W011`, `W040`, `W041`).

use std::collections::HashSet;

use crate::kdl::{NodeId, NodeRef};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::gjson::is_malformed_gjson_path;
use crate::imports::{ResolvedProgram, SourceUnit};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::{DefineEntry, UnitEnv, is_portable_ident, make_error, make_warning};

const VALID_JSON_MODIFIERS: &[&str] = &["@omitempty", "@skip"];
const VALID_KEY_TYPES: &[&str] = &["bool", "float", "int", "str"];

/// Checks whether a JSON field node declares an inline `(dict)` schema.
pub fn is_dict_json_field(node: NodeRef<'_>) -> bool {
    let raw_ann = node.ty().unwrap_or("").trim_end_matches('?');
    let type_prop = node
        .prop("type")
        .and_then(|p| p.value.as_str())
        .unwrap_or("");
    raw_ann == "dict"
        || type_prop == "dict"
        || node.args().iter().any(|arg| {
            arg.ty().unwrap_or("").trim_end_matches('?') == "dict"
                || arg.value.as_str().unwrap_or("").trim_end_matches('?') == "(dict)"
        })
}

/// Extracts `(explicit_model_name, is_array)` from an `@value` directive node.
pub fn extract_value_model_and_array(child: NodeRef<'_>) -> (String, bool) {
    let raw_ann = child.ty().unwrap_or("").trim_end_matches('?');
    let mut is_arr = raw_ann.eq_ignore_ascii_case("array");
    let mut explicit_model_name = String::new();
    if !raw_ann.is_empty() && !raw_ann.eq_ignore_ascii_case("array") && raw_ann != "@skip" {
        explicit_model_name = raw_ann.to_owned();
    }

    for arg in child.args() {
        let arg_ann = arg.ty().unwrap_or("").trim_end_matches('?');
        if arg_ann.eq_ignore_ascii_case("array") {
            is_arr = true;
        }
        let Some(mut val_str) = arg.value.as_str() else {
            continue;
        };
        val_str = val_str.trim_end_matches('?');
        if val_str.starts_with('@') {
            continue;
        }
        if let Some(stripped) = val_str.strip_prefix("(array)") {
            is_arr = true;
            val_str = stripped.trim_end_matches('?');
        }
        if val_str.eq_ignore_ascii_case("array") || val_str.eq_ignore_ascii_case("(array)") {
            is_arr = true;
        } else if explicit_model_name.is_empty() && !val_str.is_empty() {
            explicit_model_name = val_str.to_owned();
        }
    }

    (explicit_model_name, is_arr)
}

/// Synthesizes the canonical PascalCase schema name for an inline `@value` block.
pub fn synthesize_value_schema_name(
    explicit_model_name: &str,
    explicit_dict_name: &str,
    parent_json_name: &str,
    field_name: &str,
) -> String {
    if !explicit_model_name.is_empty() {
        return explicit_model_name.to_owned();
    }
    if !explicit_dict_name.is_empty() {
        return format!("{explicit_dict_name}Value");
    }
    let clean_field = field_name.trim_end_matches('?');
    if !parent_json_name.is_empty() && !clean_field.is_empty() {
        return format!("{parent_json_name}{}Value", to_pascal_case(clean_field));
    }
    if !parent_json_name.is_empty() {
        return format!("{parent_json_name}Value");
    }
    if !clean_field.is_empty() {
        return format!("{}Value", to_pascal_case(clean_field));
    }
    "Value".to_owned()
}

/// Collects all inline hoisted `JsonDef` schema names declared inside a top-level `json` node.
pub fn collect_inline_json_names(json_node: NodeRef<'_>) -> HashSet<String> {
    let mut out = HashSet::new();
    let parent_name = json_node
        .arg_value(0)
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let is_top_dict = json_node.ty().unwrap_or("") == "dict"
        || json_node
            .prop("type")
            .and_then(|p| p.value.as_str())
            .unwrap_or("")
            == "dict";

    if is_top_dict {
        for child in json_node.children() {
            if child.name().trim_end_matches('?') == "@value" && child.has_children() {
                let (explicit_model, _) = extract_value_model_and_array(child);
                let name = synthesize_value_schema_name(&explicit_model, "", parent_name, "");
                out.insert(name.clone());
                collect_inline_json_names_from_children(child, &name, &mut out);
            }
        }
    } else {
        collect_inline_json_names_from_children(json_node, parent_name, &mut out);
    }

    out
}

fn collect_inline_json_names_from_children(
    parent_node: NodeRef<'_>,
    parent_schema_name: &str,
    out: &mut HashSet<String>,
) {
    for field_node in parent_node.children() {
        if !field_node.has_children() {
            continue;
        }
        let field_name = field_node.name();
        if is_dict_json_field(field_node) {
            let mut explicit_dict_name = String::new();
            for arg in field_node.args() {
                if let Some(s) = arg.value.as_str() {
                    if s.starts_with('@') || s == "(dict)" || s == "dict" {
                        continue;
                    }
                    let clean = s.trim_end_matches('?');
                    if !clean.is_empty() {
                        explicit_dict_name = clean.to_owned();
                        break;
                    }
                }
            }
            for child in field_node.children() {
                if child.name().trim_end_matches('?') == "@value" && child.has_children() {
                    let (explicit_model, _) = extract_value_model_and_array(child);
                    let child_schema_name = synthesize_value_schema_name(
                        &explicit_model,
                        &explicit_dict_name,
                        parent_schema_name,
                        field_name,
                    );
                    out.insert(child_schema_name.clone());
                    collect_inline_json_names_from_children(child, &child_schema_name, out);
                }
            }
        } else {
            let mut explicit_type = String::new();
            for arg in field_node.args() {
                if let Some(s) = arg.value.as_str() {
                    if !s.starts_with('@') {
                        explicit_type = s
                            .trim_start_matches("(array)")
                            .trim_end_matches('?')
                            .to_owned();
                        break;
                    }
                }
            }
            let child_schema_name = if !explicit_type.is_empty() {
                explicit_type
            } else {
                let clean_field = field_name.trim_end_matches('?');
                format!("{parent_schema_name}{}", to_pascal_case(clean_field))
            };
            out.insert(child_schema_name.clone());
            collect_inline_json_names_from_children(field_node, &child_schema_name, out);
        }
    }
}

/// Validates all top-level `json` declarations in `unit`.
pub fn lint_json_defs(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    diags: &mut Vec<Diagnostic>,
) {
    let Ok(nodes) = unit.document.select("json:root") else {
        return;
    };

    let mut seen_json_names_all: HashSet<String> = HashSet::new();
    let mut declared_json_names: HashSet<String> = HashSet::new();

    for node in &nodes {
        if let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) {
            if !name.is_empty() {
                seen_json_names_all.insert(name.to_owned());
            }
        }
    }

    for node in nodes {
        lint_single_json(
            unit,
            program,
            env,
            node,
            &mut declared_json_names,
            &mut seen_json_names_all,
            diags,
        );
    }
}

fn lint_single_json(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    node: NodeRef<'_>,
    declared_json_names: &mut HashSet<String>,
    seen_json_names_all: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    if let Some(ty) = node.ty() {
        if !matches!(ty, "array" | "dict") {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E400,
                node.type_span().unwrap_or_else(|| node.span()),
                format!(
                    "unsupported json annotation '({ty})' — only '(array)' and '(dict)' are recognized"
                ),
                "use 'json Name { ... }', '(array)json Name { ... }', or '(dict)json Name { ... }'",
            ));
        }
    }

    if let Some(type_prop) = node.prop("type") {
        let val = type_prop.value.as_str().unwrap_or("");
        if !matches!(val, "array" | "dict") {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E400,
                type_prop.span,
                format!("unknown json type '{val}'"),
                "valid json types: array, dict",
            ));
        }
    }

    for prop in node.props() {
        if !matches!(prop.key.as_str(), "path" | "type" | "doc") {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                prop.span,
                format!("unknown property '{}' on 'json'", prop.key),
                "allowed properties on 'json': path, type, doc",
            ));
        }
    }

    let name = node.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
    if node.args().is_empty() || name.is_empty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'json' requires a name",
            "example: json MySchema { ... }",
        ));
        return;
    }
    if node.args().len() > 1 {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'json' accepts only 1 name argument",
            "example: json MySchema { ... }",
        ));
    }

    if !declared_json_names.insert(name.to_owned()) {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            format!("duplicate json definition '{name}'"),
            format!("rename or remove one of the 'json {name}' definitions"),
        ));
    }

    if let Some(path_prop) = node.prop("path") {
        let Some(path_val) = path_prop.value.as_str() else {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                path_prop.span,
                "'path' property must be a non-empty string",
                "example: json MySchema path=\"response.data\" { ... }",
            ));
            return;
        };
        if path_val.is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E040,
                path_prop.span,
                "'path' property must be a non-empty string (malformed dot-path '')",
                "example: json MySchema path=\"response.data\" { ... }",
            ));
        } else {
            if path_val != path_val.trim() {
                diags.push(make_warning(
                    &unit.path,
                    DiagnosticCode::W040,
                    path_prop.span,
                    format!("leading or trailing whitespace in 'path' '{path_val}'"),
                    format!("remove whitespace: path=\"{}\"", path_val.trim()),
                ));
            }
            if is_malformed_gjson_path(path_val.trim()) {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E040,
                    path_prop.span,
                    format!("malformed dot-path '{path_val}'"),
                    "example: json MySchema path=\"response.data\" { ... }",
                ));
            }
        }
    }

    let is_dict = node.ty().unwrap_or("") == "dict"
        || node
            .prop("type")
            .and_then(|p| p.value.as_str())
            .unwrap_or("")
            == "dict";

    if is_dict {
        lint_dict_json_block(
            unit,
            program,
            env,
            node,
            name,
            "",
            name,
            seen_json_names_all,
            diags,
        );
        return;
    }

    if !node.has_children() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            format!("json '{name}' must contain at least one field"),
            "example: json MySchema { id int }",
        ));
        return;
    }

    let mut seen_fields = HashSet::new();
    let mut seen_source_keys = HashSet::new();
    let mut field_names: HashSet<String> = node
        .children()
        .into_iter()
        .map(|c| c.name().trim_end_matches('?').to_owned())
        .collect();
    let mut visiting_defines = HashSet::new();

    lint_json_children(
        unit,
        program,
        env,
        &unit.source,
        name,
        node.children(),
        &mut seen_fields,
        &mut seen_source_keys,
        &mut field_names,
        seen_json_names_all,
        &mut visiting_defines,
        diags,
    );
}

#[allow(clippy::too_many_arguments)]
fn lint_dict_json_block(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    node: NodeRef<'_>,
    parent_json_name: &str,
    field_name: &str,
    explicit_dict_name: &str,
    seen_json_names: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut seen_directives = HashSet::new();
    let mut has_value = false;

    for child in node.children() {
        let raw_name = child.name();
        let base_name = raw_name.trim_end_matches('?');
        if !matches!(base_name, "@key" | "@value") {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                child.span(),
                format!(
                    "unexpected child '{raw_name}' in dict schema block; only '@key' and '@value' directives are permitted"
                ),
                "use '@value <Type>' and optional '@key <ScalarType>'",
            ));
            continue;
        }

        if !seen_directives.insert(base_name.to_owned()) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                child.span(),
                format!("duplicate '{base_name}' directive in dict schema block"),
                format!("remove duplicate '{base_name}'"),
            ));
        }

        if base_name == "@key" {
            if child.args().is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    child.span(),
                    "'@key' requires a scalar type ('str', 'int', 'float', 'bool')",
                    "example: @key int",
                ));
            } else {
                let raw_key_type = child
                    .arg_value(0)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_end_matches('?');
                let is_arr = child.ty() == Some("array")
                    || child.args().iter().any(|a| {
                        a.ty() == Some("array")
                            || a.value.as_str().unwrap_or("").starts_with("(array)")
                    });
                if is_arr || !VALID_KEY_TYPES.contains(&raw_key_type) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E001,
                        child.span(),
                        format!(
                            "unsupported key type '{raw_key_type}' in '@key'; only scalar types (bool, float, int, str) are supported"
                        ),
                        "valid key types: bool, float, int, str",
                    ));
                }
            }
        } else if base_name == "@value" {
            has_value = true;
            let has_children = child.has_children();
            let is_empty_block = child.has_children_block() && !has_children;
            let has_skip = child.ty() == Some("@skip")
                || child
                    .args()
                    .iter()
                    .any(|a| a.value.as_str() == Some("@skip"));
            let (explicit_model_name, is_arr) = extract_value_model_and_array(child);

            if is_empty_block {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    child.span(),
                    "empty inline json block in '@value'",
                    "add fields to inline block or remove empty braces",
                ));
            }

            if has_skip && (has_children || is_empty_block) {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E002,
                    child.span(),
                    "cannot use '@skip' on inline json block in '@value'",
                    "remove '@skip' from '@value' declaration",
                ));
            }

            let child_schema_name = synthesize_value_schema_name(
                &explicit_model_name,
                explicit_dict_name,
                parent_json_name,
                field_name,
            );

            if (has_children || is_empty_block) && is_arr && explicit_model_name.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    child.span(),
                    "inline array block in '@value' requires an item model name",
                    "example: (array)@value ItemModel { ... } or @value (array)ItemModel { ... }",
                ));
            } else if (has_children || is_empty_block)
                && !child_schema_name.is_empty()
                && !seen_json_names.insert(child_schema_name.clone())
            {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    child.span(),
                    format!("duplicate json definition '{child_schema_name}'"),
                    format!("rename inline schema '{child_schema_name}' to avoid collision"),
                ));
            }

            if !has_children && !is_empty_block && explicit_model_name.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    child.span(),
                    "'@value' requires a type specification",
                    "example: @value (array)str  or  @value Item",
                ));
            }

            if has_children {
                let mut sub_seen_fields = HashSet::new();
                let mut sub_seen_source_keys = HashSet::new();
                let mut sub_field_names: HashSet<String> = child
                    .children()
                    .into_iter()
                    .map(|c| c.name().trim_end_matches('?').to_owned())
                    .collect();
                let mut sub_visiting = HashSet::new();
                lint_json_children(
                    unit,
                    program,
                    env,
                    &unit.source,
                    &child_schema_name,
                    child.children(),
                    &mut sub_seen_fields,
                    &mut sub_seen_source_keys,
                    &mut sub_field_names,
                    seen_json_names,
                    &mut sub_visiting,
                    diags,
                );
            }
        }
    }

    if !has_value {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "dict schema block is missing mandatory '@value' directive",
            "add '@value <Type>' specifying the dictionary value type",
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn lint_json_children<'a>(
    unit: &SourceUnit,
    program: &'a ResolvedProgram,
    env: &UnitEnv,
    source_text: &str,
    parent_json_name: &str,
    children: Vec<NodeRef<'a>>,
    seen_fields: &mut HashSet<String>,
    seen_source_keys: &mut HashSet<String>,
    field_names: &mut HashSet<String>,
    seen_json_names: &mut HashSet<String>,
    visiting_defines: &mut HashSet<(String, NodeId)>,
    diags: &mut Vec<Diagnostic>,
) {
    for field_node in children {
        let field_name = field_node.name();
        let clean_field_name = field_name.trim_end_matches('?');

        // Block define expansion
        if field_node.args().is_empty() && !field_node.has_children_block() {
            if let Some(&DefineEntry::Block {
                ref unit_path,
                node_id,
            }) = env.defines.get(field_name)
            {
                if visiting_defines.insert((unit_path.clone(), node_id)) {
                    if let Some(def_unit) = program.units.get(unit_path) {
                        let def_node = def_unit.document.node(node_id);
                        for c in def_node.children() {
                            field_names.insert(c.name().trim_end_matches('?').to_owned());
                        }
                        lint_json_children(
                            unit,
                            program,
                            env,
                            &def_unit.source,
                            parent_json_name,
                            def_node.children(),
                            seen_fields,
                            seen_source_keys,
                            field_names,
                            seen_json_names,
                            visiting_defines,
                            diags,
                        );
                    }
                    visiting_defines.remove(&(unit_path.clone(), node_id));
                }
                continue;
            }
        }

        // Validate field name identifier (`E403`)
        if !is_portable_ident(clean_field_name) {
            let type_repr = json_field_type_repr(field_node);
            let hint = if let Some(without_colon) = clean_field_name.strip_suffix(':') {
                let sanitized = sanitize_json_field_name(without_colon);
                format!(
                    "in KDL field name and type are separated by space ('{sanitized} {type_repr}'); if the JSON key ends with ':', use '{sanitized} {type_repr} from=\"{clean_field_name}\"'"
                )
            } else {
                let sanitized = sanitize_json_field_name(clean_field_name);
                format!(
                    "use a portable identifier with 'from', e.g. '{sanitized} {type_repr} from=\"{clean_field_name}\"'"
                )
            };
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E403,
                field_node.span(),
                format!(
                    "invalid json field name '{field_name}'; field names must be portable identifiers matching ^[A-Za-z_][A-Za-z0-9_]*$"
                ),
                hint,
            ));
        }

        // Check `path` property (`W041`)
        let path_prop = field_node.prop("path");
        if let Some(pp) = path_prop {
            let val_repr = pp
                .value
                .as_str()
                .map_or_else(|| pp.value.to_kdl(), |s| format!("'{s}'"));
            diags.push(make_warning(
                &unit.path,
                DiagnosticCode::W041,
                pp.span,
                "use 'from' instead of 'path' to specify JSON key alias on fields",
                format!("example: {field_name} ... from={val_repr}"),
            ));
        }

        for prop in field_node.props() {
            if !matches!(prop.key.as_str(), "from" | "path" | "doc" | "type") {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    prop.span,
                    format!(
                        "unknown property '{}' on json field '{field_name}'",
                        prop.key
                    ),
                    "allowed properties on json field: from, doc",
                ));
            }
        }

        let is_dict_field = is_dict_json_field(field_node);
        let mut has_type = false;
        let mut has_skip = false;
        let mut value_args: Vec<&str> = Vec::new();

        for (idx, arg) in field_node.args().iter().enumerate() {
            let Some(arg_str) = arg.value.as_str() else {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    arg.span,
                    format!("json field '{field_name}' argument must be a string or type name"),
                    "example: field_name str",
                ));
                continue;
            };
            let quoted = is_arg_quoted(source_text, field_node, idx);
            if arg_str.starts_with('@') && !quoted {
                if !VALID_JSON_MODIFIERS.contains(&arg_str) {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E002,
                        arg.span,
                        format!("unknown json field modifier '{arg_str}'"),
                        format!("valid modifiers: {}", VALID_JSON_MODIFIERS.join(", ")),
                    ));
                }
                if arg_str == "@skip" {
                    has_skip = true;
                }
            } else {
                if let Some(ann) = arg.ty() {
                    if !matches!(ann, "array" | "dict") {
                        diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E400,
                            arg.type_span.unwrap_or(arg.span),
                            format!("unsupported json field type annotation '({ann})'"),
                            "only '(array)' and '(dict)' are supported",
                        ));
                    }
                }
                has_type = true;
                value_args.push(arg_str);
            }
        }

        let has_children = field_node.has_children();
        let is_empty_block = field_node.has_children_block() && !has_children;

        if is_empty_block {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                field_node.span(),
                format!("empty inline json block '{field_name}'"),
                "add field definitions to the inline block or remove it",
            ));
        }

        if has_skip && (has_children || is_empty_block) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E002,
                field_node.span(),
                format!("cannot use '@skip' on inline json block '{field_name}'"),
                "remove '@skip' from inline block declaration",
            ));
        }

        if is_dict_field {
            has_type = true;
            let mut explicit_dict_name = String::new();
            for &arg_str in &value_args {
                if arg_str != "(dict)" && arg_str != "dict" {
                    let clean = arg_str.trim_end_matches('?');
                    if !clean.is_empty() {
                        explicit_dict_name = clean.to_owned();
                        break;
                    }
                }
            }
            lint_dict_json_block(
                unit,
                program,
                env,
                field_node,
                parent_json_name,
                clean_field_name,
                &explicit_dict_name,
                seen_json_names,
                diags,
            );
        } else if has_children {
            has_type = true;
            let is_array = field_node.ty() == Some("array")
                || field_node.args().iter().any(|a| {
                    a.ty() == Some("array") || a.value.as_str().unwrap_or("").starts_with("(array)")
                });
            let explicit_name = value_args
                .first()
                .map(|s| s.trim_start_matches("(array)").trim_end_matches('?'))
                .unwrap_or("");
            let child_schema_name = if !explicit_name.is_empty() {
                explicit_name.to_owned()
            } else {
                format!("{parent_json_name}{}", to_pascal_case(clean_field_name))
            };

            if is_array && value_args.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    field_node.span(),
                    format!("inline array block '{field_name}' requires an item model name"),
                    format!("example: {field_name} (array)ItemModelName {{ ... }}"),
                ));
            } else if !child_schema_name.is_empty()
                && !seen_json_names.insert(child_schema_name.clone())
            {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    field_node.span(),
                    format!("duplicate json definition '{child_schema_name}'"),
                    format!("rename inline schema '{child_schema_name}' to avoid collision"),
                ));
            }

            let mut sub_seen_fields = HashSet::new();
            let mut sub_seen_source_keys = HashSet::new();
            let mut sub_field_names: HashSet<String> = field_node
                .children()
                .into_iter()
                .map(|c| c.name().trim_end_matches('?').to_owned())
                .collect();
            lint_json_children(
                unit,
                program,
                env,
                source_text,
                &child_schema_name,
                field_node.children(),
                &mut sub_seen_fields,
                &mut sub_seen_source_keys,
                &mut sub_field_names,
                seen_json_names,
                visiting_defines,
                diags,
            );
        }

        if !has_type && !has_skip {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                field_node.span(),
                format!("json field '{field_name}' requires a type"),
                "example: field_name str  or  field_name @skip",
            ));
        }

        let is_dup_field = !seen_fields.insert(clean_field_name.to_owned());
        if is_dup_field {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E041,
                field_node.span(),
                format!("duplicate json field '{field_name}'"),
                format!("remove or rename the duplicate '{field_name}' field"),
            ));
        }
        field_names.insert(clean_field_name.to_owned());

        let positional_alias = if value_args.len() > 1 && !has_children {
            value_args[1]
        } else {
            ""
        };
        if !positional_alias.is_empty() {
            let type_repr = value_args.first().copied().unwrap_or("str");
            diags.push(make_warning(
                &unit.path,
                DiagnosticCode::W011,
                field_node.span(),
                "positional JSON key alias is deprecated, use from=\"...\" instead",
                format!("example: {field_name} {type_repr} from=\"{positional_alias}\""),
            ));
        }

        let mut from_val: Option<String> = None;
        if let Some(from_prop) = field_node.prop("from") {
            match from_prop.value.as_str() {
                None => {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E001,
                        from_prop.span,
                        "'from' property must be a non-empty string",
                        format!("example: {field_name} str from=\"source_key\""),
                    ));
                }
                Some("") => {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E040,
                        from_prop.span,
                        "'from' property must be a non-empty string (malformed dot-path '')",
                        format!("example: {field_name} str from=\"source_key\""),
                    ));
                }
                Some(raw_from) => {
                    let resolved_from = env.resolve_str_arg(raw_from);
                    if resolved_from != resolved_from.trim() {
                        diags.push(make_warning(
                            &unit.path,
                            DiagnosticCode::W040,
                            from_prop.span,
                            format!(
                                "leading or trailing whitespace in 'from' path '{resolved_from}'"
                            ),
                            format!("remove whitespace: from=\"{}\"", resolved_from.trim()),
                        ));
                    }
                    if is_malformed_gjson_path(resolved_from.trim()) {
                        diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E040,
                            from_prop.span,
                            format!("malformed dot-path '{resolved_from}'"),
                            format!("example: {field_name} str from=\"profile.avatar.url\""),
                        ));
                    }
                    from_val = Some(resolved_from);
                }
            }

            if !positional_alias.is_empty() {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    field_node.span(),
                    format!(
                        "cannot specify both positional alias and 'from' property on json field '{field_name}'"
                    ),
                    "remove positional alias and keep 'from=\"...\"' property",
                ));
            }
        } else if let Some(pp) = path_prop {
            if let Some(s) = pp.value.as_str() {
                if !s.is_empty() {
                    if s != s.trim() {
                        diags.push(make_warning(
                            &unit.path,
                            DiagnosticCode::W040,
                            pp.span,
                            format!("leading or trailing whitespace in 'path' '{s}'"),
                            format!("remove whitespace: from=\"{}\"", s.trim()),
                        ));
                    }
                    if is_malformed_gjson_path(s.trim()) {
                        diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E040,
                            pp.span,
                            format!("malformed dot-path '{s}'"),
                            format!("example: {field_name} str from=\"profile.avatar.url\""),
                        ));
                    }
                    from_val = Some(s.to_owned());
                } else {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E040,
                        pp.span,
                        "'path' property must be a non-empty string (malformed dot-path '')",
                        format!("example: {field_name} str from=\"source_key\""),
                    ));
                }
            }
        }

        let source_key = if let Some(ref fv) = from_val {
            fv.clone()
        } else if !positional_alias.is_empty() {
            positional_alias.to_owned()
        } else {
            clean_field_name.to_owned()
        };

        if !seen_source_keys.insert(source_key.clone()) && !is_dup_field {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E041,
                field_node.span(),
                format!("duplicate json source key '{source_key}'"),
                format!("rename or remove one of the fields mapping to '{source_key}'"),
            ));
        }

        if source_key != clean_field_name && field_names.contains(&source_key) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E041,
                field_node.span(),
                format!("json alias '{source_key}' conflicts with field name"),
                "",
            ));
        }
    }
}

pub(crate) fn is_arg_quoted(source_text: &str, node: NodeRef<'_>, arg_index: usize) -> bool {
    let Some(arg) = node.arg(arg_index) else {
        return false;
    };
    let start = arg.value_span.offset;
    let end = start + arg.value_span.len;
    let Some(raw) = source_text.get(start..end) else {
        return false;
    };
    raw.starts_with('"') || raw.starts_with('\'') || raw.starts_with('#')
}

fn sanitize_json_field_name(raw: &str) -> String {
    let trimmed = raw.trim_start_matches('@');
    let mut s = String::with_capacity(trimmed.len());
    let mut prev_underscore = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            s.push(ch);
            prev_underscore = ch == '_';
        } else if !prev_underscore {
            s.push('_');
            prev_underscore = true;
        }
    }
    let clean = s.trim_matches('_').to_owned();
    if clean.is_empty() {
        return "field".to_owned();
    }
    if clean.as_bytes()[0].is_ascii_digit() {
        format!("k_{clean}")
    } else {
        clean
    }
}

fn json_field_type_repr(field_node: NodeRef<'_>) -> String {
    for arg in field_node.args() {
        if let Some(val) = arg.value.as_str() {
            if val.starts_with('@') {
                continue;
            }
            if let Some(ann) = arg.ty() {
                return format!("({ann}){val}");
            }
            return val.to_owned();
        }
    }
    "str".to_owned()
}

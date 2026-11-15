//! Intra-file top-down declaration order validation (`E302`) for `struct`, `fn`, and `json`.

use std::collections::{HashMap, HashSet};

use crate::kdl::{NodeId, NodeRef};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::imports::{ResolvedProgram, SourceUnit};
use crate::linter::{DefineEntry, UnitEnv, make_error};
use crate::request::is_error_directive;

const VALID_JSON_PRIMITIVES: &[&str] = &["str", "int", "float", "bool", "null", "nil"];

/// Enforces intra-file top-down declaration order (`E302`) for local `struct` and `json` references.
///
/// Explicitly imported symbols resolve topologically ahead of local declarations and are never flagged with `E302`.
pub fn lint_topdown_order(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    diags: &mut Vec<Diagnostic>,
) {
    let mut local_struct_order: HashMap<String, usize> = HashMap::new();
    let mut local_json_order: HashMap<String, usize> = HashMap::new();

    for (root_idx, node) in unit.document.roots().into_iter().enumerate() {
        match node.name() {
            "struct" => {
                if let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) {
                    if !name.is_empty() {
                        local_struct_order
                            .entry(name.to_owned())
                            .or_insert(root_idx);
                    }
                }
            }
            "json" => {
                if let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) {
                    if !name.is_empty() {
                        local_json_order.entry(name.to_owned()).or_insert(root_idx);
                    }
                }
            }
            _ => {}
        }
    }

    for (caller_idx, caller) in unit.document.roots().into_iter().enumerate() {
        match caller.name() {
            "struct" | "fn" => {
                let caller_kind = caller.name();
                let caller_name = caller
                    .arg_value(0)
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unnamed>");

                // 1. Check `nested <StructName>` and `jsonify <SchemaName>` (including inside expanded block defines)
                let mut visited_defines = HashSet::new();
                check_pipeline_subtree_topdown(
                    unit,
                    program,
                    env,
                    caller,
                    caller_idx,
                    caller_kind,
                    caller_name,
                    &local_struct_order,
                    &local_json_order,
                    &mut visited_defines,
                    diags,
                );

                // 2. Check `@request response="<SchemaName>"` and `@error ... <SchemaName>` on `struct`
                if caller_kind == "struct" {
                    for child in caller.children() {
                        if child.name() == "@request" {
                            if let Some(resp_prop) = child.prop("response") {
                                if let Some(raw_schema) = resp_prop.value.as_str() {
                                    let schema_name = env.resolve_str_arg(raw_schema);
                                    if let Some(&target_idx) = local_json_order.get(&schema_name) {
                                        if target_idx > caller_idx {
                                            diags.push(make_error(
                                                &unit.path,
                                                DiagnosticCode::E302,
                                                resp_prop.value_span,
                                                format!(
                                                    "'@request response={schema_name}': json '{schema_name}' must be declared BEFORE use (top-down order)"
                                                ),
                                                format!(
                                                    "move 'json {schema_name} {{ ... }}' above 'struct {caller_name}'"
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                        } else if is_error_directive(child.name()) {
                            if let Some(raw_schema) = child.arg_value(1).and_then(|v| v.as_str()) {
                                let schema_name = env.resolve_str_arg(raw_schema);
                                if let Some(&target_idx) = local_json_order.get(&schema_name) {
                                    if target_idx > caller_idx {
                                        let span =
                                            child.arg(1).map_or_else(|| child.span(), |a| a.span);
                                        diags.push(make_error(
                                            &unit.path,
                                            DiagnosticCode::E302,
                                            span,
                                            format!(
                                                "'{} {schema_name}': json '{schema_name}' must be declared BEFORE use (top-down order)",
                                                child.name()
                                            ),
                                            format!(
                                                "move 'json {schema_name} {{ ... }}' above 'struct {caller_name}'"
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "json" => {
                let Some(caller_name) = caller.arg_value(0).and_then(|v| v.as_str()) else {
                    continue;
                };
                if caller_name.is_empty() {
                    continue;
                }
                let mut visiting = HashSet::new();
                check_json_fields_topdown(
                    unit,
                    program,
                    env,
                    caller,
                    caller_idx,
                    caller_name,
                    &local_json_order,
                    &mut visiting,
                    diags,
                );
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_pipeline_subtree_topdown(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    node: NodeRef<'_>,
    caller_idx: usize,
    caller_kind: &str,
    caller_name: &str,
    local_struct_order: &HashMap<String, usize>,
    local_json_order: &HashMap<String, usize>,
    visited_defines: &mut HashSet<(String, NodeId)>,
    diags: &mut Vec<Diagnostic>,
) {
    for child in node.children() {
        let name = child.name();
        if name == "nested" {
            if let Some(target) = child.arg_value(0).and_then(|v| v.as_str()) {
                if let Some(&target_idx) = local_struct_order.get(target) {
                    if target_idx > caller_idx {
                        diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E302,
                            child.span(),
                            format!(
                                "'nested {target}': struct '{target}' must be declared BEFORE use (top-down order)"
                            ),
                            format!(
                                "move 'struct {target} {{ ... }}' above '{caller_kind} {caller_name}'"
                            ),
                        ));
                    }
                }
            }
        } else if name == "jsonify" {
            if let Some(target) = child.arg_value(0).and_then(|v| v.as_str()) {
                if let Some(&target_idx) = local_json_order.get(target) {
                    if target_idx > caller_idx {
                        diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E302,
                            child.span(),
                            format!(
                                "'jsonify {target}': json '{target}' must be declared BEFORE use (top-down order)"
                            ),
                            format!(
                                "move 'json {target} {{ ... }}' above '{caller_kind} {caller_name}'"
                            ),
                        ));
                    }
                }
            }
        } else if let Some(&DefineEntry::Block {
            ref unit_path,
            node_id,
        }) = env.defines.get(name)
        {
            if unit_path == &unit.path && visited_defines.insert((unit_path.clone(), node_id)) {
                if let Some(def_unit) = program.units.get(unit_path) {
                    let def_node = def_unit.document.node(node_id);
                    check_pipeline_subtree_topdown(
                        unit,
                        program,
                        env,
                        def_node,
                        caller_idx,
                        caller_kind,
                        caller_name,
                        local_struct_order,
                        local_json_order,
                        visited_defines,
                        diags,
                    );
                }
                visited_defines.remove(&(unit_path.clone(), node_id));
            }
        }

        if child.has_children() {
            check_pipeline_subtree_topdown(
                unit,
                program,
                env,
                child,
                caller_idx,
                caller_kind,
                caller_name,
                local_struct_order,
                local_json_order,
                visited_defines,
                diags,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_json_fields_topdown(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
    parent_node: NodeRef<'_>,
    caller_idx: usize,
    caller_name: &str,
    local_json_order: &HashMap<String, usize>,
    visiting_defines: &mut HashSet<(String, NodeId)>,
    diags: &mut Vec<Diagnostic>,
) {
    for field_node in parent_node.children() {
        let field_name = field_node.name();
        if field_node.args().is_empty() && !field_node.has_children_block() {
            if let Some(&DefineEntry::Block {
                ref unit_path,
                node_id,
            }) = env.defines.get(field_name)
            {
                if visiting_defines.insert((unit_path.clone(), node_id)) {
                    if let Some(def_unit) = program.units.get(unit_path) {
                        let def_node = def_unit.document.node(node_id);
                        check_json_fields_topdown(
                            unit,
                            program,
                            env,
                            def_node,
                            caller_idx,
                            caller_name,
                            local_json_order,
                            visiting_defines,
                            diags,
                        );
                    }
                    visiting_defines.remove(&(unit_path.clone(), node_id));
                }
                continue;
            }
        }

        if field_node.has_children() {
            check_json_fields_topdown(
                unit,
                program,
                env,
                field_node,
                caller_idx,
                caller_name,
                local_json_order,
                visiting_defines,
                diags,
            );
            continue;
        }

        if field_name.trim_end_matches('?') == "@value" {
            let (explicit_model, _) = super::json::extract_value_model_and_array(field_node);
            if !explicit_model.is_empty()
                && !VALID_JSON_PRIMITIVES.contains(&explicit_model.as_str())
            {
                if let Some(&target_idx) = local_json_order.get(&explicit_model) {
                    if target_idx > caller_idx {
                        diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E302,
                            field_node.span(),
                            format!(
                                "json field '{field_name}': json type '{explicit_model}' must be declared BEFORE use (top-down order)"
                            ),
                            format!(
                                "move 'json {explicit_model} {{ ... }}' above 'json {caller_name}'"
                            ),
                        ));
                    }
                }
            }
            continue;
        }

        if field_name.trim_end_matches('?') == "@key" {
            continue;
        }

        // Regular scalar/ref field
        let mut type_str = String::new();
        for arg in field_node.args() {
            if let Some(s) = arg.value.as_str() {
                if s.starts_with('@') {
                    continue;
                }
                type_str = s
                    .trim_start_matches("(array)")
                    .trim_end_matches('?')
                    .to_owned();
                break;
            }
        }
        if !type_str.is_empty() && !VALID_JSON_PRIMITIVES.contains(&type_str.as_str()) {
            if let Some(&target_idx) = local_json_order.get(&type_str) {
                if target_idx > caller_idx {
                    diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E302,
                        field_node.span(),
                        format!(
                            "json field '{field_name}': json type '{type_str}' must be declared BEFORE use (top-down order)"
                        ),
                        format!("move 'json {type_str} {{ ... }}' above 'json {caller_name}'"),
                    ));
                }
            }
        }
    }
}

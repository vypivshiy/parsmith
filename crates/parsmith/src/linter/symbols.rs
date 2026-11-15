//! Pass 4 cross-reference validation (`E300`, `E301`), recursive `struct`/`json` cycle detection,
//! and 4-ecosystem (`python`, `javascript`/`typescript`, `go`, `rust`) identifier legality (`E403`)
//! and case-folded collision checks (`E402`).
//!
//! Matches `ssc_codegen/symbols.py`, `ssc_codegen/naming.py`, and `ssc_codegen/core/linter.py`.

use std::collections::{HashMap, HashSet};

use crate::kdl::{NodeId, NodeRef, Span};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::imports::{ResolvedProgram, SourceUnit, SymbolKind};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::json::{
    extract_value_model_and_array, is_dict_json_field, synthesize_value_schema_name,
};
pub use crate::linter::naming::*;
use crate::linter::{DefineEntry, UnitEnv, is_portable_ident, make_error};
use crate::request::{
    PlaceholderSpec, REQUEST_LINE_CONTINUATION_HINT, find_placeholder_tokens, is_error_directive,
    starts_with_http_signature,
};

const VALID_JSON_PRIMITIVES: &[&str] = &["str", "int", "float", "bool", "null", "nil"];

/// Namespace scope of a [`SymbolRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolScope {
    /// Module-level namespace (`Struct`, `Type`, `Json`, `Function`).
    Module,
    /// Struct-level field/init namespace (`Field`, `Init`).
    Struct,
    /// Struct-level public method namespace (`@request` and `@check`).
    Method,
    /// Per-request parameter placeholder namespace (`Placeholder`).
    Placeholder,
}

/// Normalized target symbol record for 4-ecosystem validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRecord {
    /// Symbol category.
    pub kind: TargetSymbolKind,
    /// Raw KDL declaration name.
    pub raw_name: String,
    /// Normalized target language (`"python"`, `"javascript"`, `"go"`, `"rust"`).
    pub target: String,
    /// Generated target-language identifier.
    pub symbol: String,
    /// Namespace scope.
    pub scope: SymbolScope,
    /// Scope/file key used to partition local namespaces.
    pub source: String,
    /// Source file path for diagnostic reporting.
    pub file_path: String,
    /// Source span of the declaring KDL node.
    pub span: Span,
}

impl SymbolRecord {
    /// Creates a new `SymbolRecord`.
    pub fn new(
        kind: TargetSymbolKind,
        raw_name: impl Into<String>,
        target: impl Into<String>,
        symbol: impl Into<String>,
        scope: SymbolScope,
        source: impl Into<String>,
        span: Span,
    ) -> Self {
        let src = source.into();
        let file_path = src.split('#').next().unwrap_or(&src).to_owned();
        Self {
            kind,
            raw_name: raw_name.into(),
            target: target.into(),
            symbol: symbol.into(),
            scope,
            source: src,
            file_path,
            span,
        }
    }
}

/// A symbol legality (`E403`) or collision (`E402`) finding produced by [`target_symbol_plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolFinding {
    /// Offending symbol record.
    pub record: SymbolRecord,
    /// Diagnostic code (`E402` or `E403`).
    pub code: DiagnosticCode,
    /// Diagnostic message.
    pub message: String,
    /// Actionable hint.
    pub hint: String,
    /// Previous colliding record when `code == DiagnosticCode::E402`.
    pub previous: Option<SymbolRecord>,
}

/// Validates normalized [`SymbolRecord`]s and reports `E403` / `E402` findings in record order.
///
/// Matches `ssc_codegen/symbols.py:target_symbol_plan`.
pub fn target_symbol_plan<I, S>(
    records: &[SymbolRecord],
    targets: Option<I>,
) -> Result<Vec<SymbolFinding>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let requested = normalize_targets(targets)?;
    let mut seen: HashMap<(String, SymbolScope, String), HashMap<String, SymbolRecord>> =
        HashMap::new();
    let mut findings = Vec::new();

    for record in records {
        if !requested.contains(&record.target.as_str()) {
            continue;
        }
        if !is_valid_symbol(&record.target, &record.symbol) {
            let hint = if record.kind == TargetSymbolKind::Field
                && starts_with_http_signature(&record.raw_name)
            {
                REQUEST_LINE_CONTINUATION_HINT.to_owned()
            } else {
                format!("rename '{}' to a portable identifier", record.raw_name)
            };
            findings.push(SymbolFinding {
                record: record.clone(),
                code: DiagnosticCode::E403,
                message: format!(
                    "{} '{}' produces invalid {} identifier '{}'",
                    record.kind, record.raw_name, record.target, record.symbol
                ),
                hint,
                previous: None,
            });
            continue;
        }

        let ns_source = if record.scope == SymbolScope::Module {
            String::new()
        } else {
            record.source.clone()
        };
        let namespace = (record.target.clone(), record.scope, ns_source);
        let bucket = seen.entry(namespace).or_default();
        let Some(previous) = bucket.get(&record.symbol).cloned() else {
            bucket.insert(record.symbol.clone(), record.clone());
            continue;
        };

        if record.scope == SymbolScope::Module
            && matches!(
                previous.kind,
                TargetSymbolKind::Struct | TargetSymbolKind::Type
            )
            && matches!(
                record.kind,
                TargetSymbolKind::Struct | TargetSymbolKind::Type
            )
            && previous.raw_name == record.raw_name
            && previous.source == record.source
        {
            continue;
        }

        findings.push(SymbolFinding {
            record: record.clone(),
            code: DiagnosticCode::E402,
            message: format!(
                "{} symbol collision: {} '{}' and {} '{}' both produce '{}'",
                record.target,
                record.kind,
                record.raw_name,
                previous.kind,
                previous.raw_name,
                record.symbol
            ),
            hint: format!("rename one declaration so {} names differ", record.target),
            previous: Some(previous),
        });
    }

    Ok(findings)
}

/// Runs Pass 4 cross-reference validation, cycle detection, and 4-ecosystem symbol checks.
pub fn lint_pass4_symbols(
    program: &ResolvedProgram,
    targets: Option<&[String]>,
    existing_diags: &[Diagnostic],
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    let requested_targets = match normalize_targets(targets) {
        Ok(t) => t,
        Err(err_msg) => {
            diags.push(
                Diagnostic::error(DiagnosticCode::E002, err_msg, Span::EMPTY)
                    .with_path(program.entry_path.clone())
                    .with_hint(
                        "valid targets: python, javascript, typescript, go, rust, elixir, php, csharp, kotlin",
                    ),
            );
            return diags;
        }
    };

    lint_cross_refs_and_cycles(program, existing_diags, &mut diags);

    let records = collect_program_symbol_records(program, &requested_targets);
    if let Ok(findings) = target_symbol_plan(&records, Some(requested_targets.as_slice())) {
        for finding in findings {
            diags.push(make_error(
                &finding.record.file_path,
                finding.code,
                finding.record.span,
                finding.message,
                finding.hint,
            ));
        }
    }

    diags
}

/// Validates cross-references (including scalar-define-resolved `@request`/`@error` schema names
/// and `@value` references) and checks for circular `json` reference chains (including inline schemas).
fn lint_cross_refs_and_cycles(
    program: &ResolvedProgram,
    existing_diags: &[Diagnostic],
    diags: &mut Vec<Diagnostic>,
) {
    // Track spans where E300 was already reported in Pass 2 so we never duplicate an E300 on the same node.
    let mut reported_e300_spans: HashSet<(String, usize, usize)> = existing_diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E300)
        .map(|d| (d.path.clone(), d.span.offset, d.span.len))
        .collect();

    let has_cycle_diag_already = existing_diags.iter().any(|d| {
        d.code == DiagnosticCode::E300
            && (d.message.contains("dependency cycle") || d.message.contains("circular reference"))
    });

    // Global JSON dependency graph across all reachable JSON definitions (including inline hoisted schemas)
    let mut json_graph: IndexMap<String, IndexMap<String, (String, Span)>> = IndexMap::new();
    let mut json_decl_locs: HashMap<String, (String, Span)> = HashMap::new();

    for unit in program.units.values() {
        let env = UnitEnv::build(program, &unit.path);

        // Collect visible JSON names in this unit's scope (including inline schemas)
        let mut visible_json_names: HashSet<String> = unit
            .scope
            .keys()
            .filter(|(k, _)| *k == SymbolKind::Json)
            .map(|(_, n)| n.clone())
            .collect();

        for node in unit.document.roots() {
            if node.name() == "json" {
                if let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) {
                    if !name.is_empty() {
                        visible_json_names.insert(name.to_owned());
                        json_decl_locs
                            .entry(name.to_owned())
                            .or_insert_with(|| (unit.path.clone(), node.span()));
                        json_graph.entry(name.to_owned()).or_default();
                    }
                }
                for inline_name in crate::linter::json::collect_inline_json_names(node) {
                    visible_json_names.insert(inline_name);
                }
            }
        }

        // 1. Check `@request response=...` and `@error ... <Schema>` after resolving scalar defines
        for node in unit.document.roots() {
            if node.name() == "struct" {
                for child in node.children() {
                    if child.name() == "@request" {
                        if let Some(resp_prop) = child.prop("response") {
                            if let Some(raw_schema) = resp_prop.value.as_str() {
                                let resolved = env.resolve_str_arg(raw_schema);
                                if !resolved.is_empty()
                                    && !visible_json_names.contains(&resolved)
                                    && !unit
                                        .poisoned_symbols
                                        .contains(&(SymbolKind::Json, resolved.clone()))
                                {
                                    let key = (
                                        unit.path.clone(),
                                        resp_prop.value_span.offset,
                                        resp_prop.value_span.len,
                                    );
                                    if reported_e300_spans.insert(key) {
                                        diags.push(make_error(
                                            &unit.path,
                                            DiagnosticCode::E300,
                                            resp_prop.value_span,
                                            format!(
                                                "@request response='{resolved}' references undefined json definition '{resolved}'"
                                            ),
                                            format!(
                                                "define 'json {resolved} {{ ... }}' or fix the response name"
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                    } else if is_error_directive(child.name()) {
                        if let Some(arg1) = child.arg(1) {
                            if let Some(raw_schema) = arg1.value.as_str() {
                                let resolved = env.resolve_str_arg(raw_schema);
                                if !resolved.is_empty()
                                    && !visible_json_names.contains(&resolved)
                                    && !unit
                                        .poisoned_symbols
                                        .contains(&(SymbolKind::Json, resolved.clone()))
                                {
                                    let key = (unit.path.clone(), arg1.span.offset, arg1.span.len);
                                    if reported_e300_spans.insert(key) {
                                        diags.push(make_error(
                                            &unit.path,
                                            DiagnosticCode::E300,
                                            arg1.span,
                                            format!(
                                                "@error schema '{resolved}' references undefined json definition '{resolved}'"
                                            ),
                                            format!(
                                                "define 'json {resolved} {{ ... }}' or fix the schema name"
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            } else if node.name() == "json" {
                let Some(parent_name) = node.arg_value(0).and_then(|v| v.as_str()) else {
                    continue;
                };
                if parent_name.is_empty() {
                    continue;
                }
                let mut visiting_defines = HashSet::new();
                collect_unit_json_refs(
                    unit,
                    program,
                    &env,
                    node.children(),
                    parent_name,
                    &visible_json_names,
                    &mut reported_e300_spans,
                    &mut json_graph,
                    &mut json_decl_locs,
                    &mut visiting_defines,
                    diags,
                );
            }
        }
    }

    // Check for cycles in `json_graph` (including cycles through inline hoisted schemas)
    if !has_cycle_diag_already {
        let mut visited = HashSet::new();
        for start_name in json_graph.keys() {
            visited.clear();
            let mut stack = Vec::new();
            if let Some(cycle_node) =
                find_json_cycle(start_name, &json_graph, &mut stack, &mut visited)
            {
                let (path, span) = json_decl_locs
                    .get(&cycle_node)
                    .or_else(|| json_decl_locs.get(start_name))
                    .cloned()
                    .unwrap_or_else(|| (program.entry_path.clone(), Span::EMPTY));
                diags.push(make_error(
                    &path,
                    DiagnosticCode::E300,
                    span,
                    format!("circular reference detected involving json definition '{cycle_node}'"),
                    "break the cycle by removing or changing one of the referenced types",
                ));
                break;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_unit_json_refs<'a>(
    unit: &SourceUnit,
    program: &'a ResolvedProgram,
    env: &UnitEnv,
    children: Vec<NodeRef<'a>>,
    parent_json_name: &str,
    visible_json_names: &HashSet<String>,
    reported_e300_spans: &mut HashSet<(String, usize, usize)>,
    json_graph: &mut IndexMap<String, IndexMap<String, (String, Span)>>,
    json_decl_locs: &mut HashMap<String, (String, Span)>,
    visiting_defines: &mut HashSet<(String, NodeId)>,
    diags: &mut Vec<Diagnostic>,
) {
    for field_node in children {
        let field_name = field_node.name();
        let clean_field = field_name.trim_end_matches('?');

        if field_node.args().is_empty() && !field_node.has_children_block() {
            if let Some(&DefineEntry::Block {
                ref unit_path,
                node_id,
            }) = env.defines.get(field_name)
            {
                let key = (unit_path.clone(), node_id);
                if visiting_defines.insert(key.clone()) {
                    if let Some(def_unit) = program.units.get(unit_path) {
                        let def_node = def_unit.document.node(node_id);
                        collect_unit_json_refs(
                            unit,
                            program,
                            env,
                            def_node.children(),
                            parent_json_name,
                            visible_json_names,
                            reported_e300_spans,
                            json_graph,
                            json_decl_locs,
                            visiting_defines,
                            diags,
                        );
                    }
                    visiting_defines.remove(&key);
                    continue;
                }
            }
        }

        if clean_field == "@key" {
            continue;
        }

        if clean_field == "@value" {
            let (explicit_model, _) = extract_value_model_and_array(field_node);
            if field_node.has_children() {
                let child_schema_name =
                    synthesize_value_schema_name(&explicit_model, "", parent_json_name, "");
                json_decl_locs
                    .entry(child_schema_name.clone())
                    .or_insert_with(|| (unit.path.clone(), field_node.span()));
                json_graph.entry(child_schema_name.clone()).or_default();
                json_graph
                    .entry(parent_json_name.to_owned())
                    .or_default()
                    .insert(
                        child_schema_name.clone(),
                        (unit.path.clone(), field_node.span()),
                    );
                collect_unit_json_refs(
                    unit,
                    program,
                    env,
                    field_node.children(),
                    &child_schema_name,
                    visible_json_names,
                    reported_e300_spans,
                    json_graph,
                    json_decl_locs,
                    visiting_defines,
                    diags,
                );
            } else if !explicit_model.is_empty()
                && !VALID_JSON_PRIMITIVES.contains(&explicit_model.as_str())
            {
                check_json_ref_exists(
                    unit,
                    field_node.span(),
                    field_name,
                    &explicit_model,
                    parent_json_name,
                    visible_json_names,
                    reported_e300_spans,
                    json_graph,
                    diags,
                );
            }
            continue;
        }

        if field_node.has_children() {
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
                    if child.name().trim_end_matches('?') == "@value" {
                        let (explicit_model, _) = extract_value_model_and_array(child);
                        if child.has_children() {
                            let child_schema_name = synthesize_value_schema_name(
                                &explicit_model,
                                &explicit_dict_name,
                                parent_json_name,
                                clean_field,
                            );
                            json_decl_locs
                                .entry(child_schema_name.clone())
                                .or_insert_with(|| (unit.path.clone(), child.span()));
                            json_graph.entry(child_schema_name.clone()).or_default();
                            json_graph
                                .entry(parent_json_name.to_owned())
                                .or_default()
                                .insert(
                                    child_schema_name.clone(),
                                    (unit.path.clone(), child.span()),
                                );
                            collect_unit_json_refs(
                                unit,
                                program,
                                env,
                                child.children(),
                                &child_schema_name,
                                visible_json_names,
                                reported_e300_spans,
                                json_graph,
                                json_decl_locs,
                                visiting_defines,
                                diags,
                            );
                        } else if !explicit_model.is_empty()
                            && !VALID_JSON_PRIMITIVES.contains(&explicit_model.as_str())
                        {
                            check_json_ref_exists(
                                unit,
                                child.span(),
                                field_name,
                                &explicit_model,
                                parent_json_name,
                                visible_json_names,
                                reported_e300_spans,
                                json_graph,
                                diags,
                            );
                        }
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
                    format!("{parent_json_name}{}", to_pascal_case(clean_field))
                };
                json_decl_locs
                    .entry(child_schema_name.clone())
                    .or_insert_with(|| (unit.path.clone(), field_node.span()));
                json_graph.entry(child_schema_name.clone()).or_default();
                json_graph
                    .entry(parent_json_name.to_owned())
                    .or_default()
                    .insert(
                        child_schema_name.clone(),
                        (unit.path.clone(), field_node.span()),
                    );
                collect_unit_json_refs(
                    unit,
                    program,
                    env,
                    field_node.children(),
                    &child_schema_name,
                    visible_json_names,
                    reported_e300_spans,
                    json_graph,
                    json_decl_locs,
                    visiting_defines,
                    diags,
                );
            }
            continue;
        }

        // Scalar or reference field
        for arg in field_node.args() {
            if let Some(s) = arg.value.as_str() {
                if s.starts_with('@') {
                    continue;
                }
                let raw_type = s.trim_start_matches("(array)").trim_end_matches('?');
                if !raw_type.is_empty() && !VALID_JSON_PRIMITIVES.contains(&raw_type) {
                    check_json_ref_exists(
                        unit,
                        arg.span,
                        field_name,
                        raw_type,
                        parent_json_name,
                        visible_json_names,
                        reported_e300_spans,
                        json_graph,
                        diags,
                    );
                }
                break;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_json_ref_exists(
    unit: &SourceUnit,
    span: Span,
    field_name: &str,
    ref_name: &str,
    parent_json_name: &str,
    visible_json_names: &HashSet<String>,
    reported_e300_spans: &mut HashSet<(String, usize, usize)>,
    json_graph: &mut IndexMap<String, IndexMap<String, (String, Span)>>,
    diags: &mut Vec<Diagnostic>,
) {
    if visible_json_names.contains(ref_name) {
        json_graph
            .entry(parent_json_name.to_owned())
            .or_default()
            .insert(ref_name.to_owned(), (unit.path.clone(), span));
    } else if !unit
        .poisoned_symbols
        .contains(&(SymbolKind::Json, ref_name.to_owned()))
    {
        let key = (unit.path.clone(), span.offset, span.len);
        if reported_e300_spans.insert(key) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E300,
                span,
                format!(
                    "json field '{field_name}' references undefined json definition '{ref_name}'"
                ),
                format!("define 'json {ref_name} {{ ... }}' or fix the type name"),
            ));
        }
    }
}

fn find_json_cycle(
    name: &str,
    graph: &IndexMap<String, IndexMap<String, (String, Span)>>,
    stack: &mut Vec<String>,
    visited: &mut HashSet<String>,
) -> Option<String> {
    if stack.iter().any(|s| s == name) {
        return Some(name.to_owned());
    }
    if !visited.insert(name.to_owned()) {
        return None;
    }
    stack.push(name.to_owned());
    if let Some(deps) = graph.get(name) {
        for dep in deps.keys() {
            if let Some(cycle) = find_json_cycle(dep, graph, stack, visited) {
                stack.pop();
                return Some(cycle);
            }
        }
    }
    stack.pop();
    None
}

/// Collects normalized [`SymbolRecord`]s across all top-level declarations in `program`.
pub fn collect_program_symbol_records(
    program: &ResolvedProgram,
    targets: &[&str],
) -> Vec<SymbolRecord> {
    let mut records = Vec::new();

    for (index, item) in program.items.iter().enumerate() {
        let Some(unit) = program.units.get(&item.symbol.path) else {
            continue;
        };
        let env = UnitEnv::build(program, &unit.path);
        let node = unit.document.node(item.node_id);
        let Some(raw) = node.arg_value(0).and_then(|v| v.as_str()) else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }

        let add_top = |records: &mut Vec<SymbolRecord>,
                       kind: TargetSymbolKind,
                       raw_name: &str,
                       scope: SymbolScope| {
            for &target in targets {
                for symbol in top_symbol_names(kind, raw_name, target) {
                    records.push(SymbolRecord::new(
                        kind,
                        raw_name,
                        target,
                        symbol,
                        scope,
                        unit.path.clone(),
                        node.span(),
                    ));
                }
            }
        };

        match node.name() {
            "struct" => {
                add_top(
                    &mut records,
                    TargetSymbolKind::Struct,
                    raw,
                    SymbolScope::Module,
                );
                let local_source = format!("{}#{index}", unit.path);
                let is_rest = node.ty() == Some("rest")
                    || node
                        .prop("type")
                        .and_then(|p| p.value.as_str())
                        .unwrap_or("")
                        == "rest";
                if !is_rest {
                    for &target in targets {
                        let type_sym = &top_symbol_names(TargetSymbolKind::Struct, raw, target)[1];
                        records.push(SymbolRecord::new(
                            TargetSymbolKind::Type,
                            raw,
                            target,
                            type_sym.clone(),
                            SymbolScope::Module,
                            unit.path.clone(),
                            node.span(),
                        ));
                    }
                }

                let mut seen_exact_fields = HashSet::new();
                let mut seen_exact_reqs = HashSet::new();
                let mut seen_exact_checks = HashSet::new();
                let mut seen_exact_inits = HashSet::new();

                for (child_idx, child) in node.children().into_iter().enumerate() {
                    let child_name = child.name();
                    if !child_name.starts_with('@') {
                        // Skip exact duplicate field names already reported by Pass 3
                        if !seen_exact_fields.insert(child_name.to_owned()) {
                            continue;
                        }
                        for &target in targets {
                            records.push(SymbolRecord::new(
                                TargetSymbolKind::Field,
                                child_name,
                                target,
                                local_symbol(TargetSymbolKind::Field, child_name, target),
                                SymbolScope::Struct,
                                local_source.clone(),
                                child.span(),
                            ));
                        }
                    } else if child_name == "@request" {
                        let req_name = child
                            .prop("name")
                            .and_then(|p| p.value.as_str())
                            .unwrap_or("fetch");
                        if seen_exact_reqs.insert(req_name.to_owned()) {
                            for &target in targets {
                                records.push(SymbolRecord::new(
                                    TargetSymbolKind::Method,
                                    req_name,
                                    target,
                                    local_symbol(TargetSymbolKind::Method, req_name, target),
                                    SymbolScope::Method,
                                    local_source.clone(),
                                    child.span(),
                                ));
                            }
                        }

                        let raw_payload = child.arg_value(0).and_then(|v| v.as_str()).unwrap_or("");
                        let payload = if crate::linter::is_define_name(raw_payload) {
                            env.scalar_strings
                                .get(raw_payload)
                                .map(String::as_str)
                                .unwrap_or(raw_payload)
                        } else {
                            raw_payload
                        };
                        let request_source = format!("{local_source}#req#{child_idx}");
                        let mut seen_placeholders = HashSet::new();
                        for token in find_placeholder_tokens(payload) {
                            if let Some(spec) = PlaceholderSpec::parse(token) {
                                if seen_placeholders.insert(spec.name.clone()) {
                                    for &target in targets {
                                        records.push(SymbolRecord::new(
                                            TargetSymbolKind::Placeholder,
                                            spec.name.clone(),
                                            target,
                                            local_symbol(
                                                TargetSymbolKind::Placeholder,
                                                &spec.name,
                                                target,
                                            ),
                                            SymbolScope::Placeholder,
                                            request_source.clone(),
                                            child.span(),
                                        ));
                                    }
                                }
                            }
                        }
                    } else if child_name == "@check" {
                        if let Some(check_name) = child.arg_value(0).and_then(|v| v.as_str()) {
                            if !check_name.is_empty()
                                && seen_exact_checks.insert(check_name.to_owned())
                            {
                                for &target in targets {
                                    records.push(SymbolRecord::new(
                                        TargetSymbolKind::Method,
                                        check_name,
                                        target,
                                        local_symbol(TargetSymbolKind::Method, check_name, target),
                                        SymbolScope::Method,
                                        local_source.clone(),
                                        child.span(),
                                    ));
                                }
                            }
                        }
                    } else if child_name == "@init" {
                        for init_field in child.children() {
                            let init_name = init_field.name();
                            if !seen_exact_inits.insert(init_name.to_owned()) {
                                continue;
                            }
                            for &target in targets {
                                records.push(SymbolRecord::new(
                                    TargetSymbolKind::Init,
                                    init_name,
                                    target,
                                    local_symbol(TargetSymbolKind::Init, init_name, target),
                                    SymbolScope::Struct,
                                    local_source.clone(),
                                    init_field.span(),
                                ));
                            }
                        }
                    }
                }
            }
            "json" => {
                add_top(
                    &mut records,
                    TargetSymbolKind::Json,
                    raw,
                    SymbolScope::Module,
                );
                let local_source = format!("{}#json#{index}", unit.path);
                let is_dict = node.ty().unwrap_or("").trim_end_matches('?') == "dict"
                    || node
                        .prop("type")
                        .and_then(|p| p.value.as_str())
                        .unwrap_or("")
                        == "dict";
                if is_dict {
                    for dict_child in node.children() {
                        if dict_child.name().trim_end_matches('?') == "@value"
                            && dict_child.has_children()
                        {
                            let mut visiting = HashSet::new();
                            let mut seen_exact = HashSet::new();
                            collect_json_field_records(
                                program,
                                &env,
                                dict_child.children(),
                                &format!("{local_source}#val"),
                                targets,
                                &mut visiting,
                                &mut seen_exact,
                                &mut records,
                            );
                        }
                    }
                } else {
                    let mut visiting = HashSet::new();
                    let mut seen_exact = HashSet::new();
                    collect_json_field_records(
                        program,
                        &env,
                        node.children(),
                        &local_source,
                        targets,
                        &mut visiting,
                        &mut seen_exact,
                        &mut records,
                    );
                }
            }
            "fn" => {
                add_top(
                    &mut records,
                    TargetSymbolKind::Function,
                    raw,
                    SymbolScope::Module,
                );
            }
            _ => {}
        }
    }

    records
}

#[allow(clippy::too_many_arguments)]
fn collect_json_field_records<'a>(
    program: &'a ResolvedProgram,
    env: &UnitEnv,
    children: Vec<NodeRef<'a>>,
    scope_source: &str,
    targets: &[&str],
    visiting_defines: &mut HashSet<(String, NodeId)>,
    seen_exact_fields: &mut HashSet<String>,
    records: &mut Vec<SymbolRecord>,
) {
    for (child_idx, field_node) in children.into_iter().enumerate() {
        let field_name = field_node.name();
        if field_name.is_empty() {
            continue;
        }
        let clean_name = field_name.trim_end_matches('?');

        if field_node.args().is_empty() && !field_node.has_children_block() {
            if let Some(&DefineEntry::Block {
                ref unit_path,
                node_id,
            }) = env.defines.get(field_name)
            {
                let key = (unit_path.clone(), node_id);
                if visiting_defines.insert(key.clone()) {
                    if let Some(def_unit) = program.units.get(unit_path) {
                        let def_node = def_unit.document.node(node_id);
                        collect_json_field_records(
                            program,
                            env,
                            def_node.children(),
                            scope_source,
                            targets,
                            visiting_defines,
                            seen_exact_fields,
                            records,
                        );
                    }
                    visiting_defines.remove(&key);
                    continue;
                }
            }
        }

        let is_dict_field = is_dict_json_field(field_node);

        if is_portable_ident(clean_name) && seen_exact_fields.insert(clean_name.to_owned()) {
            for &target in targets {
                records.push(SymbolRecord::new(
                    TargetSymbolKind::Field,
                    clean_name,
                    target,
                    json_field_symbol(clean_name, target),
                    SymbolScope::Struct,
                    scope_source.to_owned(),
                    field_node.span(),
                ));
            }
        }

        if is_dict_field {
            for dict_child in field_node.children() {
                if dict_child.name().trim_end_matches('?') == "@value" && dict_child.has_children()
                {
                    let mut sub_visiting = HashSet::new();
                    let mut sub_seen = HashSet::new();
                    collect_json_field_records(
                        program,
                        env,
                        dict_child.children(),
                        &format!("{scope_source}#f{child_idx}#val"),
                        targets,
                        &mut sub_visiting,
                        &mut sub_seen,
                        records,
                    );
                }
            }
        } else if field_node.has_children() {
            let mut sub_visiting = HashSet::new();
            let mut sub_seen = HashSet::new();
            collect_json_field_records(
                program,
                env,
                field_node.children(),
                &format!("{scope_source}#f{child_idx}"),
                targets,
                &mut sub_visiting,
                &mut sub_seen,
                records,
            );
        }
    }
}

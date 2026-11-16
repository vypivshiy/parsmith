//! Pass 3 structural, directive, selector, `@request`, `@error`, GJSON path, intra-file top-down order,
//! and Pass 4 cross-target symbol validation.

pub mod http;
pub mod json;
pub mod naming;
pub mod order;
pub mod pipeline;
pub mod predicates;
pub mod structs;
pub mod symbols;
pub mod top_level;

pub use naming::*;

use std::collections::{HashMap, HashSet};

use crate::kdl::{KdlValue, NodeId, Span};
use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::imports::{ResolvedProgram, SourceUnit, SymbolKind};

/// Kind of a `define` symbol (`Scalar` or `Block`).
#[derive(Debug, Clone, PartialEq)]
pub enum DefineEntry {
    /// Scalar `define KEY=value`.
    Scalar(KdlValue),
    /// Block `define NAME { ... }` referencing `(unit_path, node_id)`.
    Block { unit_path: String, node_id: NodeId },
}

/// Per-unit semantic environment populated from visible `define` and `extension` symbols in `unit.scope`.
#[derive(Debug, Default)]
pub struct UnitEnv {
    /// Visible `define` symbols in this unit's scope (`name -> DefineEntry`).
    pub defines: IndexMap<String, DefineEntry>,
    /// Visible resolved scalar `define` strings (`name -> resolved_string`).
    pub scalar_strings: HashMap<String, String>,
    /// Visible `extension` operations (`"Namespace.op"`).
    pub extension_ops: HashSet<String>,
    /// Visible `extension` namespaces (`"Namespace"`).
    pub extension_namespaces: HashSet<String>,
    /// All `define` names declared anywhere in the program (used to avoid emitting `E200` when Pass 2 already emitted `E300`).
    pub all_program_defines: HashSet<String>,
    /// All `extension` operations declared anywhere in the program (`"Namespace.op"`).
    pub all_program_extension_ops: HashSet<String>,
}

impl UnitEnv {
    /// Builds the `UnitEnv` for `unit_path` from `program`.
    pub fn build(program: &ResolvedProgram, unit_path: &str) -> Self {
        let mut env = Self::default();

        for u in program.units.values() {
            for ((kind, name), _) in &u.exports {
                if *kind == SymbolKind::Define {
                    env.all_program_defines.insert(name.clone());
                } else if *kind == SymbolKind::Extension {
                    if let Some(&ext_nid) = u
                        .exports
                        .get(&(SymbolKind::Extension, name.clone()))
                        .and_then(|sym| u.export_nodes.get(sym))
                    {
                        for op_child in u.document.node(ext_nid).children() {
                            env.all_program_extension_ops.insert(format!(
                                "{}.{}",
                                name,
                                op_child.name()
                            ));
                        }
                    }
                }
            }
        }

        let Some(unit) = program.units.get(unit_path) else {
            return env;
        };

        for ((kind, name), sym_id) in &unit.scope {
            let Some(def_unit) = program.units.get(&sym_id.path) else {
                continue;
            };
            let Some(&nid) = def_unit.export_nodes.get(sym_id) else {
                continue;
            };
            let node = def_unit.document.node(nid);

            match kind {
                SymbolKind::Define => {
                    if node.has_children_block() {
                        env.defines.insert(
                            name.clone(),
                            DefineEntry::Block {
                                unit_path: sym_id.path.clone(),
                                node_id: nid,
                            },
                        );
                    } else if let Some(prop) = node.prop(name) {
                        env.defines
                            .insert(name.clone(), DefineEntry::Scalar(prop.value.clone()));
                        let raw_str = match &prop.value {
                            KdlValue::String(s) => s.clone(),
                            KdlValue::Int(n) => n.to_string(),
                            KdlValue::Float(f) => f.to_string(),
                            KdlValue::Bool(b) => b.to_string(),
                            KdlValue::Null => String::new(),
                        };
                        let resolved = interpolate_scalar_defines(&raw_str, &env.scalar_strings);
                        env.scalar_strings.insert(name.clone(), resolved);
                    }
                }
                SymbolKind::Extension => {
                    env.extension_namespaces.insert(name.clone());
                    for op_child in node.children() {
                        env.extension_ops
                            .insert(format!("{}.{}", name, op_child.name()));
                    }
                }
                _ => {}
            }
        }

        // Second pass for scalar defines in case of forward references or imported order
        for _ in 0..2 {
            let snapshot = env.scalar_strings.clone();
            for val in env.scalar_strings.values_mut() {
                *val = interpolate_scalar_defines(val, &snapshot);
            }
        }

        env
    }

    /// Resolves a string argument against visible scalar defines if it matches `is_define_name`,
    /// and expands any `{{UPPER_DEFINE}}` interpolations inside the string.
    pub fn resolve_str_arg(&self, raw: &str) -> String {
        if is_define_name(raw) {
            if let Some(val) = self.scalar_strings.get(raw) {
                return val.clone();
            }
        }
        interpolate_scalar_defines(raw, &self.scalar_strings)
    }

    /// Validates `{{UPPER_NAME}}` define interpolations inside a `value="..."` string literal (`E300`, `E002`).
    pub fn lint_value_string_defines(
        &self,
        unit_path: &str,
        raw: &str,
        value_span: Span,
        diags: &mut Vec<Diagnostic>,
    ) {
        for token in crate::linter::top_level::extract_double_brace_tokens(raw) {
            if is_define_name(token) {
                if !self.defines.contains_key(token) && !self.all_program_defines.contains(token) {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E300,
                        value_span,
                        format!("undefined define '{token}' in value=\"{raw}\""),
                        format!("declare 'define {token}=\"...\"' before use"),
                    ));
                } else if matches!(self.defines.get(token), Some(DefineEntry::Block { .. })) {
                    diags.push(make_error(
                        unit_path,
                        DiagnosticCode::E002,
                        value_span,
                        format!("cannot interpolate block define '{token}' in value=\"{raw}\""),
                        format!("use a scalar define: define {token}=\"...\""),
                    ));
                }
            }
        }
    }
}

/// Expands `{{UPPER_NAME}}` references using `scalars`, leaving unknown or lowercase `{{param}}` tokens intact.
pub fn interpolate_scalar_defines(raw: &str, scalars: &HashMap<String, String>) -> String {
    if !raw.contains("{{") {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token = &after[..end];
        if is_define_name(token) {
            if let Some(resolved) = scalars.get(token) {
                out.push_str(resolved);
            } else {
                out.push_str("{{");
                out.push_str(token);
                out.push_str("}}");
            }
        } else {
            out.push_str("{{");
            out.push_str(token);
            out.push_str("}}");
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Runs Pass 3 validation across all loaded [`SourceUnit`]s in `program`.
pub fn lint_program(program: &mut ResolvedProgram) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let unit_paths: Vec<String> = program.units.keys().cloned().collect();

    for path in unit_paths {
        let env = UnitEnv::build(program, &path);
        let (unit_diags, parsed_requests, parsed_errors) = {
            let unit = &program.units[&path];
            lint_unit(unit, program, &env)
        };
        if let Some(unit_mut) = program.units.get_mut(&path) {
            unit_mut.parsed_requests = parsed_requests;
            unit_mut.parsed_errors = parsed_errors;
        }
        diagnostics.extend(unit_diags);
    }

    diagnostics
}

fn lint_unit(
    unit: &SourceUnit,
    program: &ResolvedProgram,
    env: &UnitEnv,
) -> (
    Vec<Diagnostic>,
    IndexMap<NodeId, crate::request::RequestSpec>,
    IndexMap<NodeId, crate::request::ErrorResponseSpec>,
) {
    let mut diags = Vec::new();
    let mut parsed_requests = IndexMap::new();
    let mut parsed_errors = IndexMap::new();

    top_level::lint_top_level(unit, &mut diags);
    top_level::lint_doc_nodes(unit, &mut diags);
    top_level::lint_extensions(unit, &mut diags);
    top_level::lint_defines(unit, &mut diags);
    json::lint_json_defs(unit, program, env, &mut diags);
    structs::lint_structs(
        unit,
        program,
        env,
        &mut diags,
        &mut parsed_requests,
        &mut parsed_errors,
    );
    structs::lint_fns(unit, program, env, &mut diags);
    order::lint_topdown_order(unit, program, env, &mut diags);

    (diags, parsed_requests, parsed_errors)
}

/// Computes edit-distance (`strsim::jaro_winkler`) suggestions against `candidates`.
pub fn suggest_closest<'a>(
    input: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let mut scored: Vec<(&'a str, f64)> = candidates
        .into_iter()
        .filter_map(|cand| {
            let sim = strsim::jaro_winkler(input, cand);
            let norm_lev = strsim::normalized_levenshtein(input, cand);
            let score = sim.max(norm_lev);
            if score >= 0.68 {
                Some((cand, score))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    scored.into_iter().take(3).map(|(c, _)| c).collect()
}

/// Formats a `"Did you mean '...'?"` hint if any close matches exist.
pub fn did_you_mean_hint<'a>(
    input: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    fallback: &str,
) -> String {
    let suggestions = suggest_closest(input, candidates);
    if !suggestions.is_empty() {
        let quoted: Vec<String> = suggestions.into_iter().map(|s| format!("'{s}'")).collect();
        format!("Did you mean {}?", quoted.join(" or "))
    } else {
        fallback.to_owned()
    }
}

/// Checks `^[A-Z_][A-Z0-9_-]*$`.
pub fn is_define_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_uppercase() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Checks `^[A-Za-z_][A-Za-z0-9_-]*$`.
pub fn is_kebab_or_snake_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Checks `^[A-Za-z_][A-Za-z0-9_]*$`.
pub fn is_portable_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn make_error(
    unit_path: &str,
    code: DiagnosticCode,
    span: Span,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error(code, message, span)
        .with_path(unit_path.to_owned())
        .with_hint(hint)
}

pub(crate) fn make_warning(
    unit_path: &str,
    code: DiagnosticCode,
    span: Span,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> Diagnostic {
    Diagnostic::warning(code, message, span)
        .with_path(unit_path.to_owned())
        .with_hint(hint)
}

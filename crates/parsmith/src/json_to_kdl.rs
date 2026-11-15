//! Bootstrap KDL v2.1 `json` and `(array)json` schemas from sample JSON payloads.

use std::collections::HashSet;

use crate::kdl::{KdlDocument, KdlNode, NodeId};
use indexmap::IndexMap;
use serde_json::{Map, Value};
use thiserror::Error;

/// Errors returned when inferring a KDL v2.1 `json` schema from a JSON example.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JsonToKdlError {
    /// Input text is not valid JSON.
    #[error("invalid JSON: {0}")]
    InvalidJson(String),

    /// Root JSON value is neither an object nor a non-empty array of objects.
    #[error("JSON root must be an object or a non-empty array of objects")]
    InvalidRoot,

    /// Root schema name is empty or whitespace-only.
    #[error("schema name must not be empty")]
    EmptySchemaName,
}

impl From<serde_json::Error> for JsonToKdlError {
    fn from(err: serde_json::Error) -> Self {
        Self::InvalidJson(err.to_string())
    }
}

/// Converts a raw string into `PascalCase` by splitting on `-`, `_`, and whitespace
/// while preserving existing internal capitalization (`myFieldName` -> `MyFieldName`).
pub fn to_pascal_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for part in s.split(|c: char| c == '-' || c == '_' || c.is_whitespace()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            for upper in first.to_uppercase() {
                out.push(upper);
            }
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Returns `true` if `ident` is a reserved keyword in KDL 2.0 or any target language
/// (`python`, `javascript`/`typescript`, `go`, `rust`).
pub fn is_reserved_field_name(ident: &str) -> bool {
    crate::linter::symbols::is_reserved_in_any_ecosystem(ident)
}

/// Normalizes a raw JSON wire key into a portable KDL v2.1 field identifier.
pub fn normalize_field_name(key: &str) -> String {
    let mut normalized = String::with_capacity(key.len());
    let mut prev_underscore = false;

    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if ch == '_' {
                if !prev_underscore {
                    normalized.push('_');
                    prev_underscore = true;
                }
            } else {
                normalized.push(ch);
                prev_underscore = false;
            }
        } else if !prev_underscore {
            normalized.push('_');
            prev_underscore = true;
        }
    }

    let trimmed = normalized.trim_matches('_');
    let base = if trimmed.is_empty() { "field" } else { trimmed };

    let starts_with_digit = base.as_bytes().first().is_some_and(u8::is_ascii_digit);
    if starts_with_digit || is_reserved_field_name(base) {
        format!("k_{base}")
    } else {
        base.to_owned()
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "nil",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
    }
}

fn unique_type_names<'a>(values: impl IntoIterator<Item = &'a Value>) -> Vec<&'static str> {
    let mut unique = Vec::new();
    for val in values {
        let ty = json_type_name(val);
        if !unique.contains(&ty) {
            unique.push(ty);
        }
    }
    unique
}

/// Infers a unified scalar type (`str`, `int`, `float`, `bool`) across non-null samples,
/// widening mixed `int` and `float` samples to `float`.
fn infer_scalar_type(non_null_types: &[&'static str]) -> Option<&'static str> {
    match non_null_types {
        [single] if matches!(*single, "str" | "int" | "float" | "bool") => Some(*single),
        ["int", "float"] | ["float", "int"] => Some("float"),
        _ => None,
    }
}

fn apply_common_modifiers(
    mut node: KdlNode,
    wire_key: &str,
    field_name: &str,
    is_omitempty: bool,
) -> KdlNode {
    if is_omitempty {
        node = node.with_arg("@omitempty");
    }
    if field_name != wire_key {
        node = node.with_prop("from", wire_key);
    }
    node
}

fn build_skip_field(
    wire_key: &str,
    field_name: &str,
    is_omitempty: bool,
    comment: &str,
    is_array: bool,
) -> KdlNode {
    let mut node = KdlNode::new(field_name);
    if is_array {
        node = node.with_typed_arg("array", "nil");
    }
    node = node.with_arg("@skip");
    node = apply_common_modifiers(node, wire_key, field_name, is_omitempty);
    node.with_trailing_comment(comment)
}

struct SchemaGenerator {
    used_model_names: HashSet<String>,
}

impl SchemaGenerator {
    fn new() -> Self {
        Self {
            used_model_names: HashSet::new(),
        }
    }

    fn resolve_item_model_name(&mut self, field_name: &str, ancestors: &[String]) -> String {
        let field_pascal = to_pascal_case(field_name);
        let candidate = format!("{field_pascal}Item");
        if self.used_model_names.insert(candidate.clone()) {
            return candidate;
        }

        let mut last_candidate = candidate;
        for i in (0..ancestors.len()).rev() {
            let prefix: String = ancestors[i..].iter().map(|a| to_pascal_case(a)).collect();
            let candidate = format!("{prefix}{field_pascal}Item");
            if self.used_model_names.insert(candidate.clone()) {
                return candidate;
            }
            last_candidate = candidate;
        }

        let base_candidate = last_candidate;
        let mut suffix = 2usize;
        loop {
            let candidate = format!("{base_candidate}{suffix}");
            if self.used_model_names.insert(candidate.clone()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn generate_document(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<KdlDocument, JsonToKdlError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(JsonToKdlError::EmptySchemaName);
        }

        let mut doc = KdlDocument::new();

        match value {
            Value::Object(map) => {
                self.used_model_names.clear();
                self.used_model_names.insert(name.to_owned());

                let mut root = KdlNode::new("json").with_arg(name);
                if map.is_empty() {
                    root = root.with_empty_children_multiline(true);
                    doc.add_node(root);
                } else {
                    let root_id = doc.add_node(root);
                    self.populate_object_fields(&mut doc, root_id, &[map], &[]);
                }
                Ok(doc)
            }
            Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => {
                let item_name = format!("{name}Item");
                self.used_model_names.clear();
                self.used_model_names.insert(item_name.clone());
                self.used_model_names.insert(name.to_owned());

                let maps: Vec<&Map<String, Value>> =
                    items.iter().filter_map(Value::as_object).collect();
                let mut root = KdlNode::new("json").with_ty("array").with_arg(&item_name);
                if maps.iter().all(|m| m.is_empty()) {
                    root = root.with_empty_children_multiline(true);
                    doc.add_node(root);
                } else {
                    let root_id = doc.add_node(root);
                    self.populate_object_fields(&mut doc, root_id, &maps, &[]);
                }
                Ok(doc)
            }
            _ => Err(JsonToKdlError::InvalidRoot),
        }
    }

    fn populate_object_fields(
        &mut self,
        doc: &mut KdlDocument,
        parent_id: NodeId,
        objects: &[&Map<String, Value>],
        ancestors: &[String],
    ) {
        let mut merged: IndexMap<&str, Vec<&Value>> = IndexMap::new();
        for obj in objects {
            for (k, v) in *obj {
                merged.entry(k.as_str()).or_default().push(v);
            }
        }

        let raw_keys: HashSet<&str> = merged.keys().copied().collect();
        let exact_valid_keys: HashSet<String> = merged
            .keys()
            .filter_map(|&k| {
                let norm = normalize_field_name(k);
                (norm == k).then_some(norm)
            })
            .collect();
        let mut seen_field_names: HashSet<String> = HashSet::with_capacity(merged.len());

        for (wire_key, samples) in merged {
            let base_field = normalize_field_name(wire_key);
            let can_use_base = !seen_field_names.contains(&base_field)
                && (wire_key == base_field
                    || (!exact_valid_keys.contains(&base_field)
                        && !raw_keys.contains(base_field.as_str())));
            let field_name = if can_use_base {
                base_field
            } else {
                let mut suffix = 2usize;
                loop {
                    let candidate = format!("{base_field}_{suffix}");
                    if !seen_field_names.contains(&candidate)
                        && !exact_valid_keys.contains(&candidate)
                        && !raw_keys.contains(candidate.as_str())
                    {
                        break candidate;
                    }
                    suffix += 1;
                }
            };
            seen_field_names.insert(field_name.clone());

            let is_omitempty = samples.len() < objects.len();
            let has_null = samples.iter().any(|s| s.is_null());
            let non_null_samples: Vec<&Value> =
                samples.iter().copied().filter(|s| !s.is_null()).collect();

            // 0. All samples are null
            if non_null_samples.is_empty() {
                let node = apply_common_modifiers(
                    KdlNode::new(&field_name).with_arg("nil"),
                    wire_key,
                    &field_name,
                    is_omitempty,
                )
                .with_trailing_comment("unknown real type");
                doc.add_child_node(parent_id, node);
                continue;
            }

            // 1. Single or merged nested objects
            if non_null_samples.iter().all(|s| s.is_object()) {
                let child_maps: Vec<&Map<String, Value>> = non_null_samples
                    .iter()
                    .filter_map(|s| s.as_object())
                    .collect();

                if child_maps.iter().all(|m| m.is_empty()) {
                    let node = build_skip_field(
                        wire_key,
                        &field_name,
                        is_omitempty,
                        "empty object",
                        false,
                    );
                    doc.add_child_node(parent_id, node);
                } else {
                    let node_name = if has_null {
                        format!("{field_name}?")
                    } else {
                        field_name.clone()
                    };
                    let node = apply_common_modifiers(
                        KdlNode::new(node_name),
                        wire_key,
                        &field_name,
                        is_omitempty,
                    );
                    let child_id = doc.add_child_node(parent_id, node);

                    let mut next_ancestors = ancestors.to_vec();
                    next_ancestors.push(field_name);
                    self.populate_object_fields(doc, child_id, &child_maps, &next_ancestors);
                }
                continue;
            }

            // 2. Arrays
            if non_null_samples.iter().all(|s| s.is_array()) {
                let all_items: Vec<&Value> = non_null_samples
                    .iter()
                    .filter_map(|s| s.as_array())
                    .flatten()
                    .collect();

                if all_items.is_empty() {
                    let node =
                        build_skip_field(wire_key, &field_name, is_omitempty, "empty array", true);
                    doc.add_child_node(parent_id, node);
                    continue;
                }

                let item_has_null = all_items.iter().any(|item| item.is_null());
                let non_null_items: Vec<&Value> = all_items
                    .iter()
                    .copied()
                    .filter(|item| !item.is_null())
                    .collect();
                let non_null_item_types = unique_type_names(non_null_items.iter().copied());

                if non_null_item_types.as_slice() == ["object"] && !item_has_null {
                    let item_maps: Vec<&Map<String, Value>> = non_null_items
                        .iter()
                        .filter_map(|item| item.as_object())
                        .collect();

                    if item_maps.iter().all(|m| m.is_empty()) {
                        let node = build_skip_field(
                            wire_key,
                            &field_name,
                            is_omitempty,
                            "empty objects",
                            true,
                        );
                        doc.add_child_node(parent_id, node);
                    } else {
                        let item_model_name = self.resolve_item_model_name(&field_name, ancestors);
                        let type_arg = if has_null {
                            format!("{item_model_name}?")
                        } else {
                            item_model_name
                        };
                        let node = apply_common_modifiers(
                            KdlNode::new(&field_name).with_typed_arg("array", type_arg),
                            wire_key,
                            &field_name,
                            is_omitempty,
                        );
                        let child_id = doc.add_child_node(parent_id, node);

                        let mut next_ancestors = ancestors.to_vec();
                        next_ancestors.push(field_name);
                        self.populate_object_fields(doc, child_id, &item_maps, &next_ancestors);
                    }
                } else if let Some(scalar_ty) = infer_scalar_type(&non_null_item_types) {
                    let type_arg = if has_null || item_has_null {
                        format!("{scalar_ty}?")
                    } else {
                        scalar_ty.to_owned()
                    };
                    let node = apply_common_modifiers(
                        KdlNode::new(&field_name).with_typed_arg("array", type_arg),
                        wire_key,
                        &field_name,
                        is_omitempty,
                    );
                    doc.add_child_node(parent_id, node);
                } else {
                    let all_item_types = unique_type_names(all_items);
                    let types_comment = all_item_types.join(", ");
                    let node =
                        build_skip_field(wire_key, &field_name, is_omitempty, &types_comment, true);
                    doc.add_child_node(parent_id, node);
                }
                continue;
            }

            // 3. Primitive scalars and heterogeneous samples
            let non_null_types = unique_type_names(non_null_samples);
            if let Some(scalar_ty) = infer_scalar_type(&non_null_types) {
                let type_arg = if has_null {
                    format!("{scalar_ty}?")
                } else {
                    scalar_ty.to_owned()
                };
                let node = apply_common_modifiers(
                    KdlNode::new(&field_name).with_arg(type_arg),
                    wire_key,
                    &field_name,
                    is_omitempty,
                );
                doc.add_child_node(parent_id, node);
            } else {
                let all_sample_types = unique_type_names(samples);
                let types_comment = all_sample_types.join(", ");
                let node =
                    build_skip_field(wire_key, &field_name, is_omitempty, &types_comment, false);
                doc.add_child_node(parent_id, node);
            }
        }
    }
}

/// Infers a KDL v2.1 [`KdlDocument`] from a parsed [`serde_json::Value`].
pub fn json_value_to_kdl_document(
    value: &Value,
    name: &str,
) -> Result<KdlDocument, JsonToKdlError> {
    SchemaGenerator::new().generate_document(value, name)
}

/// Infers a KDL v2.1 [`KdlDocument`] from a raw JSON string.
pub fn json_to_kdl_document(json_input: &str, name: &str) -> Result<KdlDocument, JsonToKdlError> {
    let value: Value = serde_json::from_str(json_input)?;
    json_value_to_kdl_document(&value, name)
}

/// Infers a formatted KDL 2.0 (`v2.1` schema) string from a parsed [`serde_json::Value`].
pub fn json_value_to_kdl(value: &Value, name: &str) -> Result<String, JsonToKdlError> {
    let doc = json_value_to_kdl_document(value, name)?;
    Ok(doc.to_kdl())
}

/// Infers a formatted KDL 2.0 (`v2.1` schema) string from a raw JSON payload.
pub fn json_to_kdl(json_input: &str, name: &str) -> Result<String, JsonToKdlError> {
    let doc = json_to_kdl_document(json_input, name)?;
    Ok(doc.to_kdl())
}

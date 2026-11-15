//! `@request` and `@error[:<suffix>]` HTTP directive validation.

use std::collections::HashSet;

use crate::kdl::{KdlValue, NodeId, NodeRef};
use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::imports::SourceUnit;
use crate::linter::{UnitEnv, is_define_name, is_kebab_or_snake_ident, make_error};
use crate::request::{
    ErrorConditionValue, ErrorResponseSpec, PlaceholderSpec, REQUEST_LINE_CONTINUATION_HINT,
    RequestSpec, err_subclass_name, find_placeholder_tokens, has_backslash_line_comment,
    is_http_status, is_valid_error_suffix, parse_to_http,
};

/// Validates an `@request` directive node and records its parsed [`RequestSpec`].
pub fn lint_request_directive(
    unit: &SourceUnit,
    env: &UnitEnv,
    node: NodeRef<'_>,
    struct_type: &str,
    seen_request_names: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
    parsed_requests: &mut IndexMap<NodeId, RequestSpec>,
) {
    if node.has_children_block() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'@request' does not accept a children block",
            "",
        ));
    }

    if node.args().is_empty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'@request' requires a raw HTTP string",
            "example: @request \"curl https://example.com\"",
        ));
        return;
    }
    if node.args().len() > 1 {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'@request' accepts only 1 HTTP string argument",
            "",
        ));
    }

    let Some(raw_arg) = node.arg_value(0).and_then(|v| v.as_str()) else {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.arg(0).map_or_else(|| node.span(), |a| a.span),
            "'@request' requires a raw HTTP string",
            "",
        ));
        return;
    };

    if raw_arg.trim().is_empty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.arg(0).map_or_else(|| node.span(), |a| a.span),
            "'@request' requires a raw HTTP string",
            "",
        ));
        return;
    }

    // Validate properties on `@request`
    for prop in node.props() {
        if !matches!(
            prop.key.as_str(),
            "name" | "response" | "doc" | "response-path" | "response-join"
        ) {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                prop.span,
                format!("unknown property '{}' on '@request'", prop.key),
                "allowed properties: name, response, doc, response-path, response-join",
            ));
        }
    }

    let method_name = node
        .prop("name")
        .and_then(|p| p.value.as_str())
        .unwrap_or("fetch");
    if !seen_request_names.insert(method_name.to_owned()) {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E402,
            node.span(),
            format!("duplicate @request method name '{method_name}'"),
            "give each @request a distinct `name=\"...\"` property",
        ));
    }

    let is_rest = struct_type == "rest";
    let response_path_val = node
        .prop("response-path")
        .and_then(|p| p.value.as_str())
        .unwrap_or("");
    let has_response_join = node.has_prop("response-join");

    if let Some(rp_prop) = node.prop("response-path") {
        let Some(rp_str) = rp_prop.value.as_str() else {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                rp_prop.span,
                "response-path must be a non-empty dot-notation string",
                "",
            ));
            return;
        };
        let bad = rp_str.is_empty()
            || rp_str
                .split('.')
                .any(|seg| seg.is_empty() || !is_kebab_or_snake_ident(seg));
        if bad {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                rp_prop.span,
                format!(
                    "response-path must be dot-notation with non-empty ASCII identifier segments (e.g. \"data.user\"); got {rp_str:?}"
                ),
                "",
            ));
        }
    }

    if has_response_join {
        let rj_span = node
            .prop("response-join")
            .map_or_else(|| node.span(), |p| p.span);
        if is_rest {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E203,
                rj_span,
                "response-join is forbidden on type=rest structs (use response-path alone; Ok.value is the extracted object)",
                "",
            ));
        } else if response_path_val.is_empty() {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                rj_span,
                "response-join requires response-path (nothing to join)",
                "",
            ));
        }
    }

    // Resolve scalar define if `raw_arg` is a define identifier (do NOT interpolate `{{UPPER}}` in inline `@request`)
    let payload = if is_define_name(raw_arg) {
        if let Some(resolved) = env.scalar_strings.get(raw_arg) {
            resolved.clone()
        } else {
            raw_arg.to_owned()
        }
    } else {
        raw_arg.to_owned()
    };

    let has_bs_comment = has_backslash_line_comment(&payload);
    if has_bs_comment {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E002,
            node.span(),
            "single-line '//' comment after '\\' line continuation in @request",
            REQUEST_LINE_CONTINUATION_HINT,
        ));
    }

    // Check placeholder tokens in `payload`
    let mut seen_specs: IndexMap<String, PlaceholderSpec> = IndexMap::new();
    for token in find_placeholder_tokens(&payload) {
        let Some(spec) = PlaceholderSpec::parse(token) else {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E002,
                node.span(),
                format!("malformed placeholder {token:?} in @request"),
                "expected syntax: {{name}} or {{name:type}} (lowercase)",
            ));
            continue;
        };

        if spec.name != spec.name.to_ascii_lowercase() {
            let lower = spec.name.to_ascii_lowercase();
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E002,
                node.span(),
                format!(
                    "placeholder '{{{{{}}}}}' in @request must be lowercase; uppercase names are define substitutions which don't resolve in @request",
                    spec.name
                ),
                format!(
                    "use lowercase for runtime params (e.g. {{{{{lower}}}}}), or compose the URL in a define first"
                ),
            ));
            continue;
        }

        if let Some(prev) = seen_specs.get(&spec.name) {
            if prev != &spec {
                diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E402,
                    node.span(),
                    format!(
                        "placeholder '{}' uses conflicting type/style declarations",
                        spec.name
                    ),
                    "use one placeholder specification consistently",
                ));
            }
        } else {
            seen_specs.insert(spec.name.clone(), spec);
        }
    }

    match parse_to_http(&payload) {
        Ok(http_spec) => {
            parsed_requests.insert(node.id(), http_spec);
        }
        Err(err_msg) => {
            let hint = if has_bs_comment {
                REQUEST_LINE_CONTINUATION_HINT
            } else {
                "use a curl command or raw HTTP request"
            };
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E002,
                node.span(),
                err_msg,
                hint,
            ));
        }
    }
}

/// Validates an `@error` or `@error:<suffix>` directive node and records its parsed [`ErrorResponseSpec`].
#[allow(clippy::too_many_arguments)]
pub fn lint_error_directive(
    unit: &SourceUnit,
    env: &UnitEnv,
    node: NodeRef<'_>,
    field_name: &str,
    struct_name: &str,
    seen_err_variants: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
    parsed_errors: &mut IndexMap<NodeId, ErrorResponseSpec>,
) {
    let mut suffix = String::new();
    let mut suffix_valid = true;

    if let Some(raw_suffix) = field_name.strip_prefix("@error:") {
        let resolved_suffix = if is_define_name(raw_suffix) {
            env.scalar_strings
                .get(raw_suffix)
                .cloned()
                .unwrap_or_else(|| raw_suffix.to_owned())
        } else {
            raw_suffix.to_owned()
        };
        if !is_valid_error_suffix(&resolved_suffix) {
            suffix_valid = false;
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E002,
                node.name_span(),
                format!(
                    "invalid @error suffix '{raw_suffix}'; suffix must match ^[A-Za-z_][A-Za-z0-9_-]*$"
                ),
                "expected syntax: @error:<name> <status> <Schema> (ASCII identifier)",
            ));
        } else {
            suffix = resolved_suffix;
        }
    }

    if node.has_children_block() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "'@error' does not accept a children block",
            "example: @error 404 ApiError",
        ));
    }

    if node.args().len() < 2 {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.span(),
            "@error requires both status and schema name",
            "example: @error 404 ApiError",
        ));
        return;
    }

    // Parse status (arg 0)
    let status_arg = &node.args()[0];
    let status_opt: Option<i128> = match &status_arg.value {
        KdlValue::Int(n) => Some(*n),
        KdlValue::String(s) => {
            let resolved = env.resolve_str_arg(s);
            resolved.parse::<i128>().ok()
        }
        _ => None,
    };

    let Some(status_int) = status_opt else {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E002,
            status_arg.span,
            format!(
                "@error status must be integer, got {}",
                status_arg.value.to_kdl()
            ),
            "use an HTTP status code from 100 through 599",
        ));
        return;
    };

    if !is_http_status(status_int) {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E002,
            status_arg.span,
            format!("@error status must be in HTTP range 100..599, got {status_int}"),
            "use an HTTP status code from 100 through 599",
        ));
        return;
    }

    let Some(raw_schema) = node.arg_value(1).and_then(|v| v.as_str()) else {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E001,
            node.args()[1].span,
            "@error schema name must be a string identifier",
            "example: @error 404 ApiError",
        ));
        return;
    };
    let schema_name = env.resolve_str_arg(raw_schema);

    let mut required_keys = Vec::new();
    let mut positional_set = HashSet::new();
    let mut duplicate_keys: Vec<String> = Vec::new();

    for arg in &node.args()[2..] {
        let Some(k) = arg.value.as_str() else {
            diags.push(make_error(
                &unit.path,
                DiagnosticCode::E001,
                arg.span,
                "@error required key argument must be a string",
                "",
            ));
            continue;
        };
        let resolved_k = env.resolve_str_arg(k);
        if !positional_set.insert(resolved_k.clone()) {
            duplicate_keys.push(resolved_k.clone());
        }
        required_keys.push(resolved_k);
    }

    let mut conditions = IndexMap::new();
    for prop in node.props() {
        let key = env.resolve_str_arg(&prop.key);
        if positional_set.contains(&key) {
            duplicate_keys.push(key.clone());
        }
        let cond_val = match &prop.value {
            KdlValue::String(s) => ErrorConditionValue::String(env.resolve_str_arg(s)),
            KdlValue::Int(i) => ErrorConditionValue::Int(*i),
            KdlValue::Float(f) => ErrorConditionValue::Float(*f),
            KdlValue::Bool(b) => ErrorConditionValue::Bool(*b),
            KdlValue::Null => ErrorConditionValue::Null,
        };
        conditions.insert(key, cond_val);
    }

    duplicate_keys.sort();
    duplicate_keys.dedup();
    if !duplicate_keys.is_empty() {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E400,
            node.span(),
            format!("@error has duplicate keys: {}", duplicate_keys.join(", ")),
            "each key must be either a positional arg (presence check) or a property (value check)",
        ));
    }

    if !suffix_valid {
        return;
    }

    let err_spec = ErrorResponseSpec {
        status: status_int as u16,
        schema_name,
        required_keys,
        conditions,
        suffix,
        span: node.span(),
    };

    let cls_name = err_subclass_name(struct_name, &err_spec);
    if !seen_err_variants.insert(cls_name.clone()) {
        diags.push(make_error(
            &unit.path,
            DiagnosticCode::E400,
            node.span(),
            format!("duplicate @error variant name '{cls_name}' in struct '{struct_name}'"),
            "disambiguate duplicate @error variant name using '@error:<name> <status> <Schema> ...'",
        ));
    }

    parsed_errors.insert(node.id(), err_spec);
}

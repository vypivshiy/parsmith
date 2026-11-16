//! Node.js Host SDK bridge over `wasm-bindgen` for universal platform fallback in `js/`.

use std::collections::BTreeMap;

use parsmith::{
    CheckOptions, CheckResult, CodegenOptions, Diagnostic, ErrorConditionValue, GenerateResult,
    MemorySourceLoader, ModuleInfo, PlaceholderSpec, RestEndpointDecl, TypeInfo,
    check_entries_with_loader_and_targets, check_source_with_loader_and_targets,
    check_source_with_options, check_source_with_targets, check_with_loader_and_targets,
    check_with_options, generate_entries_with_loader, generate_source, generate_source_with_loader,
    generate_with_loader, inspect_module,
};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use wasm_bindgen::prelude::*;

fn serialize_diagnostic_bundle(diag: &Diagnostic, sources: &BTreeMap<String, String>) -> JsonValue {
    let cached_source = sources.get(&diag.path).cloned();
    let path_opt = if diag.path.is_empty() {
        None
    } else {
        Some(diag.path.clone())
    };
    let camel = json!({
        "code": diag.code.as_str(),
        "codeName": diag.code.name(),
        "severity": diag.severity.as_str(),
        "message": diag.message,
        "path": path_opt,
        "line": diag.line,
        "col": diag.col,
        "endLine": diag.end_line,
        "endCol": diag.end_col,
        "hint": diag.hint,
        "notes": diag.notes,
        "label": diag.label,
    });
    let snake = json!({
        "code": diag.code.as_str(),
        "code_name": diag.code.name(),
        "severity": diag.severity.as_str(),
        "message": diag.message,
        "path": path_opt,
        "line": diag.line,
        "col": diag.col,
        "end_line": diag.end_line,
        "end_col": diag.end_col,
        "hint": diag.hint,
        "notes": diag.notes,
        "label": diag.label,
    });
    json!({
        "raw": diag,
        "camel": camel,
        "snake": snake,
        "renderedDefault": diag.render(cached_source.as_deref()),
    })
}

fn build_loader(sources_json: Option<String>) -> Result<Option<MemorySourceLoader>, String> {
    let Some(raw) = sources_json else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let map: BTreeMap<String, String> =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid sources JSON: {e}"))?;
    let mut loader = MemorySourceLoader::new();
    for (k, v) in map {
        loader.insert(k, v);
    }
    Ok(Some(loader))
}

fn serialize_check_result(res: &CheckResult) -> String {
    let diags: Vec<JsonValue> = res
        .diagnostics
        .iter()
        .map(|d| serialize_diagnostic_bundle(d, &res.sources))
        .collect();
    let payload = json!({
        "ok": !res.has_errors(),
        "diagnostics": diags,
        "formatText": res.format_text(),
        "formatJson": res.format_json(),
    });
    serde_json::to_string(&payload).unwrap_or_default()
}

fn serialize_generate_result(res: &GenerateResult) -> String {
    let diags: Vec<JsonValue> = res
        .diagnostics
        .iter()
        .map(|d| serialize_diagnostic_bundle(d, &res.sources))
        .collect();
    let mut files_map = JsonMap::new();
    for (k, v) in &res.files {
        files_map.insert(k.clone(), JsonValue::String(v.clone()));
    }
    let payload = json!({
        "ok": !res.has_errors(),
        "files": JsonValue::Object(files_map),
        "primaryCode": res.primary_code(),
        "diagnostics": diags,
        "formatText": res.format_text(),
        "formatJson": res.format_json(),
    });
    serde_json::to_string(&payload).unwrap_or_default()
}

fn serialize_type_info_bundle(ti: &TypeInfo) -> JsonValue {
    let key_bundle = ti.key_type.as_deref().map(serialize_type_info_bundle);
    let val_bundle = ti.value_type.as_deref().map(serialize_type_info_bundle);
    let camel = json!({
        "base": ti.base.dsl_name().to_ascii_lowercase(),
        "dslName": ti.base.dsl_name(),
        "isArray": ti.is_array,
        "isOptional": ti.is_optional,
        "isDict": ti.is_dict,
        "reference": ti.ref_name(),
        "keyType": key_bundle.as_ref().map(|b| b["camel"].clone()),
        "valueType": val_bundle.as_ref().map(|b| b["camel"].clone()),
        "omitempty": ti.omitempty,
        "skip": ti.skip,
    });
    let snake = serde_json::to_value(ti).unwrap_or(JsonValue::Null);
    json!({
        "camel": camel,
        "snake": snake,
        "keyType": key_bundle,
        "valueType": val_bundle,
    })
}

fn serialize_placeholder_bundle(ph: &PlaceholderSpec) -> JsonValue {
    let camel = json!({
        "name": ph.name,
        "typeName": ph.type_name.as_str(),
        "isArray": ph.is_array,
        "isOptional": ph.is_optional,
        "style": ph.style.map(|s| s.as_str()),
        "token": ph.to_token(),
    });
    let snake = serde_json::to_value(ph).unwrap_or(JsonValue::Null);
    json!({
        "camel": camel,
        "snake": snake,
    })
}

fn serialize_rest_endpoint_bundle(ep: &RestEndpointDecl) -> JsonValue {
    let phs: Vec<JsonValue> = ep
        .placeholders
        .iter()
        .map(serialize_placeholder_bundle)
        .collect();
    let phs_camel: Vec<JsonValue> = phs.iter().map(|p| p["camel"].clone()).collect();
    let phs_snake: Vec<JsonValue> = phs.iter().map(|p| p["snake"].clone()).collect();
    let camel = json!({
        "name": ep.name,
        "method": ep.spec.method,
        "url": ep.spec.url.source(),
        "doc": ep.doc,
        "responseSchema": ep.response_schema,
        "responsePath": ep.response_path,
        "responseIsArray": ep.response_is_array,
        "resultAliasName": ep.result_alias_name,
        "bodyKind": ep.spec.body_kind.as_str(),
        "placeholders": phs_camel,
    });
    let snake = json!({
        "name": ep.name,
        "method": ep.spec.method,
        "url": ep.spec.url.source(),
        "doc": ep.doc,
        "response_schema": ep.response_schema,
        "response_path": ep.response_path,
        "response_is_array": ep.response_is_array,
        "result_alias_name": ep.result_alias_name,
        "body_kind": ep.spec.body_kind.as_str(),
        "placeholders": phs_snake,
    });
    json!({
        "camel": camel,
        "snake": snake,
        "placeholders": phs,
    })
}

fn serialize_module_info_bundle(check_res: CheckResult) -> String {
    let mut check_res = check_res;
    let sources = std::mem::take(&mut check_res.sources);
    let diagnostics = std::mem::take(&mut check_res.diagnostics);
    let mut rest_endpoints_by_struct: BTreeMap<String, Vec<RestEndpointDecl>> = BTreeMap::new();

    let inner = if let Some(module) = check_res.module.take() {
        for s in &module.structs {
            if !s.rest_endpoints.is_empty() {
                rest_endpoints_by_struct.insert(s.name.clone(), s.rest_endpoints.clone());
            }
        }
        inspect_module(&module, diagnostics)
    } else {
        ModuleInfo {
            diagnostics,
            ..Default::default()
        }
    };

    let ok = !inner.diagnostics.iter().any(|d| d.severity.is_error());
    let format_text = CheckResult::new(inner.diagnostics.clone())
        .with_sources(sources.clone())
        .format_text();
    let format_json = CheckResult::new(inner.diagnostics.clone()).format_json();
    let to_json_pretty = serde_json::to_string_pretty(&inner).unwrap_or_default();
    let mut module_snake = serde_json::to_value(&inner).unwrap_or(JsonValue::Null);
    if let Some(obj) = module_snake.as_object_mut() {
        obj.insert("ok".to_owned(), JsonValue::Bool(ok));
    }

    let mut struct_kinds_map = JsonMap::new();
    for (k, v) in &inner.struct_kinds {
        struct_kinds_map.insert(k.clone(), JsonValue::String(v.as_str().to_owned()));
    }

    let structs_bundles: Vec<JsonValue> = inner
        .structs
        .iter()
        .map(|s| {
            let fields_b: Vec<JsonValue> = s
                .fields
                .iter()
                .map(|f| {
                    let ti_b = serialize_type_info_bundle(&f.type_info);
                    json!({
                        "camel": {
                            "name": f.name,
                            "typeInfo": ti_b["camel"],
                            "hasFallback": f.has_fallback,
                        },
                        "snake": serde_json::to_value(f).unwrap_or(JsonValue::Null),
                        "typeInfo": ti_b,
                    })
                })
                .collect();

            let init_b: Vec<JsonValue> = s
                .init_fields
                .iter()
                .map(|i| {
                    let ti_b = serialize_type_info_bundle(&i.type_info);
                    json!({
                        "camel": {
                            "name": i.name,
                            "typeInfo": ti_b["camel"],
                        },
                        "snake": serde_json::to_value(i).unwrap_or(JsonValue::Null),
                        "typeInfo": ti_b,
                    })
                })
                .collect();

            let checks_b: Vec<JsonValue> = s
                .checks
                .iter()
                .map(|c| {
                    let ti_b = serialize_type_info_bundle(&c.return_type);
                    json!({
                        "camel": {
                            "name": c.name,
                            "returnType": ti_b["camel"],
                        },
                        "snake": serde_json::to_value(c).unwrap_or(JsonValue::Null),
                        "returnType": ti_b,
                    })
                })
                .collect();

            let reqs_b: Vec<JsonValue> = s
                .requests
                .iter()
                .map(|r| {
                    let phs_b: Vec<JsonValue> = r
                        .placeholders
                        .iter()
                        .map(serialize_placeholder_bundle)
                        .collect();
                    let phs_camel: Vec<JsonValue> =
                        phs_b.iter().map(|p| p["camel"].clone()).collect();
                    json!({
                        "camel": {
                            "name": r.name,
                            "method": r.method,
                            "url": r.url,
                            "doc": r.doc,
                            "responseSchema": r.response_schema,
                            "responsePath": r.response_path,
                            "responseJoin": r.response_join,
                            "resultAliasName": r.result_alias_name,
                            "bodyKind": r.body_kind.as_str(),
                            "placeholders": phs_camel,
                        },
                        "snake": serde_json::to_value(r).unwrap_or(JsonValue::Null),
                        "placeholders": phs_b,
                    })
                })
                .collect();

            let eps = rest_endpoints_by_struct
                .get(&s.name)
                .cloned()
                .unwrap_or_default();
            let rest_eps_b: Vec<JsonValue> =
                eps.iter().map(serialize_rest_endpoint_bundle).collect();

            let errs_b: Vec<JsonValue> = s
                .errors
                .iter()
                .map(|e| {
                    let mut cond_map = JsonMap::new();
                    for (ck, cv) in &e.conditions {
                        let jv = match cv {
                            ErrorConditionValue::String(st) => JsonValue::String(st.clone()),
                            ErrorConditionValue::Int(i) => json!(*i),
                            ErrorConditionValue::Float(f) => json!(*f),
                            ErrorConditionValue::Bool(b) => JsonValue::Bool(*b),
                            ErrorConditionValue::Null => JsonValue::Null,
                        };
                        cond_map.insert(ck.clone(), jv);
                    }
                    json!({
                        "camel": {
                            "variantName": e.variant_name,
                            "name": e.variant_name,
                            "status": e.status,
                            "schemaName": e.schema_name,
                            "suffix": e.suffix,
                            "requiredKeys": e.required_keys,
                            "conditions": JsonValue::Object(cond_map),
                        },
                        "snake": serde_json::to_value(e).unwrap_or(JsonValue::Null),
                    })
                })
                .collect();

            let kt_b = s.key_type.as_ref().map(serialize_type_info_bundle);
            let vt_b = s.value_type.as_ref().map(serialize_type_info_bundle);

            let camel = json!({
                "name": s.name,
                "kind": s.kind.as_str(),
                "doc": s.doc,
                "keepOrder": s.keep_order,
                "fields": fields_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                "initFields": init_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                "checks": checks_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                "hasPreValidate": s.has_pre_validate,
                "keyType": kt_b.as_ref().map(|x| x["camel"].clone()),
                "valueType": vt_b.as_ref().map(|x| x["camel"].clone()),
                "requests": reqs_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                "restEndpoints": rest_eps_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                "errors": errs_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
            });

            json!({
                "camel": camel,
                "snake": serde_json::to_value(s).unwrap_or(JsonValue::Null),
                "fields": fields_b,
                "initFields": init_b,
                "checks": checks_b,
                "keyType": kt_b,
                "valueType": vt_b,
                "requests": reqs_b,
                "restEndpoints": rest_eps_b,
                "errors": errs_b,
            })
        })
        .collect();

    let fns_bundles: Vec<JsonValue> = inner
        .functions
        .iter()
        .map(|f| {
            let params_b: Vec<JsonValue> = f
                .params
                .iter()
                .map(|p| {
                    let ti_b = serialize_type_info_bundle(&p.type_info);
                    json!({
                        "camel": {
                            "name": p.name,
                            "typeInfo": ti_b["camel"],
                        },
                        "snake": serde_json::to_value(p).unwrap_or(JsonValue::Null),
                        "typeInfo": ti_b,
                    })
                })
                .collect();
            let ret_b = serialize_type_info_bundle(&f.return_type);
            json!({
                "camel": {
                    "name": f.name,
                    "isRaw": f.is_raw,
                    "doc": f.doc,
                    "params": params_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                    "returnType": ret_b["camel"],
                },
                "snake": serde_json::to_value(f).unwrap_or(JsonValue::Null),
                "params": params_b,
                "returnType": ret_b,
            })
        })
        .collect();

    let json_defs_bundles: Vec<JsonValue> = inner
        .json_defs
        .iter()
        .map(|j| {
            let fields_b: Vec<JsonValue> = j
                .fields
                .iter()
                .map(|f| {
                    let ti_b = serialize_type_info_bundle(&f.type_info);
                    json!({
                        "camel": {
                            "name": f.name,
                            "typeInfo": ti_b["camel"],
                            "alias": f.alias,
                            "doc": f.doc,
                            "isOptional": f.is_optional,
                            "omitempty": f.omitempty,
                            "skip": f.skip,
                            "isDict": f.is_dict,
                        },
                        "snake": serde_json::to_value(f).unwrap_or(JsonValue::Null),
                        "typeInfo": ti_b,
                    })
                })
                .collect();
            let kt_b = j.key_type.as_ref().map(serialize_type_info_bundle);
            let vt_b = j.value_type.as_ref().map(serialize_type_info_bundle);
            json!({
                "camel": {
                    "name": j.name,
                    "kind": j.kind.as_str(),
                    "doc": j.doc,
                    "isArray": j.is_array,
                    "isDict": j.is_dict,
                    "isHoisted": j.is_hoisted,
                    "path": j.path,
                    "fields": fields_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                    "keyType": kt_b.as_ref().map(|x| x["camel"].clone()),
                    "valueType": vt_b.as_ref().map(|x| x["camel"].clone()),
                },
                "snake": serde_json::to_value(j).unwrap_or(JsonValue::Null),
                "fields": fields_b,
                "keyType": kt_b,
                "valueType": vt_b,
            })
        })
        .collect();

    let exts_bundles: Vec<JsonValue> = inner
        .extensions
        .iter()
        .map(|ext| {
            let ops_b: Vec<JsonValue> = ext
                .operations
                .iter()
                .map(|op| {
                    json!({
                        "camel": {
                            "name": op.name,
                            "qualifiedName": op.qualified_name,
                            "inputType": op.input.expected_name(),
                            "outputType": op.output.expected_name(),
                            "isGeneric": op.input.is_generic(),
                            "targets": op.targets,
                        },
                        "snake": serde_json::to_value(op).unwrap_or(JsonValue::Null),
                    })
                })
                .collect();
            json!({
                "camel": {
                    "namespace": ext.namespace,
                    "name": ext.namespace,
                    "operations": ops_b.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
                },
                "snake": serde_json::to_value(ext).unwrap_or(JsonValue::Null),
                "operations": ops_b,
            })
        })
        .collect();

    let diags_bundles: Vec<JsonValue> = inner
        .diagnostics
        .iter()
        .map(|d| serialize_diagnostic_bundle(d, &sources))
        .collect();

    let module_camel = json!({
        "ok": ok,
        "doc": inner.doc,
        "sourceFile": inner.source_file,
        "structKinds": JsonValue::Object(struct_kinds_map),
        "structs": structs_bundles.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
        "functions": fns_bundles.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
        "jsonDefs": json_defs_bundles.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
        "extensions": exts_bundles.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
        "diagnostics": diags_bundles.iter().map(|x| x["camel"].clone()).collect::<Vec<_>>(),
    });

    let root = json!({
        "camel": module_camel,
        "snake": module_snake,
        "toJson": to_json_pretty,
        "formatText": format_text,
        "formatJson": format_json,
        "structs": structs_bundles,
        "functions": fns_bundles,
        "jsonDefs": json_defs_bundles,
        "extensions": exts_bundles,
        "diagnostics": diags_bundles,
    });

    serde_json::to_string(&root).unwrap_or_default()
}

/// Renders a serialized `parsmith::Diagnostic` with an optional custom source text.
#[wasm_bindgen]
pub fn sdk_render_diagnostic(diag_json: &str, source: Option<String>) -> String {
    let Ok(diag) = serde_json::from_str::<Diagnostic>(diag_json) else {
        return String::new();
    };
    diag.render(source.as_deref())
}

/// Executes `parsmith.check()` for the Node.js WASM fallback.
#[wasm_bindgen]
pub fn sdk_check(
    entry_or_source: &str,
    is_path: bool,
    logical_path: Option<String>,
    sources_json: Option<String>,
    targets_json: Option<String>,
) -> Result<String, JsValue> {
    let vfs_loader = build_loader(sources_json).map_err(|e| JsValue::from_str(&e))?;
    let targets: Option<Vec<String>> = match targets_json {
        Some(ref s) if !s.trim().is_empty() => {
            Some(serde_json::from_str(s).map_err(|e| JsValue::from_str(&e.to_string()))?)
        }
        _ => None,
    };

    let multi_entries: Option<Vec<String>> =
        if is_path && entry_or_source.trim_start().starts_with('[') {
            serde_json::from_str(entry_or_source).ok()
        } else {
            None
        };

    let res = match (is_path, multi_entries, vfs_loader) {
        (true, Some(entries), Some(loader)) => {
            check_entries_with_loader_and_targets(&entries, &loader, targets)
        }
        (true, Some(entries), None) => {
            let empty_loader = MemorySourceLoader::new();
            check_entries_with_loader_and_targets(&entries, &empty_loader, targets)
        }
        (true, None, Some(loader)) => {
            check_with_loader_and_targets(entry_or_source, &loader, targets)
        }
        (false, _, Some(loader)) => {
            let lp = logical_path.as_deref().unwrap_or("<memory>.kdl");
            check_source_with_loader_and_targets(entry_or_source, Some(lp), &loader, targets)
        }
        (true, None, None) => {
            let empty_loader = MemorySourceLoader::new();
            check_with_loader_and_targets(entry_or_source, &empty_loader, targets)
        }
        (false, _, None) => {
            check_source_with_targets(entry_or_source, logical_path.as_deref(), targets)
        }
    };

    Ok(serialize_check_result(&res))
}

/// Executes `parsmith.generate()` for the Node.js WASM fallback.
#[wasm_bindgen]
pub fn sdk_generate(
    entry_or_source: &str,
    is_path: bool,
    logical_path: Option<String>,
    sources_json: Option<String>,
    options_json: &str,
) -> Result<String, JsValue> {
    let vfs_loader = build_loader(sources_json).map_err(|e| JsValue::from_str(&e))?;
    let opts: CodegenOptions =
        serde_json::from_str(options_json).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let multi_entries: Option<Vec<String>> =
        if is_path && entry_or_source.trim_start().starts_with('[') {
            serde_json::from_str(entry_or_source).ok()
        } else {
            None
        };

    let res = match (is_path, multi_entries, vfs_loader) {
        (true, Some(entries), Some(loader)) => {
            generate_entries_with_loader(&entries, &loader, &opts)
        }
        (true, Some(entries), None) => {
            let empty_loader = MemorySourceLoader::new();
            generate_entries_with_loader(&entries, &empty_loader, &opts)
        }
        (true, None, Some(loader)) => generate_with_loader(entry_or_source, &loader, &opts),
        (false, _, Some(loader)) => {
            let lp = logical_path.as_deref().unwrap_or("<memory>.kdl");
            generate_source_with_loader(entry_or_source, Some(lp), &loader, &opts)
        }
        (true, None, None) => {
            let empty_loader = MemorySourceLoader::new();
            generate_with_loader(entry_or_source, &empty_loader, &opts)
        }
        (false, _, None) => generate_source(entry_or_source, logical_path.as_deref(), &opts),
    };

    Ok(serialize_generate_result(&res))
}

/// Executes `parsmith.inspectSchema()` for the Node.js WASM fallback.
#[wasm_bindgen]
pub fn sdk_inspect(
    entry_or_source: &str,
    is_path: bool,
    logical_path: Option<String>,
    sources_json: Option<String>,
) -> Result<String, JsValue> {
    let vfs_loader = build_loader(sources_json).map_err(|e| JsValue::from_str(&e))?;
    let check_opts = CheckOptions::default();

    let res = match (is_path, vfs_loader) {
        (true, Some(loader)) => check_with_options(entry_or_source, &loader, &check_opts),
        (false, Some(loader)) => {
            let lp = logical_path.as_deref().unwrap_or("<memory>.kdl");
            check_source_with_options(entry_or_source, Some(lp), &loader, &check_opts)
        }
        (true, None) => {
            let empty_loader = MemorySourceLoader::new();
            check_with_options(entry_or_source, &empty_loader, &check_opts)
        }
        (false, None) => {
            let empty_loader = MemorySourceLoader::new();
            check_source_with_options(
                entry_or_source,
                logical_path.as_deref(),
                &empty_loader,
                &check_opts,
            )
        }
    };

    Ok(serialize_module_info_bundle(res))
}

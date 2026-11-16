//! Pass 5 semantic lowering: `define` expansion, `extension` lowering with `ts` -> `js` fallback,
//! post-order inline `json` & `@value` hoisting, `OpSig` pipeline type checking (`E100`),
//! `StructTypeDef` / `RestArtifact` synthesis, and immutable [`Module`] AST construction.

use std::collections::HashSet;

use crate::kdl::{KdlValue, NodeId, NodeRef, Span};
use indexmap::IndexMap;

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, ExtensionDecl, ExtensionHelper, ExtensionImport,
    ExtensionOp, ExtensionTargetBlock, ExtensionTypeSig, FieldDecl, FnParam, FunctionDef,
    InitFieldDecl, JsonDef, JsonFieldDef, JsonKind, LiteralValue, MatcherEntry, MatcherListDef,
    Module, Pipeline, RawMode, RequestDecl, RestArtifact, RestEndpointDecl, ResultAliasDef,
    ResultVariantDef, StepOp, StructDecl, StructKind, StructTypeDef, TypeDefField, TypeInfo,
    TypedStep,
};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::gjson::GjsonPath;
use crate::imports::{ResolvedProgram, SourceUnit, SymbolKind};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::json::{
    extract_value_model_and_array, is_arg_quoted, is_dict_json_field, synthesize_value_schema_name,
};
use crate::linter::{DefineEntry, UnitEnv, make_error};
use crate::request::{ErrorResponseSpec, err_subclass_name, is_error_directive};
use crate::typecheck::{
    apply_op_sig, base_type_compatible, check_and_lower_fallback, op_signature, type_mismatch_hint,
};
use crate::validators::normalize_regex_pattern;

/// Cleans a docstring by stripping leading/trailing blank lines and common leading indentation
/// (equivalent to Python's `inspect.cleandoc`).
pub fn clean_doc(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let min_indent = lines
        .iter()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);

    let mut out_lines: Vec<&str> = Vec::with_capacity(lines.len());
    out_lines.push(lines[0].trim());
    for &line in &lines[1..] {
        if line.trim().is_empty() {
            out_lines.push("");
        } else if line.len() >= min_indent {
            out_lines.push(line[min_indent..].trim_end());
        } else {
            out_lines.push(line.trim());
        }
    }

    while out_lines.first().is_some_and(|l| l.is_empty()) {
        out_lines.remove(0);
    }
    while out_lines.last().is_some_and(|l| l.is_empty()) {
        out_lines.pop();
    }

    out_lines.join("\n")
}

/// Lowers a validated [`ResolvedProgram`] into an immutable [`Module`] AST, recording any Pass 5
/// diagnostics (`E001` hoisting collisions, `E100` pipeline type mismatches) into `diags`.
pub fn lower_program(program: &ResolvedProgram, diags: &mut Vec<Diagnostic>) -> Module {
    let source_file = std::path::Path::new(&program.entry_path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.entry_path.clone());

    let doc = program
        .doc_comment
        .as_deref()
        .map(clean_doc)
        .filter(|s| !s.is_empty());

    let mut ctx = LoweringCtx::new(program, diags);
    ctx.lower_extensions();
    ctx.index_struct_kinds();
    ctx.lower_json_defs();
    ctx.lower_structs();
    ctx.lower_functions();

    Module {
        doc,
        source_file,
        extensions: ctx.extension_decls,
        json_defs: ctx.json_defs.into_values().collect(),
        typedefs: ctx.typedefs,
        rest_artifacts: ctx.rest_artifacts,
        structs: ctx.structs,
        functions: ctx.functions,
    }
}

struct LoweringCtx<'a> {
    program: &'a ResolvedProgram,
    diags: &'a mut Vec<Diagnostic>,
    extension_decls: Vec<ExtensionDecl>,
    extension_ops: IndexMap<String, ExtensionOp>,
    json_defs: IndexMap<String, JsonDef>,
    all_declared_json_names: HashSet<String>,
    struct_kinds: IndexMap<String, StructKind>,
    typedefs: Vec<StructTypeDef>,
    rest_artifacts: Vec<RestArtifact>,
    structs: Vec<StructDecl>,
    functions: Vec<FunctionDef>,
}

impl<'a> LoweringCtx<'a> {
    fn new(program: &'a ResolvedProgram, diags: &'a mut Vec<Diagnostic>) -> Self {
        let mut all_declared_json_names = HashSet::new();
        for item in &program.items {
            if item.symbol.kind == SymbolKind::Json {
                all_declared_json_names.insert(item.symbol.name.clone());
            }
        }
        Self {
            program,
            diags,
            extension_decls: Vec::new(),
            extension_ops: IndexMap::new(),
            json_defs: IndexMap::new(),
            all_declared_json_names,
            struct_kinds: IndexMap::new(),
            typedefs: Vec::new(),
            rest_artifacts: Vec::new(),
            structs: Vec::new(),
            functions: Vec::new(),
        }
    }

    fn lower_extensions(&mut self) {
        for item in &self.program.items {
            if item.symbol.kind != SymbolKind::Extension {
                continue;
            }
            let Some(unit) = self.program.units.get(&item.symbol.path) else {
                continue;
            };
            let node = unit.document.node(item.node_id);
            let Some(namespace) = node.arg_value(0).and_then(|v| v.as_str()) else {
                continue;
            };

            let mut operations = Vec::new();
            for op_node in node.children() {
                let op_name = op_node.name();
                let Some(sig_node) = op_node.children().into_iter().find(|c| c.name() == "sig")
                else {
                    continue;
                };
                if sig_node.args().len() != 2 {
                    continue;
                }
                let Some(input_sig) = parse_extension_sig_arg(&sig_node.args()[0]) else {
                    continue;
                };
                let Some(output_sig) = parse_extension_sig_arg(&sig_node.args()[1]) else {
                    continue;
                };

                let mut targets = IndexMap::new();
                for child in op_node.children() {
                    let canon = match child.name() {
                        "py" => Some("py"),
                        "js" => Some("js"),
                        "ts" => Some("ts"),
                        "go" => Some("go"),
                        "rust" => Some("rust"),
                        "ex" | "elixir" => Some("ex"),
                        "php" => Some("php"),
                        "cs" | "csharp" => Some("cs"),
                        "kt" | "kotlin" => Some("kt"),
                        _ => None,
                    };
                    if let Some(key) = canon {
                        let mut block = parse_extension_target_block(child);
                        block.language = key.to_owned();
                        targets.insert(key.to_owned(), block);
                    }
                }

                // Automatic fallback from `ts` to `js` when `ts` is omitted
                if !targets.contains_key("ts") {
                    if let Some(js_block) = targets.get("js").cloned() {
                        targets.insert(
                            "ts".to_owned(),
                            ExtensionTargetBlock {
                                language: "ts".to_owned(),
                                ..js_block
                            },
                        );
                    }
                }

                let qualified_name = format!("{namespace}.{op_name}");
                let ext_op = ExtensionOp {
                    namespace: namespace.to_owned(),
                    name: op_name.to_owned(),
                    qualified_name: qualified_name.clone(),
                    input: input_sig,
                    output: output_sig,
                    targets,
                    span: op_node.span(),
                };
                self.extension_ops.insert(qualified_name, ext_op.clone());
                operations.push(ext_op);
            }

            self.extension_decls.push(ExtensionDecl {
                namespace: namespace.to_owned(),
                operations,
                span: node.span(),
            });
        }
    }

    fn index_struct_kinds(&mut self) {
        for item in &self.program.items {
            if item.symbol.kind != SymbolKind::Struct {
                continue;
            }
            let Some(unit) = self.program.units.get(&item.symbol.path) else {
                continue;
            };
            let node = unit.document.node(item.node_id);
            let kind = resolve_struct_kind(node);
            self.struct_kinds.insert(item.symbol.name.clone(), kind);
        }
    }

    fn lower_json_defs(&mut self) {
        let items: Vec<_> = self
            .program
            .items
            .iter()
            .filter(|i| i.symbol.kind == SymbolKind::Json)
            .cloned()
            .collect();

        for item in items {
            let Some(unit) = self.program.units.get(&item.symbol.path) else {
                continue;
            };
            let env = UnitEnv::build(self.program, &unit.path);
            let node = unit.document.node(item.node_id);
            let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }

            let raw_ann = node.ty().unwrap_or("").trim_end_matches('?');
            let type_prop = node
                .prop("type")
                .and_then(|p| p.value.as_str())
                .unwrap_or("");
            let is_array = raw_ann == "array" || type_prop == "array";
            let is_dict = raw_ann == "dict" || type_prop == "dict";
            let kind = if is_dict {
                JsonKind::Dict
            } else if is_array {
                JsonKind::Array
            } else {
                JsonKind::Object
            };

            let raw_path = node
                .prop("path")
                .and_then(|p| p.value.as_str())
                .map(|s| env.resolve_str_arg(s));
            let path = raw_path
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let parsed_path = path.as_deref().and_then(|p| GjsonPath::parse(p).ok());

            let doc = node
                .prop("doc")
                .and_then(|p| p.value.as_str())
                .map(|s| clean_doc(&env.resolve_str_arg(s)))
                .filter(|s| !s.is_empty());

            if is_dict {
                let (key_type, value_type) =
                    self.lower_dict_json_directives(unit, &env, node.children(), name, "", name);
                let json_def = JsonDef {
                    name: name.to_owned(),
                    kind,
                    doc,
                    is_array: false,
                    is_dict: true,
                    path,
                    parsed_path,
                    fields: Vec::new(),
                    key_type: Some(key_type),
                    value_type: Some(value_type),
                    is_hoisted: false,
                    span: node.span(),
                };
                self.json_defs.insert(name.to_owned(), json_def);
            } else {
                let mut visiting_defines = HashSet::new();
                let fields = self.lower_json_fields(
                    unit,
                    &env,
                    &unit.source,
                    name,
                    node.children(),
                    &mut visiting_defines,
                );
                let json_def = JsonDef {
                    name: name.to_owned(),
                    kind,
                    doc,
                    is_array,
                    is_dict: false,
                    path,
                    parsed_path,
                    fields,
                    key_type: None,
                    value_type: None,
                    is_hoisted: false,
                    span: node.span(),
                };
                self.json_defs.insert(name.to_owned(), json_def);
            }
        }
    }

    fn lower_dict_json_directives<'b>(
        &mut self,
        unit: &SourceUnit,
        env: &UnitEnv,
        children: Vec<NodeRef<'b>>,
        parent_json_name: &str,
        field_name: &str,
        explicit_dict_name: &str,
    ) -> (TypeInfo, TypeInfo) {
        let mut key_info: Option<TypeInfo> = None;
        let mut val_info: Option<TypeInfo> = None;

        for child in children {
            let base_name = child.name().trim_end_matches('?');
            if base_name == "@key" {
                let key_arg = child.arg_value(0).and_then(|v| v.as_str()).unwrap_or("str");
                let is_opt = child.name().ends_with('?') || key_arg.ends_with('?');
                let clean_key = key_arg.trim_end_matches('?');
                let base_ti = match clean_key {
                    "int" => TypeInfo::int(),
                    "float" => TypeInfo::float(),
                    "bool" => TypeInfo::bool(),
                    _ => TypeInfo::string(),
                };
                key_info = Some(base_ti.with_optional(is_opt));
            } else if base_name == "@value" {
                let (explicit_model_name, is_arr) = extract_value_model_and_array(child);
                let is_opt = child.name().ends_with('?')
                    || child.ty().is_some_and(|t| t.ends_with('?'))
                    || child.args().iter().any(|arg| {
                        arg.value
                            .as_str()
                            .is_some_and(|s| !s.starts_with('@') && s.ends_with('?'))
                    });

                if child.has_children() {
                    let child_schema_name = synthesize_value_schema_name(
                        &explicit_model_name,
                        explicit_dict_name,
                        parent_json_name,
                        field_name,
                    );

                    // Post-order: recursively lower child fields first (hoisting any deeper nested blocks ahead of `child_schema_name`)
                    let mut sub_visiting = HashSet::new();
                    let sub_fields = self.lower_json_fields(
                        unit,
                        env,
                        &unit.source,
                        &child_schema_name,
                        child.children(),
                        &mut sub_visiting,
                    );

                    self.check_hoisted_collision(unit, child.span(), &child_schema_name);

                    let sub_json_def = JsonDef {
                        name: child_schema_name.clone(),
                        kind: JsonKind::Object,
                        doc: None,
                        is_array: false,
                        is_dict: false,
                        path: None,
                        parsed_path: None,
                        fields: sub_fields,
                        key_type: None,
                        value_type: None,
                        is_hoisted: true,
                        span: child.span(),
                    };
                    self.json_defs
                        .insert(child_schema_name.clone(), sub_json_def);

                    val_info =
                        Some(TypeInfo::json(child_schema_name, is_arr).with_optional(is_opt));
                } else {
                    let val_arg = if explicit_model_name.is_empty() {
                        "str"
                    } else {
                        explicit_model_name.as_str()
                    };
                    let base_ti = match val_arg {
                        "str" => TypeInfo::string(),
                        "int" => TypeInfo::int(),
                        "float" => TypeInfo::float(),
                        "bool" => TypeInfo::bool(),
                        "null" | "nil" => TypeInfo::null(),
                        other => TypeInfo::json(other, is_arr),
                    };
                    val_info = Some(base_ti.with_array(is_arr).with_optional(is_opt));
                }
            }
        }

        (
            key_info.unwrap_or_else(TypeInfo::string),
            val_info.unwrap_or_else(TypeInfo::string),
        )
    }

    fn lower_json_fields<'b>(
        &mut self,
        unit: &SourceUnit,
        env: &UnitEnv,
        source_text: &str,
        parent_json_name: &str,
        nodes: Vec<NodeRef<'b>>,
        visiting_defines: &mut HashSet<(String, NodeId)>,
    ) -> Vec<JsonFieldDef> {
        let mut fields = Vec::new();

        for node in nodes {
            let raw_name = node.name();
            if raw_name.is_empty() {
                continue;
            }

            // 1. Block `define` expansion inside `json`
            if node.args().is_empty() && node.props().is_empty() && !node.has_children_block() {
                if let Some(&DefineEntry::Block {
                    ref unit_path,
                    node_id,
                }) = env.defines.get(raw_name)
                {
                    let key = (unit_path.clone(), node_id);
                    if visiting_defines.insert(key.clone()) {
                        if let Some(def_unit) = self.program.units.get(unit_path) {
                            let def_node = def_unit.document.node(node_id);
                            let expanded = self.lower_json_fields(
                                unit,
                                env,
                                &def_unit.source,
                                parent_json_name,
                                def_node.children(),
                                visiting_defines,
                            );
                            fields.extend(expanded);
                        }
                        visiting_defines.remove(&key);
                        continue;
                    }
                }
            }

            let mut is_optional = raw_name.ends_with('?');
            let name = raw_name.trim_end_matches('?').to_owned();

            // 2. Inline `(dict)` field
            if is_dict_json_field(node) {
                let mut skip = false;
                let mut omitempty = false;
                let mut explicit_dict_name = String::new();

                for (idx, arg) in node.args().iter().enumerate() {
                    let Some(a) = arg.value.as_str() else {
                        continue;
                    };
                    let quoted = is_arg_quoted(source_text, node, idx);
                    if a == "@skip" && !quoted {
                        skip = true;
                    } else if a == "@omitempty" && !quoted {
                        omitempty = true;
                    } else if a.starts_with('@') && !quoted {
                        continue;
                    } else if explicit_dict_name.is_empty()
                        && a != "(dict)"
                        && a != "dict"
                        && !quoted
                    {
                        let mut clean = a;
                        if clean.ends_with('?') {
                            is_optional = true;
                            clean = clean.trim_end_matches('?');
                        }
                        if !clean.is_empty() {
                            explicit_dict_name = clean.to_owned();
                        }
                    }
                }

                let alias = extract_json_field_alias(node, env, None);
                let gjson_path = parse_field_gjson_path(&name, alias.as_deref());
                let doc = extract_json_field_doc(node, env);

                let (key_info, val_info) = self.lower_dict_json_directives(
                    unit,
                    env,
                    node.children(),
                    parent_json_name,
                    &name,
                    &explicit_dict_name,
                );

                let type_info = TypeInfo::dict(key_info.clone(), val_info.clone())
                    .with_optional(is_optional)
                    .with_omitempty(omitempty)
                    .with_skip(skip);

                fields.push(JsonFieldDef {
                    name,
                    type_info,
                    alias,
                    gjson_path,
                    doc,
                    is_optional,
                    omitempty,
                    skip,
                    is_dict: true,
                    key_type: Some(key_info),
                    value_type: Some(val_info),
                    value: None,
                    span: node.span(),
                });
                continue;
            }

            // 3. Inline object or array block
            if node.has_children() {
                let mut skip = false;
                let mut omitempty = false;
                let mut explicit_type = String::new();

                for (idx, arg) in node.args().iter().enumerate() {
                    let Some(a) = arg.value.as_str() else {
                        continue;
                    };
                    let quoted = is_arg_quoted(source_text, node, idx);
                    if a == "@skip" && !quoted {
                        skip = true;
                    } else if a == "@omitempty" && !quoted {
                        omitempty = true;
                    } else if a.starts_with('@') && !quoted {
                        continue;
                    } else if explicit_type.is_empty() {
                        explicit_type = a.to_owned();
                    }
                }

                let mut is_array = node.ty() == Some("array")
                    || node
                        .args()
                        .iter()
                        .any(|arg| arg.ty().unwrap_or("") == "array");
                if let Some(stripped) = explicit_type.strip_prefix("(array)") {
                    is_array = true;
                    explicit_type = stripped.to_owned();
                }
                if explicit_type.ends_with('?') {
                    is_optional = true;
                    explicit_type = explicit_type.trim_end_matches('?').to_owned();
                }

                let child_schema_name = if !explicit_type.is_empty() {
                    explicit_type
                } else {
                    format!("{parent_json_name}{}", to_pascal_case(&name))
                };

                // Post-order: recursively lower child fields first
                let mut sub_visiting = HashSet::new();
                let sub_fields = self.lower_json_fields(
                    unit,
                    env,
                    source_text,
                    &child_schema_name,
                    node.children(),
                    &mut sub_visiting,
                );

                self.check_hoisted_collision(unit, node.span(), &child_schema_name);

                let sub_json_def = JsonDef {
                    name: child_schema_name.clone(),
                    kind: JsonKind::Object,
                    doc: None,
                    is_array: false,
                    is_dict: false,
                    path: None,
                    parsed_path: None,
                    fields: sub_fields,
                    key_type: None,
                    value_type: None,
                    is_hoisted: true,
                    span: node.span(),
                };
                self.json_defs
                    .insert(child_schema_name.clone(), sub_json_def);

                let alias = extract_json_field_alias(node, env, None);
                let gjson_path = parse_field_gjson_path(&name, alias.as_deref());
                let doc = extract_json_field_doc(node, env);

                let type_info = TypeInfo::json(child_schema_name, is_array)
                    .with_optional(is_optional)
                    .with_omitempty(omitempty)
                    .with_skip(skip);

                fields.push(JsonFieldDef {
                    name,
                    type_info,
                    alias,
                    gjson_path,
                    doc,
                    is_optional,
                    omitempty,
                    skip,
                    is_dict: false,
                    key_type: None,
                    value_type: None,
                    value: None,
                    span: node.span(),
                });
                continue;
            }

            // 4. Scalar or reference field
            let mut skip = false;
            let mut omitempty = false;
            let mut type_str = String::new();
            let mut positional_alias: Option<String> = None;
            let mut is_array = node.ty() == Some("array");

            for (idx, arg) in node.args().iter().enumerate() {
                let Some(a) = arg.value.as_str() else {
                    continue;
                };
                let quoted = is_arg_quoted(source_text, node, idx);
                if a == "@skip" && !quoted {
                    skip = true;
                } else if a == "@omitempty" && !quoted {
                    omitempty = true;
                } else if a.starts_with('@') && !quoted {
                    continue;
                } else if type_str.is_empty() {
                    if arg.ty() == Some("array") {
                        is_array = true;
                    }
                    type_str = a.to_owned();
                } else if positional_alias.is_none() {
                    positional_alias = Some(a.to_owned());
                }
            }

            if type_str.is_empty() && skip {
                type_str = "str".to_owned();
            }
            if let Some(stripped) = type_str.strip_prefix("(array)") {
                is_array = true;
                type_str = stripped.to_owned();
            }
            if type_str.ends_with('?') {
                is_optional = true;
                type_str = type_str.trim_end_matches('?').to_owned();
            }

            let base_ti = match type_str.as_str() {
                "str" | "" => TypeInfo::string(),
                "int" => TypeInfo::int(),
                "float" => TypeInfo::float(),
                "bool" => TypeInfo::bool(),
                "null" | "nil" => TypeInfo::null(),
                other => TypeInfo::json(other, is_array),
            };
            let type_info = base_ti
                .with_array(is_array)
                .with_optional(is_optional)
                .with_omitempty(omitempty)
                .with_skip(skip);

            let alias = extract_json_field_alias(node, env, positional_alias.as_deref());
            let gjson_path = parse_field_gjson_path(&name, alias.as_deref());
            let doc = extract_json_field_doc(node, env);
            let value = node
                .prop("value")
                .and_then(|p| lower_scalar_kdl_value(&p.value, env).map(|(lit, _)| lit));

            fields.push(JsonFieldDef {
                name,
                type_info,
                alias,
                gjson_path,
                doc,
                is_optional,
                omitempty,
                skip,
                is_dict: false,
                key_type: None,
                value_type: None,
                value,
                span: node.span(),
            });
        }

        fields
    }

    fn check_hoisted_collision(&mut self, unit: &SourceUnit, span: Span, schema_name: &str) {
        if schema_name.is_empty() {
            return;
        }
        let collides = !self.all_declared_json_names.insert(schema_name.to_owned())
            || self.json_defs.contains_key(schema_name);
        if collides {
            let already_reported = self.diags.iter().any(|d| {
                d.code == DiagnosticCode::E001
                    && d.path == unit.path
                    && d.span == span
                    && d.message.contains(schema_name)
            });
            if !already_reported {
                self.diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E001,
                    span,
                    format!("duplicate json definition '{schema_name}'"),
                    format!("rename inline schema '{schema_name}' to avoid collision"),
                ));
            }
        }
    }

    fn lower_structs(&mut self) {
        let items: Vec<_> = self
            .program
            .items
            .iter()
            .filter(|i| i.symbol.kind == SymbolKind::Struct)
            .cloned()
            .collect();

        for item in items {
            let Some(unit) = self.program.units.get(&item.symbol.path) else {
                continue;
            };
            let env = UnitEnv::build(self.program, &unit.path);
            let node = unit.document.node(item.node_id);
            let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }

            let kind = resolve_struct_kind(node);
            let keep_order = node
                .prop("keep-order")
                .and_then(|p| p.value.as_bool())
                .unwrap_or(false);

            let start_type = kind.start_type();

            let mut doc: Option<String> = None;
            let mut init_fields: Vec<InitFieldDecl> = Vec::new();
            let mut init_types: IndexMap<String, TypeInfo> = IndexMap::new();
            let mut pre_validate: Option<Pipeline> = None;
            let mut checks: Vec<CheckDecl> = Vec::new();
            let mut split_doc: Option<Pipeline> = None;
            let mut key_pipeline: Option<Pipeline> = None;
            let mut value_pipeline: Option<Pipeline> = None;
            let mut table_pipeline: Option<Pipeline> = None;
            let mut rows_pipeline: Option<Pipeline> = None;
            let mut match_pipeline: Option<Pipeline> = None;
            let mut fields: Vec<FieldDecl> = Vec::new();
            let mut requests: Vec<RequestDecl> = Vec::new();
            let mut errors: Vec<ErrorResponseSpec> = Vec::new();
            let mut rest_endpoints: Vec<RestEndpointDecl> = Vec::new();

            // 1. Lower `@init` first so `@<name>` references in fields resolve their `TypeInfo`
            for child in node.children() {
                if child.name() == "@init" {
                    let empty_inits = IndexMap::new();
                    for init_child in child.children() {
                        let init_name = init_child.name().to_owned();
                        let pipeline = self.lower_field_pipeline(
                            unit,
                            &env,
                            init_child,
                            start_type.clone(),
                            &empty_inits,
                            false,
                        );
                        let ti = pipeline.output.clone();
                        init_types.insert(init_name.clone(), ti.clone());
                        init_fields.push(InitFieldDecl {
                            name: init_name,
                            pipeline,
                            type_info: ti,
                            span: init_child.span(),
                        });
                    }
                }
            }

            // 2. Lower remaining directives and fields
            for child in node.children() {
                let child_name = child.name();
                if child_name.is_empty() || child_name == "@init" {
                    continue;
                }

                if child_name == "@doc" {
                    if let Some(raw_doc) = child.arg_value(0).and_then(|v| v.as_str()) {
                        let cleaned = clean_doc(raw_doc);
                        if !cleaned.is_empty() {
                            doc = Some(cleaned);
                        }
                    }
                } else if child_name == "@pre-validate" {
                    pre_validate = Some(self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        start_type.clone(),
                        &init_types,
                        true,
                    ));
                } else if child_name == "@check" {
                    let check_name = child
                        .arg_value(0)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let pipeline = self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        start_type.clone(),
                        &init_types,
                        false,
                    );
                    if pipeline.output.base != BaseType::Bool
                        && pipeline.output.base != BaseType::Auto
                    {
                        self.diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E100,
                            child.span(),
                            format!(
                                "@check {check_name} must return BOOL, got {}",
                                pipeline.output.base.dsl_name()
                            ),
                            "finish the pipeline with 'to-bool' or a BOOL extension",
                        ));
                    }
                    checks.push(CheckDecl {
                        name: check_name,
                        pipeline,
                        span: child.span(),
                    });
                } else if child_name == "@split-doc" {
                    split_doc = Some(self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        start_type.clone(),
                        &init_types,
                        false,
                    ));
                } else if child_name == "@key" {
                    key_pipeline = Some(self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        TypeInfo::document(),
                        &init_types,
                        false,
                    ));
                } else if child_name == "@value" {
                    value_pipeline = Some(self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        TypeInfo::document(),
                        &init_types,
                        false,
                    ));
                } else if child_name == "@table" {
                    table_pipeline = Some(self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        TypeInfo::document(),
                        &init_types,
                        false,
                    ));
                } else if child_name == "@rows" {
                    rows_pipeline = Some(self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        TypeInfo::document(),
                        &init_types,
                        false,
                    ));
                } else if child_name == "@match" {
                    match_pipeline = Some(self.lower_pipeline(
                        unit,
                        &env,
                        child.children(),
                        TypeInfo::document(),
                        &init_types,
                        false,
                    ));
                } else if child_name == "@request" {
                    let spec = unit
                        .parsed_requests
                        .get(&child.id())
                        .cloned()
                        .unwrap_or_default();

                    let raw_method_name = child
                        .prop("name")
                        .and_then(|p| p.value.as_str())
                        .unwrap_or("");
                    let method_name = if raw_method_name.is_empty() {
                        "fetch".to_owned()
                    } else {
                        raw_method_name.to_owned()
                    };

                    let req_doc = child
                        .prop("doc")
                        .and_then(|p| p.value.as_str())
                        .map(|s| clean_doc(&env.resolve_str_arg(s)))
                        .filter(|s| !s.is_empty());

                    let response_path = child
                        .prop("response-path")
                        .and_then(|p| p.value.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);

                    let response_join = child
                        .prop("response-join")
                        .and_then(|p| p.value.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);

                    let response_schema = child
                        .prop("response")
                        .and_then(|p| p.value.as_str())
                        .map(|s| env.resolve_str_arg(s))
                        .filter(|s| !s.is_empty());

                    let placeholders = spec.placeholders();

                    requests.push(RequestDecl {
                        name: method_name.clone(),
                        doc: req_doc.clone(),
                        response_path: response_path.clone(),
                        response_join,
                        response_schema: response_schema.clone(),
                        spec: spec.clone(),
                        placeholders: placeholders.clone(),
                        span: child.span(),
                    });

                    if kind == StructKind::Rest {
                        let response_is_array = response_schema
                            .as_deref()
                            .and_then(|s| self.json_defs.get(s))
                            .is_some_and(|j| j.is_array);
                        let result_alias_name = format!("{}Result", to_pascal_case(&method_name));
                        rest_endpoints.push(RestEndpointDecl {
                            name: method_name,
                            doc: req_doc,
                            response_path,
                            response_schema,
                            response_is_array,
                            result_alias_name,
                            spec,
                            placeholders,
                            span: child.span(),
                        });
                    }
                } else if is_error_directive(child_name) {
                    if let Some(err_spec) = unit.parsed_errors.get(&child.id()) {
                        errors.push(err_spec.clone());
                    }
                } else if !child_name.starts_with('@') {
                    let pipeline = self.lower_field_pipeline(
                        unit,
                        &env,
                        child,
                        start_type.clone(),
                        &init_types,
                        kind == StructKind::Table,
                    );
                    let ti = pipeline.output.clone();
                    fields.push(FieldDecl {
                        name: child_name.to_owned(),
                        pipeline,
                        type_info: ti,
                        span: child.span(),
                    });
                }
            }

            let check_pipeline = checks.first().map(|c| c.pipeline.clone());

            let struct_decl = StructDecl {
                kind,
                name: name.to_owned(),
                doc,
                keep_order,
                init_fields,
                pre_validate,
                check_pipeline,
                checks,
                split_doc,
                key_pipeline,
                value_pipeline,
                table_pipeline,
                rows_pipeline,
                match_pipeline,
                fields,
                requests,
                errors,
                rest_endpoints,
                span: node.span(),
            };

            if kind == StructKind::Rest {
                let artifacts = synthesize_rest_artifacts(&struct_decl, &self.json_defs);
                self.rest_artifacts.extend(artifacts);
            } else {
                self.typedefs.push(synthesize_typedef(&struct_decl));
            }

            self.structs.push(struct_decl);
        }
    }

    fn lower_functions(&mut self) {
        let items: Vec<_> = self
            .program
            .items
            .iter()
            .filter(|i| i.symbol.kind == SymbolKind::Fn)
            .cloned()
            .collect();

        for item in items {
            let Some(unit) = self.program.units.get(&item.symbol.path) else {
                continue;
            };
            let env = UnitEnv::build(self.program, &unit.path);
            let node = unit.document.node(item.node_id);
            let Some(name) = node.arg_value(0).and_then(|v| v.as_str()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }

            let is_raw = node.ty() == Some("raw");
            let start_type = if is_raw {
                TypeInfo::string()
            } else {
                TypeInfo::document()
            };

            let mut doc: Option<String> = None;
            let mut pipeline_nodes = Vec::new();
            for child in node.children() {
                if child.name() == "@doc" {
                    if let Some(raw_doc) = child.arg_value(0).and_then(|v| v.as_str()) {
                        let cleaned = clean_doc(raw_doc);
                        if !cleaned.is_empty() {
                            doc = Some(cleaned);
                        }
                    }
                } else if !child.name().starts_with('@') {
                    pipeline_nodes.push(child);
                }
            }

            let empty_inits = IndexMap::new();
            let pipeline = self.lower_pipeline(
                unit,
                &env,
                pipeline_nodes,
                start_type.clone(),
                &empty_inits,
                false,
            );
            let return_type = pipeline.output.clone();

            self.functions.push(FunctionDef {
                name: name.to_owned(),
                is_raw,
                params: vec![FnParam {
                    name: "document".to_owned(),
                    type_info: start_type,
                }],
                return_type,
                pipeline,
                doc,
                span: node.span(),
            });
        }
    }

    fn lower_field_pipeline(
        &mut self,
        unit: &'a SourceUnit,
        env: &UnitEnv,
        field_node: NodeRef<'a>,
        start_type: TypeInfo,
        init_types: &IndexMap<String, TypeInfo>,
        is_table_field: bool,
    ) -> Pipeline {
        let field_name = field_node.name();
        let declared_type = if field_node.ty().is_none() && field_node.args().len() == 1 {
            crate::linter::structs::parse_postfix_field_type(&field_node.args()[0], field_name).ok()
        } else {
            None
        };

        let seed_value = if !is_table_field {
            field_node.prop("value").and_then(|prop| {
                lower_scalar_kdl_value(&prop.value, env).map(|(lit, ti)| (lit, ti, prop.span))
            })
        } else {
            None
        };

        self.lower_pipeline_inner(
            unit,
            env,
            field_node.children(),
            start_type,
            init_types,
            false,
            Some(FieldPipelineMeta {
                field_name,
                field_span: field_node.span(),
                declared_type,
                seed_value,
            }),
        )
    }

    fn lower_pipeline(
        &mut self,
        unit: &'a SourceUnit,
        env: &UnitEnv,
        raw_nodes: Vec<NodeRef<'a>>,
        start_type: TypeInfo,
        init_types: &IndexMap<String, TypeInfo>,
        is_pre_validate: bool,
    ) -> Pipeline {
        self.lower_pipeline_inner(
            unit,
            env,
            raw_nodes,
            start_type,
            init_types,
            is_pre_validate,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_pipeline_inner(
        &mut self,
        unit: &'a SourceUnit,
        env: &UnitEnv,
        raw_nodes: Vec<NodeRef<'a>>,
        start_type: TypeInfo,
        init_types: &IndexMap<String, TypeInfo>,
        is_pre_validate: bool,
        field_meta: Option<FieldPipelineMeta<'_>>,
    ) -> Pipeline {
        let expanded = crate::linter::structs::expand_defines(raw_nodes, self.program, env);
        let has_pipeline_ops = !expanded.is_empty();

        let (field_name, field_span, declared_type, seed_value) = match field_meta {
            Some(m) => (m.field_name, m.field_span, m.declared_type, m.seed_value),
            None => ("", Span::EMPTY, None, None),
        };

        let mut current = start_type.clone();
        let mut steps: Vec<TypedStep> = Vec::new();
        let mut fallback = None;
        let mut pending_fallback: Option<NodeRef<'a>> = None;

        if let Some((lit, lit_ti, lit_span)) = seed_value {
            steps.push(TypedStep {
                op: StepOp::Value { value: lit },
                input: start_type.clone(),
                output: lit_ti.clone(),
                span: lit_span,
            });
            current = lit_ti;
        }

        for node in expanded {
            let op_name = node.name();
            if op_name.is_empty() {
                continue;
            }

            // 1. `@<name>` precomputed field load
            if let Some(init_name) = op_name.strip_prefix('@') {
                let resolved_ti = init_types
                    .get(init_name)
                    .cloned()
                    .unwrap_or_else(TypeInfo::auto);
                steps.push(TypedStep {
                    op: StepOp::LoadInit {
                        name: init_name.to_owned(),
                    },
                    input: resolved_ti.clone(),
                    output: resolved_ti.clone(),
                    span: node.span(),
                });
                current = resolved_ti;
                continue;
            }

            // 2. `!Namespace.op` user-defined extension call
            if let Some(ext_ref) = op_name.strip_prefix('!') {
                let (ns, op) = ext_ref.split_once('.').unwrap_or((ext_ref, ""));
                let step_in = current.clone();
                let step_out = if let Some(ext_op) = self.extension_ops.get(ext_ref) {
                    if !ext_op.input.accepts(&current) {
                        self.diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E100,
                            node.span(),
                            format!(
                                "custom operation '{ext_ref}' does not accept {}; expected {}",
                                current.base.dsl_name(),
                                ext_op.input.expected_name()
                            ),
                            "",
                        ));
                    }
                    ext_op.resolve_return(&current)
                } else {
                    TypeInfo::auto()
                };

                steps.push(TypedStep {
                    op: StepOp::ExtensionCall {
                        namespace: ns.to_owned(),
                        name: op.to_owned(),
                        qualified_name: ext_ref.to_owned(),
                        bound_input: step_in.clone(),
                        bound_output: step_out.clone(),
                    },
                    input: step_in,
                    output: step_out.clone(),
                    span: node.span(),
                });
                current = step_out;
                continue;
            }

            // 3. `fallback` terminal literal (held until after optional field auto-cast)
            if op_name == "fallback" {
                pending_fallback = Some(node);
                continue;
            }

            // 4. `filter { ... }`
            if op_name == "filter" {
                if !current.is_array && current.base != BaseType::Auto {
                    self.diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        node.span(),
                        format!(
                            "'filter' requires a list type, got {}",
                            current.base.dsl_name()
                        ),
                        "use 'css-all', 'xpath-all', 're-all', or 'split' first",
                    ));
                }
                let cond = self.lower_predicate_block(node.children(), env);
                let step_in = current.clone();
                let mut step_out = current.clone();
                step_out.is_array = true;
                steps.push(TypedStep {
                    op: StepOp::Filter { cond },
                    input: step_in,
                    output: step_out.clone(),
                    span: node.span(),
                });
                current = step_out;
                continue;
            }

            // 5. `assert ["message"] { ... }`
            if op_name == "assert" {
                let message = node
                    .arg_value(0)
                    .and_then(|v| v.as_str())
                    .map(|s| env.resolve_str_arg(s))
                    .filter(|s| !s.is_empty());
                let cond = self.lower_predicate_block(node.children(), env);
                let step_in = current.clone();
                let step_out = if is_pre_validate && steps.is_empty() {
                    TypeInfo::null()
                } else {
                    current.clone()
                };
                steps.push(TypedStep {
                    op: StepOp::Assert { message, cond },
                    input: step_in,
                    output: step_out.clone(),
                    span: node.span(),
                });
                current = step_out;
                continue;
            }

            // 6. `match { ... }`
            if op_name == "match" {
                if !steps.is_empty() {
                    self.diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        node.span(),
                        "'match' must be the first operation in the field pipeline",
                        "",
                    ));
                } else if !base_type_compatible(&current.base, Some(&BaseType::Document)) {
                    self.diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        node.span(),
                        format!("'match' requires DOCUMENT, got {}", current.base.dsl_name()),
                        "",
                    ));
                }
                let cond = self.lower_predicate_block(node.children(), env);
                let step_in = current.clone();
                let step_out = TypeInfo::string();
                steps.push(TypedStep {
                    op: StepOp::Match { cond },
                    input: step_in,
                    output: step_out.clone(),
                    span: node.span(),
                });
                current = step_out;
                continue;
            }

            // 7. Guard: `to-bool` as the very first operation on a `DOCUMENT` pipeline
            if steps.is_empty() && op_name == "to-bool" && start_type.base == BaseType::Document {
                self.diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E100,
                    node.span(),
                    format!("'{op_name}' requires a preceding operation"),
                    "add a producer first, e.g. `css '.x'; text`",
                ));
                current = TypeInfo::bool();
                continue;
            }

            // 8. `nested <StructName>`
            if op_name == "nested" {
                let struct_name = node
                    .arg_value(0)
                    .and_then(|v| v.as_str())
                    .map(|s| env.resolve_str_arg(s))
                    .unwrap_or_default();
                let target_kind = self
                    .struct_kinds
                    .get(&struct_name)
                    .copied()
                    .unwrap_or(StructKind::Item);

                let step_in = current.clone();
                if target_kind == StructKind::Raw {
                    if current.is_array && current.base != BaseType::Auto {
                        self.diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E100,
                            node.span(),
                            format!(
                                "'nested' does not accept {}; expected DOCUMENT",
                                current.base.dsl_name()
                            ),
                            type_mismatch_hint("nested", &current.base, current.is_array),
                        ));
                    } else if current.base != BaseType::String && current.base != BaseType::Auto {
                        self.diags.push(make_error(
                            &unit.path,
                            DiagnosticCode::E100,
                            node.span(),
                            format!(
                                "'nested {struct_name}': target is a (raw)struct and requires STRING input, got {}",
                                current.base.dsl_name()
                            ),
                            "add 'text', 'raw', or 'attr' before 'nested' to extract a string",
                        ));
                    }
                } else if !base_type_compatible(&current.base, Some(&BaseType::Document))
                    || current.is_array
                {
                    self.diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        node.span(),
                        format!(
                            "'nested' does not accept {}; expected DOCUMENT",
                            current.base.dsl_name()
                        ),
                        type_mismatch_hint("nested", &current.base, current.is_array),
                    ));
                }

                let step_out = TypeInfo::nested(struct_name.clone(), target_kind.produces_array());
                steps.push(TypedStep {
                    op: StepOp::Nested { struct_name },
                    input: step_in,
                    output: step_out.clone(),
                    span: node.span(),
                });
                current = step_out;
                continue;
            }

            // 9. `jsonify <SchemaName> [path="..."]`
            if op_name == "jsonify" {
                let schema_name = node
                    .arg_value(0)
                    .and_then(|v| v.as_str())
                    .map(|s| env.resolve_str_arg(s))
                    .unwrap_or_default();
                let path = node
                    .prop("path")
                    .and_then(|p| p.value.as_str())
                    .map(|s| env.resolve_str_arg(s))
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty());
                let parsed_path = path.as_deref().and_then(|p| GjsonPath::parse(p).ok());

                if !base_type_compatible(&current.base, Some(&BaseType::String)) {
                    self.diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        node.span(),
                        format!(
                            "'jsonify' does not accept {}; expected STRING",
                            current.base.dsl_name()
                        ),
                        type_mismatch_hint("jsonify", &current.base, current.is_array),
                    ));
                }

                let is_array = self.json_defs.get(&schema_name).is_some_and(|j| j.is_array);
                let step_in = current.clone();
                let step_out = TypeInfo::json(schema_name.clone(), is_array);
                steps.push(TypedStep {
                    op: StepOp::Jsonify {
                        schema_name,
                        path,
                        parsed_path,
                    },
                    input: step_in,
                    output: step_out.clone(),
                    span: node.span(),
                });
                current = step_out;
                continue;
            }

            // 10. Built-in pipeline operation
            let Some(sig) = op_signature(op_name) else {
                current = TypeInfo::auto();
                continue;
            };

            let step_in = current.clone();
            let mut type_ok = true;

            if let Some(ref expected_base) = sig.accept {
                if !base_type_compatible(&current.base, Some(expected_base)) {
                    self.diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        node.span(),
                        format!(
                            "'{op_name}' does not accept {}; expected {}",
                            current.base.dsl_name(),
                            expected_base.dsl_name()
                        ),
                        type_mismatch_hint(op_name, &current.base, current.is_array),
                    ));
                    type_ok = false;
                }
            }

            if type_ok && sig.requires_list && !current.is_array && current.base != BaseType::Auto {
                let hint = type_mismatch_hint(op_name, &current.base, current.is_array);
                self.diags.push(make_error(
                    &unit.path,
                    DiagnosticCode::E100,
                    node.span(),
                    hint.clone(),
                    hint,
                ));
                type_ok = false;
            }

            let mut step_out = if type_ok {
                apply_op_sig(&sig, &current)
            } else {
                apply_op_sig(&sig, &TypeInfo::auto())
            };
            if op_name == "attr" && node.args().len() > 1 {
                step_out.is_array = true;
            }

            if let Some(step_op) = lower_builtin_step_op(node, env) {
                steps.push(TypedStep {
                    op: step_op,
                    input: step_in,
                    output: step_out.clone(),
                    span: node.span(),
                });
            }

            current = step_out;
        }

        // Auto-cast before terminal `fallback` when a postfix field `<type>` is declared
        if let Some(ref target_ti) = declared_type {
            if has_pipeline_ops && !steps.is_empty() {
                if target_ti.base == BaseType::Bool
                    && !target_ti.is_array
                    && (current.base != BaseType::Bool || current.is_array)
                    && current.base != BaseType::Auto
                {
                    let out_ti = TypeInfo::bool();
                    steps.push(TypedStep {
                        op: StepOp::ToBool,
                        input: current.clone(),
                        output: out_ti.clone(),
                        span: field_span,
                    });
                    current = out_ti;
                } else if target_ti.base == BaseType::Int
                    && current.base == BaseType::String
                    && current.is_array == target_ti.is_array
                {
                    let out_ti = TypeInfo::int().with_array(current.is_array);
                    steps.push(TypedStep {
                        op: StepOp::ToInt,
                        input: current.clone(),
                        output: out_ti.clone(),
                        span: field_span,
                    });
                    current = out_ti;
                } else if target_ti.base == BaseType::Float
                    && current.base == BaseType::String
                    && current.is_array == target_ti.is_array
                {
                    let out_ti = TypeInfo::float().with_array(current.is_array);
                    steps.push(TypedStep {
                        op: StepOp::ToFloat,
                        input: current.clone(),
                        output: out_ti.clone(),
                        span: field_span,
                    });
                    current = out_ti;
                }
            }
        }

        if let Some(fb_node) = pending_fallback {
            let (fb_lit, updated_ti) = check_and_lower_fallback(
                &unit.path,
                fb_node,
                env,
                !steps.is_empty(),
                &current,
                self.diags,
            );
            fallback = fb_lit;
            current = updated_ti;
        }

        if let Some(ref target_ti) = declared_type {
            if has_pipeline_ops {
                if current.base == BaseType::Auto {
                    current = target_ti.clone();
                } else if current.base != target_ti.base
                    || current.is_array != target_ti.is_array
                    || current.is_optional != target_ti.is_optional
                {
                    let hint = if current.base == BaseType::Document
                        && matches!(
                            target_ti.base,
                            BaseType::String | BaseType::Int | BaseType::Float
                        ) {
                        "add 'text', 'raw', or 'attr' to extract a string from DOCUMENT"
                    } else if current.is_optional != target_ti.is_optional {
                        if current.is_optional {
                            "use a nullable field type (e.g. 'str?') when using 'fallback #null'"
                        } else {
                            "add 'fallback #null' to produce an optional type, or remove '?' from the field type"
                        }
                    } else if current.is_array != target_ti.is_array {
                        if target_ti.is_array {
                            "use '(array)' selector/operation (e.g. 'css-all', 'split') or remove '(array)' from field type"
                        } else {
                            "pipeline produces a list; declare field type with '(array)' or reduce to scalar with 'first'/'join'"
                        }
                    } else {
                        ""
                    };

                    self.diags.push(make_error(
                        &unit.path,
                        DiagnosticCode::E100,
                        field_span,
                        format!(
                            "field '{field_name}' declared as {}, but pipeline produces {}",
                            target_ti.dsl_name(),
                            current.dsl_name(),
                        ),
                        hint,
                    ));
                }
            }
        }

        Pipeline {
            input: start_type,
            output: current,
            steps,
            fallback,
        }
    }

    fn lower_predicate_block(&self, nodes: Vec<NodeRef<'a>>, env: &UnitEnv) -> Cond {
        let expanded = crate::linter::structs::expand_defines(nodes, self.program, env);
        let exprs: Vec<Cond> = expanded
            .into_iter()
            .filter_map(|n| self.lower_single_predicate(n, env))
            .collect();
        Cond::all(exprs)
    }

    fn lower_single_predicate(&self, node: NodeRef<'a>, env: &UnitEnv) -> Option<Cond> {
        let name = node.name();
        match name {
            "and" => {
                let expanded =
                    crate::linter::structs::expand_defines(node.children(), self.program, env);
                let items = expanded
                    .into_iter()
                    .filter_map(|n| self.lower_single_predicate(n, env))
                    .collect();
                Some(Cond::And(items))
            }
            "or" => {
                let expanded =
                    crate::linter::structs::expand_defines(node.children(), self.program, env);
                let items = expanded
                    .into_iter()
                    .filter_map(|n| self.lower_single_predicate(n, env))
                    .collect();
                Some(Cond::Or(items))
            }
            "not" => {
                let inner = self.lower_predicate_block(node.children(), env);
                Some(Cond::Not(Box::new(inner)))
            }
            "eq" => Some(Cond::Atom(CondOp::Eq {
                values: collect_pred_values(node, env),
            })),
            "ne" => Some(Cond::Atom(CondOp::Ne {
                values: collect_pred_values(node, env),
            })),
            "in" => Some(Cond::Atom(CondOp::In {
                values: collect_pred_values(node, env),
            })),
            "gt" => Some(Cond::Atom(CondOp::Gt {
                value: node_int_arg(node, 0, env),
            })),
            "lt" => Some(Cond::Atom(CondOp::Lt {
                value: node_int_arg(node, 0, env),
            })),
            "ge" => Some(Cond::Atom(CondOp::Ge {
                value: node_int_arg(node, 0, env),
            })),
            "le" => Some(Cond::Atom(CondOp::Le {
                value: node_int_arg(node, 0, env),
            })),
            "starts" => Some(Cond::Atom(CondOp::Starts {
                values: collect_str_args(node, 0, env),
            })),
            "ends" => Some(Cond::Atom(CondOp::Ends {
                values: collect_str_args(node, 0, env),
            })),
            "contains" => Some(Cond::Atom(CondOp::Contains {
                values: collect_str_args(node, 0, env),
            })),
            "re" => Some(Cond::Atom(CondOp::Re {
                pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
            })),
            "re-any" => Some(Cond::Atom(CondOp::ReAny {
                pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
            })),
            "re-all" => Some(Cond::Atom(CondOp::ReAll {
                pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
            })),
            "css" => Some(Cond::Atom(CondOp::Css {
                query: node_str_arg(node, 0, env),
            })),
            "xpath" => Some(Cond::Atom(CondOp::Xpath {
                query: node_str_arg(node, 0, env),
            })),
            "has-attr" => Some(Cond::Atom(CondOp::HasAttr {
                attrs: collect_str_args(node, 0, env),
            })),
            "attr-eq" => Some(Cond::Atom(CondOp::AttrEq {
                name: node_str_arg(node, 0, env),
                values: collect_str_args(node, 1, env),
            })),
            "attr-ne" => Some(Cond::Atom(CondOp::AttrNe {
                name: node_str_arg(node, 0, env),
                values: collect_str_args(node, 1, env),
            })),
            "attr-in" => Some(Cond::Atom(CondOp::AttrIn {
                name: node_str_arg(node, 0, env),
                values: collect_str_args(node, 1, env),
            })),
            "attr-starts" => Some(Cond::Atom(CondOp::AttrStarts {
                name: node_str_arg(node, 0, env),
                values: collect_str_args(node, 1, env),
            })),
            "attr-ends" => Some(Cond::Atom(CondOp::AttrEnds {
                name: node_str_arg(node, 0, env),
                values: collect_str_args(node, 1, env),
            })),
            "attr-contains" => Some(Cond::Atom(CondOp::AttrContains {
                name: node_str_arg(node, 0, env),
                values: collect_str_args(node, 1, env),
            })),
            "attr-re" => Some(Cond::Atom(CondOp::AttrRe {
                name: node_str_arg(node, 0, env),
                pattern: normalize_regex_pattern(&node_str_arg(node, 1, env)),
            })),
            "text-eq" => Some(Cond::Atom(CondOp::TextEq {
                values: collect_str_args(node, 0, env),
            })),
            "text-ne" => Some(Cond::Atom(CondOp::TextNe {
                values: collect_str_args(node, 0, env),
            })),
            "text-in" => Some(Cond::Atom(CondOp::TextIn {
                values: collect_str_args(node, 0, env),
            })),
            "text-starts" => Some(Cond::Atom(CondOp::TextStarts {
                values: collect_str_args(node, 0, env),
            })),
            "text-ends" => Some(Cond::Atom(CondOp::TextEnds {
                values: collect_str_args(node, 0, env),
            })),
            "text-contains" => Some(Cond::Atom(CondOp::TextContains {
                values: collect_str_args(node, 0, env),
            })),
            "text-re" => Some(Cond::Atom(CondOp::TextRe {
                pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
            })),
            "len-eq" => Some(Cond::Atom(CondOp::LenEq {
                values: collect_int_args(node, env),
            })),
            "len-ne" => Some(Cond::Atom(CondOp::LenNe {
                values: collect_int_args(node, env),
            })),
            "len-gt" => Some(Cond::Atom(CondOp::LenGt {
                value: node_int_arg(node, 0, env),
            })),
            "len-lt" => Some(Cond::Atom(CondOp::LenLt {
                value: node_int_arg(node, 0, env),
            })),
            "len-ge" => Some(Cond::Atom(CondOp::LenGe {
                value: node_int_arg(node, 0, env),
            })),
            "len-le" => Some(Cond::Atom(CondOp::LenLe {
                value: node_int_arg(node, 0, env),
            })),
            "len-range" => Some(Cond::Atom(CondOp::LenRange {
                start: node_int_arg(node, 0, env),
                end: node_int_arg(node, 1, env),
            })),
            _ => None,
        }
    }
}

fn lower_builtin_step_op(node: NodeRef<'_>, env: &UnitEnv) -> Option<StepOp> {
    let name = node.name();
    match name {
        "css" => Some(StepOp::Css {
            queries: collect_selector_queries(node, env),
        }),
        "css-all" => Some(StepOp::CssAll {
            queries: collect_selector_queries(node, env),
        }),
        "xpath" => Some(StepOp::Xpath {
            queries: collect_selector_queries(node, env),
        }),
        "xpath-all" => Some(StepOp::XpathAll {
            queries: collect_selector_queries(node, env),
        }),
        "css-remove" => Some(StepOp::CssRemove {
            query: node_str_arg(node, 0, env),
        }),
        "xpath-remove" => Some(StepOp::XpathRemove {
            query: node_str_arg(node, 0, env),
        }),
        "text" => Some(StepOp::Text),
        "raw" => {
            let mode = match node.arg_value(0).and_then(|v| v.as_str()) {
                Some("inner") => RawMode::Inner,
                _ => RawMode::Outer,
            };
            Some(StepOp::Raw { mode })
        }
        "attr" => Some(StepOp::Attr {
            keys: collect_str_args(node, 0, env),
        }),
        "trim" => Some(StepOp::Trim {
            chars: node_str_arg(node, 0, env),
        }),
        "ltrim" => Some(StepOp::Ltrim {
            chars: node_str_arg(node, 0, env),
        }),
        "rtrim" => Some(StepOp::Rtrim {
            chars: node_str_arg(node, 0, env),
        }),
        "normalize-space" => Some(StepOp::NormalizeSpace),
        "rm-prefix" => Some(StepOp::RmPrefix {
            substr: node_str_arg(node, 0, env),
        }),
        "rm-suffix" => Some(StepOp::RmSuffix {
            substr: node_str_arg(node, 0, env),
        }),
        "rm-prefix-suffix" => Some(StepOp::RmPrefixSuffix {
            substr: node_str_arg(node, 0, env),
        }),
        "fmt" => Some(StepOp::Fmt {
            template: node_str_arg(node, 0, env),
        }),
        "repl" => {
            if node.has_children() {
                let mut replacements = IndexMap::new();
                for child in node.children() {
                    let k = env.resolve_str_arg(child.name());
                    let v = child
                        .arg_value(0)
                        .and_then(|val| val.as_str())
                        .map(|s| env.resolve_str_arg(s))
                        .unwrap_or_default();
                    replacements.insert(k, v);
                }
                Some(StepOp::ReplMap { replacements })
            } else {
                Some(StepOp::Repl {
                    old: node_str_arg(node, 0, env),
                    new: node_str_arg(node, 1, env),
                })
            }
        }
        "lower" => Some(StepOp::Lower),
        "upper" => Some(StepOp::Upper),
        "split" => Some(StepOp::Split {
            sep: node_str_arg(node, 0, env),
        }),
        "join" => Some(StepOp::Join {
            sep: node_str_arg(node, 0, env),
        }),
        "unescape" => Some(StepOp::Unescape),
        "re" => Some(StepOp::Re {
            pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
        }),
        "re-all" => Some(StepOp::ReAll {
            pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
        }),
        "re-sub" => Some(StepOp::ReSub {
            pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
            repl: node_str_arg(node, 1, env),
        }),
        "re-split" => Some(StepOp::ReSplit {
            pattern: normalize_regex_pattern(&node_str_arg(node, 0, env)),
        }),
        "index" => Some(StepOp::Index {
            index: node_int_arg(node, 0, env),
        }),
        "first" => Some(StepOp::Index { index: 0 }),
        "last" => Some(StepOp::Index { index: -1 }),
        "slice" => Some(StepOp::Slice {
            start: node_int_arg(node, 0, env),
            end: node_int_arg(node, 1, env),
        }),
        "len" => Some(StepOp::Len),
        "unique" => {
            let keep_order = node
                .prop("keep-order")
                .and_then(|p| p.value.as_bool())
                .unwrap_or(false);
            Some(StepOp::Unique { keep_order })
        }
        "to-int" => Some(StepOp::ToInt),
        "to-float" => Some(StepOp::ToFloat),
        "to-bool" => Some(StepOp::ToBool),
        _ => None,
    }
}

fn collect_selector_queries(node: NodeRef<'_>, env: &UnitEnv) -> Vec<String> {
    if node.has_children() {
        node.children()
            .into_iter()
            .map(|c| env.resolve_str_arg(c.name()))
            .collect()
    } else {
        vec![node_str_arg(node, 0, env)]
    }
}

fn node_str_arg(node: NodeRef<'_>, idx: usize, env: &UnitEnv) -> String {
    node.arg_value(idx)
        .and_then(|v| v.as_str())
        .map(|s| env.resolve_str_arg(s))
        .unwrap_or_default()
}

fn node_int_arg(node: NodeRef<'_>, idx: usize, env: &UnitEnv) -> i64 {
    let Some(arg) = node.arg(idx) else {
        return 0;
    };
    match &arg.value {
        KdlValue::Int(n) => *n as i64,
        KdlValue::String(s) => env.resolve_str_arg(s).parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

fn collect_str_args(node: NodeRef<'_>, start_idx: usize, env: &UnitEnv) -> Vec<String> {
    node.args()
        .iter()
        .skip(start_idx)
        .filter_map(|a| a.value.as_str().map(|s| env.resolve_str_arg(s)))
        .collect()
}

fn collect_int_args(node: NodeRef<'_>, env: &UnitEnv) -> Vec<i64> {
    node.args()
        .iter()
        .filter_map(|a| match &a.value {
            KdlValue::Int(n) => Some(*n as i64),
            KdlValue::String(s) => env.resolve_str_arg(s).parse::<i64>().ok(),
            _ => None,
        })
        .collect()
}

fn collect_pred_values(node: NodeRef<'_>, env: &UnitEnv) -> Vec<CondValue> {
    node.args()
        .iter()
        .map(|a| match &a.value {
            KdlValue::Int(n) => CondValue::Int(*n as i64),
            KdlValue::Float(f) => CondValue::Float(*f),
            KdlValue::Bool(b) => CondValue::Bool(*b),
            KdlValue::String(s) => CondValue::String(env.resolve_str_arg(s)),
            KdlValue::Null => CondValue::String(String::new()),
        })
        .collect()
}

fn resolve_struct_kind(node: NodeRef<'_>) -> StructKind {
    let raw = node
        .ty()
        .or_else(|| node.prop("type").and_then(|p| p.value.as_str()))
        .unwrap_or("item");
    raw.parse::<StructKind>().unwrap_or(StructKind::Item)
}

fn parse_extension_sig_arg(arg: &crate::kdl::KdlArg) -> Option<ExtensionTypeSig> {
    let raw_val = arg.value.as_str()?;
    let is_optional = raw_val.ends_with('?');
    let name = raw_val.trim_end_matches('?');
    let is_array = arg.ty() == Some("array");

    if name == "T" {
        return Some(ExtensionTypeSig {
            base: None,
            generic: Some("T".to_owned()),
            is_array: false,
            is_optional: false,
        });
    }

    let base = match name {
        "bool" => BaseType::Bool,
        "doc" => BaseType::Document,
        "float" => BaseType::Float,
        "int" => BaseType::Int,
        "str" => BaseType::String,
        _ => return None,
    };

    Some(ExtensionTypeSig {
        base: Some(base),
        generic: None,
        is_array,
        is_optional,
    })
}

fn parse_extension_target_block(target_node: NodeRef<'_>) -> ExtensionTargetBlock {
    let mut imports = Vec::new();
    let mut helpers = Vec::new();
    let mut emit = String::new();

    for child in target_node.children() {
        match child.name() {
            "import" => {
                if let Some(val) = child.arg_value(0).and_then(|v| v.as_str()) {
                    let alias = child
                        .prop("alias")
                        .and_then(|p| p.value.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);
                    imports.push(ExtensionImport {
                        value: val.to_owned(),
                        alias,
                    });
                }
            }
            "helper" => {
                if let Some(helper_name) = child.arg_value(0).and_then(|v| v.as_str()) {
                    let mut h_imports = Vec::new();
                    let mut h_source = String::new();
                    for entry in child.children() {
                        if entry.name() == "import" {
                            if let Some(val) = entry.arg_value(0).and_then(|v| v.as_str()) {
                                let alias = entry
                                    .prop("alias")
                                    .and_then(|p| p.value.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_owned);
                                h_imports.push(ExtensionImport {
                                    value: val.to_owned(),
                                    alias,
                                });
                            }
                        } else if entry.name() == "source" {
                            if let Some(src) = entry.arg_value(0).and_then(|v| v.as_str()) {
                                h_source = src.to_owned();
                            }
                        }
                    }
                    helpers.push(ExtensionHelper {
                        name: helper_name.to_owned(),
                        source: h_source,
                        imports: h_imports,
                    });
                }
            }
            "emit" => {
                if let Some(tmpl) = child.arg_value(0).and_then(|v| v.as_str()) {
                    emit = tmpl.to_owned();
                }
            }
            _ => {}
        }
    }

    ExtensionTargetBlock {
        language: target_node.name().to_owned(),
        emit,
        imports,
        helpers,
    }
}

struct FieldPipelineMeta<'a> {
    field_name: &'a str,
    field_span: Span,
    declared_type: Option<TypeInfo>,
    seed_value: Option<(LiteralValue, TypeInfo, Span)>,
}

fn lower_scalar_kdl_value(val: &KdlValue, env: &UnitEnv) -> Option<(LiteralValue, TypeInfo)> {
    match val {
        KdlValue::String(s) => Some((
            LiteralValue::String(env.resolve_str_arg(s)),
            TypeInfo::string(),
        )),
        KdlValue::Int(n) => Some((LiteralValue::Int(*n as i64), TypeInfo::int())),
        KdlValue::Float(f) => Some((LiteralValue::Float(*f), TypeInfo::float())),
        KdlValue::Bool(b) => Some((LiteralValue::Bool(*b), TypeInfo::bool())),
        KdlValue::Null => None,
    }
}

fn extract_json_field_alias(
    node: NodeRef<'_>,
    env: &UnitEnv,
    positional_alias: Option<&str>,
) -> Option<String> {
    if let Some(from_prop) = node.prop("from") {
        if let Some(s) = from_prop.value.as_str() {
            let resolved = env.resolve_str_arg(s);
            let trimmed = resolved.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    if let Some(path_prop) = node.prop("path") {
        if let Some(s) = path_prop.value.as_str() {
            let resolved = env.resolve_str_arg(s);
            let trimmed = resolved.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    positional_alias
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn parse_field_gjson_path(field_name: &str, alias: Option<&str>) -> GjsonPath {
    let raw_key = alias.unwrap_or(field_name);
    GjsonPath::parse(raw_key).unwrap_or_else(|_| GjsonPath {
        raw: raw_key.to_owned(),
        segments: Vec::new(),
        steps: vec![crate::gjson::GjsonStep::Key(raw_key.to_owned())],
    })
}

fn extract_json_field_doc(node: NodeRef<'_>, env: &UnitEnv) -> Option<String> {
    node.prop("doc")
        .and_then(|p| p.value.as_str())
        .map(|s| clean_doc(&env.resolve_str_arg(s)))
        .filter(|s| !s.is_empty())
}

fn synthesize_typedef(struct_decl: &StructDecl) -> StructTypeDef {
    let fields = if struct_decl.kind == StructKind::Dict {
        let mut out = Vec::new();
        if let Some(ref kp) = struct_decl.key_pipeline {
            out.push(TypeDefField {
                name: "key".to_owned(),
                type_info: kp.output.clone(),
            });
        }
        if let Some(ref vp) = struct_decl.value_pipeline {
            out.push(TypeDefField {
                name: "value".to_owned(),
                type_info: vp.output.clone(),
            });
        }
        out
    } else {
        struct_decl
            .fields
            .iter()
            .map(|f| TypeDefField {
                name: f.name.clone(),
                type_info: f.type_info.clone(),
            })
            .collect()
    };

    StructTypeDef {
        name: struct_decl.name.clone(),
        struct_kind: struct_decl.kind,
        fields,
    }
}

fn synthesize_rest_artifacts(
    struct_decl: &StructDecl,
    json_defs: &IndexMap<String, JsonDef>,
) -> Vec<RestArtifact> {
    let mut out = Vec::new();

    // 1. Unique error variant definitions
    let mut seen_variants = HashSet::new();
    let mut err_variant_names = Vec::new();
    for err in &struct_decl.errors {
        let cls_name = err_subclass_name(&struct_decl.name, err);
        if !seen_variants.insert(cls_name.clone()) {
            continue;
        }
        err_variant_names.push(cls_name.clone());
        let schema_is_array = json_defs.get(&err.schema_name).is_some_and(|j| j.is_array);
        out.push(RestArtifact::ErrorVariant(ResultVariantDef {
            struct_name: struct_decl.name.clone(),
            name: cls_name,
            status: err.status,
            schema_name: err.schema_name.clone(),
            schema_is_array,
        }));
    }

    // 2. Per-endpoint result union aliases
    for ep in &struct_decl.rest_endpoints {
        out.push(RestArtifact::ResultAlias(ResultAliasDef {
            struct_name: struct_decl.name.clone(),
            name: ep.result_alias_name.clone(),
            response_schema: ep.response_schema.clone(),
            response_is_array: ep.response_is_array,
            err_variants: err_variant_names.clone(),
        }));
    }

    // 3. Per-struct ordered error matcher list
    let mut seen_entries = HashSet::new();
    let mut entries = Vec::new();
    for err in &struct_decl.errors {
        let cls_name = err_subclass_name(&struct_decl.name, err);
        let key = (err.status, cls_name.clone());
        if !seen_entries.insert(key) {
            continue;
        }
        entries.push(MatcherEntry {
            status: err.status,
            required_keys: err.required_keys.clone(),
            conditions: err.conditions.clone(),
            factory_name: cls_name,
            error_schema: err.schema_name.clone(),
        });
    }

    out.push(RestArtifact::MatcherList(MatcherListDef {
        struct_name: struct_decl.name.clone(),
        entries,
    }));

    out
}

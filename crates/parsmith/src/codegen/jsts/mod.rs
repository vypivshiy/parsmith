//! TypeScript (`.ts`) and JavaScript (`.js` + `.d.ts`) target backend (`JsTsEmitter`).
//!
//! Supports all HTML (`item`, `list`, `dict`, `table`, `flat`, `fn`), Raw (`(raw)struct`, `(raw)fn`),
//! `extension` (`ts` with fallback to `js`), and allowlist JSON projector (`json`, `(array)json`,
//! `(dict)json`, `jsonify`) constructs over standard Web/DOM APIs (`DOMParser`, `Document`, `Element`,
//! `XPathResult`).

pub mod helpers;
pub mod rest;

use indexmap::IndexSet;

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, FieldDecl, FunctionDef, InitFieldDecl, JsonDef,
    JsonFieldDef, Module, Pipeline, RawMode, RequestDecl, RestArtifact, RestEndpointDecl, StepOp,
    StructDecl, StructKind, StructTypeDef, TypeInfo, TypedStep,
};
use crate::codegen::builder::{ModuleBuilder, emit_extension_call};
use crate::codegen::emitter::{LanguageEmitter, drive_module};
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{
    CodegenOptions, module_has_fetch, module_has_html, module_has_rest, module_has_table,
    module_uses_http,
};
use crate::diagnostic::Diagnostic;
use crate::gjson::classify_gjson_path;
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::{js_json_projector_name, to_camel_case, to_snake_case};

pub use helpers::{
    STD_ASSERT_JS, STD_ASSERT_TS, STD_CSS_REMOVE_JS, STD_CSS_REMOVE_TS, STD_FETCH_DTS,
    STD_FETCH_JS, STD_FETCH_KEY, STD_FETCH_TS, STD_HTTP_TRANSPORT_DTS, STD_HTTP_TRANSPORT_JS,
    STD_HTTP_TRANSPORT_KEY, STD_HTTP_TRANSPORT_TS, STD_JSON_PROJECT_JS, STD_JSON_PROJECT_TS,
    STD_LTRIM_CHARS_JS, STD_LTRIM_CHARS_TS, STD_NORMALIZE_TEXT_JS, STD_NORMALIZE_TEXT_TS,
    STD_PARSE_HTML_JS, STD_PARSE_HTML_TS, STD_RE_SEARCH_JS, STD_RE_SEARCH_TS, STD_REPL_MAP_JS,
    STD_REPL_MAP_TS, STD_REST_CORE_DTS, STD_REST_CORE_JS, STD_REST_CORE_KEY, STD_REST_CORE_TS,
    STD_RM_PREFIX_JS, STD_RM_PREFIX_TS, STD_RM_SUFFIX_JS, STD_RM_SUFFIX_TS, STD_RTRIM_CHARS_JS,
    STD_RTRIM_CHARS_TS, STD_TRIM_CHARS_JS, STD_TRIM_CHARS_TS, STD_UNESCAPE_JS, STD_UNESCAPE_TS,
    STD_UNMATCHED_TABLE_ROW, STD_XPATH_ALL_JS, STD_XPATH_ALL_TS, STD_XPATH_FIRST_JS,
    STD_XPATH_FIRST_TS, STD_XPATH_REMOVE_JS, STD_XPATH_REMOVE_TS, emit_jsdoc, is_js_ident,
    js_literal, js_pred_value, js_regex_literal, js_str, js_str_array,
};

/// Emission flavor for [`JsTsEmitter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsTsMode {
    /// Standalone TypeScript module (`.ts`) with inline `export interface` / `export type` models
    /// and typed parser classes/functions/helpers.
    TypeScript,
    /// Clean ESM JavaScript module (`.js`) without TypeScript type syntax.
    JavaScript,
    /// Companion TypeScript declaration file (`.d.ts`) paired with a `.js` module.
    Declaration,
}

impl JsTsMode {
    /// Returns `true` if this mode emits TypeScript syntax (`TypeScript` or `Declaration`).
    #[inline]
    pub const fn is_ts(self) -> bool {
        matches!(self, Self::TypeScript | Self::Declaration)
    }

    /// Returns `true` if this mode is `Declaration` (`.d.ts`).
    #[inline]
    pub const fn is_dts(self) -> bool {
        matches!(self, Self::Declaration)
    }
}

fn ts_key_type(key_type: Option<&TypeInfo>) -> &'static str {
    match key_type.map(|k| &k.base) {
        Some(BaseType::Int | BaseType::Float) => "number",
        _ => "string",
    }
}

/// TypeScript and JavaScript `LanguageEmitter` implementation.
pub struct JsTsEmitter {
    /// Code generation options.
    pub options: CodegenOptions,
    /// Output representation mode (`TypeScript`, `JavaScript`, or `Declaration`).
    pub mode: JsTsMode,
    /// Any diagnostics accumulated during code generation (e.g. missing `"js"`/`"ts"` target in `extension`).
    pub diagnostics: Vec<Diagnostic>,
}

impl JsTsEmitter {
    /// Creates a new `JsTsEmitter` for `options` and `mode`.
    pub fn new(options: CodegenOptions, mode: JsTsMode) -> Self {
        Self {
            options,
            mode,
            diagnostics: Vec::new(),
        }
    }

    /// Resolves a [`TypeInfo`] into a TypeScript type annotation (`string`, `number`, `T[]`, `Record<K, V>`, `T | null`).
    pub fn resolve_type(type_info: &TypeInfo, module: &Module) -> String {
        if type_info.is_dict {
            let key_type = ts_key_type(type_info.key_type.as_deref());
            let val_type = match type_info.value_type.as_deref() {
                Some(vt) => Self::resolve_type(vt, module),
                None => "any".to_owned(),
            };
            let mut res = format!("Record<{key_type}, {val_type}>");
            if type_info.is_array {
                res = Self::wrap_array(res);
            }
            return Self::wrap_optional(res, type_info.is_optional);
        }

        let mut base_str = match &type_info.base {
            BaseType::String => "string".to_owned(),
            BaseType::Int | BaseType::Float => "number".to_owned(),
            BaseType::Bool => "boolean".to_owned(),
            BaseType::Null => "null".to_owned(),
            BaseType::Document => {
                if type_info.is_array {
                    "Element[]".to_owned()
                } else {
                    "Document | Element".to_owned()
                }
            }
            BaseType::Nested(ref_name) => {
                let pascal = to_pascal_case(ref_name);
                if let Some(target_struct) = module.struct_decl(ref_name) {
                    match target_struct.kind {
                        StructKind::Flat => {
                            return Self::wrap_optional(
                                "string[]".to_owned(),
                                type_info.is_optional,
                            );
                        }
                        StructKind::List => {
                            return Self::wrap_optional(
                                format!("{pascal}Type[]"),
                                type_info.is_optional,
                            );
                        }
                        StructKind::Raw if target_struct.split_doc.is_some() => {
                            return Self::wrap_optional(
                                format!("{pascal}Type[]"),
                                type_info.is_optional,
                            );
                        }
                        _ => {}
                    }
                }
                format!("{pascal}Type")
            }
            BaseType::Json(ref_name) => {
                let pascal = to_pascal_case(ref_name);
                let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
                if type_info.is_array || def_is_arr {
                    return Self::wrap_optional(format!("{pascal}Json[]"), type_info.is_optional);
                }
                format!("{pascal}Json")
            }
            BaseType::Auto | BaseType::DictValue => "any".to_owned(),
        };

        if type_info.is_array && type_info.base != BaseType::Document {
            base_str = Self::wrap_array(base_str);
        }
        Self::wrap_optional(base_str, type_info.is_optional)
    }

    fn wrap_array(ty: String) -> String {
        if ty.contains(" | ") {
            format!("({ty})[]")
        } else {
            format!("{ty}[]")
        }
    }

    fn wrap_optional(ty: String, is_optional: bool) -> String {
        if is_optional && ty != "null" && ty != "any" && !ty.ends_with(" | null") {
            format!("{ty} | null")
        } else {
            ty
        }
    }

    /// Resolves the return type annotation of `.parse()` for a [`StructDecl`].
    pub fn resolve_parse_return_type(struct_decl: &StructDecl) -> String {
        let name = to_pascal_case(&struct_decl.name);
        match struct_decl.kind {
            StructKind::Item | StructKind::Dict | StructKind::Table => format!("{name}Type"),
            StructKind::List => format!("{name}Type[]"),
            StructKind::Flat => "string[]".to_owned(),
            StructKind::Raw => {
                if struct_decl.split_doc.is_some() {
                    format!("{name}Type[]")
                } else {
                    format!("{name}Type")
                }
            }
            StructKind::Rest => "void".to_owned(),
        }
    }

    fn require_std_helper(
        &self,
        name: &str,
        ts_code: &str,
        js_code: &str,
        builder: &mut ModuleBuilder,
    ) {
        let code = if self.mode.is_ts() { ts_code } else { js_code };
        builder.require_std(name, code);
    }

    fn transform_json_value_expr(
        &self,
        val_expr: &str,
        field: &JsonFieldDef,
        target: &str,
        module: &Module,
    ) -> String {
        let info = &field.type_info;
        let target_lit = js_str(target);
        let as_any = if self.mode.is_ts() { " as any" } else { "" };

        if field.is_dict || info.is_dict {
            let f_val_info = field.value_type.as_ref().or(info.value_type.as_deref());
            if let Some(v_info) = f_val_info {
                if let BaseType::Json(ref child_ref) = v_info.base {
                    let child_proj = js_json_projector_name(child_ref);
                    let child_is_arr = module.json_def(child_ref).is_some_and(|j| j.is_array);
                    if v_info.is_array && !child_is_arr {
                        return format!(
                            "Object.fromEntries(Object.entries(rtExpectObject({val_expr}, {target_lit})).map(([k, v]) => [k, v != null ? rtExpectArray(v, {target_lit}).filter((x) => x != null).map((x) => {child_proj}(x)) : null])){as_any}"
                        );
                    }
                    return format!(
                        "Object.fromEntries(Object.entries(rtExpectObject({val_expr}, {target_lit})).map(([k, v]) => [k, v != null ? {child_proj}(v) : null])){as_any}"
                    );
                }
                if v_info.is_array {
                    return format!(
                        "Object.fromEntries(Object.entries(rtExpectObject({val_expr}, {target_lit})).map(([k, v]) => [k, v != null ? rtExpectArray(v, {target_lit}) : null])){as_any}"
                    );
                }
            }
            return format!("{{ ...rtExpectObject({val_expr}, {target_lit}) }}{as_any}");
        }

        if let BaseType::Json(ref child_ref) = info.base {
            let child_proj = js_json_projector_name(child_ref);
            let child_is_arr = module.json_def(child_ref).is_some_and(|j| j.is_array);
            if info.is_array && !child_is_arr {
                return format!(
                    "rtExpectArray({val_expr}, {target_lit}).filter((x) => x != null).map((x) => {child_proj}(x))"
                );
            }
            return format!("{child_proj}({val_expr})");
        }

        if info.is_array {
            return format!("rtExpectArray({val_expr}, {target_lit})");
        }

        val_expr.to_owned()
    }

    fn emit_json_projector(&self, json_def: &JsonDef, module: &Module, writer: &mut CodeWriter) {
        let fn_name = js_json_projector_name(&json_def.name);
        let pascal = to_pascal_case(&json_def.name);
        let schema_lit = js_str(&pascal);
        let is_ts = self.mode.is_ts();
        let as_schema = if is_ts {
            format!(" as {pascal}Json")
        } else {
            String::new()
        };

        if json_def.is_dict {
            if is_ts {
                wln!(
                    writer,
                    "export function {fn_name}(data: unknown): {pascal}Json {{"
                );
            } else {
                wln!(writer, "export function {fn_name}(data) {{");
            }
            writer.indented(|writer| {
                if let Some(ref val_info) = json_def.value_type {
                    if let BaseType::Json(ref child_ref) = val_info.base {
                        let child_proj = js_json_projector_name(child_ref);
                        let child_is_arr = module.json_def(child_ref).is_some_and(|j| j.is_array);
                        if val_info.is_array && !child_is_arr {
                            wln!(writer,
                                "return Object.fromEntries(Object.entries(rtExpectObject(data, {schema_lit})).map(([k, v]) => [k, v != null ? rtExpectArray(v, {schema_lit}).filter((x) => x != null).map((x) => {child_proj}(x)) : null])){as_schema};"
                            );
                        } else {
                            wln!(writer,
                                "return Object.fromEntries(Object.entries(rtExpectObject(data, {schema_lit})).map(([k, v]) => [k, v != null ? {child_proj}(v) : null])){as_schema};"
                            );
                        }
                    } else if val_info.is_array {
                        wln!(writer,
                            "return Object.fromEntries(Object.entries(rtExpectObject(data, {schema_lit})).map(([k, v]) => [k, v != null ? rtExpectArray(v, {schema_lit}) : null])){as_schema};"
                        );
                    } else {
                        wln!(writer,
                            "return {{ ...rtExpectObject(data, {schema_lit}) }}{as_schema};"
                        );
                    }
                } else {
                    wln!(writer,
                        "return {{ ...rtExpectObject(data, {schema_lit}) }}{as_schema};"
                    );
                }
            });
            writer.writeln("}");
            writer.blankline();
            return;
        }

        let active_fields: Vec<&JsonFieldDef> =
            json_def.fields.iter().filter(|f| !f.skip).collect();
        let has_omitempty = active_fields.iter().any(|f| f.omitempty);
        let needs_temp_var = active_fields.iter().any(|f| {
            f.omitempty
                || (f.is_optional && crate::codegen::python::json::json_field_needs_transform(f))
        });

        let mut obj_entries: Vec<String> = Vec::new();
        let mut omitempty_lines: Vec<String> = Vec::new();

        for field in &active_fields {
            let canonical = &field.name;
            let wire_path = field.alias.as_deref().unwrap_or(&field.name);
            let target = format!("{pascal}.{canonical}");
            let (is_direct_key, unescaped_key) = classify_gjson_path(wire_path);
            let flags_suffix = if field.omitempty {
                format!(
                    ", {}, true",
                    if field.is_optional { "true" } else { "false" }
                )
            } else if field.is_optional {
                ", true".to_owned()
            } else {
                String::new()
            };

            let extract_expr = if is_direct_key {
                let field_target = if unescaped_key == *canonical {
                    pascal.as_str()
                } else {
                    target.as_str()
                };
                format!(
                    "rtJsonField(data, {}, {}{flags_suffix})",
                    js_str(&unescaped_key),
                    js_str(field_target)
                )
            } else {
                format!(
                    "rtJsonPath(data, {}, {}{flags_suffix})",
                    js_str(wire_path),
                    js_str(&target)
                )
            };

            let needs_transform = crate::codegen::python::json::json_field_needs_transform(field);
            let obj_key = if is_js_ident(canonical) {
                canonical.clone()
            } else {
                js_str(canonical)
            };
            let prop_target = if is_js_ident(canonical) {
                format!("result.{canonical}")
            } else {
                format!("result[{}]", js_str(canonical))
            };

            if field.omitempty {
                let val_expr = if needs_transform {
                    self.transform_json_value_expr("_v", field, &target, module)
                } else {
                    "_v".to_owned()
                };
                omitempty_lines.push(format!(
                    "if ((_v = {extract_expr}) !== undefined) {prop_target} = {val_expr};"
                ));
            } else if field.is_optional && needs_transform {
                let val_expr = self.transform_json_value_expr("_v", field, &target, module);
                obj_entries.push(format!(
                    "{obj_key}: (_v = {extract_expr}) != null ? {val_expr} : null,"
                ));
            } else {
                let val_expr = if needs_transform {
                    self.transform_json_value_expr(&extract_expr, field, &target, module)
                } else {
                    extract_expr
                };
                obj_entries.push(format!("{obj_key}: {val_expr},"));
            }
        }

        let emit_object_body = |writer: &mut CodeWriter| {
            wln!(writer, "rtExpectObject(data, {schema_lit});");
            if needs_temp_var {
                if is_ts {
                    writer.writeln("let _v: any;");
                } else {
                    writer.writeln("let _v;");
                }
            }
            if has_omitempty {
                if obj_entries.is_empty() {
                    if is_ts {
                        wln!(writer, "const result: {pascal}Json = {{}};");
                    } else {
                        writer.writeln("const result = {};");
                    }
                } else {
                    if is_ts {
                        wln!(writer, "const result: {pascal}Json = {{");
                    } else {
                        writer.writeln("const result = {");
                    }
                    writer.indented(|writer| {
                        for entry in &obj_entries {
                            writer.writeln(entry);
                        }
                    });
                    writer.writeln("};");
                }
                for line in &omitempty_lines {
                    writer.writeln(line);
                }
                writer.writeln("return result;");
            } else if obj_entries.is_empty() {
                writer.writeln("return {};");
            } else {
                writer.writeln("return {");
                writer.indented(|writer| {
                    for entry in &obj_entries {
                        writer.writeln(entry);
                    }
                });
                writer.writeln("};");
            }
        };

        if json_def.is_array {
            if is_ts {
                wln!(
                    writer,
                    "export function {fn_name}(items: unknown): {pascal}Json[] {{"
                );
            } else {
                wln!(writer, "export function {fn_name}(items) {{");
            }
            writer.indented(|writer| {
                if is_ts {
                    wln!(
                        writer,
                        "return rtExpectArray(items, {schema_lit}).map((data: any) => {{"
                    );
                } else {
                    wln!(
                        writer,
                        "return rtExpectArray(items, {schema_lit}).map((data) => {{"
                    );
                }
                writer.indented(emit_object_body);
                writer.writeln("});");
            });
            writer.writeln("}");
            writer.blankline();
        } else {
            if is_ts {
                wln!(
                    writer,
                    "export function {fn_name}(data: unknown): {pascal}Json {{"
                );
            } else {
                wln!(writer, "export function {fn_name}(data) {{");
            }
            writer.indented(emit_object_body);
            writer.writeln("}");
            writer.blankline();
        }
    }

    /// Emits a complete [`Pipeline`] body inside a method or function.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_pipeline_body(
        &mut self,
        pipeline: &Pipeline,
        initial_var: &str,
        location: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref fb_lit) = pipeline.fallback {
            writer.writeln("try {");
            writer.indented(|writer| {
                let final_var = self.emit_pipeline_steps(
                    &pipeline.steps,
                    initial_var,
                    location,
                    module,
                    builder,
                    writer,
                );
                if is_pre_validate {
                    writer.writeln("return;");
                } else {
                    wln!(writer, "return {final_var};");
                }
            });
            writer.writeln("} catch (_e) {");
            let fb_str = js_literal(fb_lit);
            writer.indented(|writer| {
                if is_pre_validate {
                    writer.writeln("return;");
                } else {
                    wln!(writer, "return {fb_str};");
                }
            });
            writer.writeln("}");
        } else {
            let final_var = self.emit_pipeline_steps(
                &pipeline.steps,
                initial_var,
                location,
                module,
                builder,
                writer,
            );
            if is_pre_validate {
                writer.writeln("return;");
            } else {
                wln!(writer, "return {final_var};");
            }
        }
    }

    fn emit_pipeline_steps(
        &mut self,
        steps: &[TypedStep],
        initial_var: &str,
        location: &str,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) -> String {
        let mut cur_var = initial_var.to_owned();
        for (idx, step) in steps.iter().enumerate() {
            let nxt_var = writer.next_var();
            let next_is_to_bool = steps
                .get(idx + 1)
                .is_some_and(|next| matches!(next.op, StepOp::ToBool));
            self.emit_step(
                step,
                &cur_var,
                &nxt_var,
                next_is_to_bool,
                location,
                module,
                builder,
                writer,
            );
            cur_var = nxt_var;
        }
        cur_var
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_step(
        &mut self,
        step: &TypedStep,
        prv: &str,
        nxt: &str,
        next_is_to_bool: bool,
        location: &str,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_arr_in = step.input.is_array;
        let is_arr_out = step.output.is_array;
        let is_ts = self.mode.is_ts();

        match &step.op {
            StepOp::LoadInit { name } => {
                let camel = to_camel_case(name);
                wln!(writer, "let {nxt} = this._{camel};");
            }
            StepOp::Css { queries } => {
                for (i, query) in queries.iter().enumerate() {
                    let q = js_str(query);
                    if i == 0 {
                        wln!(writer, "let {nxt} = {prv}.querySelector({q});");
                    } else {
                        wln!(
                            writer,
                            "if ({nxt} === null) {nxt} = {prv}.querySelector({q});"
                        );
                    }
                }
                if !next_is_to_bool {
                    wln!(
                        writer,
                        "if ({nxt} === null) throw new Error(\"CSS selector matched no element\");"
                    );
                }
            }
            StepOp::CssAll { queries } => {
                for (i, query) in queries.iter().enumerate() {
                    let q = js_str(query);
                    if i == 0 {
                        wln!(
                            writer,
                            "let {nxt} = Array.from({prv}.querySelectorAll({q}));"
                        );
                    } else {
                        wln!(
                            writer,
                            "if ({nxt}.length === 0) {nxt} = Array.from({prv}.querySelectorAll({q}));"
                        );
                    }
                }
            }
            StepOp::CssRemove { query } => {
                self.require_std_helper(
                    "rtCssRemove",
                    STD_CSS_REMOVE_TS,
                    STD_CSS_REMOVE_JS,
                    builder,
                );
                let q = js_str(query);
                if is_arr_in {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((el) => rtCssRemove(el, {q}));"
                    );
                } else {
                    wln!(writer, "let {nxt} = rtCssRemove({prv}, {q});");
                }
            }
            StepOp::Xpath { queries } => {
                self.require_std_helper(
                    "rtXpathFirst",
                    STD_XPATH_FIRST_TS,
                    STD_XPATH_FIRST_JS,
                    builder,
                );
                let args = queries
                    .iter()
                    .map(|q| js_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(writer, "let {nxt} = rtXpathFirst({prv}, {args});");
                if !next_is_to_bool {
                    wln!(
                        writer,
                        "if ({nxt} === null) throw new Error(\"XPath expression matched no element\");"
                    );
                }
            }
            StepOp::XpathAll { queries } => {
                self.require_std_helper("rtXpathAll", STD_XPATH_ALL_TS, STD_XPATH_ALL_JS, builder);
                let args = queries
                    .iter()
                    .map(|q| js_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(writer, "let {nxt} = rtXpathAll({prv}, {args});");
            }
            StepOp::XpathRemove { query } => {
                self.require_std_helper(
                    "rtXpathRemove",
                    STD_XPATH_REMOVE_TS,
                    STD_XPATH_REMOVE_JS,
                    builder,
                );
                let q = js_str(query);
                if is_arr_in {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((el) => rtXpathRemove(el, {q}));"
                    );
                } else {
                    wln!(writer, "let {nxt} = rtXpathRemove({prv}, {q});");
                }
            }
            StepOp::Text => {
                if is_arr_in {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((el) => el.textContent ?? \"\");"
                    );
                } else {
                    wln!(writer, "let {nxt} = {prv}.textContent ?? \"\";");
                }
            }
            StepOp::Raw { mode } => {
                let prop = if *mode == RawMode::Inner {
                    "innerHTML"
                } else {
                    "outerHTML"
                };
                if is_arr_in {
                    wln!(writer, "let {nxt} = {prv}.map((el) => el.{prop});");
                } else if is_ts {
                    wln!(writer, "let {nxt} = ({prv} as Element).{prop};");
                } else {
                    wln!(writer, "let {nxt} = {prv}.{prop};");
                }
            }
            StepOp::Attr { keys } => {
                if !is_arr_in {
                    if keys.len() == 1 {
                        let k = js_str(&keys[0]);
                        if is_ts {
                            wln!(
                                writer,
                                "let {nxt} = ({prv} as Element).getAttribute({k}) ?? \"\";"
                            );
                        } else {
                            wln!(writer, "let {nxt} = {prv}.getAttribute({k}) ?? \"\";");
                        }
                    } else {
                        let kl = js_str_array(keys);
                        if is_ts {
                            wln!(
                                writer,
                                "let {nxt} = {kl}.map((k) => ({prv} as Element).getAttribute(k)).filter((v): v is string => Boolean(v));"
                            );
                        } else {
                            wln!(
                                writer,
                                "let {nxt} = {kl}.map((k) => {prv}.getAttribute(k)).filter(Boolean);"
                            );
                        }
                    }
                } else if keys.len() == 1 {
                    let k = js_str(&keys[0]);
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((el) => el.getAttribute({k}) ?? \"\");"
                    );
                } else {
                    let kl = js_str_array(keys);
                    if is_ts {
                        wln!(
                            writer,
                            "let {nxt} = {prv}.flatMap((el) => {kl}.map((k) => el.getAttribute(k)).filter((v): v is string => Boolean(v)));"
                        );
                    } else {
                        wln!(
                            writer,
                            "let {nxt} = {prv}.flatMap((el) => {kl}.map((k) => el.getAttribute(k)).filter(Boolean));"
                        );
                    }
                }
            }
            StepOp::ToBool => {
                if is_arr_in {
                    wln!(writer, "let {nxt} = {prv}.length > 0;");
                } else {
                    wln!(writer, "let {nxt} = Boolean({prv});");
                }
            }
            StepOp::Trim { chars } => {
                if chars.is_empty() {
                    if is_arr_out {
                        wln!(writer, "let {nxt} = {prv}.map((s) => s.trim());");
                    } else {
                        wln!(writer, "let {nxt} = {prv}.trim();");
                    }
                } else {
                    self.require_std_helper(
                        "rtTrimChars",
                        STD_TRIM_CHARS_TS,
                        STD_TRIM_CHARS_JS,
                        builder,
                    );
                    let c = js_str(chars);
                    if is_arr_out {
                        wln!(writer, "let {nxt} = {prv}.map((s) => rtTrimChars(s, {c}));");
                    } else {
                        wln!(writer, "let {nxt} = rtTrimChars({prv}, {c});");
                    }
                }
            }
            StepOp::Ltrim { chars } => {
                if chars.is_empty() {
                    if is_arr_out {
                        wln!(writer, "let {nxt} = {prv}.map((s) => s.trimStart());");
                    } else {
                        wln!(writer, "let {nxt} = {prv}.trimStart();");
                    }
                } else {
                    self.require_std_helper(
                        "rtLtrimChars",
                        STD_LTRIM_CHARS_TS,
                        STD_LTRIM_CHARS_JS,
                        builder,
                    );
                    let c = js_str(chars);
                    if is_arr_out {
                        wln!(
                            writer,
                            "let {nxt} = {prv}.map((s) => rtLtrimChars(s, {c}));"
                        );
                    } else {
                        wln!(writer, "let {nxt} = rtLtrimChars({prv}, {c});");
                    }
                }
            }
            StepOp::Rtrim { chars } => {
                if chars.is_empty() {
                    if is_arr_out {
                        wln!(writer, "let {nxt} = {prv}.map((s) => s.trimEnd());");
                    } else {
                        wln!(writer, "let {nxt} = {prv}.trimEnd();");
                    }
                } else {
                    self.require_std_helper(
                        "rtRtrimChars",
                        STD_RTRIM_CHARS_TS,
                        STD_RTRIM_CHARS_JS,
                        builder,
                    );
                    let c = js_str(chars);
                    if is_arr_out {
                        wln!(
                            writer,
                            "let {nxt} = {prv}.map((s) => rtRtrimChars(s, {c}));"
                        );
                    } else {
                        wln!(writer, "let {nxt} = rtRtrimChars({prv}, {c});");
                    }
                }
            }
            StepOp::NormalizeSpace => {
                self.require_std_helper(
                    "rtNormalizeText",
                    STD_NORMALIZE_TEXT_TS,
                    STD_NORMALIZE_TEXT_JS,
                    builder,
                );
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => rtNormalizeText(s));");
                } else {
                    wln!(writer, "let {nxt} = rtNormalizeText({prv});");
                }
            }
            StepOp::RmPrefix { substr } => {
                self.require_std_helper("rtRmPrefix", STD_RM_PREFIX_TS, STD_RM_PREFIX_JS, builder);
                let v = js_str(substr);
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => rtRmPrefix(s, {v}));");
                } else {
                    wln!(writer, "let {nxt} = rtRmPrefix({prv}, {v});");
                }
            }
            StepOp::RmSuffix { substr } => {
                self.require_std_helper("rtRmSuffix", STD_RM_SUFFIX_TS, STD_RM_SUFFIX_JS, builder);
                let v = js_str(substr);
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => rtRmSuffix(s, {v}));");
                } else {
                    wln!(writer, "let {nxt} = rtRmSuffix({prv}, {v});");
                }
            }
            StepOp::RmPrefixSuffix { substr } => {
                self.require_std_helper("rtRmPrefix", STD_RM_PREFIX_TS, STD_RM_PREFIX_JS, builder);
                self.require_std_helper("rtRmSuffix", STD_RM_SUFFIX_TS, STD_RM_SUFFIX_JS, builder);
                let v = js_str(substr);
                if is_arr_out {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((s) => rtRmSuffix(rtRmPrefix(s, {v}), {v}));"
                    );
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rtRmSuffix(rtRmPrefix({prv}, {v}), {v});"
                    );
                }
            }
            StepOp::Fmt { template } => {
                let escaped = template
                    .replace('\\', "\\\\")
                    .replace('`', "\\`")
                    .replace("${", "\\${")
                    .replace("{{}}", "${_v}");
                let js_tmpl = format!("`{escaped}`");
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((_v) => {js_tmpl});");
                } else if is_ts {
                    wln!(writer, "let {nxt} = ((_v: string) => {js_tmpl})({prv});");
                } else {
                    wln!(writer, "let {nxt} = ((_v) => {js_tmpl})({prv});");
                }
            }
            StepOp::Repl { old, new } => {
                let old_s = js_str(old);
                let new_s = js_str(new);
                if is_arr_out {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((s) => s.replaceAll({old_s}, {new_s}));"
                    );
                } else {
                    wln!(writer, "let {nxt} = {prv}.replaceAll({old_s}, {new_s});");
                }
            }
            StepOp::ReplMap { replacements } => {
                self.require_std_helper("rtReplMap", STD_REPL_MAP_TS, STD_REPL_MAP_JS, builder);
                let entries = replacements
                    .iter()
                    .map(|(k, v)| format!("{}: {}", js_str(k), js_str(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let map_lit = format!("{{{entries}}}");
                if is_arr_out {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((s) => rtReplMap(s, {map_lit}));"
                    );
                } else {
                    wln!(writer, "let {nxt} = rtReplMap({prv}, {map_lit});");
                }
            }
            StepOp::Lower => {
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => s.toLowerCase());");
                } else {
                    wln!(writer, "let {nxt} = {prv}.toLowerCase();");
                }
            }
            StepOp::Upper => {
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => s.toUpperCase());");
                } else {
                    wln!(writer, "let {nxt} = {prv}.toUpperCase();");
                }
            }
            StepOp::Split { sep } => {
                let sep_s = js_str(sep);
                wln!(writer, "let {nxt} = {prv}.split({sep_s});");
            }
            StepOp::Join { sep } => {
                let sep_s = js_str(sep);
                wln!(writer, "let {nxt} = {prv}.join({sep_s});");
            }
            StepOp::Unescape => {
                self.require_std_helper(
                    "rtUnescapeText",
                    STD_UNESCAPE_TS,
                    STD_UNESCAPE_JS,
                    builder,
                );
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => rtUnescapeText(s));");
                } else {
                    wln!(writer, "let {nxt} = rtUnescapeText({prv});");
                }
            }
            StepOp::Re { pattern } => {
                self.require_std_helper("rtReSearch", STD_RE_SEARCH_TS, STD_RE_SEARCH_JS, builder);
                let loc_str = if location.is_empty() {
                    String::new()
                } else {
                    format!(" at {location}")
                };
                let msg = format!(
                    "{}:{}:{} re-match failed{loc_str} pattern={pattern}",
                    module.source_file, step.span.line, step.span.col
                );
                let rx = js_regex_literal(pattern, false);
                let msg_s = js_str(&msg);
                if is_arr_out {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((s) => rtReSearch({rx}, s, {msg_s}));"
                    );
                } else {
                    wln!(writer, "let {nxt} = rtReSearch({rx}, {prv}, {msg_s});");
                }
            }
            StepOp::ReAll { pattern } => {
                let rx_g = js_regex_literal(pattern, true);
                wln!(
                    writer,
                    "let {nxt} = Array.from({prv}.matchAll({rx_g}), (m) => m[1] ?? m[0]);"
                );
            }
            StepOp::ReSub { pattern, repl } => {
                let rx_g = js_regex_literal(pattern, true);
                let repl_s = js_str(repl);
                if is_arr_out {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.map((s) => s.replace({rx_g}, {repl_s}));"
                    );
                } else {
                    wln!(writer, "let {nxt} = {prv}.replace({rx_g}, {repl_s});");
                }
            }
            StepOp::ReSplit { pattern } => {
                let rx = js_regex_literal(pattern, false);
                wln!(writer, "let {nxt} = {prv}.split({rx});");
            }
            StepOp::Index { index } => {
                let idx_expr = if *index < 0 {
                    format!("{prv}.length - {}", index.unsigned_abs())
                } else {
                    index.to_string()
                };
                wln!(writer, "let {nxt} = {prv}[{idx_expr}];");
                wln!(
                    writer,
                    "if ({nxt} === undefined) throw new RangeError(\"index out of bounds\");"
                );
            }
            StepOp::Slice { start, end } => {
                wln!(writer, "let {nxt} = {prv}.slice({start}, {end});");
            }
            StepOp::Len => {
                wln!(writer, "let {nxt} = {prv}.length;");
            }
            StepOp::Unique { keep_order } => {
                if *keep_order {
                    wln!(
                        writer,
                        "let {nxt} = [...new Map({prv}.map((x) => [x, x])).keys()];"
                    );
                } else {
                    wln!(writer, "let {nxt} = [...new Set({prv})];");
                }
            }
            StepOp::ToInt => {
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => parseInt(s, 10));");
                } else {
                    wln!(writer, "let {nxt} = parseInt({prv}, 10);");
                }
            }
            StepOp::ToFloat => {
                if is_arr_out {
                    wln!(writer, "let {nxt} = {prv}.map((s) => parseFloat(s));");
                } else {
                    wln!(writer, "let {nxt} = parseFloat({prv});");
                }
            }
            StepOp::Jsonify {
                schema_name,
                path,
                parsed_path: _,
            } => {
                let effective_path = path
                    .as_deref()
                    .or_else(|| module.json_def(schema_name).and_then(|j| j.path.as_deref()));
                let raw_expr = if let Some(p) = effective_path {
                    self.require_std_helper(
                        "rtResolveDotpath",
                        STD_JSON_PROJECT_TS,
                        STD_JSON_PROJECT_JS,
                        builder,
                    );
                    format!("rtResolveDotpath(JSON.parse({prv}), {}, false)", js_str(p))
                } else {
                    format!("JSON.parse({prv})")
                };

                if module.json_def(schema_name).is_some() {
                    self.require_std_helper(
                        "rtResolveDotpath",
                        STD_JSON_PROJECT_TS,
                        STD_JSON_PROJECT_JS,
                        builder,
                    );
                    let proj_name = js_json_projector_name(schema_name);
                    wln!(writer, "let {nxt} = {proj_name}({raw_expr});");
                } else {
                    wln!(writer, "let {nxt} = {raw_expr};");
                }
            }
            StepOp::Nested { struct_name } => {
                let pascal = to_pascal_case(struct_name);
                wln!(writer, "let {nxt} = new {pascal}({prv}).parse();");
            }
            StepOp::Filter { cond } => {
                wln!(writer, "let {nxt} = {prv}.filter((i) => (");
                writer.indented(|writer| {
                    let elem_type = TypeInfo {
                        is_array: false,
                        ..step.input.clone()
                    };
                    self.emit_predicate_root(cond, "i", true, &elem_type, builder, writer);
                });
                writer.writeln("));");
            }
            StepOp::Assert { message, cond } => {
                self.require_std_helper("rtAssert", STD_ASSERT_TS, STD_ASSERT_JS, builder);
                let msg = match message {
                    Some(m) => m.clone(),
                    None => {
                        let loc_str = if location.is_empty() {
                            String::new()
                        } else {
                            format!(" at {location}")
                        };
                        format!(
                            "{}:{}:{} assertion failed{loc_str}",
                            module.source_file, step.span.line, step.span.col
                        )
                    }
                };
                let msg_s = js_str(&msg);
                let local = format!("i{prv}");
                wln!(writer, "let {local} = {prv};");
                writer.writeln("rtAssert(");
                writer.indented(|writer| {
                    writer.writeln("(");
                    writer.indented(|writer| {
                        self.emit_predicate_root(cond, &local, false, &step.input, builder, writer);
                    });
                    writer.writeln("),");
                    wln!(writer, "{msg_s},");
                });
                writer.writeln(");");
                wln!(writer, "let {nxt} = {prv};");
            }
            StepOp::Match { cond } => {
                builder.require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW);
                let local = format!("i{prv}");
                wln!(writer, "let {local} = this._tableMatchKey({prv});");
                writer.writeln("if (!(");
                writer.indented(|writer| {
                    let key_type = TypeInfo::string();
                    self.emit_predicate_root(cond, &local, false, &key_type, builder, writer);
                });
                writer.writeln(")) {");
                writer.indented(|writer| {
                    writer.writeln("return RT_UNMATCHED_TABLE_ROW;");
                });
                writer.writeln("}");
                wln!(writer, "let {nxt} = this._parseValue({prv});");
            }
            StepOp::ExtensionCall {
                qualified_name,
                bound_input,
                bound_output,
                ..
            } => {
                let lang_key = if self.mode.is_ts() { "ts" } else { "js" };
                let in_type = Self::resolve_type(bound_input, module);
                let out_type = Self::resolve_type(bound_output, module);
                emit_extension_call(
                    module,
                    qualified_name,
                    step.span,
                    &[lang_key],
                    &format!("'{lang_key}'"),
                    prv,
                    nxt,
                    &in_type,
                    &out_type,
                    |imp| imp.value.clone(),
                    str::to_owned,
                    builder,
                    writer,
                    &mut self.diagnostics,
                );
            }
        }
    }

    fn emit_predicate_root(
        &self,
        expr: &Cond,
        target: &str,
        is_filter: bool,
        input_type: &TypeInfo,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        match expr {
            Cond::And(items) => {
                for (idx, item) in items.iter().enumerate() {
                    let join_op = if idx == 0 { None } else { Some("&&") };
                    self.emit_predicate_expr(
                        item, join_op, target, is_filter, input_type, builder, writer,
                    );
                }
            }
            other => {
                self.emit_predicate_expr(
                    other, None, target, is_filter, input_type, builder, writer,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_predicate_expr(
        &self,
        expr: &Cond,
        join_op: Option<&str>,
        target: &str,
        is_filter: bool,
        input_type: &TypeInfo,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let prefix = match join_op {
            Some("&&") => "&& ",
            Some("||") => "|| ",
            _ => "",
        };

        match expr {
            Cond::Atom(op) => {
                let cond = self.render_pred_op(op, target, is_filter, input_type, builder);
                wln!(writer, "{prefix}{cond}");
            }
            Cond::Not(inner) => {
                wln!(writer, "{prefix}!(");
                writer.indented(|writer| {
                    self.emit_predicate_root(inner, target, is_filter, input_type, builder, writer);
                });
                writer.writeln(")");
            }
            Cond::And(items) => {
                wln!(writer, "{prefix}(");
                writer.indented(|writer| {
                    for (idx, item) in items.iter().enumerate() {
                        let child_join = if idx == 0 { None } else { Some("&&") };
                        self.emit_predicate_expr(
                            item, child_join, target, is_filter, input_type, builder, writer,
                        );
                    }
                });
                writer.writeln(")");
            }
            Cond::Or(items) => {
                wln!(writer, "{prefix}(");
                writer.indented(|writer| {
                    for (idx, item) in items.iter().enumerate() {
                        let child_join = if idx == 0 { None } else { Some("||") };
                        self.emit_predicate_expr(
                            item, child_join, target, is_filter, input_type, builder, writer,
                        );
                    }
                });
                writer.writeln(")");
            }
        }
    }

    fn render_pred_op(
        &self,
        op: &CondOp,
        target: &str,
        is_filter: bool,
        input_type: &TypeInfo,
        builder: &mut ModuleBuilder,
    ) -> String {
        let el = if self.mode.is_ts() && !is_filter {
            format!("({target} as Element)")
        } else {
            target.to_owned()
        };
        let text_target = if input_type.base == BaseType::Document {
            format!("({target}.textContent ?? \"\")")
        } else {
            target.to_owned()
        };

        match op {
            CondOp::Css { query } => {
                format!("{target}.querySelector({}) !== null", js_str(query))
            }
            CondOp::Xpath { query } => {
                self.require_std_helper(
                    "rtXpathFirst",
                    STD_XPATH_FIRST_TS,
                    STD_XPATH_FIRST_JS,
                    builder,
                );
                format!("rtXpathFirst({target}, {}) !== null", js_str(query))
            }
            CondOp::HasAttr { attrs } => {
                if attrs.len() == 1 {
                    format!("{el}.hasAttribute({})", js_str(&attrs[0]))
                } else {
                    format!("{}.some((k) => {el}.hasAttribute(k))", js_str_array(attrs))
                }
            }
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                if values.len() == 1 {
                    format!(
                        "{el}.getAttribute({}) === {}",
                        js_str(name),
                        js_str(&values[0])
                    )
                } else {
                    format!(
                        "{}.some((v) => {el}.getAttribute({}) === v)",
                        js_str_array(values),
                        js_str(name)
                    )
                }
            }
            CondOp::AttrNe { name, values } => {
                if values.len() == 1 {
                    format!(
                        "{el}.getAttribute({}) !== {}",
                        js_str(name),
                        js_str(&values[0])
                    )
                } else {
                    format!(
                        "{}.every((v) => {el}.getAttribute({}) !== v)",
                        js_str_array(values),
                        js_str(name)
                    )
                }
            }
            CondOp::AttrStarts { name, values } => {
                if values.len() == 1 {
                    format!(
                        "({el}.getAttribute({}) ?? \"\").startsWith({})",
                        js_str(name),
                        js_str(&values[0])
                    )
                } else {
                    format!(
                        "{}.some((v) => ({el}.getAttribute({}) ?? \"\").startsWith(v))",
                        js_str_array(values),
                        js_str(name)
                    )
                }
            }
            CondOp::AttrEnds { name, values } => {
                if values.len() == 1 {
                    format!(
                        "({el}.getAttribute({}) ?? \"\").endsWith({})",
                        js_str(name),
                        js_str(&values[0])
                    )
                } else {
                    format!(
                        "{}.some((v) => ({el}.getAttribute({}) ?? \"\").endsWith(v))",
                        js_str_array(values),
                        js_str(name)
                    )
                }
            }
            CondOp::AttrContains { name, values } => {
                if values.len() == 1 {
                    format!(
                        "({el}.getAttribute({}) ?? \"\").includes({})",
                        js_str(name),
                        js_str(&values[0])
                    )
                } else {
                    format!(
                        "{}.some((v) => ({el}.getAttribute({}) ?? \"\").includes(v))",
                        js_str_array(values),
                        js_str(name)
                    )
                }
            }
            CondOp::AttrRe { name, pattern } => {
                let rx = js_regex_literal(pattern, false);
                format!("{rx}.test({el}.getAttribute({}) ?? \"\")", js_str(name))
            }
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                if values.len() == 1 {
                    format!("{text_target} === {}", js_str(&values[0]))
                } else {
                    format!("{}.some((v) => {text_target} === v)", js_str_array(values))
                }
            }
            CondOp::TextNe { values } => {
                if values.len() == 1 {
                    format!("{text_target} !== {}", js_str(&values[0]))
                } else {
                    format!("{}.every((v) => {text_target} !== v)", js_str_array(values))
                }
            }
            CondOp::TextStarts { values } => {
                if values.len() == 1 {
                    format!("{text_target}.startsWith({})", js_str(&values[0]))
                } else {
                    format!(
                        "{}.some((v) => {text_target}.startsWith(v))",
                        js_str_array(values)
                    )
                }
            }
            CondOp::TextEnds { values } => {
                if values.len() == 1 {
                    format!("{text_target}.endsWith({})", js_str(&values[0]))
                } else {
                    format!(
                        "{}.some((v) => {text_target}.endsWith(v))",
                        js_str_array(values)
                    )
                }
            }
            CondOp::TextContains { values } => {
                if values.len() == 1 {
                    format!("{text_target}.includes({})", js_str(&values[0]))
                } else {
                    format!(
                        "{}.some((v) => {text_target}.includes(v))",
                        js_str_array(values)
                    )
                }
            }
            CondOp::TextRe { pattern } => {
                let rx = js_regex_literal(pattern, false);
                format!("{rx}.test({text_target})")
            }
            CondOp::Contains { values } => {
                if values.len() == 1 {
                    format!("{target}.includes({})", js_str(&values[0]))
                } else {
                    format!("{}.some((v) => {target}.includes(v))", js_str_array(values))
                }
            }
            CondOp::Starts { values } => {
                if values.len() == 1 {
                    format!("{target}.startsWith({})", js_str(&values[0]))
                } else {
                    format!(
                        "{}.some((v) => {target}.startsWith(v))",
                        js_str_array(values)
                    )
                }
            }
            CondOp::Ends { values } => {
                if values.len() == 1 {
                    format!("{target}.endsWith({})", js_str(&values[0]))
                } else {
                    format!("{}.some((v) => {target}.endsWith(v))", js_str_array(values))
                }
            }
            CondOp::Re { pattern } => {
                let rx = js_regex_literal(pattern, false);
                format!("{rx}.test({target})")
            }
            CondOp::ReAny { pattern } => {
                let rx = js_regex_literal(pattern, false);
                format!("{target}.some((j) => {rx}.test(j))")
            }
            CondOp::ReAll { pattern } => {
                let rx = js_regex_literal(pattern, false);
                format!("{target}.every((j) => {rx}.test(j))")
            }
            CondOp::Eq { values } | CondOp::In { values } => {
                if values.len() == 1
                    && let CondValue::Int(n) = values[0]
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array)
                {
                    format!("{target}.length === {n}")
                } else if values.len() == 1 {
                    format!("{target} === {}", js_pred_value(&values[0]))
                } else {
                    let list = values
                        .iter()
                        .map(js_pred_value)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("[{list}].some((v) => {target} === v)")
                }
            }
            CondOp::Ne { values } => {
                if values.len() == 1
                    && let CondValue::Int(n) = values[0]
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array)
                {
                    format!("{target}.length !== {n}")
                } else if values.len() == 1 {
                    format!("{target} !== {}", js_pred_value(&values[0]))
                } else {
                    let list = values
                        .iter()
                        .map(js_pred_value)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("[{list}].every((v) => {target} !== v)")
                }
            }
            CondOp::Gt { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} > {value}")
                } else {
                    format!("{target}.length > {value}")
                }
            }
            CondOp::Lt { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} < {value}")
                } else {
                    format!("{target}.length < {value}")
                }
            }
            CondOp::Ge { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} >= {value}")
                } else {
                    format!("{target}.length >= {value}")
                }
            }
            CondOp::Le { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} <= {value}")
                } else {
                    format!("{target}.length <= {value}")
                }
            }
            CondOp::LenEq { values } => {
                if values.len() == 1 {
                    format!("{target}.length === {}", values[0])
                } else {
                    format!("{values:?}.some((v) => {target}.length === v)")
                }
            }
            CondOp::LenNe { values } => {
                if values.len() == 1 {
                    format!("{target}.length !== {}", values[0])
                } else {
                    format!("{values:?}.every((v) => {target}.length !== v)")
                }
            }
            CondOp::LenGt { value } => format!("{target}.length > {value}"),
            CondOp::LenLt { value } => format!("{target}.length < {value}"),
            CondOp::LenGe { value } => format!("{target}.length >= {value}"),
            CondOp::LenLe { value } => format!("{target}.length <= {value}"),
            CondOp::LenRange { start, end } => {
                format!("{start} < {target}.length && {target}.length < {end}")
            }
        }
    }
}

impl LanguageEmitter for JsTsEmitter {
    fn indent_unit(&self) -> &'static str {
        "  "
    }

    fn on_module_start(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.writeln("// Auto-generated by parsmith. DO NOT EDIT.");
        if let Some(ref doc) = module.doc {
            emit_jsdoc(doc, writer);
        }
        if module_has_html(module) {
            self.require_std_helper("rtParseHtml", STD_PARSE_HTML_TS, STD_PARSE_HTML_JS, builder);
        }
        if module_has_table(module) {
            builder.require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW);
        }
        if !module.json_defs.is_empty() {
            self.require_std_helper(
                "rtResolveDotpath",
                STD_JSON_PROJECT_TS,
                STD_JSON_PROJECT_JS,
                builder,
            );
        }
        if module_uses_http(module) {
            self.require_http_transport_std(builder);
        }
        if module_has_fetch(module) {
            self.require_fetch_std(builder);
        }
        if module_has_rest(module) {
            self.require_rest_std(builder);
        }
    }

    fn emit_imports(
        &mut self,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let mut all_imports: IndexSet<String> = IndexSet::new();
        for imp in builder.imports() {
            all_imports.insert(imp.to_owned());
        }
        if !self.mode.is_dts() {
            for imp in builder.std_imports() {
                all_imports.insert(imp.to_owned());
            }
            for imp in builder.runtime_imports() {
                all_imports.insert(imp.to_owned());
            }
        }

        if !all_imports.is_empty() {
            for imp in &all_imports {
                writer.write_block(imp);
            }
            writer.blankline();
        } else {
            writer.blankline();
        }
    }

    fn emit_utilities(
        &mut self,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() {
            for key in builder.std_defs().keys() {
                match key.as_str() {
                    "rtParseHtml" => {
                        writer.writeln(
                            "export declare function rtParseHtml(document: string | Document | Element): Document | Element;",
                        );
                        writer.blankline();
                    }
                    "rtCssRemove" => {
                        writer.writeln(
                            "export declare function rtCssRemove<T extends Document | Element>(root: T, query: string): T;",
                        );
                        writer.blankline();
                    }
                    "rtXpathFirst" => {
                        writer.writeln(
                            "export declare function rtXpathFirst(root: Document | Element, ...queries: string[]): Element | null;",
                        );
                        writer.blankline();
                    }
                    "rtXpathAll" => {
                        writer.writeln(
                            "export declare function rtXpathAll(root: Document | Element, ...queries: string[]): Element[];",
                        );
                        writer.blankline();
                    }
                    "rtXpathRemove" => {
                        writer.writeln(
                            "export declare function rtXpathRemove<T extends Document | Element>(root: T, query: string): T;",
                        );
                        writer.blankline();
                    }
                    "rtAssert" => {
                        writer.writeln("export declare class RtAssertionError extends Error {");
                        writer.indented(|writer| {
                            writer.writeln("constructor(message: string);");
                        });
                        writer.writeln("}");
                        writer.writeln(
                            "export declare function rtAssert(cond: boolean, msg?: string): void;",
                        );
                        writer.blankline();
                    }
                    "rtReSearch" => {
                        writer.writeln("export declare class RtRegexError extends Error {");
                        writer.indented(|writer| {
                            writer.writeln("constructor(message: string);");
                        });
                        writer.writeln("}");
                        writer.writeln(
                            "export declare function rtReSearch(pattern: RegExp, value: string, msg?: string): string;",
                        );
                        writer.blankline();
                    }
                    "rtTrimChars" => {
                        writer.writeln(
                            "export declare function rtTrimChars(s: string, chars: string): string;",
                        );
                        writer.blankline();
                    }
                    "rtLtrimChars" => {
                        writer.writeln(
                            "export declare function rtLtrimChars(s: string, chars: string): string;",
                        );
                        writer.blankline();
                    }
                    "rtRtrimChars" => {
                        writer.writeln(
                            "export declare function rtRtrimChars(s: string, chars: string): string;",
                        );
                        writer.blankline();
                    }
                    "rtRmPrefix" => {
                        writer.writeln(
                            "export declare function rtRmPrefix(s: string, p: string): string;",
                        );
                        writer.blankline();
                    }
                    "rtRmSuffix" => {
                        writer.writeln(
                            "export declare function rtRmSuffix(s: string, p: string): string;",
                        );
                        writer.blankline();
                    }
                    "rtReplMap" => {
                        writer.writeln(
                            "export declare function rtReplMap(s: string, map: Record<string, string>): string;",
                        );
                        writer.blankline();
                    }
                    "rtNormalizeText" => {
                        writer
                            .writeln("export declare function rtNormalizeText(s: string): string;");
                        writer.blankline();
                    }
                    "rtUnescapeText" => {
                        writer
                            .writeln("export declare function rtUnescapeText(s: string): string;");
                        writer.blankline();
                    }
                    "RT_UNMATCHED_TABLE_ROW" => {
                        writer
                            .writeln("export declare const RT_UNMATCHED_TABLE_ROW: unique symbol;");
                        writer.blankline();
                    }
                    "rtResolveDotpath" => {
                        writer.writeln("export declare class RtJsonError extends Error {");
                        writer.indented(|writer| {
                            writer.writeln("constructor(message: string);");
                        });
                        writer.writeln("}");
                        writer
                            .writeln("export declare class RtJsonPathError extends RtJsonError {");
                        writer.indented(|writer| {
                            writer.writeln("constructor(message: string);");
                        });
                        writer.writeln("}");
                        writer.writeln(
                            "export declare class RtJsonFieldMissingError extends RtJsonError {",
                        );
                        writer.indented(|writer| {
                            writer.writeln("constructor(message: string);");
                        });
                        writer.writeln("}");
                        writer.writeln(
                            "export declare class RtJsonSchemaError extends RtJsonError {",
                        );
                        writer.indented(|writer| {
                            writer.writeln("constructor(message: string);");
                        });
                        writer.writeln("}");
                        writer.writeln("export declare function rtTypeName(v: unknown): string;");
                        writer.writeln(
                            "export declare function rtExpectObject(data: unknown, target?: string): Record<string, any>;",
                        );
                        writer.writeln(
                            "export declare function rtExpectArray(data: unknown, target?: string): any[];",
                        );
                        writer.writeln(
                            "export declare function rtSplitGjsonPath(path: string): Array<{ raw: string; key: string; escaped: boolean }>;",
                        );
                        writer.writeln(
                            "export declare function rtResolveDotpath(data: any, path: string, isOptional?: boolean, target?: string): any;",
                        );
                        writer.writeln(
                            "export declare function rtJsonField(data: any, key: string, target: string, isOptional?: boolean, isOmitempty?: boolean): any;",
                        );
                        writer.writeln(
                            "export declare function rtJsonPath(data: any, path: string, target: string, isOptional?: boolean, isOmitempty?: boolean): any;",
                        );
                        writer.blankline();
                    }
                    STD_HTTP_TRANSPORT_KEY => {
                        writer.write_block(STD_HTTP_TRANSPORT_DTS.trim());
                        writer.blankline();
                    }
                    STD_FETCH_KEY => {
                        writer.write_block(STD_FETCH_DTS.trim());
                        writer.blankline();
                    }
                    STD_REST_CORE_KEY => {
                        writer.write_block(STD_REST_CORE_DTS.trim());
                        writer.blankline();
                    }
                    _ => {}
                }
            }
            return;
        }

        for def in builder.runtime_defs().values() {
            writer.write_block(&def.code);
            writer.blankline();
        }
        for def in builder.std_defs().values() {
            writer.write_block(&def.code);
            writer.blankline();
        }
    }

    fn emit_json_def(
        &mut self,
        json_def: &JsonDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.require_std_helper(
            "rtResolveDotpath",
            STD_JSON_PROJECT_TS,
            STD_JSON_PROJECT_JS,
            builder,
        );

        let pascal = to_pascal_case(&json_def.name);

        if self.mode.is_ts() {
            if let Some(ref doc) = json_def.doc {
                emit_jsdoc(doc, writer);
            }
            if json_def.is_dict {
                let key_type = ts_key_type(json_def.key_type.as_ref());
                let val_type = match json_def.value_type.as_ref() {
                    Some(vt) => Self::resolve_type(vt, module),
                    None => "any".to_owned(),
                };
                wln!(
                    writer,
                    "export type {pascal}Json = Record<{key_type}, {val_type}>;"
                );
                writer.blankline();
            } else {
                wln!(writer, "export interface {pascal}Json {{");
                writer.indented(|writer| {
                    for field in &json_def.fields {
                        if field.skip {
                            continue;
                        }
                        if let Some(ref fdoc) = field.doc {
                            emit_jsdoc(fdoc, writer);
                        }
                        let prop_key = if is_js_ident(&field.name) {
                            field.name.clone()
                        } else {
                            js_str(&field.name)
                        };
                        let opt_q = if field.omitempty { "?" } else { "" };
                        let ftype = Self::resolve_type(&field.type_info, module);
                        wln!(writer, "{prop_key}{opt_q}: {ftype};");
                    }
                });
                writer.writeln("}");
                writer.blankline();
            }
        }

        if self.mode.is_dts() {
            let fn_name = js_json_projector_name(&json_def.name);
            let ret_ty = if json_def.is_array {
                format!("{pascal}Json[]")
            } else {
                format!("{pascal}Json")
            };
            let param = if json_def.is_array { "items" } else { "data" };
            wln!(
                writer,
                "export declare function {fn_name}({param}: unknown): {ret_ty};"
            );
            writer.blankline();
        } else {
            self.emit_json_projector(json_def, module, writer);
        }
    }

    fn emit_typedef(
        &mut self,
        typedef: &StructTypeDef,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if !self.mode.is_ts() {
            return;
        }
        let name = to_pascal_case(&typedef.name);
        match typedef.struct_kind {
            StructKind::Rest => {}
            StructKind::Flat => {
                wln!(writer, "export type {name}Type = string[];");
                writer.blankline();
            }
            StructKind::Dict => {
                let val_type = typedef
                    .fields
                    .iter()
                    .find(|f| f.name == "value")
                    .map(|f| Self::resolve_type(&f.type_info, module))
                    .unwrap_or_else(|| "string".to_owned());
                wln!(
                    writer,
                    "export type {name}Type = Record<string, {val_type}>;"
                );
                writer.blankline();
            }
            StructKind::Item | StructKind::List | StructKind::Table | StructKind::Raw => {
                wln!(writer, "export interface {name}Type {{");
                writer.indented(|writer| {
                    for field in &typedef.fields {
                        let fname = to_camel_case(&field.name);
                        let ftype = Self::resolve_type(&field.type_info, module);
                        wln!(writer, "{fname}: {ftype};");
                    }
                });
                writer.writeln("}");
                writer.blankline();
            }
        }
    }

    fn emit_rest_artifact(
        &mut self,
        artifact: &RestArtifact,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.emit_rest_artifact_impl(artifact, module, builder, writer);
    }

    fn on_struct_start(
        &mut self,
        struct_decl: &StructDecl,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref doc) = struct_decl.doc {
            emit_jsdoc(doc, writer);
        }
        let name = to_pascal_case(&struct_decl.name);
        if self.mode.is_dts() {
            wln!(writer, "export declare class {name} {{");
            writer.indent();
            return;
        }

        wln!(writer, "export class {name} {{");
        writer.indent();

        if self.mode == JsTsMode::TypeScript && struct_decl.kind != StructKind::Rest {
            if struct_decl.kind == StructKind::Raw {
                writer.writeln("private _doc: string;");
            } else {
                writer.writeln("private _doc: Document | Element;");
            }
            for init_f in &struct_decl.init_fields {
                let camel = to_camel_case(&init_f.name);
                let ftype = Self::resolve_type(&init_f.type_info, module);
                wln!(writer, "private _{camel}: {ftype};");
            }
            writer.blankline();
        }
    }

    fn emit_init(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if struct_decl.kind == StructKind::Rest {
            return;
        }
        if self.mode.is_dts() {
            if struct_decl.kind == StructKind::Raw {
                writer.writeln("constructor(document: string);");
            } else {
                writer.writeln("constructor(document: string | Document | Element);");
            }
            return;
        }

        if struct_decl.kind == StructKind::Raw {
            if self.mode.is_ts() {
                writer.writeln("constructor(document: string) {");
            } else {
                writer.writeln("constructor(document) {");
            }
            writer.indented(|writer| {
                writer.writeln("this._doc = document;");
                for init_f in &struct_decl.init_fields {
                    let camel = to_camel_case(&init_f.name);
                    let pascal = to_pascal_case(&init_f.name);
                    wln!(writer, "this._{camel} = this._init{pascal}(this._doc);");
                }
            });
            writer.writeln("}");
        } else {
            self.require_std_helper("rtParseHtml", STD_PARSE_HTML_TS, STD_PARSE_HTML_JS, builder);
            if self.mode.is_ts() {
                writer.writeln("constructor(document: string | Document | Element) {");
            } else {
                writer.writeln("constructor(document) {");
            }
            writer.indented(|writer| {
                writer.writeln("this._doc = rtParseHtml(document);");
                for init_f in &struct_decl.init_fields {
                    let camel = to_camel_case(&init_f.name);
                    let pascal = to_pascal_case(&init_f.name);
                    wln!(writer, "this._{camel} = this._init{pascal}(this._doc);");
                }
            });
            writer.writeln("}");
        }
        writer.blankline();
    }

    fn emit_init_field(
        &mut self,
        struct_decl: &StructDecl,
        init_field: &InitFieldDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let pascal = to_pascal_case(&init_field.name);
        let location = format!(
            "{}.@init.{}",
            to_pascal_case(&struct_decl.name),
            init_field.name
        );

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&init_field.pipeline.input, module);
            let t_ret = Self::resolve_type(&init_field.type_info, module);
            wln!(writer, "private _init{pascal}({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_init{pascal}({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(
                &init_field.pipeline,
                &v0,
                &location,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_pre_validate(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.@pre-validate", to_pascal_case(&struct_decl.name));

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&pipeline.input, module);
            wln!(writer, "private _preValidate({v0}: {t_arg}): void {{");
        } else {
            wln!(writer, "_preValidate({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, true, module, builder, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_check(
        &mut self,
        struct_decl: &StructDecl,
        check: &CheckDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let method_name = to_camel_case(&to_snake_case(&check.name));
        let t_ret = Self::resolve_type(&check.pipeline.output, module);
        if self.mode.is_dts() && !writer.is_dry_run() {
            wln!(writer, "{method_name}(): {t_ret};");
            return;
        }

        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.{}", to_pascal_case(&struct_decl.name), check.name);

        if self.mode.is_ts() {
            wln!(writer, "{method_name}(): {t_ret} {{");
        } else {
            wln!(writer, "{method_name}() {{");
        }
        writer.indented(|writer| {
            wln!(writer, "let {v0} = this._doc;");
            self.emit_pipeline_body(
                &check.pipeline,
                &v0,
                &location,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_split_doc(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.@split-doc", to_pascal_case(&struct_decl.name));

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&pipeline.input, module);
            let t_ret = Self::resolve_type(&pipeline.output, module);
            wln!(writer, "private _splitDoc({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_splitDoc({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_key_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.key", to_pascal_case(&struct_decl.name));

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&pipeline.input, module);
            let t_ret = Self::resolve_type(&pipeline.output, module);
            wln!(writer, "private _parseKey({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_parseKey({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_value_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.value", to_pascal_case(&struct_decl.name));

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&pipeline.input, module);
            let t_ret = Self::resolve_type(&pipeline.output, module);
            wln!(writer, "private _parseValue({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_parseValue({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_table_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.@table", to_pascal_case(&struct_decl.name));

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&pipeline.input, module);
            let t_ret = Self::resolve_type(&pipeline.output, module);
            wln!(writer, "private _tableConfig({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_tableConfig({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_rows_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.@rows", to_pascal_case(&struct_decl.name));

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&pipeline.input, module);
            let t_ret = Self::resolve_type(&pipeline.output, module);
            wln!(writer, "private _tableRows({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_tableRows({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_match_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let location = format!("{}.@match", to_pascal_case(&struct_decl.name));

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&pipeline.input, module);
            let t_ret = Self::resolve_type(&pipeline.output, module);
            wln!(writer, "private _tableMatchKey({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_tableMatchKey({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_field(
        &mut self,
        struct_decl: &StructDecl,
        field: &FieldDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.mode.is_dts() && !writer.is_dry_run() {
            return;
        }
        writer.reset_vars();
        let v0 = writer.next_var();
        let pascal = to_pascal_case(&field.name);
        let location = format!("{}.{}", to_pascal_case(&struct_decl.name), field.name);

        if self.mode.is_ts() {
            let t_arg = Self::resolve_type(&field.pipeline.input, module);
            let mut t_ret = Self::resolve_type(&field.type_info, module);
            if struct_decl.kind == StructKind::Table {
                builder.require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW);
                t_ret = format!("{t_ret} | typeof RT_UNMATCHED_TABLE_ROW");
            }
            wln!(writer, "private _parse{pascal}({v0}: {t_arg}): {t_ret} {{");
        } else {
            wln!(writer, "_parse{pascal}({v0}) {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(
                &field.pipeline,
                &v0,
                &location,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_start_parse(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = to_pascal_case(&struct_decl.name);
        let t_ret = Self::resolve_parse_return_type(struct_decl);
        let is_ts = self.mode.is_ts();

        if self.mode.is_dts() {
            wln!(writer, "parse(): {t_ret};");
            return;
        }

        if is_ts {
            wln!(writer, "parse(): {t_ret} {{");
        } else {
            writer.writeln("parse() {");
        }

        writer.indented(|writer| {
            if struct_decl.pre_validate.is_some() {
                writer.writeln("this._preValidate(this._doc);");
            }

            match struct_decl.kind {
                StructKind::Item => {
                    Self::emit_item_return(&struct_decl.fields, "this._doc", writer);
                }
                StructKind::List => {
                    Self::emit_list_return(&struct_decl.fields, writer);
                }
                StructKind::Dict => {
                    if is_ts {
                        wln!(writer, "const _result: {name}Type = {{}};");
                    } else {
                        writer.writeln("const _result = {};");
                    }
                    writer.writeln("for (const e of this._splitDoc(this._doc)) {");
                    writer.indented(|writer| {
                        writer.writeln("_result[this._parseKey(e)] = this._parseValue(e);");
                    });
                    writer.writeln("}");
                    writer.writeln("return _result;");
                }
                StructKind::Flat => {
                    if is_ts {
                        writer.writeln("let _result: string[] = [];");
                    } else {
                        writer.writeln("let _result = [];");
                    }
                    for field in &struct_decl.fields {
                        let camel = to_camel_case(&field.name);
                        let pascal = to_pascal_case(&field.name);
                        if field.type_info.is_optional {
                            wln!(writer,
                                "const _{camel} = this._parse{pascal}(this._doc);"
                            );
                            wln!(writer, "if (_{camel} !== null) {{");
                            writer.indented(|writer| {
                                if field.type_info.is_array {
                                    wln!(writer,
                                        "_result = _result.concat(_{camel});"
                                    );
                                } else {
                                    wln!(writer, "_result.push(_{camel});");
                                }
                            });
                            writer.writeln("}");
                        } else if field.type_info.is_array {
                            wln!(writer,
                                "_result = _result.concat(this._parse{pascal}(this._doc));"
                            );
                        } else {
                            wln!(writer,
                                "_result.push(this._parse{pascal}(this._doc));"
                            );
                        }
                    }
                    if struct_decl.keep_order {
                        writer
                            .writeln("return [...new Map(_result.map((x) => [x, x])).keys()];");
                    } else {
                        writer.writeln("return [...new Set(_result)];");
                    }
                }
                StructKind::Table => {
                    builder.require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW);
                    if is_ts {
                        writer.writeln("const _result: Record<string, any> = {};");
                    } else {
                        writer.writeln("const _result = {};");
                    }
                    writer.writeln("const _table = this._tableConfig(this._doc);");
                    writer.writeln("for (const _row of this._tableRows(_table)) {");
                    writer.indented(|writer| {
                        for field in &struct_decl.fields {
                            let camel = to_camel_case(&field.name);
                            let pascal = to_pascal_case(&field.name);
                            let key_lit = js_str(&camel);
                            wln!(writer,
                                "const _{camel} = this._parse{pascal}(_row);"
                            );
                            wln!(writer,
                                "if (_{camel} !== RT_UNMATCHED_TABLE_ROW && !Object.prototype.hasOwnProperty.call(_result, {key_lit})) {{"
                            );
                            writer.indented(|writer| {
                                wln!(writer, "_result[{key_lit}] = _{camel};");
                            });
                            writer.writeln("}");
                        }
                    });
                    writer.writeln("}");
                    if is_ts {
                        wln!(writer, "return _result as {name}Type;");
                    } else {
                        writer.writeln("return _result;");
                    }
                }
                StructKind::Raw => {
                    if struct_decl.split_doc.is_some() {
                        Self::emit_list_return(&struct_decl.fields, writer);
                    } else {
                        Self::emit_item_return(&struct_decl.fields, "this._doc", writer);
                    }
                }
                StructKind::Rest => {}
            }
        });
        writer.writeln("}");
    }

    fn emit_request(
        &mut self,
        struct_decl: &StructDecl,
        request: &RequestDecl,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.emit_request_impl(struct_decl, request, builder, writer);
    }

    fn emit_rest_endpoint(
        &mut self,
        struct_decl: &StructDecl,
        endpoint: &RestEndpointDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.emit_rest_endpoint_impl(struct_decl, endpoint, module, builder, writer);
    }

    fn on_struct_end(
        &mut self,
        _struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.dedent();
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_function(
        &mut self,
        func: &FunctionDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let camel = to_camel_case(&func.name);
        let t_ret = Self::resolve_type(&func.return_type, module);

        if let Some(ref doc) = func.doc
            && (!self.mode.is_dts() || !writer.is_dry_run())
        {
            emit_jsdoc(doc, writer);
        }

        if self.mode.is_dts() && !writer.is_dry_run() {
            if func.is_raw {
                wln!(
                    writer,
                    "export declare function {camel}(document: string): {t_ret};"
                );
            } else {
                wln!(
                    writer,
                    "export declare function {camel}(document: string | Document | Element): {t_ret};"
                );
            }
            writer.blankline();
            return;
        }

        writer.reset_vars();
        let v0 = writer.next_var();

        if func.is_raw {
            if self.mode.is_ts() {
                wln!(
                    writer,
                    "export function {camel}(document: string): {t_ret} {{"
                );
            } else {
                wln!(writer, "export function {camel}(document) {{");
            }
        } else {
            self.require_std_helper("rtParseHtml", STD_PARSE_HTML_TS, STD_PARSE_HTML_JS, builder);
            if self.mode.is_ts() {
                wln!(
                    writer,
                    "export function {camel}(document: string | Document | Element): {t_ret} {{"
                );
            } else {
                wln!(writer, "export function {camel}(document) {{");
            }
        }

        writer.indented(|writer| {
            if func.is_raw {
                wln!(writer, "let {v0} = document;");
            } else {
                wln!(writer, "let {v0} = rtParseHtml(document);");
            }
            self.emit_pipeline_body(
                &func.pipeline,
                &v0,
                &func.name,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("}");
        writer.blankline();
    }
}

impl JsTsEmitter {
    fn emit_item_return(fields: &[FieldDecl], doc_expr: &str, writer: &mut CodeWriter) {
        if fields.is_empty() {
            writer.writeln("return {};");
            return;
        }
        writer.writeln("return {");
        writer.indented(|writer| {
            for field in fields {
                let camel = to_camel_case(&field.name);
                let pascal = to_pascal_case(&field.name);
                wln!(writer, "{camel}: this._parse{pascal}({doc_expr}),");
            }
        });
        writer.writeln("};");
    }

    fn emit_list_return(fields: &[FieldDecl], writer: &mut CodeWriter) {
        writer.writeln("return Array.from(this._splitDoc(this._doc)).map((i) => ({");
        writer.indented(|writer| {
            for field in fields {
                let camel = to_camel_case(&field.name);
                let pascal = to_pascal_case(&field.name);
                wln!(writer, "{camel}: this._parse{pascal}(i),");
            }
        });
        writer.writeln("}));");
    }
}

/// Emits a standalone TypeScript (`.ts`) module for `module` and `options`.
pub fn emit_ts_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = JsTsEmitter::new(options.clone(), JsTsMode::TypeScript);
    let (ts_code, builder) = drive_module(
        &mut emitter,
        module,
        options.prologue.as_deref(),
        options.epilogue.as_deref(),
    );
    if !emitter.diagnostics.is_empty() {
        return Err(emitter.diagnostics);
    }
    Ok((ts_code, builder))
}

/// Emits a TypeScript declaration (`.d.ts`) module for `module` and `options`.
pub fn emit_dts_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = JsTsEmitter::new(options.clone(), JsTsMode::Declaration);
    let (dts_code, builder) = drive_module(&mut emitter, module, None, None);
    if !emitter.diagnostics.is_empty() {
        return Err(emitter.diagnostics);
    }
    Ok((dts_code, builder))
}

/// Emits clean ESM JavaScript (`.js`) paired with a companion TypeScript declaration (`.d.ts`) module.
pub fn emit_js_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, String, ModuleBuilder), Vec<Diagnostic>> {
    let mut js_emitter = JsTsEmitter::new(options.clone(), JsTsMode::JavaScript);
    let (js_code, builder) = drive_module(
        &mut js_emitter,
        module,
        options.prologue.as_deref(),
        options.epilogue.as_deref(),
    );
    if !js_emitter.diagnostics.is_empty() {
        return Err(js_emitter.diagnostics);
    }
    let (dts_code, _) = emit_dts_module(module, options)?;
    Ok((js_code, dts_code, builder))
}

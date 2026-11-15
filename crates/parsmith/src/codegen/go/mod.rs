//! Go 1.26+ target backend (`GoEmitter`: `goquery` + `net/http` + `gjson`).
//!
//! Emits `gofmt`-compliant tab-indented Go source directly from [`CodeWriter`] without invoking
//! external `gofmt` subprocesses. Supports both single-file self-contained output (default) and
//! `-R / --separate-runtime` (`<runtime_name>.go` in the same package).

pub mod json;
pub mod rest;
pub mod runtime;

use crate::kdl::Span;

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, FieldDecl, FunctionDef, InitFieldDecl, JsonDef,
    LiteralValue, Module, Pipeline, RawMode, RequestDecl, RestArtifact, RestEndpointDecl, StepOp,
    StructDecl, StructKind, StructTypeDef, TypeInfo, TypedStep,
};
use crate::codegen::builder::{ModuleBuilder, dedent_code_block, emit_extension_call};
use crate::codegen::emitter::{LanguageEmitter, drive_module};
use crate::codegen::go::json::resolve_dict_key_type;
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, module_has_html, module_uses_http};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::{is_valid_symbol, to_camel_case, to_snake_case};
use crate::lowering::clean_doc;

pub use runtime::{
    BASE_REST_RUNTIME, BASE_RUNTIME, NETHTTP_REST_RUNTIME, REST_IMPORTS, get_go_runtime_helper,
    go_regex_raw, go_str, go_str_array, go_str_map,
};

/// Validates that `pkg` is a legal Go package identifier (`^[A-Za-z_][A-Za-z0-9_]*$` and not a Go keyword).
pub fn validate_go_package_name(pkg: &str) -> bool {
    !pkg.is_empty() && is_valid_symbol("go", pkg)
}

/// Returns the short Go method receiver identifier for `struct_name` (first lowercase ASCII letter).
pub(crate) fn receiver(struct_name: &str) -> String {
    struct_name
        .chars()
        .next()
        .map(|c| c.to_ascii_lowercase().to_string())
        .unwrap_or_else(|| "x".to_owned())
}

fn go_method_name(field_name: &str) -> String {
    format!("parse{}", to_pascal_case(field_name))
}

fn go_zero_value(go_type: &str) -> String {
    if go_type.starts_with('*') || go_type.starts_with("map[") || go_type == "any" {
        "nil".to_owned()
    } else if go_type.starts_with("[]") {
        format!("{go_type}{{}}")
    } else if go_type == "string" {
        "\"\"".to_owned()
    } else if matches!(go_type, "int64" | "int" | "int32" | "float64" | "float32") {
        "0".to_owned()
    } else if go_type == "bool" {
        "false".to_owned()
    } else {
        format!("{go_type}{{}}")
    }
}

/// Writes `// ...` Go doc comments from `doc` into `writer`.
pub(crate) fn emit_go_doc(doc: &str, writer: &mut CodeWriter) {
    let cleaned = clean_doc(doc);
    if cleaned.is_empty() {
        return;
    }
    for line in cleaned.lines() {
        if line.trim().is_empty() {
            writer.writeln("//");
        } else {
            wln!(writer, "// {line}");
        }
    }
}

/// Writes struct fields with `gofmt`-compliant column alignment for names, types, and struct tags.
pub(crate) fn emit_aligned_struct_fields(
    fields: &[(String, String, Option<String>)],
    writer: &mut CodeWriter,
) {
    if fields.is_empty() {
        return;
    }
    let max_name_len = fields.iter().map(|(n, _, _)| n.len()).max().unwrap_or(0);
    let has_any_tag = fields.iter().any(|(_, _, t)| t.is_some());
    let max_type_len = if has_any_tag {
        fields.iter().map(|(_, ty, _)| ty.len()).max().unwrap_or(0)
    } else {
        0
    };

    for (name, ty, tag_opt) in fields {
        let name_pad = " ".repeat(max_name_len - name.len() + 1);
        if let Some(tag) = tag_opt {
            let type_pad = " ".repeat(max_type_len - ty.len() + 1);
            wln!(writer, "{name}{name_pad}{ty}{type_pad}{tag}");
        } else {
            wln!(writer, "{name}{name_pad}{ty}");
        }
    }
}

/// Writes composite literal `Key: Value,` entries with `gofmt`-compliant column alignment.
pub(crate) fn emit_aligned_composite_fields(entries: &[(String, String)], writer: &mut CodeWriter) {
    if entries.is_empty() {
        return;
    }
    let max_key_len = entries.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (k, val) in entries {
        let pad = " ".repeat(max_key_len - k.len() + 1);
        wln!(writer, "{k}:{pad}{val},");
    }
}

/// Normalizes leading space indentation in a dedented Go snippet to `\t` tabs for `gofmt` compliance.
pub(crate) fn normalize_go_tabs(raw: &str) -> String {
    let dedented = dedent_code_block(raw);
    let lines: Vec<&str> = dedented.lines().collect();
    let step = lines
        .iter()
        .filter(|l| !l.trim().is_empty() && l.starts_with(' '))
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(4)
        .max(1);

    let mut out = Vec::with_capacity(lines.len());
    let mut in_raw_str = false;
    for line in lines {
        let formatted = if in_raw_str {
            line.to_owned()
        } else if line.trim().is_empty() {
            String::new()
        } else if line.starts_with(' ') {
            let spaces = line.len() - line.trim_start_matches(' ').len();
            let tabs = spaces / step;
            let rem = spaces % step;
            format!(
                "{}{}{}",
                "\t".repeat(tabs),
                " ".repeat(rem),
                &line[spaces..]
            )
        } else {
            line.trim_end().to_owned()
        };
        let backtick_count = line.chars().filter(|&c| c == '`').count();
        if backtick_count % 2 == 1 {
            in_raw_str = !in_raw_str;
        }
        out.push(formatted);
    }
    out.join("\n")
}

fn import_sort_key(imp: &str) -> (&str, &str) {
    let trimmed = imp.trim();
    if let Some(start) = trimmed.find('"')
        && let Some(end) = trimmed.rfind('"')
        && end > start
    {
        (&trimmed[start + 1..end], trimmed)
    } else {
        (trimmed, trimmed)
    }
}

fn sort_go_imports(imports: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut list: Vec<String> = imports.into_iter().collect();
    list.sort_by(|a, b| import_sort_key(a).cmp(&import_sort_key(b)));
    list.dedup();
    list
}

fn strip_outer_parens(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'(' && bytes[bytes.len() - 1] == b')' {
        let mut depth = 0usize;
        let mut in_str = false;
        let mut in_raw = false;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if in_str {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == b'"' {
                    in_str = false;
                }
            } else if in_raw {
                if b == b'`' {
                    in_raw = false;
                }
            } else {
                match b {
                    b'"' => in_str = true,
                    b'`' => in_raw = true,
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 && i + 1 < bytes.len() {
                            return s;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        if depth == 0 {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Go 1.26+ `LanguageEmitter` implementation (`goquery` + `net/http` + `gjson`).
pub struct GoEmitter {
    /// Code generation options (`package`, `separate_runtime`, `runtime_name`, etc.).
    pub options: CodegenOptions,
    /// Diagnostics accumulated during code generation (`E203` XPath, `E400` missing `"go"` extension target).
    pub diagnostics: Vec<Diagnostic>,
    current_struct_name: String,
    current_table_field_ret: Option<String>,
}

impl GoEmitter {
    /// Creates a new `GoEmitter` for `options`.
    pub fn new(options: CodegenOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            current_struct_name: String::new(),
            current_table_field_ret: None,
        }
    }

    /// Registers a selective `rt*` helper from [`runtime::get_go_runtime_helper`] in `builder`.
    ///
    /// Automatically registers the scalar base helper when `name` ends with `"Arr"`.
    pub fn require(&self, name: &str, builder: &mut ModuleBuilder) {
        if let Some(base) = name.strip_suffix("Arr")
            && let Some((base_imps, base_code)) = get_go_runtime_helper(base)
        {
            builder.require_std_with_imports(base, base_code, base_imps.iter().copied());
        }
        if let Some((imps, code)) = get_go_runtime_helper(name) {
            builder.require_std_with_imports(name, code, imps.iter().copied());
        }
    }

    /// Resolves a [`TypeInfo`] into a Go type signature (`string`, `int64`, `float64`, `bool`,
    /// `*goquery.Selection`, `[]T`, `map[K]V`, `*T`).
    pub fn resolve_type(type_info: &TypeInfo, module: &Module) -> String {
        if type_info.is_dict {
            let key_type = resolve_dict_key_type(type_info.key_type.as_deref());
            let val_type = match type_info.value_type.as_deref() {
                Some(vt) => Self::resolve_type(vt, module),
                None => "any".to_owned(),
            };
            let mut res = format!("map[{key_type}]{val_type}");
            if type_info.is_array && type_info.base != BaseType::Document {
                res = format!("[]{res}");
            }
            if type_info.is_optional || type_info.omitempty {
                res = format!("*{res}");
            }
            return res;
        }

        let mut base_str = match &type_info.base {
            BaseType::String => "string".to_owned(),
            BaseType::Int => "int64".to_owned(),
            BaseType::Float => "float64".to_owned(),
            BaseType::Bool => "bool".to_owned(),
            BaseType::Null | BaseType::Auto | BaseType::DictValue => "any".to_owned(),
            BaseType::Document => "*goquery.Selection".to_owned(),
            BaseType::Nested(ref_name) => {
                let pascal = to_pascal_case(ref_name);
                if let Some(target_struct) = module.struct_decl(ref_name) {
                    let mut st_type = match target_struct.kind {
                        StructKind::Flat => "[]string".to_owned(),
                        StructKind::List => format!("[]{pascal}Type"),
                        StructKind::Raw if target_struct.split_doc.is_some() => {
                            format!("[]{pascal}Type")
                        }
                        _ => format!("{pascal}Type"),
                    };
                    if type_info.is_optional || type_info.omitempty {
                        st_type = format!("*{st_type}");
                    }
                    return st_type;
                }
                format!("{pascal}Type")
            }
            BaseType::Json(ref_name) => {
                let pascal = to_pascal_case(ref_name);
                let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
                if type_info.is_array || def_is_arr {
                    let mut arr_ty = format!("[]{pascal}Json");
                    if type_info.is_optional || type_info.omitempty {
                        arr_ty = format!("*{arr_ty}");
                    }
                    return arr_ty;
                }
                format!("{pascal}Json")
            }
        };

        if type_info.is_array && type_info.base != BaseType::Document {
            base_str = format!("[]{base_str}");
        }
        if (type_info.is_optional || type_info.omitempty) && base_str != "any" {
            base_str = format!("*{base_str}");
        }
        base_str
    }

    /// Resolves the return type of `.Parse()` for a [`StructDecl`].
    pub fn resolve_parse_return_type(struct_decl: &StructDecl) -> String {
        let name = to_pascal_case(&struct_decl.name);
        match struct_decl.kind {
            StructKind::Item | StructKind::Dict | StructKind::Table => format!("{name}Type"),
            StructKind::List => format!("[]{name}Type"),
            StructKind::Flat => "[]string".to_owned(),
            StructKind::Raw => {
                if struct_decl.split_doc.is_some() {
                    format!("[]{name}Type")
                } else {
                    format!("{name}Type")
                }
            }
            StructKind::Rest => "any".to_owned(),
        }
    }

    /// Emits a complete [`Pipeline`] body inside a method or function.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_pipeline_body(
        &mut self,
        pipeline: &Pipeline,
        initial_var: &str,
        ret_type: &str,
        location: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref fb_lit) = pipeline.fallback {
            self.require("rtFallback", builder);
            let fb_val = go_fallback_value(fb_lit, ret_type);
            wln!(writer, "return rtFallback(func() {ret_type} {{");
            writer.indented(|writer| {
                let final_var = self.emit_pipeline_steps(
                    &pipeline.steps,
                    initial_var,
                    location,
                    is_pre_validate,
                    module,
                    builder,
                    writer,
                );
                let step_produces_optional =
                    pipeline.steps.last().is_some_and(|s| s.output.is_optional);
                if ret_type.starts_with('*') && !step_produces_optional {
                    wln!(writer, "_fallbackValue := {final_var}");
                    writer.writeln("return &_fallbackValue");
                } else {
                    wln!(writer, "return {final_var}");
                }
            });
            wln!(writer, "}}, {fb_val})");
        } else {
            let final_var = self.emit_pipeline_steps(
                &pipeline.steps,
                initial_var,
                location,
                is_pre_validate,
                module,
                builder,
                writer,
            );
            if !is_pre_validate {
                wln!(writer, "return {final_var}");
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_steps(
        &mut self,
        steps: &[TypedStep],
        initial_var: &str,
        location: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) -> String {
        let mut cur_var = initial_var.to_owned();
        let last_idx = steps.len().saturating_sub(1);
        for (idx, step) in steps.iter().enumerate() {
            let nxt_var = writer.next_var();
            let next_is_to_bool = steps
                .get(idx + 1)
                .is_some_and(|next| matches!(next.op, StepOp::ToBool));
            let is_last_pre_validate_step = is_pre_validate && idx == last_idx;
            self.emit_step(
                step,
                &cur_var,
                &nxt_var,
                next_is_to_bool,
                is_last_pre_validate_step,
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
        is_last_pre_validate_step: bool,
        location: &str,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_arr_in = step.input.is_array;
        let is_arr_out = step.output.is_array;

        match &step.op {
            StepOp::LoadInit { name } => {
                let rcv = receiver(&self.current_struct_name);
                let camel = to_camel_case(name);
                wln!(writer, "{nxt} := {rcv}.{camel}");
            }
            StepOp::Css { queries } => {
                if next_is_to_bool {
                    for (i, query) in queries.iter().enumerate() {
                        let q = go_str(query);
                        if i == 0 {
                            wln!(writer, "{nxt} := {prv}.Find({q}).First()");
                        } else {
                            wln!(writer, "if {nxt}.Length() == 0 {{");
                            writer.indented(|writer| {
                                wln!(writer, "{nxt} = {prv}.Find({q}).First()");
                            });
                            writer.writeln("}");
                        }
                    }
                } else {
                    self.require("rtCssSelect", builder);
                    let args = queries
                        .iter()
                        .map(|q| go_str(q))
                        .collect::<Vec<_>>()
                        .join(", ");
                    wln!(writer, "{nxt} := rtCssSelect({prv}, {args})");
                }
            }
            StepOp::CssAll { queries } => {
                for (i, query) in queries.iter().enumerate() {
                    let q = go_str(query);
                    if i == 0 {
                        wln!(writer, "{nxt} := {prv}.Find({q})");
                    } else {
                        wln!(writer, "if {nxt}.Length() == 0 {{");
                        writer.indented(|writer| {
                            wln!(writer, "{nxt} = {prv}.Find({q})");
                        });
                        writer.writeln("}");
                    }
                }
            }
            StepOp::CssRemove { query } => {
                let q = go_str(query);
                wln!(writer, "{prv}.Find({q}).Remove()");
                if !is_last_pre_validate_step {
                    wln!(writer, "{nxt} := {prv}");
                }
            }
            StepOp::Xpath { .. } | StepOp::XpathAll { .. } | StepOp::XpathRemove { .. } => {
                if writer.is_dry_run() {
                    self.record_unsupported_xpath(step.span, &module.source_file);
                }
            }
            StepOp::Text => {
                if !is_arr_out {
                    wln!(writer, "{nxt} := {prv}.Text()");
                } else {
                    wln!(
                        writer,
                        "{nxt} := {prv}.Map(func(_ int, s *goquery.Selection) string {{"
                    );
                    writer.indented(|writer| {
                        writer.writeln("return s.Text()");
                    });
                    writer.writeln("})");
                }
            }
            StepOp::Raw { mode } => match (mode, is_arr_out) {
                (RawMode::Inner, false) => {
                    wln!(writer, "_{nxt}Html, _ := {prv}.Html()");
                    wln!(writer, "{nxt} := _{nxt}Html");
                }
                (RawMode::Inner, true) => {
                    wln!(
                        writer,
                        "{nxt} := {prv}.Map(func(_ int, s *goquery.Selection) string {{"
                    );
                    writer.indented(|writer| {
                        writer.writeln("_h, _ := s.Html()");
                        writer.writeln("return _h");
                    });
                    writer.writeln("})");
                }
                (RawMode::Outer, false) => {
                    wln!(writer, "_{nxt}Html, _ := goquery.OuterHtml({prv})");
                    wln!(writer, "{nxt} := _{nxt}Html");
                }
                (RawMode::Outer, true) => {
                    wln!(
                        writer,
                        "{nxt} := {prv}.Map(func(_ int, s *goquery.Selection) string {{"
                    );
                    writer.indented(|writer| {
                        writer.writeln("_h, _ := goquery.OuterHtml(s)");
                        writer.writeln("return _h");
                    });
                    writer.writeln("})");
                }
            },
            StepOp::Attr { keys } => {
                if !is_arr_in && keys.len() == 1 {
                    self.require("rtAttr", builder);
                    let k = go_str(&keys[0]);
                    wln!(writer, "{nxt} := rtAttr({prv}, {k})");
                } else if is_arr_in && keys.len() == 1 {
                    self.require("rtAttrList", builder);
                    let k = go_str(&keys[0]);
                    wln!(writer, "{nxt} := rtAttrList({prv}, {k})");
                } else {
                    self.require("rtAttrMulti", builder);
                    let args = keys
                        .iter()
                        .map(|k| go_str(k))
                        .collect::<Vec<_>>()
                        .join(", ");
                    wln!(writer, "{nxt} := rtAttrMulti({prv}, {args})");
                }
            }
            StepOp::Trim { chars } => {
                let fn_name = if is_arr_out { "rtTrimArr" } else { "rtTrim" };
                self.require(fn_name, builder);
                let c = go_str(chars);
                wln!(writer, "{nxt} := {fn_name}({prv}, {c})");
            }
            StepOp::Ltrim { chars } => {
                let fn_name = if is_arr_out { "rtLTrimArr" } else { "rtLTrim" };
                self.require(fn_name, builder);
                let c = go_str(chars);
                wln!(writer, "{nxt} := {fn_name}({prv}, {c})");
            }
            StepOp::Rtrim { chars } => {
                let fn_name = if is_arr_out { "rtRTrimArr" } else { "rtRTrim" };
                self.require(fn_name, builder);
                let c = go_str(chars);
                wln!(writer, "{nxt} := {fn_name}({prv}, {c})");
            }
            StepOp::NormalizeSpace => {
                let fn_name = if is_arr_out {
                    "rtNormalizeSpaceArr"
                } else {
                    "rtNormalizeSpace"
                };
                self.require(fn_name, builder);
                wln!(writer, "{nxt} := {fn_name}({prv})");
            }
            StepOp::RmPrefix { substr } => {
                let fn_name = if is_arr_out {
                    "rtRmPrefixArr"
                } else {
                    "rtRmPrefix"
                };
                self.require(fn_name, builder);
                let s = go_str(substr);
                wln!(writer, "{nxt} := {fn_name}({prv}, {s})");
            }
            StepOp::RmSuffix { substr } => {
                let fn_name = if is_arr_out {
                    "rtRmSuffixArr"
                } else {
                    "rtRmSuffix"
                };
                self.require(fn_name, builder);
                let s = go_str(substr);
                wln!(writer, "{nxt} := {fn_name}({prv}, {s})");
            }
            StepOp::RmPrefixSuffix { substr } => {
                let fn_name = if is_arr_out {
                    "rtRmPrefixSuffixArr"
                } else {
                    "rtRmPrefixSuffix"
                };
                self.require(fn_name, builder);
                let s = go_str(substr);
                wln!(writer, "{nxt} := {fn_name}({prv}, {s})");
            }
            StepOp::Fmt { template } => {
                let tmpl = go_str(template);
                if is_arr_out {
                    self.require("rtFmtArr", builder);
                    wln!(writer, "{nxt} := rtFmtArr({prv}, {tmpl})");
                } else {
                    self.require("rtFmt", builder);
                    wln!(writer, "{nxt} := rtFmt({tmpl}, {prv})");
                }
            }
            StepOp::Repl { old, new } => {
                let fn_name = if is_arr_out { "rtReplArr" } else { "rtRepl" };
                self.require(fn_name, builder);
                let old_s = go_str(old);
                let new_s = go_str(new);
                wln!(writer, "{nxt} := {fn_name}({prv}, {old_s}, {new_s})");
            }
            StepOp::ReplMap { replacements } => {
                let fn_name = if is_arr_out {
                    "rtReplMapArr"
                } else {
                    "rtReplMap"
                };
                self.require(fn_name, builder);
                let map_lit = go_str_map(replacements);
                wln!(writer, "{nxt} := {fn_name}({prv}, {map_lit})");
            }
            StepOp::Lower => {
                if is_arr_out {
                    self.require("rtLowerArr", builder);
                    wln!(writer, "{nxt} := rtLowerArr({prv})");
                } else {
                    builder.require_import("\"strings\"");
                    wln!(writer, "{nxt} := strings.ToLower({prv})");
                }
            }
            StepOp::Upper => {
                if is_arr_out {
                    self.require("rtUpperArr", builder);
                    wln!(writer, "{nxt} := rtUpperArr({prv})");
                } else {
                    builder.require_import("\"strings\"");
                    wln!(writer, "{nxt} := strings.ToUpper({prv})");
                }
            }
            StepOp::Split { sep } => {
                builder.require_import("\"strings\"");
                let sep_s = go_str(sep);
                wln!(writer, "{nxt} := strings.Split({prv}, {sep_s})");
            }
            StepOp::Join { sep } => {
                builder.require_import("\"strings\"");
                let sep_s = go_str(sep);
                wln!(writer, "{nxt} := strings.Join({prv}, {sep_s})");
            }
            StepOp::Unescape => {
                let fn_name = if is_arr_out {
                    "rtUnescapeArr"
                } else {
                    "rtUnescape"
                };
                self.require(fn_name, builder);
                wln!(writer, "{nxt} := {fn_name}({prv})");
            }
            StepOp::Re { pattern } => {
                let fn_name = if is_arr_out {
                    "rtReSearchArr"
                } else {
                    "rtReSearch"
                };
                self.require(fn_name, builder);
                let loc_str = if location.is_empty() {
                    String::new()
                } else {
                    format!(" at {location}")
                };
                let msg = format!(
                    "{}:{}:{} re-match failed{loc_str} pattern={pattern}",
                    module.source_file, step.span.line, step.span.col
                );
                let pat_s = go_regex_raw(pattern);
                let msg_s = go_str(&msg);
                if is_arr_out {
                    wln!(writer, "{nxt} := rtReSearchArr({prv}, {pat_s}, {msg_s})");
                } else {
                    wln!(writer, "{nxt} := rtReSearch({pat_s}, {prv}, {msg_s})");
                }
            }
            StepOp::ReAll { pattern } => {
                self.require("rtReAll", builder);
                let pat_s = go_regex_raw(pattern);
                wln!(writer, "{nxt} := rtReAll({pat_s}, {prv})");
            }
            StepOp::ReSub { pattern, repl } => {
                let pat_s = go_regex_raw(pattern);
                let repl_s = go_str(repl);
                if is_arr_out {
                    self.require("rtReSubArr", builder);
                    wln!(writer, "{nxt} := rtReSubArr({prv}, {pat_s}, {repl_s})");
                } else {
                    self.require("rtReSub", builder);
                    wln!(writer, "{nxt} := rtReSub({pat_s}, {repl_s}, {prv})");
                }
            }
            StepOp::ReSplit { pattern } => {
                self.require("rtReSplit", builder);
                let pat_s = go_regex_raw(pattern);
                wln!(writer, "{nxt} := rtReSplit({pat_s}, {prv})");
            }
            StepOp::Index { index } => {
                if step.input.base == BaseType::Document {
                    self.require("rtNodeIndex", builder);
                    wln!(writer, "{nxt} := rtNodeIndex({prv}, {index})");
                } else if step.input.base == BaseType::String && !step.input.is_array {
                    self.require("rtStrIndex", builder);
                    wln!(writer, "{nxt} := rtStrIndex({prv}, {index})");
                } else {
                    self.require("rtIndex", builder);
                    wln!(writer, "{nxt} := rtIndex({prv}, {index})");
                }
            }
            StepOp::Slice { start, end } => {
                if step.input.base == BaseType::Document {
                    self.require("rtNodeSlice", builder);
                    wln!(writer, "{nxt} := rtNodeSlice({prv}, {start}, {end})");
                } else if step.input.base == BaseType::String && !step.input.is_array {
                    self.require("rtStrSlice", builder);
                    wln!(writer, "{nxt} := rtStrSlice({prv}, {start}, {end})");
                } else {
                    self.require("rtSlice", builder);
                    wln!(writer, "{nxt} := rtSlice({prv}, {start}, {end})");
                }
            }
            StepOp::Len => {
                if step.input.base == BaseType::Document {
                    wln!(writer, "{nxt} := int64({prv}.Length())");
                } else if step.input.base == BaseType::String && !step.input.is_array {
                    builder.require_import("\"unicode/utf8\"");
                    wln!(writer, "{nxt} := int64(utf8.RuneCountInString({prv}))");
                } else {
                    wln!(writer, "{nxt} := int64(len({prv}))");
                }
            }
            StepOp::Unique { .. } => {
                self.require("rtUnique", builder);
                wln!(writer, "{nxt} := rtUnique({prv})");
            }
            StepOp::ToInt => {
                let fn_name = if is_arr_out { "rtToIntArr" } else { "rtToInt" };
                self.require(fn_name, builder);
                wln!(writer, "{nxt} := {fn_name}({prv})");
            }
            StepOp::ToFloat => {
                let fn_name = if is_arr_out {
                    "rtToFloatArr"
                } else {
                    "rtToFloat"
                };
                self.require(fn_name, builder);
                wln!(writer, "{nxt} := {fn_name}({prv})");
            }
            StepOp::ToBool => {
                if step.input.base == BaseType::Document {
                    wln!(writer, "{nxt} := {prv}.Length() > 0");
                } else if step.input.is_array {
                    wln!(writer, "{nxt} := len({prv}) > 0");
                } else if step.input.base == BaseType::String {
                    wln!(writer, "{nxt} := {prv} != \"\"");
                } else if matches!(step.input.base, BaseType::Int | BaseType::Float) {
                    wln!(writer, "{nxt} := {prv} != 0");
                } else if step.input.base == BaseType::Bool {
                    wln!(writer, "{nxt} := {prv}");
                } else {
                    self.require("rtToBool", builder);
                    wln!(writer, "{nxt} := rtToBool({prv})");
                }
            }
            StepOp::Jsonify {
                schema_name,
                path,
                parsed_path: _,
            } => {
                self.require("rtJsonify", builder);
                let effective_path = path
                    .as_deref()
                    .or_else(|| module.json_def(schema_name).and_then(|j| j.path.as_deref()))
                    .unwrap_or("");
                let path_s = go_str(effective_path);
                let mut ti = step.output.clone();
                ti.is_optional = false;
                ti.omitempty = false;
                let go_type = Self::resolve_type(&ti, module);
                wln!(writer, "var {nxt} {go_type}");
                wln!(writer, "rtJsonify({prv}, {path_s}, &{nxt})");
            }
            StepOp::Nested { struct_name } => {
                let pascal = to_pascal_case(struct_name);
                let target_decl = module.struct_decl(struct_name);
                let is_child_raw = target_decl.is_some_and(|s| s.kind == StructKind::Raw);
                let is_input_str = step.input.base == BaseType::String;
                let has_init = target_decl.is_some_and(|s| !s.init_fields.is_empty());
                if is_input_str && !is_child_raw {
                    wln!(writer, "_{nxt}Sub, _{nxt}Err := New{pascal}({prv})");
                    wln!(writer, "if _{nxt}Err != nil {{");
                    writer.indented(|writer| {
                        wln!(writer, "panic(_{nxt}Err)");
                    });
                    writer.writeln("}");
                    wln!(writer, "{nxt} := _{nxt}Sub.Parse()");
                } else if has_init {
                    wln!(writer, "_{nxt}Sub := &{pascal}{{sel: {prv}}}");
                    wln!(writer, "_{nxt}Sub.init()");
                    wln!(writer, "{nxt} := _{nxt}Sub.Parse()");
                } else {
                    wln!(writer, "{nxt} := (&{pascal}{{sel: {prv}}}).Parse()");
                }
            }
            StepOp::Filter { cond } => {
                let elem_type = TypeInfo {
                    is_array: false,
                    ..step.input.clone()
                };
                let expr = self.render_cond_expr(
                    cond,
                    true,
                    "i",
                    &elem_type,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                    builder,
                );
                let clean_expr = strip_outer_parens(&expr);
                if step.input.base == BaseType::Document {
                    wln!(
                        writer,
                        "{nxt} := {prv}.FilterFunction(func(_ int, i *goquery.Selection) bool {{"
                    );
                    writer.indented(|writer| {
                        wln!(writer, "return {clean_expr}");
                    });
                    writer.writeln("})");
                } else {
                    let elem_go = Self::resolve_type(&elem_type, module);
                    wln!(writer, "{nxt} := make([]{elem_go}, 0)");
                    wln!(writer, "for _, i := range {prv} {{");
                    writer.indented(|writer| {
                        wln!(writer, "if {clean_expr} {{");
                        writer.indented(|writer| {
                            wln!(writer, "{nxt} = append({nxt}, i)");
                        });
                        writer.writeln("}");
                    });
                    writer.writeln("}");
                }
            }
            StepOp::Assert { message, cond } => {
                self.require("rtAssert", builder);
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
                let msg_s = go_str(&msg);
                let expr = self.render_cond_expr(
                    cond,
                    true,
                    prv,
                    &step.input,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                    builder,
                );
                let clean_expr = strip_outer_parens(&expr);
                wln!(writer, "rtAssert({clean_expr}, {msg_s})");
                if !is_last_pre_validate_step {
                    wln!(writer, "{nxt} := {prv}");
                }
            }
            StepOp::Match { cond } => {
                let rcv = receiver(&self.current_struct_name);
                let key_type = TypeInfo::string();
                let expr = self.render_cond_expr(
                    cond,
                    true,
                    "_key",
                    &key_type,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                    builder,
                );
                let clean_expr = strip_outer_parens(&expr);
                let t_ret = self.current_table_field_ret.as_deref().unwrap_or("string");
                let zero = go_zero_value(t_ret);
                wln!(writer, "_key := {rcv}.tableMatchKey({prv})");
                wln!(writer, "if !({clean_expr}) {{");
                writer.indented(|writer| {
                    writer.writeln("_matched = false");
                    wln!(writer, "return {zero}");
                });
                writer.writeln("}");
                wln!(writer, "{nxt} := {rcv}.parseValue({prv})");
            }
            StepOp::ExtensionCall {
                qualified_name,
                bound_input,
                bound_output,
                ..
            } => {
                let in_type = Self::resolve_type(bound_input, module);
                let out_type = Self::resolve_type(bound_output, module);
                emit_extension_call(
                    module,
                    qualified_name,
                    step.span,
                    &["go"],
                    "'go'",
                    prv,
                    nxt,
                    &in_type,
                    &out_type,
                    |imp| render_go_import(&imp.value, imp.alias.as_deref()),
                    normalize_go_tabs,
                    builder,
                    writer,
                    &mut self.diagnostics,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_cond_expr(
        &mut self,
        expr: &Cond,
        is_root: bool,
        target: &str,
        input_type: &TypeInfo,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
        builder: &mut ModuleBuilder,
    ) -> String {
        match expr {
            Cond::Atom(op) => self.render_cond_op(
                op,
                target,
                input_type,
                span,
                source_file,
                is_dry_run,
                builder,
            ),
            Cond::Not(inner) => {
                let inner_s = self.render_cond_expr(
                    inner,
                    true,
                    target,
                    input_type,
                    span,
                    source_file,
                    is_dry_run,
                    builder,
                );
                format!("!({})", strip_outer_parens(&inner_s))
            }
            Cond::And(items) => {
                if items.is_empty() {
                    return "true".to_owned();
                }
                let joined = items
                    .iter()
                    .map(|it| {
                        self.render_cond_expr(
                            it,
                            false,
                            target,
                            input_type,
                            span,
                            source_file,
                            is_dry_run,
                            builder,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" && ");
                if is_root || items.len() == 1 {
                    joined
                } else {
                    format!("({joined})")
                }
            }
            Cond::Or(items) => {
                if items.is_empty() {
                    return "false".to_owned();
                }
                let joined = items
                    .iter()
                    .map(|it| {
                        self.render_cond_expr(
                            it,
                            false,
                            target,
                            input_type,
                            span,
                            source_file,
                            is_dry_run,
                            builder,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" || ");
                if is_root || items.len() == 1 {
                    joined
                } else {
                    format!("({joined})")
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_cond_op(
        &mut self,
        op: &CondOp,
        target: &str,
        input_type: &TypeInfo,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
        builder: &mut ModuleBuilder,
    ) -> String {
        let text_target = if input_type.base == BaseType::Document {
            format!("{target}.Text()")
        } else {
            target.to_owned()
        };

        match op {
            CondOp::Css { query } => {
                format!("{target}.Find({}).Length() > 0", go_str(query))
            }
            CondOp::Xpath { .. } => {
                if is_dry_run {
                    self.record_unsupported_xpath(span, source_file);
                }
                "false".to_owned()
            }
            CondOp::HasAttr { attrs } => {
                self.require("rtHasAttr", builder);
                any_of(attrs, |a| format!("rtHasAttr({target}, {})", go_str(a)))
            }
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                self.require("rtAttrOr", builder);
                let attr_expr = format!("rtAttrOr({target}, {})", go_str(name));
                any_of(values, |v| format!("{attr_expr} == {}", go_str(v)))
            }
            CondOp::AttrNe { name, values } => {
                self.require("rtAttrOr", builder);
                let attr_expr = format!("rtAttrOr({target}, {})", go_str(name));
                all_of(values, |v| format!("{attr_expr} != {}", go_str(v)))
            }
            CondOp::AttrStarts { name, values } => {
                builder.require_import("\"strings\"");
                self.require("rtAttrOr", builder);
                let attr_expr = format!("rtAttrOr({target}, {})", go_str(name));
                any_of(values, |v| {
                    format!("strings.HasPrefix({attr_expr}, {})", go_str(v))
                })
            }
            CondOp::AttrEnds { name, values } => {
                builder.require_import("\"strings\"");
                self.require("rtAttrOr", builder);
                let attr_expr = format!("rtAttrOr({target}, {})", go_str(name));
                any_of(values, |v| {
                    format!("strings.HasSuffix({attr_expr}, {})", go_str(v))
                })
            }
            CondOp::AttrContains { name, values } => {
                builder.require_import("\"strings\"");
                self.require("rtAttrOr", builder);
                let attr_expr = format!("rtAttrOr({target}, {})", go_str(name));
                any_of(values, |v| {
                    format!("strings.Contains({attr_expr}, {})", go_str(v))
                })
            }
            CondOp::AttrRe { name, pattern } => {
                self.require("rtReMatch", builder);
                self.require("rtAttrOr", builder);
                let rx = go_regex_raw(pattern);
                format!("rtReMatch({rx}, rtAttrOr({target}, {}))", go_str(name))
            }
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                any_of(values, |v| format!("{text_target} == {}", go_str(v)))
            }
            CondOp::TextNe { values } => {
                all_of(values, |v| format!("{text_target} != {}", go_str(v)))
            }
            CondOp::TextStarts { values } => {
                builder.require_import("\"strings\"");
                any_of(values, |v| {
                    format!("strings.HasPrefix({text_target}, {})", go_str(v))
                })
            }
            CondOp::TextEnds { values } => {
                builder.require_import("\"strings\"");
                any_of(values, |v| {
                    format!("strings.HasSuffix({text_target}, {})", go_str(v))
                })
            }
            CondOp::TextContains { values } => {
                builder.require_import("\"strings\"");
                any_of(values, |v| {
                    format!("strings.Contains({text_target}, {})", go_str(v))
                })
            }
            CondOp::TextRe { pattern } => {
                self.require("rtReMatch", builder);
                let rx = go_regex_raw(pattern);
                format!("rtReMatch({rx}, {text_target})")
            }
            CondOp::Contains { values } => {
                builder.require_import("\"strings\"");
                any_of(values, |v| {
                    format!("strings.Contains({text_target}, {})", go_str(v))
                })
            }
            CondOp::Starts { values } => {
                builder.require_import("\"strings\"");
                any_of(values, |v| {
                    format!("strings.HasPrefix({text_target}, {})", go_str(v))
                })
            }
            CondOp::Ends { values } => {
                builder.require_import("\"strings\"");
                any_of(values, |v| {
                    format!("strings.HasSuffix({text_target}, {})", go_str(v))
                })
            }
            CondOp::Re { pattern } => {
                self.require("rtReMatch", builder);
                let rx = go_regex_raw(pattern);
                format!("rtReMatch({rx}, {text_target})")
            }
            CondOp::ReAny { pattern } => {
                self.require("rtReAnyMatch", builder);
                let rx = go_regex_raw(pattern);
                format!("rtReAnyMatch({rx}, {target})")
            }
            CondOp::ReAll { pattern } => {
                self.require("rtReAllMatch", builder);
                let rx = go_regex_raw(pattern);
                format!("rtReAllMatch({rx}, {target})")
            }
            CondOp::Eq { values } | CondOp::In { values } => {
                if !values.is_empty()
                    && values.iter().all(|v| matches!(v, CondValue::Int(_)))
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array)
                {
                    let cnt = self.len_expr(target, input_type, builder);
                    any_of(values, |v| format!("{cnt} == {}", go_pred_value(v)))
                } else {
                    any_of(values, |v| format!("{text_target} == {}", go_pred_value(v)))
                }
            }
            CondOp::Ne { values } => {
                if !values.is_empty()
                    && values.iter().all(|v| matches!(v, CondValue::Int(_)))
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array)
                {
                    let cnt = self.len_expr(target, input_type, builder);
                    all_of(values, |v| format!("{cnt} != {}", go_pred_value(v)))
                } else {
                    all_of(values, |v| format!("{text_target} != {}", go_pred_value(v)))
                }
            }
            CondOp::Gt { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} > {value}")
                } else {
                    let cnt = self.len_expr(target, input_type, builder);
                    format!("{cnt} > {value}")
                }
            }
            CondOp::Lt { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} < {value}")
                } else {
                    let cnt = self.len_expr(target, input_type, builder);
                    format!("{cnt} < {value}")
                }
            }
            CondOp::Ge { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} >= {value}")
                } else {
                    let cnt = self.len_expr(target, input_type, builder);
                    format!("{cnt} >= {value}")
                }
            }
            CondOp::Le { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("{target} <= {value}")
                } else {
                    let cnt = self.len_expr(target, input_type, builder);
                    format!("{cnt} <= {value}")
                }
            }
            CondOp::LenEq { values } => {
                let cnt = self.len_expr(target, input_type, builder);
                any_of(values, |n| format!("{cnt} == {n}"))
            }
            CondOp::LenNe { values } => {
                let cnt = self.len_expr(target, input_type, builder);
                all_of(values, |n| format!("{cnt} != {n}"))
            }
            CondOp::LenGt { value } => {
                let cnt = self.len_expr(target, input_type, builder);
                format!("{cnt} > {value}")
            }
            CondOp::LenLt { value } => {
                let cnt = self.len_expr(target, input_type, builder);
                format!("{cnt} < {value}")
            }
            CondOp::LenGe { value } => {
                let cnt = self.len_expr(target, input_type, builder);
                format!("{cnt} >= {value}")
            }
            CondOp::LenLe { value } => {
                let cnt = self.len_expr(target, input_type, builder);
                format!("{cnt} <= {value}")
            }
            CondOp::LenRange { start, end } => {
                let cnt = self.len_expr(target, input_type, builder);
                format!("({start} < {cnt} && {cnt} < {end})")
            }
        }
    }

    fn len_expr(&self, target: &str, input_type: &TypeInfo, builder: &mut ModuleBuilder) -> String {
        if input_type.base == BaseType::Document {
            format!("int64({target}.Length())")
        } else if input_type.is_array {
            format!("int64(len({target}))")
        } else if input_type.base == BaseType::String {
            builder.require_import("\"unicode/utf8\"");
            format!("int64(utf8.RuneCountInString({target}))")
        } else {
            self.require("rtLen", builder);
            format!("rtLen({target})")
        }
    }

    fn record_unsupported_xpath(&mut self, span: Span, source_file: &str) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E203,
                "XPath operations ('xpath', 'xpath-all', 'xpath-remove') are not supported by the 'go' backend",
                span,
            )
            .with_path(source_file.to_owned()),
        );
    }
}

impl LanguageEmitter for GoEmitter {
    fn indent_unit(&self) -> &'static str {
        "\t"
    }

    fn on_module_start(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let pkg = if self.options.package.is_empty() {
            "main"
        } else {
            self.options.package.as_str()
        };
        writer.writeln("// Code generated by parsmith. DO NOT EDIT.");
        writer.blankline();
        wln!(writer, "package {pkg}");
        writer.blankline();
        if let Some(ref doc) = module.doc {
            emit_go_doc(doc, writer);
            writer.blankline();
        }
        if module_has_html(module) {
            builder.require_import("\"github.com/PuerkitoBio/goquery\"");
            builder.require_import("\"strings\"");
        }
    }

    fn emit_imports(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let mut raw_imports: Vec<String> = builder.imports().iter().cloned().collect();

        if !self.options.separate_runtime {
            for imp in builder.std_imports() {
                raw_imports.push(imp.to_owned());
            }
            for imp in builder.runtime_imports() {
                raw_imports.push(imp.to_owned());
            }
            if module_uses_http(module) {
                for &imp in REST_IMPORTS {
                    raw_imports.push(imp.to_owned());
                }
            }
        }

        let sorted = sort_go_imports(raw_imports);
        if !sorted.is_empty() {
            writer.writeln("import (");
            writer.indented(|writer| {
                for imp in &sorted {
                    writer.writeln(imp);
                }
            });
            writer.writeln(")");
            writer.blankline();
        }
    }

    fn emit_utilities(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.options.separate_runtime {
            return;
        }

        if module_uses_http(module) {
            writer.write_block(BASE_REST_RUNTIME);
            writer.blankline();
            writer.write_block(NETHTTP_REST_RUNTIME);
            writer.blankline();
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
        self.emit_json_def_impl(json_def, module, builder, writer);
    }

    fn emit_typedef(
        &mut self,
        typedef: &StructTypeDef,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = to_pascal_case(&typedef.name);
        match typedef.struct_kind {
            StructKind::Rest => {}
            StructKind::Flat => {
                wln!(writer, "type {name}Type = []string");
                writer.blankline();
            }
            StructKind::Dict => {
                let val_type = typedef
                    .fields
                    .iter()
                    .find(|f| to_camel_case(&f.name) == "value")
                    .map(|f| Self::resolve_type(&f.type_info, module))
                    .unwrap_or_else(|| "any".to_owned());
                wln!(writer, "type {name}Type = map[string]{val_type}");
                writer.blankline();
            }
            StructKind::Table => {
                wln!(writer, "type {name}Type = map[string]any");
                writer.blankline();
            }
            StructKind::Item | StructKind::List | StructKind::Raw => {
                let mut struct_fields = Vec::with_capacity(typedef.fields.len());
                for field in &typedef.fields {
                    let fname = to_pascal_case(&field.name);
                    let ftype = Self::resolve_type(&field.type_info, module);
                    let raw_tag = to_snake_case(&field.name);
                    let omitempty = if field.type_info.omitempty {
                        ",omitempty"
                    } else {
                        ""
                    };
                    let tag = format!("`json:\"{raw_tag}{omitempty}\"`");
                    struct_fields.push((fname, ftype, Some(tag)));
                }
                wln!(writer, "type {name}Type struct {{");
                writer.indented(|writer| {
                    emit_aligned_struct_fields(&struct_fields, writer);
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
        let name = to_pascal_case(&struct_decl.name);
        self.current_struct_name = name.clone();

        if let Some(ref doc) = struct_decl.doc {
            emit_go_doc(doc, writer);
        }

        if struct_decl.kind == StructKind::Rest {
            wln!(writer, "type {name} struct {{");
            writer.writeln("}");
            writer.blankline();
            wln!(writer, "func New{name}() {name} {{ return {name}{{}} }}");
            writer.blankline();
            return;
        }

        let sel_type = if struct_decl.kind == StructKind::Raw {
            "string".to_owned()
        } else {
            "*goquery.Selection".to_owned()
        };

        let mut struct_fields = Vec::with_capacity(1 + struct_decl.init_fields.len());
        struct_fields.push(("sel".to_owned(), sel_type, None));
        for init_f in &struct_decl.init_fields {
            let cn = to_camel_case(&init_f.name);
            let gt = Self::resolve_type(&init_f.type_info, module);
            struct_fields.push((cn, gt, None));
        }

        wln!(writer, "type {name} struct {{");
        writer.indented(|writer| {
            emit_aligned_struct_fields(&struct_fields, writer);
        });
        writer.writeln("}");
        writer.blankline();
    }

    fn emit_init(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&name);
        let is_raw = struct_decl.kind == StructKind::Raw;

        if is_raw {
            wln!(writer, "func New{name}(input string) *{name} {{");
            writer.indented(|writer| {
                wln!(writer, "{rcv} := &{name}{{sel: input}}");
                if !struct_decl.init_fields.is_empty() {
                    wln!(writer, "{rcv}.init()");
                }
                wln!(writer, "return {rcv}");
            });
            writer.writeln("}");
        } else {
            wln!(writer, "func New{name}(input string) (*{name}, error) {{");
            writer.indented(|writer| {
                writer
                    .writeln("doc, err := goquery.NewDocumentFromReader(strings.NewReader(input))");
                writer.writeln("if err != nil {");
                writer.indented(|writer| {
                    writer.writeln("return nil, err");
                });
                writer.writeln("}");
                wln!(writer, "{rcv} := &{name}{{sel: doc.Selection}}");
                if !struct_decl.init_fields.is_empty() {
                    wln!(writer, "{rcv}.init()");
                }
                wln!(writer, "return {rcv}, nil");
            });
            writer.writeln("}");
        }
        writer.blankline();

        if !struct_decl.init_fields.is_empty() {
            wln!(writer, "func ({rcv} *{name}) init() {{");
            writer.indented(|writer| {
                for init_f in &struct_decl.init_fields {
                    let cn = to_camel_case(&init_f.name);
                    let pascal = to_pascal_case(&init_f.name);
                    wln!(writer, "{rcv}.{cn} = {rcv}.init{pascal}({rcv}.sel)");
                }
            });
            writer.writeln("}");
            writer.blankline();
        }
    }

    fn emit_init_field(
        &mut self,
        struct_decl: &StructDecl,
        init_field: &InitFieldDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let pascal = to_pascal_case(&init_field.name);
        let t_arg = Self::resolve_type(&init_field.pipeline.input, module);
        let t_ret = Self::resolve_type(&init_field.type_info, module);
        let location = format!("{struct_name}.@init.{}", init_field.name);

        wln!(
            writer,
            "func ({rcv} *{struct_name}) init{pascal}({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                &init_field.pipeline,
                &v0,
                &t_ret,
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let t_arg = Self::resolve_type(&pipeline.input, module);
        let location = format!("{struct_name}.@pre-validate");

        wln!(
            writer,
            "func ({rcv} *{struct_name}) preValidate({v0} {t_arg}) {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, "", &location, true, module, builder, writer);
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let method_name = to_camel_case(&check.name);
        let t_ret = Self::resolve_type(&check.pipeline.output, module);
        let location = format!("{struct_name}.{}", check.name);

        wln!(
            writer,
            "func ({rcv} *{struct_name}) {method_name}() {t_ret} {{"
        );
        writer.indented(|writer| {
            wln!(writer, "{v0} := {rcv}.sel");
            self.emit_pipeline_body(
                &check.pipeline,
                &v0,
                &t_ret,
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let t_arg = Self::resolve_type(&pipeline.input, module);
        let t_ret = Self::resolve_type(&pipeline.output, module);
        let location = format!("{struct_name}.@split-doc");

        wln!(
            writer,
            "func ({rcv} *{struct_name}) splitDoc({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline, &v0, &t_ret, &location, false, module, builder, writer,
            );
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let t_arg = Self::resolve_type(&pipeline.input, module);
        let t_ret = Self::resolve_type(&pipeline.output, module);
        let location = format!("{struct_name}.key");

        wln!(
            writer,
            "func ({rcv} *{struct_name}) parseKey({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline, &v0, &t_ret, &location, false, module, builder, writer,
            );
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let t_arg = Self::resolve_type(&pipeline.input, module);
        let t_ret = Self::resolve_type(&pipeline.output, module);
        let location = format!("{struct_name}.value");

        wln!(
            writer,
            "func ({rcv} *{struct_name}) parseValue({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline, &v0, &t_ret, &location, false, module, builder, writer,
            );
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let t_arg = Self::resolve_type(&pipeline.input, module);
        let t_ret = Self::resolve_type(&pipeline.output, module);
        let location = format!("{struct_name}.@table");

        wln!(
            writer,
            "func ({rcv} *{struct_name}) tableConfig({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline, &v0, &t_ret, &location, false, module, builder, writer,
            );
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let t_arg = Self::resolve_type(&pipeline.input, module);
        let t_ret = Self::resolve_type(&pipeline.output, module);
        let location = format!("{struct_name}.@rows");

        wln!(
            writer,
            "func ({rcv} *{struct_name}) tableRows({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline, &v0, &t_ret, &location, false, module, builder, writer,
            );
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let t_arg = Self::resolve_type(&pipeline.input, module);
        let t_ret = Self::resolve_type(&pipeline.output, module);
        let location = format!("{struct_name}.@match");

        wln!(
            writer,
            "func ({rcv} *{struct_name}) tableMatchKey({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline, &v0, &t_ret, &location, false, module, builder, writer,
            );
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let struct_name = to_pascal_case(&struct_decl.name);
        let rcv = receiver(&struct_name);
        let mn = go_method_name(&field.name);
        let t_arg = Self::resolve_type(&field.pipeline.input, module);
        let t_ret = Self::resolve_type(&field.type_info, module);
        let location = format!("{struct_name}.{}", field.name);

        if struct_decl.kind == StructKind::Table {
            self.current_table_field_ret = Some(t_ret.clone());
            wln!(
                writer,
                "func ({rcv} *{struct_name}) {mn}({v0} *goquery.Selection) ({t_ret}, bool) {{"
            );
            writer.indented(|writer| {
                writer.writeln("_matched := true");
                wln!(writer, "_result := func() {t_ret} {{");
                writer.indented(|writer| {
                    self.emit_pipeline_body(
                        &field.pipeline,
                        &v0,
                        &t_ret,
                        &location,
                        false,
                        module,
                        builder,
                        writer,
                    );
                });
                writer.writeln("}()");
                writer.writeln("return _result, _matched");
            });
            writer.writeln("}");
            writer.blankline();
            self.current_table_field_ret = None;
            return;
        }

        wln!(
            writer,
            "func ({rcv} *{struct_name}) {mn}({v0} {t_arg}) {t_ret} {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                &field.pipeline,
                &v0,
                &t_ret,
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
        let rcv = receiver(&name);
        let ret_type = Self::resolve_parse_return_type(struct_decl);

        wln!(writer, "func ({rcv} *{name}) Parse() {ret_type} {{");
        writer.indented(|writer| {
            if struct_decl.pre_validate.is_some() {
                wln!(writer, "{rcv}.preValidate({rcv}.sel)");
            }

            match struct_decl.kind {
                StructKind::Item => {
                    Self::emit_composite_return(
                        &name,
                        &rcv,
                        &struct_decl.fields,
                        &format!("{rcv}.sel"),
                        writer,
                    );
                }
                StructKind::Raw => {
                    if struct_decl.split_doc.is_some() {
                        wln!(writer, "rows := {rcv}.splitDoc({rcv}.sel)");
                        wln!(writer, "result := make([]{name}Type, 0, len(rows))");
                        writer.writeln("for _, item := range rows {");
                        writer.indented(|writer| {
                            Self::emit_composite_append(
                                &name,
                                &rcv,
                                &struct_decl.fields,
                                "item",
                                writer,
                            );
                        });
                        writer.writeln("}");
                        writer.writeln("return result");
                    } else {
                        Self::emit_composite_return(
                            &name,
                            &rcv,
                            &struct_decl.fields,
                            &format!("{rcv}.sel"),
                            writer,
                        );
                    }
                }
                StructKind::List => {
                    wln!(writer, "rows := {rcv}.splitDoc({rcv}.sel)");
                    wln!(writer, "result := make([]{name}Type, 0, rows.Length())");
                    writer.writeln("rows.Each(func(_ int, item *goquery.Selection) {");
                    writer.indented(|writer| {
                        Self::emit_composite_append(
                            &name,
                            &rcv,
                            &struct_decl.fields,
                            "item",
                            writer,
                        );
                    });
                    writer.writeln("})");
                    writer.writeln("return result");
                }
                StructKind::Flat => {
                    self.require("rtUnique", builder);
                    writer.writeln("result := make([]string, 0)");
                    for field in &struct_decl.fields {
                        let mn = go_method_name(&field.name);
                        let camel = to_camel_case(&field.name);
                        if field.type_info.is_optional {
                            wln!(
                                writer,
                                "if _{camel} := {rcv}.{mn}({rcv}.sel); _{camel} != nil {{"
                            );
                            writer.indented(|writer| {
                                if field.type_info.is_array {
                                    wln!(writer, "result = append(result, (*_{camel})...)");
                                } else {
                                    wln!(writer, "result = append(result, *_{camel})");
                                }
                            });
                            writer.writeln("}");
                        } else if field.type_info.is_array {
                            wln!(writer, "result = append(result, {rcv}.{mn}({rcv}.sel)...)");
                        } else {
                            wln!(writer, "result = append(result, {rcv}.{mn}({rcv}.sel))");
                        }
                    }
                    writer.writeln("return rtUnique(result)");
                }
                StructKind::Dict => {
                    wln!(writer, "rows := {rcv}.splitDoc({rcv}.sel)");
                    wln!(writer, "result := make({name}Type)");
                    writer.writeln("rows.Each(func(_ int, item *goquery.Selection) {");
                    writer.indented(|writer| {
                        wln!(
                            writer,
                            "result[{rcv}.parseKey(item)] = {rcv}.parseValue(item)"
                        );
                    });
                    writer.writeln("})");
                    writer.writeln("return result");
                }
                StructKind::Table => {
                    wln!(writer, "result := make({name}Type)");
                    wln!(writer, "table := {rcv}.tableConfig({rcv}.sel)");
                    wln!(
                        writer,
                        "{rcv}.tableRows(table).Each(func(_ int, _row *goquery.Selection) {{"
                    );
                    writer.indented(|writer| {
                        for field in &struct_decl.fields {
                            let mn = go_method_name(&field.name);
                            let tag = to_snake_case(&field.name);
                            let tag_lit = go_str(&tag);
                            wln!(writer, "if _val, _ok := {rcv}.{mn}(_row); _ok {{");
                            writer.indented(|writer| {
                                wln!(writer, "if _, _exists := result[{tag_lit}]; !_exists {{");
                                writer.indented(|writer| {
                                    wln!(writer, "result[{tag_lit}] = _val");
                                });
                                writer.writeln("}");
                            });
                            writer.writeln("}");
                        }
                    });
                    writer.writeln("})");
                    writer.writeln("return result");
                }
                StructKind::Rest => {}
            }
        });
        writer.writeln("}");
        writer.blankline();
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

    fn emit_function(
        &mut self,
        func: &FunctionDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.reset_vars();
        let v0 = writer.next_var();
        let name = to_pascal_case(&func.name);
        let t_ret = Self::resolve_type(&func.return_type, module);

        if let Some(ref doc) = func.doc {
            emit_go_doc(doc, writer);
        }

        wln!(writer, "func {name}(document string) {t_ret} {{");
        writer.indented(|writer| {
            if func.is_raw {
                wln!(writer, "{v0} := document");
            } else {
                builder.require_import("\"strings\"");
                writer.writeln(
                    "doc, err := goquery.NewDocumentFromReader(strings.NewReader(document))",
                );
                writer.writeln("if err != nil {");
                writer.indented(|writer| {
                    writer.writeln("panic(err)");
                });
                writer.writeln("}");
                wln!(writer, "{v0} := doc.Selection");
            }
            self.emit_pipeline_body(
                &func.pipeline,
                &v0,
                &t_ret,
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

impl GoEmitter {
    fn emit_composite_return(
        name: &str,
        rcv: &str,
        fields: &[FieldDecl],
        arg_expr: &str,
        writer: &mut CodeWriter,
    ) {
        if fields.is_empty() {
            wln!(writer, "return {name}Type{{}}");
            return;
        }
        let entries: Vec<(String, String)> = fields
            .iter()
            .map(|f| {
                let fn_name = to_pascal_case(&f.name);
                let mn = go_method_name(&f.name);
                (fn_name, format!("{rcv}.{mn}({arg_expr})"))
            })
            .collect();
        wln!(writer, "return {name}Type{{");
        writer.indented(|writer| {
            emit_aligned_composite_fields(&entries, writer);
        });
        writer.writeln("}");
    }

    fn emit_composite_append(
        name: &str,
        rcv: &str,
        fields: &[FieldDecl],
        arg_expr: &str,
        writer: &mut CodeWriter,
    ) {
        if fields.is_empty() {
            wln!(writer, "result = append(result, {name}Type{{}})");
            return;
        }
        let entries: Vec<(String, String)> = fields
            .iter()
            .map(|f| {
                let fn_name = to_pascal_case(&f.name);
                let mn = go_method_name(&f.name);
                (fn_name, format!("{rcv}.{mn}({arg_expr})"))
            })
            .collect();
        wln!(writer, "result = append(result, {name}Type{{");
        writer.indented(|writer| {
            emit_aligned_composite_fields(&entries, writer);
        });
        writer.writeln("})");
    }
}

fn go_fallback_value(lit: &LiteralValue, go_type: &str) -> String {
    if go_type.starts_with("[]") {
        return format!("{go_type}{{}}");
    }
    if go_type.starts_with('*') || matches!(lit, LiteralValue::Null) {
        return "nil".to_owned();
    }
    match lit {
        LiteralValue::Bool(b) => b.to_string(),
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => go_str(s),
        LiteralValue::EmptyArray => format!("{go_type}{{}}"),
        LiteralValue::Null => "nil".to_owned(),
    }
}

fn go_pred_value(val: &CondValue) -> String {
    match val {
        CondValue::Int(n) => n.to_string(),
        CondValue::Float(f) => format!("{f:?}"),
        CondValue::Bool(b) => b.to_string(),
        CondValue::String(s) => go_str(s),
    }
}

fn render_go_import(value: &str, alias: Option<&str>) -> String {
    let spec = if value.starts_with('"') && value.ends_with('"') {
        value.to_owned()
    } else {
        go_str(value)
    };
    match alias.filter(|a| !a.is_empty()) {
        Some(a) => format!("{a} {spec}"),
        None => spec,
    }
}

fn any_of<T, F: Fn(&T) -> String>(items: &[T], fmt_fn: F) -> String {
    let parts: Vec<String> = items.iter().map(fmt_fn).collect();
    if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        format!("({})", parts.join(" || "))
    }
}

fn all_of<T, F: Fn(&T) -> String>(items: &[T], fmt_fn: F) -> String {
    let parts: Vec<String> = items.iter().map(fmt_fn).collect();
    if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        format!("({})", parts.join(" && "))
    }
}

fn finalize_go_source(mut src: String) -> String {
    let trimmed_len = src.trim_end_matches('\n').len();
    src.truncate(trimmed_len);
    src.push('\n');
    src
}

/// Generates the standalone Go shared runtime file (`<runtime_name>.go`) for `-R / --separate-runtime`.
pub fn emit_go_runtime_module(
    module: &Module,
    builder: &ModuleBuilder,
    options: &CodegenOptions,
) -> String {
    let pkg = if options.package.is_empty() {
        "main"
    } else {
        options.package.as_str()
    };
    let mut writer = CodeWriter::with_indent("\t");
    writer.writeln("// Code generated by parsmith. DO NOT EDIT.");
    writer.blankline();
    wln!(writer, "package {pkg}");
    writer.blankline();

    let has_http = module_uses_http(module);
    let mut raw_imports: Vec<String> = Vec::new();
    for imp in builder.std_imports() {
        raw_imports.push(imp.to_owned());
    }
    for imp in builder.runtime_imports() {
        raw_imports.push(imp.to_owned());
    }
    if has_http {
        for &imp in REST_IMPORTS {
            raw_imports.push(imp.to_owned());
        }
    }

    let sorted = sort_go_imports(raw_imports);
    if !sorted.is_empty() {
        writer.writeln("import (");
        writer.indented(|writer| {
            for imp in &sorted {
                writer.writeln(imp);
            }
        });
        writer.writeln(")");
        writer.blankline();
    }

    writer.write_block(BASE_RUNTIME);
    writer.blankline();

    if has_http {
        writer.write_block(BASE_REST_RUNTIME);
        writer.blankline();
        writer.write_block(NETHTTP_REST_RUNTIME);
        writer.blankline();
    }

    for def in builder.runtime_defs().values() {
        writer.write_block(&def.code);
        writer.blankline();
    }
    for (name, def) in builder.std_defs() {
        if name != "rtFallback" {
            writer.write_block(&def.code);
            writer.blankline();
        }
    }

    finalize_go_source(writer.finish())
}

/// Emits Go source files for a compiled [`Module`] and [`CodegenOptions`].
///
/// Returns `(parser_source, optional_runtime_source, builder)` or `Err(diagnostics)`.
pub fn emit_go_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, Option<String>, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = GoEmitter::new(options.clone());
    let (parser_code, builder) = drive_module(
        &mut emitter,
        module,
        options.prologue.as_deref(),
        options.epilogue.as_deref(),
    );
    if !emitter.diagnostics.is_empty() {
        return Err(emitter.diagnostics);
    }
    let formatted_parser = finalize_go_source(parser_code);
    let runtime_code = if options.separate_runtime {
        Some(emit_go_runtime_module(module, &builder, options))
    } else {
        None
    };
    Ok((formatted_parser, runtime_code, builder))
}

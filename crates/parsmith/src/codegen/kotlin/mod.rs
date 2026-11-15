//! Kotlin 2.x / JVM (`org.jsoup:jsoup` + `kotlinx.serialization.json` + `java.net.http.HttpClient`)
//! target backend (`KotlinEmitter`).
//!
//! Emits pre-formatted 4-space indented Kotlin source code directly from [`CodeWriter`],
//! defaulting to a single self-contained `<stem>.kt` file with an inline `object Rt { ... }`
//! (omitting `org.jsoup` when `!module_has_html(module)`), while supporting `-R / --separate-runtime`
//! (`<stem>.kt` + `<runtime_name>.kt`) and `--package <pkg>`.

pub mod json;
pub mod rest;
pub mod runtime;

use std::fmt::Write as _;

use indexmap::IndexSet;

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, FieldDecl, FunctionDef, InitFieldDecl, JsonDef,
    LiteralValue, Module, Pipeline, RawMode, RequestDecl, RestArtifact, RestEndpointDecl, StepOp,
    StructDecl, StructKind, StructTypeDef, TypeInfo, TypedStep,
};
use crate::codegen::CodegenOptions;
use crate::codegen::builder::{ModuleBuilder, emit_extension_call};
use crate::codegen::emitter::{LanguageEmitter, drive_module};
use crate::codegen::writer::{CodeWriter, wln};
use crate::diagnostic::Diagnostic;
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::{
    KOTLIN_CANNOT_BE_RAW, KOTLIN_RESERVED, is_valid_symbol, to_camel_case, to_snake_case,
};
use crate::lowering::clean_doc;
use crate::validators::normalize_regex_pattern;

pub use rest::{kotlin_ph_type, kotlin_variant_name, render_json_body, render_template};
pub use runtime::{
    KOTLIN_CORE_RUNTIME, KOTLIN_HTML_RUNTIME, KOTLIN_HTTP_RUNTIME, collect_kotlin_runtime_imports,
    emit_kotlin_runtime_module, write_kotlin_runtime_body,
};

/// Validates that `pkg` is either empty / `"main"` (omitted) or a `.`-separated sequence of valid Kotlin identifiers.
pub fn validate_kotlin_package_name(pkg: &str) -> bool {
    if pkg.is_empty() || pkg == "main" {
        return true;
    }
    pkg.split('.')
        .all(|seg| !seg.is_empty() && is_valid_symbol("kotlin", seg))
}

/// Renders the optional `package <pkg>` line when `pkg` is non-empty and not `"main"`.
pub fn render_kotlin_package(pkg: &str) -> Option<String> {
    if pkg.is_empty() || pkg == "main" {
        return None;
    }
    let segments: Vec<String> = pkg
        .split('.')
        .map(|seg| {
            if KOTLIN_RESERVED.contains(&seg) {
                format!("`{seg}`")
            } else {
                seg.to_owned()
            }
        })
        .collect();
    Some(format!("package {}", segments.join(".")))
}

fn kotlin_regex_str(pattern: &str) -> String {
    kotlin_str(&normalize_regex_pattern(pattern))
}

/// Formats a `&str` as a valid double-quoted Kotlin string literal (`"..."`),
/// escaping `$` as `\$` and control characters as `\uXXXX`.
pub fn kotlin_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Renders a DSL identifier as a valid `camelCase` Kotlin identifier, escaping reserved keywords with backticks.
pub fn kotlin_ident(raw: &str) -> String {
    let clean = raw.trim_end_matches('?');
    let name = to_camel_case(&to_snake_case(clean));
    let name = if name.is_empty() {
        "value".to_owned()
    } else {
        name
    };
    if KOTLIN_RESERVED.contains(&name.as_str()) && !KOTLIN_CANNOT_BE_RAW.contains(&name.as_str()) {
        format!("`{name}`")
    } else {
        name
    }
}

/// Renders a DSL identifier as a `PascalCase` Kotlin type name, escaping reserved keywords with backticks.
pub fn kotlin_pascal(raw: &str) -> String {
    let clean = raw.trim_end_matches('?');
    let name = to_pascal_case(clean).replace('-', "");
    let name = if name.is_empty() {
        "Value".to_owned()
    } else {
        name
    };
    if KOTLIN_RESERVED.contains(&name.as_str()) && !KOTLIN_CANNOT_BE_RAW.contains(&name.as_str()) {
        format!("`{name}`")
    } else {
        name
    }
}

/// Maps a dictionary key [`TypeInfo`] to its Kotlin type representation.
pub fn kotlin_key_type(info: Option<&TypeInfo>) -> &'static str {
    let Some(info) = info else {
        return "String";
    };
    match info.base {
        BaseType::String => "String",
        BaseType::Int => "Long",
        BaseType::Float => "Double",
        BaseType::Bool => "Boolean",
        _ => "String",
    }
}

/// Formats a [`LiteralValue`] as a Kotlin literal expression.
pub fn kotlin_literal(val: &LiteralValue) -> String {
    match val {
        LiteralValue::Null => "null".to_owned(),
        LiteralValue::Bool(b) => b.to_string(),
        LiteralValue::Int(n) => format!("{n}L"),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => kotlin_str(s),
        LiteralValue::EmptyArray => "emptyList()".to_owned(),
    }
}

/// Translates DSL/Python numeric replacement groups (`\1` or `$1`) into Kotlin/Java `Regex.replace`
/// replacement syntax, escaping literal dollar signs as `\$`.
pub fn kotlin_replacement(replacement: &str) -> String {
    let mut out = String::with_capacity(replacement.len() + 4);
    let bytes = replacement.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    while i < n {
        if bytes[i] == b'\\' && i + 1 < n && bytes[i + 1].is_ascii_digit() && bytes[i + 1] != b'0' {
            let start = i + 1;
            let mut end = start + 1;
            while end < n && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let group = &replacement[start..end];
            let _ = write!(out, "${group}");
            i = end;
        } else if bytes[i] == b'$' {
            if i + 1 < n && bytes[i + 1] == b'$' {
                out.push_str("\\$");
                i += 2;
            } else if i + 1 < n && bytes[i + 1].is_ascii_digit() && bytes[i + 1] != b'0' {
                let start = i + 1;
                let mut end = start + 1;
                while end < n && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let group = &replacement[start..end];
                let _ = write!(out, "${group}");
                i = end;
            } else {
                out.push_str("\\$");
                i += 1;
            }
        } else if bytes[i] == b'\\' {
            out.push_str("\\\\");
            i += 1;
        } else {
            let ch = replacement[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }

    out
}

/// Writes cleaned Kotlin documentation lines (`// ...`) to `writer`.
pub fn emit_kotlin_doc(doc: &str, prefix: &str, writer: &mut CodeWriter) {
    let cleaned = clean_doc(doc);
    if cleaned.is_empty() {
        return;
    }
    for line in cleaned.lines() {
        if line.trim().is_empty() {
            writer.writeln(prefix);
        } else {
            wln!(writer, "{prefix} {line}");
        }
    }
}

fn join_or(exprs: &[String]) -> String {
    if exprs.is_empty() {
        "false".to_owned()
    } else if exprs.len() == 1 {
        exprs[0].clone()
    } else {
        format!("({})", exprs.join(" || "))
    }
}

fn join_and(exprs: &[String]) -> String {
    if exprs.is_empty() {
        "true".to_owned()
    } else if exprs.len() == 1 {
        exprs[0].clone()
    } else {
        format!("({})", exprs.join(" && "))
    }
}

/// Kotlin 2.x target `LanguageEmitter` implementation.
pub struct KotlinEmitter {
    /// Code generation options.
    pub options: CodegenOptions,
    /// Diagnostics accumulated during code generation (`E400`).
    pub diagnostics: Vec<Diagnostic>,
    /// Tracks already-emitted REST `Result` sealed interfaces and typealiases within the current pass.
    pub(crate) emitted_aliases: IndexSet<String>,
}

impl KotlinEmitter {
    /// Creates a new `KotlinEmitter` for `options`.
    pub fn new(options: CodegenOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            emitted_aliases: IndexSet::new(),
        }
    }

    /// Resolves a [`TypeInfo`] into its Kotlin type annotation.
    pub fn resolve_type(info: &TypeInfo, module: &Module) -> String {
        let optional = info.is_optional || info.omitempty;
        if info.is_dict {
            let key_type = kotlin_key_type(info.key_type.as_deref());
            let val_type = info
                .value_type
                .as_deref()
                .map(|vt| Self::resolve_type(vt, module))
                .unwrap_or_else(|| "String".to_owned());
            let mut res = format!("Map<{key_type}, {val_type}>");
            if info.is_array {
                res = format!("List<{res}>");
            }
            if optional {
                res = format!("{res}?");
            }
            return res;
        }

        let mut res = match &info.base {
            BaseType::Document => {
                if info.is_array {
                    "List<Element>".to_owned()
                } else {
                    "Element".to_owned()
                }
            }
            BaseType::Nested(ref_name) => Self::resolve_struct_result_type(ref_name, module),
            BaseType::Json(ref_name) => {
                let pascal = kotlin_pascal(ref_name);
                let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
                if info.is_array || def_is_arr {
                    format!("List<{pascal}Json>")
                } else {
                    format!("{pascal}Json")
                }
            }
            BaseType::String | BaseType::Auto | BaseType::DictValue => "String".to_owned(),
            BaseType::Int => "Long".to_owned(),
            BaseType::Float => "Double".to_owned(),
            BaseType::Bool => "Boolean".to_owned(),
            BaseType::Null => "JsonElement?".to_owned(),
        };

        if info.is_array
            && !matches!(
                info.base,
                BaseType::Document | BaseType::Nested(_) | BaseType::Json(_)
            )
        {
            res = format!("List<{res}>");
        }
        if optional && !res.ends_with('?') {
            res = format!("{res}?");
        }
        res
    }

    /// Resolves the return type of a struct's `.parse()` method.
    pub fn resolve_struct_result_type(name: &str, module: &Module) -> String {
        let base = format!("{}Type", kotlin_pascal(name));
        let Some(struct_decl) = module.struct_decl(name) else {
            return base;
        };
        match struct_decl.kind {
            StructKind::List => format!("List<{base}>"),
            StructKind::Flat => "List<String>".to_owned(),
            StructKind::Raw if struct_decl.split_doc.is_some() => format!("List<{base}>"),
            _ => base,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_method(
        &mut self,
        method_name: &str,
        is_private: bool,
        is_raw: bool,
        pipeline: &Pipeline,
        ret_type: &str,
        context: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let arg_ty = if is_raw {
            "String".to_owned()
        } else {
            Self::resolve_type(&pipeline.input, module)
        };
        let vis = if is_private { "private " } else { "" };
        writer.reset_vars();
        let _ = writer.next_var(); // reserve v0 so steps start at v1
        if is_pre_validate {
            wln!(writer, "{vis}fun {method_name}(v: {arg_ty}) {{");
        } else {
            wln!(writer, "{vis}fun {method_name}(v: {arg_ty}): {ret_type} {{");
        }
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline,
                "v",
                context,
                is_pre_validate,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("}");
        writer.blankline();
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_body(
        &mut self,
        pipeline: &Pipeline,
        initial_var: &str,
        context: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref fb_val) = pipeline.fallback {
            let fallback_lit = kotlin_literal(fb_val);
            if is_pre_validate {
                writer.writeln("Rt.fallback({");
                writer.indented(|writer| {
                    let _ = self.emit_pipeline_steps(
                        &pipeline.steps,
                        initial_var,
                        context,
                        is_pre_validate,
                        module,
                        builder,
                        writer,
                    );
                });
                writer.writeln("}, Unit)");
            } else {
                writer.writeln("return Rt.fallback({");
                writer.indented(|writer| {
                    let last = self.emit_pipeline_steps(
                        &pipeline.steps,
                        initial_var,
                        context,
                        is_pre_validate,
                        module,
                        builder,
                        writer,
                    );
                    writer.writeln(last);
                });
                wln!(writer, "}}, {fallback_lit})");
            }
        } else {
            let last = self.emit_pipeline_steps(
                &pipeline.steps,
                initial_var,
                context,
                is_pre_validate,
                module,
                builder,
                writer,
            );
            if !is_pre_validate {
                wln!(writer, "return {last}");
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_steps(
        &mut self,
        steps: &[TypedStep],
        initial_var: &str,
        context: &str,
        is_pre_validate: bool,
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
            let is_last_step = idx + 1 == steps.len();
            self.emit_step(
                step,
                &cur_var,
                &nxt_var,
                next_is_to_bool,
                is_last_step,
                context,
                is_pre_validate,
                module,
                builder,
                writer,
            );
            cur_var = nxt_var;
        }
        cur_var
    }

    fn emit_string_call(
        step: &TypedStep,
        prv: &str,
        nxt: &str,
        func: &str,
        args: &[String],
        writer: &mut CodeWriter,
    ) {
        let suffix = if args.is_empty() {
            String::new()
        } else {
            format!(", {}", args.join(", "))
        };
        if step.output.is_array {
            wln!(
                writer,
                "val {nxt} = {prv}.map {{ value -> Rt.{func}(value{suffix}) }}"
            );
        } else {
            wln!(writer, "val {nxt} = Rt.{func}({prv}{suffix})");
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_step(
        &mut self,
        step: &TypedStep,
        prv: &str,
        nxt: &str,
        next_is_to_bool: bool,
        is_last_step: bool,
        context: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let ctx_lit = kotlin_str(context);

        match &step.op {
            StepOp::LoadInit { name } => {
                let ident = to_camel_case(&to_snake_case(name));
                wln!(writer, "val {nxt} = this._{ident}");
            }
            StepOp::Css { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| kotlin_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                if next_is_to_bool {
                    wln!(writer, "val {nxt} = Rt.selectOpt({prv}, listOf({q_list}))");
                } else {
                    wln!(
                        writer,
                        "val {nxt} = Rt.select({prv}, listOf({q_list}), {ctx_lit})"
                    );
                }
            }
            StepOp::CssAll { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| kotlin_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(writer, "val {nxt} = Rt.selectAll({prv}, listOf({q_list}))");
            }
            StepOp::CssRemove { query } => {
                let q_lit = kotlin_str(query);
                if step.input.is_array {
                    wln!(writer, "val {nxt} = Rt.cssRemoveAll({prv}, {q_lit})");
                } else {
                    wln!(writer, "val {nxt} = Rt.cssRemove({prv}, {q_lit})");
                }
            }
            StepOp::Xpath { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| kotlin_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                if next_is_to_bool {
                    wln!(
                        writer,
                        "val {nxt} = Rt.xpathSelectOpt({prv}, listOf({q_list}))"
                    );
                } else {
                    wln!(
                        writer,
                        "val {nxt} = Rt.xpathSelect({prv}, listOf({q_list}), {ctx_lit})"
                    );
                }
            }
            StepOp::XpathAll { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| kotlin_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(
                    writer,
                    "val {nxt} = Rt.xpathSelectAll({prv}, listOf({q_list}))"
                );
            }
            StepOp::XpathRemove { query } => {
                let q_lit = kotlin_str(query);
                if step.input.is_array {
                    wln!(writer, "val {nxt} = Rt.xpathRemoveAll({prv}, {q_lit})");
                } else {
                    wln!(writer, "val {nxt} = Rt.xpathRemove({prv}, {q_lit})");
                }
            }
            StepOp::Text => {
                if step.output.is_array || step.input.is_array {
                    wln!(writer, "val {nxt} = Rt.textAll({prv})");
                } else {
                    wln!(writer, "val {nxt} = Rt.text({prv})");
                }
            }
            StepOp::Raw { mode } => {
                let inner = if *mode == RawMode::Inner {
                    "true"
                } else {
                    "false"
                };
                if step.output.is_array || step.input.is_array {
                    wln!(writer, "val {nxt} = Rt.rawAll({prv}, {inner})");
                } else {
                    wln!(writer, "val {nxt} = Rt.raw({prv}, {inner})");
                }
            }
            StepOp::Attr { keys } => {
                let names = keys
                    .iter()
                    .map(|k| kotlin_str(k))
                    .collect::<Vec<_>>()
                    .join(", ");
                if keys.len() > 1 {
                    wln!(writer, "val {nxt} = Rt.attrMulti({prv}, listOf({names}))");
                } else if step.input.is_array {
                    wln!(
                        writer,
                        "val {nxt} = Rt.attrList({prv}, listOf({names}), {ctx_lit})"
                    );
                } else {
                    wln!(
                        writer,
                        "val {nxt} = Rt.attr({prv}, listOf({names}), {ctx_lit})"
                    );
                }
            }
            StepOp::Trim { chars } => {
                let arg = if chars.is_empty() {
                    "null".to_owned()
                } else {
                    kotlin_str(chars)
                };
                Self::emit_string_call(step, prv, nxt, "trim", &[arg], writer);
            }
            StepOp::Ltrim { chars } => {
                let arg = if chars.is_empty() {
                    "null".to_owned()
                } else {
                    kotlin_str(chars)
                };
                Self::emit_string_call(step, prv, nxt, "ltrim", &[arg], writer);
            }
            StepOp::Rtrim { chars } => {
                let arg = if chars.is_empty() {
                    "null".to_owned()
                } else {
                    kotlin_str(chars)
                };
                Self::emit_string_call(step, prv, nxt, "rtrim", &[arg], writer);
            }
            StepOp::NormalizeSpace => {
                Self::emit_string_call(step, prv, nxt, "normalizeSpace", &[], writer);
            }
            StepOp::RmPrefix { substr } => {
                Self::emit_string_call(step, prv, nxt, "rmPrefix", &[kotlin_str(substr)], writer);
            }
            StepOp::RmSuffix { substr } => {
                Self::emit_string_call(step, prv, nxt, "rmSuffix", &[kotlin_str(substr)], writer);
            }
            StepOp::RmPrefixSuffix { substr } => {
                Self::emit_string_call(
                    step,
                    prv,
                    nxt,
                    "rmPrefixSuffix",
                    &[kotlin_str(substr)],
                    writer,
                );
            }
            StepOp::Fmt { template } => {
                let tmpl_lit = kotlin_str(template);
                if step.output.is_array {
                    wln!(
                        writer,
                        "val {nxt} = {prv}.map {{ value -> Rt.fmtValue({tmpl_lit}, value) }}"
                    );
                } else {
                    wln!(writer, "val {nxt} = Rt.fmtValue({tmpl_lit}, {prv})");
                }
            }
            StepOp::Repl { old, new } => {
                Self::emit_string_call(
                    step,
                    prv,
                    nxt,
                    "repl",
                    &[kotlin_str(old), kotlin_str(new)],
                    writer,
                );
            }
            StepOp::ReplMap { replacements } => {
                let pairs = replacements
                    .iter()
                    .map(|(k, v)| format!("{} to {}", kotlin_str(k), kotlin_str(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let list_arg = format!("listOf({pairs})");
                Self::emit_string_call(step, prv, nxt, "replMap", &[list_arg], writer);
            }
            StepOp::Lower => {
                Self::emit_string_call(step, prv, nxt, "lower", &[], writer);
            }
            StepOp::Upper => {
                Self::emit_string_call(step, prv, nxt, "upper", &[], writer);
            }
            StepOp::Split { sep } => {
                wln!(writer, "val {nxt} = {prv}.split({})", kotlin_str(sep));
            }
            StepOp::Join { sep } => {
                wln!(
                    writer,
                    "val {nxt} = {prv}.joinToString({})",
                    kotlin_str(sep)
                );
            }
            StepOp::Unescape => {
                Self::emit_string_call(step, prv, nxt, "unescape", &[], writer);
            }
            StepOp::Re { pattern } => {
                let pat_lit = kotlin_regex_str(pattern);
                if step.output.is_array {
                    wln!(
                        writer,
                        "val {nxt} = {prv}.map {{ value -> Rt.reSearch(value, {pat_lit}, {ctx_lit}) }}"
                    );
                } else {
                    wln!(
                        writer,
                        "val {nxt} = Rt.reSearch({prv}, {pat_lit}, {ctx_lit})"
                    );
                }
            }
            StepOp::ReAll { pattern } => {
                let pat_lit = kotlin_regex_str(pattern);
                wln!(writer, "val {nxt} = Rt.reAll({prv}, {pat_lit}, {ctx_lit})");
            }
            StepOp::ReSub { pattern, repl } => {
                let pat_lit = kotlin_regex_str(pattern);
                let repl_lit = kotlin_str(&kotlin_replacement(repl));
                if step.output.is_array {
                    wln!(
                        writer,
                        "val {nxt} = {prv}.map {{ value -> Rt.reSub(value, {pat_lit}, {repl_lit}, {ctx_lit}) }}"
                    );
                } else {
                    wln!(
                        writer,
                        "val {nxt} = Rt.reSub({prv}, {pat_lit}, {repl_lit}, {ctx_lit})"
                    );
                }
            }
            StepOp::ReSplit { pattern } => {
                let pat_lit = kotlin_regex_str(pattern);
                wln!(
                    writer,
                    "val {nxt} = Rt.reSplit({prv}, {pat_lit}, {ctx_lit})"
                );
            }
            StepOp::Index { index } => {
                if step.input.base == BaseType::Document {
                    wln!(writer, "val {nxt} = Rt.nodeAt({prv}, {index}L, {ctx_lit})");
                } else if step.input.is_array {
                    wln!(writer, "val {nxt} = Rt.itemAt({prv}, {index}L, {ctx_lit})");
                } else {
                    wln!(
                        writer,
                        "val {nxt} = Rt.strIndex({prv}, {index}L, {ctx_lit})"
                    );
                }
            }
            StepOp::Slice { start, end } => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "val {nxt} = Rt.strSlice({prv}, {start}L, {end}L)");
                } else {
                    wln!(writer, "val {nxt} = Rt.sliceList({prv}, {start}L, {end}L)");
                }
            }
            StepOp::Len => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "val {nxt} = Rt.strLen({prv})");
                } else {
                    wln!(writer, "val {nxt} = {prv}.size.toLong()");
                }
            }
            StepOp::Unique { .. } => {
                wln!(writer, "val {nxt} = Rt.unique({prv})");
            }
            StepOp::ToInt => {
                if step.output.is_array {
                    wln!(
                        writer,
                        "val {nxt} = {prv}.map {{ value -> Rt.toInt(value, {ctx_lit}) }}"
                    );
                } else {
                    wln!(writer, "val {nxt} = Rt.toInt({prv}, {ctx_lit})");
                }
            }
            StepOp::ToFloat => {
                if step.output.is_array {
                    wln!(
                        writer,
                        "val {nxt} = {prv}.map {{ value -> Rt.toFloat(value, {ctx_lit}) }}"
                    );
                } else {
                    wln!(writer, "val {nxt} = Rt.toFloat({prv}, {ctx_lit})");
                }
            }
            StepOp::ToBool => {
                let expr = if step.input.base == BaseType::Document {
                    format!("Rt.nodeBool({prv})")
                } else if step.input.is_array {
                    format!("{prv}.isNotEmpty()")
                } else if step.input.base == BaseType::Int {
                    format!("Rt.numBool({prv})")
                } else if step.input.base == BaseType::Float {
                    format!("Rt.floatBool({prv})")
                } else if step.input.base == BaseType::Bool {
                    prv.to_owned()
                } else {
                    format!("Rt.stringBool({prv})")
                };
                wln!(writer, "val {nxt} = {expr}");
            }
            StepOp::Jsonify {
                schema_name, path, ..
            } => {
                let pascal = kotlin_pascal(schema_name);
                let effective_path = path
                    .as_deref()
                    .or_else(|| module.json_def(schema_name).and_then(|j| j.path.as_deref()))
                    .unwrap_or("");
                let parsed = format!(
                    "Rt.parseJson({prv}, {}, {ctx_lit})",
                    kotlin_str(effective_path)
                );
                let target_def = module.json_def(schema_name);
                let def_is_arr = target_def.is_some_and(|d| d.is_array);
                if step.output.is_array && !def_is_arr {
                    wln!(
                        writer,
                        "val {nxt} = Rt.expectArray({parsed}, {ctx_lit}).map {{ Rt.project{pascal}(it) }}"
                    );
                } else {
                    wln!(writer, "val {nxt} = Rt.project{pascal}({parsed})");
                }
            }
            StepOp::Nested { struct_name } => {
                let cls = kotlin_pascal(struct_name);
                wln!(writer, "val {nxt} = {cls}({prv}).parse()");
            }
            StepOp::Filter { cond } => {
                let elem_type = TypeInfo {
                    is_array: false,
                    ..step.input.clone()
                };
                let cond_str = Self::render_cond(cond, "i", &elem_type);
                wln!(writer, "val {nxt} = {prv}.filter {{ i -> {cond_str} }}");
            }
            StepOp::Assert { message, cond } => {
                let cond_str = Self::render_cond(cond, prv, &step.input);
                let msg = message.as_deref().unwrap_or("assertion failed");
                wln!(
                    writer,
                    "Rt.assertTrue({cond_str}, {ctx_lit}, {})",
                    kotlin_str(msg)
                );
                if !(is_pre_validate && is_last_step) {
                    wln!(writer, "val {nxt} = {prv}");
                }
            }
            StepOp::Match { .. } => {}
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
                    &["kt"],
                    "'kt'",
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

    fn render_cond(cond: &Cond, target: &str, input_type: &TypeInfo) -> String {
        match cond {
            Cond::Atom(op) => Self::render_cond_op(op, target, input_type),
            Cond::Not(inner) => {
                let inner_s = Self::render_cond(inner, target, input_type);
                format!("!({inner_s})")
            }
            Cond::And(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|c| Self::render_cond(c, target, input_type))
                    .collect();
                join_and(&parts)
            }
            Cond::Or(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|c| Self::render_cond(c, target, input_type))
                    .collect();
                join_or(&parts)
            }
        }
    }

    fn render_cond_op(op: &CondOp, target: &str, input_type: &TypeInfo) -> String {
        let text_val = if input_type.base == BaseType::Document {
            format!("Rt.predText({target})")
        } else {
            target.to_owned()
        };
        let count_expr = if input_type.base == BaseType::String && !input_type.is_array {
            format!("Rt.strLen({target})")
        } else {
            format!("{target}.size.toLong()")
        };

        match op {
            CondOp::Css { query } => {
                format!("Rt.predCss({target}, {})", kotlin_str(query))
            }
            CondOp::Xpath { query } => {
                format!("Rt.predXpath({target}, {})", kotlin_str(query))
            }
            CondOp::HasAttr { attrs } => {
                let checks: Vec<String> = attrs
                    .iter()
                    .map(|name| format!("Rt.predHasAttr({target}, {})", kotlin_str(name)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                let attr = format!("Rt.predAttr({target}, {})", kotlin_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} == {}", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrNe { name, values } => {
                let attr = format!("Rt.predAttr({target}, {})", kotlin_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} != {}", kotlin_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::AttrStarts { name, values } => {
                let attr = format!("Rt.predAttr({target}, {})", kotlin_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.starts({attr}, {})", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEnds { name, values } => {
                let attr = format!("Rt.predAttr({target}, {})", kotlin_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.ends({attr}, {})", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrContains { name, values } => {
                let attr = format!("Rt.predAttr({target}, {})", kotlin_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.contains({attr}, {})", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrRe { name, pattern } => {
                let attr = format!("Rt.predAttr({target}, {})", kotlin_str(name));
                format!("Rt.reMatch({attr}, {})", kotlin_regex_str(pattern))
            }
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} == {}", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} != {}", kotlin_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::TextStarts { values } | CondOp::Starts { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.starts({text_val}, {})", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextEnds { values } | CondOp::Ends { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.ends({text_val}, {})", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextContains { values } | CondOp::Contains { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.contains({text_val}, {})", kotlin_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextRe { pattern } | CondOp::Re { pattern } => {
                format!("Rt.reMatch({text_val}, {})", kotlin_regex_str(pattern))
            }
            CondOp::ReAny { pattern } => {
                format!("Rt.reAny({target}, {})", kotlin_regex_str(pattern))
            }
            CondOp::ReAll { pattern } => {
                format!("Rt.reAllMatch({target}, {})", kotlin_regex_str(pattern))
            }
            CondOp::Eq { values } | CondOp::In { values } => {
                let is_len_cmp = values
                    .first()
                    .is_some_and(|v| matches!(v, CondValue::Int(_)))
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array);
                if is_len_cmp {
                    let checks: Vec<String> = values
                        .iter()
                        .map(|v| format!("{count_expr} == {}L", v.as_str_cow()))
                        .collect();
                    join_or(&checks)
                } else {
                    let checks: Vec<String> = values
                        .iter()
                        .map(|v| format!("{target} == {}", Self::format_cond_val(v, input_type)))
                        .collect();
                    join_or(&checks)
                }
            }
            CondOp::Ne { values } => {
                let is_len_cmp = values
                    .first()
                    .is_some_and(|v| matches!(v, CondValue::Int(_)))
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array);
                if is_len_cmp {
                    let checks: Vec<String> = values
                        .iter()
                        .map(|v| format!("{count_expr} != {}L", v.as_str_cow()))
                        .collect();
                    join_and(&checks)
                } else {
                    let checks: Vec<String> = values
                        .iter()
                        .map(|v| format!("{target} != {}", Self::format_cond_val(v, input_type)))
                        .collect();
                    join_and(&checks)
                }
            }
            CondOp::Gt { value } => {
                Self::format_num_or_len_cmp(target, &count_expr, ">", *value, input_type)
            }
            CondOp::Lt { value } => {
                Self::format_num_or_len_cmp(target, &count_expr, "<", *value, input_type)
            }
            CondOp::Ge { value } => {
                Self::format_num_or_len_cmp(target, &count_expr, ">=", *value, input_type)
            }
            CondOp::Le { value } => {
                Self::format_num_or_len_cmp(target, &count_expr, "<=", *value, input_type)
            }
            CondOp::LenEq { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{count_expr} == {v}L"))
                    .collect();
                join_or(&checks)
            }
            CondOp::LenNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{count_expr} != {v}L"))
                    .collect();
                join_and(&checks)
            }
            CondOp::LenGt { value } => format!("{count_expr} > {value}L"),
            CondOp::LenLt { value } => format!("{count_expr} < {value}L"),
            CondOp::LenGe { value } => format!("{count_expr} >= {value}L"),
            CondOp::LenLe { value } => format!("{count_expr} <= {value}L"),
            CondOp::LenRange { start, end } => {
                format!("{start}L < {count_expr} && {count_expr} < {end}L")
            }
        }
    }

    fn format_cond_val(val: &CondValue, input_type: &TypeInfo) -> String {
        match val {
            CondValue::Int(n) => {
                if input_type.base == BaseType::Float && !input_type.is_array {
                    format!("{n}.toDouble()")
                } else {
                    format!("{n}L")
                }
            }
            CondValue::Float(f) => format!("{f:?}"),
            CondValue::Bool(b) => b.to_string(),
            CondValue::String(s) => kotlin_str(s),
        }
    }

    fn format_num_or_len_cmp(
        target: &str,
        count_expr: &str,
        op: &str,
        value: i64,
        input_type: &TypeInfo,
    ) -> String {
        if input_type.base == BaseType::Int && !input_type.is_array {
            format!("{target} {op} {value}L")
        } else if input_type.base == BaseType::Float && !input_type.is_array {
            format!("{target} {op} {value}.toDouble()")
        } else {
            format!("{count_expr} {op} {value}L")
        }
    }

    fn emit_table_field(
        &mut self,
        struct_decl: &StructDecl,
        field: &FieldDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let method_name = format!("_parse{}", to_pascal_case(&field.name));
        let context = format!("{}.{}", struct_decl.name, field.name);

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        let _ = writer.next_var(); // v1 (used for _parseValue(v))

        wln!(writer, "private fun {method_name}(v: Element): Any? {{");
        writer.indented(|writer| {
            let steps = &field.pipeline.steps;
            let (match_cond, remaining_steps) = match steps.first().map(|s| &s.op) {
                Some(StepOp::Match { cond }) => (Some(cond), &steps[1..]),
                _ => (None, &steps[..]),
            };

            if let Some(cond) = match_cond {
                let key_type = TypeInfo::string();
                let cond_str = Self::render_cond(cond, "_key", &key_type);
                writer.writeln("val _key = this._tableMatchKey(v)");
                wln!(
                    writer,
                    "if (!({cond_str})) return Rt.RT_UNMATCHED_TABLE_ROW"
                );
            }

            writer.writeln("val v1 = this._parseValue(v)");

            if let Some(ref fb_val) = field.pipeline.fallback {
                let fallback_lit = kotlin_literal(fb_val);
                writer.writeln("return Rt.fallback({");
                writer.indented(|writer| {
                    let last = if remaining_steps.is_empty() {
                        "v1".to_owned()
                    } else {
                        self.emit_pipeline_steps(
                            remaining_steps,
                            "v1",
                            &context,
                            false,
                            module,
                            builder,
                            writer,
                        )
                    };
                    writer.writeln(last);
                });
                wln!(writer, "}}, {fallback_lit})");
            } else {
                let last = if remaining_steps.is_empty() {
                    "v1".to_owned()
                } else {
                    self.emit_pipeline_steps(
                        remaining_steps,
                        "v1",
                        &context,
                        false,
                        module,
                        builder,
                        writer,
                    )
                };
                wln!(writer, "return {last}");
            }
        });
        writer.writeln("}");
        writer.blankline();
    }
}

impl LanguageEmitter for KotlinEmitter {
    fn indent_unit(&self) -> &'static str {
        "    "
    }

    fn on_module_start(
        &mut self,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.emitted_aliases.clear();
        writer.writeln("// Auto-generated by parsmith. DO NOT EDIT.");
        writer.writeln("@file:Suppress(\"unused\", \"RedundantVisibilityModifier\", \"MemberVisibilityCanBePrivate\", \"FunctionName\", \"PropertyName\", \"ClassName\", \"LocalVariableName\", \"CanBeParameter\")");
        writer.blankline();

        if let Some(pkg_line) = render_kotlin_package(&self.options.package) {
            writer.writeln(pkg_line);
            writer.blankline();
        }

        if let Some(ref doc) = module.doc {
            emit_kotlin_doc(doc, "//", writer);
            writer.blankline();
        }
    }

    fn emit_imports(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let mut imports = collect_kotlin_runtime_imports(module, builder);

        for imp in builder.imports() {
            let trimmed = imp.trim();
            if trimmed.starts_with("import ") {
                imports.insert(trimmed.trim_end_matches(';').to_owned());
            } else if !trimmed.is_empty() {
                imports.insert(format!("import {}", trimmed.trim_end_matches(';')));
            }
        }

        let pkg_prefix = if self.options.package.is_empty() || self.options.package == "main" {
            String::new()
        } else {
            format!("{}.", self.options.package)
        };
        for rt_name in builder.runtime_names() {
            imports.insert(format!("import {pkg_prefix}Rt.{rt_name}"));
        }

        for imp in &imports {
            writer.write_block(imp);
        }
        writer.blankline();
    }

    fn emit_utilities(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if !self.options.separate_runtime {
            writer.writeln("object Rt {");
            writer.indented(|writer| {
                write_kotlin_runtime_body(module, builder, writer);
            });
            writer.writeln("}");
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
        let name = format!("{}Type", kotlin_pascal(&typedef.name));
        match typedef.struct_kind {
            StructKind::Rest => {}
            StructKind::Flat => {
                wln!(writer, "typealias {name} = List<String>");
                writer.blankline();
            }
            StructKind::Dict => {
                let val_type = typedef
                    .fields
                    .iter()
                    .find(|f| f.name == "value")
                    .map(|f| Self::resolve_type(&f.type_info, module))
                    .unwrap_or_else(|| "String".to_owned());
                wln!(writer, "typealias {name} = Map<String, {val_type}>");
                writer.blankline();
            }
            StructKind::Table => {
                wln!(writer, "typealias {name} = Map<String, JsonElement>");
                writer.blankline();
            }
            StructKind::Item | StructKind::List | StructKind::Raw => {
                if typedef.fields.is_empty() {
                    wln!(writer, "class {name} : Rt.JsonEncodable {{");
                    writer.indented(|writer| {
                        writer.writeln(
                            "override fun toJsonElement(): JsonElement = JsonObject(emptyMap())",
                        );
                        wln!(
                            writer,
                            "override fun equals(other: Any?): Boolean = other is {name}"
                        );
                        writer.writeln("override fun hashCode(): Int = 0");
                        wln!(writer, "override fun toString(): String = \"{}()\"", name);
                    });
                    writer.writeln("}");
                    writer.blankline();
                    return;
                }

                wln!(writer, "data class {name}(");
                writer.indented(|writer| {
                    for field in &typedef.fields {
                        let typ = Self::resolve_type(&field.type_info, module);
                        let f_ident = kotlin_ident(&field.name);
                        if field.type_info.is_optional || field.type_info.omitempty {
                            wln!(writer, "val {f_ident}: {typ} = null,");
                        } else {
                            wln!(writer, "val {f_ident}: {typ},");
                        }
                    }
                });
                writer.writeln(") : Rt.JsonEncodable {");
                writer.indented(|writer| {
                    writer.writeln("override fun toJsonElement(): JsonElement {");
                    writer.indented(|writer| {
                        writer.writeln("val map = LinkedHashMap<String, JsonElement>()");
                        for field in &typedef.fields {
                            let f_ident = kotlin_ident(&field.name);
                            let snake_key = kotlin_str(&to_snake_case(&field.name));
                            if field.type_info.omitempty {
                                wln!(writer, "if (this.{f_ident} != null) {{");
                                writer.indented(|writer| {
                                    wln!(
                                        writer,
                                        "map[{snake_key}] = Rt.toJsonElement(this.{f_ident})"
                                    );
                                });
                                writer.writeln("}");
                            } else {
                                wln!(
                                    writer,
                                    "map[{snake_key}] = Rt.toJsonElement(this.{f_ident})"
                                );
                            }
                        }
                        writer.writeln("return JsonObject(map)");
                    });
                    writer.writeln("}");
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
        let name = kotlin_pascal(&struct_decl.name);
        if let Some(ref doc) = struct_decl.doc {
            emit_kotlin_doc(doc, "//", writer);
        }

        if struct_decl.kind == StructKind::Rest {
            wln!(writer, "class {name} {{");
            writer.indent();
            writer.writeln("companion object {");
            writer.indent();
            wln!(writer, "fun new(): {name} = {name}()");
            writer.blankline();
            return;
        }

        let doc_ty = if struct_decl.kind == StructKind::Raw {
            "String"
        } else {
            "Element"
        };
        wln!(writer, "class {name}(private val _doc: {doc_ty}) {{");
        writer.indent();

        for init_f in &struct_decl.init_fields {
            let ident = to_camel_case(&to_snake_case(&init_f.name));
            let typ = Self::resolve_type(&init_f.type_info, module);
            wln!(writer, "private val _{ident}: {typ}");
        }
        if !struct_decl.init_fields.is_empty() {
            writer.blankline();
        }
    }

    fn emit_init(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        if !is_raw {
            writer.writeln("constructor(input: String) : this(Rt.parseHtml(input))");
            writer.blankline();
        }

        if !struct_decl.init_fields.is_empty() {
            writer.writeln("init {");
            writer.indented(|writer| {
                for init_f in &struct_decl.init_fields {
                    let ident = to_camel_case(&to_snake_case(&init_f.name));
                    let pascal = to_pascal_case(&init_f.name);
                    wln!(writer, "this._{ident} = this._init{pascal}(this._doc)");
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
        let is_raw = struct_decl.kind == StructKind::Raw;
        let pascal = to_pascal_case(&init_field.name);
        let method_name = format!("_init{pascal}");
        let ret_type = Self::resolve_type(&init_field.type_info, module);
        let context = format!("{}.{}", struct_decl.name, init_field.name);
        self.emit_pipeline_method(
            &method_name,
            true,
            is_raw,
            &init_field.pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_pre_validate(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "_preValidate",
            true,
            is_raw,
            pipeline,
            "Unit",
            &context,
            true,
            module,
            builder,
            writer,
        );
    }

    fn emit_check(
        &mut self,
        struct_decl: &StructDecl,
        check: &CheckDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let method_ident = kotlin_ident(&check.name);
        let snake_name = to_snake_case(&check.name);
        let context = format!("{}.{}", struct_decl.name, check.name);

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        wln!(writer, "fun {method_ident}(): Boolean {{");
        writer.indented(|writer| {
            writer.writeln("val v = this._doc");
            self.emit_pipeline_body(
                &check.pipeline,
                "v",
                &context,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("}");
        writer.blankline();

        let snake_ident = if KOTLIN_RESERVED.contains(&snake_name.as_str()) {
            format!("`{snake_name}`")
        } else {
            snake_name
        };
        if snake_ident != method_ident {
            wln!(writer, "fun {snake_ident}(): Boolean = {method_ident}()");
            writer.blankline();
        }
    }

    fn emit_split_doc(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = if is_raw {
            "List<String>".to_owned()
        } else {
            Self::resolve_type(&pipeline.output, module)
        };
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "_splitDoc",
            true,
            is_raw,
            pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_key_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = Self::resolve_type(&pipeline.output, module);
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "_parseKey",
            true,
            is_raw,
            pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_value_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = Self::resolve_type(&pipeline.output, module);
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "_parseValue",
            true,
            is_raw,
            pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_table_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = Self::resolve_type(&pipeline.output, module);
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "_tableConfig",
            true,
            is_raw,
            pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_rows_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = Self::resolve_type(&pipeline.output, module);
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "_tableRows",
            true,
            is_raw,
            pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_match_pipeline(
        &mut self,
        struct_decl: &StructDecl,
        pipeline: &Pipeline,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = Self::resolve_type(&pipeline.output, module);
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "_tableMatchKey",
            true,
            is_raw,
            pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_field(
        &mut self,
        struct_decl: &StructDecl,
        field: &FieldDecl,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if struct_decl.kind == StructKind::Table {
            self.emit_table_field(struct_decl, field, module, builder, writer);
            return;
        }
        let is_raw = struct_decl.kind == StructKind::Raw;
        let method_name = format!("_parse{}", to_pascal_case(&field.name));
        let ret_type = Self::resolve_type(&field.type_info, module);
        let context = format!("{}.{}", struct_decl.name, field.name);
        self.emit_pipeline_method(
            &method_name,
            true,
            is_raw,
            &field.pipeline,
            &ret_type,
            &context,
            false,
            module,
            builder,
            writer,
        );
    }

    fn emit_start_parse(
        &mut self,
        struct_decl: &StructDecl,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = kotlin_pascal(&struct_decl.name);
        let result_type = Self::resolve_struct_result_type(&struct_decl.name, module);
        let is_raw = struct_decl.kind == StructKind::Raw;
        let has_raw_split = is_raw && struct_decl.split_doc.is_some();

        wln!(writer, "fun parse(): {result_type} {{");
        writer.indented(|writer| {
            if struct_decl.pre_validate.is_some() {
                writer.writeln("this._preValidate(this._doc)");
            }

            if struct_decl.kind == StructKind::List || has_raw_split {
                writer.writeln("val rows = this._splitDoc(this._doc)");
                wln!(writer, "val result = ArrayList<{name}Type>(rows.size)");
                writer.writeln("for (row in rows) {");
                writer.indented(|writer| {
                    if struct_decl.fields.is_empty() {
                        wln!(writer, "result.add({name}Type())");
                    } else {
                        wln!(writer, "result.add({name}Type(");
                        writer.indented(|writer| {
                            for field in &struct_decl.fields {
                                let ident = kotlin_ident(&field.name);
                                let pascal = to_pascal_case(&field.name);
                                wln!(writer, "{ident} = this._parse{pascal}(row),");
                            }
                        });
                        writer.writeln("))");
                    }
                });
                writer.writeln("}");
                writer.writeln("return result");
            } else if struct_decl.kind == StructKind::Flat {
                writer.writeln("val result = ArrayList<String>()");
                for field in &struct_decl.fields {
                    let pascal = to_pascal_case(&field.name);
                    let ftype = Self::resolve_type(&field.type_info, module);
                    if field.type_info.is_optional || field.type_info.omitempty {
                        wln!(writer, "val _val{pascal} = this._parse{pascal}(this._doc)");
                        wln!(writer, "if (_val{pascal} != null) {{");
                        writer.indented(|writer| {
                            let inner = ftype.strip_suffix('?').unwrap_or(&ftype);
                            if inner.starts_with("List<") {
                                wln!(writer, "result.addAll(_val{pascal})");
                            } else {
                                wln!(writer, "result.add(_val{pascal})");
                            }
                        });
                        writer.writeln("}");
                    } else if ftype.starts_with("List<") {
                        wln!(writer, "result.addAll(this._parse{pascal}(this._doc))");
                    } else {
                        wln!(writer, "result.add(this._parse{pascal}(this._doc))");
                    }
                }
                writer.writeln("return Rt.unique(result)");
            } else if struct_decl.kind == StructKind::Dict {
                let val_type = struct_decl
                    .value_pipeline
                    .as_ref()
                    .map(|p| Self::resolve_type(&p.output, module))
                    .unwrap_or_else(|| "String".to_owned());
                writer.writeln("val rows = this._splitDoc(this._doc)");
                wln!(writer, "val result = LinkedHashMap<String, {val_type}>()");
                writer.writeln("for (row in rows) {");
                writer.indented(|writer| {
                    writer.writeln("result[this._parseKey(row)] = this._parseValue(row)");
                });
                writer.writeln("}");
                writer.writeln("return result");
            } else if struct_decl.kind == StructKind::Table {
                writer.writeln("val table = this._tableConfig(this._doc)");
                writer.writeln("val rows = this._tableRows(table)");
                writer.writeln("val result = LinkedHashMap<String, JsonElement>()");
                writer.writeln("for (row in rows) {");
                writer.indented(|writer| {
                    for field in &struct_decl.fields {
                        let key_name = to_snake_case(&field.name);
                        let key_lit = kotlin_str(&key_name);
                        let pascal = to_pascal_case(&field.name);
                        wln!(writer, "if (!result.containsKey({key_lit})) {{");
                        writer.indented(|writer| {
                            wln!(writer, "val value = this._parse{pascal}(row)");
                            writer.writeln("if (value !== Rt.RT_UNMATCHED_TABLE_ROW) {");
                            writer.indented(|writer| {
                                wln!(writer, "result[{key_lit}] = Rt.toJsonElement(value)");
                            });
                            writer.writeln("}");
                        });
                        writer.writeln("}");
                    }
                });
                writer.writeln("}");
                writer.writeln("return result");
            } else if struct_decl.fields.is_empty() {
                wln!(writer, "return {name}Type()");
            } else {
                wln!(writer, "return {name}Type(");
                writer.indented(|writer| {
                    for field in &struct_decl.fields {
                        let ident = kotlin_ident(&field.name);
                        let pascal = to_pascal_case(&field.name);
                        wln!(writer, "{ident} = this._parse{pascal}(this._doc),");
                    }
                });
                writer.writeln(")");
            }
        });
        writer.writeln("}");
        writer.blankline();

        writer.writeln("companion object {");
        writer.indent();
        wln!(writer, "fun new(input: String): {name} = {name}(input)");
        writer.blankline();
        if !is_raw {
            wln!(
                writer,
                "fun fromExisting(root: Element): {name} = {name}(root)"
            );
            writer.blankline();
        }
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
        struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = kotlin_pascal(&struct_decl.name);
        writer.dedent();
        writer.writeln("}"); // close companion object
        writer.dedent();
        writer.writeln("}"); // close class
        writer.blankline();
        wln!(writer, "typealias {name}Parser = {name}");
        writer.blankline();
    }

    fn emit_function(
        &mut self,
        func: &FunctionDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let camel_name = kotlin_ident(&func.name);
        let snake_name = to_snake_case(&func.name);
        let ret_type = Self::resolve_type(&func.return_type, module);
        if let Some(ref doc) = func.doc {
            emit_kotlin_doc(doc, "//", writer);
        }

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        wln!(writer, "fun {camel_name}(document: String): {ret_type} {{");
        writer.indented(|writer| {
            if func.is_raw {
                writer.writeln("val v = document");
            } else {
                writer.writeln("val v: Element = Rt.parseHtml(document)");
            }
            self.emit_pipeline_body(
                &func.pipeline,
                "v",
                &func.name,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("}");
        writer.blankline();

        let snake_ident = if KOTLIN_RESERVED.contains(&snake_name.as_str()) {
            format!("`{snake_name}`")
        } else {
            snake_name
        };
        if snake_ident != camel_name {
            wln!(
                writer,
                "fun {snake_ident}(document: String): {ret_type} = {camel_name}(document)"
            );
            writer.blankline();
        }
    }
}

/// Emits Kotlin source files for a compiled [`Module`] and [`CodegenOptions`].
///
/// Returns `(parser_source, optional_runtime_source, builder)` or `Err(diagnostics)`.
pub fn emit_kotlin_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, Option<String>, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = KotlinEmitter::new(options.clone());
    let (parser_code, builder) = drive_module(
        &mut emitter,
        module,
        options.prologue.as_deref(),
        options.epilogue.as_deref(),
    );
    if !emitter.diagnostics.is_empty() {
        return Err(emitter.diagnostics);
    }
    let runtime_code = if options.separate_runtime {
        Some(emit_kotlin_runtime_module(module, &builder, options))
    } else {
        None
    };
    Ok((parser_code, runtime_code, builder))
}

//! Rust (`dom_query` + `serde` + `reqwest`) target backend (`RustEmitter`).
//!
//! Emits pre-formatted 4-space indented Rust 2021/2024 source code directly from [`CodeWriter`],
//! defaulting to a single self-contained `<stem>.rs` file with an inline `pub mod rt { ... }`
//! (omitting `dom_query` when `!module_has_html(module)`), while supporting `-R / --separate-runtime`
//! (`<stem>.rs` + `<runtime_name>.rs`).

pub mod json;
pub mod rest;
pub mod runtime;

use std::fmt::Write as _;

use crate::kdl::Span;
use indexmap::IndexSet;

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, FieldDecl, FunctionDef, InitFieldDecl, JsonDef,
    LiteralValue, Module, Pipeline, RawMode, RequestDecl, RestArtifact, RestEndpointDecl, StepOp,
    StructDecl, StructKind, StructTypeDef, TypeInfo, TypedStep,
};
use crate::codegen::builder::{ModuleBuilder, emit_extension_call};
use crate::codegen::emitter::{LanguageEmitter, drive_module};
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, module_has_html};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::{RUST_CANNOT_BE_RAW, RUST_RESERVED, to_snake_case};
use crate::lowering::clean_doc;
use crate::validators::normalize_regex_pattern;

pub use rest::{render_json_body, render_template, rust_ph_type, rust_variant_name};
pub use runtime::{
    RUST_CORE_RUNTIME, RUST_HTML_RUNTIME, RUST_HTTP_RUNTIME, emit_rust_runtime_module,
    write_rust_runtime_body,
};

fn rust_regex_str(pattern: &str) -> String {
    rust_str(&normalize_regex_pattern(pattern))
}

/// Formats a `&str` as a valid double-quoted Rust string literal (`"..."`).
pub fn rust_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Renders a DSL identifier as a valid `snake_case` Rust identifier, escaping reserved keywords with `r#`.
pub fn rust_ident(value: &str) -> String {
    let name = to_snake_case(value).replace('-', "_");
    let name = if name.is_empty() {
        "value".to_owned()
    } else {
        name
    };
    if RUST_RESERVED.contains(&name.as_str()) && !RUST_CANNOT_BE_RAW.contains(&name.as_str()) {
        format!("r#{name}")
    } else {
        name
    }
}

/// Renders a DSL identifier as a `PascalCase` Rust type name, escaping reserved keywords with `r#`.
pub fn rust_pascal(value: &str) -> String {
    let name = to_pascal_case(value).replace('-', "");
    let name = if name.is_empty() {
        "Value".to_owned()
    } else {
        name
    };
    if RUST_RESERVED.contains(&name.as_str()) && !RUST_CANNOT_BE_RAW.contains(&name.as_str()) {
        format!("r#{name}")
    } else {
        name
    }
}

/// Maps a dictionary key [`TypeInfo`] to its Rust type representation.
pub fn rust_key_type(info: Option<&TypeInfo>) -> &'static str {
    let Some(info) = info else {
        return "String";
    };
    match info.base {
        BaseType::String => "String",
        BaseType::Int => "i64",
        BaseType::Float => "f64",
        BaseType::Bool => "bool",
        _ => "String",
    }
}

/// Formats a [`LiteralValue`] as a typed Rust literal expression for `typ`.
pub fn rust_literal(val: &LiteralValue, typ: &str) -> String {
    if matches!(val, LiteralValue::Null) {
        return if typ.starts_with("Option<") {
            "None".to_owned()
        } else {
            "Default::default()".to_owned()
        };
    }
    if let Some(inner) = typ
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return format!("Some({})", rust_literal(val, inner));
    }
    match val {
        LiteralValue::Null => "Default::default()".to_owned(),
        LiteralValue::Bool(b) => b.to_string(),
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => format!("{}.to_string()", rust_str(s)),
        LiteralValue::EmptyArray => "Vec::new()".to_owned(),
    }
}

/// Translates DSL/Python numeric replacement groups (`\1` or `$1`) into Rust `regex` replacement syntax,
/// while escaping literal dollar signs as `$$`.
pub fn rust_replacement(replacement: &str) -> String {
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
            if end < n && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                let _ = write!(out, "${{{group}}}");
            } else {
                let _ = write!(out, "${group}");
            }
            i = end;
        } else if bytes[i] == b'$' {
            if i + 1 < n && bytes[i + 1] == b'$' {
                out.push_str("$$");
                i += 2;
            } else if i + 1 < n && bytes[i + 1].is_ascii_digit() && bytes[i + 1] != b'0' {
                let start = i + 1;
                let mut end = start + 1;
                while end < n && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let group = &replacement[start..end];
                if end < n && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    let _ = write!(out, "${{{group}}}");
                } else {
                    let _ = write!(out, "${group}");
                }
                i = end;
            } else {
                out.push_str("$$");
                i += 1;
            }
        } else {
            let ch = replacement[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }

    out
}

/// Writes cleaned Rust documentation lines (`/// ...` or `// ...`) to `writer`.
pub fn emit_rust_doc(doc: &str, prefix: &str, writer: &mut CodeWriter) {
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

/// Rust target `LanguageEmitter` implementation.
pub struct RustEmitter {
    /// Code generation options.
    pub options: CodegenOptions,
    /// Diagnostics accumulated during code generation (`E201`, `E203`, `E400`).
    pub diagnostics: Vec<Diagnostic>,
    /// Tracks already-emitted REST `Result` type aliases within the current pass.
    pub(crate) emitted_aliases: IndexSet<String>,
}

impl RustEmitter {
    /// Creates a new `RustEmitter` for `options`.
    pub fn new(options: CodegenOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            emitted_aliases: IndexSet::new(),
        }
    }

    /// Resolves a [`TypeInfo`] into its Rust type annotation.
    pub fn resolve_type(info: &TypeInfo, module: &Module) -> String {
        let optional = info.is_optional || info.omitempty;
        if info.is_dict {
            let key_type = rust_key_type(info.key_type.as_deref());
            let val_type = info
                .value_type
                .as_deref()
                .map(|vt| Self::resolve_type(vt, module))
                .unwrap_or_else(|| "String".to_owned());
            let mut res = format!("std::collections::HashMap<{key_type}, {val_type}>");
            if info.is_array {
                res = format!("Vec<{res}>");
            }
            if optional {
                res = format!("Option<{res}>");
            }
            return res;
        }

        let mut res = match &info.base {
            BaseType::Document => "rt::Nodes".to_owned(),
            BaseType::Nested(ref_name) => Self::resolve_struct_result_type(ref_name, module),
            BaseType::Json(ref_name) => {
                let pascal = rust_pascal(ref_name);
                let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
                if info.is_array || def_is_arr {
                    format!("Vec<{pascal}Json>")
                } else {
                    format!("{pascal}Json")
                }
            }
            BaseType::String | BaseType::Auto | BaseType::DictValue => "String".to_owned(),
            BaseType::Int => "i64".to_owned(),
            BaseType::Float => "f64".to_owned(),
            BaseType::Bool => "bool".to_owned(),
            BaseType::Null => "()".to_owned(),
        };

        if info.is_array
            && !matches!(
                info.base,
                BaseType::Document | BaseType::Nested(_) | BaseType::Json(_)
            )
        {
            res = format!("Vec<{res}>");
        }
        if optional && res != "()" && !res.starts_with("Option<") {
            res = format!("Option<{res}>");
        }
        res
    }

    /// Resolves the owned result type of a struct's `.parse()` method.
    pub fn resolve_struct_result_type(name: &str, module: &Module) -> String {
        let base = format!("{}Type", rust_pascal(name));
        let Some(struct_decl) = module.struct_decl(name) else {
            return base;
        };
        match struct_decl.kind {
            StructKind::List => format!("Vec<{base}>"),
            StructKind::Flat => "Vec<String>".to_owned(),
            StructKind::Raw if struct_decl.split_doc.is_some() => format!("Vec<{base}>"),
            _ => base,
        }
    }

    fn record_unsupported_xpath(&mut self, span: Span, source_file: &str) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E203,
                "XPath operations ('xpath', 'xpath-all', 'xpath-remove') are not supported by the 'rust' backend",
                span,
            )
            .with_path(source_file.to_owned()),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_method(
        &mut self,
        method_name: &str,
        is_pub: bool,
        is_raw: bool,
        pipeline: &Pipeline,
        ret_type: &str,
        context: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let arg_ty = if is_raw { "String" } else { "rt::Nodes" };
        let vis = if is_pub { "pub " } else { "" };
        writer.reset_vars();
        let _ = writer.next_var(); // reserve v0 so steps start at v1
        wln!(
            writer,
            "{vis}fn {method_name}(&mut self, v: {arg_ty}) -> Result<{ret_type}, rt::RtError> {{"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline,
                "v",
                ret_type,
                context,
                is_raw,
                false,
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
        ret_type: &str,
        context: &str,
        is_raw: bool,
        in_function: bool,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref fb_val) = pipeline.fallback {
            let fallback_lit = rust_literal(fb_val, ret_type);
            wln!(
                writer,
                "return Ok(match (|| -> Result<{ret_type}, rt::RtError> {{"
            );
            writer.indented(|writer| {
                let last = self.emit_pipeline_steps(
                    &pipeline.steps,
                    initial_var,
                    context,
                    is_raw,
                    in_function,
                    is_pre_validate,
                    module,
                    builder,
                    writer,
                );
                if is_pre_validate {
                    writer.writeln("Ok(())");
                } else {
                    let steps_end_opt = pipeline
                        .steps
                        .last()
                        .is_some_and(|s| s.output.is_optional || s.output.omitempty);
                    if ret_type.starts_with("Option<") && !steps_end_opt {
                        wln!(writer, "Ok(Some({last}))");
                    } else {
                        wln!(writer, "Ok({last})");
                    }
                }
            });
            wln!(
                writer,
                "}})() {{ Ok(value) => value, Err(_) => {fallback_lit} }});"
            );
        } else {
            let last = self.emit_pipeline_steps(
                &pipeline.steps,
                initial_var,
                context,
                is_raw,
                in_function,
                is_pre_validate,
                module,
                builder,
                writer,
            );
            if is_pre_validate {
                writer.writeln("Ok(())");
            } else {
                wln!(writer, "Ok({last})");
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_steps(
        &mut self,
        steps: &[TypedStep],
        initial_var: &str,
        context: &str,
        is_raw: bool,
        in_function: bool,
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
                is_raw,
                in_function,
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
                "let {nxt} = {prv}.into_iter().map(|value| rt::{func}(value{suffix})).collect::<Vec<_>>();"
            );
        } else {
            wln!(writer, "let {nxt} = rt::{func}({prv}{suffix});");
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
        is_raw: bool,
        in_function: bool,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let dom_ref = if in_function { "&dom" } else { "&self.dom" };
        let dom_clone = if in_function {
            "dom.clone()"
        } else {
            "self.dom.clone()"
        };
        let ctx_lit = rust_str(context);

        match &step.op {
            StepOp::LoadInit { name } => {
                let ident = rust_ident(name);
                wln!(writer, "let {nxt} = self.{ident}.clone();");
            }
            StepOp::Css { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| rust_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                if next_is_to_bool {
                    wln!(
                        writer,
                        "let {nxt} = rt::select_opt({dom_ref}, &{prv}, &[{q_list}]);"
                    );
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rt::select({dom_ref}, &{prv}, &[{q_list}], {ctx_lit})?;"
                    );
                }
            }
            StepOp::CssAll { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| rust_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(
                    writer,
                    "let {nxt} = rt::select_all({dom_ref}, &{prv}, &[{q_list}], {ctx_lit})?;"
                );
            }
            StepOp::CssRemove { query } => {
                let removed = format!("_{nxt}_removed");
                let q_lit = rust_str(query);
                wln!(
                    writer,
                    "let {removed} = rt::select_all({dom_ref}, &{prv}, &[{q_lit}], {ctx_lit})?;"
                );
                wln!(writer, "rt::remove({dom_ref}, &{removed})?;");
                wln!(writer, "let {nxt} = {prv}.clone();");
            }
            StepOp::Xpath { .. } | StepOp::XpathAll { .. } | StepOp::XpathRemove { .. } => {
                if writer.is_dry_run() {
                    self.record_unsupported_xpath(step.span, &module.source_file);
                }
            }
            StepOp::Text => {
                if step.output.is_array || step.input.is_array {
                    wln!(writer, "let {nxt} = rt::text_all({dom_ref}, &{prv});");
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rt::text({dom_ref}, &{prv}, {ctx_lit})?;"
                    );
                }
            }
            StepOp::Raw { mode } => {
                let inner = if *mode == RawMode::Inner {
                    "true"
                } else {
                    "false"
                };
                if step.output.is_array || step.input.is_array {
                    wln!(
                        writer,
                        "let {nxt} = rt::raw_all({dom_ref}, &{prv}, {inner});"
                    );
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rt::raw({dom_ref}, &{prv}, {inner}, {ctx_lit})?;"
                    );
                }
            }
            StepOp::Attr { keys } => {
                let names = keys
                    .iter()
                    .map(|k| rust_str(k))
                    .collect::<Vec<_>>()
                    .join(", ");
                if keys.len() > 1 {
                    wln!(
                        writer,
                        "let {nxt} = rt::attr_multi({dom_ref}, &{prv}, &[{names}]);"
                    );
                } else if step.input.is_array {
                    wln!(
                        writer,
                        "let {nxt} = rt::attr_list({dom_ref}, &{prv}, &[{names}], {ctx_lit})?;"
                    );
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rt::attr({dom_ref}, &{prv}, &[{names}], {ctx_lit})?;"
                    );
                }
            }
            StepOp::Trim { chars } => {
                let arg = if chars.is_empty() {
                    "None".to_owned()
                } else {
                    format!("Some({})", rust_str(chars))
                };
                Self::emit_string_call(step, prv, nxt, "trim", &[arg], writer);
            }
            StepOp::Ltrim { chars } => {
                let arg = if chars.is_empty() {
                    "None".to_owned()
                } else {
                    format!("Some({})", rust_str(chars))
                };
                Self::emit_string_call(step, prv, nxt, "ltrim", &[arg], writer);
            }
            StepOp::Rtrim { chars } => {
                let arg = if chars.is_empty() {
                    "None".to_owned()
                } else {
                    format!("Some({})", rust_str(chars))
                };
                Self::emit_string_call(step, prv, nxt, "rtrim", &[arg], writer);
            }
            StepOp::NormalizeSpace => {
                Self::emit_string_call(step, prv, nxt, "normalize_space", &[], writer);
            }
            StepOp::RmPrefix { substr } => {
                Self::emit_string_call(step, prv, nxt, "rm_prefix", &[rust_str(substr)], writer);
            }
            StepOp::RmSuffix { substr } => {
                Self::emit_string_call(step, prv, nxt, "rm_suffix", &[rust_str(substr)], writer);
            }
            StepOp::RmPrefixSuffix { substr } => {
                Self::emit_string_call(
                    step,
                    prv,
                    nxt,
                    "rm_prefix_suffix",
                    &[rust_str(substr)],
                    writer,
                );
            }
            StepOp::Fmt { template } => {
                let tmpl_lit = rust_str(template);
                if step.output.is_array {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.into_iter().map(|value| rt::fmt_value({tmpl_lit}, &value)).collect::<Vec<_>>();"
                    );
                } else {
                    wln!(writer, "let {nxt} = rt::fmt_value({tmpl_lit}, &{prv});");
                }
            }
            StepOp::Repl { old, new } => {
                Self::emit_string_call(
                    step,
                    prv,
                    nxt,
                    "repl",
                    &[rust_str(old), rust_str(new)],
                    writer,
                );
            }
            StepOp::ReplMap { replacements } => {
                let pairs = replacements
                    .iter()
                    .map(|(k, v)| format!("({}, {})", rust_str(k), rust_str(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let slice_arg = format!("&[{pairs}]");
                Self::emit_string_call(step, prv, nxt, "repl_map", &[slice_arg], writer);
            }
            StepOp::Lower => {
                Self::emit_string_call(step, prv, nxt, "lower", &[], writer);
            }
            StepOp::Upper => {
                Self::emit_string_call(step, prv, nxt, "upper", &[], writer);
            }
            StepOp::Split { sep } => {
                wln!(
                    writer,
                    "let {nxt} = {prv}.split({}).map(str::to_string).collect::<Vec<_>>();",
                    rust_str(sep)
                );
            }
            StepOp::Join { sep } => {
                wln!(writer, "let {nxt} = {prv}.join({});", rust_str(sep));
            }
            StepOp::Unescape => {
                Self::emit_string_call(step, prv, nxt, "unescape", &[], writer);
            }
            StepOp::Re { pattern } => {
                let pat_lit = rust_regex_str(pattern);
                if step.output.is_array {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.into_iter().map(|value| rt::regex_search(value, {pat_lit}, {ctx_lit})).collect::<Result<Vec<_>, _>>()?;"
                    );
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rt::regex_search({prv}, {pat_lit}, {ctx_lit})?;"
                    );
                }
            }
            StepOp::ReAll { pattern } => {
                let pat_lit = rust_regex_str(pattern);
                wln!(
                    writer,
                    "let {nxt} = rt::regex_all({prv}, {pat_lit}, {ctx_lit})?;"
                );
            }
            StepOp::ReSub { pattern, repl } => {
                let pat_lit = rust_regex_str(pattern);
                let repl_lit = rust_str(&rust_replacement(repl));
                if step.output.is_array {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.into_iter().map(|value| rt::regex_sub(value, {pat_lit}, {repl_lit}, {ctx_lit})).collect::<Result<Vec<_>, _>>()?;"
                    );
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rt::regex_sub({prv}, {pat_lit}, {repl_lit}, {ctx_lit})?;"
                    );
                }
            }
            StepOp::ReSplit { pattern } => {
                let pat_lit = rust_regex_str(pattern);
                wln!(
                    writer,
                    "let {nxt} = rt::regex_split({prv}, {pat_lit}, {ctx_lit})?;"
                );
            }
            StepOp::Index { index } => {
                if step.input.base == BaseType::Document {
                    wln!(
                        writer,
                        "let {nxt} = rt::node_at(&{prv}, {index}, {ctx_lit})?;"
                    );
                } else if step.input.is_array {
                    wln!(
                        writer,
                        "let {nxt} = rt::item_at(&{prv}, {index}, {ctx_lit})?;"
                    );
                } else {
                    wln!(
                        writer,
                        "let {nxt} = rt::index(&{prv}, {index}, {ctx_lit})?;"
                    );
                }
            }
            StepOp::Slice { start, end } => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "let {nxt} = rt::slice(&{prv}, {start}, {end});");
                } else {
                    wln!(writer, "let {nxt} = rt::slice_vec(&{prv}, {start}, {end});");
                }
            }
            StepOp::Len => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "let {nxt} = {prv}.chars().count() as i64;");
                } else {
                    wln!(writer, "let {nxt} = {prv}.len() as i64;");
                }
            }
            StepOp::Unique { .. } => {
                wln!(writer, "let {nxt} = rt::unique({prv});");
            }
            StepOp::ToInt => {
                if step.output.is_array {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.into_iter().map(|value| rt::to_int(value, {ctx_lit})).collect::<Result<Vec<_>, _>>()?;"
                    );
                } else {
                    wln!(writer, "let {nxt} = rt::to_int({prv}, {ctx_lit})?;");
                }
            }
            StepOp::ToFloat => {
                if step.output.is_array {
                    wln!(
                        writer,
                        "let {nxt} = {prv}.into_iter().map(|value| rt::to_float(value, {ctx_lit})).collect::<Result<Vec<_>, _>>()?;"
                    );
                } else {
                    wln!(writer, "let {nxt} = rt::to_float({prv}, {ctx_lit})?;");
                }
            }
            StepOp::ToBool => {
                let expr = if step.input.base == BaseType::Document {
                    format!("rt::node_bool(&{prv})")
                } else if step.input.is_array {
                    format!("!{prv}.is_empty()")
                } else if step.input.base == BaseType::Int {
                    format!("rt::num_bool({prv})")
                } else if step.input.base == BaseType::Float {
                    format!("rt::float_bool({prv})")
                } else if step.input.base == BaseType::Bool {
                    prv.to_owned()
                } else {
                    format!("rt::string_bool(&{prv})")
                };
                wln!(writer, "let {nxt} = {expr};");
            }
            StepOp::Jsonify {
                schema_name, path, ..
            } => {
                let name = format!("{}Json", rust_pascal(schema_name));
                let effective_path = path
                    .as_deref()
                    .or_else(|| module.json_def(schema_name).and_then(|j| j.path.as_deref()))
                    .unwrap_or("");
                let parsed = format!(
                    "rt::parse_json(&{prv}, {}, {ctx_lit})?",
                    rust_str(effective_path)
                );
                let target_def = module.json_def(schema_name);
                if target_def.is_some_and(|d| d.is_dict) {
                    let target_type = if step.output.is_array {
                        format!("Vec<{name}>")
                    } else {
                        name
                    };
                    wln!(
                        writer,
                        "let {nxt} = rt::decode::<{target_type}>({parsed}, {ctx_lit})?;"
                    );
                } else if step.output.is_array || target_def.is_some_and(|d| d.is_array) {
                    wln!(
                        writer,
                        "let {nxt} = {parsed}.as_array().ok_or_else(|| rt::RtError::new({ctx_lit}, \"expected JSON array\"))?.iter().cloned().map({name}::from_value).collect::<Result<Vec<_>, _>>()?;"
                    );
                } else {
                    wln!(writer, "let {nxt} = {name}::from_value({parsed})?;");
                }
            }
            StepOp::Nested { struct_name } => {
                let cls = rust_pascal(struct_name);
                let is_child_raw = module
                    .struct_decl(struct_name)
                    .is_some_and(|s| s.kind == StructKind::Raw);
                let is_input_str = step.input.base == BaseType::String;
                if is_raw || is_child_raw || is_input_str {
                    wln!(writer, "let {nxt} = {cls}Parser::new(&{prv})?.parse()?;");
                } else {
                    wln!(
                        writer,
                        "let {nxt} = {cls}Parser::from_existing({dom_clone}, rt::first_node(&{prv}, {ctx_lit})?)?.parse()?;"
                    );
                }
            }
            StepOp::Filter { cond } => {
                let elem_type = TypeInfo {
                    is_array: false,
                    ..step.input.clone()
                };
                let target = if step.input.base == BaseType::Document {
                    "&i"
                } else {
                    "i"
                };
                let cond_str = self.render_cond(
                    cond,
                    target,
                    &elem_type,
                    in_function,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                wln!(writer, "let {nxt} = {{");
                writer.indented(|writer| {
                    writer.writeln("let mut out = Vec::new();");
                    if step.input.base == BaseType::Document {
                        wln!(writer, "for id in {prv}.iter() {{");
                        writer.indented(|writer| {
                            writer.writeln("let i = vec![*id];");
                            wln!(writer, "if {cond_str} {{ out.push(*id); }}");
                        });
                        writer.writeln("}");
                    } else {
                        wln!(writer, "for i in {prv}.into_iter() {{");
                        writer.indented(|writer| {
                            wln!(writer, "if {cond_str} {{ out.push(i); }}");
                        });
                        writer.writeln("}");
                    }
                    writer.writeln("out");
                });
                writer.writeln("};");
            }
            StepOp::Assert { message, cond } => {
                let target = if step.input.base == BaseType::Document {
                    format!("&{prv}")
                } else {
                    prv.to_owned()
                };
                let cond_str = self.render_cond(
                    cond,
                    &target,
                    &step.input,
                    in_function,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                let msg = message.as_deref().unwrap_or("assertion failed");
                wln!(
                    writer,
                    "rt::assert_true({cond_str}, {ctx_lit}, {})?;",
                    rust_str(msg)
                );
                if !(is_pre_validate && is_last_step) {
                    wln!(writer, "let {nxt} = {prv}.clone();");
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
                    &["rust"],
                    "'rust'",
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

    #[allow(clippy::too_many_arguments)]
    fn render_cond(
        &mut self,
        cond: &Cond,
        target: &str,
        input_type: &TypeInfo,
        in_function: bool,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
    ) -> String {
        match cond {
            Cond::Atom(op) => self.render_cond_op(
                op,
                target,
                input_type,
                in_function,
                span,
                source_file,
                is_dry_run,
            ),
            Cond::Not(inner) => {
                let inner_s = self.render_cond(
                    inner,
                    target,
                    input_type,
                    in_function,
                    span,
                    source_file,
                    is_dry_run,
                );
                format!("!({inner_s})")
            }
            Cond::And(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|c| {
                        self.render_cond(
                            c,
                            target,
                            input_type,
                            in_function,
                            span,
                            source_file,
                            is_dry_run,
                        )
                    })
                    .collect();
                join_and(&parts)
            }
            Cond::Or(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|c| {
                        self.render_cond(
                            c,
                            target,
                            input_type,
                            in_function,
                            span,
                            source_file,
                            is_dry_run,
                        )
                    })
                    .collect();
                join_or(&parts)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_cond_op(
        &mut self,
        op: &CondOp,
        target: &str,
        input_type: &TypeInfo,
        in_function: bool,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
    ) -> String {
        let dom_ref = if in_function { "&dom" } else { "&self.dom" };
        let text_val = if target.starts_with('&') {
            format!("rt::pred_text({dom_ref}, {target})")
        } else {
            target.to_owned()
        };
        let count_expr = if input_type.base == BaseType::String && !input_type.is_array {
            format!("{target}.chars().count()")
        } else {
            format!("{target}.len()")
        };

        match op {
            CondOp::Css { query } => {
                format!("rt::pred_css({dom_ref}, {target}, {})", rust_str(query))
            }
            CondOp::Xpath { .. } => {
                if is_dry_run {
                    self.record_unsupported_xpath(span, source_file);
                }
                "false".to_owned()
            }
            CondOp::HasAttr { attrs } => {
                let checks: Vec<String> = attrs
                    .iter()
                    .map(|name| {
                        format!(
                            "rt::pred_attr({dom_ref}, {target}, {}).is_some()",
                            rust_str(name)
                        )
                    })
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                let attr = format!(
                    "rt::pred_attr({dom_ref}, {target}, {}).unwrap_or_default()",
                    rust_str(name)
                );
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} == {}", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrNe { name, values } => {
                let attr = format!(
                    "rt::pred_attr({dom_ref}, {target}, {}).unwrap_or_default()",
                    rust_str(name)
                );
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} != {}", rust_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::AttrStarts { name, values } => {
                let attr = format!(
                    "rt::pred_attr({dom_ref}, {target}, {}).unwrap_or_default()",
                    rust_str(name)
                );
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("rt::starts(&{attr}, {})", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEnds { name, values } => {
                let attr = format!(
                    "rt::pred_attr({dom_ref}, {target}, {}).unwrap_or_default()",
                    rust_str(name)
                );
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("rt::ends(&{attr}, {})", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrContains { name, values } => {
                let attr = format!(
                    "rt::pred_attr({dom_ref}, {target}, {}).unwrap_or_default()",
                    rust_str(name)
                );
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("rt::contains(&{attr}, {})", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrRe { name, pattern } => {
                let attr = format!(
                    "rt::pred_attr({dom_ref}, {target}, {}).unwrap_or_default()",
                    rust_str(name)
                );
                format!("rt::regex_match(&{attr}, {})", rust_regex_str(pattern))
            }
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} == {}", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} != {}", rust_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::TextStarts { values } | CondOp::Starts { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("rt::starts(&{text_val}, {})", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextEnds { values } | CondOp::Ends { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("rt::ends(&{text_val}, {})", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextContains { values } | CondOp::Contains { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("rt::contains(&{text_val}, {})", rust_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextRe { pattern } | CondOp::Re { pattern } => {
                format!("rt::regex_match(&{text_val}, {})", rust_regex_str(pattern))
            }
            CondOp::ReAny { pattern } => {
                format!(
                    "{target}.iter().any(|value| rt::regex_match(value, {}))",
                    rust_regex_str(pattern)
                )
            }
            CondOp::ReAll { pattern } => {
                format!(
                    "{target}.iter().all(|value| rt::regex_match(value, {}))",
                    rust_regex_str(pattern)
                )
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
                        .map(|v| format!("{count_expr} == {}", v.as_str_cow()))
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
                        .map(|v| format!("{count_expr} != {}", v.as_str_cow()))
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
                    .map(|v| format!("{count_expr} == {v}"))
                    .collect();
                join_or(&checks)
            }
            CondOp::LenNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{count_expr} != {v}"))
                    .collect();
                join_and(&checks)
            }
            CondOp::LenGt { value } => format!("{count_expr} > {value}"),
            CondOp::LenLt { value } => format!("{count_expr} < {value}"),
            CondOp::LenGe { value } => format!("{count_expr} >= {value}"),
            CondOp::LenLe { value } => format!("{count_expr} <= {value}"),
            CondOp::LenRange { start, end } => {
                format!("{start} < {count_expr} && {count_expr} < {end}")
            }
        }
    }

    fn format_cond_val(val: &CondValue, input_type: &TypeInfo) -> String {
        match val {
            CondValue::Int(n) => {
                if input_type.base == BaseType::Float && !input_type.is_array {
                    format!("{n} as f64")
                } else {
                    n.to_string()
                }
            }
            CondValue::Float(f) => format!("{f:?}"),
            CondValue::Bool(b) => b.to_string(),
            CondValue::String(s) => rust_str(s),
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
            format!("{target} {op} {value}")
        } else if input_type.base == BaseType::Float && !input_type.is_array {
            format!("{target} {op} {value} as f64")
        } else {
            format!("{count_expr} {op} {value}")
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
        let field_ident = rust_ident(&field.name);
        let field_type = Self::resolve_type(&field.type_info, module);
        let context = format!("{}.{}", struct_decl.name, field.name);

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        let _ = writer.next_var(); // v1 (used for self.parse_value(v)?)

        wln!(
            writer,
            "fn {field_ident}(&mut self, v: rt::Nodes) -> Result<Option<{field_type}>, rt::RtError> {{"
        );
        writer.indented(|writer| {
            let steps = &field.pipeline.steps;
            let (match_cond, remaining_steps) = match steps.first().map(|s| &s.op) {
                Some(StepOp::Match { cond }) => (Some(cond), &steps[1..]),
                _ => (None, &steps[..]),
            };

            if let Some(cond) = match_cond {
                let key_type = TypeInfo::string();
                let cond_str = self.render_cond(
                    cond,
                    "_key",
                    &key_type,
                    false,
                    field.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                writer.writeln("let _key = self.table_match_key(v.clone())?;");
                wln!(writer, "if !({cond_str}) {{ return Ok(None); }}");
            }

            writer.writeln("let v1 = self.parse_value(v)?;");

            if let Some(ref fb_val) = field.pipeline.fallback {
                let fallback_lit = rust_literal(fb_val, &field_type);
                wln!(
                    writer,
                    "let value = match (|| -> Result<{field_type}, rt::RtError> {{"
                );
                writer.indented(|writer| {
                    let last = if remaining_steps.is_empty() {
                        "v1".to_owned()
                    } else {
                        self.emit_pipeline_steps(
                            remaining_steps,
                            "v1",
                            &context,
                            false,
                            false,
                            false,
                            module,
                            builder,
                            writer,
                        )
                    };
                    let steps_end_opt = remaining_steps
                        .last()
                        .is_some_and(|s| s.output.is_optional || s.output.omitempty);
                    if field_type.starts_with("Option<") && !steps_end_opt {
                        wln!(writer, "Ok(Some({last}))");
                    } else {
                        wln!(writer, "Ok({last})");
                    }
                });
                wln!(
                    writer,
                    "}})() {{ Ok(val) => val, Err(_) => {fallback_lit} }};"
                );
                writer.writeln("Ok(Some(value))");
            } else {
                let last = if remaining_steps.is_empty() {
                    "v1".to_owned()
                } else {
                    self.emit_pipeline_steps(
                        remaining_steps,
                        "v1",
                        &context,
                        false,
                        false,
                        false,
                        module,
                        builder,
                        writer,
                    )
                };
                wln!(writer, "Ok(Some({last}))");
            }
        });
        writer.writeln("}");
        writer.blankline();
    }
}

impl LanguageEmitter for RustEmitter {
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
        writer.writeln("#![allow(dead_code, unused_imports)]");
        writer.blankline();
        if let Some(ref doc) = module.doc {
            emit_rust_doc(doc, "//", writer);
            writer.blankline();
        }
    }

    fn emit_imports(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let mut imports: IndexSet<String> = IndexSet::new();
        if module_has_html(module) {
            imports.insert("use std::cell::RefCell;".to_owned());
            imports.insert("use std::rc::Rc;".to_owned());
            imports.insert("use dom_query::{Document, NodeId};".to_owned());
        }
        if self.options.separate_runtime {
            let clean_rt = self
                .options
                .runtime_name
                .trim_start_matches('.')
                .trim_end_matches(".rs");
            let rt_ident = rust_ident(clean_rt);
            imports.insert(format!("use super::{rt_ident} as rt;"));
        }
        imports.insert("use serde::Serialize;".to_owned());
        for imp in builder.imports() {
            imports.insert(imp.to_owned());
        }
        for rt_name in builder.runtime_names() {
            imports.insert(format!("use rt::{};", rust_ident(rt_name)));
        }
        imports.insert("pub use rt::RtError;".to_owned());

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
            writer.writeln("pub mod rt {");
            writer.indented(|writer| {
                write_rust_runtime_body(module, builder, writer);
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
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = format!("{}Type", rust_pascal(&typedef.name));
        match typedef.struct_kind {
            StructKind::Rest => {}
            StructKind::Flat => {
                wln!(writer, "pub type {name} = Vec<String>;");
                writer.blankline();
            }
            StructKind::Dict => {
                let val_type = typedef
                    .fields
                    .iter()
                    .find(|f| f.name == "value")
                    .map(|f| Self::resolve_type(&f.type_info, module))
                    .unwrap_or_else(|| "String".to_owned());
                wln!(
                    writer,
                    "pub type {name} = std::collections::HashMap<String, {val_type}>;"
                );
                writer.blankline();
            }
            StructKind::Table => {
                builder.require_import("use serde_json::Value;");
                wln!(
                    writer,
                    "pub type {name} = std::collections::HashMap<String, serde_json::Value>;"
                );
                writer.blankline();
            }
            StructKind::Item | StructKind::List | StructKind::Raw => {
                writer.writeln("#[derive(Debug, Clone, PartialEq, Serialize)]");
                wln!(writer, "pub struct {name} {{");
                writer.indented(|writer| {
                    for field in &typedef.fields {
                        let typ = Self::resolve_type(&field.type_info, module);
                        if field.type_info.omitempty {
                            writer.writeln("#[serde(skip_serializing_if = \"Option::is_none\")]");
                        }
                        let f_ident = rust_ident(&field.name);
                        wln!(writer, "pub {f_ident}: {typ},");
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
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = rust_pascal(&struct_decl.name);
        if struct_decl.kind == StructKind::Rest {
            builder.require_import("use serde_json::Value;");
            if let Some(ref doc) = struct_decl.doc {
                emit_rust_doc(doc, "//", writer);
            }
            wln!(writer, "pub struct {name};");
            writer.blankline();
            wln!(writer, "pub type {name}Parser = {name};");
            writer.blankline();
            wln!(writer, "impl {name} {{");
            writer.indent();
            writer.writeln("pub fn new() -> Self {");
            writer.indented(|writer| {
                writer.writeln("Self");
            });
            writer.writeln("}");
            writer.blankline();
            return;
        }

        if struct_decl.kind == StructKind::Table {
            builder.require_import("use serde_json::Value;");
        }
        if let Some(ref doc) = struct_decl.doc {
            emit_rust_doc(doc, "///", writer);
        }

        let is_raw = struct_decl.kind == StructKind::Raw;
        wln!(writer, "pub struct {name}Parser {{");
        writer.indented(|writer| {
            if is_raw {
                writer.writeln("source: String,");
            } else {
                writer.writeln("dom: rt::Dom,");
                writer.writeln("root: NodeId,");
            }
            for init_f in &struct_decl.init_fields {
                let ident = rust_ident(&init_f.name);
                let typ = Self::resolve_type(&init_f.type_info, module);
                wln!(writer, "{ident}: {typ},");
            }
        });
        writer.writeln("}");
        writer.blankline();
        wln!(writer, "impl {name}Parser {{");
        writer.indent();
    }

    fn emit_init(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;

        writer.writeln("pub fn new(input: impl Into<String>) -> Result<Self, rt::RtError> {");
        writer.indented(|writer| {
            if is_raw {
                writer.writeln("let mut parser = Self {");
                writer.indented(|writer| {
                    writer.writeln("source: input.into(),");
                    for init_f in &struct_decl.init_fields {
                        let ident = rust_ident(&init_f.name);
                        wln!(writer, "{ident}: Default::default(),");
                    }
                });
                writer.writeln("};");
            } else {
                writer.writeln("let document = Document::from(input.into());");
                writer.writeln("let dom = Rc::new(RefCell::new(document));");
                writer.writeln("let root = rt::root_id(&dom);");
                writer.writeln("let mut parser = Self {");
                writer.indented(|writer| {
                    writer.writeln("dom,");
                    writer.writeln("root,");
                    for init_f in &struct_decl.init_fields {
                        let ident = rust_ident(&init_f.name);
                        wln!(writer, "{ident}: Default::default(),");
                    }
                });
                writer.writeln("};");
            }
            writer.writeln("parser.init()?;");
            writer.writeln("Ok(parser)");
        });
        writer.writeln("}");
        writer.blankline();

        if !is_raw {
            writer.writeln(
                "fn from_existing(dom: rt::Dom, root: NodeId) -> Result<Self, rt::RtError> {",
            );
            writer.indented(|writer| {
                writer.writeln("let mut parser = Self {");
                writer.indented(|writer| {
                    writer.writeln("dom,");
                    writer.writeln("root,");
                    for init_f in &struct_decl.init_fields {
                        let ident = rust_ident(&init_f.name);
                        wln!(writer, "{ident}: Default::default(),");
                    }
                });
                writer.writeln("};");
                writer.writeln("parser.init()?;");
                writer.writeln("Ok(parser)");
            });
            writer.writeln("}");
            writer.blankline();

            writer.writeln("fn root_nodes(&self) -> rt::Nodes { vec![self.root] }");
            writer.blankline();
        }

        writer.writeln("fn init(&mut self) -> Result<(), rt::RtError> {");
        writer.indented(|writer| {
            let input_expr = if is_raw {
                "self.source.clone()"
            } else {
                "self.root_nodes()"
            };
            for init_f in &struct_decl.init_fields {
                let ident = rust_ident(&init_f.name);
                wln!(writer, "self.{ident} = self.init_{ident}({input_expr})?;");
            }
            writer.writeln("Ok(())");
        });
        writer.writeln("}");
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
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ident = rust_ident(&init_field.name);
        let method_name = format!("init_{ident}");
        let ret_type = Self::resolve_type(&init_field.type_info, module);
        let context = format!("{}.{}", struct_decl.name, init_field.name);
        self.emit_pipeline_method(
            &method_name,
            false,
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
            "pre_validate",
            false,
            is_raw,
            pipeline,
            "()",
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
        let is_raw = struct_decl.kind == StructKind::Raw;
        let arg_ty = if is_raw { "String" } else { "rt::Nodes" };
        let init_val = if is_raw {
            "self.source.clone()"
        } else {
            "self.root_nodes()"
        };
        let method_ident = rust_ident(&check.name);
        let context = format!("{}.{}", struct_decl.name, check.name);

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        wln!(
            writer,
            "pub fn {method_ident}(&mut self) -> Result<bool, rt::RtError> {{"
        );
        writer.indented(|writer| {
            wln!(writer, "let v: {arg_ty} = {init_val};");
            self.emit_pipeline_body(
                &check.pipeline,
                "v",
                "bool",
                &context,
                is_raw,
                false,
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
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = if is_raw { "Vec<String>" } else { "rt::Nodes" };
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "split_doc",
            false,
            is_raw,
            pipeline,
            ret_type,
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
            "parse_key",
            false,
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
            "parse_value",
            false,
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
        builder.require_import("use serde_json::Value;");
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = Self::resolve_type(&pipeline.output, module);
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "table_config",
            false,
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
            "table_rows",
            false,
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
            "table_match_key",
            false,
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
        let method_ident = rust_ident(&field.name);
        let ret_type = Self::resolve_type(&field.type_info, module);
        let context = format!("{}.{}", struct_decl.name, field.name);
        self.emit_pipeline_method(
            &method_ident,
            false,
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
        let name = rust_pascal(&struct_decl.name);
        let result_type = Self::resolve_struct_result_type(&struct_decl.name, module);
        let is_raw = struct_decl.kind == StructKind::Raw;
        let has_raw_split = is_raw && struct_decl.split_doc.is_some();

        wln!(
            writer,
            "pub fn parse(&mut self) -> Result<{result_type}, rt::RtError> {{"
        );
        writer.indented(|writer| {
            if struct_decl.pre_validate.is_some() {
                let val = if is_raw {
                    "self.source.clone()"
                } else {
                    "self.root_nodes()"
                };
                wln!(writer, "self.pre_validate({val})?;");
            }

            if struct_decl.kind == StructKind::List || has_raw_split {
                let input_val = if has_raw_split {
                    "self.source.clone()"
                } else {
                    "self.root_nodes()"
                };
                wln!(writer, "let rows = self.split_doc({input_val})?;");
                writer.writeln("let mut result = Vec::new();");
                writer.writeln("for row in rows {");
                writer.indented(|writer| {
                    if !has_raw_split {
                        writer.writeln("let row = vec![row];");
                    }
                    if struct_decl.fields.is_empty() {
                        wln!(writer, "result.push({name}Type {{}});");
                    } else {
                        wln!(writer, "result.push({name}Type {{");
                        writer.indented(|writer| {
                            for field in &struct_decl.fields {
                                let ident = rust_ident(&field.name);
                                wln!(writer,
                                    "{ident}: self.{ident}(row.clone())?,"
                                );
                            }
                        });
                        writer.writeln("});");
                    }
                });
                writer.writeln("}");
                writer.writeln("Ok(result)");
            } else if struct_decl.kind == StructKind::Flat {
                writer.writeln("let mut result = Vec::new();");
                for field in &struct_decl.fields {
                    let ident = rust_ident(&field.name);
                    let ftype = Self::resolve_type(&field.type_info, module);
                    if field.type_info.is_optional || field.type_info.omitempty {
                        wln!(writer,
                            "if let Some(val) = self.{ident}(self.root_nodes())? {{"
                        );
                        writer.indented(|writer| {
                            let inner = ftype
                                .strip_prefix("Option<")
                                .and_then(|s| s.strip_suffix('>'))
                                .unwrap_or(&ftype);
                            if inner.starts_with("Vec<") {
                                writer.writeln("result.extend(val);");
                            } else {
                                writer.writeln("result.push(val);");
                            }
                        });
                        writer.writeln("}");
                    } else if ftype.starts_with("Vec<") {
                        wln!(writer,
                            "result.extend(self.{ident}(self.root_nodes())?);"
                        );
                    } else {
                        wln!(writer,
                            "result.push(self.{ident}(self.root_nodes())?);"
                        );
                    }
                }
                writer.writeln("Ok(rt::unique(result))");
            } else if struct_decl.kind == StructKind::Dict {
                writer.writeln("let rows = self.split_doc(self.root_nodes())?;");
                writer.writeln("let mut result = std::collections::HashMap::new();");
                writer.writeln("for row in rows {");
                writer.indented(|writer| {
                    writer.writeln("let row = vec![row];");
                    writer.writeln(
                        "result.insert(self.parse_key(row.clone())?, self.parse_value(row)?);",
                    );
                });
                writer.writeln("}");
                writer.writeln("Ok(result)");
            } else if struct_decl.kind == StructKind::Table {
                writer.writeln("let table = self.table_config(self.root_nodes())?;");
                writer.writeln("let rows = self.table_rows(table)?;");
                writer.writeln("let mut result = std::collections::HashMap::new();");
                writer.writeln("for row in rows {");
                writer.indented(|writer| {
                    writer.writeln("let row = vec![row];");
                    for field in &struct_decl.fields {
                        let key_name = to_snake_case(&field.name);
                        let key_lit = rust_str(&key_name);
                        let ident = rust_ident(&field.name);
                        let ctx_lit = rust_str(&format!("{name}.{}", field.name));
                        wln!(writer, "if !result.contains_key({key_lit}) {{");
                        writer.indented(|writer| {
                            wln!(writer,
                                "if let Some(value) = self.{ident}(row.clone())? {{"
                            );
                            writer.indented(|writer| {
                                wln!(writer,
                                    "result.insert({key_lit}.to_string(), serde_json::to_value(value).map_err(|error| rt::RtError::new({ctx_lit}, error.to_string()))?);"
                                );
                            });
                            writer.writeln("}");
                        });
                        writer.writeln("}");
                    }
                });
                writer.writeln("}");
                writer.writeln("Ok(result)");
            } else if struct_decl.fields.is_empty() {
                wln!(writer, "Ok({name}Type {{}})");
            } else {
                let val = if is_raw {
                    "self.source.clone()"
                } else {
                    "self.root_nodes()"
                };
                wln!(writer, "Ok({name}Type {{");
                writer.indented(|writer| {
                    for field in &struct_decl.fields {
                        let ident = rust_ident(&field.name);
                        wln!(writer, "{ident}: self.{ident}({val})?,");
                    }
                });
                writer.writeln("})");
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
        let name = rust_ident(&func.name);
        let ret_type = Self::resolve_type(&func.return_type, module);
        if let Some(ref doc) = func.doc {
            emit_rust_doc(doc, "///", writer);
        }

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        wln!(
            writer,
            "pub fn {name}(document: impl Into<String>) -> Result<{ret_type}, rt::RtError> {{"
        );
        writer.indented(|writer| {
            if func.is_raw {
                writer.writeln("let v = document.into();");
            } else {
                writer.writeln("let document = Document::from(document.into());");
                writer.writeln("let dom = Rc::new(RefCell::new(document));");
                writer.writeln("let root = rt::root_id(&dom);");
                writer.writeln("let v = vec![root];");
            }
            self.emit_pipeline_body(
                &func.pipeline,
                "v",
                &ret_type,
                &func.name,
                func.is_raw,
                true,
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

/// Emits Rust source files for a compiled [`Module`] and [`CodegenOptions`].
///
/// Returns `(parser_source, optional_runtime_source, builder)` or `Err(diagnostics)`.
pub fn emit_rust_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, Option<String>, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = RustEmitter::new(options.clone());
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
        Some(emit_rust_runtime_module(module, &builder, options))
    } else {
        None
    };
    Ok((parser_code, runtime_code, builder))
}

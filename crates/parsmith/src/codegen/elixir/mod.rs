//! Elixir 1.18+ (`Floki` + built-in `JSON` + OTP `:httpc` / `Req`) target backend (`ElixirEmitter`).
//!
//! Emits 2-space indented Elixir source code directly from [`CodeWriter`], defaulting to a single
//! self-contained `<stem>.ex` file with an inline `defmodule <Prefix>Rt` (omitting `Floki` helpers
//! when `!module_has_html(module)`), while supporting `-R / --separate-runtime`
//! (`<stem>.ex` + `<runtime_name>.ex`) and `--package <ModulePrefix>`.

pub mod json;
pub mod rest;
pub mod runtime;

use std::fmt::Write as _;

use crate::kdl::Span;

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, FieldDecl, FunctionDef, InitFieldDecl, JsonDef,
    LiteralValue, Module, Pipeline, RawMode, RequestDecl, RestArtifact, RestEndpointDecl, StepOp,
    StructDecl, StructKind, StructTypeDef, TypeInfo, TypedStep,
};
use crate::codegen::CodegenOptions;
use crate::codegen::builder::{ModuleBuilder, emit_extension_call};
use crate::codegen::emitter::{LanguageEmitter, drive_module};
use crate::codegen::writer::{CodeWriter, wln};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::{ELIXIR_RESERVED, is_valid_symbol, to_snake_case};
use crate::lowering::clean_doc;
use crate::validators::normalize_regex_pattern;

pub use rest::{render_json_body, render_template};
pub use runtime::{
    ELIXIR_CORE_RUNTIME, ELIXIR_HTML_RUNTIME, ELIXIR_HTTP_RUNTIME, emit_elixir_runtime_module,
    write_elixir_runtime_body,
};

/// Validates that `pkg` is either empty, `"main"`, or a `.`-separated sequence of valid Elixir
/// module segments (`MyApp.Parsers`, `my_app`, etc.).
pub fn validate_elixir_package_name(pkg: &str) -> bool {
    if pkg.is_empty() || pkg == "main" {
        return true;
    }
    pkg.split('.').all(|seg| {
        !seg.is_empty() && is_valid_symbol("elixir", seg) && !ELIXIR_RESERVED.contains(&seg)
    })
}

/// Formats `pkg` as a trailing-dot `PascalCase` Elixir module prefix (e.g. `"MyApp.Parsers."`),
/// or `""` when `pkg` is empty or `"main"`.
pub fn format_elixir_module_prefix(pkg: &str) -> String {
    if pkg.is_empty() || pkg == "main" {
        return String::new();
    }
    let joined = pkg
        .split('.')
        .map(elixir_pascal)
        .collect::<Vec<_>>()
        .join(".");
    format!("{joined}.")
}

/// Escapes the inner content of a double-quoted Elixir string literal (including `\#{` interpolation prevention).
pub fn elixir_str_inner(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '#' if chars.peek() == Some(&'{') => out.push_str("\\#"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Formats a `&str` as a valid double-quoted Elixir string literal (`"..."`).
pub fn elixir_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    out.push_str(&elixir_str_inner(s));
    out.push('"');
    out
}

fn elixir_regex_str(pattern: &str) -> String {
    elixir_str(&normalize_regex_pattern(pattern))
}

/// Renders a DSL identifier as a `snake_case` Elixir struct field atom name (preserving exact JSON keys).
pub fn elixir_field_name(value: &str) -> String {
    let name = to_snake_case(value).replace('-', "_");
    if name.is_empty() {
        "value".to_owned()
    } else {
        name
    }
}

/// Renders a DSL identifier as a valid `snake_case` Elixir function or variable identifier,
/// suffixing reserved keywords with `_`.
pub fn elixir_ident(value: &str) -> String {
    let name = elixir_field_name(value);
    if ELIXIR_RESERVED.contains(&name.as_str()) {
        format!("{name}_")
    } else {
        name
    }
}

/// Renders a DSL identifier as a `PascalCase` Elixir module segment name.
pub fn elixir_pascal(value: &str) -> String {
    let name = to_pascal_case(value).replace('-', "");
    if name.is_empty() {
        "Value".to_owned()
    } else {
        name
    }
}

/// Maps a dictionary key [`TypeInfo`] to its Elixir `@type` representation.
pub fn elixir_key_type(info: Option<&TypeInfo>) -> &'static str {
    let Some(info) = info else {
        return "String.t()";
    };
    match info.base {
        BaseType::String => "String.t()",
        BaseType::Int => "integer()",
        BaseType::Float => "float()",
        BaseType::Bool => "boolean()",
        _ => "String.t()",
    }
}

/// Formats a [`LiteralValue`] as an Elixir literal expression.
pub fn elixir_literal(val: &LiteralValue) -> String {
    match val {
        LiteralValue::Null => "nil".to_owned(),
        LiteralValue::Bool(b) => b.to_string(),
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => elixir_str(s),
        LiteralValue::EmptyArray => "[]".to_owned(),
    }
}

/// Translates DSL/Python numeric replacement groups (`\1`, `$1`, `${1}`) into Elixir `Regex.replace`
/// `\g{1}` backreference syntax while preserving literal `$` and escaping literal `\`.
pub fn elixir_replacement(replacement: &str) -> String {
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
            let _ = write!(out, "\\g{{{group}}}");
            i = end;
        } else if bytes[i] == b'$' {
            if i + 1 < n && bytes[i + 1] == b'$' {
                out.push('$');
                i += 2;
            } else if i + 1 < n && bytes[i + 1] == b'{' {
                let start = i + 2;
                let mut end = start;
                while end < n && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if end > start && end < n && bytes[end] == b'}' && bytes[start] != b'0' {
                    let group = &replacement[start..end];
                    let _ = write!(out, "\\g{{{group}}}");
                    i = end + 1;
                } else {
                    out.push('$');
                    i += 1;
                }
            } else if i + 1 < n && bytes[i + 1].is_ascii_digit() && bytes[i + 1] != b'0' {
                let start = i + 1;
                let mut end = start + 1;
                while end < n && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let group = &replacement[start..end];
                let _ = write!(out, "\\g{{{group}}}");
                i = end;
            } else {
                out.push('$');
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

/// Writes cleaned Elixir documentation (`@moduledoc` / `@doc` or `# ...`) to `writer`.
pub fn emit_elixir_doc(doc: &str, attr: &str, writer: &mut CodeWriter) {
    let cleaned = clean_doc(doc);
    if cleaned.is_empty() {
        return;
    }
    if attr == "#" {
        for line in cleaned.lines() {
            if line.trim().is_empty() {
                writer.writeln("#");
            } else {
                wln!(writer, "# {line}");
            }
        }
    } else if !cleaned.contains('\n') {
        wln!(writer, "{attr} {}", elixir_str(&cleaned));
    } else {
        wln!(writer, "{attr} \"\"\"");
        for line in cleaned.lines() {
            writer.writeln(elixir_str_inner(line));
        }
        writer.writeln("\"\"\"");
    }
}

fn join_or(exprs: &[String]) -> String {
    if exprs.is_empty() {
        "false".to_owned()
    } else if exprs.len() == 1 {
        exprs[0].clone()
    } else {
        format!("({})", exprs.join(" or "))
    }
}

fn join_and(exprs: &[String]) -> String {
    if exprs.is_empty() {
        "true".to_owned()
    } else if exprs.len() == 1 {
        exprs[0].clone()
    } else {
        format!("({})", exprs.join(" and "))
    }
}

/// Elixir target `LanguageEmitter` implementation.
pub struct ElixirEmitter {
    /// Code generation options.
    pub options: CodegenOptions,
    /// Diagnostics accumulated during code generation (`E203`, `E400`).
    pub diagnostics: Vec<Diagnostic>,
    /// Tracks whether `defmodule <Prefix>Functions` has been opened in the current pass.
    in_functions_module: bool,
}

impl ElixirEmitter {
    /// Creates a new `ElixirEmitter` for `options`.
    pub fn new(options: CodegenOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            in_functions_module: false,
        }
    }

    /// Emits `alias <Prefix><RtMod>, as: Rt` inside a generated module when needed.
    pub(crate) fn emit_rt_alias(&self, writer: &mut CodeWriter) {
        let prefix = format_elixir_module_prefix(&self.options.package);
        if self.options.separate_runtime {
            let clean_rt = self
                .options
                .runtime_name
                .trim_start_matches('.')
                .trim_end_matches(".ex");
            let rt_mod = format!("{prefix}{}", elixir_pascal(clean_rt));
            if rt_mod != "Rt" {
                wln!(writer, "alias {rt_mod}, as: Rt");
                writer.blankline();
            }
        } else if !prefix.is_empty() {
            wln!(writer, "alias {prefix}Rt, as: Rt");
            writer.blankline();
        }
    }

    /// Resolves a [`TypeInfo`] into its Elixir `@type` specification.
    pub fn resolve_type(&self, info: &TypeInfo, module: &Module) -> String {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let optional = info.is_optional || info.omitempty;
        if info.is_dict {
            let key_type = elixir_key_type(info.key_type.as_deref());
            let val_type = info
                .value_type
                .as_deref()
                .map(|vt| self.resolve_type(vt, module))
                .unwrap_or_else(|| "String.t()".to_owned());
            let mut res = format!("%{{optional({key_type}) => {val_type}}}");
            if info.is_array {
                res = format!("[{res}]");
            }
            if optional {
                res = format!("{res} | nil");
            }
            return res;
        }

        let mut res = match &info.base {
            BaseType::Document => "Rt.nodes()".to_owned(),
            BaseType::Nested(ref_name) => self.resolve_struct_result_type(ref_name, module),
            BaseType::Json(ref_name) => {
                let pascal = elixir_pascal(ref_name);
                let def_is_arr = module.json_def(ref_name).is_some_and(|j| j.is_array);
                if info.is_array && !def_is_arr {
                    format!("[{prefix}{pascal}Json.t()]")
                } else {
                    format!("{prefix}{pascal}Json.t()")
                }
            }
            BaseType::String | BaseType::Auto | BaseType::DictValue => "String.t()".to_owned(),
            BaseType::Int => "integer()".to_owned(),
            BaseType::Float => "float()".to_owned(),
            BaseType::Bool => "boolean()".to_owned(),
            BaseType::Null => "nil".to_owned(),
        };

        if info.is_array
            && !matches!(
                info.base,
                BaseType::Document | BaseType::Nested(_) | BaseType::Json(_)
            )
        {
            res = format!("[{res}]");
        }
        if optional && res != "nil" && !res.ends_with("| nil") {
            res = format!("{res} | nil");
        }
        res
    }

    /// Resolves the `@type` specification for the output of a struct's `parse/1` function.
    pub fn resolve_struct_result_type(&self, name: &str, module: &Module) -> String {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let base = format!("{prefix}{}Type.t()", elixir_pascal(name));
        let Some(struct_decl) = module.struct_decl(name) else {
            return base;
        };
        match struct_decl.kind {
            StructKind::List => format!("[{base}]"),
            StructKind::Flat => "[String.t()]".to_owned(),
            StructKind::Raw if struct_decl.split_doc.is_some() => format!("[{base}]"),
            _ => base,
        }
    }

    fn record_unsupported_xpath(&mut self, span: Span, source_file: &str) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E203,
                "XPath operations ('xpath', 'xpath-all', 'xpath-remove') are not supported by the 'elixir' backend",
                span,
            )
            .with_path(source_file.to_owned()),
        );
    }

    fn pipeline_uses_self(pipeline: &Pipeline) -> bool {
        pipeline
            .steps
            .iter()
            .any(|s| matches!(s.op, StepOp::LoadInit { .. }))
    }

    fn pipeline_uses_v(pipeline: &Pipeline) -> bool {
        pipeline.steps.is_empty() || !matches!(pipeline.steps[0].op, StepOp::LoadInit { .. })
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_method(
        &mut self,
        method_name: &str,
        is_pub: bool,
        is_raw: bool,
        pipeline: &Pipeline,
        context: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let def_kw = if is_pub { "def" } else { "defp" };
        let self_param = if Self::pipeline_uses_self(pipeline) {
            "%__MODULE__{} = self"
        } else {
            "_self"
        };
        let v_param = if Self::pipeline_uses_v(pipeline) {
            "v"
        } else {
            "_v"
        };

        writer.reset_vars();
        let _ = writer.next_var(); // reserve v0 so steps start at v1
        wln!(writer, "{def_kw} {method_name}({self_param}, {v_param}) do");
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline,
                "v",
                context,
                is_raw,
                is_pre_validate,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("end");
        writer.blankline();
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_body(
        &mut self,
        pipeline: &Pipeline,
        initial_var: &str,
        context: &str,
        is_raw: bool,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref fb_val) = pipeline.fallback {
            let fallback_lit = elixir_literal(fb_val);
            writer.writeln("Rt.fallback(fn ->");
            writer.indented(|writer| {
                let last = self.emit_pipeline_steps(
                    &pipeline.steps,
                    initial_var,
                    context,
                    is_raw,
                    is_pre_validate,
                    module,
                    builder,
                    writer,
                );
                if is_pre_validate {
                    writer.writeln(":ok");
                } else {
                    writer.writeln(last);
                }
            });
            wln!(writer, "end, {fallback_lit})");
        } else {
            let last = self.emit_pipeline_steps(
                &pipeline.steps,
                initial_var,
                context,
                is_raw,
                is_pre_validate,
                module,
                builder,
                writer,
            );
            if is_pre_validate {
                writer.writeln(":ok");
            } else {
                writer.writeln(last);
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
                "{nxt} = Enum.map({prv}, fn value -> Rt.{func}(value{suffix}) end)"
            );
        } else {
            wln!(writer, "{nxt} = Rt.{func}({prv}{suffix})");
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
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let ctx_lit = elixir_str(context);
        let prefix = format_elixir_module_prefix(&self.options.package);

        match &step.op {
            StepOp::Value { .. } => {}
            StepOp::LoadInit { name } => {
                let f_name = elixir_field_name(name);
                wln!(writer, "{nxt} = self.{f_name}");
            }
            StepOp::Css { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| elixir_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                if next_is_to_bool {
                    wln!(writer, "{nxt} = Rt.select_opt({prv}, [{q_list}])");
                } else {
                    wln!(
                        writer,
                        "{nxt} = Rt.select_first({prv}, [{q_list}], {ctx_lit})"
                    );
                }
            }
            StepOp::CssAll { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| elixir_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(
                    writer,
                    "{nxt} = Rt.select_all({prv}, [{q_list}], {ctx_lit})"
                );
            }
            StepOp::CssRemove { query } => {
                wln!(
                    writer,
                    "{nxt} = Rt.select_remove({prv}, {})",
                    elixir_str(query)
                );
            }
            StepOp::Xpath { .. } | StepOp::XpathAll { .. } | StepOp::XpathRemove { .. } => {
                if writer.is_dry_run() {
                    self.record_unsupported_xpath(step.span, &module.source_file);
                }
            }
            StepOp::Text => {
                if step.output.is_array || step.input.is_array {
                    wln!(writer, "{nxt} = Rt.text_all({prv})");
                } else {
                    wln!(writer, "{nxt} = Rt.text({prv}, {ctx_lit})");
                }
            }
            StepOp::Raw { mode } => {
                let inner = if *mode == RawMode::Inner {
                    "true"
                } else {
                    "false"
                };
                if step.output.is_array || step.input.is_array {
                    wln!(writer, "{nxt} = Rt.raw_all({prv}, {inner})");
                } else {
                    wln!(writer, "{nxt} = Rt.raw_html({prv}, {inner}, {ctx_lit})");
                }
            }
            StepOp::Attr { keys } => {
                let names = keys
                    .iter()
                    .map(|k| elixir_str(k))
                    .collect::<Vec<_>>()
                    .join(", ");
                if keys.len() > 1 {
                    wln!(writer, "{nxt} = Rt.attr_multi({prv}, [{names}])");
                } else if step.input.is_array {
                    wln!(writer, "{nxt} = Rt.attr_list({prv}, [{names}], {ctx_lit})");
                } else {
                    wln!(writer, "{nxt} = Rt.attr({prv}, [{names}], {ctx_lit})");
                }
            }
            StepOp::Trim { chars } => {
                let arg = if chars.is_empty() {
                    "nil".to_owned()
                } else {
                    elixir_str(chars)
                };
                Self::emit_string_call(step, prv, nxt, "trim", &[arg], writer);
            }
            StepOp::Ltrim { chars } => {
                let arg = if chars.is_empty() {
                    "nil".to_owned()
                } else {
                    elixir_str(chars)
                };
                Self::emit_string_call(step, prv, nxt, "ltrim", &[arg], writer);
            }
            StepOp::Rtrim { chars } => {
                let arg = if chars.is_empty() {
                    "nil".to_owned()
                } else {
                    elixir_str(chars)
                };
                Self::emit_string_call(step, prv, nxt, "rtrim", &[arg], writer);
            }
            StepOp::NormalizeSpace => {
                Self::emit_string_call(step, prv, nxt, "normalize_space", &[], writer);
            }
            StepOp::RmPrefix { substr } => {
                Self::emit_string_call(step, prv, nxt, "rm_prefix", &[elixir_str(substr)], writer);
            }
            StepOp::RmSuffix { substr } => {
                Self::emit_string_call(step, prv, nxt, "rm_suffix", &[elixir_str(substr)], writer);
            }
            StepOp::RmPrefixSuffix { substr } => {
                Self::emit_string_call(
                    step,
                    prv,
                    nxt,
                    "rm_prefix_suffix",
                    &[elixir_str(substr)],
                    writer,
                );
            }
            StepOp::Fmt { template } => {
                let tmpl_lit = elixir_str(template);
                if step.output.is_array {
                    wln!(
                        writer,
                        "{nxt} = Enum.map({prv}, fn value -> Rt.fmt_value({tmpl_lit}, value) end)"
                    );
                } else {
                    wln!(writer, "{nxt} = Rt.fmt_value({tmpl_lit}, {prv})");
                }
            }
            StepOp::Repl { old, new } => {
                Self::emit_string_call(
                    step,
                    prv,
                    nxt,
                    "repl",
                    &[elixir_str(old), elixir_str(new)],
                    writer,
                );
            }
            StepOp::ReplMap { replacements } => {
                let pairs = replacements
                    .iter()
                    .map(|(k, v)| format!("{{{}, {}}}", elixir_str(k), elixir_str(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let list_arg = format!("[{pairs}]");
                Self::emit_string_call(step, prv, nxt, "repl_map", &[list_arg], writer);
            }
            StepOp::Lower => {
                Self::emit_string_call(step, prv, nxt, "lower", &[], writer);
            }
            StepOp::Upper => {
                Self::emit_string_call(step, prv, nxt, "upper", &[], writer);
            }
            StepOp::Split { sep } => {
                wln!(writer, "{nxt} = String.split({prv}, {})", elixir_str(sep));
            }
            StepOp::Join { sep } => {
                wln!(writer, "{nxt} = Enum.join({prv}, {})", elixir_str(sep));
            }
            StepOp::Unescape => {
                Self::emit_string_call(step, prv, nxt, "unescape", &[], writer);
            }
            StepOp::Re { pattern } => {
                let pat_lit = elixir_regex_str(pattern);
                if step.output.is_array {
                    wln!(
                        writer,
                        "{nxt} = Enum.map({prv}, fn value -> Rt.regex_search(value, {pat_lit}, {ctx_lit}) end)"
                    );
                } else {
                    wln!(
                        writer,
                        "{nxt} = Rt.regex_search({prv}, {pat_lit}, {ctx_lit})"
                    );
                }
            }
            StepOp::ReAll { pattern } => {
                let pat_lit = elixir_regex_str(pattern);
                wln!(writer, "{nxt} = Rt.regex_all({prv}, {pat_lit}, {ctx_lit})");
            }
            StepOp::ReSub { pattern, repl } => {
                let pat_lit = elixir_regex_str(pattern);
                let repl_lit = elixir_str(&elixir_replacement(repl));
                if step.output.is_array {
                    wln!(
                        writer,
                        "{nxt} = Enum.map({prv}, fn value -> Rt.regex_sub(value, {pat_lit}, {repl_lit}, {ctx_lit}) end)"
                    );
                } else {
                    wln!(
                        writer,
                        "{nxt} = Rt.regex_sub({prv}, {pat_lit}, {repl_lit}, {ctx_lit})"
                    );
                }
            }
            StepOp::ReSplit { pattern } => {
                let pat_lit = elixir_regex_str(pattern);
                wln!(
                    writer,
                    "{nxt} = Rt.regex_split({prv}, {pat_lit}, {ctx_lit})"
                );
            }
            StepOp::Index { index } => {
                if step.input.base == BaseType::Document {
                    wln!(writer, "{nxt} = Rt.node_at({prv}, {index}, {ctx_lit})");
                } else if step.input.is_array {
                    wln!(writer, "{nxt} = Rt.item_at({prv}, {index}, {ctx_lit})");
                } else {
                    wln!(writer, "{nxt} = Rt.index({prv}, {index}, {ctx_lit})");
                }
            }
            StepOp::Slice { start, end } => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "{nxt} = Rt.slice({prv}, {start}, {end})");
                } else {
                    wln!(writer, "{nxt} = Rt.slice_vec({prv}, {start}, {end})");
                }
            }
            StepOp::Len => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "{nxt} = Rt.str_len({prv})");
                } else {
                    wln!(writer, "{nxt} = Rt.len({prv})");
                }
            }
            StepOp::Unique { .. } => {
                wln!(writer, "{nxt} = Rt.unique({prv})");
            }
            StepOp::ToInt => {
                if step.output.is_array {
                    wln!(
                        writer,
                        "{nxt} = Enum.map({prv}, fn value -> Rt.to_int(value, {ctx_lit}) end)"
                    );
                } else {
                    wln!(writer, "{nxt} = Rt.to_int({prv}, {ctx_lit})");
                }
            }
            StepOp::ToFloat => {
                if step.output.is_array {
                    wln!(
                        writer,
                        "{nxt} = Enum.map({prv}, fn value -> Rt.to_float(value, {ctx_lit}) end)"
                    );
                } else {
                    wln!(writer, "{nxt} = Rt.to_float({prv}, {ctx_lit})");
                }
            }
            StepOp::ToBool => {
                let expr = if step.input.base == BaseType::Document {
                    format!("Rt.node_bool({prv})")
                } else if step.input.is_array {
                    format!("Rt.seq_bool({prv})")
                } else if step.input.base == BaseType::Int {
                    format!("Rt.num_bool({prv})")
                } else if step.input.base == BaseType::Float {
                    format!("Rt.float_bool({prv})")
                } else if step.input.base == BaseType::Bool {
                    prv.to_owned()
                } else {
                    format!("Rt.string_bool({prv})")
                };
                wln!(writer, "{nxt} = {expr}");
            }
            StepOp::Jsonify {
                schema_name, path, ..
            } => {
                let mod_name = format!("{prefix}{}Json", elixir_pascal(schema_name));
                let effective_path = path
                    .as_deref()
                    .or_else(|| module.json_def(schema_name).and_then(|j| j.path.as_deref()))
                    .unwrap_or("");
                let parsed = format!(
                    "Rt.parse_json({prv}, {}, {ctx_lit})",
                    elixir_str(effective_path)
                );
                let def_is_arr = module.json_def(schema_name).is_some_and(|j| j.is_array);
                if step.output.is_array && !def_is_arr {
                    wln!(
                        writer,
                        "{nxt} = Rt.decode_list({parsed}, &{mod_name}.project/1, {ctx_lit})"
                    );
                } else {
                    wln!(writer, "{nxt} = {mod_name}.project({parsed})");
                }
            }
            StepOp::Nested { struct_name } => {
                let cls = format!("{prefix}{}", elixir_pascal(struct_name));
                let is_child_raw = module
                    .struct_decl(struct_name)
                    .is_some_and(|s| s.kind == StructKind::Raw);
                let is_input_str = step.input.base == BaseType::String;
                if is_raw || is_child_raw || is_input_str {
                    wln!(writer, "{nxt} = {cls}.new({prv}) |> {cls}.parse()");
                } else {
                    wln!(writer, "{nxt} = {cls}.from_nodes({prv}) |> {cls}.parse()");
                }
            }
            StepOp::Filter { cond } => {
                let elem_type = TypeInfo {
                    is_array: false,
                    ..step.input.clone()
                };
                let cond_str = self.render_cond(
                    cond,
                    "i",
                    &elem_type,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                if step.input.base == BaseType::Document {
                    wln!(
                        writer,
                        "{nxt} = Enum.filter({prv}, fn node -> i = [node]; {cond_str} end)"
                    );
                } else {
                    wln!(writer, "{nxt} = Enum.filter({prv}, fn i -> {cond_str} end)");
                }
            }
            StepOp::Assert { message, cond } => {
                let cond_str = self.render_cond(
                    cond,
                    prv,
                    &step.input,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                let msg = message.as_deref().unwrap_or("assertion failed");
                wln!(
                    writer,
                    "Rt.assert_true({cond_str}, {ctx_lit}, {})",
                    elixir_str(msg)
                );
                if !(is_pre_validate && is_last_step) {
                    wln!(writer, "{nxt} = {prv}");
                }
            }
            StepOp::Match { .. } => {}
            StepOp::ExtensionCall {
                qualified_name,
                bound_input,
                bound_output,
                ..
            } => {
                let in_type = self.resolve_type(bound_input, module);
                let out_type = self.resolve_type(bound_output, module);
                emit_extension_call(
                    module,
                    qualified_name,
                    step.span,
                    &["ex"],
                    "'ex'",
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

    fn render_cond(
        &mut self,
        cond: &Cond,
        target: &str,
        input_type: &TypeInfo,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
    ) -> String {
        match cond {
            Cond::Atom(op) => {
                self.render_cond_op(op, target, input_type, span, source_file, is_dry_run)
            }
            Cond::Not(inner) => {
                let inner_s =
                    self.render_cond(inner, target, input_type, span, source_file, is_dry_run);
                format!("not ({inner_s})")
            }
            Cond::And(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|c| self.render_cond(c, target, input_type, span, source_file, is_dry_run))
                    .collect();
                join_and(&parts)
            }
            Cond::Or(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|c| self.render_cond(c, target, input_type, span, source_file, is_dry_run))
                    .collect();
                join_or(&parts)
            }
        }
    }

    fn render_cond_op(
        &mut self,
        op: &CondOp,
        target: &str,
        input_type: &TypeInfo,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
    ) -> String {
        let text_val = if input_type.base == BaseType::Document {
            format!("Rt.pred_text({target})")
        } else {
            target.to_owned()
        };
        let count_expr = if input_type.base == BaseType::String && !input_type.is_array {
            format!("Rt.str_len({target})")
        } else {
            format!("Rt.len({target})")
        };

        match op {
            CondOp::Css { query } => {
                format!("Rt.pred_css?({target}, {})", elixir_str(query))
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
                    .map(|name| format!("Rt.pred_has_attr?({target}, {})", elixir_str(name)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                let attr = format!("(Rt.pred_attr({target}, {}) || \"\")", elixir_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} == {}", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrNe { name, values } => {
                let attr = format!("(Rt.pred_attr({target}, {}) || \"\")", elixir_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} != {}", elixir_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::AttrStarts { name, values } => {
                let attr = format!("(Rt.pred_attr({target}, {}) || \"\")", elixir_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.starts?({attr}, {})", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEnds { name, values } => {
                let attr = format!("(Rt.pred_attr({target}, {}) || \"\")", elixir_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.ends?({attr}, {})", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrContains { name, values } => {
                let attr = format!("(Rt.pred_attr({target}, {}) || \"\")", elixir_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.contains?({attr}, {})", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrRe { name, pattern } => {
                let attr = format!("(Rt.pred_attr({target}, {}) || \"\")", elixir_str(name));
                format!("Rt.regex_match?({attr}, {})", elixir_regex_str(pattern))
            }
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} == {}", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} != {}", elixir_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::TextStarts { values } | CondOp::Starts { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.starts?({text_val}, {})", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextEnds { values } | CondOp::Ends { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.ends?({text_val}, {})", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextContains { values } | CondOp::Contains { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.contains?({text_val}, {})", elixir_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextRe { pattern } | CondOp::Re { pattern } => {
                format!("Rt.regex_match?({text_val}, {})", elixir_regex_str(pattern))
            }
            CondOp::ReAny { pattern } => {
                format!(
                    "Enum.any?({target}, fn value -> Rt.regex_match?(value, {}) end)",
                    elixir_regex_str(pattern)
                )
            }
            CondOp::ReAll { pattern } => {
                format!(
                    "Enum.all?({target}, fn value -> Rt.regex_match?(value, {}) end)",
                    elixir_regex_str(pattern)
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
                        .map(|v| format!("{target} == {}", Self::format_cond_val(v)))
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
                        .map(|v| format!("{target} != {}", Self::format_cond_val(v)))
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
                format!("{start} < {count_expr} and {count_expr} < {end}")
            }
        }
    }

    fn format_cond_val(val: &CondValue) -> String {
        match val {
            CondValue::Int(n) => n.to_string(),
            CondValue::Float(f) => format!("{f:?}"),
            CondValue::Bool(b) => b.to_string(),
            CondValue::String(s) => elixir_str(s),
        }
    }

    fn format_num_or_len_cmp(
        target: &str,
        count_expr: &str,
        op: &str,
        value: i64,
        input_type: &TypeInfo,
    ) -> String {
        if matches!(input_type.base, BaseType::Int | BaseType::Float) && !input_type.is_array {
            format!("{target} {op} {value}")
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
        let f_name = elixir_field_name(&field.name);
        let method_name = format!("parse_{f_name}");
        let context = format!("{}.{}", struct_decl.name, field.name);

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        let _ = writer.next_var(); // v1 (used for parse_value(self, v))

        wln!(writer, "defp {method_name}(self, v) do");
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
                    "key",
                    &key_type,
                    field.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                writer.writeln("key = table_match_key(self, v)");
                wln!(writer, "if not ({cond_str}) do");
                writer.indented(|writer| {
                    writer.writeln(":unmatched");
                });
                writer.writeln("else");
                writer.indented(|writer| {
                    self.emit_table_field_matched_body(
                        field,
                        remaining_steps,
                        &context,
                        module,
                        builder,
                        writer,
                    );
                });
                writer.writeln("end");
            } else {
                self.emit_table_field_matched_body(
                    field,
                    remaining_steps,
                    &context,
                    module,
                    builder,
                    writer,
                );
            }
        });
        writer.writeln("end");
        writer.blankline();
    }

    fn emit_table_field_matched_body(
        &mut self,
        field: &FieldDecl,
        remaining_steps: &[TypedStep],
        context: &str,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.writeln("v1 = parse_value(self, v)");
        if let Some(ref fb_val) = field.pipeline.fallback {
            let fallback_lit = elixir_literal(fb_val);
            writer.writeln("value = Rt.fallback(fn ->");
            writer.indented(|writer| {
                let last = if remaining_steps.is_empty() {
                    "v1".to_owned()
                } else {
                    self.emit_pipeline_steps(
                        remaining_steps,
                        "v1",
                        context,
                        false,
                        false,
                        module,
                        builder,
                        writer,
                    )
                };
                writer.writeln(last);
            });
            wln!(writer, "end, {fallback_lit})");
            writer.writeln("{:matched, value}");
        } else {
            let last = if remaining_steps.is_empty() {
                "v1".to_owned()
            } else {
                self.emit_pipeline_steps(
                    remaining_steps,
                    "v1",
                    context,
                    false,
                    false,
                    module,
                    builder,
                    writer,
                )
            };
            wln!(writer, "{{:matched, {last}}}");
        }
    }
}

impl LanguageEmitter for ElixirEmitter {
    fn indent_unit(&self) -> &'static str {
        "  "
    }

    fn on_module_start(
        &mut self,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.in_functions_module = false;
        writer.writeln("# Auto-generated by parsmith. DO NOT EDIT.");
        writer.blankline();
        if let Some(ref doc) = module.doc {
            emit_elixir_doc(doc, "#", writer);
            writer.blankline();
        }
    }

    fn emit_imports(
        &mut self,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        for imp in builder.imports() {
            writer.write_block(imp);
        }
        if !builder.imports().is_empty() {
            writer.blankline();
        }
    }

    fn emit_utilities(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if !self.options.separate_runtime {
            let prefix = format_elixir_module_prefix(&self.options.package);
            wln!(writer, "defmodule {prefix}Rt do");
            writer.indented(|writer| {
                write_elixir_runtime_body(module, builder, &self.options, writer);
            });
            writer.writeln("end");
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
        let prefix = format_elixir_module_prefix(&self.options.package);
        let mod_name = format!("{prefix}{}Type", elixir_pascal(&typedef.name));
        match typedef.struct_kind {
            StructKind::Rest => {}
            StructKind::Flat => {
                wln!(writer, "defmodule {mod_name} do");
                writer.indented(|writer| {
                    writer.writeln("@type t :: [String.t()]");
                });
                writer.writeln("end");
                writer.blankline();
            }
            StructKind::Dict => {
                let val_type = typedef
                    .fields
                    .iter()
                    .find(|f| f.name == "value")
                    .map(|f| self.resolve_type(&f.type_info, module))
                    .unwrap_or_else(|| "String.t()".to_owned());
                wln!(writer, "defmodule {mod_name} do");
                writer.indented(|writer| {
                    self.emit_rt_alias(writer);
                    wln!(writer, "@type t :: %{{optional(String.t()) => {val_type}}}");
                });
                writer.writeln("end");
                writer.blankline();
            }
            StructKind::Table => {
                wln!(writer, "defmodule {mod_name} do");
                writer.indented(|writer| {
                    writer.writeln("@type t :: %{optional(String.t()) => term()}");
                });
                writer.writeln("end");
                writer.blankline();
            }
            StructKind::Item | StructKind::List | StructKind::Raw => {
                let omitempty_fields: Vec<String> = typedef
                    .fields
                    .iter()
                    .filter(|f| f.type_info.omitempty)
                    .map(|f| format!(":{}", elixir_field_name(&f.name)))
                    .collect();

                wln!(writer, "defmodule {mod_name} do");
                writer.indented(|writer| {
                    self.emit_rt_alias(writer);
                    if omitempty_fields.is_empty() {
                        writer.writeln("@derive JSON.Encoder");
                    }
                    let field_atoms: Vec<String> = typedef
                        .fields
                        .iter()
                        .map(|f| format!(":{}", elixir_field_name(&f.name)))
                        .collect();
                    wln!(writer, "defstruct [{}]", field_atoms.join(", "));
                    writer.blankline();

                    if typedef.fields.is_empty() {
                        writer.writeln("@type t :: %__MODULE__{}");
                    } else {
                        writer.writeln("@type t :: %__MODULE__{");
                        writer.indented(|writer| {
                            for (idx, field) in typedef.fields.iter().enumerate() {
                                let f_atom = elixir_field_name(&field.name);
                                let f_ty = self.resolve_type(&field.type_info, module);
                                let comma = if idx + 1 < typedef.fields.len() {
                                    ","
                                } else {
                                    ""
                                };
                                wln!(writer, "{f_atom}: {f_ty}{comma}");
                            }
                        });
                        writer.writeln("}");
                    }

                    if !omitempty_fields.is_empty() {
                        writer.blankline();
                        writer.writeln("defimpl JSON.Encoder do");
                        writer.indented(|writer| {
                            writer.writeln("def encode(value, encoder) do");
                            writer.indented(|writer| {
                                writer.writeln("value");
                                writer.writeln("|> Map.from_struct()");
                                wln!(
                                    writer,
                                    "|> Map.reject(fn {{k, v}} -> k in [{}] and is_nil(v) end)",
                                    omitempty_fields.join(", ")
                                );
                                writer.writeln("|> JSON.Encoder.Map.encode(encoder)");
                            });
                            writer.writeln("end");
                        });
                        writer.writeln("end");
                    }
                });
                writer.writeln("end");
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
        let prefix = format_elixir_module_prefix(&self.options.package);
        let name = elixir_pascal(&struct_decl.name);
        let mod_name = format!("{prefix}{name}");

        wln!(writer, "defmodule {mod_name} do");
        writer.indent();
        self.emit_rt_alias(writer);

        if let Some(ref doc) = struct_decl.doc {
            emit_elixir_doc(doc, "@moduledoc", writer);
        }

        if struct_decl.kind == StructKind::Rest {
            writer.writeln("defstruct []");
            writer.blankline();
            writer.writeln("def new, do: %__MODULE__{}");
            writer.blankline();
            self.emit_struct_matchers(struct_decl, module, writer);
            return;
        }

        let is_raw = struct_decl.kind == StructKind::Raw;
        let mut struct_fields = Vec::new();
        if is_raw {
            struct_fields.push("source: \"\"".to_owned());
        } else {
            struct_fields.push("nodes: []".to_owned());
        }
        for init_f in &struct_decl.init_fields {
            let f_name = elixir_field_name(&init_f.name);
            struct_fields.push(format!("{f_name}: nil"));
        }
        wln!(writer, "defstruct [{}]", struct_fields.join(", "));
        writer.blankline();
    }

    fn emit_init(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let is_raw = struct_decl.kind == StructKind::Raw;

        writer.writeln("def new(input) when is_binary(input) do");
        writer.indented(|writer| {
            if is_raw {
                writer.writeln("parser = %__MODULE__{source: input}");
                writer.writeln("init(parser)");
            } else {
                writer.writeln("from_nodes(Rt.parse_doc(input))");
            }
        });
        writer.writeln("end");
        writer.blankline();

        if !is_raw {
            writer.writeln("def from_nodes(nodes) when is_list(nodes) do");
            writer.indented(|writer| {
                writer.writeln("parser = %__MODULE__{nodes: nodes}");
                writer.writeln("init(parser)");
            });
            writer.writeln("end");
            writer.blankline();
        }

        if struct_decl.init_fields.is_empty() {
            writer.writeln("defp init(parser), do: parser");
        } else {
            writer.writeln("defp init(%__MODULE__{} = parser) do");
            writer.indented(|writer| {
                let input_expr = if is_raw {
                    "parser.source"
                } else {
                    "parser.nodes"
                };
                for init_f in &struct_decl.init_fields {
                    let f_name = elixir_field_name(&init_f.name);
                    wln!(writer,
                        "parser = %__MODULE__{{parser | {f_name}: init_{f_name}(parser, {input_expr})}}"
                    );
                }
                writer.writeln("parser");
            });
            writer.writeln("end");
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
        let is_raw = struct_decl.kind == StructKind::Raw;
        let f_name = elixir_field_name(&init_field.name);
        let method_name = format!("init_{f_name}");
        let context = format!("{}.{}", struct_decl.name, init_field.name);
        self.emit_pipeline_method(
            &method_name,
            false,
            is_raw,
            &init_field.pipeline,
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
        let init_val = if is_raw { "self.source" } else { "self.nodes" };
        let method_ident = elixir_ident(&check.name);
        let context = format!("{}.{}", struct_decl.name, check.name);

        wln!(
            writer,
            "def {method_ident}({{:ok, %__MODULE__{{}} = self}}), do: {method_ident}(self)"
        );
        wln!(
            writer,
            "def {method_ident}(doc) when is_binary(doc), do: doc |> new() |> {method_ident}()"
        );

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        wln!(writer, "def {method_ident}(%__MODULE__{{}} = self) do");
        writer.indented(|writer| {
            if Self::pipeline_uses_v(&check.pipeline) {
                wln!(writer, "v = {init_val}");
            }
            self.emit_pipeline_body(
                &check.pipeline,
                "v",
                &context,
                is_raw,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("end");
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
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "split_doc",
            false,
            is_raw,
            pipeline,
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
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "parse_key",
            false,
            is_raw,
            pipeline,
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
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "parse_value",
            false,
            is_raw,
            pipeline,
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
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "table_config",
            false,
            is_raw,
            pipeline,
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
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "table_rows",
            false,
            is_raw,
            pipeline,
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
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "table_match_key",
            false,
            is_raw,
            pipeline,
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
        let f_name = elixir_field_name(&field.name);
        let method_name = format!("parse_{f_name}");
        let context = format!("{}.{}", struct_decl.name, field.name);
        self.emit_pipeline_method(
            &method_name,
            false,
            is_raw,
            &field.pipeline,
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
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let prefix = format_elixir_module_prefix(&self.options.package);
        let name = elixir_pascal(&struct_decl.name);
        let type_mod = format!("{prefix}{name}Type");
        let is_raw = struct_decl.kind == StructKind::Raw;
        let has_raw_split = is_raw && struct_decl.split_doc.is_some();

        writer.writeln("def parse({:ok, %__MODULE__{} = self}), do: parse(self)");
        writer.writeln("def parse(doc) when is_binary(doc), do: doc |> new() |> parse()");

        let uses_self = struct_decl.pre_validate.is_some()
            || matches!(
                struct_decl.kind,
                StructKind::List | StructKind::Flat | StructKind::Dict | StructKind::Table
            )
            || has_raw_split
            || !struct_decl.fields.is_empty();
        let self_param = if uses_self { "self" } else { "_self" };

        wln!(writer, "def parse(%__MODULE__{{}} = {self_param}) do");
        writer.indented(|writer| {
            let root_val = if is_raw { "self.source" } else { "self.nodes" };
            if struct_decl.pre_validate.is_some() {
                wln!(writer, "pre_validate(self, {root_val})");
            }

            if struct_decl.kind == StructKind::List || has_raw_split {
                wln!(writer, "rows = split_doc(self, {root_val})");
                writer.writeln("Enum.map(rows, fn row ->");
                writer.indented(|writer| {
                    let row_arg = if has_raw_split { "row" } else { "[row]" };
                    if struct_decl.fields.is_empty() {
                        wln!(writer, "%{type_mod}{{}}");
                    } else {
                        wln!(writer, "%{type_mod}{{");
                        writer.indented(|writer| {
                            for (idx, field) in struct_decl.fields.iter().enumerate() {
                                let f_name = elixir_field_name(&field.name);
                                let comma = if idx + 1 < struct_decl.fields.len() {
                                    ","
                                } else {
                                    ""
                                };
                                wln!(writer, "{f_name}: parse_{f_name}(self, {row_arg}){comma}");
                            }
                        });
                        writer.writeln("}");
                    }
                });
                writer.writeln("end)");
            } else if struct_decl.kind == StructKind::Flat {
                writer.writeln("result = []");
                for field in &struct_decl.fields {
                    let f_name = elixir_field_name(&field.name);
                    if field.type_info.is_optional || field.type_info.omitempty {
                        wln!(writer, "val = parse_{f_name}(self, {root_val})");
                        if field.type_info.is_array {
                            writer.writeln(
                                "result = if is_nil(val), do: result, else: result ++ val",
                            );
                        } else {
                            writer.writeln(
                                "result = if is_nil(val), do: result, else: result ++ [val]",
                            );
                        }
                    } else if field.type_info.is_array {
                        wln!(
                            writer,
                            "result = result ++ parse_{f_name}(self, {root_val})"
                        );
                    } else {
                        wln!(
                            writer,
                            "result = result ++ [parse_{f_name}(self, {root_val})]"
                        );
                    }
                }
                writer.writeln("Rt.unique(result)");
            } else if struct_decl.kind == StructKind::Dict {
                wln!(writer, "rows = split_doc(self, {root_val})");
                writer.writeln("Map.new(rows, fn row ->");
                writer.indented(|writer| {
                    writer.writeln("row_nodes = [row]");
                    writer.writeln("{parse_key(self, row_nodes), parse_value(self, row_nodes)}");
                });
                writer.writeln("end)");
            } else if struct_decl.kind == StructKind::Table {
                wln!(writer, "table = table_config(self, {root_val})");
                writer.writeln("rows = table_rows(self, table)");
                writer.writeln("Enum.reduce(rows, %{}, fn row, acc ->");
                writer.indented(|writer| {
                    if struct_decl.fields.is_empty() {
                        writer.writeln("_ = row");
                        writer.writeln("acc");
                    } else {
                        writer.writeln("row_nodes = [row]");
                        for field in &struct_decl.fields {
                            let key_name = to_snake_case(&field.name);
                            let key_lit = elixir_str(&key_name);
                            let f_name = elixir_field_name(&field.name);
                            wln!(writer, "acc = if Map.has_key?(acc, {key_lit}) do");
                            writer.indented(|writer| {
                                writer.writeln("acc");
                            });
                            writer.writeln("else");
                            writer.indented(|writer| {
                                wln!(writer, "case parse_{f_name}(self, row_nodes) do");
                                writer.indented(|writer| {
                                    wln!(
                                        writer,
                                        "{{:matched, value}} -> Map.put(acc, {key_lit}, value)"
                                    );
                                    writer.writeln(":unmatched -> acc");
                                });
                                writer.writeln("end");
                            });
                            writer.writeln("end");
                        }
                        writer.writeln("acc");
                    }
                });
                writer.writeln("end)");
            } else if struct_decl.fields.is_empty() {
                wln!(writer, "%{type_mod}{{}}");
            } else {
                wln!(writer, "%{type_mod}{{");
                writer.indented(|writer| {
                    for (idx, field) in struct_decl.fields.iter().enumerate() {
                        let f_name = elixir_field_name(&field.name);
                        let comma = if idx + 1 < struct_decl.fields.len() {
                            ","
                        } else {
                            ""
                        };
                        wln!(writer, "{f_name}: parse_{f_name}(self, {root_val}){comma}");
                    }
                });
                writer.writeln("}");
            }
        });
        writer.writeln("end");
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
        writer.writeln("end");
        writer.blankline();
    }

    fn emit_function(
        &mut self,
        func: &FunctionDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if !self.in_functions_module {
            let prefix = format_elixir_module_prefix(&self.options.package);
            wln!(writer, "defmodule {prefix}Functions do");
            writer.indent();
            self.emit_rt_alias(writer);
            self.in_functions_module = true;
        }

        let name = elixir_ident(&func.name);
        if let Some(ref doc) = func.doc {
            emit_elixir_doc(doc, "@doc", writer);
        }

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        wln!(writer, "def {name}(document) when is_binary(document) do");
        writer.indented(|writer| {
            if func.is_raw {
                writer.writeln("v = document");
            } else {
                writer.writeln("v = Rt.parse_doc(document)");
            }
            self.emit_pipeline_body(
                &func.pipeline,
                "v",
                &func.name,
                func.is_raw,
                false,
                module,
                builder,
                writer,
            );
        });
        writer.writeln("end");
        writer.blankline();
    }

    fn on_module_end(
        &mut self,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if self.in_functions_module {
            writer.dedent();
            writer.writeln("end");
            writer.blankline();
            self.in_functions_module = false;
        }
    }
}

/// Emits Elixir source files for a compiled [`Module`] and [`CodegenOptions`].
///
/// Returns `(parser_source, optional_runtime_source, builder)` or `Err(diagnostics)`.
pub fn emit_elixir_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, Option<String>, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = ElixirEmitter::new(options.clone());
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
        Some(emit_elixir_runtime_module(module, &builder, options))
    } else {
        None
    };
    Ok((parser_code, runtime_code, builder))
}

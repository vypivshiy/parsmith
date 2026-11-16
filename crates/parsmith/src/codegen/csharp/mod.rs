//! C# / .NET 8+ (`AngleSharp` + conditional `AngleSharp.XPath` + `System.Text.Json` + `HttpClient`)
//! target backend (`CSharpEmitter`).
//!
//! Emits 4-space indented C# 12 (.NET 8+) source code directly from [`CodeWriter`],
//! defaulting to a single self-contained `<stem>.cs` file with an inline `public static partial class Rt`
//! (omitting `AngleSharp` when `!module_has_html(module)` and omitting `AngleSharp.XPath` when
//! `!module_has_xpath(module)`), while supporting `-R / --separate-runtime` (`<stem>.cs` + `<runtime_name>.cs`)
//! and optional `--package <Namespace>`.

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
    CSHARP_CANNOT_BE_RAW, CSHARP_RESERVED, is_valid_symbol, to_camel_case, to_snake_case,
};
use crate::lowering::clean_doc;
use crate::validators::normalize_regex_pattern;

pub use rest::{csharp_ph_type, csharp_variant_name, render_json_body, render_template};
pub use runtime::{
    CSHARP_CORE_RUNTIME, CSHARP_HTML_RUNTIME, CSHARP_HTTP_RUNTIME, CSHARP_XPATH_RUNTIME,
    collect_csharp_usings, cond_has_xpath, emit_csharp_runtime_module, module_has_xpath,
    normalize_csharp_using, pipeline_has_xpath, write_csharp_runtime_body,
};

/// Validates that `pkg` is either empty / `"main"` (global namespace) or dot-separated valid C# identifiers.
pub fn validate_csharp_namespace(pkg: &str) -> bool {
    let trimmed = pkg.trim();
    if trimmed.is_empty() || trimmed == "main" {
        return true;
    }
    trimmed.split('.').all(|seg| {
        !seg.is_empty() && is_valid_symbol("csharp", seg) && !CSHARP_RESERVED.contains(&seg)
    })
}

fn csharp_regex_str(pattern: &str) -> String {
    csharp_str(&normalize_regex_pattern(pattern))
}

/// Formats a `&str` as a valid double-quoted C# string literal (`"..."`).
pub fn csharp_str(s: &str) -> String {
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
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Renders a DSL identifier as a valid `camelCase` C# identifier, escaping reserved keywords with `@`.
pub fn csharp_ident(value: &str) -> String {
    let snake = to_snake_case(value);
    let name = to_camel_case(&snake).replace('-', "_");
    let name = if name.is_empty() {
        "value".to_owned()
    } else {
        name
    };
    if CSHARP_RESERVED.contains(&name.as_str()) && !CSHARP_CANNOT_BE_RAW.contains(&name.as_str()) {
        format!("@{name}")
    } else {
        name
    }
}

/// Renders a DSL identifier as a `PascalCase` C# identifier, escaping reserved keywords with `@`.
pub fn csharp_pascal(value: &str) -> String {
    let name = to_pascal_case(value).replace('-', "");
    let name = if name.is_empty() {
        "Value".to_owned()
    } else {
        name
    };
    if CSHARP_RESERVED.contains(&name.as_str()) && !CSHARP_CANNOT_BE_RAW.contains(&name.as_str()) {
        format!("@{name}")
    } else {
        name
    }
}

/// Maps a dictionary key [`TypeInfo`] to its C# type representation.
pub fn csharp_key_type(info: Option<&TypeInfo>) -> &'static str {
    let Some(info) = info else {
        return "string";
    };
    match info.base {
        BaseType::String => "string",
        BaseType::Int => "long",
        BaseType::Float => "double",
        BaseType::Bool => "bool",
        _ => "string",
    }
}

/// Formats a [`LiteralValue`] as a typed C# literal expression for `typ`.
pub fn csharp_literal(val: &LiteralValue, typ: &str) -> String {
    match val {
        LiteralValue::Null => {
            if typ.ends_with('?') {
                "null".to_owned()
            } else {
                "default!".to_owned()
            }
        }
        LiteralValue::Bool(b) => b.to_string(),
        LiteralValue::Int(n) => format!("{n}L"),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => csharp_str(s),
        LiteralValue::EmptyArray => {
            let clean = typ.trim_end_matches('?');
            if clean.starts_with("List<") {
                format!("new {clean}()")
            } else {
                "new List<string>()".to_owned()
            }
        }
    }
}

/// Translates DSL/Python numeric replacement groups (`\1` or `$1`) into .NET `Regex.Replace` syntax,
/// while escaping literal dollar signs as `$$`.
pub fn csharp_replacement(replacement: &str) -> String {
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

/// Writes cleaned C# documentation lines (`/// ...` or `// ...`) to `writer`.
pub fn emit_csharp_doc(doc: &str, prefix: &str, writer: &mut CodeWriter) {
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

/// C# (.NET 8+) target `LanguageEmitter` implementation.
pub struct CSharpEmitter {
    /// Code generation options.
    pub options: CodegenOptions,
    /// Diagnostics accumulated during code generation (`E400`).
    pub diagnostics: Vec<Diagnostic>,
    /// Tracks already-emitted REST `Result` records within the current pass.
    pub(crate) emitted_aliases: IndexSet<String>,
}

impl CSharpEmitter {
    /// Creates a new `CSharpEmitter` for `options`.
    pub fn new(options: CodegenOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            emitted_aliases: IndexSet::new(),
        }
    }

    /// Resolves a [`TypeInfo`] into its C# type annotation.
    pub fn resolve_type(info: &TypeInfo, module: &Module) -> String {
        let optional = info.is_optional || info.omitempty;
        if info.is_dict {
            let key_type = csharp_key_type(info.key_type.as_deref());
            let val_type = info
                .value_type
                .as_deref()
                .map(|vt| Self::resolve_type(vt, module))
                .unwrap_or_else(|| "string".to_owned());
            let mut res = format!("Dictionary<{key_type}, {val_type}>");
            if info.is_array {
                res = format!("List<{res}>");
            }
            if optional {
                res.push('?');
            }
            return res;
        }

        let mut res = match &info.base {
            BaseType::Document => {
                if info.is_array {
                    "List<IElement>".to_owned()
                } else {
                    "IElement".to_owned()
                }
            }
            BaseType::Nested(ref_name) => Self::resolve_struct_result_type(ref_name, module),
            BaseType::Json(ref_name) => {
                let target_def = module.json_def(ref_name);
                if let Some(def) = target_def
                    && def.is_dict
                {
                    let key_type = csharp_key_type(def.key_type.as_ref());
                    let val_type = def
                        .value_type
                        .as_ref()
                        .map(|vt| Self::resolve_type(vt, module))
                        .unwrap_or_else(|| "string".to_owned());
                    let dict_ty = format!("Dictionary<{key_type}, {val_type}>");
                    if info.is_array {
                        format!("List<{dict_ty}>")
                    } else {
                        dict_ty
                    }
                } else {
                    let clean_pascal = to_pascal_case(ref_name).replace('-', "");
                    let def_is_arr = target_def.is_some_and(|j| j.is_array);
                    if info.is_array || def_is_arr {
                        format!("List<{clean_pascal}Json>")
                    } else {
                        format!("{clean_pascal}Json")
                    }
                }
            }
            BaseType::String | BaseType::Auto | BaseType::DictValue => "string".to_owned(),
            BaseType::Int => "long".to_owned(),
            BaseType::Float => "double".to_owned(),
            BaseType::Bool => "bool".to_owned(),
            BaseType::Null => "object?".to_owned(),
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
            res.push('?');
        }
        res
    }

    /// Resolves the return type of a struct's `.Parse()` method.
    pub fn resolve_struct_result_type(name: &str, module: &Module) -> String {
        let clean_pascal = to_pascal_case(name).replace('-', "");
        let base = format!("{clean_pascal}Type");
        let Some(struct_decl) = module.struct_decl(name) else {
            return base;
        };
        match struct_decl.kind {
            StructKind::List => format!("List<{base}>"),
            StructKind::Flat => "List<string>".to_owned(),
            StructKind::Dict => {
                let key_ty = struct_decl
                    .key_pipeline
                    .as_ref()
                    .map(|p| csharp_key_type(Some(&p.output)))
                    .unwrap_or("string");
                let val_ty = struct_decl
                    .value_pipeline
                    .as_ref()
                    .map(|p| Self::resolve_type(&p.output, module))
                    .unwrap_or_else(|| "string".to_owned());
                format!("Dictionary<{key_ty}, {val_ty}>")
            }
            StructKind::Raw if struct_decl.split_doc.is_some() => format!("List<{base}>"),
            StructKind::Rest => "object?".to_owned(),
            _ => base,
        }
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
        let arg_ty = if is_raw { "string" } else { "IElement" };
        let vis = if is_pub { "public" } else { "private" };
        let actual_ret = if is_pre_validate { "void" } else { ret_type };
        writer.reset_vars();
        let _ = writer.next_var(); // reserve v0 so steps start at v1
        wln!(writer, "{vis} {actual_ret} {method_name}({arg_ty} v)");
        writer.writeln("{");
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline,
                "v",
                ret_type,
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
        ret_type: &str,
        context: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref fb_val) = pipeline.fallback {
            let fallback_lit = csharp_literal(fb_val, ret_type);
            wln!(writer, "return Rt.Fallback<{ret_type}>(() =>");
            writer.writeln("{");
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
                wln!(writer, "return {last};");
            });
            wln!(writer, "}}, {fallback_lit});");
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
                wln!(writer, "return {last};");
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
        let ctx_lit = csharp_str(context);

        match &step.op {
            StepOp::Value { value } => {
                let ret_type = Self::resolve_type(&step.output, module);
                let lit = csharp_literal(value, &ret_type);
                wln!(writer, "var {nxt} = {lit};");
            }
            StepOp::LoadInit { name } => {
                let snake = to_snake_case(name);
                wln!(writer, "var {nxt} = this._init_{snake};");
            }
            StepOp::Css { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| csharp_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                if next_is_to_bool {
                    wln!(
                        writer,
                        "var {nxt} = Rt.CssSelectOpt({prv}, new[] {{ {q_list} }});"
                    );
                } else {
                    wln!(
                        writer,
                        "var {nxt} = Rt.CssSelect({prv}, new[] {{ {q_list} }}, {ctx_lit});"
                    );
                }
            }
            StepOp::CssAll { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| csharp_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(
                    writer,
                    "var {nxt} = Rt.CssSelectAll({prv}, new[] {{ {q_list} }});"
                );
            }
            StepOp::CssRemove { query } => {
                let q_lit = csharp_str(query);
                wln!(writer, "var {nxt} = Rt.CssRemove({prv}, {q_lit});");
            }
            StepOp::Xpath { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| csharp_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                if next_is_to_bool {
                    wln!(
                        writer,
                        "var {nxt} = Rt.XpathSelectOpt({prv}, new[] {{ {q_list} }});"
                    );
                } else {
                    wln!(
                        writer,
                        "var {nxt} = Rt.XpathSelect({prv}, new[] {{ {q_list} }}, {ctx_lit});"
                    );
                }
            }
            StepOp::XpathAll { queries } => {
                let q_list = queries
                    .iter()
                    .map(|q| csharp_str(q))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(
                    writer,
                    "var {nxt} = Rt.XpathSelectAll({prv}, new[] {{ {q_list} }});"
                );
            }
            StepOp::XpathRemove { query } => {
                let q_lit = csharp_str(query);
                wln!(writer, "var {nxt} = Rt.XpathRemove({prv}, {q_lit});");
            }
            StepOp::Text => {
                if step.output.is_array || step.input.is_array {
                    wln!(writer, "var {nxt} = Rt.TextAll({prv});");
                } else {
                    wln!(writer, "var {nxt} = Rt.Text({prv}, {ctx_lit});");
                }
            }
            StepOp::Raw { mode } => {
                let inner = if *mode == RawMode::Inner {
                    "true"
                } else {
                    "false"
                };
                if step.output.is_array || step.input.is_array {
                    wln!(writer, "var {nxt} = Rt.RawHtmlAll({prv}, {inner});");
                } else {
                    wln!(writer, "var {nxt} = Rt.RawHtml({prv}, {inner}, {ctx_lit});");
                }
            }
            StepOp::Attr { keys } => {
                let names = keys
                    .iter()
                    .map(|k| csharp_str(k))
                    .collect::<Vec<_>>()
                    .join(", ");
                if keys.len() > 1 {
                    wln!(
                        writer,
                        "var {nxt} = Rt.AttrMulti({prv}, new[] {{ {names} }});"
                    );
                } else if step.input.is_array {
                    wln!(
                        writer,
                        "var {nxt} = Rt.AttrList({prv}, new[] {{ {names} }}, {ctx_lit});"
                    );
                } else {
                    wln!(
                        writer,
                        "var {nxt} = Rt.Attr({prv}, new[] {{ {names} }}, {ctx_lit});"
                    );
                }
            }
            StepOp::Trim { chars } => {
                let arg = if chars.is_empty() {
                    "null".to_owned()
                } else {
                    csharp_str(chars)
                };
                wln!(writer, "var {nxt} = Rt.Trim({prv}, {arg});");
            }
            StepOp::Ltrim { chars } => {
                let arg = if chars.is_empty() {
                    "null".to_owned()
                } else {
                    csharp_str(chars)
                };
                wln!(writer, "var {nxt} = Rt.Ltrim({prv}, {arg});");
            }
            StepOp::Rtrim { chars } => {
                let arg = if chars.is_empty() {
                    "null".to_owned()
                } else {
                    csharp_str(chars)
                };
                wln!(writer, "var {nxt} = Rt.Rtrim({prv}, {arg});");
            }
            StepOp::NormalizeSpace => {
                wln!(writer, "var {nxt} = Rt.NormalizeSpace({prv});");
            }
            StepOp::RmPrefix { substr } => {
                wln!(
                    writer,
                    "var {nxt} = Rt.RmPrefix({prv}, {});",
                    csharp_str(substr)
                );
            }
            StepOp::RmSuffix { substr } => {
                wln!(
                    writer,
                    "var {nxt} = Rt.RmSuffix({prv}, {});",
                    csharp_str(substr)
                );
            }
            StepOp::RmPrefixSuffix { substr } => {
                wln!(
                    writer,
                    "var {nxt} = Rt.RmPrefixSuffix({prv}, {});",
                    csharp_str(substr)
                );
            }
            StepOp::Fmt { template } => {
                let tmpl_lit = csharp_str(template);
                wln!(writer, "var {nxt} = Rt.FmtValue({tmpl_lit}, {prv});");
            }
            StepOp::Repl { old, new } => {
                wln!(
                    writer,
                    "var {nxt} = Rt.Repl({prv}, {}, {});",
                    csharp_str(old),
                    csharp_str(new)
                );
            }
            StepOp::ReplMap { replacements } => {
                let pairs = replacements
                    .iter()
                    .map(|(k, v)| format!("({}, {})", csharp_str(k), csharp_str(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                wln!(
                    writer,
                    "var {nxt} = Rt.ReplMap({prv}, new (string, string)[] {{ {pairs} }});"
                );
            }
            StepOp::Lower => {
                wln!(writer, "var {nxt} = Rt.Lower({prv});");
            }
            StepOp::Upper => {
                wln!(writer, "var {nxt} = Rt.Upper({prv});");
            }
            StepOp::Split { sep } => {
                wln!(writer, "var {nxt} = Rt.Split({prv}, {});", csharp_str(sep));
            }
            StepOp::Join { sep } => {
                wln!(writer, "var {nxt} = Rt.Join({prv}, {});", csharp_str(sep));
            }
            StepOp::Unescape => {
                wln!(writer, "var {nxt} = Rt.Unescape({prv});");
            }
            StepOp::Re { pattern } => {
                let pat_lit = csharp_regex_str(pattern);
                wln!(
                    writer,
                    "var {nxt} = Rt.RegexSearch({prv}, {pat_lit}, {ctx_lit});"
                );
            }
            StepOp::ReAll { pattern } => {
                let pat_lit = csharp_regex_str(pattern);
                wln!(
                    writer,
                    "var {nxt} = Rt.RegexAll({prv}, {pat_lit}, {ctx_lit});"
                );
            }
            StepOp::ReSub { pattern, repl } => {
                let pat_lit = csharp_regex_str(pattern);
                let repl_lit = csharp_str(&csharp_replacement(repl));
                wln!(
                    writer,
                    "var {nxt} = Rt.RegexSub({prv}, {pat_lit}, {repl_lit}, {ctx_lit});"
                );
            }
            StepOp::ReSplit { pattern } => {
                let pat_lit = csharp_regex_str(pattern);
                wln!(
                    writer,
                    "var {nxt} = Rt.RegexSplit({prv}, {pat_lit}, {ctx_lit});"
                );
            }
            StepOp::Index { index } => {
                if step.input.is_array {
                    wln!(writer, "var {nxt} = Rt.ItemAt({prv}, {index}L, {ctx_lit});");
                } else if step.input.base == BaseType::Document {
                    if *index == 0 || *index == -1 {
                        wln!(writer, "var {nxt} = {prv};");
                    } else {
                        wln!(
                            writer,
                            "var {nxt} = Rt.ItemAt(new List<IElement> {{ {prv} }}, {index}L, {ctx_lit});"
                        );
                    }
                } else {
                    wln!(
                        writer,
                        "var {nxt} = Rt.StrIndex({prv}, {index}L, {ctx_lit});"
                    );
                }
            }
            StepOp::Slice { start, end } => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "var {nxt} = Rt.StrSlice({prv}, {start}L, {end}L);");
                } else {
                    wln!(writer, "var {nxt} = Rt.SliceList({prv}, {start}L, {end}L);");
                }
            }
            StepOp::Len => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "var {nxt} = Rt.StrLen({prv});");
                } else if step.input.is_array {
                    wln!(writer, "var {nxt} = Rt.Len({prv});");
                } else {
                    wln!(writer, "var {nxt} = ({prv} is null ? 0L : 1L);");
                }
            }
            StepOp::Unique { .. } => {
                wln!(writer, "var {nxt} = Rt.Unique({prv});");
            }
            StepOp::ToInt => {
                wln!(writer, "var {nxt} = Rt.ToInt({prv}, {ctx_lit});");
            }
            StepOp::ToFloat => {
                wln!(writer, "var {nxt} = Rt.ToFloat({prv}, {ctx_lit});");
            }
            StepOp::ToBool => {
                let expr = if step.input.base == BaseType::Document {
                    format!("Rt.NodeBool({prv})")
                } else if step.input.is_array {
                    format!("Rt.SeqBool({prv})")
                } else if step.input.base == BaseType::Int {
                    format!("Rt.NumBool({prv})")
                } else if step.input.base == BaseType::Float {
                    format!("Rt.FloatBool({prv})")
                } else if step.input.base == BaseType::Bool {
                    prv.to_owned()
                } else {
                    format!("Rt.StringBool({prv})")
                };
                wln!(writer, "var {nxt} = {expr};");
            }
            StepOp::Jsonify {
                schema_name, path, ..
            } => {
                let clean_pascal = to_pascal_case(schema_name).replace('-', "");
                let effective_path = path
                    .as_deref()
                    .or_else(|| module.json_def(schema_name).and_then(|j| j.path.as_deref()))
                    .unwrap_or("");
                let path_lit = csharp_str(effective_path);
                let target_def = module.json_def(schema_name);
                let is_dict_def = target_def.is_some_and(|d| d.is_dict);
                let def_is_arr = target_def.is_some_and(|d| d.is_array);
                let parsed_expr = format!("Rt.ParseJson({prv}, {path_lit}, {ctx_lit})");

                if is_dict_def {
                    if step.output.is_array {
                        wln!(
                            writer,
                            "var {nxt} = Rt.DecodeList({parsed_expr}, {ctx_lit}, _item => {clean_pascal}Json.FromNode(_item, {ctx_lit}));"
                        );
                    } else {
                        wln!(
                            writer,
                            "var {nxt} = {clean_pascal}Json.FromNode({parsed_expr});"
                        );
                    }
                } else if step.output.is_array || def_is_arr {
                    wln!(
                        writer,
                        "var {nxt} = Rt.DecodeList({parsed_expr}, {ctx_lit}, _item => {clean_pascal}Json.FromNode(_item, {ctx_lit}));"
                    );
                } else {
                    wln!(
                        writer,
                        "var {nxt} = {clean_pascal}Json.FromNode({parsed_expr});"
                    );
                }
            }
            StepOp::Nested { struct_name } => {
                let cls = csharp_pascal(struct_name);
                wln!(writer, "var {nxt} = new {cls}({prv}).Parse();");
            }
            StepOp::Filter { cond } => {
                let elem_type = TypeInfo {
                    is_array: false,
                    ..step.input.clone()
                };
                let cond_str = Self::render_cond(cond, "_item", &elem_type);
                wln!(
                    writer,
                    "var {nxt} = {prv}.Where(_item => {cond_str}).ToList();"
                );
            }
            StepOp::Assert { message, cond } => {
                let cond_str = Self::render_cond(cond, prv, &step.input);
                let msg = message.as_deref().unwrap_or("assertion failed");
                wln!(
                    writer,
                    "Rt.AssertTrue({cond_str}, {ctx_lit}, {});",
                    csharp_str(msg)
                );
                if !(is_pre_validate && is_last_step) {
                    wln!(writer, "var {nxt} = {prv};");
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
                    &["cs", "csharp"],
                    "'cs' ('csharp')",
                    prv,
                    nxt,
                    &in_type,
                    &out_type,
                    |imp| normalize_csharp_using(&imp.value),
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
            format!("Rt.PredText({target})")
        } else {
            target.to_owned()
        };
        let count_expr = if input_type.base == BaseType::String && !input_type.is_array {
            format!("Rt.StrLen({target})")
        } else if input_type.is_array {
            format!("Rt.Len({target})")
        } else {
            format!("({target} is null ? 0L : 1L)")
        };

        match op {
            CondOp::Css { query } => {
                format!("Rt.PredCss({target}, {})", csharp_str(query))
            }
            CondOp::Xpath { query } => {
                format!("Rt.PredXpath({target}, {})", csharp_str(query))
            }
            CondOp::HasAttr { attrs } => {
                let names = attrs
                    .iter()
                    .map(|a| csharp_str(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Rt.PredHasAttr({target}, new[] {{ {names} }})")
            }
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                let attr = format!("Rt.PredAttr({target}, {})", csharp_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} == {}", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrNe { name, values } => {
                let attr = format!("Rt.PredAttr({target}, {})", csharp_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} != {}", csharp_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::AttrStarts { name, values } => {
                let attr = format!("Rt.PredAttr({target}, {})", csharp_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.Starts({attr}, {})", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEnds { name, values } => {
                let attr = format!("Rt.PredAttr({target}, {})", csharp_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.Ends({attr}, {})", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrContains { name, values } => {
                let attr = format!("Rt.PredAttr({target}, {})", csharp_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.Contains({attr}, {})", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrRe { name, pattern } => {
                let attr = format!("Rt.PredAttr({target}, {})", csharp_str(name));
                format!("Rt.RegexMatch({attr}, {})", csharp_regex_str(pattern))
            }
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} == {}", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} != {}", csharp_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::TextStarts { values } | CondOp::Starts { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.Starts({text_val}, {})", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextEnds { values } | CondOp::Ends { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.Ends({text_val}, {})", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextContains { values } | CondOp::Contains { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("Rt.Contains({text_val}, {})", csharp_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextRe { pattern } | CondOp::Re { pattern } => {
                format!("Rt.RegexMatch({text_val}, {})", csharp_regex_str(pattern))
            }
            CondOp::ReAny { pattern } => {
                format!("Rt.ReAny({target}, {})", csharp_regex_str(pattern))
            }
            CondOp::ReAll { pattern } => {
                format!("Rt.ReAllMatch({target}, {})", csharp_regex_str(pattern))
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
                format!("({start}L < {count_expr} && {count_expr} < {end}L)")
            }
        }
    }

    fn format_cond_val(val: &CondValue, input_type: &TypeInfo) -> String {
        match val {
            CondValue::Int(n) => {
                if input_type.base == BaseType::Float && !input_type.is_array {
                    format!("{n}.0")
                } else {
                    format!("{n}L")
                }
            }
            CondValue::Float(f) => format!("{f:?}"),
            CondValue::Bool(b) => b.to_string(),
            CondValue::String(s) => csharp_str(s),
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
            format!("{target} {op} {value}.0")
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
        let snake = to_snake_case(&field.name);
        let field_type = Self::resolve_type(&field.type_info, module);
        let context = format!("{}.{}", struct_decl.name, field.name);

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        let _ = writer.next_var(); // v1 (used for this.__ParseValue(v))

        wln!(
            writer,
            "private bool __Parse_{snake}(IElement v, out {field_type} result)"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            let steps = &field.pipeline.steps;
            let (match_cond, remaining_steps) = match steps.first().map(|s| &s.op) {
                Some(StepOp::Match { cond }) => (Some(cond), &steps[1..]),
                _ => (None, &steps[..]),
            };

            if let Some(cond) = match_cond {
                let key_type = TypeInfo::string();
                let cond_str = Self::render_cond(cond, "_key", &key_type);
                writer.writeln("var _key = this.__TableMatchKey(v);");
                wln!(writer, "if (!({cond_str}))");
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("result = default!;");
                    writer.writeln("return false;");
                });
                writer.writeln("}");
            }

            writer.writeln("var v1 = this.__ParseValue(v);");

            if let Some(ref fb_val) = field.pipeline.fallback {
                let fallback_lit = csharp_literal(fb_val, &field_type);
                wln!(writer, "result = Rt.Fallback<{field_type}>(() =>");
                writer.writeln("{");
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
                    wln!(writer, "return {last};");
                });
                wln!(writer, "}}, {fallback_lit});");
                writer.writeln("return true;");
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
                wln!(writer, "result = {last};");
                writer.writeln("return true;");
            }
        });
        writer.writeln("}");
        writer.blankline();
    }
}

impl LanguageEmitter for CSharpEmitter {
    fn indent_unit(&self) -> &'static str {
        "    "
    }

    fn on_module_start(
        &mut self,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        self.emitted_aliases.clear();
        writer.writeln("// Auto-generated by parsmith. DO NOT EDIT.");
        writer.writeln("#nullable enable");
        writer.writeln(
            "#pragma warning disable CS8600, CS8601, CS8602, CS8603, CS8604, CS8618, CS8625, CS0162, CS0219",
        );
        writer.blankline();
    }

    fn emit_imports(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let usings = collect_csharp_usings(module, builder);
        for u in &usings {
            writer.writeln(u);
        }
        let pkg = self.options.package.trim();
        let has_ns = !pkg.is_empty() && pkg != "main";
        if has_ns {
            wln!(writer, "using static {pkg}.Rt;");
        } else {
            writer.writeln("using static Rt;");
        }
        writer.blankline();

        if has_ns {
            wln!(writer, "namespace {pkg};");
            writer.blankline();
        }

        if let Some(ref doc) = module.doc {
            emit_csharp_doc(doc, "//", writer);
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
            writer.writeln("public static partial class Rt");
            writer.writeln("{");
            writer.indented(|writer| {
                write_csharp_runtime_body(module, builder, writer);
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
        let clean_pascal = to_pascal_case(&typedef.name).replace('-', "");
        let name = format!("{clean_pascal}Type");
        match typedef.struct_kind {
            StructKind::Rest | StructKind::Flat | StructKind::Dict => {}
            StructKind::Item | StructKind::List | StructKind::Table | StructKind::Raw => {
                if typedef.fields.is_empty() {
                    wln!(writer, "public sealed record {name}();");
                    writer.blankline();
                } else {
                    wln!(writer, "public sealed record {name}(");
                    writer.indented(|writer| {
                        for (idx, field) in typedef.fields.iter().enumerate() {
                            let typ = Self::resolve_type(&field.type_info, module);
                            let wire_prop = to_snake_case(&field.name);
                            let wire_lit = csharp_str(&wire_prop);
                            let prop_ident = csharp_pascal(&field.name);
                            let comma = if idx + 1 < typedef.fields.len() {
                                ","
                            } else {
                                ");"
                            };
                            if field.type_info.omitempty {
                                wln!(writer,
                                    "[property: JsonPropertyName({wire_lit}), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] {typ} {prop_ident}{comma}"
                                );
                            } else {
                                wln!(writer,
                                    "[property: JsonPropertyName({wire_lit})] {typ} {prop_ident}{comma}"
                                );
                            }
                        }
                    });
                    writer.blankline();
                }
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
        let name = csharp_pascal(&struct_decl.name);
        if let Some(ref doc) = struct_decl.doc {
            emit_csharp_doc(doc, "///", writer);
        }

        if struct_decl.kind == StructKind::Rest {
            wln!(writer, "public sealed class {name}");
            writer.writeln("{");
            writer.indent();
            wln!(writer, "public {name}() {{ }}");
            wln!(writer, "public static {name} New() => new {name}();");
            writer.blankline();
            return;
        }

        let is_raw = struct_decl.kind == StructKind::Raw;
        wln!(writer, "public sealed class {name}");
        writer.writeln("{");
        writer.indent();
        if is_raw {
            writer.writeln("private readonly string _doc;");
        } else {
            writer.writeln("private readonly IElement _doc;");
        }
        for init_f in &struct_decl.init_fields {
            let snake = to_snake_case(&init_f.name);
            let typ = Self::resolve_type(&init_f.type_info, module);
            wln!(writer, "private {typ} _init_{snake} = default!;");
        }
        writer.blankline();
    }

    fn emit_init(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = csharp_pascal(&struct_decl.name);
        let is_raw = struct_decl.kind == StructKind::Raw;

        if is_raw {
            wln!(writer, "public {name}(string document)");
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("this._doc = document;");
                writer.writeln("this.__Init();");
            });
            writer.writeln("}");
            writer.blankline();
            wln!(
                writer,
                "public static {name} New(string document) => new {name}(document);"
            );
            writer.blankline();
        } else {
            wln!(writer, "public {name}(string document)");
            writer.indented(|writer| {
                writer.writeln(": this(Rt.ParseHtml(document))");
            });
            writer.writeln("{");
            writer.writeln("}");
            writer.blankline();

            wln!(writer, "public {name}(IElement document)");
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("this._doc = document;");
                writer.writeln("this.__Init();");
            });
            writer.writeln("}");
            writer.blankline();

            wln!(
                writer,
                "public static {name} New(string document) => new {name}(document);"
            );
            wln!(
                writer,
                "public static {name} New(IElement document) => new {name}(document);"
            );
            writer.blankline();
        }

        writer.writeln("private void __Init()");
        writer.writeln("{");
        writer.indented(|writer| {
            for init_f in &struct_decl.init_fields {
                let snake = to_snake_case(&init_f.name);
                wln!(
                    writer,
                    "this._init_{snake} = this.__Init_{snake}(this._doc);"
                );
            }
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
        let snake = to_snake_case(&init_field.name);
        let method_name = format!("__Init_{snake}");
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
            "__PreValidate",
            false,
            is_raw,
            pipeline,
            "void",
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
        let method_ident = csharp_pascal(&check.name);
        let context = format!("{}.{}", struct_decl.name, check.name);

        writer.reset_vars();
        let _ = writer.next_var(); // v0
        wln!(writer, "public bool {method_ident}()");
        writer.writeln("{");
        writer.indented(|writer| {
            writer.writeln("var v = this._doc;");
            self.emit_pipeline_body(
                &check.pipeline,
                "v",
                "bool",
                &context,
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
        let ret_type = if is_raw {
            "List<string>"
        } else {
            "List<IElement>"
        };
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "__SplitDoc",
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
            "__ParseKey",
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
            "__ParseValue",
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
        let is_raw = struct_decl.kind == StructKind::Raw;
        let ret_type = Self::resolve_type(&pipeline.output, module);
        let context = struct_decl.name.clone();
        self.emit_pipeline_method(
            "__TableConfig",
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
            "__TableRows",
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
            "__TableMatchKey",
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
        let snake = to_snake_case(&field.name);
        let method_ident = format!("__Parse_{snake}");
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
        let clean_pascal = to_pascal_case(&struct_decl.name).replace('-', "");
        let type_name = format!("{clean_pascal}Type");
        let result_type = Self::resolve_struct_result_type(&struct_decl.name, module);
        let is_raw = struct_decl.kind == StructKind::Raw;
        let has_raw_split = is_raw && struct_decl.split_doc.is_some();

        wln!(writer, "public {result_type} Parse()");
        writer.writeln("{");
        writer.indented(|writer| {
            if struct_decl.pre_validate.is_some() {
                writer.writeln("this.__PreValidate(this._doc);");
            }

            if struct_decl.kind == StructKind::List || has_raw_split {
                writer.writeln("var _rows = this.__SplitDoc(this._doc);");
                wln!(writer,
                    "var _result = new List<{type_name}>(_rows.Count);"
                );
                writer.writeln("foreach (var _row in _rows)");
                writer.writeln("{");
                writer.indented(|writer| {
                    if struct_decl.fields.is_empty() {
                        wln!(writer, "_result.Add(new {type_name}());");
                    } else {
                        let args: Vec<String> = struct_decl
                            .fields
                            .iter()
                            .map(|f| format!("this.__Parse_{}(_row)", to_snake_case(&f.name)))
                            .collect();
                        wln!(writer,
                            "_result.Add(new {type_name}({}));",
                            args.join(", ")
                        );
                    }
                });
                writer.writeln("}");
                writer.writeln("return _result;");
            } else if struct_decl.kind == StructKind::Flat {
                writer.writeln("var _result = new List<string>();");
                for field in &struct_decl.fields {
                    let snake = to_snake_case(&field.name);
                    let ftype = Self::resolve_type(&field.type_info, module);
                    if field.type_info.is_optional || field.type_info.omitempty {
                        wln!(writer,
                            "var _{snake} = this.__Parse_{snake}(this._doc);"
                        );
                        wln!(writer, "if (_{snake} is not null)");
                        writer.writeln("{");
                        writer.indented(|writer| {
                            let inner = ftype.trim_end_matches('?');
                            if inner.starts_with("List<") {
                                wln!(writer, "_result.AddRange(_{snake});");
                            } else {
                                wln!(writer, "_result.Add(_{snake});");
                            }
                        });
                        writer.writeln("}");
                    } else if ftype.starts_with("List<") {
                        wln!(writer,
                            "_result.AddRange(this.__Parse_{snake}(this._doc));"
                        );
                    } else {
                        wln!(writer,
                            "_result.Add(this.__Parse_{snake}(this._doc));"
                        );
                    }
                }
                writer.writeln("return Rt.Unique(_result);");
            } else if struct_decl.kind == StructKind::Dict {
                writer.writeln("var _rows = this.__SplitDoc(this._doc);");
                wln!(writer, "var _result = new {result_type}();");
                writer.writeln("foreach (var _row in _rows)");
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln(
                        "_result[this.__ParseKey(_row)] = this.__ParseValue(_row);",
                    );
                });
                writer.writeln("}");
                writer.writeln("return _result;");
            } else if struct_decl.kind == StructKind::Table {
                writer.writeln("var _table = this.__TableConfig(this._doc);");
                writer.writeln("var _rows = this.__TableRows(_table);");
                for field in &struct_decl.fields {
                    let snake = to_snake_case(&field.name);
                    let ftype = Self::resolve_type(&field.type_info, module);
                    wln!(writer, "{ftype} _val_{snake} = default!;");
                    wln!(writer, "bool _matched_{snake} = false;");
                }
                writer.writeln("foreach (var _row in _rows)");
                writer.writeln("{");
                writer.indented(|writer| {
                    for field in &struct_decl.fields {
                        let snake = to_snake_case(&field.name);
                        wln!(writer,
                            "if (!_matched_{snake} && this.__Parse_{snake}(_row, out var _cand_{snake}))"
                        );
                        writer.writeln("{");
                        writer.indented(|writer| {
                            wln!(writer, "_val_{snake} = _cand_{snake};");
                            wln!(writer, "_matched_{snake} = true;");
                        });
                        writer.writeln("}");
                    }
                });
                writer.writeln("}");
                if struct_decl.fields.is_empty() {
                    wln!(writer, "return new {type_name}();");
                } else {
                    let args: Vec<String> = struct_decl
                        .fields
                        .iter()
                        .map(|f| format!("_val_{}", to_snake_case(&f.name)))
                        .collect();
                    wln!(writer, "return new {type_name}({});", args.join(", "));
                }
            } else if struct_decl.fields.is_empty() {
                wln!(writer, "return new {type_name}();");
            } else {
                let args: Vec<String> = struct_decl
                    .fields
                    .iter()
                    .map(|f| format!("this.__Parse_{}(this._doc)", to_snake_case(&f.name)))
                    .collect();
                wln!(writer, "return new {type_name}({});", args.join(", "));
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
        let pascal_name = csharp_pascal(&func.name);
        let ret_type = Self::resolve_type(&func.return_type, module);

        writer.writeln("public static partial class Functions");
        writer.writeln("{");
        writer.indented(|writer| {
            if let Some(ref doc) = func.doc {
                emit_csharp_doc(doc, "///", writer);
            }
            writer.reset_vars();
            let _ = writer.next_var(); // v0
            wln!(
                writer,
                "public static {ret_type} {pascal_name}(string document)"
            );
            writer.writeln("{");
            writer.indented(|writer| {
                if func.is_raw {
                    writer.writeln("var v = document;");
                } else {
                    writer.writeln("var v = Rt.ParseHtml(document);");
                }
                self.emit_pipeline_body(
                    &func.pipeline,
                    "v",
                    &ret_type,
                    &func.name,
                    false,
                    module,
                    builder,
                    writer,
                );
            });
            writer.writeln("}");

            if !func.is_raw {
                writer.blankline();
                writer.reset_vars();
                let _ = writer.next_var(); // v0
                wln!(
                    writer,
                    "public static {ret_type} {pascal_name}(IElement document)"
                );
                writer.writeln("{");
                writer.indented(|writer| {
                    writer.writeln("var v = document;");
                    self.emit_pipeline_body(
                        &func.pipeline,
                        "v",
                        &ret_type,
                        &func.name,
                        false,
                        module,
                        builder,
                        writer,
                    );
                });
                writer.writeln("}");
            }
        });
        writer.writeln("}");
        writer.blankline();
    }
}

/// Emits C# source files for a compiled [`Module`] and [`CodegenOptions`].
///
/// Returns `(parser_source, optional_runtime_source, builder)` or `Err(diagnostics)`.
pub fn emit_csharp_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, Option<String>, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = CSharpEmitter::new(options.clone());
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
        Some(emit_csharp_runtime_module(module, &builder, options))
    } else {
        None
    };
    Ok((parser_code, runtime_code, builder))
}

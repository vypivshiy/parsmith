//! PHP 8.1+ target backend (`PhpEmitter`, `PhpHtmlLib::Native` `ext-dom` & `PhpHtmlLib::DomCrawler` `Symfony\Component\DomCrawler\Crawler`).
//!
//! Emits 4-space indented PHP 8.1+ source code starting with `<?php\ndeclare(strict_types=1);\n`,
//! supporting single-file inline `final class Rt` (default), `-R / --separate-runtime`
//! (`require_once __DIR__ . '/<runtime_name>.php';`), and `--package <Namespace>`.

pub mod dom;
pub mod json;
pub mod rest;
pub mod runtime;

use std::fmt::Write as _;

use crate::kdl::Span;
use indexmap::IndexSet;

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, FieldDecl, FunctionDef, InitFieldDecl, JsonDef,
    LiteralValue, Module, Pipeline, RequestDecl, RestArtifact, RestEndpointDecl, StepOp,
    StructDecl, StructKind, StructTypeDef, TypeInfo, TypedStep,
};
use crate::codegen::builder::{ModuleBuilder, emit_extension_call};
use crate::codegen::emitter::{LanguageEmitter, drive_module};
use crate::codegen::php::dom::{PhpDom, PhpDomSpelling};
use crate::codegen::php::json::{is_dict_type_info, php_json_projector_method};
use crate::codegen::php::runtime::{emit_php_runtime_module, write_php_runtime_body};
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, module_has_html};
use crate::diagnostic::Diagnostic;
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::{PHP_RESERVED, is_valid_symbol, to_snake_case};
use crate::lowering::clean_doc;
use crate::validators::normalize_regex_pattern;

pub use runtime::{PHP_ERROR_CLASSES, PHP_HTTP_CLASSES, PHP_RT_HTTP_METHODS, PHP_RT_METHODS};

const PHP_SOFT_RESERVED_TYPES: &[&str] = &[
    "int", "float", "bool", "string", "true", "false", "null", "void", "never", "object", "mixed",
    "iterable", "callable", "array", "self", "parent", "static", "readonly", "enum", "resource",
    "numeric", "scalar",
];

/// Returns `true` if `ident` is a reserved keyword or built-in type name that cannot be used as a PHP class name.
pub fn is_php_reserved_class_name(ident: &str) -> bool {
    PHP_RESERVED.iter().any(|kw| kw.eq_ignore_ascii_case(ident))
        || PHP_SOFT_RESERVED_TYPES
            .iter()
            .any(|kw| kw.eq_ignore_ascii_case(ident))
}

/// Normalizes and validates a PHP namespace string (`options.package`).
///
/// - Empty or `"main"` returns `Some("")` (no `namespace` declaration).
/// - Non-empty strings normalize `.` to `\` and validate each segment as a valid PHP identifier.
/// - Returns `None` if any segment is invalid or a PHP reserved keyword.
pub fn normalize_php_namespace(pkg: &str) -> Option<String> {
    let trimmed = pkg.trim();
    if trimmed.is_empty() || trimmed == "main" {
        return Some(String::new());
    }
    let normalized = trimmed.replace('.', "\\");
    if normalized.starts_with('\\') || normalized.ends_with('\\') {
        return None;
    }
    for seg in normalized.split('\\') {
        if seg.is_empty()
            || !is_valid_symbol("php", seg)
            || PHP_RESERVED.iter().any(|kw| kw.eq_ignore_ascii_case(seg))
            || PHP_SOFT_RESERVED_TYPES
                .iter()
                .any(|kw| kw.eq_ignore_ascii_case(seg))
        {
            return None;
        }
    }
    Some(normalized)
}

/// Returns `true` if `pkg` is a valid `--package` / `--namespace` option for PHP.
pub fn validate_php_namespace(pkg: &str) -> bool {
    normalize_php_namespace(pkg).is_some()
}

/// Formats a `&str` as a valid PHP string literal (`'...'` or `"..."` with `$` escaped).
pub fn php_str(s: &str) -> String {
    let has_control = s
        .chars()
        .any(|c| matches!(c, '\n' | '\r' | '\t' | '\0') || (c as u32) < 0x20 || (c as u32) == 0x7f);
    if !has_control {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('\'');
        for ch in s.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                c => out.push(c),
            }
        }
        out.push('\'');
        return out;
    }

    let mut out = String::with_capacity(s.len() + 4);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
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

/// Formats a slice of strings as a PHP array literal (`['a', 'b']`).
pub fn php_str_array(items: &[String]) -> String {
    let inner = items
        .iter()
        .map(|s| php_str(s))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}

/// Wraps a DSL regex pattern in PCRE `~...~u` delimiters and formats it as a PHP string literal.
pub fn php_pcre_pattern(raw_pattern: &str) -> String {
    let norm = normalize_regex_pattern(raw_pattern);
    let mut escaped = String::with_capacity(norm.len() + 4);
    escaped.push('~');
    let chars: Vec<char> = norm.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            escaped.push('\\');
            if i + 1 < chars.len() {
                escaped.push(chars[i + 1]);
                i += 2;
                continue;
            }
        } else if c == '~' {
            escaped.push_str("\\~");
        } else {
            escaped.push(c);
        }
        i += 1;
    }
    escaped.push_str("~u");
    php_str(&escaped)
}

/// Normalizes a `re-sub` replacement string for PHP `preg_replace` (`\1` / `$1` -> `${1}`).
pub fn php_replacement(replacement: &str) -> String {
    let mut out = String::with_capacity(replacement.len() + 4);
    let bytes = replacement.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    while i < n {
        if (bytes[i] == b'\\' || bytes[i] == b'$')
            && i + 1 < n
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 1] != b'0'
        {
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
            } else {
                out.push_str("\\$");
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

/// Renders a DSL identifier as a `snake_case` PHP variable/property/method identifier (without leading `$`).
pub fn php_ident(value: &str) -> String {
    let name = to_snake_case(value).replace('-', "_");
    if name.is_empty() {
        "value".to_owned()
    } else {
        name
    }
}

/// Renders a DSL identifier as a `PascalCase` PHP name.
pub fn php_pascal(value: &str) -> String {
    let name = to_pascal_case(value).replace('-', "");
    if name.is_empty() {
        "Value".to_owned()
    } else {
        name
    }
}

/// Renders a DSL struct name as a valid PHP class name, suffixing reserved words with `Parser`.
pub fn php_class_name(value: &str) -> String {
    let pascal = php_pascal(value);
    if is_php_reserved_class_name(&pascal) {
        format!("{pascal}Parser")
    } else {
        pascal
    }
}

/// Renders a top-level DSL function name as a valid PHP function identifier, prefixing `_` if reserved.
pub fn php_fn_name(value: &str) -> String {
    let snake = php_ident(value);
    if PHP_RESERVED
        .iter()
        .any(|kw| kw.eq_ignore_ascii_case(&snake))
    {
        format!("_{snake}")
    } else {
        snake
    }
}

/// Formats a [`LiteralValue`] as a PHP literal expression.
pub fn php_literal(val: &LiteralValue) -> String {
    match val {
        LiteralValue::Null => "null".to_owned(),
        LiteralValue::Bool(true) => "true".to_owned(),
        LiteralValue::Bool(false) => "false".to_owned(),
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => php_str(s),
        LiteralValue::EmptyArray => "[]".to_owned(),
    }
}

/// Formats a [`CondValue`] as a PHP literal expression.
pub fn php_cond_val(val: &CondValue, input_type: &TypeInfo) -> String {
    match val {
        CondValue::Int(n) => {
            if input_type.base == BaseType::Float && !input_type.is_array {
                format!("{n}.0")
            } else {
                n.to_string()
            }
        }
        CondValue::Float(f) => format!("{f:?}"),
        CondValue::Bool(true) => "true".to_owned(),
        CondValue::Bool(false) => "false".to_owned(),
        CondValue::String(s) => php_str(s),
    }
}

/// Writes a PHPDoc block (`/** ... */`) from `doc` to `writer`.
pub fn emit_php_doc(doc: &str, writer: &mut CodeWriter) {
    let cleaned = clean_doc(doc);
    if cleaned.is_empty() {
        return;
    }
    let lines: Vec<&str> = cleaned.lines().collect();
    if lines.len() == 1 {
        wln!(writer, "/** {} */", lines[0].replace("*/", "* /"));
    } else {
        writer.writeln("/**");
        for line in lines {
            if line.trim().is_empty() {
                writer.writeln(" *");
            } else {
                wln!(writer, " * {}", line.replace("*/", "* /"));
            }
        }
        writer.writeln(" */");
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

/// PHP 8.1+ `LanguageEmitter` implementation.
pub struct PhpEmitter<D: PhpDomSpelling = PhpDom> {
    /// Code generation options.
    pub options: CodegenOptions,
    /// DOM extraction spelling strategy (`PhpNativeSpelling` or `PhpDomCrawlerSpelling`).
    pub dom: D,
    /// Diagnostics accumulated during code generation (`E002` CSS-to-XPath lowering, `E400` missing `"php"` extension target).
    pub diagnostics: Vec<Diagnostic>,
}

impl PhpEmitter<PhpDom> {
    /// Creates a new `PhpEmitter` configured from `options`.
    pub fn new(options: CodegenOptions) -> Self {
        let dom = PhpDom::from_options(&options);
        Self {
            options,
            dom,
            diagnostics: Vec::new(),
        }
    }
}

impl<D: PhpDomSpelling> PhpEmitter<D> {
    /// Creates a new `PhpEmitter` with an explicit [`PhpDomSpelling`] implementation.
    pub fn with_dom(options: CodegenOptions, dom: D) -> Self {
        Self {
            options,
            dom,
            diagnostics: Vec::new(),
        }
    }

    /// Resolves a [`TypeInfo`] into a PHP 8.1+ type hint (`string`, `int`, `float`, `bool`, `array`, `?T`, `mixed`).
    pub fn resolve_type(&self, info: &TypeInfo, module: &Module) -> String {
        let optional = info.is_optional || info.omitempty;
        if info.is_dict {
            return if optional {
                "?array".to_owned()
            } else {
                "array".to_owned()
            };
        }

        let mut base = match &info.base {
            BaseType::String => "string".to_owned(),
            BaseType::Int => "int".to_owned(),
            BaseType::Float => "float".to_owned(),
            BaseType::Bool => "bool".to_owned(),
            BaseType::Null | BaseType::Auto | BaseType::DictValue => "mixed".to_owned(),
            BaseType::Document => {
                if info.is_array {
                    self.dom.document_array_type().to_owned()
                } else {
                    self.dom.document_type().to_owned()
                }
            }
            BaseType::Nested(ref_name) => Self::resolve_struct_result_type(ref_name, module),
            BaseType::Json(ref_name) => {
                let target_json = module.json_def(ref_name);
                if target_json.is_some_and(|j| j.is_array || j.is_dict) || info.is_array {
                    "array".to_owned()
                } else {
                    format!("{}Json", php_pascal(ref_name))
                }
            }
        };

        if info.is_array
            && !matches!(
                info.base,
                BaseType::Document | BaseType::Nested(_) | BaseType::Json(_)
            )
        {
            base = "array".to_owned();
        }

        if optional && base != "mixed" && !base.starts_with('?') {
            format!("?{base}")
        } else {
            base
        }
    }

    /// Resolves the PHP return type of a struct's `parse()` method.
    pub fn resolve_struct_result_type(name: &str, module: &Module) -> String {
        let base = format!("{}Type", php_pascal(name));
        let Some(struct_decl) = module.struct_decl(name) else {
            return base;
        };
        match struct_decl.kind {
            StructKind::List | StructKind::Flat | StructKind::Dict | StructKind::Table => {
                "array".to_owned()
            }
            StructKind::Raw if struct_decl.split_doc.is_some() => "array".to_owned(),
            _ => base,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_pipeline_method(
        &mut self,
        method_name: &str,
        vis: &str,
        arg_type: &str,
        ret_type: &str,
        pipeline: &Pipeline,
        context: &str,
        is_pre_validate: bool,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.reset_vars();
        let v0 = writer.next_var();
        wln!(
            writer,
            "{vis} function {method_name}({arg_type} ${v0}): {ret_type}"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            self.emit_pipeline_body(
                pipeline,
                &v0,
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
    pub(crate) fn emit_pipeline_body(
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
            let fb_lit = php_literal(fb_val);
            writer.writeln("try {");
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
                if is_pre_validate {
                    writer.writeln("return;");
                } else {
                    wln!(writer, "return ${last};");
                }
            });
            writer.writeln("} catch (\\Throwable) {");
            writer.indented(|writer| {
                if is_pre_validate {
                    writer.writeln("return;");
                } else {
                    wln!(writer, "return {fb_lit};");
                }
            });
            writer.writeln("}");
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
                wln!(writer, "return ${last};");
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
        let is_arr_in = step.input.is_array;
        let is_arr_out = step.output.is_array;
        let ctx_lit = php_str(context);

        match &step.op {
            StepOp::Value { value } => {
                let lit = php_literal(value);
                wln!(writer, "${nxt} = {lit};");
            }
            StepOp::LoadInit { name } => {
                let snake = php_ident(name);
                wln!(writer, "${nxt} = $this->_{snake};");
            }
            StepOp::Css { queries } => {
                self.dom.emit_css_select(
                    prv,
                    nxt,
                    queries,
                    next_is_to_bool,
                    context,
                    step.span,
                    &module.source_file,
                    &mut self.diagnostics,
                    writer,
                );
            }
            StepOp::CssAll { queries } => {
                self.dom.emit_css_select_all(
                    prv,
                    nxt,
                    queries,
                    step.span,
                    &module.source_file,
                    &mut self.diagnostics,
                    writer,
                );
            }
            StepOp::CssRemove { query } => {
                self.dom.emit_css_remove(
                    prv,
                    nxt,
                    query,
                    step.span,
                    &module.source_file,
                    &mut self.diagnostics,
                    writer,
                );
            }
            StepOp::Xpath { queries } => {
                self.dom
                    .emit_xpath_select(prv, nxt, queries, next_is_to_bool, context, writer);
            }
            StepOp::XpathAll { queries } => {
                self.dom.emit_xpath_select_all(prv, nxt, queries, writer);
            }
            StepOp::XpathRemove { query } => {
                self.dom.emit_xpath_remove(prv, nxt, query, writer);
            }
            StepOp::Text => {
                self.dom
                    .emit_text(prv, nxt, is_arr_out || is_arr_in, writer);
            }
            StepOp::Raw { mode } => {
                self.dom
                    .emit_raw(prv, nxt, *mode, is_arr_out || is_arr_in, writer);
            }
            StepOp::Attr { keys } => {
                self.dom
                    .emit_attr(prv, nxt, keys, is_arr_in, context, writer);
            }
            StepOp::ToBool => {
                self.dom.emit_to_bool(prv, nxt, &step.input, writer);
            }
            StepOp::Trim { chars } => {
                let c_lit = php_str(chars);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::trim($i, {c_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::trim(${prv}, {c_lit});");
                }
            }
            StepOp::Ltrim { chars } => {
                let c_lit = php_str(chars);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::ltrim($i, {c_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::ltrim(${prv}, {c_lit});");
                }
            }
            StepOp::Rtrim { chars } => {
                let c_lit = php_str(chars);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::rtrim($i, {c_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::rtrim(${prv}, {c_lit});");
                }
            }
            StepOp::NormalizeSpace => {
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::normalizeSpace($i), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::normalizeSpace(${prv});");
                }
            }
            StepOp::RmPrefix { substr } => {
                let s_lit = php_str(substr);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::rmPrefix($i, {s_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::rmPrefix(${prv}, {s_lit});");
                }
            }
            StepOp::RmSuffix { substr } => {
                let s_lit = php_str(substr);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::rmSuffix($i, {s_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::rmSuffix(${prv}, {s_lit});");
                }
            }
            StepOp::RmPrefixSuffix { substr } => {
                let s_lit = php_str(substr);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::rmPrefixSuffix($i, {s_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::rmPrefixSuffix(${prv}, {s_lit});");
                }
            }
            StepOp::Fmt { template } => {
                let tmpl_lit = php_str(template);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(mixed $i): string => Rt::fmt({tmpl_lit}, $i), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::fmt({tmpl_lit}, ${prv});");
                }
            }
            StepOp::Repl { old, new } => {
                let old_lit = php_str(old);
                let new_lit = php_str(new);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => str_replace({old_lit}, {new_lit}, $i), ${prv}));"
                    );
                } else {
                    wln!(
                        writer,
                        "${nxt} = str_replace({old_lit}, {new_lit}, ${prv});"
                    );
                }
            }
            StepOp::ReplMap { replacements } => {
                let pairs = replacements
                    .iter()
                    .map(|(k, v)| format!("{} => {}", php_str(k), php_str(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let map_lit = format!("[{pairs}]");
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::replMap($i, {map_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::replMap(${prv}, {map_lit});");
                }
            }
            StepOp::Lower => {
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::lower($i), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::lower(${prv});");
                }
            }
            StepOp::Upper => {
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::upper($i), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::upper(${prv});");
                }
            }
            StepOp::Split { sep } => {
                let sep_lit = php_str(sep);
                wln!(writer, "${nxt} = explode({sep_lit}, ${prv});");
            }
            StepOp::Join { sep } => {
                let sep_lit = php_str(sep);
                wln!(writer, "${nxt} = implode({sep_lit}, ${prv});");
            }
            StepOp::Unescape => {
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::unescape($i), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::unescape(${prv});");
                }
            }
            StepOp::Re { pattern } => {
                let pat_lit = php_pcre_pattern(pattern);
                let loc_str = if context.is_empty() {
                    String::new()
                } else {
                    format!(" at {context}")
                };
                let msg = format!(
                    "{}:{}:{} re-match failed{loc_str} pattern={pattern}",
                    module.source_file, step.span.line, step.span.col
                );
                let msg_lit = php_str(&msg);
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::reSearch({pat_lit}, $i, {msg_lit}), ${prv}));"
                    );
                } else {
                    wln!(
                        writer,
                        "${nxt} = Rt::reSearch({pat_lit}, ${prv}, {msg_lit});"
                    );
                }
            }
            StepOp::ReAll { pattern } => {
                let pat_lit = php_pcre_pattern(pattern);
                wln!(writer, "${nxt} = Rt::reAll({pat_lit}, ${prv});");
            }
            StepOp::ReSub { pattern, repl } => {
                let pat_lit = php_pcre_pattern(pattern);
                let repl_lit = php_str(&php_replacement(repl));
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): string => Rt::reSub({pat_lit}, {repl_lit}, $i), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::reSub({pat_lit}, {repl_lit}, ${prv});");
                }
            }
            StepOp::ReSplit { pattern } => {
                let pat_lit = php_pcre_pattern(pattern);
                wln!(writer, "${nxt} = Rt::reSplit({pat_lit}, ${prv});");
            }
            StepOp::Index { index } => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "${nxt} = Rt::strIndex(${prv}, {index}, {ctx_lit});");
                } else {
                    wln!(writer, "${nxt} = Rt::itemAt(${prv}, {index}, {ctx_lit});");
                }
            }
            StepOp::Slice { start, end } => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "${nxt} = Rt::strSlice(${prv}, {start}, {end});");
                } else {
                    wln!(writer, "${nxt} = Rt::sliceVec(${prv}, {start}, {end});");
                }
            }
            StepOp::Len => {
                if step.input.base == BaseType::String && !step.input.is_array {
                    wln!(writer, "${nxt} = Rt::strLen(${prv});");
                } else {
                    wln!(writer, "${nxt} = count(${prv});");
                }
            }
            StepOp::Unique { .. } => {
                wln!(writer, "${nxt} = Rt::unique(${prv});");
            }
            StepOp::ToInt => {
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): int => Rt::toInt($i, {ctx_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::toInt(${prv}, {ctx_lit});");
                }
            }
            StepOp::ToFloat => {
                if is_arr_out {
                    wln!(
                        writer,
                        "${nxt} = array_values(array_map(static fn(string $i): float => Rt::toFloat($i, {ctx_lit}), ${prv}));"
                    );
                } else {
                    wln!(writer, "${nxt} = Rt::toFloat(${prv}, {ctx_lit});");
                }
            }
            StepOp::Jsonify {
                schema_name, path, ..
            } => {
                let effective_path = path
                    .as_deref()
                    .or_else(|| module.json_def(schema_name).and_then(|j| j.path.as_deref()))
                    .unwrap_or("");
                let raw_expr = format!(
                    "Rt::parseJson(${prv}, {}, {ctx_lit})",
                    php_str(effective_path)
                );
                if let Some(target_def) = module.json_def(schema_name) {
                    let s_cls = format!("{}Json", php_pascal(schema_name));
                    let proj = php_json_projector_method(schema_name);
                    if is_arr_out && !target_def.is_array {
                        wln!(
                            writer,
                            "${nxt} = array_values(array_map(static fn(mixed $_x): mixed => {s_cls}::{proj}($_x), Rt::expectList({raw_expr}, {ctx_lit})));"
                        );
                    } else {
                        wln!(writer, "${nxt} = {s_cls}::{proj}({raw_expr});");
                    }
                } else {
                    wln!(writer, "${nxt} = Rt::toPlainPhp({raw_expr});");
                }
            }
            StepOp::Nested { struct_name } => {
                let cls = php_class_name(struct_name);
                wln!(writer, "${nxt} = (new {cls}(${prv}))->parse();");
            }
            StepOp::Filter { cond } => {
                let elem_type = TypeInfo {
                    is_array: false,
                    ..step.input.clone()
                };
                let cond_str = self.render_cond(
                    cond,
                    "$i",
                    &elem_type,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                wln!(writer, "${nxt} = [];");
                wln!(writer, "foreach (${prv} as $i) {{");
                writer.indented(|writer| {
                    wln!(writer, "if ({cond_str}) {{");
                    writer.indented(|writer| {
                        wln!(writer, "${nxt}[] = $i;");
                    });
                    writer.writeln("}");
                });
                writer.writeln("}");
            }
            StepOp::Assert { message, cond } => {
                let target = format!("${prv}");
                let cond_str = self.render_cond(
                    cond,
                    &target,
                    &step.input,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                let msg = match message {
                    Some(m) => m.clone(),
                    None => {
                        let loc_str = if context.is_empty() {
                            String::new()
                        } else {
                            format!(" at {context}")
                        };
                        format!(
                            "{}:{}:{} assertion failed{loc_str}",
                            module.source_file, step.span.line, step.span.col
                        )
                    }
                };
                wln!(writer, "Rt::assertTrue({cond_str}, {});", php_str(&msg));
                if !(is_pre_validate && is_last_step) {
                    wln!(writer, "${nxt} = ${prv};");
                }
            }
            StepOp::Match { cond } => {
                let key_type = TypeInfo::string();
                let cond_str = self.render_cond(
                    cond,
                    "$_key",
                    &key_type,
                    step.span,
                    &module.source_file,
                    writer.is_dry_run(),
                );
                writer.writeln("try {");
                writer.indented(|writer| {
                    wln!(writer, "$_key = $this->_table_match_key(${prv});");
                });
                writer.writeln("} catch (\\Throwable) {");
                writer.indented(|writer| {
                    writer.writeln("return Rt::UNMATCHED_TABLE_ROW;");
                });
                writer.writeln("}");
                wln!(writer, "if (!({cond_str})) {{");
                writer.indented(|writer| {
                    writer.writeln("return Rt::UNMATCHED_TABLE_ROW;");
                });
                writer.writeln("}");
                wln!(writer, "${nxt} = $this->_parse_value(${prv});");
            }
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
                    &["php"],
                    "'php'",
                    &format!("${prv}"),
                    &format!("${nxt}"),
                    &in_type,
                    &out_type,
                    |imp| imp.value.clone(),
                    |code| {
                        let normalized = code
                            .replace("${{in}}", "{{in}}")
                            .replace("${{out}}", "{{out}}");
                        let trimmed = normalized.trim_end();
                        if !trimmed.is_empty() && !trimmed.ends_with(';') && !trimmed.ends_with('}')
                        {
                            format!("{trimmed};")
                        } else {
                            trimmed.to_owned()
                        }
                    },
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
                format!("!({inner_s})")
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
            format!("Rt::text({target})")
        } else {
            target.to_owned()
        };
        let count_expr = if input_type.base == BaseType::String && !input_type.is_array {
            format!("Rt::strLen({target})")
        } else {
            format!("count({target})")
        };

        match op {
            CondOp::Css { query } => self.dom.pred_css(
                target,
                query,
                span,
                source_file,
                is_dry_run,
                &mut self.diagnostics,
            ),
            CondOp::Xpath { query } => self.dom.pred_xpath(target, query),
            CondOp::HasAttr { attrs } => {
                format!("Rt::predHasAttr({target}, {})", php_str_array(attrs))
            }
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                let attr = format!("Rt::predAttr({target}, {})", php_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} === {}", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrNe { name, values } => {
                let attr = format!("Rt::predAttr({target}, {})", php_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{attr} !== {}", php_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::AttrStarts { name, values } => {
                let attr = format!("Rt::predAttr({target}, {})", php_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("str_starts_with({attr}, {})", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrEnds { name, values } => {
                let attr = format!("Rt::predAttr({target}, {})", php_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("str_ends_with({attr}, {})", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrContains { name, values } => {
                let attr = format!("Rt::predAttr({target}, {})", php_str(name));
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("str_contains({attr}, {})", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::AttrRe { name, pattern } => {
                let attr = format!("Rt::predAttr({target}, {})", php_str(name));
                format!("Rt::reMatch({}, {attr})", php_pcre_pattern(pattern))
            }
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} === {}", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{text_val} !== {}", php_str(v)))
                    .collect();
                join_and(&checks)
            }
            CondOp::TextStarts { values } | CondOp::Starts { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("str_starts_with({text_val}, {})", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextEnds { values } | CondOp::Ends { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("str_ends_with({text_val}, {})", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextContains { values } | CondOp::Contains { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("str_contains({text_val}, {})", php_str(v)))
                    .collect();
                join_or(&checks)
            }
            CondOp::TextRe { pattern } | CondOp::Re { pattern } => {
                format!("Rt::reMatch({}, {text_val})", php_pcre_pattern(pattern))
            }
            CondOp::ReAny { pattern } => {
                format!("Rt::reAny({}, {target})", php_pcre_pattern(pattern))
            }
            CondOp::ReAll { pattern } => {
                format!("Rt::reAllMatch({}, {target})", php_pcre_pattern(pattern))
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
                        .map(|v| format!("{count_expr} === {}", v.as_str_cow()))
                        .collect();
                    join_or(&checks)
                } else {
                    let checks: Vec<String> = values
                        .iter()
                        .map(|v| format!("{target} === {}", php_cond_val(v, input_type)))
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
                        .map(|v| format!("{count_expr} !== {}", v.as_str_cow()))
                        .collect();
                    join_and(&checks)
                } else {
                    let checks: Vec<String> = values
                        .iter()
                        .map(|v| format!("{target} !== {}", php_cond_val(v, input_type)))
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
                    .map(|v| format!("{count_expr} === {v}"))
                    .collect();
                join_or(&checks)
            }
            CondOp::LenNe { values } => {
                let checks: Vec<String> = values
                    .iter()
                    .map(|v| format!("{count_expr} !== {v}"))
                    .collect();
                join_and(&checks)
            }
            CondOp::LenGt { value } => format!("{count_expr} > {value}"),
            CondOp::LenLt { value } => format!("{count_expr} < {value}"),
            CondOp::LenGe { value } => format!("{count_expr} >= {value}"),
            CondOp::LenLe { value } => format!("{count_expr} <= {value}"),
            CondOp::LenRange { start, end } => {
                format!("({start} < {count_expr} && {count_expr} < {end})")
            }
        }
    }

    fn format_num_or_len_cmp(
        target: &str,
        count_expr: &str,
        op: &str,
        value: i64,
        input_type: &TypeInfo,
    ) -> String {
        if (input_type.base == BaseType::Int || input_type.base == BaseType::Float)
            && !input_type.is_array
        {
            format!("({target} {op} {value})")
        } else {
            format!("({count_expr} {op} {value})")
        }
    }
}

impl<D: PhpDomSpelling> LanguageEmitter for PhpEmitter<D> {
    fn indent_unit(&self) -> &'static str {
        "    "
    }

    fn on_module_start(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.writeln("<?php");
        writer.writeln("declare(strict_types=1);");
        writer.writeln("// Auto-generated by parsmith. DO NOT EDIT.");
        writer.blankline();

        if let Some(ns) = normalize_php_namespace(&self.options.package)
            && !ns.is_empty()
        {
            wln!(writer, "namespace {ns};");
            writer.blankline();
        }

        if let Some(ref doc) = module.doc {
            emit_php_doc(doc, writer);
            writer.blankline();
        }

        if module_has_html(module) {
            self.dom.register_imports(builder);
        }
    }

    fn emit_imports(
        &mut self,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let mut imports: IndexSet<String> = IndexSet::new();
        for imp in builder.imports() {
            let trimmed = imp.trim();
            if !trimmed.is_empty() {
                if trimmed.ends_with(';') {
                    imports.insert(trimmed.to_owned());
                } else {
                    imports.insert(format!("{trimmed};"));
                }
            }
        }
        if !self.options.separate_runtime {
            for imp in builder.runtime_imports() {
                let trimmed = imp.trim();
                if !trimmed.is_empty() {
                    if trimmed.ends_with(';') {
                        imports.insert(trimmed.to_owned());
                    } else {
                        imports.insert(format!("{trimmed};"));
                    }
                }
            }
        }

        for imp in &imports {
            writer.write_block(imp);
        }
        if !imports.is_empty() {
            writer.blankline();
        }

        if self.options.separate_runtime {
            let clean_rt = self
                .options
                .runtime_name
                .trim_start_matches('.')
                .trim_end_matches(".php");
            wln!(writer, "require_once __DIR__ . '/{clean_rt}.php';");
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
            write_php_runtime_body(module, builder, &self.options, writer);
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
        match typedef.struct_kind {
            StructKind::Rest | StructKind::Flat | StructKind::Dict | StructKind::Table => {}
            StructKind::Item | StructKind::List | StructKind::Raw => {
                let name = format!("{}Type", php_pascal(&typedef.name));
                wln!(
                    writer,
                    "readonly class {name} implements \\JsonSerializable, \\ArrayAccess"
                );
                writer.writeln("{");
                writer.indented(|writer| {
                    if typedef.fields.is_empty() {
                        writer.writeln("public function __construct()");
                        writer.writeln("{");
                        writer.writeln("}");
                    } else {
                        writer.writeln("public function __construct(");
                        writer.indented(|writer| {
                            for field in &typedef.fields {
                                let f_ident = php_ident(&field.name);
                                let f_type = self.resolve_type(&field.type_info, module);
                                wln!(writer, "public {f_type} ${f_ident},");
                            }
                        });
                        writer.writeln(") {");
                        writer.writeln("}");
                    }
                    writer.blankline();

                    writer.writeln("public function jsonSerialize(): mixed");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        if typedef.fields.is_empty() {
                            writer.writeln("return new \\stdClass();");
                        } else {
                            writer.writeln("$out = [];");
                            for field in &typedef.fields {
                                let f_ident = php_ident(&field.name);
                                let key_lit = php_str(&to_snake_case(&field.name));
                                let is_dict = is_dict_type_info(&field.type_info, module);
                                let val_expr = if is_dict {
                                    format!("Rt::serializeDict($this->{f_ident})")
                                } else {
                                    format!("$this->{f_ident}")
                                };
                                if field.type_info.omitempty {
                                    wln!(writer,
                                        "if ($this->{f_ident} !== null) {{"
                                    );
                                    writer.indented(|writer| {
                                        wln!(writer,
                                            "$out[{key_lit}] = {val_expr};"
                                        );
                                    });
                                    writer.writeln("}");
                                } else {
                                    wln!(writer, "$out[{key_lit}] = {val_expr};");
                                }
                            }
                            writer.writeln("return empty($out) ? new \\stdClass() : $out;");
                        }
                    });
                    writer.writeln("}");
                    writer.blankline();

                    writer.writeln("public function offsetExists(mixed $offset): bool");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        writer.writeln(
                            "return is_string($offset) && property_exists($this, $offset) && $this->{$offset} !== null;",
                        );
                    });
                    writer.writeln("}");
                    writer.blankline();

                    writer.writeln("public function offsetGet(mixed $offset): mixed");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        writer.writeln("return $this->{$offset};");
                    });
                    writer.writeln("}");
                    writer.blankline();

                    writer.writeln("public function offsetSet(mixed $offset, mixed $value): void");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        writer.writeln("throw new \\BadMethodCallException('Cannot modify readonly object');");
                    });
                    writer.writeln("}");
                    writer.blankline();

                    writer.writeln("public function offsetUnset(mixed $offset): void");
                    writer.writeln("{");
                    writer.indented(|writer| {
                        writer.writeln("throw new \\BadMethodCallException('Cannot modify readonly object');");
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
        if let Some(ref doc) = struct_decl.doc {
            emit_php_doc(doc, writer);
        }
        let cls_name = php_class_name(&struct_decl.name);
        wln!(writer, "class {cls_name}");
        writer.writeln("{");
        writer.indent();

        if struct_decl.kind == StructKind::Rest {
            writer.writeln("public function __construct()");
            writer.writeln("{");
            writer.writeln("}");
            writer.blankline();
            writer.writeln("public static function new(): static");
            writer.writeln("{");
            writer.indented(|writer| {
                writer.writeln("return new static();");
            });
            writer.writeln("}");
            writer.blankline();
            Self::emit_struct_matchers_method(struct_decl, module, writer);
            return;
        }

        let is_raw = struct_decl.kind == StructKind::Raw;
        let doc_type = if is_raw {
            "string"
        } else {
            self.dom.document_type()
        };
        wln!(writer, "private {doc_type} $_doc;");
        for init_f in &struct_decl.init_fields {
            let snake = php_ident(&init_f.name);
            let typ = self.resolve_type(&init_f.type_info, module);
            wln!(writer, "private {typ} $_{snake};");
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
        let is_raw = struct_decl.kind == StructKind::Raw;
        let arg_ty = if is_raw {
            "string"
        } else {
            self.dom.init_arg_type()
        };

        wln!(writer, "public function __construct({arg_ty} $document)");
        writer.writeln("{");
        writer.indented(|writer| {
            if is_raw {
                writer.writeln("$this->_doc = $document;");
            } else if self.dom.backend_name() == "php-dom-crawler" {
                writer.writeln(
                    "$this->_doc = $document instanceof Crawler ? $document : new Crawler($document);",
                );
            } else {
                writer.writeln("$this->_doc = Rt::parseHtml($document);");
            }
            for init_f in &struct_decl.init_fields {
                let snake = php_ident(&init_f.name);
                wln!(writer,
                    "$this->_{snake} = $this->_init_{snake}($this->_doc);"
                );
            }
        });
        writer.writeln("}");
        writer.blankline();

        wln!(
            writer,
            "public static function new({arg_ty} $document): static"
        );
        writer.writeln("{");
        writer.indented(|writer| {
            writer.writeln("return new static($document);");
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
        let snake = php_ident(&init_field.name);
        let method_name = format!("_init_{snake}");
        let arg_ty = self.resolve_type(&init_field.pipeline.input, module);
        let ret_ty = self.resolve_type(&init_field.type_info, module);
        let context = format!(
            "{}.@init.{}",
            php_pascal(&struct_decl.name),
            init_field.name
        );
        self.emit_pipeline_method(
            &method_name,
            "private",
            &arg_ty,
            &ret_ty,
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
        let arg_ty = self.resolve_type(&pipeline.input, module);
        let context = format!("{}.@pre-validate", php_pascal(&struct_decl.name));
        self.emit_pipeline_method(
            "_pre_validate",
            "private",
            &arg_ty,
            "void",
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let snake = php_ident(&check.name);
        let ret_ty = self.resolve_type(&check.pipeline.output, module);
        let context = format!("{}.{}", php_pascal(&struct_decl.name), check.name);

        wln!(writer, "public function {snake}(): {ret_ty}");
        writer.writeln("{");
        writer.indented(|writer| {
            wln!(writer, "${v0} = $this->_doc;");
            self.emit_pipeline_body(
                &check.pipeline,
                &v0,
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
        let arg_ty = self.resolve_type(&pipeline.input, module);
        let ret_ty = self.resolve_type(&pipeline.output, module);
        let context = format!("{}.@split-doc", php_pascal(&struct_decl.name));
        self.emit_pipeline_method(
            "_split_doc",
            "private",
            &arg_ty,
            &ret_ty,
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
        let arg_ty = self.resolve_type(&pipeline.input, module);
        let ret_ty = self.resolve_type(&pipeline.output, module);
        let context = format!("{}.key", php_pascal(&struct_decl.name));
        self.emit_pipeline_method(
            "_parse_key",
            "private",
            &arg_ty,
            &ret_ty,
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
        let arg_ty = self.resolve_type(&pipeline.input, module);
        let ret_ty = self.resolve_type(&pipeline.output, module);
        let context = format!("{}.value", php_pascal(&struct_decl.name));
        self.emit_pipeline_method(
            "_parse_value",
            "private",
            &arg_ty,
            &ret_ty,
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
        let arg_ty = self.resolve_type(&pipeline.input, module);
        let ret_ty = self.resolve_type(&pipeline.output, module);
        let context = format!("{}.@table", php_pascal(&struct_decl.name));
        self.emit_pipeline_method(
            "_table_config",
            "private",
            &arg_ty,
            &ret_ty,
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
        let arg_ty = self.resolve_type(&pipeline.input, module);
        let ret_ty = self.resolve_type(&pipeline.output, module);
        let context = format!("{}.@rows", php_pascal(&struct_decl.name));
        self.emit_pipeline_method(
            "_table_rows",
            "private",
            &arg_ty,
            &ret_ty,
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
        let arg_ty = self.resolve_type(&pipeline.input, module);
        let ret_ty = self.resolve_type(&pipeline.output, module);
        let context = format!("{}.@match", php_pascal(&struct_decl.name));
        self.emit_pipeline_method(
            "_table_match_key",
            "private",
            &arg_ty,
            &ret_ty,
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
        let snake = php_ident(&field.name);
        let method_name = format!("_parse_{snake}");
        let arg_ty = self.resolve_type(&field.pipeline.input, module);
        let ret_ty = if struct_decl.kind == StructKind::Table {
            "mixed".to_owned()
        } else {
            self.resolve_type(&field.type_info, module)
        };
        let context = format!("{}.{}", php_pascal(&struct_decl.name), field.name);
        self.emit_pipeline_method(
            &method_name,
            "private",
            &arg_ty,
            &ret_ty,
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
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let pascal = php_pascal(&struct_decl.name);
        let ret_ty = Self::resolve_struct_result_type(&struct_decl.name, module);
        let is_raw = struct_decl.kind == StructKind::Raw;
        let has_raw_split = is_raw && struct_decl.split_doc.is_some();

        wln!(writer, "public function parse(): {ret_ty}");
        writer.writeln("{");
        writer.indented(|writer| {
            if struct_decl.pre_validate.is_some() {
                writer.writeln("$this->_pre_validate($this->_doc);");
            }

            if struct_decl.kind == StructKind::List || has_raw_split {
                writer.writeln("$_rows = $this->_split_doc($this->_doc);");
                writer.writeln("$_result = [];");
                writer.writeln("foreach ($_rows as $_row) {");
                writer.indented(|writer| {
                    if struct_decl.fields.is_empty() {
                        wln!(writer, "$_result[] = new {pascal}Type();");
                    } else {
                        wln!(writer, "$_result[] = new {pascal}Type(");
                        writer.indented(|writer| {
                            for field in &struct_decl.fields {
                                let snake = php_ident(&field.name);
                                wln!(writer, "$this->_parse_{snake}($_row),");
                            }
                        });
                        writer.writeln(");");
                    }
                });
                writer.writeln("}");
                writer.writeln("return $_result;");
            } else if struct_decl.kind == StructKind::Flat {
                writer.writeln("$_result = [];");
                for field in &struct_decl.fields {
                    let snake = php_ident(&field.name);
                    if field.type_info.is_optional || field.type_info.omitempty {
                        wln!(writer,
                            "$_v_{snake} = $this->_parse_{snake}($this->_doc);"
                        );
                        wln!(writer, "if ($_v_{snake} !== null) {{");
                        writer.indented(|writer| {
                            if field.type_info.is_array {
                                wln!(writer,
                                    "array_push($_result, ...$_v_{snake});"
                                );
                            } else {
                                wln!(writer, "$_result[] = $_v_{snake};");
                            }
                        });
                        writer.writeln("}");
                    } else if field.type_info.is_array {
                        wln!(writer,
                            "array_push($_result, ...$this->_parse_{snake}($this->_doc));"
                        );
                    } else {
                        wln!(writer,
                            "$_result[] = $this->_parse_{snake}($this->_doc);"
                        );
                    }
                }
                writer.writeln("return Rt::unique($_result);");
            } else if struct_decl.kind == StructKind::Dict {
                writer.writeln("$_rows = $this->_split_doc($this->_doc);");
                writer.writeln("$_result = [];");
                writer.writeln("foreach ($_rows as $_row) {");
                writer.indented(|writer| {
                    writer.writeln(
                        "$_result[(string) $this->_parse_key($_row)] = $this->_parse_value($_row);",
                    );
                });
                writer.writeln("}");
                writer.writeln("return $_result;");
            } else if struct_decl.kind == StructKind::Table {
                writer.writeln("$_result = [];");
                writer.writeln("$_table = $this->_table_config($this->_doc);");
                writer.writeln("foreach ($this->_table_rows($_table) as $_row) {");
                writer.indented(|writer| {
                    for field in &struct_decl.fields {
                        let snake = php_ident(&field.name);
                        let key_lit = php_str(&to_snake_case(&field.name));
                        wln!(writer,
                            "$_v_{snake} = $this->_parse_{snake}($_row);"
                        );
                        wln!(writer,
                            "if ($_v_{snake} !== Rt::UNMATCHED_TABLE_ROW && !array_key_exists({key_lit}, $_result)) {{"
                        );
                        writer.indented(|writer| {
                            wln!(writer, "$_result[{key_lit}] = $_v_{snake};");
                            writer.writeln("continue;");
                        });
                        writer.writeln("}");
                    }
                });
                writer.writeln("}");
                writer.writeln("return $_result;");
            } else if struct_decl.fields.is_empty() {
                wln!(writer, "return new {pascal}Type();");
            } else {
                wln!(writer, "return new {pascal}Type(");
                writer.indented(|writer| {
                    for field in &struct_decl.fields {
                        let snake = php_ident(&field.name);
                        wln!(writer, "$this->_parse_{snake}($this->_doc),");
                    }
                });
                writer.writeln(");");
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
        struct_decl: &StructDecl,
        module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.dedent();
        writer.writeln("}");
        writer.blankline();

        let cls_name = php_class_name(&struct_decl.name);
        if !cls_name.ends_with("Parser") {
            let parser_alias = format!("{cls_name}Parser");
            let collides = module
                .structs
                .iter()
                .any(|s| php_class_name(&s.name).eq_ignore_ascii_case(&parser_alias));
            if !collides {
                wln!(writer, "class {parser_alias} extends {cls_name}");
                writer.writeln("{");
                writer.writeln("}");
                writer.blankline();
            }
        }
    }

    fn emit_function(
        &mut self,
        func: &FunctionDef,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if let Some(ref doc) = func.doc {
            emit_php_doc(doc, writer);
        }
        let fn_name = php_fn_name(&func.name);
        let ret_ty = self.resolve_type(&func.return_type, module);
        let arg_ty = if func.is_raw {
            "string"
        } else {
            self.dom.init_arg_type()
        };

        writer.reset_vars();
        let v0 = writer.next_var();
        wln!(writer, "function {fn_name}({arg_ty} $document): {ret_ty}");
        writer.writeln("{");
        writer.indented(|writer| {
            if func.is_raw {
                wln!(writer, "${v0} = $document;");
            } else if self.dom.backend_name() == "php-dom-crawler" {
                wln!(
                    writer,
                    "${v0} = $document instanceof Crawler ? $document : new Crawler($document);"
                );
            } else {
                wln!(writer, "${v0} = Rt::parseHtml($document);");
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

/// Emits PHP 8.1+ source files for a compiled [`Module`] and [`CodegenOptions`].
///
/// Returns `(parser_source, optional_runtime_source, builder)` or `Err(diagnostics)`.
pub fn emit_php_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, Option<String>, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = PhpEmitter::new(options.clone());
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
        Some(emit_php_runtime_module(module, &builder, options))
    } else {
        None
    };
    Ok((parser_code, runtime_code, builder))
}

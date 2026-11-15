//! Python 3.10+ target backend (`PythonEmitter`, `Bs4Spelling`, `TypedDict` / `@dataclass(slots=True)`, separate runtime).

pub mod dom;
pub mod json;
pub mod rest;

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::kdl::Span;
use indexmap::{IndexMap, IndexSet};

use crate::ast::{
    BaseType, CheckDecl, Cond, CondOp, CondValue, FieldDecl, FunctionDef, InitFieldDecl, JsonDef,
    LiteralValue, Module, Pipeline, RequestDecl, RestArtifact, RestEndpointDecl, StepOp,
    StructDecl, StructKind, StructTypeDef, TypeInfo, TypedStep,
};
use crate::codegen::builder::{ModuleBuilder, dedent_code_block, emit_extension_call};
use crate::codegen::emitter::{LanguageEmitter, drive_module};
use crate::codegen::python::json::{
    JSON_PROJECT_RUNTIME_SYMBOLS, STD_JSON_PROJECT_CODE, STD_JSON_PROJECT_KEY,
};
use crate::codegen::python::rest::{
    HTTP_TRANSPORT_RUNTIME_SYMBOLS, REST_CORE_RUNTIME_SYMBOLS, STD_FETCH_ASYNC_CODE,
    STD_FETCH_ASYNC_KEY, STD_FETCH_SYNC_CODE, STD_FETCH_SYNC_KEY, STD_HTTP_TRANSPORT_CODE,
    STD_HTTP_TRANSPORT_KEY, STD_REST_ASYNC_CODE, STD_REST_ASYNC_KEY, STD_REST_CORE_CODE,
    STD_REST_CORE_KEY, STD_REST_SYNC_CODE, STD_REST_SYNC_KEY,
};
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, PythonModels};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::json_to_kdl::to_pascal_case;
use crate::linter::symbols::to_snake_case;
use crate::lowering::clean_doc;

pub use crate::codegen::{
    module_has_fetch, module_has_html, module_has_rest, module_has_table, module_uses_http,
};
pub use dom::{
    Bs4Spelling, LxmlSpelling, ParselSpelling, PyDomSpelling, PyDomSpelling as DomSpelling,
    PythonDom, SelectolaxSpelling,
};
pub use json::python_json_projector_name;

const STD_ASSERT_CODE: &str = r#"
class RtAssertionError(ValueError):
    pass


def rt_assert(cond: bool, msg: str = "") -> None:
    if not cond:
        raise RtAssertionError(msg or "parsmith assertion failed")
"#;

const STD_RE_SEARCH_CODE: &str = r#"
class RtRegexError(ValueError):
    pass


def rt_re_search(pattern: str, value: str, msg: str = "") -> str:
    m = re.search(pattern, value)
    if m is None:
        raise RtRegexError(msg or "parsmith re-match failed")
    return m[1]
"#;

const STD_REPL_MAP_CODE: &str = r#"
def rt_repl_map(s: str, replacements: dict[str, str]) -> str:
    for old, new in replacements.items():
        s = s.replace(old, new)
    return s
"#;

const STD_UNESCAPE_CODE: &str = r#"
RT_RE_HEX_ENTITY = re.compile(r"&#x([0-9a-fA-F]+);")
RT_RE_UNICODE_ENTITY = re.compile(r"\\u([0-9a-fA-F]{4})")
RT_RE_BYTES_ENTITY = re.compile(r"\\x([0-9a-fA-F]{2})")
RT_RE_CHARS_MAP: dict[str, str] = {
    "\\b": "\b",
    "\\f": "\f",
    "\\n": "\n",
    "\\r": "\r",
    "\\t": "\t",
}


def rt_unescape_text(text: str) -> str:
    s = html_unescape(text)
    s = RT_RE_HEX_ENTITY.sub(lambda m: chr(int(m.group(1), 16)), s)
    s = RT_RE_UNICODE_ENTITY.sub(lambda m: chr(int(m.group(1), 16)), s)
    s = RT_RE_BYTES_ENTITY.sub(lambda m: chr(int(m.group(1), 16)), s)
    for ch, r in RT_RE_CHARS_MAP.items():
        s = s.replace(ch, r)
    return s
"#;

const STD_UNMATCHED_TABLE_ROW_CODE: &str = r#"
class RtUnmatchedTableRow:
    pass


RT_UNMATCHED_TABLE_ROW = RtUnmatchedTableRow()
"#;

const SHARED_RUNTIME_STD_KEYS: &[&str] = &[
    "RT_UNMATCHED_TABLE_ROW",
    "rt_assert",
    "rt_re_search",
    "rt_repl_map",
    "rt_unescape_text",
    STD_JSON_PROJECT_KEY,
    STD_HTTP_TRANSPORT_KEY,
    STD_FETCH_SYNC_KEY,
    STD_FETCH_ASYNC_KEY,
    STD_REST_CORE_KEY,
    STD_REST_SYNC_KEY,
    STD_REST_ASYNC_KEY,
];

/// Formats a Rust `&str` as a valid Python string literal (`'...'` or `"..."`), matching Python `repr(s)`.
pub fn py_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(quote);
            }
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Formats a slice of strings as a Python list literal (`['a', 'b']`).
pub fn py_str_list(items: &[String]) -> String {
    let inner = items
        .iter()
        .map(|s| py_str(s))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}

/// Formats a slice of strings as a Python tuple literal (`('a',)` or `('a', 'b')`).
pub fn py_str_tuple(items: &[String]) -> String {
    if items.len() == 1 {
        format!("({},)", py_str(&items[0]))
    } else {
        let inner = items
            .iter()
            .map(|s| py_str(s))
            .collect::<Vec<_>>()
            .join(", ");
        format!("({inner})")
    }
}

/// Formats a [`LiteralValue`] as a Python literal expression.
pub fn py_literal(lit: &LiteralValue) -> String {
    match lit {
        LiteralValue::Null => "None".to_owned(),
        LiteralValue::Bool(true) => "True".to_owned(),
        LiteralValue::Bool(false) => "False".to_owned(),
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => py_str(s),
        LiteralValue::EmptyArray => "[]".to_owned(),
    }
}

/// Formats a [`CondValue`] as a Python literal expression.
pub fn py_pred_value(val: &CondValue) -> String {
    match val {
        CondValue::Int(n) => n.to_string(),
        CondValue::Float(f) => format!("{f:?}"),
        CondValue::Bool(true) => "True".to_owned(),
        CondValue::Bool(false) => "False".to_owned(),
        CondValue::String(s) => py_str(s),
    }
}

/// Writes a cleaned Python docstring (`"""..."""`) to `writer`.
pub fn emit_docstring(doc: &str, writer: &mut CodeWriter) {
    let cleaned = clean_doc(doc);
    if cleaned.is_empty() {
        return;
    }
    let escaped = cleaned
        .replace('\\', "\\\\")
        .replace("\"\"\"", "\\\"\\\"\\\"");
    if !escaped.contains('\n') {
        wln!(writer, "\"\"\"{escaped}\"\"\"");
    } else {
        writer.writeln("\"\"\"");
        for line in escaped.lines() {
            if line.trim().is_empty() {
                writer.blankline();
            } else {
                writer.writeln(line);
            }
        }
        writer.writeln("\"\"\"");
    }
}

enum ImportSlot {
    Raw(String),
    FromModule(String),
}

fn parse_simple_from_import(line: &str) -> Option<(&str, Vec<&str>)> {
    if line.contains('\n')
        || line.contains('#')
        || line.contains('(')
        || line.contains(')')
        || line.contains(" as ")
    {
        return None;
    }
    let rest = line.strip_prefix("from ")?;
    let (module, names_str) = rest.split_once(" import ")?;
    let module = module.trim();
    if module.is_empty() {
        return None;
    }
    let names: Vec<&str> = names_str
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() || names.contains(&"*") {
        return None;
    }
    Some((module, names))
}

fn merge_python_imports(raw_imports: &IndexSet<String>) -> (IndexSet<String>, BTreeSet<String>) {
    let mut slots: Vec<ImportSlot> = Vec::with_capacity(raw_imports.len());
    let mut from_groups: IndexMap<String, BTreeSet<String>> = IndexMap::new();
    let mut typing_symbols: BTreeSet<String> = BTreeSet::new();

    for imp in raw_imports {
        if let Some((module, names)) = parse_simple_from_import(imp) {
            if module == "__future__" && names == ["annotations"] {
                continue;
            }
            if module == "typing" {
                typing_symbols.extend(names.into_iter().map(str::to_owned));
                continue;
            }
            if !from_groups.contains_key(module) {
                slots.push(ImportSlot::FromModule(module.to_owned()));
            }
            from_groups
                .entry(module.to_owned())
                .or_default()
                .extend(names.into_iter().map(str::to_owned));
        } else {
            slots.push(ImportSlot::Raw(imp.to_owned()));
        }
    }

    let mut merged: IndexSet<String> = IndexSet::with_capacity(slots.len());
    for slot in slots {
        match slot {
            ImportSlot::Raw(line) => {
                merged.insert(line);
            }
            ImportSlot::FromModule(module) => {
                if let Some(syms) = from_groups.get(&module) {
                    let joined = syms
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    merged.insert(format!("from {module} import {joined}"));
                }
            }
        }
    }

    (merged, typing_symbols)
}

/// Python 3.10+ `LanguageEmitter` implementation.
pub struct PythonEmitter<D: PyDomSpelling = PythonDom> {
    /// Code generation options (`html_lib`, `models`, `bs4_parser`, `http_io`, `separate_runtime`, `runtime_name`).
    pub options: CodegenOptions,
    /// DOM extraction spelling strategy (`PythonDom`, `Bs4Spelling`, `LxmlSpelling`, `ParselSpelling`, `SelectolaxSpelling`).
    pub dom: D,
    /// Any diagnostics accumulated during code generation (e.g. missing `"py"` target in `extension`).
    pub diagnostics: Vec<Diagnostic>,
}

impl PythonEmitter<PythonDom> {
    /// Creates a new `PythonEmitter` configured with `PythonDom` from `options`.
    pub fn new(options: CodegenOptions) -> Self {
        let dom = PythonDom::from_options(&options);
        Self {
            options,
            dom,
            diagnostics: Vec::new(),
        }
    }
}

pub(crate) fn py_key_type(key_type: Option<&TypeInfo>) -> &'static str {
    match key_type.map(|k| &k.base) {
        Some(BaseType::Int) => "int",
        Some(BaseType::Float) => "float",
        Some(BaseType::Bool) => "bool",
        _ => "str",
    }
}

impl<D: PyDomSpelling> PythonEmitter<D> {
    /// Creates a new `PythonEmitter` with a custom [`PyDomSpelling`] implementation.
    pub fn with_dom(options: CodegenOptions, dom: D) -> Self {
        Self {
            options,
            dom,
            diagnostics: Vec::new(),
        }
    }

    /// Resolves a [`TypeInfo`] into a Python 3.10+ type annotation (`str`, `int`, `list[T]`, `dict[K, V]`, `T | None`).
    pub fn resolve_type(
        &self,
        type_info: &TypeInfo,
        module: &Module,
        builder: &mut ModuleBuilder,
    ) -> String {
        if type_info.is_dict {
            let key_type = py_key_type(type_info.key_type.as_deref());
            let val_type = match type_info.value_type.as_deref() {
                Some(vt) => self.resolve_type(vt, module, builder),
                None => {
                    builder.require_import("from typing import Any");
                    "Any".to_owned()
                }
            };
            let mut res = format!("dict[{key_type}, {val_type}]");
            if type_info.is_array {
                res = format!("list[{res}]");
            }
            if type_info.is_optional {
                res = format!("{res} | None");
            }
            return res;
        }

        let mut base_str = match &type_info.base {
            BaseType::String => "str".to_owned(),
            BaseType::Int => "int".to_owned(),
            BaseType::Float => "float".to_owned(),
            BaseType::Bool => "bool".to_owned(),
            BaseType::Null => "None".to_owned(),
            BaseType::Document => {
                if type_info.is_array {
                    self.dom.document_array_type().to_owned()
                } else {
                    self.dom.document_type().to_owned()
                }
            }
            BaseType::Nested(ref_name) => {
                let pascal = to_pascal_case(ref_name);
                if let Some(target_struct) = module.struct_decl(ref_name) {
                    match target_struct.kind {
                        StructKind::Flat => {
                            return Self::wrap_optional(
                                "list[str]".to_owned(),
                                type_info.is_optional,
                            );
                        }
                        StructKind::List => {
                            return Self::wrap_optional(
                                format!("list[{pascal}Type]"),
                                type_info.is_optional,
                            );
                        }
                        StructKind::Raw if target_struct.split_doc.is_some() => {
                            return Self::wrap_optional(
                                format!("list[{pascal}Type]"),
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
                if let Some(target_json) = module.json_def(ref_name)
                    && target_json.is_array
                {
                    return Self::wrap_optional(
                        format!("list[{pascal}Json]"),
                        type_info.is_optional,
                    );
                }
                format!("{pascal}Json")
            }
            BaseType::Auto | BaseType::DictValue => {
                builder.require_import("from typing import Any");
                "Any".to_owned()
            }
        };

        if type_info.is_array && type_info.base != BaseType::Document {
            base_str = format!("list[{base_str}]");
        }
        Self::wrap_optional(base_str, type_info.is_optional)
    }

    fn wrap_optional(ty: String, is_optional: bool) -> String {
        if is_optional && ty != "None" {
            format!("{ty} | None")
        } else {
            ty
        }
    }

    /// Resolves the return type annotation of `.parse(self)` for a [`StructDecl`].
    pub fn resolve_parse_return_type(&self, struct_decl: &StructDecl) -> String {
        let name = to_pascal_case(&struct_decl.name);
        match struct_decl.kind {
            StructKind::Item | StructKind::Dict | StructKind::Table => format!("{name}Type"),
            StructKind::List => format!("list[{name}Type]"),
            StructKind::Flat => "list[str]".to_owned(),
            StructKind::Raw => {
                if struct_decl.split_doc.is_some() {
                    format!("list[{name}Type]")
                } else {
                    format!("{name}Type")
                }
            }
            StructKind::Rest => "None".to_owned(),
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
            writer.writeln("try:");
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
                    writer.writeln("return");
                } else {
                    wln!(writer, "return {final_var}");
                }
            });
            writer.writeln("except Exception:");
            let fb_str = py_literal(fb_lit);
            writer.indented(|writer| {
                wln!(writer, "return {fb_str}");
            });
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
                writer.writeln("return");
            } else {
                wln!(writer, "return {final_var}");
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

        match &step.op {
            StepOp::LoadInit { name } => {
                let snake = to_snake_case(name);
                wln!(writer, "{nxt} = self._{snake}");
            }
            StepOp::Css { queries } => {
                self.dom
                    .emit_css_select(prv, nxt, queries, next_is_to_bool, builder, writer);
            }
            StepOp::CssAll { queries } => {
                self.dom
                    .emit_css_select_all(prv, nxt, queries, builder, writer);
            }
            StepOp::CssRemove { query } => {
                self.dom
                    .emit_css_remove(prv, nxt, query, is_arr_in, builder, writer);
            }
            StepOp::Xpath { queries } => {
                if self.dom.supports_xpath() {
                    self.dom
                        .emit_xpath_select(prv, nxt, queries, next_is_to_bool, builder, writer);
                } else if writer.is_dry_run() {
                    self.record_unsupported_xpath(step.span, &module.source_file);
                }
            }
            StepOp::XpathAll { queries } => {
                if self.dom.supports_xpath() {
                    self.dom
                        .emit_xpath_select_all(prv, nxt, queries, builder, writer);
                } else if writer.is_dry_run() {
                    self.record_unsupported_xpath(step.span, &module.source_file);
                }
            }
            StepOp::XpathRemove { query } => {
                if self.dom.supports_xpath() {
                    self.dom
                        .emit_xpath_remove(prv, nxt, query, is_arr_in, builder, writer);
                } else if writer.is_dry_run() {
                    self.record_unsupported_xpath(step.span, &module.source_file);
                }
            }
            StepOp::Text => {
                self.dom.emit_text(prv, nxt, is_arr_in, builder, writer);
            }
            StepOp::Raw { mode } => {
                self.dom
                    .emit_raw(prv, nxt, *mode, is_arr_in, builder, writer);
            }
            StepOp::Attr { keys } => {
                self.dom
                    .emit_attr(prv, nxt, keys, is_arr_in, builder, writer);
            }
            StepOp::ToBool => {
                if is_arr_in {
                    wln!(writer, "{nxt} = len({prv}) > 0");
                } else if step.input.base == BaseType::Document {
                    wln!(writer, "{nxt} = {prv} is not None");
                } else {
                    wln!(writer, "{nxt} = bool({prv})");
                }
            }
            StepOp::Trim { chars } => {
                let arg = if chars.is_empty() {
                    String::new()
                } else {
                    py_str(chars)
                };
                if is_arr_out {
                    wln!(writer, "{nxt} = [i.strip({arg}) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {prv}.strip({arg})");
                }
            }
            StepOp::Ltrim { chars } => {
                let arg = if chars.is_empty() {
                    String::new()
                } else {
                    py_str(chars)
                };
                if is_arr_out {
                    wln!(writer, "{nxt} = [i.lstrip({arg}) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {prv}.lstrip({arg})");
                }
            }
            StepOp::Rtrim { chars } => {
                let arg = if chars.is_empty() {
                    String::new()
                } else {
                    py_str(chars)
                };
                if is_arr_out {
                    wln!(writer, "{nxt} = [i.rstrip({arg}) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {prv}.rstrip({arg})");
                }
            }
            StepOp::NormalizeSpace => {
                if is_arr_out {
                    wln!(
                        writer,
                        "{nxt} = [' '.join(i.split()) if i else '' for i in {prv}]"
                    );
                } else {
                    wln!(writer, "{nxt} = ' '.join({prv}.split()) if {prv} else ''");
                }
            }
            StepOp::RmPrefix { substr } => {
                let val = py_str(substr);
                if is_arr_out {
                    wln!(writer, "{nxt} = [i.removeprefix({val}) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {prv}.removeprefix({val})");
                }
            }
            StepOp::RmSuffix { substr } => {
                let val = py_str(substr);
                if is_arr_out {
                    wln!(writer, "{nxt} = [i.removesuffix({val}) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {prv}.removesuffix({val})");
                }
            }
            StepOp::RmPrefixSuffix { substr } => {
                let val = py_str(substr);
                if is_arr_out {
                    wln!(
                        writer,
                        "{nxt} = [i.removeprefix({val}).removesuffix({val}) for i in {prv}]"
                    );
                } else {
                    wln!(
                        writer,
                        "{nxt} = {prv}.removeprefix({val}).removesuffix({val})"
                    );
                }
            }
            StepOp::Fmt { template } => {
                let replaced = if template.matches("{{}}").count() <= 1 {
                    template.replacen("{{}}", "{}", 1)
                } else {
                    template.replace("{{}}", "{0}")
                };
                let tmpl = py_str(&replaced);
                if is_arr_out {
                    wln!(writer, "{nxt} = [{tmpl}.format(i) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {tmpl}.format({prv})");
                }
            }
            StepOp::Repl { old, new } => {
                let old_s = py_str(old);
                let new_s = py_str(new);
                if is_arr_out {
                    wln!(
                        writer,
                        "{nxt} = [i.replace({old_s}, {new_s}) for i in {prv}]"
                    );
                } else {
                    wln!(writer, "{nxt} = {prv}.replace({old_s}, {new_s})");
                }
            }
            StepOp::ReplMap { replacements } => {
                builder.require_std("rt_repl_map", STD_REPL_MAP_CODE);
                let entries = replacements
                    .iter()
                    .map(|(k, v)| format!("{}: {}", py_str(k), py_str(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let dict_lit = format!("{{{entries}}}");
                if is_arr_out {
                    wln!(
                        writer,
                        "{nxt} = [rt_repl_map(i, {dict_lit}) for i in {prv}]"
                    );
                } else {
                    wln!(writer, "{nxt} = rt_repl_map({prv}, {dict_lit})");
                }
            }
            StepOp::Lower => {
                if is_arr_out {
                    wln!(writer, "{nxt} = [i.lower() for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {prv}.lower()");
                }
            }
            StepOp::Upper => {
                if is_arr_out {
                    wln!(writer, "{nxt} = [i.upper() for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = {prv}.upper()");
                }
            }
            StepOp::Split { sep } => {
                let sep_s = py_str(sep);
                wln!(writer, "{nxt} = {prv}.split({sep_s})");
            }
            StepOp::Join { sep } => {
                let sep_s = py_str(sep);
                wln!(writer, "{nxt} = {sep_s}.join({prv})");
            }
            StepOp::Unescape => {
                builder.require_std_with_imports(
                    "rt_unescape_text",
                    STD_UNESCAPE_CODE,
                    ["from html import unescape as html_unescape", "import re"],
                );
                if is_arr_out {
                    wln!(writer, "{nxt} = [rt_unescape_text(i) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = rt_unescape_text({prv})");
                }
            }
            StepOp::Re { pattern } => {
                builder.require_std_with_imports("rt_re_search", STD_RE_SEARCH_CODE, ["import re"]);
                let loc_str = if location.is_empty() {
                    String::new()
                } else {
                    format!(" at {location}")
                };
                let msg = format!(
                    "{}:{}:{} re-match failed{loc_str} pattern={pattern}",
                    module.source_file, step.span.line, step.span.col
                );
                let pat_s = py_str(pattern);
                let msg_s = py_str(&msg);
                if is_arr_out {
                    wln!(
                        writer,
                        "{nxt} = [rt_re_search({pat_s}, i, {msg_s}) for i in {prv}]"
                    );
                } else {
                    wln!(writer, "{nxt} = rt_re_search({pat_s}, {prv}, {msg_s})");
                }
            }
            StepOp::ReAll { pattern } => {
                builder.require_import("import re");
                let pat_s = py_str(pattern);
                wln!(writer, "{nxt}: list[str] = re.findall({pat_s}, {prv})");
            }
            StepOp::ReSub { pattern, repl } => {
                builder.require_import("import re");
                let pat_s = py_str(pattern);
                let repl_s = py_str(repl);
                if is_arr_out {
                    wln!(
                        writer,
                        "{nxt} = [re.sub({pat_s}, {repl_s}, i) for i in {prv}]"
                    );
                } else {
                    wln!(writer, "{nxt} = re.sub({pat_s}, {repl_s}, {prv})");
                }
            }
            StepOp::ReSplit { pattern } => {
                builder.require_import("import re");
                let pat_s = py_str(pattern);
                wln!(writer, "{nxt} = re.split({pat_s}, {prv})");
            }
            StepOp::Index { index } => {
                wln!(writer, "{nxt} = {prv}[{index}]");
            }
            StepOp::Slice { start, end } => {
                wln!(writer, "{nxt} = {prv}[{start}:{end}]");
            }
            StepOp::Len => {
                wln!(writer, "{nxt} = len({prv})");
            }
            StepOp::Unique { keep_order } => {
                if *keep_order {
                    wln!(writer, "{nxt} = list(dict.fromkeys({prv}))");
                } else {
                    wln!(writer, "{nxt} = list(set({prv}))");
                }
            }
            StepOp::ToInt => {
                if is_arr_out {
                    wln!(writer, "{nxt} = [int(i) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = int({prv})");
                }
            }
            StepOp::ToFloat => {
                if is_arr_out {
                    wln!(writer, "{nxt} = [float(i) for i in {prv}]");
                } else {
                    wln!(writer, "{nxt} = float({prv})");
                }
            }
            StepOp::Jsonify {
                schema_name,
                path,
                parsed_path: _,
            } => {
                builder.require_import("import json");
                let raw_expr = if let Some(p) = path {
                    json::require_json_project_std(builder);
                    format!(
                        "rt_resolve_dotpath(json.loads({prv}), {}, is_optional=False)",
                        py_str(p)
                    )
                } else {
                    format!("json.loads({prv})")
                };
                if module.json_def(schema_name).is_some() {
                    json::require_json_project_std(builder);
                    let proj_name = json::python_json_projector_name(schema_name);
                    wln!(writer, "{nxt} = {proj_name}({raw_expr})");
                } else {
                    wln!(writer, "{nxt} = {raw_expr}");
                }
            }
            StepOp::Nested { struct_name } => {
                let pascal = to_pascal_case(struct_name);
                wln!(writer, "{nxt} = {pascal}({prv}).parse()");
            }
            StepOp::Filter { cond } => {
                wln!(writer, "{nxt} = [i for i in {prv} if ");
                writer.indented(|writer| {
                    self.emit_predicate_root(
                        cond,
                        step.span,
                        &module.source_file,
                        &step.input,
                        builder,
                        writer,
                    );
                });
                writer.writeln("]");
            }
            StepOp::Assert { message, cond } => {
                builder.require_std("rt_assert", STD_ASSERT_CODE);
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
                let msg_s = py_str(&msg);
                wln!(writer, "i = {prv}");
                writer.writeln("rt_assert(");
                writer.indented(|writer| {
                    writer.writeln("(");
                    writer.indented(|writer| {
                        self.emit_predicate_root(
                            cond,
                            step.span,
                            &module.source_file,
                            &step.input,
                            builder,
                            writer,
                        );
                    });
                    writer.writeln("),");
                    wln!(writer, "{msg_s},");
                });
                writer.writeln(")");
                wln!(writer, "{nxt} = {prv}");
            }
            StepOp::Match { cond } => {
                builder.require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW_CODE);
                wln!(writer, "i = self._table_match_key({prv})");
                writer.writeln("if not (");
                writer.indented(|writer| {
                    let key_type = TypeInfo::string();
                    self.emit_predicate_root(
                        cond,
                        step.span,
                        &module.source_file,
                        &key_type,
                        builder,
                        writer,
                    );
                });
                writer.writeln("):");
                writer.indented(|writer| {
                    writer.writeln("return RT_UNMATCHED_TABLE_ROW");
                });
                wln!(writer, "{nxt} = self._parse_value({prv})");
            }
            StepOp::ExtensionCall {
                qualified_name,
                bound_input,
                bound_output,
                ..
            } => {
                let in_type = self.resolve_type(bound_input, module, builder);
                let out_type = self.resolve_type(bound_output, module, builder);
                emit_extension_call(
                    module,
                    qualified_name,
                    step.span,
                    &["py"],
                    "'py'",
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
        &mut self,
        expr: &Cond,
        span: Span,
        source_file: &str,
        input_type: &TypeInfo,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        match expr {
            Cond::And(items) => {
                for (idx, item) in items.iter().enumerate() {
                    let join_op = if idx == 0 { None } else { Some("and") };
                    self.emit_predicate_expr(
                        item,
                        join_op,
                        span,
                        source_file,
                        input_type,
                        builder,
                        writer,
                    );
                }
            }
            other => {
                self.emit_predicate_expr(
                    other,
                    None,
                    span,
                    source_file,
                    input_type,
                    builder,
                    writer,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_predicate_expr(
        &mut self,
        expr: &Cond,
        join_op: Option<&str>,
        span: Span,
        source_file: &str,
        input_type: &TypeInfo,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let prefix = match join_op {
            Some("and") => "and ",
            Some("or") => "or ",
            _ => "",
        };

        match expr {
            Cond::Atom(op) => {
                let cond = self.render_pred_op(
                    op,
                    span,
                    source_file,
                    writer.is_dry_run(),
                    input_type,
                    builder,
                );
                wln!(writer, "{prefix}{cond}");
            }
            Cond::Not(inner) => {
                wln!(writer, "{prefix}not (");
                writer.indented(|writer| {
                    self.emit_predicate_root(inner, span, source_file, input_type, builder, writer);
                });
                writer.writeln(")");
            }
            Cond::And(items) => {
                wln!(writer, "{prefix}(");
                writer.indented(|writer| {
                    for (idx, item) in items.iter().enumerate() {
                        let child_join = if idx == 0 { None } else { Some("and") };
                        self.emit_predicate_expr(
                            item,
                            child_join,
                            span,
                            source_file,
                            input_type,
                            builder,
                            writer,
                        );
                    }
                });
                writer.writeln(")");
            }
            Cond::Or(items) => {
                wln!(writer, "{prefix}(");
                writer.indented(|writer| {
                    for (idx, item) in items.iter().enumerate() {
                        let child_join = if idx == 0 { None } else { Some("or") };
                        self.emit_predicate_expr(
                            item,
                            child_join,
                            span,
                            source_file,
                            input_type,
                            builder,
                            writer,
                        );
                    }
                });
                writer.writeln(")");
            }
        }
    }

    fn render_pred_op(
        &mut self,
        op: &CondOp,
        span: Span,
        source_file: &str,
        is_dry_run: bool,
        input_type: &TypeInfo,
        builder: &mut ModuleBuilder,
    ) -> String {
        match op {
            CondOp::Css { query } => self.dom.pred_css(query, builder),
            CondOp::Xpath { query } => {
                if self.dom.supports_xpath() {
                    self.dom.pred_xpath(query, builder)
                } else {
                    if is_dry_run {
                        self.record_unsupported_xpath(span, source_file);
                    }
                    "False".to_owned()
                }
            }
            CondOp::HasAttr { attrs } => self.dom.pred_has_attr(attrs, builder),
            CondOp::AttrEq { name, values } | CondOp::AttrIn { name, values } => {
                self.dom.pred_attr_eq(name, values, builder)
            }
            CondOp::AttrNe { name, values } => self.dom.pred_attr_ne(name, values, builder),
            CondOp::AttrStarts { name, values } => self.dom.pred_attr_starts(name, values, builder),
            CondOp::AttrEnds { name, values } => self.dom.pred_attr_ends(name, values, builder),
            CondOp::AttrContains { name, values } => {
                self.dom.pred_attr_contains(name, values, builder)
            }
            CondOp::AttrRe { name, pattern } => self.dom.pred_attr_re(name, pattern, builder),
            CondOp::TextEq { values } | CondOp::TextIn { values } => {
                self.dom.pred_text_eq(values, builder)
            }
            CondOp::TextNe { values } => self.dom.pred_text_ne(values, builder),
            CondOp::TextStarts { values } => self.dom.pred_text_starts(values, builder),
            CondOp::TextEnds { values } => self.dom.pred_text_ends(values, builder),
            CondOp::TextContains { values } => self.dom.pred_text_contains(values, builder),
            CondOp::TextRe { pattern } => self.dom.pred_text_re(pattern, builder),
            CondOp::Contains { values } => {
                format!("any(v in i for v in {})", py_str_list(values))
            }
            CondOp::Starts { values } => {
                format!("i.startswith({})", py_str_tuple(values))
            }
            CondOp::Ends { values } => {
                format!("i.endswith({})", py_str_tuple(values))
            }
            CondOp::Re { pattern } => {
                builder.require_import("import re");
                format!("bool(re.search({}, i))", py_str(pattern))
            }
            CondOp::ReAny { pattern } => {
                builder.require_import("import re");
                format!("any(bool(re.search({}, j)) for j in i)", py_str(pattern))
            }
            CondOp::ReAll { pattern } => {
                builder.require_import("import re");
                format!("all(bool(re.search({}, j)) for j in i)", py_str(pattern))
            }
            CondOp::Eq { values } | CondOp::In { values } => {
                if values.len() == 1
                    && let CondValue::Int(n) = values[0]
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array)
                {
                    format!("len(i) == {n}")
                } else {
                    let list = values
                        .iter()
                        .map(py_pred_value)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("any(i == v for v in [{list}])")
                }
            }
            CondOp::Ne { values } => {
                if values.len() == 1
                    && let CondValue::Int(n) = values[0]
                    && (!matches!(input_type.base, BaseType::Int | BaseType::Float)
                        || input_type.is_array)
                {
                    format!("len(i) != {n}")
                } else {
                    let list = values
                        .iter()
                        .map(py_pred_value)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("all(i != v for v in [{list}])")
                }
            }
            CondOp::Gt { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("i > {value}")
                } else {
                    format!("len(i) > {value}")
                }
            }
            CondOp::Lt { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("i < {value}")
                } else {
                    format!("len(i) < {value}")
                }
            }
            CondOp::Ge { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("i >= {value}")
                } else {
                    format!("len(i) >= {value}")
                }
            }
            CondOp::Le { value } => {
                if matches!(input_type.base, BaseType::Int | BaseType::Float)
                    && !input_type.is_array
                {
                    format!("i <= {value}")
                } else {
                    format!("len(i) <= {value}")
                }
            }
            CondOp::LenEq { values } => {
                if values.len() == 1 {
                    format!("len(i) == {}", values[0])
                } else {
                    format!("any(len(i) == v for v in {values:?})")
                }
            }
            CondOp::LenNe { values } => {
                if values.len() == 1 {
                    format!("len(i) != {}", values[0])
                } else {
                    format!("all(len(i) != v for v in {values:?})")
                }
            }
            CondOp::LenGt { value } => format!("len(i) > {value}"),
            CondOp::LenLt { value } => format!("len(i) < {value}"),
            CondOp::LenGe { value } => format!("len(i) >= {value}"),
            CondOp::LenLe { value } => format!("len(i) <= {value}"),
            CondOp::LenRange { start, end } => format!("{start} < len(i) < {end}"),
        }
    }

    fn record_unsupported_xpath(&mut self, span: Span, source_file: &str) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::E203,
                format!(
                    "XPath operations ('xpath', 'xpath-all', 'xpath-remove') are not supported by the '{}' backend",
                    self.dom.backend_name()
                ),
                span,
            )
            .with_path(source_file.to_owned()),
        );
    }

    fn collect_separate_runtime_imports(&self, builder: &ModuleBuilder) -> Vec<String> {
        let mut names: IndexSet<String> = IndexSet::new();
        names.insert("RtAssertionError".to_owned());
        names.insert("RtRegexError".to_owned());
        for std_key in builder.std_names() {
            match std_key {
                "RT_UNMATCHED_TABLE_ROW" => {
                    names.insert("RtUnmatchedTableRow".to_owned());
                    names.insert("RT_UNMATCHED_TABLE_ROW".to_owned());
                }
                "rt_assert" => {
                    names.insert("RtAssertionError".to_owned());
                    names.insert("rt_assert".to_owned());
                }
                "rt_re_search" => {
                    names.insert("RtRegexError".to_owned());
                    names.insert("rt_re_search".to_owned());
                }
                STD_JSON_PROJECT_KEY => {
                    for sym in JSON_PROJECT_RUNTIME_SYMBOLS {
                        names.insert((*sym).to_owned());
                    }
                }
                STD_HTTP_TRANSPORT_KEY => {
                    for sym in HTTP_TRANSPORT_RUNTIME_SYMBOLS {
                        names.insert((*sym).to_owned());
                    }
                }
                STD_REST_CORE_KEY => {
                    for sym in REST_CORE_RUNTIME_SYMBOLS {
                        names.insert((*sym).to_owned());
                    }
                }
                other if SHARED_RUNTIME_STD_KEYS.contains(&other) => {
                    names.insert(other.to_owned());
                }
                _ => {}
            }
        }
        for rt_name in builder.runtime_names() {
            names.insert(rt_name.to_owned());
        }
        names.into_iter().collect()
    }
}

impl<D: PyDomSpelling> LanguageEmitter for PythonEmitter<D> {
    fn on_module_start(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer.writeln("# Auto-generated by parsmith. DO NOT EDIT.");
        if let Some(ref doc) = module.doc {
            emit_docstring(doc, writer);
        }
        if module_has_html(module) {
            self.dom.register_imports(builder);
        }
        if module_has_table(module) {
            builder.require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW_CODE);
        }
    }

    fn emit_imports(
        &mut self,
        _module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        writer
            .writeln("from __future__ import annotations")
            .blankline();

        let (mut all_imports, mut typing_symbols) = merge_python_imports(builder.imports());
        let mut add_aux_import = |imp: &str| {
            if let Some(("typing", names)) = parse_simple_from_import(imp) {
                typing_symbols.extend(names.into_iter().map(str::to_owned));
            } else {
                all_imports.insert(imp.to_owned());
            }
        };
        if !self.options.separate_runtime {
            for imp in builder.std_imports() {
                add_aux_import(imp);
            }
            for imp in builder.runtime_imports() {
                add_aux_import(imp);
            }
        } else {
            // Even with separate runtime, inlined DOM-specific std helpers keep their imports
            for (name, def) in builder.std_defs() {
                if !SHARED_RUNTIME_STD_KEYS.contains(&name.as_str()) {
                    for imp in &def.imports {
                        add_aux_import(imp);
                    }
                }
            }
        }

        for imp in &all_imports {
            writer.write_block(imp);
        }

        if !typing_symbols.is_empty() {
            let syms = typing_symbols.into_iter().collect::<Vec<_>>().join(", ");
            wln!(writer, "from typing import {syms}");
        }

        if self.options.separate_runtime {
            let rt_names = self.collect_separate_runtime_imports(builder);
            if !rt_names.is_empty() {
                let joined = rt_names.join(", ");
                wln!(writer, "from {} import {joined}", self.options.runtime_name);
            }
        }

        writer.blankline();
    }

    fn emit_utilities(
        &mut self,
        module: &Module,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if module_has_html(module) {
            self.dom.emit_utilities(writer);
        }

        if self.options.separate_runtime {
            for (name, def) in builder.std_defs() {
                if !SHARED_RUNTIME_STD_KEYS.contains(&name.as_str()) {
                    writer.write_block(&def.code);
                    writer.blankline();
                }
            }
        } else {
            for def in builder.runtime_defs().values() {
                writer.write_block(&def.code);
                writer.blankline();
            }
            for def in builder.std_defs().values() {
                writer.write_block(&def.code);
                writer.blankline();
            }
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
        let name = to_pascal_case(&typedef.name);
        match typedef.struct_kind {
            StructKind::Rest => {}
            StructKind::Flat => {
                builder.require_import("from typing import TypeAlias");
                wln!(writer, "{name}Type: TypeAlias = list[str]");
                writer.blankline();
            }
            StructKind::Dict => {
                builder.require_import("from typing import TypeAlias");
                let val_type = typedef
                    .fields
                    .iter()
                    .find(|f| f.name == "value")
                    .map(|f| self.resolve_type(&f.type_info, module, builder))
                    .unwrap_or_else(|| "str".to_owned());
                wln!(writer, "{name}Type: TypeAlias = dict[str, {val_type}]");
                writer.blankline();
            }
            StructKind::Item | StructKind::List | StructKind::Table | StructKind::Raw => {
                match self.options.models {
                    PythonModels::TypedDict => {
                        builder.require_import("from typing import TypedDict");
                        wln!(writer, "class {name}Type(TypedDict):");
                    }
                    PythonModels::Dataclass => {
                        builder.require_import("from dataclasses import dataclass");
                        writer.writeln("@dataclass(slots=True)");
                        wln!(writer, "class {name}Type:");
                    }
                }
                writer.indented(|writer| {
                    if typedef.fields.is_empty() {
                        writer.writeln("pass");
                    } else {
                        for field in &typedef.fields {
                            let fname = to_snake_case(&field.name);
                            let ftype = self.resolve_type(&field.type_info, module, builder);
                            wln!(writer, "{fname}: {ftype}");
                        }
                    }
                });
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
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        let name = to_pascal_case(&struct_decl.name);
        wln!(writer, "class {name}:");
        writer.indent();
        if let Some(ref doc) = struct_decl.doc {
            emit_docstring(doc, writer);
        }
    }

    fn emit_init(
        &mut self,
        struct_decl: &StructDecl,
        _module: &Module,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if struct_decl.kind == StructKind::Raw {
            writer.writeln("def __init__(self, document: str) -> None:");
            writer.indented(|writer| {
                writer.writeln("self._doc: str = document");
                for init_f in &struct_decl.init_fields {
                    let snake = to_snake_case(&init_f.name);
                    wln!(writer, "self._{snake} = self._init_{snake}(self._doc)");
                }
            });
        } else {
            let arg_ty = self.dom.init_arg_type();
            let extra = self.dom.extra_params();
            wln!(
                writer,
                "def __init__(self, document: {arg_ty}{extra}) -> None:"
            );
            writer.indented(|writer| {
                self.dom.emit_doc_init("self._doc", writer);
                for init_f in &struct_decl.init_fields {
                    let snake = to_snake_case(&init_f.name);
                    wln!(writer, "self._{snake} = self._init_{snake}(self._doc)");
                }
            });
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
        writer.reset_vars();
        let v0 = writer.next_var();
        let snake = to_snake_case(&init_field.name);
        let t_arg = self.resolve_type(&init_field.pipeline.input, module, builder);
        let t_ret = self.resolve_type(&init_field.type_info, module, builder);
        let location = format!(
            "{}.@init.{}",
            to_pascal_case(&struct_decl.name),
            init_field.name
        );

        wln!(writer, "def _init_{snake}(self, {v0}: {t_arg}) -> {t_ret}:");
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
        let t_arg = self.resolve_type(&pipeline.input, module, builder);
        let location = format!("{}.@pre-validate", to_pascal_case(&struct_decl.name));

        wln!(writer, "def _pre_validate(self, {v0}: {t_arg}) -> None:");
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, true, module, builder, writer);
        });
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
        let snake = to_snake_case(&check.name);
        let t_ret = self.resolve_type(&check.pipeline.output, module, builder);
        let location = format!("{}.{}", to_pascal_case(&struct_decl.name), check.name);

        wln!(writer, "def {snake}(self) -> {t_ret}:");
        writer.indented(|writer| {
            wln!(writer, "{v0} = self._doc");
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
        let t_arg = self.resolve_type(&pipeline.input, module, builder);
        let t_ret = self.resolve_type(&pipeline.output, module, builder);
        let location = format!("{}.@split-doc", to_pascal_case(&struct_decl.name));

        wln!(writer, "def _split_doc(self, {v0}: {t_arg}) -> {t_ret}:");
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
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
        let t_arg = self.resolve_type(&pipeline.input, module, builder);
        let t_ret = self.resolve_type(&pipeline.output, module, builder);
        let location = format!("{}.key", to_pascal_case(&struct_decl.name));

        wln!(writer, "def _parse_key(self, {v0}: {t_arg}) -> {t_ret}:");
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
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
        let t_arg = self.resolve_type(&pipeline.input, module, builder);
        let t_ret = self.resolve_type(&pipeline.output, module, builder);
        let location = format!("{}.value", to_pascal_case(&struct_decl.name));

        wln!(writer, "def _parse_value(self, {v0}: {t_arg}) -> {t_ret}:");
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
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
        let t_arg = self.resolve_type(&pipeline.input, module, builder);
        let t_ret = self.resolve_type(&pipeline.output, module, builder);
        let location = format!("{}.@table", to_pascal_case(&struct_decl.name));

        wln!(writer, "def _table_config(self, {v0}: {t_arg}) -> {t_ret}:");
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
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
        let t_arg = self.resolve_type(&pipeline.input, module, builder);
        let t_ret = self.resolve_type(&pipeline.output, module, builder);
        let location = format!("{}.@rows", to_pascal_case(&struct_decl.name));

        wln!(writer, "def _table_rows(self, {v0}: {t_arg}) -> {t_ret}:");
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
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
        let t_arg = self.resolve_type(&pipeline.input, module, builder);
        let t_ret = self.resolve_type(&pipeline.output, module, builder);
        let location = format!("{}.@match", to_pascal_case(&struct_decl.name));

        wln!(
            writer,
            "def _table_match_key(self, {v0}: {t_arg}) -> {t_ret}:"
        );
        writer.indented(|writer| {
            self.emit_pipeline_body(pipeline, &v0, &location, false, module, builder, writer);
        });
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
        let snake = to_snake_case(&field.name);
        let t_arg = self.resolve_type(&field.pipeline.input, module, builder);
        let mut t_ret = self.resolve_type(&field.type_info, module, builder);
        if struct_decl.kind == StructKind::Table {
            builder.require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW_CODE);
            t_ret = format!("{t_ret} | RtUnmatchedTableRow");
        }
        let location = format!("{}.{}", to_pascal_case(&struct_decl.name), field.name);

        wln!(
            writer,
            "def _parse_{snake}(self, {v0}: {t_arg}) -> {t_ret}:"
        );
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
        let t_ret = self.resolve_parse_return_type(struct_decl);
        let use_dataclass = self.options.models == PythonModels::Dataclass;

        wln!(writer, "def parse(self) -> {t_ret}:");
        writer.indented(|writer| {
            if struct_decl.pre_validate.is_some() {
                writer.writeln("self._pre_validate(self._doc)");
            }

            match struct_decl.kind {
                StructKind::Item => {
                    Self::emit_item_return(&name, &struct_decl.fields, "self._doc", use_dataclass, writer);
                }
                StructKind::List => {
                    Self::emit_list_return(&name, &struct_decl.fields, use_dataclass, writer);
                }
                StructKind::Dict => {
                    writer.writeln("return {");
                    writer.indented(|writer| {
                        writer.writeln(
                            "self._parse_key(i): self._parse_value(i) for i in self._split_doc(self._doc)",
                        );
                    });
                    writer.writeln("}");
                }
                StructKind::Flat => {
                    writer.writeln("_result: list[str] = []");
                    for field in &struct_decl.fields {
                        let fn_name = to_snake_case(&field.name);
                        if field.type_info.is_optional {
                            wln!(writer,
                                "_{fn_name} = self._parse_{fn_name}(self._doc)"
                            );
                            wln!(writer, "if _{fn_name} is not None:");
                            writer.indented(|writer| {
                                if field.type_info.is_array {
                                    wln!(writer, "_result.extend(_{fn_name})");
                                } else {
                                    wln!(writer, "_result.append(_{fn_name})");
                                }
                            });
                        } else if field.type_info.is_array {
                            wln!(writer,
                                "_result.extend(self._parse_{fn_name}(self._doc))"
                            );
                        } else {
                            wln!(writer,
                                "_result.append(self._parse_{fn_name}(self._doc))"
                            );
                        }
                    }
                    if struct_decl.keep_order {
                        writer.writeln("return list(dict.fromkeys(_result))");
                    } else {
                        writer.writeln("return list(set(_result))");
                    }
                }
                StructKind::Table => {
                    builder
                        .require_import("from typing import Any")
                        .require_std("RT_UNMATCHED_TABLE_ROW", STD_UNMATCHED_TABLE_ROW_CODE);
                    if !use_dataclass {
                        builder.require_import("from typing import cast");
                    }
                    writer.writeln("_result: dict[str, Any] = {}");
                    writer.writeln("_table = self._table_config(self._doc)");
                    writer.writeln("for _row in self._table_rows(_table):");
                    writer.indented(|writer| {
                        if struct_decl.fields.is_empty() {
                            writer.writeln("pass");
                        } else {
                            for field in &struct_decl.fields {
                                let fn_name = to_snake_case(&field.name);
                                let key_lit = py_str(&fn_name);
                                wln!(writer,
                                    "_{fn_name} = self._parse_{fn_name}(_row)"
                                );
                                wln!(writer,
                                    "if not isinstance(_{fn_name}, RtUnmatchedTableRow) and {key_lit} not in _result:"
                                );
                                writer.indented(|writer| {
                                    wln!(writer,
                                        "_result[{key_lit}] = _{fn_name}"
                                    );
                                    writer.writeln("continue");
                                });
                            }
                        }
                    });
                    if use_dataclass {
                        wln!(writer, "return {name}Type(**_result)");
                    } else {
                        wln!(writer, "return cast({name}Type, _result)");
                    }
                }
                StructKind::Raw => {
                    if struct_decl.split_doc.is_some() {
                        Self::emit_list_return(&name, &struct_decl.fields, use_dataclass, writer);
                    } else {
                        Self::emit_item_return(&name, &struct_decl.fields, "self._doc", use_dataclass, writer);
                    }
                }
                StructKind::Rest => {}
            }
        });
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
        writer.blankline();
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
        let snake = to_snake_case(&func.name);
        let t_ret = self.resolve_type(&func.return_type, module, builder);

        if func.is_raw {
            wln!(writer, "def {snake}(document: str) -> {t_ret}:");
        } else {
            let arg_ty = self.dom.init_arg_type();
            let extra = self.dom.extra_params();
            wln!(writer, "def {snake}(document: {arg_ty}{extra}) -> {t_ret}:");
        }

        writer.indented(|writer| {
            if let Some(ref doc) = func.doc {
                emit_docstring(doc, writer);
            }
            if func.is_raw {
                wln!(writer, "{v0} = document");
            } else {
                self.dom.emit_doc_init(&v0, writer);
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
        writer.blankline();
    }
}

impl<D: PyDomSpelling> PythonEmitter<D> {
    fn emit_item_return(
        name: &str,
        fields: &[FieldDecl],
        doc_expr: &str,
        use_dataclass: bool,
        writer: &mut CodeWriter,
    ) {
        if use_dataclass {
            if fields.is_empty() {
                wln!(writer, "return {name}Type()");
            } else {
                wln!(writer, "return {name}Type(");
                writer.indented(|writer| {
                    for field in fields {
                        let fn_name = to_snake_case(&field.name);
                        wln!(writer, "{fn_name}=self._parse_{fn_name}({doc_expr}),");
                    }
                });
                writer.writeln(")");
            }
        } else if fields.is_empty() {
            writer.writeln("return {}");
        } else {
            writer.writeln("return {");
            writer.indented(|writer| {
                for field in fields {
                    let fn_name = to_snake_case(&field.name);
                    let key_lit = py_str(&fn_name);
                    wln!(writer, "{key_lit}: self._parse_{fn_name}({doc_expr}),");
                }
            });
            writer.writeln("}");
        }
    }

    fn emit_list_return(
        name: &str,
        fields: &[FieldDecl],
        use_dataclass: bool,
        writer: &mut CodeWriter,
    ) {
        if use_dataclass {
            writer.writeln("return [");
            writer.indented(|writer| {
                if fields.is_empty() {
                    wln!(writer, "{name}Type()");
                } else {
                    wln!(writer, "{name}Type(");
                    writer.indented(|writer| {
                        for field in fields {
                            let fn_name = to_snake_case(&field.name);
                            wln!(writer, "{fn_name}=self._parse_{fn_name}(i),");
                        }
                    });
                    writer.writeln(")");
                }
                writer.writeln("for i in self._split_doc(self._doc)");
            });
            writer.writeln("]");
        } else {
            writer.writeln("return [");
            writer.indented(|writer| {
                if fields.is_empty() {
                    writer.writeln("{}");
                } else {
                    writer.writeln("{");
                    writer.indented(|writer| {
                        for field in fields {
                            let fn_name = to_snake_case(&field.name);
                            let key_lit = py_str(&fn_name);
                            wln!(writer, "{key_lit}: self._parse_{fn_name}(i),");
                        }
                    });
                    writer.writeln("}");
                }
                writer.writeln("for i in self._split_doc(self._doc)");
            });
            writer.writeln("]");
        }
    }
}

/// Generates the standalone Python shared runtime module (`<runtime_name>.py`) for `-R / --separate-runtime`.
pub fn emit_python_runtime_module(module: &Module, builder: &ModuleBuilder) -> String {
    let mut writer = CodeWriter::emit();
    writer.writeln("# Auto-generated runtime helpers by parsmith. DO NOT EDIT.");
    writer.writeln("from __future__ import annotations");
    writer.blankline();

    let has_json = builder.has_std_helper(STD_JSON_PROJECT_KEY) || !module.json_defs.is_empty();
    let has_http = builder.has_std_helper(STD_HTTP_TRANSPORT_KEY) || module_uses_http(module);
    let has_fetch = builder.has_std_helper(STD_FETCH_SYNC_KEY)
        || builder.has_std_helper(STD_FETCH_ASYNC_KEY)
        || module_has_fetch(module);
    let has_rest = builder.has_std_helper(STD_REST_CORE_KEY) || module_has_rest(module);

    let mut imports: IndexSet<String> = IndexSet::new();
    imports.insert("from html import unescape as html_unescape".to_owned());
    imports.insert("import re".to_owned());
    for imp in builder.std_imports() {
        imports.insert(imp.to_owned());
    }
    for imp in builder.runtime_imports() {
        imports.insert(imp.to_owned());
    }
    for imp in &imports {
        writer.write_block(imp);
    }

    let mut typing_syms: Vec<&str> = Vec::new();
    if has_json || has_http || has_fetch || has_rest {
        typing_syms.push("Any");
    }
    if has_rest {
        typing_syms.push("Generic");
    }
    if has_http || has_rest {
        typing_syms.push("Literal");
    }
    if has_http {
        typing_syms.push("Protocol");
    }
    if has_rest {
        typing_syms.push("TypeVar");
    }
    if !typing_syms.is_empty() {
        wln!(writer, "from typing import {}", typing_syms.join(", "));
    }
    writer.blankline();

    writer.write_block(&dedent_code_block(STD_ASSERT_CODE));
    writer.blankline();
    writer.write_block(&dedent_code_block(STD_RE_SEARCH_CODE));
    writer.blankline();
    writer.write_block(&dedent_code_block(STD_REPL_MAP_CODE));
    writer.blankline();
    writer.write_block(&dedent_code_block(STD_UNESCAPE_CODE));
    writer.blankline();
    writer.write_block(&dedent_code_block(STD_UNMATCHED_TABLE_ROW_CODE));
    writer.blankline();

    if has_json {
        writer.write_block(&dedent_code_block(STD_JSON_PROJECT_CODE));
        writer.blankline();
    }
    if has_http {
        writer.write_block(&dedent_code_block(STD_HTTP_TRANSPORT_CODE));
        writer.blankline();
    }
    if has_fetch {
        writer.write_block(&dedent_code_block(STD_FETCH_SYNC_CODE));
        writer.blankline();
        writer.write_block(&dedent_code_block(STD_FETCH_ASYNC_CODE));
        writer.blankline();
    }
    if has_rest {
        writer.write_block(&dedent_code_block(STD_REST_CORE_CODE));
        writer.blankline();
        writer.write_block(&dedent_code_block(STD_REST_SYNC_CODE));
        writer.blankline();
        writer.write_block(&dedent_code_block(STD_REST_ASYNC_CODE));
        writer.blankline();
    }

    for def in builder.runtime_defs().values() {
        writer.write_block(&def.code);
        writer.blankline();
    }

    writer.finish()
}

/// Emits Python 3.10+ source files for a compiled [`Module`] and [`CodegenOptions`].
///
/// Returns `(parser_source, optional_runtime_source, builder)` or `Err(diagnostics)`.
pub fn emit_python_module(
    module: &Module,
    options: &CodegenOptions,
) -> Result<(String, Option<String>, ModuleBuilder), Vec<Diagnostic>> {
    let mut emitter = PythonEmitter::new(options.clone());
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
        Some(emit_python_runtime_module(module, &builder))
    } else {
        None
    };
    Ok((parser_code, runtime_code, builder))
}

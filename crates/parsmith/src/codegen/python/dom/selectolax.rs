//! `SelectolaxSpelling` (`selectolax.lexbor.LexborHTMLParser` + `LexborNode`) implementation of [`PyDomSpelling`].

use crate::ast::RawMode;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::python::dom::PyDomSpelling;
use crate::codegen::python::{py_str, py_str_list, py_str_tuple};
use crate::codegen::writer::{CodeWriter, wln};

const STD_SELECT_FIRST: &str = r#"
def rt_select_first(node: LexborNode, *queries: str) -> LexborNode | None:
    for q in queries:
        res = node.css_first(q)
        if res is not None:
            return res
    return None
"#;

const STD_SELECT_ALL_FIRST: &str = r#"
def rt_select_all_first(node: LexborNode, *queries: str) -> list[LexborNode]:
    for q in queries:
        res = node.css(q)
        if res:
            return res
    return []
"#;

const STD_SELECT_REMOVE: &str = r#"
def rt_select_remove(tag: LexborNode, q: str) -> LexborNode:
    for _el in tag.css(q):
        _el.decompose()
    return tag
"#;

/// `selectolax.lexbor` (`selectolax` / `slax`) implementation of [`PyDomSpelling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectolaxSpelling;

impl SelectolaxSpelling {
    /// Creates a new `SelectolaxSpelling`.
    pub const fn new() -> Self {
        Self
    }
}

impl PyDomSpelling for SelectolaxSpelling {
    fn backend_name(&self) -> &'static str {
        "python-selectolax"
    }

    fn supports_xpath(&self) -> bool {
        false
    }

    fn register_imports(&self, builder: &mut ModuleBuilder) {
        builder
            .require_import("from selectolax.lexbor import LexborHTMLParser, LexborNode")
            .require_import("from typing import TypeAlias");
    }

    fn emit_utilities(&self, writer: &mut CodeWriter) {
        writer
            .writeln("RtSelectolaxHtml: TypeAlias = str | LexborHTMLParser | LexborNode")
            .blankline();
    }

    fn document_type(&self) -> &'static str {
        "LexborNode"
    }

    fn document_array_type(&self) -> &'static str {
        "list[LexborNode]"
    }

    fn init_arg_type(&self) -> &'static str {
        "RtSelectolaxHtml"
    }

    fn extra_params(&self) -> &'static str {
        ""
    }

    fn init_from_str_expr(&self) -> &'static str {
        "LexborHTMLParser(document).root"
    }

    fn emit_doc_init(&self, target_var: &str, writer: &mut CodeWriter) {
        writer.writeln("if isinstance(document, str):");
        writer.indented(|writer| {
            writer.writeln("_root = LexborHTMLParser(document).root");
            writer.writeln("assert _root is not None");
            wln!(writer, "{target_var}: LexborNode = _root");
        });
        writer.writeln("elif isinstance(document, LexborHTMLParser):");
        writer.indented(|writer| {
            writer.writeln("_root = document.root");
            writer.writeln("assert _root is not None");
            wln!(writer, "{target_var} = _root");
        });
        writer.writeln("else:");
        writer.indented(|writer| {
            wln!(writer, "{target_var} = document");
        });
    }

    fn emit_css_select(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        next_is_to_bool: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if queries.len() == 1 {
            let q = py_str(&queries[0]);
            wln!(writer, "{nxt} = {prv}.css_first({q})");
        } else {
            builder.require_std("rt_select_first", STD_SELECT_FIRST);
            let args = queries
                .iter()
                .map(|q| py_str(q))
                .collect::<Vec<_>>()
                .join(", ");
            wln!(writer, "{nxt} = rt_select_first({prv}, {args})");
        }
        if !next_is_to_bool {
            wln!(writer, "assert {nxt} is not None");
        }
    }

    fn emit_css_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if queries.len() == 1 {
            let q = py_str(&queries[0]);
            wln!(writer, "{nxt} = {prv}.css({q})");
        } else {
            builder.require_std("rt_select_all_first", STD_SELECT_ALL_FIRST);
            let args = queries
                .iter()
                .map(|q| py_str(q))
                .collect::<Vec<_>>()
                .join(", ");
            wln!(writer, "{nxt} = rt_select_all_first({prv}, {args})");
        }
    }

    fn emit_css_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_std("rt_select_remove", STD_SELECT_REMOVE);
        let q = py_str(query);
        if is_array {
            wln!(writer, "{nxt} = [rt_select_remove(i, {q}) for i in {prv}]");
        } else {
            wln!(writer, "{nxt} = rt_select_remove({prv}, {q})");
        }
    }

    fn emit_text(
        &self,
        prv: &str,
        nxt: &str,
        is_array: bool,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if is_array {
            wln!(writer, "{nxt} = [i.text() for i in {prv}]");
        } else {
            wln!(writer, "{nxt} = {prv}.text()");
        }
    }

    fn emit_raw(
        &self,
        prv: &str,
        nxt: &str,
        mode: RawMode,
        is_array: bool,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        match (mode, is_array) {
            (RawMode::Inner, true) => {
                wln!(writer, "{nxt} = [i.inner_html or '' for i in {prv}]");
            }
            (RawMode::Inner, false) => {
                wln!(writer, "{nxt} = {prv}.inner_html or ''");
            }
            (RawMode::Outer, true) => {
                wln!(writer, "{nxt} = [i.html or '' for i in {prv}]");
            }
            (RawMode::Outer, false) => {
                wln!(writer, "{nxt} = {prv}.html or ''");
            }
        }
    }

    fn emit_attr(
        &self,
        prv: &str,
        nxt: &str,
        keys: &[String],
        is_array: bool,
        _builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if !is_array {
            if keys.len() == 1 {
                let k = py_str(&keys[0]);
                wln!(writer, "{nxt} = {prv}.attributes[{k}] or ''");
            } else {
                let ks = py_str_list(keys);
                wln!(
                    writer,
                    "{nxt} = [{prv}.attributes[k] or '' for k in {ks} if {prv}.attributes.get(k)]"
                );
            }
        } else if keys.len() == 1 {
            let k = py_str(&keys[0]);
            wln!(writer, "{nxt} = [e.attributes[{k}] or '' for e in {prv}]");
        } else {
            let ks = py_str_list(keys);
            wln!(
                writer,
                "{nxt} = [e.attributes[k] or '' for e in {prv} for k in {ks} if e.attributes.get(k)]"
            );
        }
    }

    fn pred_css(&self, query: &str, _builder: &mut ModuleBuilder) -> String {
        format!("i.css_first({}) is not None", py_str(query))
    }

    fn pred_has_attr(&self, attrs: &[String], _builder: &mut ModuleBuilder) -> String {
        if attrs.len() == 1 {
            format!("{} in i.attributes", py_str(&attrs[0]))
        } else {
            format!(
                "any(attr in i.attributes for attr in {})",
                py_str_list(attrs)
            )
        }
    }

    fn pred_attr_eq(&self, name: &str, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!(
                "({k} in i.attributes) and (i.attributes.get({k}) or '') == {}",
                py_str(&values[0])
            )
        } else {
            format!(
                "({k} in i.attributes) and (i.attributes.get({k}) or '') in {}",
                py_str_list(values)
            )
        }
    }

    fn pred_attr_ne(&self, name: &str, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!(
                "({k} in i.attributes) and (i.attributes.get({k}) or '') != {}",
                py_str(&values[0])
            )
        } else {
            format!(
                "({k} in i.attributes) and (i.attributes.get({k}) or '') not in {}",
                py_str_list(values)
            )
        }
    }

    fn pred_attr_starts(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!(
                "(i.attributes.get({k}) or '').startswith({})",
                py_str(&values[0])
            )
        } else {
            format!(
                "(i.attributes.get({k}) or '').startswith({})",
                py_str_tuple(values)
            )
        }
    }

    fn pred_attr_ends(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!(
                "(i.attributes.get({k}) or '').endswith({})",
                py_str(&values[0])
            )
        } else {
            format!(
                "(i.attributes.get({k}) or '').endswith({})",
                py_str_tuple(values)
            )
        }
    }

    fn pred_attr_contains(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!("{} in (i.attributes.get({k}) or '')", py_str(&values[0]))
        } else {
            format!(
                "any(v in (i.attributes.get({k}) or '') for v in {})",
                py_str_list(values)
            )
        }
    }

    fn pred_attr_re(&self, name: &str, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        format!(
            "bool(re.search({}, (i.attributes.get({}) or '')))",
            py_str(pattern),
            py_str(name)
        )
    }

    fn pred_text_eq(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("i.text() == {}", py_str(&values[0]))
        } else {
            format!("i.text() in {}", py_str_list(values))
        }
    }

    fn pred_text_ne(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("i.text() != {}", py_str(&values[0]))
        } else {
            format!("i.text() not in {}", py_str_list(values))
        }
    }

    fn pred_text_starts(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("i.text().startswith({})", py_str(&values[0]))
        } else {
            format!("i.text().startswith({})", py_str_tuple(values))
        }
    }

    fn pred_text_ends(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("i.text().endswith({})", py_str(&values[0]))
        } else {
            format!("i.text().endswith({})", py_str_tuple(values))
        }
    }

    fn pred_text_contains(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("{} in i.text()", py_str(&values[0]))
        } else {
            format!("any(v in i.text() for v in {})", py_str_list(values))
        }
    }

    fn pred_text_re(&self, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        format!("bool(re.search({}, i.text()))", py_str(pattern))
    }
}

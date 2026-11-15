//! `LxmlSpelling` (`lxml.html.HtmlElement`) implementation of [`PyDomSpelling`].

use crate::ast::RawMode;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::python::dom::PyDomSpelling;
use crate::codegen::python::{py_str, py_str_list, py_str_tuple};
use crate::codegen::writer::{CodeWriter, wln};

const STD_SELECT_FIRST: &str = r#"
def rt_select_first(el: HtmlElement, *queries: str) -> HtmlElement | None:
    for q in queries:
        res = el.cssselect(q)
        if res:
            return res[0]
    return None
"#;

const STD_SELECT_ALL_FIRST: &str = r#"
def rt_select_all_first(el: HtmlElement, *queries: str) -> list[HtmlElement]:
    for q in queries:
        res: list[HtmlElement] = el.cssselect(q)
        if res:
            return res
    return []
"#;

const STD_SELECT_REMOVE: &str = r#"
def rt_select_remove(tag: HtmlElement, q: str) -> HtmlElement:
    for _el in tag.cssselect(q):
        if _el.getparent() is not None:
            _el.drop_tree()
    return tag
"#;

const STD_XPATH_FIRST: &str = r#"
def rt_xpath_first(el: HtmlElement, *queries: str) -> HtmlElement | None:
    for q in queries:
        res = el.xpath(q)
        if res:
            return res[0]
    return None
"#;

const STD_XPATH_ALL_FIRST: &str = r#"
def rt_xpath_all_first(el: HtmlElement, *queries: str) -> list[HtmlElement]:
    for q in queries:
        res: list[HtmlElement] = el.xpath(q)
        if res:
            return res
    return []
"#;

const STD_XPATH_REMOVE: &str = r#"
def rt_xpath_remove(tag: HtmlElement, q: str) -> HtmlElement:
    for _el in tag.xpath(q):
        if _el.getparent() is not None:
            _el.drop_tree()
    return tag
"#;

const STD_INNER_HTML: &str = r#"
def rt_inner_html(el: HtmlElement) -> str:
    parts: list[str] = [el.text or ""]
    for child in el.iterchildren():
        parts.append(html.tostring(child, encoding="unicode"))
    return "".join(parts)
"#;

/// `lxml.html` (`lxml`) implementation of [`PyDomSpelling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LxmlSpelling;

impl LxmlSpelling {
    /// Creates a new `LxmlSpelling`.
    pub const fn new() -> Self {
        Self
    }
}

impl PyDomSpelling for LxmlSpelling {
    fn backend_name(&self) -> &'static str {
        "python-lxml"
    }

    fn supports_xpath(&self) -> bool {
        true
    }

    fn register_imports(&self, builder: &mut ModuleBuilder) {
        builder
            .require_import("from lxml import html")
            .require_import("from lxml.html import HtmlElement")
            .require_import("from typing import TypeAlias");
    }

    fn emit_utilities(&self, writer: &mut CodeWriter) {
        writer
            .writeln("RT_FALLBACK_HTML_STR = \"<html><body></body></html>\"")
            .writeln("RtLxmlHtml: TypeAlias = str | HtmlElement")
            .blankline();
    }

    fn document_type(&self) -> &'static str {
        "HtmlElement"
    }

    fn document_array_type(&self) -> &'static str {
        "list[HtmlElement]"
    }

    fn init_arg_type(&self) -> &'static str {
        "RtLxmlHtml"
    }

    fn extra_params(&self) -> &'static str {
        ""
    }

    fn init_from_str_expr(&self) -> &'static str {
        "html.fromstring(document.strip() or RT_FALLBACK_HTML_STR)"
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
            if next_is_to_bool {
                wln!(
                    writer,
                    "{nxt}: HtmlElement | None = _m[0] if (_m := {prv}.cssselect({q})) else None"
                );
            } else {
                wln!(writer, "{nxt}: HtmlElement = {prv}.cssselect({q})[0]");
            }
        } else {
            builder.require_std("rt_select_first", STD_SELECT_FIRST);
            let args = queries
                .iter()
                .map(|q| py_str(q))
                .collect::<Vec<_>>()
                .join(", ");
            wln!(writer, "{nxt} = rt_select_first({prv}, {args})");
            if !next_is_to_bool {
                wln!(writer, "assert {nxt} is not None");
            }
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
            wln!(writer, "{nxt}: list[HtmlElement] = {prv}.cssselect({q})");
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

    fn emit_xpath_select(
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
            if next_is_to_bool {
                wln!(
                    writer,
                    "{nxt}: HtmlElement | None = _m[0] if (_m := {prv}.xpath({q})) else None"
                );
            } else {
                wln!(writer, "{nxt}: HtmlElement = {prv}.xpath({q})[0]");
            }
        } else {
            builder.require_std("rt_xpath_first", STD_XPATH_FIRST);
            let args = queries
                .iter()
                .map(|q| py_str(q))
                .collect::<Vec<_>>()
                .join(", ");
            wln!(writer, "{nxt} = rt_xpath_first({prv}, {args})");
            if !next_is_to_bool {
                wln!(writer, "assert {nxt} is not None");
            }
        }
    }

    fn emit_xpath_select_all(
        &self,
        prv: &str,
        nxt: &str,
        queries: &[String],
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        if queries.len() == 1 {
            let q = py_str(&queries[0]);
            wln!(writer, "{nxt}: list[HtmlElement] = {prv}.xpath({q})");
        } else {
            builder.require_std("rt_xpath_all_first", STD_XPATH_ALL_FIRST);
            let args = queries
                .iter()
                .map(|q| py_str(q))
                .collect::<Vec<_>>()
                .join(", ");
            wln!(writer, "{nxt} = rt_xpath_all_first({prv}, {args})");
        }
    }

    fn emit_xpath_remove(
        &self,
        prv: &str,
        nxt: &str,
        query: &str,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        builder.require_std("rt_xpath_remove", STD_XPATH_REMOVE);
        let q = py_str(query);
        if is_array {
            wln!(writer, "{nxt} = [rt_xpath_remove(i, {q}) for i in {prv}]");
        } else {
            wln!(writer, "{nxt} = rt_xpath_remove({prv}, {q})");
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
            wln!(
                writer,
                "{nxt}: list[str] = [i.text_content() for i in {prv}]"
            );
        } else {
            wln!(writer, "{nxt}: str = {prv}.text_content()");
        }
    }

    fn emit_raw(
        &self,
        prv: &str,
        nxt: &str,
        mode: RawMode,
        is_array: bool,
        builder: &mut ModuleBuilder,
        writer: &mut CodeWriter,
    ) {
        match (mode, is_array) {
            (RawMode::Inner, true) => {
                builder.require_std("rt_inner_html", STD_INNER_HTML);
                wln!(writer, "{nxt} = [rt_inner_html(i) for i in {prv}]");
            }
            (RawMode::Inner, false) => {
                builder.require_std("rt_inner_html", STD_INNER_HTML);
                wln!(writer, "{nxt} = rt_inner_html({prv})");
            }
            (RawMode::Outer, true) => {
                wln!(
                    writer,
                    "{nxt}: list[str] = [html.tostring(i, encoding='unicode') for i in {prv}]"
                );
            }
            (RawMode::Outer, false) => {
                wln!(
                    writer,
                    "{nxt}: str = html.tostring({prv}, encoding='unicode')"
                );
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
                wln!(writer, "{nxt}: str = {prv}.attrib[{k}]");
            } else {
                let ks = py_str_list(keys);
                wln!(
                    writer,
                    "{nxt}: list[str] = [{prv}.attrib[k] for k in {ks} if {prv}.get(k)]"
                );
            }
        } else if keys.len() == 1 {
            let k = py_str(&keys[0]);
            wln!(writer, "{nxt}: list[str] = [i.attrib[{k}] for i in {prv}]");
        } else {
            let ks = py_str_list(keys);
            wln!(
                writer,
                "{nxt}: list[str] = [i.attrib[k] for i in {prv} for k in {ks} if i.get(k)]"
            );
        }
    }

    fn pred_css(&self, query: &str, _builder: &mut ModuleBuilder) -> String {
        format!("bool(i.cssselect({}))", py_str(query))
    }

    fn pred_xpath(&self, query: &str, _builder: &mut ModuleBuilder) -> String {
        format!("bool(i.xpath({}))", py_str(query))
    }

    fn pred_has_attr(&self, attrs: &[String], _builder: &mut ModuleBuilder) -> String {
        if attrs.len() == 1 {
            format!("{} in i.attrib", py_str(&attrs[0]))
        } else {
            format!("any(attr in i.attrib for attr in {})", py_str_list(attrs))
        }
    }

    fn pred_attr_eq(&self, name: &str, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!("i.get({k}, '') == {}", py_str(&values[0]))
        } else {
            format!("i.get({k}, '') in {}", py_str_list(values))
        }
    }

    fn pred_attr_ne(&self, name: &str, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!(
                "({k} in i.attrib) and i.get({k}, '') != {}",
                py_str(&values[0])
            )
        } else {
            format!(
                "({k} in i.attrib) and i.get({k}, '') not in {}",
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
        let arg = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("i.get({k}, '').startswith({arg})")
    }

    fn pred_attr_ends(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        let arg = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("i.get({k}, '').endswith({arg})")
    }

    fn pred_attr_contains(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!("{} in i.get({k}, '')", py_str(&values[0]))
        } else {
            format!("any(v in i.get({k}, '') for v in {})", py_str_list(values))
        }
    }

    fn pred_attr_re(&self, name: &str, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        format!(
            "bool(re.search({}, i.get({}, '')))",
            py_str(pattern),
            py_str(name)
        )
    }

    fn pred_text_eq(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("i.text_content() == {}", py_str(&values[0]))
        } else {
            format!("i.text_content() in {}", py_str_list(values))
        }
    }

    fn pred_text_ne(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("i.text_content() != {}", py_str(&values[0]))
        } else {
            format!("i.text_content() not in {}", py_str_list(values))
        }
    }

    fn pred_text_starts(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let arg = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("i.text_content().startswith({arg})")
    }

    fn pred_text_ends(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let arg = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("i.text_content().endswith({arg})")
    }

    fn pred_text_contains(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!("{} in i.text_content()", py_str(&values[0]))
        } else {
            format!(
                "any(v in i.text_content() for v in {})",
                py_str_list(values)
            )
        }
    }

    fn pred_text_re(&self, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        format!("bool(re.search({}, i.text_content()))", py_str(pattern))
    }
}

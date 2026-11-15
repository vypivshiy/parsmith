//! `ParselSpelling` (`parsel.Selector` + `SelectorList`) implementation of [`PyDomSpelling`].

use crate::ast::RawMode;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::python::dom::PyDomSpelling;
use crate::codegen::python::{py_str, py_str_list, py_str_tuple};
use crate::codegen::writer::{CodeWriter, wln};

const STD_SELECT_FIRST: &str = r#"
def rt_select_first(sel: Selector, *queries: str) -> Selector | None:
    for q in queries:
        res = sel.css(q)
        if res:
            return res[0]
    return None
"#;

const STD_XPATH_FIRST: &str = r#"
def rt_xpath_first(sel: Selector, *queries: str) -> Selector | None:
    for q in queries:
        res = sel.xpath(q)
        if res:
            return res[0]
    return None
"#;

const STD_SELECT_ALL_FIRST: &str = r#"
def rt_select_all_first(sel: Selector, *queries: str) -> SelectorList[Selector]:
    for q in queries:
        res = sel.css(q)
        if res:
            return res
    return SelectorList()
"#;

const STD_XPATH_ALL_FIRST: &str = r#"
def rt_xpath_all_first(sel: Selector, *queries: str) -> SelectorList[Selector]:
    for q in queries:
        res = sel.xpath(q)
        if res:
            return res
    return SelectorList()
"#;

const STD_SELECT_REMOVE: &str = r#"
def rt_select_remove(tag: Selector, q: str) -> Selector:
    tag.css(q).drop()
    return tag
"#;

const STD_XPATH_REMOVE: &str = r#"
def rt_xpath_remove(tag: Selector, q: str) -> Selector:
    tag.xpath(q).drop()
    return tag
"#;

/// `parsel` (`Selector` / `SelectorList`) implementation of [`PyDomSpelling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParselSpelling;

impl ParselSpelling {
    /// Creates a new `ParselSpelling`.
    pub const fn new() -> Self {
        Self
    }
}

impl PyDomSpelling for ParselSpelling {
    fn backend_name(&self) -> &'static str {
        "python-parsel"
    }

    fn supports_xpath(&self) -> bool {
        true
    }

    fn register_imports(&self, builder: &mut ModuleBuilder) {
        builder
            .require_import("from parsel import Selector, SelectorList")
            .require_import("from typing import TypeAlias");
    }

    fn emit_utilities(&self, writer: &mut CodeWriter) {
        writer
            .writeln("RtParselHtml: TypeAlias = str | Selector | SelectorList[Selector]")
            .blankline();
    }

    fn document_type(&self) -> &'static str {
        "Selector"
    }

    fn document_array_type(&self) -> &'static str {
        "SelectorList[Selector]"
    }

    fn init_arg_type(&self) -> &'static str {
        "RtParselHtml"
    }

    fn extra_params(&self) -> &'static str {
        ""
    }

    fn init_from_str_expr(&self) -> &'static str {
        "Selector(document)"
    }

    fn emit_doc_init(&self, target_var: &str, writer: &mut CodeWriter) {
        writer.writeln("if isinstance(document, str):");
        writer.indented(|writer| {
            wln!(writer, "{target_var}: Selector = Selector(document)");
        });
        writer.writeln("elif isinstance(document, SelectorList):");
        writer.indented(|writer| {
            wln!(writer, "{target_var} = document[0]");
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
            if next_is_to_bool {
                wln!(writer, "{nxt} = _m[0] if (_m := {prv}.css({q})) else None");
            } else {
                wln!(writer, "{nxt} = {prv}.css({q})[0]");
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
            wln!(
                writer,
                "{nxt} = SelectorList([rt_select_remove(i, {q}) for i in {prv}])"
            );
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
                    "{nxt} = _m[0] if (_m := {prv}.xpath({q})) else None"
                );
            } else {
                wln!(writer, "{nxt} = {prv}.xpath({q})[0]");
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
            wln!(writer, "{nxt} = {prv}.xpath({q})");
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
            wln!(
                writer,
                "{nxt} = SelectorList([rt_xpath_remove(i, {q}) for i in {prv}])"
            );
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
                "{nxt} = [''.join(i.xpath('.//text()').getall()) for i in {prv}]"
            );
        } else {
            wln!(writer, "{nxt} = ''.join({prv}.xpath('.//text()').getall())");
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
                wln!(
                    writer,
                    "{nxt} = [''.join(i.xpath('node()').getall()) for i in {prv}]"
                );
            }
            (RawMode::Inner, false) => {
                wln!(writer, "{nxt} = ''.join({prv}.xpath('node()').getall())");
            }
            (RawMode::Outer, true) => {
                wln!(writer, "{nxt} = {prv}.getall()");
            }
            (RawMode::Outer, false) => {
                wln!(writer, "{nxt} = str({prv}.get() or '')");
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
                wln!(writer, "{nxt} = {prv}.attrib[{k}]");
            } else {
                let ks = py_str_list(keys);
                wln!(
                    writer,
                    "{nxt} = [{prv}.attrib[k] for k in {ks} if {prv}.attrib.get(k)]"
                );
            }
        } else if keys.len() == 1 {
            let k = py_str(&keys[0]);
            wln!(writer, "{nxt} = [e.attrib[{k}] for e in {prv}]");
        } else {
            let ks = py_str_list(keys);
            wln!(
                writer,
                "{nxt} = [e.attrib[k] for e in {prv} for k in {ks} if e.attrib.get(k)]"
            );
        }
    }

    fn pred_css(&self, query: &str, _builder: &mut ModuleBuilder) -> String {
        format!("bool(i.css({}))", py_str(query))
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
            format!("i.attrib.get({k}, '') == {}", py_str(&values[0]))
        } else {
            format!("i.attrib.get({k}, '') in {}", py_str_list(values))
        }
    }

    fn pred_attr_ne(&self, name: &str, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!(
                "({k} in i.attrib) and i.attrib.get({k}, '') != {}",
                py_str(&values[0])
            )
        } else {
            format!(
                "({k} in i.attrib) and i.attrib.get({k}, '') not in {}",
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
        let v_or_tuple = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("i.attrib.get({k}, '').startswith({v_or_tuple})")
    }

    fn pred_attr_ends(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        let v_or_tuple = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("i.attrib.get({k}, '').endswith({v_or_tuple})")
    }

    fn pred_attr_contains(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        if values.len() == 1 {
            format!("{} in i.attrib.get({k}, '')", py_str(&values[0]))
        } else {
            format!(
                "any(v in i.attrib.get({k}, '') for v in {})",
                py_str_list(values)
            )
        }
    }

    fn pred_attr_re(&self, name: &str, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        format!(
            "bool(re.search({}, i.attrib.get({}, '')))",
            py_str(pattern),
            py_str(name)
        )
    }

    fn pred_text_eq(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!(
                "''.join(i.xpath('.//text()').getall()) == {}",
                py_str(&values[0])
            )
        } else {
            format!(
                "''.join(i.xpath('.//text()').getall()) in {}",
                py_str_list(values)
            )
        }
    }

    fn pred_text_ne(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!(
                "''.join(i.xpath('.//text()').getall()) != {}",
                py_str(&values[0])
            )
        } else {
            format!(
                "''.join(i.xpath('.//text()').getall()) not in {}",
                py_str_list(values)
            )
        }
    }

    fn pred_text_starts(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let v_or_tuple = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("''.join(i.xpath('.//text()').getall()).startswith({v_or_tuple})")
    }

    fn pred_text_ends(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let v_or_tuple = if values.len() == 1 {
            py_str(&values[0])
        } else {
            py_str_tuple(values)
        };
        format!("''.join(i.xpath('.//text()').getall()).endswith({v_or_tuple})")
    }

    fn pred_text_contains(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        if values.len() == 1 {
            format!(
                "{} in ''.join(i.xpath('.//text()').getall())",
                py_str(&values[0])
            )
        } else {
            format!(
                "any(v in ''.join(i.xpath('.//text()').getall()) for v in {})",
                py_str_list(values)
            )
        }
    }

    fn pred_text_re(&self, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        format!(
            "bool(re.search({}, ''.join(i.xpath('.//text()').getall())))",
            py_str(pattern)
        )
    }
}

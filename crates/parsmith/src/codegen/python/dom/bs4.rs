//! `Bs4Spelling` (`BeautifulSoup` + `Tag`) implementation of [`PyDomSpelling`].

use crate::ast::RawMode;
use crate::codegen::Bs4Parser;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::python::dom::PyDomSpelling;
use crate::codegen::python::{py_str, py_str_list, py_str_tuple};
use crate::codegen::writer::{CodeWriter, wln};

const STD_SELECT_FIRST: &str = r#"
def rt_select_first(tag: BeautifulSoup | Tag, *queries: str) -> Tag | None:
    for q in queries:
        t = tag.select_one(q)
        if t is not None:
            return t
    return None
"#;

const STD_SELECT_ALL_FIRST: &str = r#"
def rt_select_all_first(tag: BeautifulSoup | Tag, *queries: str) -> list[Tag]:
    for q in queries:
        result = tag.select(q)
        if result:
            return list(result)
    return []
"#;

const STD_SELECT_REMOVE: &str = r#"
def rt_select_remove(tag: BeautifulSoup | Tag, q: str) -> BeautifulSoup | Tag:
    for _el in tag.select(q):
        _el.decompose()
    return tag
"#;

/// `BeautifulSoup4` (`bs4`) implementation of [`PyDomSpelling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bs4Spelling {
    /// Configured `bs4` HTML parser engine (`lxml`, `html.parser`, or `html5lib`).
    pub parser: Bs4Parser,
}

impl Bs4Spelling {
    /// Creates a new `Bs4Spelling` with the given `bs4` parser engine.
    pub const fn new(parser: Bs4Parser) -> Self {
        Self { parser }
    }
}

impl Default for Bs4Spelling {
    fn default() -> Self {
        Self::new(Bs4Parser::Lxml)
    }
}

impl PyDomSpelling for Bs4Spelling {
    fn backend_name(&self) -> &'static str {
        "python-bs4"
    }

    fn supports_xpath(&self) -> bool {
        false
    }

    fn register_imports(&self, builder: &mut ModuleBuilder) {
        builder
            .require_import("from bs4 import BeautifulSoup, Tag")
            .require_import("from typing import Literal, TypeAlias");
    }

    fn emit_utilities(&self, writer: &mut CodeWriter) {
        wln!(
            writer,
            "RT_BS4_FEATURES: Literal[\"html.parser\", \"lxml\", \"html5lib\"] = {}",
            py_str(self.parser.as_str())
        )
        .writeln("RtBs4Html: TypeAlias = str | BeautifulSoup | Tag")
        .blankline();
    }

    fn document_type(&self) -> &'static str {
        "BeautifulSoup | Tag"
    }

    fn document_array_type(&self) -> &'static str {
        "list[Tag]"
    }

    fn init_arg_type(&self) -> &'static str {
        "RtBs4Html"
    }

    fn extra_params(&self) -> &'static str {
        ", *, features: Literal[\"html.parser\", \"lxml\", \"html5lib\"] = RT_BS4_FEATURES"
    }

    fn init_from_str_expr(&self) -> &'static str {
        "BeautifulSoup(document, features=features)"
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
            wln!(writer, "{nxt} = {prv}.select_one({q})");
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
            wln!(writer, "{nxt} = {prv}.select({q})");
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
            wln!(writer, "{nxt} = [i.text for i in {prv}]");
        } else {
            wln!(writer, "{nxt} = {prv}.text");
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
                wln!(writer, "{nxt} = [i.decode_contents() for i in {prv}]");
            }
            (RawMode::Inner, false) => {
                wln!(writer, "{nxt} = {prv}.decode_contents()");
            }
            (RawMode::Outer, true) => {
                wln!(writer, "{nxt} = [str(i) for i in {prv}]");
            }
            (RawMode::Outer, false) => {
                wln!(writer, "{nxt} = str({prv})");
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
                wln!(
                    writer,
                    "{nxt} = _v if isinstance(_v := {prv}[{k}], str) else ' '.join(_v)"
                );
            } else {
                let ks = py_str_list(keys);
                wln!(
                    writer,
                    "{nxt} = [(_v if isinstance(_v := {prv}[k], str) else ' '.join(_v)) for k in {ks} if {prv}.get(k)]"
                );
            }
        } else if keys.len() == 1 {
            let k = py_str(&keys[0]);
            wln!(
                writer,
                "{nxt} = [(_v if isinstance(_v := i[{k}], str) else ' '.join(_v)) for i in {prv}]"
            );
        } else {
            let ks = py_str_list(keys);
            wln!(
                writer,
                "{nxt} = [(_v if isinstance(_v := i[k], str) else ' '.join(_v)) for i in {prv} for k in {ks} if i.get(k)]"
            );
        }
    }

    fn pred_css(&self, query: &str, _builder: &mut ModuleBuilder) -> String {
        let q = py_str(query);
        format!("i.select_one({q}) is not None")
    }

    fn pred_has_attr(&self, attrs: &[String], _builder: &mut ModuleBuilder) -> String {
        if attrs.len() == 1 {
            format!("bool(i.get({}, False))", py_str(&attrs[0]))
        } else {
            format!("any(bool(i.get(k, False)) for k in {})", py_str_list(attrs))
        }
    }

    fn pred_attr_eq(&self, name: &str, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let k = py_str(name);
        let vals = py_str_list(values);
        format!(
            "bool(i.get({k})) and any(v == ' '.join(s for s in i.get_attribute_list({k}) if s is not None) for v in {vals})"
        )
    }

    fn pred_attr_ne(&self, name: &str, values: &[String], _builder: &mut ModuleBuilder) -> String {
        let k = py_str(name);
        let vals = py_str_list(values);
        format!(
            "bool(i.get({k})) and all(v != ' '.join(s for s in i.get_attribute_list({k}) if s is not None) for v in {vals})"
        )
    }

    fn pred_attr_starts(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        let vals = py_str_list(values);
        format!(
            "bool(i.get({k})) and any(' '.join(s for s in i.get_attribute_list({k}) if s is not None).startswith(v) for v in {vals})"
        )
    }

    fn pred_attr_ends(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        let vals = py_str_list(values);
        format!(
            "bool(i.get({k})) and any(' '.join(s for s in i.get_attribute_list({k}) if s is not None).endswith(v) for v in {vals})"
        )
    }

    fn pred_attr_contains(
        &self,
        name: &str,
        values: &[String],
        _builder: &mut ModuleBuilder,
    ) -> String {
        let k = py_str(name);
        let vals = py_str_list(values);
        format!(
            "bool(i.get({k})) and any(v in ' '.join(s for s in i.get_attribute_list({k}) if s is not None) for v in {vals})"
        )
    }

    fn pred_attr_re(&self, name: &str, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        let k = py_str(name);
        let pat = py_str(pattern);
        format!(
            "bool(i.get({k})) and bool(re.search({pat}, ' '.join(s for s in i.get_attribute_list({k}) if s is not None)))"
        )
    }

    fn pred_text_eq(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        format!("any(i.text == v for v in {})", py_str_list(values))
    }

    fn pred_text_ne(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        format!("all(i.text != v for v in {})", py_str_list(values))
    }

    fn pred_text_starts(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        format!("i.text.startswith({})", py_str_tuple(values))
    }

    fn pred_text_ends(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        format!("i.text.endswith({})", py_str_tuple(values))
    }

    fn pred_text_contains(&self, values: &[String], _builder: &mut ModuleBuilder) -> String {
        format!("any(v in i.text for v in {})", py_str_list(values))
    }

    fn pred_text_re(&self, pattern: &str, builder: &mut ModuleBuilder) -> String {
        builder.require_import("import re");
        format!("bool(re.search({}, i.text))", py_str(pattern))
    }
}

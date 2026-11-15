mod common;

use std::path::PathBuf;
use std::process::Command;

use common::SCHEMA_PYTHON_EXTENSIONS as SCHEMA_EXTENSIONS;
use common::*;
use parsmith::{CodegenOptions, PythonHtmlLib, PythonModels, Target, generate_source};

const SCHEMA_LXML_XPATH_AND_CONTRACTS: &str = r####"
@doc "Dedicated lxml XPath, CSS multi-query, drop_tree tail preservation, to-bool, and attr contract coverage."

struct LxmlXpathAndContracts {
    xpath_single {
        xpath "//section[@class='single']/h1"
        text
        trim
    }

    xpath_multi_fallback_query {
        xpath {
            "//nonexistent/h1"
            "//section[@class='single']/h1"
        }
        text
        trim
    }

    css_multi_fallback_query {
        css {
            ".nonexistent-heading"
            "section.single h1"
        }
        text
        trim
    }

    xpath_bool_missing {
        xpath "//div[@class='does-not-exist']"
        to-bool
    }

    xpath_multi_bool_missing {
        xpath {
            "//div[@class='missing-1']"
            "//div[@class='missing-2']"
        }
        to-bool
    }

    xpath_bool_empty_child {
        xpath "//span[@class='empty-child']"
        to-bool
    }

    xpath_multi_bool_present {
        xpath {
            "//span[@class='missing']"
            "//span[@class='empty-child']"
        }
        to-bool
    }

    css_bool_missing {
        css ".does-not-exist"
        to-bool
    }

    css_multi_bool_missing {
        css {
            ".missing-1"
            ".missing-2"
        }
        to-bool
    }

    css_bool_empty_child {
        css "span.empty-child"
        to-bool
    }

    css_multi_bool_present {
        css {
            "span.missing"
            "span.empty-child"
        }
        to-bool
    }

    xpath_all_single {
        xpath-all "//div[@class='item']/span[@class='label']"
        text
    }

    xpath_all_multi {
        xpath-all {
            "//div[@class='missing']/span"
            "//div[@class='item']/span[@class='label']"
        }
        text
    }

    css_all_multi {
        css-all {
            ".missing-items"
            "div.item span.label"
        }
        text
    }

    xpath_all_bool {
        xpath-all "//div[@class='item']"
        to-bool
    }

    xpath_remove_single_tail {
        xpath "//p[@class='tail-host']"
        xpath-remove ".//b"
        text
        normalize-space
    }

    xpath_remove_array_tail {
        xpath-all "//div[@class='tail-list']/p"
        xpath-remove ".//b"
        text
        normalize-space
    }

    css_remove_single_tail {
        css "p.css-tail-host"
        css-remove "b"
        text
        normalize-space
    }

    css_remove_array_tail {
        css-all "div.css-tail-list p"
        css-remove "b"
        text
        normalize-space
    }

    raw_outer_array {
        css-all "div.raw-items span"
        raw
    }

    raw_inner_array {
        css-all "div.raw-items span"
        raw inner
    }

    xpath_filtered_items {
        xpath-all "//ul[@class='pred-list']/li"
        filter {
            xpath ".//em"
            has-attr "data-id"
            attr-eq "data-role" "primary"
            attr-ne "data-skip" "yes"
            attr-starts "data-code" "PRD-"
            attr-ends "data-code" "-OK"
            attr-contains "data-tags" "featured"
            attr-contains "data-tags" "featured" "pinned"
            attr-re "data-id" #"^\d+$"#
            text-eq "Alpha Item" "Beta Item"
            text-ne "Ignored Item" "Blocked Item"
            text-starts "Alpha"
            text-ends "Item"
            text-contains "Alpha"
            text-re #"^Alpha"#
        }
        attr "data-id"
    }

    single_pred_items {
        css-all "ul.pred-list li"
        filter {
            text-eq "Alpha Item"
            text-ne "Ignored Item"
        }
        attr "data-id"
    }

    missing_attr_fallback {
        css "a.primary"
        attr "data-nonexistent"
        fallback "ATTR_FALLBACK_OK"
    }

    multi_attr_single_el {
        css "a.primary"
        attr "href" "data-missing" "rel"
    }

    multi_attr_array_el {
        css-all "a.link-item"
        attr "href" "data-extra"
    }
}
"####;

#[test]
fn lxml_codegen_emits_expected_imports_and_utilities() {
    let opts = CodegenOptions::new(Target::Python).with_html_lib(PythonHtmlLib::Lxml);
    let res = generate_source(
        SCHEMA_LXML_XPATH_AND_CONTRACTS,
        Some("lxml_spec.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);

    let code = res.primary_code().unwrap();
    assert!(code.contains("from lxml import html\n"));
    assert!(code.contains("from lxml.html import HtmlElement\n"));
    assert!(code.contains("RT_FALLBACK_HTML_STR = \"<html><body></body></html>\"\n"));
    assert!(code.contains("RtLxmlHtml: TypeAlias = str | HtmlElement\n"));
    assert!(
        code.contains("def rt_xpath_first(el: HtmlElement, *queries: str) -> HtmlElement | None:")
    );
    assert!(
        code.contains(
            "def rt_xpath_all_first(el: HtmlElement, *queries: str) -> list[HtmlElement]:"
        )
    );
    assert!(code.contains("def rt_xpath_remove(tag: HtmlElement, q: str) -> HtmlElement:"));
    assert!(
        code.contains("def rt_select_first(el: HtmlElement, *queries: str) -> HtmlElement | None:")
    );
    assert!(
        code.contains(
            "def rt_select_all_first(el: HtmlElement, *queries: str) -> list[HtmlElement]:"
        )
    );
    assert!(code.contains("def rt_select_remove(tag: HtmlElement, q: str) -> HtmlElement:"));
    assert!(code.contains("def rt_inner_html(el: HtmlElement) -> str:"));
    assert!(code.contains("_el.drop_tree()"));
}

#[test]
fn python_lxml_end_to_end_pytest_and_mypy_strict() {
    let tmp = TempTestDir::new("lxml_suite");
    tmp.write("dsl_coverage.html", HTML_FIXTURE);

    let standard_schemas = [
        ("item", SCHEMA_ITEM_INIT_CHECK),
        ("strings", SCHEMA_01_STRINGS_BASIC),
        ("arrays", SCHEMA_02_ARRAYS_AND_CONVERSIONS),
        ("filters", SCHEMA_03_FILTERS_AND_PREDICATES),
        ("flat", SCHEMA_05_FLAT),
        ("dict", SCHEMA_06_DICT),
        ("table", SCHEMA_07_TABLE),
        ("fn", SCHEMA_25_FN),
        ("ext", SCHEMA_EXTENSIONS),
    ];

    let mut generated_files: Vec<PathBuf> = Vec::new();

    // 1. Generate all standard schemas for both `lxml` and `bs4` in TypedDict mode for parity comparison
    let lxml_td_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Lxml)
        .with_models(PythonModels::TypedDict);
    let bs4_td_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Bs4)
        .with_models(PythonModels::TypedDict);

    for (stem, src) in &standard_schemas {
        let lxml_name = format!("lxml_{stem}.kdl");
        let lxml_res = generate_source(src, Some(&lxml_name), &lxml_td_opts);
        assert!(
            !lxml_res.has_errors(),
            "lxml TypedDict generation failed for {lxml_name}: {:?}",
            lxml_res.diagnostics
        );
        for (fname, code) in &lxml_res.files {
            generated_files.push(tmp.write(fname, code));
        }

        let bs4_name = format!("bs4_{stem}.kdl");
        let bs4_res = generate_source(src, Some(&bs4_name), &bs4_td_opts);
        assert!(
            !bs4_res.has_errors(),
            "bs4 TypedDict generation failed for {bs4_name}: {:?}",
            bs4_res.diagnostics
        );
        for (fname, code) in &bs4_res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 2. Generate key schemas in Dataclass mode with `lxml`
    let dc_schemas = [
        ("lxml_item_dc.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("lxml_strings_dc.kdl", SCHEMA_01_STRINGS_BASIC),
        ("lxml_table_dc.kdl", SCHEMA_07_TABLE),
        ("lxml_xpath_dc.kdl", SCHEMA_LXML_XPATH_AND_CONTRACTS),
    ];
    let lxml_dc_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Lxml)
        .with_models(PythonModels::Dataclass);
    for (name, src) in &dc_schemas {
        let res = generate_source(src, Some(name), &lxml_dc_opts);
        assert!(
            !res.has_errors(),
            "lxml Dataclass generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 3. Generate dedicated XPath & contract schema in TypedDict mode with `lxml`
    let res_xpath_td = generate_source(
        SCHEMA_LXML_XPATH_AND_CONTRACTS,
        Some("lxml_xpath_td.kdl"),
        &lxml_td_opts,
    );
    assert!(!res_xpath_td.has_errors(), "{:?}", res_xpath_td.diagnostics);
    for (fname, code) in &res_xpath_td.files {
        generated_files.push(tmp.write(fname, code));
    }

    // 4. Generate separate runtime (-R) modules with `lxml`
    let lxml_sep_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Lxml)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");

    let res_sep_ext = generate_source(SCHEMA_EXTENSIONS, Some("lxml_ext_sep.kdl"), &lxml_sep_opts);
    assert!(!res_sep_ext.has_errors(), "{:?}", res_sep_ext.diagnostics);
    for (fname, code) in &res_sep_ext.files {
        generated_files.push(tmp.write(fname, code));
    }

    let res_sep_table =
        generate_source(SCHEMA_07_TABLE, Some("lxml_table_sep.kdl"), &lxml_sep_opts);
    assert!(
        !res_sep_table.has_errors(),
        "{:?}",
        res_sep_table.diagnostics
    );
    generated_files.push(tmp.write(
        "lxml_table_sep.py",
        &res_sep_table.files["lxml_table_sep.py"],
    ));

    let res_sep_xpath = generate_source(
        SCHEMA_LXML_XPATH_AND_CONTRACTS,
        Some("lxml_xpath_sep.kdl"),
        &lxml_sep_opts,
    );
    assert!(
        !res_sep_xpath.has_errors(),
        "{:?}",
        res_sep_xpath.diagnostics
    );
    generated_files.push(tmp.write(
        "lxml_xpath_sep.py",
        &res_sep_xpath.files["lxml_xpath_sep.py"],
    ));

    // 5. Write comprehensive pytest + mypy verification suite
    let test_suite_py = r#"from __future__ import annotations

import warnings
from pathlib import Path
from lxml import html as lxml_html
from lxml.html import HtmlElement
import pytest

import bs4_item
import bs4_strings
import bs4_arrays
import bs4_filters
import bs4_flat
import bs4_dict
import bs4_table
import bs4_fn
import bs4_ext

import lxml_item
import lxml_strings
import lxml_arrays
import lxml_filters
import lxml_flat
import lxml_dict
import lxml_table
import lxml_fn
import lxml_ext

import lxml_item_dc
import lxml_strings_dc
import lxml_table_dc
import lxml_xpath_dc
import lxml_xpath_td
import lxml_ext_sep
import lxml_table_sep
import lxml_xpath_sep
import parsmith_runtime


@pytest.fixture(scope="module")
def coverage_html() -> str:
    return (Path(__file__).parent / "dsl_coverage.html").read_text(encoding="utf-8")


def test_lxml_parity_with_bs4_on_dsl_coverage(coverage_html: str) -> None:
    assert lxml_item.PageItem(coverage_html).has_cards() == bs4_item.PageItem(coverage_html).has_cards()
    assert lxml_item.PageItem(coverage_html).parse() == bs4_item.PageItem(coverage_html).parse()

    lxml_str_res = lxml_strings.StringsBasic(coverage_html).parse()
    bs4_str_res = bs4_strings.StringsBasic(coverage_html).parse()
    assert len(lxml_str_res) == len(bs4_str_res) == 2
    for l_item, b_item in zip(lxml_str_res, bs4_str_res):
        assert l_item["title"] == b_item["title"]
        assert l_item["link"] == b_item["link"]
        assert l_item["slug"] == b_item["slug"]
        assert l_item["normalized_code"] == b_item["normalized_code"]
        assert l_item["active_flag"] == b_item["active_flag"]
        assert "<h2 class=\"title\">" in l_item["inner_html"]
        assert len(l_item["inner_html"]) < len(l_item["clean_html"])

    assert lxml_arrays.ArraysAndConversions(coverage_html).parse() == bs4_arrays.ArraysAndConversions(coverage_html).parse()
    assert lxml_filters.FiltersAndPredicates(coverage_html).parse() == bs4_filters.FiltersAndPredicates(coverage_html).parse()

    inline_card = (
        '<div class="card">'
        '<a href="/guide.html" data-kind="nav" data-alt="alt-1" rel="noopener">'
        '<span class="icon"></span>Go guide now</a>'
        '<div class="logic">prealpha</div>'
        '<div class="code">code-8</div>'
        '</div>'
    )
    assert lxml_filters.FiltersAndPredicates(inline_card).parse() == bs4_filters.FiltersAndPredicates(inline_card).parse()

    assert lxml_flat.FlatCoverage(coverage_html).parse() == bs4_flat.FlatCoverage(coverage_html).parse()
    assert lxml_dict.MetaDict(coverage_html).parse() == bs4_dict.MetaDict(coverage_html).parse()
    assert lxml_dict.DictRoot(coverage_html).parse() == bs4_dict.DictRoot(coverage_html).parse()
    assert lxml_table.TableCoverage(coverage_html).parse() == bs4_table.TableCoverage(coverage_html).parse()

    assert lxml_fn.page_title(coverage_html) == bs4_fn.page_title(coverage_html)
    assert lxml_fn.all_links(coverage_html) == bs4_fn.all_links(coverage_html)
    assert lxml_fn.first_line("first\nsecond") == bs4_fn.first_line("first\nsecond")
    assert lxml_fn.extract_version("v version=2.3.4") == bs4_fn.extract_version("v version=2.3.4")

    assert lxml_ext.ExtRawParser("  hello   world  ").parse() == bs4_ext.ExtRawParser("  hello   world  ").parse()
    assert lxml_ext.encode_normalized("  hello   world  ") == bs4_ext.encode_normalized("  hello   world  ")


def test_lxml_dataclass_and_separate_runtime(coverage_html: str) -> None:
    dc_item = lxml_item_dc.PageItem(coverage_html).parse()
    assert isinstance(dc_item, lxml_item_dc.PageItemType)
    assert dc_item.heading == "Coverage Root"
    assert dc_item.canonical == "https://example.test/coverage"
    assert dc_item.single.title == "Single Item Title"
    assert dc_item.single.active is True

    dc_strings = lxml_strings_dc.StringsBasic(coverage_html).parse()
    assert len(dc_strings) == 2
    assert isinstance(dc_strings[0], lxml_strings_dc.StringsBasicType)
    assert dc_strings[0].title == "bar Title One"

    dc_table = lxml_table_dc.TableCoverage(coverage_html).parse()
    assert isinstance(dc_table, lxml_table_dc.TableCoverageType)
    assert dc_table.identifier == "ABC-123"
    assert dc_table.price == 9.99

    sep_table = lxml_table_sep.TableCoverage(coverage_html).parse()
    assert sep_table == lxml_table.TableCoverage(coverage_html).parse()

    sep_ext = lxml_ext_sep.ExtRawParser("  hello   world  ").parse()
    assert sep_ext == {"encoded": "aGVsbG8gd29ybGQ="}
    assert parsmith_runtime.normalize_value("  a   b  ") == "a b"


def test_lxml_accepts_str_and_preparsed_htmlelement_and_empty_str(coverage_html: str) -> None:
    preparsed: HtmlElement = lxml_html.fromstring(coverage_html)
    from_str = lxml_item.PageItem(coverage_html).parse()
    from_el = lxml_item.PageItem(preparsed).parse()
    assert from_str == from_el

    assert lxml_fn.page_title(preparsed) == "Coverage Root"

    # Empty/whitespace string falls back to RT_FALLBACK_HTML_STR without crashing on html.fromstring
    assert lxml_item.PageItem("   ").has_cards() is False
    with pytest.raises((lxml_item.RtAssertionError, ValueError)):
        lxml_item.PageItem("   ").parse()


def test_lxml_xpath_css_to_bool_drop_tree_tail_and_attr_contracts() -> None:
    doc = """
    <html>
      <body>
        <section class="single">
          <h1>XPath Heading</h1>
        </section>
        <span class="empty-child"></span>
        <div class="item"><span class="label">L1</span></div>
        <div class="item"><span class="label">L2</span></div>
        <p class="tail-host">Hello <b>REMOVE_ME</b> world!</p>
        <div class="tail-list">
          <p>First <b>GONE_1</b> tail</p>
          <p>Second <b>GONE_2</b> tail</p>
        </div>
        <p class="css-tail-host">Start <b>CSS_RM</b> end!</p>
        <div class="css-tail-list">
          <p>A <b>X</b> B</p>
          <p>C <b>Y</b> D</p>
        </div>
        <div class="raw-items"><span><em>in1</em></span><span><em>in2</em></span></div>
        <ul class="pred-list">
          <li data-id="101" data-role="primary" data-skip="no" data-code="PRD-1-OK" data-tags="featured pinned"><em>Alpha</em> Item</li>
          <li data-id="102" data-role="primary" data-code="PRD-2-OK" data-tags="featured"><em>Alpha</em> Item</li>
          <li data-id="103" data-role="secondary" data-skip="no" data-code="PRD-3-OK" data-tags="featured"><em>Alpha</em> Item</li>
        </ul>
        <a class="primary" href="/main" rel="noopener">Main</a>
        <a class="link-item" href="/a" data-extra="ex-a">A</a>
        <a class="link-item" href="/b">B</a>
      </body>
    </html>
    """

    with warnings.catch_warnings():
        warnings.simplefilter("error", FutureWarning)
        td_out = lxml_xpath_td.LxmlXpathAndContracts(doc).parse()
        dc_out = lxml_xpath_dc.LxmlXpathAndContracts(lxml_html.fromstring(doc)).parse()
        sep_out = lxml_xpath_sep.LxmlXpathAndContracts(doc).parse()

    expected = {
        "xpath_single": "XPath Heading",
        "xpath_multi_fallback_query": "XPath Heading",
        "css_multi_fallback_query": "XPath Heading",
        "xpath_bool_missing": False,
        "xpath_multi_bool_missing": False,
        "xpath_bool_empty_child": True,
        "xpath_multi_bool_present": True,
        "css_bool_missing": False,
        "css_multi_bool_missing": False,
        "css_bool_empty_child": True,
        "css_multi_bool_present": True,
        "xpath_all_single": ["L1", "L2"],
        "xpath_all_multi": ["L1", "L2"],
        "css_all_multi": ["L1", "L2"],
        "xpath_all_bool": True,
        "xpath_remove_single_tail": "Hello world!",
        "xpath_remove_array_tail": ["First tail", "Second tail"],
        "css_remove_single_tail": "Start end!",
        "css_remove_array_tail": ["A B", "C D"],
        "raw_outer_array": ["<span><em>in1</em></span>", "<span><em>in2</em></span>"],
        "raw_inner_array": ["<em>in1</em>", "<em>in2</em>"],
        "xpath_filtered_items": ["101"],
        "single_pred_items": ["101", "102", "103"],
        "missing_attr_fallback": "ATTR_FALLBACK_OK",
        "multi_attr_single_el": ["/main", "noopener"],
        "multi_attr_array_el": ["/a", "ex-a", "/b"],
    }

    assert td_out == expected
    assert sep_out == expected
    assert dc_out.xpath_single == "XPath Heading"
    assert dc_out.xpath_bool_missing is False
    assert dc_out.xpath_bool_empty_child is True
    assert dc_out.css_bool_missing is False
    assert dc_out.css_bool_empty_child is True
    assert dc_out.xpath_remove_single_tail == "Hello world!"
    assert dc_out.xpath_remove_array_tail == ["First tail", "Second tail"]
    assert dc_out.missing_attr_fallback == "ATTR_FALLBACK_OK"
"#;
    let test_suite_path = tmp.write("test_lxml_generated_suite.py", test_suite_py);
    generated_files.push(test_suite_path.clone());

    // 6. Run `uv run mypy --strict` on all generated files + test suite
    let mut mypy_cmd = Command::new("uv");
    mypy_cmd.arg("run").arg("mypy").arg("--strict");
    for file in &generated_files {
        mypy_cmd.arg(file);
    }
    let mypy_out = mypy_cmd.output().expect("execute uv run mypy --strict");
    assert!(
        mypy_out.status.success(),
        "mypy --strict failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&mypy_out.stdout),
        String::from_utf8_lossy(&mypy_out.stderr)
    );

    // 7. Run `uv run pytest` on the generated test suite
    let pytest_out = Command::new("uv")
        .args(["run", "pytest", "-q"])
        .arg(&test_suite_path)
        .env("PYTHONPATH", &tmp.path)
        .output()
        .expect("execute uv run pytest");
    assert!(
        pytest_out.status.success(),
        "pytest failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pytest_out.stdout),
        String::from_utf8_lossy(&pytest_out.stderr)
    );
}

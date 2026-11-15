mod common;

use std::path::PathBuf;
use std::process::Command;

use common::SCHEMA_PYTHON_EXTENSIONS as SCHEMA_EXTENSIONS;
use common::*;
use parsmith::{CodegenOptions, PythonHtmlLib, PythonModels, Target, generate_source};

const SCHEMA_ATTR_CONTRACT: &str = r#"
struct AttrContract {
    missing_with_fallback {
        css "a.primary"
        attr "data-missing"
        fallback "FALLBACK_TRIGGERED"
    }

    single_class {
        css "a.primary"
        attr "class"
    }

    multi_key_attrs {
        css "a.primary"
        attr "rel" "class" "data-missing"
    }

    all_classes {
        css-all "a"
        attr "class"
    }

    eq_multi_val {
        css-all "a"
        filter {
            attr-eq "rel" "noopener noreferrer"
            attr-eq "class" "primary btn"
        }
        attr "href"
    }

    ne_multi_val {
        css-all "a"
        filter {
            attr-ne "rel" "noopener noreferrer"
        }
        attr "href"
    }
}

fn strict_required_attr {
    css "a.primary"
    attr "data-missing"
}
"#;

const SCHEMA_PARSEL_XPATH_AND_DOM: &str = r####"
struct ParselXpathAndDom {
    xpath_single {
        xpath "//section[@class='single']/h1"
        text
        trim
    }

    xpath_multi_fallback_query {
        xpath {
            "//section[@class='nonexistent']/h1"
            "//section[@class='single']/h1"
        }
        text
        trim
    }

    xpath_present_bool {
        xpath "//section[@class='single']/h1"
        to-bool
    }

    xpath_missing_bool {
        xpath "//section[@class='does-not-exist']"
        to-bool
    }

    xpath_empty_child_bool {
        xpath "//div[@data-flag='Alpha']"
        to-bool
    }

    xpath_multi_present_bool {
        xpath {
            "//nonexistent"
            "//div[@data-flag='Alpha']"
        }
        to-bool
    }

    xpath_multi_missing_bool {
        xpath {
            "//nonexistent-1"
            "//nonexistent-2"
        }
        to-bool
    }

    css_present_bool {
        css "section.single > h1"
        to-bool
    }

    css_missing_bool {
        css "section.does-not-exist"
        to-bool
    }

    css_empty_child_bool {
        css "div[data-flag='Alpha']"
        to-bool
    }

    css_multi_query {
        css {
            "section.nonexistent > h1"
            "section.single > h1"
        }
        text
        trim
    }

    css_multi_present_bool {
        css {
            ".nonexistent"
            "div[data-flag='Alpha']"
        }
        to-bool
    }

    css_multi_missing_bool {
        css {
            ".nonexistent-1"
            ".nonexistent-2"
        }
        to-bool
    }

    css_all_multi_hit {
        css-all {
            ".nonexistent"
            ".nested-card .name"
        }
        text
        trim
    }

    css_all_multi_empty {
        css-all {
            ".nonexistent-1"
            ".nonexistent-2"
        }
        text
    }

    xpath_all_single {
        xpath-all "//div[@class='nested-card']/span[@class='name']"
        text
        trim
    }

    xpath_all_multi_hit {
        xpath-all {
            "//div[@class='nonexistent']"
            "//div[@class='nested-card']/a"
        }
        attr "href"
    }

    xpath_all_multi_empty {
        xpath-all {
            "//nonexistent-1"
            "//nonexistent-2"
        }
        text
    }

    filtered_xpath_remove_array {
        xpath-all "//div[@class='nested-card']"
        filter {
            xpath ".//a[@href]"
        }
        xpath-remove ".//a"
        text
        trim
    }

    single_xpath_remove_tail {
        xpath "//p[@class='tail-host']"
        xpath-remove ".//b"
        text
    }

    array_xpath_remove_raw_outer {
        xpath-all "//p[@class='tail-host']"
        xpath-remove ".//b"
        raw
    }

    single_css_remove_tail {
        css "div.css-tail-host"
        css-remove "span.drop-me"
        text
    }

    array_css_remove_raw_outer {
        css-all "div.css-tail-host"
        css-remove "span.drop-me"
        raw
    }

    array_raw_inner {
        css-all "div.raw-box"
        raw inner
    }

    single_raw_outer {
        css "div.raw-box"
        raw
    }

    pred_coverage {
        css-all "li.pred-item"
        filter {
            has-attr "data-k"
            attr-eq "data-k" "v1"
            attr-ne "data-other" "bad"
            attr-starts "data-k" "v"
            attr-ends "data-k" "1"
            attr-contains "data-k" "v"
            attr-contains "data-tags" "x" "y"
            attr-re "data-k" #"^v\d$"#
            text-eq "Item One"
            text-ne "Forbidden"
            text-starts "Item"
            text-ends "One"
            text-contains "One"
            text-re #"^Item\s+One$"#
        }
        attr "data-k"
    }

    pred_multi_text {
        css-all "li.pred-item"
        filter {
            text-eq "Item One" "Item Two"
            text-ne "Skip A" "Skip B"
        }
        text
    }
}
"####;

#[test]
fn parsel_emitted_code_structure_and_helpers() {
    let opts = CodegenOptions::new(Target::Python).with_html_lib(PythonHtmlLib::Parsel);
    let res = generate_source(SCHEMA_PARSEL_XPATH_AND_DOM, Some("p_xpath.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();

    assert!(code.contains("from parsel import Selector, SelectorList\n"));
    assert!(code.contains("RtParselHtml: TypeAlias = str | Selector | SelectorList[Selector]\n"));
    assert!(code.contains(
        "elif isinstance(document, SelectorList):\n            self._doc = document[0]\n"
    ));
    assert!(code.contains("def rt_select_first(sel: Selector, *queries: str) -> Selector | None:"));
    assert!(code.contains("def rt_xpath_first(sel: Selector, *queries: str) -> Selector | None:"));
    assert!(code.contains(
        "def rt_select_all_first(sel: Selector, *queries: str) -> SelectorList[Selector]:"
    ));
    assert!(code.contains(
        "def rt_xpath_all_first(sel: Selector, *queries: str) -> SelectorList[Selector]:"
    ));
    assert!(code.contains("def rt_select_remove(tag: Selector, q: str) -> Selector:\n    tag.css(q).drop()\n    return tag"));
    assert!(code.contains("def rt_xpath_remove(tag: Selector, q: str) -> Selector:\n    tag.xpath(q).drop()\n    return tag"));
    assert!(
        code.contains(
            "_m[0] if (_m := v0.xpath(\"//section[@class='does-not-exist']\")) else None"
        )
    );
    assert!(code.contains("_m[0] if (_m := v0.css('section.does-not-exist')) else None"));
}

#[test]
fn parsel_end_to_end_codegen_pytest_and_mypy_strict() {
    let tmp = TempTestDir::new("e2e");
    tmp.write("dsl_coverage.html", HTML_FIXTURE);

    let shared_schemas = [
        ("item", SCHEMA_ITEM_INIT_CHECK),
        ("strings", SCHEMA_01_STRINGS_BASIC),
        ("arrays", SCHEMA_02_ARRAYS_AND_CONVERSIONS),
        ("filters", SCHEMA_03_FILTERS_AND_PREDICATES),
        ("flat", SCHEMA_05_FLAT),
        ("dict", SCHEMA_06_DICT),
        ("table", SCHEMA_07_TABLE),
        ("raw", SCHEMA_23_RAW_STRUCT),
        ("fn", SCHEMA_25_FN),
        ("ext", SCHEMA_EXTENSIONS),
        ("attr_contract", SCHEMA_ATTR_CONTRACT),
    ];

    let mut generated_files: Vec<PathBuf> = Vec::new();

    // 1. Generate all shared schemas for Parsel (TypedDict) and Bs4 (TypedDict) for cross-backend comparison
    let parsel_td_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Parsel)
        .with_models(PythonModels::TypedDict);
    let bs4_td_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Bs4)
        .with_models(PythonModels::TypedDict);

    for (stem, src) in &shared_schemas {
        let p_name = format!("p_{stem}.kdl");
        let p_res = generate_source(src, Some(&p_name), &parsel_td_opts);
        assert!(
            !p_res.has_errors(),
            "Parsel TypedDict generation failed for {p_name}: {:?}",
            p_res.diagnostics
        );
        for (fname, code) in &p_res.files {
            generated_files.push(tmp.write(fname, code));
        }

        let b_name = format!("b_{stem}.kdl");
        let b_res = generate_source(src, Some(&b_name), &bs4_td_opts);
        assert!(
            !b_res.has_errors(),
            "Bs4 TypedDict generation failed for {b_name}: {:?}",
            b_res.diagnostics
        );
        for (fname, code) in &b_res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 2. Generate Parsel schemas in Dataclass mode
    let dc_schemas = [
        ("p_item_dc.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("p_strings_dc.kdl", SCHEMA_01_STRINGS_BASIC),
        ("p_table_dc.kdl", SCHEMA_07_TABLE),
        ("p_raw_dc.kdl", SCHEMA_23_RAW_STRUCT),
        ("p_xpath_dc.kdl", SCHEMA_PARSEL_XPATH_AND_DOM),
    ];
    let parsel_dc_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Parsel)
        .with_models(PythonModels::Dataclass);
    for (name, src) in &dc_schemas {
        let res = generate_source(src, Some(name), &parsel_dc_opts);
        assert!(
            !res.has_errors(),
            "Parsel Dataclass generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 3. Generate Parsel XPath & DOM edge-case schema in TypedDict and Separate Runtime (-R) modes
    let res_xpath_td = generate_source(
        SCHEMA_PARSEL_XPATH_AND_DOM,
        Some("p_xpath_td.kdl"),
        &parsel_td_opts,
    );
    assert!(
        !res_xpath_td.has_errors(),
        "Parsel XPath TypedDict generation failed: {:?}",
        res_xpath_td.diagnostics
    );
    for (fname, code) in &res_xpath_td.files {
        generated_files.push(tmp.write(fname, code));
    }

    let parsel_sep_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Parsel)
        .with_separate_runtime(true)
        .with_runtime_name("parsel_shared_rt");

    let res_sep_ext = generate_source(SCHEMA_EXTENSIONS, Some("p_ext_sep.kdl"), &parsel_sep_opts);
    assert!(!res_sep_ext.has_errors(), "{:?}", res_sep_ext.diagnostics);
    for (fname, code) in &res_sep_ext.files {
        generated_files.push(tmp.write(fname, code));
    }

    let res_sep_table = generate_source(SCHEMA_07_TABLE, Some("p_table_sep.kdl"), &parsel_sep_opts);
    assert!(
        !res_sep_table.has_errors(),
        "{:?}",
        res_sep_table.diagnostics
    );
    generated_files.push(tmp.write("p_table_sep.py", &res_sep_table.files["p_table_sep.py"]));

    let res_sep_xpath = generate_source(
        SCHEMA_PARSEL_XPATH_AND_DOM,
        Some("p_xpath_sep.kdl"),
        &parsel_sep_opts,
    );
    assert!(
        !res_sep_xpath.has_errors(),
        "{:?}",
        res_sep_xpath.diagnostics
    );
    generated_files.push(tmp.write("p_xpath_sep.py", &res_sep_xpath.files["p_xpath_sep.py"]));

    // 4. Write comprehensive pytest + mypy --strict test suite
    let test_suite_py = r#"from __future__ import annotations

from pathlib import Path
from parsel import Selector, SelectorList
import pytest

import b_item
import b_strings
import b_arrays
import b_filters
import b_flat
import b_dict
import b_table
import b_raw
import b_fn
import b_ext
import b_attr_contract

import p_item
import p_strings
import p_arrays
import p_filters
import p_flat
import p_dict
import p_table
import p_raw
import p_fn
import p_ext
import p_attr_contract

import p_item_dc
import p_strings_dc
import p_table_dc
import p_raw_dc
import p_xpath_dc
import p_xpath_td
import p_ext_sep
import p_table_sep
import p_xpath_sep
import parsel_shared_rt


@pytest.fixture(scope="module")
def html() -> str:
    return (Path(__file__).parent / "dsl_coverage.html").read_text(encoding="utf-8")


def test_parsel_matches_bs4_across_all_standard_schemas(html: str) -> None:
    assert p_item.PageItem(html).has_cards() == b_item.PageItem(html).has_cards()
    assert p_item.PageItem(html).parse() == b_item.PageItem(html).parse()

    p_str_res = p_strings.StringsBasic(html).parse()
    b_str_res = b_strings.StringsBasic(html).parse()
    assert len(p_str_res) == len(b_str_res) == 2
    for p_card, b_card in zip(p_str_res, b_str_res):
        assert p_card["title"] == b_card["title"]
        assert p_card["link"] == b_card["link"]
        assert p_card["slug"] == b_card["slug"]
        assert p_card["normalized_code"] == b_card["normalized_code"]
        assert p_card["active_flag"] == b_card["active_flag"]
        assert "<h2 class=\"title\">" in p_card["inner_html"]
        assert "Advertisement" not in p_card["clean_html"]
        assert "<script>" not in p_card["clean_html"]

    p_arr_res = p_arrays.ArraysAndConversions(html).parse()
    b_arr_res = b_arrays.ArraysAndConversions(html).parse()
    assert p_arr_res == b_arr_res

    assert p_filters.FiltersAndPredicates(html).parse() == b_filters.FiltersAndPredicates(html).parse()

    inline_card = (
        '<div class="card">'
        '<a href="/guide.html" data-kind="nav" data-alt="alt-1" rel="noopener">'
        '<span class="icon"></span>Go guide now</a>'
        '<div class="logic">prealpha</div>'
        '<div class="code">code-8</div>'
        '</div>'
    )
    assert (
        p_filters.FiltersAndPredicates(inline_card).parse()
        == b_filters.FiltersAndPredicates(inline_card).parse()
    )

    assert p_flat.FlatCoverage(html).parse() == b_flat.FlatCoverage(html).parse()
    assert p_dict.MetaDict(html).parse() == b_dict.MetaDict(html).parse()
    assert p_dict.DictRoot(html).parse() == b_dict.DictRoot(html).parse()
    assert p_table.TableCoverage(html).parse() == b_table.TableCoverage(html).parse()

    js_text = '<script>var player = new Playerjs({id:"player",file:"/v/list/abc.txt"});</script>'
    url_text = "https://ru.yummyani.me/iframeCVH.html?dubbing_code=Sanae&anime_id=339&episode=1"
    playlist_text = "[480p]/v/anime/01_480p.m3u8\n[720p]/v/anime/01_720p.m3u8"
    assert p_raw.RawPlayerScript(js_text).parse() == b_raw.RawPlayerScript(js_text).parse()
    assert p_raw.RawUrlParams(url_text).parse() == b_raw.RawUrlParams(url_text).parse()
    assert p_raw.RawLineItems(playlist_text).parse() == b_raw.RawLineItems(playlist_text).parse()
    assert p_raw.RawTextTransform("hello   world").parse() == b_raw.RawTextTransform("hello   world").parse()

    assert p_fn.page_title(html) == b_fn.page_title(html) == "Coverage Root"
    assert p_fn.all_links(html) == b_fn.all_links(html)
    assert p_fn.first_line("first\nsecond") == b_fn.first_line("first\nsecond")
    assert p_fn.extract_version("v version=2.0.1!") == b_fn.extract_version("v version=2.0.1!")

    assert p_ext.ExtRawParser("  hello   world  ").parse() == b_ext.ExtRawParser("  hello   world  ").parse()
    assert p_ext.encode_normalized("  hello   world  ") == b_ext.encode_normalized("  hello   world  ")


def test_parsel_dataclass_and_separate_runtime_modes(html: str) -> None:
    dc_item = p_item_dc.PageItem(html).parse()
    assert isinstance(dc_item, p_item_dc.PageItemType)
    assert dc_item.heading == "Coverage Root"
    assert dc_item.canonical == "https://example.test/coverage"
    assert dc_item.single == p_item_dc.SingleCardType(title="Single Item Title", active=True)

    dc_strings = p_strings_dc.StringsBasic(html).parse()
    assert len(dc_strings) == 2
    assert isinstance(dc_strings[0], p_strings_dc.StringsBasicType)
    assert dc_strings[0].title == "bar Title One"
    assert dc_strings[1].title == "bar Title Two"

    dc_table = p_table_dc.TableCoverage(html).parse()
    assert isinstance(dc_table, p_table_dc.TableCoverageType)
    assert dc_table.identifier == "ABC-123"
    assert dc_table.price == 9.99

    sep_table = p_table_sep.TableCoverage(html).parse()
    assert sep_table == p_table.TableCoverage(html).parse()

    sep_ext = p_ext_sep.ExtRawParser("  hello   world  ").parse()
    assert sep_ext == {"encoded": "aGVsbG8gd29ybGQ="}
    assert parsel_shared_rt.normalize_value("  a   b  ") == "a b"


def test_parsel_accepts_str_selector_and_selector_list_inputs(html: str) -> None:
    sel = Selector(html)
    sel_list: SelectorList[Selector] = sel.css("html")

    res_str = p_item.PageItem(html).parse()
    res_sel = p_item.PageItem(sel).parse()
    res_list = p_item.PageItem(sel_list).parse()
    assert res_str == res_sel == res_list

    body_list: SelectorList[Selector] = sel.css("body")
    assert p_fn.page_title(html) == "Coverage Root"
    assert p_fn.page_title(sel) == "Coverage Root"
    assert p_fn.page_title(body_list) == "Coverage Root"

    assert p_fn.all_links(html) == p_fn.all_links(sel) == p_fn.all_links(body_list)


def test_parsel_attr_keyerror_and_fallback_contract() -> None:
    sample = (
        '<div>'
        '<a class="primary btn" rel="noopener noreferrer" href="/one">One</a>'
        '<a class="secondary" rel="nofollow" href="/two">Two</a>'
        '</div>'
    )
    p_out = p_attr_contract.AttrContract(sample).parse()
    b_out = b_attr_contract.AttrContract(sample).parse()
    assert p_out == b_out == {
        "missing_with_fallback": "FALLBACK_TRIGGERED",
        "single_class": "primary btn",
        "multi_key_attrs": ["noopener noreferrer", "primary btn"],
        "all_classes": ["primary btn", "secondary"],
        "eq_multi_val": ["/one"],
        "ne_multi_val": ["/two"],
    }

    with pytest.raises(KeyError):
        p_attr_contract.strict_required_attr(sample)


def test_parsel_xpath_and_dom_edge_cases(html: str) -> None:
    extra_html = (
        html.replace(
            "</body>",
            '<p class="tail-host">Keep <b>drop</b>tail</p>'
            '<div class="css-tail-host">Start <span class="drop-me">gone</span>end</div>'
            '<div class="raw-box"><em>inner</em>text</div>'
            '<ul>'
            '<li class="pred-item" data-k="v1" data-other="ok" data-tags="x">Item One</li>'
            '<li class="pred-item" data-k="v2" data-tags="z">Item Two</li>'
            '<li class="pred-item" data-k="v1" data-other="bad" data-tags="y">Item One</li>'
            '</ul>'
            "</body>",
        )
    )

    td_res = p_xpath_td.ParselXpathAndDom(extra_html).parse()
    dc_res = p_xpath_dc.ParselXpathAndDom(extra_html).parse()
    sep_res = p_xpath_sep.ParselXpathAndDom(extra_html).parse()

    assert td_res == sep_res
    assert td_res["xpath_single"] == "Single Item Title"
    assert td_res["xpath_multi_fallback_query"] == "Single Item Title"
    assert td_res["xpath_present_bool"] is True
    assert td_res["xpath_missing_bool"] is False
    assert td_res["xpath_empty_child_bool"] is True
    assert td_res["xpath_multi_present_bool"] is True
    assert td_res["xpath_multi_missing_bool"] is False

    assert td_res["css_present_bool"] is True
    assert td_res["css_missing_bool"] is False
    assert td_res["css_empty_child_bool"] is True
    assert td_res["css_multi_query"] == "Single Item Title"
    assert td_res["css_multi_present_bool"] is True
    assert td_res["css_multi_missing_bool"] is False

    assert td_res["css_all_multi_hit"] == ["Nested One", "Nested Two"]
    assert td_res["css_all_multi_empty"] == []

    assert td_res["xpath_all_single"] == ["Nested One", "Nested Two"]
    assert td_res["xpath_all_multi_hit"] == ["/nested/one.html", "/nested/two.html"]
    assert td_res["xpath_all_multi_empty"] == []

    assert td_res["filtered_xpath_remove_array"] == ["Nested One", "Nested Two"]
    assert td_res["single_xpath_remove_tail"] == "Keep tail"
    assert td_res["array_xpath_remove_raw_outer"] == ['<p class="tail-host">Keep tail</p>']
    assert td_res["single_css_remove_tail"] == "Start end"
    assert td_res["array_css_remove_raw_outer"] == ['<div class="css-tail-host">Start end</div>']
    assert td_res["array_raw_inner"] == ["<em>inner</em>text"]
    assert td_res["single_raw_outer"] == '<div class="raw-box"><em>inner</em>text</div>'
    assert td_res["pred_coverage"] == ["v1"]
    assert td_res["pred_multi_text"] == ["Item One", "Item Two", "Item One"]

    assert isinstance(dc_res, p_xpath_dc.ParselXpathAndDomType)
    assert dc_res.xpath_single == "Single Item Title"
    assert dc_res.xpath_missing_bool is False
    assert dc_res.xpath_empty_child_bool is True
    assert dc_res.single_xpath_remove_tail == "Keep tail"
    assert dc_res.single_css_remove_tail == "Start end"
"#;

    let test_suite_path = tmp.write("test_parsel_suite.py", test_suite_py);
    generated_files.push(test_suite_path.clone());

    // 5. Run `uv run mypy --strict` on all generated files + test suite
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

    // 6. Run `uv run pytest` on the generated test suite
    let pytest_out = Command::new("uv")
        .args(["run", "pytest", "-q", "--rootdir"])
        .arg(&tmp.path)
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

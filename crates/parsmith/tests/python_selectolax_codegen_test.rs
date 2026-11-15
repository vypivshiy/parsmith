mod common;

use std::path::PathBuf;
use std::process::Command;

use common::SCHEMA_PYTHON_EXTENSIONS as SCHEMA_EXTENSIONS;
use common::*;
use parsmith::{CodegenOptions, PythonHtmlLib, PythonModels, Target, generate_source};

const SCHEMA_SELECTOLAX_CONTRACTS: &str = r#"
@doc "Selectolax runtime contracts: css + to-bool, attr KeyError fallback, boolean attrs, multi-query css, array css-remove, and predicates."

struct SelectolaxContracts {
    present_bool {
        css "a.primary"
        to-bool
    }

    empty_node_bool {
        css "span.empty-node"
        to-bool
    }

    missing_bool {
        css "div.does-not-exist"
        to-bool
    }

    multi_css_present_bool {
        css {
            "div.does-not-exist"
            "a.primary"
        }
        to-bool
    }

    multi_css_missing_bool {
        css {
            "div.does-not-exist"
            "span.also-missing"
        }
        to-bool
    }

    multi_css_text {
        css {
            "div.does-not-exist"
            "a.primary"
        }
        text
    }

    multi_css_all_hrefs {
        css-all {
            "div.does-not-exist"
            "a"
        }
        attr "href"
    }

    missing_attr_with_fallback {
        css "a.primary"
        attr "data-missing"
        fallback "FALLBACK_TRIGGERED"
    }

    boolean_disabled_attr {
        css "input.disabled-input"
        attr "disabled"
    }

    empty_val_attr {
        css "input.disabled-input"
        attr "data-empty"
    }

    array_to_bool_present {
        css-all "a"
        to-bool
    }

    array_to_bool_missing {
        css-all "div.does-not-exist"
        to-bool
    }

    str_to_bool_nonempty {
        css "a.primary"
        text
        to-bool
    }

    str_to_bool_empty {
        css "span.empty-node"
        text
        to-bool
    }

    single_class {
        css "a.primary"
        attr "class"
    }

    single_multi_key_attrs {
        css "a.primary"
        attr "rel" "class" "data-missing"
    }

    array_single_key_classes {
        css-all "a"
        attr "class"
    }

    array_multi_key_attrs {
        css-all "a"
        attr "href" "data-extra"
    }

    array_cleaned_inner {
        css-all "div.clean-target"
        css-remove "span.noise"
        raw inner
    }

    array_cleaned_outer {
        css-all "div.clean-target"
        css-remove "span.noise"
        raw outer
    }

    single_predicates {
        css-all "a"
        filter {
            has-attr "href"
            attr-eq "data-role" "main"
            attr-ne "rel" "nofollow"
            attr-starts "href" "/one"
            attr-ends "href" "one"
            attr-contains "class" "primary"
            text-eq "One"
            text-ne "Two"
            text-starts "O"
            text-ends "ne"
            text-contains "n"
        }
        attr "href"
    }

    multi_predicates {
        css-all "a"
        filter {
            has-attr "data-extra" "rel"
            attr-eq "rel" "noopener noreferrer" "nofollow"
            attr-ne "data-role" "ignored" "blocked"
            attr-starts "href" "/o" "/t"
            attr-ends "href" "one" "two"
            attr-contains "rel" "noopener" "follow"
            text-eq "One" "Two"
            text-ne "Three" "Four"
            text-starts "O" "T"
            text-ends "ne" "wo"
            text-contains "n" "w"
        }
        attr "href"
    }
}

fn strict_missing_attr {
    css "a.primary"
    attr "data-missing"
}
"#;

#[test]
fn selectolax_emits_expected_lexbor_types_and_init() {
    let opts = CodegenOptions::new(Target::Python).with_html_lib(PythonHtmlLib::Selectolax);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("page_item.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();

    assert!(code.contains("from selectolax.lexbor import LexborHTMLParser, LexborNode"));
    assert!(code.contains("RtSelectolaxHtml: TypeAlias = str | LexborHTMLParser | LexborNode"));
    assert!(code.contains("def __init__(self, document: RtSelectolaxHtml) -> None:"));
    assert!(code.contains("_root = LexborHTMLParser(document).root"));
    assert!(code.contains("self._doc: LexborNode = _root"));
    assert!(code.contains("elif isinstance(document, LexborHTMLParser):"));
    assert!(!code.contains("RT_BS4_FEATURES"));
}

#[test]
fn selectolax_end_to_end_pytest_and_mypy_strict_with_bs4_parity() {
    let tmp = TempTestDir::new("e2e");
    tmp.write("dsl_coverage.html", HTML_FIXTURE);

    let schemas = [
        ("slax_item.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("slax_strings.kdl", SCHEMA_01_STRINGS_BASIC),
        ("slax_arrays.kdl", SCHEMA_02_ARRAYS_AND_CONVERSIONS),
        ("slax_filters.kdl", SCHEMA_03_FILTERS_AND_PREDICATES),
        ("slax_flat.kdl", SCHEMA_05_FLAT),
        ("slax_dict.kdl", SCHEMA_06_DICT),
        ("slax_table.kdl", SCHEMA_07_TABLE),
        ("slax_raw.kdl", SCHEMA_23_RAW_STRUCT),
        ("slax_fn.kdl", SCHEMA_25_FN),
        ("slax_ext.kdl", SCHEMA_EXTENSIONS),
        ("slax_contracts.kdl", SCHEMA_SELECTOLAX_CONTRACTS),
    ];

    let mut generated_files: Vec<PathBuf> = Vec::new();

    // 1. Generate all schemas for Selectolax in TypedDict mode
    let slax_td_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Selectolax)
        .with_models(PythonModels::TypedDict);
    for (name, src) in &schemas {
        let res = generate_source(src, Some(name), &slax_td_opts);
        assert!(
            !res.has_errors(),
            "Selectolax TypedDict generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 2. Generate BS4 reference modules for cross-backend parity comparison
    let bs4_schemas = [
        ("bs4_item.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("bs4_strings.kdl", SCHEMA_01_STRINGS_BASIC),
        ("bs4_arrays.kdl", SCHEMA_02_ARRAYS_AND_CONVERSIONS),
        ("bs4_filters.kdl", SCHEMA_03_FILTERS_AND_PREDICATES),
        ("bs4_flat.kdl", SCHEMA_05_FLAT),
        ("bs4_dict.kdl", SCHEMA_06_DICT),
        ("bs4_table.kdl", SCHEMA_07_TABLE),
        ("bs4_fn.kdl", SCHEMA_25_FN),
    ];
    let bs4_td_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Bs4)
        .with_models(PythonModels::TypedDict);
    for (name, src) in &bs4_schemas {
        let res = generate_source(src, Some(name), &bs4_td_opts);
        assert!(
            !res.has_errors(),
            "BS4 TypedDict generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 3. Generate key schemas for Selectolax in Dataclass mode
    let dc_schemas = [
        ("slax_item_dc.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("slax_strings_dc.kdl", SCHEMA_01_STRINGS_BASIC),
        ("slax_table_dc.kdl", SCHEMA_07_TABLE),
        ("slax_raw_dc.kdl", SCHEMA_23_RAW_STRUCT),
    ];
    let slax_dc_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Selectolax)
        .with_models(PythonModels::Dataclass);
    for (name, src) in &dc_schemas {
        let res = generate_source(src, Some(name), &slax_dc_opts);
        assert!(
            !res.has_errors(),
            "Selectolax Dataclass generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 4. Generate separate runtime (-R) modules for Selectolax
    let slax_sep_opts = CodegenOptions::new(Target::Python)
        .with_html_lib(PythonHtmlLib::Selectolax)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res_sep_ext = generate_source(SCHEMA_EXTENSIONS, Some("slax_ext_sep.kdl"), &slax_sep_opts);
    assert!(!res_sep_ext.has_errors());
    for (fname, code) in &res_sep_ext.files {
        generated_files.push(tmp.write(fname, code));
    }
    let res_sep_table =
        generate_source(SCHEMA_07_TABLE, Some("slax_table_sep.kdl"), &slax_sep_opts);
    assert!(!res_sep_table.has_errors());
    generated_files.push(tmp.write(
        "slax_table_sep.py",
        &res_sep_table.files["slax_table_sep.py"],
    ));
    let res_sep_strings = generate_source(
        SCHEMA_01_STRINGS_BASIC,
        Some("slax_strings_sep.kdl"),
        &slax_sep_opts,
    );
    assert!(!res_sep_strings.has_errors());
    generated_files.push(tmp.write(
        "slax_strings_sep.py",
        &res_sep_strings.files["slax_strings_sep.py"],
    ));

    // 5. Write comprehensive pytest + mypy test suite
    let test_suite_py = r#"from __future__ import annotations

from pathlib import Path
import pytest
from selectolax.lexbor import LexborHTMLParser, LexborNode

import slax_item
import slax_strings
import slax_arrays
import slax_filters
import slax_flat
import slax_dict
import slax_table
import slax_raw
import slax_fn
import slax_ext
import slax_contracts
import slax_item_dc
import slax_strings_dc
import slax_table_dc
import slax_raw_dc
import slax_ext_sep
import slax_table_sep
import slax_strings_sep
import parsmith_runtime

import bs4_item
import bs4_strings
import bs4_arrays
import bs4_filters
import bs4_flat
import bs4_dict
import bs4_table
import bs4_fn


@pytest.fixture(scope="module")
def html() -> str:
    return (Path(__file__).parent / "dsl_coverage.html").read_text(encoding="utf-8")


def test_item_init_check_prevalidate_and_bs4_parity(html: str) -> None:
    parser = slax_item.PageItem(html)
    bs4_parser = bs4_item.PageItem(html)
    assert parser.has_cards() is True
    assert parser.has_cards() == bs4_parser.has_cards()

    data = parser.parse()
    bs4_data = bs4_parser.parse()
    assert data == bs4_data
    assert data["heading"] == "Coverage Root"
    assert data["canonical"] == "https://example.test/coverage"
    assert data["single"] == {"title": "Single Item Title", "active": True}

    dc_parser = slax_item_dc.PageItem(html)
    assert dc_parser.has_cards() is True
    dc_data = dc_parser.parse()
    assert isinstance(dc_data, slax_item_dc.PageItemType)
    assert dc_data.heading == "Coverage Root"
    assert dc_data.canonical == "https://example.test/coverage"
    assert isinstance(dc_data.single, slax_item_dc.SingleCardType)
    assert dc_data.single.title == "Single Item Title"
    assert dc_data.single.active is True

    with pytest.raises((slax_item.RtAssertionError, ValueError)):
        slax_item.PageItem("<html><body></body></html>").parse()


def test_input_types_str_lexbor_parser_and_lexbor_node(html: str) -> None:
    from_str = slax_item.PageItem(html).parse()

    lexbor_parser = LexborHTMLParser(html)
    from_parser = slax_item.PageItem(lexbor_parser).parse()

    root_node: LexborNode | None = lexbor_parser.root
    assert root_node is not None
    from_node = slax_item.PageItem(root_node).parse()

    assert from_str == from_parser == from_node

    # Also verify module-level fn accepts str, LexborHTMLParser, and LexborNode
    snippet = "<html><body><h1>Hello Selectolax</h1><a href='/x'>X</a></body></html>"
    p = LexborHTMLParser(snippet)
    n = p.root
    assert n is not None
    assert slax_fn.page_title(snippet) == "Hello Selectolax"
    assert slax_fn.page_title(p) == "Hello Selectolax"
    assert slax_fn.page_title(n) == "Hello Selectolax"
    assert slax_fn.all_links(snippet) == bs4_fn.all_links(snippet)


def test_strings_basic_typeddict_dataclass_separate_runtime_and_bs4_parity(html: str) -> None:
    res = slax_strings.StringsBasic(html).parse()
    bs4_res = bs4_strings.StringsBasic(html).parse()
    assert len(res) == len(bs4_res) == 2

    for s_item, b_item in zip(res, bs4_res):
        assert s_item["title"] == b_item["title"]
        assert s_item["link"] == b_item["link"]
        assert s_item["slug"] == b_item["slug"]
        assert s_item["normalized_code"] == b_item["normalized_code"]
        assert s_item["active_flag"] == b_item["active_flag"]
        assert '<h2 class="title">' in s_item["inner_html"]
        assert '<div class="ad">' not in s_item["clean_html"]
        assert "<script>" not in s_item["clean_html"]
        assert len(s_item["inner_html"]) < len(s_item["clean_html"])

    dc_res = slax_strings_dc.StringsBasic(html).parse()
    assert len(dc_res) == 2
    assert isinstance(dc_res[0], slax_strings_dc.StringsBasicType)
    assert dc_res[0].title == "bar Title One"
    assert dc_res[1].title == "bar Title Two"
    assert dc_res[1].normalized_code == "code-4"

    sep_res = slax_strings_sep.StringsBasic(html).parse()
    assert sep_res == res


def test_arrays_and_conversions_and_bs4_parity(html: str) -> None:
    res = slax_arrays.ArraysAndConversions(html).parse()
    bs4_res = bs4_arrays.ArraysAndConversions(html).parse()
    assert res == bs4_res


def test_filters_and_predicates_and_bs4_parity(html: str) -> None:
    res = slax_filters.FiltersAndPredicates(html).parse()
    bs4_res = bs4_filters.FiltersAndPredicates(html).parse()
    assert res == bs4_res

    inline_card = (
        '<div class="card">'
        '<a href="/guide.html" data-kind="nav" data-alt="alt-1" rel="noopener">'
        '<span class="icon"></span>Go guide now</a>'
        '<div class="logic">prealpha</div>'
        '<div class="code">code-8</div>'
        '</div>'
    )
    res_inline = slax_filters.FiltersAndPredicates(inline_card).parse()
    bs4_inline = bs4_filters.FiltersAndPredicates(inline_card).parse()
    assert res_inline == bs4_inline == [
        {
            "filtered_links": ["/guide.html", "alt-1"],
            "logic_check": "prealpha",
            "numeric_check": "code-8",
        }
    ]


def test_flat_and_dict_structs_and_bs4_parity(html: str) -> None:
    flat_res = slax_flat.FlatCoverage(html).parse()
    bs4_flat_res = bs4_flat.FlatCoverage(html).parse()
    assert flat_res == bs4_flat_res

    meta_dict = slax_dict.MetaDict(html).parse()
    bs4_meta_dict = bs4_dict.MetaDict(html).parse()
    assert meta_dict == bs4_meta_dict

    dict_root = slax_dict.DictRoot(html).parse()
    bs4_dict_root = bs4_dict.DictRoot(html).parse()
    assert dict_root == bs4_dict_root


def test_table_typeddict_dataclass_separate_runtime_and_bs4_parity(html: str) -> None:
    td_res = slax_table.TableCoverage(html).parse()
    bs4_td_res = bs4_table.TableCoverage(html).parse()
    assert td_res == bs4_td_res == {
        "identifier": "ABC-123",
        "code_value": "CODE-1",
        "price": 9.99,
        "tax_or_fee": 1.25,
        "state": "active",
    }

    dc_res = slax_table_dc.TableCoverage(html).parse()
    assert isinstance(dc_res, slax_table_dc.TableCoverageType)
    assert dc_res.identifier == "ABC-123"
    assert dc_res.price == 9.99
    assert dc_res.state == "active"

    sep_res = slax_table_sep.TableCoverage(html).parse()
    assert sep_res == td_res


def test_raw_structs_functions_and_extensions() -> None:
    js_text = '<script>var player = new Playerjs({id:"player",file:"/v/list/abc.txt"});</script>'
    url_text = "https://ru.yummyani.me/iframeCVH.html?dubbing_code=Sanae&anime_id=339&episode=1"
    playlist_text = "[480p]/v/anime/01_480p.m3u8\n[720p]/v/anime/01_720p.m3u8"

    assert slax_raw.RawPlayerScript(js_text).parse() == {
        "playlist_url": "/v/list/abc.txt",
        "player_id": "player",
    }
    p_dc = slax_raw_dc.RawPlayerScript(js_text).parse()
    assert p_dc.playlist_url == "/v/list/abc.txt"
    assert p_dc.player_id == "player"

    assert slax_raw.RawUrlParams(url_text).parse() == {"dubbing_code": "Sanae", "anime_id": 339}

    lines_dc = slax_raw_dc.RawLineItems(playlist_text).parse()
    assert len(lines_dc) == 2
    assert lines_dc[0].quality == "480p"
    assert lines_dc[1].url == "/v/anime/01_720p.m3u8"

    assert slax_fn.first_line("first\nsecond") == "first"
    assert slax_fn.extract_version("app version=1.2.3 released") == "1.2.3"

    ext_parser = slax_ext.ExtRawParser("  hello   world  ")
    assert ext_parser.is_valid() is True
    assert ext_parser.parse() == {"encoded": "aGVsbG8gd29ybGQ="}
    assert slax_ext.encode_normalized("  hello   world  ") == "aGVsbG8gd29ybGQ="

    sep_parser = slax_ext_sep.ExtRawParser("  hello   world  ")
    assert sep_parser.parse() == {"encoded": "aGVsbG8gd29ybGQ="}
    assert parsmith_runtime.normalize_value("  a   b  ") == "a b"


def test_selectolax_specific_contracts() -> None:
    sample = (
        '<div>'
        '<a class="primary btn" rel="noopener noreferrer" data-role="main" data-extra="ex1" href="/one">One</a>'
        '<a class="secondary" rel="nofollow" data-role="sub" href="/two">Two</a>'
        '<span class="empty-node"></span>'
        '<input class="disabled-input" data-empty="" disabled>'
        '<div class="clean-target"><span class="noise">X</span><b>Keep1</b></div>'
        '<div class="clean-target"><span class="noise">Y</span><i>Keep2</i></div>'
        '</div>'
    )
    out = slax_contracts.SelectolaxContracts(sample).parse()
    assert out["present_bool"] is True
    assert out["empty_node_bool"] is True
    assert out["missing_bool"] is False
    assert out["multi_css_present_bool"] is True
    assert out["multi_css_missing_bool"] is False
    assert out["multi_css_text"] == "One"
    assert out["multi_css_all_hrefs"] == ["/one", "/two"]
    assert out["missing_attr_with_fallback"] == "FALLBACK_TRIGGERED"
    assert out["boolean_disabled_attr"] == ""
    assert out["empty_val_attr"] == ""
    assert out["array_to_bool_present"] is True
    assert out["array_to_bool_missing"] is False
    assert out["str_to_bool_nonempty"] is True
    assert out["str_to_bool_empty"] is False
    assert out["single_class"] == "primary btn"
    assert out["single_multi_key_attrs"] == ["noopener noreferrer", "primary btn"]
    assert out["array_single_key_classes"] == ["primary btn", "secondary"]
    assert out["array_multi_key_attrs"] == ["/one", "ex1", "/two"]
    assert out["array_cleaned_inner"] == ["<b>Keep1</b>", "<i>Keep2</i>"]
    assert out["array_cleaned_outer"] == [
        '<div class="clean-target"><b>Keep1</b></div>',
        '<div class="clean-target"><i>Keep2</i></div>',
    ]
    assert out["single_predicates"] == ["/one"]
    assert out["multi_predicates"] == ["/one", "/two"]

    with pytest.raises(KeyError):
        slax_contracts.strict_missing_attr(sample)

    # Empty string document initializes cleanly and returns False on has_cards()
    assert slax_item.PageItem("").has_cards() is False
"#;
    let test_suite_path = tmp.write("test_selectolax_suite.py", test_suite_py);
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
        .arg(format!("--rootdir={}", tmp.path.display()))
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

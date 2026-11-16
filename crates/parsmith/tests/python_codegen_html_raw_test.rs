mod common;

use std::path::PathBuf;
use std::process::Command;

use common::SCHEMA_PYTHON_EXTENSIONS as SCHEMA_EXTENSIONS;
use common::SCHEMA_XPATH_DOM as SCHEMA_XPATH_BS4;
use common::*;
use parsmith::{
    Bs4Parser, CodeWriter, CodegenOptions, DiagnosticCode, MemorySourceLoader, ModuleBuilder,
    PythonEmitter, PythonHtmlLib, PythonModels, Target, drive_module, generate_source,
    generate_source_with_loader, parse_target_or_shorthand,
};

const SCHEMA_BS4_ATTR_CONTRACT: &str = r#"
struct Bs4AttrContract {
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
"#;

#[test]
fn code_writer_dry_run_populates_module_builder_with_zero_allocation() {
    let module = parsmith::compile_source(SCHEMA_EXTENSIONS, Some("ext.kdl")).unwrap();
    let opts = CodegenOptions::new(Target::Python);
    let mut emitter = PythonEmitter::new(opts);
    let mut builder = ModuleBuilder::new();
    let mut dry_writer = CodeWriter::dry_run();

    parsmith::drive_single_pass(
        &mut emitter,
        &module,
        &mut builder,
        &mut dry_writer,
        None,
        None,
    );

    assert_eq!(dry_writer.capacity(), 0);
    assert!(dry_writer.finish().is_empty());
    assert!(builder.imports().contains("from base64 import b64encode"));
    assert!(builder.has_runtime());
    assert_eq!(
        builder.runtime_names().collect::<Vec<_>>(),
        vec!["normalize_value"]
    );

    let (emitted, _) = drive_module(&mut emitter, &module, None, None);
    assert!(emitted.contains("def normalize_value(value: str) -> str:"));
    assert_eq!(emitted.matches("def normalize_value").count(), 1);
}

#[test]
fn snapshot_python_item_init_check_prevalidate() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("page_item.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_list_strings_basic() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_01_STRINGS_BASIC, Some("01_strings_basic.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_list_arrays_and_conversions() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(
        SCHEMA_02_ARRAYS_AND_CONVERSIONS,
        Some("02_arrays_and_conversions.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_list_filters_and_predicates() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(
        SCHEMA_03_FILTERS_AND_PREDICATES,
        Some("03_filters_and_predicates.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_flat_struct() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_05_FLAT, Some("05_flat.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_dict_struct() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_06_DICT, Some("06_dict.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_table_struct() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_07_TABLE, Some("07_table.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_raw_struct() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_23_RAW_STRUCT, Some("23_raw_struct.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_fn_and_raw_fn() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_25_FN, Some("25_fn.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_extensions_inline() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_EXTENSIONS, Some("extensions.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_models_dataclass() {
    let opts = CodegenOptions::new(Target::Python)
        .with_models(PythonModels::Dataclass)
        .with_bs4_parser(Bs4Parser::HtmlParser);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("page_item_dc.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_separate_runtime() {
    let opts = CodegenOptions::new(Target::Python)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res = generate_source(SCHEMA_EXTENSIONS, Some("ext_sep.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert_eq!(res.files.len(), 2);
    insta::assert_snapshot!("separate_runtime_parser", &res.files["ext_sep.py"]);
    insta::assert_snapshot!("separate_runtime_module", &res.files["parsmith_runtime.py"]);
}

#[test]
fn vfs_import_and_missing_extension_target_diagnostics() {
    let mut vfs = MemorySourceLoader::new();
    vfs.insert(
        "shared/ext.kdl",
        r#"
extension OnlyJs {
    noop {
        sig str str
        js { emit "const {{out}} = {{in}};" }
    }
}
"#,
    );
    let main_src = r#"
import "./shared/ext.kdl" { (extension)OnlyJs }
(raw)fn test_fn { !OnlyJs.noop }
"#;
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source_with_loader(main_src, Some("main.kdl"), &vfs, &opts);
    assert!(res.has_errors());
    assert_eq!(res.diagnostics[0].code, DiagnosticCode::E400);
    assert!(
        res.diagnostics[0]
            .message
            .contains("extension operation 'OnlyJs.noop' has no 'py' target")
    );
}

#[test]
fn xpath_operations_rejected_in_python_bs4_and_selectolax_with_e203() {
    for (lib, expected_backend) in [
        (PythonHtmlLib::Bs4, "python-bs4"),
        (PythonHtmlLib::Selectolax, "python-selectolax"),
    ] {
        let py_opts = CodegenOptions::new(Target::Python).with_html_lib(lib);

        // Full schema containing xpath, xpath-all, filter { xpath }, and xpath-remove
        let res = generate_source(SCHEMA_XPATH_BS4, Some("m_xpath.kdl"), &py_opts);
        assert!(res.has_errors());
        assert_eq!(res.diagnostics.len(), 4);
        assert!(
            res.diagnostics
                .iter()
                .all(|d| d.code == DiagnosticCode::E203 && d.message.contains(expected_backend))
        );

        // Individual XPath operations
        let cases = [
            r#"struct S { f { xpath "//h1"; text } }"#,
            r#"struct S { f { xpath-all "//p"; text } }"#,
            r#"struct S { f { xpath-remove "//script"; text } }"#,
            r#"struct S { f { css-all "div"; filter { xpath ".//a" }; text } }"#,
        ];
        for src in cases {
            let r = generate_source(src, Some("case.kdl"), &py_opts);
            assert!(
                r.has_errors(),
                "expected E203 for {expected_backend}: {src}"
            );
            assert_eq!(r.diagnostics[0].code, DiagnosticCode::E203);
            assert!(r.diagnostics[0].message.contains(expected_backend));
        }
    }

    // XPath-capable Python backends (lxml and parsel) accept XPath operations
    for lib in [PythonHtmlLib::Lxml, PythonHtmlLib::Parsel] {
        let py_opts = CodegenOptions::new(Target::Python).with_html_lib(lib);
        let res = generate_source(SCHEMA_XPATH_BS4, Some("m_xpath.kdl"), &py_opts);
        assert!(
            !res.has_errors(),
            "unexpected error for {lib}: {:?}",
            res.diagnostics
        );
    }

    // TypeScript and JavaScript targets continue to emit valid XPath code
    let ts_res = generate_source(
        SCHEMA_XPATH_BS4,
        Some("m_xpath.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!ts_res.has_errors(), "{:?}", ts_res.diagnostics);
    let js_res = generate_source(
        SCHEMA_XPATH_BS4,
        Some("m_xpath.kdl"),
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(!js_res.has_errors(), "{:?}", js_res.diagnostics);
}

#[test]
fn python_html_lib_parsing_and_target_shorthands() {
    use std::str::FromStr;

    assert_eq!(PythonHtmlLib::from_str("bs4").unwrap(), PythonHtmlLib::Bs4);
    assert_eq!(
        PythonHtmlLib::from_str("BeautifulSoup4").unwrap(),
        PythonHtmlLib::Bs4
    );
    assert_eq!(
        PythonHtmlLib::from_str("lxml").unwrap(),
        PythonHtmlLib::Lxml
    );
    assert_eq!(
        PythonHtmlLib::from_str("parsel").unwrap(),
        PythonHtmlLib::Parsel
    );
    assert_eq!(
        PythonHtmlLib::from_str("selectolax").unwrap(),
        PythonHtmlLib::Selectolax
    );
    assert_eq!(
        PythonHtmlLib::from_str("slax").unwrap(),
        PythonHtmlLib::Selectolax
    );
    assert_eq!(
        PythonHtmlLib::from_str("SLAX").unwrap(),
        PythonHtmlLib::Selectolax
    );
    assert_eq!(PythonHtmlLib::Selectolax.as_str(), "selectolax");
    assert_eq!(
        PythonHtmlLib::Selectolax.backend_name(),
        "python-selectolax"
    );

    assert_eq!(
        parse_target_or_shorthand("py-bs4").unwrap(),
        (Target::Python, Some(PythonHtmlLib::Bs4))
    );
    assert_eq!(
        parse_target_or_shorthand("python-lxml").unwrap(),
        (Target::Python, Some(PythonHtmlLib::Lxml))
    );
    assert_eq!(
        parse_target_or_shorthand("py-parsel").unwrap(),
        (Target::Python, Some(PythonHtmlLib::Parsel))
    );
    assert_eq!(
        parse_target_or_shorthand("py-selectolax").unwrap(),
        (Target::Python, Some(PythonHtmlLib::Selectolax))
    );
    assert_eq!(
        parse_target_or_shorthand("python-selectolax").unwrap(),
        (Target::Python, Some(PythonHtmlLib::Selectolax))
    );
    assert_eq!(
        parse_target_or_shorthand("py-slax").unwrap(),
        (Target::Python, Some(PythonHtmlLib::Selectolax))
    );
    assert_eq!(
        parse_target_or_shorthand("python-slax").unwrap(),
        (Target::Python, Some(PythonHtmlLib::Selectolax))
    );
}

#[test]
fn python_emitter_merges_and_deduplicates_from_module_imports() {
    use parsmith::LanguageEmitter;

    let module = parsmith::compile_source(SCHEMA_25_FN, Some("25_fn.kdl")).unwrap();
    let mut emitter = PythonEmitter::new(CodegenOptions::new(Target::Python));
    let mut builder = ModuleBuilder::new();
    builder
        .require_import("from typing import TypedDict, Any")
        .require_import("from dataclasses import field")
        .require_import("from typing import Literal, TypeAlias, Any")
        .require_import("from dataclasses import dataclass, field")
        .require_import("from typing import cast");

    let mut writer = CodeWriter::emit();
    emitter.emit_imports(&module, &mut builder, &mut writer);
    let out = writer.finish();

    assert!(out.contains("from dataclasses import dataclass, field\n"));
    assert_eq!(out.matches("from dataclasses import").count(), 1);
    assert!(out.contains("from typing import Any, Literal, TypeAlias, TypedDict, cast\n"));
    assert_eq!(out.matches("from typing import").count(), 1);
}

#[test]
fn python_toolchain_pytest_and_mypy_strict() {
    let tmp = TempTestDir::new("toolchain");
    tmp.write("dsl_coverage.html", HTML_FIXTURE);

    let schemas = [
        ("m_item.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("m_strings.kdl", SCHEMA_01_STRINGS_BASIC),
        ("m_arrays.kdl", SCHEMA_02_ARRAYS_AND_CONVERSIONS),
        ("m_filters.kdl", SCHEMA_03_FILTERS_AND_PREDICATES),
        ("m_flat.kdl", SCHEMA_05_FLAT),
        ("m_dict.kdl", SCHEMA_06_DICT),
        ("m_table.kdl", SCHEMA_07_TABLE),
        ("m_raw.kdl", SCHEMA_23_RAW_STRUCT),
        ("m_fn.kdl", SCHEMA_25_FN),
        ("m_ext.kdl", SCHEMA_EXTENSIONS),
        ("m_attr_contract.kdl", SCHEMA_BS4_ATTR_CONTRACT),
    ];

    let mut generated_files: Vec<PathBuf> = Vec::new();

    // 1. Generate all schemas in TypedDict mode
    let td_opts = CodegenOptions::new(Target::Python).with_models(PythonModels::TypedDict);
    for (name, src) in &schemas {
        let res = generate_source(src, Some(name), &td_opts);
        assert!(
            !res.has_errors(),
            "TypedDict generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 2. Generate key schemas in Dataclass mode
    let dc_schemas = [
        ("m_item_dc.kdl", SCHEMA_ITEM_INIT_CHECK),
        ("m_strings_dc.kdl", SCHEMA_01_STRINGS_BASIC),
        ("m_table_dc.kdl", SCHEMA_07_TABLE),
        ("m_raw_dc.kdl", SCHEMA_23_RAW_STRUCT),
    ];
    let dc_opts = CodegenOptions::new(Target::Python)
        .with_models(PythonModels::Dataclass)
        .with_bs4_parser(Bs4Parser::HtmlParser);
    for (name, src) in &dc_schemas {
        let res = generate_source(src, Some(name), &dc_opts);
        assert!(
            !res.has_errors(),
            "Dataclass generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            generated_files.push(tmp.write(fname, code));
        }
    }

    // 3. Generate separate runtime (-R) modules
    let sep_opts = CodegenOptions::new(Target::Python)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res_sep_ext = generate_source(SCHEMA_EXTENSIONS, Some("m_ext_sep.kdl"), &sep_opts);
    assert!(!res_sep_ext.has_errors());
    for (fname, code) in &res_sep_ext.files {
        generated_files.push(tmp.write(fname, code));
    }
    let res_sep_table = generate_source(SCHEMA_07_TABLE, Some("m_table_sep.kdl"), &sep_opts);
    assert!(!res_sep_table.has_errors());
    generated_files.push(tmp.write("m_table_sep.py", &res_sep_table.files["m_table_sep.py"]));

    // 4. Write comprehensive pytest + mypy test suite
    let test_suite_py = r#"from __future__ import annotations

from pathlib import Path
import pytest

import m_item
import m_strings
import m_arrays
import m_filters
import m_flat
import m_dict
import m_table
import m_raw
import m_fn
import m_ext
import m_item_dc
import m_strings_dc
import m_table_dc
import m_raw_dc
import m_ext_sep
import m_table_sep
import m_attr_contract
import parsmith_runtime


@pytest.fixture(scope="module")
def html() -> str:
    return (Path(__file__).parent / "dsl_coverage.html").read_text(encoding="utf-8")


def test_item_init_check_prevalidate_typeddict_and_dataclass(html: str) -> None:
    parser = m_item.PageItem(html)
    assert parser.has_cards() is True
    data = parser.parse()
    assert data["heading"] == "Coverage Root"
    assert data["canonical"] == "https://example.test/coverage"
    assert data["single"] == {"title": "Single Item Title", "active": True}

    dc_parser = m_item_dc.PageItem(html, features="lxml")
    assert dc_parser.has_cards() is True
    dc_data = dc_parser.parse()
    assert isinstance(dc_data, m_item_dc.PageItemType)
    assert dc_data.heading == "Coverage Root"
    assert dc_data.canonical == "https://example.test/coverage"
    assert isinstance(dc_data.single, m_item_dc.SingleCardType)
    assert dc_data.single.title == "Single Item Title"
    assert dc_data.single.active is True

    with pytest.raises((m_item.RtAssertionError, ValueError)):
        m_item.PageItem("<html><body></body></html>").parse()


def test_strings_basic_typeddict_and_dataclass(html: str) -> None:
    res = m_strings.StringsBasic(html).parse()
    assert len(res) == 2
    assert res[0]["title"] == "bar Title One"
    assert res[0]["link"] == "https://example.testguide"
    assert res[0]["slug"] == "slug-one"
    assert res[0]["normalized_code"] == "foo-bar"
    assert res[0]["active_flag"] is True
    assert "<h2 class=\"title\">" in res[0]["inner_html"]
    assert len(res[0]["inner_html"]) < len(res[0]["clean_html"])

    dc_res = m_strings_dc.StringsBasic(html).parse()
    assert len(dc_res) == 2
    assert isinstance(dc_res[0], m_strings_dc.StringsBasicType)
    assert dc_res[0].title == "bar Title One"
    assert dc_res[1].title == "bar Title Two"
    assert dc_res[1].normalized_code == "code-4"


def test_arrays_and_conversions(html: str) -> None:
    res = m_arrays.ArraysAndConversions(html).parse()
    assert len(res) == 2
    item0 = res[0]
    assert item0["token_list"] == ["tag-core-alpha1-x", "item-beta2-y", "ref3"]
    assert item0["first_token"] == "tag-core-alpha1-x"
    assert item0["second_token"] == "item-beta2-y"
    assert item0["last_token"] == "ref3"
    assert item0["token_window"] == "tag-core-alpha1-x|item-beta2-y"
    assert item0["state_code"] == 1
    assert item0["score"] == 41
    assert item0["ratio"] == 0.75
    assert item0["any_link_count"] == 2
    assert item0["flags_joined"] == "ALPHA,BETA"


def test_filters_and_predicates(html: str) -> None:
    res = m_filters.FiltersAndPredicates(html).parse()
    assert len(res) == 2
    assert res[0]["filtered_links"] == []
    assert res[0]["logic_check"] == "prealpha"
    assert res[0]["numeric_check"] == ""
    assert res[1]["numeric_check"] == "code-4"

    inline_card = (
        '<div class="card">'
        '<a href="/guide.html" data-kind="nav" data-alt="alt-1" rel="noopener">'
        '<span class="icon"></span>Go guide now</a>'
        '<div class="logic">prealpha</div>'
        '<div class="code">code-8</div>'
        '</div>'
    )
    res_inline = m_filters.FiltersAndPredicates(inline_card).parse()
    assert res_inline == [
        {
            "filtered_links": ["/guide.html", "alt-1"],
            "logic_check": "prealpha",
            "numeric_check": "code-8",
        }
    ]


def test_flat_and_dict_structs(html: str) -> None:
    flat_res = m_flat.FlatCoverage(html).parse()
    assert isinstance(flat_res, list)
    assert "/nested/one.html" in flat_res
    assert "tag-core-alpha1-x" in flat_res

    meta_dict = m_dict.MetaDict(html).parse()
    assert meta_dict["description"] == "Coverage description"

    dict_root = m_dict.DictRoot(html).parse()
    assert dict_root["named_meta"]["keywords"] == "alpha,beta,core"
    assert dict_root["alias_meta"]["og:title"] == "Coverage OG Title"


def test_table_typeddict_dataclass_and_separate_runtime(html: str) -> None:
    td_res = m_table.TableCoverage(html).parse()
    assert td_res == {
        "identifier": "ABC-123",
        "code_value": "CODE-1",
        "price": 9.99,
        "tax_or_fee": 1.25,
        "state": "active",
    }

    dc_res = m_table_dc.TableCoverage(html).parse()
    assert isinstance(dc_res, m_table_dc.TableCoverageType)
    assert dc_res.identifier == "ABC-123"
    assert dc_res.price == 9.99
    assert dc_res.state == "active"

    sep_res = m_table_sep.TableCoverage(html).parse()
    assert sep_res == td_res


def test_raw_structs_and_functions(html: str) -> None:
    js_text = '<script>var player = new Playerjs({id:"player",file:"/v/list/abc.txt"});</script>'
    url_text = "https://ru.yummyani.me/iframeCVH.html?dubbing_code=Sanae&anime_id=339&episode=1"
    playlist_text = "[480p]/v/anime/01_480p.m3u8\n[720p]/v/anime/01_720p.m3u8"

    p_td = m_raw.RawPlayerScript(js_text).parse()
    assert p_td == {"playlist_url": "/v/list/abc.txt", "player_id": "player"}

    p_dc = m_raw_dc.RawPlayerScript(js_text).parse()
    assert p_dc.playlist_url == "/v/list/abc.txt"
    assert p_dc.player_id == "player"

    u_td = m_raw.RawUrlParams(url_text).parse()
    assert u_td == {"dubbing_code": "Sanae", "anime_id": 339}

    lines_dc = m_raw_dc.RawLineItems(playlist_text).parse()
    assert len(lines_dc) == 2
    assert lines_dc[0].quality == "480p"
    assert lines_dc[1].url == "/v/anime/01_720p.m3u8"

    t_td = m_raw.RawTextTransform("hello   world").parse()
    assert t_td == {"upper_text": "HELLO   WORLD", "word_count": 2, "first_word": "hello"}

    assert m_fn.page_title("<html><body><h1>Hello World</h1></body></html>") == "Hello World"
    assert m_fn.all_links("<html><body><a href='/a'>A</a><a href='/b'>B</a></body></html>") == ["/a", "/b"]
    assert m_fn.first_line("first\nsecond") == "first"
    assert m_fn.extract_version("app version=1.2.3 released") == "1.2.3"


def test_extensions(html: str) -> None:
    ext_parser = m_ext.ExtRawParser("  hello   world  ")
    assert ext_parser.is_valid() is True
    assert ext_parser.parse() == {"encoded": "aGVsbG8gd29ybGQ="}
    assert m_ext.encode_normalized("  hello   world  ") == "aGVsbG8gd29ybGQ="

    sep_parser = m_ext_sep.ExtRawParser("  hello   world  ")
    assert sep_parser.parse() == {"encoded": "aGVsbG8gd29ybGQ="}
    assert parsmith_runtime.normalize_value("  a   b  ") == "a b"


def test_bs4_attr_contract_keyerror_fallback_and_multivalued_attrs() -> None:
    sample = (
        '<div>'
        '<a class="primary btn" rel="noopener noreferrer" href="/one">One</a>'
        '<a class="secondary" rel="nofollow" href="/two">Two</a>'
        '</div>'
    )
    out = m_attr_contract.Bs4AttrContract(sample).parse()
    assert out == {
        "missing_with_fallback": "FALLBACK_TRIGGERED",
        "single_class": "primary btn",
        "multi_key_attrs": ["noopener noreferrer", "primary btn"],
        "all_classes": ["primary btn", "secondary"],
        "eq_multi_val": ["/one"],
        "ne_multi_val": ["/two"],
    }
"#;
    let test_suite_path = tmp.write("test_generated_suite.py", test_suite_py);
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

#[test]
fn python_struct_value_constants_and_postfix_autocast() {
    let schema = r#"
define SOURCE="habr"

struct FeedCard {
    @init {
        seed_prefix str value="{{SOURCE}}" { upper }
    }
    source str value="{{SOURCE}}"
    kind value="article"
    version int value=1
    ratio float value=1.5
    is_live bool value=#false
    prefixed_id str value="{{SOURCE}}" { fmt "{{}}-01" }
    init_ref str { @seed_prefix }
    has_badge bool { css ".badge" }
    views int { css ".views"; text; fallback 0 }
    rating float { css ".rating"; text; fallback 0.0 }
}
"#;
    let res = generate_source(
        schema,
        Some("feed_card.kdl"),
        &CodegenOptions::new(Target::Python),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("v1 = 'habr'"));
    assert!(code.contains("v1 = 'article'"));
    assert!(code.contains("v1 = 1"));
    assert!(code.contains("v1 = 1.5"));
    assert!(code.contains("v1 = False"));
    assert!(code.contains("v2 = '{}-01'.format(v1)"));
    assert!(code.contains("v2 = v1 is not None"));
    assert!(code.contains("v3 = int(v2)"));
    assert!(code.contains("v3 = float(v2)"));
}

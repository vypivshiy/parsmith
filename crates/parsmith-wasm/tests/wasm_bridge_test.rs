use parsmith_wasm::{
    CompileAndLintResult, JsonToKdlResult, PrepareTestHtmlResult, compile_and_lint,
    compile_and_lint_result, json_to_kdl, prepare_test_html, prepare_test_html_result, version,
};

const SAMPLE_HTML_SCHEMA: &str = r#"
struct Author {
    name { css ".author-name"; text }
    bio {
        css ".author-bio"
        text
        fallback #null
    }
}

(list)struct Catalogue {
    @split-doc { css-all ".card" }
    @pre-validate {
        assert {
            css ".catalogue"
            not { xpath "//div[@class='blocked']" }
        }
    }
    @init {
        base_url {
            css "base"
            attr "href"
            fallback "https://example.com"
        }
    }
    @check has_cards {
        css-all ".card"
        len
        assert { gt 0 }
        to-bool
    }

    title {
        css { "h1.main-title"; "h2.sub-title" }
        css-remove ".ad-badge"
        text
    }
    tags {
        xpath-all "//ul[@class='tags']/li"
        xpath-remove ".//span[@class='hidden']"
        filter {
            or {
                css "a.active"
                xpath "./self::li[@data-highlight]"
            }
        }
        text
        fallback {}
    }
    count {
        xpath "//span[@class='count']"
        text
        to-int
        fallback 0
    }
    rating {
        css ".rating"
        text
        to-float
        fallback 4.5
    }
    featured {
        css ".featured"
        to-bool
        fallback #false
    }
    active {
        css ".active"
        to-bool
        fallback #true
    }
    author {
        css ".author-box"
        nested Author
    }
}
"#;

#[test]
fn test_compile_and_lint_default_and_all_five_targets() {
    assert_eq!(version(), parsmith::version());

    // 1. Default (empty target_json -> Python bs4)
    let default_raw = compile_and_lint(SAMPLE_HTML_SCHEMA, "");
    let default_res: CompileAndLintResult = serde_json::from_str(&default_raw).unwrap();
    // Note: SAMPLE_HTML_SCHEMA uses xpath, which on default Python bs4 emits E203 codegen error
    assert!(!default_res.ok);
    assert!(
        default_res
            .target_error
            .as_deref()
            .unwrap_or("")
            .contains("XPath")
    );
    assert_eq!(default_res.structs.len(), 2);

    // Pure CSS schema for all 5 targets
    let css_schema = r#"
struct Author {
    name { css ".name"; text }
}

(list)struct BookList {
    @split-doc { css-all ".book" }
    title { css "h2"; text }
    author { css ".author"; nested Author }
    @request "curl https://example.com/books"
}
"#;

    // Python (bs4 + dataclass + html.parser + sync + separate_runtime)
    let py_res = compile_and_lint_result(
        css_schema,
        r#"{
            "lang": "python",
            "lib": "bs4",
            "models": "dataclass",
            "bs4_parser": "html.parser",
            "http_io": "sync",
            "separate_runtime": true
        }"#,
    );
    assert!(py_res.ok, "unexpected error: {:?}", py_res.target_error);
    assert!(py_res.files.contains_key("parser.py"));
    assert!(py_res.files.contains_key("parsmith_runtime.py"));
    let py_code = &py_res.files["parser.py"];
    assert!(py_code.contains("@dataclass(slots=True)"));
    assert!(py_code.contains("html.parser"));
    assert!(py_code.contains("def fetch("));
    assert!(!py_code.contains("async def async_fetch("));

    // Python selectolax via legacy "slax" alias
    let slax_res = compile_and_lint_result(css_schema, r#"{"lang": "python", "lib": "slax"}"#);
    assert!(slax_res.ok);
    assert!(
        slax_res.files["parser.py"].contains("LexborHTMLParser")
            || slax_res.files["parser.py"].contains("selectolax")
    );

    // Python lxml supports XPath
    let lxml_res = compile_and_lint_result(
        SAMPLE_HTML_SCHEMA,
        r#"{"lang": "python", "html_lib": "lxml"}"#,
    );
    assert!(lxml_res.ok, "lxml should support XPath: {:?}", lxml_res);
    assert!(lxml_res.files.contains_key("parser.py"));

    // TypeScript
    let ts_res = compile_and_lint_result(css_schema, r#"{"lang": "ts"}"#);
    assert!(ts_res.ok);
    assert!(ts_res.files.contains_key("parser.ts"));
    assert_eq!(ts_res.files.len(), 1);

    // JavaScript (.js + .d.ts)
    let js_res = compile_and_lint_result(css_schema, r#"{"lang": "js"}"#);
    assert!(js_res.ok);
    assert!(js_res.files.contains_key("parser.js"));
    assert!(js_res.files.contains_key("parser.d.ts"));

    // Go (custom package + separate_runtime)
    let go_res = compile_and_lint_result(
        css_schema,
        r#"{"lang": "go", "package": "catalog", "separate_runtime": true}"#,
    );
    assert!(go_res.ok);
    assert!(go_res.files.contains_key("parser.go"));
    assert!(go_res.files.contains_key("parsmith_runtime.go"));
    assert!(go_res.files["parser.go"].contains("package catalog"));

    // Rust (separate_runtime)
    let rs_res = compile_and_lint_result(
        css_schema,
        r#"{"target": "rust", "separate_runtime": true}"#,
    );
    assert!(rs_res.ok);
    assert!(rs_res.files.contains_key("parser.rs"));
    assert!(rs_res.files.contains_key("parsmith_runtime.rs"));

    // Elixir (custom package + separate_runtime)
    let ex_res = compile_and_lint_result(
        css_schema,
        r#"{"lang": "elixir", "package": "MyApp.Parsers", "separate_runtime": true}"#,
    );
    assert!(ex_res.ok);
    assert!(ex_res.files.contains_key("parser.ex"));
    assert!(ex_res.files.contains_key("parsmith_runtime.ex"));
    assert!(ex_res.files["parser.ex"].contains("defmodule MyApp.Parsers.BookList do"));

    // PHP native & dom-crawler
    let php_native_res = compile_and_lint_result(
        SAMPLE_HTML_SCHEMA,
        r#"{"lang": "php", "lib": "native", "package": "App\\Parsers", "separate_runtime": true}"#,
    );
    assert!(
        php_native_res.ok,
        "PHP native should support CSS and XPath: {:?}",
        php_native_res.target_error
    );
    assert!(php_native_res.files.contains_key("parser.php"));
    assert!(php_native_res.files.contains_key("parsmith_runtime.php"));
    assert!(php_native_res.files["parser.php"].contains("namespace App\\Parsers;"));

    let php_crawler_res =
        compile_and_lint_result(SAMPLE_HTML_SCHEMA, r#"{"lang": "php-dom-crawler"}"#);
    assert!(php_crawler_res.ok);
    assert!(
        php_crawler_res.files["parser.php"].contains("Symfony\\Component\\DomCrawler\\Crawler")
    );

    // C# (supports XPath)
    let cs_res = compile_and_lint_result(
        SAMPLE_HTML_SCHEMA,
        r#"{"lang": "csharp", "package": "MyApp.Parsers", "separate_runtime": true}"#,
    );
    assert!(cs_res.ok);
    assert!(cs_res.files.contains_key("parser.cs"));
    assert!(cs_res.files.contains_key("parsmith_runtime.cs"));
    assert!(cs_res.files["parser.cs"].contains("namespace MyApp.Parsers;"));
    assert!(cs_res.files["parser.cs"].contains("using AngleSharp.XPath;"));

    // Kotlin (supports XPath)
    let kt_res = compile_and_lint_result(
        SAMPLE_HTML_SCHEMA,
        r#"{"lang": "kotlin", "package": "com.example.parsers", "separate_runtime": true}"#,
    );
    assert!(kt_res.ok);
    assert!(kt_res.files.contains_key("parser.kt"));
    assert!(kt_res.files.contains_key("parsmith_runtime.kt"));
    assert!(kt_res.files["parser.kt"].contains("package com.example.parsers"));
}

#[test]
fn test_compile_and_lint_target_errors_still_populate_structs_and_diagnostics() {
    let schema = r#"
struct Item {
    title { css "h1"; text }
}
"#;

    let bad_lang = compile_and_lint_result(schema, r#"{"lang": "cobol"}"#);
    assert!(!bad_lang.ok);
    assert!(bad_lang.target_error.is_some());
    assert_eq!(bad_lang.structs.len(), 1);
    assert_eq!(bad_lang.structs[0].name, "Item");

    let bad_go_pkg = compile_and_lint_result(schema, r#"{"lang": "go", "package": "func"}"#);
    assert!(!bad_go_pkg.ok);
    assert!(
        bad_go_pkg
            .target_error
            .as_deref()
            .unwrap_or("")
            .contains("Invalid Go package name")
    );
    assert_eq!(bad_go_pkg.structs.len(), 1);

    let bad_json = compile_and_lint_result(schema, "{not valid json");
    assert!(!bad_json.ok);
    assert!(
        bad_json
            .target_error
            .as_deref()
            .unwrap_or("")
            .contains("Invalid target JSON")
    );
    assert_eq!(bad_json.structs.len(), 1);
}

#[test]
fn test_compile_and_lint_diagnostics_and_cst_struct_fallback() {
    // 1. E003 import diagnostic in single-buffer playground
    let import_schema = r#"import "./external.kdl" {
    (struct)ExternalItem
}

struct LocalItem {
    title { css "h1"; text }
}
"#;
    let import_res = compile_and_lint_result(import_schema, r#"{"lang": "python"}"#);
    assert!(!import_res.ok);
    let import_codes: Vec<&str> = import_res
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        import_codes.contains(&"E003"),
        "expected E003 in {import_codes:?}"
    );
    assert_eq!(import_res.structs.len(), 1);
    assert_eq!(import_res.structs[0].name, "LocalItem");

    // 2. E100 pipeline type mismatch with CST struct fallback preserving referenced_by
    let broken_schema = r#"
struct Child {
    name { css ".name"; text }
}

(list)struct Parent {
    @split-doc { css-all ".row" }
    bad_field {
        to-int
    }
    child {
        css ".child"
        nested Child
    }
}
"#;

    let res = compile_and_lint_result(broken_schema, r#"{"lang": "python"}"#);
    assert!(!res.ok);
    assert!(res.files.is_empty());

    let codes: Vec<&str> = res.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"E100"), "expected E100 in {codes:?}");

    for diag in &res.diagnostics {
        assert!(diag.span.start.line >= 1);
        assert!(diag.span.start.column >= 1);
        assert!(diag.span.end.line >= diag.span.start.line);
        assert!(diag.span.end.column >= 1);
        assert!(diag.span.end.offset >= diag.span.start.offset);
    }

    // Fallback CST struct scan should still find Child and Parent with referenced_by populated
    assert_eq!(res.structs.len(), 2);
    assert_eq!(res.structs[0].name, "Child");
    assert_eq!(res.structs[0].class_name, "Child");
    assert_eq!(res.structs[0].struct_type, "item");
    assert_eq!(res.structs[0].referenced_by, vec!["Parent".to_owned()]);

    assert_eq!(res.structs[1].name, "Parent");
    assert_eq!(res.structs[1].struct_type, "list");
    assert!(res.structs[1].referenced_by.is_empty());
}

#[test]
fn test_prepare_test_html_selector_extraction_and_struct_selection() {
    // Auto-selects Catalogue (the root non-REST struct) even if a REST struct follows it
    let schema_with_rest = format!(
        "{SAMPLE_HTML_SCHEMA}\n\njson Ping {{ ok bool }}\n(rest)struct ApiClient {{\n    @request \"curl https://example.com/ping\" response=\"Ping\"\n}}\n"
    );

    let raw = prepare_test_html(&schema_with_rest, None);
    let res: PrepareTestHtmlResult = serde_json::from_str(&raw).unwrap();
    assert!(res.ok, "unexpected error: {:?}", res.error);
    assert_eq!(res.struct_name.as_deref(), Some("Catalogue"));
    assert_eq!(res.class_name.as_deref(), Some("Catalogue"));
    assert_eq!(res.struct_kind.as_deref(), Some("list"));
    assert!(
        res.js_code
            .as_deref()
            .unwrap_or("")
            .contains("export class Catalogue")
    );

    let paths: Vec<&str> = res.selectors.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "Catalogue.@split-doc",
            "Catalogue.@pre-validate",
            "Catalogue.@pre-validate",
            "Catalogue.@init.base_url",
            "Catalogue.@check:has_cards",
            "Catalogue.title",
            "Catalogue.title",
            "Catalogue.tags",
            "Catalogue.tags",
            "Catalogue.tags",
            "Catalogue.tags",
            "Catalogue.count",
            "Catalogue.rating",
            "Catalogue.featured",
            "Catalogue.active",
            "Catalogue.author",
            "Catalogue.author[Author].name",
            "Catalogue.author[Author].bio",
        ]
    );

    // Check multi-query report_query and fallbacks
    let title_css = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.title" && s.selector_type == "css")
        .unwrap();
    assert_eq!(title_css.queries, vec!["h1.main-title", "h2.sub-title"]);
    assert_eq!(title_css.report_query, "h1.main-title || h2.sub-title");
    assert_eq!(title_css.fallback_value, None);

    let title_rm = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.title" && s.selector_type == "css-remove")
        .unwrap();
    assert_eq!(title_rm.report_query, ".ad-badge");

    let base_url_sel = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.@init.base_url")
        .unwrap();
    assert_eq!(
        base_url_sel.fallback_value.as_deref(),
        Some("\"https://example.com\"")
    );

    let tags_sel = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.tags" && s.selector_type == "xpath-all")
        .unwrap();
    assert_eq!(tags_sel.fallback_value.as_deref(), Some("[]"));

    let count_sel = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.count")
        .unwrap();
    assert_eq!(count_sel.fallback_value.as_deref(), Some("0"));

    let rating_sel = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.rating")
        .unwrap();
    assert_eq!(rating_sel.fallback_value.as_deref(), Some("4.5"));

    let featured_sel = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.featured")
        .unwrap();
    assert_eq!(featured_sel.fallback_value.as_deref(), Some("#false"));

    let active_sel = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.active")
        .unwrap();
    assert_eq!(active_sel.fallback_value.as_deref(), Some("#true"));

    let bio_sel = res
        .selectors
        .iter()
        .find(|s| s.path == "Catalogue.author[Author].bio")
        .unwrap();
    assert_eq!(bio_sel.fallback_value.as_deref(), Some("#null"));
}

#[test]
fn test_prepare_test_html_table_dict_raw_and_rest_rejection() {
    let table_and_dict = r##"
(dict)struct MetaDict {
    @split-doc { css-all "dl > div" }
    @key { css "dt"; text }
    @value { css "dd"; text }
}

(table)struct SpecsTable {
    @table { css "table.specs" }
    @rows { css-all "tr" }
    @match { css "th"; text }
    @value { css "td"; text }
    weight {
        match { eq "Weight" }
    }
}

(raw)struct RawToken {
    token { re #"token=(\w+)"# }
}

json User { id int }
(rest)struct UserApi {
    @request "curl https://example.com/user" response="User"
}
"##;

    let dict_res = prepare_test_html_result(table_and_dict, Some("MetaDict"));
    assert!(dict_res.ok, "dict_res error: {:?}", dict_res.error);
    let dict_paths: Vec<&str> = dict_res.selectors.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        dict_paths,
        vec!["MetaDict.@split-doc", "MetaDict.@key", "MetaDict.@value"]
    );

    let table_res = prepare_test_html_result(table_and_dict, Some("SpecsTable"));
    assert!(table_res.ok, "table_res error: {:?}", table_res.error);
    let table_paths: Vec<&str> = table_res
        .selectors
        .iter()
        .map(|s| s.path.as_str())
        .collect();
    assert_eq!(
        table_paths,
        vec![
            "SpecsTable.@value",
            "SpecsTable.@table",
            "SpecsTable.@rows",
            "SpecsTable.@match",
        ]
    );

    let raw_res = prepare_test_html_result(table_and_dict, Some("RawToken"));
    assert!(raw_res.ok, "raw_res error: {:?}", raw_res.error);
    assert_eq!(raw_res.struct_kind.as_deref(), Some("raw"));
    assert!(raw_res.selectors.is_empty());
    assert!(
        raw_res
            .js_code
            .as_deref()
            .unwrap_or("")
            .contains("export class RawToken")
    );

    let rest_res = prepare_test_html_result(table_and_dict, Some("UserApi"));
    assert!(!rest_res.ok);
    assert_eq!(
        rest_res.error.as_deref(),
        Some(
            "Struct 'UserApi' is a (rest)struct. REST schemas support linting and code generation only; HTML parser execution and selector health checks are not supported."
        )
    );

    let missing_res = prepare_test_html_result(table_and_dict, Some("NonExistent"));
    assert!(!missing_res.ok);
    assert!(
        missing_res
            .error
            .as_deref()
            .unwrap_or("")
            .contains("Struct 'NonExistent' not found in schema. Available:")
    );
}

#[test]
fn test_json_to_kdl_success_and_error() {
    let raw_ok = json_to_kdl(r#"{"id": 1, "user_name": "alice", "tags": ["a"]}"#, None);
    let ok_res: JsonToKdlResult = serde_json::from_str(&raw_ok).unwrap();
    assert!(ok_res.ok);
    assert!(ok_res.error.is_none());
    assert!(ok_res.kdl.contains("json JsonResponse"));
    assert!(ok_res.kdl.contains("id int"));
    assert!(ok_res.kdl.contains("user_name str"));

    let raw_named = json_to_kdl(r#"[{"id": 1}]"#, Some("UsersList".to_owned()));
    let named_res: JsonToKdlResult = serde_json::from_str(&raw_named).unwrap();
    assert!(named_res.ok);
    assert!(named_res.kdl.contains("(array)json UsersList"));

    let raw_err = json_to_kdl("{invalid", None);
    let err_res: JsonToKdlResult = serde_json::from_str(&raw_err).unwrap();
    assert!(!err_res.ok);
    assert!(err_res.kdl.is_empty());
    assert!(err_res.error.is_some());
}

#[test]
fn test_warnings_preserve_ok_true_and_populate_diagnostics() {
    let warn_schema = r#"
json LegacyUser {
    id int "user_id"
    name str from=" profile.name "
    email str path="contact.email"
}

struct Holder {
    user {
        css "script#user"
        text
        jsonify LegacyUser
    }
}
"#;

    let res = compile_and_lint_result(warn_schema, r#"{"lang": "python"}"#);
    assert!(res.ok, "warnings should not fail compilation: {:?}", res);
    assert!(res.files.contains_key("parser.py"));
    let warn_codes: Vec<&str> = res.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(warn_codes.contains(&"W011"));
    assert!(warn_codes.contains(&"W040"));
    assert!(warn_codes.contains(&"W041"));
    for d in &res.diagnostics {
        assert_eq!(d.severity, "warning");
    }
}

#[test]
fn test_extension_missing_target_surfaces_codegen_error() {
    let ext_schema = r#"
extension Custom {
    clean {
        sig str str
        py {
            emit "{{out}} = {{in}}.strip()"
        }
    }
}

struct Doc {
    title {
        css "h1"
        text
        !Custom.clean
    }
}
"#;

    let py_ok = compile_and_lint_result(ext_schema, r#"{"lang": "python"}"#);
    assert!(py_ok.ok);
    assert!(py_ok.files.contains_key("parser.py"));

    let js_fail = compile_and_lint_result(ext_schema, r#"{"lang": "js"}"#);
    assert!(!js_fail.ok);
    assert!(js_fail.files.is_empty());
    assert!(
        js_fail
            .target_error
            .as_deref()
            .unwrap_or("")
            .contains("Custom.clean")
    );
    assert_eq!(js_fail.structs.len(), 1);

    let test_html_fail = prepare_test_html_result(ext_schema, None);
    assert!(!test_html_fail.ok);
    assert!(
        test_html_fail
            .error
            .as_deref()
            .unwrap_or("")
            .contains("Custom.clean")
    );
}

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use common::*;
use parsmith::codegen::rust::{rust_ident, rust_pascal, rust_replacement};
use parsmith::{
    CodeWriter, CodegenOptions, DiagnosticCode, ModuleBuilder, RustEmitter, Target, drive_module,
    emit_rust_module, emit_rust_runtime_module, generate_path, generate_source,
    parse_target_or_shorthand,
};

const SCHEMA_ITEM_INIT_CHECK: &str = r#"
@doc "Item struct with @init, @pre-validate, @check, and nested struct."

struct SingleCard {
    title {
        css "h1"
        text
        trim
    }
    active {
        css ".is-active"
        to-bool
    }
}

struct PageItem {
    @doc "Main page item parser."

    @init {
        canonical-url {
            css "link[rel='canonical']"
            attr "href"
            fallback ""
        }
    }

    @pre-validate {
        assert "missing single section" {
            css "section.single"
        }
    }

    @check has-cards {
        css ".card"
        to-bool
    }

    heading {
        css "body > h1"
        text
        trim
    }

    canonical {
        @canonical-url
    }

    single {
        css "section.single"
        nested SingleCard
    }
}
"#;

const SCHEMA_RAW_AND_FN: &str = r####"
@doc "Pure raw struct and functions without HTML DOM dependencies."

(raw)struct LogEntry {
    @split-doc {
        split "\n"
    }

    level {
        re #"^\[([A-Z]+)\]"#
        fallback "INFO"
    }

    message {
        re-sub #"^\[[A-Z]+\]\s*"# ""
        trim
    }
}

(raw)fn extract_version {
    @doc "Extract semantic version from raw string."
    re #"version=([0-9]+\.[0-9]+\.[0-9]+)"#
}
"####;

const SCHEMA_EXTENSIONS: &str = r####"
extension Utils {
    normalize {
        sig str str

        rust {
            helper normalize_value {
                import "use std::borrow::Cow;"
                source #"""
                    pub fn normalize_value(value: &str) -> String {
                        let trimmed: Cow<'_, str> = Cow::Borrowed(value.trim());
                        trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
                    }
                    """#
            }
            emit #"let {{out}} = normalize_value(&{{in}});"#
        }
    }

    truthy {
        sig T bool
        rust {
            emit #"let {{out}} = !{{in}}.is_empty();"#
        }
    }
}

(raw)struct ExtRawParser {
    @check is-valid {
        !Utils.truthy
    }

    normalized {
        !Utils.normalize
    }
}

(raw)fn normalize_text {
    !Utils.normalize
}
"####;

const SCHEMA_HTML_FETCH_REQUESTS: &str = r####"
@doc "HTML and Raw structs with @request, typed placeholders, query array styles, and response-join."

struct ArticlePage {
    @doc "Article HTML page parser with async reqwest fetch."

    @request doc="Fetch article by ID and query filters." """
    GET /articles/{{id:int}}?q={{q:str?}}&tags={{tags:str[]?|csv}}&pipes={{pipes:int[]?|pipe}}&spaces={{spaces:str[]?|space}}&brackets={{brackets:int[]?|bracket}}&repeats={{repeats:str[]?}} HTTP/1.1
    Host: example.com
    X-Static: parsmith-test
    X-Trace: {{trace:str?}}
    Cookie: sid={{sid:str}}; theme={{theme:str?}}
    """

    @request name="by-slug" """
    POST /articles/slug/{{slug:str}} HTTP/1.1
    Host: example.com
    Content-Type: application/x-www-form-urlencoded

    mode=preview&token={{token:str?}}
    """

    heading {
        css "h1"
        text
        trim
    }
}

(raw)struct ChunkedLog {
    @request response-path="payload.lines" response-join="\n" """
    GET /logs/{{stream_id:str}} HTTP/1.1
    Host: example.com
    """

    lines {
        split "\n"
    }
}
"####;

const SCHEMA_REST_CLIENT: &str = r####"
@doc "REST client coverage: typed @error / @error:<suffix> variants, Transport, Decode, and Unknown variants."

json User {
    id int
    name str
    nickname str?
    role str @omitempty
}

(array)json ArticleList {
    article_id int from="id"
    headline str from="title"
}

json ApiError {
    code int from="error.code"
    message str from="error.message"
}

json SpecificNotFoundError {
    error str
    code int
}

json GenericError {
    message str
}

(rest)struct UserApi {
    @doc "User service REST client."

    @error 200 ApiError "error.code"
    @error:not-found 404 SpecificNotFoundError error="Not Found"
    @error 404 GenericError
    @error 400 ApiError

    @request name="get-user" response=User doc="Fetch a user by ID." """
    GET /users/{{id:int}}?include_meta={{include_meta:bool?}} HTTP/1.1
    Host: api.example.com
    X-App: parsmith
    """

    @request name="create-user" response=User """
    POST /users?tag={{tag:str}}&filter={{filter:str?}} HTTP/1.1
    Host: api.example.com
    Content-Type: application/json
    X-Trace: {{trace:str?}}
    Cookie: sid={{sid:str}}; pref={{pref:str?}}

    {"name": "{{name:str}}", "nickname": {{nickname:str?}}, "active": {{active:bool}}}
    """

    @request name="get-profile-enveloped" response=User response-path="data.user" """
    GET /me HTTP/1.1
    Host: api.example.com
    """

    @request name="list-articles" response=ArticleList """
    GET /articles HTTP/1.1
    Host: api.example.com
    """

    @request name="ping" """
    GET /ping HTTP/1.1
    Host: api.example.com
    """
}
"####;

#[test]
fn snapshot_rust_item_init_check() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("item_init.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_rust_list_strings_arrays_conversions() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_LIST_STRINGS_ARRAYS, Some("list_strings.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_rust_filters_and_predicates() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(
        SCHEMA_FILTERS_AND_PREDICATES,
        Some("filters_preds.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_rust_flat_dict_table() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_FLAT_DICT_TABLE, Some("flat_dict_table.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_rust_raw_struct_and_fn() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_RAW_AND_FN, Some("raw_fn.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(
        !code.contains("dom_query"),
        "pure raw module must omit dom_query dependency"
    );
    insta::assert_snapshot!(code);
}

#[test]
fn snapshot_rust_html_fn() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_HTML_FN, Some("html_fn.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_rust_extensions_inline() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_EXTENSIONS, Some("ext_inline.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_rust_extensions_separate_runtime() {
    let opts = CodegenOptions::new(Target::Rust)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res = generate_source(SCHEMA_EXTENSIONS, Some("ext_sep.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert_eq!(res.files.len(), 2);
    insta::assert_snapshot!("rust_ext_separate_parser", &res.files["ext_sep.rs"]);
    insta::assert_snapshot!(
        "rust_ext_separate_runtime",
        &res.files["parsmith_runtime.rs"]
    );
}

#[test]
fn snapshot_rust_json_projectors() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(
        SCHEMA_CATALOG_JSON_PROJECTORS,
        Some("json_projectors.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(
        !code.contains("dom_query"),
        "pure JSON + raw module must omit dom_query dependency"
    );
    insta::assert_snapshot!(code);
}

#[test]
fn snapshot_rust_html_fetch_requests() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_HTML_FETCH_REQUESTS, Some("html_fetch.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_rust_rest_client() {
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(SCHEMA_REST_CLIENT, Some("rest_client.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(
        !code.contains("dom_query"),
        "pure REST module must omit dom_query dependency"
    );
    assert!(code.contains("Decode(u16, rt::RtError),"));
    assert!(code.contains("const RT_USER_API_MATCHERS: &[rt::ErrorMatcher<UserApiError>]"));
    assert!(!code.contains("SscError"));
    assert!(!code.contains("UNMATCHED_TABLE_ROW") || code.contains("RT_UNMATCHED_TABLE_ROW"));
    insta::assert_snapshot!(code);
}

#[test]
fn snapshot_rust_separate_runtime() {
    let opts = CodegenOptions::new(Target::Rust)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("page_sep.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert_eq!(res.files.len(), 2);
    insta::assert_snapshot!("rust_separate_runtime_parser", &res.files["page_sep.rs"]);
    insta::assert_snapshot!(
        "rust_separate_runtime_module",
        &res.files["parsmith_runtime.rs"]
    );
}

#[test]
fn rust_xpath_rejected_with_e203() {
    let schemas = [
        r#"struct A { x { xpath "//h1"; text } }"#,
        r#"struct B { x { xpath-all "//a"; text } }"#,
        r#"struct C { x { xpath-remove "//script"; text } }"#,
        r#"struct D { x { css-all "div"; filter { xpath ".//a" }; text } }"#,
    ];

    let opts = CodegenOptions::new(Target::Rust);
    for src in schemas {
        let res = generate_source(src, Some("xpath.kdl"), &opts);
        assert!(res.has_errors(), "expected E203 for schema: {src}");
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E203 && d.message.contains("'rust' backend")),
            "unexpected diagnostics: {:?}",
            res.diagnostics
        );
    }
}

#[test]
fn rust_missing_extension_target_reports_e400() {
    let src = r#"
extension OnlyPy {
    noop {
        sig str str
        py { emit "{{out}} = {{in}}" }
    }
}
(raw)fn run {
    !OnlyPy.noop
}
"#;
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(src, Some("missing_rust_ext.kdl"), &opts);
    assert!(res.has_errors());
    assert!(res.diagnostics.iter().any(|d| {
        d.code == DiagnosticCode::E400
            && d.message
                .contains("extension operation 'OnlyPy.noop' has no 'rust' target")
    }));
}

#[test]
fn rust_invalid_runtime_name_reports_e002() {
    let opts = CodegenOptions::new(Target::Rust)
        .with_separate_runtime(true)
        .with_runtime_name("123-bad-name");
    let res = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &opts);
    assert!(res.has_errors());
    assert!(
        res.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );
}

#[test]
fn rust_escapable_keywords_use_raw_identifiers() {
    let src = r#"
struct Data {
    ref { css "a"; text }
    type { css "b"; text }
    match { css "c"; text }
}
(rest)struct Api {
    @request name=get """
    GET /?pub={{pub}}&async={{async:bool?}} HTTP/1.1
    Host: example.com
    """
}
"#;
    let opts = CodegenOptions::new(Target::Rust);
    let res = generate_source(src, Some("keywords.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("pub r#ref: String,"));
    assert!(code.contains("pub r#type: String,"));
    assert!(code.contains("pub r#match: String,"));
    assert!(code.contains("fn r#ref(&mut self"));
    assert!(code.contains("fn r#type(&mut self"));
    assert!(code.contains("fn r#match(&mut self"));
    assert!(code.contains("r#pub: &str"));
    assert!(code.contains("r#async: Option<bool>"));
    assert_eq!(rust_ident("ref"), "r#ref");
    assert_eq!(rust_pascal("self"), "Self");
}

#[test]
fn rust_replacement_translates_groups_and_escapes_dollar() {
    assert_eq!(rust_replacement(r"\1-ok"), "$1-ok");
    assert_eq!(rust_replacement("$1-ok"), "$1-ok");
    assert_eq!(rust_replacement(r"\1_ok"), "${1}_ok");
    assert_eq!(rust_replacement("$1_ok"), "${1}_ok");
    assert_eq!(rust_replacement("price: $foo"), "price: $$foo");
    assert_eq!(rust_replacement("$$"), "$$");
    assert_eq!(rust_replacement("$"), "$$");
}

#[test]
fn rust_target_parsing_and_two_pass_dry_run_zero_allocation() {
    assert_eq!(
        parse_target_or_shorthand("rust").unwrap(),
        (Target::Rust, None)
    );
    assert_eq!(
        parse_target_or_shorthand("rs").unwrap(),
        (Target::Rust, None)
    );
    assert_eq!(Target::Rust.as_str(), "rust");
    assert_eq!(Target::Rust.ecosystem_name(), "rust");
    assert_eq!(Target::Rust.extension(), "rs");

    let module = parsmith::compile_source(SCHEMA_ITEM_INIT_CHECK, Some("item.kdl")).unwrap();
    let mut emitter = RustEmitter::new(CodegenOptions::new(Target::Rust));
    let mut dry_writer = CodeWriter::dry_run();
    let mut builder = ModuleBuilder::new();
    parsmith::drive_single_pass(
        &mut emitter,
        &module,
        &mut builder,
        &mut dry_writer,
        None,
        None,
    );
    assert_eq!(dry_writer.capacity(), 0);
    let (emitted, _) = drive_module(
        &mut emitter,
        &module,
        Some("// prologue"),
        Some("// epilogue"),
    );
    assert!(emitted.contains("// prologue"));
    assert!(emitted.contains("// epilogue"));
}

#[test]
fn rust_toolchain_check_and_execution() {
    let tmp = TempTestDir::new("toolchain");
    let cargo_toml = r#"[package]
name = "parsmith_rust_toolchain_test"
version = "0.1.0"
edition = "2021"

[dependencies]
dom_query = "0.28"
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", default-features = false, features = ["json"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
"#;
    tmp.write("Cargo.toml", cargo_toml);

    let schemas = [
        ("sep_item", SCHEMA_ITEM_INIT_CHECK),
        ("sep_list", SCHEMA_LIST_STRINGS_ARRAYS),
        ("sep_filters", SCHEMA_FILTERS_AND_PREDICATES),
        ("sep_flat_dict_table", SCHEMA_FLAT_DICT_TABLE),
        ("sep_raw", SCHEMA_RAW_AND_FN),
        ("sep_html_fn", SCHEMA_HTML_FN),
        ("sep_ext", SCHEMA_EXTENSIONS),
        ("sep_json", SCHEMA_CATALOG_JSON_PROJECTORS),
        ("sep_fetch", SCHEMA_HTML_FETCH_REQUESTS),
        ("sep_rest", SCHEMA_REST_CLIENT),
    ];

    let mut mod_decls = Vec::new();
    let mut all_rt_builder = ModuleBuilder::new();

    // 1. Emit each schema in inline runtime mode (`separate_runtime = false`) and separate runtime mode (`separate_runtime = true`)
    for (idx, (sep_name, src)) in schemas.iter().enumerate() {
        let inline_name = format!("inline_{idx}");
        let inline_res = generate_source(
            src,
            Some(&format!("{inline_name}.kdl")),
            &CodegenOptions::new(Target::Rust),
        );
        assert!(
            !inline_res.has_errors(),
            "inline Rust generation failed for {sep_name}: {:?}",
            inline_res.diagnostics
        );
        tmp.write(
            &format!("src/{inline_name}.rs"),
            inline_res.primary_code().unwrap(),
        );
        mod_decls.push(format!("pub mod {inline_name};"));

        let module = parsmith::compile_source(src, Some(&format!("{sep_name}.kdl"))).unwrap();
        let sep_opts = CodegenOptions::new(Target::Rust)
            .with_separate_runtime(true)
            .with_runtime_name("parsmith_runtime");
        let (parser_code, _, builder) =
            emit_rust_module(&module, &sep_opts).expect("emit_rust_module");
        tmp.write(&format!("src/{sep_name}.rs"), &parser_code);
        mod_decls.push(format!("pub mod {sep_name};"));

        for (rt_name, rt_def) in builder.runtime_defs() {
            all_rt_builder
                .require_runtime(rt_name, &rt_def.code, rt_def.imports.clone())
                .unwrap();
        }
    }

    // 2. Emit live loopback versions of SCHEMA_HTML_FETCH_REQUESTS and SCHEMA_REST_CLIENT
    let live_fetch_kdl = SCHEMA_HTML_FETCH_REQUESTS
        .replace("GET /", "GET http://127.0.0.1:{{port:int}}/")
        .replace("POST /", "POST http://127.0.0.1:{{port:int}}/")
        .replace("    Host: example.com\n", "");
    let live_fetch_mod = parsmith::compile_source(&live_fetch_kdl, Some("live_fetch.kdl")).unwrap();
    let sep_opts = CodegenOptions::new(Target::Rust)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let (live_fetch_code, _, _) =
        emit_rust_module(&live_fetch_mod, &sep_opts).expect("emit live_fetch");
    tmp.write("src/live_fetch.rs", &live_fetch_code);
    mod_decls.push("pub mod live_fetch;".to_owned());

    let live_rest_kdl = SCHEMA_REST_CLIENT
        .replace("GET /", "GET http://127.0.0.1:{{port:int}}/")
        .replace("POST /", "POST http://127.0.0.1:{{port:int}}/")
        .replace("    Host: api.example.com\n", "");
    let live_rest_mod = parsmith::compile_source(&live_rest_kdl, Some("live_rest.kdl")).unwrap();
    let (live_rest_code, _, _) =
        emit_rust_module(&live_rest_mod, &sep_opts).expect("emit live_rest");
    tmp.write("src/live_rest.rs", &live_rest_code);
    mod_decls.push("pub mod live_rest;".to_owned());

    // Emit shared runtime module (using `live_fetch_mod` which has both HTML and HTTP enabled, plus `all_rt_builder`)
    let shared_rt = emit_rust_runtime_module(&live_fetch_mod, &all_rt_builder, &sep_opts);
    tmp.write("src/parsmith_runtime.rs", &shared_rt);
    mod_decls.push("pub mod parsmith_runtime;".to_owned());

    // 3. Also compile every integration schema from `tests/integration/schemas/*.kdl`
    let schemas_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/integration/schemas");
    let mut schema_paths: Vec<PathBuf> = fs::read_dir(&schemas_dir)
        .expect("read tests/integration/schemas")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("kdl"))
        .collect();
    schema_paths.sort();
    assert!(!schema_paths.is_empty());

    for (idx, path) in schema_paths.iter().enumerate() {
        let fname = path.file_name().unwrap().to_string_lossy();
        let mod_name = format!("int_schema_{idx}");
        let res = generate_path(
            path,
            &CodegenOptions::new(Target::Rust)
                .with_separate_runtime(true)
                .with_runtime_name("parsmith_runtime"),
        );
        assert!(
            !res.has_errors(),
            "Rust generation failed for {fname}: {:?}",
            res.diagnostics
        );
        let stem = path.file_stem().unwrap().to_string_lossy();
        let parser_file = format!("{stem}.rs");
        tmp.write(&format!("src/{mod_name}.rs"), &res.files[&parser_file]);
        mod_decls.push(format!("pub mod {mod_name};"));
    }

    let lib_rs = format!(
        r###"{}

#[cfg(test)]
mod tests {{
    use super::*;
    use std::io::{{Read, Write}};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn test_html_item_list_filters_flat_dict_table_and_fn() {{
        let item_html = r#"<html><head><link rel="canonical" href="https://example.test/item"></head><body><h1>Main Heading</h1><div class="card">c</div><section class="single"><h1>Card Title</h1><span class="is-active">1</span></section></body></html>"#;
        let mut p_inline = inline_0::PageItemParser::new(item_html).unwrap();
        assert!(p_inline.has_cards().unwrap());
        let d_inline = p_inline.parse().unwrap();
        assert_eq!(d_inline.heading, "Main Heading");
        assert_eq!(d_inline.canonical, "https://example.test/item");
        assert_eq!(d_inline.single.title, "Card Title");
        assert!(d_inline.single.active);

        let mut p_sep = sep_item::PageItemParser::new(item_html).unwrap();
        let d_sep = p_sep.parse().unwrap();
        assert_eq!(d_sep.heading, d_inline.heading);
        assert_eq!(d_sep.single.title, d_inline.single.title);

        let list_html = r#"<div class="product-card"><div class="ad">ad</div><h2 class="title">  foo &amp; Widget  </h2><a class="link" href="/-my-slug-.html">Link</a><img src="/img.png"><span class="tags"> alpha, beta , gamma, delta </span><span class="sku">SKU-42</span><span class="price">$19.99</span><span class="stock-badge">in</span></div>"#;
        let mut list_p = sep_list::ProductCatalogParser::new(list_html).unwrap();
        let items = list_p.parse().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "bar & Widget");
        assert_eq!(items[0].slug, "https://example.test/my-slug?ref=my-slug");
        assert_eq!(items[0].tags, vec!["ALPHA", "BETA", "GAMMA"]);
        assert_eq!(items[0].first_tag, "alpha");
        assert_eq!(items[0].sku, 42);
        assert_eq!(items[0].price, 19.99);
        assert!(items[0].in_stock);

        let filter_html = r##"<div><a href="#skip" data-role="nav">Skip</a><a href="/nav" data-role="nav">Nav</a><a href="/more">Read More</a><span class="code">PRD-100-OK</span><span class="code">PRD-TEST-OK</span></div>"##;
        let mut fp = sep_filters::FilteredPageParser::new(filter_html).unwrap();
        let f_res = fp.parse().unwrap();
        assert_eq!(f_res.active_links, vec!["/nav", "/more"]);
        assert_eq!(f_res.valid_codes, vec!["PRD-100-OK"]);

        let fdt_html = r#"<html><head><meta name="Author" content=" Alice "><meta name="Theme" content="dark"></head><body><span class="tag-primary">t1</span><span class="tag-primary">t2</span><table class="specs"><tr><th>Product ID</th><td>P-900</td></tr><tr><th>Quantity</th><td>5</td></tr><tr><th>Price</th><td>12.5</td></tr></table></body></html>"#;
        let flat_res = sep_flat_dict_table::FlatTagsParser::new(fdt_html).unwrap().parse().unwrap();
        assert_eq!(flat_res, vec!["t1".to_string(), "t2".to_string()]);
        let dict_res = sep_flat_dict_table::MetaPropsParser::new(fdt_html).unwrap().parse().unwrap();
        assert_eq!(dict_res.get("author").map(String::as_str), Some("Alice"));
        let tbl_res = sep_flat_dict_table::ProductSpecsParser::new(fdt_html).unwrap().parse().unwrap();
        assert_eq!(tbl_res.get("product_id").and_then(|v| v.as_str()), Some("P-900"));
        assert_eq!(tbl_res.get("total_qty").and_then(|v| v.as_i64()), Some(5));
        assert_eq!(tbl_res.get("unit_price").and_then(|v| v.as_f64()), Some(12.5));

        let fn_html = r#"<html><body><h1>Hello Fn</h1><div class="card featured"><h2>Featured!</h2></div></body></html>"#;
        assert_eq!(sep_html_fn::page_title(fn_html).unwrap(), "Hello Fn");
        assert_eq!(sep_html_fn::parse_featured_card(fn_html).unwrap().title, "Featured!");
    }}

    #[test]
    fn test_raw_and_extensions() {{
        let raw_logs = "[WARN] disk full\nplain message";
        let entries = sep_raw::LogEntryParser::new(raw_logs).unwrap().parse().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, "WARN");
        assert_eq!(entries[0].message, "disk full");
        assert_eq!(entries[1].level, "INFO");
        assert_eq!(entries[1].message, "plain message");
        assert_eq!(sep_raw::extract_version("v=1 version=2.4.6 ok").unwrap(), "2.4.6");

        let mut ext_p = sep_ext::ExtRawParserParser::new("  hello   rust   world ").unwrap();
        assert!(ext_p.is_valid().unwrap());
        assert_eq!(ext_p.parse().unwrap().normalized, "hello rust world");
        assert_eq!(sep_ext::normalize_text("  a   b  ").unwrap(), "a b");
    }}

    #[test]
    fn test_strict_json_projectors_and_gjson() {{
        let cat_json = serde_json::json!({{
            "@id": 10,
            "os.version": "24.04",
            "meta": {{ "build.number": "b42" }},
            "a\\b": "bs_val",
            "#": "hash_val",
            "rating_scores": [99, 88],
            "tags": [
                {{ "name": "rust" }},
                {{ "other": "skip" }},
                {{ "name": "kdl" }}
            ],
            "author": {{
                "dc:creator": "Alex",
                "verified": true
            }},
            "links": [
                {{ "id": 1, "relation": "parent" }}
            ],
            "channels": {{
                "stable": {{
                    "stream_endpoint": "https://cdn.test/s",
                    "codec": null,
                    "internal_flag": true
                }}
            }},
            "translations": {{
                "en": {{
                    "wire_name": "English",
                    "active": true
                }}
            }}
        }});
        let envelope = serde_json::json!({{
            "@id": 10,
            "os.version": "24.04",
            "meta": {{ "build.number": "b42" }},
            "a\\b": "bs_val",
            "#": "hash_val",
            "rating_scores": [99, 88],
            "tags": [{{ "name": "rust" }}, {{ "name": "kdl" }}],
            "author": {{ "dc:creator": "Alex", "verified": true }},
            "links": [{{ "id": 1, "relation": "parent" }}],
            "channels": {{ "stable": {{ "stream_endpoint": "https://cdn.test/s", "codec": null }} }},
            "payload.data": cat_json
        }});
        let parsed = sep_json::CatalogExtractorParser::new(envelope.to_string()).unwrap().parse().unwrap();
        assert_eq!(parsed.catalog.release_id, 10);
        assert_eq!(parsed.catalog.os_version, "24.04");
        assert_eq!(parsed.catalog.nested_dot, "b42");
        assert_eq!(parsed.catalog.backslash_key, "bs_val");
        assert_eq!(parsed.catalog.hash_key, "hash_val");
        assert_eq!(parsed.catalog.first_score, 99);
        assert_eq!(parsed.catalog.tag_names, vec!["rust", "kdl"]);
        assert_eq!(parsed.catalog.tag_count, 2);
        assert_eq!(parsed.catalog.missing_note, None);
        assert_eq!(parsed.catalog.omitted_val, None);
        assert_eq!(parsed.envelope_catalog.author.author_name, "Alex");

        // Verify @omitempty omits `omitted_val` while nullable `missing_note` serializes as `null`
        let serialized: serde_json::Value = serde_json::to_value(&parsed.catalog).unwrap();
        assert!(serialized.get("omitted_val").is_none());
        assert_eq!(serialized.get("missing_note"), Some(&serde_json::Value::Null));

        // Verify strict error rejection on non-object root or wrong type
        assert!(sep_json::GeoLocationJson::from_value(serde_json::json!([1, 2])).is_err());
        assert!(sep_json::GeoLocationJson::from_value(serde_json::json!(null)).is_err());
        assert!(sep_json::GeoLocationJson::from_value(serde_json::json!({{"lat": "bad", "lng": 1.0}})).is_err());
    }}

    #[tokio::test]
    async fn test_fetch_and_rest_client_error_fallthrough_and_decode_err() {{
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port() as i64;

        thread::spawn(move || {{
            for stream in listener.incoming() {{
                let Ok(mut stream) = stream else {{ break }};
                let mut buf = [0u8; 4096];
                let n = match stream.read(&mut buf) {{
                    Ok(n) if n > 0 => n,
                    _ => continue,
                }};
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let (status, ct, body) = if first_line.starts_with("GET /articles/7?") {{
                    (200, "text/html", "<html><body><h1>Article 7</h1></body></html>".to_string())
                }} else if first_line.starts_with("POST /articles/slug/hello") {{
                    (200, "text/html", "<html><body><h1>Slug hello</h1></body></html>".to_string())
                }} else if first_line.starts_with("GET /logs/s1") {{
                    (200, "application/json", r#"{{"payload":{{"lines":["l1","l2"]}}}}"#.to_string())
                }} else if first_line.starts_with("GET /users/1") {{
                    (200, "application/json", r#"{{"id":1,"name":"Alice","nickname":null}}"#.to_string())
                }} else if first_line.starts_with("GET /users/2") {{
                    (200, "application/json", r#"{{"error":{{"code":1001,"message":"Quota exceeded"}}}}"#.to_string())
                }} else if first_line.starts_with("GET /users/3") {{
                    (404, "application/json", r#"{{"error":"Not Found","code":404}}"#.to_string())
                }} else if first_line.starts_with("GET /users/4") {{
                    (404, "application/json", r#"{{"error":"Not Found","message":"Fallback generic 404"}}"#.to_string())
                }} else if first_line.starts_with("GET /users/5") {{
                    (503, "application/json", r#"{{"reason":"maintenance"}}"#.to_string())
                }} else if first_line.starts_with("GET /users/6") {{
                    (200, "text/html", "<html>not json</html>".to_string())
                }} else if first_line.starts_with("GET /users/7") {{
                    (200, "application/json", r#"{{"id":7}}"#.to_string())
                }} else if first_line.starts_with("GET /users/8") {{
                    (400, "application/json", r#"{{"unexpected":"shape"}}"#.to_string())
                }} else if first_line.starts_with("GET /ping") {{
                    (204, "text/plain", String::new())
                }} else if first_line.starts_with("GET /me") {{
                    (200, "application/json", r#"{{"data":{{"user":{{"id":42,"name":"Bob"}}}}}}"#.to_string())
                }} else if first_line.starts_with("GET /articles") {{
                    (200, "application/json", r#"[{{"id":10,"title":"News"}}]"#.to_string())
                }} else {{
                    (404, "application/json", r#"{{"message":"not found"}}"#.to_string())
                }};
                let status_text = match status {{
                    200 => "200 OK",
                    204 => "204 No Content",
                    400 => "400 Bad Request",
                    404 => "404 Not Found",
                    503 => "503 Service Unavailable",
                    _ => "500 Internal Server Error",
                }};
                let resp = format!(
                    "HTTP/1.1 {{status_text}}\r\nContent-Type: {{ct}}\r\nContent-Length: {{}}\r\nConnection: close\r\n\r\n{{body}}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }}
        }});

        let client = reqwest::Client::new();

        // 1. HTML & Raw @request fetch
        let mut art = live_fetch::ArticlePageParser::fetch(
            &client,
            port,
            7,
            "s1",
            None,
            None,
            None,
            Some(&["a", "b"]),
            Some(&[1, 2]),
            Some(&["x", "y"]),
            Some(&[10, 20]),
            Some(&["r1", "r2"]),
        )
        .await
        .unwrap();
        assert_eq!(art.parse().unwrap().heading, "Article 7");

        let mut slug_art = live_fetch::ArticlePageParser::fetch_by_slug(&client, port, "hello", Some("tok")).await.unwrap();
        assert_eq!(slug_art.parse().unwrap().heading, "Slug hello");

        let mut log_p = live_fetch::ChunkedLogParser::fetch(&client, port, "s1").await.unwrap();
        assert_eq!(log_p.parse().unwrap().lines, vec!["l1", "l2"]);

        // 2. REST 200 OK
        let u1 = live_rest::UserApi::get_user(&client, port, 1, None).await.unwrap();
        assert_eq!(u1.id, 1);
        assert_eq!(u1.name, "Alice");
        assert_eq!(u1.nickname, None);

        // 3. REST @error 200 match
        match live_rest::UserApi::get_user(&client, port, 2, None).await {{
            Err(live_rest::UserApiError::Err200Code(e)) => assert_eq!(e.code, 1001),
            other => panic!("expected Err200Code, got {{other:?}}"),
        }}

        // 4. REST @error:not-found 404 match vs fallthrough to @error 404 GenericError
        match live_rest::UserApi::get_user(&client, port, 3, None).await {{
            Err(live_rest::UserApiError::Err404NotFound(e)) => assert_eq!(e.code, 404),
            other => panic!("expected Err404NotFound, got {{other:?}}"),
        }}
        match live_rest::UserApi::get_user(&client, port, 4, None).await {{
            Err(live_rest::UserApiError::Err404(e)) => assert_eq!(e.message, "Fallback generic 404"),
            other => panic!("expected Err404 after fallthrough, got {{other:?}}"),
        }}

        // 5. REST Unknown(503, _)
        match live_rest::UserApi::get_user(&client, port, 5, None).await {{
            Err(err @ live_rest::UserApiError::Unknown(503, _)) => assert_eq!(err.status(), Some(503)),
            other => panic!("expected Unknown(503), got {{other:?}}"),
        }}

        // 6. REST Decode(200, _) on HTML body and missing required field, and Decode(400, _) on error schema mismatch
        match live_rest::UserApi::get_user(&client, port, 6, None).await {{
            Err(err @ live_rest::UserApiError::Decode(200, _)) => assert_eq!(err.status(), Some(200)),
            other => panic!("expected Decode(200) on HTML body, got {{other:?}}"),
        }}
        match live_rest::UserApi::get_user(&client, port, 7, None).await {{
            Err(live_rest::UserApiError::Decode(200, err)) => assert!(err.to_string().contains("User.name")),
            other => panic!("expected Decode(200) on missing field, got {{other:?}}"),
        }}
        match live_rest::UserApi::get_user(&client, port, 8, None).await {{
            Err(live_rest::UserApiError::Decode(400, _)) => {{}},
            other => panic!("expected Decode(400) on error schema mismatch, got {{other:?}}"),
        }}

        // 7. Void, enveloped response-path, array response, and Transport error on closed port
        live_rest::UserApi::ping(&client, port).await.unwrap();
        let me = live_rest::UserApi::get_profile_enveloped(&client, port).await.unwrap();
        assert_eq!(me.id, 42);
        let arts = live_rest::UserApi::list_articles(&client, port).await.unwrap();
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].article_id, 10);

        let closed_err = live_rest::UserApi::get_user(&client, 1, 1, None).await.unwrap_err();
        assert_eq!(closed_err.status(), None);
        assert!(matches!(closed_err, live_rest::UserApiError::Transport(_)));
    }}
}}
"###,
        mod_decls.join("\n")
    );
    tmp.write("src/lib.rs", &lib_rs);

    let shared_target_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/e2e_rust_target");
    let test_out = Command::new("cargo")
        .args(["test", "--quiet"])
        .env("CARGO_TARGET_DIR", &shared_target_dir)
        .current_dir(&tmp.path)
        .output()
        .expect("execute cargo test on generated Rust modules");
    assert!(
        test_out.status.success(),
        "cargo test failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
}

#[test]
fn rust_struct_and_json_value_constants_and_postfix_autocast() {
    let schema = r#"
define SOURCE="habr"
extension Feed {
    now_ts {
        sig int int
        rust { emit "let {{out}} = {{in}} + 100;" }
    }
}

struct FeedCard {
    @init {
        seed_prefix str value="{{SOURCE}}" { upper }
        fetched_at int value=0 { !Feed.now_ts }
    }
    source str value="{{SOURCE}}"
    kind value="article"
    version int value=1
    ratio float value=1.5
    is_live bool value=#false
    prefixed_id str value="{{SOURCE}}" { fmt "{{}}-01" }
    subtitle str? { css ".lead"; text; trim; fallback #null }
    tags (array)str { css-all ".tag"; text; trim }
    counts (array)int { css-all ".cnt"; text }
    scores (array)float { css-all ".score"; text }
    has_badge bool { css ".badge" }
    views int { css ".views"; text; fallback 0 }
    rating float { css ".rating"; text; fallback 0.0 }
    url { css "a.link"; attr "href" }
}

json ReleaseCard {
    source str value="{{SOURCE}}"
    channel str value="{{SOURCE}}"
    kind str value="event"
    version int value=1
    weight float value=2.5
    is_live bool value=#false
    id str from="node_id"
}
"#;
    let res = generate_source(
        schema,
        Some("feed_card.kdl"),
        &CodegenOptions::new(Target::Rust),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("let v1 = \"habr\".to_string();"));
    assert!(code.contains("let v1 = \"article\".to_string();"));
    assert!(code.contains("let v1 = 1;"));
    assert!(code.contains("let v1 = 1.5;"));
    assert!(code.contains("let v1 = false;"));
    assert!(code.contains("let v2 = v1 + 100;"));
    assert!(code.contains("let v2 = rt::fmt_value(\"{{}}-01\", &v1);"));
    assert!(code.contains("let v2 = rt::node_bool(&v1);"));
    assert!(code.contains(
        "let v3 = v2.into_iter().map(|value| rt::to_int(value, \"FeedCard.counts\")).collect::<Result<Vec<_>, _>>()?;"
    ));
    assert!(code.contains(
        "let v3 = v2.into_iter().map(|value| rt::to_float(value, \"FeedCard.scores\")).collect::<Result<Vec<_>, _>>()?;"
    ));
    assert!(code.contains("let v3 = rt::to_int(v2, \"FeedCard.views\")?;"));
    assert!(code.contains("let v3 = rt::to_float(v2, \"FeedCard.rating\")?;"));
    assert!(code.contains("let source: String = \"habr\".to_string();"));
    assert!(code.contains("let channel: String = \"habr\".to_string();"));
    assert!(code.contains("let kind: String = \"event\".to_string();"));
    assert!(code.contains("let version: i64 = 1;"));
    assert!(code.contains("let weight: f64 = 2.5;"));
    assert!(code.contains("let is_live: bool = false;"));
    assert!(!code.contains("rt::json_path(&value, \"source\""));
}

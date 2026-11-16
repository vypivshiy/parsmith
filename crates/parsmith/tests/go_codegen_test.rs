mod common;

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use common::*;
use parsmith::{
    CodeWriter, CodegenOptions, DiagnosticCode, GoEmitter, MemorySourceLoader, ModuleBuilder,
    Target, drive_module, drive_single_pass, generate_path, generate_source,
    generate_source_with_loader, parse_target_or_shorthand, validate_go_package_name,
};

fn assert_gofmt_clean(source: &str, label: &str) {
    let mut child = Command::new("gofmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn gofmt");

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(source.as_bytes())
            .expect("write source to gofmt stdin");
    }
    let output = child.wait_with_output().expect("wait for gofmt");
    assert!(
        output.status.success(),
        "gofmt failed on {label}:\n{}\nsource:\n{source}",
        String::from_utf8_lossy(&output.stderr)
    );
    let formatted = String::from_utf8(output.stdout).expect("gofmt stdout utf8");
    assert_eq!(formatted, source, "gofmt produced a diff for {label}!");
}

const SCHEMA_EXTENSIONS_GO: &str = r####"
extension Utils {
    to-base64 {
        sig str str

        go {
            import "encoding/base64"
            emit #"{{out}} := base64.StdEncoding.EncodeToString([]byte({{in}}))"#
        }
    }

    normalize {
        sig str str

        go {
            helper normalizeValue {
                import "strings"
                source #"""
                    func normalizeValue(value string) string {
                        return strings.Join(strings.Fields(value), " ")
                    }
                    """#
            }
            emit #"{{out}} := normalizeValue({{in}})"#
        }
    }

    truthy {
        sig T bool
        go { emit #"{{out}} := {{in}} != """# }
    }
}

(raw)struct ExtRawParser {
    @check is-valid {
        !Utils.truthy
    }

    encoded {
        !Utils.normalize
        !Utils.to-base64
    }
}

(raw)fn encode_normalized {
    !Utils.normalize
    !Utils.to-base64
}
"####;

const SCHEMA_REST_CLIENT: &str = r####"
@doc "REST client coverage: typed @error / @error:<suffix> variants, UnknownErr, TransportErr, and DecodeErr."

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
fn target_go_and_package_validation() {
    assert_eq!("go".parse::<Target>().unwrap(), Target::Go);
    assert_eq!("golang".parse::<Target>().unwrap(), Target::Go);
    assert_eq!(parse_target_or_shorthand("go").unwrap(), (Target::Go, None));
    assert_eq!(
        parse_target_or_shorthand("golang").unwrap(),
        (Target::Go, None)
    );
    assert_eq!(Target::Go.as_str(), "go");
    assert_eq!(Target::Go.ecosystem_name(), "go");
    assert_eq!(Target::Go.extension(), "go");

    assert!(validate_go_package_name("main"));
    assert!(validate_go_package_name("parsers_v1"));
    assert!(!validate_go_package_name(""));
    assert!(!validate_go_package_name("123bad"));
    assert!(!validate_go_package_name("my-pkg"));
    assert!(!validate_go_package_name("func"));
    assert!(!validate_go_package_name("package"));
    assert!(!validate_go_package_name("type"));
}

#[test]
fn invalid_go_package_or_runtime_name_reports_e002() {
    let bad_pkg_opts = CodegenOptions::new(Target::Go).with_package("func");
    let res_pkg = generate_source(SCHEMA_25_FN, Some("sample.kdl"), &bad_pkg_opts);
    assert!(res_pkg.has_errors());
    assert!(res_pkg.files.is_empty());
    assert_eq!(res_pkg.diagnostics[0].code, DiagnosticCode::E002);
    assert!(
        res_pkg.diagnostics[0]
            .message
            .contains("invalid Go package name 'func'")
    );

    let bad_rt_opts = CodegenOptions::new(Target::Go)
        .with_separate_runtime(true)
        .with_runtime_name("123-bad");
    let res_rt = generate_source(SCHEMA_25_FN, Some("sample.kdl"), &bad_rt_opts);
    assert!(res_rt.has_errors());
    assert!(res_rt.files.is_empty());
    assert_eq!(res_rt.diagnostics[0].code, DiagnosticCode::E002);
    assert!(
        res_rt.diagnostics[0]
            .message
            .contains("invalid Go runtime name '123-bad'")
    );
}

#[test]
fn go_dry_run_populates_module_builder_with_zero_allocation() {
    let module = parsmith::compile_source(SCHEMA_EXTENSIONS_GO, Some("ext.kdl")).unwrap();
    let opts = CodegenOptions::new(Target::Go);
    let mut emitter = GoEmitter::new(opts);
    let mut builder = ModuleBuilder::new();
    let mut dry_writer = CodeWriter::dry_run();

    drive_single_pass(
        &mut emitter,
        &module,
        &mut builder,
        &mut dry_writer,
        None,
        None,
    );

    assert_eq!(dry_writer.capacity(), 0);
    assert!(dry_writer.finish().is_empty());
    assert!(builder.has_runtime());
    assert_eq!(
        builder.runtime_names().collect::<Vec<_>>(),
        vec!["normalizeValue"]
    );

    let (emitted, _) = drive_module(&mut emitter, &module, None, None);
    assert!(emitted.contains("func normalizeValue(value string) string {"));
    assert_eq!(emitted.matches("func normalizeValue").count(), 1);
}

#[test]
fn snapshot_go_item_init_check() {
    let res = generate_source(
        SCHEMA_ITEM_INIT_CHECK,
        Some("page_item.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = &res.files["page_item.go"];
    assert_gofmt_clean(code, "page_item.go");
    insta::assert_snapshot!("go_page_item", code);
}

#[test]
fn snapshot_go_list_strings_basic() {
    let res = generate_source(
        SCHEMA_01_STRINGS_BASIC,
        Some("01_strings_basic.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "01_strings_basic.go");
    insta::assert_snapshot!("go_01_strings_basic", code);
}

#[test]
fn snapshot_go_list_arrays_and_conversions() {
    let res = generate_source(
        SCHEMA_02_ARRAYS_AND_CONVERSIONS,
        Some("02_arrays_and_conversions.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "02_arrays_and_conversions.go");
    insta::assert_snapshot!("go_02_arrays_and_conversions", code);
}

#[test]
fn snapshot_go_list_filters_and_predicates() {
    let res = generate_source(
        SCHEMA_03_FILTERS_AND_PREDICATES,
        Some("03_filters_and_predicates.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "03_filters_and_predicates.go");
    insta::assert_snapshot!("go_03_filters_and_predicates", code);
}

#[test]
fn snapshot_go_flat_struct() {
    let res = generate_source(
        SCHEMA_05_FLAT,
        Some("05_flat.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "05_flat.go");
    insta::assert_snapshot!("go_05_flat", code);
}

#[test]
fn snapshot_go_dict_struct() {
    let res = generate_source(
        SCHEMA_06_DICT,
        Some("06_dict.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "06_dict.go");
    insta::assert_snapshot!("go_06_dict", code);
}

#[test]
fn snapshot_go_table_struct() {
    let res = generate_source(
        SCHEMA_07_TABLE,
        Some("07_table.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "07_table.go");
    insta::assert_snapshot!("go_07_table", code);
}

#[test]
fn snapshot_go_raw_struct() {
    let res = generate_source(
        SCHEMA_23_RAW_STRUCT,
        Some("23_raw_struct.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "23_raw_struct.go");
    insta::assert_snapshot!("go_23_raw_struct", code);
}

#[test]
fn snapshot_go_fn_and_raw_fn() {
    let res = generate_source(
        SCHEMA_25_FN,
        Some("25_fn.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "25_fn.go");
    insta::assert_snapshot!("go_25_fn", code);
}

#[test]
fn snapshot_go_extensions_inline_and_separate_runtime() {
    let inline_res = generate_source(
        SCHEMA_EXTENSIONS_GO,
        Some("extensions.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!inline_res.has_errors(), "{:?}", inline_res.diagnostics);
    assert_eq!(inline_res.files.len(), 1);
    let inline_code = &inline_res.files["extensions.go"];
    assert_gofmt_clean(inline_code, "extensions.go (inline)");
    insta::assert_snapshot!("go_extensions_inline", inline_code);

    let sep_opts = CodegenOptions::new(Target::Go)
        .with_package("parsers")
        .with_separate_runtime(true)
        .with_runtime_name("sscgen_runtime");
    let sep_res = generate_source(SCHEMA_EXTENSIONS_GO, Some("extensions.kdl"), &sep_opts);
    assert!(!sep_res.has_errors(), "{:?}", sep_res.diagnostics);
    assert_eq!(sep_res.files.len(), 2);
    let parser_code = &sep_res.files["extensions.go"];
    let runtime_code = &sep_res.files["sscgen_runtime.go"];
    assert_gofmt_clean(parser_code, "extensions.go (separate)");
    assert_gofmt_clean(runtime_code, "sscgen_runtime.go");
    insta::assert_snapshot!("go_extensions_separate_parser", parser_code);
    insta::assert_snapshot!("go_extensions_separate_runtime", runtime_code);
}

#[test]
fn snapshot_go_json_projectors() {
    let res = generate_source(
        SCHEMA_JSON_PROJECTORS,
        Some("json_proj.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = &res.files["json_proj.go"];
    assert_gofmt_clean(code, "json_proj.go");
    insta::assert_snapshot!("go_json_projectors", code);
}

#[test]
fn snapshot_go_html_fetch_requests() {
    let res = generate_source(
        SCHEMA_HTML_FETCH_REQUESTS,
        Some("html_fetch.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = &res.files["html_fetch.go"];
    assert_gofmt_clean(code, "html_fetch.go");
    insta::assert_snapshot!("go_html_fetch_requests", code);
}

#[test]
fn snapshot_go_rest_client_inline_and_separate_runtime() {
    let inline_res = generate_source(
        SCHEMA_REST_CLIENT,
        Some("rest_client.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(!inline_res.has_errors(), "{:?}", inline_res.diagnostics);
    let inline_code = &inline_res.files["rest_client.go"];
    assert_gofmt_clean(inline_code, "rest_client.go (inline)");
    insta::assert_snapshot!("go_rest_client_inline", inline_code);

    let sep_opts = CodegenOptions::new(Target::Go)
        .with_package("apiclient")
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let sep_res = generate_source(SCHEMA_REST_CLIENT, Some("rest_client.kdl"), &sep_opts);
    assert!(!sep_res.has_errors(), "{:?}", sep_res.diagnostics);
    assert_eq!(sep_res.files.len(), 2);
    let parser_code = &sep_res.files["rest_client.go"];
    let runtime_code = &sep_res.files["parsmith_runtime.go"];
    assert_gofmt_clean(parser_code, "rest_client.go (separate)");
    assert_gofmt_clean(runtime_code, "parsmith_runtime.go");
    insta::assert_snapshot!("go_rest_client_separate_parser", parser_code);
    insta::assert_snapshot!("go_rest_client_separate_runtime", runtime_code);
}

#[test]
fn missing_go_extension_target_reports_e400() {
    let mut vfs = MemorySourceLoader::new();
    vfs.insert(
        "shared/ext.kdl",
        r#"
extension OnlyPy {
    noop {
        sig str str
        py { emit "{{out}} = {{in}}" }
    }
}
"#,
    );
    let main_src = r#"
import "./shared/ext.kdl" { (extension)OnlyPy }
(raw)fn test_fn { !OnlyPy.noop }
"#;
    let res = generate_source_with_loader(
        main_src,
        Some("main.kdl"),
        &vfs,
        &CodegenOptions::new(Target::Go),
    );
    assert!(res.has_errors());
    assert_eq!(res.diagnostics[0].code, DiagnosticCode::E400);
    assert!(
        res.diagnostics[0]
            .message
            .contains("extension operation 'OnlyPy.noop' has no 'go' target")
    );
}

#[test]
fn xpath_operations_in_go_report_e203() {
    let res = generate_source(
        SCHEMA_XPATH_DOM,
        Some("xpath.kdl"),
        &CodegenOptions::new(Target::Go),
    );
    assert!(res.has_errors());
    assert!(res.files.is_empty());
    assert_eq!(res.diagnostics.len(), 4);
    for diag in &res.diagnostics {
        assert_eq!(diag.code, DiagnosticCode::E203);
        assert!(
            diag.message
                .contains("XPath operations ('xpath', 'xpath-all', 'xpath-remove') are not supported by the 'go' backend")
        );
    }
}

#[test]
fn generate_path_for_go() {
    let tmp = TempTestDir::new("gen_path");
    let schema_file = tmp.write("sample.kdl", SCHEMA_25_FN);

    let go_res = generate_path(&schema_file, &CodegenOptions::new(Target::Go));
    assert!(!go_res.has_errors(), "{:?}", go_res.diagnostics);
    assert!(go_res.files.contains_key("sample.go"));
    assert_gofmt_clean(&go_res.files["sample.go"], "sample.go");
}

#[test]
fn go_toolchain_vet_and_execution() {
    let tmp = TempTestDir::new("toolchain");
    tmp.write("dsl_coverage.html", HTML_FIXTURE);

    let go_mod = r#"module parsmith_go_test

go 1.26

require (
	github.com/PuerkitoBio/goquery v1.12.0
	github.com/tidwall/gjson v1.18.0
)

require (
	github.com/andybalholm/cascadia v1.3.3 // indirect
	github.com/tidwall/match v1.1.1 // indirect
	github.com/tidwall/pretty v1.2.0 // indirect
	golang.org/x/net v0.52.0 // indirect
)
"#;
    let go_sum = r#"github.com/PuerkitoBio/goquery v1.12.0 h1:pAcL4g3WRXekcB9AU/y1mbKez2dbY2AajVhtkO8RIBo=
github.com/PuerkitoBio/goquery v1.12.0/go.mod h1:802ej+gV2y7bbIhOIoPY5sT183ZW0YFofScC4q/hIpQ=
github.com/andybalholm/cascadia v1.3.3 h1:AG2YHrzJIm4BZ19iwJ/DAua6Btl3IwJX+VI4kktS1LM=
github.com/andybalholm/cascadia v1.3.3/go.mod h1:xNd9bqTn98Ln4DwST8/nG+H0yuB8Hmgu1YHNnWw0GeA=
github.com/tidwall/gjson v1.18.0 h1:FIDeeyB800efLX89e5a8Y0BNH+LOngJyGrIWxG2FKQY=
github.com/tidwall/gjson v1.18.0/go.mod h1:/wbyibRr2FHMks5tjHJ5F8dMZh3AcwJEMf5vlfC0lxk=
github.com/tidwall/match v1.1.1 h1:+Ho715JplO36QYgwN9PGYNhgZvoUSc9X2c80KVTi+GA=
github.com/tidwall/match v1.1.1/go.mod h1:eRSPERbgtNPcGhD8UCthc6PmLEQXEWd3PRB5JTxsfmM=
github.com/tidwall/pretty v1.2.0 h1:RWIZEg2iJ8/g6fDDYzMpobmaoGh5OLl4AXtGUGPcqCs=
github.com/tidwall/pretty v1.2.0/go.mod h1:ITEVvHYasfjBbM0u2Pg8T2nJnzm8xPwvNhhsoaGGjNU=
golang.org/x/net v0.52.0 h1:He/TN1l0e4mmR3QqHMT2Xab3Aj3L9qjbhRm78/6jrW0=
golang.org/x/net v0.52.0/go.mod h1:R1MAz7uMZxVMualyPXb+VaqGSa3LIaUqk0eEt3w36Sw=
"#;
    tmp.write("go.mod", go_mod);
    tmp.write("go.sum", go_sum);

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
        ("m_ext.kdl", SCHEMA_EXTENSIONS_GO),
        ("m_json.kdl", SCHEMA_JSON_PROJECTORS),
        ("m_fetch.kdl", SCHEMA_HTML_FETCH_REQUESTS),
        ("m_rest.kdl", SCHEMA_REST_CLIENT),
    ];

    // 1. Test every schema in isolated single-file inline mode (`separate_runtime = false`)
    for (idx, (name, src)) in schemas.iter().enumerate() {
        let pkg = format!("inline_pkg_{idx}");
        let opts = CodegenOptions::new(Target::Go).with_package(&pkg);
        let res = generate_source(src, Some(name), &opts);
        assert!(
            !res.has_errors(),
            "Go inline generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            assert_gofmt_clean(code, &format!("{pkg}/{fname}"));
            tmp.write(&format!("{pkg}/{fname}"), code);
        }
    }

    // 2. Test separate-runtime mode (`separate_runtime = true`) in `sep_pkg` + run full Go test suite
    let mut all_std_builder = ModuleBuilder::new();
    for (name, src) in &schemas {
        let module = parsmith::compile_source(src, Some(name)).unwrap();
        let opts = CodegenOptions::new(Target::Go)
            .with_package("sep_pkg")
            .with_separate_runtime(true)
            .with_runtime_name("parsmith_runtime");
        let (parser_code, _, builder) =
            parsmith::emit_go_module(&module, &opts).expect("emit_go_module");
        let stem = name.trim_end_matches(".kdl");
        assert_gofmt_clean(&parser_code, &format!("sep_pkg/{stem}.go"));
        tmp.write(&format!("sep_pkg/{stem}.go"), &parser_code);

        for (std_name, std_def) in builder.std_defs() {
            all_std_builder.require_std_with_imports(std_name, &std_def.code, &std_def.imports);
        }
        for (rt_name, rt_def) in builder.runtime_defs() {
            all_std_builder
                .require_runtime(rt_name, &rt_def.code, rt_def.imports.clone())
                .unwrap();
        }
    }
    let rest_mod = parsmith::compile_source(SCHEMA_REST_CLIENT, Some("m_rest.kdl")).unwrap();
    let sep_opts = CodegenOptions::new(Target::Go)
        .with_package("sep_pkg")
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let shared_rt = parsmith::emit_go_runtime_module(&rest_mod, &all_std_builder, &sep_opts);
    assert_gofmt_clean(&shared_rt, "sep_pkg/parsmith_runtime.go");
    tmp.write("sep_pkg/parsmith_runtime.go", &shared_rt);

    let suite_test_go = r#"package sep_pkg

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func loadHTMLFixture(t *testing.T) string {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "dsl_coverage.html"))
	if err != nil {
		t.Fatalf("read dsl_coverage.html: %v", err)
	}
	return string(b)
}

func TestHTMLAndRawParsers(t *testing.T) {
	html := loadHTMLFixture(t)

	// 1. Item + @init + @check + @pre-validate + nested
	itemParser, err := NewPageItem(html)
	if err != nil {
		t.Fatalf("NewPageItem: %v", err)
	}
	if !itemParser.hasCards() {
		t.Errorf("expected hasCards() == true")
	}
	itemData := itemParser.Parse()
	if itemData.Heading != "Coverage Root" {
		t.Errorf("unexpected heading: %q", itemData.Heading)
	}
	if itemData.Canonical != "https://example.test/coverage" {
		t.Errorf("unexpected canonical: %q", itemData.Canonical)
	}
	if itemData.Single.Title != "Single Item Title" || !itemData.Single.Active {
		t.Errorf("unexpected single: %+v", itemData.Single)
	}

	// 2. List (01_strings_basic)
	sbParser, err := NewStringsBasic(html)
	if err != nil {
		t.Fatalf("NewStringsBasic: %v", err)
	}
	sbRes := sbParser.Parse()
	if len(sbRes) != 2 {
		t.Fatalf("expected 2 StringsBasic items, got %d", len(sbRes))
	}
	if sbRes[0].Title != "bar Title One" || sbRes[0].Link != "https://example.testguide" || sbRes[0].Slug != "slug-one" || sbRes[0].NormalizedCode != "foo-bar" || !sbRes[0].ActiveFlag {
		t.Errorf("unexpected sbRes[0]: %+v", sbRes[0])
	}

	// 3. Arrays & conversions (02_arrays_and_conversions)
	acParser, err := NewArraysAndConversions(html)
	if err != nil {
		t.Fatalf("NewArraysAndConversions: %v", err)
	}
	acRes := acParser.Parse()
	if len(acRes) != 2 {
		t.Fatalf("expected 2 ArraysAndConversions items, got %d", len(acRes))
	}
	if !reflect.DeepEqual(acRes[0].TokenList, []string{"tag-core-alpha1-x", "item-beta2-y", "ref3"}) {
		t.Errorf("unexpected TokenList: %v", acRes[0].TokenList)
	}
	if acRes[0].FirstToken != "tag-core-alpha1-x" || acRes[0].SecondToken != "item-beta2-y" || acRes[0].LastToken != "ref3" {
		t.Errorf("unexpected tokens: %+v", acRes[0])
	}
	if acRes[0].TokenWindow != "tag-core-alpha1-x|item-beta2-y" {
		t.Errorf("unexpected TokenWindow: %q", acRes[0].TokenWindow)
	}
	if acRes[0].StateCode == nil || *acRes[0].StateCode != 1 || acRes[0].Score != 41 || acRes[0].Ratio == nil || *acRes[0].Ratio != 0.75 || acRes[0].AnyLinkCount != 2 || acRes[0].FlagsJoined != "ALPHA,BETA" {
		t.Errorf("unexpected numeric/flags on acRes[0]: %+v", acRes[0])
	}

	// 4. Filters & predicates (03_filters_and_predicates)
	fpParser, err := NewFiltersAndPredicates(html)
	if err != nil {
		t.Fatalf("NewFiltersAndPredicates: %v", err)
	}
	fpRes := fpParser.Parse()
	if len(fpRes) != 2 || len(fpRes[0].FilteredLinks) != 0 || fpRes[0].LogicCheck != "prealpha" || fpRes[1].NumericCheck != "code-4" {
		t.Errorf("unexpected fpRes: %+v", fpRes)
	}

	// 5. Flat, Dict, Table structs
	flatParser, err := NewFlatCoverage(html)
	if err != nil {
		t.Fatalf("NewFlatCoverage: %v", err)
	}
	flatRes := flatParser.Parse()
	if len(flatRes) == 0 {
		t.Errorf("expected non-empty flatRes")
	}

	dictRootParser, err := NewDictRoot(html)
	if err != nil {
		t.Fatalf("NewDictRoot: %v", err)
	}
	dictRoot := dictRootParser.Parse()
	if dictRoot.NamedMeta["keywords"] != "alpha,beta,core" || dictRoot.AliasMeta["og:title"] != "Coverage OG Title" {
		t.Errorf("unexpected dictRoot: %+v", dictRoot)
	}

	tblParser, err := NewTableCoverage(html)
	if err != nil {
		t.Fatalf("NewTableCoverage: %v", err)
	}
	tblRes := tblParser.Parse()
	if tblRes["identifier"] != "ABC-123" || tblRes["code_value"] != "CODE-1" || tblRes["price"] != 9.99 || tblRes["tax_or_fee"] != 1.25 || tblRes["state"] != "active" {
		t.Errorf("unexpected tblRes: %+v", tblRes)
	}

	// 6. Raw structs, fn, (raw)fn, and extensions
	jsText := `<script>var player = new Playerjs({id:"player",file:"/v/list/abc.txt"});</script>`
	rawPlayer := NewRawPlayerScript(jsText).Parse()
	if rawPlayer.PlaylistUrl != "/v/list/abc.txt" || rawPlayer.PlayerId != "player" {
		t.Errorf("unexpected rawPlayer: %+v", rawPlayer)
	}
	rawTransform := NewRawTextTransform("hello   world").Parse()
	if rawTransform.UpperText != "HELLO   WORLD" || rawTransform.WordCount != 2 || rawTransform.FirstWord != "hello" {
		t.Errorf("unexpected rawTransform: %+v", rawTransform)
	}
	if PageTitle("<html><body><h1>Hello World</h1></body></html>") != "Hello World" {
		t.Errorf("unexpected PageTitle")
	}
	if FirstLine("first\nsecond") != "first" || ExtractVersion("app version=1.2.3 released") != "1.2.3" {
		t.Errorf("unexpected raw fn result")
	}
	extParser := NewExtRawParser("  hello   world  ")
	if !extParser.isValid() || extParser.Parse().Encoded != "aGVsbG8gd29ybGQ=" || EncodeNormalized("  hello   world  ") != "aGVsbG8gd29ybGQ=" {
		t.Errorf("unexpected extension result")
	}
}

func TestStrictJsonProjectors(t *testing.T) {
	html := loadHTMLFixture(t)
	jbParser, err := NewJsonBasic(html)
	if err != nil {
		t.Fatalf("NewJsonBasic: %v", err)
	}
	jb := jbParser.Parse()
	if len(jb.FullPayload) != 2 || jb.FullPayload[0].Text != "Quote one" || jb.FirstAuthor.Name != "Author One" || jb.SecondAuthor.Name != "Author Two" {
		t.Errorf("unexpected JsonBasic: %+v", jb)
	}

	// Reject non-object root
	var author JsonAuthorJson
	if err := json.Unmarshal([]byte(`["not", "an", "object"]`), &author); err == nil {
		t.Errorf("expected error when unmarshaling array into JsonAuthorJson")
	}
	if err := json.Unmarshal([]byte(`null`), &author); err == nil {
		t.Errorf("expected error when unmarshaling null into JsonAuthorJson")
	}
	// Reject wrong scalar type (number instead of string)
	if err := json.Unmarshal([]byte(`{"name": 123, "slug": "s"}`), &author); err == nil {
		t.Errorf("expected error when unmarshaling number into string field")
	}
}

func TestRestClientAndDecodeErr(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/users/1":
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"id": 1, "name": "Alice", "nickname": null}`))
		case "/users/2":
			// 200 error condition "error.code"
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"error": {"code": 1001, "message": "Quota exceeded"}}`))
		case "/users/3":
			// Specific 404 Not Found
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error": "Not Found", "code": 404}`))
		case "/users/4":
			// Fallthrough from SpecificNotFoundError (missing 'code') to GenericError
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error": "Not Found", "message": "Fallback generic 404"}`))
		case "/users/5":
			// Unmatched 503 -> UnknownErr
			w.WriteHeader(http.StatusServiceUnavailable)
			_, _ = w.Write([]byte(`{"reason": "maintenance"}`))
		case "/users/6":
			// 200 with malformed HTML -> DecodeErr
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`<html>not json</html>`))
		case "/users/7":
			// 200 with missing required 'name' -> DecodeErr
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{"id": 7}`))
		case "/logs/s1":
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"payload": {"lines": ["line-1", "line-2"]}}`))
		}
	}))
	defer srv.Close()

	// Verify DecodeErr on 200 malformed HTML
	_, err := rtRestCallValue[UserJson](srv.Client(), rtUserApiMatchers, "GET", srv.URL+"/users/6", "", nil)
	var decErr *DecodeErr
	if !errors.As(err, &decErr) || decErr.Status != 200 {
		t.Fatalf("expected *DecodeErr on HTML 200, got %T: %v", err, err)
	}

	// Verify DecodeErr on 200 schema mismatch
	decErr = nil
	_, err = rtRestCallValue[UserJson](srv.Client(), rtUserApiMatchers, "GET", srv.URL+"/users/7", "", nil)
	if !errors.As(err, &decErr) || decErr.Status != 200 {
		t.Fatalf("expected *DecodeErr on schema mismatch 200, got %T: %v", err, err)
	}

	// Verify @error 200 match
	_, err = rtRestCallValue[UserJson](srv.Client(), rtUserApiMatchers, "GET", srv.URL+"/users/2", "", nil)
	var err200 *UserApiErr200Code
	if !errors.As(err, &err200) || err200.Body.Code != 1001 {
		t.Fatalf("expected *UserApiErr200Code, got %T: %v", err, err)
	}

	// Verify @error:not-found 404 vs fallthrough to @error 404 GenericError
	_, err = rtRestCallValue[UserJson](srv.Client(), rtUserApiMatchers, "GET", srv.URL+"/users/3", "", nil)
	var err404Spec *UserApiErr404NotFound
	if !errors.As(err, &err404Spec) || err404Spec.Body.Code != 404 {
		t.Fatalf("expected *UserApiErr404NotFound, got %T: %v", err, err)
	}

	_, err = rtRestCallValue[UserJson](srv.Client(), rtUserApiMatchers, "GET", srv.URL+"/users/4", "", nil)
	var err404Gen *UserApiErr404
	if !errors.As(err, &err404Gen) || err404Gen.Body.Message != "Fallback generic 404" {
		t.Fatalf("expected *UserApiErr404 after fallthrough, got %T: %v", err, err)
	}

	// Verify UnknownErr on 503
	_, err = rtRestCallValue[UserJson](srv.Client(), rtUserApiMatchers, "GET", srv.URL+"/users/5", "", nil)
	var unkErr *UnknownErr
	if !errors.As(err, &unkErr) || unkErr.Status != 503 {
		t.Fatalf("expected *UnknownErr on 503, got %T: %v", err, err)
	}

	// Verify rtFetchHTML with response-path and response-join
	body, err := rtFetchHTML(context.Background(), srv.Client(), "ChunkedLog", "GET", srv.URL+"/logs/s1", "payload.lines", "\n", nil)
	if err != nil {
		t.Fatalf("rtFetchHTML failed: %v", err)
	}
	logRes := NewChunkedLog(body).Parse()
	if !reflect.DeepEqual(logRes.Lines, []string{"line-1", "line-2"}) {
		t.Errorf("unexpected ChunkedLog lines: %v", logRes.Lines)
	}
}
"#;
    tmp.write("sep_pkg/suite_test.go", suite_test_go);

    let vet = Command::new("go")
        .args(["vet", "./..."])
        .current_dir(&tmp.path)
        .output()
        .expect("execute go vet");
    assert!(
        vet.status.success(),
        "go vet failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&vet.stdout),
        String::from_utf8_lossy(&vet.stderr)
    );

    let test_out = Command::new("go")
        .args(["test", "./sep_pkg"])
        .current_dir(&tmp.path)
        .output()
        .expect("execute go test");
    assert!(
        test_out.status.success(),
        "go test failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
}

#[test]
fn all_integration_schemas_gofmt_and_go_vet_clean() {
    let schemas_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/integration/schemas");
    let entries = fs::read_dir(&schemas_dir).expect("read tests/integration/schemas");

    let tmp = TempTestDir::new("all_schemas");
    let go_mod = r#"module parsmith_all_schemas_test

go 1.26

require (
	github.com/PuerkitoBio/goquery v1.12.0
	github.com/tidwall/gjson v1.18.0
)

require (
	github.com/andybalholm/cascadia v1.3.3 // indirect
	github.com/tidwall/match v1.1.1 // indirect
	github.com/tidwall/pretty v1.2.0 // indirect
	golang.org/x/net v0.52.0 // indirect
)
"#;
    let go_sum = r#"github.com/PuerkitoBio/goquery v1.12.0 h1:pAcL4g3WRXekcB9AU/y1mbKez2dbY2AajVhtkO8RIBo=
github.com/PuerkitoBio/goquery v1.12.0/go.mod h1:802ej+gV2y7bbIhOIoPY5sT183ZW0YFofScC4q/hIpQ=
github.com/andybalholm/cascadia v1.3.3 h1:AG2YHrzJIm4BZ19iwJ/DAua6Btl3IwJX+VI4kktS1LM=
github.com/andybalholm/cascadia v1.3.3/go.mod h1:xNd9bqTn98Ln4DwST8/nG+H0yuB8Hmgu1YHNnWw0GeA=
github.com/tidwall/gjson v1.18.0 h1:FIDeeyB800efLX89e5a8Y0BNH+LOngJyGrIWxG2FKQY=
github.com/tidwall/gjson v1.18.0/go.mod h1:/wbyibRr2FHMks5tjHJ5F8dMZh3AcwJEMf5vlfC0lxk=
github.com/tidwall/match v1.1.1 h1:+Ho715JplO36QYgwN9PGYNhgZvoUSc9X2c80KVTi+GA=
github.com/tidwall/match v1.1.1/go.mod h1:eRSPERbgtNPcGhD8UCthc6PmLEQXEWd3PRB5JTxsfmM=
github.com/tidwall/pretty v1.2.0 h1:RWIZEg2iJ8/g6fDDYzMpobmaoGh5OLl4AXtGUGPcqCs=
github.com/tidwall/pretty v1.2.0/go.mod h1:ITEVvHYasfjBbM0u2Pg8T2nJnzm8xPwvNhhsoaGGjNU=
golang.org/x/net v0.52.0 h1:He/TN1l0e4mmR3QqHMT2Xab3Aj3L9qjbhRm78/6jrW0=
golang.org/x/net v0.52.0/go.mod h1:R1MAz7uMZxVMualyPXb+VaqGSa3LIaUqk0eEt3w36Sw=
"#;
    tmp.write("go.mod", go_mod);
    tmp.write("go.sum", go_sum);

    let mut schema_paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("kdl"))
        .collect();
    schema_paths.sort();
    assert!(!schema_paths.is_empty());

    for (idx, path) in schema_paths.iter().enumerate() {
        let fname = path.file_name().unwrap().to_string_lossy();
        let pkg = format!("schema_pkg_{idx}");
        let opts = CodegenOptions::new(Target::Go).with_package(&pkg);
        let res = generate_path(path, &opts);
        assert!(
            !res.has_errors(),
            "Go generation failed for {fname}: {:?}",
            res.diagnostics
        );
        for (out_name, code) in &res.files {
            assert_gofmt_clean(code, &format!("{fname} -> {out_name}"));
            tmp.write(&format!("{pkg}/{out_name}"), code);
        }
    }

    let vet = Command::new("go")
        .args(["vet", "./..."])
        .current_dir(&tmp.path)
        .output()
        .expect("execute go vet on all integration schemas");
    assert!(
        vet.status.success(),
        "go vet failed on integration schemas!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&vet.stdout),
        String::from_utf8_lossy(&vet.stderr)
    );
}

#[test]
fn go_struct_and_json_value_constants_and_postfix_autocast() {
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
    has_badge bool { css ".badge" }
    views int { css ".views"; text; fallback 0 }
    rating float { css ".rating"; text; fallback 0.0 }
}

json ReleaseCard {
    source str value="{{SOURCE}}"
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
        &CodegenOptions::new(Target::Go),
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert_gofmt_clean(code, "feed_card.go");
    assert!(code.contains("v1 := \"habr\""));
    assert!(code.contains("v1 := \"article\""));
    assert!(code.contains("v1 := int64(1)"));
    assert!(code.contains("v1 := 1.5"));
    assert!(code.contains("v1 := false"));
    assert!(code.contains("v2 := rtFmt(\"{{}}-01\", v1)"));
    assert!(code.contains("v2 := v1.Length() > 0"));
    assert!(code.contains("v3 := rtToInt(v2)"));
    assert!(code.contains("v3 := rtToFloat(v2)"));
    assert!(code.contains("res.Source = \"habr\""));
    assert!(code.contains("res.Kind = \"event\""));
    assert!(code.contains("res.Version = int64(1)"));
    assert!(code.contains("res.Weight = 2.5"));
    assert!(code.contains("res.IsLive = false"));
    assert!(!code.contains("gjson.GetBytes(raw, \"source\")"));
}

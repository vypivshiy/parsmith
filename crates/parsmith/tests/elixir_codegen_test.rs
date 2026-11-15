mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use common::*;
use parsmith::codegen::elixir::{
    elixir_field_name, elixir_ident, elixir_pascal, elixir_replacement,
    format_elixir_module_prefix, validate_elixir_package_name,
};
use parsmith::{
    CodeWriter, CodegenOptions, DiagnosticCode, ElixirEmitter, ModuleBuilder, Target, drive_module,
    emit_elixir_module, emit_elixir_runtime_module, generate_path, generate_source,
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

        ex {
            helper normalize_value {
                source #"""
                    def normalize_value(value) when is_binary(value) do
                      value
                      |> String.trim()
                      |> String.split(~r/\s+/u, trim: true)
                      |> Enum.join(" ")
                    end
                    """#
            }
            emit #"{{out}} = Rt.normalize_value({{in}})"#
        }
    }

    truthy {
        sig T bool
        elixir {
            emit #"{{out}} = String.trim({{in}}) != """#
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
    @doc "Article HTML page parser with HTTP fetch."

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

#[test]
fn elixir_item_init_check_generation() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("item_init.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("defmodule Rt do"));
    assert!(code.contains("defmodule SingleCardType do"));
    assert!(code.contains("defmodule PageItemType do"));
    assert!(code.contains("defmodule PageItem do"));
    assert!(code.contains("def has_cards(%__MODULE__{} = self) do"));
}

#[test]
fn elixir_list_strings_arrays_conversions_generation() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(SCHEMA_LIST_STRINGS_ARRAYS, Some("list_strings.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("defmodule ProductCatalogType do"));
    assert!(code.contains("defmodule ProductCatalog do"));
    assert!(code.contains("Rt.select_remove("));
    assert!(code.contains("Rt.attr_multi("));
    assert!(code.contains("Rt.unescape("));
    assert!(code.contains("Rt.slice_vec("));
}

#[test]
fn elixir_filters_and_predicates_generation() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(
        SCHEMA_FILTERS_AND_PREDICATES,
        Some("filters_preds.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("defmodule FilteredPage do"));
    assert!(code.contains("Rt.pred_has_attr?("));
    assert!(code.contains("Rt.assert_true("));
}

#[test]
fn elixir_flat_dict_table_generation() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(SCHEMA_FLAT_DICT_TABLE, Some("flat_dict_table.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("defmodule FlatTags do"));
    assert!(code.contains("defmodule MetaProps do"));
    assert!(code.contains("defmodule ProductSpecs do"));
    assert!(code.contains("table_match_key(self, v)"));
}

#[test]
fn elixir_raw_struct_and_fn_omits_floki() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(SCHEMA_RAW_AND_FN, Some("raw_fn.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(
        !code.contains("Floki."),
        "pure raw module must omit Floki dependency"
    );
    assert!(code.contains("defmodule LogEntry do"));
    assert!(code.contains("defmodule Functions do"));
    assert!(code.contains("def extract_version(document) when is_binary(document) do"));
}

#[test]
fn elixir_html_fn_generation() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(SCHEMA_HTML_FN, Some("html_fn.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("defmodule Functions do"));
    assert!(code.contains("def page_title(document)"));
    assert!(code.contains("def parse_featured_card(document)"));
}

#[test]
fn elixir_extensions_inline_and_separate_runtime() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(SCHEMA_EXTENSIONS, Some("ext_inline.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("def normalize_value(value)"));
    assert!(code.contains("Rt.normalize_value("));

    let sep_opts = CodegenOptions::new(Target::Elixir)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let sep_res = generate_source(SCHEMA_EXTENSIONS, Some("ext_sep.kdl"), &sep_opts);
    assert!(!sep_res.has_errors(), "{:?}", sep_res.diagnostics);
    assert_eq!(sep_res.files.len(), 2);
    assert!(sep_res.files["ext_sep.ex"].contains("alias ParsmithRuntime, as: Rt"));
    assert!(sep_res.files["parsmith_runtime.ex"].contains("defmodule ParsmithRuntime do"));
    assert!(sep_res.files["parsmith_runtime.ex"].contains("def normalize_value(value)"));
}

#[test]
fn elixir_json_projectors_generation() {
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(
        SCHEMA_CATALOG_JSON_PROJECTORS,
        Some("json_projectors.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(
        !code.contains("Floki."),
        "pure JSON + raw module must omit Floki dependency"
    );
    assert!(code.contains("defmodule GeoLocationJson do"));
    assert!(code.contains("@derive JSON.Encoder"));
    assert!(code.contains("defmodule ReleaseCatalogJson do"));
    assert!(code.contains("defimpl JSON.Encoder do"));
    assert!(code.contains(
        "def project_release_catalog(data), do: apply(ReleaseCatalogJson, :project, [data])"
    ));
}

#[test]
fn elixir_html_fetch_and_rest_client_generation() {
    let opts = CodegenOptions::new(Target::Elixir);
    let fetch_res = generate_source(SCHEMA_HTML_FETCH_REQUESTS, Some("html_fetch.kdl"), &opts);
    assert!(!fetch_res.has_errors(), "{:?}", fetch_res.diagnostics);
    let fetch_code = fetch_res.primary_code().unwrap();
    assert!(fetch_code.contains("def fetch(client, id, sid,"));
    assert!(fetch_code.contains("def fetch_by_slug(client, slug, token \\\\ nil, opts \\\\ [])"));

    let rest_res = generate_source(SCHEMA_REST_CLIENT, Some("rest_client.kdl"), &opts);
    assert!(!rest_res.has_errors(), "{:?}", rest_res.diagnostics);
    let rest_code = rest_res.primary_code().unwrap();
    assert!(
        !rest_code.contains("Floki."),
        "pure REST module must omit Floki dependency"
    );
    assert!(rest_code.contains("defmodule UserApiErr200Code do"));
    assert!(rest_code.contains("defmodule UserApiErr404NotFound do"));
    assert!(rest_code.contains("defmodule UserApiErr404 do"));
    assert!(rest_code.contains("defmodule UserApiErr400 do"));
    assert!(rest_code.contains("defmodule UserApi do"));
    assert!(rest_code.contains("def get_user(client, id, include_meta \\\\ nil, opts \\\\ [])"));
}

#[test]
fn elixir_package_prefix_and_separate_runtime() {
    assert!(validate_elixir_package_name(""));
    assert!(validate_elixir_package_name("main"));
    assert!(validate_elixir_package_name("MyApp.Parsers"));
    assert!(validate_elixir_package_name("my_app.scrapers"));
    assert!(!validate_elixir_package_name("MyApp..Bad"));
    assert!(!validate_elixir_package_name("123Bad"));
    assert!(!validate_elixir_package_name("MyApp.do"));

    assert_eq!(format_elixir_module_prefix(""), "");
    assert_eq!(format_elixir_module_prefix("main"), "");
    assert_eq!(
        format_elixir_module_prefix("my_app.parsers"),
        "MyApp.Parsers."
    );

    let opts = CodegenOptions::new(Target::Elixir)
        .with_package("MyApp.Parsers")
        .with_separate_runtime(true)
        .with_runtime_name("custom_rt");
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("page_pkg.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert_eq!(res.files.len(), 2);
    let parser_code = &res.files["page_pkg.ex"];
    let rt_code = &res.files["custom_rt.ex"];
    assert!(parser_code.contains("defmodule MyApp.Parsers.PageItem do"));
    assert!(parser_code.contains("alias MyApp.Parsers.CustomRt, as: Rt"));
    assert!(rt_code.contains("defmodule MyApp.Parsers.CustomRt do"));
}

#[test]
fn elixir_invalid_package_or_runtime_name_reports_e002() {
    let bad_pkg = CodegenOptions::new(Target::Elixir).with_package("123-bad-pkg");
    let res_pkg = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &bad_pkg);
    assert!(res_pkg.has_errors());
    assert!(
        res_pkg
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );

    let bad_rt = CodegenOptions::new(Target::Elixir)
        .with_separate_runtime(true)
        .with_runtime_name("123-bad-rt");
    let res_rt = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &bad_rt);
    assert!(res_rt.has_errors());
    assert!(
        res_rt
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );
}

#[test]
fn elixir_xpath_rejected_with_e203() {
    let schemas = [
        r#"struct A { x { xpath "//h1"; text } }"#,
        r#"struct B { x { xpath-all "//a"; text } }"#,
        r#"struct C { x { xpath-remove "//script"; text } }"#,
        r#"struct D { x { css-all "div"; filter { xpath ".//a" }; text } }"#,
    ];

    let opts = CodegenOptions::new(Target::Elixir);
    for src in schemas {
        let res = generate_source(src, Some("xpath.kdl"), &opts);
        assert!(res.has_errors(), "expected E203 for schema: {src}");
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E203 && d.message.contains("'elixir' backend")),
            "unexpected diagnostics: {:?}",
            res.diagnostics
        );
    }
}

#[test]
fn elixir_missing_extension_target_reports_e400() {
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
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(src, Some("missing_ex_ext.kdl"), &opts);
    assert!(res.has_errors());
    assert!(res.diagnostics.iter().any(|d| {
        d.code == DiagnosticCode::E400
            && d.message
                .contains("extension operation 'OnlyPy.noop' has no 'ex' target")
    }));
}

#[test]
fn elixir_keywords_and_replacements() {
    let src = r#"
struct Data {
    fn { css "a"; text }
    do { css "b"; text }
    end { css "c"; text }
    in { css "d"; text }
    match { css "e"; text }
}
(rest)struct Api {
    @request name=fn """
    GET /?in={{in}}&when={{when:bool?}} HTTP/1.1
    Host: example.com
    """
}
"#;
    let opts = CodegenOptions::new(Target::Elixir);
    let res = generate_source(src, Some("keywords.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("defstruct [:fn, :do, :end, :in, :match]"));
    assert!(code.contains("def fn_(client, in_, when_ \\\\ nil, opts \\\\ [])"));
    assert_eq!(elixir_field_name("fn"), "fn");
    assert_eq!(elixir_ident("fn"), "fn_");
    assert_eq!(elixir_ident("do"), "do_");
    assert_eq!(elixir_pascal("my-api"), "MyApi");

    assert_eq!(elixir_replacement(r"\1-ok"), r"\g{1}-ok");
    assert_eq!(elixir_replacement("$1-ok"), r"\g{1}-ok");
    assert_eq!(elixir_replacement("${1}_ok"), r"\g{1}_ok");
    assert_eq!(elixir_replacement("price: $foo"), "price: $foo");
    assert_eq!(elixir_replacement("$$"), "$");
}

#[test]
fn elixir_target_parsing_and_two_pass_dry_run_zero_allocation() {
    assert_eq!(
        parse_target_or_shorthand("elixir").unwrap(),
        (Target::Elixir, None)
    );
    assert_eq!(
        parse_target_or_shorthand("ex").unwrap(),
        (Target::Elixir, None)
    );
    assert_eq!(Target::Elixir.as_str(), "elixir");
    assert_eq!(Target::Elixir.ecosystem_name(), "elixir");
    assert_eq!(Target::Elixir.extension(), "ex");

    let module = parsmith::compile_source(SCHEMA_ITEM_INIT_CHECK, Some("item.kdl")).unwrap();
    let mut emitter = ElixirEmitter::new(CodegenOptions::new(Target::Elixir));
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
        Some("# prologue"),
        Some("# epilogue"),
    );
    assert!(emitted.contains("# prologue"));
    assert!(emitted.contains("# epilogue"));
}

#[test]
fn elixir_all_integration_schemas_generate_cleanly() {
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
        let pkg = format!("IntSchema{idx}");
        let res = generate_path(
            path,
            &CodegenOptions::new(Target::Elixir)
                .with_package(&pkg)
                .with_separate_runtime(true)
                .with_runtime_name("parsmith_runtime"),
        );
        assert!(
            !res.has_errors(),
            "Elixir generation failed for {fname}: {:?}",
            res.diagnostics
        );
    }
}

#[cfg(windows)]
fn resolve_latest_erts_bin() -> Option<PathBuf> {
    let mut otp_roots = Vec::new();
    if let Some(path_os) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&path_os) {
            if entry.join("erl.exe").exists()
                && let Some(parent) = entry.parent()
            {
                otp_roots.push(parent.to_path_buf());
                if let Some(grandparent) = parent.parent() {
                    otp_roots.push(grandparent.to_path_buf());
                }
            }
        }
    }
    let default_root = PathBuf::from(r"C:\Program Files\Erlang OTP");
    if default_root.exists() {
        otp_roots.push(default_root);
    }

    let mut best: Option<((u32, u32, u32, u32), PathBuf)> = None;
    for root in otp_roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(ver_str) = name.strip_prefix("erts-") else {
                continue;
            };
            let bin_dir = path.join("bin");
            if !bin_dir.join("erl.exe").exists() || !bin_dir.join("beam.smp.dll").exists() {
                continue;
            }
            let mut parts = ver_str.split('.').map(|s| s.parse::<u32>().unwrap_or(0));
            let key = (
                parts.next().unwrap_or(0),
                parts.next().unwrap_or(0),
                parts.next().unwrap_or(0),
                parts.next().unwrap_or(0),
            );
            if best.as_ref().is_none_or(|(best_key, _)| key > *best_key) {
                best = Some((key, bin_dir));
            }
        }
    }
    best.map(|(_, dir)| dir)
}

#[test]
fn elixir_toolchain_check_and_execution() {
    let elixir_bin = if cfg!(windows) {
        "elixir.bat"
    } else {
        "elixir"
    };

    let tmp = TempTestDir::new("elixir_toolchain");

    let schemas = [
        ("SepItem", SCHEMA_ITEM_INIT_CHECK),
        ("SepList", SCHEMA_LIST_STRINGS_ARRAYS),
        ("SepFilters", SCHEMA_FILTERS_AND_PREDICATES),
        ("SepFlatDictTable", SCHEMA_FLAT_DICT_TABLE),
        ("SepRaw", SCHEMA_RAW_AND_FN),
        ("SepHtmlFn", SCHEMA_HTML_FN),
        ("SepExt", SCHEMA_EXTENSIONS),
        ("SepJson", SCHEMA_CATALOG_JSON_PROJECTORS),
    ];

    let mut parallel_files = Vec::new();

    for (pkg_name, src) in &schemas {
        let opts = CodegenOptions::new(Target::Elixir).with_package(*pkg_name);
        let res = generate_source(src, Some(&format!("{pkg_name}.kdl")), &opts);
        assert!(
            !res.has_errors(),
            "Elixir generation failed for {pkg_name}: {:?}",
            res.diagnostics
        );
        let file_name = format!("{pkg_name}.ex");
        tmp.write(&file_name, res.primary_code().unwrap());
        parallel_files.push(format!("Path.expand({:?}, __DIR__)", file_name));
    }

    // Also test separate runtime mode with live loopback HTTP fetch and REST client
    let live_fetch_kdl = SCHEMA_HTML_FETCH_REQUESTS
        .replace("GET /", "GET http://127.0.0.1:{{port:int}}/")
        .replace("POST /", "POST http://127.0.0.1:{{port:int}}/")
        .replace("    Host: example.com\n", "");
    let live_fetch_mod = parsmith::compile_source(&live_fetch_kdl, Some("live_fetch.kdl")).unwrap();
    let live_fetch_opts = CodegenOptions::new(Target::Elixir)
        .with_package("LiveFetch")
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let (live_fetch_code, live_fetch_rt, _) =
        emit_elixir_module(&live_fetch_mod, &live_fetch_opts).expect("emit live_fetch");
    tmp.write("live_fetch_rt.ex", &live_fetch_rt.unwrap());
    tmp.write("live_fetch.ex", &live_fetch_code);
    parallel_files.push("Path.expand(\"live_fetch.ex\", __DIR__)".to_owned());

    let live_rest_kdl = SCHEMA_REST_CLIENT
        .replace("GET /", "GET http://127.0.0.1:{{port:int}}/")
        .replace("POST /", "POST http://127.0.0.1:{{port:int}}/")
        .replace("    Host: api.example.com\n", "");
    let live_rest_mod = parsmith::compile_source(&live_rest_kdl, Some("live_rest.kdl")).unwrap();
    let live_rest_opts = CodegenOptions::new(Target::Elixir)
        .with_package("LiveRest")
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let (live_rest_code, live_rest_rt, live_rest_builder) =
        emit_elixir_module(&live_rest_mod, &live_rest_opts).expect("emit live_rest");
    let explicit_rt =
        emit_elixir_runtime_module(&live_rest_mod, &live_rest_builder, &live_rest_opts);
    assert_eq!(live_rest_rt.unwrap(), explicit_rt);
    tmp.write("live_rest_rt.ex", &explicit_rt);
    tmp.write("live_rest.ex", &live_rest_code);
    parallel_files.push("Path.expand(\"live_rest.ex\", __DIR__)".to_owned());

    // Also compile every integration schema from `tests/integration/schemas/*.kdl` with Elixir
    let schemas_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/integration/schemas");
    let mut schema_paths: Vec<PathBuf> = fs::read_dir(&schemas_dir)
        .expect("read tests/integration/schemas")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("kdl"))
        .collect();
    schema_paths.sort();

    for (idx, path) in schema_paths.iter().enumerate() {
        let fname = path.file_name().unwrap().to_string_lossy();
        let pkg = format!("IntSchema{idx}");
        let res = generate_path(
            path,
            &CodegenOptions::new(Target::Elixir)
                .with_package(&pkg)
                .with_module_stem(format!("int_schema_{idx}")),
        );
        assert!(
            !res.has_errors(),
            "Elixir generation failed for {fname}: {:?}",
            res.diagnostics
        );
        let ex_file = format!("int_schema_{idx}.ex");
        tmp.write(&ex_file, &res.files[&ex_file]);
        parallel_files.push(format!("Path.expand({:?}, __DIR__)", ex_file));
    }
    let require_block = format!(
        "Code.require_file(\"live_fetch_rt.ex\", __DIR__)\nCode.require_file(\"live_rest_rt.ex\", __DIR__)\n{{:ok, _, _}} = Kernel.ParallelCompiler.compile([{}], return_diagnostics: true)",
        parallel_files.join(", ")
    );

    let runner_exs = format!(
        r###"Mix.start()

case Path.wildcard(
       Path.join([
         Mix.Utils.mix_cache(),
         "installs",
         "ex-#{{System.version()}}-erl-#{{:erlang.system_info(:version)}}",
         "*",
         "_build",
         "dev",
         "lib",
         "floki",
         "ebin"
       ])
     ) do
  [ebin | _] ->
    Code.prepend_path(ebin)
    {{:ok, _}} = Application.ensure_all_started(:floki)

  [] ->
    Mix.install([{{:floki, "~> 0.36"}}], consolidate_protocols: false)
end

{}

defmodule ToolchainRunner do
  def assert_eq(actual, expected, label \\ "") do
    if actual != expected do
      raise "ASSERTION FAILED (#{{label}}):\n  actual:   #{{inspect(actual)}}\n  expected: #{{inspect(expected)}}"
    end
  end

  def run do
    # 1. HTML item, list, filters, flat, dict, table, and fn
    item_html = ~s(<html><head><link rel="canonical" href="https://example.test/item"></head><body><h1>Main Heading</h1><div class="card">c</div><section class="single"><h1>Card Title</h1><span class="is-active">1</span></section></body></html>)
    p_item = SepItem.PageItem.new(item_html)
    assert_eq(SepItem.PageItem.has_cards(p_item), true, "has_cards")
    d_item = SepItem.PageItem.parse(p_item)
    assert_eq(d_item.heading, "Main Heading", "heading")
    assert_eq(d_item.canonical, "https://example.test/item", "canonical")
    assert_eq(d_item.single.title, "Card Title", "single.title")
    assert_eq(d_item.single.active, true, "single.active")

    list_html = ~s(<div class="product-card"><div class="ad">ad</div><h2 class="title">  foo &amp; Widget  </h2><a class="link" href="/-my-slug-.html">Link</a><img src="/img.png"><span class="tags"> alpha, beta , gamma, delta </span><span class="sku">SKU-42</span><span class="price">$19.99</span><span class="stock-badge">in</span></div>)
    items = SepList.ProductCatalog.new(list_html) |> SepList.ProductCatalog.parse()
    assert_eq(length(items), 1, "items.len")
    item0 = hd(items)
    assert_eq(item0.title, "bar & Widget", "item0.title")
    assert_eq(item0.slug, "https://example.test/my-slug?ref=my-slug", "item0.slug")
    assert_eq(item0.tags, ["ALPHA", "BETA", "GAMMA"], "item0.tags")
    assert_eq(item0.first_tag, "alpha", "item0.first_tag")
    assert_eq(item0.sku, 42, "item0.sku")
    assert_eq(item0.price, 19.99, "item0.price")
    assert_eq(item0.in_stock, true, "item0.in_stock")

    filter_html = ~s(<div><a href="#skip" data-role="nav">Skip</a><a href="/nav" data-role="nav">Nav</a><a href="/more">Read More</a><span class="code">PRD-100-OK</span><span class="code">PRD-TEST-OK</span></div>)
    f_res = SepFilters.FilteredPage.parse(filter_html)
    assert_eq(f_res.active_links, ["/nav", "/more"], "active_links")
    assert_eq(f_res.valid_codes, ["PRD-100-OK"], "valid_codes")

    fdt_html = ~s(<html><head><meta name="Author" content=" Alice "><meta name="Theme" content="dark"></head><body><span class="tag-primary">t1</span><span class="tag-primary">t2</span><table class="specs"><tr><th>Product ID</th><td>P-900</td></tr><tr><th>Quantity</th><td>5</td></tr><tr><th>Price</th><td>12.5</td></tr></table></body></html>)
    assert_eq(SepFlatDictTable.FlatTags.parse(fdt_html), ["t1", "t2"], "flat")
    dict_res = SepFlatDictTable.MetaProps.parse(fdt_html)
    assert_eq(dict_res["author"], "Alice", "dict author")
    tbl_res = SepFlatDictTable.ProductSpecs.parse(fdt_html)
    assert_eq(tbl_res["product_id"], "P-900", "table product_id")
    assert_eq(tbl_res["total_qty"], 5, "table total_qty")
    assert_eq(tbl_res["unit_price"], 12.5, "table unit_price")

    fn_html = ~s(<html><body><h1>Hello Fn</h1><div class="card featured"><h2>Featured!</h2></div></body></html>)
    assert_eq(SepHtmlFn.Functions.page_title(fn_html), "Hello Fn", "page_title")
    assert_eq(SepHtmlFn.Functions.parse_featured_card(fn_html).title, "Featured!", "parse_featured_card")

    # 2. Raw structs and extensions
    raw_logs = "[WARN] disk full\nplain message"
    entries = SepRaw.LogEntry.parse(raw_logs)
    assert_eq(length(entries), 2, "raw entries len")
    assert_eq(Enum.at(entries, 0).level, "WARN", "entry0.level")
    assert_eq(Enum.at(entries, 0).message, "disk full", "entry0.message")
    assert_eq(Enum.at(entries, 1).level, "INFO", "entry1.level")
    assert_eq(Enum.at(entries, 1).message, "plain message", "entry1.message")
    assert_eq(SepRaw.Functions.extract_version("v=1 version=2.4.6 ok"), "2.4.6", "extract_version")

    ext_p = SepExt.ExtRawParser.new("  hello   elixir   world ")
    assert_eq(SepExt.ExtRawParser.is_valid(ext_p), true, "ext is_valid")
    assert_eq(SepExt.ExtRawParser.parse(ext_p).normalized, "hello elixir world", "ext normalized")
    assert_eq(SepExt.Functions.normalize_text("  a   b  "), "a b", "ext normalize_text")

    # 3. Strict JSON projectors, GJSON paths, and @omitempty encoding
    cat_json = %{{
      "@id" => 10,
      "os.version" => "24.04",
      "meta" => %{{"build.number" => "b42"}},
      "a\\b" => "bs_val",
      "#" => "hash_val",
      "rating_scores" => [99, 88],
      "tags" => [
        %{{"name" => "elixir"}},
        %{{"other" => "skip"}},
        %{{"name" => "kdl"}}
      ],
      "author" => %{{
        "dc:creator" => "Alex",
        "verified" => true
      }},
      "links" => [
        %{{"id" => 1, "relation" => "parent"}}
      ],
      "channels" => %{{
        "stable" => %{{
          "stream_endpoint" => "https://cdn.test/s",
          "codec" => nil,
          "internal_flag" => true
        }}
      }},
      "translations" => %{{
        "en" => %{{
          "wire_name" => "English",
          "active" => true
        }}
      }}
    }}
    envelope =
      cat_json
      |> Map.put("tags", [%{{"name" => "elixir"}}, %{{"name" => "kdl"}}])
      |> Map.put("payload.data", cat_json)
    parsed = SepJson.CatalogExtractor.parse(JSON.encode!(envelope))
    assert_eq(parsed.catalog.release_id, 10, "release_id")
    assert_eq(parsed.catalog.os_version, "24.04", "os_version")
    assert_eq(parsed.catalog.nested_dot, "b42", "nested_dot")
    assert_eq(parsed.catalog.backslash_key, "bs_val", "backslash_key")
    assert_eq(parsed.catalog.hash_key, "hash_val", "hash_key")
    assert_eq(parsed.catalog.first_score, 99, "first_score")
    assert_eq(parsed.catalog.tag_names, ["elixir", "kdl"], "tag_names")
    assert_eq(parsed.catalog.tag_count, 2, "tag_count")
    assert_eq(parsed.catalog.missing_note, nil, "missing_note")
    assert_eq(parsed.catalog.omitted_val, nil, "omitted_val")
    assert_eq(parsed.envelope_catalog.author.author_name, "Alex", "author_name")

    serialized = parsed.catalog |> JSON.encode!() |> JSON.decode!()
    assert_eq(Map.has_key?(serialized, "omitted_val"), false, "@omitempty omitted_val")
    assert_eq(Map.has_key?(serialized, "missing_note"), true, "nullable missing_note key present")
    assert_eq(serialized["missing_note"], nil, "nullable missing_note is null")

    assert_eq(match?({{:error, _}}, SepJson.GeoLocationJson.from_value([1, 2])), true, "reject array root")
    assert_eq(match?({{:error, _}}, SepJson.GeoLocationJson.from_value(nil)), true, "reject null root")
    assert_eq(match?({{:error, _}}, SepJson.GeoLocationJson.from_value(%{{ "lat" => "bad", "lng" => 1.0 }})), true, "reject bad float")

    # 4. Live loopback HTTP fetch and REST client
    {{:ok, listen}} = :gen_tcp.listen(0, [:binary, packet: :raw, active: false, reuseaddr: true])
    {{:ok, port}} = :inet.port(listen)

    spawn(fn ->
      loop_accept(listen)
    end)

    client = LiveRest.ParsmithRuntime.via_httpc()

    {{:ok, art}} = LiveFetch.ArticlePage.fetch(
      client,
      port,
      7,
      "s1",
      nil,
      nil,
      nil,
      ["a", "b"],
      [1, 2],
      ["x", "y"],
      [10, 20],
      ["r1", "r2"]
    )
    assert_eq(LiveFetch.ArticlePage.parse(art).heading, "Article 7", "fetch Article 7")

    slug_art = LiveFetch.ArticlePage.fetch_by_slug!(client, port, "hello", "tok")
    assert_eq(LiveFetch.ArticlePage.parse(slug_art).heading, "Slug hello", "fetch_by_slug")

    log_p = LiveFetch.ChunkedLog.fetch!(client, port, "s1")
    assert_eq(LiveFetch.ChunkedLog.parse(log_p).lines, ["l1", "l2"], "ChunkedLog fetch")

    # REST 200 OK
    {{:ok, u1}} = LiveRest.UserApi.get_user(client, port, 1, nil)
    assert_eq(u1.id, 1, "u1.id")
    assert_eq(u1.name, "Alice", "u1.name")
    assert_eq(u1.nickname, nil, "u1.nickname")

    # REST @error 200 match
    {{:error, %LiveRest.UserApiErr200Code{{status: 200, body: e200}}}} = LiveRest.UserApi.get_user(client, port, 2, nil)
    assert_eq(e200.code, 1001, "err 200 code")

    # REST @error:not-found 404 match vs fallthrough to @error 404 GenericError
    {{:error, %LiveRest.UserApiErr404NotFound{{status: 404, body: e404_nf}}}} = LiveRest.UserApi.get_user(client, port, 3, nil)
    assert_eq(e404_nf.code, 404, "err 404 not-found")

    {{:error, %LiveRest.UserApiErr404{{status: 404, body: e404_gen}}}} = LiveRest.UserApi.get_user(client, port, 4, nil)
    assert_eq(e404_gen.message, "Fallback generic 404", "err 404 fallthrough")

    # REST UnknownErr(503)
    {{:error, %LiveRest.ParsmithRuntime.UnknownErr{{status: 503}}}} = LiveRest.UserApi.get_user(client, port, 5, nil)

    # REST DecodeErr on HTML body, missing field, and error schema mismatch
    {{:error, %LiveRest.ParsmithRuntime.DecodeErr{{status: 200}}}} = LiveRest.UserApi.get_user(client, port, 6, nil)
    {{:error, %LiveRest.ParsmithRuntime.DecodeErr{{status: 200, cause: cause7}}}} = LiveRest.UserApi.get_user(client, port, 7, nil)
    assert_eq(String.contains?(cause7, "User.name"), true, "DecodeErr cause User.name")
    {{:error, %LiveRest.ParsmithRuntime.DecodeErr{{status: 400}}}} = LiveRest.UserApi.get_user(client, port, 8, nil)

    # Void, enveloped response-path, array response, and TransportErr on closed port
    {{:ok, nil}} = LiveRest.UserApi.ping(client, port)
    {{:ok, me}} = LiveRest.UserApi.get_profile_enveloped(client, port)
    assert_eq(me.id, 42, "me.id")
    {{:ok, arts}} = LiveRest.UserApi.list_articles(client, port)
    assert_eq(length(arts), 1, "arts.len")
    assert_eq(hd(arts).article_id, 10, "arts[0].article_id")

    {{:error, %LiveRest.ParsmithRuntime.TransportErr{{status: 0}}}} = LiveRest.UserApi.get_user(client, 1, 1, nil)

    IO.write("ELIXIR_TOOLCHAIN_OK")
  end

  defp loop_accept(listen) do
    case :gen_tcp.accept(listen) do
      {{:ok, sock}} ->
        spawn(fn -> handle_conn(sock) end)
        loop_accept(listen)

      _ ->
        :ok
    end
  end

  defp handle_conn(sock) do
    case :gen_tcp.recv(sock, 0, 5000) do
      {{:ok, req}} ->
        first_line = req |> String.split("\r\n") |> List.first("")

        {{status, ct, body}} =
          cond do
            String.starts_with?(first_line, "GET /articles/7?") ->
              {{200, "text/html", "<html><body><h1>Article 7</h1></body></html>"}}

            String.starts_with?(first_line, "POST /articles/slug/hello") ->
              {{200, "text/html", "<html><body><h1>Slug hello</h1></body></html>"}}

            String.starts_with?(first_line, "GET /logs/s1") ->
              {{200, "application/json", ~s({{"payload":{{"lines":["l1","l2"]}}}})}}

            String.starts_with?(first_line, "GET /users/1") ->
              {{200, "application/json", ~s({{"id":1,"name":"Alice","nickname":null}})}}

            String.starts_with?(first_line, "GET /users/2") ->
              {{200, "application/json", ~s({{"error":{{"code":1001,"message":"Quota exceeded"}}}})}}

            String.starts_with?(first_line, "GET /users/3") ->
              {{404, "application/json", ~s({{"error":"Not Found","code":404}})}}

            String.starts_with?(first_line, "GET /users/4") ->
              {{404, "application/json", ~s({{"error":"Not Found","message":"Fallback generic 404"}})}}

            String.starts_with?(first_line, "GET /users/5") ->
              {{503, "application/json", ~s({{"reason":"maintenance"}})}}

            String.starts_with?(first_line, "GET /users/6") ->
              {{200, "text/html", "<html>not json</html>"}}

            String.starts_with?(first_line, "GET /users/7") ->
              {{200, "application/json", ~s({{"id":7}})}}

            String.starts_with?(first_line, "GET /users/8") ->
              {{400, "application/json", ~s({{"unexpected":"shape"}})}}

            String.starts_with?(first_line, "GET /ping") ->
              {{204, "text/plain", ""}}

            String.starts_with?(first_line, "GET /me") ->
              {{200, "application/json", ~s({{"data":{{"user":{{"id":42,"name":"Bob"}}}}}})}}

            String.starts_with?(first_line, "GET /articles") ->
              {{200, "application/json", ~s([{{"id":10,"title":"News"}}])}}

            true ->
              {{404, "application/json", ~s({{"message":"not found"}})}}
          end

        status_text =
          case status do
            200 -> "200 OK"
            204 -> "204 No Content"
            400 -> "400 Bad Request"
            404 -> "404 Not Found"
            503 -> "503 Service Unavailable"
            _ -> "500 Internal Server Error"
          end

        resp =
          "HTTP/1.1 #{{status_text}}\r\nContent-Type: #{{ct}}\r\nContent-Length: #{{byte_size(body)}}\r\nConnection: close\r\n\r\n#{{body}}"

        :gen_tcp.send(sock, resp)
        :gen_tcp.close(sock)

      _ ->
        :gen_tcp.close(sock)
    end
  end
end

ToolchainRunner.run()
"###,
        require_block
    );

    tmp.write("runner.exs", &runner_exs);

    let mut cmd = Command::new(elixir_bin);
    cmd.arg("runner.exs").current_dir(&tmp.path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
        if let Some(erts_bin) = resolve_latest_erts_bin() {
            let mut paths = vec![erts_bin];
            if let Some(existing) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&existing));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
    }
    let out = cmd.output().expect("execute elixir runner.exs");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains("ELIXIR_TOOLCHAIN_OK") && stderr.trim().is_empty(),
        "elixir runner.exs failed or emitted warnings! (status: {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
}

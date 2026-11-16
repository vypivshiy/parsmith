mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use common::*;
use parsmith::codegen::csharp::{
    csharp_ident, csharp_pascal, csharp_replacement, validate_csharp_namespace,
};
use parsmith::{
    CSharpEmitter, CodeWriter, CodegenOptions, DiagnosticCode, ModuleBuilder, Target, drive_module,
    emit_csharp_module, emit_csharp_runtime_module, generate_path, generate_source,
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

(raw)struct RuneExtractor {
    verified {
        assert "expected 5 Unicode runes" {
            len-eq 5
        }
    }
}

(raw)fn extract_version {
    @doc "Extract semantic version from raw string."
    re #"version=([0-9]+\.[0-9]+\.[0-9]+)"#
}
"####;

const SCHEMA_XPATH_COVERAGE: &str = r#"
@doc "XPath 1.0 extraction and predicate coverage using AngleSharp.XPath."

struct XpathPage {
    heading {
        xpath "//h1"
        text
        trim
    }

    clean-box {
        xpath "//div[@class='box']"
        xpath-remove "//script"
        text
        trim
    }

    items {
        xpath-all "//ul[@class='items']/li"
        filter {
            xpath ".//span[@class='keep']"
        }
        text
        trim
    }

    has-footer {
        xpath "//footer"
        to-bool
    }
}
"#;

const SCHEMA_EXTENSIONS: &str = r####"
extension Utils {
    normalize {
        sig str str

        cs {
            helper NormalizeValue {
                import "System.Text"
                source #"""
                    public static string NormalizeValue(string value)
                    {
                        return string.Join(" ", value.Trim().Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));
                    }
                    """#
            }
            emit #"var {{out}} = NormalizeValue({{in}});"#
        }
    }

    truthy {
        sig T bool
        csharp {
            emit #"var {{out}} = !string.IsNullOrEmpty({{in}});"#
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
    @doc "Article HTML page parser with HttpClient fetch."

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
fn csharp_item_init_check_and_conditional_imports() {
    let opts = CodegenOptions::new(Target::CSharp);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("item_init.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("#nullable enable"));
    assert!(code.contains("using AngleSharp.Dom;"));
    assert!(code.contains("using AngleSharp.Html.Parser;"));
    assert!(
        !code.contains("using AngleSharp.XPath;"),
        "CSS-only schema must omit AngleSharp.XPath"
    );
    assert!(code.contains("public sealed record SingleCardType("));
    assert!(code.contains("public sealed record PageItemType("));
    assert!(code.contains("public sealed class PageItem"));
}

#[test]
fn csharp_xpath_enables_anglesharp_xpath() {
    let opts = CodegenOptions::new(Target::CSharp);
    let res = generate_source(SCHEMA_XPATH_COVERAGE, Some("xpath.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("using AngleSharp.XPath;"));
    assert!(code.contains("Rt.XpathSelect("));
    assert!(code.contains("Rt.XpathSelectAll("));
    assert!(code.contains("Rt.XpathRemove("));
    assert!(code.contains("Rt.PredXpath("));
}

#[test]
fn csharp_raw_and_json_and_rest_omit_anglesharp() {
    let opts = CodegenOptions::new(Target::CSharp);

    let raw_res = generate_source(SCHEMA_RAW_AND_FN, Some("raw_fn.kdl"), &opts);
    assert!(!raw_res.has_errors(), "{:?}", raw_res.diagnostics);
    let raw_code = raw_res.primary_code().unwrap();
    assert!(
        !raw_code.contains("AngleSharp"),
        "pure raw module must omit AngleSharp dependency"
    );

    let json_res = generate_source(SCHEMA_CATALOG_JSON_PROJECTORS, Some("json_proj.kdl"), &opts);
    assert!(!json_res.has_errors(), "{:?}", json_res.diagnostics);
    let json_code = json_res.primary_code().unwrap();
    assert!(
        !json_code.contains("AngleSharp"),
        "pure JSON + raw module must omit AngleSharp dependency"
    );
    assert!(json_code.contains("public static ReleaseCatalogJson ProjectReleaseCatalog("));

    let rest_res = generate_source(SCHEMA_REST_CLIENT, Some("rest.kdl"), &opts);
    assert!(!rest_res.has_errors(), "{:?}", rest_res.diagnostics);
    let rest_code = rest_res.primary_code().unwrap();
    assert!(
        !rest_code.contains("AngleSharp"),
        "pure REST module must omit AngleSharp dependency"
    );
    assert!(rest_code.contains("public abstract record GetUserResult"));
    assert!(rest_code.contains("public sealed record Ok(UserJson Value) : GetUserResult"));
    assert!(rest_code.contains(
        "public sealed record DecodeErr(int Status, string Body, string Cause) : GetUserResult"
    ));
    assert!(
        rest_code.contains("public sealed record TransportErr(Exception Cause) : GetUserResult")
    );
    assert!(
        rest_code.contains(
            "public sealed record UnknownErr(int Status, JsonNode? Body) : GetUserResult"
        )
    );
    assert!(rest_code.contains(
        "public sealed record Err404NotFound(int Status, SpecificNotFoundErrorJson Body) : GetUserResult"
    ));
}

#[test]
fn csharp_separate_runtime_and_namespace_package() {
    let opts = CodegenOptions::new(Target::CSharp)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime")
        .with_package("MyApp.Parsers");
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("page_sep.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert_eq!(res.files.len(), 2);
    let parser_cs = &res.files["page_sep.cs"];
    let runtime_cs = &res.files["parsmith_runtime.cs"];
    assert!(parser_cs.contains("namespace MyApp.Parsers;"));
    assert!(parser_cs.contains("using static MyApp.Parsers.Rt;"));
    assert!(!parser_cs.contains("public static partial class Rt"));
    assert!(runtime_cs.contains("namespace MyApp.Parsers;"));
    assert!(runtime_cs.contains("public static partial class Rt"));
}

#[test]
fn csharp_missing_extension_target_reports_e400() {
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
    let opts = CodegenOptions::new(Target::CSharp);
    let res = generate_source(src, Some("missing_cs_ext.kdl"), &opts);
    assert!(res.has_errors());
    assert!(res.diagnostics.iter().any(|d| {
        d.code == DiagnosticCode::E400
            && d.message.contains("OnlyPy.noop")
            && (d.message.contains("'cs'") || d.message.contains("'csharp'"))
    }));
}

#[test]
fn csharp_invalid_runtime_name_and_namespace_report_e002() {
    let bad_rt_opts = CodegenOptions::new(Target::CSharp)
        .with_separate_runtime(true)
        .with_runtime_name("123-bad-name");
    let res_rt = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &bad_rt_opts);
    assert!(res_rt.has_errors());
    assert!(
        res_rt
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );

    let bad_ns_opts = CodegenOptions::new(Target::CSharp).with_package("123.Invalid..Namespace");
    let res_ns = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &bad_ns_opts);
    assert!(res_ns.has_errors());
    assert!(
        res_ns
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );

    assert!(validate_csharp_namespace(""));
    assert!(validate_csharp_namespace("main"));
    assert!(validate_csharp_namespace("MyApp.Parsers"));
    assert!(!validate_csharp_namespace("MyApp..Parsers"));
    assert!(!validate_csharp_namespace("MyApp.int"));
}

#[test]
fn csharp_escapable_keywords_use_verbatim_identifiers() {
    let src = r#"
struct Data {
    ref { css "a"; text }
    type { css "b"; text }
    match { css "c"; text }
}
(rest)struct Api {
    @request name=get """
    GET /?pub={{pub}}&async={{async:bool?}}&class={{class}}&int={{int:int?}} HTTP/1.1
    Host: example.com
    """
}
"#;
    let opts = CodegenOptions::new(Target::CSharp);
    let res = generate_source(src, Some("keywords.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("string @class"));
    assert!(code.contains("long? @int = null"));
    assert_eq!(csharp_ident("ref"), "@ref");
    assert_eq!(csharp_ident("class"), "@class");
    assert_eq!(csharp_ident("namespace"), "@namespace");
    assert_eq!(csharp_pascal("single-card"), "SingleCard");
}

#[test]
fn csharp_replacement_translates_groups_and_escapes_dollar() {
    assert_eq!(csharp_replacement(r"\1-ok"), "$1-ok");
    assert_eq!(csharp_replacement("$1-ok"), "$1-ok");
    assert_eq!(csharp_replacement(r"\1_ok"), "${1}_ok");
    assert_eq!(csharp_replacement("$1_ok"), "${1}_ok");
    assert_eq!(csharp_replacement("price: $foo"), "price: $$foo");
    assert_eq!(csharp_replacement("$$"), "$$");
    assert_eq!(csharp_replacement("$"), "$$");
}

#[test]
fn csharp_target_parsing_and_two_pass_dry_run_zero_allocation() {
    assert_eq!(
        parse_target_or_shorthand("csharp").unwrap(),
        (Target::CSharp, None)
    );
    assert_eq!(
        parse_target_or_shorthand("cs").unwrap(),
        (Target::CSharp, None)
    );
    assert_eq!(Target::CSharp.as_str(), "csharp");
    assert_eq!(Target::CSharp.ecosystem_name(), "csharp");
    assert_eq!(Target::CSharp.extension(), "cs");

    let module = parsmith::compile_source(SCHEMA_ITEM_INIT_CHECK, Some("item.kdl")).unwrap();
    let mut emitter = CSharpEmitter::new(CodegenOptions::new(Target::CSharp));
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

const SCHEMA_VALUE_AND_AUTOCAST: &str = r#"
define SOURCE="habr"

struct FeedCard {
    @init {
        seed_ver int value=1
        seed_tag str value="{{SOURCE}}" { upper }
    }

    source str value="{{SOURCE}}"
    kind value="article"
    version int value=2
    ratio float value=1.5
    is_live bool value=#false
    seeded str value="{{SOURCE}}" { upper }
    from_init str { @seed_tag }
    views int { css ".views"; text; fallback 0 }
    score float { css ".score"; text; fallback 0.0 }
    has_badge bool { css ".badge" }
    tags (array)int { css-all ".tag"; text }
}

json ReleaseCard {
    source str value="{{SOURCE}}"
    version int value=1
    ratio float value=2.5
    is_live bool value=#true
    id str from="node_id"
}
"#;

#[test]
fn csharp_value_constants_and_postfix_autocast() {
    let opts = CodegenOptions::new(Target::CSharp);
    let res = generate_source(SCHEMA_VALUE_AND_AUTOCAST, Some("value_cast.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();

    // StepOp::Value in @init and struct fields
    assert!(code.contains("var v1 = 1L;"));
    assert!(code.contains("var v1 = \"habr\";"));
    assert!(code.contains("var v1 = \"article\";"));
    assert!(code.contains("var v1 = 2L;"));
    assert!(code.contains("var v1 = 1.5;"));
    assert!(code.contains("var v1 = false;"));
    assert!(code.contains("var v2 = Rt.Upper(v1);"));

    // Postfix auto-cast in struct fields
    assert!(code.contains("Rt.ToInt("));
    assert!(code.contains("Rt.ToFloat("));
    assert!(code.contains("Rt.NodeBool("));

    // JsonFieldDef.value in json projector
    assert!(code.contains("string _f_source = \"habr\";"));
    assert!(code.contains("long _f_version = 1L;"));
    assert!(code.contains("double _f_ratio = 2.5;"));
    assert!(code.contains("bool _f_is_live = true;"));
}

#[test]
fn csharp_toolchain_check_and_execution() {
    let tmp = TempTestDir::new("csharp_toolchain");
    let csproj = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net8.0</TargetFramework>
    <Nullable>enable</Nullable>
    <NoWarn>$(NoWarn);NU1902;NU1903</NoWarn>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="AngleSharp" Version="1.2.0" />
    <PackageReference Include="AngleSharp.XPath" Version="2.0.4" />
  </ItemGroup>
</Project>
"#;
    tmp.write("Runner.csproj", csproj);

    let schemas = [
        ("SepItem", SCHEMA_ITEM_INIT_CHECK),
        ("SepList", SCHEMA_LIST_STRINGS_ARRAYS),
        ("SepFilters", SCHEMA_FILTERS_AND_PREDICATES),
        ("SepFlatDictTable", SCHEMA_FLAT_DICT_TABLE),
        ("SepRaw", SCHEMA_RAW_AND_FN),
        ("SepHtmlFn", SCHEMA_HTML_FN),
        ("SepXpath", SCHEMA_XPATH_COVERAGE),
        ("SepExt", SCHEMA_EXTENSIONS),
        ("SepJson", SCHEMA_CATALOG_JSON_PROJECTORS),
        ("SepFetch", SCHEMA_HTML_FETCH_REQUESTS),
        ("SepRest", SCHEMA_REST_CLIENT),
        ("SepValueCast", SCHEMA_VALUE_AND_AUTOCAST),
    ];

    // 1. Emit each schema in inline runtime mode (in namespace `Inline<Idx>`) and separate runtime mode (in namespace `<SepName>` with shared `SepRt`)
    for (idx, (ns_name, src)) in schemas.iter().enumerate() {
        let inline_ns = format!("Inline{idx}");
        let inline_res = generate_source(
            src,
            Some(&format!("{inline_ns}.kdl")),
            &CodegenOptions::new(Target::CSharp).with_package(&inline_ns),
        );
        assert!(
            !inline_res.has_errors(),
            "inline C# generation failed for {ns_name}: {:?}",
            inline_res.diagnostics
        );
        tmp.write(
            &format!("{inline_ns}.cs"),
            inline_res.primary_code().unwrap(),
        );

        let sep_opts = CodegenOptions::new(Target::CSharp)
            .with_separate_runtime(true)
            .with_runtime_name(format!("{ns_name}Rt"))
            .with_package(*ns_name);
        let sep_res = generate_source(src, Some(&format!("{ns_name}.kdl")), &sep_opts);
        assert!(
            !sep_res.has_errors(),
            "separate C# generation failed for {ns_name}: {:?}",
            sep_res.diagnostics
        );
        tmp.write(
            &format!("{ns_name}.cs"),
            &sep_res.files[&format!("{ns_name}.cs")],
        );
        tmp.write(
            &format!("{ns_name}Rt.cs"),
            &sep_res.files[&format!("{ns_name}Rt.cs")],
        );
    }

    // 2. Emit live loopback versions of SCHEMA_HTML_FETCH_REQUESTS and SCHEMA_REST_CLIENT
    let live_fetch_kdl = SCHEMA_HTML_FETCH_REQUESTS
        .replace("GET /", "GET http://127.0.0.1:{{port:int}}/")
        .replace("POST /", "POST http://127.0.0.1:{{port:int}}/")
        .replace("    Host: example.com\n", "");
    let live_fetch_mod = parsmith::compile_source(&live_fetch_kdl, Some("live_fetch.kdl")).unwrap();
    let live_fetch_opts = CodegenOptions::new(Target::CSharp)
        .with_separate_runtime(true)
        .with_runtime_name("LiveFetchRt")
        .with_package("LiveFetch");
    let (live_fetch_code, live_fetch_rt, live_fetch_builder) =
        emit_csharp_module(&live_fetch_mod, &live_fetch_opts).expect("emit LiveFetch");
    tmp.write("LiveFetch.cs", &live_fetch_code);
    tmp.write(
        "LiveFetchRt.cs",
        &live_fetch_rt.unwrap_or_else(|| {
            emit_csharp_runtime_module(&live_fetch_mod, &live_fetch_builder, &live_fetch_opts)
        }),
    );

    let live_rest_kdl = SCHEMA_REST_CLIENT
        .replace("GET /", "GET http://127.0.0.1:{{port:int}}/")
        .replace("POST /", "POST http://127.0.0.1:{{port:int}}/")
        .replace("    Host: api.example.com\n", "");
    let live_rest_opts = CodegenOptions::new(Target::CSharp).with_package("LiveRest");
    let live_rest_res = generate_source(&live_rest_kdl, Some("live_rest.kdl"), &live_rest_opts);
    assert!(
        !live_rest_res.has_errors(),
        "{:?}",
        live_rest_res.diagnostics
    );
    tmp.write("LiveRest.cs", live_rest_res.primary_code().unwrap());

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
        let ns_name = format!("IntSchema{idx}");
        let res = generate_path(
            path,
            &CodegenOptions::new(Target::CSharp).with_package(&ns_name),
        );
        assert!(
            !res.has_errors(),
            "C# generation failed for {fname}: {:?}",
            res.diagnostics
        );
        tmp.write(&format!("{ns_name}.cs"), res.primary_code().unwrap());
    }

    let dsl_coverage_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/integration/fixtures/dsl_coverage.html");
    tmp.write(
        "dsl_coverage.html",
        &fs::read_to_string(&dsl_coverage_path).expect("read dsl_coverage.html"),
    );

    let program_cs = r###"#nullable enable
using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading.Tasks;

public static class Program
{
    private static void Assert(bool cond, string message = "Assertion failed")
    {
        if (!cond)
        {
            throw new Exception(message);
        }
    }

    public static async Task Main()
    {
        // 1. HTML Item, List, Filters, Flat, Dict, Table, Functions, and XPath
        var itemHtml = @"<html><head><link rel=""canonical"" href=""https://example.test/item""></head><body><h1>Main Heading</h1><div class=""card"">c</div><section class=""single""><h1>Card Title</h1><span class=""is-active"">1</span></section></body></html>";
        var pInline = new Inline0.PageItem(itemHtml);
        Assert(pInline.HasCards(), "pInline.HasCards()");
        var dInline = pInline.Parse();
        Assert(dInline.Heading == "Main Heading", dInline.Heading);
        Assert(dInline.Canonical == "https://example.test/item", dInline.Canonical);
        Assert(dInline.Single.Title == "Card Title", dInline.Single.Title);
        Assert(dInline.Single.Active, "dInline.Single.Active");

        var pSep = SepItem.PageItem.New(itemHtml);
        var dSep = pSep.Parse();
        Assert(dSep.Heading == dInline.Heading, "dSep.Heading");
        Assert(dSep.Single.Title == dInline.Single.Title, "dSep.Single.Title");

        var listHtml = @"<div class=""product-card""><div class=""ad"">ad</div><h2 class=""title"">  foo &amp; Widget  </h2><a class=""link"" href=""/-my-slug-.html"">Link</a><img src=""/img.png""><span class=""tags""> alpha, beta , gamma, delta </span><span class=""sku"">SKU-42</span><span class=""price"">$19.99</span><span class=""stock-badge"">in</span></div>";
        var listP = new SepList.ProductCatalog(listHtml);
        var items = listP.Parse();
        Assert(items.Count == 1, $"items.Count={items.Count}");
        Assert(items[0].Title == "bar & Widget", items[0].Title);
        Assert(items[0].Slug == "https://example.test/my-slug?ref=my-slug", items[0].Slug);
        Assert(string.Join(",", items[0].Tags) == "ALPHA,BETA,GAMMA", string.Join(",", items[0].Tags));
        Assert(items[0].FirstTag == "alpha", items[0].FirstTag);
        Assert(items[0].Sku == 42L, $"Sku={items[0].Sku}");
        Assert(Math.Abs(items[0].Price - 19.99) < 1e-9, $"Price={items[0].Price}");
        Assert(items[0].InStock, "InStock");

        var filterHtml = @"<div><a href=""#skip"" data-role=""nav"">Skip</a><a href=""/nav"" data-role=""nav"">Nav</a><a href=""/more"">Read More</a><span class=""code"">PRD-100-OK</span><span class=""code"">PRD-TEST-OK</span></div>";
        var fRes = new SepFilters.FilteredPage(filterHtml).Parse();
        Assert(string.Join(",", fRes.ActiveLinks) == "/nav,/more", string.Join(",", fRes.ActiveLinks));
        Assert(string.Join(",", fRes.ValidCodes) == "PRD-100-OK", string.Join(",", fRes.ValidCodes));

        var fdtHtml = @"<html><head><meta name=""Author"" content="" Alice ""><meta name=""Theme"" content=""dark""></head><body><span class=""tag-primary"">t1</span><span class=""tag-primary"">t2</span><table class=""specs""><tr><th>Product ID</th><td>P-900</td></tr><tr><th>Quantity</th><td>5</td></tr><tr><th>Price</th><td>12.5</td></tr></table></body></html>";
        var flatRes = new SepFlatDictTable.FlatTags(fdtHtml).Parse();
        Assert(string.Join(",", flatRes) == "t1,t2", string.Join(",", flatRes));
        var dictRes = new SepFlatDictTable.MetaProps(fdtHtml).Parse();
        Assert(dictRes["author"] == "Alice", dictRes["author"]);
        var tblRes = new SepFlatDictTable.ProductSpecs(fdtHtml).Parse();
        Assert(tblRes.ProductId == "P-900", tblRes.ProductId);
        Assert(tblRes.TotalQty == 5L, $"TotalQty={tblRes.TotalQty}");
        Assert(Math.Abs(tblRes.UnitPrice - 12.5) < 1e-9, $"UnitPrice={tblRes.UnitPrice}");

        var fnHtml = @"<html><body><h1>Hello Fn</h1><div class=""card featured""><h2>Featured!</h2></div></body></html>";
        Assert(SepHtmlFn.Functions.PageTitle(fnHtml) == "Hello Fn", "PageTitle");
        Assert(SepHtmlFn.Functions.ParseFeaturedCard(fnHtml).Title == "Featured!", "ParseFeaturedCard");

        var xpathHtml = @"<html><body><h1>XPath Heading</h1><div class=""box"">KeepText<script>bad()</script></div><ul class=""items""><li><span class=""keep"">Item 1</span></li><li><span>Skip</span></li><li><span class=""keep"">Item 2</span></li></ul><footer>Footer</footer></body></html>";
        var xpRes = new SepXpath.XpathPage(xpathHtml).Parse();
        Assert(xpRes.Heading == "XPath Heading", xpRes.Heading);
        Assert(xpRes.CleanBox == "KeepText", xpRes.CleanBox);
        Assert(string.Join("|", xpRes.Items) == "Item 1|Item 2", string.Join("|", xpRes.Items));
        Assert(xpRes.HasFooter, "xpRes.HasFooter");

        // Verify 00_full.kdl (IntSchema0) against dsl_coverage.html
        var covHtml = File.ReadAllText("dsl_coverage.html", Encoding.UTF8);
        var covRes = new IntSchema0.CoverageRoot(covHtml).Parse();
        Assert(covRes.Title == "DSL Coverage Fixture", covRes.Title);
        Assert(covRes.Canonical == "https://example.test/coverage", covRes.Canonical ?? "null");
        Assert(covRes.FirstAuthor.Name == "Author One" && covRes.FirstAuthor.Slug == "author-one", "FirstAuthor");
        Assert(covRes.NestedItem.Title == "Single Item Title" && covRes.NestedItem.Active, "NestedItem");
        Assert(covRes.Cards.Count == 2, $"Cards.Count={covRes.Cards.Count}");
        Assert(covRes.TableData.Identifier == "ABC-123", covRes.TableData.Identifier);
        Assert(covRes.BodySize == 3L, $"BodySize={covRes.BodySize}");
        Assert(covRes.CodeLengthOk == "foo   bar", covRes.CodeLengthOk);

        // 2. Raw, Unicode Runes, and Extensions
        var rawLogs = "[WARN] disk full\nplain message";
        var entries = new SepRaw.LogEntry(rawLogs).Parse();
        Assert(entries.Count == 2, $"entries.Count={entries.Count}");
        Assert(entries[0].Level == "WARN" && entries[0].Message == "disk full", "entries[0]");
        Assert(entries[1].Level == "INFO" && entries[1].Message == "plain message", "entries[1]");
        Assert(SepRaw.Functions.ExtractVersion("v=1 version=2.4.6 ok") == "2.4.6", "ExtractVersion");

        var runeRes = new SepRaw.RuneExtractor("a😀b🚀c").Parse();
        Assert(runeRes.Verified == "a😀b🚀c", $"Verified={runeRes.Verified}");
        Assert(SepRaw.Rt.StrLen("a😀b🚀c") == 5L, "StrLen(a😀b🚀c)");
        Assert(SepRaw.Rt.StrIndex("a😀b🚀c", 1L, "test") == "😀", "StrIndex(1)");
        Assert(SepRaw.Rt.StrIndex("a😀b🚀c", -2L, "test") == "🚀", "StrIndex(-2)");
        Assert(SepRaw.Rt.StrSlice("a😀b🚀c", 1L, 4L) == "😀b🚀", "StrSlice(1, 4)");

        var extP = new SepExt.ExtRawParser("  hello   csharp   world ");
        Assert(extP.IsValid(), "extP.IsValid()");
        Assert(extP.Parse().Normalized == "hello csharp world", "extP.Parse().Normalized");
        Assert(SepExt.Functions.NormalizeText("  a   b  ") == "a b", "NormalizeText");

        // 3. Strict JSON Projectors and GJSON paths
        var catJson = @"{
            ""@id"": 10,
            ""os.version"": ""24.04"",
            ""meta"": { ""build.number"": ""b42"" },
            ""a\\b"": ""bs_val"",
            ""#"": ""hash_val"",
            ""rating_scores"": [99, 88],
            ""tags"": [
                { ""name"": ""csharp"" },
                { ""other"": ""skip"" },
                { ""name"": ""kdl"" }
            ],
            ""author"": {
                ""dc:creator"": ""Alex"",
                ""verified"": true
            },
            ""links"": [
                { ""id"": 1, ""relation"": ""parent"" }
            ],
            ""channels"": {
                ""stable"": {
                    ""stream_endpoint"": ""https://cdn.test/s"",
                    ""codec"": null,
                    ""internal_flag"": true
                }
            },
            ""translations"": {
                ""en"": {
                    ""wire_name"": ""English"",
                    ""active"": true
                }
            }
        }";
        var envelope = $@"{{
            ""@id"": 10,
            ""os.version"": ""24.04"",
            ""meta"": {{ ""build.number"": ""b42"" }},
            ""a\\b"": ""bs_val"",
            ""#"": ""hash_val"",
            ""rating_scores"": [99, 88],
            ""tags"": [{{ ""name"": ""csharp"" }}, {{ ""name"": ""kdl"" }}],
            ""author"": {{ ""dc:creator"": ""Alex"", ""verified"": true }},
            ""links"": [{{ ""id"": 1, ""relation"": ""parent"" }}],
            ""channels"": {{ ""stable"": {{ ""stream_endpoint"": ""https://cdn.test/s"", ""codec"": null }} }},
            ""payload.data"": {catJson}
        }}";
        var parsedCat = new SepJson.CatalogExtractor(envelope).Parse();
        Assert(parsedCat.Catalog.ReleaseId == 10L, "ReleaseId");
        Assert(parsedCat.Catalog.OsVersion == "24.04", "OsVersion");
        Assert(parsedCat.Catalog.NestedDot == "b42", "NestedDot");
        Assert(parsedCat.Catalog.BackslashKey == "bs_val", "BackslashKey");
        Assert(parsedCat.Catalog.HashKey == "hash_val", "HashKey");
        Assert(parsedCat.Catalog.FirstScore == 99L, "FirstScore");
        Assert(string.Join(",", parsedCat.Catalog.TagNames) == "csharp,kdl", "TagNames");
        Assert(parsedCat.Catalog.TagCount == 2L, "TagCount");
        Assert(parsedCat.Catalog.MissingNote is null, "MissingNote");
        Assert(parsedCat.Catalog.OmittedVal is null, "OmittedVal");
        Assert(parsedCat.EnvelopeCatalog.Author.AuthorName == "Alex", "AuthorName");
        Assert(parsedCat.EnvelopeCatalog.Translations!["en"].Title == "English", "Translations[en]");

        var serializedCat = JsonNode.Parse(JsonSerializer.Serialize(parsedCat.Catalog))!.AsObject();
        Assert(!serializedCat.ContainsKey("omitted_val"), "omitted_val must be omitted");
        Assert(serializedCat.ContainsKey("missing_note") && serializedCat["missing_note"] is null, "missing_note must be null");

        bool threwArrayRoot = false;
        try { SepJson.GeoLocationJson.FromJson("[1, 2]"); } catch (SepJson.Rt.RtJsonSchemaError) { threwArrayRoot = true; }
        Assert(threwArrayRoot, "expected RtJsonSchemaError on array root");

        bool threwBadType = false;
        try { SepJson.GeoLocationJson.FromJson(@"{""lat"": ""bad"", ""lng"": 1.0}"); } catch (SepJson.Rt.RtJsonSchemaError) { threwBadType = true; }
        Assert(threwBadType, "expected RtJsonSchemaError on bad field type");

        // 3b. Value constants and postfix auto-cast
        var vcHtml = @"<div><span class=""views"">42</span><span class=""score"">9.5</span><span class=""badge"">ok</span><span class=""tag"">10</span><span class=""tag"">20</span></div>";
        var vcRes = new SepValueCast.FeedCard(vcHtml).Parse();
        Assert(vcRes.Source == "habr", vcRes.Source);
        Assert(vcRes.Kind == "article", vcRes.Kind);
        Assert(vcRes.Version == 2L, $"vcRes.Version={vcRes.Version}");
        Assert(Math.Abs(vcRes.Ratio - 1.5) < 1e-9, $"vcRes.Ratio={vcRes.Ratio}");
        Assert(!vcRes.IsLive, "vcRes.IsLive");
        Assert(vcRes.Seeded == "HABR", vcRes.Seeded);
        Assert(vcRes.FromInit == "HABR", vcRes.FromInit);
        Assert(vcRes.Views == 42L, $"vcRes.Views={vcRes.Views}");
        Assert(Math.Abs(vcRes.Score - 9.5) < 1e-9, $"vcRes.Score={vcRes.Score}");
        Assert(vcRes.HasBadge, "vcRes.HasBadge");
        Assert(string.Join(",", vcRes.Tags) == "10,20", string.Join(",", vcRes.Tags));

        var rcRes = SepValueCast.ReleaseCardJson.FromJson(@"{""node_id"":""rel-1""}");
        Assert(rcRes.Source == "habr", rcRes.Source);
        Assert(rcRes.Version == 1L, $"rcRes.Version={rcRes.Version}");
        Assert(Math.Abs(rcRes.Ratio - 2.5) < 1e-9, $"rcRes.Ratio={rcRes.Ratio}");
        Assert(rcRes.IsLive, "rcRes.IsLive");
        Assert(rcRes.Id == "rel-1", rcRes.Id);

        // 4. Live Loopback HTTP Fetch and REST Client tests
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        long port = ((IPEndPoint)listener.LocalEndpoint).Port;

        _ = Task.Run(async () =>
        {
            while (true)
            {
                TcpClient tcp;
                try
                {
                    tcp = await listener.AcceptTcpClientAsync();
                }
                catch
                {
                    break;
                }
                using (tcp)
                using (var stream = tcp.GetStream())
                {
                    var buf = new byte[4096];
                    int n = await stream.ReadAsync(buf);
                    if (n <= 0) continue;
                    var req = Encoding.UTF8.GetString(buf, 0, n);
                    var firstLine = req.Split("\r\n")[0];
                    int status = 404;
                    string ct = "application/json";
                    string body = @"{""message"":""not found""}";

                    if (firstLine.StartsWith("GET /articles/7?"))
                    {
                        status = 200; ct = "text/html"; body = "<html><body><h1>Article 7</h1></body></html>";
                    }
                    else if (firstLine.StartsWith("POST /articles/slug/hello"))
                    {
                        status = 200; ct = "text/html"; body = "<html><body><h1>Slug hello</h1></body></html>";
                    }
                    else if (firstLine.StartsWith("GET /logs/s1"))
                    {
                        status = 200; ct = "application/json"; body = @"{""payload"":{""lines"":[""l1"",""l2""]}}";
                    }
                    else if (firstLine.StartsWith("GET /users/1"))
                    {
                        status = 200; ct = "application/json"; body = @"{""id"":1,""name"":""Alice"",""nickname"":null}";
                    }
                    else if (firstLine.StartsWith("GET /users/2"))
                    {
                        status = 200; ct = "application/json"; body = @"{""error"":{""code"":1001,""message"":""Quota exceeded""}}";
                    }
                    else if (firstLine.StartsWith("GET /users/3"))
                    {
                        status = 404; ct = "application/json"; body = @"{""error"":""Not Found"",""code"":404}";
                    }
                    else if (firstLine.StartsWith("GET /users/4"))
                    {
                        status = 404; ct = "application/json"; body = @"{""error"":""Not Found"",""message"":""Fallback generic 404""}";
                    }
                    else if (firstLine.StartsWith("GET /users/5"))
                    {
                        status = 503; ct = "application/json"; body = @"{""reason"":""maintenance""}";
                    }
                    else if (firstLine.StartsWith("GET /users/6"))
                    {
                        status = 200; ct = "text/html"; body = "<html>not json</html>";
                    }
                    else if (firstLine.StartsWith("GET /users/7"))
                    {
                        status = 200; ct = "application/json"; body = @"{""id"":7}";
                    }
                    else if (firstLine.StartsWith("GET /users/8"))
                    {
                        status = 400; ct = "application/json"; body = @"{""unexpected"":""shape""}";
                    }
                    else if (firstLine.StartsWith("GET /ping"))
                    {
                        status = 204; ct = "text/plain"; body = "";
                    }
                    else if (firstLine.StartsWith("GET /me"))
                    {
                        status = 200; ct = "application/json"; body = @"{""data"":{""user"":{""id"":42,""name"":""Bob""}}}";
                    }
                    else if (firstLine.StartsWith("GET /articles"))
                    {
                        status = 200; ct = "application/json"; body = @"[{""id"":10,""title"":""News""}]";
                    }

                    string statusText = status switch
                    {
                        200 => "200 OK",
                        204 => "204 No Content",
                        400 => "400 Bad Request",
                        404 => "404 Not Found",
                        503 => "503 Service Unavailable",
                        _ => "500 Internal Server Error",
                    };
                    var bodyBytes = Encoding.UTF8.GetBytes(body);
                    var headerStr = $"HTTP/1.1 {statusText}\r\nContent-Type: {ct}\r\nContent-Length: {bodyBytes.Length}\r\nConnection: close\r\n\r\n";
                    var headerBytes = Encoding.UTF8.GetBytes(headerStr);
                    await stream.WriteAsync(headerBytes);
                    if (bodyBytes.Length > 0)
                    {
                        await stream.WriteAsync(bodyBytes);
                    }
                }
            }
        });

        using var client = new HttpClient();

        // HTML & Raw @request fetch (both async and sync)
        var art = await LiveFetch.ArticlePage.FetchAsync(
            client,
            port,
            7L,
            "s1",
            null,
            null,
            null,
            new[] { "a", "b" },
            new[] { 1L, 2L },
            new[] { "x", "y" },
            new[] { 10L, 20L },
            new[] { "r1", "r2" });
        Assert(art.Parse().Heading == "Article 7", "Article 7");

        var slugArt = LiveFetch.ArticlePage.FetchBySlug(client, port, "hello", "tok");
        Assert(slugArt.Parse().Heading == "Slug hello", "Slug hello");

        var logP = LiveFetch.ChunkedLog.Fetch(client, port, "s1");
        Assert(string.Join(",", logP.Parse().Lines) == "l1,l2", "ChunkedLog");

        // REST 200 OK
        var r1 = LiveRest.UserApi.GetUser(client, port, 1L);
        Assert(r1 is LiveRest.GetUserResult.Ok { Value: { Id: 1L, Name: "Alice", Nickname: null } }, $"r1={r1}");

        // REST @error 200 match
        var r2 = await LiveRest.UserApi.GetUserAsync(client, port, 2L);
        Assert(r2 is LiveRest.GetUserResult.Err200Code { Status: 200, Body.Code: 1001L }, $"r2={r2}");

        // REST @error:not-found 404 match vs fallthrough to @error 404 GenericError
        var r3 = LiveRest.UserApi.GetUser(client, port, 3L);
        Assert(r3 is LiveRest.GetUserResult.Err404NotFound { Status: 404, Body.Code: 404L }, $"r3={r3}");

        var r4 = LiveRest.UserApi.GetUser(client, port, 4L);
        Assert(r4 is LiveRest.GetUserResult.Err404 { Status: 404, Body.Message: "Fallback generic 404" }, $"r4={r4}");

        // REST UnknownErr(503)
        var r5 = LiveRest.UserApi.GetUser(client, port, 5L);
        Assert(r5 is LiveRest.GetUserResult.UnknownErr { Status: 503 }, $"r5={r5}");

        // REST DecodeErr(200) on HTML body and missing required field, and DecodeErr(400) on error schema mismatch
        var r6 = LiveRest.UserApi.GetUser(client, port, 6L);
        Assert(r6 is LiveRest.GetUserResult.DecodeErr { Status: 200 }, $"r6={r6}");

        var r7 = LiveRest.UserApi.GetUser(client, port, 7L);
        Assert(r7 is LiveRest.GetUserResult.DecodeErr { Status: 200 } dec7 && dec7.Cause.Contains("User.name"), $"r7={r7}");

        var r8 = LiveRest.UserApi.GetUser(client, port, 8L);
        Assert(r8 is LiveRest.GetUserResult.DecodeErr { Status: 400 }, $"r8={r8}");

        // Void, enveloped response-path, array response, and TransportErr on closed port
        var rPing = LiveRest.UserApi.Ping(client, port);
        Assert(rPing is LiveRest.PingResult.Ok { Status: 204 }, $"rPing={rPing}");

        var rMe = LiveRest.UserApi.GetProfileEnveloped(client, port);
        Assert(rMe is LiveRest.GetProfileEnvelopedResult.Ok { Value.Id: 42L }, $"rMe={rMe}");

        var rArts = LiveRest.UserApi.ListArticles(client, port);
        Assert(rArts is LiveRest.ListArticlesResult.Ok { Value: { Count: 1 } } okArts && okArts.Value[0].ArticleId == 10L, $"rArts={rArts}");

        var rClosed = LiveRest.UserApi.GetUser(
            client,
            1L,
            1L,
            null,
            LiveRest.Rt.WithTimeout(TimeSpan.FromMilliseconds(200)));
        Assert(rClosed is LiveRest.GetUserResult.TransportErr, $"rClosed={rClosed}");

        listener.Stop();
        Console.Write("CSHARP_TOOLCHAIN_OK");
    }
}
"###;
    tmp.write("Program.cs", program_cs);

    let run_out = Command::new("dotnet")
        .args(["run", "--project", "Runner.csproj"])
        .current_dir(&tmp.path)
        .output()
        .expect("execute dotnet run on generated C# modules");
    let stdout = String::from_utf8_lossy(&run_out.stdout);
    let stderr = String::from_utf8_lossy(&run_out.stderr);
    assert!(
        run_out.status.success(),
        "dotnet run failed!\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("CSHARP_TOOLCHAIN_OK"),
        "unexpected stdout: {stdout}"
    );
}

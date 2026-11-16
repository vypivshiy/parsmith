mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use common::*;
use parsmith::codegen::kotlin::{kotlin_ident, kotlin_pascal, kotlin_replacement};
use parsmith::{
    CodeWriter, CodegenOptions, DiagnosticCode, KotlinEmitter, ModuleBuilder, Target, drive_module,
    emit_kotlin_module, emit_kotlin_runtime_module, generate_path, generate_source,
    parse_target_or_shorthand, validate_kotlin_package_name,
};

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

(raw)struct UnicodeProbe {
    unescaped_text {
        unescape
        assert {
            len-eq 9
        }
    }
}
"####;

const SCHEMA_EXTENSIONS: &str = r####"
extension Utils {
    normalize {
        sig str str

        kt {
            helper normalizeValue {
                import "java.util.Locale"
                source #"""
                    fun normalizeValue(value: String): String {
                        return value.trim().split(Regex("\\s+")).joinToString(" ").lowercase(Locale.ROOT)
                    }
                    """#
            }
            emit #"val {{out}} = normalizeValue({{in}})"#
        }
    }

    truthy {
        sig T bool
        kotlin {
            emit #"val {{out}} = {{in}}.isNotEmpty()"#
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

#[test]
fn kotlin_item_init_check_and_xpath_generation() {
    let opts = CodegenOptions::new(Target::Kotlin);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("item_init.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("import org.jsoup.Jsoup"));
    assert!(code.contains("data class SingleCardType("));
    assert!(code.contains("data class PageItemType("));
    assert!(code.contains("class PageItem(private val _doc: Element)"));
    assert!(code.contains("fun hasCards(): Boolean"));
    assert!(code.contains("override fun toJsonElement(): JsonElement"));

    let xpath_res = generate_source(SCHEMA_XPATH_DOM, Some("xpath.kdl"), &opts);
    assert!(!xpath_res.has_errors(), "{:?}", xpath_res.diagnostics);
    let xpath_code = xpath_res.primary_code().unwrap();
    assert!(xpath_code.contains("Rt.xpathSelect("));
    assert!(xpath_code.contains("Rt.xpathSelectAll("));
    assert!(xpath_code.contains("Rt.xpathRemoveAll("));
    assert!(xpath_code.contains("Rt.predXpath("));
}

#[test]
fn kotlin_list_filters_flat_dict_table_generation() {
    let opts = CodegenOptions::new(Target::Kotlin);
    for (name, src) in [
        ("list_strings.kdl", SCHEMA_LIST_STRINGS_ARRAYS),
        ("filters_preds.kdl", SCHEMA_FILTERS_AND_PREDICATES),
        ("flat_dict_table.kdl", SCHEMA_FLAT_DICT_TABLE),
        ("html_fn.kdl", SCHEMA_HTML_FN),
    ] {
        let res = generate_source(src, Some(name), &opts);
        assert!(!res.has_errors(), "{name}: {:?}", res.diagnostics);
        let code = res.primary_code().unwrap();
        assert!(code.contains("object Rt {"));
    }
}

#[test]
fn kotlin_raw_json_and_rest_omit_jsoup_when_no_html() {
    let opts = CodegenOptions::new(Target::Kotlin);

    let raw_res = generate_source(SCHEMA_RAW_AND_FN, Some("raw_fn.kdl"), &opts);
    assert!(!raw_res.has_errors(), "{:?}", raw_res.diagnostics);
    let raw_code = raw_res.primary_code().unwrap();
    assert!(
        !raw_code.contains("org.jsoup"),
        "pure raw module must omit org.jsoup imports and helpers"
    );

    let json_res = generate_source(SCHEMA_CATALOG_JSON_PROJECTORS, Some("json_proj.kdl"), &opts);
    assert!(!json_res.has_errors(), "{:?}", json_res.diagnostics);
    let json_code = json_res.primary_code().unwrap();
    assert!(
        !json_code.contains("org.jsoup"),
        "pure JSON + raw module must omit org.jsoup imports and helpers"
    );
    assert!(
        json_code.contains("fun Rt.projectReleaseCatalog(el: JsonElement): ReleaseCatalogJson")
    );

    let rest_res = generate_source(SCHEMA_REST_CLIENT, Some("rest_client.kdl"), &opts);
    assert!(!rest_res.has_errors(), "{:?}", rest_res.diagnostics);
    let rest_code = rest_res.primary_code().unwrap();
    assert!(
        !rest_code.contains("org.jsoup"),
        "pure REST module must omit org.jsoup imports and helpers"
    );
    assert!(rest_code.contains("sealed interface GetUserResult"));
    assert!(rest_code.contains("data class Ok(val value: UserJson) : GetUserResult"));
    assert!(rest_code.contains(
        "data class DecodeErr(val status: Int, val body: String, val cause: String) : GetUserResult"
    ));
    assert!(rest_code.contains("data class TransportErr(val cause: Throwable) : GetUserResult"));
    assert!(rest_code.contains(
        "data class UnknownErr(val status: Int, val body: JsonElement?) : GetUserResult"
    ));
    assert!(rest_code.contains("data class Err404NotFound(val status: Int, val body: SpecificNotFoundErrorJson) : GetUserResult"));
}

#[test]
fn kotlin_separate_runtime_and_package_handling() {
    let default_opts = CodegenOptions::new(Target::Kotlin);
    let default_res = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &default_opts);
    assert!(!default_res.has_errors());
    assert!(
        !default_res.primary_code().unwrap().contains("\npackage "),
        "default 'main' package must omit package declaration in Kotlin"
    );

    let sep_opts = CodegenOptions::new(Target::Kotlin)
        .with_package("com.example.parsers")
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let sep_res = generate_source(SCHEMA_EXTENSIONS, Some("ext_sep.kdl"), &sep_opts);
    assert!(!sep_res.has_errors(), "{:?}", sep_res.diagnostics);
    assert_eq!(sep_res.files.len(), 2);
    let parser_code = &sep_res.files["ext_sep.kt"];
    let rt_code = &sep_res.files["parsmith_runtime.kt"];
    assert!(parser_code.contains("package com.example.parsers\n"));
    assert!(rt_code.contains("package com.example.parsers\n"));
    assert!(!parser_code.contains("object Rt {"));
    assert!(rt_code.contains("object Rt {"));
    assert!(rt_code.contains("fun normalizeValue("));
    assert!(parser_code.contains("import com.example.parsers.Rt.normalizeValue"));

    assert!(validate_kotlin_package_name(""));
    assert!(validate_kotlin_package_name("main"));
    assert!(validate_kotlin_package_name("com.example.parsers"));
    assert!(!validate_kotlin_package_name("123.bad"));
    assert!(!validate_kotlin_package_name("com..bad"));
    assert!(!validate_kotlin_package_name(".leading"));
}

#[test]
fn kotlin_invalid_package_and_runtime_name_report_e002() {
    let bad_pkg_opts = CodegenOptions::new(Target::Kotlin).with_package("123.invalid-pkg");
    let res1 = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &bad_pkg_opts);
    assert!(res1.has_errors());
    assert!(
        res1.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );

    let bad_rt_opts = CodegenOptions::new(Target::Kotlin)
        .with_separate_runtime(true)
        .with_runtime_name("123-bad-runtime");
    let res2 = generate_source(SCHEMA_RAW_AND_FN, Some("raw.kdl"), &bad_rt_opts);
    assert!(res2.has_errors());
    assert!(
        res2.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );
}

#[test]
fn kotlin_missing_extension_target_reports_e400() {
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
    let opts = CodegenOptions::new(Target::Kotlin);
    let res = generate_source(src, Some("missing_kt_ext.kdl"), &opts);
    assert!(res.has_errors());
    assert!(res.diagnostics.iter().any(|d| {
        d.code == DiagnosticCode::E400
            && d.message
                .contains("extension operation 'OnlyPy.noop' has no 'kt' target")
    }));
}

#[test]
fn kotlin_escapable_keywords_use_backtick_identifiers() {
    let src = r#"
struct Data {
    in { css "a"; text }
    object { css "b"; text }
    when { css "c"; text }
    val { css "d"; text }
    fun { css "e"; text }
}
(rest)struct Api {
    @request name=get """
    GET /?in={{in}}&object={{object:bool?}} HTTP/1.1
    Host: example.com
    """
}
"#;
    let opts = CodegenOptions::new(Target::Kotlin);
    let res = generate_source(src, Some("keywords.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.contains("val `in`: String,"));
    assert!(code.contains("val `object`: String,"));
    assert!(code.contains("val `when`: String,"));
    assert!(code.contains("val `val`: String,"));
    assert!(code.contains("val `fun`: String,"));
    assert!(code.contains("`in`: String"));
    assert!(code.contains("`object`: Boolean? = null"));
    assert_eq!(kotlin_ident("in"), "`in`");
    assert_eq!(kotlin_ident("object"), "`object`");
    assert_eq!(kotlin_ident("when"), "`when`");
    assert_eq!(kotlin_pascal("my-struct"), "MyStruct");
}

#[test]
fn kotlin_replacement_translates_groups_and_escapes_dollar() {
    assert_eq!(kotlin_replacement(r"\1-ok"), "$1-ok");
    assert_eq!(kotlin_replacement("$1-ok"), "$1-ok");
    assert_eq!(kotlin_replacement(r"\1_ok"), "$1_ok");
    assert_eq!(kotlin_replacement("$1_ok"), "$1_ok");
    assert_eq!(kotlin_replacement("price: $foo"), r"price: \$foo");
    assert_eq!(kotlin_replacement("$$"), r"\$");
    assert_eq!(kotlin_replacement("$"), r"\$");
}

#[test]
fn kotlin_target_parsing_and_two_pass_dry_run_zero_allocation() {
    assert_eq!(
        parse_target_or_shorthand("kotlin").unwrap(),
        (Target::Kotlin, None)
    );
    assert_eq!(
        parse_target_or_shorthand("kt").unwrap(),
        (Target::Kotlin, None)
    );
    assert_eq!(Target::Kotlin.as_str(), "kotlin");
    assert_eq!(Target::Kotlin.ecosystem_name(), "kotlin");
    assert_eq!(Target::Kotlin.extension(), "kt");

    let module = parsmith::compile_source(SCHEMA_ITEM_INIT_CHECK, Some("item.kdl")).unwrap();
    let mut emitter = KotlinEmitter::new(CodegenOptions::new(Target::Kotlin));
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

fn write_if_changed(path: impl AsRef<std::path::Path>, content: &str) {
    let path = path.as_ref();
    if let Ok(existing) = fs::read_to_string(path)
        && existing == content
    {
        return;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
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
fn kotlin_value_constants_and_postfix_autocast() {
    let opts = CodegenOptions::new(Target::Kotlin);
    let res = generate_source(SCHEMA_VALUE_AND_AUTOCAST, Some("value_cast.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();

    // StepOp::Value in @init and struct fields
    assert!(code.contains("val v1 = 1L"));
    assert!(code.contains("val v1 = \"habr\""));
    assert!(code.contains("val v1 = \"article\""));
    assert!(code.contains("val v1 = 2L"));
    assert!(code.contains("val v1 = 1.5"));
    assert!(code.contains("val v1 = false"));
    assert!(code.contains("val v2 = Rt.upper(v1)"));

    // Postfix auto-cast in struct fields
    assert!(code.contains("Rt.toInt("));
    assert!(code.contains("Rt.toFloat("));
    assert!(code.contains("Rt.nodeBool("));

    // JsonFieldDef.value in json projector
    assert!(code.contains("val source: String = \"habr\""));
    assert!(code.contains("val version: Long = 1L"));
    assert!(code.contains("val ratio: Double = 2.5"));
    assert!(code.contains("val isLive: Boolean = true"));
}

#[test]
fn kotlin_toolchain_check_and_execution() {
    let gradle_cmd = if cfg!(windows) {
        "gradle.bat"
    } else {
        "gradle"
    };

    let project_dir = std::env::temp_dir().join("parsmith_kotlin_gradle_test");
    let src_main = project_dir.join("src/main/kotlin");
    let src_test = project_dir.join("src/test/kotlin");
    fs::create_dir_all(&src_main).expect("create src/main/kotlin");
    fs::create_dir_all(&src_test).expect("create src/test/kotlin");

    let settings_gradle = "rootProject.name = \"parsmith-kotlin-toolchain-test\"\n";
    write_if_changed(project_dir.join("settings.gradle.kts"), settings_gradle);

    let gradle_props =
        "org.gradle.jvmargs=-Xmx2g -XX:MaxMetaspaceSize=512m\nkotlin.daemon.jvmargs=-Xmx2g\n";
    write_if_changed(project_dir.join("gradle.properties"), gradle_props);

    let build_gradle = r#"plugins {
    kotlin("jvm") version "2.1.10"
}

repositories {
    mavenCentral()
}

dependencies {
    implementation("org.jsoup:jsoup:1.18.3")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.0")
    testImplementation(kotlin("test"))
}

kotlin {
    jvmToolchain(17)
}

tasks.test {
    useJUnitPlatform()
    outputs.upToDateWhen { false }
}
"#;
    write_if_changed(project_dir.join("build.gradle.kts"), build_gradle);

    let schemas = [
        ("sep_item", SCHEMA_ITEM_INIT_CHECK),
        ("sep_list", SCHEMA_LIST_STRINGS_ARRAYS),
        ("sep_filters", SCHEMA_FILTERS_AND_PREDICATES),
        ("sep_flat_dict_table", SCHEMA_FLAT_DICT_TABLE),
        ("sep_xpath", SCHEMA_XPATH_DOM),
        ("sep_raw", SCHEMA_RAW_AND_FN),
        ("sep_html_fn", SCHEMA_HTML_FN),
        ("sep_ext", SCHEMA_EXTENSIONS),
        ("sep_json", SCHEMA_CATALOG_JSON_PROJECTORS),
        ("sep_fetch", SCHEMA_HTML_FETCH_REQUESTS),
        ("sep_rest", SCHEMA_REST_CLIENT),
        ("sep_value_cast", SCHEMA_VALUE_AND_AUTOCAST),
    ];

    let mut all_rt_builder = ModuleBuilder::new();

    // 1. Emit inline_0 (HTML) and inline_raw (pure Raw) in inline runtime mode,
    //    and emit all schemas in separate runtime mode in shared package `sep`
    let inline_0_res = generate_source(
        SCHEMA_ITEM_INIT_CHECK,
        Some("inline_0.kdl"),
        &CodegenOptions::new(Target::Kotlin).with_package("inline_0"),
    );
    assert!(!inline_0_res.has_errors(), "{:?}", inline_0_res.diagnostics);
    write_if_changed(
        src_main.join("inline_0.kt"),
        inline_0_res.primary_code().unwrap(),
    );

    for (sep_name, src) in &schemas {
        let module = parsmith::compile_source(src, Some(&format!("{sep_name}.kdl"))).unwrap();
        let sep_opts = CodegenOptions::new(Target::Kotlin)
            .with_package("sep")
            .with_separate_runtime(true)
            .with_runtime_name("parsmith_runtime");
        let (parser_code, _, builder) =
            emit_kotlin_module(&module, &sep_opts).expect("emit_kotlin_module");
        write_if_changed(src_main.join(format!("sep_{sep_name}.kt")), &parser_code);

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
    let live_fetch_opts = CodegenOptions::new(Target::Kotlin)
        .with_package("live_fetch")
        .with_separate_runtime(true)
        .with_prologue("import sep.Rt");
    let (live_fetch_code, _, _) =
        emit_kotlin_module(&live_fetch_mod, &live_fetch_opts).expect("emit live_fetch");
    write_if_changed(src_main.join("live_fetch.kt"), &live_fetch_code);

    let live_rest_kdl = SCHEMA_REST_CLIENT
        .replace("GET /", "GET http://127.0.0.1:{{port:int}}/")
        .replace("POST /", "POST http://127.0.0.1:{{port:int}}/")
        .replace("    Host: api.example.com\n", "");
    let live_rest_mod = parsmith::compile_source(&live_rest_kdl, Some("live_rest.kdl")).unwrap();
    let live_rest_opts = CodegenOptions::new(Target::Kotlin)
        .with_package("live_rest")
        .with_separate_runtime(true)
        .with_prologue("import sep.Rt");
    let (live_rest_code, _, _) =
        emit_kotlin_module(&live_rest_mod, &live_rest_opts).expect("emit live_rest");
    write_if_changed(src_main.join("live_rest.kt"), &live_rest_code);

    // Emit shared runtime module for package `sep` (using `live_fetch_mod` which has both HTML and HTTP enabled)
    let sep_rt_opts = CodegenOptions::new(Target::Kotlin)
        .with_package("sep")
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let shared_rt = emit_kotlin_runtime_module(&live_fetch_mod, &all_rt_builder, &sep_rt_opts);
    write_if_changed(src_main.join("parsmith_runtime.kt"), &shared_rt);

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
        let pkg_name = format!("int_schema_{idx}");
        let res = generate_path(
            path,
            &CodegenOptions::new(Target::Kotlin)
                .with_package(&pkg_name)
                .with_separate_runtime(true)
                .with_prologue("import sep.Rt"),
        );
        assert!(
            !res.has_errors(),
            "Kotlin generation failed for {fname}: {:?}",
            res.diagnostics
        );
        let stem = path.file_stem().unwrap().to_string_lossy();
        let parser_file = format!("{stem}.kt");
        write_if_changed(
            src_main.join(format!("{pkg_name}.kt")),
            &res.files[&parser_file],
        );
    }

    let test_kt = r###"import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.ServerSocket
import java.net.http.HttpClient
import java.nio.charset.StandardCharsets
import kotlin.concurrent.thread

class ParsmithKotlinGeneratedTest {
    @Test
    fun testHtmlCssXpathFiltersFlatDictTableAndFn() {
        val itemHtml = """<html><head><link rel="canonical" href="https://example.test/item"></head><body><h1>Main Heading</h1><div class="card">c</div><section class="single"><h1>Card Title</h1><span class="is-active">1</span></section></body></html>"""
        val pInline = inline_0.PageItem(itemHtml)
        assertTrue(pInline.hasCards())
        val dInline = pInline.parse()
        assertEquals("Main Heading", dInline.heading)
        assertEquals("https://example.test/item", dInline.canonical)
        assertEquals("Card Title", dInline.single.title)
        assertTrue(dInline.single.active)

        val pSep = sep.PageItem.new(itemHtml)
        val dSep = pSep.parse()
        assertEquals(dInline.heading, dSep.heading)
        assertEquals(dInline.single.title, dSep.single.title)
        assertEquals(dInline.toJsonString(), dSep.toJsonString())

        val listHtml = """<div class="product-card"><div class="ad">ad</div><h2 class="title">  foo &amp; Widget  </h2><a class="link" href="/-my-slug-.html">Link</a><img src="/img.png"><span class="tags"> alpha, beta , gamma, delta </span><span class="sku">SKU-42</span><span class="price">$19.99</span><span class="stock-badge">in</span></div>"""
        val listP = sep.ProductCatalog(listHtml)
        val items = listP.parse()
        assertEquals(1, items.size)
        assertFalse(items[0].cleanHtml.contains("class=\"ad\""))
        assertTrue(items[0].innerHtml.contains("class=\"ad\""))
        assertEquals("bar & Widget", items[0].title)
        assertEquals("https://example.test/my-slug?ref=my-slug", items[0].slug)
        assertEquals(listOf("ALPHA", "BETA", "GAMMA"), items[0].tags)
        assertEquals("alpha", items[0].firstTag)
        assertEquals(42L, items[0].sku)
        assertEquals(19.99, items[0].price)
        assertTrue(items[0].inStock)

        val filterHtml = """<div><a href="#skip" data-role="nav">Skip</a><a href="/nav" data-role="nav">Nav</a><a href="/more">Read More</a><span class="code">PRD-100-OK</span><span class="code">PRD-TEST-OK</span></div>"""
        val fRes = sep.FilteredPage(filterHtml).parse()
        assertEquals(listOf("/nav", "/more"), fRes.activeLinks)
        assertEquals(listOf("PRD-100-OK"), fRes.validCodes)

        val fdtHtml = """<html><head><meta name="Author" content=" Alice "><meta name="Theme" content="dark"></head><body><span class="tag-primary">t1</span><span class="tag-primary">t2</span><table class="specs"><tr><th>Product ID</th><td>P-900</td></tr><tr><th>Quantity</th><td>5</td></tr><tr><th>Price</th><td>12.5</td></tr></table></body></html>"""
        val flatRes = sep.FlatTags(fdtHtml).parse()
        assertEquals(listOf("t1", "t2"), flatRes)
        val dictRes = sep.MetaProps(fdtHtml).parse()
        assertEquals("Alice", dictRes["author"])
        val tblRes = sep.ProductSpecs(fdtHtml).parse()
        assertEquals("P-900", tblRes["product_id"]?.jsonPrimitive?.content)
        assertEquals("5", tblRes["total_qty"]?.jsonPrimitive?.content)
        assertEquals("12.5", tblRes["unit_price"]?.jsonPrimitive?.content)

        val xpathHtml = """<html><body><section class="single"><h1>XPath Heading</h1></section><div class="nested-card">Card One <a href="/1">Link1</a></div><div class="nested-card">No Link</div></body></html>"""
        val xpRes = sep.XpathArticle(xpathHtml).parse()
        assertEquals("XPath Heading", xpRes.heading)
        assertEquals(listOf("Card One"), xpRes.nestedNames)

        val fnHtml = """<html><body><h1>Hello Fn</h1><div class="card featured"><h2>Featured!</h2></div></body></html>"""
        assertEquals("Hello Fn", sep.pageTitle(fnHtml))
        assertEquals("Featured!", sep.parseFeaturedCard(fnHtml).title)
    }

    @Test
    fun testRawUnicodeCodepointsUnescapeAndExtensions() {
        val rawLogs = "[WARN] disk full\nplain message"
        val entries = sep.LogEntry(rawLogs).parse()
        assertEquals(2, entries.size)
        assertEquals("WARN", entries[0].level)
        assertEquals("disk full", entries[0].message)
        assertEquals("INFO", entries[1].level)
        assertEquals("plain message", entries[1].message)
        assertEquals("2.4.6", sep.extractVersion("v=1 version=2.4.6 ok"))

        // Non-BMP Unicode codepoints (🦀 and 🚀 are surrogate pairs in UTF-16, but single codepoints!)
        val unicodeInput = "a\uD83E\uDD80b\uD83D\uDE80&amp;&#x3A;\\u0041\\x42\\n"
        val probe = sep.UnicodeProbe(unicodeInput).parse()
        assertEquals("a\uD83E\uDD80b\uD83D\uDE80&:AB\n", probe.unescapedText)
        assertEquals(9L, sep.Rt.strLen(probe.unescapedText))
        assertEquals("\uD83E\uDD80", sep.Rt.strIndex(probe.unescapedText, 1L, "test"))
        assertEquals("\n", sep.Rt.strIndex(probe.unescapedText, -1L, "test"))
        assertEquals("\uD83E\uDD80b\uD83D\uDE80", sep.Rt.strSlice(probe.unescapedText, 1L, 4L))

        val extP = sep.ExtRawParser("  HELLO   Kotlin   World ")
        assertTrue(extP.isValid())
        assertEquals("hello kotlin world", extP.parse().normalized)
        assertEquals("a b", sep.normalizeText("  A   B  "))
    }

    @Test
    fun testStrictJsonProjectorsAndGjson() {
        val catJson = """{
            "@id": 10,
            "os.version": "24.04",
            "meta": { "build.number": "b42" },
            "a\\b": "bs_val",
            "#": "hash_val",
            "rating_scores": [99, 88],
            "tags": [
                { "name": "kotlin" },
                { "other": "skip" },
                { "name": "kdl" }
            ],
            "author": {
                "dc:creator": "Alex",
                "verified": true
            },
            "links": [
                { "id": 1, "relation": "parent" }
            ],
            "channels": {
                "stable": {
                    "stream_endpoint": "https://cdn.test/s",
                    "codec": null,
                    "internal_flag": true
                }
            },
            "translations": {
                "en": {
                    "wire_name": "English",
                    "active": true
                }
            }
        }"""
        val envelope = """{
            "@id": 10,
            "os.version": "24.04",
            "meta": { "build.number": "b42" },
            "a\\b": "bs_val",
            "#": "hash_val",
            "rating_scores": [99, 88],
            "tags": [{ "name": "kotlin" }, { "name": "kdl" }],
            "author": { "dc:creator": "Alex", "verified": true },
            "links": [{ "id": 1, "relation": "parent" }],
            "channels": { "stable": { "stream_endpoint": "https://cdn.test/s", "codec": null } },
            "payload.data": $catJson
        }"""
        val parsed = sep.CatalogExtractor(envelope).parse()
        assertEquals(10L, parsed.catalog.releaseId)
        assertEquals("24.04", parsed.catalog.osVersion)
        assertEquals("b42", parsed.catalog.nestedDot)
        assertEquals("bs_val", parsed.catalog.backslashKey)
        assertEquals("hash_val", parsed.catalog.hashKey)
        assertEquals(99L, parsed.catalog.firstScore)
        assertEquals(listOf("kotlin", "kdl"), parsed.catalog.tagNames)
        assertEquals(2L, parsed.catalog.tagCount)
        assertNull(parsed.catalog.missingNote)
        assertNull(parsed.catalog.omittedVal)
        assertEquals("Alex", parsed.envelopeCatalog.author.authorName)

        // Verify @omitempty omits `omitted_val` while nullable `missing_note` serializes as `null`
        val serialized = parsed.catalog.toJsonElement() as JsonObject
        assertFalse(serialized.containsKey("omitted_val"))
        assertEquals(JsonNull, serialized["missing_note"])

        // Verify strict error rejection on non-object root or wrong primitive type
        assertFailsWith<sep.Rt.RtError> {
            sep.GeoLocationJson.fromJson("[1, 2]")
        }
        assertFailsWith<sep.Rt.RtError> {
            sep.GeoLocationJson.fromJson("null")
        }
        assertFailsWith<sep.Rt.RtError> {
            sep.GeoLocationJson.fromJson("""{"lat": "bad", "lng": 1.0}""")
        }

        // Value constants and postfix auto-cast
        val vcHtml = """<div><span class="views">42</span><span class="score">9.5</span><span class="badge">ok</span><span class="tag">10</span><span class="tag">20</span></div>"""
        val vcRes = sep.FeedCard(vcHtml).parse()
        assertEquals("habr", vcRes.source)
        assertEquals("article", vcRes.kind)
        assertEquals(2L, vcRes.version)
        assertEquals(1.5, vcRes.ratio)
        assertFalse(vcRes.isLive)
        assertEquals("HABR", vcRes.seeded)
        assertEquals("HABR", vcRes.fromInit)
        assertEquals(42L, vcRes.views)
        assertEquals(9.5, vcRes.score)
        assertTrue(vcRes.hasBadge)
        assertEquals(listOf(10L, 20L), vcRes.tags)

        val rcRes = sep.ReleaseCardJson.fromJson("""{"node_id":"rel-1"}""")
        assertEquals("habr", rcRes.source)
        assertEquals(1L, rcRes.version)
        assertEquals(2.5, rcRes.ratio)
        assertTrue(rcRes.isLive)
        assertEquals("rel-1", rcRes.id)
    }

    @Test
    fun testFetchAndRestClientErrorFallthroughAndDecodeErr() {
        val server = ServerSocket(0)
        val port = server.localPort.toLong()

        val serverThread = thread(isDaemon = true) {
            while (!server.isClosed) {
                val sock = try {
                    server.accept()
                } catch (_: Throwable) {
                    break
                }
                val reader = BufferedReader(InputStreamReader(sock.getInputStream(), StandardCharsets.UTF_8))
                val firstLine = reader.readLine() ?: ""
                while (true) {
                    val line = reader.readLine()
                    if (line.isNullOrEmpty()) break
                }
                val (status, ct, body) = when {
                    firstLine.startsWith("GET /articles/7?") ->
                        Triple(200, "text/html", "<html><body><h1>Article 7</h1></body></html>")
                    firstLine.startsWith("POST /articles/slug/hello") ->
                        Triple(200, "text/html", "<html><body><h1>Slug hello</h1></body></html>")
                    firstLine.startsWith("GET /logs/s1") ->
                        Triple(200, "application/json", """{"payload":{"lines":["l1","l2"]}}""")
                    firstLine.startsWith("GET /users/1") ->
                        Triple(200, "application/json", """{"id":1,"name":"Alice","nickname":null}""")
                    firstLine.startsWith("GET /users/2") ->
                        Triple(200, "application/json", """{"error":{"code":1001,"message":"Quota exceeded"}}""")
                    firstLine.startsWith("GET /users/3") ->
                        Triple(404, "application/json", """{"error":"Not Found","code":404}""")
                    firstLine.startsWith("GET /users/4") ->
                        Triple(404, "application/json", """{"error":"Not Found","message":"Fallback generic 404"}""")
                    firstLine.startsWith("GET /users/5") ->
                        Triple(503, "application/json", """{"reason":"maintenance"}""")
                    firstLine.startsWith("GET /users/6") ->
                        Triple(200, "text/html", "<html>not json</html>")
                    firstLine.startsWith("GET /users/7") ->
                        Triple(200, "application/json", """{"id":7}""")
                    firstLine.startsWith("GET /users/8") ->
                        Triple(400, "application/json", """{"unexpected":"shape"}""")
                    firstLine.startsWith("GET /ping") ->
                        Triple(204, "text/plain", "")
                    firstLine.startsWith("GET /me") ->
                        Triple(200, "application/json", """{"data":{"user":{"id":42,"name":"Bob"}}}""")
                    firstLine.startsWith("GET /articles") ->
                        Triple(200, "application/json", """[{"id":10,"title":"News"}]""")
                    else ->
                        Triple(404, "application/json", """{"message":"not found"}""")
                }
                val statusText = when (status) {
                    200 -> "200 OK"
                    204 -> "204 No Content"
                    400 -> "400 Bad Request"
                    404 -> "404 Not Found"
                    503 -> "503 Service Unavailable"
                    else -> "500 Internal Server Error"
                }
                val bodyBytes = body.toByteArray(StandardCharsets.UTF_8)
                val headers = "HTTP/1.1 $statusText\r\nContent-Type: $ct\r\nContent-Length: ${bodyBytes.size}\r\nConnection: close\r\n\r\n"
                val out = sock.getOutputStream()
                out.write(headers.toByteArray(StandardCharsets.UTF_8))
                out.write(bodyBytes)
                out.flush()
                sock.close()
            }
        }

        val client = HttpClient.newHttpClient()

        try {
            // 1. HTML & Raw @request fetch
            val art = live_fetch.ArticlePage.fetch(
                client,
                port,
                7L,
                "s1",
                null,
                null,
                null,
                listOf("a", "b"),
                listOf(1L, 2L),
                listOf("x", "y"),
                listOf(10L, 20L),
                listOf("r1", "r2"),
                sep.Rt.withHeader("X-Extra", "1"),
            )
            assertEquals("Article 7", art.parse().heading)

            val slugArt = live_fetch.ArticlePage.fetchBySlug(client, port, "hello", "tok")
            assertEquals("Slug hello", slugArt.parse().heading)

            val logP = live_fetch.ChunkedLog.fetch(client, port, "s1")
            assertEquals(listOf("l1", "l2"), logP.parse().lines)

            // 2. REST 200 OK
            when (val r1 = live_rest.UserApi.getUser(client, port, 1L, null)) {
                is live_rest.GetUserResult.Ok -> {
                    assertEquals(1L, r1.value.id)
                    assertEquals("Alice", r1.value.name)
                    assertNull(r1.value.nickname)
                }
                else -> kotlin.test.fail("expected Ok, got $r1")
            }

            // 3. REST @error 200 match
            when (val r2 = live_rest.UserApi.getUser(client, port, 2L, null)) {
                is live_rest.GetUserResult.Err200Code -> {
                    assertEquals(200, r2.status)
                    assertEquals(1001L, r2.body.code)
                }
                else -> kotlin.test.fail("expected Err200Code, got $r2")
            }

            // 4. REST @error:not-found 404 match vs fallthrough to @error 404 GenericError
            when (val r3 = live_rest.UserApi.getUser(client, port, 3L, null)) {
                is live_rest.GetUserResult.Err404NotFound -> {
                    assertEquals(404, r3.status)
                    assertEquals(404L, r3.body.code)
                }
                else -> kotlin.test.fail("expected Err404NotFound, got $r3")
            }
            when (val r4 = live_rest.UserApi.getUser(client, port, 4L, null)) {
                is live_rest.GetUserResult.Err404 -> {
                    assertEquals(404, r4.status)
                    assertEquals("Fallback generic 404", r4.body.message)
                }
                else -> kotlin.test.fail("expected Err404 after fallthrough, got $r4")
            }

            // 5. REST UnknownErr(503, _)
            when (val r5 = live_rest.UserApi.getUser(client, port, 5L, null)) {
                is live_rest.GetUserResult.UnknownErr -> assertEquals(503, r5.status)
                else -> kotlin.test.fail("expected UnknownErr(503), got $r5")
            }

            // 6. REST DecodeErr(200, _) on HTML body and missing required field, and DecodeErr(400, _) on error schema mismatch
            when (val r6 = live_rest.UserApi.getUser(client, port, 6L, null)) {
                is live_rest.GetUserResult.DecodeErr -> assertEquals(200, r6.status)
                else -> kotlin.test.fail("expected DecodeErr(200) on HTML body, got $r6")
            }
            when (val r7 = live_rest.UserApi.getUser(client, port, 7L, null)) {
                is live_rest.GetUserResult.DecodeErr -> {
                    assertEquals(200, r7.status)
                    assertTrue(r7.cause.contains("User.name"))
                }
                else -> kotlin.test.fail("expected DecodeErr(200) on missing field, got $r7")
            }
            when (val r8 = live_rest.UserApi.getUser(client, port, 8L, null)) {
                is live_rest.GetUserResult.DecodeErr -> assertEquals(400, r8.status)
                else -> kotlin.test.fail("expected DecodeErr(400) on error schema mismatch, got $r8")
            }

            // 7. Void, enveloped response-path, array response, and TransportErr on closed port
            assertTrue(live_rest.UserApi.ping(client, port) is live_rest.PingResult.Ok)
            when (val me = live_rest.UserApi.getProfileEnveloped(client, port)) {
                is live_rest.GetProfileEnvelopedResult.Ok -> assertEquals(42L, me.value.id)
                else -> kotlin.test.fail("expected Ok(42), got $me")
            }
            when (val arts = live_rest.UserApi.listArticles(client, port)) {
                is live_rest.ListArticlesResult.Ok -> {
                    assertEquals(1, arts.value.size)
                    assertEquals(10L, arts.value[0].articleId)
                }
                else -> kotlin.test.fail("expected Ok(articles), got $arts")
            }

            val closedRes = live_rest.UserApi.getUser(
                client,
                1L,
                1L,
                null,
                sep.Rt.withTimeout(300L),
            )
            assertTrue(closedRes is live_rest.GetUserResult.TransportErr)
        } finally {
            server.close()
            serverThread.join(1000)
        }
    }
}
"###;
    write_if_changed(src_test.join("ParsmithKotlinGeneratedTest.kt"), test_kt);

    let test_out = Command::new(gradle_cmd)
        .args(["test", "--console=plain", "-q"])
        .current_dir(&project_dir)
        .output()
        .expect("execute gradle test on generated Kotlin modules");
    assert!(
        test_out.status.success(),
        "gradle test failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
}

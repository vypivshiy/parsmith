mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

use common::*;
use parsmith::codegen::php::{
    normalize_php_namespace, php_class_name, php_ident, php_pascal, php_pcre_pattern,
    php_replacement, validate_php_namespace,
};
use parsmith::{
    CodeWriter, CodegenOptions, DiagnosticCode, ModuleBuilder, PhpEmitter, PhpHtmlLib, Target,
    drive_module, generate_path, generate_source, parse_target_with_libs,
};

const SCHEMA_PHP_EXTENSIONS: &str = r####"
extension Utils {
    normalize {
        sig str str

        php {
            helper normalize_value {
                source #"""
                    public static function normalize_value(string $value): string
                    {
                        return implode(' ', preg_split('/\s+/u', trim($value), -1, PREG_SPLIT_NO_EMPTY) ?: []);
                    }
                    """#
            }
            emit #"{{out}} = Rt::normalize_value({{in}});"#
        }
    }

    truthy {
        sig T bool
        php {
            emit #"{{out}} = ({{in}} !== '');"#
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
fn php_native_lowers_css_to_xpath_and_emits_strict_types_header() {
    let opts = CodegenOptions::new(Target::Php).with_php_html_lib(PhpHtmlLib::Native);
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("item_init.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.starts_with("<?php\ndeclare(strict_types=1);\n"));
    assert!(code.contains("final class Rt"));
    assert!(code.contains("readonly class SingleCardType implements \\JsonSerializable"));
    assert!(code.contains("readonly class PageItemType implements \\JsonSerializable"));
    assert!(code.contains("Rt::xpathFirst("));
    assert!(code.contains("'.//body/h1'"));
    assert!(code.contains("'.//link[@rel = \\'canonical\\']'"));
    assert!(!code.contains("Symfony\\Component\\DomCrawler\\Crawler"));
}

#[test]
fn php_dom_crawler_emits_crawler_import_and_css_methods() {
    let opts = CodegenOptions::new(Target::Php).with_php_html_lib(PhpHtmlLib::DomCrawler);
    let res = generate_source(SCHEMA_01_STRINGS_BASIC, Some("strings.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(code.starts_with("<?php\ndeclare(strict_types=1);\n"));
    assert!(code.contains("use Symfony\\Component\\DomCrawler\\Crawler;"));
    assert!(code.contains("Rt::cssAll($v0, ['.card'])"));
    assert!(code.contains("Rt::cssRemove($v0, '.ad, script')"));
    assert!(code.contains("Rt::cssFirst($v0, ['.title']"));
}

#[test]
fn php_native_and_dom_crawler_support_xpath_operations() {
    for lib in [PhpHtmlLib::Native, PhpHtmlLib::DomCrawler] {
        let opts = CodegenOptions::new(Target::Php).with_php_html_lib(lib);
        let res = generate_source(SCHEMA_XPATH_DOM, Some("xpath.kdl"), &opts);
        assert!(
            !res.has_errors(),
            "unexpected error for {lib}: {:?}",
            res.diagnostics
        );
        let code = res.primary_code().unwrap();
        assert!(code.contains("Rt::xpathFirst($v0, ['//section[@class=\\'single\\']/h1']"));
        assert!(code.contains("Rt::xpathAll($v0, ['//div[@class=\\'nested-card\\']']"));
        assert!(code.contains("Rt::predXpath($i, './/a[@href]')"));
        assert!(code.contains("Rt::xpathRemove($v2, './/a')"));
    }
}

#[test]
fn php_native_rejects_non_translatable_css4_selectors_with_e002_while_dom_crawler_accepts() {
    let schemas = [
        r#"struct A { x { css "p::before"; text } }"#,
        r#"struct B { x { css-all "a:hover"; text } }"#,
        r#"struct C { x { css-remove "div:not(.a > .b)"; text } }"#,
        r#"struct D { x { css-all "div"; filter { css "input:focus" }; text } }"#,
    ];

    let native_opts = CodegenOptions::new(Target::Php).with_php_html_lib(PhpHtmlLib::Native);
    let crawler_opts = CodegenOptions::new(Target::Php).with_php_html_lib(PhpHtmlLib::DomCrawler);

    for src in schemas {
        let res_native = generate_source(src, Some("css4.kdl"), &native_opts);
        assert!(res_native.has_errors(), "expected E002 for {src}");
        assert!(
            res_native.diagnostics.iter().any(|d| {
                d.code == DiagnosticCode::E002
                    && d.message
                        .contains("cannot be lowered to XPath 1.0 for php-native")
                    && d.hint
                        .as_deref()
                        .unwrap_or("")
                        .contains("--html-lib dom-crawler")
            }),
            "unexpected diagnostics: {:?}",
            res_native.diagnostics
        );

        let res_crawler = generate_source(src, Some("css4.kdl"), &crawler_opts);
        assert!(
            !res_crawler.has_errors(),
            "DomCrawler should accept CSS4 selector in {src}: {:?}",
            res_crawler.diagnostics
        );
    }
}

#[test]
fn php_extensions_inline_and_separate_runtime_and_missing_target_e400() {
    let inline_opts = CodegenOptions::new(Target::Php);
    let res_inline = generate_source(SCHEMA_PHP_EXTENSIONS, Some("ext.kdl"), &inline_opts);
    assert!(!res_inline.has_errors(), "{:?}", res_inline.diagnostics);
    let inline_code = res_inline.primary_code().unwrap();
    assert!(inline_code.contains("public static function normalize_value(string $value): string"));
    assert!(inline_code.contains("$v1 = Rt::normalize_value($v0);"));

    let sep_opts = CodegenOptions::new(Target::Php)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res_sep = generate_source(SCHEMA_PHP_EXTENSIONS, Some("ext_sep.kdl"), &sep_opts);
    assert!(!res_sep.has_errors(), "{:?}", res_sep.diagnostics);
    assert_eq!(res_sep.files.len(), 2);
    assert!(
        res_sep.files["ext_sep.php"].contains("require_once __DIR__ . '/parsmith_runtime.php';")
    );
    assert!(!res_sep.files["ext_sep.php"].contains("final class Rt"));
    assert!(res_sep.files["parsmith_runtime.php"].contains("final class Rt"));
    assert!(
        res_sep.files["parsmith_runtime.php"].contains("public static function normalize_value")
    );

    // Missing 'php' block in extension -> E400
    let res_missing = generate_source(
        SCHEMA_PYTHON_EXTENSIONS,
        Some("py_only.kdl"),
        &CodegenOptions::new(Target::Php),
    );
    assert!(res_missing.has_errors());
    assert!(res_missing.diagnostics.iter().any(|d| {
        d.code == DiagnosticCode::E400
            && d.message
                .contains("extension operation 'Utils.truthy' has no 'php' target")
    }));
}

#[test]
fn php_namespace_normalization_and_validation() {
    assert_eq!(normalize_php_namespace(""), Some(String::new()));
    assert_eq!(normalize_php_namespace("main"), Some(String::new()));
    assert_eq!(
        normalize_php_namespace("App\\Parsers"),
        Some("App\\Parsers".to_owned())
    );
    assert_eq!(
        normalize_php_namespace("App.Parsers.V1"),
        Some("App\\Parsers\\V1".to_owned())
    );
    assert!(!validate_php_namespace("123Invalid"));
    assert!(!validate_php_namespace("App\\\\Bad"));
    assert!(!validate_php_namespace("App\\class"));

    let opts = CodegenOptions::new(Target::Php)
        .with_package("App.Parsers")
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("namespaced.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert!(res.files["namespaced.php"].contains("namespace App\\Parsers;"));
    assert!(res.files["parsmith_runtime.php"].contains("namespace App\\Parsers;"));

    let bad_opts = CodegenOptions::new(Target::Php).with_package("123BadNamespace");
    let bad_res = generate_source(SCHEMA_ITEM_INIT_CHECK, Some("bad_ns.kdl"), &bad_opts);
    assert!(bad_res.has_errors());
    assert!(
        bad_res
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );
}

#[test]
fn php_invalid_runtime_name_reports_e002() {
    let opts = CodegenOptions::new(Target::Php)
        .with_separate_runtime(true)
        .with_runtime_name("123-bad-rt");
    let res = generate_source(SCHEMA_23_RAW_STRUCT, Some("raw.kdl"), &opts);
    assert!(res.has_errors());
    assert!(
        res.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002)
    );
}

#[test]
fn php_target_parsing_helpers_and_two_pass_dry_run_zero_allocation() {
    assert_eq!(
        parse_target_with_libs("php").unwrap(),
        (Target::Php, None, None)
    );
    assert_eq!(
        parse_target_with_libs("php-native").unwrap(),
        (Target::Php, None, Some(PhpHtmlLib::Native))
    );
    assert_eq!(
        parse_target_with_libs("php-dom-crawler").unwrap(),
        (Target::Php, None, Some(PhpHtmlLib::DomCrawler))
    );
    assert_eq!(php_ident("clean-html"), "clean_html");
    assert_eq!(php_pascal("user-api"), "UserApi");
    assert_eq!(php_class_name("Match"), "MatchParser");
    assert_eq!(php_pcre_pattern("a~b"), "'~a\\\\~b~u'");
    assert_eq!(php_replacement(r"\1-ok"), "$1-ok");
    assert_eq!(php_replacement(r"\10"), "$10");
    assert_eq!(php_replacement("price: $5"), "price: $5");
    assert_eq!(php_replacement("price: $foo"), "price: \\$foo");

    let module = parsmith::compile_source(SCHEMA_ITEM_INIT_CHECK, Some("item.kdl")).unwrap();
    let mut emitter = PhpEmitter::new(CodegenOptions::new(Target::Php));
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
fn php_value_constants_and_postfix_autocast() {
    let opts = CodegenOptions::new(Target::Php);
    let res = generate_source(SCHEMA_VALUE_AND_AUTOCAST, Some("value_cast.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();

    // StepOp::Value in @init and struct fields
    assert!(code.contains("$v1 = 1;"));
    assert!(code.contains("$v1 = 'habr';"));
    assert!(code.contains("$v1 = 'article';"));
    assert!(code.contains("$v1 = 2;"));
    assert!(code.contains("$v1 = 1.5;"));
    assert!(code.contains("$v1 = false;"));
    assert!(code.contains("$v2 = Rt::upper($v1);"));

    // Postfix auto-cast in struct fields
    assert!(code.contains("Rt::toInt("));
    assert!(code.contains("Rt::toFloat("));

    // JsonFieldDef.value in json projector
    assert!(code.contains("$_f_source = 'habr';"));
    assert!(code.contains("$_f_version = 1;"));
    assert!(code.contains("$_f_ratio = 2.5;"));
    assert!(code.contains("$_f_is_live = true;"));
}

#[test]
fn php_toolchain_lint_and_execution() {
    let tmp = TempTestDir::new("php_toolchain");

    let schemas = [
        ("item_init", SCHEMA_ITEM_INIT_CHECK),
        ("s01_strings", SCHEMA_01_STRINGS_BASIC),
        ("s02_arrays", SCHEMA_02_ARRAYS_AND_CONVERSIONS),
        ("s03_filters", SCHEMA_03_FILTERS_AND_PREDICATES),
        ("s05_flat", SCHEMA_05_FLAT),
        ("s06_dict", SCHEMA_06_DICT),
        ("s07_table", SCHEMA_07_TABLE),
        ("s23_raw", SCHEMA_23_RAW_STRUCT),
        ("s25_fn", SCHEMA_25_FN),
        ("s_xpath", SCHEMA_XPATH_DOM),
        ("s_ext", SCHEMA_PHP_EXTENSIONS),
        ("s_json", SCHEMA_JSON_PROJECTORS),
        ("s_fetch", SCHEMA_HTML_FETCH_REQUESTS),
        ("s_rest", SCHEMA_REST_CLIENT),
        ("s_value_cast", SCHEMA_VALUE_AND_AUTOCAST),
    ];

    let check_php_syntax = |file_path: &std::path::Path| {
        let out = Command::new("php")
            .arg("-l")
            .arg(file_path)
            .output()
            .expect("run php -l");
        assert!(
            out.status.success(),
            "php -l failed for {}:\n{}\n{}",
            file_path.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    };

    // 1. Syntax-check both Native and DomCrawler modes (inline and separate runtime) across all schemas
    for (name, src) in &schemas {
        for (lib_suffix, lib) in [
            ("native", PhpHtmlLib::Native),
            ("crawler", PhpHtmlLib::DomCrawler),
        ] {
            let ns = format!("TestNs\\{}_{}", php_pascal(name), php_pascal(lib_suffix));
            let opts = CodegenOptions::new(Target::Php)
                .with_php_html_lib(lib)
                .with_package(&ns);
            let res = generate_source(src, Some(&format!("{name}.kdl")), &opts);
            assert!(
                !res.has_errors(),
                "PHP generation failed for {name} ({lib}): {:?}",
                res.diagnostics
            );
            let fpath = tmp.write(
                &format!("inline_{name}_{lib_suffix}.php"),
                res.primary_code().unwrap(),
            );
            check_php_syntax(&fpath);
        }
    }

    // 2. Also syntax-check all integration schemas in tests/integration/schemas/*.kdl
    let schemas_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/integration/schemas");
    let mut schema_paths: Vec<PathBuf> = fs::read_dir(&schemas_dir)
        .expect("read tests/integration/schemas")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("kdl"))
        .collect();
    schema_paths.sort();

    for (idx, path) in schema_paths.iter().enumerate() {
        let ns = format!("IntSchema\\S{idx}");
        let res = generate_path(
            path,
            &CodegenOptions::new(Target::Php)
                .with_php_html_lib(PhpHtmlLib::Native)
                .with_package(&ns),
        );
        assert!(
            !res.has_errors(),
            "PHP generation failed for {}: {:?}",
            path.display(),
            res.diagnostics
        );
        let fpath = tmp.write(&format!("int_{idx}.php"), res.primary_code().unwrap());
        check_php_syntax(&fpath);
    }

    // 3. Spin up a local loopback HTTP server for @request and (rest)struct E2E verification
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = match stream.read(&mut buf) {
                Ok(n) if n > 0 => n,
                _ => continue,
            };
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            let (status, ct, body) = if first_line.starts_with("GET /articles/7?") {
                (
                    200,
                    "text/html",
                    "<html><body><h1>Article 7</h1><p class=\"summary\">Summary 7</p></body></html>"
                        .to_string(),
                )
            } else if first_line.starts_with("POST /articles/slug/hello") {
                (
                    200,
                    "text/html",
                    "<html><body><h1>Slug hello</h1><p class=\"summary\">From POST</p></body></html>"
                        .to_string(),
                )
            } else if first_line.starts_with("GET /logs/s1") {
                (
                    200,
                    "application/json",
                    r#"{"payload":{"lines":["l1","l2"]}}"#.to_string(),
                )
            } else if first_line.starts_with("GET /users/1") {
                (
                    200,
                    "application/json",
                    r#"{"id":1,"name":"Alice","nickname":null}"#.to_string(),
                )
            } else if first_line.starts_with("GET /users/2") {
                (
                    200,
                    "application/json",
                    r#"{"error":{"code":1001,"message":"Quota exceeded"}}"#.to_string(),
                )
            } else if first_line.starts_with("GET /users/3") {
                (
                    404,
                    "application/json",
                    r#"{"error":"Not Found","code":404}"#.to_string(),
                )
            } else if first_line.starts_with("GET /users/4") {
                (
                    404,
                    "application/json",
                    r#"{"error":"Not Found","message":"Fallback generic 404"}"#.to_string(),
                )
            } else if first_line.starts_with("GET /users/5") {
                (
                    503,
                    "application/json",
                    r#"{"reason":"maintenance"}"#.to_string(),
                )
            } else if first_line.starts_with("GET /users/6") {
                (200, "text/html", "<html>not json</html>".to_string())
            } else if first_line.starts_with("GET /users/7") {
                (200, "application/json", r#"{"id":7}"#.to_string())
            } else if first_line.starts_with("GET /users/8") {
                (
                    400,
                    "application/json",
                    r#"{"unexpected":"shape"}"#.to_string(),
                )
            } else if first_line.starts_with("GET /ping") {
                (204, "text/plain", String::new())
            } else if first_line.starts_with("GET /me") {
                (
                    200,
                    "application/json",
                    r#"{"data":{"user":{"id":42,"name":"Bob"}}}"#.to_string(),
                )
            } else if first_line.starts_with("GET /articles") {
                (
                    200,
                    "application/json",
                    r#"[{"id":10,"title":"News"}]"#.to_string(),
                )
            } else {
                (
                    404,
                    "application/json",
                    r#"{"message":"not found"}"#.to_string(),
                )
            };
            let status_text = match status {
                200 => "200 OK",
                204 => "204 No Content",
                400 => "400 Bad Request",
                404 => "404 Not Found",
                503 => "503 Service Unavailable",
                _ => "500 Internal Server Error",
            };
            let resp = format!(
                "HTTP/1.1 {status_text}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let live_fetch_kdl = SCHEMA_HTML_FETCH_REQUESTS
        .replace("GET /", &format!("GET http://127.0.0.1:{port}/"))
        .replace("POST /", &format!("POST http://127.0.0.1:{port}/"))
        .replace("    Host: example.com\n", "");
    let res_live_fetch = generate_source(
        &live_fetch_kdl,
        Some("live_fetch.kdl"),
        &CodegenOptions::new(Target::Php).with_package("LiveFetch"),
    );
    assert!(!res_live_fetch.has_errors());
    tmp.write("live_fetch.php", res_live_fetch.primary_code().unwrap());

    let live_rest_kdl = SCHEMA_REST_CLIENT
        .replace("GET /", &format!("GET http://127.0.0.1:{port}/"))
        .replace("POST /", &format!("POST http://127.0.0.1:{port}/"))
        .replace("    Host: api.example.com\n", "");
    let res_live_rest = generate_source(
        &live_rest_kdl,
        Some("live_rest.kdl"),
        &CodegenOptions::new(Target::Php)
            .with_package("LiveRest")
            .with_separate_runtime(true)
            .with_runtime_name("live_rest_rt"),
    );
    assert!(!res_live_rest.has_errors());
    tmp.write("live_rest.php", &res_live_rest.files["live_rest.php"]);
    tmp.write("live_rest_rt.php", &res_live_rest.files["live_rest_rt.php"]);

    // Also generate 00_full.kdl in Native mode
    let full_kdl_path = schemas_dir.join("00_full.kdl");
    let res_full = generate_path(
        &full_kdl_path,
        &CodegenOptions::new(Target::Php).with_package("FullSuite"),
    );
    assert!(!res_full.has_errors());
    tmp.write("full_suite.php", res_full.primary_code().unwrap());

    tmp.write("dsl_coverage.html", HTML_FIXTURE);

    // Write a lightweight Symfony\Component\DomCrawler\Crawler test shim so DomCrawler-generated
    // parsers can also be executed end-to-end without requiring external Composer downloads.
    let crawler_shim_php = r#"<?php
declare(strict_types=1);

namespace Symfony\Component\DomCrawler;

class Crawler
{
    /** @var list<\DOMElement> */
    private array $nodes = [];

    public function __construct(mixed $node = null)
    {
        if ($node instanceof \DOMElement) {
            $this->nodes = [$node];
        } elseif ($node instanceof \DOMNodeList) {
            foreach ($node as $n) {
                if ($n instanceof \DOMElement) {
                    $this->nodes[] = $n;
                }
            }
        } elseif (is_array($node)) {
            foreach ($node as $n) {
                if ($n instanceof \DOMElement) {
                    $this->nodes[] = $n;
                }
            }
        } elseif (is_string($node)) {
            $this->nodes = [\TestNs\ItemInit_Native\Rt::parseHtml($node)];
        }
    }

    public function count(): int
    {
        return count($this->nodes);
    }

    public function first(): self
    {
        return new self(isset($this->nodes[0]) ? [$this->nodes[0]] : []);
    }

    public function getNode(int $position): ?\DOMElement
    {
        return $this->nodes[$position] ?? null;
    }

    /** @return list<mixed> */
    public function each(\Closure $closure): array
    {
        $out = [];
        foreach ($this->nodes as $idx => $el) {
            $out[] = $closure(new self($el), $idx);
        }
        return $out;
    }

    public function filterXPath(string $xpath): self
    {
        $out = [];
        foreach ($this->nodes as $el) {
            foreach (\TestNs\ItemInit_Native\Rt::xpathQueryDom($el, $xpath) as $m) {
                $out[] = $m;
            }
        }
        return new self($out);
    }

    public function filter(string $selector): self
    {
        // Map common CSS selectors used in test schemas to XPath for shim execution
        $map = [
            'h1' => './/h1',
            '.is-active' => './/*[contains(concat(" ", normalize-space(@class), " "), " is-active ")]',
            "link[rel='canonical']" => './/link[@rel="canonical"]',
            'section.single' => './/section[contains(concat(" ", normalize-space(@class), " "), " single ")]',
            '.card' => './/*[contains(concat(" ", normalize-space(@class), " "), " card ")]',
            'body > h1' => './/body/h1',
            '.ad, script' => './/*[contains(concat(" ", normalize-space(@class), " "), " ad ")] | .//script',
            '.title' => './/*[contains(concat(" ", normalize-space(@class), " "), " title ")]',
            'a[href]' => './/a[@href]',
            '.slug' => './/*[contains(concat(" ", normalize-space(@class), " "), " slug ")]',
            '.code' => './/*[contains(concat(" ", normalize-space(@class), " "), " code ")]',
        ];
        $xp = $map[$selector] ?? './/*';
        return $this->filterXPath($xp);
    }
}
"#;
    tmp.write("crawler_shim.php", crawler_shim_php);

    // Write PHP test runner executing all generated modules and asserting exact values
    let runner_php = r#"<?php
declare(strict_types=1);

require_once __DIR__ . '/inline_item_init_native.php';
require_once __DIR__ . '/crawler_shim.php';
require_once __DIR__ . '/inline_item_init_crawler.php';
require_once __DIR__ . '/inline_s01_strings_native.php';
require_once __DIR__ . '/inline_s01_strings_crawler.php';
require_once __DIR__ . '/inline_s02_arrays_native.php';
require_once __DIR__ . '/inline_s03_filters_native.php';
require_once __DIR__ . '/inline_s05_flat_native.php';
require_once __DIR__ . '/inline_s06_dict_native.php';
require_once __DIR__ . '/inline_s07_table_native.php';
require_once __DIR__ . '/inline_s23_raw_native.php';
require_once __DIR__ . '/inline_s25_fn_native.php';
require_once __DIR__ . '/inline_s_xpath_native.php';
require_once __DIR__ . '/inline_s_xpath_crawler.php';
require_once __DIR__ . '/inline_s_ext_native.php';
require_once __DIR__ . '/inline_s_json_native.php';
require_once __DIR__ . '/inline_s_value_cast_native.php';
require_once __DIR__ . '/full_suite.php';
require_once __DIR__ . '/live_fetch.php';
require_once __DIR__ . '/live_rest.php';

function assert_eq(mixed $actual, mixed $expected, string $label = ''): void {
    $aJson = json_encode($actual, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
    $eJson = json_encode($expected, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
    if ($aJson !== $eJson) {
        fwrite(STDERR, "Assertion failed ($label):\n  actual:   $aJson\n  expected: $eJson\n");
        exit(1);
    }
}

$html = (string) file_get_contents(__DIR__ . '/dsl_coverage.html');

// 1. Item + @init + @pre-validate + @check + nested
$page = new \TestNs\ItemInit_Native\PageItem($html);
assert_eq($page->has_cards(), true, 'PageItem::has_cards');
$pageRes = $page->parse();
assert_eq($pageRes->heading, 'Coverage Root', 'PageItem.heading');
assert_eq($pageRes->canonical, 'https://example.test/coverage', 'PageItem.canonical');
assert_eq($pageRes->single->title, 'Single Item Title', 'PageItem.single.title');
assert_eq($pageRes->single->active, true, 'PageItem.single.active');

// 2. 00_full.kdl CoverageRoot
$full = (new \FullSuite\CoverageRoot($html))->parse();
assert_eq($full->title, 'DSL Coverage Fixture', 'CoverageRoot.title');
assert_eq($full->canonical, 'https://example.test/coverage', 'CoverageRoot.canonical');
assert_eq($full->first_author, ['name' => 'Author One', 'slug' => 'author-one'], 'CoverageRoot.first_author');
assert_eq($full->nested_item, ['title' => 'Single Item Title', 'active' => true], 'CoverageRoot.nested_item');
assert_eq(count($full->cards), 2, 'CoverageRoot.cards len');
assert_eq($full->table_data['identifier'], 'ABC-123', 'CoverageRoot.table_data.identifier');
assert_eq($full->body_size, 3, 'CoverageRoot.body_size');
assert_eq($full->code_length_ok, 'foo   bar', 'CoverageRoot.code_length_ok');

// 3. 01_strings_basic.kdl
$s01 = (new \TestNs\S01Strings_Native\StringsBasic($html))->parse();
assert_eq(count($s01), 2, 'StringsBasic len');
assert_eq($s01[0]->title, 'bar Title One', 'StringsBasic[0].title');
assert_eq($s01[0]->link, 'https://example.testguide', 'StringsBasic[0].link');
assert_eq($s01[0]->slug, 'slug-one', 'StringsBasic[0].slug');
assert_eq($s01[0]->normalized_code, 'foo-bar', 'StringsBasic[0].normalized_code');
assert_eq($s01[0]->active_flag, true, 'StringsBasic[0].active_flag');
assert_eq(str_contains($s01[0]->clean_html, 'Advertisement'), false, 'css-remove .ad');
assert_eq(str_contains($s01[0]->clean_html, 'var ignored'), false, 'css-remove script');
assert_eq(str_contains($s01[0]->inner_html, '<h2 class="title">'), true, 'raw inner');

// 4. 02_arrays_and_conversions.kdl
$s02 = (new \TestNs\S02Arrays_Native\ArraysAndConversions($html))->parse();
assert_eq($s02[0]->token_list, ['tag-core-alpha1-x', 'item-beta2-y', 'ref3'], 'Arrays[0].token_list');
assert_eq($s02[0]->first_token, 'tag-core-alpha1-x', 'Arrays[0].first_token');
assert_eq($s02[0]->second_token, 'item-beta2-y', 'Arrays[0].second_token');
assert_eq($s02[0]->last_token, 'ref3', 'Arrays[0].last_token');
assert_eq($s02[0]->token_window, 'tag-core-alpha1-x|item-beta2-y', 'Arrays[0].token_window');
assert_eq($s02[0]->state_code, 1, 'Arrays[0].state_code');
assert_eq($s02[0]->score, 41, 'Arrays[0].score');
assert_eq($s02[0]->ratio, 0.75, 'Arrays[0].ratio');
assert_eq($s02[0]->any_link_count, 2, 'Arrays[0].any_link_count');
assert_eq($s02[0]->flags_joined, 'ALPHA,BETA', 'Arrays[0].flags_joined');

// 5. 03_filters_and_predicates.kdl, 05_flat.kdl, 06_dict.kdl, 07_table.kdl
$s03 = (new \TestNs\S03Filters_Native\FiltersAndPredicates($html))->parse();
assert_eq($s03, [
    ['filtered_links' => [], 'logic_check' => 'prealpha', 'numeric_check' => ''],
    ['filtered_links' => [], 'logic_check' => 'prealpha', 'numeric_check' => 'code-4'],
], 'FiltersAndPredicates');

$s05 = (new \TestNs\S05Flat_Native\FlatCoverage($html))->parse();
assert_eq($s05, [
    '/nested/one.html',
    '/nested/two.html',
    '/guide.html',
    'https://example.test/docs/',
    '/readme/',
    'tag-core-alpha1-x',
    'item-beta2-y',
    'ref3',
], 'FlatCoverage');

$s06 = (new \TestNs\S06Dict_Native\DictRoot($html))->parse();
assert_eq($s06, [
    'named_meta' => [
        'description' => 'Coverage description',
        'keywords' => 'alpha,beta,core',
    ],
    'alias_meta' => [
        'description' => 'Coverage description',
        'og:title' => 'Coverage OG Title',
        'keywords' => 'alpha,beta,core',
    ],
], 'DictRoot');

$s07 = (new \TestNs\S07Table_Native\TableCoverage($html))->parse();
assert_eq($s07, [
    'identifier' => 'ABC-123',
    'code_value' => 'CODE-1',
    'price' => 9.99,
    'tax_or_fee' => 1.25,
    'state' => 'active',
], 'TableCoverage');

// 6. XPath, Raw, Functions, and Extensions
$xpRes = (new \TestNs\SXpath_Native\XpathArticle($html))->parse();
assert_eq($xpRes, [
    'heading' => 'Single Item Title',
    'nested_names' => ['Nested One', 'Nested Two'],
], 'XpathArticle');

$rawScript = '<script>var player = new Playerjs({id:"player",file:"/v/list/abc.txt"});</script>';
assert_eq((new \TestNs\S23Raw_Native\RawPlayerScript($rawScript))->parse(), [
    'playlist_url' => '/v/list/abc.txt',
    'player_id' => 'player',
], 'RawPlayerScript');

$fnHtml = "<html><body><h1>Hello World</h1><a href='/a'>A</a><a href='/b'>B</a></body></html>";
assert_eq(\TestNs\S25Fn_Native\page_title($fnHtml), 'Hello World', 'fn page_title');
assert_eq(\TestNs\S25Fn_Native\all_links($fnHtml), ['/a', '/b'], 'fn all_links');
assert_eq(\TestNs\S25Fn_Native\extract_version('app version=1.2.3 ok'), '1.2.3', 'fn extract_version');

$extP = new \TestNs\SExt_Native\ExtRawParser('  hello   php   world  ');
assert_eq($extP->is_valid(), true, 'ExtRawParser::is_valid');
assert_eq($extP->parse()->normalized, 'hello php world', 'ExtRawParser.normalized');
assert_eq(\TestNs\SExt_Native\normalize_text('  a   b  '), 'a b', 'fn normalize_text');

// 7. JSON Projectors, GJSON paths, @omitempty vs nullable, and RtJson*Error
$jb = (new \TestNs\SJson_Native\JsonBasic($html))->parse();
assert_eq($jb->first_author, ['name' => 'Author One', 'slug' => 'author-one'], 'JsonBasic.first_author');
assert_eq($jb->full_payload[1]->note, null, 'JsonBasic.full_payload[1].note');

$manifestPayload = json_encode([
    'envelope.data' => [
        'os.version' => '24.04',
        'meta' => [
            'os.version' => '6.8.0-generic',
            'path\\dir' => 'C:\\bin',
            '#' => 'pound_val',
        ],
        'items' => ['a', 'b', 'c'],
        'tags' => [
            ['name' => 'action', 'id' => 1],
            ['name' => 'sci-fi', 'id' => 2],
            ['other' => 'skipped'],
        ],
        'groups' => [
            ['meta' => ['id' => 10]],
            ['meta' => ['id' => 20]],
        ],
        'inline_meta' => [
            'env' => 'prod',
        ],
    ],
]);
$mp = (new \TestNs\SJson_Native\ManifestParser((string) $manifestPayload))->parse();
assert_eq($mp->manifest->os_version, '24.04', 'Manifest.os_version');
assert_eq($mp->manifest->nested_version, '6.8.0-generic', 'Manifest.nested_version');
assert_eq($mp->manifest->backslash_key, 'C:\\bin', 'Manifest.backslash_key');
assert_eq($mp->manifest->hash_key, 'pound_val', 'Manifest.hash_key');
assert_eq($mp->manifest->item_count, 3, 'Manifest.item_count');
assert_eq($mp->manifest->opt_count, null, 'Manifest.opt_count');
assert_eq($mp->manifest->tag_names, ['action', 'sci-fi'], 'Manifest.tag_names');
assert_eq($mp->manifest->deep_ids, [10, 20], 'Manifest.deep_ids');

$mpSerialized = json_decode((string) json_encode($mp->manifest), true);
assert_eq(array_key_exists('legacy_id', $mpSerialized), false, '@omitempty legacy_id omitted');
assert_eq(array_key_exists('opt_count', $mpSerialized), true, 'nullable opt_count present');
assert_eq($mpSerialized['opt_count'], null, 'nullable opt_count is null');

// 7b. Value constants and postfix auto-cast
$vcHtml = '<div><span class="views">42</span><span class="score">9.5</span><span class="badge">ok</span><span class="tag">10</span><span class="tag">20</span></div>';
$vcRes = (new \TestNs\SValueCast_Native\FeedCard($vcHtml))->parse();
assert_eq($vcRes->source, 'habr', 'FeedCard.source');
assert_eq($vcRes->kind, 'article', 'FeedCard.kind');
assert_eq($vcRes->version, 2, 'FeedCard.version');
assert_eq($vcRes->ratio, 1.5, 'FeedCard.ratio');
assert_eq($vcRes->is_live, false, 'FeedCard.is_live');
assert_eq($vcRes->seeded, 'HABR', 'FeedCard.seeded');
assert_eq($vcRes->from_init, 'HABR', 'FeedCard.from_init');
assert_eq($vcRes->views, 42, 'FeedCard.views');
assert_eq($vcRes->score, 9.5, 'FeedCard.score');
assert_eq($vcRes->has_badge, true, 'FeedCard.has_badge');
assert_eq($vcRes->tags, [10, 20], 'FeedCard.tags');

$rcRes = \TestNs\SValueCast_Native\ReleaseCardJson::fromValue(['node_id' => 'rel-1']);
assert_eq($rcRes->source, 'habr', 'ReleaseCardJson.source');
assert_eq($rcRes->version, 1, 'ReleaseCardJson.version');
assert_eq($rcRes->ratio, 2.5, 'ReleaseCardJson.ratio');
assert_eq($rcRes->is_live, true, 'ReleaseCardJson.is_live');
assert_eq($rcRes->id, 'rel-1', 'ReleaseCardJson.id');

// 8. Live HTTP @request fetch and (rest)struct endpoints
$tx = \LiveFetch\Rt::viaStream(5.0);
$art = \LiveFetch\ArticlePage::fetch(
    $tx,
    7,
    's1',
    null,
    null,
    null,
    ['a', 'b'],
    [1, 2],
    ['x', 'y'],
    [10, 20],
    ['r1', 'r2'],
);
assert_eq($art->parse(), ['heading' => 'Article 7', 'summary' => 'Summary 7'], 'ArticlePage::fetch');

$slugArt = \LiveFetch\ArticlePage::fetch_by_slug($tx, 'hello', 'tok');
assert_eq($slugArt->parse(), ['heading' => 'Slug hello', 'summary' => 'From POST'], 'ArticlePage::fetch_by_slug');

$logRes = \LiveFetch\ChunkedLog::fetch($tx, 's1');
assert_eq($logRes->parse(), ['lines' => ['l1', 'l2']], 'ChunkedLog::fetch');

$rtx = \LiveRest\Rt::viaStream(5.0);
$u1 = \LiveRest\UserApi::get_user($rtx, 1);
assert_eq($u1 instanceof \LiveRest\Ok, true, 'UserApi::get_user(1) Ok');
assert_eq($u1->value, ['id' => 1, 'name' => 'Alice', 'nickname' => null], 'UserApi::get_user(1) value');

$u2 = \LiveRest\UserApi::get_user($rtx, 2);
assert_eq($u2 instanceof \LiveRest\UserApiErr200Code, true, 'UserApi::get_user(2) Err200Code');
assert_eq($u2->value->code, 1001, 'UserApi::get_user(2) code');

$u3 = \LiveRest\UserApi::get_user($rtx, 3);
assert_eq($u3 instanceof \LiveRest\UserApiErr404NotFound, true, 'UserApi::get_user(3) Err404NotFound');

$u4 = \LiveRest\UserApi::get_user($rtx, 4);
assert_eq($u4 instanceof \LiveRest\UserApiErr404, true, 'UserApi::get_user(4) Err404 fallthrough');
assert_eq($u4->value->message, 'Fallback generic 404', 'UserApi::get_user(4) message');

$u5 = \LiveRest\UserApi::get_user($rtx, 5);
assert_eq($u5 instanceof \LiveRest\UnknownErr, true, 'UserApi::get_user(5) UnknownErr');
assert_eq($u5->status, 503, 'UserApi::get_user(5) status 503');

$u6 = \LiveRest\UserApi::get_user($rtx, 6);
assert_eq($u6 instanceof \LiveRest\DecodeErr, true, 'UserApi::get_user(6) DecodeErr HTML');

$u7 = \LiveRest\UserApi::get_user($rtx, 7);
assert_eq($u7 instanceof \LiveRest\DecodeErr, true, 'UserApi::get_user(7) DecodeErr missing field');

$u8 = \LiveRest\UserApi::get_user($rtx, 8);
assert_eq($u8 instanceof \LiveRest\DecodeErr, true, 'UserApi::get_user(8) DecodeErr 400 mismatch');

$ping = \LiveRest\UserApi::ping($rtx);
assert_eq($ping instanceof \LiveRest\Ok && $ping->value === null, true, 'UserApi::ping void Ok');

$me = \LiveRest\UserApi::get_profile_enveloped($rtx);
assert_eq($me instanceof \LiveRest\Ok && $me->value->id === 42, true, 'UserApi::get_profile_enveloped');

$arts = \LiveRest\UserApi::list_articles($rtx);
assert_eq($arts instanceof \LiveRest\Ok && count($arts->value) === 1 && $arts->value[0]->article_id === 10, true, 'UserApi::list_articles');

// 9. Verify DomCrawler-generated parsers produce identical output to Native parsers
$pageCrawler = new \TestNs\ItemInit_Crawler\PageItem($html);
assert_eq($pageCrawler->has_cards(), true, 'DomCrawler PageItem::has_cards');
assert_eq($pageCrawler->parse(), $pageRes, 'DomCrawler PageItem parity');

$s01Crawler = (new \TestNs\S01Strings_Crawler\StringsBasic($html))->parse();
assert_eq($s01Crawler, $s01, 'DomCrawler StringsBasic parity');

$xpCrawler = (new \TestNs\SXpath_Crawler\XpathArticle($html))->parse();
assert_eq($xpCrawler, $xpRes, 'DomCrawler XpathArticle parity');

// 10. Verify Rt::viaCurl() transport against local loopback server
$curlTx = \LiveRest\Rt::viaCurl(5.0);
$u1Curl = \LiveRest\UserApi::get_user($curlTx, 1);
assert_eq($u1Curl instanceof \LiveRest\Ok && $u1Curl->value->name === 'Alice', true, 'viaCurl UserApi::get_user(1)');

echo "PHP_TOOLCHAIN_E2E_OK";
"#;
    let runner_path = tmp.write("runner.php", runner_php);
    check_php_syntax(&runner_path);

    let run_out = Command::new("php")
        .arg(&runner_path)
        .output()
        .expect("execute runner.php");
    assert!(
        run_out.status.success(),
        "runner.php failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_out.stdout),
        String::from_utf8_lossy(&run_out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run_out.stdout),
        "PHP_TOOLCHAIN_E2E_OK"
    );
}

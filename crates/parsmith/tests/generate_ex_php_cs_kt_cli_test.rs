use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use parsmith::{CheckResult, DiagnosticCode};

static GEN_MULTI_CLI_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct CliTempDir {
    path: PathBuf,
}

impl CliTempDir {
    fn new(prefix: &str) -> Self {
        let id = GEN_MULTI_CLI_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "parsmith_gen_multi_cli_{}_{}_{}",
            prefix,
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        Self { path: dir }
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let full = self.path.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&full, content).expect("write test file");
        full
    }
}

impl Drop for CliTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn parsmith_bin() -> &'static str {
    env!("CARGO_BIN_EXE_parsmith")
}

#[test]
fn cli_generate_elixir_and_ex_stdout_file_dir_package_and_separate_runtime() {
    let tmp = CliTempDir::new("elixir_ok");
    let schema = tmp.write(
        "article.kdl",
        "struct Article {\n    title { css \"h1\"; text; trim }\n}\n",
    );

    // 1. Write to stdout with `elixir`
    let out_stdout = Command::new(parsmith_bin())
        .args(["generate", "elixir", &schema.to_string_lossy()])
        .output()
        .expect("run parsmith generate elixir to stdout");
    assert!(
        out_stdout.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_stdout.stderr)
    );
    let stdout_str = String::from_utf8_lossy(&out_stdout.stdout);
    assert!(stdout_str.contains("defmodule Rt do"));
    assert!(stdout_str.contains("defmodule ArticleType do"));
    assert!(stdout_str.contains("defmodule Article do"));

    // 2. Write to explicit `-o file.ex` using alias `ex` and `--namespace MyApp.Parsers`
    let out_file = tmp.path.join("single").join("custom_article.ex");
    let out_file_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "ex",
            &schema.to_string_lossy(),
            "-o",
            &out_file.to_string_lossy(),
            "--namespace",
            "MyApp.Parsers",
            "--prologue",
            "# EX_PROLOGUE",
            "--epilogue",
            "# EX_EPILOGUE",
        ])
        .output()
        .expect("run parsmith generate ex -o file.ex");
    assert!(
        out_file_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_file_cmd.stderr)
    );
    assert!(out_file.is_file());
    let file_src = fs::read_to_string(&out_file).unwrap();
    assert!(file_src.contains("defmodule MyApp.Parsers.Article do"));
    assert!(file_src.contains("# EX_PROLOGUE"));
    assert!(file_src.contains("# EX_EPILOGUE"));

    // 3. Write to `-o dir/` with `-R --runtime-name rt_ex`
    let out_dir = tmp.path.join("ex_dir");
    let out_dir_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "elixir",
            &schema.to_string_lossy(),
            "-o",
            &out_dir.to_string_lossy(),
            "--package",
            "MyApp.Parsers",
            "-R",
            "--runtime-name",
            "rt_ex",
        ])
        .output()
        .expect("run parsmith generate elixir -o dir/ -R");
    assert!(
        out_dir_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_dir_cmd.stderr)
    );
    let parser_ex = out_dir.join("article.ex");
    let runtime_ex = out_dir.join("rt_ex.ex");
    assert!(parser_ex.is_file());
    assert!(runtime_ex.is_file());
    let parser_src = fs::read_to_string(&parser_ex).unwrap();
    let runtime_src = fs::read_to_string(&runtime_ex).unwrap();
    assert!(parser_src.contains("alias MyApp.Parsers.RtEx, as: Rt"));
    assert!(!parser_src.contains("defmodule MyApp.Parsers.RtEx do"));
    assert!(runtime_src.contains("defmodule MyApp.Parsers.RtEx do"));

    // 4. XPath operation fails with E203 on Elixir
    let xpath_schema = tmp.write(
        "xpath.kdl",
        "struct XPathCard {\n    title { xpath \"//h1\"; text }\n}\n",
    );
    let bad_xpath = Command::new(parsmith_bin())
        .args([
            "generate",
            "ex",
            &xpath_schema.to_string_lossy(),
            "-f",
            "json",
        ])
        .output()
        .expect("run parsmith generate ex on xpath schema");
    assert_eq!(bad_xpath.status.code(), Some(1));
    let parsed_xpath: CheckResult =
        serde_json::from_str(&String::from_utf8_lossy(&bad_xpath.stderr)).unwrap();
    assert!(
        parsed_xpath
            .errors()
            .any(|d| d.code == DiagnosticCode::E203)
    );
}

#[test]
fn cli_generate_php_native_and_dom_crawler_stdout_file_dir_and_separate_runtime() {
    let tmp = CliTempDir::new("php_ok");
    let schema = tmp.write(
        "article.kdl",
        "struct Article {\n    title { css \"h1.main\"; text; trim }\n    subtitle { xpath \"//h2\"; text; fallback \"\" }\n}\n",
    );

    // 1. Default `--html-lib native` lowers CSS to XPath and uses \DOMXPath
    let out_native = Command::new(parsmith_bin())
        .args(["generate", "php", &schema.to_string_lossy()])
        .output()
        .expect("run parsmith generate php to stdout");
    assert!(
        out_native.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_native.stderr)
    );
    let native_str = String::from_utf8_lossy(&out_native.stdout);
    assert!(native_str.contains("declare(strict_types=1);"));
    assert!(native_str.contains("final class Rt"));
    assert!(native_str.contains("readonly class ArticleType"));
    assert!(native_str.contains("class Article"));
    assert!(native_str.contains("\\DOMDocument"));

    // 2. `-L dom-crawler` with `-o file.php` and `--namespace App\\Parsers`
    let out_file = tmp.path.join("single").join("custom_article.php");
    let out_crawler = Command::new(parsmith_bin())
        .args([
            "generate",
            "php",
            &schema.to_string_lossy(),
            "-L",
            "dom-crawler",
            "-o",
            &out_file.to_string_lossy(),
            "--namespace",
            "App\\Parsers",
            "--prologue",
            "// PHP_PROLOGUE",
            "--epilogue",
            "// PHP_EPILOGUE",
        ])
        .output()
        .expect("run parsmith generate php -L dom-crawler -o file.php");
    assert!(
        out_crawler.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_crawler.stderr)
    );
    assert!(out_file.is_file());
    let file_src = fs::read_to_string(&out_file).unwrap();
    assert!(file_src.contains("namespace App\\Parsers;"));
    assert!(file_src.contains("Symfony\\Component\\DomCrawler\\Crawler"));
    assert!(file_src.contains("// PHP_PROLOGUE"));
    assert!(file_src.contains("// PHP_EPILOGUE"));

    // 3. Separate runtime `-R --runtime-name rt_php`
    let out_dir = tmp.path.join("php_dir");
    let out_dir_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "php",
            &schema.to_string_lossy(),
            "-o",
            &out_dir.to_string_lossy(),
            "-R",
            "--runtime-name",
            "rt_php",
        ])
        .output()
        .expect("run parsmith generate php -o dir/ -R");
    assert!(
        out_dir_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_dir_cmd.stderr)
    );
    let parser_php = out_dir.join("article.php");
    let runtime_php = out_dir.join("rt_php.php");
    assert!(parser_php.is_file());
    assert!(runtime_php.is_file());
    let parser_src = fs::read_to_string(&parser_php).unwrap();
    let runtime_src = fs::read_to_string(&runtime_php).unwrap();
    assert!(parser_src.contains("require_once __DIR__ . '/rt_php.php';"));
    assert!(!parser_src.contains("final class Rt"));
    assert!(runtime_src.contains("final class Rt"));
}

#[test]
fn cli_generate_csharp_and_kotlin_stdout_file_dir_and_separate_runtime() {
    let tmp = CliTempDir::new("cs_kt_ok");
    let schema = tmp.write(
        "article.kdl",
        "struct Article {\n    title { css \"h1\"; text; trim }\n    subtitle { xpath \"//h2\"; text; fallback \"\" }\n}\n",
    );

    // 1. C# (`csharp` and alias `cs`)
    let out_cs = tmp.path.join("single").join("custom_article.cs");
    let cs_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "cs",
            &schema.to_string_lossy(),
            "-o",
            &out_cs.to_string_lossy(),
            "--namespace",
            "MyApp.Parsers",
            "--prologue",
            "// CS_PROLOGUE",
            "--epilogue",
            "// CS_EPILOGUE",
        ])
        .output()
        .expect("run parsmith generate cs -o file.cs");
    assert!(
        cs_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cs_cmd.stderr)
    );
    assert!(out_cs.is_file());
    let cs_src = fs::read_to_string(&out_cs).unwrap();
    assert!(cs_src.contains("namespace MyApp.Parsers;"));
    assert!(cs_src.contains("using AngleSharp.XPath;"));
    assert!(cs_src.contains("public static partial class Rt"));
    assert!(cs_src.contains("public sealed record ArticleType"));
    assert!(cs_src.contains("public sealed class Article"));
    assert!(cs_src.contains("// CS_PROLOGUE"));
    assert!(cs_src.contains("// CS_EPILOGUE"));

    let cs_dir = tmp.path.join("cs_dir");
    let cs_dir_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "csharp",
            &schema.to_string_lossy(),
            "-o",
            &cs_dir.to_string_lossy(),
            "-R",
            "--runtime-name",
            "RtModule",
        ])
        .output()
        .expect("run parsmith generate csharp -o dir/ -R");
    assert!(cs_dir_cmd.status.success());
    assert!(cs_dir.join("article.cs").is_file());
    assert!(cs_dir.join("RtModule.cs").is_file());

    // 2. Kotlin (`kotlin` and alias `kt`)
    let out_kt = tmp.path.join("single").join("custom_article.kt");
    let kt_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "kt",
            &schema.to_string_lossy(),
            "-o",
            &out_kt.to_string_lossy(),
            "--package",
            "com.example.parsers",
            "--prologue",
            "// KT_PROLOGUE",
            "--epilogue",
            "// KT_EPILOGUE",
        ])
        .output()
        .expect("run parsmith generate kt -o file.kt");
    assert!(
        kt_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&kt_cmd.stderr)
    );
    assert!(out_kt.is_file());
    let kt_src = fs::read_to_string(&out_kt).unwrap();
    assert!(kt_src.contains("package com.example.parsers"));
    assert!(kt_src.contains("object Rt"));
    assert!(kt_src.contains("data class ArticleType"));
    assert!(kt_src.contains("class Article("));
    assert!(kt_src.contains("// KT_PROLOGUE"));
    assert!(kt_src.contains("// KT_EPILOGUE"));

    let kt_dir = tmp.path.join("kt_dir");
    let kt_dir_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "kotlin",
            &schema.to_string_lossy(),
            "-o",
            &kt_dir.to_string_lossy(),
            "-R",
            "--runtime-name",
            "rt_kt",
        ])
        .output()
        .expect("run parsmith generate kotlin -o dir/ -R");
    assert!(kt_dir_cmd.status.success());
    assert!(kt_dir.join("article.kt").is_file());
    assert!(kt_dir.join("rt_kt.kt").is_file());
}

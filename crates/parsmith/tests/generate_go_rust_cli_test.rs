use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use parsmith::{CheckResult, DiagnosticCode};

static GEN_GO_RS_CLI_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct CliTempDir {
    path: PathBuf,
}

impl CliTempDir {
    fn new(prefix: &str) -> Self {
        let id = GEN_GO_RS_CLI_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "parsmith_gen_go_rs_cli_{}_{}_{}",
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
fn cli_generate_go_and_golang_stdout_file_dir_package_and_separate_runtime() {
    let tmp = CliTempDir::new("go_ok");
    let schema = tmp.write(
        "article.kdl",
        "struct Article {\n    title { css \"h1\"; text; trim }\n}\n",
    );

    // 1. Write to stdout with `go` (default --package main)
    let out_stdout = Command::new(parsmith_bin())
        .args(["generate", "go", &schema.to_string_lossy()])
        .output()
        .expect("run parsmith generate go to stdout");
    assert!(
        out_stdout.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_stdout.stderr)
    );
    let stdout_str = String::from_utf8_lossy(&out_stdout.stdout);
    assert!(stdout_str.contains("package main"));
    assert!(stdout_str.contains("type ArticleType struct"));
    assert!(stdout_str.contains("type Article struct"));

    // 2. Write to explicit `-o file.go` using alias `golang` and `--package custom_pkg`
    let out_file = tmp.path.join("single").join("custom_article.go");
    let out_file_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "golang",
            &schema.to_string_lossy(),
            "-o",
            &out_file.to_string_lossy(),
            "--package",
            "custom_pkg",
            "--prologue",
            "// GO_PROLOGUE",
            "--epilogue",
            "// GO_EPILOGUE",
        ])
        .output()
        .expect("run parsmith generate golang -o file.go");
    assert!(
        out_file_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_file_cmd.stderr)
    );
    assert!(out_file.is_file());
    let file_src = fs::read_to_string(&out_file).unwrap();
    assert!(file_src.contains("package custom_pkg"));
    assert!(file_src.contains("// GO_PROLOGUE"));
    assert!(file_src.contains("// GO_EPILOGUE"));

    // 3. Write to `-o dir/` with `-R --runtime-name rt_go`
    let out_dir = tmp.path.join("go_dir");
    let out_dir_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "go",
            &schema.to_string_lossy(),
            "-o",
            &out_dir.to_string_lossy(),
            "--package",
            "parsers",
            "-R",
            "--runtime-name",
            "rt_go",
        ])
        .output()
        .expect("run parsmith generate go -o dir/ -R");
    assert!(
        out_dir_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_dir_cmd.stderr)
    );
    let parser_go = out_dir.join("article.go");
    let runtime_go = out_dir.join("rt_go.go");
    assert!(parser_go.is_file());
    assert!(runtime_go.is_file());
    let parser_src = fs::read_to_string(&parser_go).unwrap();
    let runtime_src = fs::read_to_string(&runtime_go).unwrap();
    assert!(parser_src.contains("package parsers"));
    assert!(runtime_src.contains("package parsers"));
    assert!(!parser_src.contains("func rtCssSelect("));
    assert!(runtime_src.contains("func rtCssSelect("));
}

#[test]
fn cli_generate_rust_and_rs_stdout_file_dir_and_separate_runtime() {
    let tmp = CliTempDir::new("rust_ok");
    let schema = tmp.write(
        "article.kdl",
        "struct Article {\n    title { css \"h1\"; text; trim }\n}\n",
    );

    // 1. Write to stdout with `rust`
    let out_stdout = Command::new(parsmith_bin())
        .args(["generate", "rust", &schema.to_string_lossy()])
        .output()
        .expect("run parsmith generate rust to stdout");
    assert!(
        out_stdout.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_stdout.stderr)
    );
    let stdout_str = String::from_utf8_lossy(&out_stdout.stdout);
    assert!(stdout_str.contains("pub mod rt {"));
    assert!(stdout_str.contains("pub struct ArticleType {"));
    assert!(stdout_str.contains("pub struct ArticleParser {"));

    // 2. Write to explicit `-o file.rs` using alias `rs`
    let out_file = tmp.path.join("single").join("custom_article.rs");
    let out_file_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "rs",
            &schema.to_string_lossy(),
            "-o",
            &out_file.to_string_lossy(),
            "--prologue",
            "// RUST_PROLOGUE",
            "--epilogue",
            "// RUST_EPILOGUE",
        ])
        .output()
        .expect("run parsmith generate rs -o file.rs");
    assert!(
        out_file_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_file_cmd.stderr)
    );
    assert!(out_file.is_file());
    let file_src = fs::read_to_string(&out_file).unwrap();
    assert!(file_src.contains("pub mod rt {"));
    assert!(file_src.contains("// RUST_PROLOGUE"));
    assert!(file_src.contains("// RUST_EPILOGUE"));

    // 3. Write to `-o dir/` with `-R --runtime-name rt_rs`
    let out_dir = tmp.path.join("rs_dir");
    let out_dir_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "rust",
            &schema.to_string_lossy(),
            "-o",
            &out_dir.to_string_lossy(),
            "-R",
            "--runtime-name",
            "rt_rs",
        ])
        .output()
        .expect("run parsmith generate rust -o dir/ -R");
    assert!(
        out_dir_cmd.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_dir_cmd.stderr)
    );
    let parser_rs = out_dir.join("article.rs");
    let runtime_rs = out_dir.join("rt_rs.rs");
    assert!(parser_rs.is_file());
    assert!(runtime_rs.is_file());
    let parser_src = fs::read_to_string(&parser_rs).unwrap();
    let runtime_src = fs::read_to_string(&runtime_rs).unwrap();
    assert!(parser_src.contains("use super::rt_rs as rt;"));
    assert!(!parser_src.contains("pub mod rt {"));
    assert!(runtime_src.contains("pub struct RtError"));
}

#[test]
fn cli_generate_go_and_rust_diagnostics_e002_and_e203() {
    let tmp = CliTempDir::new("go_rs_err");
    let valid_schema = tmp.write(
        "card.kdl",
        "struct Card {\n    title { css \"h1\"; text }\n}\n",
    );
    let xpath_schema = tmp.write(
        "xpath.kdl",
        "struct XPathCard {\n    title { xpath \"//h1\"; text }\n}\n",
    );

    for (target, alias) in [("go", "golang"), ("rust", "rs")] {
        let e002_flags: &[&str] = if target == "go" {
            &["--package", "123bad"]
        } else {
            &["-R", "--runtime-name", "123bad"]
        };

        // 1. Invalid package/runtime identifier fails with E002
        let mut cmd = Command::new(parsmith_bin());
        cmd.args(["generate", target, &valid_schema.to_string_lossy()])
            .args(e002_flags)
            .args(["-f", "json"]);
        let bad_ident = cmd.output().expect("run parsmith generate with E002 flags");
        assert_eq!(bad_ident.status.code(), Some(1));
        let parsed_ident: CheckResult =
            serde_json::from_str(&String::from_utf8_lossy(&bad_ident.stderr)).unwrap();
        assert!(
            parsed_ident
                .errors()
                .any(|d| d.code == DiagnosticCode::E002)
        );

        // 2. XPath operation fails with E203
        let bad_xpath = Command::new(parsmith_bin())
            .args([
                "generate",
                alias,
                &xpath_schema.to_string_lossy(),
                "-f",
                "text",
            ])
            .output()
            .expect("run parsmith generate on xpath schema");
        assert_eq!(bad_xpath.status.code(), Some(1));
        let stderr_str = String::from_utf8_lossy(&bad_xpath.stderr);
        assert!(stderr_str.contains("E203"));
    }
}

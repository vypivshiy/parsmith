use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use parsmith::{CheckResult, DiagnosticCode};

static CLI_TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct CliTempDir {
    path: PathBuf,
}

impl CliTempDir {
    fn new(prefix: &str) -> Self {
        let id = CLI_TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "parsmith_cli_test_{}_{}_{}",
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
fn cli_check_valid_schema_exits_zero() {
    let tmp = CliTempDir::new("valid");
    tmp.write(
        "shared.kdl",
        "struct Shared { title { css \"h1\"; text } }\n",
    );
    let root = tmp.write(
        "root.kdl",
        "import \"./shared.kdl\" { (struct)Shared }\nstruct Root { item { nested Shared } }\n",
    );

    let out = Command::new(parsmith_bin())
        .args(["check", &root.to_string_lossy(), "--target", "python"])
        .output()
        .expect("run parsmith check");

    assert!(
        out.status.success(),
        "expected exit 0, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty());
}

#[test]
fn cli_check_invalid_schema_text_and_json_exits_one() {
    let tmp = CliTempDir::new("invalid");
    let broken = tmp.write(
        "broken.kdl",
        "import \"./missing.kdl\" { (struct)Missing }\n",
    );

    // 1. Text format
    let out_text = Command::new(parsmith_bin())
        .args(["check", &broken.to_string_lossy(), "--format", "text"])
        .output()
        .expect("run parsmith check --format text");

    assert_eq!(out_text.status.code(), Some(1));
    let stdout_text = String::from_utf8_lossy(&out_text.stdout);
    assert!(stdout_text.contains("error[E003]: import: file not found"));
    assert!(stdout_text.contains("import \"./missing.kdl\""));
    assert!(stdout_text.contains("= hint:"));

    // 2. JSON format
    let out_json = Command::new(parsmith_bin())
        .args(["check", &broken.to_string_lossy(), "--format", "json"])
        .output()
        .expect("run parsmith check --format json");

    assert_eq!(out_json.status.code(), Some(1));
    let stdout_json = String::from_utf8_lossy(&out_json.stdout);
    let parsed: CheckResult =
        serde_json::from_str(&stdout_json).expect("valid JSON CheckResult output");
    assert!(parsed.has_errors());
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(parsed.diagnostics[0].code, DiagnosticCode::E003);
}

#[test]
fn cli_check_target_filtering_and_default_four_ecosystems() {
    let tmp = CliTempDir::new("target_filter");
    // Field 'self' is valid in python (`_parse_self`), but invalid in rust (`self` cannot be r#self)
    let schema = tmp.write(
        "self_field.kdl",
        "struct Data {\n    self { css \"a\"; text }\n}\n",
    );

    // 1. Default (all 4 ecosystems) -> fails with E403 on rust
    let out_default = Command::new(parsmith_bin())
        .args(["check", &schema.to_string_lossy(), "--format", "json"])
        .output()
        .expect("run parsmith check default targets");
    assert_eq!(out_default.status.code(), Some(1));
    let res_default: CheckResult =
        serde_json::from_str(&String::from_utf8_lossy(&out_default.stdout)).unwrap();
    assert!(res_default.errors().any(|d| d.code == DiagnosticCode::E403));

    // 2. Filtered to `--target python` -> exits 0
    let out_py = Command::new(parsmith_bin())
        .args(["check", &schema.to_string_lossy(), "--target", "python"])
        .output()
        .expect("run parsmith check --target python");
    assert!(out_py.status.success());
}

#[test]
fn cli_check_directory_and_multi_file_paths() {
    let tmp = CliTempDir::new("dir_multi");
    let schemas_dir = tmp.path.join("schemas");
    fs::create_dir_all(&schemas_dir).expect("create schemas dir");

    tmp.write("schemas/_common.kdl", "define CLEAN_TEXT { text; trim }\n");
    let file_a = tmp.write(
        "schemas/a.kdl",
        "import \"./_common.kdl\" { (define)CLEAN_TEXT }\nstruct A {\n    title { css \"h1\"; CLEAN_TEXT }\n}\n",
    );
    let file_b = tmp.write(
        "schemas/b.kdl",
        "struct B {\n    name { css \"span\"; text }\n}\n",
    );

    // 1. Check entire directory -> exits 0
    let out_dir = Command::new(parsmith_bin())
        .args(["check", &schemas_dir.to_string_lossy()])
        .output()
        .expect("run parsmith check <dir>");
    assert!(
        out_dir.status.success(),
        "expected exit 0 for directory check, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out_dir.stdout),
        String::from_utf8_lossy(&out_dir.stderr)
    );

    // 2. Check multiple explicit files -> exits 0
    let out_multi = Command::new(parsmith_bin())
        .args([
            "check",
            &file_a.to_string_lossy(),
            &file_b.to_string_lossy(),
        ])
        .output()
        .expect("run parsmith check <file1> <file2>");
    assert!(out_multi.status.success());

    // 3. Invalid file in multi-file list -> exits 1
    let file_bad = tmp.write("schemas/bad.kdl", "struct Bad {\n    x { trim }\n}\n");
    let out_invalid = Command::new(parsmith_bin())
        .args([
            "check",
            &file_a.to_string_lossy(),
            &file_bad.to_string_lossy(),
            "--format",
            "json",
        ])
        .output()
        .expect("run parsmith check with invalid file");
    assert_eq!(out_invalid.status.code(), Some(1));
    let parsed: CheckResult =
        serde_json::from_str(&String::from_utf8_lossy(&out_invalid.stdout)).unwrap();
    assert!(parsed.has_errors());
}

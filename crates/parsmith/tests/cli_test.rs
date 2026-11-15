use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn parsmith_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_parsmith"))
}

#[test]
fn cli_version_text_and_json() {
    let out_text = parsmith_bin()
        .arg("version")
        .output()
        .expect("should execute parsmith version");
    assert!(out_text.status.success());
    let stdout_text = String::from_utf8_lossy(&out_text.stdout);
    assert_eq!(
        stdout_text.trim(),
        format!("parsmith {}", parsmith::version())
    );

    let out_json = parsmith_bin()
        .args(["version", "--format", "json"])
        .output()
        .expect("should execute parsmith version --format json");
    assert!(out_json.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&out_json.stdout).expect("valid version JSON");
    assert_eq!(parsed["name"], "parsmith");
    assert_eq!(parsed["version"], parsmith::version());
}

#[test]
fn cli_json_to_kdl_stdin_to_stdout() {
    let mut child = parsmith_bin()
        .args(["json-to-kdl", "-", "--name", "Catalog"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("should spawn parsmith json-to-kdl");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"[{"@id": 1, "display-name": "Widget"}]"#)
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let kdl = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        kdl,
        "\
(array)json CatalogItem {
    id int from=\"@id\"
    display_name str from=\"display-name\"
}
"
    );
    parsmith::kdl::parse(&kdl).expect("CLI output must parse as KDL 2.0");
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{}_{}", prefix, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_json_to_kdl_file_to_file_and_error_handling() {
    let temp = TempDir::new("parsmith_cli_test");
    let temp_dir = &temp.0;

    let input_path = temp_dir.join("input.json");
    let output_path = temp_dir.join("nested").join("schema.kdl");

    fs::write(
        &input_path,
        r#"{"id": 1, "profile": {"user-name": "Alice"}}"#,
    )
    .unwrap();

    let res = parsmith_bin()
        .args([
            "json-to-kdl",
            input_path.to_str().unwrap(),
            "--name",
            "UserDetail",
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(res.status.success());
    let written = fs::read_to_string(&output_path).unwrap();
    assert_eq!(
        written,
        "\
json UserDetail {
    id int
    profile {
        user_name str from=\"user-name\"
    }
}
"
    );
    parsmith::kdl::parse(&written).expect("written KDL file must parse");

    // Invalid JSON input returns non-zero exit code and ERROR on stderr
    let bad_path = temp_dir.join("bad.json");
    fs::write(&bad_path, "{broken json").unwrap();
    let bad_res = parsmith_bin()
        .args(["json-to-kdl", bad_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!bad_res.status.success());
    let stderr = String::from_utf8_lossy(&bad_res.stderr);
    assert!(stderr.contains("ERROR:"));
}

use std::path::PathBuf;
use std::process::Command;

#[test]
fn node_sdk_test_suite_passes() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");

    // Ensure the cdylib (`parsmith_js.dll` / `libparsmith_js.so` / `libparsmith_js.dylib`) is up to date.
    let build_status = Command::new("cargo")
        .args(["build", "-p", "parsmith-js"])
        .current_dir(repo_root)
        .status()
        .expect("failed to invoke cargo build -p parsmith-js");
    assert!(build_status.success(), "cargo build -p parsmith-js failed");

    let test_file = repo_root.join("js").join("tests").join("parsmith.test.mjs");
    let output = Command::new("node")
        .arg("--test")
        .arg(&test_file)
        .current_dir(repo_root)
        .output()
        .expect("failed to invoke node --test");

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "node --test js/tests/parsmith.test.mjs failed:\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
        );
    }

    // Run TypeScript strict typecheck if `tsc` is available on PATH.
    let tsc_cmd = if cfg!(windows) { "tsc.cmd" } else { "tsc" };
    if let Ok(tsc_out) = Command::new(tsc_cmd)
        .args(["--noEmit", "--strict", "-p", "js/tsconfig.json"])
        .current_dir(repo_root)
        .output()
    {
        if !tsc_out.status.success() {
            let stdout = String::from_utf8_lossy(&tsc_out.stdout);
            let stderr = String::from_utf8_lossy(&tsc_out.stderr);
            panic!("tsc --noEmit --strict failed:\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}");
        }
    }
}

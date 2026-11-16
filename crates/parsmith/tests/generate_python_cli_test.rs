use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static GEN_CLI_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct CliTempDir {
    path: PathBuf,
}

impl CliTempDir {
    fn new(prefix: &str) -> Self {
        let id = GEN_CLI_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "parsmith_gen_py_cli_{}_{}_{}",
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
fn cli_generate_python_stdout_and_directory_output() {
    let tmp = CliTempDir::new("stdout_dir");
    let schema = tmp.write(
        "article.kdl",
        "struct Article {\n    title { css \"h1\"; text; trim }\n}\n",
    );

    // 1. Write to stdout
    let out_stdout = Command::new(parsmith_bin())
        .args(["generate", "python", &schema.to_string_lossy()])
        .output()
        .expect("run parsmith generate python to stdout");
    assert!(out_stdout.status.success());
    let stdout_str = String::from_utf8_lossy(&out_stdout.stdout);
    assert!(stdout_str.contains("class ArticleType(TypedDict):"));
    assert!(stdout_str.contains("class Article:"));

    // 2. Write to output directory with --models dataclass and --bs4-parser html.parser
    let out_dir = tmp.path.join("out");
    let out_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "python",
            &schema.to_string_lossy(),
            "-o",
            &out_dir.to_string_lossy(),
            "--models",
            "dataclass",
            "--bs4-parser",
            "html.parser",
            "--http-io",
            "sync",
        ])
        .output()
        .expect("run parsmith generate python to dir");
    assert!(out_cmd.status.success());

    let generated_file = out_dir.join("article.py");
    assert!(generated_file.exists());
    let content = fs::read_to_string(generated_file).unwrap();
    assert!(content.contains("@dataclass(slots=True)\nclass ArticleType:"));
    assert!(content.contains(
        "RT_BS4_FEATURES: Literal[\"html.parser\", \"lxml\", \"html5lib\"] = 'html.parser'"
    ));
}

#[test]
fn cli_generate_python_separate_runtime_and_explicit_file() {
    let tmp = CliTempDir::new("separate_rt");
    let schema = tmp.write(
        "scraper.kdl",
        "(raw)struct Scraper {\n    code { re #\"(\\d+)\"#; to-int }\n}\n",
    );
    let out_py = tmp.path.join("dist").join("custom_scraper.py");

    let out_cmd = Command::new(parsmith_bin())
        .args([
            "generate",
            "python",
            &schema.to_string_lossy(),
            "-o",
            &out_py.to_string_lossy(),
            "-R",
            "--runtime-name",
            "my_runtime",
        ])
        .output()
        .expect("run parsmith generate python -R");
    assert!(out_cmd.status.success());

    let runtime_py = tmp.path.join("dist").join("my_runtime.py");
    assert!(out_py.exists());
    assert!(runtime_py.exists());

    let parser_src = fs::read_to_string(&out_py).unwrap();
    let runtime_src = fs::read_to_string(&runtime_py).unwrap();
    assert!(
        parser_src.contains("from my_runtime import RtAssertionError, RtRegexError, rt_re_search")
    );
    assert!(!parser_src.contains("def rt_re_search("));
    assert!(runtime_src.contains("def rt_re_search("));
}

#[test]
fn cli_generate_python_reports_errors_and_exits_nonzero() {
    let tmp = CliTempDir::new("broken");
    let broken = tmp.write("broken.kdl", "struct Broken {\n    f { trim }\n}\n");

    let out = Command::new(parsmith_bin())
        .args(["generate", "python", &broken.to_string_lossy()])
        .output()
        .expect("run parsmith generate python on broken schema");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E100"));
}

#[test]
fn cli_generate_python_html_lib_flags() {
    let tmp = CliTempDir::new("html_libs");
    let schema = tmp.write(
        "card.kdl",
        "struct Card {\n    title { css \"h1\"; text; trim }\n}\n",
    );

    let cases = [
        (["--html-lib", "bs4"], "from bs4 import BeautifulSoup, Tag"),
        (["-L", "lxml"], "from lxml import html"),
        (
            ["--lib", "parsel"],
            "from parsel import Selector, SelectorList",
        ),
        (
            ["--html-lib", "selectolax"],
            "from selectolax.lexbor import LexborHTMLParser, LexborNode",
        ),
        (
            ["-L", "slax"],
            "from selectolax.lexbor import LexborHTMLParser, LexborNode",
        ),
    ];

    for (flag_pair, expected_import) in cases {
        let out = Command::new(parsmith_bin())
            .args([
                "generate",
                "python",
                &schema.to_string_lossy(),
                flag_pair[0],
                flag_pair[1],
            ])
            .output()
            .expect("run parsmith generate python with html-lib flag");
        assert!(
            out.status.success(),
            "failed for {:?}: {}",
            flag_pair,
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout_str = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout_str.contains(expected_import),
            "expected '{expected_import}' in output for {:?}, got:\n{stdout_str}",
            flag_pair
        );
    }
}

#[test]
fn cli_generate_python_directory_batch_and_separate_runtime() {
    let tmp = CliTempDir::new("dir_batch_rt");
    let schemas_dir = tmp.path.join("schemas");
    fs::create_dir_all(&schemas_dir).expect("create schemas dir");

    tmp.write("schemas/_common.kdl", "define CLEAN_TEXT { text; trim }\n");
    let kodik_file = tmp.write(
        "schemas/kodik.kdl",
        r#"import "./_common.kdl" { (define)CLEAN_TEXT }
struct KodikSearch {
    @request "curl https://kodik.example/search"
    title { css "h1"; CLEAN_TEXT }
}
"#,
    );
    let yummy_file = tmp.write(
        "schemas/yummy_anime.kdl",
        r#"json YummyItem {
    id int
    title str
}
(rest)struct YummyClient {
    @request "curl https://api.yummy.example/items/1" name="get_item" response=YummyItem
}
"#,
    );

    let out_dir = tmp.path.join("out_pkg");
    let out = Command::new(parsmith_bin())
        .args([
            "generate",
            "python",
            &schemas_dir.to_string_lossy(),
            "-o",
            &out_dir.to_string_lossy(),
            "-R",
        ])
        .output()
        .expect("run parsmith generate python <dir> -o <out_dir> -R");
    assert!(
        out.status.success(),
        "expected exit 0, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(!out_dir.join("_common.py").exists());
    assert!(out_dir.join("kodik.py").exists());
    assert!(out_dir.join("yummy_anime.py").exists());
    assert!(out_dir.join("parsmith_runtime.py").exists());

    let kodik_src = fs::read_to_string(out_dir.join("kodik.py")).unwrap();
    let yummy_src = fs::read_to_string(out_dir.join("yummy_anime.py")).unwrap();
    let runtime_src = fs::read_to_string(out_dir.join("parsmith_runtime.py")).unwrap();

    assert!(
        kodik_src.contains("from .parsmith_runtime import "),
        "expected relative runtime import in kodik.py, got:\n{kodik_src}"
    );
    assert!(
        yummy_src.contains("from .parsmith_runtime import "),
        "expected relative runtime import in yummy_anime.py, got:\n{yummy_src}"
    );
    assert!(
        runtime_src.contains("def rt_fetch_async("),
        "expected rt_fetch_async in parsmith_runtime.py"
    );
    assert!(
        runtime_src.contains("def rt_rest_call_async("),
        "expected rt_rest_call_async in parsmith_runtime.py"
    );
    assert!(
        runtime_src.contains("class Ok("),
        "expected class Ok in parsmith_runtime.py"
    );

    // Also test explicit multi-file paths with --runtime-name parsmith_runtime (non-relative)
    let out_dir_abs = tmp.path.join("out_abs");
    let out_abs = Command::new(parsmith_bin())
        .args([
            "generate",
            "python",
            &kodik_file.to_string_lossy(),
            &yummy_file.to_string_lossy(),
            "-o",
            &out_dir_abs.to_string_lossy(),
            "-R",
            "--runtime-name",
            "parsmith_runtime",
        ])
        .output()
        .expect("run parsmith generate python <file1> <file2> -o <out_dir> -R --runtime-name parsmith_runtime");
    assert!(out_abs.status.success());

    let kodik_abs_src = fs::read_to_string(out_dir_abs.join("kodik.py")).unwrap();
    let yummy_abs_src = fs::read_to_string(out_dir_abs.join("yummy_anime.py")).unwrap();
    assert!(kodik_abs_src.contains("from parsmith_runtime import "));
    assert!(yummy_abs_src.contains("from parsmith_runtime import "));
}

#[test]
fn cli_generate_multi_input_requires_output_directory() {
    let tmp = CliTempDir::new("multi_out_err");
    let schemas_dir = tmp.path.join("schemas");
    fs::create_dir_all(&schemas_dir).expect("create schemas dir");

    let file1 = tmp.write("schemas/a.kdl", "struct A { title { css \"h1\"; text } }\n");
    let file2 = tmp.write("schemas/b.kdl", "struct B { title { css \"h2\"; text } }\n");

    // 1. Directory without -o
    let out_no_o = Command::new(parsmith_bin())
        .args(["generate", "python", &schemas_dir.to_string_lossy()])
        .output()
        .expect("run parsmith generate python <dir> without -o");
    assert!(!out_no_o.status.success());
    let stderr_no_o = String::from_utf8_lossy(&out_no_o.stderr);
    assert!(stderr_no_o.contains(
        "output directory (-o / --output <DIR>) is required when compiling a directory or multiple .kdl files"
    ));

    // 2. Multiple files with -o -
    let out_dash = Command::new(parsmith_bin())
        .args([
            "generate",
            "python",
            &file1.to_string_lossy(),
            &file2.to_string_lossy(),
            "-o",
            "-",
        ])
        .output()
        .expect("run parsmith generate python <file1> <file2> -o -");
    assert!(!out_dash.status.success());
    let stderr_dash = String::from_utf8_lossy(&out_dash.stderr);
    assert!(stderr_dash.contains(
        "output directory (-o / --output <DIR>) is required when compiling a directory or multiple .kdl files"
    ));

    // 3. Directory with explicit output file -o out.py
    let out_file = Command::new(parsmith_bin())
        .args([
            "generate",
            "python",
            &schemas_dir.to_string_lossy(),
            "-o",
            "out.py",
        ])
        .output()
        .expect("run parsmith generate python <dir> -o out.py");
    assert!(!out_file.status.success());
    let stderr_file = String::from_utf8_lossy(&out_file.stderr);
    assert!(stderr_file.contains(
        "output path 'out.py' must be a directory when compiling a directory or multiple .kdl files"
    ));
}

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

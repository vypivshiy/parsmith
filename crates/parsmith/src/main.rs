//! Standalone native CLI for `parsmith`.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use parsmith::{
    Bs4Parser, CodegenOptions, GenerateResult, HttpIo, PhpHtmlLib, PythonHtmlLib, PythonModels,
    Target,
};

#[derive(Debug, Parser)]
#[command(
    name = "parsmith",
    version,
    about = "Universal Rust compiler, linter, and multi-target codegen engine for KDL 2.0 Schema DSL v2.1"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Output format for compiler and linter diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable Rust-style annotated source snippets.
    #[default]
    Text,
    /// Structured JSON diagnostic payload.
    Json,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Validate a KDL 2.0 schema file and its imports without generating code
    Check {
        /// Path(s) to .kdl schema file(s) or directory(ies) to validate
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,

        /// Target language(s) for identifier validation (`python`, `javascript`, `typescript`, `go`, `rust`, `elixir`, `php`, `csharp`, `kotlin`)
        #[arg(long, short = 't')]
        target: Vec<String>,
    },

    /// Generate typed parser code from a KDL 2.0 schema
    Generate {
        #[command(subcommand)]
        target: GenerateTarget,
    },

    /// Bootstrap a KDL v2.1 `json` schema from a sample JSON payload
    JsonToKdl {
        /// Path to the input JSON file, or `-` (or omit) to read from stdin
        #[arg(value_name = "INPUT")]
        input: Option<PathBuf>,

        /// Root schema name
        #[arg(long, default_value = "JsonResponse", value_name = "NAME")]
        name: String,

        /// Output KDL file path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,
    },

    /// Print the parsmith version
    Version {
        /// Output format (`text` or `json`)
        #[arg(short = 'f', long = "format", value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum GenerateTarget {
    /// Generate Python 3.10+ parser code (`bs4` / `lxml` / `parsel` / `selectolax`, `TypedDict` / `dataclass`)
    #[command(visible_alias = "py")]
    Python {
        /// Path(s) to .kdl schema file(s) or directory(ies) to compile
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Output file or directory path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// Python HTML DOM extraction library (`bs4`, `lxml`, `parsel`, `selectolax`, or `slax`)
        #[arg(short = 'L', long = "html-lib", visible_alias = "lib", default_value_t = PythonHtmlLib::Bs4)]
        html_lib: PythonHtmlLib,

        /// Output model flavor (`typeddict` or `dataclass`)
        #[arg(long = "models", default_value_t = PythonModels::TypedDict)]
        models: PythonModels,

        /// BeautifulSoup4 HTML parser engine (`lxml`, `html.parser`, or `html5lib`)
        #[arg(long = "bs4-parser", default_value_t = Bs4Parser::Lxml)]
        bs4_parser: Bs4Parser,

        /// HTTP method generation mode (`both`, `sync`, or `async`)
        #[arg(long = "http-io", default_value_t = HttpIo::Both)]
        http_io: HttpIo,

        /// Emit shared runtime helpers into a separate `<runtime_name>.py` module
        #[arg(short = 'R', long = "separate-runtime")]
        separate_runtime: bool,

        /// Module name for the separate runtime file
        #[arg(long = "runtime-name", default_value = parsmith::DEFAULT_PYTHON_RUNTIME_NAME, value_name = "NAME")]
        runtime_name: String,

        /// Custom code block injected after imports and runtime utilities
        #[arg(long = "prologue", value_name = "CODE")]
        prologue: Option<String>,

        /// Custom code block appended at the end of the generated module
        #[arg(long = "epilogue", value_name = "CODE")]
        epilogue: Option<String>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },

    /// Generate standalone TypeScript (`.ts`) parser code
    #[command(name = "ts", visible_alias = "typescript")]
    Ts(JsTsGenerateArgs),

    /// Generate clean ESM JavaScript (`.js`) paired with a companion `.d.ts` declaration file
    #[command(name = "js", visible_alias = "javascript")]
    Js(JsTsGenerateArgs),

    /// Generate Go 1.26+ (`goquery`, `net/http`, `gjson`) parser and REST client code
    #[command(name = "go", visible_alias = "golang")]
    Go {
        /// Path(s) to .kdl schema file(s) or directory(ies) to compile
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Output file or directory path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// Go package name for the generated file(s)
        #[arg(
            long = "package",
            visible_alias = "namespace",
            default_value = "main",
            value_name = "PKG"
        )]
        package: String,

        /// Emit shared runtime helpers into a separate `<runtime_name>.go` file
        #[arg(short = 'R', long = "separate-runtime")]
        separate_runtime: bool,

        /// File/module stem for the separate runtime file
        #[arg(long = "runtime-name", default_value = parsmith::DEFAULT_RUNTIME_NAME, value_name = "NAME")]
        runtime_name: String,

        /// Custom code block injected after imports and runtime utilities
        #[arg(long = "prologue", value_name = "CODE")]
        prologue: Option<String>,

        /// Custom code block appended at the end of the generated module
        #[arg(long = "epilogue", value_name = "CODE")]
        epilogue: Option<String>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },

    /// Generate Rust (`dom_query` + `serde` + `reqwest`) parser and REST client code
    #[command(name = "rust", visible_alias = "rs")]
    Rust {
        /// Path(s) to .kdl schema file(s) or directory(ies) to compile
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Output file or directory path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// Emit shared runtime helpers into a separate `<runtime_name>.rs` module
        #[arg(short = 'R', long = "separate-runtime")]
        separate_runtime: bool,

        /// Module name for the separate runtime file
        #[arg(long = "runtime-name", default_value = parsmith::DEFAULT_RUNTIME_NAME, value_name = "NAME")]
        runtime_name: String,

        /// Custom code block injected after imports and runtime utilities
        #[arg(long = "prologue", value_name = "CODE")]
        prologue: Option<String>,

        /// Custom code block appended at the end of the generated module
        #[arg(long = "epilogue", value_name = "CODE")]
        epilogue: Option<String>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },

    /// Generate Elixir 1.18+ (`Floki`, built-in `JSON`, `:httpc` / `Req`) parser and REST client code
    #[command(name = "elixir", visible_alias = "ex")]
    Elixir {
        /// Path(s) to .kdl schema file(s) or directory(ies) to compile
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Output file or directory path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// Elixir module prefix for generated modules
        #[arg(
            long = "package",
            visible_alias = "namespace",
            default_value = "main",
            value_name = "PKG"
        )]
        package: String,

        /// Emit shared runtime helpers into a separate `<runtime_name>.ex` file
        #[arg(short = 'R', long = "separate-runtime")]
        separate_runtime: bool,

        /// File/module stem for the separate runtime file
        #[arg(long = "runtime-name", default_value = parsmith::DEFAULT_RUNTIME_NAME, value_name = "NAME")]
        runtime_name: String,

        /// Custom code block injected after imports and runtime utilities
        #[arg(long = "prologue", value_name = "CODE")]
        prologue: Option<String>,

        /// Custom code block appended at the end of the generated module
        #[arg(long = "epilogue", value_name = "CODE")]
        epilogue: Option<String>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },

    /// Generate PHP 8.1+ (`ext-dom` / `symfony/dom-crawler`, `JsonSerializable`, `ext-curl` / `Guzzle`) parser and REST client code
    #[command(name = "php")]
    Php {
        /// Path(s) to .kdl schema file(s) or directory(ies) to compile
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Output file or directory path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// PHP HTML DOM extraction library (`native` or `dom-crawler`)
        #[arg(short = 'L', long = "html-lib", visible_alias = "lib", default_value_t = PhpHtmlLib::Native)]
        html_lib: PhpHtmlLib,

        /// PHP namespace for the generated file(s)
        #[arg(
            long = "package",
            visible_alias = "namespace",
            default_value = "main",
            value_name = "PKG"
        )]
        package: String,

        /// Emit shared runtime helpers into a separate `<runtime_name>.php` file
        #[arg(short = 'R', long = "separate-runtime")]
        separate_runtime: bool,

        /// File/module stem for the separate runtime file
        #[arg(long = "runtime-name", default_value = parsmith::DEFAULT_RUNTIME_NAME, value_name = "NAME")]
        runtime_name: String,

        /// Custom code block injected after imports and runtime utilities
        #[arg(long = "prologue", value_name = "CODE")]
        prologue: Option<String>,

        /// Custom code block appended at the end of the generated module
        #[arg(long = "epilogue", value_name = "CODE")]
        epilogue: Option<String>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },

    /// Generate C# / .NET 8+ (`AngleSharp`, `System.Text.Json`, `HttpClient`) parser and REST client code
    #[command(name = "csharp", visible_alias = "cs")]
    CSharp {
        /// Path(s) to .kdl schema file(s) or directory(ies) to compile
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Output file or directory path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// C# namespace for the generated file(s)
        #[arg(
            long = "package",
            visible_alias = "namespace",
            default_value = "main",
            value_name = "PKG"
        )]
        package: String,

        /// Emit shared runtime helpers into a separate `<runtime_name>.cs` file
        #[arg(short = 'R', long = "separate-runtime")]
        separate_runtime: bool,

        /// File/module stem for the separate runtime file
        #[arg(long = "runtime-name", default_value = parsmith::DEFAULT_RUNTIME_NAME, value_name = "NAME")]
        runtime_name: String,

        /// Custom code block injected after imports and runtime utilities
        #[arg(long = "prologue", value_name = "CODE")]
        prologue: Option<String>,

        /// Custom code block appended at the end of the generated module
        #[arg(long = "epilogue", value_name = "CODE")]
        epilogue: Option<String>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },

    /// Generate Kotlin / JVM (`org.jsoup:jsoup`, `kotlinx.serialization.json`, `java.net.http.HttpClient`) parser and REST client code
    #[command(name = "kotlin", visible_alias = "kt")]
    Kotlin {
        /// Path(s) to .kdl schema file(s) or directory(ies) to compile
        #[arg(required = true, num_args = 1.., value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Output file or directory path (writes to stdout when omitted or `-`)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// Kotlin package name for the generated file(s)
        #[arg(
            long = "package",
            visible_alias = "namespace",
            default_value = "main",
            value_name = "PKG"
        )]
        package: String,

        /// Emit shared runtime helpers into a separate `<runtime_name>.kt` file
        #[arg(short = 'R', long = "separate-runtime")]
        separate_runtime: bool,

        /// File/module stem for the separate runtime file
        #[arg(long = "runtime-name", default_value = parsmith::DEFAULT_RUNTIME_NAME, value_name = "NAME")]
        runtime_name: String,

        /// Custom code block injected after imports and runtime utilities
        #[arg(long = "prologue", value_name = "CODE")]
        prologue: Option<String>,

        /// Custom code block appended at the end of the generated module
        #[arg(long = "epilogue", value_name = "CODE")]
        epilogue: Option<String>,

        /// Diagnostic output format (`text` or `json`)
        #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Args)]
struct JsTsGenerateArgs {
    /// Path(s) to .kdl schema file(s) or directory(ies) to compile
    #[arg(required = true, num_args = 1.., value_name = "PATHS")]
    paths: Vec<PathBuf>,

    /// Output file or directory path (writes to stdout when omitted or `-`)
    #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Custom code block injected after imports and runtime utilities
    #[arg(long = "prologue", value_name = "CODE")]
    prologue: Option<String>,

    /// Custom code block appended at the end of the generated module
    #[arg(long = "epilogue", value_name = "CODE")]
    epilogue: Option<String>,

    /// Diagnostic output format (`text` or `json`)
    #[arg(long, short = 'f', value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

fn read_json_input(input: Option<&Path>) -> Result<String, String> {
    match input {
        Some(path) if path != Path::new("-") => fs::read_to_string(path)
            .map_err(|err| format!("failed to read '{}': {err}", path.display())),
        _ => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .map_err(|err| format!("failed to read stdin: {err}"))?;
            Ok(buf)
        }
    }
}

fn write_kdl_output(output: Option<&Path>, content: &str) -> Result<(), String> {
    match output {
        Some(path) if path != Path::new("-") => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("failed to create directory '{}': {err}", parent.display())
                })?;
            }
            fs::write(path, content)
                .map_err(|err| format!("failed to write '{}': {err}", path.display()))
        }
        _ => {
            print!("{content}");
            Ok(())
        }
    }
}

fn is_explicit_output_file(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    if raw.ends_with('/') || raw.ends_with('\\') || path.is_dir() {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "py" | "ts" | "js" | "go" | "rs" | "ex" | "php" | "cs" | "kt"
            )
        })
}

fn write_generated_files(output: Option<&Path>, res: &GenerateResult) -> Result<(), String> {
    match output {
        Some(path) if path != Path::new("-") => {
            if is_explicit_output_file(path) {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent).map_err(|err| {
                        format!("failed to create directory '{}': {err}", parent.display())
                    })?;
                }
                let mut iter = res.files.iter();
                if let Some((_, primary_code)) = iter.next() {
                    fs::write(path, primary_code)
                        .map_err(|err| format!("failed to write '{}': {err}", path.display()))?;
                }
                let base_dir = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                for (fname, content) in iter {
                    let companion = base_dir.join(fname);
                    fs::write(&companion, content).map_err(|err| {
                        format!("failed to write '{}': {err}", companion.display())
                    })?;
                }
            } else {
                fs::create_dir_all(path).map_err(|err| {
                    format!("failed to create directory '{}': {err}", path.display())
                })?;
                for (fname, content) in &res.files {
                    let target_path = path.join(fname);
                    fs::write(&target_path, content).map_err(|err| {
                        format!("failed to write '{}': {err}", target_path.display())
                    })?;
                }
            }
            Ok(())
        }
        _ => {
            if let Some(code) = res.primary_code() {
                print!("{code}");
            }
            Ok(())
        }
    }
}

fn format_version(format: OutputFormat) -> String {
    match format {
        OutputFormat::Text => format!("parsmith {}", parsmith::version()),
        OutputFormat::Json => serde_json::json!({
            "name": "parsmith",
            "version": parsmith::version(),
        })
        .to_string(),
    }
}

fn execute_generate(
    paths: &[PathBuf],
    output: Option<&Path>,
    mut opts: CodegenOptions,
    format: OutputFormat,
) -> Result<ExitCode, String> {
    let is_multi_input = paths.len() > 1 || paths.first().is_some_and(|p| p.is_dir());
    if is_multi_input {
        if output.is_none() || output == Some(Path::new("-")) {
            return Err(
                "output directory (-o / --output <DIR>) is required when compiling a directory or multiple .kdl files"
                    .to_string(),
            );
        }
        if let Some(out_path) = output
            && is_explicit_output_file(out_path)
        {
            return Err(format!(
                "output path '{}' must be a directory when compiling a directory or multiple .kdl files",
                out_path.display()
            ));
        }
    } else if let Some(out_path) = output
        && out_path != Path::new("-")
        && is_explicit_output_file(out_path)
        && let Some(stem) = out_path.file_stem().and_then(|s| s.to_str())
        && !stem.is_empty()
    {
        opts = opts.with_module_stem(stem);
    }

    let res = parsmith::generate_paths(paths, &opts);
    if !res.diagnostics.is_empty() {
        match format {
            OutputFormat::Text => {
                let rendered = res.format_text();
                if !rendered.is_empty() {
                    eprintln!("{rendered}");
                }
            }
            OutputFormat::Json => {
                eprintln!("{}", res.format_json());
            }
        }
    }
    if res.has_errors() {
        return Ok(ExitCode::FAILURE);
    }
    write_generated_files(output, &res)?;
    Ok(ExitCode::SUCCESS)
}

fn execute_jsts_generate(target: Target, args: JsTsGenerateArgs) -> Result<ExitCode, String> {
    let mut opts = CodegenOptions::new(target);
    if let Some(p) = args.prologue {
        opts = opts.with_prologue(p);
    }
    if let Some(e) = args.epilogue {
        opts = opts.with_epilogue(e);
    }
    execute_generate(&args.paths, args.output.as_deref(), opts, args.format)
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Some(Commands::Check {
            paths,
            format,
            target,
        }) => {
            let targets_opt = if target.is_empty() {
                None
            } else {
                Some(target)
            };
            let result = parsmith::check_paths_with_targets(&paths, targets_opt);
            match format {
                OutputFormat::Text => {
                    let rendered = result.format_text();
                    if !rendered.is_empty() {
                        println!("{rendered}");
                    }
                }
                OutputFormat::Json => {
                    println!("{}", result.format_json());
                }
            }
            if result.has_errors() {
                Ok(ExitCode::FAILURE)
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
        Some(Commands::Generate { target }) => match target {
            GenerateTarget::Python {
                paths,
                output,
                html_lib,
                models,
                bs4_parser,
                http_io,
                separate_runtime,
                runtime_name,
                prologue,
                epilogue,
                format,
            } => {
                let mut opts = CodegenOptions::new(Target::Python)
                    .with_html_lib(html_lib)
                    .with_models(models)
                    .with_bs4_parser(bs4_parser)
                    .with_http_io(http_io)
                    .with_separate_runtime(separate_runtime)
                    .with_runtime_name(runtime_name);
                if let Some(p) = prologue {
                    opts = opts.with_prologue(p);
                }
                if let Some(e) = epilogue {
                    opts = opts.with_epilogue(e);
                }
                execute_generate(&paths, output.as_deref(), opts, format)
            }
            GenerateTarget::Ts(args) => execute_jsts_generate(Target::TypeScript, args),
            GenerateTarget::Js(args) => execute_jsts_generate(Target::JavaScript, args),
            GenerateTarget::Go {
                paths,
                output,
                package,
                separate_runtime,
                runtime_name,
                prologue,
                epilogue,
                format,
            } => {
                let mut opts = CodegenOptions::new(Target::Go)
                    .with_package(package)
                    .with_separate_runtime(separate_runtime)
                    .with_runtime_name(runtime_name);
                if let Some(p) = prologue {
                    opts = opts.with_prologue(p);
                }
                if let Some(e) = epilogue {
                    opts = opts.with_epilogue(e);
                }
                execute_generate(&paths, output.as_deref(), opts, format)
            }
            GenerateTarget::Rust {
                paths,
                output,
                separate_runtime,
                runtime_name,
                prologue,
                epilogue,
                format,
            } => {
                let mut opts = CodegenOptions::new(Target::Rust)
                    .with_separate_runtime(separate_runtime)
                    .with_runtime_name(runtime_name);
                if let Some(p) = prologue {
                    opts = opts.with_prologue(p);
                }
                if let Some(e) = epilogue {
                    opts = opts.with_epilogue(e);
                }
                execute_generate(&paths, output.as_deref(), opts, format)
            }
            GenerateTarget::Elixir {
                paths,
                output,
                package,
                separate_runtime,
                runtime_name,
                prologue,
                epilogue,
                format,
            } => {
                let mut opts = CodegenOptions::new(Target::Elixir)
                    .with_package(package)
                    .with_separate_runtime(separate_runtime)
                    .with_runtime_name(runtime_name);
                if let Some(p) = prologue {
                    opts = opts.with_prologue(p);
                }
                if let Some(e) = epilogue {
                    opts = opts.with_epilogue(e);
                }
                execute_generate(&paths, output.as_deref(), opts, format)
            }
            GenerateTarget::Php {
                paths,
                output,
                html_lib,
                package,
                separate_runtime,
                runtime_name,
                prologue,
                epilogue,
                format,
            } => {
                let mut opts = CodegenOptions::new(Target::Php)
                    .with_php_html_lib(html_lib)
                    .with_package(package)
                    .with_separate_runtime(separate_runtime)
                    .with_runtime_name(runtime_name);
                if let Some(p) = prologue {
                    opts = opts.with_prologue(p);
                }
                if let Some(e) = epilogue {
                    opts = opts.with_epilogue(e);
                }
                execute_generate(&paths, output.as_deref(), opts, format)
            }
            GenerateTarget::CSharp {
                paths,
                output,
                package,
                separate_runtime,
                runtime_name,
                prologue,
                epilogue,
                format,
            } => {
                let mut opts = CodegenOptions::new(Target::CSharp)
                    .with_package(package)
                    .with_separate_runtime(separate_runtime)
                    .with_runtime_name(runtime_name);
                if let Some(p) = prologue {
                    opts = opts.with_prologue(p);
                }
                if let Some(e) = epilogue {
                    opts = opts.with_epilogue(e);
                }
                execute_generate(&paths, output.as_deref(), opts, format)
            }
            GenerateTarget::Kotlin {
                paths,
                output,
                package,
                separate_runtime,
                runtime_name,
                prologue,
                epilogue,
                format,
            } => {
                let mut opts = CodegenOptions::new(Target::Kotlin)
                    .with_package(package)
                    .with_separate_runtime(separate_runtime)
                    .with_runtime_name(runtime_name);
                if let Some(p) = prologue {
                    opts = opts.with_prologue(p);
                }
                if let Some(e) = epilogue {
                    opts = opts.with_epilogue(e);
                }
                execute_generate(&paths, output.as_deref(), opts, format)
            }
        },
        Some(Commands::JsonToKdl {
            input,
            name,
            output,
        }) => {
            let raw_json = read_json_input(input.as_deref())?;
            let kdl_source =
                parsmith::json_to_kdl(&raw_json, &name).map_err(|err| err.to_string())?;
            write_kdl_output(output.as_deref(), &kdl_source)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Commands::Version { format }) => {
            println!("{}", format_version(format));
            Ok(ExitCode::SUCCESS)
        }
        None => {
            println!("{}", format_version(OutputFormat::Text));
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ERROR: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

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
    fn cli_generate_ts_and_js_writes_expected_files() {
        let temp = TempDir::new("parsmith_cli_jsts");
        let dir = &temp.0;

        let schema_path = dir.join("card.kdl");
        fs::write(&schema_path, "struct Card { title { css \"h1\"; text } }\n").unwrap();

        let out_ts = dir.join("custom_card.ts");
        let cli_ts = Cli::parse_from([
            "parsmith",
            "generate",
            "ts",
            schema_path.to_str().unwrap(),
            "-o",
            out_ts.to_str().unwrap(),
        ]);
        assert_eq!(run(cli_ts).unwrap(), ExitCode::SUCCESS);
        assert!(out_ts.is_file());
        let ts_content = fs::read_to_string(&out_ts).unwrap();
        assert!(ts_content.contains("export interface CardType"));
        assert!(ts_content.contains("export class Card"));

        let out_js = dir.join("custom_card.js");
        let out_dts = dir.join("custom_card.d.ts");
        let cli_js = Cli::parse_from([
            "parsmith",
            "generate",
            "js",
            schema_path.to_str().unwrap(),
            "-o",
            out_js.to_str().unwrap(),
        ]);
        assert_eq!(run(cli_js).unwrap(), ExitCode::SUCCESS);
        assert!(out_js.is_file());
        assert!(out_dts.is_file());
        let js_content = fs::read_to_string(&out_js).unwrap();
        let dts_content = fs::read_to_string(&out_dts).unwrap();
        assert!(js_content.contains("export class Card"));
        assert!(dts_content.contains("export declare class Card"));

        let out_dir = dir.join("js_dir");
        let cli_js_dir = Cli::parse_from([
            "parsmith",
            "generate",
            "js",
            schema_path.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
            "--prologue",
            "// custom-prologue",
            "--epilogue",
            "// custom-epilogue",
        ]);
        assert_eq!(run(cli_js_dir).unwrap(), ExitCode::SUCCESS);
        let dir_js = out_dir.join("card.js");
        let dir_dts = out_dir.join("card.d.ts");
        assert!(dir_js.is_file());
        assert!(dir_dts.is_file());
        let dir_js_content = fs::read_to_string(&dir_js).unwrap();
        assert!(dir_js_content.contains("// custom-prologue"));
        assert!(dir_js_content.contains("// custom-epilogue"));
    }
}

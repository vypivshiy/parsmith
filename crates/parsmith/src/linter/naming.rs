//! Identifier case conversion (`to_snake_case`, `to_camel_case`), JSON projector naming,
//! target symbol formatting, and 4-ecosystem reserved-keyword tables.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::json_to_kdl::to_pascal_case;
use crate::linter::is_portable_ident;

/// Canonical 4 target ecosystems checked by Pass 4 by default.
pub const TARGETS: &[&str] = &["python", "javascript", "go", "rust"];

/// Python reserved keywords.
pub const PYTHON_RESERVED: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// JavaScript and TypeScript reserved keywords.
pub const JAVASCRIPT_RESERVED: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "constructor",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Go reserved keywords.
pub const GO_RESERVED: &[&str] = &[
    "break",
    "default",
    "func",
    "interface",
    "select",
    "case",
    "defer",
    "go",
    "map",
    "struct",
    "chan",
    "else",
    "goto",
    "package",
    "switch",
    "const",
    "fallthrough",
    "if",
    "range",
    "type",
    "continue",
    "for",
    "import",
    "return",
    "var",
];

/// Rust keywords that cannot be escaped with `r#...` raw identifiers.
pub const RUST_CANNOT_BE_RAW: &[&str] = &["crate", "self", "super", "Self", "_"];

/// Full list of Rust reserved keywords (used by codegen to decide when to emit `r#ident`).
pub const RUST_RESERVED: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
];

/// Elixir keywords that cannot be escaped in generated Elixir code.
pub const ELIXIR_CANNOT_BE_RAW: &[&str] = &[];

/// Full list of Elixir reserved words (used by codegen to escape/prefix function identifiers).
pub const ELIXIR_RESERVED: &[&str] = &[
    "do", "end", "fn", "after", "else", "catch", "rescue", "true", "false", "nil", "and", "or",
    "not", "in", "when",
];

/// PHP keywords that cannot be escaped in generated PHP code.
pub const PHP_CANNOT_BE_RAW: &[&str] = &[];

/// Full list of PHP reserved words (used by codegen to escape class/function identifiers).
pub const PHP_RESERVED: &[&str] = &[
    "__halt_compiler",
    "abstract",
    "and",
    "array",
    "as",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "die",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enddeclare",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "eval",
    "exit",
    "extends",
    "false",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "null",
    "or",
    "parent",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "self",
    "static",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "unset",
    "use",
    "var",
    "while",
    "xor",
    "yield",
];

/// C# keywords that cannot be escaped with `@...` verbatim identifiers.
pub const CSHARP_CANNOT_BE_RAW: &[&str] = &[];

/// Full list of C# reserved keywords (used by codegen to decide when to emit `@ident`).
pub const CSHARP_RESERVED: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];

/// Kotlin keywords that cannot be escaped with backtick `` `...` `` identifiers.
pub const KOTLIN_CANNOT_BE_RAW: &[&str] = &[];

/// Full list of Kotlin hard reserved keywords (used by codegen to decide when to emit `` `ident` ``).
pub const KOTLIN_RESERVED: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

/// Generated symbol category in `ssc_codegen/symbols.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetSymbolKind {
    /// Parser struct class (`Product`).
    Struct,
    /// Synthesized struct output type (`ProductType`).
    Type,
    /// JSON schema model (`UserJson`).
    Json,
    /// Standalone module function (`fn`).
    Function,
    /// Struct or JSON field (`_parse_title`, `title`).
    Field,
    /// Struct method (`@request` or `@check`).
    Method,
    /// Precomputed `@init` method (`_init_seed`).
    Init,
    /// `@request` parameter placeholder (`page_num`).
    Placeholder,
}

impl TargetSymbolKind {
    /// Returns the canonical lowercase kind name used in `E402` / `E403` diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Type => "type",
            Self::Json => "json",
            Self::Function => "fn",
            Self::Field => "field",
            Self::Method => "method",
            Self::Init => "init",
            Self::Placeholder => "placeholder",
        }
    }
}

impl fmt::Display for TargetSymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Normalizes a single target name or alias (`"py"`, `"python"`, `"js"`, `"ts"`, `"javascript"`,
/// `"typescript"`, `"go"`, `"rust"`, `"ex"`, `"elixir"`, `"php"`, `"cs"`, `"csharp"`, `"kt"`, `"kotlin"`).
pub fn normalize_target_name(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "py" | "python" | "py-bs4" | "python-bs4" | "py-lxml" | "python-lxml" | "py-parsel"
        | "python-parsel" | "py-selectolax" | "python-selectolax" | "py-slax" | "python-slax" => {
            Ok("python")
        }
        "js" | "ts" | "javascript" | "typescript" => Ok("javascript"),
        "go" | "golang" => Ok("go"),
        "rs" | "rust" => Ok("rust"),
        "ex" | "elixir" => Ok("elixir"),
        "php" | "php-native" | "php-ext-dom" | "php-dom" | "php-dom-crawler"
        | "php-dom_crawler" | "php-crawler" | "php-symfony" => Ok("php"),
        "cs" | "csharp" | "c#" => Ok("csharp"),
        "kt" | "kotlin" => Ok("kotlin"),
        _ => Err(format!(
            "Unknown target '{raw}'. Use: python, javascript, go, rust, elixir, php, csharp, kotlin."
        )),
    }
}

/// Normalizes a list of target names into deduplicated canonical target identifiers.
///
/// When `targets` is `None` or empty, returns all 4 canonical ecosystems (`["python", "javascript", "go", "rust"]`).
pub fn normalize_targets<I, S>(targets: Option<I>) -> Result<Vec<&'static str>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let Some(iter) = targets else {
        return Ok(TARGETS.to_vec());
    };
    let mut out = Vec::new();
    let mut had_any = false;
    for item in iter {
        had_any = true;
        let canonical = normalize_target_name(item.as_ref())?;
        if !out.contains(&canonical) {
            out.push(canonical);
        }
    }
    if !had_any {
        return Ok(TARGETS.to_vec());
    }
    Ok(out)
}

/// Returns the reserved keyword table for `target`.
pub fn target_reserved_words(target: &str) -> Result<&'static [&'static str], String> {
    let canonical = normalize_target_name(target)?;
    Ok(match canonical {
        "python" => PYTHON_RESERVED,
        "javascript" => JAVASCRIPT_RESERVED,
        "go" => GO_RESERVED,
        "rust" => RUST_CANNOT_BE_RAW,
        "elixir" => ELIXIR_CANNOT_BE_RAW,
        "php" => PHP_CANNOT_BE_RAW,
        "csharp" => CSHARP_CANNOT_BE_RAW,
        "kotlin" => KOTLIN_CANNOT_BE_RAW,
        _ => unreachable!(),
    })
}

/// Returns `true` if `ident` is a reserved keyword in KDL 2.0 or any target language
/// (`python`, `javascript`/`typescript`, `go`, `rust`).
pub fn is_reserved_in_any_ecosystem(ident: &str) -> bool {
    matches!(ident, "true" | "false" | "null" | "nil" | "inf" | "nan")
        || PYTHON_RESERVED.contains(&ident)
        || JAVASCRIPT_RESERVED.contains(&ident)
        || GO_RESERVED.contains(&ident)
        || RUST_RESERVED.contains(&ident)
}

/// Checks whether `symbol` is a valid identifier in `target` (`^[A-Za-z_][A-Za-z0-9_]*$` and not reserved).
pub fn is_valid_symbol(target: &str, symbol: &str) -> bool {
    let Ok(reserved) = target_reserved_words(target) else {
        return false;
    };
    is_portable_ident(symbol) && !reserved.contains(&symbol)
}

/// Converts a raw identifier string into `snake_case` matching `ssc_codegen/naming.py:to_snake_case`.
///
/// - `'myFieldName'` -> `'my_field_name'`
/// - `'MyFieldName'` -> `'my_field_name'`
/// - `'my-field-name'` -> `'my_field_name'`
/// - `'my field name'` -> `'my_field_name'`
pub fn to_snake_case(s: &str) -> String {
    // Step 1: replace runs of '-' or whitespace with '_'
    let mut step1 = String::with_capacity(s.len());
    let mut in_sep = false;
    for ch in s.chars() {
        if ch == '-' || ch.is_whitespace() {
            if !in_sep {
                step1.push('_');
                in_sep = true;
            }
        } else {
            step1.push(ch);
            in_sep = false;
        }
    }

    // Step 2: insert '_' between [a-z0-9] and [A-Z], then lowercase
    let chars: Vec<char> = step1.chars().collect();
    let mut out = String::with_capacity(chars.len() + 4);
    for (idx, &ch) in chars.iter().enumerate() {
        if idx > 0 {
            let prev = chars[idx - 1];
            if (prev.is_ascii_lowercase() || prev.is_ascii_digit()) && ch.is_ascii_uppercase() {
                out.push('_');
            }
        }
        for lower in ch.to_lowercase() {
            out.push(lower);
        }
    }
    out
}

/// Converts a raw identifier string into `camelCase` matching `ssc_codegen/naming.py:to_camel_case`.
///
/// - `'MyFieldName'` -> `'myFieldName'`
/// - `'my_field_name'` -> `'myFieldName'`
/// - `'JsnArr'` -> `'jsnArr'`
pub fn to_camel_case(s: &str) -> String {
    let parts: Vec<&str> = s
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return s.to_owned();
    }

    let mut out = String::with_capacity(s.len());
    for (idx, part) in parts.iter().enumerate() {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            if idx == 0 {
                for lower in first.to_lowercase() {
                    out.push(lower);
                }
            } else {
                for upper in first.to_uppercase() {
                    out.push(upper);
                }
            }
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Formats the canonical Python JSON projector function identifier (`rt_project_<snake_name>`).
pub fn python_json_projector_name(name: &str) -> String {
    let clean = to_snake_case(name);
    let trimmed = clean.trim_start_matches('_');
    format!("rt_project_{trimmed}")
}

/// Formats the canonical JavaScript/TypeScript JSON projector function identifier (`rtProject<PascalName>`).
pub fn js_json_projector_name(name: &str) -> String {
    format!("rtProject{}", to_pascal_case(name))
}

/// Returns the module-level symbol name(s) generated for a top-level declaration in `target`.
pub fn top_symbol_names(kind: TargetSymbolKind, raw: &str, target: &str) -> Vec<String> {
    let pascal = to_pascal_case(raw);
    match kind {
        TargetSymbolKind::Struct => vec![pascal.clone(), format!("{pascal}Type")],
        TargetSymbolKind::Type => vec![format!("{pascal}Type")],
        TargetSymbolKind::Json => vec![format!("{pascal}Json")],
        _ => {
            let canonical = normalize_target_name(target).unwrap_or(target);
            match canonical {
                "python" | "rust" | "elixir" | "php" => vec![to_snake_case(raw)],
                "javascript" | "kotlin" => vec![to_camel_case(raw)],
                "go" | "csharp" => vec![pascal],
                _ => vec![to_snake_case(raw)],
            }
        }
    }
}

/// Returns the local symbol name generated for a struct field, method, placeholder, or `@init` field in `target`.
pub fn local_symbol(kind: TargetSymbolKind, raw: &str, target: &str) -> String {
    let canonical = normalize_target_name(target).unwrap_or(target);
    let snake = to_snake_case(raw);
    let pascal = to_pascal_case(raw);
    match kind {
        TargetSymbolKind::Field => match canonical {
            "python" | "php" => format!("_parse_{snake}"),
            "javascript" | "kotlin" => format!("_parse{pascal}"),
            "go" | "csharp" => format!("parse{pascal}"),
            "rust" | "elixir" => snake,
            _ => snake,
        },
        TargetSymbolKind::Method => match canonical {
            "python" | "rust" | "elixir" | "php" => snake,
            "javascript" | "kotlin" => to_camel_case(&snake),
            "go" | "csharp" => pascal,
            _ => snake,
        },
        TargetSymbolKind::Placeholder => match canonical {
            "python" | "rust" | "elixir" | "php" => snake,
            "javascript" | "csharp" | "kotlin" => to_camel_case(&snake),
            "go" => to_camel_case(raw),
            _ => snake,
        },
        TargetSymbolKind::Init => match canonical {
            "python" | "php" => format!("_init_{snake}"),
            "javascript" | "kotlin" => format!("_init{pascal}"),
            "go" | "csharp" => format!("init{pascal}"),
            "rust" | "elixir" => format!("init_{snake}"),
            _ => format!("_init_{snake}"),
        },
        _ => snake,
    }
}

/// Returns the generated target identifier for a canonical JSON field name.
pub fn json_field_symbol(raw: &str, target: &str) -> String {
    let canonical = normalize_target_name(target).unwrap_or(target);
    let clean = raw.trim_end_matches('?');
    match canonical {
        "python" | "rust" | "elixir" | "php" => to_snake_case(clean),
        "javascript" | "kotlin" => to_camel_case(clean),
        "go" | "csharp" => to_pascal_case(clean),
        _ => to_snake_case(clean),
    }
}

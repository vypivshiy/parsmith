//! Pure-Rust portable regular expression validator and normalizer (`regex-syntax`).
//!
//! Enforces the portable DSL regex subset: supports inline flags (`(?imsx)`) including automatic
//! stripping of `(?x)` verbose whitespace and `#` comments, while rejecting lookaround assertions
//! (`(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`) and named capture groups (`(?P<name>...)`,
//! `(?<name>...)`, `(?P=name)`).

use regex_syntax::Parser;
use regex_syntax::hir::Hir;

/// Validates a regex pattern at compile time against the portable regex subset and returns
/// the number of explicit unnamed capture groups (excluding group `0`).
pub fn validate_regex(pattern: &str) -> Result<usize, String> {
    let trimmed = pattern.trim_start();
    let normalized = normalize_regex_pattern(trimmed);
    check_portable_regex_subset(&normalized)?;
    let hir = Parser::new()
        .parse(&normalized)
        .map_err(|e| e.to_string())?;
    check_hir_no_named_captures(&hir)?;
    Ok(hir.properties().explicit_captures_len())
}

fn check_portable_regex_subset(pattern: &str) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut in_char_class = false;

    while i < n {
        let b = bytes[i];
        if b == b'\\' {
            i += 1;
            if i < n {
                let ch_len = pattern[i..].chars().next().map_or(1, char::len_utf8);
                i += ch_len;
            }
            continue;
        }

        if b == b'[' && !in_char_class {
            in_char_class = true;
            i += 1;
            if i < n && bytes[i] == b'^' {
                i += 1;
            }
            if i < n && bytes[i] == b']' {
                i += 1;
            }
            continue;
        }

        if in_char_class {
            if b == b'[' && i + 1 < n && matches!(bytes[i + 1], b':' | b'.' | b'=') {
                let delim = bytes[i + 1];
                i += 2;
                while i + 1 < n && !(bytes[i] == delim && bytes[i + 1] == b']') {
                    i += 1;
                }
                if i + 1 < n {
                    i += 2;
                }
                continue;
            }
            if b == b']' {
                in_char_class = false;
                i += 1;
                continue;
            }
        }

        if !in_char_class && b == b'(' && i + 1 < n && bytes[i + 1] == b'?' {
            let rest = &pattern[i + 2..];
            if rest.starts_with('=') {
                return Err(
                    "lookaround assertion '(?=...)' is not supported in the portable regex subset"
                        .to_string(),
                );
            }
            if rest.starts_with('!') {
                return Err(
                    "lookaround assertion '(?!...)' is not supported in the portable regex subset"
                        .to_string(),
                );
            }
            if rest.starts_with("<=") {
                return Err(
                    "lookaround assertion '(?<=...)' is not supported in the portable regex subset"
                        .to_string(),
                );
            }
            if rest.starts_with("<!") {
                return Err(
                    "lookaround assertion '(?<!...)' is not supported in the portable regex subset"
                        .to_string(),
                );
            }
            if rest.starts_with("P<") {
                return Err(
                    "named capture group '(?P<name>...)' is not supported in the portable regex subset; use an unnamed '(...)' or non-capturing '(?:...)' group"
                        .to_string(),
                );
            }
            if rest.starts_with("P=") {
                return Err(
                    "named backreference '(?P=name)' is not supported in the portable regex subset; use an unnamed '(...)' or non-capturing '(?:...)' group"
                        .to_string(),
                );
            }
            if rest.starts_with('<') {
                return Err(
                    "named capture group '(?<name>...)' is not supported in the portable regex subset; use an unnamed '(...)' or non-capturing '(?:...)' group"
                        .to_string(),
                );
            }
        }

        let ch_len = pattern[i..].chars().next().map_or(1, char::len_utf8);
        i += ch_len;
    }

    Ok(())
}

fn check_hir_no_named_captures(hir: &Hir) -> Result<(), String> {
    use regex_syntax::hir::HirKind;
    match hir.kind() {
        HirKind::Capture(cap) => {
            if let Some(ref name) = cap.name {
                return Err(format!(
                    "named capture group '{name}' is not supported in the portable regex subset; use an unnamed '(...)' or non-capturing '(?:...)' group"
                ));
            }
            check_hir_no_named_captures(&cap.sub)
        }
        HirKind::Repetition(rep) => check_hir_no_named_captures(&rep.sub),
        HirKind::Concat(subs) | HirKind::Alternation(subs) => {
            for sub in subs {
                check_hir_no_named_captures(sub)?;
            }
            Ok(())
        }
        HirKind::Empty | HirKind::Literal(_) | HirKind::Class(_) | HirKind::Look(_) => Ok(()),
    }
}

/// Extracts leading inline flags `(?imsux...)` from `pattern`, returning
/// `(stripped_pattern, ignore_case, dotall, verbose)`.
pub fn extract_inline_flags(pattern: &str) -> (String, bool, bool, bool) {
    let Some(rest) = pattern.strip_prefix("(?") else {
        return (pattern.to_owned(), false, false, false);
    };
    let Some(close_idx) = rest.find(')') else {
        return (pattern.to_owned(), false, false, false);
    };
    let flags_str = &rest[..close_idx];
    if flags_str.is_empty() || !flags_str.chars().all(|c| c.is_ascii_lowercase()) {
        return (pattern.to_owned(), false, false, false);
    }

    let ignore_case = flags_str.contains('i');
    let dotall = flags_str.contains('s');
    let verbose = flags_str.contains('x');
    let remaining: String = flags_str
        .chars()
        .filter(|&c| !matches!(c, 'i' | 's' | 'x'))
        .collect();

    let body = &rest[close_idx + 1..];
    let prefix = if remaining.is_empty() {
        String::new()
    } else {
        format!("(?{remaining})")
    };
    (format!("{prefix}{body}"), ignore_case, dotall, verbose)
}

/// Strips verbose (`(?x)`) unescaped whitespace and `#` comments from `pattern`.
pub fn unverbosify_regex(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut in_char_class = false;

    while i < n {
        let ch = chars[i];
        if ch == '\\' {
            if i + 1 < n {
                let nxt = chars[i + 1];
                if !in_char_class && (nxt.is_whitespace() || nxt == '#') {
                    out.push(nxt);
                } else {
                    out.push(ch);
                    out.push(nxt);
                }
                i += 2;
            } else {
                out.push(ch);
                i += 1;
            }
            continue;
        }

        if ch == '[' && !in_char_class {
            in_char_class = true;
            out.push(ch);
            i += 1;
            if i < n && chars[i] == '^' {
                out.push(chars[i]);
                i += 1;
            }
            if i < n && chars[i] == ']' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if ch == ']' && in_char_class {
            in_char_class = false;
            out.push(ch);
            i += 1;
            continue;
        }

        if !in_char_class {
            if ch == '#' {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if ch.is_whitespace() {
                i += 1;
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    out
}

/// Normalizes a regex pattern into a single-line pattern with leading `(?is)` inline flags
/// and verbose `(?x)` whitespace/comments stripped (matching `ssc_codegen.regex_utils.normalize_regex_pattern`).
pub fn normalize_regex_pattern(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let (mut pattern, ignore_case, dotall, verbose) = extract_inline_flags(trimmed);
    if verbose {
        pattern = unverbosify_regex(&pattern);
    }

    let mut flags = String::new();
    if ignore_case {
        flags.push('i');
    }
    if dotall {
        flags.push('s');
    }
    if flags.is_empty() {
        pattern
    } else {
        format!("(?{flags}){pattern}")
    }
}

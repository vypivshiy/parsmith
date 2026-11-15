//! Standard runtime helper definitions and JS/TS literal/regex formatting helpers.

use std::fmt::Write as _;

use crate::ast::{CondValue, LiteralValue};
use crate::codegen::writer::{CodeWriter, wln};
use crate::lowering::clean_doc;
use crate::validators::unverbosify_regex;

pub use super::rest::{
    STD_FETCH_DTS, STD_FETCH_JS, STD_FETCH_KEY, STD_FETCH_TS, STD_HTTP_TRANSPORT_DTS,
    STD_HTTP_TRANSPORT_JS, STD_HTTP_TRANSPORT_KEY, STD_HTTP_TRANSPORT_TS, STD_REST_CORE_DTS,
    STD_REST_CORE_JS, STD_REST_CORE_KEY, STD_REST_CORE_TS,
};

pub const STD_PARSE_HTML_TS: &str = r#"
export function rtParseHtml(document: string | Document | Element): Document | Element {
  if (typeof document === "string") {
    return new DOMParser().parseFromString(document, "text/html");
  }
  return document;
}
"#;

pub const STD_PARSE_HTML_JS: &str = r#"
export function rtParseHtml(document) {
  if (typeof document === "string") {
    return new DOMParser().parseFromString(document, "text/html");
  }
  return document;
}
"#;

pub const STD_CSS_REMOVE_TS: &str = r#"
export function rtCssRemove<T extends Document | Element>(root: T, query: string): T {
  root.querySelectorAll(query).forEach((el) => el.remove());
  return root;
}
"#;

pub const STD_CSS_REMOVE_JS: &str = r#"
export function rtCssRemove(root, query) {
  root.querySelectorAll(query).forEach((el) => el.remove());
  return root;
}
"#;

pub const STD_XPATH_FIRST_TS: &str = r#"
export function rtXpathFirst(root: Document | Element, ...queries: string[]): Element | null {
  const doc = root.ownerDocument ?? (root as Document);
  for (const q of queries) {
    const res = doc.evaluate(q, root, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
    if (res.singleNodeValue !== null) {
      return res.singleNodeValue as Element;
    }
  }
  return null;
}
"#;

pub const STD_XPATH_FIRST_JS: &str = r#"
export function rtXpathFirst(root, ...queries) {
  const doc = root.ownerDocument ?? root;
  for (const q of queries) {
    const res = doc.evaluate(q, root, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
    if (res.singleNodeValue !== null) {
      return res.singleNodeValue;
    }
  }
  return null;
}
"#;

pub const STD_XPATH_ALL_TS: &str = r#"
export function rtXpathAll(root: Document | Element, ...queries: string[]): Element[] {
  const doc = root.ownerDocument ?? (root as Document);
  for (const q of queries) {
    const res = doc.evaluate(q, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
    if (res.snapshotLength > 0) {
      const out: Element[] = [];
      for (let i = 0; i < res.snapshotLength; i++) {
        const item = res.snapshotItem(i);
        if (item !== null) out.push(item as Element);
      }
      return out;
    }
  }
  return [];
}
"#;

pub const STD_XPATH_ALL_JS: &str = r#"
export function rtXpathAll(root, ...queries) {
  const doc = root.ownerDocument ?? root;
  for (const q of queries) {
    const res = doc.evaluate(q, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
    if (res.snapshotLength > 0) {
      const out = [];
      for (let i = 0; i < res.snapshotLength; i++) {
        const item = res.snapshotItem(i);
        if (item !== null) out.push(item);
      }
      return out;
    }
  }
  return [];
}
"#;

pub const STD_XPATH_REMOVE_TS: &str = r#"
export function rtXpathRemove<T extends Document | Element>(root: T, query: string): T {
  const doc = root.ownerDocument ?? (root as Document);
  const snap = doc.evaluate(query, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
  for (let i = snap.snapshotLength - 1; i >= 0; i--) {
    const node = snap.snapshotItem(i);
    if (node !== null && "remove" in node && typeof (node as Element).remove === "function") {
      (node as Element).remove();
    } else if (node !== null && node.parentNode !== null) {
      node.parentNode.removeChild(node);
    }
  }
  return root;
}
"#;

pub const STD_XPATH_REMOVE_JS: &str = r#"
export function rtXpathRemove(root, query) {
  const doc = root.ownerDocument ?? root;
  const snap = doc.evaluate(query, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
  for (let i = snap.snapshotLength - 1; i >= 0; i--) {
    const node = snap.snapshotItem(i);
    if (node !== null && typeof node.remove === "function") {
      node.remove();
    } else if (node !== null && node.parentNode !== null) {
      node.parentNode.removeChild(node);
    }
  }
  return root;
}
"#;

pub const STD_ASSERT_TS: &str = r#"
export class RtAssertionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RtAssertionError";
  }
}

export function rtAssert(cond: boolean, msg: string = ""): void {
  if (!cond) {
    throw new RtAssertionError(msg || "parsmith assertion failed");
  }
}
"#;

pub const STD_ASSERT_JS: &str = r#"
export class RtAssertionError extends Error {
  constructor(message) {
    super(message);
    this.name = "RtAssertionError";
  }
}

export function rtAssert(cond, msg = "") {
  if (!cond) {
    throw new RtAssertionError(msg || "parsmith assertion failed");
  }
}
"#;

pub const STD_RE_SEARCH_TS: &str = r#"
export class RtRegexError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RtRegexError";
  }
}

export function rtReSearch(pattern: RegExp, value: string, msg: string = ""): string {
  const m = value.match(pattern);
  if (m === null || m[1] === undefined) {
    throw new RtRegexError(msg || "parsmith re-match failed");
  }
  return m[1];
}
"#;

pub const STD_RE_SEARCH_JS: &str = r#"
export class RtRegexError extends Error {
  constructor(message) {
    super(message);
    this.name = "RtRegexError";
  }
}

export function rtReSearch(pattern, value, msg = "") {
  const m = value.match(pattern);
  if (m === null || m[1] === undefined) {
    throw new RtRegexError(msg || "parsmith re-match failed");
  }
  return m[1];
}
"#;

pub const STD_TRIM_CHARS_TS: &str = r#"
export function rtTrimChars(s: string, chars: string): string {
  const esc = chars.replace(/[\\\]^-]/g, "\\$&");
  return s.replace(new RegExp(`^[${esc}]+|[${esc}]+$`, "g"), "");
}
"#;

pub const STD_TRIM_CHARS_JS: &str = r#"
export function rtTrimChars(s, chars) {
  const esc = chars.replace(/[\\\]^-]/g, "\\$&");
  return s.replace(new RegExp(`^[${esc}]+|[${esc}]+$`, "g"), "");
}
"#;

pub const STD_LTRIM_CHARS_TS: &str = r#"
export function rtLtrimChars(s: string, chars: string): string {
  const esc = chars.replace(/[\\\]^-]/g, "\\$&");
  return s.replace(new RegExp(`^[${esc}]+`, "g"), "");
}
"#;

pub const STD_LTRIM_CHARS_JS: &str = r#"
export function rtLtrimChars(s, chars) {
  const esc = chars.replace(/[\\\]^-]/g, "\\$&");
  return s.replace(new RegExp(`^[${esc}]+`, "g"), "");
}
"#;

pub const STD_RTRIM_CHARS_TS: &str = r#"
export function rtRtrimChars(s: string, chars: string): string {
  const esc = chars.replace(/[\\\]^-]/g, "\\$&");
  return s.replace(new RegExp(`[${esc}]+$`, "g"), "");
}
"#;

pub const STD_RTRIM_CHARS_JS: &str = r#"
export function rtRtrimChars(s, chars) {
  const esc = chars.replace(/[\\\]^-]/g, "\\$&");
  return s.replace(new RegExp(`[${esc}]+$`, "g"), "");
}
"#;

pub const STD_RM_PREFIX_TS: &str = r#"
export function rtRmPrefix(s: string, p: string): string {
  return p && s.startsWith(p) ? s.slice(p.length) : s;
}
"#;

pub const STD_RM_PREFIX_JS: &str = r#"
export function rtRmPrefix(s, p) {
  return p && s.startsWith(p) ? s.slice(p.length) : s;
}
"#;

pub const STD_RM_SUFFIX_TS: &str = r#"
export function rtRmSuffix(s: string, p: string): string {
  return p && s.endsWith(p) ? s.slice(0, -p.length) : s;
}
"#;

pub const STD_RM_SUFFIX_JS: &str = r#"
export function rtRmSuffix(s, p) {
  return p && s.endsWith(p) ? s.slice(0, -p.length) : s;
}
"#;

pub const STD_REPL_MAP_TS: &str = r#"
export function rtReplMap(s: string, map: Record<string, string>): string {
  for (const [k, v] of Object.entries(map)) {
    s = s.split(k).join(v);
  }
  return s;
}
"#;

pub const STD_REPL_MAP_JS: &str = r#"
export function rtReplMap(s, map) {
  for (const [k, v] of Object.entries(map)) {
    s = s.split(k).join(v);
  }
  return s;
}
"#;

pub const STD_NORMALIZE_TEXT_TS: &str = r#"
export function rtNormalizeText(s: string): string {
  return s ? s.trim().replace(/\s+/g, " ") : "";
}
"#;

pub const STD_NORMALIZE_TEXT_JS: &str = r#"
export function rtNormalizeText(s) {
  return s ? s.trim().replace(/\s+/g, " ") : "";
}
"#;

pub const STD_UNESCAPE_TS: &str = r#"
export function rtUnescapeText(s: string): string {
  let out = s;
  if (typeof document !== "undefined" && typeof document.createElement === "function") {
    const el = document.createElement("textarea");
    el.innerHTML = out;
    out = el.value;
  } else {
    out = out
      .replace(/&quot;/g, '"')
      .replace(/&#39;|&apos;/g, "'")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&nbsp;/g, "\u00a0")
      .replace(/&amp;/g, "&");
  }
  return out
    .replace(/&#x([0-9a-fA-F]+);/g, (_, hex: string) => String.fromCodePoint(parseInt(hex, 16)))
    .replace(/&#(\d+);/g, (_, dec: string) => String.fromCodePoint(parseInt(dec, 10)))
    .replace(/\\u([0-9a-fA-F]{4})/g, (_, hex: string) => String.fromCharCode(parseInt(hex, 16)))
    .replace(/\\x([0-9a-fA-F]{2})/g, (_, hex: string) => String.fromCharCode(parseInt(hex, 16)))
    .replace(/\\b/g, "\b")
    .replace(/\\f/g, "\f")
    .replace(/\\n/g, "\n")
    .replace(/\\r/g, "\r")
    .replace(/\\t/g, "\t");
}
"#;

pub const STD_UNESCAPE_JS: &str = r#"
export function rtUnescapeText(s) {
  let out = s;
  if (typeof document !== "undefined" && typeof document.createElement === "function") {
    const el = document.createElement("textarea");
    el.innerHTML = out;
    out = el.value;
  } else {
    out = out
      .replace(/&quot;/g, '"')
      .replace(/&#39;|&apos;/g, "'")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&nbsp;/g, "\u00a0")
      .replace(/&amp;/g, "&");
  }
  return out
    .replace(/&#x([0-9a-fA-F]+);/g, (_, hex) => String.fromCodePoint(parseInt(hex, 16)))
    .replace(/&#(\d+);/g, (_, dec) => String.fromCodePoint(parseInt(dec, 10)))
    .replace(/\\u([0-9a-fA-F]{4})/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)))
    .replace(/\\x([0-9a-fA-F]{2})/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)))
    .replace(/\\b/g, "\b")
    .replace(/\\f/g, "\f")
    .replace(/\\n/g, "\n")
    .replace(/\\r/g, "\r")
    .replace(/\\t/g, "\t");
}
"#;

pub const STD_UNMATCHED_TABLE_ROW: &str = r#"
export const RT_UNMATCHED_TABLE_ROW = Symbol("RT_UNMATCHED_TABLE_ROW");
"#;

pub const STD_JSON_PROJECT_TS: &str = include_str!("runtime/json_project.ts");

pub const STD_JSON_PROJECT_JS: &str = include_str!("runtime/json_project.js");

/// Checks whether a string is a valid unquoted JavaScript property identifier (`^[A-Za-z_$][A-Za-z0-9_$]*$`).
pub fn is_js_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Formats a Rust `&str` as a valid JSON/JavaScript double-quoted string literal (`"..."`).
pub fn js_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Formats a slice of strings as a JavaScript array literal (`["a", "b"]`).
pub fn js_str_array(items: &[String]) -> String {
    let inner = items
        .iter()
        .map(|s| js_str(s))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}

/// Formats a [`LiteralValue`] as a JavaScript/TypeScript literal expression.
pub fn js_literal(lit: &LiteralValue) -> String {
    match lit {
        LiteralValue::Null => "null".to_owned(),
        LiteralValue::Bool(true) => "true".to_owned(),
        LiteralValue::Bool(false) => "false".to_owned(),
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Float(f) => format!("{f:?}"),
        LiteralValue::String(s) => js_str(s),
        LiteralValue::EmptyArray => "[]".to_owned(),
    }
}

/// Formats a [`CondValue`] as a JavaScript/TypeScript literal expression.
pub fn js_pred_value(val: &CondValue) -> String {
    match val {
        CondValue::Int(n) => n.to_string(),
        CondValue::Float(f) => format!("{f:?}"),
        CondValue::Bool(true) => "true".to_owned(),
        CondValue::Bool(false) => "false".to_owned(),
        CondValue::String(s) => js_str(s),
    }
}

/// Normalizes a portable DSL regex pattern (including inline flags `(?imsx)`)
/// into a valid JavaScript regex literal (`/pattern/flags`).
pub fn js_regex_literal(raw_pattern: &str, global_flag: bool) -> String {
    let mut current = raw_pattern.trim_start().to_owned();
    let mut ignore_case = false;
    let mut multiline = false;
    let mut dotall = false;
    let mut verbose = false;

    while let Some(rest) = current.strip_prefix("(?") {
        let Some(close_idx) = rest.find(')') else {
            break;
        };
        let flags_part = &rest[..close_idx];
        if flags_part.is_empty() || !flags_part.chars().all(|c| c.is_ascii_lowercase()) {
            break;
        }
        if flags_part.contains('i') {
            ignore_case = true;
        }
        if flags_part.contains('m') {
            multiline = true;
        }
        if flags_part.contains('s') {
            dotall = true;
        }
        if flags_part.contains('x') {
            verbose = true;
        }
        current = rest[close_idx + 1..].to_owned();
    }

    if verbose {
        current = unverbosify_regex(&current);
    }

    let escaped = escape_js_regex_slashes(&current);
    let body = if escaped.is_empty() {
        "(?:)".to_owned()
    } else {
        escaped
    };

    let mut flags = String::new();
    if global_flag {
        flags.push('g');
    }
    if ignore_case {
        flags.push('i');
    }
    if multiline {
        flags.push('m');
    }
    if dotall {
        flags.push('s');
    }

    format!("/{body}/{flags}")
}

fn escape_js_regex_slashes(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 4);
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut in_char_class = false;

    while i < n {
        let ch = chars[i];
        if ch == '\\' {
            if i + 1 < n {
                out.push('\\');
                out.push(chars[i + 1]);
                i += 2;
            } else {
                out.push('\\');
                i += 1;
            }
            continue;
        }
        if ch == '[' && !in_char_class {
            in_char_class = true;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == ']' && in_char_class {
            in_char_class = false;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '/' && !in_char_class {
            out.push_str("\\/");
            i += 1;
            continue;
        }
        if ch == '\n' {
            out.push_str("\\n");
            i += 1;
            continue;
        }
        if ch == '\r' {
            out.push_str("\\r");
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Writes a cleaned JSDoc comment block (`/** ... */`) to `writer`.
pub fn emit_jsdoc(doc: &str, writer: &mut CodeWriter) {
    let cleaned = clean_doc(doc);
    if cleaned.is_empty() {
        return;
    }
    writer.writeln("/**");
    for line in cleaned.lines() {
        let safe = line.replace("*/", "*\\/");
        if safe.trim().is_empty() {
            writer.writeln(" *");
        } else {
            wln!(writer, " * {safe}");
        }
    }
    writer.writeln(" */");
}

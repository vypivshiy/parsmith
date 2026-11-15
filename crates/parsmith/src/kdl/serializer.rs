use crate::kdl::document::{KdlDocument, KdlNode, NodeId};
use crate::kdl::selector::unescape_selector_str;
use crate::kdl::value::{KdlArg, KdlProp, KdlValue};

/// Reserved bare identifiers in KDL 2.0 (§3.10).
const RESERVED_BARE_IDENTS: [&str; 6] = ["true", "false", "null", "inf", "-inf", "nan"];

/// Normalizes a type annotation input string by stripping optional outer `(` and `)` delimiters.
pub fn normalize_ty_input(raw: &str) -> String {
    let mut s = raw.trim();
    while s.len() >= 2 && s.starts_with('(') && s.ends_with(')') {
        s = s[1..s.len() - 1].trim();
    }
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        if let Ok(ident) = s.parse::<kdl::KdlIdentifier>() {
            return ident.value().to_string();
        }
        return unescape_selector_str(&s[1..s.len() - 1]);
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return unescape_selector_str(&s[1..s.len() - 1]);
    }
    s.to_string()
}

/// Returns `true` if `ch` is disallowed in a KDL 2.0 bare identifier (§3.10).
#[inline]
pub fn is_disallowed_ident_char(ch: char) -> bool {
    matches!(
        ch,
        '\\'
            | '/'
            | '('
            | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | ';'
            | '"'
            | '#'
            | '='
            | '\u{0000}'..='\u{0020}'
            | '\u{007F}'..='\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{2028}'..='\u{202F}'
            | '\u{205F}'
            | '\u{2066}'..='\u{2069}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// Returns `true` if `s` can be emitted as an unquoted KDL 2.0 identifier string (§3.10).
pub fn is_bare_ident(s: &str) -> bool {
    if s.is_empty() || RESERVED_BARE_IDENTS.contains(&s) {
        return false;
    }

    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if first.is_ascii_digit() {
        return false;
    }

    let second = chars.next();
    if first == '.' && second.is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    if matches!(first, '+' | '-') {
        if second.is_some_and(|c| c.is_ascii_digit()) {
            return false;
        }
        if second == Some('.') && chars.next().is_some_and(|c| c.is_ascii_digit()) {
            return false;
        }
    }

    !s.chars().any(is_disallowed_ident_char)
}

/// Returns `true` if `ch` must be escaped inside a KDL 2.0 quoted string.
#[inline]
fn needs_escape(ch: char) -> bool {
    matches!(
        ch,
        '\\'
            | '"'
            | '\u{0000}'..='\u{001F}'
            | '\u{007F}'..='\u{009F}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Formats `s` as a single-line KDL 2.0 quoted string (`"..."`) with spec-compliant escapes.
pub fn escape_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if needs_escape(c) => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{{{:X}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Formats a string as a bare KDL 2.0 identifier if valid, or a quoted string otherwise.
pub fn format_ident(s: &str) -> String {
    if is_bare_ident(s) {
        s.to_owned()
    } else {
        escape_quoted(s)
    }
}

/// Formats a type annotation identifier as `(type)` (quoting the inner type name if required).
pub fn format_ty(ty: &str) -> String {
    let norm = normalize_ty_input(ty);
    format!("({})", format_ident(&norm))
}

/// Serializes a [`KdlValue`] to KDL 2.0 text.
pub fn value_to_kdl(value: &KdlValue) -> String {
    match value {
        KdlValue::Null => "#null".to_owned(),
        KdlValue::Bool(true) => "#true".to_owned(),
        KdlValue::Bool(false) => "#false".to_owned(),
        KdlValue::Int(i) => i.to_string(),
        KdlValue::Float(f) => {
            if f.is_nan() {
                "#nan".to_owned()
            } else if f.is_infinite() {
                if f.is_sign_positive() {
                    "#inf".to_owned()
                } else {
                    "#-inf".to_owned()
                }
            } else {
                format!("{f:?}")
            }
        }
        KdlValue::String(s) => format_ident(s),
    }
}

/// Serializes a positional [`KdlArg`] to KDL 2.0 text.
pub fn arg_to_kdl(arg: &KdlArg) -> String {
    let val = value_to_kdl(&arg.value);
    match &arg.ty {
        Some(ty) => format!("{}{val}", format_ty(ty)),
        None => val,
    }
}

/// Serializes a named [`KdlProp`] to KDL 2.0 text (`key=val` or `key=(type)val`).
pub fn prop_to_kdl(prop: &KdlProp) -> String {
    let key = format_ident(&prop.key);
    let val = match &prop.value {
        KdlValue::String(s) => escape_quoted(s),
        other => value_to_kdl(other),
    };
    match &prop.ty {
        Some(ty) => format!("{key}={}{val}", format_ty(ty)),
        None => format!("{key}={val}"),
    }
}

/// Formats the header of a node (`(type)name arg1 arg2 key1=val1`).
pub(crate) fn format_node_head(node: &KdlNode) -> String {
    let mut head = String::new();
    if let Some(ty) = &node.ty {
        head.push_str(&format_ty(ty));
    }
    head.push_str(&format_ident(&node.name));

    for arg in &node.args {
        head.push(' ');
        head.push_str(&arg_to_kdl(arg));
    }

    for prop in &node.props {
        head.push(' ');
        head.push_str(&prop_to_kdl(prop));
    }

    head
}

/// Serializes a standalone [`KdlNode`] (without document children) to KDL 2.0 text.
pub fn standalone_node_to_kdl(
    node: &KdlNode,
    indent: usize,
    indent_str: &str,
    force_children_block: bool,
) -> String {
    let head = format_node_head(node);
    let comment_suffix = node
        .trailing_comment
        .as_deref()
        .map_or(String::new(), |c| format!(" // {c}"));

    if node.empty_children_multiline {
        let closer = indent_str.repeat(indent);
        return format!("{head} {{{comment_suffix}\n{closer}}}");
    }
    if force_children_block {
        return format!("{head} {{}}{comment_suffix}");
    }
    format!("{head}{comment_suffix}")
}

/// Serializes a node and its subtree inside a [`KdlDocument`] to KDL 2.0 text.
pub fn doc_node_to_kdl(
    doc: &KdlDocument,
    id: NodeId,
    indent: usize,
    indent_str: &str,
    force_children_block: bool,
) -> String {
    let node = &doc[id];
    if node.children.is_empty() {
        return standalone_node_to_kdl(node, indent, indent_str, force_children_block);
    }

    let head = format_node_head(node);
    let comment_suffix = node
        .trailing_comment
        .as_deref()
        .map_or(String::new(), |c| format!(" // {c}"));

    let inner_indent = indent_str.repeat(indent + 1);
    let closer_indent = indent_str.repeat(indent);
    let mut body_lines = Vec::with_capacity(node.children.len());
    for &child_id in &node.children {
        let rendered_child = doc_node_to_kdl(doc, child_id, indent + 1, indent_str, false);
        body_lines.push(format!("{inner_indent}{rendered_child}"));
    }

    format!(
        "{head} {{{comment_suffix}\n{}\n{closer_indent}}}",
        body_lines.join("\n")
    )
}

/// Serializes an entire [`KdlDocument`] to KDL 2.0 text with a trailing newline (or `""` if empty).
pub fn doc_to_kdl(doc: &KdlDocument, indent_str: &str) -> String {
    if doc.root_ids().is_empty() {
        return String::new();
    }
    let mut lines = Vec::with_capacity(doc.root_ids().len());
    for &root_id in doc.root_ids() {
        lines.push(doc_node_to_kdl(doc, root_id, 0, indent_str, false));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

//! `@request` (`curl` and raw HTTP/1.1|2) and `@error[:<suffix>]` parser, validator, and AST specifications.
//!
//! Matches `ssc_codegen/parsers/curl.py`, `ssc_codegen/parsers/http.py`, `ssc_codegen/request_spec.py`,
//! `ssc_codegen/ast/struct.py`, and `ssc_codegen/core/rest_artifacts.py`.

use std::fmt;

use crate::kdl::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::json_to_kdl::to_pascal_case;

/// Prefix signatures indicating that a stray struct field name is actually a leaked HTTP `@request` line
/// caused by a `//` comment breaking a `\` line continuation.
pub const HTTP_REQUEST_SIGNATURES: &[&str] = &[
    "curl ", "http://", "https://", "GET ", "POST ", "PUT ", "DELETE ", "PATCH ", "HEAD ",
    "OPTIONS ", "-H ", "-d ", "-X ", "--",
];

/// Canonical diagnostic hint emitted when a `//` comment breaks a KDL `\` line continuation in `@request`.
pub const REQUEST_LINE_CONTINUATION_HINT: &str = "KDL terminates line continuations ('\\') at single-line '//' comments; move comments outside multi-line '@request' continuations or escape them";

/// Returns `true` if `s` starts with any [`HTTP_REQUEST_SIGNATURES`] prefix.
pub fn starts_with_http_signature(s: &str) -> bool {
    HTTP_REQUEST_SIGNATURES
        .iter()
        .any(|prefix| s.starts_with(prefix))
}

/// Returns `true` if `payload` contains a `\` line continuation followed by a `//` comment.
pub fn has_backslash_line_comment(payload: &str) -> bool {
    let bytes = payload.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if bytes[i] == b'\\' {
            let mut j = i + 1;
            while j < n && matches!(bytes[j], b' ' | b'\t') {
                j += 1;
            }
            if j + 1 < n && bytes[j] == b'/' && bytes[j + 1] == b'/' {
                return true;
            }
            if j < n && bytes[j] == b'\r' {
                j += 1;
            }
            if j < n && bytes[j] == b'\n' {
                j += 1;
                while j < n && matches!(bytes[j], b' ' | b'\t') {
                    j += 1;
                }
                if j + 1 < n && bytes[j] == b'/' && bytes[j + 1] == b'/' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Scalar type constraint of a `@request` parameter placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceholderType {
    /// `"str"` (default).
    #[default]
    Str,
    /// `"int"`.
    Int,
    /// `"float"`.
    Float,
    /// `"bool"`.
    Bool,
}

impl PlaceholderType {
    /// Returns the canonical DSL type string (`"str"`, `"int"`, `"float"`, `"bool"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Str => "str",
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        match s {
            "str" => Some(Self::Str),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            _ => None,
        }
    }
}

impl fmt::Display for PlaceholderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Array serialization style for query parameter placeholders (`repeat`, `csv`, `bracket`, `pipe`, `space`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceholderStyle {
    /// `?k=1&k=2` (default for arrays).
    Repeat,
    /// `?k=1,2`.
    Csv,
    /// `?k[]=1&k[]=2`.
    Bracket,
    /// `?k=1|2`.
    Pipe,
    /// `?k=1%202`.
    Space,
}

impl PlaceholderStyle {
    /// Returns the canonical DSL style name (`"repeat"`, `"csv"`, `"bracket"`, `"pipe"`, `"space"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Repeat => "repeat",
            Self::Csv => "csv",
            Self::Bracket => "bracket",
            Self::Pipe => "pipe",
            Self::Space => "space",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        match s {
            "repeat" => Some(Self::Repeat),
            "csv" => Some(Self::Csv),
            "bracket" => Some(Self::Bracket),
            "pipe" => Some(Self::Pipe),
            "space" => Some(Self::Space),
            _ => None,
        }
    }
}

impl fmt::Display for PlaceholderStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parsed parameter placeholder specification (`{{name:type[]?|style}}` or `{name:type[]?:style}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PlaceholderSpec {
    /// Parameter name (`[A-Za-z][A-Za-z0-9_-]*`).
    pub name: String,
    /// Scalar type (`str`, `int`, `float`, `bool`).
    pub type_name: PlaceholderType,
    /// Whether the placeholder accepts an array of values (`[]`).
    pub is_array: bool,
    /// Whether the placeholder is optional (`?`).
    pub is_optional: bool,
    /// Optional serialization style for array parameters.
    pub style: Option<PlaceholderStyle>,
}

impl PlaceholderSpec {
    /// Parses a single placeholder token (either `{{...}}` or `{...}`).
    pub fn parse(text: &str) -> Option<Self> {
        let inner = if let Some(s) = text.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
            s
        } else {
            let s = text.strip_prefix('{').and_then(|s| s.strip_suffix('}'))?;
            if s.starts_with('{') || s.ends_with('}') {
                return None;
            }
            s
        };

        Self::parse_inner(inner)
    }

    /// Parses the inner content of a placeholder token (`NAME(:PRIM)?([])?(?)?([|:]STYLE)?`).
    pub fn parse_inner(inner: &str) -> Option<Self> {
        if inner.is_empty() {
            return None;
        }

        // 1. Parse NAME = [A-Za-z][A-Za-z0-9_-]*
        let mut chars = inner.char_indices();
        let (_, first) = chars.next()?;
        if !first.is_ascii_alphabetic() {
            return None;
        }

        let mut name_end = first.len_utf8();
        for (idx, c) in chars {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                name_end = idx + c.len_utf8();
            } else {
                break;
            }
        }

        let name = &inner[..name_end];
        let mut rest = &inner[name_end..];

        // 2. Optional `:PRIM` where PRIM = str | int | float | bool
        let mut type_name = PlaceholderType::Str;
        if let Some(after_colon) = rest.strip_prefix(':') {
            let prim_end = after_colon
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(after_colon.len());
            let prim_str = &after_colon[..prim_end];
            type_name = PlaceholderType::from_name(prim_str)?;
            rest = &after_colon[prim_end..];
        }

        // 3. Optional `[]`
        let is_array = if let Some(after_arr) = rest.strip_prefix("[]") {
            rest = after_arr;
            true
        } else {
            false
        };

        // 4. Optional `?`
        let is_optional = if let Some(after_opt) = rest.strip_prefix('?') {
            rest = after_opt;
            true
        } else {
            false
        };

        // 5. Optional `|style` or `:style` (arrays only)
        let style =
            if let Some(style_str) = rest.strip_prefix('|').or_else(|| rest.strip_prefix(':')) {
                if !is_array {
                    return None;
                }
                let st = PlaceholderStyle::from_name(style_str)?;
                rest = "";
                Some(st)
            } else {
                None
            };

        if !rest.is_empty() {
            return None;
        }

        Some(Self {
            name: name.to_owned(),
            type_name,
            is_array,
            is_optional,
            style,
        })
    }

    /// Checks whether `text` contains at least one valid placeholder token.
    pub fn search(text: &str) -> bool {
        let mut i = 0;
        let bytes = text.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'{' && Self::match_at(text, i).is_some() {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Attempts to match a valid placeholder token starting at byte offset `pos`.
    pub fn match_at(text: &str, pos: usize) -> Option<(usize, Self)> {
        let rest = text.get(pos..)?;
        if let Some(after_open) = rest.strip_prefix("{{") {
            let close_rel = after_open.find("}}")?;
            let token_len = 2 + close_rel + 2;
            let token = &rest[..token_len];
            let spec = Self::parse(token)?;
            return Some((pos + token_len, spec));
        }
        if let Some(after_open) = rest.strip_prefix('{') {
            if after_open.starts_with('{') {
                return None;
            }
            let close_rel = after_open.find('}')?;
            let inner = &after_open[..close_rel];
            if inner.is_empty()
                || inner.contains('{')
                || inner.contains('"')
                || inner.chars().any(char::is_whitespace)
            {
                return None;
            }
            // Do not match if followed immediately by another `}` (part of broken `{{...}}`)
            if after_open
                .get(close_rel + 1..)
                .is_some_and(|s| s.starts_with('}'))
            {
                return None;
            }
            let token_len = 1 + close_rel + 1;
            let spec = Self::parse_inner(inner)?;
            return Some((pos + token_len, spec));
        }
        None
    }

    /// Replaces all valid placeholder tokens in `text` with `replacement`.
    pub fn sub(text: &str, replacement: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < text.len() {
            if let Some((end, _)) = Self::match_at(text, i) {
                out.push_str(replacement);
                i = end;
            } else {
                let ch = text[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        out
    }

    /// Reconstructs the canonical `{{...}}` token string.
    pub fn to_token(&self) -> String {
        let mut s = format!("{{{{{}", self.name);
        if self.type_name != PlaceholderType::Str {
            s.push(':');
            s.push_str(self.type_name.as_str());
        }
        if self.is_array {
            s.push_str("[]");
        }
        if self.is_optional {
            s.push('?');
        }
        if let Some(style) = self.style {
            s.push('|');
            s.push_str(style.as_str());
        }
        s.push_str("}}");
        s
    }

    /// Returns references to `placeholders` stably ordered with required parameters before optional parameters.
    pub fn sort_required_first(placeholders: &[PlaceholderSpec]) -> Vec<&PlaceholderSpec> {
        let mut sorted: Vec<&PlaceholderSpec> = placeholders.iter().collect();
        sorted.sort_by_key(|p| p.is_optional);
        sorted
    }

    /// Returns the delimiter string (`","`, `"|"`, or `" "`) when this placeholder is an array
    /// with `Csv`, `Pipe`, or `Space` serialization style.
    pub const fn array_join_sep(&self) -> Option<&'static str> {
        if !self.is_array {
            return None;
        }
        match self.style {
            Some(PlaceholderStyle::Csv) => Some(","),
            Some(PlaceholderStyle::Pipe) => Some("|"),
            Some(PlaceholderStyle::Space) => Some(" "),
            _ => None,
        }
    }
}

/// Scans `text` for all candidate placeholder tokens (`{{...}}` and single-brace `{...}` candidate tokens).
///
/// Used by the linter to catch malformed placeholders (such as `{{}}`, `{{_bad}}`, `{_bad}`, `{foo:unknown}`).
pub fn find_placeholder_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    while i < n {
        if bytes[i] == b'{' {
            if text[i..].starts_with("{{") {
                let after = &text[i + 2..];
                if let Some(close_rel) = after.find("}}") {
                    if !after[..close_rel].contains('{') && !after[..close_rel].contains('}') {
                        let end = i + 2 + close_rel + 2;
                        tokens.push(&text[i..end]);
                        i = end;
                        continue;
                    }
                }
            } else {
                let after = &text[i + 1..];
                if let Some(close_rel) = after.find('}') {
                    let inner = &after[..close_rel];
                    let followed_by_brace = after
                        .get(close_rel + 1..)
                        .is_some_and(|s| s.starts_with('}'));
                    if !followed_by_brace
                        && !inner.is_empty()
                        && !inner.contains('{')
                        && !inner.contains('"')
                        && !inner.contains('\'')
                        && !inner.chars().any(char::is_whitespace)
                    {
                        let end = i + 1 + close_rel + 1;
                        tokens.push(&text[i..end]);
                        i = end;
                        continue;
                    }
                }
            }
        }
        let ch = text[i..].chars().next().unwrap();
        i += ch.len_utf8();
    }

    tokens
}

/// A segment of a [`PlaceholderTemplate`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaceholderPart {
    /// Literal string segment.
    Literal(String),
    /// Parsed placeholder specification.
    Placeholder(PlaceholderSpec),
}

/// Tokenized string template containing literal string segments and [`PlaceholderSpec`] tokens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PlaceholderTemplate {
    /// Ordered parts of the template.
    pub parts: Vec<PlaceholderPart>,
}

impl PlaceholderTemplate {
    /// Tokenizes a raw string containing placeholder tokens into a `PlaceholderTemplate`.
    pub fn parse(raw: &str) -> Self {
        let mut parts = Vec::new();
        let mut buf = String::new();
        let mut i = 0;
        let n = raw.len();

        while i < n {
            if let Some((end, ph)) = PlaceholderSpec::match_at(raw, i) {
                if !buf.is_empty() {
                    parts.push(PlaceholderPart::Literal(std::mem::take(&mut buf)));
                }
                parts.push(PlaceholderPart::Placeholder(ph));
                i = end;
            } else {
                let ch = raw[i..].chars().next().unwrap();
                buf.push(ch);
                i += ch.len_utf8();
            }
        }
        if !buf.is_empty() {
            parts.push(PlaceholderPart::Literal(buf));
        }

        Self { parts }
    }

    /// Constructs a template consisting of a single literal string.
    pub fn literal(text: impl Into<String>) -> Self {
        Self {
            parts: vec![PlaceholderPart::Literal(text.into())],
        }
    }

    /// Returns `true` if the template consists solely of one placeholder without literal text.
    pub fn is_single_placeholder(&self) -> bool {
        matches!(self.parts.as_slice(), [PlaceholderPart::Placeholder(_)])
    }

    /// Returns the sole placeholder if `is_single_placeholder` is `true`.
    pub fn single_placeholder(&self) -> Option<&PlaceholderSpec> {
        match self.parts.as_slice() {
            [PlaceholderPart::Placeholder(ph)] => Some(ph),
            _ => None,
        }
    }

    /// Returns `true` if the template contains at least one placeholder.
    pub fn has_placeholders(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, PlaceholderPart::Placeholder(_)))
    }

    /// Returns all placeholder specifications in order of appearance.
    pub fn placeholders(&self) -> Vec<&PlaceholderSpec> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                PlaceholderPart::Placeholder(ph) => Some(ph),
                PlaceholderPart::Literal(_) => None,
            })
            .collect()
    }

    /// Reconstructs the canonical source string.
    pub fn source(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                PlaceholderPart::Literal(lit) => out.push_str(lit),
                PlaceholderPart::Placeholder(ph) => out.push_str(&ph.to_token()),
            }
        }
        out
    }

    /// Returns a new template with placeholder names remapped via `mapping`.
    pub fn renamed(&self, mapping: &IndexMap<String, String>) -> Self {
        if !self.has_placeholders() {
            return self.clone();
        }
        let parts = self
            .parts
            .iter()
            .map(|part| match part {
                PlaceholderPart::Literal(s) => PlaceholderPart::Literal(s.clone()),
                PlaceholderPart::Placeholder(ph) => {
                    let mut cloned = ph.clone();
                    if let Some(new_name) = mapping.get(&ph.name) {
                        cloned.name = new_name.clone();
                    }
                    PlaceholderPart::Placeholder(cloned)
                }
            })
            .collect();
        Self { parts }
    }

    /// Returns the effective query parameter key for `key`, appending `"[]"` when this template
    /// is a single array placeholder with [`PlaceholderStyle::Bracket`].
    pub fn effective_param_key<'a>(&self, key: &'a str) -> std::borrow::Cow<'a, str> {
        if let Some(ph) = self.single_placeholder()
            && ph.is_array
            && ph.style == Some(PlaceholderStyle::Bracket)
        {
            std::borrow::Cow::Owned(format!("{key}[]"))
        } else {
            std::borrow::Cow::Borrowed(key)
        }
    }
}

/// Structured JSON value with preserved [`PlaceholderSpec`]s and [`PlaceholderTemplate`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JsonTemplateValue {
    /// JSON `null`.
    Null,
    /// JSON boolean.
    Bool(bool),
    /// JSON number (preserved as raw string/number representation).
    Number(String),
    /// Plain JSON string without placeholders.
    String(String),
    /// Standalone placeholder (`{"active": {{active:bool}}}` or `{"name": "{{name}}"}`).
    Placeholder(PlaceholderSpec),
    /// Interpolated string template (`{"greeting": "Hello {{name}}"}`).
    Template(PlaceholderTemplate),
    /// JSON array.
    Array(Vec<JsonTemplateValue>),
    /// JSON object.
    Object(IndexMap<String, JsonTemplateValue>),
}

/// Kind of HTTP request body in a [`RequestSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestBodyKind {
    /// No request body.
    #[default]
    Empty,
    /// JSON body (`application/json` or `--json`).
    Json,
    /// Form body (`application/x-www-form-urlencoded` or `multipart/form-data`).
    Form,
    /// Raw text/binary body.
    Raw,
}

impl RequestBodyKind {
    /// Returns `"empty"`, `"json"`, `"form"`, or `"raw"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Json => "json",
            Self::Form => "form",
            Self::Raw => "raw",
        }
    }
}

/// Request body payload in a [`RequestSpec`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RequestPayload {
    /// No payload (`body_kind == Empty`).
    #[default]
    Empty,
    /// Raw or JSON string template (`body_kind == Json | Raw`).
    Template(PlaceholderTemplate),
    /// Form key-value dictionary (`body_kind == Form`).
    Form(IndexMap<String, PlaceholderTemplate>),
}

/// Normalized HTTP request specification parsed from a POSIX `curl` command or raw HTTP/1.1|2 request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestSpec {
    /// HTTP method verb (`"GET"`, `"POST"`, `"PUT"`, `"DELETE"`, `"PATCH"`, `"HEAD"`, `"OPTIONS"`).
    pub method: String,
    /// Tokenized URL (with query string stripped unless bare-value placeholders exist in query keys).
    pub url: PlaceholderTemplate,
    /// HTTP headers in insertion order.
    pub headers: IndexMap<String, PlaceholderTemplate>,
    /// Cookies extracted from `Cookie` header.
    pub cookies: IndexMap<String, PlaceholderTemplate>,
    /// Query parameters extracted from the URL.
    pub params: IndexMap<String, Vec<PlaceholderTemplate>>,
    /// Request body kind (`Empty`, `Json`, `Form`, `Raw`).
    pub body_kind: RequestBodyKind,
    /// Request body payload.
    pub payload: RequestPayload,
    /// Structured JSON body template when `body_kind == RequestBodyKind::Json`.
    pub json_template: Option<JsonTemplateValue>,
}

impl Default for RequestSpec {
    fn default() -> Self {
        Self {
            method: "GET".to_string(),
            url: PlaceholderTemplate::default(),
            headers: IndexMap::new(),
            cookies: IndexMap::new(),
            params: IndexMap::new(),
            body_kind: RequestBodyKind::Empty,
            payload: RequestPayload::Empty,
            json_template: None,
        }
    }
}

impl RequestSpec {
    /// Returns the deduplicated list of [`PlaceholderSpec`]s across URL, headers, cookies, params, and body
    /// in order of first appearance.
    pub fn placeholders(&self) -> Vec<PlaceholderSpec> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for tmpl in self.all_templates() {
            for ph in tmpl.placeholders() {
                if seen.insert(ph.name.clone()) {
                    out.push(ph.clone());
                }
            }
        }
        out
    }

    /// Iterates over every [`PlaceholderTemplate`] in this request.
    pub fn all_templates(&self) -> Vec<&PlaceholderTemplate> {
        let mut list = vec![&self.url];
        list.extend(self.headers.values());
        list.extend(self.cookies.values());
        for vals in self.params.values() {
            list.extend(vals.iter());
        }
        match &self.payload {
            RequestPayload::Empty => {}
            RequestPayload::Template(t) => list.push(t),
            RequestPayload::Form(map) => list.extend(map.values()),
        }
        list
    }
}

/// Parses a `@request` payload (POSIX `curl` or raw HTTP/1.1|2) into a [`RequestSpec`].
pub fn parse_to_http(payload: &str) -> Result<RequestSpec, String> {
    let fmt = detect_format(payload)?;
    let raw_req = match fmt {
        RequestFormat::Curl => parse_curl_command(payload)?,
        RequestFormat::Http => parse_http_request(payload)?,
    };

    let method = raw_req.method.to_ascii_uppercase();
    let full_url = raw_req.url;
    let raw_params = raw_req.params;

    // If any query param key contains a placeholder (e.g. `?32-{{username}}`), keep full_url
    let has_key_placeholder = raw_params.keys().any(|k| PlaceholderSpec::search(k));
    let (url_str, params_map) = if has_key_placeholder {
        (full_url, IndexMap::new())
    } else {
        (strip_url_query_and_fragment(&full_url), raw_params)
    };

    let mut headers = raw_req.headers;
    let cookies = raw_req.cookies;

    if let Some((user, pass)) = raw_req.auth {
        if PlaceholderSpec::search(&user) || PlaceholderSpec::search(&pass) {
            return Err(
                "placeholders in curl -u/--user are not supported; use an Authorization header"
                    .to_string(),
            );
        }
        let token = base64_encode(format!("{user}:{pass}").as_bytes());
        if find_header(&headers, "Authorization").is_none() {
            headers.insert("Authorization".to_string(), format!("Basic {token}"));
        }
    }

    let content_type = find_header(&headers, "Content-Type")
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut body_kind = RequestBodyKind::Empty;
    let mut body_payload = RequestPayload::Empty;
    let mut json_template = None;

    if raw_req.is_json || (raw_req.has_data && content_type.contains("application/json")) {
        body_kind = RequestBodyKind::Json;
        let raw_body = raw_req.raw_body;
        validate_json_body(&raw_body)?;
        let tmpl = PlaceholderTemplate::parse(&raw_body);
        let parsed_tmpl = parse_json_template(&tmpl)?;
        json_template = Some(parsed_tmpl);
        body_payload = RequestPayload::Template(tmpl);
    } else if let Some(form_map) = raw_req.form_data {
        body_kind = RequestBodyKind::Form;
        let mut map = IndexMap::new();
        for (k, v) in form_map {
            map.insert(k, PlaceholderTemplate::parse(&v));
        }
        body_payload = RequestPayload::Form(map);
    } else if raw_req.has_data {
        let raw_body = raw_req.raw_body;
        if content_type.contains("application/x-www-form-urlencoded") {
            body_kind = RequestBodyKind::Form;
            let mut map = IndexMap::new();
            for (k, v) in parse_urlencoded_body(&raw_body) {
                map.insert(k, PlaceholderTemplate::parse(&v));
            }
            body_payload = RequestPayload::Form(map);
        } else {
            body_kind = RequestBodyKind::Raw;
            body_payload = RequestPayload::Template(PlaceholderTemplate::parse(&raw_body));
        }
    }

    let mut templated_headers = IndexMap::new();
    for (k, v) in headers {
        templated_headers.insert(k, PlaceholderTemplate::parse(&v));
    }

    let mut templated_cookies = IndexMap::new();
    for (k, v) in cookies {
        templated_cookies.insert(k, PlaceholderTemplate::parse(&v));
    }

    let mut templated_params = IndexMap::new();
    for (k, vals) in params_map {
        templated_params.insert(
            k,
            vals.into_iter()
                .map(|v| PlaceholderTemplate::parse(&v))
                .collect(),
        );
    }

    let spec = RequestSpec {
        method,
        url: PlaceholderTemplate::parse(&url_str),
        headers: templated_headers,
        cookies: templated_cookies,
        params: templated_params,
        body_kind,
        payload: body_payload,
        json_template,
    };

    // Validate URL path placeholders: `[]`, `?`, and `|style` are forbidden in URL path segments
    let path_only = strip_url_query_and_fragment(&url_str);
    let path_tmpl = PlaceholderTemplate::parse(&path_only);
    for ph in path_tmpl.placeholders() {
        if ph.is_array || ph.is_optional || ph.style.is_some() {
            return Err(format!(
                "URL path placeholder '{}' cannot use array '[]', optional '?', or '|style' modifiers (only valid in query parameters)",
                ph.to_token()
            ));
        }
    }

    Ok(spec)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestFormat {
    Curl,
    Http,
}

fn detect_format(payload: &str) -> Result<RequestFormat, String> {
    let stripped = payload.trim_start();
    if stripped.len() >= 5
        && stripped[..4].eq_ignore_ascii_case("curl")
        && stripped[4..].starts_with(char::is_whitespace)
    {
        return Ok(RequestFormat::Curl);
    }

    if let Some(first_line) = stripped.lines().next() {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 3
            && parts[0].chars().all(|c| c.is_ascii_uppercase())
            && parts[2].starts_with("HTTP/")
            && parts[2].len() > 5
            && parts[2].as_bytes()[5].is_ascii_digit()
        {
            return Ok(RequestFormat::Http);
        }
    }

    Err(
        "Unsupported @request format: expected 'curl ...' or a raw HTTP request line (METHOD URI HTTP/x.y)"
            .to_string(),
    )
}

#[derive(Debug, Default)]
struct RawParsedRequest {
    method: String,
    url: String,
    headers: IndexMap<String, String>,
    cookies: IndexMap<String, String>,
    params: IndexMap<String, Vec<String>>,
    auth: Option<(String, String)>,
    form_data: Option<IndexMap<String, String>>,
    raw_body: String,
    has_data: bool,
    is_json: bool,
}

/// Parses a POSIX `curl` command string into its components.
fn parse_curl_command(curl_command: &str) -> Result<RawParsedRequest, String> {
    let normalized = normalize_shell_continuations(curl_command.trim());
    let parts = shlex_split(&normalized)
        .map_err(|e| format!("Invalid shell syntax in curl command: {e}"))?;

    if parts.is_empty() || !parts[0].eq_ignore_ascii_case("curl") {
        return Err("Input is not a curl command".to_string());
    }

    let args = &parts[1..];
    let mut method = "GET".to_string();
    let mut method_explicit = false;
    let mut url: Option<String> = None;
    let mut headers: IndexMap<String, String> = IndexMap::new();
    let mut data: Option<String> = None;
    let mut json_data: Option<String> = None;
    let mut auth: Option<String> = None;
    let mut form_data: IndexMap<String, String> = IndexMap::new();
    let mut urlencoded_data: IndexMap<String, String> = IndexMap::new();

    let ignored_flags = [
        "--compressed",
        "-s",
        "--silent",
        "-L",
        "--location",
        "-k",
        "--insecure",
        "-i",
        "--include",
        "-g",
        "--globoff",
    ];

    let mut i = 0;
    while i < args.len() {
        let part = &args[i];
        match part.as_str() {
            "-X" | "--request" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing method after -X/--request".to_string());
                }
                method = args[i].to_ascii_uppercase();
                method_explicit = true;
                if !matches!(
                    method.as_str(),
                    "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
                ) {
                    return Err(format!("Unsupported HTTP method: {method}"));
                }
            }
            "-H" | "--header" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing header after -H/--header".to_string());
                }
                let header = &args[i];
                let Some((k, v)) = header.split_once(':') else {
                    return Err(format!("Invalid header format: {header}"));
                };
                headers.insert(k.trim().to_string(), v.trim().to_string());
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing data after -d/--data/--data-raw".to_string());
                }
                data = Some(args[i].clone());
            }
            "--json" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing JSON data after --json".to_string());
                }
                json_data = Some(args[i].clone());
            }
            "-u" | "--user" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing user credentials after -u/--user".to_string());
                }
                auth = Some(args[i].clone());
            }
            "-F" | "--form" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing form data after -F/--form".to_string());
                }
                let form_part = &args[i];
                if let Some((k, v)) = form_part.split_once('=') {
                    if !v.starts_with('@') {
                        form_data.insert(k.to_string(), v.to_string());
                    }
                }
            }
            "--url" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing URL after --url".to_string());
                }
                if url.is_some() {
                    return Err("Multiple URLs provided".to_string());
                }
                url = Some(args[i].clone());
            }
            s if s.starts_with("--data-urlencode") => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing data after --data-urlencode".to_string());
                }
                let param_part = &args[i];
                let Some((k, v)) = param_part.split_once('=') else {
                    return Err("Invalid --data-urlencode format, expected key=value".to_string());
                };
                urlencoded_data.insert(k.to_string(), v.to_string());
            }
            s if ignored_flags.contains(&s) => {}
            s if !s.starts_with('-') => {
                if s.trim().is_empty() {
                    // skip blank tokens
                } else if url.is_some() {
                    return Err("Multiple URLs provided".to_string());
                } else {
                    url = Some(s.to_string());
                }
            }
            other => {
                return Err(format!("Unsupported curl option: {other}"));
            }
        }
        i += 1;
    }

    let Some(final_url) = url else {
        return Err("No URL found in curl command".to_string());
    };

    if !method_explicit
        && (data.is_some()
            || json_data.is_some()
            || !form_data.is_empty()
            || !urlencoded_data.is_empty())
    {
        method = "POST".to_string();
    }

    let params = parse_query_params(&final_url);

    let mut is_json = false;
    let mut raw_body = String::new();
    let mut has_data = false;

    if let Some(jd) = json_data {
        is_json = true;
        raw_body = jd;
        has_data = true;
    } else if let Some(d) = data {
        raw_body = d.clone();
        has_data = true;
        // If data parses directly as JSON, treat as JSON
        if serde_json::from_str::<serde_json::Value>(&d).is_ok() {
            is_json = true;
        }
    }

    let mut cookies = IndexMap::new();
    if let Some(cookie_val) = find_header(&headers, "Cookie").map(str::to_owned) {
        cookies = parse_cookies(&cookie_val);
        headers.retain(|k, _| !k.eq_ignore_ascii_case("cookie"));
    }

    let parsed_auth = if let Some(auth_str) = auth {
        let Some((u, p)) = auth_str.split_once(':') else {
            return Err("Invalid auth format, expected 'user:pass'".to_string());
        };
        Some((u.to_string(), p.to_string()))
    } else {
        None
    };

    let form_map = if !form_data.is_empty() {
        Some(form_data)
    } else if !urlencoded_data.is_empty() {
        Some(urlencoded_data)
    } else {
        None
    };

    Ok(RawParsedRequest {
        method,
        url: final_url,
        headers,
        cookies,
        params,
        auth: parsed_auth,
        form_data: form_map,
        raw_body,
        has_data,
        is_json,
    })
}

/// Parses a raw HTTP/1.1 or HTTP/2 request string into its components.
fn parse_http_request(raw_http: &str) -> Result<RawParsedRequest, String> {
    let lines: Vec<&str> = raw_http.trim().lines().collect();
    if lines.is_empty() {
        return Err("Empty HTTP request".to_string());
    }

    let req_line: Vec<&str> = lines[0].split_whitespace().collect();
    if req_line.len() < 3 {
        return Err("Invalid request line".to_string());
    }
    let method = req_line[0].to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
    ) {
        return Err(format!("Unsupported HTTP method: {method}"));
    }
    let path = req_line[1];

    let mut headers: IndexMap<String, String> = IndexMap::new();
    let mut i = 1;
    while i < lines.len() && !lines[i].trim().is_empty() {
        let Some((k, v)) = lines[i].split_once(':') else {
            return Err(format!("Invalid header line: {}", lines[i]));
        };
        headers.insert(k.trim().to_string(), v.trim().to_string());
        i += 1;
    }

    if i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }

    let body = if i < lines.len() {
        lines[i..].join("\n").trim().to_string()
    } else {
        String::new()
    };

    let host = find_header(&headers, "Host").map(str::to_owned);
    let url = if let Some(ref h) = host {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("https://{h}{path}")
        }
    } else {
        path.to_string()
    };

    if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with('/') {
        return Err(format!("Invalid request URL or path: '{url}'"));
    }

    let params = parse_query_params(&url);

    let mut form_data = None;
    let mut is_json = false;
    let has_data = !body.is_empty();

    if has_data {
        let ct_full = find_header(&headers, "Content-Type").unwrap_or("");
        let ct_lower = ct_full.to_ascii_lowercase();
        if ct_lower.contains("multipart/form-data") {
            if let Some((_, after_b)) = ct_full.split_once("boundary=") {
                let boundary = after_b.split(';').next().unwrap_or("").trim();
                form_data = Some(parse_multipart(&body, boundary));
            }
        } else if ct_lower.contains("application/x-www-form-urlencoded") {
            form_data = Some(parse_urlencoded_body(&body));
        } else if ct_lower.contains("application/json") {
            is_json = true;
        }
    }

    let mut cookies = IndexMap::new();
    if let Some(cookie_val) = find_header(&headers, "Cookie").map(str::to_owned) {
        cookies = parse_cookies(&cookie_val);
        headers.retain(|k, _| !k.eq_ignore_ascii_case("cookie"));
    }

    if host.is_some() {
        headers.retain(|k, _| !k.eq_ignore_ascii_case("host"));
    }

    Ok(RawParsedRequest {
        method,
        url,
        headers,
        cookies,
        params,
        auth: None,
        form_data,
        raw_body: body,
        has_data,
        is_json,
    })
}

/// Validates a JSON request body that may contain placeholders (`{{name}}` or `{name}`).
pub fn validate_json_body(raw: &str) -> Result<(), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let substituted = PlaceholderSpec::sub(trimmed, "0");
    serde_json::from_str::<serde_json::Value>(&substituted).map_err(|err| {
        format!(
            "Invalid JSON body in @request (line {}, col {}): {}\n  body: {:?}",
            err.line(),
            err.column(),
            err,
            raw
        )
    })?;
    Ok(())
}

/// Parses a JSON body template while preserving placeholders as structured [`JsonTemplateValue`] nodes.
pub fn parse_json_template(tmpl: &PlaceholderTemplate) -> Result<JsonTemplateValue, String> {
    let src = tmpl.source();
    let mut sentinels: IndexMap<String, PlaceholderSpec> = IndexMap::new();
    let mut out = String::new();
    let mut in_string = false;

    for part in &tmpl.parts {
        match part {
            PlaceholderPart::Placeholder(spec) => {
                let mut key = format!("__PARSMITH_PH_{}__", sentinels.len());
                while src.contains(&key) {
                    key.insert(0, '_');
                }
                sentinels.insert(key.clone(), spec.clone());
                if in_string {
                    out.push_str(&key);
                } else {
                    out.push('"');
                    out.push_str(&key);
                    out.push('"');
                }
            }
            PlaceholderPart::Literal(lit) => {
                let chars: Vec<char> = lit.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    let ch = chars[i];
                    if ch == '\\' && i + 1 < chars.len() {
                        out.push(ch);
                        out.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    if ch == '"' {
                        in_string = !in_string;
                    }
                    out.push(ch);
                    i += 1;
                }
            }
        }
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&out).map_err(|e| format!("Invalid JSON body in @request: {e}"))?;

    restore_json_template(parsed, &sentinels)
}

fn restore_json_template(
    value: serde_json::Value,
    sentinels: &IndexMap<String, PlaceholderSpec>,
) -> Result<JsonTemplateValue, String> {
    match value {
        serde_json::Value::Null => Ok(JsonTemplateValue::Null),
        serde_json::Value::Bool(b) => Ok(JsonTemplateValue::Bool(b)),
        serde_json::Value::Number(num) => Ok(JsonTemplateValue::Number(num.to_string())),
        serde_json::Value::String(s) => {
            if let Some(ph) = sentinels.get(&s) {
                return Ok(JsonTemplateValue::Placeholder(ph.clone()));
            }
            if !sentinels.keys().any(|k| s.contains(k)) {
                return Ok(JsonTemplateValue::String(s));
            }
            // Reconstruct template parts
            let mut parts = Vec::new();
            let mut rest = s.as_str();
            while !rest.is_empty() {
                let mut earliest: Option<(usize, &str, &PlaceholderSpec)> = None;
                for (k, ph) in sentinels {
                    if let Some(pos) = rest.find(k.as_str()) {
                        if earliest.is_none_or(|(best_pos, _, _)| pos < best_pos) {
                            earliest = Some((pos, k.as_str(), ph));
                        }
                    }
                }
                if let Some((pos, key, ph)) = earliest {
                    if pos > 0 {
                        parts.push(PlaceholderPart::Literal(rest[..pos].to_string()));
                    }
                    parts.push(PlaceholderPart::Placeholder(ph.clone()));
                    rest = &rest[pos + key.len()..];
                } else {
                    parts.push(PlaceholderPart::Literal(rest.to_string()));
                    break;
                }
            }
            Ok(JsonTemplateValue::Template(PlaceholderTemplate { parts }))
        }
        serde_json::Value::Array(arr) => {
            let mut items = Vec::with_capacity(arr.len());
            for item in arr {
                items.push(restore_json_template(item, sentinels)?);
            }
            Ok(JsonTemplateValue::Array(items))
        }
        serde_json::Value::Object(map) => {
            let mut obj = IndexMap::with_capacity(map.len());
            for (k, v) in map {
                if sentinels.keys().any(|sk| k.contains(sk)) {
                    return Err("placeholders in JSON object keys are not supported".to_string());
                }
                obj.insert(k, restore_json_template(v, sentinels)?);
            }
            Ok(JsonTemplateValue::Object(obj))
        }
    }
}

fn find_header<'a>(headers: &'a IndexMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn find_query_split_outside_braces(url: &str) -> Option<usize> {
    let mut brace_depth = 0usize;
    for (idx, ch) in url.char_indices() {
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '?' if brace_depth == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}

fn strip_url_query_and_fragment(url: &str) -> String {
    let without_frag = url.split('#').next().unwrap_or(url);
    match find_query_split_outside_braces(without_frag) {
        Some(q_idx) => without_frag[..q_idx].to_string(),
        None => without_frag.to_string(),
    }
}

fn parse_query_params(url: &str) -> IndexMap<String, Vec<String>> {
    let mut params: IndexMap<String, Vec<String>> = IndexMap::new();
    let without_frag = url.split('#').next().unwrap_or(url);
    let Some(q_idx) = find_query_split_outside_braces(without_frag) else {
        return params;
    };
    let query = &without_frag[q_idx + 1..];
    if query.is_empty() {
        return params;
    }
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (raw_k, raw_v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let k = url_decode_plus(raw_k);
        let v = url_decode_plus(raw_v);
        params.entry(k).or_default().push(v);
    }
    params
}

fn parse_urlencoded_body(raw: &str) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    if raw.is_empty() {
        return out;
    }
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode_plus(k), url_decode_plus(v)),
            None => (url_decode_plus(pair), String::new()),
        };
        out.insert(k, v);
    }
    out
}

fn url_decode_plus(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_cookies(cookie_header: &str) -> IndexMap<String, String> {
    let mut cookies = IndexMap::new();
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some((name, val)) = pair.split_once('=') {
            cookies.insert(name.trim().to_string(), val.trim().to_string());
        }
    }
    cookies
}

fn parse_multipart(body: &str, boundary: &str) -> IndexMap<String, String> {
    let mut data = IndexMap::new();
    let delimiter = format!("--{boundary}");
    for part in body.split(&delimiter) {
        let part = part.trim();
        if part.is_empty() || part == "--" {
            continue;
        }
        let lines: Vec<&str> = part.lines().collect();
        let mut headers_part = IndexMap::new();
        let mut i = 0;
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        while i < lines.len() && !lines[i].trim().is_empty() {
            if let Some((k, v)) = lines[i].split_once(':') {
                headers_part.insert(k.trim().to_string(), v.trim().to_string());
            }
            i += 1;
        }
        if i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        let content = lines[i..].join("\n").trim().to_string();
        if let Some(cd) = find_header(&headers_part, "Content-Disposition") {
            if let Some((_, after_name)) = cd.split_once("name=") {
                let name = after_name
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim_matches('"')
                    .to_string();
                if !name.is_empty() {
                    data.insert(name, content);
                }
            }
        }
    }
    data
}

fn normalize_shell_continuations(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if chars[i] == '\\' {
            let mut j = i + 1;
            while j < n && matches!(chars[j], ' ' | '\t') {
                j += 1;
            }
            if j < n && chars[j] == '\r' {
                j += 1;
            }
            if j < n && chars[j] == '\n' {
                j += 1;
                while j < n && matches!(chars[j], ' ' | '\t') {
                    j += 1;
                }
                out.push(' ');
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// POSIX shell tokenizer (`shlex.split` equivalent).
fn shlex_split(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;

    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let ch = chars[i];
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                } else {
                    current.push(ch);
                }
                i += 1;
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                    i += 1;
                } else if ch == '\\' && i + 1 < n {
                    let nxt = chars[i + 1];
                    if matches!(nxt, '"' | '\\' | '$' | '`' | '\n') {
                        if nxt != '\n' {
                            current.push(nxt);
                        }
                        i += 2;
                    } else {
                        current.push(ch);
                        i += 1;
                    }
                } else {
                    current.push(ch);
                    i += 1;
                }
            }
            Some(_) => unreachable!(),
            None => {
                if ch.is_whitespace() {
                    if in_token {
                        tokens.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                    i += 1;
                } else if ch == '\'' || ch == '"' {
                    in_token = true;
                    quote = Some(ch);
                    i += 1;
                } else if ch == '\\' {
                    if i + 1 >= n {
                        return Err("No escaped character".to_string());
                    }
                    let nxt = chars[i + 1];
                    if nxt != '\n' {
                        in_token = true;
                        current.push(nxt);
                    }
                    i += 2;
                } else {
                    in_token = true;
                    current.push(ch);
                    i += 1;
                }
            }
        }
    }

    if quote.is_some() {
        return Err("No closing quotation".to_string());
    }
    if in_token {
        tokens.push(current);
    }

    Ok(tokens)
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// ============================================================================
// `@error` and `@error:<suffix>` specifications and naming helpers
// ============================================================================

/// Condition value on an `@error` directive property (`key=val`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ErrorConditionValue {
    /// String condition value.
    String(String),
    /// Integer condition value.
    Int(i128),
    /// Float condition value.
    Float(f64),
    /// Boolean condition value.
    Bool(bool),
    /// Null condition value.
    Null,
}

/// Parsed `@error` or `@error:<suffix>` directive specification on a `(rest)struct`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponseSpec {
    /// HTTP status code (`100..=599`).
    pub status: u16,
    /// Target `json` schema name for deserializing the error body.
    pub schema_name: String,
    /// Positional keys/dot-paths required to exist in the error JSON body.
    pub required_keys: Vec<String>,
    /// Property key/dot-path value conditions on the error JSON body.
    pub conditions: IndexMap<String, ErrorConditionValue>,
    /// Optional explicit suffix from `@error:<suffix>`.
    pub suffix: String,
    /// Source span of the `@error` directive node.
    pub span: Span,
}

/// Returns `true` if `status` is a valid HTTP status code (`100..=599`).
#[inline]
pub const fn is_http_status(status: i128) -> bool {
    status >= 100 && status <= 599
}

/// Returns `true` if `name` is `"@error"` or starts with `"@error:"`.
#[inline]
pub fn is_error_directive(name: &str) -> bool {
    name == "@error" || name.starts_with("@error:")
}

/// Returns `true` if `suffix` matches `^[A-Za-z_][A-Za-z0-9_-]*$`.
pub fn is_valid_error_suffix(suffix: &str) -> bool {
    let mut chars = suffix.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Extracts the leaf non-numeric segment of a dot-path in PascalCase (matching `ssc_codegen/core/rest_artifacts.py:_leaf_key_pascal`).
pub fn leaf_key_pascal(path: &str) -> String {
    let mut raw_segs = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            current.push(chars[i]);
            current.push(chars[i + 1]);
            i += 2;
        } else if chars[i] == '.' {
            if !current.is_empty() {
                raw_segs.push(std::mem::take(&mut current));
            }
            i += 1;
        } else {
            current.push(chars[i]);
            i += 1;
        }
    }
    if !current.is_empty() {
        raw_segs.push(current);
    }

    let segs: Vec<String> = raw_segs
        .into_iter()
        .map(|s| s.replace(r"\.", ".").replace(r"\\", "\\"))
        .filter(|s| !s.is_empty())
        .collect();

    if segs.is_empty() {
        return String::new();
    }

    let non_numeric: Vec<&String> = segs
        .iter()
        .filter(|s| !s.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let leaf = non_numeric
        .last()
        .copied()
        .unwrap_or_else(|| segs.last().unwrap());

    to_pascal_case(&leaf.replace(['.', '-'], "_"))
}

/// Computes the canonical PascalCase error variant class name for an [`ErrorResponseSpec`] in `struct_name`
/// (matching `ssc_codegen/core/rest_artifacts.py:err_subclass_name`).
pub fn err_subclass_name(struct_name: &str, err: &ErrorResponseSpec) -> String {
    let mut base = format!("{}Err{}", to_pascal_case(struct_name), err.status);
    if !err.suffix.is_empty() {
        return format!("{base}{}", to_pascal_case(&err.suffix.replace('-', "_")));
    }
    let all_keys = err
        .required_keys
        .iter()
        .chain(err.conditions.keys())
        .take(2);
    for key in all_keys {
        base.push_str(&leaf_key_pascal(key));
    }
    base
}

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::kdl::document::{KdlDocument, KdlNode, NodeId, NodeMatcher, NodeRef};
use crate::kdl::error::KdlError;
use crate::kdl::serializer::normalize_ty_input;
use crate::kdl::span::{LineIndex, Span};
use crate::kdl::value::KdlValue;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Structured syntax error returned when parsing a malformed KDL selector expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorError {
    /// Human-readable description of the selector syntax error.
    pub message: String,
    /// Original selector string that failed to parse.
    pub selector: String,
    /// 0-based byte offset in `selector` where the error occurred.
    pub offset: usize,
    /// Byte length of the offending token or slice.
    pub len: usize,
    /// 1-based starting line number in `selector`.
    pub line: u32,
    /// 1-based starting column number in `selector`.
    pub col: u32,
}

impl SelectorError {
    /// Constructs a new `SelectorError` at `(offset, len)` within `selector`.
    pub fn new(selector: &str, offset: usize, len: usize, message: impl Into<String>) -> Self {
        let line_index = LineIndex::new(selector);
        let span = line_index.span(offset, len);
        Self {
            message: message.into(),
            selector: selector.to_owned(),
            offset: span.offset,
            len: span.len,
            line: span.line as u32,
            col: span.col as u32,
        }
    }

    /// Returns the error message.
    #[inline]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the selector source string that failed to parse.
    #[inline]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Returns the 0-based byte offset where the error occurred.
    #[inline]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the source span within the selector string.
    #[inline]
    pub fn span(&self) -> Span {
        LineIndex::new(self.selector.as_str()).span(self.offset, self.len)
    }

    /// Returns the 1-based line number of the error in the selector string.
    #[inline]
    pub fn line(&self) -> usize {
        self.line as usize
    }

    /// Returns the 1-based column number of the error in the selector string.
    #[inline]
    pub fn col(&self) -> usize {
        self.col as usize
    }
}

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SelectorError {}

// ---------------------------------------------------------------------------
// Selector AST
// ---------------------------------------------------------------------------

/// Type annotation constraint on a node selector or attribute/argument filter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TypeMatcher {
    /// No `(type)` filter specified — matches regardless of whether a type annotation is present.
    #[default]
    Any,
    /// Empty `()` filter — matches only when the node or value has NO type annotation (`ty.is_none()`).
    Untyped,
    /// `(type)` filter — matches only when the normalized type annotation equals the given string.
    Typed(String),
}

impl TypeMatcher {
    /// Checks whether an optional type annotation satisfies this `TypeMatcher`.
    #[inline]
    pub fn matches(&self, actual: Option<&str>) -> bool {
        match self {
            Self::Any => true,
            Self::Untyped => actual.is_none(),
            Self::Typed(expected) => actual.is_some_and(|act| {
                if act == expected {
                    true
                } else {
                    normalize_ty_input(act) == *expected
                }
            }),
        }
    }
}

/// Node name and type annotation selector (`struct`, `*`, `(item)struct`, `(raw)*`, `()struct`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct NodeSelector {
    /// Target node name (`None` for wildcard `*`).
    pub name: Option<String>,
    /// Type annotation constraint.
    pub ty: TypeMatcher,
}

impl NodeSelector {
    /// Returns the explicit `(type)` string if this is `TypeMatcher::Typed`.
    #[inline]
    pub fn type_annotation(&self) -> Option<&str> {
        match &self.ty {
            TypeMatcher::Typed(s) => Some(s.as_str()),
            TypeMatcher::Any | TypeMatcher::Untyped => None,
        }
    }
}

/// Target key of an [`AttributeFilter`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FilterKey {
    /// Named property key (`[key]`, `["0"]`, `[\0]`).
    Prop(String),
    /// Positional argument index (`[0]`, `[1="val"]`).
    Arg(isize),
    /// Any positional argument wildcard (`[*="val"]`, `[(type)*="val"]`).
    WildcardArg,
}

/// Comparison operator inside an [`AttributeFilter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FilterOp {
    /// Presence check (`[key]`, `[0]`).
    Exists,
    /// Exact equality (`=`).
    Eq,
    /// Prefix match (`^=`).
    StartsWith,
    /// Suffix match (`$=`).
    EndsWith,
    /// Substring containment (`~=`).
    Contains,
}

/// Attribute or positional argument filter (`[key=val]`, `[0^="http"]`, `[(u16)port=8080]`, `[*=val]`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttributeFilter {
    /// Property key, argument index, or `*` wildcard argument.
    pub key: FilterKey,
    /// Optional value type annotation constraint (`[(type)key=val]`, `[()key=val]`).
    pub ty: TypeMatcher,
    /// Comparison operator.
    pub op: FilterOp,
    /// Expected operand value (`None` when `op == FilterOp::Exists`).
    pub value: Option<KdlValue>,
}

/// `An+B` expression for `:nth-child(...)` (`odd`, `even`, `2n+1`, `-n+3`, `2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NthExpr {
    /// Step size `A` in `An+B` (`0` for exact index `B`).
    pub a: i64,
    /// 1-based offset `B` in `An+B`.
    pub b: i64,
}

impl NthExpr {
    /// Evaluates whether a 1-based sibling position satisfies `An+B` for `n >= 0`.
    #[inline]
    pub const fn matches_1based(self, pos: i64) -> bool {
        if self.a == 0 {
            return pos == self.b;
        }
        let diff = pos - self.b;
        if self.a < 0 {
            diff <= 0 && diff % self.a == 0
        } else {
            diff >= 0 && diff % self.a == 0
        }
    }
}

/// Structural combinator joining two [`CompoundSelector`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Combinator {
    /// Descendant combinator (`A B`).
    Descendant,
    /// Direct child combinator (`A > B`).
    Child,
    /// Adjacent sibling combinator (`A + B`).
    Adjacent,
    /// General sibling combinator (`A ~ B`).
    GeneralSibling,
}

/// Relative selector inside `:has(...)`, supporting an optional leading combinator (`>`, `+`, `~`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelativeSelector {
    /// Optional leading combinator (`Some(Child)` for `:has(> b)`, `None` for `:has(b)`).
    pub leading: Option<Combinator>,
    /// Complex selector chain evaluated relative to the anchor node.
    pub selector: ComplexSelector,
}

/// Pseudo-class filter on a [`CompoundSelector`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PseudoClass {
    /// `:root` — matches nodes with no parent in the active query context.
    Root,
    /// `:empty` — matches nodes with zero child nodes.
    Empty,
    /// `:first-child` — matches the first sibling matching the compound's [`NodeSelector`].
    FirstChild,
    /// `:last-child` — matches the last sibling matching the compound's [`NodeSelector`].
    LastChild,
    /// `:only-child` — matches when this node is the only sibling matching the compound's [`NodeSelector`].
    OnlyChild,
    /// `:nth-child(An+B | odd | even)` — 1-based positional match among siblings matching [`NodeSelector`].
    NthChild(NthExpr),
    /// `:not(<selector-list>)` — matches when none of the inner selectors match the node.
    Not(Vec<ComplexSelector>),
    /// `:has(<relative-selector-list>)` — matches when at least one relative selector matches.
    Has(Vec<RelativeSelector>),
}

/// A sequence of simple selectors (node name/type, attribute filters, and pseudo-classes) without combinators.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CompoundSelector {
    /// Node name and type annotation constraint.
    pub node: NodeSelector,
    /// Attribute and argument filters (`[...]`).
    pub filters: Vec<AttributeFilter>,
    /// Pseudo-class filters (`:root`, `:not(...)`, `:has(...)`, etc.).
    pub pseudos: Vec<PseudoClass>,
}

/// A chain of [`CompoundSelector`]s separated by [`Combinator`]s (`compounds.len() == combinators.len() + 1`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComplexSelector {
    /// Compound selectors from left to right.
    pub compounds: Vec<CompoundSelector>,
    /// Combinators between adjacent compounds (`combinators.len() == compounds.len() - 1`).
    pub combinators: Vec<Combinator>,
}

/// A parsed comma-separated KDL selector list (`a, b, c`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SelectorList {
    /// Individual complex selectors in the union.
    pub selectors: Vec<ComplexSelector>,
}

/// Compiled CSS3-like KDL selector expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Selector {
    list: Arc<SelectorList>,
}

impl Selector {
    /// Parses a selector string into a compiled [`Selector`].
    pub fn parse(selector: &str) -> Result<Self, SelectorError> {
        let list = parse_selector_cached(selector)?;
        Ok(Self { list })
    }

    /// Returns the underlying [`SelectorList`] AST.
    #[inline]
    pub fn as_list(&self) -> &SelectorList {
        &self.list
    }

    /// Returns all matching nodes in `doc` in pre-order (deduplicated).
    pub fn select<'a>(&self, doc: &'a KdlDocument) -> Vec<NodeRef<'a>> {
        self.select_ids(doc)
            .into_iter()
            .map(|id| doc.node(id))
            .collect()
    }

    pub(crate) fn select_ids(&self, doc: &KdlDocument) -> Vec<NodeId> {
        let ctx = DocContext { doc };
        SelectorMatcher::new(doc, &ctx).match_all(&self.list)
    }

    /// Returns the first matching node in `doc` in pre-order, if any.
    pub fn select_first<'a>(&self, doc: &'a KdlDocument) -> Option<NodeRef<'a>> {
        self.select_first_id(doc).map(|id| doc.node(id))
    }

    pub(crate) fn select_first_id(&self, doc: &KdlDocument) -> Option<NodeId> {
        let ctx = DocContext { doc };
        SelectorMatcher::new(doc, &ctx).match_one(&self.list)
    }

    /// Returns all matching descendant nodes within `node`'s subtree in pre-order.
    pub fn select_in<'a>(&self, node: NodeRef<'a>) -> Vec<NodeRef<'a>> {
        let doc = node.document();
        self.select_in_ids(node)
            .into_iter()
            .map(|id| doc.node(id))
            .collect()
    }

    pub(crate) fn select_in_ids(&self, node: NodeRef<'_>) -> Vec<NodeId> {
        let doc = node.document();
        let ctx = SubtreeContext {
            doc,
            root: node.id(),
        };
        SelectorMatcher::new(doc, &ctx).match_all(&self.list)
    }

    /// Returns the first matching descendant node within `node`'s subtree in pre-order, if any.
    pub fn select_first_in<'a>(&self, node: NodeRef<'a>) -> Option<NodeRef<'a>> {
        let doc = node.document();
        self.select_first_in_id(node).map(|id| doc.node(id))
    }

    pub(crate) fn select_first_in_id(&self, node: NodeRef<'_>) -> Option<NodeId> {
        let doc = node.document();
        let ctx = SubtreeContext {
            doc,
            root: node.id(),
        };
        SelectorMatcher::new(doc, &ctx).match_one(&self.list)
    }

    /// Returns `true` if `node` matches this selector within its owning document.
    pub fn matches(&self, node: NodeRef<'_>) -> bool {
        let doc = node.document();
        let ctx = DocContext { doc };
        SelectorMatcher::new(doc, &ctx).matches_list(node.id(), &self.list)
    }

    /// Returns `true` if a standalone `KdlNode` matches this selector.
    pub fn matches_standalone(&self, node: &KdlNode) -> bool {
        StandaloneMatcher { node }.matches_list(&self.list)
    }
}

impl FromStr for Selector {
    type Err = SelectorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl<'a> NodeMatcher<'a> for Selector {
    #[inline]
    fn matches_node(&self, node: NodeRef<'a>) -> bool {
        self.matches(node)
    }
}

impl<'a> NodeMatcher<'a> for &Selector {
    #[inline]
    fn matches_node(&self, node: NodeRef<'a>) -> bool {
        self.matches(node)
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum TokType {
    Ident(String),
    String(String),
    Int(i128),
    Float(f64),
    Literal(KdlValue),
    Star,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Equal,
    CaretEq,
    DollarEq,
    TildeEq,
    Colon,
    Gt,
    Plus,
    Tilde,
    Comma,
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    Ident,
    String,
    Number,
    Literal,
    Star,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Equal,
    CaretEq,
    DollarEq,
    TildeEq,
    Colon,
    Gt,
    Plus,
    Tilde,
    Comma,
    Eof,
}

impl TokKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Ident => "IDENT",
            Self::String => "STRING",
            Self::Number => "NUMBER",
            Self::Literal => "BOOL",
            Self::Star => "STAR",
            Self::LParen => "LPAREN",
            Self::RParen => "RPAREN",
            Self::LBracket => "LBRACKET",
            Self::RBracket => "RBRACKET",
            Self::Equal => "EQUAL",
            Self::CaretEq => "CARET_EQ",
            Self::DollarEq => "DOLLAR_EQ",
            Self::TildeEq => "TILDE_EQ",
            Self::Colon => "COLON",
            Self::Gt => "GT",
            Self::Plus => "PLUS",
            Self::Tilde => "TILDE",
            Self::Comma => "COMMA",
            Self::Eof => "EOF",
        }
    }
}

impl TokType {
    const fn kind(&self) -> TokKind {
        match self {
            Self::Ident(_) => TokKind::Ident,
            Self::String(_) => TokKind::String,
            Self::Int(_) | Self::Float(_) => TokKind::Number,
            Self::Literal(_) => TokKind::Literal,
            Self::Star => TokKind::Star,
            Self::LParen => TokKind::LParen,
            Self::RParen => TokKind::RParen,
            Self::LBracket => TokKind::LBracket,
            Self::RBracket => TokKind::RBracket,
            Self::Equal => TokKind::Equal,
            Self::CaretEq => TokKind::CaretEq,
            Self::DollarEq => TokKind::DollarEq,
            Self::TildeEq => TokKind::TildeEq,
            Self::Colon => TokKind::Colon,
            Self::Gt => TokKind::Gt,
            Self::Plus => TokKind::Plus,
            Self::Tilde => TokKind::Tilde,
            Self::Comma => TokKind::Comma,
            Self::Eof => TokKind::Eof,
        }
    }
}

#[derive(Debug, Clone)]
struct Tok<'a> {
    typ: TokType,
    raw: &'a str,
    offset: usize,
}

#[inline]
const fn is_special(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '='
            | '^'
            | '$'
            | '~'
            | '*'
            | '+'
            | '>'
            | ':'
            | '#'
            | '"'
            | '\''
            | '/'
            | ','
    )
}

/// Unescapes CSS-style backslash escapes inside selector identifiers and strings.
pub(crate) fn unescape_selector_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next_ch) = chars.next() {
                out.push(next_ch);
            } else {
                out.push('\\');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

struct SelectorLexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> SelectorLexer<'a> {
    const fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn tokenize(mut self) -> Result<Vec<Tok<'a>>, SelectorError> {
        let mut tokens = Vec::new();
        while self.pos < self.src.len() {
            self.skip_ws();
            if self.pos >= self.src.len() {
                break;
            }
            let ch = self.cur_char().unwrap();
            let tok = self.next_tok(ch)?;
            tokens.push(tok);
        }
        tokens.push(Tok {
            typ: TokType::Eof,
            raw: "",
            offset: self.src.len(),
        });
        Ok(tokens)
    }

    #[inline]
    fn cur_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    #[inline]
    fn peek_char(&self) -> Option<char> {
        let mut it = self.src[self.pos..].chars();
        it.next()?;
        it.next()
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.cur_char() {
            if matches!(ch, ' ' | '\t' | '\r' | '\n') {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn error_at(&self, offset: usize, len: usize, msg: impl Into<String>) -> SelectorError {
        SelectorError::new(self.src, offset, len, msg)
    }

    fn next_tok(&mut self, ch: char) -> Result<Tok<'a>, SelectorError> {
        let start = self.pos;
        match ch {
            '(' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::LParen,
                    raw: "(",
                    offset: start,
                })
            }
            ')' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::RParen,
                    raw: ")",
                    offset: start,
                })
            }
            '[' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::LBracket,
                    raw: "[",
                    offset: start,
                })
            }
            ']' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::RBracket,
                    raw: "]",
                    offset: start,
                })
            }
            '*' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::Star,
                    raw: "*",
                    offset: start,
                })
            }
            '>' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::Gt,
                    raw: ">",
                    offset: start,
                })
            }
            '+' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::Plus,
                    raw: "+",
                    offset: start,
                })
            }
            ':' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::Colon,
                    raw: ":",
                    offset: start,
                })
            }
            ',' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::Comma,
                    raw: ",",
                    offset: start,
                })
            }
            '=' => {
                self.pos += 1;
                Ok(Tok {
                    typ: TokType::Equal,
                    raw: "=",
                    offset: start,
                })
            }
            '^' => {
                self.pos += 1;
                if self.cur_char() == Some('=') {
                    self.pos += 1;
                    Ok(Tok {
                        typ: TokType::CaretEq,
                        raw: "^=",
                        offset: start,
                    })
                } else {
                    Err(self.error_at(start, 1, format!("Unexpected '^' at position {start}")))
                }
            }
            '$' => {
                self.pos += 1;
                if self.cur_char() == Some('=') {
                    self.pos += 1;
                    Ok(Tok {
                        typ: TokType::DollarEq,
                        raw: "$=",
                        offset: start,
                    })
                } else {
                    Err(self.error_at(start, 1, format!("Unexpected '$' at position {start}")))
                }
            }
            '~' => {
                self.pos += 1;
                if self.cur_char() == Some('=') {
                    self.pos += 1;
                    Ok(Tok {
                        typ: TokType::TildeEq,
                        raw: "~=",
                        offset: start,
                    })
                } else {
                    Ok(Tok {
                        typ: TokType::Tilde,
                        raw: "~",
                        offset: start,
                    })
                }
            }
            '#' => self.read_hash_literal(),
            '"' | '\'' => self.read_string(ch),
            c if c.is_ascii_digit()
                || (c == '-' && self.peek_char().is_some_and(|n| n.is_ascii_digit())) =>
            {
                self.read_number()
            }
            c if c == '\\' || (!is_special(c) && !c.is_whitespace()) => self.read_ident(),
            c => Err(self.error_at(
                start,
                c.len_utf8(),
                format!("Unexpected character '{c}' at position {start}"),
            )),
        }
    }

    fn read_hash_literal(&mut self) -> Result<Tok<'a>, SelectorError> {
        let start = self.pos;
        self.pos += 1; // '#'
        let rest = &self.src[self.pos..];
        let candidates: [(&str, KdlValue); 6] = [
            ("true", KdlValue::Bool(true)),
            ("false", KdlValue::Bool(false)),
            ("null", KdlValue::Null),
            ("-inf", KdlValue::Float(f64::NEG_INFINITY)),
            ("inf", KdlValue::Float(f64::INFINITY)),
            ("nan", KdlValue::Float(f64::NAN)),
        ];
        for (kw, val) in candidates {
            if rest.starts_with(kw) {
                let end = self.pos + kw.len();
                let next_ch = self.src[end..].chars().next();
                if next_ch.is_none_or(|c| is_special(c) || c.is_whitespace()) {
                    self.pos = end;
                    return Ok(Tok {
                        typ: TokType::Literal(val),
                        raw: &self.src[start..end],
                        offset: start,
                    });
                }
            }
        }
        Err(self.error_at(
            start,
            self.src.len() - start,
            format!(
                "Invalid token at position {start}: '{}'",
                &self.src[start..]
            ),
        ))
    }

    fn read_string(&mut self, quote: char) -> Result<Tok<'a>, SelectorError> {
        let start = self.pos;
        self.pos += 1; // opening quote
        let inner_start = self.pos;
        while let Some(ch) = self.cur_char() {
            if ch == quote {
                break;
            }
            if ch == '\\' {
                self.pos += 1;
                if let Some(escaped) = self.cur_char() {
                    self.pos += escaped.len_utf8();
                }
            } else {
                self.pos += ch.len_utf8();
            }
        }
        if self.cur_char() != Some(quote) {
            return Err(self.error_at(
                start,
                self.pos - start,
                format!("Unterminated string starting at position {start}"),
            ));
        }
        let inner = &self.src[inner_start..self.pos];
        self.pos += 1; // closing quote
        let raw = &self.src[start..self.pos];
        Ok(Tok {
            typ: TokType::String(unescape_selector_str(inner)),
            raw,
            offset: start,
        })
    }

    fn read_number(&mut self) -> Result<Tok<'a>, SelectorError> {
        let start = self.pos;
        if matches!(self.cur_char(), Some('+' | '-')) {
            self.pos += 1;
        }
        while let Some(ch) = self.cur_char() {
            if ch.is_ascii_digit() || ch == '.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let raw = &self.src[start..self.pos];
        if raw.contains('.') {
            let val = raw.parse::<f64>().map_err(|_| {
                self.error_at(
                    start,
                    raw.len(),
                    format!("Invalid number at position {start}: '{raw}'"),
                )
            })?;
            Ok(Tok {
                typ: TokType::Float(val),
                raw,
                offset: start,
            })
        } else {
            let val = raw.parse::<i128>().map_err(|_| {
                self.error_at(
                    start,
                    raw.len(),
                    format!("Invalid number at position {start}: '{raw}'"),
                )
            })?;
            Ok(Tok {
                typ: TokType::Int(val),
                raw,
                offset: start,
            })
        }
    }

    fn read_ident(&mut self) -> Result<Tok<'a>, SelectorError> {
        let start = self.pos;
        while let Some(ch) = self.cur_char() {
            if ch == '\\' {
                let bs_pos = self.pos;
                self.pos += 1;
                let Some(escaped) = self.cur_char() else {
                    return Err(self.error_at(
                        bs_pos,
                        1,
                        format!("Unterminated escape sequence at position {bs_pos}"),
                    ));
                };
                self.pos += escaped.len_utf8();
            } else if !is_special(ch) && !ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        let raw = &self.src[start..self.pos];
        Ok(Tok {
            typ: TokType::Ident(unescape_selector_str(raw)),
            raw,
            offset: start,
        })
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct SelectorParser<'a> {
    src: &'a str,
    tokens: Vec<Tok<'a>>,
    idx: usize,
}

impl<'a> SelectorParser<'a> {
    const fn new(src: &'a str, tokens: Vec<Tok<'a>>) -> Self {
        Self {
            src,
            tokens,
            idx: 0,
        }
    }

    fn parse(mut self) -> Result<SelectorList, SelectorError> {
        let first = self.parse_complex()?;
        let mut selectors = vec![first];
        while self.cur_kind() == TokKind::Comma {
            self.advance();
            selectors.push(self.parse_complex()?);
        }
        if self.cur_kind() != TokKind::Eof {
            let cur = self.cur();
            return Err(self.error_at_tok(cur, format!("Unexpected token: {}", cur.raw)));
        }
        Ok(SelectorList { selectors })
    }

    #[inline]
    fn cur(&self) -> &Tok<'a> {
        &self.tokens[self.idx]
    }

    #[inline]
    fn cur_kind(&self) -> TokKind {
        self.tokens[self.idx].typ.kind()
    }

    #[inline]
    fn advance(&mut self) -> Tok<'a> {
        let tok = self.tokens[self.idx].clone();
        if self.idx + 1 < self.tokens.len() {
            self.idx += 1;
        }
        tok
    }

    fn expect(&mut self, expected: TokKind) -> Result<Tok<'a>, SelectorError> {
        let tok = self.advance();
        if tok.typ.kind() != expected {
            return Err(self.error_at_tok(
                &tok,
                format!("Expected {}, got '{}'", expected.name(), tok.raw),
            ));
        }
        Ok(tok)
    }

    fn error_at_tok(&self, tok: &Tok<'a>, msg: impl Into<String>) -> SelectorError {
        SelectorError::new(self.src, tok.offset, tok.raw.len(), msg)
    }

    fn parse_complex(&mut self) -> Result<ComplexSelector, SelectorError> {
        let first = self.parse_compound()?;
        let mut compounds = vec![first];
        let mut combinators = Vec::new();
        while let Some(comb) = self.try_combinator() {
            combinators.push(comb);
            compounds.push(self.parse_compound()?);
        }
        Ok(ComplexSelector {
            compounds,
            combinators,
        })
    }

    fn try_combinator(&mut self) -> Option<Combinator> {
        match self.cur_kind() {
            TokKind::Gt => {
                self.advance();
                Some(Combinator::Child)
            }
            TokKind::Plus => {
                self.advance();
                Some(Combinator::Adjacent)
            }
            TokKind::Tilde => {
                self.advance();
                Some(Combinator::GeneralSibling)
            }
            TokKind::Ident | TokKind::Star | TokKind::LParen | TokKind::String => {
                Some(Combinator::Descendant)
            }
            _ => None,
        }
    }

    fn parse_compound(&mut self) -> Result<CompoundSelector, SelectorError> {
        let node = self.parse_node_selector()?;
        let mut filters = Vec::new();
        while self.cur_kind() == TokKind::LBracket {
            filters.push(self.parse_filter()?);
        }
        let mut pseudos = Vec::new();
        while self.cur_kind() == TokKind::Colon {
            pseudos.push(self.parse_pseudo()?);
        }
        Ok(CompoundSelector {
            node,
            filters,
            pseudos,
        })
    }

    fn parse_node_selector(&mut self) -> Result<NodeSelector, SelectorError> {
        if self.cur_kind() == TokKind::LParen {
            let ty = self.parse_type_annotation()?;
            let name = match self.cur_kind() {
                TokKind::Ident | TokKind::String => {
                    let tok = self.advance();
                    match tok.typ {
                        TokType::Ident(s) | TokType::String(s) => Some(s),
                        _ => unreachable!(),
                    }
                }
                TokKind::Star => {
                    self.advance();
                    None
                }
                _ => None,
            };
            return Ok(NodeSelector { name, ty });
        }
        if self.cur_kind() == TokKind::Star {
            self.advance();
            return Ok(NodeSelector {
                name: None,
                ty: TypeMatcher::Any,
            });
        }
        if matches!(self.cur_kind(), TokKind::Ident | TokKind::String) {
            let tok = self.advance();
            let name = match tok.typ {
                TokType::Ident(s) | TokType::String(s) => s,
                _ => unreachable!(),
            };
            return Ok(NodeSelector {
                name: Some(name),
                ty: TypeMatcher::Any,
            });
        }
        if matches!(self.cur_kind(), TokKind::LBracket | TokKind::Colon) {
            return Ok(NodeSelector {
                name: None,
                ty: TypeMatcher::Any,
            });
        }
        let cur = self.cur();
        Err(self.error_at_tok(cur, format!("Expected node selector, got '{}'", cur.raw)))
    }

    fn parse_type_annotation(&mut self) -> Result<TypeMatcher, SelectorError> {
        self.expect(TokKind::LParen)?;
        if self.cur_kind() == TokKind::RParen {
            self.advance();
            return Ok(TypeMatcher::Untyped);
        }
        let tok = self.cur().clone();
        let val = match &tok.typ {
            TokType::Ident(s) | TokType::String(s) => {
                let v = s.clone();
                self.advance();
                v
            }
            _ => {
                return Err(self.error_at_tok(
                    &tok,
                    format!(
                        "Expected type annotation identifier or string, got '{}'",
                        tok.raw
                    ),
                ));
            }
        };
        self.expect(TokKind::RParen)?;
        Ok(TypeMatcher::Typed(normalize_ty_input(&val)))
    }

    fn parse_filter(&mut self) -> Result<AttributeFilter, SelectorError> {
        self.expect(TokKind::LBracket)?;
        let filt = self.parse_filter_inner()?;
        self.expect(TokKind::RBracket)?;
        Ok(filt)
    }

    fn parse_filter_inner(&mut self) -> Result<AttributeFilter, SelectorError> {
        let ty = if self.cur_kind() == TokKind::LParen {
            self.parse_type_annotation()?
        } else {
            TypeMatcher::Any
        };

        if self.cur_kind() == TokKind::Star {
            self.advance();
            if self.cur_kind() == TokKind::RBracket {
                return Ok(AttributeFilter {
                    key: FilterKey::WildcardArg,
                    ty,
                    op: FilterOp::Exists,
                    value: None,
                });
            }
            let (op, val) = self.parse_operator_and_value()?;
            return Ok(AttributeFilter {
                key: FilterKey::WildcardArg,
                ty,
                op,
                value: Some(val),
            });
        }

        let key = match &self.cur().typ {
            TokType::Int(i) => {
                let idx = *i;
                let tok = self.advance();
                let idx_isize = isize::try_from(idx).map_err(|_| {
                    self.error_at_tok(&tok, format!("Expected integer index, got '{}'", tok.raw))
                })?;
                FilterKey::Arg(idx_isize)
            }
            TokType::Float(_) => {
                let tok = self.advance();
                return Err(
                    self.error_at_tok(&tok, format!("Expected integer index, got '{}'", tok.raw))
                );
            }
            TokType::Ident(_) | TokType::String(_) => {
                let tok = self.advance();
                match tok.typ {
                    TokType::Ident(s) | TokType::String(s) => FilterKey::Prop(s),
                    _ => unreachable!(),
                }
            }
            _ => {
                let cur = self.cur();
                return Err(self.error_at_tok(
                    cur,
                    format!("Expected key name or index, got '{}'", cur.raw),
                ));
            }
        };

        if self.cur_kind() == TokKind::RBracket {
            return Ok(AttributeFilter {
                key,
                ty,
                op: FilterOp::Exists,
                value: None,
            });
        }

        let (op, val) = self.parse_operator_and_value()?;
        Ok(AttributeFilter {
            key,
            ty,
            op,
            value: Some(val),
        })
    }

    fn parse_operator_and_value(&mut self) -> Result<(FilterOp, KdlValue), SelectorError> {
        let tok = self.advance();
        let op = match tok.typ {
            TokType::Equal => FilterOp::Eq,
            TokType::CaretEq => FilterOp::StartsWith,
            TokType::DollarEq => FilterOp::EndsWith,
            TokType::TildeEq => FilterOp::Contains,
            _ => {
                return Err(
                    self.error_at_tok(&tok, format!("Expected operator, got '{}'", tok.raw))
                );
            }
        };
        let val = self.parse_value()?;
        Ok((op, val))
    }

    fn parse_value(&mut self) -> Result<KdlValue, SelectorError> {
        let tok = self.cur().clone();
        match tok.typ {
            TokType::Plus => {
                self.advance();
                let num_tok = self.cur().clone();
                if num_tok.raw.starts_with('-') {
                    return Err(self.error_at_tok(
                        &num_tok,
                        format!("Expected number after '+', got '{}'", num_tok.raw),
                    ));
                }
                match num_tok.typ {
                    TokType::Int(i) => {
                        self.advance();
                        Ok(KdlValue::Int(i))
                    }
                    TokType::Float(f) => {
                        self.advance();
                        Ok(KdlValue::Float(f))
                    }
                    _ => Err(self.error_at_tok(
                        &num_tok,
                        format!("Expected number after '+', got '{}'", num_tok.raw),
                    )),
                }
            }
            TokType::String(s) | TokType::Ident(s) => {
                self.advance();
                Ok(KdlValue::String(s))
            }
            TokType::Int(i) => {
                self.advance();
                Ok(KdlValue::Int(i))
            }
            TokType::Float(f) => {
                self.advance();
                Ok(KdlValue::Float(f))
            }
            TokType::Literal(v) => {
                self.advance();
                Ok(v)
            }
            _ => Err(self.error_at_tok(&tok, format!("Expected value, got '{}'", tok.raw))),
        }
    }

    fn parse_pseudo(&mut self) -> Result<PseudoClass, SelectorError> {
        self.expect(TokKind::Colon)?;
        let name_tok = self.expect(TokKind::Ident)?;
        let name = match &name_tok.typ {
            TokType::Ident(s) => s.clone(),
            _ => unreachable!(),
        };

        if self.cur_kind() == TokKind::LParen {
            self.advance();
            let pseudo = match name.as_str() {
                "not" => {
                    let first = self.parse_complex()?;
                    let mut ns = vec![first];
                    while self.cur_kind() == TokKind::Comma {
                        self.advance();
                        ns.push(self.parse_complex()?);
                    }
                    PseudoClass::Not(ns)
                }
                "has" => {
                    let mut hs = Vec::new();
                    let leading = self.try_has_initial_combinator();
                    let selector = self.parse_complex()?;
                    hs.push(RelativeSelector { leading, selector });
                    while self.cur_kind() == TokKind::Comma {
                        self.advance();
                        let leading = self.try_has_initial_combinator();
                        let selector = self.parse_complex()?;
                        hs.push(RelativeSelector { leading, selector });
                    }
                    PseudoClass::Has(hs)
                }
                "nth-child" => {
                    let nth = self.parse_nth_expr()?;
                    PseudoClass::NthChild(nth)
                }
                other => {
                    return Err(self.error_at_tok(
                        &name_tok,
                        format!("Unknown functional pseudo-class ':{other}()'"),
                    ));
                }
            };
            self.expect(TokKind::RParen)?;
            Ok(pseudo)
        } else {
            match name.as_str() {
                "root" => Ok(PseudoClass::Root),
                "empty" => Ok(PseudoClass::Empty),
                "first-child" => Ok(PseudoClass::FirstChild),
                "last-child" => Ok(PseudoClass::LastChild),
                "only-child" => Ok(PseudoClass::OnlyChild),
                "not" | "has" | "nth-child" => Err(self.error_at_tok(
                    &name_tok,
                    format!("Pseudo-class ':{name}' requires parenthesized arguments"),
                )),
                other => {
                    Err(self.error_at_tok(&name_tok, format!("Unknown pseudo-class ':{other}'")))
                }
            }
        }
    }

    fn try_has_initial_combinator(&mut self) -> Option<Combinator> {
        match self.cur_kind() {
            TokKind::Gt => {
                self.advance();
                Some(Combinator::Child)
            }
            TokKind::Plus => {
                self.advance();
                Some(Combinator::Adjacent)
            }
            TokKind::Tilde => {
                self.advance();
                Some(Combinator::GeneralSibling)
            }
            _ => None,
        }
    }

    fn parse_nth_expr(&mut self) -> Result<NthExpr, SelectorError> {
        if self.cur_kind() == TokKind::Ident {
            if self.cur().raw == "odd" {
                self.advance();
                return Ok(NthExpr { a: 2, b: 1 });
            }
            if self.cur().raw == "even" {
                self.advance();
                return Ok(NthExpr { a: 2, b: 0 });
            }
        }

        let mut has_plus = false;
        if self.cur_kind() == TokKind::Plus {
            has_plus = true;
            self.advance();
        }

        let cur = self.cur().clone();
        if let TokType::Int(num_i128) = cur.typ {
            if has_plus && cur.raw.starts_with('-') {
                return Err(
                    self.error_at_tok(&cur, format!("Invalid nth expression, got '{}'", cur.raw))
                );
            }
            let num = i64::try_from(num_i128).map_err(|_| {
                self.error_at_tok(&cur, format!("Invalid nth expression, got '{}'", cur.raw))
            })?;
            self.advance();
            if self.cur_kind() == TokKind::Ident {
                let ident_raw = self.cur().raw;
                if ident_raw == "n" {
                    self.advance();
                    let b = self.parse_nth_offset()?;
                    return Ok(NthExpr { a: num, b });
                }
                if let Some(rest) = ident_raw.strip_prefix("n-") {
                    if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                        let b = ident_raw[1..].parse::<i64>().map_err(|_| {
                            self.error_at_tok(
                                self.cur(),
                                format!("Invalid nth expression, got '{ident_raw}'"),
                            )
                        })?;
                        self.advance();
                        return Ok(NthExpr { a: num, b });
                    }
                }
            }
            return Ok(NthExpr { a: 0, b: num });
        }

        if cur.typ.kind() == TokKind::Ident {
            let ident_raw = cur.raw;
            if has_plus && ident_raw.starts_with('-') {
                return Err(
                    self.error_at_tok(&cur, format!("Invalid nth expression, got '{ident_raw}'"))
                );
            }
            if ident_raw == "n" {
                self.advance();
                let b = self.parse_nth_offset()?;
                return Ok(NthExpr { a: 1, b });
            }
            if ident_raw == "-n" {
                self.advance();
                let b = self.parse_nth_offset()?;
                return Ok(NthExpr { a: -1, b });
            }
            if let Some(rest) = ident_raw.strip_prefix("n-") {
                if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                    let b = ident_raw[1..].parse::<i64>().map_err(|_| {
                        self.error_at_tok(
                            &cur,
                            format!("Invalid nth expression, got '{ident_raw}'"),
                        )
                    })?;
                    self.advance();
                    return Ok(NthExpr { a: 1, b });
                }
            }
            if let Some(rest) = ident_raw.strip_prefix("-n-") {
                if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                    let b = ident_raw[2..].parse::<i64>().map_err(|_| {
                        self.error_at_tok(
                            &cur,
                            format!("Invalid nth expression, got '{ident_raw}'"),
                        )
                    })?;
                    self.advance();
                    return Ok(NthExpr { a: -1, b });
                }
            }
        }

        Err(self.error_at_tok(&cur, format!("Invalid nth expression, got '{}'", cur.raw)))
    }

    fn parse_nth_offset(&mut self) -> Result<i64, SelectorError> {
        if self.cur_kind() == TokKind::Plus {
            self.advance();
            let tok = self.expect(TokKind::Number)?;
            if tok.raw.starts_with('-') {
                return Err(
                    self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw))
                );
            }
            if let TokType::Int(v) = tok.typ {
                return i64::try_from(v).map_err(|_| {
                    self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw))
                });
            }
            return Err(self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw)));
        }

        if self.cur_kind() == TokKind::Ident && self.cur().raw == "-" {
            self.advance();
            let tok = self.expect(TokKind::Number)?;
            if tok.raw.starts_with('-') {
                return Err(
                    self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw))
                );
            }
            if let TokType::Int(v) = tok.typ {
                let n = i64::try_from(v).map_err(|_| {
                    self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw))
                })?;
                return Ok(-n);
            }
            return Err(self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw)));
        }

        if self.cur_kind() == TokKind::Number && self.cur().raw.starts_with('-') {
            let tok = self.advance();
            if let TokType::Int(v) = tok.typ {
                return i64::try_from(v).map_err(|_| {
                    self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw))
                });
            }
            return Err(self.error_at_tok(&tok, format!("Invalid nth offset, got '{}'", tok.raw)));
        }

        Ok(0)
    }
}

// ---------------------------------------------------------------------------
// Thread-local Selector Cache
// ---------------------------------------------------------------------------

thread_local! {
    static SELECTOR_CACHE: RefCell<HashMap<String, Arc<SelectorList>>> = RefCell::new(HashMap::new());
}

/// Parses a selector string (using a thread-local cache for repeated queries).
pub(crate) fn parse_selector_cached(selector: &str) -> Result<Arc<SelectorList>, SelectorError> {
    if let Some(cached) = SELECTOR_CACHE.with(|cache| cache.borrow().get(selector).cloned()) {
        return Ok(cached);
    }
    let tokens = SelectorLexer::new(selector).tokenize()?;
    let parsed = Arc::new(SelectorParser::new(selector, tokens).parse()?);
    SELECTOR_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if map.len() >= 512 {
            map.clear();
        }
        map.insert(selector.to_owned(), Arc::clone(&parsed));
    });
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Match Contexts & Matcher
// ---------------------------------------------------------------------------

trait MatchContext {
    fn iter_node_ids(&self) -> Vec<NodeId>;
    fn parent_of(&self, id: NodeId) -> Option<NodeId>;
    fn siblings_of(&self, id: NodeId) -> &[NodeId];
    fn index_of(&self, id: NodeId) -> usize;
}

struct DocContext<'a> {
    doc: &'a KdlDocument,
}

impl<'a> MatchContext for DocContext<'a> {
    fn iter_node_ids(&self) -> Vec<NodeId> {
        self.doc.iter_nodes().map(|n| n.id()).collect()
    }

    #[inline]
    fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.doc[id].parent
    }

    #[inline]
    fn siblings_of(&self, id: NodeId) -> &[NodeId] {
        self.doc.sibling_ids(id)
    }

    #[inline]
    fn index_of(&self, id: NodeId) -> usize {
        self.doc[id].index_in_parent
    }
}

struct SubtreeContext<'a> {
    doc: &'a KdlDocument,
    root: NodeId,
}

impl<'a> MatchContext for SubtreeContext<'a> {
    fn iter_node_ids(&self) -> Vec<NodeId> {
        self.doc
            .node(self.root)
            .iter_descendants()
            .map(|n| n.id())
            .collect()
    }

    #[inline]
    fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        if id == self.root {
            None
        } else {
            self.doc[id].parent
        }
    }

    #[inline]
    fn siblings_of(&self, id: NodeId) -> &[NodeId] {
        match self.doc[id].parent {
            Some(pid) => &self.doc[pid].children,
            None => &self.doc[self.root].children,
        }
    }

    #[inline]
    fn index_of(&self, id: NodeId) -> usize {
        self.doc[id].index_in_parent
    }
}

fn value_as_match_str(val: &KdlValue) -> String {
    match val {
        KdlValue::String(s) => s.clone(),
        KdlValue::Int(i) => i.to_string(),
        KdlValue::Float(f) => {
            if f.is_nan() {
                "nan".to_owned()
            } else if f.is_infinite() {
                if f.is_sign_positive() {
                    "inf".to_owned()
                } else {
                    "-inf".to_owned()
                }
            } else {
                f.to_string()
            }
        }
        KdlValue::Bool(true) => "true".to_owned(),
        KdlValue::Bool(false) => "false".to_owned(),
        KdlValue::Null => "null".to_owned(),
    }
}

fn compare_values(actual: &KdlValue, op: FilterOp, expected: &KdlValue) -> bool {
    match op {
        FilterOp::Exists => true,
        FilterOp::Eq => actual == expected,
        FilterOp::StartsWith => {
            let act_str = value_as_match_str(actual);
            let exp_str = value_as_match_str(expected);
            act_str.starts_with(&exp_str)
        }
        FilterOp::EndsWith => {
            let act_str = value_as_match_str(actual);
            let exp_str = value_as_match_str(expected);
            act_str.ends_with(&exp_str)
        }
        FilterOp::Contains => {
            let act_str = value_as_match_str(actual);
            let exp_str = value_as_match_str(expected);
            act_str.contains(&exp_str)
        }
    }
}

fn matches_node_selector(node: &KdlNode, sel: &NodeSelector) -> bool {
    if let Some(expected_name) = &sel.name {
        if node.name != *expected_name {
            return false;
        }
    }
    sel.ty.matches(node.ty())
}

fn entry_matches_type_and_op(
    entry_ty: Option<&str>,
    entry_val: &KdlValue,
    ty: &TypeMatcher,
    op: FilterOp,
    expected: Option<&KdlValue>,
) -> bool {
    if !ty.matches(entry_ty) {
        return false;
    }
    match op {
        FilterOp::Exists => true,
        op => expected.is_some_and(|exp| compare_values(entry_val, op, exp)),
    }
}

fn matches_attribute_filter(node: &KdlNode, filt: &AttributeFilter) -> bool {
    match &filt.key {
        FilterKey::WildcardArg => node.args.iter().any(|arg| {
            entry_matches_type_and_op(arg.ty(), &arg.value, &filt.ty, filt.op, filt.value.as_ref())
        }),
        FilterKey::Arg(idx) => {
            if *idx < 0 {
                return false;
            }
            let Some(arg) = node.args.get(*idx as usize) else {
                return false;
            };
            entry_matches_type_and_op(arg.ty(), &arg.value, &filt.ty, filt.op, filt.value.as_ref())
        }
        FilterKey::Prop(key) => {
            let Some(prop) = node.prop(key) else {
                return false;
            };
            entry_matches_type_and_op(
                prop.ty(),
                &prop.value,
                &filt.ty,
                filt.op,
                filt.value.as_ref(),
            )
        }
    }
}

struct SelectorMatcher<'a, C: MatchContext> {
    doc: &'a KdlDocument,
    ctx: &'a C,
}

impl<'a, C: MatchContext> SelectorMatcher<'a, C> {
    const fn new(doc: &'a KdlDocument, ctx: &'a C) -> Self {
        Self { doc, ctx }
    }

    fn match_all(&self, selector: &SelectorList) -> Vec<NodeId> {
        let mut results = Vec::new();
        for id in self.ctx.iter_node_ids() {
            if self.matches_list(id, selector) {
                results.push(id);
            }
        }
        results
    }

    fn match_one(&self, selector: &SelectorList) -> Option<NodeId> {
        self.ctx
            .iter_node_ids()
            .into_iter()
            .find(|&id| self.matches_list(id, selector))
    }

    fn matches_list(&self, id: NodeId, selector: &SelectorList) -> bool {
        selector
            .selectors
            .iter()
            .any(|complex| self.matches_complex(id, complex))
    }

    fn matches_complex(&self, id: NodeId, sel: &ComplexSelector) -> bool {
        let Some(last_compound) = sel.compounds.last() else {
            return false;
        };
        if !self.matches_compound(id, last_compound) {
            return false;
        }
        if sel.combinators.is_empty() {
            return true;
        }
        self.matches_chain_from_right(id, sel, sel.combinators.len() as isize - 1, None, None)
    }

    fn matches_chain_from_right(
        &self,
        current: NodeId,
        sel: &ComplexSelector,
        comb_idx: isize,
        boundary: Option<NodeId>,
        allowed_first: Option<&HashSet<NodeId>>,
    ) -> bool {
        if comb_idx < 0 {
            if let Some(allowed) = allowed_first {
                return allowed.contains(&current);
            }
            return true;
        }

        let comb = sel.combinators[comb_idx as usize];
        let target = &sel.compounds[comb_idx as usize];

        match comb {
            Combinator::Child => {
                let parent = if Some(current) == boundary {
                    None
                } else {
                    self.ctx.parent_of(current)
                };
                if let Some(pid) = parent {
                    if Some(pid) != boundary && self.matches_compound(pid, target) {
                        return self.matches_chain_from_right(
                            pid,
                            sel,
                            comb_idx - 1,
                            boundary,
                            allowed_first,
                        );
                    }
                }
                false
            }
            Combinator::Descendant => {
                let mut cur = if Some(current) == boundary {
                    None
                } else {
                    self.ctx.parent_of(current)
                };
                while let Some(cid) = cur {
                    if Some(cid) == boundary {
                        break;
                    }
                    if self.matches_compound(cid, target)
                        && self.matches_chain_from_right(
                            cid,
                            sel,
                            comb_idx - 1,
                            boundary,
                            allowed_first,
                        )
                    {
                        return true;
                    }
                    cur = self.ctx.parent_of(cid);
                }
                false
            }
            Combinator::Adjacent => {
                if Some(current) == boundary {
                    return false;
                }
                let idx = self.ctx.index_of(current);
                if idx == 0 {
                    return false;
                }
                let siblings = self.ctx.siblings_of(current);
                let prev = siblings[idx - 1];
                if self.matches_compound(prev, target) {
                    return self.matches_chain_from_right(
                        prev,
                        sel,
                        comb_idx - 1,
                        boundary,
                        allowed_first,
                    );
                }
                false
            }
            Combinator::GeneralSibling => {
                if Some(current) == boundary {
                    return false;
                }
                let idx = self.ctx.index_of(current);
                if idx == 0 {
                    return false;
                }
                let siblings = self.ctx.siblings_of(current);
                for &prev in siblings[..idx].iter().rev() {
                    if self.matches_compound(prev, target)
                        && self.matches_chain_from_right(
                            prev,
                            sel,
                            comb_idx - 1,
                            boundary,
                            allowed_first,
                        )
                    {
                        return true;
                    }
                }
                false
            }
        }
    }

    fn matches_compound(&self, id: NodeId, compound: &CompoundSelector) -> bool {
        let node = &self.doc[id];
        if !matches_node_selector(node, &compound.node) {
            return false;
        }
        for filt in &compound.filters {
            if !matches_attribute_filter(node, filt) {
                return false;
            }
        }
        for pseudo in &compound.pseudos {
            if !self.matches_pseudo(id, pseudo, &compound.node) {
                return false;
            }
        }
        true
    }

    fn matches_pseudo(&self, id: NodeId, pseudo: &PseudoClass, node_sel: &NodeSelector) -> bool {
        match pseudo {
            PseudoClass::Not(selectors) => {
                !selectors.iter().any(|sel| self.matches_complex(id, sel))
            }
            PseudoClass::Has(rels) => rels
                .iter()
                .any(|rel| self.matches_has(id, rel.leading, &rel.selector)),
            PseudoClass::Root => self.ctx.parent_of(id).is_none(),
            PseudoClass::Empty => self.doc[id].children.is_empty(),
            PseudoClass::FirstChild => {
                for &s in self.ctx.siblings_of(id) {
                    if matches_node_selector(&self.doc[s], node_sel) {
                        return s == id;
                    }
                }
                false
            }
            PseudoClass::LastChild => {
                for &s in self.ctx.siblings_of(id).iter().rev() {
                    if matches_node_selector(&self.doc[s], node_sel) {
                        return s == id;
                    }
                }
                false
            }
            PseudoClass::OnlyChild => {
                let mut found = false;
                for &s in self.ctx.siblings_of(id) {
                    if matches_node_selector(&self.doc[s], node_sel) {
                        if s == id {
                            found = true;
                        } else {
                            return false;
                        }
                    }
                }
                found
            }
            PseudoClass::NthChild(nth) => {
                let mut pos = 0i64;
                for &s in self.ctx.siblings_of(id) {
                    if matches_node_selector(&self.doc[s], node_sel) {
                        pos += 1;
                        if s == id {
                            return nth.matches_1based(pos);
                        }
                    }
                }
                false
            }
        }
    }

    fn matches_has(&self, id: NodeId, initial: Option<Combinator>, sel: &ComplexSelector) -> bool {
        match initial {
            None | Some(Combinator::Child) => {
                let allowed_first = if initial.is_none() {
                    if sel.combinators.is_empty() {
                        return self
                            .doc
                            .node(id)
                            .iter_descendants()
                            .any(|c| self.matches_compound(c.id(), &sel.compounds[0]));
                    }
                    None
                } else {
                    let matched_children: HashSet<NodeId> = self.doc[id]
                        .children
                        .iter()
                        .copied()
                        .filter(|&c| self.matches_compound(c, &sel.compounds[0]))
                        .collect();
                    if matched_children.is_empty() {
                        return false;
                    }
                    if sel.combinators.is_empty() {
                        return true;
                    }
                    Some(matched_children)
                };

                let last = sel.compounds.last().unwrap();
                let last_comb_idx = sel.combinators.len() as isize - 1;
                for target_node in self.doc.node(id).iter_descendants() {
                    let tid = target_node.id();
                    if self.matches_compound(tid, last)
                        && self.matches_chain_from_right(
                            tid,
                            sel,
                            last_comb_idx,
                            Some(id),
                            allowed_first.as_ref(),
                        )
                    {
                        return true;
                    }
                }
                false
            }
            Some(Combinator::Adjacent | Combinator::GeneralSibling) => {
                let siblings = self.ctx.siblings_of(id);
                let idx = self.ctx.index_of(id);
                if idx + 1 >= siblings.len() {
                    return false;
                }
                let foll_siblings = if initial == Some(Combinator::Adjacent) {
                    &siblings[idx + 1..=idx + 1]
                } else {
                    &siblings[idx + 1..]
                };
                let matched_sibs: HashSet<NodeId> = foll_siblings
                    .iter()
                    .copied()
                    .filter(|&s| self.matches_compound(s, &sel.compounds[0]))
                    .collect();
                if matched_sibs.is_empty() {
                    return false;
                }
                if sel.combinators.is_empty() {
                    return true;
                }
                let boundary = self.ctx.parent_of(id);
                let last = sel.compounds.last().unwrap();
                let last_comb_idx = sel.combinators.len() as isize - 1;
                for &s in &siblings[idx + 1..] {
                    if self.matches_compound(s, last)
                        && self.matches_chain_from_right(
                            s,
                            sel,
                            last_comb_idx,
                            boundary,
                            Some(&matched_sibs),
                        )
                    {
                        return true;
                    }
                    for desc in self.doc.node(s).iter_descendants() {
                        let did = desc.id();
                        if self.matches_compound(did, last)
                            && self.matches_chain_from_right(
                                did,
                                sel,
                                last_comb_idx,
                                boundary,
                                Some(&matched_sibs),
                            )
                        {
                            return true;
                        }
                    }
                }
                false
            }
            Some(Combinator::Descendant) => unreachable!(),
        }
    }
}

struct StandaloneMatcher<'a> {
    node: &'a KdlNode,
}

impl<'a> StandaloneMatcher<'a> {
    fn matches_list(&self, list: &SelectorList) -> bool {
        list.selectors
            .iter()
            .any(|complex| self.matches_complex(complex))
    }

    fn matches_complex(&self, sel: &ComplexSelector) -> bool {
        if !sel.combinators.is_empty() {
            return false;
        }
        sel.compounds
            .first()
            .is_some_and(|c| self.matches_compound(c))
    }

    fn matches_compound(&self, compound: &CompoundSelector) -> bool {
        if !matches_node_selector(self.node, &compound.node) {
            return false;
        }
        for filt in &compound.filters {
            if !matches_attribute_filter(self.node, filt) {
                return false;
            }
        }
        for pseudo in &compound.pseudos {
            if !self.matches_pseudo(pseudo) {
                return false;
            }
        }
        true
    }

    fn matches_pseudo(&self, pseudo: &PseudoClass) -> bool {
        match pseudo {
            PseudoClass::Root
            | PseudoClass::Empty
            | PseudoClass::FirstChild
            | PseudoClass::LastChild
            | PseudoClass::OnlyChild => true,
            PseudoClass::NthChild(nth) => nth.matches_1based(1),
            PseudoClass::Not(selectors) => !selectors.iter().any(|s| self.matches_complex(s)),
            PseudoClass::Has(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Document & Node Entry Points
// ---------------------------------------------------------------------------

pub(crate) fn select_doc<'a>(
    doc: &'a KdlDocument,
    selector: &str,
) -> Result<Vec<NodeRef<'a>>, KdlError> {
    Ok(Selector::parse(selector)?.select(doc))
}

pub(crate) fn select_doc_ids(doc: &KdlDocument, selector: &str) -> Result<Vec<NodeId>, KdlError> {
    Ok(Selector::parse(selector)?.select_ids(doc))
}

pub(crate) fn select_doc_first<'a>(
    doc: &'a KdlDocument,
    selector: &str,
) -> Result<Option<NodeRef<'a>>, KdlError> {
    Ok(Selector::parse(selector)?.select_first(doc))
}

pub(crate) fn select_doc_first_id(
    doc: &KdlDocument,
    selector: &str,
) -> Result<Option<NodeId>, KdlError> {
    Ok(Selector::parse(selector)?.select_first_id(doc))
}

pub(crate) fn select_subtree<'a>(
    node: NodeRef<'a>,
    selector: &str,
) -> Result<Vec<NodeRef<'a>>, KdlError> {
    Ok(Selector::parse(selector)?.select_in(node))
}

pub(crate) fn select_subtree_ids(
    node: NodeRef<'_>,
    selector: &str,
) -> Result<Vec<NodeId>, KdlError> {
    Ok(Selector::parse(selector)?.select_in_ids(node))
}

pub(crate) fn select_subtree_first<'a>(
    node: NodeRef<'a>,
    selector: &str,
) -> Result<Option<NodeRef<'a>>, KdlError> {
    Ok(Selector::parse(selector)?.select_first_in(node))
}

pub(crate) fn select_subtree_first_id(
    node: NodeRef<'_>,
    selector: &str,
) -> Result<Option<NodeId>, KdlError> {
    Ok(Selector::parse(selector)?.select_first_in_id(node))
}

pub(crate) fn try_node_matches(node: NodeRef<'_>, selector: &str) -> Result<bool, KdlError> {
    Ok(Selector::parse(selector)?.matches(node))
}

pub(crate) fn try_node_closest<'a>(
    node: NodeRef<'a>,
    selector: &str,
) -> Result<Option<NodeRef<'a>>, KdlError> {
    let sel = Selector::parse(selector)?;
    let mut cur = Some(node);
    while let Some(n) = cur {
        if sel.matches(n) {
            return Ok(Some(n));
        }
        cur = n.parent();
    }
    Ok(None)
}

pub(crate) fn try_standalone_matches(node: &KdlNode, selector: &str) -> Result<bool, KdlError> {
    Ok(Selector::parse(selector)?.matches_standalone(node))
}

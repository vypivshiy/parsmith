//! Pure-Rust compile-time CSS3-to-XPath 1.0 lowering (`css_to_xpath`).
//!
//! Translates CSS3 selectors (and lowerable CSS4 selectors such as `:not(...)`, `:is(...)`,
//! `:where(...)`, `:has(...)`, and case-insensitive `[attr=val i]`) into relative W3C XPath 1.0
//! queries anchored at the context node (`.//...` or `./...`) suitable for `\DOMXPath::query()`.

use cssparser::{Parser, ParserInput, Token};

use super::css::{
    check_balanced_delimiters, format_css_error, format_parse_error, next_token_including_ws,
    skip_whitespace,
};

const UPPER_ASCII: &str = "'ABCDEFGHIJKLMNOPQRSTUVWXYZ'";
const LOWER_ASCII: &str = "'abcdefghijklmnopqrstuvwxyz'";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Debug, Clone)]
struct CompoundSelector {
    namespace: Option<String>,
    tag: Option<String>,
    predicates: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    Exact,
    NotEqual,
    Include,
    Dash,
    Prefix,
    Suffix,
    Substring,
}

/// Translates a CSS selector string into a relative XPath 1.0 expression anchored at `.`.
///
/// Returns `Err(reason)` if the selector syntax is malformed or uses CSS4 pseudo-classes /
/// pseudo-elements that cannot be represented in XPath 1.0.
pub fn css_to_xpath(selector: &str) -> Result<String, String> {
    let trimmed = selector.trim();
    if trimmed.is_empty() {
        return Err("empty CSS selector".to_string());
    }

    check_balanced_delimiters(trimmed)?;

    let mut input = ParserInput::new(trimmed);
    let mut parser = Parser::new(&mut input);
    let paths = parse_selector_list_to_xpath(&mut parser, true)?;
    Ok(paths.join(" | "))
}

/// Formats an arbitrary string as a valid XPath 1.0 string literal (`'...'`, `"..."`, or `concat(...)`).
pub fn xpath_literal(s: &str) -> String {
    if !s.contains('\'') {
        format!("'{s}'")
    } else if !s.contains('"') {
        format!("\"{s}\"")
    } else {
        let mut parts: Vec<String> = Vec::new();
        for (idx, chunk) in s.split('\'').enumerate() {
            if idx > 0 {
                parts.push("\"'\" ".trim().to_string());
            }
            if !chunk.is_empty() {
                parts.push(format!("'{chunk}'"));
            }
        }
        format!("concat({})", parts.join(", "))
    }
}

fn parse_selector_list_to_xpath<'i, 't>(
    parser: &mut Parser<'i, 't>,
    allow_leading_combinator: bool,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    out.push(parse_complex_selector_to_xpath(
        parser,
        allow_leading_combinator,
    )?);
    while !parser.is_exhausted() {
        let token = next_token_including_ws(parser)?;
        match token {
            Token::Comma => {
                out.push(parse_complex_selector_to_xpath(
                    parser,
                    allow_leading_combinator,
                )?);
            }
            other => {
                return Err(format!("unexpected token in selector list: {other:?}"));
            }
        }
    }
    Ok(out)
}

fn parse_complex_selector_steps<'i, 't>(
    parser: &mut Parser<'i, 't>,
    allow_leading_combinator: bool,
) -> Result<Vec<(Combinator, CompoundSelector)>, String> {
    skip_whitespace(parser)?;

    let mut leading_comb = Combinator::Descendant;
    if allow_leading_combinator {
        let state = parser.state();
        if let Ok(tok) = next_token_including_ws(parser) {
            match tok {
                Token::Delim('>') => {
                    leading_comb = Combinator::Child;
                    skip_whitespace(parser)?;
                }
                Token::Delim('+') => {
                    leading_comb = Combinator::NextSibling;
                    skip_whitespace(parser)?;
                }
                Token::Delim('~') => {
                    leading_comb = Combinator::SubsequentSibling;
                    skip_whitespace(parser)?;
                }
                _ => {
                    parser.reset(&state);
                }
            }
        } else {
            parser.reset(&state);
        }
    }

    let first_compound = parse_compound_selector(parser)?;
    let mut steps = vec![(leading_comb, first_compound)];

    loop {
        let mut saw_ws = false;
        loop {
            let state = parser.state();
            match parser.next_including_whitespace_and_comments() {
                Ok(Token::WhiteSpace(_) | Token::Comment(_)) => {
                    saw_ws = true;
                }
                Ok(_) => {
                    parser.reset(&state);
                    break;
                }
                Err(_) => break,
            }
        }

        if parser.is_exhausted() {
            break;
        }

        let state = parser.state();
        let tok = next_token_including_ws(parser)?;
        match tok {
            Token::Comma => {
                parser.reset(&state);
                break;
            }
            Token::Delim(c @ ('>' | '+' | '~')) => {
                let comb = match c {
                    '>' => Combinator::Child,
                    '+' => Combinator::NextSibling,
                    '~' => Combinator::SubsequentSibling,
                    _ => unreachable!(),
                };
                skip_whitespace(parser)?;
                if parser.is_exhausted() {
                    return Err("dangling combinator at end of selector".to_string());
                }
                let after_state = parser.state();
                if let Ok(Token::Comma) = next_token_including_ws(parser) {
                    return Err("dangling combinator before ','".to_string());
                }
                parser.reset(&after_state);
                let next_compound = parse_compound_selector(parser)?;
                steps.push((comb, next_compound));
            }
            _ if saw_ws => {
                parser.reset(&state);
                let next_compound = parse_compound_selector(parser)?;
                steps.push((Combinator::Descendant, next_compound));
            }
            other => {
                return Err(format!("unexpected token in selector: {other:?}"));
            }
        }
    }

    Ok(steps)
}

fn parse_complex_selector_to_xpath<'i, 't>(
    parser: &mut Parser<'i, 't>,
    allow_leading_combinator: bool,
) -> Result<String, String> {
    let steps = parse_complex_selector_steps(parser, allow_leading_combinator)?;
    let mut xpath = String::new();
    for (idx, (comb, compound)) in steps.iter().enumerate() {
        let step_str = format_compound_step(compound);
        if idx == 0 {
            match comb {
                Combinator::Descendant => {
                    xpath.push_str(".//");
                    xpath.push_str(&step_str);
                }
                Combinator::Child => {
                    xpath.push_str("./");
                    xpath.push_str(&step_str);
                }
                Combinator::NextSibling => {
                    xpath.push_str("./following-sibling::*[1]/self::");
                    xpath.push_str(&step_str);
                }
                Combinator::SubsequentSibling => {
                    xpath.push_str("./following-sibling::");
                    xpath.push_str(&step_str);
                }
            }
        } else {
            match comb {
                Combinator::Descendant => {
                    xpath.push_str("//");
                    xpath.push_str(&step_str);
                }
                Combinator::Child => {
                    xpath.push('/');
                    xpath.push_str(&step_str);
                }
                Combinator::NextSibling => {
                    xpath.push_str("/following-sibling::*[1]/self::");
                    xpath.push_str(&step_str);
                }
                Combinator::SubsequentSibling => {
                    xpath.push_str("/following-sibling::");
                    xpath.push_str(&step_str);
                }
            }
        }
    }
    Ok(xpath)
}

fn format_compound_step(compound: &CompoundSelector) -> String {
    let mut preds = Vec::new();
    let tag_out = match (&compound.namespace, &compound.tag) {
        (Some(_), Some(t)) => {
            preds.push(format!("local-name() = {}", xpath_literal(t)));
            "*"
        }
        (None, Some(t)) => t.as_str(),
        (_, None) => "*",
    };
    preds.extend(compound.predicates.iter().cloned());

    if preds.is_empty() {
        tag_out.to_string()
    } else {
        let joined = join_xpath_and(&preds);
        format!("{tag_out}[{joined}]")
    }
}

fn format_compound_as_condition(compound: &CompoundSelector) -> String {
    let mut parts = Vec::new();
    match (&compound.namespace, &compound.tag) {
        (Some(_), Some(t)) => {
            parts.push(format!("local-name() = {}", xpath_literal(t)));
        }
        (None, Some(t)) => {
            parts.push(format!("self::{t}"));
        }
        (_, None) => {}
    }
    parts.extend(compound.predicates.iter().cloned());
    if parts.is_empty() {
        "self::*".to_string()
    } else {
        join_xpath_and(&parts)
    }
}

fn join_xpath_and(preds: &[String]) -> String {
    if preds.len() == 1 {
        return preds[0].clone();
    }
    preds
        .iter()
        .map(|p| {
            if has_top_level_or(p) {
                format!("({p})")
            } else {
                p.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

fn join_xpath_or(preds: &[String]) -> String {
    if preds.len() == 1 {
        return preds[0].clone();
    }
    preds
        .iter()
        .map(|p| {
            if has_top_level_and(p) {
                format!("({p})")
            } else {
                p.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" or ")
}

fn has_top_level_or(expr: &str) -> bool {
    has_top_level_keyword(expr, " or ")
}

fn has_top_level_and(expr: &str) -> bool {
    has_top_level_keyword(expr, " and ")
}

fn has_top_level_keyword(expr: &str, kw: &str) -> bool {
    let bytes = expr.as_bytes();
    let kw_bytes = kw.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            _ if depth == 0 && bytes[i..].starts_with(kw_bytes) => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

fn parse_compound_selector<'i, 't>(
    parser: &mut Parser<'i, 't>,
) -> Result<CompoundSelector, String> {
    let mut count = 0usize;
    let mut seen_type_or_universal = false;
    let mut namespace: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut predicates: Vec<String> = Vec::new();

    loop {
        if parser.is_exhausted() {
            break;
        }
        let state = parser.state();
        let tok = match parser.next_including_whitespace_and_comments() {
            Ok(t) => t.clone(),
            Err(err) => return Err(format_parse_error(&err.kind)),
        };

        match tok {
            Token::WhiteSpace(_) | Token::Comment(_) | Token::Comma => {
                parser.reset(&state);
                break;
            }
            Token::Delim('>' | '+' | '~') => {
                if count == 0 {
                    return Err("unexpected leading combinator".to_string());
                }
                parser.reset(&state);
                break;
            }
            Token::Ident(ref name) => {
                if count > 0 || seen_type_or_universal {
                    return Err(format!(
                        "type selector '{name}' must appear at the start of a compound selector"
                    ));
                }
                seen_type_or_universal = true;
                count += 1;
                if let Some(local) = consume_optional_namespace_tail(parser)? {
                    if name.as_ref() != "*" {
                        namespace = Some(name.as_ref().to_string());
                    }
                    tag = local;
                } else {
                    tag = Some(name.as_ref().to_string());
                }
            }
            Token::Delim('*') => {
                if count > 0 || seen_type_or_universal {
                    return Err(
                        "universal selector '*' must appear at the start of a compound selector"
                            .to_string(),
                    );
                }
                seen_type_or_universal = true;
                count += 1;
                if let Some(local) = consume_optional_namespace_tail(parser)? {
                    tag = local;
                } else {
                    tag = None;
                }
            }
            Token::Delim('|') => {
                if count > 0 || seen_type_or_universal {
                    return Err(
                        "namespace separator '|' must appear at the start of a compound selector"
                            .to_string(),
                    );
                }
                let next_tok = parser
                    .next_including_whitespace_and_comments()
                    .map_err(|e| format_parse_error(&e.kind))?;
                match next_tok {
                    Token::Ident(local) => {
                        seen_type_or_universal = true;
                        count += 1;
                        tag = Some(local.as_ref().to_string());
                    }
                    Token::Delim('*') => {
                        seen_type_or_universal = true;
                        count += 1;
                        tag = None;
                    }
                    other => {
                        return Err(format!("expected tag name or '*' after '|', got {other:?}"));
                    }
                }
            }
            Token::IDHash(ref id_name) => {
                if id_name.is_empty() {
                    return Err("empty ID selector '#'".to_string());
                }
                predicates.push(format!("@id = {}", xpath_literal(id_name.as_ref())));
                count += 1;
            }
            Token::Hash(_) => {
                return Err("invalid ID selector identifier after '#'".to_string());
            }
            Token::Delim('.') => {
                let next_tok = parser
                    .next_including_whitespace_and_comments()
                    .map_err(|_| "expected class name after '.'".to_string())?;
                match next_tok {
                    Token::Ident(cls) if !cls.is_empty() => {
                        let needle = format!(" {} ", cls.as_ref());
                        predicates.push(format!(
                            "contains(concat(' ', normalize-space(@class), ' '), {})",
                            xpath_literal(&needle)
                        ));
                        count += 1;
                    }
                    other => {
                        return Err(format!(
                            "expected class identifier after '.', got {other:?}"
                        ));
                    }
                }
            }
            Token::SquareBracketBlock => {
                let pred = parser
                    .parse_nested_block(|nested| {
                        parse_attribute_selector_to_xpath(nested)
                            .map_err(|msg| nested.new_custom_error::<String, String>(msg))
                    })
                    .map_err(format_css_error)?;
                predicates.push(pred);
                count += 1;
            }
            Token::Colon => {
                let pred = parse_pseudo_to_xpath(parser, tag.as_deref())?;
                predicates.push(pred);
                count += 1;
            }
            other => {
                return Err(format!("unexpected token in selector: {other:?}"));
            }
        }
    }

    if count == 0 {
        return Err("expected selector".to_string());
    }

    Ok(CompoundSelector {
        namespace,
        tag,
        predicates,
    })
}

fn consume_optional_namespace_tail<'i, 't>(
    parser: &mut Parser<'i, 't>,
) -> Result<Option<Option<String>>, String> {
    let state = parser.state();
    if let Ok(Token::Delim('|')) = parser.next_including_whitespace_and_comments() {
        let next_tok = parser
            .next_including_whitespace_and_comments()
            .map_err(|_| "expected element name or '*' after namespace '|'".to_string())?;
        match next_tok {
            Token::Ident(local) => Ok(Some(Some(local.as_ref().to_string()))),
            Token::Delim('*') => Ok(Some(None)),
            other => Err(format!(
                "expected element name or '*' after namespace '|', got {other:?}"
            )),
        }
    } else {
        parser.reset(&state);
        Ok(None)
    }
}

fn parse_attribute_selector_to_xpath<'i, 't>(
    parser: &mut Parser<'i, 't>,
) -> Result<String, String> {
    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Err("empty attribute selector '[]'".to_string());
    }

    let first = next_token_including_ws(parser)?;
    let attr_name: String = match first {
        Token::Ident(ref name) => {
            let state = parser.state();
            if let Ok(Token::Delim('|')) = parser.next_including_whitespace_and_comments() {
                let after = parser
                    .next_including_whitespace_and_comments()
                    .map_err(|_| "expected attribute name after '|'".to_string())?;
                if let Token::Ident(local) = after {
                    if name.as_ref() == "*" || name.is_empty() {
                        local.as_ref().to_string()
                    } else {
                        format!("{}:{}", name.as_ref(), local.as_ref())
                    }
                } else {
                    return Err("expected attribute name after '|'".to_string());
                }
            } else {
                parser.reset(&state);
                name.as_ref().to_string()
            }
        }
        Token::Delim('*') => {
            if let Ok(Token::Delim('|')) = parser.next_including_whitespace_and_comments() {
                let after = parser
                    .next_including_whitespace_and_comments()
                    .map_err(|_| "expected attribute name after '|'".to_string())?;
                if let Token::Ident(local) = after {
                    local.as_ref().to_string()
                } else {
                    return Err("expected attribute name after '|'".to_string());
                }
            } else {
                return Err("expected '|' after '*' in attribute selector".to_string());
            }
        }
        Token::Delim('|') => {
            let after = parser
                .next_including_whitespace_and_comments()
                .map_err(|_| "expected attribute name after '|'".to_string())?;
            if let Token::Ident(local) = after {
                local.as_ref().to_string()
            } else {
                return Err("expected attribute name after '|'".to_string());
            }
        }
        other => {
            return Err(format!("expected attribute name in '[...]', got {other:?}"));
        }
    };

    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Ok(format!("@{attr_name}"));
    }

    let op_tok = next_token_including_ws(parser)?;
    let op = match op_tok {
        Token::Delim('=') => AttrOp::Exact,
        Token::Delim('!') => {
            let eq_tok = next_token_including_ws(parser)?;
            if !matches!(eq_tok, Token::Delim('=')) {
                return Err(format!(
                    "expected '=' after '!' in attribute selector, got {eq_tok:?}"
                ));
            }
            AttrOp::NotEqual
        }
        Token::IncludeMatch => AttrOp::Include,
        Token::DashMatch => AttrOp::Dash,
        Token::PrefixMatch => AttrOp::Prefix,
        Token::SuffixMatch => AttrOp::Suffix,
        Token::SubstringMatch => AttrOp::Substring,
        other => {
            return Err(format!("invalid attribute operator in '[...]': {other:?}"));
        }
    };

    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Err("missing attribute value after operator in '[...]'".to_string());
    }

    let val_tok = next_token_including_ws(parser)?;
    let raw_val = match val_tok {
        Token::Ident(ref s) | Token::QuotedString(ref s) => s.as_ref().to_string(),
        other => {
            return Err(format!(
                "attribute value must be an identifier or string, got {other:?}"
            ));
        }
    };

    let mut case_insensitive = false;
    skip_whitespace(parser)?;
    if !parser.is_exhausted() {
        let modifier_tok = next_token_including_ws(parser)?;
        match modifier_tok {
            Token::Ident(ref flag) if flag.eq_ignore_ascii_case("i") => {
                case_insensitive = true;
            }
            Token::Ident(ref flag) if flag.eq_ignore_ascii_case("s") => {
                case_insensitive = false;
            }
            other => {
                return Err(format!(
                    "unexpected token after attribute value in '[...]': {other:?}"
                ));
            }
        }
        skip_whitespace(parser)?;
        if !parser.is_exhausted() {
            return Err("unexpected trailing tokens in attribute selector '[...]'".to_string());
        }
    }

    let attr_ref = format!("@{attr_name}");
    let attr_expr = if case_insensitive {
        format!("translate({attr_ref}, {UPPER_ASCII}, {LOWER_ASCII})")
    } else {
        attr_ref.clone()
    };
    let val = if case_insensitive {
        raw_val.to_ascii_lowercase()
    } else {
        raw_val
    };

    let expr = match op {
        AttrOp::Exact => format!("{attr_expr} = {}", xpath_literal(&val)),
        AttrOp::NotEqual => {
            format!("not({attr_ref}) or {attr_expr} != {}", xpath_literal(&val))
        }
        AttrOp::Include => {
            if val.is_empty() || val.chars().any(char::is_whitespace) {
                "false()".to_string()
            } else {
                let needle = format!(" {val} ");
                format!(
                    "contains(concat(' ', normalize-space({attr_expr}), ' '), {})",
                    xpath_literal(&needle)
                )
            }
        }
        AttrOp::Dash => {
            let prefix = format!("{val}-");
            format!(
                "{attr_expr} = {} or starts-with({attr_expr}, {})",
                xpath_literal(&val),
                xpath_literal(&prefix)
            )
        }
        AttrOp::Prefix => {
            if val.is_empty() {
                "false()".to_string()
            } else {
                format!("starts-with({attr_expr}, {})", xpath_literal(&val))
            }
        }
        AttrOp::Suffix => {
            if val.is_empty() {
                "false()".to_string()
            } else {
                let lit = xpath_literal(&val);
                format!(
                    "substring({attr_expr}, string-length({attr_ref}) - string-length({lit}) + 1) = {lit}"
                )
            }
        }
        AttrOp::Substring => {
            if val.is_empty() {
                "false()".to_string()
            } else {
                format!("contains({attr_expr}, {})", xpath_literal(&val))
            }
        }
    };

    Ok(expr)
}

fn parse_pseudo_to_xpath<'i, 't>(
    parser: &mut Parser<'i, 't>,
    tag_name: Option<&str>,
) -> Result<String, String> {
    let tok = parser
        .next_including_whitespace_and_comments()
        .map_err(|_| "expected pseudo-class or pseudo-element after ':'".to_string())?
        .clone();

    match tok {
        Token::Colon => {
            let elem_tok = parser
                .next_including_whitespace_and_comments()
                .map_err(|_| "expected pseudo-element name after '::'".to_string())?
                .clone();
            match elem_tok {
                Token::Ident(ref name) | Token::Function(ref name) => Err(format!(
                    "CSS pseudo-element '::{name}' cannot be lowered to XPath 1.0"
                )),
                other => Err(format!(
                    "expected pseudo-element identifier after '::', got {other:?}"
                )),
            }
        }
        Token::Ident(ref name) if !name.is_empty() => {
            let lower = name.to_ascii_lowercase();
            match lower.as_str() {
                "first-child" => Ok("count(preceding-sibling::*) = 0".to_string()),
                "last-child" => Ok("count(following-sibling::*) = 0".to_string()),
                "only-child" => Ok(
                    "count(preceding-sibling::*) = 0 and count(following-sibling::*) = 0"
                        .to_string(),
                ),
                "first-of-type" => {
                    let t = require_explicit_tag_for_of_type(tag_name, "first-of-type")?;
                    Ok(format!("count(preceding-sibling::{t}) = 0"))
                }
                "last-of-type" => {
                    let t = require_explicit_tag_for_of_type(tag_name, "last-of-type")?;
                    Ok(format!("count(following-sibling::{t}) = 0"))
                }
                "only-of-type" => {
                    let t = require_explicit_tag_for_of_type(tag_name, "only-of-type")?;
                    Ok(format!(
                        "count(preceding-sibling::{t}) = 0 and count(following-sibling::{t}) = 0"
                    ))
                }
                "empty" => Ok("not(node())".to_string()),
                "root" => Ok("not(parent::*)".to_string()),
                "checked" => Ok("@checked".to_string()),
                "disabled" => Ok("@disabled".to_string()),
                "enabled" => Ok("not(@disabled)".to_string()),
                _ => Err(format!(
                    "unsupported CSS pseudo-class ':{name}' for XPath 1.0 lowering"
                )),
            }
        }
        Token::Function(ref name) if !name.is_empty() => {
            let lower = name.to_ascii_lowercase();
            parser
                .parse_nested_block(|nested| {
                    translate_functional_pseudo(name.as_ref(), &lower, tag_name, nested)
                        .map_err(|msg| nested.new_custom_error::<String, String>(msg))
                })
                .map_err(format_css_error)
        }
        other => Err(format!(
            "expected pseudo-class name after ':', got {other:?}"
        )),
    }
}

fn require_explicit_tag_for_of_type<'a>(
    tag_name: Option<&'a str>,
    pseudo: &str,
) -> Result<&'a str, String> {
    match tag_name {
        Some(t) if !t.is_empty() && t != "*" => Ok(t),
        _ => Err(format!(
            "':{pseudo}' without an explicit element tag name cannot be lowered to XPath 1.0"
        )),
    }
}

fn translate_functional_pseudo<'i, 't>(
    raw_name: &str,
    lower_name: &str,
    tag_name: Option<&str>,
    parser: &mut Parser<'i, 't>,
) -> Result<String, String> {
    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Err(format!("':{raw_name}()' requires arguments"));
    }

    match lower_name {
        "not" => {
            let conds = parse_compound_selector_list_as_conditions(parser, raw_name)?;
            let inner = join_xpath_or(&conds);
            Ok(format!("not({inner})"))
        }
        "is" | "where" | "matches" => {
            let conds = parse_compound_selector_list_as_conditions(parser, raw_name)?;
            Ok(join_xpath_or(&conds))
        }
        "has" => {
            let rel_paths = parse_selector_list_to_xpath(parser, true)?;
            Ok(rel_paths.join(" or "))
        }
        "nth-child" => {
            let (a, b) = parse_nth_args(parser, "nth-child")?;
            Ok(format_nth_xpath("count(preceding-sibling::*)", a, b))
        }
        "nth-last-child" => {
            let (a, b) = parse_nth_args(parser, "nth-last-child")?;
            Ok(format_nth_xpath("count(following-sibling::*)", a, b))
        }
        "nth-of-type" => {
            let t = require_explicit_tag_for_of_type(tag_name, "nth-of-type")?;
            let (a, b) = parse_nth_args(parser, "nth-of-type")?;
            let axis_count = format!("count(preceding-sibling::{t})");
            Ok(format_nth_xpath(&axis_count, a, b))
        }
        "nth-last-of-type" => {
            let t = require_explicit_tag_for_of_type(tag_name, "nth-last-of-type")?;
            let (a, b) = parse_nth_args(parser, "nth-last-of-type")?;
            let axis_count = format!("count(following-sibling::{t})");
            Ok(format_nth_xpath(&axis_count, a, b))
        }
        _ => Err(format!(
            "unsupported CSS functional pseudo-class ':{raw_name}()' for XPath 1.0 lowering"
        )),
    }
}

fn parse_compound_selector_list_as_conditions<'i, 't>(
    parser: &mut Parser<'i, 't>,
    pseudo_name: &str,
) -> Result<Vec<String>, String> {
    let mut conds = Vec::new();
    let first_steps = parse_complex_selector_steps(parser, false)?;
    if first_steps.len() != 1 {
        return Err(format!(
            "complex selector with combinators inside ':{pseudo_name}()' cannot be lowered to XPath 1.0"
        ));
    }
    conds.push(format_compound_as_condition(&first_steps[0].1));

    while !parser.is_exhausted() {
        let tok = next_token_including_ws(parser)?;
        match tok {
            Token::Comma => {
                let steps = parse_complex_selector_steps(parser, false)?;
                if steps.len() != 1 {
                    return Err(format!(
                        "complex selector with combinators inside ':{pseudo_name}()' cannot be lowered to XPath 1.0"
                    ));
                }
                conds.push(format_compound_as_condition(&steps[0].1));
            }
            other => {
                return Err(format!(
                    "unexpected token in ':{pseudo_name}()' selector list: {other:?}"
                ));
            }
        }
    }

    Ok(conds)
}

fn parse_nth_args<'i, 't>(
    parser: &mut Parser<'i, 't>,
    pseudo_name: &str,
) -> Result<(i64, i64), String> {
    let mut expr_str = String::new();

    while !parser.is_exhausted() {
        let tok = next_token_including_ws(parser)?;
        match tok {
            Token::WhiteSpace(_) | Token::Comment(_) => {
                expr_str.push(' ');
            }
            Token::Ident(ref ident) if ident.eq_ignore_ascii_case("of") => {
                return Err(format!(
                    "CSS4 ':{pseudo_name}(... of ...)' selector filtering is not supported in XPath 1.0"
                ));
            }
            Token::Ident(ref ident) => {
                expr_str.push_str(ident);
            }
            Token::Number {
                has_sign,
                int_value: Some(i),
                ..
            } => {
                if has_sign && i >= 0 {
                    expr_str.push('+');
                }
                expr_str.push_str(&i.to_string());
            }
            Token::Dimension {
                has_sign,
                int_value: Some(i),
                ref unit,
                ..
            } => {
                if has_sign && i >= 0 {
                    expr_str.push('+');
                }
                expr_str.push_str(&i.to_string());
                expr_str.push_str(unit);
            }
            Token::Delim(c @ ('+' | '-')) => {
                expr_str.push(c);
            }
            other => {
                return Err(format!(
                    "invalid token in ':{pseudo_name}()' expression: {other:?}"
                ));
            }
        }
    }

    parse_an_plus_b(expr_str.trim())
}

pub(crate) fn parse_an_plus_b(expr: &str) -> Result<(i64, i64), String> {
    if expr.is_empty() {
        return Err("empty :nth-*() expression".to_string());
    }
    let lower = expr.to_ascii_lowercase();
    if lower == "odd" {
        return Ok((2, 1));
    }
    if lower == "even" {
        return Ok((2, 0));
    }
    let compact: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    if let Ok(b) = compact.parse::<i64>() {
        return Ok((0, b));
    }
    if let Some((a_part, b_part)) = compact.split_once('n') {
        let a = match a_part {
            "" | "+" => 1,
            "-" => -1,
            other => other
                .parse::<i64>()
                .map_err(|_| format!("invalid An+B expression '{expr}'"))?,
        };
        let b = if b_part.is_empty() {
            0
        } else if (b_part.starts_with('+') || b_part.starts_with('-'))
            && b_part[1..].parse::<u64>().is_ok()
        {
            b_part
                .parse::<i64>()
                .map_err(|_| format!("invalid An+B expression '{expr}'"))?
        } else {
            return Err(format!("invalid An+B expression '{expr}'"));
        };
        return Ok((a, b));
    }
    Err(format!("invalid An+B expression '{expr}'"))
}

fn format_nth_xpath(axis_count: &str, a: i64, b: i64) -> String {
    if a == 0 {
        if b <= 0 {
            return "false()".to_string();
        }
        return format!("{axis_count} = {}", b - 1);
    }

    if a == 1 {
        if b <= 1 {
            return "true()".to_string();
        }
        return format!("{axis_count} >= {}", b - 1);
    }

    if a == 2 && (b == 1 || b == -1) {
        return format!("({axis_count} + 1) mod 2 = 1");
    }

    if a == 2 && b == 0 {
        return format!("({axis_count} + 1) mod 2 = 0");
    }

    if a > 0 {
        if b > 1 {
            format!(
                "{axis_count} >= {} and ({axis_count} + 1 - {b}) mod {a} = 0",
                b - 1
            )
        } else if b == 1 {
            format!("{axis_count} mod {a} = 0")
        } else if b == 0 {
            format!("({axis_count} + 1) mod {a} = 0")
        } else {
            let r = ((b % a) + a) % a;
            format!("({axis_count} + 1) mod {a} = {r}")
        }
    } else {
        if b <= 0 {
            return "false()".to_string();
        }
        let abs_a = -a;
        if abs_a == 1 {
            format!("{axis_count} <= {}", b - 1)
        } else {
            format!(
                "{axis_count} <= {} and ({b} - ({axis_count} + 1)) mod {abs_a} = 0",
                b - 1
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validators::validate_xpath;

    fn assert_valid_xpath(css: &str, expected: &str) {
        let actual =
            css_to_xpath(css).unwrap_or_else(|e| panic!("css_to_xpath({css:?}) failed: {e}"));
        assert_eq!(actual, expected, "mismatch for CSS selector: {css}");
        validate_xpath(&actual)
            .unwrap_or_else(|e| panic!("generated XPath {actual:?} for {css:?} is invalid: {e}"));
    }

    #[test]
    fn translates_tags_classes_ids_and_universal() {
        assert_valid_xpath("div", ".//div");
        assert_valid_xpath("*", ".//*");
        assert_valid_xpath("#main-content", ".//*[@id = 'main-content']");
        assert_valid_xpath(
            ".card",
            ".//*[contains(concat(' ', normalize-space(@class), ' '), ' card ')]",
        );
        assert_valid_xpath(
            "article.product_pod#book-1",
            ".//article[contains(concat(' ', normalize-space(@class), ' '), ' product_pod ') and @id = 'book-1']",
        );
        assert_valid_xpath("svg|path", ".//*[local-name() = 'path']");
    }

    #[test]
    fn translates_all_attribute_operators_and_case_sensitivity_flags() {
        assert_valid_xpath("a[href]", ".//a[@href]");
        assert_valid_xpath("meta[name='description']", ".//meta[@name = 'description']");
        assert_valid_xpath(
            "link[rel~='icon']",
            ".//link[contains(concat(' ', normalize-space(@rel), ' '), ' icon ')]",
        );
        assert_valid_xpath(
            "p[lang|='en']",
            ".//p[@lang = 'en' or starts-with(@lang, 'en-')]",
        );
        assert_valid_xpath(
            "a[href^='https://']",
            ".//a[starts-with(@href, 'https://')]",
        );
        assert_valid_xpath(
            "a[href$='.pdf']",
            ".//a[substring(@href, string-length(@href) - string-length('.pdf') + 1) = '.pdf']",
        );
        assert_valid_xpath("a[href*='example']", ".//a[contains(@href, 'example')]");
        assert_valid_xpath(
            "a[rel!='nofollow']",
            ".//a[not(@rel) or @rel != 'nofollow']",
        );
        assert_valid_xpath(
            "a[data-kind*='Nav' i]",
            ".//a[contains(translate(@data-kind, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'), 'nav')]",
        );
        assert_valid_xpath("a[data-kind='Nav' s]", ".//a[@data-kind = 'Nav']");
    }

    #[test]
    fn translates_combinators_and_selector_unions() {
        assert_valid_xpath("table tr", ".//table//tr");
        assert_valid_xpath("h3 > a", ".//h3/a");
        assert_valid_xpath(
            "h2 + p.lead",
            ".//h2/following-sibling::*[1]/self::p[contains(concat(' ', normalize-space(@class), ' '), ' lead ')]",
        );
        assert_valid_xpath("h2 ~ p", ".//h2/following-sibling::p");
        assert_valid_xpath(
            "meta[property='og:title'][content], meta[name='og:title'][content]",
            ".//meta[@property = 'og:title' and @content] | .//meta[@name = 'og:title' and @content]",
        );
    }

    #[test]
    fn translates_structural_and_logical_pseudo_classes() {
        assert_valid_xpath("li:first-child", ".//li[count(preceding-sibling::*) = 0]");
        assert_valid_xpath("li:last-child", ".//li[count(following-sibling::*) = 0]");
        assert_valid_xpath(
            "li:only-child",
            ".//li[count(preceding-sibling::*) = 0 and count(following-sibling::*) = 0]",
        );
        assert_valid_xpath("p:first-of-type", ".//p[count(preceding-sibling::p) = 0]");
        assert_valid_xpath("p:last-of-type", ".//p[count(following-sibling::p) = 0]");
        assert_valid_xpath(
            "p:only-of-type",
            ".//p[count(preceding-sibling::p) = 0 and count(following-sibling::p) = 0]",
        );
        assert_valid_xpath("tr:nth-child(1)", ".//tr[count(preceding-sibling::*) = 0]");
        assert_valid_xpath("tr:nth-child(3)", ".//tr[count(preceding-sibling::*) = 2]");
        assert_valid_xpath(
            "tr:nth-child(odd)",
            ".//tr[(count(preceding-sibling::*) + 1) mod 2 = 1]",
        );
        assert_valid_xpath(
            "tr:nth-child(even)",
            ".//tr[(count(preceding-sibling::*) + 1) mod 2 = 0]",
        );
        assert_valid_xpath(
            "tr:nth-child(2n+1)",
            ".//tr[(count(preceding-sibling::*) + 1) mod 2 = 1]",
        );
        assert_valid_xpath(
            "tr:nth-child(3n+2)",
            ".//tr[count(preceding-sibling::*) >= 1 and (count(preceding-sibling::*) + 1 - 2) mod 3 = 0]",
        );
        assert_valid_xpath(
            "tr:nth-child(-n+3)",
            ".//tr[count(preceding-sibling::*) <= 2]",
        );
        assert_valid_xpath(
            "tr:nth-last-child(2)",
            ".//tr[count(following-sibling::*) = 1]",
        );
        assert_valid_xpath(
            "td:nth-of-type(even)",
            ".//td[(count(preceding-sibling::td) + 1) mod 2 = 0]",
        );
        assert_valid_xpath(
            "td:nth-last-of-type(1)",
            ".//td[count(following-sibling::td) = 0]",
        );
        assert_valid_xpath("div:empty", ".//div[not(node())]");
        assert_valid_xpath("html:root", ".//html[not(parent::*)]");
        assert_valid_xpath("input:checked", ".//input[@checked]");
        assert_valid_xpath("input:disabled", ".//input[@disabled]");
        assert_valid_xpath("input:enabled", ".//input[not(@disabled)]");
        assert_valid_xpath(
            "div:not(.ad, .promo)",
            ".//div[not(contains(concat(' ', normalize-space(@class), ' '), ' ad ') or contains(concat(' ', normalize-space(@class), ' '), ' promo '))]",
        );
        assert_valid_xpath(
            "div:is(.card, article[data-id])",
            ".//div[contains(concat(' ', normalize-space(@class), ' '), ' card ') or (self::article and @data-id)]",
        );
        assert_valid_xpath(
            "div.card:has(> img.poster, + .caption)",
            ".//div[contains(concat(' ', normalize-space(@class), ' '), ' card ') and (./img[contains(concat(' ', normalize-space(@class), ' '), ' poster ')] or ./following-sibling::*[1]/self::*[contains(concat(' ', normalize-space(@class), ' '), ' caption ')])]",
        );
    }

    #[test]
    fn escapes_xpath_string_literals_with_single_and_double_quotes() {
        assert_eq!(xpath_literal("simple"), "'simple'");
        assert_eq!(xpath_literal("it's"), "\"it's\"");
        assert_eq!(xpath_literal("a'b\"c"), "concat('a', \"'\", 'b\"c')");
        let xp = css_to_xpath(r#"a[title="a'b\"c"]"#).expect("quote escaping");
        validate_xpath(&xp).expect("valid escaped XPath");
    }

    #[test]
    fn translates_all_css_selectors_in_integration_schemas() {
        use crate::{Cond, CondOp, StepOp, compile_path};
        use std::fs;
        use std::path::Path;

        fn check_cond(cond: &Cond, file: &str) {
            match cond {
                Cond::And(items) | Cond::Or(items) => {
                    for c in items {
                        check_cond(c, file);
                    }
                }
                Cond::Not(inner) => check_cond(inner, file),
                Cond::Atom(CondOp::Css { query }) => {
                    let xp = css_to_xpath(query).unwrap_or_else(|e| {
                        panic!("failed to lower CSS condition {query:?} in {file}: {e}")
                    });
                    validate_xpath(&xp).unwrap_or_else(|e| {
                        panic!("invalid XPath {xp:?} from CSS {query:?} in {file}: {e}")
                    });
                }
                _ => {}
            }
        }

        fn check_steps(steps: &[crate::TypedStep], file: &str) {
            for step in steps {
                match &step.op {
                    StepOp::Css { queries } | StepOp::CssAll { queries } => {
                        for q in queries {
                            let xp = css_to_xpath(q).unwrap_or_else(|e| {
                                panic!("failed to lower CSS step {q:?} in {file}: {e}")
                            });
                            validate_xpath(&xp).unwrap_or_else(|e| {
                                panic!("invalid XPath {xp:?} from CSS {q:?} in {file}: {e}")
                            });
                        }
                    }
                    StepOp::CssRemove { query } => {
                        let xp = css_to_xpath(query).unwrap_or_else(|e| {
                            panic!("failed to lower css-remove {query:?} in {file}: {e}")
                        });
                        validate_xpath(&xp).unwrap_or_else(|e| {
                            panic!("invalid XPath {xp:?} from css-remove {query:?} in {file}: {e}")
                        });
                    }
                    StepOp::Filter { cond }
                    | StepOp::Assert { cond, .. }
                    | StepOp::Match { cond } => check_cond(cond, file),
                    _ => {}
                }
            }
        }

        let schemas_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/integration/schemas");
        let mut count = 0usize;
        for entry in fs::read_dir(&schemas_dir).expect("read tests/integration/schemas") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("kdl") {
                continue;
            }
            let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
            let module = compile_path(&path).unwrap_or_else(|res| {
                panic!("failed to compile {file_name}: {}", res.format_text())
            });

            for s in &module.structs {
                for init in &s.init_fields {
                    check_steps(&init.pipeline.steps, &file_name);
                }
                for p in [
                    &s.pre_validate,
                    &s.check_pipeline,
                    &s.split_doc,
                    &s.key_pipeline,
                    &s.value_pipeline,
                    &s.table_pipeline,
                    &s.rows_pipeline,
                    &s.match_pipeline,
                ]
                .into_iter()
                .flatten()
                {
                    check_steps(&p.steps, &file_name);
                }
                for chk in &s.checks {
                    check_steps(&chk.pipeline.steps, &file_name);
                }
                for field in &s.fields {
                    check_steps(&field.pipeline.steps, &file_name);
                }
            }
            for func in &module.functions {
                check_steps(&func.pipeline.steps, &file_name);
            }
            count += 1;
        }
        assert!(
            count >= 30,
            "expected to verify integration schemas, found {count}"
        );
    }

    #[test]
    fn rejects_unsupported_css4_pseudo_classes_and_pseudo_elements() {
        let unsupported = [
            "p::before",
            "span::first-line",
            "x-tabs::part(tab)",
            "a:hover",
            "input:focus",
            "div:-soup-contains('hello')",
            "div:contains('hello')",
            "li:nth-child(2n+1 of .active)",
            "*:first-of-type",
            ".card:nth-of-type(2)",
            "div:not(.a > .b)",
        ];

        for sel in unsupported {
            assert!(
                css_to_xpath(sel).is_err(),
                "expected css_to_xpath({sel:?}) to return Err"
            );
        }
    }
}

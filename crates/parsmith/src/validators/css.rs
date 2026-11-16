//! Pure-Rust formal CSS3/4 selector syntax validator built on `cssparser`.
//!
//! Validates formal CSS3 and CSS4 selector syntax (balanced delimiters, combinators, attribute
//! selectors, and inner grammar of structural functional pseudo-classes) without allowlisting or
//! denylisting `:pseudo-class` or `::pseudo-element` identifier names.

use cssparser::{BasicParseErrorKind, Parser, ParserInput, Token};

/// Validates a CSS3/4 selector string at compile time.
pub fn validate_css_selector(selector: &str) -> Result<(), String> {
    let trimmed = selector.trim();
    if trimmed.is_empty() {
        return Err("empty CSS selector".to_string());
    }

    check_balanced_delimiters(trimmed)?;

    let mut input = ParserInput::new(trimmed);
    let mut parser = Parser::new(&mut input);
    parse_selector_list(&mut parser, false)
}

pub(crate) fn check_balanced_delimiters(s: &str) -> Result<(), String> {
    let mut stack: Vec<char> = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut quote: Option<char> = None;

    while i < n {
        let ch = chars[i];
        if let Some(q) = quote {
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == q {
                quote = None;
            }
            i += 1;
            continue;
        }

        if ch == '\\' {
            i += 2;
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            i += 1;
            continue;
        }
        if ch == '/' && i + 1 < n && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 >= n {
                return Err("unclosed comment in CSS selector".to_string());
            }
            i += 2;
            continue;
        }

        match ch {
            '[' => stack.push(']'),
            '(' => stack.push(')'),
            ']' | ')' if stack.pop() != Some(ch) => {
                return Err(format!("unmatched '{ch}' in CSS selector"));
            }
            _ => {}
        }
        i += 1;
    }

    if let Some(q) = quote {
        return Err(format!("unclosed string literal ({q}) in CSS selector"));
    }
    if let Some(expected) = stack.pop() {
        return Err(format!(
            "unclosed delimiter (expected '{expected}') in CSS selector"
        ));
    }
    Ok(())
}

fn parse_selector_list<'i, 't>(
    parser: &mut Parser<'i, 't>,
    allow_leading_combinator: bool,
) -> Result<(), String> {
    parse_complex_selector(parser, allow_leading_combinator)?;
    while !parser.is_exhausted() {
        let token = next_token_including_ws(parser)?;
        match token {
            Token::Comma => {
                parse_complex_selector(parser, allow_leading_combinator)?;
            }
            other => {
                return Err(format!("unexpected token in selector list: {other:?}"));
            }
        }
    }
    Ok(())
}

fn parse_complex_selector<'i, 't>(
    parser: &mut Parser<'i, 't>,
    allow_leading_combinator: bool,
) -> Result<(), String> {
    skip_whitespace(parser)?;

    if allow_leading_combinator {
        let state = parser.state();
        if let Ok(tok) = next_token_including_ws(parser) {
            if matches!(tok, Token::Delim('>' | '+' | '~')) {
                skip_whitespace(parser)?;
            } else {
                parser.reset(&state);
            }
        } else {
            parser.reset(&state);
        }
    }

    parse_compound_selector(parser)?;

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
            Token::Delim('>' | '+' | '~') => {
                skip_whitespace(parser)?;
                if parser.is_exhausted() {
                    return Err("dangling combinator at end of selector".to_string());
                }
                let after_state = parser.state();
                if let Ok(Token::Comma) = next_token_including_ws(parser) {
                    return Err("dangling combinator before ','".to_string());
                }
                parser.reset(&after_state);
                parse_compound_selector(parser)?;
            }
            _ if saw_ws => {
                parser.reset(&state);
                parse_compound_selector(parser)?;
            }
            other => {
                return Err(format!("unexpected token in selector: {other:?}"));
            }
        }
    }

    Ok(())
}

fn parse_compound_selector<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<(), String> {
    let mut count = 0usize;
    let mut seen_type_or_universal = false;

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
                    return Err(
                        "unexpected leading combinator (only valid inside ':has(...)')".to_string(),
                    );
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
                // Optional namespace pipe `ns|tag` or `ns|*`
                consume_optional_namespace_tail(parser)?;
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
                consume_optional_namespace_tail(parser)?;
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
                    Token::Ident(_) | Token::Delim('*') => {
                        seen_type_or_universal = true;
                        count += 1;
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
                parser
                    .parse_nested_block(|nested| {
                        parse_attribute_selector(nested)
                            .map_err(|msg| nested.new_custom_error::<String, String>(msg))
                    })
                    .map_err(format_css_error)?;
                count += 1;
            }
            Token::Colon => {
                parse_pseudo(parser)?;
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
    Ok(())
}

fn consume_optional_namespace_tail<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<(), String> {
    let state = parser.state();
    if let Ok(Token::Delim('|')) = parser.next_including_whitespace_and_comments() {
        let next_tok = parser
            .next_including_whitespace_and_comments()
            .map_err(|_| "expected element name or '*' after namespace '|'".to_string())?;
        if !matches!(next_tok, Token::Ident(_) | Token::Delim('*')) {
            return Err(format!(
                "expected element name or '*' after namespace '|', got {next_tok:?}"
            ));
        }
    } else {
        parser.reset(&state);
    }
    Ok(())
}

fn parse_attribute_selector<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<(), String> {
    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Err("empty attribute selector '[]'".to_string());
    }

    let first = next_token_including_ws(parser)?;
    match first {
        Token::Ident(_) | Token::Delim('*') => {
            // Optional `|attr` namespace
            let state = parser.state();
            if let Ok(Token::Delim('|')) = parser.next_including_whitespace_and_comments() {
                let after = parser
                    .next_including_whitespace_and_comments()
                    .map_err(|_| "expected attribute name after '|'".to_string())?;
                if !matches!(after, Token::Ident(_)) {
                    return Err("expected attribute name after '|'".to_string());
                }
            } else {
                parser.reset(&state);
            }
        }
        Token::Delim('|') => {
            let after = parser
                .next_including_whitespace_and_comments()
                .map_err(|_| "expected attribute name after '|'".to_string())?;
            if !matches!(after, Token::Ident(_)) {
                return Err("expected attribute name after '|'".to_string());
            }
        }
        other => {
            return Err(format!("expected attribute name in '[...]', got {other:?}"));
        }
    }

    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Ok(());
    }

    // Attribute operator: `=`, `~=`, `|=`, `^=`, `$=`, `*=`
    let op_tok = next_token_including_ws(parser)?;
    match op_tok {
        Token::Delim('=')
        | Token::IncludeMatch
        | Token::DashMatch
        | Token::PrefixMatch
        | Token::SuffixMatch
        | Token::SubstringMatch => {}
        other => {
            return Err(format!("invalid attribute operator in '[...]': {other:?}"));
        }
    }

    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Err("missing attribute value after operator in '[...]'".to_string());
    }

    let val_tok = next_token_including_ws(parser)?;
    match val_tok {
        Token::Ident(_) | Token::QuotedString(_) => {}
        other => {
            return Err(format!(
                "attribute value must be an identifier or string, got {other:?}"
            ));
        }
    }

    skip_whitespace(parser)?;
    if !parser.is_exhausted() {
        let modifier_tok = next_token_including_ws(parser)?;
        match modifier_tok {
            Token::Ident(ref flag)
                if flag.eq_ignore_ascii_case("i") || flag.eq_ignore_ascii_case("s") => {}
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

    Ok(())
}

fn parse_pseudo<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<(), String> {
    let tok = parser
        .next_including_whitespace_and_comments()
        .map_err(|_| "expected pseudo-class or pseudo-element after ':'".to_string())?
        .clone();

    match tok {
        Token::Colon => {
            // `::pseudo-element`
            let elem_tok = parser
                .next_including_whitespace_and_comments()
                .map_err(|_| "expected pseudo-element name after '::'".to_string())?
                .clone();
            match elem_tok {
                Token::Ident(ref name) if !name.is_empty() => Ok(()),
                Token::Function(ref name) if !name.is_empty() => parser
                    .parse_nested_block(|nested| {
                        consume_non_empty_balanced_tokens("::", name, nested)
                            .map_err(|msg| nested.new_custom_error::<String, String>(msg))
                    })
                    .map_err(format_css_error),
                other => Err(format!(
                    "expected pseudo-element identifier after '::', got {other:?}"
                )),
            }
        }
        Token::Ident(ref name) if !name.is_empty() => Ok(()),
        Token::Function(ref name) if !name.is_empty() => {
            let lower = name.to_ascii_lowercase();
            parser
                .parse_nested_block(|nested| {
                    validate_functional_pseudo(name, &lower, nested)
                        .map_err(|msg| nested.new_custom_error::<String, String>(msg))
                })
                .map_err(format_css_error)
        }
        other => Err(format!(
            "expected pseudo-class name after ':', got {other:?}"
        )),
    }
}

fn validate_functional_pseudo<'i, 't>(
    raw_name: &str,
    lower_name: &str,
    parser: &mut Parser<'i, 't>,
) -> Result<(), String> {
    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Err(format!("':{raw_name}()' requires arguments"));
    }

    match lower_name {
        "not" | "is" | "where" | "matches" => parse_selector_list(parser, false),
        "has" => parse_selector_list(parser, true),
        "nth-child" | "nth-last-child" => parse_nth_expression(parser, true),
        "nth-of-type" | "nth-last-of-type" => parse_nth_expression(parser, false),
        _ => consume_balanced_tokens_inner(parser),
    }
}

fn consume_non_empty_balanced_tokens<'i, 't>(
    prefix: &str,
    raw_name: &str,
    parser: &mut Parser<'i, 't>,
) -> Result<(), String> {
    skip_whitespace(parser)?;
    if parser.is_exhausted() {
        return Err(format!("'{prefix}{raw_name}()' requires arguments"));
    }
    consume_balanced_tokens_inner(parser)
}

fn consume_balanced_tokens_inner<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<(), String> {
    while !parser.is_exhausted() {
        let tok = next_token_including_ws(parser)?;
        match tok {
            Token::BadString(s) => {
                return Err(format!("bad string token in selector: {s:?}"));
            }
            Token::BadUrl(u) => {
                return Err(format!("bad url token in selector: {u:?}"));
            }
            Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock => {
                parser
                    .parse_nested_block(|nested| {
                        consume_balanced_tokens_inner(nested)
                            .map_err(|msg| nested.new_custom_error::<String, String>(msg))
                    })
                    .map_err(format_css_error)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_nth_expression<'i, 't>(
    parser: &mut Parser<'i, 't>,
    allow_of_clause: bool,
) -> Result<(), String> {
    // Collect tokens up to optional `of` keyword
    let mut expr_str = String::new();
    let mut saw_of = false;

    while !parser.is_exhausted() {
        let tok = next_token_including_ws(parser)?;
        match tok {
            Token::WhiteSpace(_) | Token::Comment(_) => {
                expr_str.push(' ');
            }
            Token::Ident(ref ident) if allow_of_clause && ident.eq_ignore_ascii_case("of") => {
                saw_of = true;
                break;
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
                return Err(format!("invalid token in :nth-*() expression: {other:?}"));
            }
        }
    }

    super::css_to_xpath::parse_an_plus_b(expr_str.trim()).map(|_| ())?;

    if saw_of {
        skip_whitespace(parser)?;
        if parser.is_exhausted() {
            return Err("expected selector list after 'of' in :nth-child(...)".to_string());
        }
        parse_selector_list(parser, false)?;
    }

    Ok(())
}

pub(crate) fn skip_whitespace<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<(), String> {
    loop {
        let state = parser.state();
        match parser.next_including_whitespace_and_comments() {
            Ok(Token::WhiteSpace(_) | Token::Comment(_)) => {}
            Ok(_) => {
                parser.reset(&state);
                return Ok(());
            }
            Err(_) => return Ok(()),
        }
    }
}

pub(crate) fn next_token_including_ws<'i, 't>(
    parser: &mut Parser<'i, 't>,
) -> Result<Token<'i>, String> {
    parser
        .next_including_whitespace_and_comments()
        .cloned()
        .map_err(|e| format_parse_error(&e.kind))
}

pub(crate) fn format_parse_error(kind: &BasicParseErrorKind<'_>) -> String {
    match kind {
        BasicParseErrorKind::UnexpectedToken(tok) => format!("unexpected token {tok:?}"),
        BasicParseErrorKind::EndOfInput => "unexpected end of selector".to_string(),
        BasicParseErrorKind::AtRuleInvalid(name) => format!("invalid @-rule '@{name}'"),
        BasicParseErrorKind::AtRuleBodyInvalid => "invalid @-rule body".to_string(),
        BasicParseErrorKind::QualifiedRuleInvalid => "invalid selector rule".to_string(),
    }
}

pub(crate) fn format_css_error(err: cssparser::ParseError<'_, String>) -> String {
    match err.kind {
        cssparser::ParseErrorKind::Custom(msg) => msg,
        cssparser::ParseErrorKind::Basic(basic) => format_parse_error(&basic),
    }
}

//! Pure-Rust W3C XPath 1.0 grammar parser and validator (`E002`).
//!
//! Zero C / `libxml2` dependencies. Validates location paths, all 13 standard axes, node tests,
//! predicates, function calls, variable references, literals, numbers, and operators.

#[derive(Debug, Clone, PartialEq)]
enum XPathToken<'a> {
    Slash,
    DoubleSlash,
    Dot,
    DoubleDot,
    At,
    DoubleColon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Pipe,
    Plus,
    Minus,
    Star,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Dollar,
    Literal(&'a str),
    Number(&'a str),
    Name(&'a str),
}

const VALID_AXES: &[&str] = &[
    "ancestor",
    "ancestor-or-self",
    "attribute",
    "child",
    "descendant",
    "descendant-or-self",
    "following",
    "following-sibling",
    "namespace",
    "parent",
    "preceding",
    "preceding-sibling",
    "self",
];

const NODE_TYPE_NAMES: &[&str] = &["comment", "text", "processing-instruction", "node"];

/// Validates an XPath 1.0 expression string at compile time.
pub fn validate_xpath(expr: &str) -> Result<(), String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err("empty XPath expression".to_string());
    }

    let tokens = tokenize_xpath(trimmed)?;
    if tokens.is_empty() {
        return Err("empty XPath expression".to_string());
    }

    let mut parser = XPathParser {
        tokens: &tokens,
        pos: 0,
    };
    parser.parse_expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!(
            "unexpected trailing token {:?} in XPath expression",
            parser.tokens[parser.pos]
        ));
    }
    Ok(())
}

fn is_ncname_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ncname_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

fn tokenize_xpath(input: &str) -> Result<Vec<XPathToken<'_>>, String> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    let n = bytes.len();

    while i < n {
        let ch = input[i..].chars().next().unwrap();
        if ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }

        match ch {
            '/' => {
                if input[i..].starts_with("//") {
                    tokens.push(XPathToken::DoubleSlash);
                    i += 2;
                } else {
                    tokens.push(XPathToken::Slash);
                    i += 1;
                }
            }
            '.' => {
                if input[i..].starts_with("..") {
                    tokens.push(XPathToken::DoubleDot);
                    i += 2;
                } else if i + 1 < n && bytes[i + 1].is_ascii_digit() {
                    let start = i;
                    i += 1;
                    while i < n && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    tokens.push(XPathToken::Number(&input[start..i]));
                } else {
                    tokens.push(XPathToken::Dot);
                    i += 1;
                }
            }
            '@' => {
                tokens.push(XPathToken::At);
                i += 1;
            }
            ':' => {
                if input[i..].starts_with("::") {
                    tokens.push(XPathToken::DoubleColon);
                    i += 2;
                } else {
                    return Err("unexpected single ':' in XPath expression".to_string());
                }
            }
            '(' => {
                tokens.push(XPathToken::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(XPathToken::RParen);
                i += 1;
            }
            '[' => {
                tokens.push(XPathToken::LBracket);
                i += 1;
            }
            ']' => {
                tokens.push(XPathToken::RBracket);
                i += 1;
            }
            ',' => {
                tokens.push(XPathToken::Comma);
                i += 1;
            }
            '|' => {
                tokens.push(XPathToken::Pipe);
                i += 1;
            }
            '+' => {
                tokens.push(XPathToken::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(XPathToken::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(XPathToken::Star);
                i += 1;
            }
            '=' => {
                tokens.push(XPathToken::Eq);
                i += 1;
            }
            '!' => {
                if input[i..].starts_with("!=") {
                    tokens.push(XPathToken::Ne);
                    i += 2;
                } else {
                    return Err("unexpected '!' in XPath expression (expected '!=')".to_string());
                }
            }
            '<' => {
                if input[i..].starts_with("<=") {
                    tokens.push(XPathToken::Le);
                    i += 2;
                } else {
                    tokens.push(XPathToken::Lt);
                    i += 1;
                }
            }
            '>' => {
                if input[i..].starts_with(">=") {
                    tokens.push(XPathToken::Ge);
                    i += 2;
                } else {
                    tokens.push(XPathToken::Gt);
                    i += 1;
                }
            }
            '$' => {
                tokens.push(XPathToken::Dollar);
                i += 1;
            }
            '"' | '\'' => {
                let quote = ch;
                i += 1;
                let start = i;
                while i < n {
                    let c = input[i..].chars().next().unwrap();
                    if c == quote {
                        break;
                    }
                    i += c.len_utf8();
                }
                if i >= n {
                    return Err(format!(
                        "unclosed string literal in XPath expression ({quote})"
                    ));
                }
                tokens.push(XPathToken::Literal(&input[start..i]));
                i += 1;
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < n && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < n && bytes[i] == b'.' {
                    i += 1;
                    while i < n && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                tokens.push(XPathToken::Number(&input[start..i]));
            }
            c if is_ncname_start(c) => {
                let start = i;
                i += c.len_utf8();
                while i < n {
                    let nc = input[i..].chars().next().unwrap();
                    if is_ncname_char(nc) {
                        i += nc.len_utf8();
                    } else {
                        break;
                    }
                }
                // Check for QName `prefix:local` or `prefix:*` (not `axis::`)
                if i < n && bytes[i] == b':' && !input[i..].starts_with("::") {
                    i += 1;
                    if i < n && bytes[i] == b'*' {
                        i += 1;
                    } else if i < n {
                        let lc = input[i..].chars().next().unwrap();
                        if !is_ncname_start(lc) {
                            return Err("invalid QName local part after ':'".to_string());
                        }
                        i += lc.len_utf8();
                        while i < n {
                            let nc = input[i..].chars().next().unwrap();
                            if is_ncname_char(nc) {
                                i += nc.len_utf8();
                            } else {
                                break;
                            }
                        }
                    } else {
                        return Err("dangling ':' at end of XPath name".to_string());
                    }
                }
                tokens.push(XPathToken::Name(&input[start..i]));
            }
            other => {
                return Err(format!(
                    "unexpected character '{other}' in XPath expression"
                ));
            }
        }
    }

    Ok(tokens)
}

struct XPathParser<'a> {
    tokens: &'a [XPathToken<'a>],
    pos: usize,
}

impl<'a> XPathParser<'a> {
    fn peek(&self) -> Option<&'a XPathToken<'a>> {
        self.tokens.get(self.pos)
    }

    fn peek_next(&self) -> Option<&'a XPathToken<'a>> {
        self.tokens.get(self.pos + 1)
    }

    fn bump(&mut self) -> Option<&'a XPathToken<'a>> {
        let tok = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(tok)
    }

    fn parse_expr(&mut self) -> Result<(), String> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<(), String> {
        self.parse_and_expr()?;
        while matches!(self.peek(), Some(XPathToken::Name("or"))) {
            self.bump();
            self.parse_and_expr()?;
        }
        Ok(())
    }

    fn parse_and_expr(&mut self) -> Result<(), String> {
        self.parse_equality_expr()?;
        while matches!(self.peek(), Some(XPathToken::Name("and"))) {
            self.bump();
            self.parse_equality_expr()?;
        }
        Ok(())
    }

    fn parse_equality_expr(&mut self) -> Result<(), String> {
        self.parse_relational_expr()?;
        while matches!(self.peek(), Some(XPathToken::Eq | XPathToken::Ne)) {
            self.bump();
            self.parse_relational_expr()?;
        }
        Ok(())
    }

    fn parse_relational_expr(&mut self) -> Result<(), String> {
        self.parse_additive_expr()?;
        while matches!(
            self.peek(),
            Some(XPathToken::Lt | XPathToken::Le | XPathToken::Gt | XPathToken::Ge)
        ) {
            self.bump();
            self.parse_additive_expr()?;
        }
        Ok(())
    }

    fn parse_additive_expr(&mut self) -> Result<(), String> {
        self.parse_multiplicative_expr()?;
        while matches!(self.peek(), Some(XPathToken::Plus | XPathToken::Minus)) {
            self.bump();
            self.parse_multiplicative_expr()?;
        }
        Ok(())
    }

    fn parse_multiplicative_expr(&mut self) -> Result<(), String> {
        self.parse_unary_expr()?;
        while matches!(
            self.peek(),
            Some(XPathToken::Star | XPathToken::Name("div" | "mod"))
        ) {
            self.bump();
            self.parse_unary_expr()?;
        }
        Ok(())
    }

    fn parse_unary_expr(&mut self) -> Result<(), String> {
        while matches!(self.peek(), Some(XPathToken::Minus)) {
            self.bump();
        }
        self.parse_union_expr()
    }

    fn parse_union_expr(&mut self) -> Result<(), String> {
        self.parse_path_expr()?;
        while matches!(self.peek(), Some(XPathToken::Pipe)) {
            self.bump();
            self.parse_path_expr()?;
        }
        Ok(())
    }

    fn parse_path_expr(&mut self) -> Result<(), String> {
        if self.starts_filter_expr() {
            self.parse_filter_expr()?;
            if matches!(
                self.peek(),
                Some(XPathToken::Slash | XPathToken::DoubleSlash)
            ) {
                self.bump();
                self.parse_relative_location_path()?;
            }
            Ok(())
        } else {
            self.parse_location_path()
        }
    }

    fn starts_filter_expr(&self) -> bool {
        match self.peek() {
            Some(
                XPathToken::Dollar
                | XPathToken::LParen
                | XPathToken::Literal(_)
                | XPathToken::Number(_),
            ) => true,
            Some(XPathToken::Name(name)) => {
                matches!(self.peek_next(), Some(XPathToken::LParen))
                    && !NODE_TYPE_NAMES.contains(name)
            }
            _ => false,
        }
    }

    fn parse_filter_expr(&mut self) -> Result<(), String> {
        self.parse_primary_expr()?;
        while matches!(self.peek(), Some(XPathToken::LBracket)) {
            self.parse_predicate()?;
        }
        Ok(())
    }

    fn parse_primary_expr(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(XPathToken::Dollar) => {
                self.bump();
                match self.bump() {
                    Some(XPathToken::Name(_)) => Ok(()),
                    other => Err(format!("expected variable name after '$', got {other:?}")),
                }
            }
            Some(XPathToken::LParen) => {
                self.bump();
                if matches!(self.peek(), Some(XPathToken::RParen)) {
                    return Err("empty parentheses '()' in XPath expression".to_string());
                }
                self.parse_expr()?;
                match self.bump() {
                    Some(XPathToken::RParen) => Ok(()),
                    other => Err(format!("expected ')', got {other:?}")),
                }
            }
            Some(XPathToken::Literal(_) | XPathToken::Number(_)) => {
                self.bump();
                Ok(())
            }
            Some(XPathToken::Name(fn_name)) => {
                if NODE_TYPE_NAMES.contains(fn_name) {
                    return Err(format!(
                        "unexpected node test '{fn_name}()' as primary expression"
                    ));
                }
                self.bump();
                match self.bump() {
                    Some(XPathToken::LParen) => {}
                    other => {
                        return Err(format!(
                            "expected '(' after function name '{fn_name}', got {other:?}"
                        ));
                    }
                }
                if !matches!(self.peek(), Some(XPathToken::RParen)) {
                    self.parse_expr()?;
                    while matches!(self.peek(), Some(XPathToken::Comma)) {
                        self.bump();
                        self.parse_expr()?;
                    }
                }
                match self.bump() {
                    Some(XPathToken::RParen) => Ok(()),
                    other => Err(format!(
                        "expected ')' closing function call '{fn_name}(...)', got {other:?}"
                    )),
                }
            }
            other => Err(format!("expected primary XPath expression, got {other:?}")),
        }
    }

    fn parse_location_path(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(XPathToken::Slash) => {
                self.bump();
                if self.starts_step() {
                    self.parse_relative_location_path()?;
                }
                Ok(())
            }
            Some(XPathToken::DoubleSlash) => {
                self.bump();
                self.parse_relative_location_path()
            }
            _ => self.parse_relative_location_path(),
        }
    }

    fn starts_step(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                XPathToken::Dot
                    | XPathToken::DoubleDot
                    | XPathToken::At
                    | XPathToken::Star
                    | XPathToken::Name(_)
            )
        )
    }

    fn parse_relative_location_path(&mut self) -> Result<(), String> {
        self.parse_step()?;
        while matches!(
            self.peek(),
            Some(XPathToken::Slash | XPathToken::DoubleSlash)
        ) {
            self.bump();
            self.parse_step()?;
        }
        Ok(())
    }

    fn parse_step(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(XPathToken::Dot | XPathToken::DoubleDot) => {
                self.bump();
                Ok(())
            }
            _ => {
                self.parse_axis_specifier()?;
                self.parse_node_test()?;
                while matches!(self.peek(), Some(XPathToken::LBracket)) {
                    self.parse_predicate()?;
                }
                Ok(())
            }
        }
    }

    fn parse_axis_specifier(&mut self) -> Result<(), String> {
        if matches!(self.peek(), Some(XPathToken::At)) {
            self.bump();
            return Ok(());
        }
        if let (Some(XPathToken::Name(axis)), Some(XPathToken::DoubleColon)) =
            (self.peek(), self.peek_next())
        {
            if !VALID_AXES.contains(axis) {
                return Err(format!("unknown XPath axis '{axis}::'"));
            }
            self.bump();
            self.bump();
        }
        Ok(())
    }

    fn parse_node_test(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(XPathToken::Star) => {
                self.bump();
                Ok(())
            }
            Some(XPathToken::Name(name)) => {
                if matches!(self.peek_next(), Some(XPathToken::LParen)) {
                    if !NODE_TYPE_NAMES.contains(name) {
                        return Err(format!("unknown XPath node test '{name}()'"));
                    }
                    let is_pi = *name == "processing-instruction";
                    self.bump();
                    self.bump(); // '('
                    if is_pi && matches!(self.peek(), Some(XPathToken::Literal(_))) {
                        self.bump();
                    }
                    match self.bump() {
                        Some(XPathToken::RParen) => Ok(()),
                        other => Err(format!(
                            "expected ')' in node test '{name}()', got {other:?}"
                        )),
                    }
                } else {
                    self.bump();
                    Ok(())
                }
            }
            other => Err(format!("expected XPath node test, got {other:?}")),
        }
    }

    fn parse_predicate(&mut self) -> Result<(), String> {
        match self.bump() {
            Some(XPathToken::LBracket) => {}
            other => return Err(format!("expected '[', got {other:?}")),
        }
        if matches!(self.peek(), Some(XPathToken::RBracket)) {
            return Err("empty predicate '[]' in XPath expression".to_string());
        }
        self.parse_expr()?;
        match self.bump() {
            Some(XPathToken::RBracket) => Ok(()),
            other => Err(format!(
                "expected ']' closing XPath predicate, got {other:?}"
            )),
        }
    }
}

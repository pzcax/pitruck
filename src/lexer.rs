use crate::token::{Token, TemplatePart};
use crate::error::PitruckError;

pub struct Lexer {
    source: Vec<char>,
    pos:    usize,
    line:   usize,
    col:    usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer { source: source.chars().collect(), pos: 0, line: 1, col: 1 }
    }

    pub fn tokenize(&mut self) -> Result<Vec<(Token, usize, usize)>, PitruckError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let done = tok.0 == Token::EOF;
            tokens.push(tok);
            if done { break; }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> { self.source.get(self.pos).copied() }
    fn peek_next(&self) -> Option<char> { self.source.get(self.pos + 1).copied() }
    fn peek_at(&self, offset: usize) -> Option<char> { self.source.get(self.pos + offset).copied() }

    fn lex_escape_into(&mut self, s: &mut String) {
        match self.advance() {
            Some('n')  => s.push('\n'),
            Some('t')  => s.push('\t'),
            Some('r')  => s.push('\r'),
            Some('e')  => s.push('\x1B'),
            Some('"')  => s.push('"'),
            Some('`')  => s.push('`'),
            Some('\\') => s.push('\\'),
            Some('$')  => s.push('$'),
            Some('x')  => {
                let mut hex = String::new();
                if let Some(c1) = self.advance() { hex.push(c1); }
                if let Some(c2) = self.advance() { hex.push(c2); }
                if let Ok(val) = u8::from_str_radix(&hex, 16) { s.push(val as char); }
                else { s.push('x'); s.push_str(&hex); }
            }
            Some('u') => {
                if self.peek() == Some('{') {
                    self.advance();
                    let mut hex = String::new();
                    while let Some(c) = self.peek() {
                        if c == '}' { break; }
                        hex.push(c);
                        self.advance();
                    }
                    if self.peek() == Some('}') { self.advance(); }
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) { s.push(ch); }
                    }
                } else {
                    s.push('u');
                }
            }
            Some(c) => s.push(c),
            None => {}
        }
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.source.get(self.pos).copied();
        if let Some(ch) = c {
            self.pos += 1;
            if ch == '\n' { self.line += 1; self.col = 1; }
            else          { self.col  += 1; }
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\r') | Some('\n')) {
            self.advance();
        }
    }

    fn next_token(&mut self) -> Result<(Token, usize, usize), PitruckError> {
        self.skip_whitespace();
        let line = self.line;
        let col  = self.col;

        match self.peek() {
            None => Ok((Token::EOF, line, col)),

            Some('#') => {
                while self.peek().is_some() && self.peek() != Some('\n') { self.advance(); }
                self.next_token()
            }

            Some('"') if self.peek_next() == Some('"') && self.peek_at(2) == Some('"') => {
                self.advance(); self.advance(); self.advance();
                let mut s = String::new();
                loop {
                    if self.peek() == Some('"') && self.peek_next() == Some('"') && self.peek_at(2) == Some('"') {
                        self.advance(); self.advance(); self.advance();
                        break;
                    }
                    match self.advance() {
                        None => return Err(PitruckError::LexError {
                            line, col, message: "unterminated triple-quoted string".to_string(),
                        }),
                        Some('\\') => self.lex_escape_into(&mut s),
                        Some(c) => s.push(c),
                    }
                }
                Ok((Token::StringLit(s), line, col))
            }
            Some('"') => {
                self.advance();
                let mut s = String::new();
                loop {
                    match self.advance() {
                        None | Some('\n') => return Err(PitruckError::LexError {
                            line, col, message: "unterminated string literal".to_string(),
                        }),
                        Some('"') => break,
                        Some('\\') => self.lex_escape_into(&mut s),
                        Some(c) => s.push(c),
                    }
                }
                Ok((Token::StringLit(s), line, col))
            }
            Some('`') => {
                self.advance();
                let mut parts = Vec::new();
                let mut cur = String::new();
                loop {
                    match self.peek() {
                        None => return Err(PitruckError::LexError {
                            line, col, message: "unterminated template string".to_string(),
                        }),
                        Some('`') => { self.advance(); break; }
                        Some('\\') => { self.advance(); self.lex_escape_into(&mut cur); }
                        Some('$') if self.peek_next() == Some('{') => {
                            self.advance(); self.advance();
                            parts.push(TemplatePart::Str(std::mem::take(&mut cur)));
                            let mut depth = 1;
                            let mut inner = String::new();
                            loop {
                                match self.advance() {
                                    None => return Err(PitruckError::LexError {
                                        line, col, message: "unterminated template expression".to_string(),
                                    }),
                                    Some('{') => { depth += 1; inner.push('{'); }
                                    Some('}') => {
                                        depth -= 1;
                                        if depth == 0 { break; }
                                        inner.push('}');
                                    }
                                    Some(c) => inner.push(c),
                                }
                            }
                            let mut sub_lexer = Lexer::new(&inner);
                            let sub_tokens = sub_lexer.tokenize()?;
                            parts.push(TemplatePart::Code(sub_tokens));
                        }
                        Some(c) => { cur.push(c); self.advance(); }
                    }
                }
                parts.push(TemplatePart::Str(cur));
                Ok((Token::TemplateStr(parts), line, col))
            }

            Some(c) if c.is_ascii_digit() => {
                let mut num = String::new();
                let mut has_dot = false;
                while let Some(c) = self.peek() {
                    if c == '_' { self.advance(); }
                    else if c.is_ascii_digit() { num.push(c); self.advance(); }
                    else if c == '.' && !has_dot && self.peek_next().map(|d| d.is_ascii_digit()).unwrap_or(false) {
                        has_dot = true; num.push(c); self.advance();
                    } else { break; }
                }
                let val: f64 = num.parse().map_err(|_| PitruckError::LexError {
                    line, col, message: format!("invalid number '{num}'"),
                })?;
                Ok((Token::Number(val), line, col))
            }

            Some(c) if c.is_alphabetic() || c == '_' => {
                let mut ident = String::new();
                while let Some(c) = self.peek() {
                    if c.is_alphanumeric() || c == '_' { ident.push(c); self.advance(); }
                    else { break; }
                }
                let tok = match ident.as_str() {
                    "var"    => Token::Var,
                    "bring"  => Token::Bring,
                    "func"   => Token::Func,
                    "return" => Token::Return,
                    "if"     => Token::If,
                    "elif"   => Token::Elif,
                    "else"   => Token::Else,
                    "while"  => Token::While,
                    "for"    => Token::For,
                    "in"     => Token::In,
                    "print"  => Token::Print,
                    "and"    => Token::And,
                    "or"     => Token::Or,
                    "not"    => Token::Not,
                    "true"   => Token::True,
                    "false"  => Token::False,
                    "null"   => Token::Null,
                    "class"  => Token::Class,
                    "self"   => Token::Self_,
                    "match"  => Token::Match,
                    "break"  => Token::Break,
                    "continue" => Token::Continue,
                    "try"    => Token::Try,
                    "catch"  => Token::Catch,
                    "_"      => Token::Underscore,
                    _        => Token::Ident(ident),
                };
                Ok((tok, line, col))
            }

            Some('+') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::PlusEq, line, col)) }
                else { Ok((Token::Plus, line, col)) }
            }
            Some('-') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::MinusEq, line, col)) }
                else { Ok((Token::Minus, line, col)) }
            }
            Some('*') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::StarEq, line, col)) }
                else { Ok((Token::Star, line, col)) }
            }
            Some('/') => {
                self.advance();
                if self.peek() == Some('/') {
                    while self.peek().is_some() && self.peek() != Some('\n') { self.advance(); }
                    self.next_token()
                } else if self.peek() == Some('*') {
                    self.advance();
                    loop {
                        match self.peek() {
                            None => break,
                            Some('*') if self.peek_next() == Some('/') => { self.advance(); self.advance(); break; }
                            _ => { self.advance(); }
                        }
                    }
                    self.next_token()
                } else if self.peek() == Some('=') {
                    self.advance(); Ok((Token::SlashEq, line, col))
                } else {
                    Ok((Token::Slash, line, col))
                }
            }
            Some('%') => { self.advance(); Ok((Token::Percent, line, col)) }
            Some('{') => { self.advance(); Ok((Token::LBrace,   line, col)) }
            Some('}') => { self.advance(); Ok((Token::RBrace,   line, col)) }
            Some('(') => { self.advance(); Ok((Token::LParen,   line, col)) }
            Some(')') => { self.advance(); Ok((Token::RParen,   line, col)) }
            Some('[') => { self.advance(); Ok((Token::LBracket, line, col)) }
            Some(']') => { self.advance(); Ok((Token::RBracket, line, col)) }
            Some(',') => { self.advance(); Ok((Token::Comma,    line, col)) }
            Some('.') => { self.advance(); Ok((Token::Dot,      line, col)) }
            Some(':') => { self.advance(); Ok((Token::Colon,    line, col)) }
            Some('?') => {
                self.advance();
                if self.peek() == Some('?') { self.advance(); Ok((Token::QuestionQuestion, line, col)) }
                else if self.peek() == Some('.') { self.advance(); Ok((Token::OptDot, line, col)) }
                else if self.peek() == Some('[') { self.advance(); Ok((Token::OptLBracket, line, col)) }
                else { Ok((Token::Question, line, col)) }
            }
            Some('=') => {
                self.advance();
                if self.peek() == Some('=')      { self.advance(); Ok((Token::EqEq,     line, col)) }
                else if self.peek() == Some('>') { self.advance(); Ok((Token::FatArrow, line, col)) }
                else                             { Ok((Token::Eq, line, col)) }
            }
            Some('!') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::BangEq, line, col)) }
                else { Err(PitruckError::LexError { line, col, message: "unexpected '!' -- did you mean '!='?".to_string() }) }
            }
            Some('<') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::LtEq, line, col)) }
                else                        { Ok((Token::Lt, line, col)) }
            }
            Some('>') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::GtEq, line, col)) }
                else                        { Ok((Token::Gt, line, col)) }
            }
            Some(c) => {
                let c = c;
                self.advance();
                Err(PitruckError::LexError { line, col, message: format!("unexpected character '{c}'") })
            }
        }
    }
}
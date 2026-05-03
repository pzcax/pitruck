use crate::token::Token;
use crate::error::PitruckError;

pub struct Lexer {
    source:          Vec<char>,
    pos:             usize,
    line:            usize,
    col:             usize,
    nesting_level:   usize,
    indent_stack:    Vec<usize>,
    pending_dedents: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            source:          source.chars().collect(),
            pos:             0,
            line:            1,
            col:             1,
            nesting_level:   0,
            indent_stack:    vec![0],
            pending_dedents: 0,
        }
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

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
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

    fn skip_inline_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\r')) {
            self.advance();
        }
    }

    fn consume_indent(&mut self) -> usize {
        let mut level = 0usize;
        while let Some(c) = self.peek() {
            match c {
                ' '  => { level += 1; self.advance(); }
                '\t' => { level += 4; self.advance(); }
                _    => break,
            }
        }
        level
    }

    fn next_token(&mut self) -> Result<(Token, usize, usize), PitruckError> {
        if self.pending_dedents > 0 {
            self.pending_dedents -= 1;
            return Ok((Token::Dedent, self.line, self.col));
        }

        self.skip_inline_whitespace();

        let line = self.line;
        let col  = self.col;

        match self.peek() {
            None => {
                if self.nesting_level == 0 && self.indent_stack.len() > 1 {
                    self.indent_stack.pop();
                    return Ok((Token::Dedent, line, col));
                }
                Ok((Token::EOF, line, col))
            }

            Some('#') => {
                while self.peek().is_some() && self.peek() != Some('\n') {
                    self.advance();
                }
                self.next_token()
            }

            Some('\n') => {
                self.advance();

                if self.nesting_level > 0 {
                    return self.next_token();
                }

                let indent = self.consume_indent();

                if matches!(self.peek(), Some('\n') | Some('#') | None) {
                    return self.next_token();
                }

                let current = *self.indent_stack.last().unwrap();

                if indent > current {
                    self.indent_stack.push(indent);
                    Ok((Token::Indent, line, col))
                } else if indent < current {
                    let mut count = 0;
                    while self.indent_stack.len() > 1 && *self.indent_stack.last().unwrap() > indent {
                        self.indent_stack.pop();
                        count += 1;
                    }
                    if *self.indent_stack.last().unwrap() != indent {
                        return Err(PitruckError::LexError {
                            line,
                            col,
                            message: "inconsistent indentation".to_string(),
                        });
                    }
                    self.pending_dedents = count - 1;
                    Ok((Token::Dedent, line, col))
                } else {
                    Ok((Token::Newline, line, col))
                }
            }

            Some('"') => {
                self.advance();
                let mut s = String::new();
                loop {
                    match self.advance() {
                        None | Some('\n') => {
                            return Err(PitruckError::LexError {
                                line, col,
                                message: "unterminated string literal".to_string(),
                            });
                        }
                        Some('"') => break,
                        Some('\\') => match self.advance() {
                            Some('n')  => s.push('\n'),
                            Some('t')  => s.push('\t'),
                            Some('r')  => s.push('\r'),
                            Some('e')  => s.push('\x1B'),
                            Some('"')  => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some('x')  => {
                               
                                let mut hex = String::new();
                                if let Some(c1) = self.advance() { hex.push(c1); }
                                if let Some(c2) = self.advance() { hex.push(c2); }
                                if let Ok(val) = u8::from_str_radix(&hex, 16) {
                                    s.push(val as char);
                                } else {
                                    s.push('x');
                                    s.push_str(&hex);
                                }
                            }
                            Some(c)    => s.push(c),
                            None => return Err(PitruckError::LexError {
                                line, col, message: "unterminated escape sequence".to_string(),
                            }),
                        },
                        Some(c) => s.push(c),
                    }
                }
                Ok((Token::StringLit(s), line, col))
            }

            Some(c) if c.is_ascii_digit() => {
                let mut num = String::new();
                let mut has_dot = false;
                while let Some(c) = self.peek() {
                    if c == '_' {
                        self.advance();
                    } else if c.is_ascii_digit() {
                        num.push(c);
                        self.advance();
                    } else if c == '.' && !has_dot && self.peek_next().map(|d| d.is_ascii_digit()).unwrap_or(false) {
                        has_dot = true;
                        num.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                let val: f64 = num.parse().map_err(|_| PitruckError::LexError {
                    line, col, message: format!("invalid number '{num}'"),
                })?;
                Ok((Token::Number(val), line, col))
            }

            Some(c) if c.is_alphabetic() || c == '_' || c == '#' => {
                let mut ident = String::new();
                while let Some(c) = self.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        ident.push(c);
                        self.advance();
                    } else {
                        break;
                    }
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
                    "_"      => Token::Underscore,
                    _        => Token::Ident(ident),
                };
                Ok((tok, line, col))
            }

            Some('+') => { self.advance(); Ok((Token::Plus,    line, col)) }
            Some('-') => { self.advance(); Ok((Token::Minus,   line, col)) }
            Some('*') => { self.advance(); Ok((Token::Star,    line, col)) }
            Some('/') => { 
                self.advance(); 
                if self.peek() == Some('/') {
                    while self.peek().is_some() && self.peek() != Some('\n') { 
                        self.advance(); 
                    }
                    self.next_token()   
                } else {
                    Ok((Token::Slash,   line, col)) 
                }
            }
            Some('%') => { self.advance(); Ok((Token::Percent, line, col)) }
            Some('{') | Some('[') | Some('(') => {
                let c = self.advance().unwrap();
                self.nesting_level += 1;
                let tok = match c {
                    '{' => Token::LBrace,
                    '[' => Token::LBracket,
                    '(' => Token::LParen,
                    _ => unreachable!(),
                };
                Ok((tok, line, col))
            }
            Some('}') | Some(']') | Some(')') => {
                let c = self.advance().unwrap();
                if self.nesting_level > 0 { self.nesting_level -= 1; }
                let tok = match c {
                    '}' => Token::RBrace,
                    ']' => Token::RBracket,
                    ')' => Token::RParen,
                    _ => unreachable!(),
                };
                Ok((tok, line, col))
            }
            Some(',') => { self.advance(); Ok((Token::Comma,   line, col)) }
            Some('.') => { self.advance(); Ok((Token::Dot, line, col)) }
            Some(':') => { self.advance(); Ok((Token::Colon, line, col)) }

            Some('=') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::EqEq,   line, col)) }
                else if self.peek() == Some('>') { self.advance(); Ok((Token::FatArrow, line, col)) }
                else                        { Ok((Token::Eq, line, col)) }
            }
            Some('!') => {
                self.advance();
                if self.peek() == Some('=') { self.advance(); Ok((Token::BangEq, line, col)) }
                else {
                    Err(PitruckError::LexError { line, col, message: "unexpected '!', did you mean '!='?".to_string() })
                }
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
                Err(PitruckError::LexError {
                    line, col,
                    message: format!("unexpected character '{c}'"),
                })
            }
        }
    }
}

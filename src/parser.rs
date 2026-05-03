use crate::token::Token;
use crate::ast::*;
use crate::error::PitruckError;

pub struct Parser {
    tokens:     Vec<(Token, usize, usize)>,
    pos:        usize,
    brace_mode: bool,
}

impl Parser {
    pub fn new(tokens: Vec<(Token, usize, usize)>) -> Self {
        Parser { tokens, pos: 0, brace_mode: false }
    }

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, PitruckError> {
        self.skip_newlines();
        let mut stmts = Vec::new();
        while !self.at_eof() {
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].0
    }

    fn span(&self) -> (usize, usize) {
        let (_, l, c) = &self.tokens[self.pos];
        (*l, *c)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::EOF)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].0.clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), PitruckError> {
        let (line, col) = self.span();
        let got = self.peek().clone();
        if &got == expected {
            self.advance();
            Ok(())
        } else {
            Err(PitruckError::ParseError {
                line,
                col,
                message: format!(
                    "expected `{}`, found `{}`",
                    token_display(expected),
                    token_display(&got)
                ),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String, PitruckError> {
        let (line, col) = self.span();
        match self.peek().clone() {
            Token::Ident(name) => { self.advance(); Ok(name) }
            other => Err(PitruckError::ParseError {
                line,
                col,
                message: format!("expected identifier, found `{}`", token_display(&other)),
            }),
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    fn expect_stmt_end(&mut self) -> Result<(), PitruckError> {
        let (line, col) = self.span();
        match self.peek() {
            Token::Newline => { self.skip_newlines(); Ok(()) }
            Token::RBrace | Token::Dedent | Token::EOF => Ok(()),
            Token::Var | Token::Bring | Token::Func | Token::If | Token::Elif | Token::Else
            | Token::While | Token::Return | Token::Print | Token::Class | Token::Match 
            | Token::Ident(_) | Token::Self_ | Token::LBracket | Token::LBrace | Token::Comma => Ok(()),
            other => {
                let msg = format!(
                    "unexpected `{}` - did you forget a closing `)`?",
                    token_display(other)
                );
                Err(PitruckError::ParseError { line, col, message: msg })
            }
        }
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, PitruckError> {
        let (line, col) = self.span();
        if matches!(self.peek(), Token::LBrace) {
            self.expect(&Token::LBrace)?;
            self.skip_newlines();
            let mut stmts = Vec::new();
            while !matches!(self.peek(), Token::RBrace | Token::EOF) {
                stmts.push(self.parse_stmt()?);
                self.skip_newlines();
            }
            self.expect(&Token::RBrace)?;
            Ok(stmts)
        } else if matches!(self.peek(), Token::Newline | Token::Indent) {
            self.skip_newlines();
            self.expect(&Token::Indent)?;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), Token::Dedent | Token::EOF) {
                stmts.push(self.parse_stmt()?);
                self.skip_newlines();
            }
            if matches!(self.peek(), Token::Dedent) { self.advance(); }
            Ok(stmts)
        } else {
            Err(PitruckError::ParseError {
                line,
                col,
                message: "expected an indented block or `{ ... }`".to_string(),
            })
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, PitruckError> {
        let (line, col) = self.span();

        match self.peek().clone() {
            Token::Var => {
                self.advance();
                let name  = self.expect_ident()?;
                self.expect(&Token::Eq)?;
                let value = self.parse_expr()?;
                self.expect_stmt_end()?;
                Ok(Stmt::VarDecl { name, value, line })
            }

            Token::Bring => {
                self.advance();
                let module = self.expect_ident()?;
                self.expect_stmt_end()?;
                Ok(Stmt::Bring { module, line })
            }

            Token::Func => {
                self.advance();
                let name   = self.expect_ident()?;
                self.expect(&Token::LParen)?;
                let params = self.parse_params()?;
                self.expect(&Token::RParen)?;
                let body   = self.parse_block()?;
                Ok(Stmt::FuncDef { name, params, body, line })
            }

            Token::Class => {
                self.advance();
                let name = self.expect_ident()?;
                let methods = self.parse_block()?;
                Ok(Stmt::ClassDef { name, methods, line })
            }

            Token::Match => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::LBrace)?;
                self.skip_newlines();
                let mut arms = Vec::new();
                let mut default = None;

                while !matches!(self.peek(), Token::RBrace | Token::EOF) {
                    if matches!(self.peek(), Token::Underscore) {
                        self.advance(); 
                        self.expect(&Token::FatArrow)?;
                        let body = if matches!(self.peek(), Token::LBrace | Token::Newline | Token::Indent) {
                            self.parse_block()?
                        } else {
                            vec![self.parse_stmt()?]
                        };
                        default = Some(body);
                        if matches!(self.peek(), Token::Comma) { self.advance(); }
                        self.skip_newlines();
                    } else {
                        let val = self.parse_expr()?;
                        self.expect(&Token::FatArrow)?;
                        let body = if matches!(self.peek(), Token::LBrace | Token::Newline | Token::Indent) {
                            self.parse_block()?
                        } else {
                            vec![self.parse_stmt()?]
                        };
                        arms.push((val, body));
                        if matches!(self.peek(), Token::Comma) { self.advance(); }
                        self.skip_newlines();
                    }
                }
                self.expect(&Token::RBrace)?;
                Ok(Stmt::Match { expr, arms, default, line })
            }

            Token::Return => {
                self.advance();
                let value = if self.is_expr_start() {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect_stmt_end()?;
                Ok(Stmt::Return { value, line })
            }

            Token::Print => {
                self.advance();
                let value = self.parse_expr()?;
                self.expect_stmt_end()?;
                Ok(Stmt::Print { value, line })
            }

            Token::If    => self.parse_if(),

            Token::While => {
                self.advance();
                let condition = self.parse_expr()?;
                let body      = self.parse_block()?;
                Ok(Stmt::While { condition, body, line })
            }

            _ => {
                let expr = self.parse_expr()?;
                if matches!(self.peek(), Token::Eq) {
                    self.advance();
                    let value = self.parse_expr()?;
                    self.expect_stmt_end()?;
                    match expr {
                        Expr::Ident { name, line } => Ok(Stmt::Assign { name, value, line }),
                        Expr::Get { object, name, line } => Ok(Stmt::Set { object: *object, name, value, line }),
                        Expr::IndexGet { object, index, line } => Ok(Stmt::IndexSet { object: *object, index: *index, value, line }),
                        _ => Err(PitruckError::ParseError { line, col, message: "invalid assignment target".to_string() }),
                    }
                } else {
                    self.expect_stmt_end()?;
                    Ok(Stmt::ExprStmt { expr, line })
                }
            }
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, PitruckError> {
        let (line, _) = self.span();
        self.advance();

        let condition   = self.parse_expr()?;
        let then_branch = self.parse_block()?;

        let mut elif_branches = Vec::new();
        let mut else_branch   = None;

        loop {
            self.skip_newlines();
            if matches!(self.peek(), Token::Elif) {
                self.advance();
                let cond  = self.parse_expr()?;
                let block = self.parse_block()?;
                elif_branches.push((cond, block));
            } else if matches!(self.peek(), Token::Else) {
                self.advance();
                else_branch = Some(self.parse_block()?);
                break;
            } else {
                break;
            }
        }

        Ok(Stmt::If { condition, then_branch, elif_branches, else_branch, line })
    }

    fn parse_params(&mut self) -> Result<Vec<String>, PitruckError> {
        let mut params = Vec::new();
        if matches!(self.peek(), Token::RParen) { return Ok(params); }
        params.push(self.expect_ident()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            params.push(self.expect_ident()?);
        }
        Ok(params)
    }

    fn is_expr_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::Number(_) | Token::StringLit(_) | Token::True
            | Token::False   | Token::Null          | Token::Ident(_)
            | Token::LParen  | Token::Minus         | Token::Not
            | Token::LBracket| Token::LBrace        | Token::Self_
        )
    }

    fn parse_expr(&mut self) -> Result<Expr, PitruckError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, PitruckError> {
        let (line, _) = self.span();
        let mut left  = self.parse_and()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinOp { op: BinOpKind::Or, left: Box::new(left), right: Box::new(right), line };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, PitruckError> {
        let (line, _) = self.span();
        let mut left  = self.parse_not()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinOp { op: BinOpKind::And, left: Box::new(left), right: Box::new(right), line };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, PitruckError> {
        let (line, _) = self.span();
        if matches!(self.peek(), Token::Not) {
            self.advance();
            let expr = self.parse_not()?;
            return Ok(Expr::Unary { op: UnaryOpKind::Not, expr: Box::new(expr), line });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, PitruckError> {
        let (line, _) = self.span();
        let mut left  = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::EqEq   => BinOpKind::Eq,
                Token::BangEq => BinOpKind::NotEq,
                Token::Lt     => BinOpKind::Lt,
                Token::Gt     => BinOpKind::Gt,
                Token::LtEq   => BinOpKind::LtEq,
                Token::GtEq   => BinOpKind::GtEq,
                _             => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right), line };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, PitruckError> {
        let (line, _) = self.span();
        let mut left  = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus  => BinOpKind::Add,
                Token::Minus => BinOpKind::Sub,
                _            => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right), line };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, PitruckError> {
        let (line, _) = self.span();
        let mut left  = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star    => BinOpKind::Mul,
                Token::Slash   => BinOpKind::Div,
                Token::Percent => BinOpKind::Mod,
                _              => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right), line };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, PitruckError> {
        let (line, _) = self.span();
        if matches!(self.peek(), Token::Minus) {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary { op: UnaryOpKind::Neg, expr: Box::new(expr), line });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, PitruckError> {
        let mut expr = self.parse_primary()?;
        loop {
            let (line, _) = self.span();
            match self.peek().clone() {
                Token::LParen => {
                    self.advance();
                    let args = self.parse_call_args()?;
                    self.expect(&Token::RParen)?;
                    expr = Expr::Call { callee: Box::new(expr), args, line };
                }
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::IndexGet { object: Box::new(expr), index: Box::new(index), line };
                }
                Token::Dot => {
                    self.advance();
                    let name = self.expect_ident()?;
                    expr = Expr::Get { object: Box::new(expr), name, line };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn is_lambda_start(&self) -> bool {
        if !matches!(self.tokens[self.pos].0, Token::LParen) { return false; }
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match &self.tokens[i].0 {
                Token::Ident(_) => i += 1,
                Token::Comma => i += 1,
                Token::RParen => {
                    return matches!(self.tokens.get(i + 1).map(|t| &t.0), Some(Token::FatArrow));
                }
                _ => return false,
            }
        }
        false
    }

    fn parse_primary(&mut self) -> Result<Expr, PitruckError> {
        let (line, col) = self.span();

        if self.is_lambda_start() {
            self.expect(&Token::LParen)?;
            let params = self.parse_params()?;
            self.expect(&Token::RParen)?;
            self.expect(&Token::FatArrow)?;
            let body = if matches!(self.peek(), Token::LBrace | Token::Newline | Token::Indent) {
                self.parse_block()?
            } else {
                let e = self.parse_expr()?;
                vec![Stmt::Return { value: Some(e), line }]
            };
            return Ok(Expr::Lambda { params, body, line });
        }

        match self.peek().clone() {
            Token::Number(n)    => { self.advance(); Ok(Expr::Number(n)) }
            Token::StringLit(s) => { self.advance(); Ok(Expr::StringLit(s)) }
            Token::True         => { self.advance(); Ok(Expr::Bool(true)) }
            Token::False        => { self.advance(); Ok(Expr::Bool(false)) }
            Token::Null         => { self.advance(); Ok(Expr::Null) }
            Token::Self_        => { self.advance(); Ok(Expr::Self_ { line }) }

            Token::Ident(name) => {
                self.advance();
                Ok(Expr::Ident { name, line })
            }

            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }

            Token::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                if !matches!(self.peek(), Token::RBracket) {
                    elements.push(self.parse_expr()?);
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        if matches!(self.peek(), Token::RBracket) { break; }
                        elements.push(self.parse_expr()?);
                    }
                }
                self.expect(&Token::RBracket)?;
                Ok(Expr::List { elements, line })
            }

            Token::LBrace => {
                self.advance();
                let mut elements = Vec::new();
                if !matches!(self.peek(), Token::RBrace) {
                    let key = self.parse_expr()?;
                    self.expect(&Token::Colon)?;
                    let val = self.parse_expr()?;
                    elements.push((key, val));
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        if matches!(self.peek(), Token::RBrace) { break; }
                        let key = self.parse_expr()?;
                        self.expect(&Token::Colon)?;
                        let val = self.parse_expr()?;
                        elements.push((key, val));
                    }
                }
                self.expect(&Token::RBrace)?;
                Ok(Expr::Dict { elements, line })
            }

            other => Err(PitruckError::ParseError {
                line,
                col,
                message: format!("expected an expression, found `{}`", token_display(&other)),
            }),
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, PitruckError> {
        let mut args = Vec::new();
        if matches!(self.peek(), Token::RParen) { return Ok(args); }
        args.push(self.parse_expr()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            if matches!(self.peek(), Token::RParen) { break; }
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }
}

fn token_display(t: &Token) -> String {
    match t {
        Token::Number(n)    => format!("{n}"),
        Token::StringLit(s) => format!("\"{s}\""),
        Token::Ident(s)     => s.clone(),
        Token::Var          => "var".to_string(),
        Token::Bring        => "bring".to_string(),
        Token::Func         => "func".to_string(),
        Token::Return       => "return".to_string(),
        Token::If           => "if".to_string(),
        Token::Elif         => "elif".to_string(),
        Token::Else         => "else".to_string(),
        Token::While        => "while".to_string(),
        Token::Print        => "print".to_string(),
        Token::Class        => "class".to_string(),
        Token::Self_        => "self".to_string(),
        Token::Match        => "match".to_string(),
        Token::And          => "and".to_string(),
        Token::Or           => "or".to_string(),
        Token::Not          => "not".to_string(),
        Token::True         => "true".to_string(),
        Token::False        => "false".to_string(),
        Token::Null         => "null".to_string(),
        Token::Plus         => "+".to_string(),
        Token::Minus        => "-".to_string(),
        Token::Star         => "*".to_string(),
        Token::Slash        => "/".to_string(),
        Token::Percent      => "%".to_string(),
        Token::EqEq         => "==".to_string(),
        Token::BangEq       => "!=".to_string(),
        Token::Lt           => "<".to_string(),
        Token::Gt           => ">".to_string(),
        Token::LtEq         => "<=".to_string(),
        Token::GtEq         => ">=".to_string(),
        Token::Eq           => "=".to_string(),
        Token::LBrace       => "{".to_string(),
        Token::RBrace       => "}".to_string(),
        Token::LParen       => "(".to_string(),
        Token::RParen       => ")".to_string(),
        Token::LBracket     => "[".to_string(),
        Token::RBracket     => "]".to_string(),
        Token::Comma        => ",".to_string(),
        Token::Dot          => ".".to_string(),
        Token::FatArrow     => "=>".to_string(),
        Token::Colon        => ":".to_string(),
        Token::Underscore   => "_".to_string(),
        Token::Newline      => "newline".to_string(),
        Token::Indent       => "indent".to_string(),
        Token::Dedent       => "dedent".to_string(),
        Token::EOF          => "end of file".to_string(),
    }
}

use crate::ast::{Expr, Program, Stmt};
use crate::lexer::Token;

pub fn parse(tokens: &[Token]) -> Result<Program, String> {
    let mut p = Parser { tokens, pos: 0 };
    p.program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    fn program(&mut self) -> Result<Program, String> {
        let mut statements = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek(), Token::Eof) {
            statements.push(self.statement()?);
            self.skip_newlines();
        }
        Ok(Program { statements })
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        // Only one statement form exists right now: `name = expr`.
        let name = match self.advance() {
            Token::Ident(name) => name,
            other => return Err(format!("expected a variable name, found {other:?}")),
        };
        match self.advance() {
            Token::Equals => {}
            other => return Err(format!("expected '=' after '{name}', found {other:?}")),
        }
        let value = self.expr()?;
        match self.peek() {
            Token::Newline | Token::Eof => {}
            other => return Err(format!("expected end of statement, found {other:?}")),
        }
        Ok(Stmt::Assign { name, value })
    }

    fn expr(&mut self) -> Result<Expr, String> {
        // No operators yet — an expression is a single literal, identifier,
        // or a (possibly nested) JSON array/object literal.
        match self.peek() {
            Token::LBracket => self.array(),
            Token::LBrace => self.object(),
            _ => match self.advance() {
                Token::Number(n) => Ok(Expr::Number(n)),
                Token::Str(s) => Ok(Expr::Str(s)),
                Token::True => Ok(Expr::Bool(true)),
                Token::False => Ok(Expr::Bool(false)),
                Token::Null => Ok(Expr::Null),
                Token::Ident(name) => Ok(Expr::Ident(name)),
                other => Err(format!("expected an expression, found {other:?}")),
            },
        }
    }

    fn array(&mut self) -> Result<Expr, String> {
        self.advance(); // '['
        self.skip_newlines();
        let mut items = Vec::new();
        if matches!(self.peek(), Token::RBracket) {
            self.advance();
            return Ok(Expr::Array(items));
        }
        loop {
            items.push(self.expr()?);
            self.skip_newlines();
            match self.advance() {
                Token::Comma => self.skip_newlines(),
                Token::RBracket => break,
                other => return Err(format!("expected ',' or ']' in array, found {other:?}")),
            }
        }
        Ok(Expr::Array(items))
    }

    fn object(&mut self) -> Result<Expr, String> {
        self.advance(); // '{'
        self.skip_newlines();
        let mut fields = Vec::new();
        if matches!(self.peek(), Token::RBrace) {
            self.advance();
            return Ok(Expr::Object(fields));
        }
        loop {
            let key = match self.advance() {
                Token::Str(s) => s,
                other => return Err(format!("expected a string key, found {other:?}")),
            };
            match self.advance() {
                Token::Colon => {}
                other => return Err(format!("expected ':' after key, found {other:?}")),
            }
            self.skip_newlines();
            let value = self.expr()?;
            fields.push((key, value));
            self.skip_newlines();
            match self.advance() {
                Token::Comma => self.skip_newlines(),
                Token::RBrace => break,
                other => return Err(format!("expected ',' or '}}' in object, found {other:?}")),
            }
        }
        Ok(Expr::Object(fields))
    }
}

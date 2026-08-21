use crate::ast::{BinOp, Expr, Program, Stmt, UnOp};
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

    // Precedence, loosest to tightest binding: `or` < `and` < `not` <
    // comparison (`== != < > <= >=`, non-chaining — `1 < 2 < 3` parses as
    // `(1 < 2) < 3`, not specially handled) < `+ -` < `* /` < unary `-` <
    // postfix (`.field` / `[index]`) < primary. Standard recursive-descent
    // precedence climbing, one method per tier.
    fn expr(&mut self) -> Result<Expr, String> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut e = self.and_expr()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let rhs = self.and_expr()?;
            e = Expr::Binary(Box::new(e), BinOp::Or, Box::new(rhs));
        }
        Ok(e)
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut e = self.not_expr()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let rhs = self.not_expr()?;
            e = Expr::Binary(Box::new(e), BinOp::And, Box::new(rhs));
        }
        Ok(e)
    }

    fn not_expr(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Token::Not) {
            self.advance();
            let e = self.not_expr()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let e = self.additive()?;
        let op = match self.peek() {
            Token::EqEq => BinOp::Eq,
            Token::NotEq => BinOp::Ne,
            Token::Lt => BinOp::Lt,
            Token::Gt => BinOp::Gt,
            Token::Le => BinOp::Le,
            Token::Ge => BinOp::Ge,
            _ => return Ok(e),
        };
        self.advance();
        let rhs = self.additive()?;
        Ok(Expr::Binary(Box::new(e), op, Box::new(rhs)))
    }

    fn additive(&mut self) -> Result<Expr, String> {
        let mut e = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.multiplicative()?;
            e = Expr::Binary(Box::new(e), op, Box::new(rhs));
        }
        Ok(e)
    }

    fn multiplicative(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let rhs = self.unary()?;
            e = Expr::Binary(Box::new(e), op, Box::new(rhs));
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Token::Minus) {
            self.advance();
            let e = self.unary()?;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(e)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Token::Dot => {
                    self.advance();
                    let field = match self.advance() {
                        Token::Ident(name) => name,
                        other => {
                            return Err(format!("expected a field name after '.', found {other:?}"))
                        }
                    };
                    e = Expr::Field(Box::new(e), field);
                }
                Token::LBracket => {
                    self.advance();
                    self.skip_newlines();
                    let index = self.expr()?;
                    self.skip_newlines();
                    match self.advance() {
                        Token::RBracket => {}
                        other => return Err(format!("expected ']', found {other:?}")),
                    }
                    e = Expr::Index(Box::new(e), Box::new(index));
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
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

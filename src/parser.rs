use crate::ast::{BinOp, EmitTarget, Expr, LoopAccumulator, LoopOver, Program, Stmt, UnOp};
use crate::lexer::{Lexed, StringPart, Token};
use crate::span::Located;

fn starts_uppercase(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

pub fn parse(lexed: &Lexed) -> Result<Program, Located> {
    let mut p = Parser::new(lexed);
    // Every error site below stays a plain `String`; the position is attached
    // once, here, from wherever the parser had got to. That's what keeps
    // locations from having to be threaded through two dozen error sites.
    p.program().map_err(|msg| p.locate(msg))
}

/// Parses a single expression from `tokens` rather than a whole program — a
/// thin wrapper around the same `Parser::expr()` that already parses every
/// literal (`Number`/`Str`/`Bool`/`Null`/`Array`/`Object`) inside a real
/// `.code` program. Exists to decode JSON text into a `Value`
/// (`interpreter::eval_literal`) without a second JSON parser: the
/// language's own literal grammar already *is* JSON's — see `value.rs`'s
/// `Display` impl, which emits the other direction for free. Used by
/// `crates/code-wasm` to turn a JS callback's returned JSON string back into
/// a `Value`. Errors if anything besides the one expression (plus
/// surrounding newlines) remains.
pub fn parse_expr(lexed: &Lexed) -> Result<Expr, Located> {
    let mut p = Parser::new(lexed);
    let parsed = (|| {
        p.skip_newlines();
        let expr = p.expr()?;
        p.skip_newlines();
        if !matches!(p.peek(), Token::Eof) {
            p.err_here();
            return Err(format!("expected end of input, found {:?}", p.peek()));
        }
        Ok(expr)
    })();
    parsed.map_err(|msg| p.locate(msg))
}

struct Parser<'a> {
    tokens: &'a [Token],
    /// Parallel to `tokens` — see `Lexed`. Only ever read by `locate`.
    starts: &'a [u32],
    pos: usize,
    /// Which token an error should point at. Nearly every error site here
    /// reports on a token it has just `advance()`d past, so this tracks the
    /// last token *consumed* rather than the current one; the few sites that
    /// `peek()` instead call `err_here` to correct it.
    err_pos: usize,
    /// How many `loop` bodies enclose the statement being parsed — the only
    /// piece of context this otherwise context-free parser carries. `break`
    /// is rejected here, at zero depth, rather than in a later pass so that
    /// both output modes reject it identically for free (the interpreter has
    /// no equivalent of codegen's `verify_defined` pass to hook into).
    loop_depth: usize,
    /// How many `{ }` bodies enclose the statement being parsed. `link` and
    /// `export` are module-structure declarations and are rejected anywhere
    /// but zero: a name declared inside a block is block-local, so exporting
    /// it could never mean anything, and confining `link` to the top level
    /// means `loader.rs` only has to scan one flat statement list.
    block_depth: usize,
}

impl<'a> Parser<'a> {
    fn new(lexed: &'a Lexed) -> Self {
        Parser {
            tokens: &lexed.tokens,
            starts: &lexed.starts,
            pos: 0,
            err_pos: 0,
            loop_depth: 0,
            block_depth: 0,
        }
    }

    /// Turns one of this parser's plain-`String` errors into a located one,
    /// using wherever `err_pos` last pointed. The single place a position is
    /// attached — see `parse`.
    fn locate(&self, msg: String) -> Located {
        Located {
            at: self.starts.get(self.err_pos).copied(),
            msg,
        }
    }

    /// Points the next error at the *current* token rather than the last
    /// consumed one — for the handful of sites that `peek()` and reject
    /// without consuming.
    fn err_here(&mut self) {
        self.err_pos = self.pos;
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.err_pos = self.pos;
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
        if matches!(self.peek(), Token::Assert) {
            self.advance();
            let value = self.expr()?;
            self.expect_end_of_statement()?;
            return Ok(Stmt::Assert(value));
        }

        if matches!(self.peek(), Token::If) {
            self.advance();
            let condition = self.expr()?;
            let body = self.block()?;
            self.expect_end_of_statement()?;
            return Ok(Stmt::If { condition, body });
        }

        if matches!(self.peek(), Token::Loop) {
            self.advance();
            return self.loop_statement();
        }

        if matches!(self.peek(), Token::Emit) {
            self.advance();
            let particle = self.expr()?;
            match self.advance() {
                Token::To => {}
                other => {
                    return Err(format!(
                        "expected 'to' after emit's argument, found {other:?}"
                    ))
                }
            }
            // `emit Name to ...` — a bare uppercase name is the empty
            // particle of that class, exactly what `Name {}` desugars to
            // in `primary`. Rewritten here, after `to` is confirmed, so
            // `primary` keeps treating an uppercase name as an ordinary
            // identifier everywhere else. Lowercase names fall through
            // untouched: they stay variable reads.
            let particle = match particle {
                Expr::Ident(name) if starts_uppercase(&name) => {
                    Expr::Object(vec![("_class".to_string(), Expr::Str(name))])
                }
                other => other,
            };
            let target = match self.advance() {
                Token::Core => EmitTarget::Core,
                Token::Ident(name) => EmitTarget::Module(name),
                other => {
                    return Err(format!(
                        "expected 'core' or a linked module's name after 'to', found {other:?}"
                    ))
                }
            };
            let result = if matches!(self.peek(), Token::Get) {
                self.advance();
                match self.advance() {
                    Token::Ident(name) => Some(name),
                    other => return Err(format!("expected a name after 'get', found {other:?}")),
                }
            } else {
                None
            };
            self.expect_end_of_statement()?;
            return Ok(Stmt::Emit {
                particle,
                target,
                result,
            });
        }

        if matches!(self.peek(), Token::Break) {
            self.advance();
            if self.loop_depth == 0 {
                return Err("'break' outside of a loop".to_string());
            }
            self.expect_end_of_statement()?;
            return Ok(Stmt::Break);
        }

        if matches!(self.peek(), Token::Continue) {
            self.advance();
            if self.loop_depth == 0 {
                return Err("'continue' outside of a loop".to_string());
            }
            self.expect_end_of_statement()?;
            return Ok(Stmt::Continue);
        }

        // A bare block: unambiguous at statement-start, since object
        // literals only ever appear in expression position (the right-hand
        // side of `=`, an array element, ...), never here.
        if matches!(self.peek(), Token::LBrace) {
            let body = self.block()?;
            self.expect_end_of_statement()?;
            return Ok(Stmt::Block(body));
        }

        if matches!(self.peek(), Token::Link) {
            self.advance();
            if self.block_depth > 0 {
                return Err("'link' is only allowed at the top level of a file".to_string());
            }
            let path = match self.advance() {
                Token::Str(path) => path,
                other => {
                    return Err(format!(
                        "expected a quoted module path after 'link', found {other:?}"
                    ))
                }
            };
            let alias = if matches!(self.peek(), Token::As) {
                self.advance();
                match self.advance() {
                    Token::Ident(name) => Some(name),
                    other => return Err(format!("expected a name after 'as', found {other:?}")),
                }
            } else {
                None
            };
            self.expect_end_of_statement()?;
            return Ok(Stmt::Link { path, alias });
        }

        let exported = if matches!(self.peek(), Token::Export) {
            self.advance();
            if self.block_depth > 0 {
                return Err("'export' is only allowed at the top level of a file".to_string());
            }
            if !matches!(self.peek(), Token::Let) {
                self.err_here();
                return Err(format!(
                    "'export' must be followed by 'let' — it marks a declaration, and \
                     'let' is the only way to declare a name (found {:?})",
                    self.peek()
                ));
            }
            true
        } else {
            false
        };

        if matches!(self.peek(), Token::Let) {
            self.advance();
            let name = match self.advance() {
                Token::Ident(name) => name,
                other => {
                    return Err(format!(
                        "expected a variable name after 'let', found {other:?}"
                    ))
                }
            };
            match self.advance() {
                Token::Equals => {}
                other => return Err(format!("expected '=' after 'let {name}', found {other:?}")),
            }
            let value = self.expr()?;
            self.expect_end_of_statement()?;
            return Ok(Stmt::Let {
                name,
                value,
                exported,
            });
        }

        // Otherwise the only statement form is `name = expr` (reassignment
        // — see `ast::Stmt::Assign`'s doc comment; `let` is the only way to
        // introduce a name).
        let name = match self.advance() {
            Token::Ident(name) => name,
            other => {
                return Err(format!(
                "expected a variable name, 'let', 'assert', 'if', 'loop', or '{{', found {other:?}"
            ))
            }
        };
        // `+=` is the one compound form. It is pure sugar, rewritten here
        // into `name = name + expr` so that neither backend — nor anything
        // else downstream — learns it exists. Whatever `+` means for the two
        // operands is therefore exactly what `+=` means, including appending
        // to an array (see `ast::BinOp`).
        let compound = match self.advance() {
            Token::Equals => false,
            Token::PlusEq => true,
            other => {
                return Err(format!(
                    "expected '=' or '+=' after '{name}', found {other:?}"
                ))
            }
        };
        let value = self.expr()?;
        self.expect_end_of_statement()?;
        let value = if compound {
            Expr::Binary(
                Box::new(Expr::Ident(name.clone())),
                BinOp::Add,
                Box::new(value),
            )
        } else {
            value
        };
        Ok(Stmt::Assign { name, value })
    }

    /// Everything after the `loop` keyword:
    /// `[key, ] value over iterable] [get name [= init]] { body }`.
    ///
    /// Both clauses are optional and independent — see `Stmt::Loop`. The
    /// `over` clause is recognised by a leading identifier, which is
    /// unambiguous: the only other things that can follow `loop` are `get`
    /// and `{`, both of which are their own token.
    ///
    /// The first identifier is provisionally `value`; if a `,` follows, that
    /// identifier demotes to `key` and the name after the comma becomes the
    /// real `value` — this is what makes `loop v over xs` bind the value
    /// with no comma at all, per the right-alignment rule on `Stmt::Loop`.
    fn loop_statement(&mut self) -> Result<Stmt, String> {
        let over = if matches!(self.peek(), Token::Ident(_)) {
            let Token::Ident(first) = self.advance() else {
                unreachable!("just peeked an Ident")
            };
            let (key, value) = if matches!(self.peek(), Token::Comma) {
                self.advance();
                match self.advance() {
                    Token::Ident(name) => (Some(first), name),
                    other => {
                        return Err(format!(
                            "expected a value variable name after ',', found {other:?}"
                        ))
                    }
                }
            } else {
                (None, first)
            };
            match self.advance() {
                Token::Over => {}
                other => {
                    return Err(format!(
                        "expected 'over' after 'loop {value}', found {other:?} \
                         (a bare infinite loop is written `loop {{ }}`, with no variable)"
                    ))
                }
            }
            // The `{` that opens the body can't be mistaken for an object
            // literal here for the same reason as `if`'s condition: `{` is
            // not an operator, so the expression grammar always stops before
            // it (see `primary`'s LBrace case, only reachable in operand
            // position).
            let iterable = self.expr()?;
            Some(LoopOver {
                key,
                value,
                iterable,
            })
        } else {
            None
        };

        // `get <name> [= <init>]`. The init expression stops before `{` for
        // the same reason `iterable` does.
        let result_name = if matches!(self.peek(), Token::Get) {
            self.advance();
            let name = match self.advance() {
                Token::Ident(name) => name,
                other => return Err(format!("expected a name after 'get', found {other:?}")),
            };
            let init = if matches!(self.peek(), Token::Equals) {
                self.advance();
                Some(self.expr()?)
            } else {
                None
            };
            Some((name, init))
        } else {
            None
        };

        self.loop_depth += 1;
        let body = self.block();
        self.loop_depth -= 1;
        let body = body?;
        self.expect_end_of_statement()?;

        let result = result_name.map(|(name, init)| LoopAccumulator {
            name,
            // No `= init` means the accumulator has nothing to start from.
            // Null rather than `[]`: the body decides what it is building by
            // what it assigns, and guessing "array" would be wrong as often
            // as right.
            init: init.unwrap_or(Expr::Null),
        });

        Ok(Stmt::Loop { over, result, body })
    }

    /// `{ stmt* }` — shared by `if`, `loop`, and the bare-block statement:
    /// "brace, statements separated/terminated by newlines, brace".
    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        match self.advance() {
            Token::LBrace => {}
            other => return Err(format!("expected '{{', found {other:?}")),
        }
        self.skip_newlines();
        let mut statements = Vec::new();
        self.block_depth += 1;
        while !matches!(self.peek(), Token::RBrace) {
            match self.statement() {
                Ok(stmt) => statements.push(stmt),
                Err(e) => {
                    self.block_depth -= 1;
                    return Err(e);
                }
            }
            self.skip_newlines();
        }
        self.block_depth -= 1;
        self.advance(); // '}'
        Ok(statements)
    }

    fn expect_end_of_statement(&mut self) -> Result<(), String> {
        match self.peek() {
            Token::Newline | Token::Eof => Ok(()),
            other => {
                let msg = format!("expected end of statement, found {other:?}");
                self.err_here();
                Err(msg)
            }
        }
    }

    // Precedence, loosest to tightest binding: `or` < `and` < `not` <
    // comparison (`= ≠ < > ≤ ≥`) < `is` < `+ -` < `* /` < unary `-` < postfix
    // (`.field` / `[index]`) < primary. Standard recursive-descent
    // precedence climbing, one method per tier. Comparison is the one
    // non-looping tier: it matches at most one operator, so a chain like
    // `1 < 2 < 3` is a parse error ("expected end of statement") rather than
    // grouping as `(1 < 2) < 3`. The old language wrote its comparisons the
    // same way but split them across two tiers, which made `a < b = c`
    // legal there; here it is not.
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
        let e = self.is_expr()?;
        let op = match self.peek() {
            // The same `=` that separates a name from its value in a
            // statement. See `lexer::Token::Equals` for why the two uses
            // can't collide.
            Token::Equals => BinOp::Eq,
            Token::NotEq => BinOp::Ne,
            Token::Lt => BinOp::Lt,
            Token::Gt => BinOp::Gt,
            Token::Le => BinOp::Le,
            Token::Ge => BinOp::Ge,
            _ => return Ok(e),
        };
        self.advance();
        let rhs = self.is_expr()?;
        Ok(Expr::Binary(Box::new(e), op, Box::new(rhs)))
    }

    /// `expr is ClassName` — the type test (see `Expr::Is`). Sits between
    /// comparison and additive on the ladder, so `t is TimestampResult and
    /// t.value ≥ 0` reads left-to-right without parentheses, and `1 + x is
    /// Foo` groups as `(1 + x) is Foo`. Non-looping, like comparison:
    /// `a is B is C` falls out as "expected end of statement", which is
    /// fine — chaining tests is nonsense anyway.
    fn is_expr(&mut self) -> Result<Expr, String> {
        let e = self.additive()?;
        if !matches!(self.peek(), Token::Is) {
            return Ok(e);
        }
        self.advance();
        let class = match self.advance() {
            Token::Ident(name) => name,
            other => return Err(format!("expected a class name after 'is', found {other:?}")),
        };
        Ok(Expr::Is(Box::new(e), class))
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
            // `(expr)` is pure grouping — no AST node of its own, just
            // however `expr` composed while parsing inside the parens (the
            // full precedence chain again, so `(a + b) * c` groups exactly
            // as written).
            Token::LParen => {
                self.advance();
                self.skip_newlines();
                let e = self.expr()?;
                self.skip_newlines();
                match self.advance() {
                    Token::RParen => Ok(e),
                    other => Err(format!("expected ')', found {other:?}")),
                }
            }
            _ => match self.advance() {
                Token::Number(n) => Ok(Expr::Number(n)),
                Token::Str(s) => Ok(Expr::Str(s)),
                // Nothing to split here — the lexer hands the parts over
                // already separated, since it is the quote-scanning loop that
                // owns string internals.
                Token::InterpStr(parts) => Ok(Expr::Interpolated(
                    parts
                        .into_iter()
                        .map(|part| match part {
                            StringPart::Lit(s) => Expr::Str(s),
                            StringPart::Var(name) => Expr::Ident(name),
                        })
                        .collect(),
                )),
                Token::True => Ok(Expr::Bool(true)),
                Token::False => Ok(Expr::Bool(false)),
                Token::Null => Ok(Expr::Null),
                // `ClassName { fields }` — particle construction. Pure sugar,
                // resolved entirely here: it desugars into the exact same
                // `Expr::Object` a plain `{ ... }` literal would produce,
                // with a `"_class"` field holding the name prepended. No new
                // AST node, no new Value kind — see memory
                // `new-code-particle`. The only rule is lexical: an
                // uppercase-first name immediately followed by `{` is a
                // particle; anything else (no `{` next, or a lowercase
                // name) is an ordinary identifier, exactly as before this
                // was added.
                Token::Ident(name)
                    if starts_uppercase(&name) && matches!(self.peek(), Token::LBrace) =>
                {
                    let mut fields = self.object_fields()?;
                    fields.insert(0, ("_class".to_string(), Expr::Str(name)));
                    Ok(Expr::Object(fields))
                }
                Token::Ident(name) => Ok(Expr::Ident(name)),
                // A stray `=` where a value belongs is what `==`, `<=` and
                // `>=` all decay into now that each comparison operator is
                // a single character — the first character is consumed as
                // its own operator and the `=` is left stranded here. Worth
                // naming explicitly: every program written before the
                // operators changed hits exactly this.
                Token::Equals => Err(
                    "expected an expression, found '='. The comparison operators are \
                     '=' '≠' '<' '>' '≤' '≥' — '==', '<=' and '>=' are not operators"
                        .to_string(),
                ),
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
        Ok(Expr::Object(self.object_fields()?))
    }

    /// `{ "key": expr, ... }`'s body, from just after the `{` — shared by
    /// `object()` and particle construction (see `primary`'s `Token::Ident`
    /// case), which is why particle fields accept exactly the same syntax
    /// and reject exactly the same malformed input (trailing comma, a bare
    /// identifier as a key, ...) as a plain object literal does.
    fn object_fields(&mut self) -> Result<Vec<(String, Expr)>, String> {
        self.advance(); // '{'
        self.skip_newlines();
        let mut fields = Vec::new();
        if matches!(self.peek(), Token::RBrace) {
            self.advance();
            return Ok(fields);
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
        Ok(fields)
    }
}

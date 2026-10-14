//! The interpreter's executable tree. Names are resolved once, after verification.
//! Lexical blocks share their function's frame; only file and handler calls
//! switch frames. The parsed AST stays untouched for checking and compilation.
use std::collections::HashMap;
use std::ops::{Deref, Range};
use std::rc::Rc;

use crate::ast::{self, BinOp, IsTest, NativeFormat, UnOp};
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Binding {
    pub name: String,
    pub frame: usize,
    pub slot: usize,
    pub number: Option<usize>,
}
impl Deref for Binding {
    type Target = str;
    fn deref(&self) -> &str {
        &self.name
    }
}
impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub(super) struct Field {
    pub field: String,
    pub name: Binding,
}
#[derive(Debug, Clone)]
pub(super) enum EmitResult {
    Whole(Binding),
    Fields(Vec<Field>),
}
#[derive(Debug, Clone)]
pub(super) enum EmitTarget {
    Core,
    This,
    Base,
    Module(String, Option<Binding>),
}
#[derive(Debug, Clone)]
pub(super) struct LoopOver {
    pub key: Option<Binding>,
    pub value: Binding,
    pub iterable: Expr,
}
#[derive(Debug, Clone)]
pub(super) enum FieldKey {
    Literal(String),
    Computed(Expr),
}
#[derive(Debug, Clone)]
pub(super) enum NumberExpr {
    Literal(f64),
    Slot { frame: usize, number: usize },
    Neg(Box<NumberExpr>),
    Binary(Box<NumberExpr>, NumberOp, Box<NumberExpr>),
}
#[derive(Debug, Clone, Copy)]
pub(super) enum NumberOp {
    Add,
    Sub,
    Mul,
    Div,
}
#[derive(Debug, Clone)]
pub(super) enum Expr {
    Number {
        fast: NumberExpr,
        fallback: Box<Expr>,
    },
    NumberCompare {
        lhs: Box<NumberExpr>,
        op: BinOp,
        rhs: Box<NumberExpr>,
        fallback: Box<Expr>,
    },
    Literal(Value),
    Ident(Binding),
    Interpolated(Vec<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(FieldKey, Expr)>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Slice {
        value: Box<Expr>,
        from: Box<Expr>,
        to: Box<Expr>,
    },
    LengthOf(Box<Expr>),
    SetAt {
        base: Box<Expr>,
        path: Vec<Expr>,
        value: Box<Expr>,
    },
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Is(Box<Expr>, IsTest),
}
#[derive(Debug, Clone)]
pub(super) enum Stmt {
    Assign {
        name: Binding,
        value: Expr,
    },
    Assert(Expr),
    If {
        condition: Expr,
        body: Block,
    },
    Loop {
        over: Option<LoopOver>,
        body: Block,
    },
    Import {
        alias: Option<Binding>,
        body: Block,
        file: usize,
    },
    ImportNative {
        alias: Binding,
        path: String,
        format: NativeFormat,
    },
    LinkRuntime {
        alias: Binding,
        path: Expr,
    },
    Unlink(Expr),
    Emit {
        particle: Expr,
        target: EmitTarget,
        result: Option<EmitResult>,
    },
    Break,
    Continue,
    HandlerDef {
        class_name: String,
        fields: Vec<Field>,
        body: Block,
    },
    Return {
        value: Expr,
    },
    TypeDef,
}

pub(super) type Names = Rc<HashMap<String, usize>>;
#[derive(Debug, Clone, Default)]
pub(super) struct Layout {
    pub names: Names,
    pub size: usize,
    pub numbers: Rc<HashMap<usize, usize>>,
}
#[derive(Debug, Clone)]
pub(super) struct Block {
    pub statements: Vec<Stmt>,
    /// Slots introduced here (including nested blocks), cleared on every exit.
    pub locals: Range<usize>,
    pub frame: usize,
    pub layout: Layout,
}
impl Deref for Block {
    type Target = [Stmt];
    fn deref(&self) -> &[Stmt] {
        &self.statements
    }
}

pub(super) struct Tree {
    pub body: Block,
    pub files: Vec<Layout>,
}

struct Resolver {
    scopes: Vec<HashMap<String, Binding>>,
    next: usize,
    frame: usize,
    files: Vec<Layout>,
    kinds: crate::kinds::Kinds,
    scalar: HashMap<String, crate::kinds::Kind>,
    numbers: HashMap<usize, usize>,
    inferred: crate::infer::Layouts,
}
impl Resolver {
    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes.iter().rev().find_map(|s| s.get(name)).cloned()
    }
    fn bind(&mut self, name: &str) -> Binding {
        let number = (self.scalar.get(name) == Some(&crate::kinds::Kind::Number)).then(|| {
            let index = self.numbers.len();
            self.numbers.insert(self.next, index);
            index
        });
        let binding = Binding {
            name: name.into(),
            frame: self.frame,
            slot: self.next,
            number,
        };
        self.next += 1;
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.into(), binding.clone());
        binding
    }
    fn fields(&mut self, fields: &[ast::Field]) -> Vec<Field> {
        fields
            .iter()
            .map(|f| Field {
                field: f.field.clone(),
                name: self.bind(&f.name),
            })
            .collect()
    }
    fn number(&self, e: &ast::Expr) -> Option<NumberExpr> {
        Some(match e {
            ast::Expr::Number(n) => NumberExpr::Literal(*n),
            ast::Expr::Ident(name)
                if self
                    .inferred
                    .numbers
                    .contains(&(e as *const ast::Expr as usize)) =>
            {
                let binding = self.lookup(name)?;
                NumberExpr::Slot {
                    frame: binding.frame,
                    number: binding.number?,
                }
            }
            ast::Expr::Unary(UnOp::Neg, value) => NumberExpr::Neg(Box::new(self.number(value)?)),
            ast::Expr::Binary(lhs, op, rhs) => {
                let op = match op {
                    BinOp::Add => NumberOp::Add,
                    BinOp::Sub => NumberOp::Sub,
                    BinOp::Mul => NumberOp::Mul,
                    BinOp::Div => NumberOp::Div,
                    _ => return None,
                };
                NumberExpr::Binary(Box::new(self.number(lhs)?), op, Box::new(self.number(rhs)?))
            }
            _ => return None,
        })
    }
    fn expr(&self, e: &ast::Expr) -> Expr {
        // A literal already borrows its payload in generic expressions.
        // Numeric arithmetic includes it directly in its own compact tree.
        if matches!(e, ast::Expr::Number(_)) {
            return self.plain_expr(e, false);
        }
        if let Some(fast) = self.number(e) {
            return Expr::Number {
                fast,
                fallback: Box::new(self.plain_expr(e, false)),
            };
        }
        if let ast::Expr::Binary(
            lhs,
            op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge),
            rhs,
        ) = e
        {
            if let (Some(lhs), Some(rhs)) = (self.number(lhs), self.number(rhs)) {
                return Expr::NumberCompare {
                    lhs: Box::new(lhs),
                    op: *op,
                    rhs: Box::new(rhs),
                    fallback: Box::new(self.plain_expr(e, false)),
                };
            }
        }
        self.plain_expr(e, true)
    }
    fn plain_expr(&self, e: &ast::Expr, specialise: bool) -> Expr {
        let child = |e| {
            if specialise {
                self.expr(e)
            } else {
                self.plain_expr(e, false)
            }
        };
        let boxed = |e| Box::new(child(e));
        match e {
            ast::Expr::Number(n) => Expr::Literal(Value::Number(*n)),
            ast::Expr::Str(s) => Expr::Literal(Value::Str(Rc::from(s.as_str()))),
            ast::Expr::Bool(b) => Expr::Literal(Value::Bool(*b)),
            ast::Expr::Null => Expr::Literal(Value::Null),
            ast::Expr::Ident(n) => Expr::Ident(self.lookup(n).expect("verified name")),
            ast::Expr::Interpolated(v) => Expr::Interpolated(v.iter().map(&child).collect()),
            ast::Expr::Array(v) => Expr::Array(v.iter().map(&child).collect()),
            ast::Expr::Object(v) => Expr::Object(
                v.iter()
                    .map(|(k, v)| {
                        (
                            match k {
                                ast::FieldKey::Literal(s) => FieldKey::Literal(s.clone()),
                                ast::FieldKey::Computed(e) => FieldKey::Computed(child(e)),
                            },
                            child(v),
                        )
                    })
                    .collect(),
            ),
            ast::Expr::Field(e, n) => Expr::Field(boxed(e), n.clone()),
            ast::Expr::Index(e, i) => Expr::Index(boxed(e), boxed(i)),
            ast::Expr::Slice { value, from, to } => Expr::Slice {
                value: boxed(value),
                from: boxed(from),
                to: boxed(to),
            },
            ast::Expr::LengthOf(e) => Expr::LengthOf(boxed(e)),
            ast::Expr::SetAt { base, path, value } => Expr::SetAt {
                base: boxed(base),
                path: path.iter().map(&child).collect(),
                value: boxed(value),
            },
            ast::Expr::Binary(a, op, b) => Expr::Binary(boxed(a), *op, boxed(b)),
            ast::Expr::Unary(op, e) => Expr::Unary(*op, boxed(e)),
            ast::Expr::Is(e, t) => Expr::Is(boxed(e), t.clone()),
        }
    }
    fn block(
        &mut self,
        stmts: &[ast::Stmt],
        fields: &[ast::Field],
        over: Option<&ast::LoopOver>,
    ) -> (Block, Vec<Field>, Option<LoopOver>) {
        let start = self.next;
        // The iterable belongs to the enclosing scope, before the binders.
        let iterable = over.map(|o| self.expr(&o.iterable));
        self.scopes.push(HashMap::new());
        let fields = self.fields(fields);
        let over = over.zip(iterable).map(|(o, iterable)| LoopOver {
            key: o.key.as_ref().map(|k| self.bind(k)),
            value: self.bind(&o.value),
            iterable,
        });
        let statements = self.stmts(stmts);
        let names = self
            .scopes
            .pop()
            .unwrap()
            .into_iter()
            .map(|(n, b)| (n, b.slot))
            .collect();
        (
            Block {
                statements,
                locals: start..self.next,
                frame: self.frame,
                layout: Layout {
                    names: Rc::new(names),
                    size: self.next,
                    numbers: Rc::default(),
                },
            },
            fields,
            over,
        )
    }
    fn stmts(&mut self, stmts: &[ast::Stmt]) -> Vec<Stmt> {
        stmts
            .iter()
            .map(|s| match s {
                ast::Stmt::Assign { name, value, .. } => {
                    let value = self.expr(value);
                    let name = self.lookup(name).unwrap_or_else(|| self.bind(name));
                    Stmt::Assign { name, value }
                }
                ast::Stmt::Assert(e) => Stmt::Assert(self.expr(e)),
                ast::Stmt::If { condition, body } => {
                    let condition = self.expr(condition);
                    let (body, _, _) = self.block(body, &[], None);
                    Stmt::If { condition, body }
                }
                ast::Stmt::Loop { over, body } => {
                    let (body, _, over) = self.block(body, &[], over.as_ref());
                    Stmt::Loop { over, body }
                }
                ast::Stmt::HandlerDef {
                    class_name,
                    fields,
                    body,
                } => {
                    let globals = self.scopes[0].clone();
                    let saved = std::mem::replace(&mut self.scopes, vec![globals]);
                    let next = std::mem::replace(&mut self.next, 0);
                    let frame = std::mem::replace(&mut self.frame, 1);
                    let numbers = std::mem::take(&mut self.numbers);
                    let scalar = std::mem::replace(
                        &mut self.scalar,
                        self.kinds
                            .handlers
                            .get(class_name)
                            .cloned()
                            .unwrap_or_default(),
                    );
                    let (mut body, fields, _) = self.block(body, fields, None);
                    body.layout.numbers = Rc::new(std::mem::replace(&mut self.numbers, numbers));
                    self.scalar = scalar;
                    self.scopes = saved;
                    self.next = next;
                    self.frame = frame;
                    Stmt::HandlerDef {
                        class_name: class_name.clone(),
                        fields,
                        body,
                    }
                }
                ast::Stmt::Import {
                    alias, body, file, ..
                } => {
                    let saved = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
                    let next = std::mem::replace(&mut self.next, 0);
                    let frame = std::mem::replace(&mut self.frame, 0);
                    let numbers = std::mem::take(&mut self.numbers);
                    let statements = self.stmts(body);
                    let layout = self.layout();
                    if self.files.len() <= *file {
                        self.files.resize_with(file + 1, Layout::default);
                    }
                    self.files[*file] = layout.clone();
                    let body = Block {
                        statements,
                        locals: 0..self.next,
                        frame: 0,
                        layout,
                    };
                    self.scopes = saved;
                    self.next = next;
                    self.frame = frame;
                    self.numbers = numbers;
                    Stmt::Import {
                        alias: alias.as_ref().map(|n| self.bind(n)),
                        body,
                        file: *file,
                    }
                }
                ast::Stmt::ImportNative {
                    alias,
                    path,
                    format,
                } => Stmt::ImportNative {
                    alias: self.bind(alias),
                    path: path.clone(),
                    format: format.clone(),
                },
                ast::Stmt::LinkRuntime { alias, path } => {
                    let path = self.expr(path);
                    Stmt::LinkRuntime {
                        alias: self.bind(alias),
                        path,
                    }
                }
                ast::Stmt::Unlink(e) => Stmt::Unlink(self.expr(e)),
                ast::Stmt::Emit {
                    particle,
                    target,
                    result,
                    ..
                } => {
                    let particle = self.expr(particle);
                    let target = match target {
                        ast::EmitTarget::Core => EmitTarget::Core,
                        ast::EmitTarget::This => EmitTarget::This,
                        ast::EmitTarget::Base => EmitTarget::Base,
                        ast::EmitTarget::Module(n) => EmitTarget::Module(n.clone(), self.lookup(n)),
                    };
                    let result = result.as_ref().map(|r| match r {
                        ast::EmitResult::Whole(n) => EmitResult::Whole(self.bind(n)),
                        ast::EmitResult::Fields(f) => EmitResult::Fields(self.fields(f)),
                    });
                    Stmt::Emit {
                        particle,
                        target,
                        result,
                    }
                }
                ast::Stmt::Return { value, .. } => Stmt::Return {
                    value: self.expr(value),
                },
                ast::Stmt::Break => Stmt::Break,
                ast::Stmt::Continue => Stmt::Continue,
                ast::Stmt::TypeDef { .. } => Stmt::TypeDef,
                ast::Stmt::Link { .. } => unreachable!("verified resolved links"),
            })
            .collect()
    }
    fn layout(&self) -> Layout {
        Layout {
            names: Rc::new(
                self.scopes[0]
                    .iter()
                    .map(|(n, b)| (n.clone(), b.slot))
                    .collect(),
            ),
            size: self.next,
            numbers: Rc::new(self.numbers.clone()),
        }
    }
}
pub(super) fn resolve(program: &ast::Program) -> Tree {
    let kinds = crate::kinds::of_program(program);
    let scalar = kinds.top.clone();
    let mut r = Resolver {
        scopes: vec![HashMap::new()],
        next: 0,
        frame: 0,
        files: vec![Layout::default()],
        kinds,
        scalar,
        numbers: HashMap::new(),
        inferred: crate::infer::field_layouts(program),
    };
    let statements = r.stmts(&program.statements);
    let layout = r.layout();
    r.files[0] = layout.clone();
    Tree {
        body: Block {
            statements,
            locals: 0..r.next,
            frame: 0,
            layout,
        },
        files: r.files,
    }
}

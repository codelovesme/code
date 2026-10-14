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
pub(super) enum Expr {
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
}
impl Resolver {
    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes.iter().rev().find_map(|s| s.get(name)).cloned()
    }
    fn bind(&mut self, name: &str) -> Binding {
        let binding = Binding {
            name: name.into(),
            frame: self.frame,
            slot: self.next,
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
    fn expr(&self, e: &ast::Expr) -> Expr {
        let boxed = |e| Box::new(self.expr(e));
        match e {
            ast::Expr::Number(n) => Expr::Literal(Value::Number(*n)),
            ast::Expr::Str(s) => Expr::Literal(Value::Str(Rc::from(s.as_str()))),
            ast::Expr::Bool(b) => Expr::Literal(Value::Bool(*b)),
            ast::Expr::Null => Expr::Literal(Value::Null),
            ast::Expr::Ident(n) => Expr::Ident(self.lookup(n).expect("verified name")),
            ast::Expr::Interpolated(v) => {
                Expr::Interpolated(v.iter().map(|e| self.expr(e)).collect())
            }
            ast::Expr::Array(v) => Expr::Array(v.iter().map(|e| self.expr(e)).collect()),
            ast::Expr::Object(v) => Expr::Object(
                v.iter()
                    .map(|(k, v)| {
                        (
                            match k {
                                ast::FieldKey::Literal(s) => FieldKey::Literal(s.clone()),
                                ast::FieldKey::Computed(e) => FieldKey::Computed(self.expr(e)),
                            },
                            self.expr(v),
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
                path: path.iter().map(|e| self.expr(e)).collect(),
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
                    let (body, fields, _) = self.block(body, fields, None);
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
        }
    }
}
pub(super) fn resolve(program: &ast::Program) -> Tree {
    let mut r = Resolver {
        scopes: vec![HashMap::new()],
        next: 0,
        frame: 0,
        files: vec![Layout::default()],
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

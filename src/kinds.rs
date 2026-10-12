//! Which variables only ever hold a number, or only ever a boolean (ticket
//! 113). Such a variable needs no `CodeValue` slot: codegen keeps it in a
//! plain `double` (or flag) on the stack, which LLVM puts in a register, and
//! boxes it only where it leaves — into a list, an object, an emit.
//!
//! The rule is about operators, not declarations: `-`, `*`, `/` and unary
//! `-` can only answer a number (or fail), `+` a number when both sides are
//! numbers, and the comparisons, `=`, `≠`, `and`, `or`, `not` and `∈` only a
//! boolean. A variable is a number when every assignment to it is one of
//! those — worked out together, so `i = i + 1` keeps `i` a number. That is
//! sound by execution order: the first assignment to run reads only
//! variables that already hold values, and each of those that is classed a
//! number holds one.
//!
//! The answer is by *name*, per function — the top level of a program (with
//! everything it links, which runs in the same function) or one handler.
//! Two bindings of one name in one function are classed together, which can
//! only ever leave one boxed that need not be. A name is left out entirely
//! when something other than an assignment binds it (a loop variable, a
//! handler field, a `get`, a link), and where it could be the same binding
//! as one in another function: a top-level name any handler mentions, and a
//! handler's name that is also a top-level one.

use std::collections::{HashMap, HashSet};

use crate::ast::{BinOp, EmitResult, EmitTarget, Expr, FieldKey, LoopOver, Program, Stmt, UnOp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Number,
    Boolean,
}

/// The classed variables: the top level's, and each handler's by class.
#[derive(Default)]
pub(crate) struct Kinds {
    pub top: HashMap<String, Kind>,
    pub handlers: HashMap<String, HashMap<String, Kind>>,
}

pub(crate) fn of_program(program: &Program) -> Kinds {
    let mut top = Function::default();
    let mut handlers = Vec::new();
    top.collect(&program.statements, &mut handlers);

    // Names a handler mentions, read or written: any of them may be the
    // top level's own binding.
    let mut mentioned = HashSet::new();
    for (_, function) in &handlers {
        mentioned.extend(function.mentioned.iter().cloned());
    }
    let top_names: HashSet<String> = top
        .assigned
        .iter()
        .map(|(name, _)| name.clone())
        .chain(top.bound.iter().cloned())
        .collect();

    let mut kinds = Kinds {
        top: top.classify(&mentioned),
        handlers: HashMap::new(),
    };
    for (class, function) in &handlers {
        kinds
            .handlers
            .insert(class.clone(), function.classify(&top_names));
    }
    kinds
}

/// What one function's statements do with names.
#[derive(Default)]
struct Function<'p> {
    /// Every assignment, in order: the name and the value.
    assigned: Vec<(String, &'p Expr)>,
    /// Names bound some other way, or used where a module is expected.
    bound: HashSet<String>,
    /// Every name read or written — what another function could share.
    mentioned: HashSet<String>,
}

impl<'p> Function<'p> {
    /// Walks `stmts`, gathering nested handler bodies into `handlers` as
    /// functions of their own.
    fn collect(&mut self, stmts: &'p [Stmt], handlers: &mut Vec<(String, Function<'p>)>) {
        for stmt in stmts {
            match stmt {
                Stmt::Assign { name, value, .. } => {
                    self.mentioned.insert(name.clone());
                    self.expr(value);
                    self.assigned.push((name.clone(), value));
                }
                Stmt::Assert(value) | Stmt::Unlink(value) => self.expr(value),
                Stmt::If { condition, body } => {
                    self.expr(condition);
                    self.collect(body, handlers);
                }
                Stmt::Loop { over, body } => {
                    if let Some(LoopOver {
                        key,
                        value,
                        iterable,
                    }) = over
                    {
                        self.expr(iterable);
                        self.bind(value);
                        if let Some(key) = key {
                            self.bind(key);
                        }
                    }
                    self.collect(body, handlers);
                }
                Stmt::Continue | Stmt::Break | Stmt::TypeDef { .. } => {}
                Stmt::Link { alias, .. } => {
                    if let Some(alias) = alias {
                        self.bind(alias);
                    }
                }
                Stmt::Import { alias, body, .. } => {
                    if let Some(alias) = alias {
                        self.bind(alias);
                    }
                    self.collect(body, handlers);
                }
                Stmt::ImportNative { alias, .. } => self.bind(alias),
                Stmt::LinkRuntime { alias, path } => {
                    self.expr(path);
                    self.bind(alias);
                }
                Stmt::Emit {
                    particle,
                    target,
                    result,
                    ..
                } => {
                    self.expr(particle);
                    match target {
                        EmitTarget::Module(name) => self.bind(name),
                        EmitTarget::Core | EmitTarget::This | EmitTarget::Base => {}
                    }
                    match result {
                        Some(EmitResult::Whole(name)) => self.bind(name),
                        Some(EmitResult::Fields(fields)) => {
                            for field in fields {
                                self.bind(&field.name);
                            }
                        }
                        None => {}
                    }
                }
                Stmt::HandlerDef {
                    class_name,
                    fields,
                    body,
                } => {
                    let mut handler = Function::default();
                    for field in fields {
                        handler.bind(&field.name);
                    }
                    handler.collect(body, handlers);
                    handlers.push((class_name.clone(), handler));
                }
                Stmt::Return { value, .. } => self.expr(value),
            }
        }
    }

    fn bind(&mut self, name: &str) {
        self.bound.insert(name.to_string());
        self.mentioned.insert(name.to_string());
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Number(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null => {}
            Expr::Ident(name) => {
                self.mentioned.insert(name.clone());
            }
            Expr::Interpolated(parts) | Expr::Array(parts) => {
                for part in parts {
                    self.expr(part);
                }
            }
            Expr::Object(fields) => {
                for (key, value) in fields {
                    if let FieldKey::Computed(key) = key {
                        self.expr(key);
                    }
                    self.expr(value);
                }
            }
            Expr::Field(value, _)
            | Expr::LengthOf(value)
            | Expr::Unary(_, value)
            | Expr::Is(value, _) => self.expr(value),
            Expr::Index(value, index) => {
                self.expr(value);
                self.expr(index);
            }
            Expr::Slice { value, from, to } => {
                self.expr(value);
                self.expr(from);
                self.expr(to);
            }
            Expr::SetAt { base, path, value } => {
                self.expr(base);
                for key in path {
                    self.expr(key);
                }
                self.expr(value);
            }
            Expr::Binary(lhs, _, rhs) => {
                self.expr(lhs);
                self.expr(rhs);
            }
        }
    }

    /// The largest set of names whose every assignment is a number (then,
    /// separately, a boolean), leaving out `excluded` and whatever is bound
    /// another way.
    fn classify(&self, excluded: &HashSet<String>) -> HashMap<String, Kind> {
        let mut kinds = HashMap::new();
        for kind in [Kind::Number, Kind::Boolean] {
            let mut names: HashSet<&str> = self
                .assigned
                .iter()
                .map(|(name, _)| name.as_str())
                .filter(|name| !excluded.contains(*name) && !self.bound.contains(*name))
                .collect();
            loop {
                let before = names.len();
                for (name, value) in &self.assigned {
                    if names.contains(name.as_str()) && kind_of(value, &names, kind) != Some(kind) {
                        names.remove(name.as_str());
                    }
                }
                if names.len() == before {
                    break;
                }
            }
            for name in names {
                kinds.insert(name.to_string(), kind);
            }
        }
        kinds
    }
}

/// What `expr` always answers, given that every name in `names` holds a
/// `assumed` — `None` when it could be anything.
fn kind_of(expr: &Expr, names: &HashSet<&str>, assumed: Kind) -> Option<Kind> {
    match expr {
        Expr::Number(_) => Some(Kind::Number),
        Expr::Bool(_) => Some(Kind::Boolean),
        Expr::Ident(name) if names.contains(name.as_str()) => Some(assumed),
        Expr::Unary(UnOp::Neg, _) => Some(Kind::Number),
        Expr::Unary(UnOp::Not, _) | Expr::Is(..) => Some(Kind::Boolean),
        Expr::Binary(_, BinOp::Sub | BinOp::Mul | BinOp::Div, _) => Some(Kind::Number),
        Expr::Binary(lhs, BinOp::Add, rhs) => {
            let numbers = kind_of(lhs, names, assumed) == Some(Kind::Number)
                && kind_of(rhs, names, assumed) == Some(Kind::Number);
            numbers.then_some(Kind::Number)
        }
        Expr::Binary(
            _,
            BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::And
            | BinOp::Or,
            _,
        ) => Some(Kind::Boolean),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Kinds {
        let program = crate::parser::parse(&crate::lexer::tokenize(source).unwrap()).unwrap();
        of_program(&program)
    }

    #[test]
    fn a_counter_and_a_running_total_are_numbers() {
        let k = kinds(
            "n = 10\ni = 0\nt = 0\nloop\n    if i = n, break\n    t = t + i * i\n    i = i + 1\n",
        );
        assert_eq!(k.top.get("i"), Some(&Kind::Number));
        assert_eq!(k.top.get("t"), Some(&Kind::Number));
        assert_eq!(k.top.get("n"), Some(&Kind::Number));
    }

    #[test]
    fn a_variable_that_changes_kind_stays_boxed() {
        let k = kinds("x = 1\nx = \"a\"\ny = 1\ny = y + x\nz = 2 - x\n");
        assert_eq!(k.top.get("x"), None);
        assert_eq!(
            k.top.get("y"),
            None,
            "`+` with something unknown may join text"
        );
        assert_eq!(
            k.top.get("z"),
            Some(&Kind::Number),
            "`-` only ever answers a number"
        );
    }

    #[test]
    fn comparisons_make_booleans() {
        let k = kinds("a = 1\nb = a < 2\nc = b and not b\n");
        assert_eq!(k.top.get("b"), Some(&Kind::Boolean));
        assert_eq!(k.top.get("c"), Some(&Kind::Boolean));
    }

    #[test]
    fn a_top_level_name_a_handler_mentions_stays_boxed() {
        let k = kinds("count = 0\nlimit = 3\nTick =>\n    count = count + 1\n    local = count * 2\n    return Done\n");
        assert_eq!(k.top.get("count"), None);
        assert_eq!(k.top.get("limit"), Some(&Kind::Number));
        assert_eq!(k.handlers["Tick"].get("local"), Some(&Kind::Number));
        assert_eq!(k.handlers["Tick"].get("count"), None);
    }

    #[test]
    fn names_bound_another_way_stay_boxed() {
        let k = kinds("Step { dx } =>\n    dx2 = dx * 2\n    return Done\nloop v over [1, 2]\n    w = v - 1\n");
        assert_eq!(k.handlers["Step"].get("dx"), None);
        assert_eq!(k.handlers["Step"].get("dx2"), Some(&Kind::Number));
        assert_eq!(k.top.get("v"), None);
        assert_eq!(k.top.get("w"), Some(&Kind::Number));
    }
}

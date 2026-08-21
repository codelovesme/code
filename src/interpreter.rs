use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BinOp, Expr, Program, Stmt, UnOp};
use crate::value::Value;

/// A name -> Value binding table, scoped for `if` (see memory
/// `new-code-if-scoping`): a stack of maps, innermost last. Reading and
/// reassigning search from innermost to outermost, using the first match
/// found — a name already bound in some outer scope keeps being *that*
/// binding, wherever it's written from; a name not found anywhere becomes a
/// new binding in the current (innermost) scope. Rebinding a name to a
/// Value of a different variant is not an error — variables are untyped,
/// only Values are.
#[derive(Debug)]
pub struct Environment {
    scopes: Vec<HashMap<String, Value>>,
    /// First-assignment order of names in the *outermost* scope only — the
    /// only scope whose bindings ever get dumped (see `iter_in_order`); an
    /// `if`-local binding never appears here even if the `if` runs.
    order: Vec<String>,
}

impl Default for Environment {
    fn default() -> Self {
        Environment {
            scopes: vec![HashMap::new()],
            order: Vec::new(),
        }
    }
}

impl Environment {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn set(&mut self, name: String, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(&name) {
                *slot = value;
                return;
            }
        }
        if self.scopes.len() == 1 {
            self.order.push(name.clone());
        }
        self.scopes.last_mut().unwrap().insert(name, value);
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Bindings in first-assignment order, for stable, deterministic output
    /// — outermost scope only (see `order`'s doc comment).
    pub fn iter_in_order(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.order.iter().map(|name| {
            (
                name,
                self.scopes[0]
                    .get(name)
                    .expect("outermost binding must still exist"),
            )
        })
    }
}

pub fn run(program: &Program) -> Result<Environment, String> {
    let mut env = Environment::default();
    for stmt in &program.statements {
        exec(stmt, &mut env)?;
    }
    Ok(env)
}

fn exec(stmt: &Stmt, env: &mut Environment) -> Result<(), String> {
    match stmt {
        Stmt::Assign { name, value } => {
            let v = eval(value, env)?;
            env.set(name.clone(), v);
            Ok(())
        }
        Stmt::Assert(expr) => match eval(expr, env)? {
            Value::Bool(true) => Ok(()),
            Value::Bool(false) => Err("assertion failed".to_string()),
            v => Err(format!(
                "assert requires a boolean, found a {}",
                type_name(&v)
            )),
        },
        Stmt::If { condition, body } => match eval(condition, env)? {
            Value::Bool(true) => {
                env.push_scope();
                let result = body.iter().try_for_each(|s| exec(s, env));
                env.pop_scope();
                result
            }
            Value::Bool(false) => Ok(()),
            v => Err(format!("if requires a boolean, found a {}", type_name(&v))),
        },
        Stmt::Block(body) => {
            env.push_scope();
            let result = body.iter().try_for_each(|s| exec(s, env));
            env.pop_scope();
            result
        }
    }
}

fn eval(expr: &Expr, env: &Environment) -> Result<Value, String> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::Str(s) => Ok(Value::Str(Rc::from(s.as_str()))),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Null => Ok(Value::Null),
        // Cheap regardless of size: `Value::clone` on Str/Array/Object is
        // just an Rc refcount bump, not a deep copy (see value.rs).
        Expr::Ident(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined variable '{name}'")),
        Expr::Array(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(eval(item, env)?);
            }
            Ok(Value::Array(Rc::new(values)))
        }
        Expr::Object(fields) => {
            let mut values = Vec::with_capacity(fields.len());
            for (key, value) in fields {
                values.push((key.clone(), eval(value, env)?));
            }
            Ok(Value::Object(Rc::new(values)))
        }
        // Invalid access (non-object, missing field / non-array, bad index)
        // returns Null rather than erroring — decided 2026-08-21, permissive
        // like JS, unlike undefined-variable reads which still error.
        Expr::Field(obj, field) => {
            let v = eval(obj, env)?;
            Ok(match v {
                Value::Object(fields) => fields
                    .iter()
                    .find(|(k, _)| k == field)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            })
        }
        Expr::Index(arr, index) => {
            let v = eval(arr, env)?;
            let i = eval(index, env)?;
            Ok(match (v, i) {
                (Value::Array(items), Value::Number(n)) if n.fract() == 0.0 && n >= 0.0 => {
                    items.get(n as usize).cloned().unwrap_or(Value::Null)
                }
                _ => Value::Null,
            })
        }
        Expr::Unary(op, e) => {
            let v = eval(e, env)?;
            match (op, v) {
                (UnOp::Neg, Value::Number(n)) => Ok(Value::Number(-n)),
                (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                (UnOp::Neg, v) => Err(format!("cannot negate a {}", type_name(&v))),
                (UnOp::Not, v) => Err(format!(
                    "'not' requires a boolean, found a {}",
                    type_name(&v)
                )),
            }
        }
        // `and`/`or` short-circuit: the right side is only evaluated (and
        // only needs to be a bool) when the left side didn't already
        // determine the result.
        Expr::Binary(lhs, BinOp::And, rhs) => match eval(lhs, env)? {
            Value::Bool(false) => Ok(Value::Bool(false)),
            Value::Bool(true) => require_bool(eval(rhs, env)?, "and"),
            v => Err(format!(
                "'and' requires booleans, found a {}",
                type_name(&v)
            )),
        },
        Expr::Binary(lhs, BinOp::Or, rhs) => match eval(lhs, env)? {
            Value::Bool(true) => Ok(Value::Bool(true)),
            Value::Bool(false) => require_bool(eval(rhs, env)?, "or"),
            v => Err(format!("'or' requires booleans, found a {}", type_name(&v))),
        },
        // Equality is well-defined for any two values, including mismatched
        // kinds (simply `false`, never an error) — `Value`'s derived
        // `PartialEq` already does exactly that.
        Expr::Binary(lhs, BinOp::Eq, rhs) => Ok(Value::Bool(eval(lhs, env)? == eval(rhs, env)?)),
        Expr::Binary(lhs, BinOp::Ne, rhs) => Ok(Value::Bool(eval(lhs, env)? != eval(rhs, env)?)),
        Expr::Binary(lhs, op, rhs) => {
            let l = eval(lhs, env)?;
            let r = eval(rhs, env)?;
            apply_binop(*op, l, r)
        }
    }
}

fn require_bool(v: Value, op: &str) -> Result<Value, String> {
    match v {
        Value::Bool(_) => Ok(v),
        v => Err(format!(
            "'{op}' requires booleans, found a {}",
            type_name(&v)
        )),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Number(_) => "number",
        Value::Str(_) => "string",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `Add`/`Sub`/`Mul`/`Div`/`Lt`/`Gt`/`Le`/`Ge` — see `ast::BinOp`'s doc
/// comment for the exact operand-type rules this implements. `And`/`Or`/
/// `Eq`/`Ne` are handled directly in `eval` (short-circuiting / always-
/// defined, respectively) and never reach here.
fn apply_binop(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
    use Value::*;
    let result = match (op, &l, &r) {
        (BinOp::Add, Number(a), Number(b)) => Some(Number(a + b)),
        (BinOp::Add, Str(a), Str(b)) => Some(Str(Rc::from(format!("{a}{b}").as_str()))),
        (BinOp::Add, Array(a), Array(b)) => {
            let mut items = (**a).clone();
            items.extend(b.iter().cloned());
            Some(Array(Rc::new(items)))
        }
        (BinOp::Sub, Number(a), Number(b)) => Some(Number(a - b)),
        (BinOp::Mul, Number(a), Number(b)) => Some(Number(a * b)),
        (BinOp::Div, Number(a), Number(b)) => {
            if *b == 0.0 {
                return Err("division by zero".to_string());
            }
            Some(Number(a / b))
        }
        (BinOp::Lt, Number(a), Number(b)) => Some(Bool(a < b)),
        (BinOp::Gt, Number(a), Number(b)) => Some(Bool(a > b)),
        (BinOp::Le, Number(a), Number(b)) => Some(Bool(a <= b)),
        (BinOp::Ge, Number(a), Number(b)) => Some(Bool(a >= b)),
        (BinOp::Lt, Str(a), Str(b)) => Some(Bool(a < b)),
        (BinOp::Gt, Str(a), Str(b)) => Some(Bool(a > b)),
        (BinOp::Le, Str(a), Str(b)) => Some(Bool(a <= b)),
        (BinOp::Ge, Str(a), Str(b)) => Some(Bool(a >= b)),
        _ => None,
    };
    result.ok_or_else(|| {
        format!(
            "cannot apply {op:?} to a {} and a {}",
            type_name(&l),
            type_name(&r)
        )
    })
}

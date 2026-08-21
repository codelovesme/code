use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Expr, Program, Stmt};
use crate::value::Value;

/// A name -> Value binding table. Rebinding a name to a Value of a different
/// variant is not an error — variables are untyped, only Values are.
#[derive(Debug, Default)]
pub struct Environment {
    vars: HashMap<String, Value>,
    /// First-assignment order of each name, kept separately from `vars`
    /// since a `HashMap` has none — used only to make output deterministic.
    order: Vec<String>,
}

impl Environment {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    fn set(&mut self, name: String, value: Value) {
        if !self.vars.contains_key(&name) {
            self.order.push(name.clone());
        }
        self.vars.insert(name, value);
    }

    /// Bindings in first-assignment order, for stable, deterministic output.
    pub fn iter_in_order(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.order.iter().map(|name| (name, &self.vars[name]))
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
    }
}

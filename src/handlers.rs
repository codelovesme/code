//! The one rule handlers have beyond scoping: **the call graph must be
//! acyclic**.
//!
//! A handler may emit to another handler in the same program, but no handler
//! may re-enter one that is already running — not itself (`A => { emit A }`),
//! and not around a longer loop (`A -> B -> A`, `A -> B -> C -> A`).
//!
//! This is what keeps handler calls bounded: with no cycle, the deepest a
//! chain can go is the number of distinct handlers in the program, so the
//! stack cannot run away. Allowing recursion instead meant a program could
//! overflow — measured at roughly 4k frames in a compiled binary, where it
//! arrived as a bare SIGSEGV with no message. That is the same failure the
//! iterative rewrite of value traversal removed (see `runtime.c`'s
//! "Iterative traversal" section and `tests/stress_deep_nesting.code`);
//! forbidding cycles closes the door recursion reopened, rather than capping
//! depth at a number nobody could justify.
//!
//! Two halves, because dispatch is by the particle's *runtime* `_class`:
//!
//! - [`check_cycles`] runs before the program does, in both output modes,
//!   and rejects every cycle it can see — which is every `emit` whose
//!   particle is written literally at the call site.
//! - A **re-entry guard** in each backend catches the rest at runtime, where
//!   the particle came from a variable and no static pass could have known
//!   which handler it names. See `interpreter::dispatch_handler` and
//!   `codegen`'s per-handler `_code_active_*` flag.

use std::collections::{HashMap, HashSet};

use crate::ast::{EmitTarget, Expr, Program, Stmt};

/// Rejects a program whose handlers form a cycle, naming the whole path so
/// the fix is obvious: `handler cycle: A -> B -> A`.
pub fn check_cycles(program: &Program) -> Result<(), String> {
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    collect(&program.statements, &mut edges);

    // Depth-first, tracking the current path so a hit can be reported as the
    // route that produced it rather than just "somewhere there is a cycle".
    let mut done: HashSet<String> = HashSet::new();
    let mut path: Vec<String> = Vec::new();
    let mut names: Vec<&String> = edges.keys().collect();
    names.sort();
    for name in names {
        walk(name, &edges, &mut done, &mut path)?;
    }
    Ok(())
}

/// Every `emit <literal> to this` inside each handler body, as
/// `handler -> class` edges. Descends through `if`/block/loop bodies (a
/// cycle is a cycle wherever the emit sits) and through `Stmt::Import`, so a
/// linked module's handlers are part of the same graph they dispatch in.
fn collect(stmts: &[Stmt], edges: &mut HashMap<String, Vec<String>>) {
    for stmt in stmts {
        match stmt {
            Stmt::HandlerDef {
                class_name, body, ..
            } => {
                let mut targets = Vec::new();
                emits_in(body, &mut targets);
                edges.entry(class_name.clone()).or_default().extend(targets);
            }
            Stmt::Import { body, .. } => collect(body, edges),
            _ => {}
        }
    }
}

fn emits_in(stmts: &[Stmt], out: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            Stmt::Emit {
                particle,
                target: EmitTarget::This,
                ..
            } => {
                // `Foo { ... }` and a bare `Foo` both reach here as an object
                // whose `_class` is a string literal — that is the only shape
                // a static pass can resolve. A particle held in a variable
                // is left to the runtime guard.
                if let Some(name) = literal_class(particle) {
                    out.push(name);
                }
            }
            Stmt::If { body, .. } | Stmt::Block(body) | Stmt::Loop { body, .. } => {
                emits_in(body, out)
            }
            _ => {}
        }
    }
}

fn literal_class(particle: &Expr) -> Option<String> {
    match particle {
        Expr::Object(fields) => {
            fields
                .iter()
                .find_map(|(key, value)| match (key.as_str(), value) {
                    ("_class", Expr::Str(name)) => Some(name.clone()),
                    _ => None,
                })
        }
        _ => None,
    }
}

fn walk(
    name: &str,
    edges: &HashMap<String, Vec<String>>,
    done: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Result<(), String> {
    if let Some(at) = path.iter().position(|seen| seen == name) {
        let mut cycle: Vec<&str> = path[at..].iter().map(String::as_str).collect();
        cycle.push(name);
        return Err(format!(
            "handler cycle: {} — a handler cannot re-enter one that is already running",
            cycle.join(" -> ")
        ));
    }
    if done.contains(name) {
        return Ok(());
    }
    path.push(name.to_string());
    if let Some(targets) = edges.get(name) {
        for target in targets {
            walk(target, edges, done, path)?;
        }
    }
    path.pop();
    done.insert(name.to_string());
    Ok(())
}

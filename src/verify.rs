//! The one check both backends run before a program starts.
//!
//! `verify_defined` lived in `codegen.rs` until 2026-08-28, which made it a
//! *compile-time* check that `code run` had no counterpart for: the
//! interpreter found an undefined name only when it evaluated it, so
//! `code build` refused a program that `code run` would happily start and
//! then fail partway through, after any side effects it had already caused.
//!
//! Phase 4 made that gap load-bearing rather than merely untidy. Once a
//! runtime error inside a handler becomes an `Exception` value, an undefined
//! name in a handler body stops ending the interpreted program at all — so
//! `code build` would reject a program `code run` ran to completion, which is
//! the one divergence the two output modes are not allowed to have (the
//! fixture harness compares pass/fail, so it *would* have caught this, and
//! did).
//!
//! Nothing here touches inkwell, so it compiles in the wasm build too, where
//! `codegen` does not exist at all. Owner's call, 2026-08-28: an error before
//! the program starts is acceptable, and preferable to one halfway through.

use std::collections::HashSet;

use crate::ast::{EmitTarget, Expr, FieldKey, Program, Stmt};

/// Checks every `Expr::Ident` is reachable from an earlier assignment,
/// mirroring the interpreter's runtime "undefined variable" error as a
/// compile-time error instead (the language has no forward references or
/// hoisting, so this is a simple sequential scan) — scope-aware since `if`
/// bodies get their own scope (see memory `new-code-if-scoping`): a name
/// first assigned inside an `if` is only "defined" for the rest of that
/// `if`'s body, not after it, unless it was already defined outside.
///
/// Also refuses an `emit … to base` outside a linked module: the parser
/// cannot see the `link` (every file is parsed on its own), so the resolved
/// tree is the earliest place the question is answerable, and this is the
/// one check both backends share — see this file's header.
pub fn verify_defined(program: &Program) -> Result<(), String> {
    let mut scopes = vec![HashSet::new()];
    let mut natives = HashSet::new();
    verify_stmts(&program.statements, &mut scopes, &mut natives, 0)
}

fn verify_stmts(
    stmts: &[Stmt],
    scopes: &mut Vec<HashSet<String>>,
    natives: &mut HashSet<String>,
    depth: usize,
) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            Stmt::HandlerDef { fields, body, .. } => {
                let scope: HashSet<String> = fields.iter().cloned().collect();
                // Only the top level is visible, matching what the body will
                // actually close over — not whatever scopes happen to be open
                // where the definition sits (it is top-level only anyway).
                let enclosing = std::mem::replace(scopes, vec![scope]);
                scopes.insert(0, enclosing[0].clone());
                // A handler keeps the depth of the level it is defined at:
                // a `to base` inside its body means *its* parent, matching
                // `interpreter::HandlerBody::defining_depth`.
                let verified = verify_stmts(body, scopes, natives, depth);
                *scopes = enclosing;
                verified?;
            }
            Stmt::Return(value) => verify_expr(value, scopes)?,
            Stmt::Let { name, value, .. } => {
                verify_expr(value, scopes)?;
                // Always binds in the current scope, even if `name` is
                // already defined here or further out — shadowing.
                scopes.last_mut().unwrap().insert(name.clone());
            }
            Stmt::Link { path, .. } => {
                return Err(format!(
                    "internal error: link \"{path}\" reached codegen unresolved"
                ))
            }
            Stmt::Import {
                alias,
                body,
                exports,
            } => {
                scopes.push(HashSet::new());
                // One level further out in the module graph — where `to
                // base` becomes legal, and where its parent lives.
                let result = verify_stmts(body, scopes, natives, depth + 1);
                scopes.pop();
                result?;
                // The module's own scope is gone; only what it exported is
                // reachable from here.
                match alias {
                    Some(alias) => {
                        scopes.last_mut().unwrap().insert(alias.clone());
                    }
                    None => {
                        for name in exports {
                            scopes.last_mut().unwrap().insert(name.clone());
                        }
                    }
                }
            }
            Stmt::ImportNative { alias, .. } => {
                // **The name is the organelle.** Linking twice under one name
                // is refused, whichever files they are.
                //
                // Not a matter of taste. Until this check existed, a second
                // link silently shadowed the first: `link "math.so" as m`
                // followed by `link "jwt.so" as m` compiled, ran, and sent
                // `m`'s particles to whichever won — answering null for
                // every class the other one handled, with nothing said. And
                // linking the *same* file twice was worse, because it looked
                // like two organelles and was one: configuring the second
                // changed the first's settings underneath it.
                //
                // Both readings of a repeated name are mistakes, and neither
                // has a sensible meaning to preserve. Refusing before the
                // program starts is the whole of the fix for the first, and
                // half of it for the second — see `Stmt::ImportNative`'s doc
                // comment for the other half.
                if !natives.insert(alias.clone()) {
                    return Err(format!(
                        "'{alias}' is already linked — a name is one organelle, and linking \
                         another under the same name would silently replace it"
                    ));
                }
                // The alias serves two roles, so it is recorded in both
                // namespaces: in `natives` (a separate namespace from
                // `scopes`, matching `interpreter::Environment::native_modules`)
                // so `emit ... to <alias>` can dispatch to the module, and in
                // `scopes` so `alias.name` — the module's exported variables,
                // bound as an object — resolves as an ordinary field access.
                scopes.last_mut().unwrap().insert(alias.clone());
            }
            Stmt::Assign { name, value } => {
                verify_expr(value, scopes)?;
                if !is_defined(scopes, name) {
                    return Err(format!(
                        "undefined variable '{name}' (use 'let {name} = ...' to declare it)"
                    ));
                }
            }
            Stmt::Assert(expr) => verify_expr(expr, scopes)?,
            Stmt::If { condition, body } => {
                verify_expr(condition, scopes)?;
                scopes.push(HashSet::new());
                let result = verify_stmts(body, scopes, natives, depth);
                scopes.pop();
                result?
            }
            Stmt::Block(body) => {
                scopes.push(HashSet::new());
                let result = verify_stmts(body, scopes, natives, depth);
                scopes.pop();
                result?;
            }
            Stmt::Loop { over, result, body } => {
                // Both the iterable and the accumulator's initial value are
                // evaluated in the *enclosing* scope, before the loop
                // variables exist — so `loop x over x` correctly resolves
                // the right-hand `x` to an outer binding, or errors if there
                // isn't one.
                if let Some(over) = over {
                    verify_expr(&over.iterable, scopes)?;
                }
                if let Some(acc) = result {
                    verify_expr(&acc.init, scopes)?;
                    // Declared in the enclosing scope, matching where the
                    // binding actually lands (see `ast::LoopAccumulator`) —
                    // which is also what makes it defined *after* the loop.
                    scopes.last_mut().unwrap().insert(acc.name.clone());
                }
                let mut scope = HashSet::new();
                if let Some(over) = over {
                    scope.insert(over.value.clone());
                    if let Some(key) = &over.key {
                        scope.insert(key.clone());
                    }
                }
                scopes.push(scope);
                let verified = verify_stmts(body, scopes, natives, depth);
                scopes.pop();
                verified?;
            }
            Stmt::Emit {
                particle,
                target,
                result,
            } => {
                verify_expr(particle, scopes)?;
                if let EmitTarget::Module(alias) = target {
                    // A statically linked alias is the common case and keeps
                    // its check exactly as it was. Otherwise the name may be
                    // an ordinary binding holding an address from a `link`
                    // that ran inside a handler (`Stmt::LinkRuntime`) — which
                    // cannot be checked further here, since whether it really
                    // holds an address is only knowable at runtime.
                    //
                    // What survives is the check that actually catches
                    // mistakes: a name that is neither is still refused
                    // before the program starts, so a typo'd alias never
                    // becomes a runtime failure.
                    if !natives.contains(alias) && !is_defined(scopes, alias) {
                        return Err(format!(
                            "'emit ... to {alias}' but no native module is linked as '{alias}' \
                             and no variable of that name is in scope"
                        ));
                    }
                }
                if matches!(target, EmitTarget::Base) && depth == 0 {
                    return Err("'emit ... to base' outside a linked module — there is no \
                         parent to send it to"
                        .to_string());
                }
                if let Some(name) = result {
                    scopes.last_mut().unwrap().insert(name.clone());
                }
            }
            Stmt::LinkRuntime { alias, path } => {
                verify_expr(path, scopes)?;
                // An ordinary binding in the current scope, not a native
                // alias: what it holds is an address value, and `emit ... to
                // <alias>` reaches it through the variable lookup above, not
                // through `natives`. Registering it here would claim a
                // compile-time guarantee this form cannot make.
                scopes.last_mut().unwrap().insert(alias.clone());
            }
            Stmt::Unlink(address) => verify_expr(address, scopes)?,
            // Nothing to check — the parser already rejected any `break`
            // or `continue` that isn't inside a loop.
            Stmt::Break | Stmt::Continue => {}
        }
    }
    Ok(())
}

fn is_defined(scopes: &[HashSet<String>], name: &str) -> bool {
    scopes.iter().rev().any(|s| s.contains(name))
}

fn verify_expr(expr: &Expr, scopes: &[HashSet<String>]) -> Result<(), String> {
    match expr {
        Expr::Number(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null => Ok(()),
        Expr::Interpolated(parts) => parts.iter().try_for_each(|part| verify_expr(part, scopes)),
        Expr::Ident(name) => {
            if is_defined(scopes, name) {
                Ok(())
            } else {
                Err(format!("undefined variable '{name}'"))
            }
        }
        Expr::Array(items) => items.iter().try_for_each(|item| verify_expr(item, scopes)),
        // A computed key (`{ "$name" = v }`) reads a variable like anything
        // else, so it is checked like anything else — otherwise `code build`
        // would refuse a program `code run` accepted, or the reverse.
        Expr::Object(fields) => fields.iter().try_for_each(|(key, value)| {
            if let FieldKey::Computed(expr) = key {
                verify_expr(expr, scopes)?;
            }
            verify_expr(value, scopes)
        }),
        Expr::Field(obj, _) => verify_expr(obj, scopes),
        Expr::Index(arr, index) => {
            verify_expr(arr, scopes)?;
            verify_expr(index, scopes)
        }
        Expr::Unary(_, e) => verify_expr(e, scopes),
        Expr::Is(e, _) => verify_expr(e, scopes),
        Expr::Binary(lhs, _, rhs) => {
            verify_expr(lhs, scopes)?;
            verify_expr(rhs, scopes)
        }
    }
}

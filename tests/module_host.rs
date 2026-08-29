//! Exercises `Environment::provide_module` + `interpreter::run_with`
//! directly against a hand-built `Program` — the `NativeFormat::JsBridge`
//! path `crates/code-wasm` uses, standing a plain Rust closure in for what
//! would really be a JS callback. Built by hand rather than through
//! `loader::load` because producing a `JsBridge`-formatted `ImportNative` is
//! `crates/code-wasm`'s own resolver's job, not `loader.rs`'s (see
//! `ast::NativeFormat::JsBridge`'s doc comment) — the root crate has no
//! resolver that ever emits one.

use std::rc::Rc;

use code::ast::{EmitTarget, Expr, FieldKey, NativeFormat, Program, Stmt};
use code::interpreter::{self, Environment};
use code::value::Value;

fn link_stmt(path: &str, alias: &str) -> Stmt {
    Stmt::ImportNative {
        alias: alias.to_string(),
        path: path.to_string(),
        format: NativeFormat::JsBridge,
    }
}

fn doubler_module(env: &mut Environment, name: &str) {
    env.provide_module(
        name,
        Value::Object(Rc::new(vec![("answer".to_string(), Value::Number(42.0))])),
        Rc::new(|particle: &Value| {
            let Value::Object(fields) = particle else {
                return Ok(Value::Null);
            };
            match fields.iter().find(|(k, _)| k == "value") {
                Some((_, Value::Number(n))) => Ok(Value::Number(n * 2.0)),
                _ => Ok(Value::Null),
            }
        }),
    );
}

/// `Double { value = n }` — the emitted particle these tests send.
///
/// They emitted a bare `Expr::Number` until 2026-08-28, which was a shortcut
/// rather than a supported shape: the JsBridge contract is
/// `dispatch(particleJson)`, and since that date `emit` refuses a
/// non-particle *before* any target is chosen (see `check_emittable`), so the
/// shortcut stopped working. Sending a real particle is what a JS module
/// receives anyway.
fn double_particle(n: f64) -> Expr {
    Expr::Object(vec![
        (
            FieldKey::Literal("_class".to_string()),
            Expr::Str("Double".to_string()),
        ),
        (FieldKey::Literal("value".to_string()), Expr::Number(n)),
    ])
}

#[test]
fn dispatches_through_a_provided_closure() {
    let mut env = Environment::default();
    doubler_module(&mut env, "m");

    let program = Program {
        statements: vec![
            link_stmt("m", "m"),
            Stmt::Emit {
                particle: double_particle(21.0),
                target: EmitTarget::Module("m".to_string()),
                result: Some("n".to_string()),
            },
            Stmt::Assert(Expr::Binary(
                Box::new(Expr::Ident("n".to_string())),
                code::ast::BinOp::Eq,
                Box::new(Expr::Number(42.0)),
            )),
        ],
        // Hand-built: no source text, so no runtime error locations.
        ..Default::default()
    };

    let result = interpreter::run_with(&program, env).expect("program should run");
    assert_eq!(result.get("n"), Some(&Value::Number(42.0)));
    // The alias itself is bound to whatever `vars` `provide_module` was
    // given — ordinary field access, exactly like a `.so`'s exported
    // variables.
    assert_eq!(
        result.get("m"),
        Some(&Value::Object(Rc::new(vec![(
            "answer".to_string(),
            Value::Number(42.0)
        )])))
    );
}

#[test]
fn link_as_can_rename_a_provided_module() {
    // The host provides it under "m"; the script is free to bind it under a
    // different name entirely — `path`/`alias` are as decoupled here as a
    // `.so`'s file path and its `as` alias always are.
    let mut env = Environment::default();
    doubler_module(&mut env, "m");

    let program = Program {
        statements: vec![
            link_stmt("m", "renamed"),
            Stmt::Emit {
                particle: double_particle(10.0),
                target: EmitTarget::Module("renamed".to_string()),
                result: Some("n".to_string()),
            },
        ],
        // Hand-built: no source text, so no runtime error locations.
        ..Default::default()
    };

    let result = interpreter::run_with(&program, env).expect("program should run");
    assert_eq!(result.get("n"), Some(&Value::Number(20.0)));
    assert_eq!(result.get("m"), None, "the host's own name is never bound");
}

#[test]
fn a_jsbridge_link_with_no_provided_module_is_a_clear_error() {
    let env = Environment::default();
    let program = Program {
        statements: vec![link_stmt("missing", "missing")],
        ..Default::default()
    };

    let err = interpreter::run_with(&program, env).expect_err("should fail: never provided");
    assert!(
        err.contains("missing"),
        "error should name the alias, got: {err}"
    );
}

#[test]
fn emit_to_an_unlinked_alias_is_a_clear_error() {
    // No `link` statement at all — mirrors `EmitTarget::Module`'s own
    // "no linked module named" check, independent of the `ImportNative`
    // presence check above.
    let env = Environment::default();
    let program = Program {
        statements: vec![Stmt::Emit {
            // A real particle: since 2026-08-28 `emit` checks that before it
            // looks at the target at all, so a non-particle here would report
            // *that* and never reach the alias lookup this test is about.
            particle: double_particle(1.0),
            target: EmitTarget::Module("nope".to_string()),
            result: None,
        }],
        ..Default::default()
    };

    let err = interpreter::run_with(&program, env).expect_err("should fail: never linked");
    assert!(
        err.contains("nope"),
        "error should name the alias, got: {err}"
    );
}

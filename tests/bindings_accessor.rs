//! Regression test for `Interpreter::bindings()` (T19 foundation): the
//! read-only accessor a future WASM/playground host uses to observe a
//! program's final top-level bindings.

use code_lang::interpreter::Interpreter;
use code_lang::parser;
use code_lang::runtime::Value;
use chumsky::Parser;

/// Runs `body` (which does its own parse/execute/assert) on a thread with
/// enough stack for the parser combinator tree (main.rs works around the same
/// requirement for the CLI binary). `Interpreter` holds `Rc`, so it isn't
/// `Send` — the check has to run inside the spawned thread, not be returned
/// out of it.
fn on_big_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(body)
        .expect("failed to spawn test thread")
        .join()
        .expect("test thread panicked");
}

fn run(src: &str) -> Interpreter {
    let program = parser::parser().parse(src).expect("parse failed");
    let mut interp = Interpreter::new();
    interp.execute(program).expect("execution failed");
    interp
}

#[test]
fn bindings_reports_resolved_values() {
    on_big_stack(|| {
        let interp = run("a = 5\nb = \"hi\"\n");
        let bindings = interp.bindings();

        let a = bindings.iter().find(|(n, _)| n == "a").expect("missing a");
        match a.1.as_deref() {
            Some(Value::Number(n)) => assert_eq!(*n, 5.0),
            other => panic!("expected a = Number(5), got {:?}", other),
        }

        let b = bindings.iter().find(|(n, _)| n == "b").expect("missing b");
        match b.1.as_deref() {
            Some(Value::String(s)) => assert_eq!(s, "hi"),
            other => panic!("expected b = String(\"hi\"), got {:?}", other),
        }
    });
}

#[test]
fn bindings_reports_none_for_unresolved_domain() {
    on_big_stack(|| {
        // Range-constrained but never pinned to an exact value.
        let interp = run("a > 5\na < 12\n");
        let bindings = interp.bindings();
        let a = bindings.iter().find(|(n, _)| n == "a").expect("missing a");
        assert!(a.1.is_none(), "expected a's domain to be unresolved, got {:?}", a.1);
    });
}

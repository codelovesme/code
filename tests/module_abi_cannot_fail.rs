//! Nothing a module can call is allowed to fail.
//!
//! Since phase 3 of `docs/todo/errors-as-particles.md`, a runtime failure in
//! `runtime.c` sets `code_failed` and returns normally; the *host's* generated
//! code checks that flag after every fallible call. A `.so` module carries its
//! own copy of that runtime, so a failure raised inside a module sets the
//! module's flag — which nobody reads. It does not surface as an error, a
//! crash, or a wrong answer. It does not surface at all.
//!
//! That makes a fallible entry in `code_abi.h` a hole rather than a sharp
//! edge, and holes do not stay documented. Four functions were removed for
//! this reason (`code_bool_value`, `code_assert`, `code_field`, `code_index`),
//! all of them found by reading; this is what stops the fifth being found the
//! same way, or not at all.
//!
//! Structural rather than a list of names on purpose: a new function added to
//! the header is checked automatically, which is exactly the case a list would
//! miss.

use std::collections::BTreeSet;

const HEADER: &str = include_str!("../src/code_abi.h");
const RUNTIME: &str = include_str!("../src/runtime.c");

/// Every `code_*` function the header declares.
///
/// Declarations only: `code_slot_at` is a `static inline` *definition* in the
/// header, pure pointer arithmetic over no state, and has no body in
/// `runtime.c` to inspect. It cannot fail and is skipped by construction,
/// since this only collects names that end in `;`.
fn declared_in_header() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in HEADER.lines() {
        let line = line.trim();
        if line.starts_with('*') || line.starts_with("/*") || !line.contains("code_") {
            continue;
        }
        let Some(open) = line.find('(') else { continue };
        // A declaration ends in `;` on this line or a later one; either way
        // the name sits immediately before the `(`.
        let before = &line[..open];
        let name: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if name.starts_with("code_") && name != "code_slot_at" {
            names.insert(name);
        }
    }
    assert!(
        names.len() > 10,
        "the header parse found only {names:?}; it has stopped matching the \
         declaration shape and would pass vacuously"
    );
    names
}

/// The body of `name` in `runtime.c`, from its definition line to the closing
/// brace in column zero.
fn body_of(name: &str) -> Option<String> {
    let mut lines = RUNTIME.lines();
    let opener = format!("{name}(");
    let start = lines.by_ref().find(|line| {
        !line.starts_with(' ')
            && !line.starts_with('*')
            && line.contains(&opener)
            && line.trim_end().ends_with('{')
    })?;
    let mut body = String::from(start);
    for line in lines {
        body.push('\n');
        body.push_str(line);
        if line == "}" {
            break;
        }
    }
    Some(body)
}

/// Whether a body reaches the failure channel *directly*. Not transitively:
/// no function in the header calls one that fails, which the assertion below
/// states as the property being kept rather than something being computed.
fn fails(body: &str) -> bool {
    body.lines().any(|line| {
        let line = line.trim();
        if line.starts_with("/*") || line.starts_with('*') || line.starts_with("//") {
            return false;
        }
        line.contains("fail(") || line.contains("fail_operand(") || line.contains("fail_binary(")
    })
}

#[test]
fn no_function_in_the_module_abi_can_fail() {
    let mut offenders = Vec::new();
    for name in declared_in_header() {
        // `code_runtime_error` is declared and *does* end the program, which
        // is the one thing a module may never do — but it is deprecated out
        // of `code-native`'s API and kept in the header only so an existing C
        // module still links. It is not the flag hazard this guards.
        if name == "code_runtime_error" {
            continue;
        }
        let Some(body) = body_of(&name) else {
            continue;
        };
        if fails(&body) {
            offenders.push(name);
        }
    }
    assert!(
        offenders.is_empty(),
        "these are declared in code_abi.h and reach runtime.c's failure \
         channel: {offenders:?}.\n\nA module's copy of the runtime is not the \
         one the host checks, so a failure raised inside a module is swallowed \
         in silence. Either make the function total for modules and keep the \
         fallible one out of the header (as `code_field`/`code_index` were \
         split), or leave it out of the header entirely (as `code_bool_value` \
         and `code_assert` were)."
    );
}

/// The guard's own guard: `body_of` has to actually find bodies, or the test
/// above passes by looking at nothing.
#[test]
fn the_header_functions_are_actually_being_inspected() {
    let declared = declared_in_header();
    let found = declared.iter().filter(|n| body_of(n).is_some()).count();
    assert!(
        found > declared.len() / 2,
        "only {found} of {} declared functions were located in runtime.c; the \
         body scan has stopped matching definitions",
        declared.len()
    );
}

/// And proof the scan can see a failure when there is one: `code_assert` is
/// exactly the shape that used to be in the header, and still fails.
#[test]
fn the_scan_detects_a_function_that_does_fail() {
    let body = body_of("code_assert").expect("code_assert is defined in runtime.c");
    assert!(
        fails(&body),
        "code_assert reaches `fail`, so if this cannot see it the check above \
         proves nothing"
    );
    assert!(
        !declared_in_header().contains("code_assert"),
        "code_assert is back in the module ABI"
    );
}

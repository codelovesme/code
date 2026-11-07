//! The in-place runtime shortcut must match generic addition even for IEEE
//! values whose sign or NaN result cannot be distinguished by Code equality.
#![cfg(feature = "llvm")]

use std::{fs, path::Path, process::Command};

#[test]
fn numeric_accumulation_matches_generic_ieee_addition() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("code-numeric-add-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("probe.c");
    fs::write(
        &source,
        r#"
#include "code_abi.h"
#include <assert.h>
#include <float.h>
#include <math.h>
#include <stdint.h>
#include <string.h>
void code_add(CodeValue *, const CodeValue *, const CodeValue *);
void code_add_into(CodeValue *, const CodeValue *);
void code_add_take(CodeValue *, CodeValue *);
void code_check_leaks(void);

static void same(const CodeValue *actual, const CodeValue *expected) {
    assert(actual->tag == CODE_NUMBER && actual->heap == 0);
    assert(expected->tag == CODE_NUMBER && expected->heap == 0);
    if (isnan(expected->number)) {
        assert(isnan(actual->number));
    } else {
        uint64_t a, b;
        memcpy(&a, &actual->number, sizeof a);
        memcpy(&b, &expected->number, sizeof b);
        assert(a == b); /* includes the sign of zero and infinities */
    }
}
int main(void) {
    double values[] = {0.0, -0.0, 1.0, -1.0, DBL_MIN, -DBL_MIN,
                       DBL_TRUE_MIN, -DBL_TRUE_MIN, DBL_MAX, -DBL_MAX,
                       INFINITY, -INFINITY, NAN, -NAN, 1.0 / 3.0, 1e16};
    for (unsigned i = 0; i < sizeof values / sizeof *values; ++i) {
        CodeValue a = {0}, expected = {0}, actual = {0};
        code_number(&a, values[i]);
        code_add(&expected, &a, &a);
        code_number(&actual, values[i]);
        code_add_into(&actual, &actual); /* identical slot on both sides */
        same(&actual, &expected);
        for (unsigned j = 0; j < sizeof values / sizeof *values; ++j) {
            CodeValue b = {0}, saved = {0};
            code_number(&b, values[j]);
            saved = b;
            code_add(&expected, &a, &b);
            code_number(&actual, values[i]);
            code_add_into(&actual, &b);
            same(&actual, &expected);
            assert(memcmp(&b, &saved, sizeof b) == 0); /* RHS unchanged */
            code_number(&actual, values[i]);
            code_add_take(&actual, &b); /* consumed arithmetic scratch */
            same(&actual, &expected);
            assert(b.tag == CODE_NUMBER && b.heap == 0 && b.number == 0);
        }
    }
    code_check_leaks();
    return 0;
}
"#,
    )
    .unwrap();
    for opt in ["-O0", "-O2"] {
        let binary = dir.join(if opt == "-O0" { "dev" } else { "release" });
        let output = Command::new("cc")
            .args(["-std=c11", opt, "-fPIE", "-pthread", "-I"])
            .arg(root.join("src"))
            .arg(&source)
            .arg(root.join("src/runtime.c"))
            .args(["-ldl", "-lm", "-o"])
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&binary)
            .env("CODE_CHECK_LEAKS", "1")
            .env("MALLOC_PERTURB_", "165")
            .env("GLIBC_TUNABLES", "glibc.malloc.tcache_count=0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
    fs::remove_dir_all(dir).unwrap();
}

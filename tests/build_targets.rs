//! `code build --target ...` — phase 1 of `docs/todo/build-targets.md`.
//!
//! The fixture suite (`run_language_tests.rs`) covers *behaviour* under the
//! default `Exe` target; this file covers *container shape*: each target
//! produces the artifact its flag promises, and the artifact is usable —
//! a `.so` the dynamic loader actually accepts, a `.a` holding exactly the
//! program object, and a wasm module that Node can instantiate and run.

#![cfg(feature = "llvm")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

/// The four functions a wasm build expects from whatever runs it. Two are the
/// clock and the error sink; the other two are how a number becomes text —
/// the exact expansion of a double and reading one back, which a freestanding
/// build cannot compute for itself and JavaScript answers in one call each
/// (see `src/wasm_shim.h`). Written once here because both wasm tests need
/// the same host.
const WASM_HOST_JS: &str = "\
  const dec = new TextDecoder(), enc = new TextEncoder();\n\
  let memory;\n\
  const env = {\n\
    code_host_error(ptr, len) {\n\
      throw new Error('wasm error: ' + dec.decode(new Uint8Array(memory.buffer, ptr, len)));\n\
    },\n\
    code_host_now() { return Date.now() / 1000; },\n\
    code_host_number_exact(value, ptr, cap) {\n\
      const b = enc.encode(value.toExponential(40));\n\
      if (b.length >= cap) return -1;\n\
      new Uint8Array(memory.buffer).set(b, ptr);\n\
      return b.length;\n\
    },\n\
    code_host_number_parse(ptr, len) {\n\
      return Number(dec.decode(new Uint8Array(memory.buffer, ptr, len)));\n\
    },\n\
  };\n";

/// One private directory per test; the tag distinguishes tests within this
/// process, the pid distinguishes processes (the same convention as
/// `concurrent_builds.rs`).
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-build-target-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

#[test]
fn shared_target_produces_a_loadable_library() {
    let dir = temp_dir("shared");
    let out = dir.join("libarith.so");
    code::compile_file(
        &fixture("arithmetic_basic.code"),
        code::BuildTarget::Shared,
        &out,
        false,
    )
    .expect("build --target shared");
    assert!(out.is_file(), "expected a .so at {}", out.display());

    // A `.so` whose only entry point is `main` is close to useless — the
    // real acceptance bar is that the dynamic loader accepts it: every
    // symbol resolved, any initializers run. `ctypes.CDLL` does exactly
    // that with nothing beyond the stdlib; where python3 is unavailable
    // the existence check above still stands.
    let probe = Command::new("python3")
        .arg("-c")
        .arg(format!("import ctypes; ctypes.CDLL({out:?})"))
        .output();
    match probe {
        Ok(output) if output.status.success() => {}
        Ok(output) => panic!(
            "built .so failed to load: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(e) => eprintln!("note: python3 unavailable ({e}); skipped load check"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn static_target_produces_an_archive_holding_only_the_program_object() {
    let dir = temp_dir("static");
    let out = dir.join("libarith.a");
    code::compile_file(
        &fixture("arithmetic_basic.code"),
        code::BuildTarget::Static,
        &out,
        false,
    )
    .expect("build --target static");

    // `ar t` lists members. Exactly one is expected: the program object.
    // No runtime — `Static` deliberately never links one in (consumers of
    // the archive supply their own), so a second member would mean the
    // link step leaked into the archive step.
    let listing = Command::new("ar")
        .arg("t")
        .arg(&out)
        .output()
        .expect("run ar t");
    assert!(listing.status.success(), "ar t failed on {}", out.display());
    let members = String::from_utf8(listing.stdout).expect("ar t output is utf-8");
    assert_eq!(
        members.lines().collect::<Vec<_>>(),
        vec!["program.o"],
        "unexpected archive contents: {members}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn wasm_target_produces_a_runnable_module() {
    let dir = temp_dir("wasm");
    let out = dir.join("arith.wasm");
    code::compile_file(
        &fixture("arithmetic_basic.code"),
        code::BuildTarget::Wasm,
        &out,
        false,
    )
    .expect("build --target wasm");
    assert!(out.is_file(), "expected a wasm file at {}", out.display());

    run_wasm_under_node(&dir, &out);
    let _ = fs::remove_dir_all(&dir);
}

/// Rendering a number as text has to spell it the same way whichever mode
/// produced the program — the interpreter, a native binary, or a wasm module.
/// The interpreter and the native binary are held to that by the fixture
/// harness; wasm is held to it here, by building the *same* fixture and
/// running it. Its asserts are the check: a disagreement ends the program,
/// which reaches the host as an error rather than a clean exit.
///
/// This was the one place a feature did not behave identically everywhere.
/// Until 2026-08-29 a fractional number had no spelling on wasm at all and
/// interpolating one was a loud error, because the freestanding build has no
/// float formatting; the host supplies the two missing pieces now.
#[test]
fn wasm_spells_numbers_the_way_the_other_modes_do() {
    let dir = temp_dir("wasm-numbers");
    let out = dir.join("numbers.wasm");
    code::compile_file(
        &fixture("interp_number_text.code"),
        code::BuildTarget::Wasm,
        &out,
        false,
    )
    .expect("build interp_number_text.code --target wasm");

    run_wasm_under_node(&dir, &out);
    let _ = fs::remove_dir_all(&dir);
}

/// A page fires back, and the program's own handler answers it.
///
/// Four things are held here: the value the page sent arrives as a `value`
/// field, an event that carried nothing leaves the field off altogether, a
/// class nobody handles is silence rather than an error, and — the one that
/// took longest to see — all of this happens *after* `main` has returned. On
/// a page that is not the program ending, so a wasm build skips the
/// end-of-run sweep; with the sweep in place the first click found a program
/// whose state had already been freed.
///
/// The handlers report by making a fractional number into text, which a
/// freestanding build cannot do for itself and asks the host to. Counting
/// those calls is what tells a wrong value from a right one: an assert would
/// not do, because a failing assert inside a handler is an `Exception` handed
/// back to whoever emitted, not the end of the program.
#[test]
fn a_page_fires_events_back_into_the_program() {
    let dir = temp_dir("wasm-events");
    let out = dir.join("events.wasm");
    code::compile_file(
        &fixture("wasm_events.code"),
        code::BuildTarget::Wasm,
        &out,
        false,
    )
    .expect("build wasm_events.code --target wasm");

    let probe = dir.join("fire.mjs");
    fs::write(
        &probe,
        format!(
            "import {{ readFileSync }} from 'node:fs';\n\
             {WASM_HOST_JS}\
             let answered = 0;\n\
             const spelled = env.code_host_number_exact;\n\
             env.code_host_number_exact = (v, p, c) => {{ answered++; return spelled(v, p, c); }};\n\
             const {{ instance }} = await WebAssembly.instantiate(readFileSync({out:?}), {{ env }});\n\
             memory = instance.exports.memory;\n\
             if (instance.exports.main() !== 0) throw new Error('main returned an error');\n\
             const e = instance.exports;\n\
             const classAt = e.code_event_class(), classCap = Number(e.code_event_class_capacity());\n\
             const textAt = e.code_event_text(), textCap = Number(e.code_event_text_capacity());\n\
             const write = (s, at, cap) => {{\n\
               const b = enc.encode(s);\n\
               if (b.length >= cap) throw new Error('event string does not fit');\n\
               new Uint8Array(memory.buffer).set(b, at);\n\
               return BigInt(b.length);\n\
             }};\n\
             const fire = (cls, value) => {{\n\
               const before = answered;\n\
               const n = write(cls, classAt, classCap);\n\
               const v = value === null ? -1n : write(value, textAt, textCap);\n\
               e.code_event_fire(n, v);\n\
               return answered - before;\n\
             }};\n\
             const check = (what, got, want) => {{\n\
               if (got !== want) throw new Error(what + ': answered ' + got + ', wanted ' + want);\n\
             }};\n\
             check('the value the page sent did not reach the handler', fire('Clicked', 'merhaba'), 1);\n\
             check('a different value was treated as the right one', fire('Clicked', 'baska'), 0);\n\
             check('an event carrying nothing still arrived with a value', fire('Bare', null), 1);\n\
             check('an event carrying nothing was not recognised', fire('Bare', ''), 0);\n\
             check('a class nobody handles was not silent', fire('Nobody', 'x'), 0);\n"
        ),
    )
    .expect("write event probe");

    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run wasm under node");
    assert!(
        output.status.success(),
        "firing events into the module failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Instantiate `module` under Node with the host above and run its `main`,
/// failing the test with whatever Node reported if it does not come back 0.
fn run_wasm_under_node(dir: &Path, module: &Path) {
    let probe = dir.join("run-wasm.mjs");
    fs::write(
        &probe,
        format!(
            "import {{ readFileSync }} from 'node:fs';\n\
             {WASM_HOST_JS}\
             const {{ instance }} = await WebAssembly.instantiate(readFileSync({module:?}), {{ env }});\n\
             memory = instance.exports.memory;\n\
             if (typeof instance.exports.main !== 'function') throw new Error('main was not exported');\n\
             if (instance.exports.main() !== 0) throw new Error('main returned an error');\n"
        ),
    )
    .expect("write wasm probe");
    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run wasm under node");
    assert!(
        output.status.success(),
        "node could not run wasm: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_default_output_name_follows_the_target() {
    let dir = temp_dir("cli");
    // Copy the fixture in rather than pointing at tests/: the default output
    // path derives from the input, and building would leave a `build/`
    // directory among the fixtures.
    let src = dir.join("arith.code");
    fs::copy(fixture("arithmetic_basic.code"), &src).expect("copy fixture");

    // File first, flags after — the same shape as `-o` (see `main.rs`).
    let status = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg(&src)
        .args(["--target", "shared"])
        .current_dir(&dir)
        .status()
        .expect("spawn code build");
    assert!(status.success(), "code build --target shared failed");
    assert!(
        dir.join("build/libarith.so").is_file(),
        "expected build/libarith.so beside the source in {}",
        dir.display()
    );

    // And the `build/` is beside the *source*, not in the working directory —
    // the two are the same above, which is why this second case exists: it is
    // the only shape that tells them apart.
    let nested = dir.join("src");
    fs::create_dir_all(&nested).expect("create src/");
    let nested_src = nested.join("deep.code");
    fs::copy(fixture("arithmetic_basic.code"), &nested_src).expect("copy fixture");
    let status = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg("src/deep.code")
        .current_dir(&dir)
        .status()
        .expect("spawn code build");
    assert!(status.success(), "code build src/deep.code failed");
    assert!(
        nested.join("build/deep").is_file(),
        "expected src/build/deep beside its source in {}",
        dir.display()
    );
    assert!(
        !dir.join("build/deep").exists(),
        "the artifact landed in the working directory's build/ rather than beside the source"
    );

    // Every flag has a short form: `-t`, `-r`, `-o`. Checked together, since
    // one argument loop parses them and a rewrite would drop them together.
    let status = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg("src/deep.code")
        .args(["-t", "static", "-r", "-o", "short.a"])
        .current_dir(&dir)
        .status()
        .expect("spawn code build");
    assert!(status.success(), "short flags were not accepted");
    assert!(dir.join("short.a").is_file());

    // An unparseable target value is a usage error, reported as such.
    let output = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg(&src)
        .args(["--target", "bogus"])
        .current_dir(&dir)
        .output()
        .expect("spawn code build");
    assert!(!output.status.success(), "bogus target must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown target 'bogus'"),
        "usage error should name the bad value, got: {stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `--release` is the only knob in `code build` that changes nothing about
/// what a program *means*, which makes it the easy one to break silently:
/// drop the plumbing anywhere between the CLI flag and
/// `create_target_machine` and every test still passes. So this asserts the
/// two settings actually produce different output — and that the optimized
/// one still runs, since `-O2` is the path no other test in the suite
/// exercises now that the default is `-O0`.
#[test]
fn release_optimizes_and_the_default_does_not() {
    let dir = temp_dir("release");
    let source = fixture("object_merge.code");

    // Compared at the *object* level, and by content rather than length.
    // Both of those are load-bearing. An earlier version of this test
    // compared the sizes of two linked executables and passed locally while
    // failing on CI, where the linker's section padding happened to round
    // both to the same length — the objects underneath differed all along.
    // No linker sits between `compile_to_object` and the flag, so nothing
    // can absorb the difference here.
    let program = {
        let text = fs::read_to_string(&source).expect("read fixture");
        let lexed = code::lexer::tokenize(&text).expect("tokenize fixture");
        code::parser::parse(&lexed).expect("parse fixture")
    };
    let mut objects = Vec::new();
    for (name, release) in [("dev.o", false), ("release.o", true)] {
        let path = dir.join(name);
        code::codegen::compile_to_object(&program, code::BuildTarget::Exe, &path, release)
            .expect("compile to object");
        objects.push(fs::read(&path).expect("read object"));
    }
    assert_ne!(
        objects[0],
        objects[1],
        "-O0 and -O2 produced byte-identical objects ({} bytes) — \
         the flag is not reaching the target machine",
        objects[0].len()
    );

    // Then end to end, where the optimizer has to not have changed what the
    // program means. The fixture is a wall of asserts, so a non-zero exit is
    // the program itself reporting that it did.
    let release = dir.join("release");
    code::compile_file(&source, code::BuildTarget::Exe, &release, true).expect("build --release");
    let status = Command::new(&release).status().expect("run release build");
    assert!(status.success(), "release build failed its own assertions");

    // And the CLI half: the flag is spelled the way the usage string
    // promises, and does not fall through to `unknown argument`.
    let output = Command::new(env!("CARGO_BIN_EXE_code"))
        .arg("build")
        .arg(&source)
        .arg("--release")
        .args(["-o", &dir.join("cli").display().to_string()])
        .output()
        .expect("spawn code build --release");
    assert!(
        output.status.success(),
        "code build --release failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

/// An application and its modules in **one** `.wasm`, with nothing left to
/// load.
///
/// This is what a `.a` is for on this target. A `.so` is opened while the
/// program runs and wasm has no way to do that; a `.a` is linked in before
/// there is a `.wasm` at all, so the module's code ends up inside the same
/// module as the application and the runtime. The alternative — several wasm
/// modules instantiated separately and wired together from JavaScript — is
/// the host's business and not a `link`.
///
/// The module here is C rather than Rust on purpose. A Rust `staticlib` for
/// wasm32 brings its own standard library and allocator into a link that is
/// deliberately freestanding, which is a real question and not this one.
/// This test asks only whether the static-module contract survives the change
/// of target.
///
/// Skipped where the wasm toolchain is not installed. `clang` targeting
/// wasm32 is needed to compile the module, and a symbol reader that
/// understands wasm objects (`llvm-nm`) to discover its prefix — the system
/// `nm` reads a native `.a` and not this one.
#[test]
fn a_wasm_build_links_a_static_module_into_the_same_module() {
    if !tool_exists("clang") {
        eprintln!("skipped: needs clang to build a wasm .a");
        return;
    }

    let dir = temp_dir("wasm-static");
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/native_modules/test_wasm_static/test_wasm_static.c");
    let obj = dir.join("module.o");
    let archive = dir.join("test_wasm_static.a");

    let compiled = Command::new("clang")
        .args([
            "--target=wasm32-unknown-unknown",
            "-nostdlib",
            "-fno-builtin",
        ])
        .arg("-I")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .arg("-c")
        .arg(&src)
        .arg("-o")
        .arg(&obj)
        .output()
        .expect("run clang for the wasm module");
    assert!(
        compiled.status.success(),
        "clang could not build the wasm module: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    // GNU `ar` writes the archive fine — it is only *reading* a wasm member
    // that it cannot do, which is the loader's problem and not this step's.
    let archived = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&obj)
        .status()
        .expect("run ar");
    assert!(archived.success(), "ar failed");

    // The program links it by path and emits to it like any other module.
    let program = dir.join("app.code");
    fs::write(
        &program,
        format!(
            "link {:?} as m\n\nemit Double {{ value = 21 }} to m get r\nassert r.value = 42\n",
            archive.to_string_lossy()
        ),
    )
    .expect("write program");

    let out = dir.join("app.wasm");
    code::compile_file(&program, code::BuildTarget::Wasm, &out, false)
        .expect("build a wasm program that links a .a module");
    assert!(out.is_file(), "expected {}", out.display());

    // One file, and the module's answer inside it: the `assert` above is the
    // check, and it ends the program — which reaches the host as an error
    // rather than a clean exit — if the module did not multiply.
    run_wasm_under_node(&dir, &out);

    let _ = fs::remove_dir_all(&dir);
}

/// Two archives naming their exports the same way are refused by name.
///
/// The linker used to be the one to notice, with `duplicate symbol`. A wasm
/// link no longer lets it: Rust puts a panic handler in every archive it
/// produces, so two Rust modules always collide there and duplicates had to
/// be allowed for a web application to link at all. That is fine for a panic
/// handler and not fine for a module's exports — one would quietly replace
/// the other — so the case that actually matters is checked by name instead,
/// before anything is linked.
#[test]
fn two_static_modules_sharing_a_prefix_are_refused() {
    if !tool_exists("clang") {
        eprintln!("skipped: needs clang to build a wasm .a");
        return;
    }

    let dir = temp_dir("wasm-static-clash");
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/native_modules/test_wasm_static/test_wasm_static.c");
    let obj = dir.join("module.o");
    let compiled = Command::new("clang")
        .args([
            "--target=wasm32-unknown-unknown",
            "-nostdlib",
            "-fno-builtin",
        ])
        .arg("-I")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .arg("-c")
        .arg(&src)
        .arg("-o")
        .arg(&obj)
        .output()
        .expect("run clang for the wasm module");
    assert!(
        compiled.status.success(),
        "clang could not build the wasm module: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    // The same object in two archives: different files, one prefix — which
    // is exactly the shape a second module built from a copied template
    // arrives in.
    let mut archives = Vec::new();
    for name in ["first.a", "second.a"] {
        let archive = dir.join(name);
        let archived = Command::new("ar")
            .arg("rcs")
            .arg(&archive)
            .arg(&obj)
            .status()
            .expect("run ar");
        assert!(archived.success(), "ar failed");
        archives.push(archive);
    }

    let program = dir.join("app.code");
    fs::write(
        &program,
        format!(
            "link {:?} as a\nlink {:?} as b\n\nemit Double {{ value = 21 }} to a get r\n",
            archives[0].to_string_lossy(),
            archives[1].to_string_lossy()
        ),
    )
    .expect("write program");

    let out = dir.join("app.wasm");
    let error = code::compile_file(&program, code::BuildTarget::Wasm, &out, false)
        .expect_err("two archives sharing a prefix must be refused");
    assert!(
        error.contains("wasmmath_code_module_*")
            && error.contains("first.a")
            && error.contains("second.a"),
        "the refusal should name the prefix and both archives, got: {error}"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// A `.so` still cannot be linked into wasm, and the refusal says why rather
/// than repeating the old blanket "supply modules from the host".
#[test]
fn a_wasm_build_still_refuses_a_shared_module() {
    let dir = temp_dir("wasm-so");
    let program = dir.join("app.code");
    fs::write(&program, "link \"whatever.so\" as m\n").expect("write program");
    let err = code::compile_file(
        &program,
        code::BuildTarget::Wasm,
        &dir.join("app.wasm"),
        false,
    )
    .expect_err("a .so link must be refused for wasm");
    assert!(err.contains("wasm"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

fn tool_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// `Path` is needed by `run` too now that it resolves modules; only `build`'s
// output path is LLVM-gated.
use std::path::Path;
#[cfg(feature = "llvm")]
use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(feature = "llvm")]
use code::codegen::BuildTarget;

/// Printed by `--version` / `-v`. The release workflow rewrites the package
/// version from the git tag before building (release.yml), so a tagged build
/// reports the release version here.
const VERSION: &str = concat!("Code v", env!("CARGO_PKG_VERSION"));

/// `run` interprets; `build` compiles (LLVM, see codegen.rs) and links a
/// standalone executable via the system `cc`. Both are meant to run every
/// language feature identically (see memory `new-language-rewrite`).
fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let command = argv.first().cloned().unwrap_or_default();
    let rest: Vec<String> = argv.into_iter().skip(1).collect();
    // `-h`/`--help` anywhere after the command asks about *that* command, so
    // it is answered before the command's own arguments are parsed — a help
    // flag should never have to be in the right position, and asking for help
    // is never an error (stdout, exit 0).
    if rest.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", help_for(&command));
        return ExitCode::SUCCESS;
    }
    let mut args = rest.into_iter();
    match command.as_str() {
        "run" => {
            // No path means the directory you are standing in, which is a
            // project — the same default `build` takes.
            let path = args.next().unwrap_or_else(|| ".".to_string());
            if let Some(unknown) = args.next() {
                eprintln!("code run takes one path, not '{unknown}'");
                return ExitCode::FAILURE;
            }
            match entry_point(&path) {
                Ok(entry) => run_file(&entry),
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::FAILURE
                }
            }
        }
        #[cfg(feature = "llvm")]
        "build" => {
            // The path is optional and defaults to `.`, like `run`'s. A flag
            // in first position is not a path.
            let mut args = args.peekable();
            let path = match args.peek() {
                Some(first) if !first.starts_with('-') => args.next().unwrap_or_default(),
                _ => ".".to_string(),
            };
            let mut out: Option<PathBuf> = None;
            let mut target = BuildTarget::Exe;
            let mut release = false;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    // `--output` too: the long form is what a reader
                    // expects beside `--target` and `--release`.
                    "-o" | "--output" => out = args.next().map(PathBuf::from),
                    "-r" | "--release" => release = true,
                    "-t" | "--target" => {
                        let Some(value) = args.next() else {
                            eprintln!("--target takes a value (exe|shared|static|wasm)");
                            return ExitCode::FAILURE;
                        };
                        match BuildTarget::parse(&value) {
                            Some(t) => target = t,
                            None => {
                                eprintln!(
                                    "unknown target '{value}' (expected exe|shared|static|wasm)"
                                );
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    other => {
                        eprintln!("unknown argument '{other}'");
                        return ExitCode::FAILURE;
                    }
                }
            }
            let entry = match entry_point(&path) {
                Ok(entry) => entry,
                Err(message) => {
                    eprintln!("{message}");
                    return ExitCode::FAILURE;
                }
            };
            let out = out.unwrap_or_else(|| default_output_path(&path, target));
            if let Some(parent) = out.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        eprintln!("cannot create '{}': {e}", parent.display());
                        return ExitCode::FAILURE;
                    }
                }
            }
            build_file(&entry, target, &out, release)
        }
        "init" => cmd_init(args.collect()),
        #[cfg(feature = "install")]
        "install" => cmd_install(args.collect()),
        #[cfg(feature = "install")]
        "remove" | "rm" => cmd_remove(args.collect()),
        #[cfg(feature = "install")]
        "ls" => cmd_ls(),
        "test" => cmd_test(args.collect()),
        "format" => cmd_format(args.collect()),
        // Global flags rather than subcommands, so they take no feature gate
        // and work even in the wasm-only interpreter build.
        "--version" | "-v" | "version" => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        "--help" | "-h" | "help" => {
            // `code help build` answers about `build`, like `git help`.
            println!("{}", help_for(args.next().unwrap_or_default().as_str()));
            ExitCode::SUCCESS
        }
        "" => {
            // No command at all is a usage error, not a request for help —
            // but printing the help is still the most useful thing to say.
            eprintln!("{HELP}");
            ExitCode::FAILURE
        }
        other => {
            eprintln!("unknown command '{other}' — run `code --help`");
            ExitCode::FAILURE
        }
    }
}

/// The help text for one command, or the whole tool when the command is not
/// one that has its own. Kept beside `main`'s dispatch so a command that
/// gains a flag and a command that gains a line of help are the same edit.
fn help_for(command: &str) -> &'static str {
    match command {
        "run" => "usage: code run [path]\n\nInterprets a file, or a project's main.code. Defaults to `.`. `link` \
resolves relative to the file doing the linking.",
        "build" => BUILD_HELP,
        "install" | "remove" | "rm" | "ls" | "module" => MODULE_HELP,
        // Reachable as `code help init` too: someone who knows the command
        // exists but not where it lives should still find it.
        "init" => INIT_HELP,
        "format" => FORMAT_HELP,
        "test" => TEST_HELP,
        _ => HELP,
    }
}

const HELP: &str = "\
code — the Code programming language

usage:
  code <command> [arguments]

commands:
  init [name]                    scaffold a project here, or in <name>
  run [path]                     interpret a file, or a project's main.code
  build [path] [options]         compile one, into a build/ beside it
  install <name-or-url>          fetch a module into ./.code/modules
  remove <name>                  delete it, and its lock entry
  ls                             what is installed, and what is available
  test [path]...                 run the fixtures in tests/, or the ones named
  format [--check] <path>...     the canonical layout, rewritten in place

  -h, --help [command]           this, or one command's own help
  -v, --version                  which build this is

`run` and `build` take either a file or a directory, and default to `.`. A
directory means its main.code. Artifacts always go in a build/ beside what
you named.";

const BUILD_HELP: &str = "\
usage: code build [path] [options]

Compiles a file, or a project's main.code. Defaults to `.`. Without -o the
artifact goes in a build/ directory beside what you named, called after it:

  code build              -> build/<this directory>
  code build demo         -> demo/build/demo
  code build x.code       -> build/x
  code build src/x.code   -> src/build/x

options:
  -t, --target exe|shared|static|wasm   default exe
  -r, --release                         -O2; the default is unoptimized
  -o, --output <path>                   where to write it";

const MODULE_HELP: &str = "\
usage: code install <name-or-url> [--global]
       code remove <name>
       code ls

A first-party module installs by name, from this binary's own release — one
tag carries the CLI and every module at one version. Anything else installs
by the URL of its manifest. Bytes land in ./.code/modules and are pinned by
sha256 in ./.code/lock.json — `--global` puts them in ~/.code/modules and
records the same lock entry.";

const INIT_HELP: &str = "\
usage: code init [name]

Scaffolds a project in the current directory, or in <name>. Writes main.code
(which runs as written, with nothing installed), an empty .code/lock.json —
.code/ is what marks the project root — and a .gitignore. An existing file is
a refusal, never a merge.";

const TEST_HELP: &str = "\
usage: code test [path]...

Interprets each fixture and reports it. A fixture passes by running to the
end; one whose file name starts with `fail_` passes by *not* doing so. There
is nothing else to declare — `assert` is already the language's way of saying
what should hold.

No path means ./tests, walked for *.code. A path may be a directory or a
single fixture. Exits non-zero if any fixture did not do what its name says.";

const FORMAT_HELP: &str = "\
usage: code format [--check] <path>...

Rewrites .code files in the one canonical layout. A path may be a directory,
walked for *.code. --check writes nothing and exits non-zero if anything
would change. A file that does not parse is reported and skipped.";

/// The directory `path` names, when it names one. A project is a directory
/// with a `main.code` in it; anything else is a file, or a mistake the caller
/// hears about from `entry_point`.
fn project_dir(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    path.is_dir().then_some(path)
}

/// The `.code` file `path` means: itself when it is a file, and its
/// `main.code` when it is a directory.
///
/// One command rather than two, decided by the argument — `code run x.code`
/// and `code run .` are the same question about different things, and a
/// second command spelling would only make the caller say which kind of path
/// they had just typed. What *is* decided by the kind is where `build` puts
/// the artifact: beside a file, in `build/` for a project.
fn entry_point(path: &str) -> Result<String, String> {
    match project_dir(path) {
        Some(dir) => {
            let entry = dir.join(PROJECT_ENTRY);
            if entry.is_file() {
                Ok(entry.to_string_lossy().into_owned())
            } else {
                Err(format!(
                    "no {PROJECT_ENTRY} in '{}' — a project's entry point is {PROJECT_ENTRY} (see code init)",
                    dir.display()
                ))
            }
        }
        None if Path::new(path).is_file() => Ok(path.to_string()),
        None => Err(format!("no such file or directory: '{path}'")),
    }
}

/// A project's entry point — the file `code init` writes, and the one thing
/// `run`/`build` assume about a directory.
const PROJECT_ENTRY: &str = "main.code";
/// Where a project's artifacts go: one directory, deletable in one go.
#[cfg(feature = "llvm")]
const BUILD_DIR: &str = "build";

/// What a directory is called, for naming its artifact. `.` and `..` have no
/// name of their own, so ask the filesystem which directory they actually
/// are.
#[cfg(feature = "llvm")]
fn directory_name(dir: &Path) -> String {
    dir.canonicalize()
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "app".to_string())
}

/// `code init [name]` — a project that runs before anything is installed.
///
/// Three files, and the reasoning for each is that a fourth would be
/// decoration:
///
/// - `main.code`, which **runs as written**. The obvious template prints
///   something, and printing needs the `terminal` module, which is not
///   installed yet — so the first thing a new project would do is fail. This
///   one uses only the language and a core handler, and the next-steps text
///   says how to get printing.
/// - `.code/lock.json`, empty. Not ceremony: `.code/` is what marks the
///   project root, and `link` resolves an installed module by walking up to
///   the nearest one (`loader::find_project_code_dir`). Creating it is what
///   makes `code install` put modules *here* rather than in some ancestor
///   that happens to have a `.code/` of its own.
/// - `.gitignore`, one line. The lockfile is committed and the downloaded
///   binaries are not, which is the same split every lockfile ecosystem
///   makes and is easier to state now than to explain later.
///
/// Nothing is ever overwritten: an existing file is a refusal, not a merge.
fn cmd_init(args: Vec<String>) -> ExitCode {
    if let Some(unknown) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!("code init takes a directory name, not '{unknown}'");
        return ExitCode::FAILURE;
    }
    // No name means "here", which is what a directory you have already made
    // and `cd`ed into wants.
    let root = match args.first() {
        Some(name) => Path::new(name).to_path_buf(),
        None => Path::new(".").to_path_buf(),
    };

    let files = [
        (root.join("main.code"), MAIN_TEMPLATE),
        (root.join(".code").join("lock.json"), LOCKFILE_TEMPLATE),
        (root.join(".gitignore"), GITIGNORE_TEMPLATE),
    ];
    // Checked before anything is written, so a refusal leaves the directory
    // exactly as it was rather than half-initialized.
    for (path, _) in &files {
        if path.exists() {
            eprintln!("'{}' already exists — leaving it alone", path.display());
            return ExitCode::FAILURE;
        }
    }
    for (path, contents) in &files {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("cannot create '{}': {e}", parent.display());
                return ExitCode::FAILURE;
            }
        }
        if let Err(e) = std::fs::write(path, contents) {
            eprintln!("cannot write '{}': {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!("created {}", path.display());
    }

    let where_to = match args.first() {
        Some(name) => format!("cd {name} && "),
        None => String::new(),
    };
    println!();
    println!("  {where_to}code run");
    println!();
    println!("Printing lives in a module rather than the language:");
    println!("  code install terminal");
    println!("  then: link \"terminal.so\" as term");
    println!("        emit Print {{ value = \"hello\" }} to term");
    ExitCode::SUCCESS
}

/// Every construct here is one a reader will need on their first day, and
/// nothing here needs a module — see `cmd_init`.
const MAIN_TEMPLATE: &str = r#"-- A new Code program. `code run main.code` runs this as written.
--
-- There is no print statement: writing to a terminal is a module's job, not
-- the language's. `code install terminal` gets you one.

let name = "world"
let scores = [88, 94, 71]

-- `emit` sends a particle to a recipient. `core` is compiled in, so this
-- works with nothing installed.
emit Length { value = scores } to core get n
assert n.value = 3

-- The only loop form there is. `get` declares a result that survives it.
loop score over scores get best = 0 {
    if score > best {
        best = score
    }
}
assert best = 94

-- Handlers are how a program answers its own particles. There are no
-- functions.
Greet { who } => {
    return Greeting { text = "hello, $who" }
}

emit Greet { who = name } to this get greeting
assert greeting.text = "hello, world"
"#;

const LOCKFILE_TEMPLATE: &str = "{\n  \"modules\": {}\n}\n";

const GITIGNORE_TEMPLATE: &str = "\
# Installed module binaries. The lockfile beside them pins what they are,
# so a checkout can reproduce them with `code install`.
.code/modules/

# Where `code build <directory>` puts artifacts.
build/
";

/// A project's fixtures live here, and `code test` with no path means this.
const TESTS_DIR: &str = "tests";

/// `code test [path]...` — run every fixture and say which of them did what
/// its name says it would.
///
/// The convention is the one this repository's own suite already runs on: a
/// fixture must reach its end, and a `fail_*.code` fixture must not. That is
/// the whole contract, and it is deliberately not a framework — the language
/// has `assert`, so a test is just a program, and a runner only has to say
/// which programs stopped.
///
/// It lives here rather than in a wrapper because nothing about it is
/// specific to any framework built on Code: walking `tests/`, interpreting a
/// file, and reading the `fail_` prefix are all things `code` already knows
/// how to do, and every project that has fixtures wants the same answer.
///
/// **It interprets, and does not also build — deliberately.**
/// `tests/run_language_tests.rs` runs this repository's own fixtures through
/// *both* output modes, because those fixtures exist to prove the two modes
/// agree. The fixtures this command runs are somebody's *application* tests:
/// they assert what their program computes, and they are entitled to assume
/// what the language already guarantees. Compiling each one to check the same
/// assertion twice would double the wait to re-prove a property that is not
/// theirs to prove.
fn cmd_test(args: Vec<String>) -> ExitCode {
    if let Some(unknown) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!("code test takes paths, not '{unknown}'");
        return ExitCode::FAILURE;
    }

    let mut files = Vec::new();
    if args.is_empty() {
        let default_dir = Path::new(TESTS_DIR);
        if !default_dir.is_dir() {
            eprintln!(
                "no {TESTS_DIR}/ directory here — name the fixtures instead: code test <path>..."
            );
            return ExitCode::FAILURE;
        }
        collect_code_files(default_dir, &mut files);
    } else {
        for path in &args {
            collect_code_files(Path::new(path), &mut files);
        }
    }
    files.sort();

    if files.is_empty() {
        println!("no *.code fixtures found");
        return ExitCode::SUCCESS;
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("cannot find this binary to run fixtures with: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut passed = 0usize;
    let mut failed = 0usize;
    for file in &files {
        let must_fail = file
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("fail_"));
        let outcome = run_fixture(&exe, file);

        if outcome.is_ok() != must_fail {
            println!("ok    {}", file.display());
            passed += 1;
            continue;
        }

        println!("FAIL  {}", file.display());
        match &outcome {
            // Indented whole: an error carries its source excerpt over
            // several lines, and only indenting the first would break the
            // caret's alignment with the line above it.
            Err(e) => e.lines().for_each(|line| println!("      {line}")),
            Ok(()) => println!("      ran to the end, but `fail_` says it should not have"),
        }
        failed += 1;
    }

    println!();
    println!("{passed} passed, {failed} failed");
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Interpret one fixture, in a child process, and say whether it finished.
///
/// A child rather than a call straight into the interpreter, for the same
/// reason `tests/run_language_tests.rs` shells out: a fixture may `link` a
/// native module, and a module that dies takes its whole host process down
/// with it (see `docs/todo/native-module-linking.md`). Run in-process, one
/// bad module would kill the run and take the report of every other fixture
/// with it. In a child it is an exit code like any other failure — the
/// fixture is marked FAIL and the rest still run, which is the one thing a
/// test runner has to get right.
///
/// The fixture's own output is captured, so a passing run is silent and a
/// failing one shows what it said, verbatim: that text is the diagnostic,
/// and re-wording it here would only hide it.
fn run_fixture(exe: &Path, file: &Path) -> Result<(), String> {
    let out = std::process::Command::new(exe)
        .arg("run")
        .arg(file)
        .output()
        .map_err(|e| format!("cannot run '{}': {e}", exe.display()))?;
    if out.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        stderr.into_owned()
    };
    // A fixture killed by a signal — the linked-module case this runs in a
    // child for — usually says nothing at all, so the status is the report.
    Err(if said.trim().is_empty() {
        format!("no output ({})", out.status)
    } else {
        said.trim_end().to_string()
    })
}

/// `code format <path>...` rewrites in place; `--check` writes nothing and
/// exits non-zero if anything would change. A path may be a directory, walked
/// for `*.code`.
///
/// Never calls the loader: this lexes and parses *one file's text*, so a file
/// that `link`s a module which isn't installed still formats — resolving
/// anything is not the formatter's business.
///
/// A file that does not parse is reported and **skipped**, not failed. That
/// is the point rather than a leniency: `tests/` is full of `fail_*.code`
/// fixtures that are deliberate parse errors, and they have no layout to
/// canonicalize. Failing on them would make the CI gate unusable.
fn cmd_format(args: Vec<String>) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if paths.is_empty() {
        eprintln!("usage: code format [--check] <path>...");
        return ExitCode::FAILURE;
    }

    let mut files = Vec::new();
    for path in paths {
        collect_code_files(Path::new(path), &mut files);
    }
    files.sort();

    let mut changed = Vec::new();
    let mut failed = false;
    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("error: cannot read {}: {e}", file.display());
                failed = true;
                continue;
            }
        };
        let formatted = match code::format::format(&src) {
            Ok(formatted) => formatted,
            // Reported, not counted against the exit status — see the doc
            // comment above.
            Err(e) => {
                eprintln!("skipped {} ({})", file.display(), e.msg);
                continue;
            }
        };
        if formatted == src {
            continue;
        }
        changed.push(file.clone());
        if !check {
            if let Err(e) = std::fs::write(file, &formatted) {
                eprintln!("error: cannot write {}: {e}", file.display());
                failed = true;
            }
        }
    }

    if check && !changed.is_empty() {
        for file in &changed {
            eprintln!("would reformat {}", file.display());
        }
        eprintln!("{} file(s) need formatting", changed.len());
        return ExitCode::FAILURE;
    }
    if !check && !changed.is_empty() {
        println!("formatted {} file(s)", changed.len());
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Every `*.code` at or under `path`. A file is taken as given whatever its
/// extension — an explicitly named file is an explicit request — while a
/// directory is filtered, so `code format tests/` doesn't try to rewrite the
/// `.so` modules sitting beside the fixtures.
fn collect_code_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            eprintln!("error: cannot read directory {}", path.display());
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let child = entry.path();
            if child.is_dir() || child.extension().is_some_and(|x| x == "code") {
                collect_code_files(&child, out);
            }
        }
    } else if path.exists() {
        out.push(path.to_path_buf());
    } else {
        eprintln!("error: no such path: {}", path.display());
    }
}

#[cfg(feature = "llvm")]
/// Where an artifact goes when `-o` is not given: **always a `build/`
/// directory beside what was named**, holding a file named after it.
///
/// ```text
/// code build                -> build/<this directory>
/// code build demo           -> demo/build/demo
/// code build x.code         -> build/x
/// code build src/x.code     -> src/build/x
/// ```
///
/// One rule for both kinds of path, which is what makes it predictable: the
/// artifact is never dropped loose next to the source, and `build/` is always
/// where you just looked. Deleting a build is deleting one directory, and
/// `code init`'s `.gitignore` already ignores every one of them (a bare
/// `build/` pattern matches at any depth).
fn default_output_path(input: &str, target: BuildTarget) -> PathBuf {
    let path = Path::new(input);
    let (dir, stem) = match project_dir(input) {
        // A directory: its own `build/`, named for the directory.
        Some(dir) => {
            let name = directory_name(&dir);
            (dir, name)
        }
        // A file: the `build/` beside it, named for the file.
        None => {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "a.out".to_string());
            let dir = match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                _ => PathBuf::from("."),
            };
            (dir, stem)
        }
    };
    dir.join(BUILD_DIR).join(artifact_name(&stem, target))
}

/// The filename an artifact takes when `-o` is not given. The extension
/// follows the target rather than always being the bare stem: an executable
/// has none, but a library should look like one.
#[cfg(feature = "llvm")]
fn artifact_name(stem: &str, target: BuildTarget) -> String {
    match target {
        BuildTarget::Exe => stem.to_string(),
        BuildTarget::Shared => format!("lib{stem}.so"),
        BuildTarget::Static => format!("lib{stem}.a"),
        BuildTarget::Wasm => format!("{stem}.wasm"),
    }
}

fn run_file(path: &str) -> ExitCode {
    // The path, not the text: `link` resolves relative to the file doing the
    // linking, so reading the source here and passing a string would lose the
    // only thing module resolution has to work from.
    match code::run_file(Path::new(path)) {
        // Silent on success: a program's observable output is whatever it
        // itself emits (through a linked module such as `terminal`), not a
        // dump of its final bindings. Errors go to stderr.
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "llvm")]
fn build_file(path: &str, target: BuildTarget, out: &Path, release: bool) -> ExitCode {
    match code::compile_file(Path::new(path), target, out, release) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `code install <name-or-url> [--global]` — download a module's artifact
/// for this platform, verify its sha256 against the manifest, lay it down
/// under the project's (or `~/.code`'s) `modules/<name>/<version>/`, and pin
/// it in `.code/lock.json`. A first-party name resolves to this binary's own
/// release, since one tag publishes the CLI and every module at one version;
/// community modules come by the URL of their manifest (not a release page —
/// that serves HTML, not JSON).
#[cfg(feature = "install")]
fn cmd_install(mut args: Vec<String>) -> ExitCode {
    use code::module_install::{self, InstallScope};

    let mut global = false;
    let mut positionals: Vec<String> = Vec::new();
    while let Some(arg) = args.pop() {
        match arg.as_str() {
            "--global" => global = true,
            other => positionals.push(other.to_string()),
        }
    }
    // Popping reverses order, so the single allowed positional ends up first.
    positionals.reverse();
    if positionals.len() != 1 {
        eprintln!("usage: code install <name-or-url> [--global]");
        return ExitCode::FAILURE;
    }
    let reference = positionals.pop().expect("checked above");

    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("error: cannot determine the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let scope = if global {
        InstallScope::Global
    } else {
        InstallScope::Project
    };

    match module_install::install(&cwd, &reference, scope, env!("CARGO_PKG_VERSION")) {
        Ok(installed) => {
            println!(
                "installed {}@{} (sha256 {})",
                installed.name, installed.version, installed.sha256
            );
            println!("  {}", installed.path.display());
            // The bytes land under modules/<name>/<version>/ keeping their
            // platform-suffixed release asset name, but `link` can name the
            // module itself — the resolver maps `<name>.so` back to the
            // pinned asset through .code/lock.json. Both spellings resolve;
            // this is the tidy one.
            let ext = installed
                .path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_else(|| "so".to_string());
            println!(
                "link it with:  link \"{}.{ext}\" as <alias>",
                installed.name
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `code remove <name>` — drop the lock entry and delete the installed
/// bytes in both scopes. Idempotent: removing what is not installed reports
/// rather than errors.
#[cfg(feature = "install")]
fn cmd_remove(args: Vec<String>) -> ExitCode {
    use code::module_install;

    if args.len() != 1 {
        eprintln!("usage: code remove <name>");
        return ExitCode::FAILURE;
    }
    let name = args.into_iter().next().expect("checked above");
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("error: cannot determine the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    match module_install::remove(&cwd, &name) {
        Ok(notes) => {
            for note in notes {
                println!("{note}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `code ls` — installed modules (from the nearest lockfile, with their
/// bytes checked for presence) and available ones. The available list is the
/// first-party modules at this binary's own version, so it needs no network.
#[cfg(feature = "install")]
fn cmd_ls() -> ExitCode {
    use code::module_install::{self, InstallScope};

    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("error: cannot determine the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("installed:");
    let mut any_installed = false;
    if let Some(lock_path) = module_install::lockfile_path(&cwd) {
        if let Ok(lock) = module_install::read_lockfile(&lock_path) {
            for (name, entry) in &lock.modules {
                any_installed = true;
                let present = [InstallScope::Project, InstallScope::Global]
                    .iter()
                    .copied()
                    .filter_map(|scope| module_install::modules_root(&cwd, scope))
                    .any(|root| root.join(&entry.name).join(&entry.version).is_dir());
                let marker = if present { "" } else { " (bytes missing)" };
                let scope_marker = if entry.global { " [global]" } else { "" };
                println!("  {name}@{}{marker}{scope_marker}", entry.version);
            }
        }
    }
    if !any_installed {
        println!("  (none)");
    }

    println!("available:");
    for name in module_install::FIRST_PARTY {
        println!("  {name}@{}", env!("CARGO_PKG_VERSION"));
    }
    ExitCode::SUCCESS
}

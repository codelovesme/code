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

/// Where the first-party module index lives — the JSON file served from the
/// Pages site (`docs/todo/community-modules.md`: "starts life as a JSON file
/// in this repo, served from the Pages site"). Overridable for offline work
/// and pre-deploy dogfooding: point `CODE_MODULE_INDEX` at any URL serving
/// the same shape.
#[cfg(feature = "install")]
const MODULE_INDEX_URL: &str = "https://codelovesme.github.io/code/modules-index.json";

/// `run` interprets; `build` compiles (LLVM, see codegen.rs) and links a
/// standalone executable via the system `cc`. Both are meant to run every
/// language feature identically (see memory `new-language-rewrite`).
fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => {
            let Some(path) = args.next() else {
                eprintln!("usage: code run <file>");
                return ExitCode::FAILURE;
            };
            run_file(&path)
        }
        #[cfg(feature = "llvm")]
        Some("build") => {
            let Some(path) = args.next() else {
                eprintln!(
                    "usage: code build <file> [-t|--target exe|shared|static|wasm] [-r|--release] [-o|--output <path>]"
                );
                return ExitCode::FAILURE;
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
            let out = out.unwrap_or_else(|| default_output_path(&path, target));
            build_file(&path, target, &out, release)
        }
        Some("app") => cmd_app(args.collect()),
        Some("init") => cmd_init(args.collect()),
        #[cfg(feature = "install")]
        Some("module") => cmd_module(args.collect()),
        Some("format") => cmd_format(args.collect()),
        // A global flag rather than a subcommand, so it takes no feature gate
        // and works even in the wasm-only interpreter build.
        Some("--version") | Some("-v") => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command '{other}' ({USAGE})");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("usage: code {USAGE}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "init [name] | run <file> | build <file> [-t|--target exe|shared|static|wasm] [-r|--release] [-o|--output <path>] | app run|build [dir] | module install <name-or-url> [--global] | module remove <name> | module ls | format [--check] <path>...";

/// `code app run|build [dir]` — a *directory* where `run`/`build` take a
/// file.
///
/// Two commands rather than one that guesses, because they answer different
/// questions. A file is a file: `code build x.code` writes `x` beside it and
/// is done. A directory is a project, so it has an entry point by convention
/// (`main.code`, which `code init` writes) and its artifacts belong somewhere
/// deletable in one go (`build/`). Letting `build` take both would make the
/// output location depend on which kind of argument was passed — one command
/// quietly doing two things.
fn cmd_app(mut args: Vec<String>) -> ExitCode {
    let sub = if args.first().map(|a| !a.starts_with('-')).unwrap_or(false) {
        args.remove(0)
    } else {
        String::new()
    };
    match sub.as_str() {
        "run" | "build" => {}
        "" => {
            eprintln!("usage: code app run|build [dir]");
            return ExitCode::FAILURE;
        }
        other => {
            eprintln!("unknown app command '{other}' (expected run or build)");
            return ExitCode::FAILURE;
        }
    }

    // The directory is optional and defaults to `.` — a project you are
    // already standing in is the common case.
    let dir = match args.first() {
        Some(first) if !first.starts_with('-') => PathBuf::from(args.remove(0)),
        _ => PathBuf::from("."),
    };
    if !dir.is_dir() {
        eprintln!("'{}' is not a directory", dir.display());
        return ExitCode::FAILURE;
    }
    let entry = dir.join(APP_ENTRY);
    if !entry.is_file() {
        eprintln!(
            "no {APP_ENTRY} in '{}' — an app's entry point is {APP_ENTRY} (see code init)",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    let entry_path = entry.to_string_lossy().into_owned();

    if sub == "run" {
        if let Some(unknown) = args.first() {
            eprintln!("code app run takes a directory, not '{unknown}'");
            return ExitCode::FAILURE;
        }
        return run_file(&entry_path);
    }
    cmd_app_build(&dir, &entry_path, args)
}

#[cfg(not(feature = "llvm"))]
fn cmd_app_build(_dir: &Path, _entry: &str, _args: Vec<String>) -> ExitCode {
    eprintln!("this build has no compiler (built without the `llvm` feature)");
    ExitCode::FAILURE
}

#[cfg(feature = "llvm")]
fn cmd_app_build(dir: &Path, entry: &str, args: Vec<String>) -> ExitCode {
    let mut out: Option<PathBuf> = None;
    let mut target = BuildTarget::Exe;
    let mut release = false;
    let mut rest = args.into_iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" | "--output" => out = rest.next().map(PathBuf::from),
            "-r" | "--release" => release = true,
            "-t" | "--target" => {
                let Some(value) = rest.next() else {
                    eprintln!("--target takes a value (exe|shared|static|wasm)");
                    return ExitCode::FAILURE;
                };
                match BuildTarget::parse(&value) {
                    Some(t) => target = t,
                    None => {
                        eprintln!("unknown target '{value}' (expected exe|shared|static|wasm)");
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
    let out = out.unwrap_or_else(|| dir.join(APP_BUILD_DIR).join(app_artifact_name(dir, target)));
    if let Some(parent) = out.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("cannot create '{}': {e}", parent.display());
            return ExitCode::FAILURE;
        }
    }
    build_file(entry, target, &out, release)
}

/// An app's entry point. `code init` writes this name, and `code app` is the
/// only thing that assumes it.
const APP_ENTRY: &str = "main.code";
/// Where `code app build` puts artifacts: one directory, deletable in one go.
#[cfg(feature = "llvm")]
const APP_BUILD_DIR: &str = "build";

/// `build/<project>` rather than `build/main`: the artifact is named after
/// the thing being built, and every app's entry file has the same name.
#[cfg(feature = "llvm")]
fn app_artifact_name(dir: &Path, target: BuildTarget) -> String {
    // `.` and `..` have no name of their own, so ask the filesystem which
    // directory they actually are.
    let name = dir
        .canonicalize()
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "app".to_string());
    artifact_name(&name, target)
}

/// `code module install|remove|ls` — everything to do with modules under one
/// noun, because they are one subject and `code ls` never said what it was
/// listing.
#[cfg(feature = "install")]
fn cmd_module(mut args: Vec<String>) -> ExitCode {
    let sub = if args.is_empty() {
        String::new()
    } else {
        args.remove(0)
    };
    match sub.as_str() {
        "install" => cmd_install(args),
        "remove" | "rm" => cmd_remove(args),
        "ls" => cmd_ls(),
        "" => {
            eprintln!("usage: code module install <name-or-url> [--global] | remove <name> | ls");
            ExitCode::FAILURE
        }
        other => {
            eprintln!("unknown module command '{other}' (expected install, remove or ls)");
            ExitCode::FAILURE
        }
    }
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
    println!("  {where_to}code run main.code");
    println!();
    println!("Printing lives in a module rather than the language:");
    println!("  code module install terminal");
    println!("  then: link \"terminal.so\" as term");
    println!("        emit Print {{ \"value\": \"hello\" }} to term");
    ExitCode::SUCCESS
}

/// Every construct here is one a reader will need on their first day, and
/// nothing here needs a module — see `cmd_init`.
const MAIN_TEMPLATE: &str = r#"-- A new Code program. `code run main.code` runs this as written.
--
-- There is no print statement: writing to a terminal is a module's job, not
-- the language's. `code module install terminal` gets you one.

let name = "world"
let scores = [88, 94, 71]

-- `emit` sends a particle to a recipient. `core` is compiled in, so this
-- works with nothing installed.
emit Length { "value": scores } to core get n
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
    return Greeting { "text": "hello, $who" }
}

emit Greet { "who": name } to this get greeting
assert greeting.text = "hello, world"
"#;

const LOCKFILE_TEMPLATE: &str = "{\n  \"modules\": {}\n}\n";

const GITIGNORE_TEMPLATE: &str = "\
# Installed module binaries. The lockfile beside them pins what they are,
# so a checkout can reproduce them with `code module install`.
.code/modules/

# Where `code app build` puts artifacts.
build/
";

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
fn default_output_path(input: &str, target: BuildTarget) -> PathBuf {
    let input = Path::new(input);
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "a.out".to_string());
    // Beside the source, not in the working directory: `code build x.code`
    // answers with `x`, and `code build src/x.code` with `src/x`. Building a
    // *file* leaves one artifact next to it and nothing else — owning a
    // directory is `code app build`'s job.
    match input.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(artifact_name(&stem, target)),
        _ => PathBuf::from(artifact_name(&stem, target)),
    }
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
/// it in `.code/lock.json`. First-party names resolve through the index;
/// community modules come by the URL of their manifest (not a release page —
/// that serves HTML, not JSON).
#[cfg(feature = "install")]
fn cmd_install(mut args: Vec<String>) -> ExitCode {
    use code::module_install::{self, Index, InstallScope};

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

    let index_url =
        std::env::var("CODE_MODULE_INDEX").unwrap_or_else(|_| MODULE_INDEX_URL.to_string());
    let index_text = match module_install::fetch_url(&index_url) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let index: Index = match serde_json::from_str(&index_text) {
        Ok(index) => index,
        Err(e) => {
            eprintln!("error: malformed module index at '{index_url}': {e}");
            return ExitCode::FAILURE;
        }
    };

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

    match module_install::install(&cwd, &reference, scope, &index) {
        Ok(installed) => {
            println!(
                "installed {}@{} (sha256 {})",
                installed.name, installed.version, installed.sha256
            );
            println!("  {}", installed.path.display());
            // The bytes land under modules/<name>/<version>/ keeping their
            // release asset name, so the link line uses that name — the
            // fallback chain finds it without any further configuration.
            let asset_name = installed
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            println!("link it with:  link \"{asset_name}\" as <alias>");
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
/// bytes checked for presence) and available ones (from the index).
#[cfg(feature = "install")]
fn cmd_ls() -> ExitCode {
    use code::module_install::{self, Index, InstallScope};

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
    let index_url =
        std::env::var("CODE_MODULE_INDEX").unwrap_or_else(|_| MODULE_INDEX_URL.to_string());
    match module_install::fetch_url(&index_url) {
        Ok(text) => match serde_json::from_str::<Index>(&text) {
            Ok(index) => {
                if index.is_empty() {
                    println!("  (empty index)");
                }
                for (name, entry) in &index {
                    println!("  {name}@{}", entry.version);
                }
            }
            Err(e) => {
                eprintln!("warning: malformed module index at '{index_url}': {e}");
            }
        },
        Err(e) => {
            eprintln!("warning: could not fetch the module index: {e}");
        }
    }
    ExitCode::SUCCESS
}

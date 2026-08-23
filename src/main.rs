// `Path` is needed by `run` too now that it resolves modules; only `build`'s
// output path is LLVM-gated.
use std::path::Path;
#[cfg(feature = "llvm")]
use std::path::PathBuf;
use std::process::ExitCode;

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
                eprintln!("usage: code build <file> [-o <output>]");
                return ExitCode::FAILURE;
            };
            let mut out: Option<PathBuf> = None;
            while let Some(arg) = args.next() {
                if arg == "-o" {
                    out = args.next().map(PathBuf::from);
                } else {
                    eprintln!("unknown argument '{arg}'");
                    return ExitCode::FAILURE;
                }
            }
            let out = out.unwrap_or_else(|| default_output_path(&path));
            build_file(&path, &out)
        }
        #[cfg(feature = "install")]
        Some("install") => cmd_install(args.collect()),
        #[cfg(feature = "install")]
        Some("remove") | Some("rm") => cmd_remove(args.collect()),
        #[cfg(feature = "install")]
        Some("ls") => cmd_ls(),
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

const USAGE: &str = "run <file> | build <file> [-o <output>] | install <name-or-url> [--global] | remove <name> | ls";

#[cfg(feature = "llvm")]
fn default_output_path(input: &str) -> PathBuf {
    Path::new(input)
        .file_stem()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("a.out"))
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
fn build_file(path: &str, out: &Path) -> ExitCode {
    match code::compile_file(Path::new(path), out) {
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

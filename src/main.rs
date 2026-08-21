#[cfg(feature = "llvm")]
use std::path::{Path, PathBuf};
use std::process::ExitCode;

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
        Some(other) => {
            eprintln!("unknown command '{other}' (expected: run <file> | build <file>)");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("usage: code run <file> | code build <file> [-o <output>]");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "llvm")]
fn default_output_path(input: &str) -> PathBuf {
    Path::new(input)
        .file_stem()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("a.out"))
}

fn run_file(path: &str) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading '{path}': {e}");
            return ExitCode::FAILURE;
        }
    };

    match code::run_source(&src) {
        // There's no print/output construct in the language yet (an open
        // design question, not decided either way) — dump the final
        // bindings so a run is actually observable in the meantime.
        Ok(env) => {
            print!("{}", code::format_bindings(&env));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "llvm")]
fn build_file(path: &str, out: &Path) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading '{path}': {e}");
            return ExitCode::FAILURE;
        }
    };

    match code::compile_source(&src, out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

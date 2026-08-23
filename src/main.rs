// `Path` is needed by `run` too now that it resolves modules; only `build`'s
// output path is LLVM-gated.
use std::path::Path;
#[cfg(feature = "llvm")]
use std::path::PathBuf;
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

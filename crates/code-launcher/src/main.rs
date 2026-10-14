//! Shipped as `code` beside the full `code-driver`. A warm run never loads
//! LLVM; all other commands and misses exec the full driver unchanged.
#[allow(dead_code)]
#[path = "../../../src/cache.rs"]
mod cache;
#[allow(dead_code)]
#[path = "../../../src/run_cache.rs"]
mod run_cache;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn driver() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let parent = me.parent()?;
    // The source tree keeps the existing code binary for tools/embedders;
    // release packaging renames it to code-driver and this frontend to code.
    let name = if me.file_stem()? == "code-launcher" {
        "code"
    } else {
        "code-driver"
    };
    let path = parent.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::canonicalize(path).ok()
}

fn entry(args: &[String]) -> Option<PathBuf> {
    if args.first()?.as_str() != "run" || std::env::var("CODE_RUN").is_ok_and(|v| v == "interpret")
    {
        return None;
    }
    // Every option, including strict analysis, goes through the real CLI.
    // Only the ordinary no-option warm run can bypass parsing/checking.
    if args.len() > 2 || args.iter().skip(1).any(|v| v.starts_with('-')) {
        return None;
    }
    let path = PathBuf::from(args.get(1).map_or(".", String::as_str));
    let path = if path.is_dir() {
        path.join("main.code")
    } else {
        path
    };
    path.is_file().then_some(path)
}

fn start(exe: &Path, args: &[String]) -> Result<ExitCode, std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(Command::new(exe).args(args).exec())
    }
    #[cfg(not(unix))]
    {
        Command::new(exe).args(args).status().map(|status| {
            status
                .code()
                .map_or(ExitCode::FAILURE, |n| ExitCode::from(n as u8))
        })
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let Some(driver) = driver() else {
        eprintln!(
            "error: code-driver must be installed beside code (keep the release archive together)"
        );
        return ExitCode::FAILURE;
    };
    if let Some(entry) = entry(&args) {
        let hit = run_cache::lookup(&entry, true, &driver).or_else(|| {
            let hit = run_cache::lookup(&entry, false, &driver)?;
            run_cache::spawn_optimised_build(&driver, &entry, &hit.optimised_name);
            Some(hit)
        });
        if let Some(hit) = hit {
            match start(&hit.executable, &[]) {
                Ok(status) => return status,
                Err(e) => {
                    eprintln!("note: could not start the built program ({e}); interpreting it");
                    args.push("--interpret".into());
                }
            }
        }
    }
    start(&driver, &args).unwrap_or_else(|e| {
        eprintln!("error: could not start code-driver ({e})");
        ExitCode::FAILURE
    })
}

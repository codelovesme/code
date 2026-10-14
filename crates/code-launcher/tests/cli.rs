//! Exercise the layout actually shipped: code beside code-driver.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bundle(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-launcher-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let launcher = Path::new(env!("CARGO_BIN_EXE_code-launcher"));
    let backend = launcher
        .parent()
        .unwrap()
        .join(format!("code{}", std::env::consts::EXE_SUFFIX));
    fs::copy(
        launcher,
        dir.join(format!("code{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    fs::copy(
        backend,
        dir.join(format!("code-driver{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    dir
}

fn command(dir: &Path) -> Command {
    let mut cmd = Command::new(dir.join(format!("code{}", std::env::consts::EXE_SUFFIX)));
    cmd.current_dir(dir)
        .env_remove("CODE_RUN")
        .env("CODE_CACHE_DIR", dir.join("cache"));
    cmd
}

fn run(dir: &Path, args: &[&str]) -> Output {
    command(dir).args(args).output().unwrap()
}

#[test]
fn the_bundle_delegates_other_commands_and_strict_checks() {
    let dir = bundle("commands");
    let version = run(&dir, &["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        format!("Code v{}\n", env!("CARGO_PKG_VERSION"))
    );
    fs::write(
        dir.join("main.code"),
        "Grade { score } => return Ack\nemit Grdae { score = 88 } to this\n",
    )
    .unwrap();
    assert!(run(&dir, &["run"]).status.success());
    let strict = run(&dir, &["run", "--strict"]);
    assert!(!strict.status.success());
    assert!(String::from_utf8_lossy(&strict.stderr).contains("unknown-handler"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_frontend_invalidates_a_changed_linked_file() {
    let dir = bundle("changed-source");
    fs::write(
        dir.join("main.code"),
        "link \"child.code\"\nemit Answer to this get r\nassert r.value = 1\n",
    )
    .unwrap();
    fs::write(
        dir.join("child.code"),
        "Answer {} => return R { value = 1 }\n",
    )
    .unwrap();
    assert!(run(&dir, &["run"]).status.success());
    assert!(run(&dir, &["run"]).status.success());
    fs::write(
        dir.join("child.code"),
        "Answer {} => return R { value = 2 }\n",
    )
    .unwrap();
    let stale = run(&dir, &["run"]);
    assert!(!stale.status.success(), "the old executable would pass");
    assert!(String::from_utf8_lossy(&stale.stderr).contains("assertion failed"));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
fn cached(dir: &Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = fs::read_dir(dir.join("cache/run"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_none())
        .unwrap();
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn the_frontend_execs_with_the_same_process_streams_and_status() {
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;
    let dir = bundle("process");
    fs::write(dir.join("main.code"), "assert true\n").unwrap();
    assert!(run(&dir, &["run"]).status.success());
    cached(&dir, "#!/bin/sh\nread -r value\nprintf '%s:%s:%s:%s\\n' \"$$\" \"$#\" \"$CODE_EXEC_TEST\" \"$value\"\nprintf 'stderr intact\\n' >&2\nexit 7\n");
    let mut child = command(&dir)
        .arg("run")
        .env("CODE_EXEC_TEST", "inherited")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    child.stdin.take().unwrap().write_all(b"input\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{pid}:0:inherited:input\n")
    );
    assert_eq!(out.stderr, b"stderr intact\n");
    cached(&dir, "#!/bin/sh\nkill -TERM $$\n");
    assert_eq!(run(&dir, &["run"]).status.signal(), Some(15));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn failed_frontend_exec_falls_back_to_interpreting() {
    use std::os::unix::fs::PermissionsExt;
    let dir = bundle("exec-fails");
    fs::write(dir.join("main.code"), "assert true\n").unwrap();
    assert!(run(&dir, &["run"]).status.success());
    let exe = cached(&dir, "not executable\n");
    fs::set_permissions(exe, fs::Permissions::from_mode(0o644)).unwrap();
    let output = run(&dir, &["run"]);
    assert!(output.status.success());
    let note = String::from_utf8(output.stderr).unwrap();
    assert!(note.starts_with("note: could not start the built program ("));
    assert!(note.ends_with("); interpreting it\n"));
    fs::remove_dir_all(dir).unwrap();
}

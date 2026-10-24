//! `link "math" as math` — a module named, not a file (ticket 142).
//!
//! A local registry stands in for a release: a directory laid out as
//! `releases/download/v<version>/`, reached through `CODE_MODULE_RELEASE`
//! and `file://`, carrying each module's `.so`, its native archive and the
//! module JSON that names both (`linux-x86_64` and `linux-x86_64-static`,
//! as `publish-modules.yml` writes it). `code install` from there pins both;
//! then one unchanged source builds:
//!
//! - as a native executable, linking the archives in — and runs with every
//!   `.so` taken away;
//! - with `--link dynamic`, opening the `.so`s;
//! - interpreted, which always opens the `.so`s;
//!
//! all with the same answers. And the failures say what is wrong: a
//! tampered archive, a lock pinned before archives existed, a browser build
//! with no browser half installed. A literal `.so` path stays what it says.
#![cfg(all(feature = "llvm", feature = "install"))]
#[path = "support/modules.rs"]
mod modules;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const VERSION: &str = "9.9.9";

const PROGRAM: &str = r#"
link "math" as math
link "strings" as text
link "console.so" as term
emit Floor { value = 3.7 } to math get f
emit Upper { value = "abc" } to text get u
emit Print { value = "$f.value $u.value" } to term
"#;

fn sha256(path: &Path) -> String {
    let out = Command::new("sha256sum").arg(path).output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// `releases/download/v<VERSION>/` with `name`'s `.so`, `.a` and JSON.
fn publish(registry: &Path, name: &str) {
    let dir = registry.join(format!("releases/download/v{VERSION}"));
    fs::create_dir_all(&dir).unwrap();
    for script in ["build-module-linux.sh", "build-module-static.sh"] {
        let out = Command::new(modules::repo().join("scripts").join(script))
            .arg(name)
            .arg(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{script} {name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let so = format!("{name}-linux-x86_64.so");
    let archive = format!("{name}-linux-x86_64.a");
    let libs: Vec<String> = fs::read_to_string(dir.join(format!("{archive}.libs")))
        .unwrap()
        .lines()
        .map(|l| format!("\"{l}\""))
        .collect();
    fs::write(
        dir.join(format!("{name}.json")),
        format!(
            r#"{{ "name": "{name}", "version": "{VERSION}", "abi_version": 2, "handlers": [],
  "platforms": {{
    "linux-x86_64": {{ "asset": "{so}", "sha256": "{}" }},
    "linux-x86_64-static": {{ "asset": "{archive}", "sha256": "{}", "link": [{}] }}
  }} }}"#,
            sha256(&dir.join(&so)),
            sha256(&dir.join(&archive)),
            libs.join(", ")
        ),
    )
    .unwrap();
}

fn code(project: &Path, registry: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_code"))
        .args(args)
        .current_dir(project)
        .env(
            "CODE_MODULE_RELEASE",
            format!("file://{}/releases/tag/v{VERSION}", registry.display()),
        )
        .env_remove("CODE_LINK")
        .output()
        .unwrap()
}

fn ok(out: Output, what: &str) -> String {
    assert!(
        out.status.success(),
        "{what}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn err(out: Output, what: &str) -> String {
    assert!(!out.status.success(), "{what} should fail");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn installed(project: &Path, name: &str, ext: &str) -> PathBuf {
    project.join(format!(
        ".code/modules/{name}/{VERSION}/{name}-linux-x86_64.{ext}"
    ))
}

#[test]
fn one_source_links_static_or_dynamic_by_build() {
    let root = std::env::temp_dir().join(format!("code-module-identity-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (registry, project) = (root.join("registry"), root.join("project"));
    fs::create_dir_all(project.join(".code")).unwrap();
    publish(&registry, "math");
    publish(&registry, "strings");
    for name in ["math", "strings"] {
        ok(
            code(&project, &registry, &["install", name]),
            &format!("install {name}"),
        );
    }
    fs::copy(modules::build_so("console"), project.join("console.so")).unwrap();
    let lock = fs::read_to_string(project.join(".code/lock.json")).unwrap();
    assert_eq!(lock.matches("\"static_archive\"").count(), 2, "{lock}");
    assert!(
        installed(&project, "math", "a.libs").is_file(),
        "link flags beside the archive"
    );
    fs::write(project.join("main.code"), PROGRAM).unwrap();

    // An executable links the archives in, and needs no `.so` to run.
    ok(
        code(&project, &registry, &["build", "main.code", "-o", "static"]),
        "static build",
    );
    let hidden: Vec<(PathBuf, PathBuf)> = ["math", "strings"]
        .iter()
        .map(|n| {
            let so = installed(&project, n, "so");
            (so.clone(), so.with_extension("so.hidden"))
        })
        .collect();
    for (so, away) in &hidden {
        fs::rename(so, away).unwrap();
    }
    let linked_in = ok(
        Command::new(project.join("static")).output().unwrap(),
        "static run",
    );
    for (so, away) in &hidden {
        fs::rename(away, so).unwrap();
    }

    // The same source, opened beside the program; and interpreted.
    ok(
        code(
            &project,
            &registry,
            &["build", "main.code", "--link", "dynamic", "-o", "dynamic"],
        ),
        "dynamic build",
    );
    let opened = ok(
        Command::new(project.join("dynamic")).output().unwrap(),
        "dynamic run",
    );
    let interpreted = ok(
        code(&project, &registry, &["run", "--interpret", "main.code"]),
        "interpret",
    );
    assert_eq!(linked_in, "3 ABC\n");
    assert_eq!(opened, linked_in);
    assert_eq!(interpreted, linked_in);

    // A literal path is what it says, in any build.
    fs::write(
        project.join("literal.code"),
        "link \"math.so\" as m\nemit Floor { value = 1.5 } to m get f\nassert f.value = 1\n",
    )
    .unwrap();
    ok(
        code(
            &project,
            &registry,
            &["build", "literal.code", "-o", "literal"],
        ),
        "literal build",
    );
    let literal = Command::new("ldd")
        .arg(project.join("literal"))
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&literal.stdout).is_empty());

    // Tampered: the archive no longer matches its pin.
    let archive = installed(&project, "math", "a");
    let original = fs::read(&archive).unwrap();
    let mut changed = original.clone();
    changed.push(0);
    fs::write(&archive, &changed).unwrap();
    let tampered = err(
        code(&project, &registry, &["build", "main.code", "-o", "x"]),
        "tampered",
    );
    assert!(tampered.contains("does not match the sha256"), "{tampered}");
    fs::write(&archive, &original).unwrap();

    // A lock pinned before releases carried archives: say so, don't fall back.
    let old: String = lock
        .lines()
        .scan(0, |skip, line| {
            if line.contains("\"static_archive\"") {
                *skip = 1;
            }
            let keep = *skip == 0;
            if *skip > 0 && line.trim_start().starts_with('}') {
                *skip = 0;
            }
            Some(keep.then(|| line.to_string()))
        })
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let old = old
        .replace(",\n    }", "\n    }")
        .replace("\"global\": false,\n", "\"global\": false\n");
    fs::write(project.join(".code/lock.json"), &old).unwrap();
    let before = err(
        code(&project, &registry, &["build", "main.code", "-o", "x"]),
        "old lock",
    );
    assert!(before.contains("links it as a native archive"), "{before}");
    assert!(before.contains("--link dynamic"), "{before}");
    ok(
        code(
            &project,
            &registry,
            &["build", "main.code", "--link", "dynamic", "-o", "y"],
        ),
        "old lock, dynamic",
    );
    fs::write(project.join(".code/lock.json"), &lock).unwrap();

    // A browser build wants the browser half, and none is installed.
    let page = err(
        code(
            &project,
            &registry,
            &["build", "main.code", "--target", "wasm", "-o", "p.wasm"],
        ),
        "wasm",
    );
    assert!(page.contains("wasm32"), "{page}");
}

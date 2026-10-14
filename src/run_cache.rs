//! A warm run checks the inputs of its last build without loading the program.
//! Records are hints: missing, stale, unreadable, or raced inputs take the full
//! loader/compiler path. Executables retain their full build keys.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const SCHEMA: &str = "code-run-inputs\t1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    canonical: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    directory: bool,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

pub(crate) fn absolute(path: &Path) -> Option<PathBuf> {
    Some(if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().ok()?.join(path)
    })
}

pub(crate) fn stamp(path: &Path) -> Result<Option<Stamp>, String> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let directory = meta.is_dir();
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        // Directory contents are guarded by their candidate file paths. Merely
        // adding unrelated files must not invalidate a program's run cache.
        (
            meta.dev(),
            meta.ino(),
            if directory { 0 } else { meta.ctime() },
            if directory { 0 } else { meta.ctime_nsec() },
        )
    };
    Ok(Some(Stamp {
        canonical: fs::canonicalize(path).map_err(|e| e.to_string())?,
        size: if directory { 0 } else { meta.len() },
        modified: if directory {
            None
        } else {
            Some(meta.modified().map_err(|e| e.to_string())?)
        },
        directory,
        #[cfg(unix)]
        identity,
    }))
}

#[derive(Clone)]
pub(crate) struct Inputs {
    pub(crate) entry: PathBuf,
    pub(crate) compiler: PathBuf,
    pub(crate) context: Vec<Option<String>>,
    pub(crate) files: BTreeMap<PathBuf, Option<Stamp>>,
}

pub(crate) fn context() -> Option<Vec<Option<String>>> {
    ["HOME", "CODE_MODULE_PATH"]
        .iter()
        .map(|name| {
            std::env::var_os(name)
                .map(|v| v.into_string())
                .transpose()
                .ok()
        })
        .collect()
}

impl Inputs {
    pub(crate) fn matches(&self) -> bool {
        std::env::current_exe()
            .ok()
            .is_some_and(|compiler| self.matches_compiler(&compiler))
    }

    fn matches_compiler(&self, compiler: &Path) -> bool {
        context().as_ref() == Some(&self.context)
            && compiler == self.compiler
            && self
                .files
                .iter()
                .all(|(path, expected)| stamp(path).ok().as_ref() == Some(expected))
    }

    pub(crate) fn key(&self) -> Option<String> {
        Some(crate::cache::key(&[self.lines()?.as_bytes()]))
    }
}

fn target() -> String {
    format!("{}-{}-exe", std::env::consts::ARCH, std::env::consts::OS)
}

fn index(entry: &Path, optimised: bool) -> Option<String> {
    Some(crate::cache::key(&[
        absolute(entry)?.to_str()?.as_bytes(),
        std::env::current_dir().ok()?.to_str()?.as_bytes(),
        env!("CARGO_PKG_VERSION").as_bytes(),
        target().as_bytes(),
        if optimised { b"O2" } else { b"O0" },
    ]))
}

// Hex keeps paths/environment values unambiguous even with tabs or newlines.
fn hex(text: &str) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

fn unhex(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let bytes: Option<Vec<u8>> = text
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| Some(digit(c[0])? * 16 + digit(c[1])?))
        .collect();
    String::from_utf8(bytes?).ok()
}

fn context_line(values: &[Option<String>]) -> String {
    values
        .iter()
        .map(|v| v.as_ref().map_or_else(|| "-".into(), |s| hex(s)))
        .collect::<Vec<_>>()
        .join("\t")
}

fn file_line(path: &Path, value: &Option<Stamp>) -> Option<String> {
    let path = hex(path.to_str()?);
    let Some(value) = value else {
        return Some(format!("file\t{path}\t-"));
    };
    let modified = value.modified.map_or_else(
        || "-".into(),
        |t| match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_nanos().to_string(),
            Err(e) => format!("-{}", e.duration().as_nanos()),
        },
    );
    #[cfg(unix)]
    let identity = value.identity;
    #[cfg(not(unix))]
    let identity = (0u64, 0u64, 0i64, 0i64);
    Some(format!(
        "file\t{path}\t{}\t{}\t{modified}\t{}\t{}\t{}\t{}\t{}",
        hex(value.canonical.to_str()?),
        value.size,
        u8::from(value.directory),
        identity.0,
        identity.1,
        identity.2,
        identity.3
    ))
}

impl Inputs {
    fn lines(&self) -> Option<String> {
        let mut out = format!(
            "entry\t{}\ncompiler\t{}\ncontext\t{}\n",
            hex(self.entry.to_str()?),
            hex(self.compiler.to_str()?),
            context_line(&self.context)
        );
        for (path, value) in &self.files {
            out.push_str(&file_line(path, value)?);
            out.push('\n');
        }
        Some(out)
    }
}

pub(crate) struct Hit {
    pub(crate) executable: PathBuf,
    pub(crate) optimised_name: String,
}

fn name_is_valid(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn lookup(entry: &Path, optimised: bool, compiler: &Path) -> Option<Hit> {
    let pointer = crate::cache::get("run-index", &index(entry, optimised)?)?;
    let name = fs::read_to_string(pointer).ok()?;
    if !name_is_valid(&name) {
        return None;
    }
    let path = crate::cache::get("run", &format!("{name}.inputs"))?;
    let record = fs::read_to_string(path).ok()?;
    let mut lines = record.lines();
    let entry = absolute(entry)?;
    let headers = [
        SCHEMA.to_owned(),
        format!("version\t{}", env!("CARGO_PKG_VERSION")),
        format!("target\t{}", target()),
        format!("tier\t{}", if optimised { "O2" } else { "O0" }),
        format!("executable\t{name}"),
    ];
    for header in headers {
        if lines.next()? != header {
            return None;
        }
    }
    let optimised_name = lines.next()?.strip_prefix("optimised\t")?.to_owned();
    if !name_is_valid(&optimised_name) {
        return None;
    }
    for header in [
        format!("entry\t{}", hex(entry.to_str()?)),
        format!("compiler\t{}", hex(compiler.to_str()?)),
        format!("context\t{}", context_line(&context()?)),
    ] {
        if lines.next()? != header {
            return None;
        }
    }
    let mut saw_entry = false;
    let mut saw_compiler = false;
    for line in lines {
        let path = PathBuf::from(unhex(line.strip_prefix("file\t")?.split('\t').next()?)?);
        let observed = stamp(&path).ok()?;
        if file_line(&path, &observed)? != line {
            return None;
        }
        saw_entry |= path == entry && observed.as_ref().is_some_and(|m| !m.directory);
        saw_compiler |= path == compiler && observed.as_ref().is_some_and(|m| !m.directory);
    }
    if !saw_entry || !saw_compiler {
        return None;
    }
    Some(Hit {
        executable: crate::cache::get("run", &name)?,
        optimised_name,
    })
}

pub(crate) fn remember(inputs: &Inputs, optimised: bool, name: &str, optimised_name: &str) {
    let Some(index) = index(&inputs.entry, optimised) else {
        return;
    };
    let Some(files) = inputs.lines() else {
        return;
    };
    let record = format!("{SCHEMA}\nversion\t{}\ntarget\t{}\ntier\t{}\nexecutable\t{name}\noptimised\t{optimised_name}\n{files}",
        env!("CARGO_PKG_VERSION"), target(), if optimised { "O2" } else { "O0" });
    crate::cache::put_bytes("run", &format!("{name}.inputs"), record.as_bytes());
    crate::cache::put_bytes("run-index", &index, name.as_bytes());
}

/// Where a background optimised build says it is under way, so a second
/// run does not start another.
pub(crate) fn building_marker(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("code-run-{name}.building"))
}

/// Starts `code --build-optimised-run <path>` on its own, unless one for
/// this build is under way already. It outlives this run, in a process
/// group of its own so that the run's Ctrl+C does not stop it.
pub(crate) fn spawn_optimised_build(compiler: &Path, source_path: &Path, name: &str) {
    let marker = building_marker(name);
    if fs::File::options()
        .write(true)
        .create_new(true)
        .open(&marker)
        .is_err()
    {
        return;
    }
    let mut command = std::process::Command::new(compiler);
    command
        .arg("--build-optimised-run")
        .arg(source_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    if command.spawn().is_err() {
        let _ = fs::remove_file(&marker);
    }
}

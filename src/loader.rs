//! Resolves `link` statements into `Import`s, before either output mode sees
//! the program.
//!
//! This is the whole of module resolution — path lookup, recursion, cycle
//! detection, working out what a module exports — and it is pure AST work.
//! That placement is deliberate: every other feature in this language needed
//! parallel work in `interpreter.rs` and `codegen.rs`, but here the expensive
//! part is written once and both backends receive a program in which `Link`
//! no longer appears.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ast::{NativeFormat, Program, Stmt};
use crate::{lexer, parser, span};

/// What a module reference resolved to.
///
/// An enum with one variant rather than a plain `(identity, text)` pair: a
/// linked module will eventually be allowed to be a *native* one — compiled
/// by `code` itself or by another language — and that variant carries a path
/// and a library kind rather than source text. Shaping the return type for it
/// now means adding it later doesn't change this trait's signature.
pub enum ResolvedModule {
    /// A `.code` module, to be parsed and inlined.
    Source { identity: String, text: String },
    /// A native module (`.so` or `.a`) — see `docs/todo/native-module-linking.md`
    /// and `code_abi.h`. Carries a real filesystem path (not source text):
    /// loading it means `dlopen` or a `cc`-time link, done by the
    /// interpreter/codegen, not here — this module stays pure AST/path work,
    /// like the `Source` case. `format` distinguishes the two — see
    /// `ast::NativeFormat`.
    Native {
        identity: String,
        path: String,
        format: NativeFormat,
    },
}

/// Where module source comes from. Abstracted so the loader is not tied to a
/// filesystem — the same reason the old language did it, though the one host
/// that needs it today (`crates/code-wasm`) deliberately doesn't support
/// `link` at all and uses [`NoModules`].
pub trait ModuleResolver {
    /// Resolve the program being run. `identity` uniquely names it and is
    /// what relative links inside it resolve against.
    fn resolve_entry(&self, entry: &str) -> Result<ResolvedModule, String>;

    /// Resolve `module_ref` as written inside the module identified by
    /// `from_identity`.
    fn resolve(&self, from_identity: &str, module_ref: &str) -> Result<ResolvedModule, String>;
}

/// Reads modules from the real filesystem. A reference resolves against a
/// short, fixed search path — the script's own directory first, then the
/// nearest project's `.code/modules/`, then `$CODE_MODULE_PATH`, then
/// `~/.code/modules/` (see `docs/todo/community-modules.md`, "Loader:
/// fallback chain") — so where a module comes from is answerable by looking
/// at the script, its lockfile, and those four fixed places.
pub struct FilesystemResolver;

/// The directory installed modules live in under a `.code` directory.
pub const MODULES_DIR: &str = "modules";

impl FilesystemResolver {
    fn read(path: &Path) -> Result<ResolvedModule, String> {
        let canonical = fs::canonicalize(path)
            .map_err(|e| format!("cannot resolve '{}': {e}", path.display()))?;
        let text = fs::read_to_string(&canonical)
            .map_err(|e| format!("cannot read '{}': {e}", canonical.display()))?;
        Ok(ResolvedModule::Source {
            identity: canonical.display().to_string(),
            text,
        })
    }

    /// The ordered roots a `module_ref` is looked up under, for a link made
    /// from a file in `base`:
    ///
    /// 1. `base` itself — explicit wins, unchanged from the original
    ///    script-directory-only behaviour;
    /// 2. the nearest ancestor's `.code/modules/` (walk up, like
    ///    `node_modules`) — where `code install` lays bytes down;
    /// 3. each `$CODE_MODULE_PATH` entry (colon-separated) — unusual setups;
    /// 4. `~/.code/modules/` — globally installed modules.
    fn candidate_roots(base: &Path) -> Vec<PathBuf> {
        let mut roots = vec![base.to_path_buf()];
        if let Some(code_dir) = find_project_code_dir(base) {
            roots.push(code_dir.join(MODULES_DIR));
        }
        if let Ok(var) = std::env::var("CODE_MODULE_PATH") {
            for dir in var.split(':').filter(|s| !s.is_empty()) {
                roots.push(PathBuf::from(dir));
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".code").join(MODULES_DIR));
        }
        roots
    }

    /// The first existing candidate for a native module reference, canonical
    /// (absolute, symlink-free) — `canonicalize` doubles as the existence
    /// test. Two shapes are tried per root:
    ///
    /// - `<root>/<ref>` verbatim — vendored copies next to the script, or a
    ///   flat `modules/` dir holding bare asset names;
    /// - the *installed* layout `<root>/<name>/<version>/<asset>` — where
    ///   `code install` lays bytes down. A bare asset name is mapped to its
    ///   pinned location through the project's lockfile, which is the single
    ///   source of truth for "what is installed here"; without an entry the
    ///   layout lookup has nothing to go on and simply does not apply.
    #[cfg(feature = "install")]
    fn locate_native(base: &Path, module_ref: &str) -> Option<PathBuf> {
        Self::locate_native_inner(base, module_ref)
    }

    /// Same as [`Self::locate_native`] minus the installed-layout step — the
    /// shape a build without the installer feature can still resolve.
    #[cfg(not(feature = "install"))]
    fn locate_native(base: &Path, module_ref: &str) -> Option<PathBuf> {
        Self::locate_native_inner(base, module_ref)
    }

    fn locate_native_inner(base: &Path, module_ref: &str) -> Option<PathBuf> {
        for root in Self::candidate_roots(base) {
            if let Ok(canonical) = fs::canonicalize(root.join(module_ref)) {
                return Some(canonical);
            }
        }
        #[cfg(feature = "install")]
        {
            let file_name = Path::new(module_ref).file_name().and_then(|n| n.to_str())?;
            let code_dir = find_project_code_dir(base)?;
            let lock_path = code_dir.join(crate::module_install::LOCK_FILE_NAME);
            let text = fs::read_to_string(&lock_path).ok()?;
            let lock: crate::module_install::Lockfile = serde_json::from_str(&text).ok()?;
            for entry in lock.modules.values() {
                if entry.asset != file_name {
                    continue;
                }
                for root in [
                    Some(code_dir.join(MODULES_DIR)),
                    crate::module_install::global_code_dir().map(|g| g.join(MODULES_DIR)),
                ]
                .into_iter()
                .flatten()
                {
                    let candidate = root.join(&entry.name).join(&entry.version).join(file_name);
                    if let Ok(canonical) = fs::canonicalize(&candidate) {
                        return Some(canonical);
                    }
                }
            }
        }
        None
    }

    /// What the failure message shows instead of a bare "not found": every
    /// root that was tried, so the answer to "where did it look?" is right
    /// there in the error.
    fn searched_list(base: &Path) -> String {
        Self::candidate_roots(base)
            .iter()
            .map(|r| format!("'{}'", r.display()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// While a lock entry pins the resolved bytes, re-check their sha256
    /// before handing them to either backend — a tampered or replaced `.so`
    /// fails loudly instead of loading (`docs/todo/community-modules.md`:
    /// "Verification"). Gated on `install` because parsing the lockfile
    /// needs serde; without the feature there is no installer, hence no
    /// lockfile, hence nothing to verify.
    #[cfg(feature = "install")]
    fn verify_locked_module(base: &Path, resolved: &Path) -> Result<(), String> {
        let Some(code_dir) = find_project_code_dir(base) else {
            return Ok(());
        };
        let lock_path = code_dir.join(crate::module_install::LOCK_FILE_NAME);
        let text = match fs::read_to_string(&lock_path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("cannot read '{}': {e}", lock_path.display())),
        };
        let lock: crate::module_install::Lockfile = serde_json::from_str(&text)
            .map_err(|e| format!("malformed lockfile '{}': {e}", lock_path.display()))?;

        let file_name = resolved.file_name().and_then(|n| n.to_str()).unwrap_or("");
        for entry in lock.modules.values() {
            if entry.asset != file_name {
                continue;
            }
            // Enforce only for bytes that actually sit in an install
            // location for this entry — a same-named file elsewhere (a
            // vendored copy next to the script) is deliberate and unpinned.
            // Roots are canonicalized when they exist so a symlinked HOME
            // cannot quietly defeat the check.
            let roots: Vec<PathBuf> = [
                Some(code_dir.join(MODULES_DIR)),
                crate::module_install::global_code_dir().map(|g| g.join(MODULES_DIR)),
            ]
            .into_iter()
            .flatten()
            .map(|root| fs::canonicalize(&root).unwrap_or(root))
            .collect();
            let under_install = roots
                .iter()
                .any(|root| resolved.starts_with(root.join(&entry.name).join(&entry.version)));
            if !under_install {
                continue;
            }
            if !crate::module_install::verify_sha256(resolved, &entry.sha256)? {
                return Err(format!(
                    "refusing to load '{}': it does not match the sha256 pinned in '{}' — \
                     re-run `code install {}`",
                    resolved.display(),
                    lock_path.display(),
                    entry.name
                ));
            }
            return Ok(());
        }
        Ok(())
    }
}

impl ModuleResolver for FilesystemResolver {
    fn resolve_entry(&self, entry: &str) -> Result<ResolvedModule, String> {
        Self::read(Path::new(entry))
    }

    fn resolve(&self, from_identity: &str, module_ref: &str) -> Result<ResolvedModule, String> {
        let base = Path::new(from_identity)
            .parent()
            .unwrap_or_else(|| Path::new("."));

        // Format decided by extension (docs/todo/native-module-linking.md):
        // `.so` loads (both output modes, via dlopen — never `cc`-time
        // static linking, see runtime.c's `code_native_open`). `.a` resolves
        // here too (both modes see the same AST either way — see
        // `ast::NativeFormat`), but only `code build` can actually link one;
        // `interpreter.rs` refuses a `Static` `ImportNative` outright.
        // `.wasm` is named in the todo doc as a future format, not built yet.
        if let Some(ext) = Path::new(module_ref).extension().and_then(|e| e.to_str()) {
            if ext == "so" {
                let canonical = Self::locate_native(base, module_ref).ok_or_else(|| {
                    format!(
                        "cannot resolve module '{module_ref}' from '{from_identity}' \
                         (looked in: {})",
                        Self::searched_list(base)
                    )
                })?;
                #[cfg(feature = "install")]
                Self::verify_locked_module(base, &canonical)?;
                return Ok(ResolvedModule::Native {
                    identity: canonical.display().to_string(),
                    path: canonical.display().to_string(),
                    format: NativeFormat::Dynamic,
                });
            }
            if ext == "a" {
                let canonical = Self::locate_native(base, module_ref).ok_or_else(|| {
                    format!(
                        "cannot resolve module '{module_ref}' from '{from_identity}' \
                         (looked in: {})",
                        Self::searched_list(base)
                    )
                })?;
                #[cfg(feature = "install")]
                Self::verify_locked_module(base, &canonical)?;
                let path = canonical.display().to_string();
                let (prefix, has_vars) = static_module_symbols(&path)?;
                return Ok(ResolvedModule::Native {
                    identity: path.clone(),
                    path,
                    format: NativeFormat::Static { prefix, has_vars },
                });
            }
            if ext == "wasm" {
                return Err(format!(
                    "cannot link '{module_ref}': .{ext} native modules aren't supported yet \
                     (see docs/todo/native-module-linking.md) — only .so, .a and .code are"
                ));
            }
        }

        for root in Self::candidate_roots(base) {
            let direct = root.join(module_ref);
            if direct.is_file() {
                return Self::read(&direct);
            }
            // The `.code` extension is optional, so `link "modules/shared_values"`
            // and `link "modules/shared_values.code"` name the same file.
            let suffixed: PathBuf = root.join(format!("{module_ref}.code"));
            if suffixed.is_file() {
                return Self::read(&suffixed);
            }
        }
        Err(format!(
            "cannot resolve module '{module_ref}' from '{from_identity}' (looked in: {})",
            Self::searched_list(base)
        ))
    }
}

/// The nearest ancestor of `dir` (including itself) containing a `.code`
/// directory — the project boundary for installed modules. Walk-up semantics
/// per `docs/todo/community-modules.md`: stop at the nearest `.code/`, like
/// `node_modules` resolution stops at the nearest `package.json`. Shared by
/// the resolver's fallback chain and `module_install`'s layout, so the two
/// can never disagree about what "this project" means.
pub fn find_project_code_dir(dir: &Path) -> Option<PathBuf> {
    let mut cursor = Some(dir);
    while let Some(d) = cursor {
        let candidate = d.join(".code");
        if candidate.is_dir() {
            return Some(candidate);
        }
        cursor = d.parent();
    }
    None
}

/// Finds a `.a` module's chosen prefix by reading its symbol table with
/// `nm` — the only way to discover it, since (unlike `.so`'s fixed
/// `code_module_dispatch`, resolved per-handle at `dlopen` time) a `.a`'s
/// entry points must be uniquely named to survive being linked into one
/// flat symbol table alongside every other `.a` in the same program (see
/// `code_abi.h`'s "`.a` static modules" section). Returns the prefix and
/// whether `<prefix>_code_module_vars` is also present (optional, exactly
/// like `.so`'s `code_module_vars`).
///
/// Read-only introspection, run at `link` time in both output modes (a
/// `code run` of a program that links a `.a` still fails, but from
/// `interpreter.rs` refusing a `Static` `ImportNative`, not from a missing
/// prefix) — consistent with the project's existing reliance on a system
/// toolchain (`cc`); `nm` ships with the same binutils.
fn static_module_symbols(path: &str) -> Result<(String, bool), String> {
    let output = Command::new("nm")
        .arg("--defined-only")
        .arg("-g")
        .arg(path)
        .output()
        .map_err(|e| format!("cannot read symbols of '{path}': failed to run nm: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot read symbols of '{path}': nm exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let names: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .collect();

    let suffix = "_code_module_dispatch";
    let matches: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| *name != suffix && name.ends_with(suffix))
        .collect();
    let prefix = match matches.as_slice() {
        [] => {
            return Err(format!(
                "cannot link '{path}': no symbol ending in '{suffix}' found — a .a module must \
                 export '<prefix>{suffix}' (see code_abi.h's \".a static modules\" section)"
            ));
        }
        [name] => name.trim_end_matches(suffix).to_string(),
        _ => {
            return Err(format!(
                "cannot link '{path}': more than one symbol ends in '{suffix}' ({}) — a .a \
                 module's prefix must be unique",
                matches.join(", ")
            ));
        }
    };

    let version_symbol = format!("{prefix}_code_module_abi_version");
    if !names.contains(&version_symbol.as_str()) {
        return Err(format!(
            "cannot link '{path}': missing '{version_symbol}' (found '{prefix}{suffix}', but a \
             .a module needs both)"
        ));
    }
    let has_vars = names.contains(&format!("{prefix}_code_module_vars").as_str());

    Ok((prefix, has_vars))
}

/// A resolver for hosts with no module story at all — the wasm playground.
/// Running the entry program works; any `link` in it is refused with a reason
/// rather than a missing-file error.
pub struct NoModules {
    pub entry_identity: String,
    pub entry_text: String,
}

impl ModuleResolver for NoModules {
    fn resolve_entry(&self, _entry: &str) -> Result<ResolvedModule, String> {
        Ok(ResolvedModule::Source {
            identity: self.entry_identity.clone(),
            text: self.entry_text.clone(),
        })
    }

    fn resolve(&self, _from: &str, module_ref: &str) -> Result<ResolvedModule, String> {
        Err(format!(
            "cannot link '{module_ref}': modules are not available in this environment"
        ))
    }
}

/// A module identity, shortened for reading. Identities are canonical
/// absolute paths because that is what cycle detection and relative
/// resolution need — but an absolute path is mostly noise in an error
/// message, so anything under the working directory is shown relative to it,
/// the way a compiler normally echoes the path you gave it. Falls back to the
/// full identity whenever that isn't possible: a module in another directory,
/// or a host with no working directory at all (wasm).
fn display_path(identity: &str) -> &str {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = Path::new(identity).strip_prefix(&cwd) {
            if let Some(shown) = rel.to_str() {
                return shown;
            }
        }
    }
    identity
}

/// Load `entry` through `resolver`, returning a program with no `Link` left
/// in it.
pub fn load(entry: &str, resolver: &dyn ModuleResolver) -> Result<Program, String> {
    // The program being run is always `.code` source — a native module can
    // only ever be a `link` target, never the entry point.
    let ResolvedModule::Source { identity, text } = resolver.resolve_entry(entry)? else {
        return Err(format!(
            "'{entry}' is a native module — it cannot be run directly"
        ));
    };
    let mut loader = Loader {
        resolver,
        // The entry goes on the stack too, so a module that links the program
        // back is caught as a cycle rather than loaded a second time.
        visiting: vec![identity.clone()],
    };
    loader.load_source(&identity, &text)
}

struct Loader<'a> {
    resolver: &'a dyn ModuleResolver,
    /// Identities currently being loaded, outermost first. A reference that
    /// is already in here is a cycle; keeping the whole stack rather than a
    /// `HashSet` is what lets the error name the loop.
    visiting: Vec<String>,
}

impl Loader<'_> {
    fn load_source(&mut self, identity: &str, text: &str) -> Result<Program, String> {
        // The only place that holds a module's source text *and* the name to
        // call it by, so it is where a located error becomes a rendered one.
        // Both backends see plain strings from here on — see `span`'s doc
        // comment.
        let shown = display_path(identity);
        let locate = |e: span::Located| span::render(text, shown, e.at, &e.msg);
        let lexed = lexer::tokenize(text).map_err(locate)?;
        let program = parser::parse(&lexed).map_err(locate)?;

        let mut statements = Vec::with_capacity(program.statements.len());
        for stmt in program.statements {
            // Only the top level is scanned, never inside a block — the
            // parser guarantees `link` appears nowhere else.
            match stmt {
                Stmt::Link { path, alias } => {
                    statements.push(self.resolve_link(identity, &path, alias)?);
                }
                other => statements.push(other),
            }
        }
        Ok(Program { statements })
    }

    fn resolve_link(
        &mut self,
        from_identity: &str,
        path: &str,
        alias: Option<String>,
    ) -> Result<Stmt, String> {
        let resolved = self.resolver.resolve(from_identity, path)?;
        let (identity, text) = match resolved {
            ResolvedModule::Source { identity, text } => (identity, text),
            ResolvedModule::Native {
                path: native_path,
                format,
                ..
            } => {
                let alias = alias.ok_or_else(|| {
                    format!(
                        "link \"{path}\" needs an alias (e.g. link \"{path}\" as m) — \
                         nothing else could name it in 'emit ... to <name>'"
                    )
                })?;
                return Ok(Stmt::ImportNative {
                    alias,
                    path: native_path,
                    format,
                });
            }
        };

        if self.visiting.iter().any(|seen| seen == &identity) {
            let mut chain: Vec<&str> = self.visiting.iter().map(String::as_str).collect();
            chain.push(&identity);
            return Err(format!("circular link: {}", chain.join(" -> ")));
        }

        self.visiting.push(identity.clone());
        let loaded = self.load_source(&identity, &text);
        self.visiting.pop();
        let module = loaded?;

        // Only this module's own `export let`s. A nested `Import` in the body
        // contributes nothing: linking is not re-exporting.
        let exports = module
            .statements
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::Let {
                    name,
                    exported: true,
                    ..
                } => Some(name.clone()),
                _ => None,
            })
            .collect();

        Ok(Stmt::Import {
            alias,
            body: module.statements,
            exports,
        })
    }
}

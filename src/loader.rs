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

use crate::ast::{Program, Stmt};
use crate::{lexer, parser};

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
    /// A native `.so` module — see `docs/todo/native-module-linking.md` and
    /// `code_abi.h`. Carries a real filesystem path (not source text):
    /// loading it means `dlopen`, done by the interpreter/codegen, not here
    /// — this module stays pure AST/path work, like the `Source` case.
    Native { identity: String, path: String },
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

/// Reads modules from the real filesystem. A reference resolves **only**
/// against the directory of the file doing the linking — there is no search
/// path and no environment variable, so where a module comes from is always
/// answerable by looking at the two files involved.
pub struct FilesystemResolver;

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
        // static linking, see runtime.c's `code_native_open`). `.a`/`.wasm`
        // are named there as future formats, not built yet.
        if let Some(ext) = Path::new(module_ref).extension().and_then(|e| e.to_str()) {
            if ext == "so" {
                let direct = base.join(module_ref);
                let canonical = fs::canonicalize(&direct)
                    .map_err(|e| format!("cannot resolve '{}': {e}", direct.display()))?;
                return Ok(ResolvedModule::Native {
                    identity: canonical.display().to_string(),
                    path: canonical.display().to_string(),
                });
            }
            if ext == "a" || ext == "wasm" {
                return Err(format!(
                    "cannot link '{module_ref}': .{ext} native modules aren't supported yet \
                     (see docs/todo/native-module-linking.md) — only .so and .code are"
                ));
            }
        }

        let direct = base.join(module_ref);
        if direct.is_file() {
            return Self::read(&direct);
        }
        // The `.code` extension is optional, so `link "modules/shared_values"`
        // and `link "modules/shared_values.code"` name the same file.
        let suffixed: PathBuf = base.join(format!("{module_ref}.code"));
        if suffixed.is_file() {
            return Self::read(&suffixed);
        }
        Err(format!(
            "cannot resolve module '{module_ref}' from '{from_identity}'"
        ))
    }
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
        let tokens = lexer::tokenize(text)?;
        let program = parser::parse(&tokens)?;

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
                path: native_path, ..
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

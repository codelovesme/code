//! Records the real loader inputs, including locks and resolution candidates.
use crate::loader::{FilesystemResolver, LinkPolicy, ModuleResolver, ResolvedModule};
use crate::run_cache::{absolute, context, stamp, Inputs};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct Resolver {
    inputs: RefCell<Inputs>,
}

impl Resolver {
    pub(crate) fn new(entry: &Path) -> Option<Self> {
        let entry = absolute(entry)?;
        let compiler = std::env::current_exe().ok()?;
        let this = Self {
            inputs: RefCell::new(Inputs {
                entry: entry.clone(),
                compiler: compiler.clone(),
                context: context()?,
                files: BTreeMap::new(),
            }),
        };
        this.watch(&entry).ok()?;
        this.watch(&compiler).ok()?;
        Some(this)
    }

    fn watch(&self, path: &Path) -> Result<(), String> {
        let path = absolute(path).ok_or("cannot resolve cache input")?;
        let now = stamp(&path)?;
        let mut inputs = self.inputs.borrow_mut();
        if let Some(before) = inputs.files.get(&path) {
            if before != &now {
                return Err("run input changed while loading".into());
            }
        } else {
            inputs.files.insert(path, now);
        }
        Ok(())
    }

    fn guards(&self, base: &Path, reference: Option<&str>) -> Result<(), String> {
        for ancestor in base.ancestors() {
            let project = ancestor.join(".code");
            self.watch(&project)?;
            self.watch(&project.join("lock.json"))?;
            if let Some(reference) = reference {
                self.watch(&project.join("modules").join(reference))?;
            }
        }
        if let Some(reference) = reference {
            self.watch(&base.join(reference))?;
            if let Some(paths) = std::env::var_os("CODE_MODULE_PATH") {
                for root in std::env::split_paths(&paths) {
                    self.watch(&root.join(reference))?;
                }
            }
            if let Some(home) = std::env::var_os("HOME") {
                self.watch(&PathBuf::from(home).join(".code/modules").join(reference))?;
            }
        }
        Ok(())
    }

    fn resolved(&self, result: ResolvedModule) -> Result<ResolvedModule, String> {
        match &result {
            ResolvedModule::Source { identity, text } => {
                self.watch(Path::new(identity))?;
                // Ensure the stamp describes the text the loader actually got,
                // rather than a replacement between its read and our stat.
                if fs::read_to_string(identity).map_err(|e| e.to_string())? != *text {
                    return Err("source changed while loading".into());
                }
            }
            ResolvedModule::Native { path, .. } => self.watch(Path::new(path))?,
        }
        Ok(result)
    }

    pub(crate) fn finish(self) -> Option<Inputs> {
        let inputs = self.inputs.into_inner();
        inputs.matches().then_some(inputs)
    }
}

impl ModuleResolver for Resolver {
    fn resolve_entry(&self, entry: &str) -> Result<ResolvedModule, String> {
        let path = absolute(Path::new(entry)).ok_or("cannot resolve entry")?;
        self.guards(path.parent().ok_or("entry has no parent")?, None)?;
        self.resolved(FilesystemResolver.resolve_entry(entry)?)
    }

    fn resolve(&self, from: &str, reference: &str) -> Result<ResolvedModule, String> {
        let base = Path::new(from).parent().ok_or("source has no parent")?;
        self.guards(base, Some(reference))?;
        // What `code run` builds is an executable, so a module identity is
        // what an executable links it as (ticket 142) — and every file it
        // could resolve to is a candidate whose appearing changes the build.
        let policy = LinkPolicy::for_target(crate::BuildTarget::Exe);
        if crate::loader::is_identity(reference) {
            for candidate in policy.candidates(reference) {
                self.guards(base, Some(&candidate))?;
            }
        }
        let resolved = FilesystemResolver::resolve_with(from, reference, policy)?;
        if let ResolvedModule::Native { path, .. } = &resolved {
            // A pin's asset path is known only after resolving its lock. Guard
            // it before the verification/symbol scan used by this build, then
            // check it again afterwards. A reinstall during loading is a miss.
            self.watch(Path::new(path))?;
            // An archive's link flags are part of what the program is made
            // of: a changed `.libs` is a different link (ticket 142).
            self.watch(Path::new(&format!("{path}.libs")))?;
            self.resolved(FilesystemResolver::resolve_with(from, reference, policy)?)
        } else {
            self.resolved(resolved)
        }
    }
}

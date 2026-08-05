use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chumsky::Parser;

use crate::ast::{ConstraintExpr, HandlerInfo, Program, Spanned, Statement, TypeInfo};
#[cfg(feature = "native-so")]
use crate::native_module;
use crate::wasm_module;
use crate::parser;

/// Abstracts where `.code` module SOURCE text comes from, so the loader can
/// run against a real filesystem (CLI) or an in-memory source map (e.g. a
/// browser/WASM host with no filesystem) without duplicating cycle detection,
/// span-shifting, or parsing logic. Only source (`.code`) modules go through
/// this — native `.so`/`.wasm` module loading is unaffected (already
/// filesystem-only and feature-gated; out of scope for non-filesystem hosts,
/// see T19).
pub trait ModuleResolver {
    /// Resolve the entry module. Returns `(identity, source)`.
    /// `identity` is an opaque, resolver-defined string that uniquely
    /// identifies this module — used for cycle detection and as the
    /// diagnostics file name. The filesystem resolver uses the canonicalized
    /// path.
    fn resolve_entry(&self, entry: &str) -> Result<(String, String), String>;

    /// Resolve `module_ref` as linked from the module with identity
    /// `from_identity`. Returns `(identity, source)`.
    fn resolve(&self, from_identity: &str, module_ref: &str) -> Result<(String, String), String>;
}

/// The CLI's resolver: reads `.code` modules from the real filesystem.
/// Search order for a bare module reference: the importing file's directory,
/// then the current working directory, then `CODE_PATH` (colon-separated).
pub struct FilesystemResolver;

impl ModuleResolver for FilesystemResolver {
    fn resolve_entry(&self, entry: &str) -> Result<(String, String), String> {
        let path = Path::new(entry);
        let canonical = fs::canonicalize(path)
            .map_err(|e| format!("Error resolving '{}': {}", path.display(), e))?;
        let source = fs::read_to_string(&canonical)
            .map_err(|e| format!("Error reading '{}': {}", canonical.display(), e))?;
        Ok((canonical.display().to_string(), source))
    }

    fn resolve(&self, from_identity: &str, module_ref: &str) -> Result<(String, String), String> {
        let module_path = resolve_source_module_path(Path::new(from_identity), module_ref)?;
        let canonical = fs::canonicalize(&module_path)
            .map_err(|e| format!("Error resolving '{}': {}", module_path.display(), e))?;
        let source = fs::read_to_string(&canonical)
            .map_err(|e| format!("Error reading '{}': {}", canonical.display(), e))?;
        Ok((canonical.display().to_string(), source))
    }
}

/// Load a source program from the real filesystem, recursively resolving all
/// `link` statements. Returns a `Program` where every `Link` has been
/// replaced by an `Import` node.
/// - `link path` (no alias) produces `Import { alias: None, ... }` (flatten mode).
/// - `link path as x` produces `Import { alias: Some("x"), ... }` (namespace mode).
pub fn load_program_with_links(entry: &Path) -> Result<(Program, SourceMap), String> {
    load_program_with_resolver(&entry.display().to_string(), &FilesystemResolver)
}

/// Load a source program using a custom [`ModuleResolver`] — e.g. an
/// in-memory source map for a host with no filesystem (a browser/WASM
/// playground). Same semantics as [`load_program_with_links`], generalized
/// over where module source text comes from.
pub fn load_program_with_resolver(
    entry: &str,
    resolver: &dyn ModuleResolver,
) -> Result<(Program, SourceMap), String> {
    let mut loader = ModuleLoader::new(resolver);
    let program = loader.load_entry(entry)?;
    Ok((program, loader.source_map))
}

/// A registered source file within the [`SourceMap`].
struct SourceFile {
    /// Global char offset where this file's spans begin.
    base: usize,
    /// Length of `source` in chars.
    len: usize,
    path: String,
    source: String,
}

/// Maps global char offsets (as carried on statement spans) back to the file and
/// local offset they came from, so linked multi-file programs can render located
/// diagnostics against the right source.
pub struct SourceMap {
    files: Vec<SourceFile>,
    next_base: usize,
}

impl SourceMap {
    fn new() -> Self {
        SourceMap { files: Vec::new(), next_base: 0 }
    }

    /// Register a file and return the global base offset assigned to it.
    fn add(&mut self, path: String, source: String) -> usize {
        let base = self.next_base;
        let len = source.chars().count();
        // +1 gap so an at-EOF offset of one file can't collide with the next.
        self.next_base = base + len + 1;
        self.files.push(SourceFile { base, len, path, source });
        base
    }

    /// Render a rustc-style diagnostic for a global char range, or `None` if the
    /// range doesn't resolve to a registered file.
    pub fn render(&self, start: usize, end: usize, message: &str) -> Option<String> {
        let f = self
            .files
            .iter()
            .find(|f| start >= f.base && start <= f.base + f.len)?;
        let local_start = start - f.base;
        let local_end = end.saturating_sub(f.base).min(f.len);
        Some(crate::diagnostics::render(
            &f.source,
            &f.path,
            local_start,
            local_end,
            message,
        ))
    }
}

/// Recursively add `base` to every statement span (and nested body spans).
fn shift_spans(stmts: &mut [Spanned<Statement>], base: usize) {
    for s in stmts {
        s.span.start += base;
        s.span.end += base;
        match &mut s.node {
            Statement::Block(body)
            | Statement::If { body, .. }
            | Statement::LoopOver { body, .. }
            | Statement::LoopInfinite { body, .. }
            | Statement::HandlerDefinition { body, .. } => shift_spans(body, base),
            _ => {}
        }
    }
}

struct ModuleLoader<'a> {
    resolver: &'a dyn ModuleResolver,
    /// Stack of module identities currently being loaded (for cycle detection).
    visiting: Vec<String>,
    /// Accumulated source files for located diagnostics.
    source_map: SourceMap,
}

impl<'a> ModuleLoader<'a> {
    fn new(resolver: &'a dyn ModuleResolver) -> Self {
        Self {
            resolver,
            visiting: Vec::new(),
            source_map: SourceMap::new(),
        }
    }

    fn load_entry(&mut self, entry: &str) -> Result<Program, String> {
        let (identity, source) = self.resolver.resolve_entry(entry)?;
        self.load(identity, source)
    }

    fn load(&mut self, identity: String, source: String) -> Result<Program, String> {
        // Circular dependency check
        if let Some(idx) = self.visiting.iter().position(|p| p == &identity) {
            let mut chain: Vec<String> = self.visiting[idx..].to_vec();
            chain.push(identity.clone());
            return Err(format!("Circular link detected: {}", chain.join(" -> ")));
        }

        let (parsed, parse_errors) = parser::parser().parse_recovery(source.as_str());

        if !parse_errors.is_empty() {
            let rendered: Vec<String> = parse_errors
                .iter()
                .map(|err| {
                    // Use the custom reason if present, otherwise the default
                    // Display which lists expected/found tokens.
                    let detail = match err.reason() {
                        chumsky::error::SimpleReason::Custom(s) => s.clone(),
                        _ => format!("{}", err),
                    };
                    let span = err.span();
                    crate::diagnostics::render(&source, &identity, span.start, span.end, &detail)
                })
                .collect();
            return Err(rendered.join("\n\n"));
        }

        let mut parsed = parsed.expect("Parser produced no output despite no errors");

        // Register this file in the shared SourceMap and shift its statement
        // spans by the assigned base, so every span self-identifies its file
        // (enabling located diagnostics across linked modules).
        let base = self.source_map.add(identity.clone(), source);
        shift_spans(&mut parsed.statements, base);

        self.visiting.push(identity.clone());

        // Track which references are linked in this file (duplicate detection).
        // Shared between native and source refs — both dedupe by their own
        // resolved identity/canonical-path string, which never collide across
        // kinds since they're always distinct paths.
        let mut linked_in_file: HashSet<String> = HashSet::new();

        let mut statements = Vec::new();
        for stmt in parsed.statements {
            let Spanned { node, span } = stmt;
            match node {
                Statement::Link { module_ref, alias } => {
                    if is_native_extension(&module_ref) {
                        // --- Native module (.so / .a / .wasm) ---
                        // Unaffected by the resolver abstraction: always
                        // filesystem-based (already feature-gated), out of
                        // scope for non-filesystem hosts (T19).

                        // Static libraries cannot be loaded at runtime.
                        if module_ref.ends_with(".a") {
                            return Err(format!(
                                "Static libraries (.a) cannot be loaded at runtime. \
                                 Use a shared library (.so) instead: '{}'",
                                module_ref
                            ));
                        }

                        let current_file = Path::new(&identity);
                        let lib_path = resolve_native_module_path(current_file, &module_ref)?;
                        let lib_canonical = fs::canonicalize(&lib_path)
                            .map_err(|e| format!("Error resolving '{}': {}", lib_path.display(), e))?;
                        let lib_identity = lib_canonical.display().to_string();

                        if !linked_in_file.insert(lib_identity) {
                            return Err(format!(
                                "Module '{}' is linked more than once in the same file",
                                module_ref
                            ));
                        }

                        let is_wasm = module_ref.ends_with(".wasm");

                        #[cfg(feature = "native-so")]
                        let native_mod = if is_wasm {
                            wasm_module::load_wasm_module(&lib_path)?
                        } else {
                            native_module::load_native_module(&lib_path)?
                        };
                        #[cfg(not(feature = "native-so"))]
                        let native_mod = if is_wasm {
                            wasm_module::load_wasm_module(&lib_path)?
                        } else {
                            return Err(format!(
                                "Native '.so' module loading is not available in this build \
                                 (missing the `native-so` feature): '{}'",
                                module_ref
                            ));
                        };

                        statements.push(Spanned::new(Statement::NativeImport {
                            alias,
                            native_path: lib_canonical.to_string_lossy().to_string(),
                            is_wasm,
                            vars: native_mod.vars,
                            handlers: native_mod.handlers,
                            types: native_mod.types,
                            emissions: native_mod.emissions.iter().map(|e| {
                                crate::ast::EmissionDecl {
                                    class_name: e.class_name.clone(),
                                    target: e.target.clone(),
                                }
                            }).collect(),
                            emit_queue: native_mod.emit_queue,
                        }, span));
                    } else {
                        // --- Source module (.code) ---
                        // Duplicate link check within the same file. We need the
                        // resolved identity up front, so resolve once here and
                        // reuse it for load_linked below (avoids re-resolving).
                        let (module_identity, module_source) =
                            self.resolver.resolve(&identity, &module_ref)?;

                        if !linked_in_file.insert(module_identity.clone()) {
                            return Err(format!(
                                "Module '{}' is linked more than once in the same file",
                                module_ref
                            ));
                        }

                        // Recursively load the linked module.
                        let module_program = self.load(module_identity, module_source)?;

                        // Collect public names (all top-level definitions minus private ones).
                        let (body, public_names, public_types, public_handlers) = collect_public_names(module_program.statements);

                        statements.push(Spanned::new(Statement::Import {
                            alias,
                            body,
                            public_names,
                            public_types,
                            public_handlers,
                        }, span));
                    }
                }
                other => statements.push(Spanned::new(other, span)),
            }
        }

        self.visiting.pop();

        Ok(Program { statements })
    }
}

/// Resolve a `.code` module name to a file path.
/// Search order: directory of the importing file, then cwd, then CODE_PATH.
fn resolve_source_module_path(current_file: &Path, module_name: &str) -> Result<PathBuf, String> {
    // If extension is explicit, require `.code`.
    // Otherwise, resolve to `<module>.code`.
    let file_name = if module_name.ends_with(".code") {
        module_name.to_string()
    } else if Path::new(module_name)
        .extension()
        .is_some_and(|ext| ext != "code")
    {
        return Err(format!(
            "Linked module '{}' uses unsupported extension. Use '.code'",
            module_name
        ));
    } else {
        format!("{}.code", module_name)
    };

    let mut candidates = Vec::new();

    // 1. Relative to the importing file's directory
    if let Some(parent) = current_file.parent() {
        candidates.push(parent.join(&file_name));
    }

    // 2. Relative to the current working directory
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(&file_name));
    }

    // 3. CODE_PATH directories
    let search_paths = std::env::var("CODE_PATH").ok();
    if let Some(paths) = search_paths {
        for base in paths.split(':').filter(|s| !s.is_empty()) {
            candidates.push(Path::new(base).join(&file_name));
        }
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    Err(format!(
        "Linked module '{}' not found (looked for '{}')",
        module_name,
        file_name
    ))
}

/// Resolve a native module path (.so / .a).
/// The module_ref is used as-is (with its extension).
/// Search order: directory of the importing file, then cwd, then CODE_PATH.
fn resolve_native_module_path(current_file: &Path, module_ref: &str) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();

    // 1. Relative to the importing file's directory
    if let Some(parent) = current_file.parent() {
        candidates.push(parent.join(module_ref));
    }

    // 2. Relative to the current working directory
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(module_ref));
    }

    // 3. CODE_PATH directories
    let search_paths = std::env::var("CODE_PATH").ok();
    if let Some(paths) = search_paths {
        for base in paths.split(':').filter(|s| !s.is_empty()) {
            candidates.push(Path::new(base).join(module_ref));
        }
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    Err(format!(
        "Native module '{}' not found",
        module_ref
    ))
}

/// Check whether a module reference points to a native library by extension.
fn is_native_extension(module_ref: &str) -> bool {
    module_ref.ends_with(".so") || module_ref.ends_with(".a") || module_ref.ends_with(".wasm")
}

/// Scan a module's resolved statements, determine public names and types, and convert
/// private constraints into non-private forms in the body (visibility
/// is tracked separately via the public_names list).
///
/// Public names = all top-level equality constraint names that are NOT also
/// declared via private constraints.
/// `Import` nodes that were flattened (alias=None) contribute their public_names
/// to this module's public set as well.
fn collect_public_names(
    statements: Vec<Spanned<Statement>>,
) -> (Vec<Spanned<Statement>>, Vec<String>, Vec<TypeInfo>, Vec<HandlerInfo>) {
    let mut body = Vec::new();
    let mut pub_names: Vec<String> = Vec::new();
    let mut pub_types: Vec<TypeInfo> = Vec::new();
    let mut pub_handlers: Vec<HandlerInfo> = Vec::new();
    let mut private_names: HashSet<String> = HashSet::new();

    for stmt in statements {
        let Spanned { node, span } = stmt;
        match node {
            Statement::Constraint { variable, constraint, private: true } => {
                private_names.insert(variable.clone());
                // Keep it in the body as a normal (non-private) constraint so it executes.
                body.push(Spanned::new(
                    Statement::Constraint { variable, constraint, private: false },
                    span,
                ));
            }
            Statement::Constraint { variable, constraint, private: false } => {
                // Only equality constraints define a name for public export.
                if matches!(constraint, ConstraintExpr::Equals(_))
                    && !private_names.contains(&variable)
                {
                    pub_names.push(variable.clone());
                }
                body.push(Spanned::new(
                    Statement::Constraint { variable, constraint, private: false },
                    span,
                ));
            }
            Statement::Import {
                alias,
                body: import_body,
                public_names,
                public_types,
                public_handlers,
            } => {
                if alias.is_none() {
                    for n in &public_names {
                        if !private_names.contains(n) {
                            pub_names.push(n.clone());
                        }
                    }
                    for t in &public_types {
                        pub_types.push(t.clone());
                    }
                    for h in &public_handlers {
                        pub_handlers.push(h.clone());
                    }
                }
                body.push(Spanned::new(
                    Statement::Import { alias, body: import_body, public_names, public_types, public_handlers },
                    span,
                ));
            }
            Statement::HandlerDefinition { class_name, body: handler_body } => {
                pub_handlers.push(HandlerInfo {
                    class_name: class_name.clone(),
                    body: handler_body.clone(),
                });
                body.push(Spanned::new(
                    Statement::HandlerDefinition { class_name, body: handler_body },
                    span,
                ));
            }
            other => body.push(Spanned::new(other, span)),
        }
    }

    (body, pub_names, pub_types, pub_handlers)
}

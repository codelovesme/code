use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chumsky::Parser;

use crate::ast::{ConstraintExpr, HandlerInfo, Program, Spanned, Statement, TypeInfo};
#[cfg(feature = "native-so")]
use crate::native_module;
use crate::wasm_module;
use crate::parser;

/// Load a source program, recursively resolving all `link` statements.
/// Returns a `Program` where every `Link` has been replaced by an `Import` node.
/// - `link path` (no alias) produces `Import { alias: None, ... }` (flatten mode).
/// - `link path as x` produces `Import { alias: Some("x"), ... }` (namespace mode).
pub fn load_program_with_links(entry: &Path) -> Result<(Program, SourceMap), String> {
    let mut loader = ModuleLoader::new();
    let program = loader.load(entry)?;
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

struct ModuleLoader {
    /// Stack of files currently being loaded (for cycle detection).
    visiting: Vec<PathBuf>,
    /// Accumulated source files for located diagnostics.
    source_map: SourceMap,
}

impl ModuleLoader {
    fn new() -> Self {
        Self {
            visiting: Vec::new(),
            source_map: SourceMap::new(),
        }
    }

    fn load(&mut self, path: &Path) -> Result<Program, String> {
        let canonical = fs::canonicalize(path)
            .map_err(|e| format!("Error resolving '{}': {}", path.display(), e))?;

        // Circular dependency check
        if let Some(idx) = self.visiting.iter().position(|p| p == &canonical) {
            let mut chain: Vec<String> = self.visiting[idx..]
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            chain.push(canonical.display().to_string());
            return Err(format!("Circular link detected: {}", chain.join(" -> ")));
        }

        let source = fs::read_to_string(&canonical)
            .map_err(|e| format!("Error reading '{}': {}", canonical.display(), e))?;

        let (parsed, parse_errors) = parser::parser().parse_recovery(source.as_str());

        if !parse_errors.is_empty() {
            let file = canonical.display().to_string();
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
                    crate::diagnostics::render(&source, &file, span.start, span.end, &detail)
                })
                .collect();
            return Err(rendered.join("\n\n"));
        }

        let mut parsed = parsed.expect("Parser produced no output despite no errors");

        // Register this file in the shared SourceMap and shift its statement
        // spans by the assigned base, so every span self-identifies its file
        // (enabling located diagnostics across linked modules).
        let base = self.source_map.add(canonical.display().to_string(), source);
        shift_spans(&mut parsed.statements, base);

        self.visiting.push(canonical.clone());

        // Track which modules are linked in this file (duplicate detection).
        let mut linked_in_file: HashSet<PathBuf> = HashSet::new();

        let mut statements = Vec::new();
        for stmt in parsed.statements {
            let Spanned { node, span } = stmt;
            match node {
                Statement::Link { module_ref, alias } => {
                    if is_native_extension(&module_ref) {
                        // --- Native module (.so / .a / .wasm) ---

                        // Static libraries cannot be loaded at runtime.
                        if module_ref.ends_with(".a") {
                            return Err(format!(
                                "Static libraries (.a) cannot be loaded at runtime. \
                                 Use a shared library (.so) instead: '{}'",
                                module_ref
                            ));
                        }

                        let lib_path = self.resolve_native_module(&canonical, &module_ref)?;
                        let lib_canonical = fs::canonicalize(&lib_path)
                            .map_err(|e| format!("Error resolving '{}': {}", lib_path.display(), e))?;

                        if !linked_in_file.insert(lib_canonical.clone()) {
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
                        let module_path = self.resolve_module(&canonical, &module_ref)?;
                        let module_canonical = fs::canonicalize(&module_path)
                            .map_err(|e| format!("Error resolving '{}': {}", module_path.display(), e))?;

                        // Duplicate link check within the same file.
                        if !linked_in_file.insert(module_canonical.clone()) {
                            return Err(format!(
                                "Module '{}' is linked more than once in the same file",
                                module_ref
                            ));
                        }

                        // Recursively load the linked module.
                        let module_program = self.load(&module_path)?;

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

    /// Resolve a module name to a file path.
    /// Search order: directory of the importing file, then cwd, then CODE_PATH.
    fn resolve_module(&self, current_file: &Path, module_name: &str) -> Result<PathBuf, String> {
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
    fn resolve_native_module(&self, current_file: &Path, module_ref: &str) -> Result<PathBuf, String> {
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
            Statement::TypeDeclaration { name, fields } => {
                pub_types.push(TypeInfo { name: name.clone(), fields: fields.clone() });
                body.push(Spanned::new(Statement::TypeDeclaration { name, fields }, span));
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
            Statement::HandlerDefinition { class_name, inline_type, body: handler_body } => {
                pub_handlers.push(HandlerInfo {
                    class_name: class_name.clone(),
                    body: handler_body.clone(),
                });
                // Propagate inline handler types so they survive scope pop in compile_import.
                if let Some(ref fields) = inline_type {
                    pub_types.push(TypeInfo { name: class_name.clone(), fields: fields.clone() });
                }
                body.push(Spanned::new(
                    Statement::HandlerDefinition { class_name, inline_type, body: handler_body },
                    span,
                ));
            }
            other => body.push(Spanned::new(other, span)),
        }
    }

    (body, pub_names, pub_types, pub_handlers)
}

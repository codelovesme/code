use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chumsky::Parser;

use crate::ast::{ConstraintExpr, HandlerInfo, Program, Statement, TypeInfo};
use crate::native_module;
use crate::wasm_module;
use crate::parser;

/// Load a source program, recursively resolving all `link` statements.
/// Returns a `Program` where every `Link` has been replaced by an `Import` node.
/// - `link path` (no alias) produces `Import { alias: None, ... }` (flatten mode).
/// - `link path as x` produces `Import { alias: Some("x"), ... }` (namespace mode).
pub fn load_program_with_links(entry: &Path) -> Result<Program, String> {
    let mut loader = ModuleLoader::new();
    loader.load(entry)
}

struct ModuleLoader {
    /// Stack of files currently being loaded (for cycle detection).
    visiting: Vec<PathBuf>,
}

impl ModuleLoader {
    fn new() -> Self {
        Self {
            visiting: Vec::new(),
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
            let mut msg = format!("Parse errors in '{}':", canonical.display());
            // Convert char offsets to line:col positions for user-friendly messages.
            let line_starts: Vec<usize> = std::iter::once(0)
                .chain(source.match_indices('\n').map(|(i, _)| i + 1))
                .collect();

            for err in &parse_errors {
                let offset = err.span().start;
                // Binary-search for the line that contains this offset.
                let line_idx = line_starts.partition_point(|&s| s <= offset).saturating_sub(1);
                let col = offset - line_starts[line_idx];
                let line_num = line_idx + 1;

                // Use the custom reason if present, otherwise fall back to the
                // default Display which lists expected/found tokens.
                let detail = match err.reason() {
                    chumsky::error::SimpleReason::Custom(s) => s.clone(),
                    _ => format!("{}", err),
                };
                msg.push_str(&format!("\n  [{}:{}] {}", line_num, col + 1, detail));
            }
            return Err(msg);
        }

        let parsed = parsed.expect("Parser produced no output despite no errors");

        self.visiting.push(canonical.clone());

        // Track which modules are linked in this file (duplicate detection).
        let mut linked_in_file: HashSet<PathBuf> = HashSet::new();

        let mut statements = Vec::new();
        for stmt in parsed.statements {
            match stmt {
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
                        let native_mod = if is_wasm {
                            wasm_module::load_wasm_module(&lib_path)?
                        } else {
                            native_module::load_native_module(&lib_path)?
                        };

                        statements.push(Statement::NativeImport {
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
                        });
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

                        statements.push(Statement::Import {
                            alias,
                            body,
                            public_names,
                            public_types,
                            public_handlers,
                        });
                    }
                }
                other => statements.push(other),
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
fn collect_public_names(statements: Vec<Statement>) -> (Vec<Statement>, Vec<String>, Vec<TypeInfo>, Vec<HandlerInfo>) {
    let mut body = Vec::new();
    let mut pub_names: Vec<String> = Vec::new();
    let mut pub_types: Vec<TypeInfo> = Vec::new();
    let mut pub_handlers: Vec<HandlerInfo> = Vec::new();
    let mut private_names: HashSet<String> = HashSet::new();

    for stmt in statements {
        match stmt {
            Statement::Constraint { ref variable, private: true, .. } => {
                private_names.insert(variable.clone());
                // Convert to a non-private constraint in the body so it executes normally.
                let Statement::Constraint { variable, constraint, .. } = stmt else { unreachable!() };
                body.push(Statement::Constraint { variable, constraint, private: false });
            }
            Statement::Constraint { ref variable, private: false, .. } => {
                // Only equality constraints define a "name" for public export.
                if let Statement::Constraint { ref constraint, .. } = stmt {
                    if matches!(constraint, ConstraintExpr::Equals(_)) && !private_names.contains(variable) {
                        pub_names.push(variable.clone());
                    }
                }
                body.push(stmt);
            }
            Statement::TypeDeclaration { ref name, ref fields } => {
                pub_types.push(TypeInfo {
                    name: name.clone(),
                    fields: fields.clone(),
                });
                body.push(stmt);
            }
            Statement::Import { alias: None, ref public_names, ref public_types, ref public_handlers, .. } => {
                for n in public_names {
                    if !private_names.contains(n) {
                        pub_names.push(n.clone());
                    }
                }
                for t in public_types {
                    pub_types.push(t.clone());
                }
                for h in public_handlers {
                    pub_handlers.push(h.clone());
                }
                body.push(stmt);
            }
            Statement::HandlerDefinition { ref class_name, ref inline_type, body: ref handler_body } => {
                pub_handlers.push(HandlerInfo {
                    class_name: class_name.clone(),
                    body: handler_body.clone(),
                });
                // Propagate inline handler types so they survive scope pop in compile_import.
                if let Some(fields) = inline_type {
                    pub_types.push(TypeInfo {
                        name: class_name.clone(),
                        fields: fields.clone(),
                    });
                }
                body.push(stmt);
            }
            other => body.push(other),
        }
    }

    (body, pub_names, pub_types, pub_handlers)
}

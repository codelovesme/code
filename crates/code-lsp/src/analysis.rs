//! Non-executing source analysis. Open buffers override disk; native targets
//! remain opaque aliases, never dlopened or inspected with external tools.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use code::ast::{NativeFormat, Program, Span, Stmt};
use code::loader::{FilesystemResolver, ModuleResolver, ResolvedModule};
use tower_lsp::lsp_types::{Diagnostic, Url};

use crate::diagnostics;

#[derive(Clone)]
pub struct Document {
    pub text: String,
    pub version: i32,
}

pub type Report = HashMap<Url, Vec<Diagnostic>>;

fn identity(path: &Path) -> String {
    if let Ok(path) = path.canonicalize() {
        return path.to_string_lossy().into_owned();
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut clean = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }
    clean.to_string_lossy().into_owned()
}

fn document_identity(uri: &Url) -> String {
    uri.to_file_path()
        .map(|p| identity(&p))
        .unwrap_or_else(|_| uri.to_string())
}

struct Resolver<'a> {
    buffers: HashMap<String, &'a str>,
    uris: HashMap<String, Url>,
    sources: RefCell<HashMap<String, String>>,
    problems: RefCell<Report>,
}

impl<'a> Resolver<'a> {
    fn new(documents: &'a HashMap<Url, Document>) -> Self {
        Self {
            buffers: documents
                .iter()
                .map(|(uri, doc)| (document_identity(uri), doc.text.as_str()))
                .collect(),
            uris: documents
                .keys()
                .map(|uri| (document_identity(uri), uri.clone()))
                .collect(),
            sources: RefCell::new(HashMap::new()),
            problems: RefCell::new(HashMap::new()),
        }
    }

    fn uri(&self, name: &str) -> Option<Url> {
        if let Some(uri) = self.uris.get(name) {
            return Some(uri.clone());
        }
        if !Path::new(name).is_absolute() {
            if let Ok(uri) = Url::parse(name) {
                if uri.scheme() != "file" {
                    return Some(uri);
                }
            }
        }
        let name = identity(Path::new(name));
        self.uris
            .get(&name)
            .cloned()
            .or_else(|| Url::from_file_path(name).ok())
    }

    fn source(&self, name: String, disk_text: Option<String>) -> Result<ResolvedModule, String> {
        let text = match self
            .buffers
            .get(&name)
            .map(|text| (*text).to_owned())
            .or(disk_text)
        {
            Some(text) => text,
            None => std::fs::read_to_string(&name)
                .map_err(|err| format!("cannot read '{name}': {err}"))?,
        };
        self.sources.borrow_mut().insert(name.clone(), text.clone());
        Ok(ResolvedModule::Source {
            identity: name,
            text,
        })
    }

    // The shared loader stops at the first broken dependency. Still discover
    // later source links for project membership, so an open sibling is not
    // mistaken for an independent program with missing handlers.
    fn discover_sources(&self) {
        let mut pending: Vec<_> = self.sources.borrow().clone().into_iter().collect();
        let mut seen = HashSet::new();
        while let Some((name, text)) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Ok(program) =
                code::lexer::tokenize(&text).and_then(|lexed| code::parser::parse(&lexed))
            else {
                continue;
            };
            for stmt in program.statements {
                if let Stmt::Link { path, .. } = stmt {
                    if let Ok(ResolvedModule::Source { identity, text }) =
                        self.resolve(&name, &path)
                    {
                        pending.push((identity, text));
                    }
                }
            }
        }
    }

    fn link_error(&self, from: &str, reference: &str, message: &str) {
        let sources = self.sources.borrow();
        let Some(text) = sources.get(from) else {
            return;
        };
        let span = code::lexer::tokenize(text)
            .ok()
            .and_then(|lexed| code::parser::parse(&lexed).ok())
            .and_then(|program| {
                program
                    .statements
                    .iter()
                    .enumerate()
                    .find_map(|(index, stmt)| {
                        if matches!(stmt, Stmt::Link { path, .. } if path == reference) {
                            Some(Span::new(
                                program.starts[index],
                                program
                                    .starts
                                    .get(index + 1)
                                    .copied()
                                    .unwrap_or(text.chars().count() as u32),
                            ))
                        } else {
                            None
                        }
                    })
            });
        if let Some(uri) = self.uri(from) {
            self.problems
                .borrow_mut()
                .entry(uri)
                .or_default()
                .push(diagnostics::message(text, span, "source-link", message));
        }
    }
}

impl ModuleResolver for Resolver<'_> {
    fn resolve_entry(&self, entry: &str) -> Result<ResolvedModule, String> {
        self.source(entry.to_string(), None)
    }

    fn resolve(&self, from: &str, reference: &str) -> Result<ResolvedModule, String> {
        // No native code or archive introspection is needed to check local
        // source contracts. Availability and native contracts remain CLI work.
        let extension = Path::new(reference).extension().and_then(|s| s.to_str());
        let native = || {
            Ok(ResolvedModule::Native {
                identity: reference.to_string(),
                path: reference.to_string(),
                format: NativeFormat::Dynamic,
            })
        };
        if matches!(extension, Some("so" | "a")) {
            return native();
        }
        if self.uri(from).is_some_and(|uri| uri.scheme() != "file") {
            let error = "source links require a file URI";
            self.link_error(from, reference, error);
            return Err(error.to_string());
        }
        let base = Path::new(from).parent().unwrap_or(Path::new("."));
        for path in [base.join(reference), base.join(format!("{reference}.code"))] {
            let name = identity(&path);
            if self.buffers.contains_key(&name) {
                return self.source(name, None);
            }
        }
        // Dynamic policy never invokes `nm`. The shared loader supplies its
        // normal source search path, including project/global module roots.
        match FilesystemResolver.resolve(from, reference) {
            Ok(ResolvedModule::Source {
                identity: name,
                text,
            }) => self.source(name, Some(text)),
            Ok(ResolvedModule::Native { .. }) => native(),
            Err(_) if extension.is_none() && !reference.contains(['/', '\\']) => native(),
            Err(error) => {
                self.link_error(from, reference, &error);
                Err(error)
            }
        }
    }
}

struct Loaded<'a> {
    program: Option<Program>,
    resolver: Resolver<'a>,
}

fn load<'a>(entry: &str, documents: &'a HashMap<Url, Document>) -> Loaded<'a> {
    let resolver = Resolver::new(documents);
    let program = match code::loader::load(entry, &resolver) {
        Ok(program) => Some(program),
        Err(error) => {
            resolver.discover_sources();
            let mut syntax_error = false;
            for (name, text) in resolver.sources.borrow().iter() {
                let parsed =
                    code::lexer::tokenize(text).and_then(|lexed| code::parser::parse(&lexed));
                if let Err(err) = parsed {
                    syntax_error = true;
                    if let Some(uri) = resolver.uri(name) {
                        resolver
                            .problems
                            .borrow_mut()
                            .entry(uri)
                            .or_default()
                            .push(diagnostics::from_located(text, &err));
                    }
                }
            }
            if !syntax_error && resolver.problems.borrow().is_empty() {
                if let Some(uri) = resolver.uri(entry) {
                    let text = resolver
                        .sources
                        .borrow()
                        .get(entry)
                        .cloned()
                        .unwrap_or_default();
                    resolver
                        .problems
                        .borrow_mut()
                        .entry(uri)
                        .or_default()
                        .push(diagnostics::message(&text, None, "source-link", &error));
                }
            }
            None
        }
    };
    Loaded { program, resolver }
}

/// Analyze complete programs that actually reach an open file. Prefer a
/// containing main.code / open entry over treating its imported file as an
/// independent program (which would invent unknown-handler errors).
pub fn analyze(documents: &HashMap<Url, Document>) -> Report {
    let mut entries: HashSet<String> = documents.keys().map(document_identity).collect();
    for uri in documents.keys() {
        if let Ok(path) = uri.to_file_path() {
            for dir in path.parent().into_iter().flat_map(Path::ancestors) {
                let main = identity(&dir.join("main.code"));
                if documents.keys().any(|uri| document_identity(uri) == main)
                    || Path::new(&main).is_file()
                {
                    entries.insert(main);
                    break;
                }
            }
        }
    }
    let mut entries: Vec<_> = entries.into_iter().collect();
    entries.sort();
    let loaded: Vec<_> = entries.iter().map(|entry| load(entry, documents)).collect();
    let mut chosen = HashSet::new();
    for uri in documents.keys() {
        let name = document_identity(uri);
        if let Some((index, _)) = loaded
            .iter()
            .enumerate()
            .filter(|(_, loaded)| loaded.resolver.sources.borrow().contains_key(&name))
            .max_by_key(|(_, loaded)| loaded.resolver.sources.borrow().len())
        {
            chosen.insert(index);
        }
    }
    let mut report = Report::new();
    for index in chosen {
        let loaded = &loaded[index];
        for (uri, problems) in loaded.resolver.problems.borrow().iter() {
            report
                .entry(uri.clone())
                .or_default()
                .extend(problems.clone());
        }
        let Some(program) = &loaded.program else {
            continue;
        };
        for diagnostic in code::diagnostics::check_handlers(program) {
            let Some(origin) = &diagnostic.origin else {
                continue;
            };
            if let Some(uri) = loaded.resolver.uri(&origin.file) {
                report
                    .entry(uri)
                    .or_default()
                    .push(diagnostics::from_static(&diagnostic, &origin.source));
            }
        }
        if let Err(error) = code::verify::check_bindings(program) {
            if let Some(origin) = &error.origin {
                if let Some(uri) = loaded.resolver.uri(&origin.file) {
                    report.entry(uri).or_default().push(diagnostics::message(
                        &origin.source,
                        error.span,
                        "binding",
                        &error.message,
                    ));
                }
            }
        }
    }
    for problems in report.values_mut() {
        let mut unique = Vec::new();
        for problem in problems.drain(..) {
            if !unique.contains(&problem) {
                unique.push(problem);
            }
        }
        unique.sort_by_key(|d| {
            (
                d.range.start.line,
                d.range.start.character,
                d.message.clone(),
            )
        });
        *problems = unique;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Project(PathBuf);
    impl Project {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "code-lsp-analysis-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn uri(&self, name: &str) -> Url {
            Url::from_file_path(self.0.join(name)).unwrap()
        }
        fn write(&self, name: &str, text: &str) {
            std::fs::write(self.0.join(name), text).unwrap();
        }
        fn open(&self, name: &str, text: &str) -> (Url, Document) {
            (
                self.uri(name),
                Document {
                    text: text.to_string(),
                    version: 1,
                },
            )
        }
    }
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn unrelated_main_does_not_supply_handlers_or_leak_errors() {
        let p = Project::new();
        p.write("main.code", "Other => return Reply\nemit Wrong to this\n");
        let report = analyze(&HashMap::from(
            [p.open("independent.code", "assert true\n")],
        ));
        assert!(report.values().all(Vec::is_empty), "{report:?}");
    }

    #[test]
    fn missing_and_circular_links_are_diagnosed_without_panicking() {
        let p = Project::new();
        let text = "assert true\nlink \"missing.code\"\n";
        let report = analyze(&HashMap::from([p.open("main.code", text)]));
        let problems = &report[&p.uri("main.code")];
        assert!(problems[0].message.contains("missing.code"));
        assert_eq!(problems[0].range.start.line, 1);
        p.write("child.code", "link \"main.code\"\n");
        let report = analyze(&HashMap::from([
            p.open("main.code", "link \"child.code\"\n")
        ]));
        assert!(
            report
                .values()
                .flatten()
                .any(|d| d.message.contains("circular link")),
            "{report:?}"
        );
    }

    #[test]
    fn closed_import_syntax_error_belongs_to_the_import() {
        let p = Project::new();
        p.write("child.code", "fn example()\n");
        let docs = HashMap::from([p.open("main.code", "link \"child.code\"\n")]);
        let report = analyze(&docs);
        assert!(report[&p.uri("child.code")][0]
            .message
            .contains("there are no functions"));
        assert!(!report.contains_key(&p.uri("main.code")), "{report:?}");
        p.write("child.code", "Ready => return Reply\n");
        assert!(analyze(&docs).values().all(Vec::is_empty));
    }

    #[test]
    fn open_custom_entry_reaches_an_unsaved_new_import() {
        let p = Project::new();
        let docs = HashMap::from([
            p.open(
                "application.code",
                "link \"new.code\"\nAnswer => return Reply\n",
            ),
            p.open(
                "new.code",
                "Read =>\n    emit Answer to this get answer\n    return answer\n",
            ),
        ]);
        assert!(analyze(&docs).values().all(Vec::is_empty));
    }

    #[test]
    fn an_earlier_broken_import_does_not_make_a_sibling_look_standalone() {
        let p = Project::new();
        p.write("helper.code", "fn broken()\n");
        let view = "Read =>\n    emit Respond to this get reply\n    return reply\n";
        p.write("view.code", view);
        p.write("main.code", "link \"helper.code\"\nlink \"view.code\"\n");
        let report = analyze(&HashMap::from([p.open("view.code", view)]));
        assert!(
            report
                .get(&p.uri("helper.code"))
                .is_some_and(|errors| errors
                    .iter()
                    .any(|e| e.message.contains("there are no functions"))),
            "{report:?}"
        );
        assert!(
            !report
                .values()
                .flatten()
                .any(|e| e.message.contains("no handler named")),
            "{report:?}"
        );
    }

    #[test]
    fn native_targets_need_neither_installation_nor_valid_binary_bytes() {
        let p = Project::new();
        p.write("bad.a", "invalid archive bytes");
        p.write("bad.so", "invalid library bytes");
        let docs = HashMap::from([p.open("main.code", "link \"bad.a\" as archive\nlink \"bad.so\" as shared\nlink \"not_installed\" as native\nemit Work to archive\nemit Work to shared\nemit Work to native\n")]);
        assert!(analyze(&docs).values().all(Vec::is_empty));
    }
}

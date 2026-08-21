//! WASM bridge for running a single, self-contained `.code` snippet in a
//! browser or other JS host — T19 (browser playground).
//!
//! v1 scope: no `link` support (no filesystem in a browser; module linking
//! via an in-memory source map is deferred — see the T19 ticket). A snippet
//! is parsed and interpreted directly; the result is the program's final
//! top-level bindings (this is a constraint language with no core I/O, so
//! bindings are the only observable output) plus any parse/runtime
//! diagnostics, located by char-offset span for the host to highlight.

use chumsky::Parser;
use code_lang::interpreter::Interpreter;
use code_lang::parser;
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// The JS-facing result shape. This is a public contract once `code-wasm` is
/// published (see T19) — change it deliberately, not as an incidental
/// side-effect of an internal refactor.
#[derive(Serialize)]
pub struct RunResult {
    pub ok: bool,
    pub bindings: Vec<BindingOut>,
    pub diagnostics: Vec<DiagnosticOut>,
}

#[derive(Serialize)]
pub struct BindingOut {
    pub name: String,
    /// Rendered display value (e.g. `"5"`, `"hello"`, `"{ x = 1 }"`), or
    /// `null` if the variable's constraint domain wasn't narrowed to a
    /// single value.
    pub value: Option<String>,
    /// Code type name (Number/String/Boolean/Object/Array/Null), or `null`
    /// alongside an unresolved `value`.
    pub kind: Option<String>,
    /// For an unresolved binding (`value` is `null`), a human-readable
    /// description of what the variable could still be — e.g. `"3 < _ < 10"`
    /// or `"possible values: {0, 1}"`. `null` for resolved bindings.
    pub domain: Option<String>,
}

#[derive(Serialize)]
pub struct DiagnosticOut {
    pub message: String,
    /// Char-offset span within `src`, when the error is located.
    pub start: Option<usize>,
    pub end: Option<usize>,
}

/// Run a single `.code` snippet. Returns a `RunResult` serialized to a plain
/// JS object: `{ ok, bindings, diagnostics }`.
#[wasm_bindgen]
pub fn run_source(src: &str) -> JsValue {
    let result = run_source_inner(src);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn run_source_inner(src: &str) -> RunResult {
    let (parsed, parse_errors) = parser::parser().parse_recovery(src);

    if !parse_errors.is_empty() {
        let diagnostics = parse_errors
            .iter()
            .map(|err| {
                // Use the custom reason if present, otherwise the default
                // Display which lists expected/found tokens — matches
                // module_loader's convention for parse-error rendering.
                let message = match err.reason() {
                    chumsky::error::SimpleReason::Custom(s) => s.clone(),
                    _ => format!("{}", err),
                };
                let span = err.span();
                DiagnosticOut {
                    message,
                    start: Some(span.start),
                    end: Some(span.end),
                }
            })
            .collect();
        return RunResult {
            ok: false,
            bindings: Vec::new(),
            diagnostics,
        };
    }

    let program = match parsed {
        Some(p) => p,
        None => {
            return RunResult {
                ok: false,
                bindings: Vec::new(),
                diagnostics: vec![DiagnosticOut {
                    message: "Parser produced no output despite no errors".to_string(),
                    start: None,
                    end: None,
                }],
            }
        }
    };

    let mut interp = Interpreter::new();
    match interp.execute(program) {
        Ok(()) => {
            let bindings = interp
                .bindings_detailed()
                .into_iter()
                .map(|(name, value, domain)| BindingOut {
                    value: value.as_deref().map(|v| v.to_string()),
                    kind: value.as_deref().map(|v| v.type_name().to_string()),
                    // Only surface the domain for unresolved bindings — for a
                    // resolved one the value already says everything.
                    domain: if value.is_none() { Some(domain) } else { None },
                    name,
                })
                .collect();
            RunResult {
                ok: true,
                bindings,
                diagnostics: Vec::new(),
            }
        }
        Err(message) => {
            let span = interp.error_span();
            RunResult {
                ok: false,
                bindings: Vec::new(),
                diagnostics: vec![DiagnosticOut {
                    message,
                    start: span.as_ref().map(|s| s.start),
                    end: span.as_ref().map(|s| s.end),
                }],
            }
        }
    }
}

//! Thin browser bridge around the interpreter (LLVM/`code build` is
//! native-only — see the root crate's `llvm` feature — so this only ever
//! wraps the interpreter). Powers `site/`'s playground and, via
//! `npm/`, a standalone `code-wasm` package any JS host can embed.
//!
//! `run_with_modules` is this crate's answer to
//! `docs/todo/native-module-linking.md`'s `.wasm` question: rather than
//! embedding a wasm runtime and a byte-offset ABI in Rust (what the old
//! language did, `old/src/wasm_module.rs` — real pain, confirmed by reading
//! it), a linked module here is nothing more than a synchronous JS callback,
//! JSON string in, JSON string out. Turning an actual third-party `.wasm`
//! file into that shape (instantiating it, reading/writing its linear
//! memory) is entirely the embedding JS app's job — this crate's Rust code
//! never touches wasm bytes, so a plain JS function is just as valid a
//! "module" as one backed by wasm.

use std::rc::Rc;

use code::ast::NativeFormat;
use code::interpreter::{self, Environment, ModuleDispatch};
use code::loader::{self, ModuleResolver, NoModules, ResolvedModule};
use code::value::Value;
use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Runs `src` and returns `""` on success or `"error: ..."` — a program's
/// output is whatever it emits itself (through a linked module), not a dump
/// of its final bindings. A single string return keeps the JS side trivial
/// for now. No module support — see `run_with_modules` for that.
#[wasm_bindgen]
pub fn run(src: &str) -> String {
    match code::run_source(src) {
        Ok(_) => String::new(),
        Err(e) => format!("error: {e}"),
    }
}

/// Like `run`, but `src` may `link` any alias present as an own-enumerable
/// key of `modules`. Each value must be a plain JS object shaped
/// `{ dispatch(particleJson) -> resultJson, vars?() -> varsJson }` —
/// `dispatch` is called synchronously for every `emit ... to <alias>`,
/// `vars` (optional, called once, up front) becomes `<alias>.<name>` field
/// access exactly like a `.so`'s exported variables
/// (`tests/native_link_vars.code`). Both exchange plain JSON text; a module
/// with no `vars` gets an empty object, matching `.so`'s "no
/// `code_module_vars` export" default.
///
/// Deliberately synchronous, not async: every module must already be
/// resolvable before this call (see `PreloadedModules`) — instantiating a
/// real `.wasm` file (`WebAssembly.instantiateStreaming`, necessarily async)
/// is the caller's job, done before calling this, not something `link`
/// triggers mid-run.
#[wasm_bindgen]
pub fn run_with_modules(src: &str, modules: Object) -> String {
    match run_with_modules_inner(src, modules) {
        Ok(_) => String::new(),
        Err(e) => format!("error: {e}"),
    }
}

fn run_with_modules_inner(src: &str, modules: Object) -> Result<Environment, String> {
    let mut env = Environment::default();
    let mut aliases = Vec::new();

    for key in Object::keys(&modules).iter() {
        let alias = key
            .as_string()
            .ok_or_else(|| "module keys must be strings".to_string())?;
        let descriptor =
            Reflect::get(&modules, &key).map_err(|e| format!("reading module '{alias}': {e:?}"))?;

        let dispatch_fn: js_sys::Function =
            Reflect::get(&descriptor, &JsValue::from_str("dispatch"))
                .map_err(|e| format!("module '{alias}' has no 'dispatch': {e:?}"))?
                .dyn_into()
                .map_err(|_| format!("module '{alias}': 'dispatch' is not a function"))?;

        let vars_fn: Option<js_sys::Function> =
            Reflect::get(&descriptor, &JsValue::from_str("vars"))
                .ok()
                .and_then(|v| v.dyn_into().ok());

        let vars = match vars_fn {
            Some(vars_fn) => {
                let result = vars_fn
                    .call0(&JsValue::NULL)
                    .map_err(|e| format!("module '{alias}'.vars(): {e:?}"))?;
                let json = result
                    .as_string()
                    .ok_or_else(|| format!("module '{alias}'.vars() must return a JSON string"))?;
                decode_json(&json)?
            }
            None => Value::Object(Rc::new(Vec::new())),
        };

        let dispatch_alias = alias.clone();
        let dispatch: ModuleDispatch = Rc::new(move |particle: &Value| -> Result<Value, String> {
            // `Display` already emits JSON (`value.rs`) — no encoder needed.
            let arg = JsValue::from_str(&particle.to_string());
            let result = dispatch_fn
                .call1(&JsValue::NULL, &arg)
                .map_err(|e| format!("calling module '{dispatch_alias}': {e:?}"))?;
            let json = result
                .as_string()
                .ok_or_else(|| format!("module '{dispatch_alias}' must return a JSON string"))?;
            decode_json(&json)
        });

        env.provide_module(&alias, vars, dispatch);
        aliases.push(alias);
    }

    let resolver = PreloadedModules {
        entry_identity: "<source>".to_string(),
        entry_text: src.to_string(),
        aliases,
    };
    let program = loader::load("<source>", &resolver)?;
    interpreter::run_with(&program, env)
}

/// Decodes a JSON string into a `Value` — a real `serde_json` parse, not
/// this language's own literal grammar. The two used to coincide (a JSON
/// object and a `.code` object literal were both `"key": value`), which is
/// why this went through `code::parser` originally; they diverged the day
/// object literals became `key = value` (computed keys), and reusing the
/// `.code` parser here started rejecting every real JSON string a module
/// returns — `{"answer": 42}` no longer parses as a `.code` expression at
/// all. JSON's own grammar hasn't changed, so this decodes it directly and
/// converts the result, rather than trying to keep two grammars in sync by
/// convention.
fn decode_json(json: &str) -> Result<Value, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    Ok(json_to_value(value))
}

/// `serde_json::Value` and this language's `Value` are both JSON's six
/// kinds, so the conversion is a straight structural walk with no room for
/// it to fail. `serde_json`'s `Number` covers ints and floats in one variant
/// (`f64` losslessly round-trips anything this language can represent — its
/// own `Value::Number` is an `f64` too); `null` is folded into `0.0` only if
/// `as_f64` somehow refuses a `Number`, which the `Number` variant's own
/// invariant rules out.
fn json_to_value(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::Str(Rc::from(s.as_str())),
        serde_json::Value::Array(items) => {
            Value::Array(Rc::new(items.into_iter().map(json_to_value).collect()))
        }
        serde_json::Value::Object(fields) => Value::Object(Rc::new(
            fields
                .into_iter()
                .map(|(k, v)| (k, json_to_value(v)))
                .collect(),
        )),
    }
}

/// Resolves `link "<alias>"` to whichever of `aliases` the JS caller
/// registered with `run_with_modules` — no filesystem, no `.so`/`.a`
/// extension logic, unlike `loader::FilesystemResolver`. A name not in
/// `aliases` is rejected the same way `NoModules` rejects every `link` —
/// there is nothing dynamic to fall back to once execution has started (see
/// `run_with_modules`'s doc comment on why this is all resolved up front).
struct PreloadedModules {
    entry_identity: String,
    entry_text: String,
    aliases: Vec<String>,
}

impl ModuleResolver for PreloadedModules {
    fn resolve_entry(&self, entry: &str) -> Result<ResolvedModule, String> {
        NoModules {
            entry_identity: self.entry_identity.clone(),
            entry_text: self.entry_text.clone(),
        }
        .resolve_entry(entry)
    }

    fn resolve(&self, _from_identity: &str, module_ref: &str) -> Result<ResolvedModule, String> {
        if self.aliases.iter().any(|a| a == module_ref) {
            Ok(ResolvedModule::Native {
                identity: module_ref.to_string(),
                path: module_ref.to_string(),
                format: NativeFormat::JsBridge,
            })
        } else {
            Err(format!(
                "cannot link '{module_ref}': no such module was provided to run_with_modules"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression this file shipped once: `decode_json` used to reuse
    /// the `.code` object-literal parser, on the assumption that grammar
    /// and JSON's were the same. They diverged when object literals became
    /// `key = value` (computed keys), and nothing in `cargo test` exercised
    /// `decode_json` with real JSON — `"key": value` — to notice. It broke
    /// silently until an actual release ran `crates/code-wasm/npm/smoke-test.mjs`
    /// against the built package, which only a `v*` tag push does.
    #[test]
    fn decodes_the_json_a_real_module_returns() {
        assert_eq!(decode_json("42").unwrap(), Value::Number(42.0));
        assert_eq!(decode_json("true").unwrap(), Value::Bool(true));
        assert_eq!(decode_json("null").unwrap(), Value::Null);
        assert_eq!(
            decode_json(r#""a:b""#).unwrap(),
            Value::Str(Rc::from("a:b"))
        );
        assert_eq!(
            decode_json(r#"{"answer": 42}"#).unwrap(),
            Value::Object(Rc::new(vec![("answer".to_string(), Value::Number(42.0))]))
        );
        assert_eq!(
            decode_json(r#"{"_class": "DoubleResult", "value": 42}"#).unwrap(),
            Value::Object(Rc::new(vec![
                ("_class".to_string(), Value::Str(Rc::from("DoubleResult"))),
                ("value".to_string(), Value::Number(42.0)),
            ]))
        );
        assert_eq!(
            decode_json(r#"[1, "two", null]"#).unwrap(),
            Value::Array(Rc::new(vec![
                Value::Number(1.0),
                Value::Str(Rc::from("two")),
                Value::Null,
            ]))
        );
        assert!(decode_json("not json").is_err());
    }
}

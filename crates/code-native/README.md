# code-native

Helper crate for writing native modules (in Rust) for the [Code programming
language](https://github.com/codelovesme/code) — a `.so`/`.wasm` extension
mechanism that lets you implement particle handlers, exported variables, and
type declarations in Rust instead of `.code` source.

`code-native` eliminates the C-ABI boilerplate: it provides `CodeValue`
builders/readers and the `code_module!` macro, which generates the required
`#[no_mangle]` entry points (`code_module_abi_version`, `code_module_init`).

## Quick start

```rust,ignore
use code_native::*;

unsafe extern "C" fn handle_add(particle: CodeValue) -> CodeValue {
    let a = read_field_number(&particle, "a");
    let b = read_field_number(&particle, "b");
    code_object(vec![
        code_field("_class", code_string("AddResult")),
        code_field("result", code_number(a + b)),
    ])
}

code_module! {
    vars: [
        "PI" => code_number(3.14159),
    ],
    types: [
        "Add" [("a", "Number"), ("b", "Number")],
    ],
    handlers: [
        "Add" => handle_add,
    ],
    emissions: [],
}
```

Compile as a `cdylib` and `link` it from `.code` source — see the [native
module linking docs](https://github.com/codelovesme/code#native-module-linking)
in the main repo for the full ABI contract and the `.wasm` variant.

## License

MIT — see [LICENSE](LICENSE).

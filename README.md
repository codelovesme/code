# Code Programming Language

A minimal programming language implementation in Rust, with both an interpreter and an LLVM backend.

> Primary source extension is now `.code`.

## Overview

Code currently demonstrates:

- **Parser**: Using `chumsky` parser combinator library
- **AST**: Clean abstract syntax tree representation
- **Runtime**: Heap-allocated values with reference-counted memory management, constraint domains
- **Environment**: Scope-stack architecture with domain-constrained variable storage
- **Module system**: `link` with flatten or alias namespace imports, plus `private` visibility
- **Native module system**: `link` to `.so` and `.wasm` native modules through stable ABIs (v1)
- **Constraint system**: Variables defined by constraints (`=`, `<`, `>`, `≤`, `≥`, `≠`, `in`), domain narrowing, contradiction detection
- **Interpreter**: Tree-walking interpreter for program execution
- **LLVM backend**: LLVM IR/object generation and native/WASM linking
- **Rust native helper crate**: `crates/code-native` for macro-first native module authoring

## Project Structure

```
src/
├── main.rs           # CLI entry point (run/build/test)
├── ast.rs            # AST definitions
├── parser.rs         # Chumsky-based parser for .code files
├── module_loader.rs  # Recursive link resolver (imports + cycle checks)
├── native_module.rs  # Native module ABI contract + .so loader
├── wasm_module.rs    # Native module ABI contract + .wasm loader (wasmi)
├── runtime.rs        # Runtime value model
├── runtime_native.c  # C bridge runtime compiled into LLVM/exe builds
├── environment.rs    # Scope stack + type/type-annotation registries
├── interpreter.rs    # Tree-walking interpreter
├── codegen.rs        # LLVM IR/object generation
└── linker.rs         # Native/WASM link helpers

crates/
├── code-native/      # Rust helper library for authoring native modules (MIT)
│   └── src/lib.rs
└── code-lsp/         # Language Server Protocol implementation for .code
    └── src/main.rs
```

## Building

```bash
cargo build
```

### LLVM Prerequisites

LLVM 17 is required for the LLVM backend. Ensure `llvm-config` is on `PATH`,
or set `LLVM_SYS_170_PREFIX` to your LLVM 17 installation root.

For WASM output, install `lld-17` (provides `wasm-ld-17`):
```bash
sudo apt install lld-17
```

### Build Targets

```bash
code build <file.code> [--target <type>]
```

| Target   | Output                    | Description                  |
|----------|---------------------------|------------------------------|
| `exe`    | `target/llvm/<name>`      | Native ELF executable (default) |
| `ir`     | `target/llvm/<name>.ll`   | LLVM IR text                 |
| `shared` | `target/llvm/lib<name>.so`| Shared library               |
| `static` | `target/llvm/lib<name>.a` | Static library               |
| `wasm`   | `target/llvm/<name>.wasm` | WebAssembly module           |

#### Examples

```bash
# Native executable (default) — run it directly
./target/debug/code build hello_world.code
./target/llvm/hello_world

# LLVM IR
./target/debug/code build hello_world.code --target ir

# Shared library
./target/debug/code build hello_world.code --target shared

# Static library
./target/debug/code build hello_world.code --target static

# WebAssembly
./target/debug/code build hello_world.code --target wasm
```

## Running

```bash
cargo run -- run hello_world.code
```

## Language Syntax

### Comments

```
-> This is a comment
-> Everything after -> until end of line is ignored
```

### Constraints (Variable Definitions)

Variables are defined and narrowed by constraints rather than assignments:

```
a = 1
b = "hello world"
```

The `=` operator pins a variable to an exact value. Variables are
**single-assignment**: once a variable is pinned, re-assigning it a different
value is a runtime error (see `tests/fail_reassignment.code`). Prior range/set
constraints on the same variable are still allowed and are checked at pin time.

#### Range Constraints

```
x > 0
x < 100
x ≥ 10
x ≤ 50
x = 25
assert x = 25
```

Range constraints narrow a variable's domain. Once pinned with `=`, the value must satisfy all prior constraints.

#### Set Membership

```
color in ["red", "green", "blue"]
```

The `in` operator constrains a variable to a finite set of allowed values.

#### Domain Constraints

```
n in Z    -> integers
r in R    -> real numbers
k in N    -> natural numbers (non-negative integers)
```

#### Contradiction Detection

Contradictory constraints produce a runtime error:

```
a > 10
a < 5     -> error: contradictory constraints, domain is empty
```

### Numbers, Strings, Booleans, Null

- **Numbers**: Floating-point literals (e.g., `1`, `3.14`, `42.0`)
- **Strings**: Double-quoted strings (e.g., `"hello"`, `"world"`)
- **Booleans**: `true` and `false` literals
- **Null**: The `Null` literal represents absence of a value

### String Interpolation

Use `$name` inside double-quoted strings to embed a variable's value. Only a
bare identifier is supported (no braces, no expressions or field access):

```
name = "World"
greeting = "Hello, $name!"
assert greeting = "Hello, World!"

count = 3
msg = "Count is $count"
```

### Expressions

#### Identifier
```
a
```
Looks up variable in current scope chain.

#### Binary Operations

Full operator support with standard precedence:

| Category    | Operators          | Operand Types | Result Type |
|-------------|--------------------|---------------|-------------|
| Arithmetic  | `+`, `-`, `*`, `/` | Number        | Number      |
| String      | `+`                | String        | String      |
| Array       | `+`                | Array         | Array       |
| Comparison  | `<`, `>`, `≤`, `≥` | Number      | Boolean     |
| Equality    | `=`, `≠`         | Any           | Boolean     |
| Logical     | `and`, `or`, `not` | Boolean     | Boolean     |

`=` and `≠` also perform type checking when the right-hand side is a type name (e.g. `Number`, `String`, `Boolean`, `Null`, `Object`, `Array`, `Function`, or a particle class name):

```
assert 42 = Number
assert "hello" ≠ Number
assert p = Point
```

**Precedence** (highest to lowest):
`()` → `not` → `*` `/` → `+` `-` → `<` `>` `≤` `≥` → `=` `≠` → `and` → `or`

```
assert (1 + 2) * 3 = 9        -> () overrides precedence
assert 2 + 3 * 4 = 14         -> * before +
assert 10 - 4 / 2 = 8         -> / before -
assert 3 < 5 and 10 > 2       -> comparisons before and
assert 1 > 2 or 3 < 4         -> comparisons before or
```

**`+` operator** is polymorphic — it concatenates strings, concatenates arrays, and adds numbers:

```
assert 1 + 2 = 3
assert "hello " + "world" = "hello world"
assert [1, 2] + [3] = [1, 2, 3]
```

Arithmetic and comparison operators require Number operands. Logical operators require Boolean operands. Type mismatches produce runtime errors.

Division by zero is a runtime error.

**Short-circuit evaluation**: `and` and `or` do not evaluate the right operand when the left operand determines the result:

```
result = false and 1 / 0 = 1   -> no error, right side not evaluated
```

#### Unary Operations

The `not` (logical NOT) operator negates a Boolean value:

```
assert not false = true
assert not true = false
assert not not true = true
```

### Assertions

```
assert a ≠ 1
assert "x" ≠ "y"
```

Assert expects a boolean. Fails if expression evaluates to `false`.

### Arrays

Arrays are ordered collections of values:

```
arr = [1, 2, 3]
assert arr[0] = 1
assert arr[2] = 3
```

Arrays support concatenation with `+` and element append/prepend:

```
a = [1, 2] + [3, 4]
assert a = [1, 2, 3, 4]

b = [1, 2] + 3
assert b = [1, 2, 3]

c = 0 + [1, 2]
assert c = [0, 1, 2]
```

### If Statements

```
x = 5
if x > 3 {
    result = "big"
}
assert result = "big"
```

The condition must be a Boolean value.

### Loop / Break

```
i = 0
loop {
    i = i + 1
    if i = 5 {
        break
    }
}
assert i = 5
```

`break` exits the innermost loop. Using `break` outside a loop is a compile-time error.

### Objects

Objects are immutable key-value collections:

```
obj = { x = 1, y = "hello" }
assert obj.x = 1
assert obj.y = "hello"
```

Objects can be nested:

```
nested = { point = { x = 1, y = 2 }, name = "origin" }
assert nested.point.x = 1
```

### Particles

Particles are specialized objects with a class name and predefined properties `_class` and `_created`. The syntax uses `ClassName { fields }`:

```
log = Log { message = "Hello World", level = "Info" }
assert log._class = "Log"
assert log._created = 0
assert log.message = "Hello World"
assert log.level = "Info"
```

Particles use uppercase class names. Objects and particles are immutable after creation.

### Type Declarations (Particles)

Declare particle schemas with named fields and field type constraints:

```
type Log { message = String, level = String }
```

When constructing a particle for a known type, field presence and field types are validated:

```
p = Log { message = "Hello World", level = "Info" }   -> valid
p = Log { message = "Hello World", level = 12 }        -> error
```

### Module Linking

Code supports two `link` modes:

- **Flatten mode**: `link modules/shared_values`
    - Public names are injected into the current scope.
- **Namespace mode**: `link modules/shared_values as shared`
    - Public names are accessed via `shared.name`.

Visibility:

- Top-level declarations are public by default.
- `private` hides a declaration from importing modules.

Example:

```
-> module file
private secret = 1
greeting = "hello"

-> importer
link modules/shared_values as m
assert m.greeting = "hello"
```

Module-qualified particle constructors are supported:

```
link modules/typed_module as module1
log = module1.Log { message = "Hello", level = "Info" }
```

If no local type is in scope (for example using bare `Log` while only `module1.Log` exists), particle construction falls back to dynamic behavior.

### Native Module Linking

Code can link native modules through two backends:

- `.so` shared libraries (C ABI, host-native runtime)
- `.wasm` modules (WASM ABI, loaded through `wasmi`)

```
link native_modules/libtest_math.so as math
assert math.PI > 3
assert math.add(2, 3) = 5
```

```code
link native_modules/test_math.wasm as math
assert math.PI > 3
assert math.add(2, 3) = 5
```

Required native exports:

```c
uint32_t code_module_abi_version(void);   // must return 2
const CodeModuleDesc* code_module_init(void);
```

For `.wasm` native modules, the required exports are:

```c
uint32_t code_module_abi_version(void);   // must return 2
uint32_t code_module_init(void);          // returns offset to CodeModuleDesc in linear memory
int32_t  code_alloc(int32_t size);        // allocator used for arg/result marshalling
```

WASM function and handler exports are resolved by index/name:

- Function exports: `code_fn_<idx>` (fallback: exported function name)
- Handler exports: `code_handler_<idx>` (fallback: `code_handler_<ClassName>`)

Native modules can export:
- Variables
- Functions (with fixed parameter counts)
- Handlers
- Type declarations

Notes:
- `.so` is supported for runtime native linking (interpreter + host-native LLVM/exe flows).
- `.wasm` is supported for runtime native linking through `wasmi`.
- `.a` native linking is rejected at runtime import.
- WASM build target rejects `.so` imports with a clear error (`Use a .wasm module instead`).
- WASM build target accepts `.wasm` native imports.
- In LLVM build mode, a small C bridge runtime is compiled and linked automatically.

### Rust Helper Crate for Native Modules

Use `crates/code-native` to remove Rust ABI boilerplate when building native modules.

The crate provides:
- ABI structs/constants matching Code runtime loader
- Value builders (`code_number`, `code_string`, `code_object`, ...)
- Read helpers (`read_str`, `read_field_str`, ...)
- `code_module!` macro to generate:
    - `code_module_abi_version`
    - `code_module_init`

See `tests/native_modules/test_helper.rs` for a compact helper-based native module example.

### Functions

Functions are first-class values defined with arrow syntax:

```
add = (a, b) => {
    return a + b
}

result = add(3, 4)
assert result = 7
```

**Parameter rules** — parameters are bare names:

```
f = (a, b) => { return b }
```

**Return types** are not annotated — functions simply return whatever value the body produces:

```
sum = (a, b) => {
    return a + b
}
```

Functions without an explicit `return` return `Null`.

**Functions as values** — functions can be stored in variables, passed as arguments, and type-checked:

```
apply = (f, x) => {
    return f(x)
}

double = (n) => {
    return n * 2
}

assert apply(double, 5) = 10
assert double = Function
```

Functions do not capture their defining scope (no closures). They execute with access to only their own parameters and local variables. Attempting to read an outer variable from inside a function is a runtime error. Type definitions from the outer scope are available inside functions, but handler definitions and handler invocations are **not allowed** inside function bodies.

### Handlers

Particle handlers respond to particle construction events. Unlike functions, handlers **can** read variables from their enclosing scope, but **cannot** mutate them — shadowing outer variables is not allowed:

```
type Ping { }
type Pong { ok = Number }

Ping{} => {
    -> handler body runs when Ping is created
    return Pong{ ok = 1 }
}

Ping{} => this => result
assert result.ok = 1
```

All handler output goes through `return` values captured with the `=> this => result` pattern.

Handlers without an explicit `return` return `Null`.

## Memory Model

### Core Principles

1. **Heap-Allocated Values**: All runtime values (`Number`, `String`, `Boolean`) live on the heap
2. **Reference Counting**: `Rc<Value>` provides automatic memory management
3. **Scope Stack**: Variables stored in frames (`Vec<HashMap<String, ConstrainedVar>>`)
4. **Constraint Domains**: Each variable has a `Domain` describing its allowed values
5. **Immutable, single-assignment**: Values never mutate, and a variable cannot be re-pinned to a different value once defined
6. **Automatic Cleanup**: When scope is dropped, references are cleaned up; heap values deallocated automatically

### Example

```code
a = 1           -> define a with Domain::Exact(Number(1.0))
b = "hello"     -> a distinct variable with its own heap value
```

When a scope is dropped, its variables' references are released and any heap
value with no remaining references is deallocated.

## Usage Example

### hello_world.code

```
-> Variables are single-assignment
a = 1
assert a = 1
greeting = "hello world"
assert greeting ≠ "goodbye"
```

### Execution

```bash
$ cargo run -- run hello_world.code
Program executed successfully.
```

## Implementation Status

- [x] Parser + AST (constraint-based)
- [x] Runtime + Environment with domain-constrained variables
- [x] Interpreter (constraint execution, domain intersection)
- [x] Constraint operators: `=`, `≠`, `<`, `>`, `≤`, `≥`, `in`
- [x] Domain types: `Exact`, `RealRange`, `IntegerRange`, `ValueSet`, `TypeDomain`, `Intersection`
- [x] Domain keywords: `Z` (integers), `R` (reals), `N` (naturals)
- [x] Contradiction detection (empty domain error)
- [x] Block scopes (no shadowing)
- [x] Objects and particles
- [x] Deep equality for nested objects/particles
- [x] Module linking (`link`, alias imports, flatten imports, cycle/duplicate detection)
- [x] Visibility control with `private`
- [x] Type declarations for particles
- [x] Particle handlers with `return` values
- [x] Type checks via `=` / `≠` with type names
- [x] `Null` literal and optional fields
- [x] `Exception` type
- [x] `if` statements
- [x] Arrays with indexing and `+` concatenation
- [x] String concatenation and interpolation
- [x] `loop` / `break`
- [x] Boolean literals (`true`, `false`)
- [x] Arithmetic operators (`-`, `*`, `/`)
- [x] Comparison operators (`<`, `>`, `≤`, `≥`)
- [x] Logical operators (`and`, `or`, `not`) with short-circuit evaluation
- [x] Full operator precedence
- [x] Functions: first-class definitions
- [x] Functions: strict no-capture scope isolation (interpreter + LLVM)
- [x] Function body restrictions: handler def/invoke prohibited inside functions
- [x] Parenthesized expressions for grouping
- [x] Parser diagnostics (Phase 1: labels and clearer error messages)
- [x] LLVM code generation (`ir`, `exe`, `shared`, `static`, `wasm`)
- [x] Built-in `.code` test runner + Rust integration tests
- [x] Error recovery: report multiple parse errors per file
- [x] Handler return enforcement: only Particle values allowed
- [x] Native module ABI linking (`link` to `.so` modules)
- [x] WASM native module ABI linking (`link` to `.wasm` modules via `wasmi`)
- [x] Native imports: variables/functions/handlers/types
- [x] Rust native helper crate (`crates/code-native`) with macro-first API

## Roadmap

Planned work is tracked as individual tickets under [`docs/tickets/`](docs/tickets/).

## Design Principles

- **Safe core, isolated `unsafe`**: The interpreter, parser, and runtime contain
  no `unsafe`; it is confined to the native-module FFI boundary
  (`native_module.rs`, `wasm_module.rs`, `crates/code-native`).
- **Clean separation**: Parser → AST → Runtime → Interpreter
- **Minimal but extensible**: Add features incrementally without breaking existing code
- **Memory conscious**: Proper reference management, no leaks

## Dependencies

```toml
[dependencies]
chumsky = "0.9"
anyhow = "1"
inkwell = { version = "0.4", features = ["llvm17-0"] }
libloading = "0.8"
wasmi = "1.0"
```

## License

The language (interpreter, compiler, LSP) is licensed under **GPL-3.0** — see the
[LICENSE](LICENSE) file. The native-module helper crate `crates/code-native` is
licensed under **MIT** (see [crates/code-native/LICENSE](crates/code-native/LICENSE))
so it can be linked into native modules under any license.

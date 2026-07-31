# T5 — README completeness and polish

- **Priority:** Low
- **Type:** Documentation
- **Area:** `README.md`

## Problem

The README under-describes the workspace and carries leftover placeholder text.

### Evidence

- **Logical operators are documented wrong.** README shows `&&`, `||`, `!`
  (`README.md:196`, `:207`, `:229-233`), but the parser implements the keywords
  `and`, `or`, `not` (`src/parser.rs:297,413,429`) and the tests use them
  (`tests/logical_basic.code`). Decision (confirmed): keep `and`/`or`/`not`; no
  symbolic operators. Fix the README to match — including the precedence line
  and the short-circuit example, and `!` → `not`.
- **`code-lsp` is undocumented.** The "Project Structure" block
  (`README.md:38-41`) lists only `crates/code-native` and never mentions the LSP
  crate that ships in the same workspace (`crates/code-lsp/`).
- **`src/runtime_native.c`** (the C bridge runtime) is absent from the structure
  block though it is part of `src/`.
- **Dependency drift:** README lists `wasmi = "1"` (`README.md:642`) while
  `Cargo.toml` pins `wasmi = "1.0"`.
- **Placeholder text:** typo "Under negotitation" (`README.md:624`) and empty
  "Future Steps" / "Under negotiation" sections.
- **License note is incomplete:** the "License" section says only "See LICENSE
  file" (GPL-3.0), but `crates/code-native` is MIT (with its own
  `crates/code-native/LICENSE`). Worth stating the split so consumers of the
  helper crate know it is permissively licensed.

## Proposed change

- Add `crates/code-lsp` and `src/runtime_native.c` to the structure block.
- Sync the dependency list with `Cargo.toml`.
- Fix the typo; either fill in or remove the empty sections.
- Add a one-line note that `code-native` is MIT-licensed while the language
  itself is GPL-3.0.

## Acceptance criteria

- Structure block matches the actual tree.
- No placeholder/empty sections remain.
- Dependency versions match `Cargo.toml`.

## Effort

Small (docs only).

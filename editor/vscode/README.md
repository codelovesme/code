# Code Language — VS Code extension

Syntax highlighting, inline diagnostics, semantic tokens, and document
formatting for the [Code](../../README.md) programming language's `.code`
files.

Everything heavy — parsing, diagnostics, semantic tokens, formatting — lives
in [`code-lsp`](../../crates/code-lsp/README.md), the official language server
built from **this repository**. This extension is deliberately thin: it finds
and launches that binary and stays out of its way. Packaging
(`../../.github/workflows/publish-editor-vsix.yml`, or run locally with
`vsce package`) builds `code-lsp` from the checked-out commit it ships beside,
so the editor integration you have matches the language rules the `code`
binary of the same version enforces. There is no second implementation to get
out of date.

## Install

From the marketplace / Open VSX once published, or unpacked locally:

```sh
cd editor/vscode
npm install
npm run compile
vsce package            # produces codelovesme-code-language-<ver>.vsix
code --install-extension codelovesme-code-language-*.vsix
```

## Settings

| Setting                         | Meaning                                                                        |
|---------------------------------|--------------------------------------------------------------------------------|
| `codelanguage.server.path`      | Launch a specific `code-lsp` binary instead of the bundled one (dev escape hatch — pin a particular build without touching the bundled binary) |
| standard LSP trace setting | Off by default; turn on `verbose` to inspect raw protocol traffic when debugging |

The bundled binary is looked up under `server/<platform>/code-lsp` inside the
extension directory (e.g. `server/linux-x64/code-lsp`); its executable bit is
forced back on at startup, since archive extraction does not guarantee it. If
it is missing the extension warns once and falls back to a plain `code-lsp` on
your `PATH` — behaviour then tracks whatever `code` happens to be installed,
which is fine accidentally for one session but not worth leaving: point
`codelanguage.server.path` at a matching binary if you need it permanently.

## License

GPL-3.0-or-later, the same license as the rest of this project — see
[`LICENSE`](LICENSE).

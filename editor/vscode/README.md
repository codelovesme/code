# Code Language — VS Code extension

Syntax highlighting, inline syntax diagnostics, semantic tokens, and document
formatting for the [Code](../../README.md) programming language's `.code`
files.

Everything heavy — parsing, diagnostics, semantic tokens, formatting — lives
in [`code-lsp`](../../crates/code-lsp), the official language server
built from **this repository**. This extension is deliberately thin: it finds
and launches that binary and stays out of its way. The
[release workflow](../../.github/workflows/publish-editor-vsix.yml) builds
`code-lsp` from the same checkout and bundles it in the VSIX, so the extension
and its language server come from one version. Local packaging must supply the
server binaries too; `vsce package` does not build `code-lsp`.

## Install

Download `code-language-<version>.vsix` from the matching
[Code release](https://github.com/codelovesme/code/releases). In VS Code, open
the Command Palette, choose **Extensions: Install from VSIX...**, and select
that file. The release workflow attaches the VSIX to GitHub Releases; it does
not publish it to Microsoft Marketplace or Open VSX.

For terminal installation, invoke the **Visual Studio Code** executable with
`--install-extension /path/to/code-language-<version>.vsix`. Its command is
also usually named `code`; the Code language CLI does not install VS Code
extensions. The Command Palette path avoids that name collision.

Release packages bundle servers for Linux x86_64 and macOS arm64. Other
platforms need a compatible `code-lsp` selected with `codelanguage.server.path`.
If no bundled server is found, the extension also tries `PATH` with a warning.
For local builds and packaging, follow the
[release workflow](../../.github/workflows/publish-editor-vsix.yml), including
its server-bundling steps.

## Diagnostics

The current server reports lexer and parser errors. It does not yet display
`code check`'s type or local-handler diagnostics, or undefined-variable errors.
Run the language CLI's `code check` and `code run --strict` to check static
contracts and local messages; run/build also refuse undefined variables.
Completion, hover, go-to-definition, references and rename are not implemented.
The server does not execute your program while you edit it.

## Settings

| Setting                         | Meaning                                                                        |
|---------------------------------|--------------------------------------------------------------------------------|
| `codelanguage.server.path`      | Launch a specific `code-lsp` binary instead of the bundled one (dev escape hatch — pin a particular build without touching the bundled binary) |
| `codelanguage.trace.server` | Off by default; turn on `verbose` to inspect raw protocol traffic when debugging |

The bundled binary is looked up under `server/<platform>/code-lsp` inside the
extension directory (e.g. `server/linux-x64/code-lsp`); its executable bit is
forced back on at startup, since archive extraction does not guarantee it. If
it is missing the extension warns once and falls back to a plain `code-lsp` on
your `PATH` — behaviour then tracks that server's version,
which is fine accidentally for one session but not worth leaving: point
`codelanguage.server.path` at a matching binary if you need it permanently.

## License

GPL-3.0-or-later, the same license as the rest of this project — see
[`LICENSE`](LICENSE).

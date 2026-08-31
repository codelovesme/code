// Client side of the `code` editor integration.
//
// This extension is deliberately a thin wrapper over `code-lsp`, the official
// language server built from this repository (`crates/code-lsp`). Everything
// heavy — parsing, formatting, semantic tokens — lives on that side and ships
// with us because the packaging builds it from exactly this checkout (see
// .github/workflows/publish-editor-vsix.yml), so the behaviour described by
// this extension always matches the engine it advertises.
//
// Transport notes: `code-lsp` speaks LSP over stdio (it takes no CLI args),
// so `ServerOptions.{command}` below launches the packed binary, hands it the
// protocol on stdout/stdin, and exits whenever VS Code does.
import * as path from 'node:path';
import * as fs from 'node:fs';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let announcedFallbackOnce = false;

// Platforms the release pipeline compiles `code-lsp` for, mirrored here. Each
// key is the value `process.platform` reports on that host; each value is the
// subfolder below `server/` where the matching binary lands inside the VSIX.
// Windows is intentionally absent (we ship no native-Windows LSP build yet);
// such hosts take the loud PATH-fallback branch below.
const BIN_SUBDIR_BY_PLATFORM: Readonly<Record<string, string>> = {
    linux: 'linux-x64',
    darwin: 'darwin-arm64',
};

/**
 * Decide which server process to spawn, in order of preference:
 *   1. the explicit setting (escape hatch for dev installs),
 *   2. the binary packed next to this extension — compiled from this commit,
 *   3. a `code-lsp` from PATH, preceded by a one-time heads-up warning
 *      because a different vintage would implement different rules.
 */
function resolveBinaryPath(context: vscode.ExtensionContext): string {
    const overridden = vscode.workspace
        .getConfiguration('codelanguage')
        .get<string>('server.path');
    if (overridden !== undefined && overridden.trim() !== '') {
        return overridden.trim();
    }

    const subdir = BIN_SUBDIR_BY_PLATFORM[process.platform];
    if (subdir !== undefined) {
        const bundled = context.asAbsolutePath(path.join('server', subdir, 'code-lsp'));
        if (fs.existsSync(bundled)) {
            // Vsix extraction is best-effort about executable bits; force it.
            fs.chmodSync(bundled, 0o755);
            return bundled;
        }
    }

    if (!announcedFallbackOnce) {
        announcedFallbackOnce = true;
        void vscode.window.showWarningMessage(
            'code language: bundled `code-lsp` not found for ' + process.platform + '; ' +
                'trying `code-lsp` on PATH instead. Behaviour tracks whatever version of `code` ' +
                    'that happens to be — point codelanguage.server.path at a matching binary to pin.',
        );
    }
    return 'code-lsp';
}

async function launch(context: vscode.ExtensionContext): Promise<void> {
    const command = resolveBinaryPath(context);

    const serverOptions: ServerOptions = { command, args: [] };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: 'code' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.code'),
        },
    };

    client = new LanguageClient(
        'codeLanguageServer',
        'Code Language Server',
        serverOptions,
        clientOptions,
    );

    try {
        await client.start();
    } catch (err) {
        void vscode.window.showErrorMessage(
            'code language: could not start the language server (' +
                (err instanceof Error ? err.message : String(err)) + '). ' +
                'Set codelanguage.trace.server=verbose, restart the window, and retry.',
        );
    }
}

export function activate(context: vscode.ExtensionContext): void {
    void launch(context);
    context.subscriptions.push({ dispose: () => void shutdown() });
}

export async function deactivate(): Promise<void> {
    await shutdown();
}

async function shutdown(): Promise<void> {
    if (client === undefined) {
        return;
    }
    const running = client;
    client = undefined;
    await running.stop().catch(() => undefined);
}

import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
  const config = vscode.workspace.getConfiguration("que");
  const enabled = config.get<boolean>("lsp.enabled", true);

  if (!enabled) {
    return;
  }

  const serverCommand = resolveServerPath(config);
  if (!serverCommand) {
    vscode.window.showWarningMessage(
      "que-lsp binary not found. Set que.lsp.path or add que-lsp to PATH."
    );
    return;
  }

  const serverOptions: ServerOptions = {
    command: serverCommand,
    args: [],
    transport: TransportKind.stdio,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "que" },
      { scheme: "untitled", language: "que" },
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/{*.que,Quefile}"),
    },
    outputChannelName: "Que Language Server",
    traceOutputChannel: vscode.window.createOutputChannel(
      "Que Language Server Trace"
    ),
  };

  client = new LanguageClient(
    "que-lsp",
    "Que Language Server",
    serverOptions,
    clientOptions
  );

  client.start();
  context.subscriptions.push({
    dispose: () => {
      if (client) {
        client.stop();
      }
    },
  });
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

function resolveServerPath(
  config: vscode.WorkspaceConfiguration
): string | undefined {
  // 1. Explicit config setting
  const explicit = config.get<string>("lsp.path", "");
  if (explicit) {
    return explicit;
  }

  // 2. Check common locations relative to the workspace
  const exe = process.platform === "win32" ? "que-lsp.exe" : "que-lsp";
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (workspaceFolders) {
    for (const folder of workspaceFolders) {
      const candidates = [
        // Cargo workspace builds to root target/ directory
        path.join(folder.uri.fsPath, "target", "release", exe),
        path.join(folder.uri.fsPath, "target", "debug", exe),
      ];
      for (const candidate of candidates) {
        try {
          const fs = require("fs");
          if (fs.existsSync(candidate)) {
            return candidate;
          }
        } catch {
          // ignore
        }
      }
    }
  }

  // 3. Fall back to PATH
  return "que-lsp";
}

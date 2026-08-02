"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.activate = activate;
exports.deactivate = deactivate;
const path = __importStar(require("path"));
const vscode = __importStar(require("vscode"));
const node_1 = require("vscode-languageclient/node");
let client;
function activate(context) {
    const config = vscode.workspace.getConfiguration("que");
    const enabled = config.get("lsp.enabled", true);
    if (!enabled) {
        return;
    }
    const serverCommand = resolveServerPath(config);
    if (!serverCommand) {
        vscode.window.showWarningMessage("que-lsp binary not found. Set que.lsp.path or add que-lsp to PATH.");
        return;
    }
    const serverOptions = {
        command: serverCommand,
        args: [],
        transport: node_1.TransportKind.stdio,
    };
    const clientOptions = {
        documentSelector: [
            { scheme: "file", language: "que" },
            { scheme: "untitled", language: "que" },
        ],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher("**/{*.que,Quefile}"),
        },
        outputChannelName: "Que Language Server",
        traceOutputChannel: vscode.window.createOutputChannel("Que Language Server Trace"),
    };
    client = new node_1.LanguageClient("que-lsp", "Que Language Server", serverOptions, clientOptions);
    client.start();
    context.subscriptions.push({
        dispose: () => {
            if (client) {
                client.stop();
            }
        },
    });
}
function deactivate() {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
function resolveServerPath(config) {
    // 1. Explicit config setting
    const explicit = config.get("lsp.path", "");
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
                }
                catch {
                    // ignore
                }
            }
        }
    }
    // 3. Fall back to PATH
    return "que-lsp";
}
//# sourceMappingURL=extension.js.map
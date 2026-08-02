# que-lsp

Language Server Protocol (LSP) implementation for the `que` scripting language.

## Features

| Feature              | Description                                                 |
|----------------------|-------------------------------------------------------------|
| **Diagnostics**      | Real-time syntax error reporting via the Que lexer/parser  |
| **Completions**      | Keywords, built-in functions, types, local symbols, snippets|
| **Hover**            | Documentation for keywords, built-ins, types, and symbols   |
| **Go to Definition** | Jump to variable, function, task, and type definitions       |
| **Document Symbols** | Outline view of functions, tasks, variables, types, enums    |
| **Semantic Tokens**  | Enhanced syntax highlighting beyond the TextMate grammar     |

## Building

```sh
cd tools/lsp
cargo build --release
```

The binary will be at `target/release/que-lsp`.

## Usage

The server communicates over **stdin/stdout** using the LSP JSON-RPC protocol.

### With VS Code

The companion VS Code extension (`tools/que_vscode`) can automatically
discover and launch the LSP server. After building:

1. Build the LSP server (see above).
2. Install the VS Code extension:
   ```sh
   cd tools/que_vscode
   npm install && npm run compile
   # Then "Install from VSIX" or use the Extensions: Install from Location… command
   ```
3. Open a `.que` file — the extension will start the server automatically.

You can also set the path explicitly in VS Code settings:

```json
{
  "que.lsp.path": "/path/to/que-lsp"
}
```

### With Neovim (nvim-lspconfig)

```lua
require('lspconfig.configs').que = {
  default_config = {
    cmd = { 'que-lsp' },
    filetypes = { 'que' },
    root_dir = function(fname)
      return require('lspconfig.util').find_git_ancestor(fname)
    end,
  },
}
require('lspconfig').que.setup{}
```

### With other editors

Any editor supporting LSP can use `que-lsp`. Configure it to run
`que-lsp` (or the full path) as a stdio language server for the
`que` language.

## Architecture

```
src/
├── main.rs          Entry point — starts the LSP server over stdin/stdout
├── server.rs        LanguageServer trait implementation (tower-lsp)
├── document.rs      In-memory document store (ropey text buffers)
├── analysis.rs      Lex → parse → extract symbols pipeline
├── diagnostics.rs   Convert parse errors to LSP diagnostics
├── completion.rs    Completion provider (keywords, builtins, symbols, snippets)
├── hover.rs         Hover provider (documentation on hover)
├── goto.rs          Go-to-definition provider
├── symbols.rs       Document symbol / outline provider
└── builtins.rs      Registry of Que built-in functions, keywords, and types
```

The server depends on the `que_lang` library crate (the same lexer and parser
used by the `que` interpreter) for accurate tokenization and parsing.

## License

MIT

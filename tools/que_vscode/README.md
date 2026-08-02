# Que Language — VS Code Extension

Syntax highlighting, language configuration, and snippets for the `que` scripting language.

## Features

### Syntax Highlighting

Full TextMate grammar covering the entire Que language:

- **Keywords** — `let`, `mut`, `fn`, `task`, `if`, `else`, `match`, `for`, `while`, `loop`, `return`, `break`, `continue`, `import`, `pub`, `defer`, `try`, `catch`, `spawn`, `parallel`, …
- **Literals** — strings (with `${interpolation}`), numbers (decimal, hex, binary, octal with `_` separators), booleans, `null`
- **Special literals** — glob `g"src/**/*.rs"`, regex `re"^\d+"`, semver `v"1.2.3"`, command backticks `` `git status` ``, duration `5s` / `500ms` / `10m` / `2h` / `1d`
- **Path builtins** — `path(...)` highlighted as a builtin call
- **Operators** — pipe `|>`, null coalesce `??`, error propagation `?`, spread `...`, ranges `..` / `..=`, arrows `=>` / `->`
- **Type names** — `Int`, `Float`, `Bool`, `String`, `Path`, `Cmd`, `Result`, `List`, `Map`, `Set`, etc.
- **Tasks** — `task name { ... }` with `@description`, `@deps`, `@inputs`, `@outputs`, `@aliases` and `@env` attributes
- **Functions** — `fn name(...)`, `pub fn`, closures `|x| expr`
- **Type declarations** — `type`, `enum`, `struct`
- **Comments** — `//`, `/* ... */`, `/// doc comments`
- **String interpolation** — `${expr}` and raw `!{expr}` inside strings and commands
- **Built-in functions** — `println`, `len`, `str`, `typeof`, `stream`, etc. plus `fs.read`, `json.parse`, `http.get` via `import std.*`

### Language Configuration

- Bracket matching and auto-closing for `{}`, `[]`, `()`, `""`, `` `` ``
- Comment toggling (`//` and `/* */`)
- Auto-indentation on `{`, `[`, `(`
- Doc-comment continuation (`///`)
- Folding via `// region` / `// endregion` markers

### Snippets

30+ snippets for common Que patterns:

| Prefix      | Description                           |
|-------------|---------------------------------------|
| `task`      | Task declaration                      |
| `taskdep`   | Task with dependencies/inputs/outputs |
| `fn`        | Function declaration                  |
| `pubfn`     | Public function                       |
| `let`       | Immutable binding                     |
| `mut`       | Mutable binding                       |
| `if`        | If/else block                         |
| `iflet`     | If-let pattern match                  |
| `match`     | Match expression                      |
| `for`       | For-in loop                           |
| `while`     | While loop                            |
| `loop`      | Infinite loop with break              |
| `import`    | Module import                         |

| `try`       | Try/catch block                       |
| `parallel`  | Parallel execution block              |
| `pipe`      | Pipe operator chain                   |
| `enum`      | Enum declaration                      |
| `struct`    | Struct type                           |
| `cmd`       | Run a command (checked)               |
| `capture`   | Capture command output                |
| `readf`     | Read file contents                    |
| `writef`    | Write file contents                   |
| `defer`     | Defer statement                       |
| `withtmp`   | Scoped temp directory                 |
| `withenv`   | Scoped env override                   |
| `pl`        | `println(...)`                        |

## File Associations

| Pattern      | Language |
|--------------|----------|
| `*.que`     | Que     |
| `Quefile`   | Que     |

## Installation

### From VSIX (local)

```bash
cd tools/que_vscode
npm install
npx vsce package
code --install-extension que-lang-0.1.0.vsix
```

### Development

1. Open this folder in VS Code
2. Press **F5** to launch an Extension Development Host
3. Open any `.que` file or `Quefile` to see highlighting

## Building

```bash
npm install
npm run package    # produces que-lang-<version>.vsix
```

## License

MIT

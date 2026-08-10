# que-std

The part of the Que standard library that is written in Que.

Everything under `std.*` (`std.fs`, `std.http`, …) is built into the
interpreter. This package holds the pieces that do not need to be: they are
ordinary Que modules, they live in this repository, and you install them like
any other dependency.

## Install

```toml
# que.toml
[dependencies]
que-std = { git = "https://github.com/jasal82/que-lang", tag = "1.1.0", subdir = "stdlib" }
```

```sh
que install
```

`subdir` is what lets one repository ship a package that is not at its root.
`que install` clones the repository into `que_packages/.sources/que_std/` and
points `que_packages/que_std/` at the `stdlib/` directory inside it, so
imports see a normal package.

The hyphen in `que-std` becomes an underscore on disk and in imports, as for
every package name.

## Modules

| Module | Import | What it does |
| --- | --- | --- |
| `colors` | `import que_std.colors { colored, Color, TextStyle }` | ANSI colors and text styles, honouring `NO_COLOR` and non-ANSI terminals |
| `select` | `import que_std.select { select }` | Interactive single-choice menu, with a plain numbered fallback when there is no terminal |

```que
import que_std.colors { colored, Color }
import que_std.select { select }

println(colored("deploying").green().bold())

let target = select("Deploy to:", ["dev", "staging", "prod"])
if target == null {
    println(colored("cancelled").yellow())
}
```

Importing the package as a whole works too — `mod.que` re-exports both
modules:

```que
import que_std

println(que_std.version())
```

Import the sub-modules directly, though, as in the example above. A
re-export carries a module's functions but not the `impl` blocks of the
types it defines, so `que_std.colors.colored(...)` would hand back a
`ColoredString` without its methods.

## Working on it locally

Inside a checkout of this repository, use a path dependency instead so edits
take effect without re-installing:

```toml
[dependencies]
que-std = { path = "../../stdlib" }
```

See [examples/stdlib](../examples/stdlib) for a runnable example that does
exactly that.

## Adding a module

1. Add `stdlib/<name>.que`. Export with `pub`; keep helpers private.
2. Add `pub import .<name>` to [mod.que](mod.que) and a row to the table above.
3. Depend only on built-in `std.*` modules or other `stdlib` modules, so the
   package stays installable on its own.

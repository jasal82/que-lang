# stdlib example

Uses the [`que-std`](../../stdlib) package: terminal colors and an
interactive select prompt.

```sh
que install     # links que_packages/que_std -> ../../stdlib
que main.que
```

The dependency in [que.toml](que.toml) is a `path` one, because the package
sits in this repository and edits to it should take effect immediately. From
anywhere else the same package is fetched from git, with `subdir` naming the
directory it lives in:

```toml
[dependencies]
que-std = { git = "https://github.com/jasal82/que-lang", tag = "1.1.0", subdir = "stdlib" }
```

`NO_COLOR=1 que main.que` shows the uncolored output, and piping the output
somewhere makes `select` fall back to a plain numbered list.

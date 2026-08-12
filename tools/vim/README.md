# Que support for Vim / Neovim

Files:

- `syntax/que.vim` — the syntax definition
- `indent/que.vim` — indentation, matching what `que fmt` produces
- `ftplugin/que.vim` — buffer options: four-space indents, comment handling
- `ftdetect/que.vim` — filetype detection for `*.que`, `Quefile`, `quefile`

## Neovim

```sh
mkdir -p ~/.config/nvim/{syntax,indent,ftplugin,ftdetect}
cp tools/vim/syntax/que.vim   ~/.config/nvim/syntax/que.vim
cp tools/vim/indent/que.vim   ~/.config/nvim/indent/que.vim
cp tools/vim/ftplugin/que.vim ~/.config/nvim/ftplugin/que.vim
cp tools/vim/ftdetect/que.vim ~/.config/nvim/ftdetect/que.vim
```

Restart Neovim and open a `.que` file or a `Quefile`. Verify with
`:set filetype?` — it should report `filetype=que`.

Indentation needs `filetype plugin indent on`, which Neovim sets by default.

Instead of the `ftdetect` file you can put this in your `init.lua`:

```lua
vim.filetype.add({
  extension = { que = "que" },
  filename = { Quefile = "que", quefile = "que" },
})
```

## Vim

```sh
mkdir -p ~/.vim/{syntax,indent,ftplugin,ftdetect}
cp tools/vim/syntax/que.vim   ~/.vim/syntax/que.vim
cp tools/vim/indent/que.vim   ~/.vim/indent/que.vim
cp tools/vim/ftplugin/que.vim ~/.vim/ftplugin/que.vim
cp tools/vim/ftdetect/que.vim ~/.vim/ftdetect/que.vim
```

Add this to your `vimrc` if it is not there already:

```vim
syntax on
filetype plugin indent on
```

## As a plugin

The directory layout already matches a normal Vim plugin, so a plugin manager
can point at it directly, e.g. with `lazy.nvim`:

```lua
{ dir = "/path/to/que/tools/vim" }
```

or with Vim 8 / Neovim packages:

```sh
mkdir -p ~/.vim/pack/plugins/start
ln -s /path/to/que/tools/vim ~/.vim/pack/plugins/start/que
```

## Indentation

New lines land where `que fmt` would put them: four spaces per level, one level
for a line that leaves a bracket open, and a closing bracket back at the level
of the line that opened it. Typing `}`, `)` or `]` re-indents the line, and `=`
works over a range, so `gg=G` lays out a whole file.

Lines inside a `"""` string, a command literal, a raw string or a block comment
are left alone — their leading whitespace belongs to the literal, not to the
program.

Pipelines keep the indent of the statement they continue, as `que fmt` writes
them:

```que
let names = items
|> map(|it| it.upper())
|> filter(|it| it != "")
```

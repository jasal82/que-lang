# Que syntax highlighting for Vim / Neovim

Files:

- `que.vim` — the syntax definition
- `ftdetect/que.vim` — filetype detection for `*.que`, `Quefile`, `quefile`

## Neovim

```sh
mkdir -p ~/.config/nvim/syntax ~/.config/nvim/ftdetect
cp tools/vim/que.vim          ~/.config/nvim/syntax/que.vim
cp tools/vim/ftdetect/que.vim ~/.config/nvim/ftdetect/que.vim
```

Restart Neovim and open a `.que` file or a `Quefile`. Verify with
`:set filetype?` — it should report `filetype=que`.

Instead of the `ftdetect` file you can put this in your `init.lua`:

```lua
vim.filetype.add({
  extension = { que = "que" },
  filename = { Quefile = "que", quefile = "que" },
})
```

## Vim

```sh
mkdir -p ~/.vim/syntax ~/.vim/ftdetect
cp tools/vim/que.vim          ~/.vim/syntax/que.vim
cp tools/vim/ftdetect/que.vim ~/.vim/ftdetect/que.vim
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

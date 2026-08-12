" Vim filetype plugin for Que
" Language:    Que
" File types:  *.que, Quefile
"
" Buffer settings that match `que fmt`: four spaces, never a tab.

if exists("b:did_ftplugin")
  finish
endif
let b:did_ftplugin = 1

setlocal expandtab
setlocal shiftwidth=4
setlocal softtabstop=4
setlocal tabstop=4

" `///` is a doc comment, so it is continued ahead of the plain `//` rule.
setlocal comments=s1:/*,mb:*,ex:*/,:///,://
setlocal commentstring=//\ %s
setlocal formatoptions-=t
setlocal formatoptions+=croqlj

setlocal suffixesadd=.que

let b:undo_ftplugin = "setlocal expandtab< shiftwidth< softtabstop< tabstop<"
      \ . " comments< commentstring< formatoptions< suffixesadd<"

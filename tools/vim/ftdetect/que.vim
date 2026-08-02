" Filetype detection for Que
augroup que_ftdetect
  autocmd!
  autocmd BufRead,BufNewFile *.que          setfiletype que
  autocmd BufRead,BufNewFile Quefile        setfiletype que
  autocmd BufRead,BufNewFile quefile        setfiletype que
augroup END

" Vim indent file for Que
" Language:    Que
" File types:  *.que, Quefile
"
" Follows what `que fmt` produces: four spaces per level, one level per
" unclosed bracket, and continuation lines (`|> …`) left at the indent of the
" statement they continue.

if exists("b:did_indent")
  finish
endif
let b:did_indent = 1

setlocal indentexpr=GetQueIndent()
setlocal indentkeys=0{,0},0),0],!^F,o,O
setlocal autoindent
setlocal nosmartindent nolisp

let b:undo_indent = "setlocal indentexpr< indentkeys< autoindent< smartindent< lisp<"

if exists("*GetQueIndent")
  finish
endif

let s:save_cpo = &cpo
set cpo&vim

" Only brackets that are code say anything about nesting, so strings, command
" literals and comments have to be recognised before they can be skipped.
" Syntax highlighting would know all of this already, but `synID()` reports
" stale groups while a `=` is running over a range, so the delimiters are
" tracked here instead.
"
" A scan carries one of these states across a line boundary:
"
"   ''        code
"   'tri'     inside a """ string
"   'cmd'     inside a `command` literal
"   'raw:N'   inside a raw string opened with N '#' characters
"   'blk:N'   inside a block comment nested N deep
"
" Everything else (plain, path, glob, regex and version strings) ends on the
" line it started on.

" What can start a literal or a comment in code. Longest alternatives come
" first: Vim takes the first branch that matches at a position.
let s:opener = '//\|/\*\|"""\|\<r#\+"\|\<re"\|\<r"\|\<p"\|\<g"\|\<v"\|"\|`'

" The subset of the above that can still be open at the end of a line. Used to
" skip the cross-line bookkeeping for files that never need it.
let s:spanning = '"""\|/\*\|`\|\<r#*"'

" The first unescaped `char` in `text` at or after `from`, or -1.
function! s:FindUnescaped(text, from, char) abort
  let i = a:from
  let len = strlen(a:text)
  while i < len
    let c = a:text[i]
    if c ==# '\'
      let i += 2
    elseif c ==# a:char
      return i
    else
      let i += 1
    endif
  endwhile
  return -1
endfunction

" Split one line into its code and the state it leaves behind.
"
" Returns [code, state]: `code` is the line with every string, command literal
" and comment taken out, `state` is what a following line would start in.
function! s:ScanLine(text, state) abort
  let text = a:text
  let state = a:state
  let code = ''
  let i = 0
  let len = strlen(text)

  while i < len
    if state ==# ''
      let start = match(text, s:opener, i)
      if start < 0
        let code .= strpart(text, i)
        break
      endif
      let code .= strpart(text, i, start - i)
      let token = matchstr(text, s:opener, i)
      let i = start + strlen(token)

      if token ==# '//'
        break
      elseif token ==# '/*'
        let state = 'blk:1'
      elseif token ==# '"""'
        let state = 'tri'
      elseif token ==# '`'
        let state = 'cmd'
      elseif token ==# 'r"'
        let state = 'raw:0'
      elseif token =~# '^r#\+"$'
        let state = 'raw:' . (strlen(token) - 2)
      else
        " A quoted literal that has to be closed on this line.
        let close = s:FindUnescaped(text, i, '"')
        if close < 0
          break
        endif
        let i = close + 1
      endif
    elseif state ==# 'tri'
      let close = match(text, '"""', i)
      if close < 0
        break
      endif
      let i = close + 3
      let state = ''
    elseif state ==# 'cmd'
      let close = s:FindUnescaped(text, i, '`')
      if close < 0
        break
      endif
      let i = close + 1
      let state = ''
    elseif state =~# '^raw:'
      let close = match(text, '"' . repeat('#', str2nr(state[4:])), i)
      if close < 0
        break
      endif
      let i = close + 1 + str2nr(state[4:])
      let state = ''
    else
      let depth = str2nr(state[4:])
      let close = match(text, '/\*\|\*/', i)
      if close < 0
        break
      endif
      let depth += strpart(text, close, 2) ==# '/*' ? 1 : -1
      let i = close + 2
      let state = depth > 0 ? 'blk:' . depth : ''
    endif
  endwhile

  return [code, state]
endfunction

" Does the buffer hold anything that can leave a line unfinished?
function! s:HasSpanning() abort
  if get(b:, 'que_spanning_tick', -1) != b:changedtick
    let b:que_spanning = match(getline(1, '$'), s:spanning) >= 0
    let b:que_spanning_tick = b:changedtick
  endif
  return b:que_spanning
endfunction

" The state every line starts in, indexed from zero.
function! s:States() abort
  if get(b:, 'que_states_tick', -1) != b:changedtick
    let states = ['']
    let state = ''
    for line in getline(1, '$')
      let state = s:ScanLine(line, state)[1]
      call add(states, state)
    endfor
    let b:que_states = states
    let b:que_states_tick = b:changedtick
  endif
  return b:que_states
endfunction

function! s:StateAt(lnum) abort
  if a:lnum <= 1 || !s:HasSpanning()
    return ''
  endif
  return get(s:States(), a:lnum - 1, '')
endfunction

" The brackets on a line, with everything that is not code removed.
function! s:Brackets(lnum) abort
  let code = s:ScanLine(getline(a:lnum), s:StateAt(a:lnum))[0]
  return substitute(code, '[^{}()\[\]]', '', 'g')
endfunction

" Does a line leave a bracket open?
"
" One level is worth one indent no matter how many brackets are involved:
" `que fmt` breaks a single level per line, so `select("…", [` is followed by
" one step of indent rather than two. Closers that have no opener on the same
" line are ignored here — they belong to a block that began earlier, and the
" line's own indent has already paid for them.
function! s:OpensBlock(lnum) abort
  let depth = 0
  for char in split(s:Brackets(a:lnum), '\zs')
    if char =~# '[{(\[]'
      let depth += 1
    elseif depth > 0
      let depth -= 1
    endif
  endfor
  return depth > 0
endfunction

" Does a line start by closing the block it sits in? Such a line belongs one
" step to the left of that block's contents.
function! s:ClosesBlock(lnum) abort
  return s:Brackets(a:lnum) =~# '^[})\]]'
endfunction

" The nearest line above `lnum` that starts real code.
"
" Blank lines carry no indent, and a line in the middle of a triple-quoted
" string or a block comment carries the literal's own layout rather than the
" program's.
function! s:PrevCodeLine(lnum) abort
  let lnum = prevnonblank(a:lnum)
  while lnum > 0
    if s:StateAt(lnum) ==# ''
      return lnum
    endif
    let lnum = prevnonblank(lnum - 1)
  endwhile
  return 0
endfunction

function! GetQueIndent() abort
  " A line that continues a multi-line literal keeps whatever the author
  " wrote: reindenting it would edit the string.
  if s:StateAt(v:lnum) !=# ''
    return -1
  endif

  let prev = s:PrevCodeLine(v:lnum - 1)
  if prev == 0
    return 0
  endif

  let ind = indent(prev) + (s:OpensBlock(prev) ? shiftwidth() : 0)
  let ind -= s:ClosesBlock(v:lnum) ? shiftwidth() : 0
  return ind > 0 ? ind : 0
endfunction

let &cpo = s:save_cpo
unlet s:save_cpo

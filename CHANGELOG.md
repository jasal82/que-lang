# Changelog

## 1.2.1 (`LANGUAGE_VERSION` 1.2.1)

String functions that take or return positions now agree on one unit: the
character (Unicode scalar value). Before, `substring` used byte offsets and
panicked inside a multi-byte character (`"aπb".substring(0, 2)`), which an
embedding host saw as an internal error.

* `len()` (also `Secret.len()` and `inspect(...).length`) counts characters,
  as documented. Use `s.bytes().len()` for the UTF-8 size.
* `substring(start, end?)` takes character indices. Indices past the end are
  clamped (a negative index clamps to the end, as before); `start >= end`
  gives `""` instead of a panic.
* `index_of(s)` returns the character index of the first match.
* `pad_start` / `pad_end` pad to a width in characters, cycling a multi-character
  fill; an empty fill pads nothing; a negative width pads nothing.
* `s[i]` with a negative `i` counts from the end in characters (it used the
  byte length, so `"aπb"[-1]` was an out-of-bounds error).
* `list.slice(start, end)` with `start >= end` gives `[]` instead of a panic.
* `std.http.url_decode("%é")` no longer panics; `std.template` no longer
  panics on an unknown directive containing non-ASCII text; `fs` glob `?`
  (`ls(pattern)`) matches one character instead of one byte.

Scripts that only use ASCII text behave as before. Scripts that relied on
byte lengths or offsets of non-ASCII text must switch to `bytes()`.

## 1.2.0

Embedding API (`que_lang::embed`), see `docs/embedding.md`.

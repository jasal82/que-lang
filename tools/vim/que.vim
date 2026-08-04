" Vim syntax file for Que
" Language:    Que
" File types:  *.que, Quefile
" Maintainer:  Generated from que.tmLanguage.json
" Last Change: 2026

if exists("b:current_syntax")
  finish
endif

" ─── Prologue ────────────────────────────────────────────────────────────────

syntax match   queShebang       "\%^#!/.*$"
syntax match   quePragma        "^#!\w\+$"

" ─── Comments ────────────────────────────────────────────────────────────────

syntax match   queDocComment    "///.*$"
syntax match   queLineComment   "//.*$"
syntax region  queBlockComment  start="/\*"  end="\*/"

" ─── Keywords ────────────────────────────────────────────────────────────────

" Control flow
syntax keyword queControl
      \ if else match for in while loop return break continue
      \ defer try catch finally with where

" Declarations
syntax keyword queDeclaration
      \ fn task type enum struct impl trait

" Variable binding / storage
syntax keyword queBinding
      \ let mut

" Visibility / module
syntax keyword queModifier
      \ pub import from as

" Concurrency / execution
syntax keyword queConcurrency
      \ spawn parallel

" Task attributes, written above the declaration
syntax match   queTaskProp
      \ "^\s*@\(deps\|description\|inputs\|outputs\|aliases\|env\)\ze("

" ─── Task and function names ─────────────────────────────────────────────────

syntax match   queTaskKeyword  "\<task\>" nextgroup=queTaskName skipwhite
syntax match   queTaskName     "\<[a-zA-Z_][a-zA-Z0-9_]*\>"
      \ contained

syntax match   queFnKeyword    "\<fn\>" nextgroup=queFnName skipwhite
syntax match   queFnName       "\<[a-zA-Z_][a-zA-Z0-9_]*\>"
      \ contained

" ─── Type declarations ───────────────────────────────────────────────────────

syntax match   queTypeKeyword  "\<\(type\|enum\|struct\)\>" nextgroup=queTypeName skipwhite
syntax match   queTypeName     "\<[A-Z][a-zA-Z0-9_]*\>"
      \ contained

" Built-in types
syntax keyword queBuiltinType
      \ Int Float Bool String Bytes Path Glob Cmd Duration Timestamp
      \ Regex Semver Secret List Map Set Result Ok Err
      \ ProcessHandle ProcessResult FileHandle Stream
      \ TempDir TempFile TypeInfo VarInfo OsInfo Logger DateTime
      \ Any Null

" ─── Constants ───────────────────────────────────────────────────────────────

syntax keyword queBoolean   true false
syntax keyword queNull      null

" ─── Numbers ─────────────────────────────────────────────────────────────────

syntax match   queHex       "\<0x[0-9a-fA-F][0-9a-fA-F_]*\>"
syntax match   queBin       "\<0b[01][01_]*\>"
syntax match   queOct       "\<0o[0-7][0-7_]*\>"
syntax match   queFloat     "\<\d[0-9_]*\.\d[0-9_]*\([eE][+-]\?\d\+\)\?\>"
syntax match   queInt       "\<\d[0-9_]*\>"
syntax match   queDuration  "\<\d[0-9_]*\(ms\|[smhd]\)\>"

" ─── Strings ─────────────────────────────────────────────────────────────────

" Escape sequences (shared by double-quoted and triple-quoted strings)
syntax match   queEscape
      \ "\\[nrtv0'\"\\]\|\\x[0-9a-fA-F]\{2\}\|\\u{[0-9a-fA-F]\{1,6\}}"
      \ contained

" String interpolation ${ ... } and !{ ... }  (shared)
syntax region  queInterp
      \ matchgroup=queInterpDelim
      \ start="\${"  end="}"
      \ contained contains=TOP

syntax region  queRawInterp
      \ matchgroup=queInterpDelim
      \ start="!{"   end="}"
      \ contained contains=TOP

" Triple-quoted string
syntax region  queTripleString
      \ start='"""'  end='"""'
      \ contains=queEscape,queInterp,queRawInterp

" Raw strings  r"..."  and  r#"..."#
syntax region  queRawString   start='r"'   end='"'
syntax region  queRawHashStr  start='r#"'  end='"#'

" Regex literal  re"..."
syntax region  queRegex       start='re"'  end='"'

" Semver literal  v"..."
syntax region  queSemver      start='v"'   end='"'

" Ordinary double-quoted string
syntax region  queString
      \ start='"'  end='"'  skip='\\"'
      \ contains=queEscape,queInterp,queRawInterp

" Backtick command literal  `...`
syntax region  queCommand
      \ start='`'  end='`'
      \ contains=queInterp,queRawInterp

" ─── Operators ───────────────────────────────────────────────────────────────

syntax match   queOperator "|>\|=>\|->\|\.\.\.\|\.\.=\|\.\.\|==\|!=\|<=\|>=\|&&\|||\|+=\|-=\|\*=\|/=\|%=\|\*\*\|??\|?\|[-+*/%<>!&|^~=]"

" ─── Built-in functions ──────────────────────────────────────────────────────

syntax keyword queBuiltinFn
      \ print println input confirm typeof str int float bool
      \ abs min max range chr ord
      \ assert compose
      \ secret fail sleep quefile_dir script_dir dry_run
      \ path glob open which retry timeout semver_parse regex
      \ cd
      \ dbg help args
      \ tasks run_task strict

" ─── Function / method calls  (general) ──────────────────────────────────────

syntax match   queFnCall    "\<[a-zA-Z_][a-zA-Z0-9_]*\ze\s*("
syntax match   queMethodCall "\.[a-zA-Z_][a-zA-Z0-9_]*\ze\s*("  contains=queDot

" ─── Namespaces ──────────────────────────────────────────────────────────────

syntax match   queNamespace "\<[a-zA-Z_][a-zA-Z0-9_]*\ze\s*\.\s*[a-zA-Z_]"

" Built-in namespace objects and bundled std modules take precedence over the
" generic rule above (Vim gives the last-defined item priority).
syntax match   queNamespaceBuiltin
      \ "\<\(env\|os\)\>\ze\s*\."
syntax match   queStdModule
      \ "\<\(archive\|config\|container\|csv\|dotenv\|fs\|git\|hash\|http\|json\|log\|net\|prompt\|reflect\|ssh\|stream\|template\|time\|toml\|tty\|watch\|yaml\)\>\ze\s*\."

" ─── Closure parameters  |param, ...| ────────────────────────────────────────

syntax region  queClosureParams
      \ matchgroup=queClosureBar
      \ start="\(^\|[^|]\)\zs|"  end="|"
      \ contains=queClosureParam
syntax match   queClosureParam  "[a-zA-Z_][a-zA-Z0-9_]*"  contained

" ─── Highlight links ─────────────────────────────────────────────────────────

highlight default link queDocComment     SpecialComment
highlight default link queLineComment    Comment
highlight default link queBlockComment   Comment
highlight default link queShebang        Comment
highlight default link quePragma         PreProc

highlight default link queControl        Keyword
highlight default link queDeclaration    Keyword
highlight default link queBinding        StorageClass
highlight default link queModifier       PreProc
highlight default link queConcurrency    Keyword
highlight default link queTaskProp       Identifier

highlight default link queTaskKeyword    Keyword
highlight default link queTaskName       Function
highlight default link queFnKeyword      Keyword
highlight default link queFnName         Function
highlight default link queTypeKeyword    Keyword
highlight default link queTypeName       Type
highlight default link queBuiltinType    Type

highlight default link queBoolean        Boolean
highlight default link queNull           Constant

highlight default link queHex            Number
highlight default link queBin            Number
highlight default link queOct            Number
highlight default link queFloat          Float
highlight default link queInt            Number
highlight default link queDuration       Number

highlight default link queEscape         SpecialChar
highlight default link queInterpDelim    Delimiter
highlight default link queTripleString   String
highlight default link queRawString      String
highlight default link queRawHashStr     String
highlight default link queRegex          String
highlight default link queSemver         String
highlight default link queString         String
highlight default link queCommand        String

highlight default link queOperator       Operator

highlight default link queBuiltinFn      Function
highlight default link queFnCall         Function
highlight default link queMethodCall     Function
highlight default link queNamespace      Identifier
highlight default link queNamespaceBuiltin Structure
highlight default link queStdModule      Structure

highlight default link queClosureBar     Delimiter
highlight default link queClosureParam   Identifier

let b:current_syntax = "que"

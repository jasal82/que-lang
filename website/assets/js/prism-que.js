/*
 * Prism.js language definition for Que (https://github.com/jasal82/que-lang)
 *
 * Que has several literal forms that don't exist in mainstream languages
 * (path/glob/regex/semver/duration/command literals, ${} interpolation
 * inside strings *and* path/glob literals, an @attribute syntax on tasks,
 * and a |> pipe operator) so it needs a dedicated grammar rather than
 * reuse of an existing "close enough" language.
 */
(function (Prism) {
    Prism.languages.que = {
        'comment': {
            pattern: /\/\/.*/,
            greedy: true
        },

        // Triple-quoted strings: """ ... """ (checked before single-line strings)
        'triple-string': {
            pattern: /"""[\s\S]*?"""/,
            greedy: true,
            alias: 'string',
            inside: {
                'interpolation': {
                    pattern: /\$\{[^}]*\}/,
                    inside: { 'punctuation': /^\$\{|\}$/ }
                }
            }
        },

        // Raw strings: r"...", r#"..."#, r##"..."##  (no interpolation, no escapes)
        'raw-string': {
            pattern: /r#{0,8}"(?:[^"]|"(?!#{0,8}(?:$|[^#"])))*?"#{0,8}/,
            greedy: true,
            alias: 'string'
        },

        // Command literals: `git status`, with ${} interpolation
        'command': {
            pattern: /`(?:\\.|\$\{[^}]*\}|[^`\\])*`/,
            greedy: true,
            inside: {
                'interpolation': {
                    pattern: /\$\{[^}]*\}/,
                    inside: { 'punctuation': /^\$\{|\}$/ }
                },
                'punctuation': /`/
            }
        },

        // Typed literals with a letter prefix directly before the quote:
        // p"...", g"...", v"...", re"..."  — all support ${} interpolation
        // except regex, which is checked first since "re" could also just
        // be an identifier followed by a string-looking thing elsewhere.
        'regex-literal': {
            pattern: /\bre"(?:\\.|[^"\\])*"/,
            greedy: true,
            alias: 'regex'
        },
        'path-literal': {
            pattern: /\bp"(?:\\.|\$\{[^}]*\}|[^"\\])*"/,
            greedy: true,
            alias: 'string',
            inside: {
                'interpolation': {
                    pattern: /\$\{[^}]*\}/,
                    inside: { 'punctuation': /^\$\{|\}$/ }
                },
                'punctuation': /^p"|"$/
            }
        },
        'glob-literal': {
            pattern: /\bg"(?:\\.|\$\{[^}]*\}|[^"\\])*"/,
            greedy: true,
            alias: 'string',
            inside: {
                'interpolation': {
                    pattern: /\$\{[^}]*\}/,
                    inside: { 'punctuation': /^\$\{|\}$/ }
                },
                'punctuation': /^g"|"$/
            }
        },
        'semver-literal': {
            pattern: /\bv"(?:\\.|[^"\\])*"/,
            greedy: true,
            alias: 'string'
        },

        // Regular interpolated strings
        'string': {
            pattern: /"(?:\\.|\$\{[^}]*\}|[^"\\])*"/,
            greedy: true,
            inside: {
                'interpolation': {
                    pattern: /\$\{[^}]*\}/,
                    inside: { 'punctuation': /^\$\{|\}$/ }
                }
            }
        },

        // Duration literals: 30s, 500ms, 2h, 7d, 15m — unit must directly
        // follow the digits with no identifier characters after it.
        'duration': {
            pattern: /\b\d[\d_]*(?:ms|s|m|h|d)\b/,
            alias: 'number'
        },

        'attribute': {
            pattern: /@[a-zA-Z_][a-zA-Z0-9_]*/,
            alias: 'function'
        },

        'keyword': /\b(?:let|mut|fn|pub|task|struct|enum|impl|trait|type|if|else|match|for|in|while|loop|return|break|continue|import|as|true|false|null|defer|try|catch|finally|with|spawn|parallel)\b/,

        'builtin': /\b(?:env|os|path|glob|args|println|print|typeof|str|int|float|bool|abs|min|max|range|chr|ord|Ok|Err|open|secret|assert|fail|dry_run|sleep|compose|retry|timeout|semver_parse|which|dbg|strict|tasks|run_task|regex|input|confirm|script_dir|quefile_dir|help)\b/,

        'number': /\b0x[0-9a-fA-F]+\b|\b0o[0-7]+\b|\b\d[\d_]*(?:\.\d+)?\b/,

        'function': {
            pattern: /\b[a-zA-Z_][a-zA-Z0-9_]*(?=\s*\()/,
        },

        'operator': /\|>|\?\?|\?\.|\.\.=|\.\.|->|=>|&&|\|\||==|!=|<=|>=|<<|>>|\*\*|[-+*/%&|^~!<>=?]/,

        'punctuation': /[{}[\]();,.:]/
    };

    // Field access / namespace calls like `fs.read`, `std.time` should not
    // be highlighted as generic functions when not followed by `(` — the
    // 'function' rule above already restricts to call position, so plain
    // property access falls through to plain text, which is intentional.
}(Prism));

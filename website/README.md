# que-lang.com

The Que marketing site and documentation, built as a static site with no
build tooling required beyond Python 3 for the docs generator.

```
website/
  index.html            landing page
  docs/
    index.html           generated — do not edit by hand, see below
  assets/
    css/                 base.css, site.css, docs.css, prism-theme.css
    js/                  main.js + vendored Prism core, plugins, prism-que.js
    img/                 favicon
  build/
    build_docs.py         the docs generator
    docs_template.html    HTML shell the generator fills in
  CNAME                  GitHub Pages custom domain (que-lang.com)
  robots.txt, sitemap.xml
```

## Single source of truth

`website/docs/index.html` is **generated** from [`tutorial2.md`](../tutorial2.md)
at the repository root — that file is the only place documentation content
should be edited. Do not hand-edit `website/docs/index.html`; the next build
overwrites it.

```sh
python3 website/build/build_docs.py
```

To rebuild automatically while editing the tutorial:

```sh
python3 website/build/build_docs.py --watch
```

The generator is stdlib-only Python (no pip install, no Node), so it runs
anywhere, including CI. It:

- Parses the markdown subset `tutorial2.md` actually uses (headings, fenced
  code blocks, tables, blockquotes — including a fenced code block nested
  inside one — lists, and inline `**bold**` / `` `code` `` / `[links](...)`).
- Regenerates the sidebar navigation and chapter anchors directly from the
  document's own headings, using the same slug algorithm GitHub uses, so the
  `#N-slug` links already written throughout `tutorial2.md` resolve.
- Tags each fenced code block with the right Prism language class —
  `` ```que `` blocks get the custom grammar in `assets/js/prism-que.js`.
- Adds a prev/next pager between chapters.

If you change the *structure* of `tutorial2.md` (add a chapter, rename a
`# Part` heading, etc.) just rerun the generator — nothing else to update.

## Que syntax highlighting

There is no existing Prism/highlight.js grammar for Que, so
`assets/js/prism-que.js` is a hand-written one covering the language's
distinctive literal forms: path (`p"..."`), glob (`g"..."`), semver
(`v"..."`), regex (`re"..."`) and command (`` `...` ``) literals, `${}`
string interpolation (including inside path/glob/command literals), duration
literals (`30s`, `500ms`), the `@attribute` task syntax, and the `|>` `??`
`?.` operators. It's vendored alongside a trimmed Prism core (no built-in
language pack) plus `bash` and `toml` for the other fenced-block languages
the docs use.

## Local preview

Any static file server works:

```sh
cd website
python3 -m http.server 8000
# → http://localhost:8000/
```

## Deploying

The site is plain static files — upload the contents of `website/` to any
static host (GitHub Pages, Netlify, Cloudflare Pages, S3 + CloudFront, nginx,
...). `website/` is the web root; nothing above it should be published.

**GitHub Pages with the `que-lang.com` domain:**

1. Push `website/` (e.g. via a `gh-pages` branch or the `docs/`-folder
   Pages source, adjusting paths accordingly) — the included `CNAME` file
   already points at `que-lang.com`.
2. At your DNS provider, point `que-lang.com` at GitHub Pages (an `ALIAS`/
   `ANAME` or the four GitHub Pages `A` records for an apex domain, or a
   `CNAME` record if using a `www` subdomain instead).
3. Enable "Enforce HTTPS" in the repository's Pages settings once the domain
   verifies.

**Any other static host:** upload `website/`'s contents to the site root and
point the domain there; drop the `CNAME` file if the host doesn't use the
GitHub Pages convention.

## Before every deploy

```sh
python3 website/build/build_docs.py   # docs reflect the latest tutorial2.md
```

There's no separate asset build step — CSS and JS are hand-written and
served as-is.

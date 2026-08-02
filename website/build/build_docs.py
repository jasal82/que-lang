#!/usr/bin/env python3
"""
Builds website/docs/index.html from ../tutorial2.md (the single source of
truth for Que's documentation). No third-party dependencies — stdlib only,
so this runs anywhere Python 3 runs, including CI.

Usage:
    python3 website/build/build_docs.py
    python3 website/build/build_docs.py --watch      # rebuild on save

The markdown subset understood here is exactly the subset tutorial2.md
uses: ATX headings (#..####), fenced code blocks, GFM tables, blockquotes
(including a fenced code block nested inside one), unordered/ordered lists,
horizontal rules, and inline **bold** / `code` / [text](link) / plain text.
It is deliberately not a general-purpose markdown engine.
"""
import html
import re
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
SITE = Path(__file__).resolve().parent.parent
TUTORIAL = ROOT / "tutorial2.md"
OUT_DIR = SITE / "docs"
OUT_FILE = OUT_DIR / "index.html"
TEMPLATE = Path(__file__).resolve().parent / "docs_template.html"

FENCE_LANG_MAP = {
    "que": "language-que",
    "sh": "language-bash",
    "bash": "language-bash",
    "toml": "language-toml",
    "console": "",
    "text": "",
    "": "",
}


def slugify(text: str) -> str:
    """Mirrors GitHub's markdown heading slugger closely enough that the
    #N-anchor links already written throughout tutorial2.md resolve."""
    text = text.lower()
    text = re.sub(r"[^\w\s-]", "", text)
    text = re.sub(r"\s", "-", text)
    return text


# ---------------------------------------------------------------------------
# Inline formatting: bold, inline code (1 or 2 backticks), links, escaping.
# ---------------------------------------------------------------------------

_CODE_SPAN_RE = re.compile(r"(``.+?``|`[^`]+?`)")
_LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
_BOLD_RE = re.compile(r"\*\*(.+?)\*\*")


def render_inline(text: str) -> str:
    # 1. Pull out inline code spans first so nothing inside them is touched
    #    by bold/link/escaping rules below.
    placeholders = []

    def stash_code(m):
        raw = m.group(1)
        if raw.startswith("``"):
            inner = raw[2:-2]
        else:
            inner = raw[1:-1]
        inner = inner.strip(" ") if inner.startswith(" ") and inner.endswith(" ") else inner
        placeholders.append(f'<code>{html.escape(inner)}</code>')
        return f"\x00{len(placeholders) - 1}\x00"

    text = _CODE_SPAN_RE.sub(stash_code, text)

    # 2. Escape remaining raw text.
    text = html.escape(text, quote=False)

    # 3. Bold.
    text = _BOLD_RE.sub(r"<strong>\1</strong>", text)

    # 4. Links — href may be an internal #anchor or an external URL.
    def link_sub(m):
        label, href = m.group(1), m.group(2)
        external = href.startswith("http://") or href.startswith("https://")
        attrs = ' target="_blank" rel="noopener"' if external else ""
        return f'<a href="{html.escape(href)}"{attrs}>{label}</a>'

    text = _LINK_RE.sub(link_sub, text)

    # 5. Restore code spans.
    for i, code_html in enumerate(placeholders):
        text = text.replace(f"\x00{i}\x00", code_html)

    return text


# ---------------------------------------------------------------------------
# Table cell splitting that respects backtick spans and escaped pipes.
# ---------------------------------------------------------------------------

def split_table_row(line: str):
    line = line.strip()
    if line.startswith("|"):
        line = line[1:]
    if line.endswith("|"):
        line = line[:-1]
    cells, buf, in_code, i = [], [], False, 0
    while i < len(line):
        ch = line[i]
        if ch == "\\" and i + 1 < len(line) and line[i + 1] == "|":
            buf.append("|")
            i += 2
            continue
        if ch == "`":
            in_code = not in_code
            buf.append(ch)
        elif ch == "|" and not in_code:
            cells.append("".join(buf).strip())
            buf = []
        else:
            buf.append(ch)
        i += 1
    cells.append("".join(buf).strip())
    return cells


_SEP_RE = re.compile(r"^\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?$")


# ---------------------------------------------------------------------------
# Block-level parser
# ---------------------------------------------------------------------------

class Parser:
    def __init__(self, lines):
        self.lines = lines
        self.i = 0
        self.n = len(lines)
        self.headings = []  # (level, text, id) in document order, for nav

    def peek(self):
        return self.lines[self.i] if self.i < self.n else None

    def parse_blocks(self):
        out = []
        while self.i < self.n:
            line = self.lines[self.i]

            if line.strip() == "":
                self.i += 1
                continue

            if line.startswith("```"):
                out.append(self.parse_fence())
                continue

            if line.startswith("#"):
                out.append(self.parse_heading())
                continue

            if line.startswith(">"):
                out.append(self.parse_blockquote())
                continue

            if line.strip() == "---":
                self.i += 1
                continue

            if line.startswith("|") and self.i + 1 < self.n and _SEP_RE.match(self.lines[self.i + 1].strip()):
                out.append(self.parse_table())
                continue

            if re.match(r"^\s*[-*]\s+", line):
                out.append(self.parse_list(ordered=False))
                continue

            if re.match(r"^\s*\d+\.\s+", line):
                out.append(self.parse_list(ordered=True))
                continue

            out.append(self.parse_paragraph())

        return out

    def parse_heading(self):
        line = self.lines[self.i]
        self.i += 1
        m = re.match(r"^(#{1,6})\s+(.*)$", line)
        level = len(m.group(1))
        text = m.group(2).strip()
        anchor = slugify(text)
        self.headings.append((level, text, anchor))
        return {"type": "heading", "level": level, "text": text, "id": anchor}

    def parse_fence(self):
        open_line = self.lines[self.i]
        fence_char = open_line[:3]
        lang = open_line[3:].strip()
        self.i += 1
        body = []
        while self.i < self.n and not self.lines[self.i].startswith(fence_char):
            body.append(self.lines[self.i])
            self.i += 1
        if self.i < self.n:
            self.i += 1  # consume closing fence
        return {"type": "code", "lang": lang, "code": "\n".join(body)}

    def parse_blockquote(self):
        raw = []
        while self.i < self.n and (self.lines[self.i].startswith(">") or self.lines[self.i].strip() == ""):
            if self.lines[self.i].strip() == "":
                # A blank line ends the quote unless the next line still
                # starts with '>' (rare here, but keep it simple/safe).
                if self.i + 1 < self.n and self.lines[self.i + 1].startswith(">"):
                    raw.append("")
                    self.i += 1
                    continue
                break
            line = self.lines[self.i]
            stripped = line[1:]
            if stripped.startswith(" "):
                stripped = stripped[1:]
            raw.append(stripped)
            self.i += 1
        inner = Parser(raw)
        blocks = inner.parse_blocks()
        self.headings.extend(inner.headings)
        return {"type": "blockquote", "blocks": blocks}

    def parse_table(self):
        header = split_table_row(self.lines[self.i])
        self.i += 2  # header + separator
        rows = []
        while self.i < self.n and self.lines[self.i].strip().startswith("|"):
            rows.append(split_table_row(self.lines[self.i]))
            self.i += 1
        return {"type": "table", "header": header, "rows": rows}

    def parse_list(self, ordered):
        items = []
        pattern = re.compile(r"^\s*\d+\.\s+(.*)$") if ordered else re.compile(r"^\s*[-*]\s+(.*)$")
        while self.i < self.n:
            line = self.lines[self.i]
            m = pattern.match(line)
            if not m:
                break
            text = m.group(1)
            self.i += 1
            # lazy continuation: an indented or plain follow-up line that
            # belongs to the same item (tutorial2.md uses this rarely, but
            # the "Discovery."-style bullets sometimes wrap).
            while self.i < self.n and self.lines[self.i].strip() != "" and not pattern.match(self.lines[self.i]) \
                    and not self.lines[self.i].startswith("#") and not self.lines[self.i].startswith("```") \
                    and not self.lines[self.i].startswith(">") and self.lines[self.i].strip() != "---" \
                    and not re.match(r"^\s*[-*]\s+", self.lines[self.i]):
                text += " " + self.lines[self.i].strip()
                self.i += 1
            items.append(text)
        return {"type": "list", "ordered": ordered, "items": items}

    def parse_paragraph(self):
        buf = []
        while self.i < self.n:
            line = self.lines[self.i]
            if line.strip() == "" or line.startswith("#") or line.startswith("```") \
                    or line.startswith(">") or line.strip() == "---" \
                    or (line.startswith("|") and self.i + 1 < self.n and _SEP_RE.match(self.lines[self.i + 1].strip())) \
                    or re.match(r"^\s*[-*]\s+", line) or re.match(r"^\s*\d+\.\s+", line):
                break
            buf.append(line.strip())
            self.i += 1
        return {"type": "paragraph", "text": " ".join(buf)}


# ---------------------------------------------------------------------------
# HTML rendering
# ---------------------------------------------------------------------------

def render_code(block):
    lang = block["lang"]
    css_class = FENCE_LANG_MAP.get(lang, "")
    label = lang if lang else "text"
    code_html = html.escape(block["code"])
    cls_attr = f' class="{css_class}"' if css_class else ""
    return (
        f'<div class="code-block"><span class="code-lang">{html.escape(label)}</span>'
        f'<pre{cls_attr}><code{cls_attr}>{code_html}</code></pre></div>'
    )


def render_blocks(blocks, depth=0):
    out = []
    for b in blocks:
        t = b["type"]
        if t == "heading":
            level = b["level"]
            text_html = render_inline(b["text"])
            if level == 1:
                out.append(f'<div class="docs-part-heading">{text_html}</div>')
            else:
                m = re.match(r"^(\d+)\.\s+(.*)$", b["text"])
                if level == 2 and m:
                    num, rest = m.group(1), render_inline(m.group(2))
                    inner = f'<span class="chapter-no">{num}.</span>{rest}'
                else:
                    inner = text_html
                out.append(
                    f'<h{level} id="{b["id"]}">{inner}'
                    f'<a class="anchor-link" href="#{b["id"]}" aria-label="Permalink">#</a></h{level}>'
                )
        elif t == "code":
            out.append(render_code(b))
        elif t == "paragraph":
            if b["text"]:
                out.append(f'<p>{render_inline(b["text"])}</p>')
        elif t == "blockquote":
            out.append(f'<blockquote>{render_blocks(b["blocks"], depth + 1)}</blockquote>')
        elif t == "list":
            tag = "ol" if b["ordered"] else "ul"
            items = "".join(f"<li>{render_inline(item)}</li>" for item in b["items"])
            out.append(f"<{tag}>{items}</{tag}>")
        elif t == "table":
            thead = "".join(f"<th>{render_inline(c)}</th>" for c in b["header"])
            rows = "".join(
                "<tr>" + "".join(f"<td>{render_inline(c)}</td>" for c in r) + "</tr>"
                for r in b["rows"]
            )
            out.append(f"<table><thead><tr>{thead}</tr></thead><tbody>{rows}</tbody></table>")
    return "\n".join(out)


# ---------------------------------------------------------------------------
# Top-level document split: intro / TOC (skipped, regenerated) / chapters
# ---------------------------------------------------------------------------

def split_source(text: str):
    lines = text.split("\n")
    toc_start = next(i for i, l in enumerate(lines) if l.strip() == "## Table of Contents")
    intro_lines = lines[:toc_start]
    # drop the trailing '---' right before the TOC heading
    while intro_lines and intro_lines[-1].strip() in ("", "---"):
        intro_lines.pop()

    part1_start = next(i for i, l in enumerate(lines) if l.startswith("# Part I"))
    body_lines = lines[part1_start:]
    return intro_lines, body_lines


def build_nav(headings):
    """headings: list of (level, text, id) across the whole body.
    Groups h2 chapters under their most recent h1 part."""
    groups = []
    current = None
    for level, text, anchor in headings:
        if level == 1:
            current = {"title": text.split("—", 1)[-1].strip() if "—" in text else text, "items": []}
            groups.append(current)
        elif level == 2 and current is not None:
            current["items"].append((text, anchor))
    parts_html = []
    for g in groups:
        items_html = "".join(
            f'<li><a href="#{anchor}">{html.escape(text)}</a></li>' for text, anchor in g["items"]
        )
        parts_html.append(
            f'<div class="docs-part">{html.escape(g["title"])}</div>'
            f'<ul class="docs-nav-list">{items_html}</ul>'
        )
    return "\n".join(parts_html)


def build_pager(headings):
    """Return {anchor: (prev, next)} where prev/next are (title, href) or None,
    walking only chapter-level (h2) headings."""
    chapters = [(text, anchor) for level, text, anchor in headings if level == 2]
    pager = {}
    for idx, (text, anchor) in enumerate(chapters):
        prev = chapters[idx - 1] if idx > 0 else None
        nxt = chapters[idx + 1] if idx < len(chapters) - 1 else None
        pager[anchor] = (prev, nxt)
    return pager


def inject_pagers(content_blocks_html, headings):
    """Append a prev/next pager after each chapter's content by splitting on
    the h2 markers already present in the rendered HTML."""
    pager = build_pager(headings)
    chapter_ids = [anchor for level, _, anchor in headings if level == 2]
    if not chapter_ids:
        return content_blocks_html

    pattern = re.compile(r'(<h2 id="([a-z0-9-]+)")')
    matches = list(pattern.finditer(content_blocks_html))
    if not matches:
        return content_blocks_html

    pieces = []
    last_end = 0
    current_id = None
    for m in matches:
        chunk = content_blocks_html[last_end:m.start()]
        if current_id is not None:
            pieces.append(chunk)
            pieces.append(render_pager(current_id, pager))
        else:
            pieces.append(chunk)
        current_id = m.group(2)
        last_end = m.start()
    tail = content_blocks_html[last_end:]
    pieces.append(tail)
    if current_id is not None:
        pieces.append(render_pager(current_id, pager))
    return "".join(pieces)


def render_pager(chapter_id, pager):
    prev, nxt = pager.get(chapter_id, (None, None))
    parts = ['<div class="docs-pager">']
    if prev:
        parts.append(
            f'<a href="#{prev[1]}" class="pager-prev"><span class="pager-dir">&larr; Previous</span>'
            f'<span class="docs-pager-title">{html.escape(prev[0])}</span></a>'
        )
    else:
        parts.append('<span></span>')
    if nxt:
        parts.append(
            f'<a href="#{nxt[1]}" class="pager-next"><span class="pager-dir">Next &rarr;</span>'
            f'<span class="docs-pager-title">{html.escape(nxt[0])}</span></a>'
        )
    parts.append('</div>')
    return "".join(parts)


def build():
    if not TUTORIAL.exists():
        sys.exit(f"error: {TUTORIAL} not found")

    text = TUTORIAL.read_text(encoding="utf-8")
    intro_lines, body_lines = split_source(text)

    intro_parser = Parser(intro_lines)
    intro_blocks = intro_parser.parse_blocks()
    # first block is the H1 title
    title_text = intro_blocks[0]["text"] if intro_blocks and intro_blocks[0]["type"] == "heading" else "Que"
    intro_rest = intro_blocks[1:]
    intro_html = render_blocks(intro_rest)

    body_parser = Parser(body_lines)
    body_blocks = body_parser.parse_blocks()
    body_html = render_blocks(body_blocks)
    body_html = inject_pagers(body_html, body_parser.headings)

    nav_html = build_nav(body_parser.headings)

    template = TEMPLATE.read_text(encoding="utf-8")
    out = (
        template
        .replace("{{TITLE}}", html.escape(title_text))
        .replace("{{NAV}}", nav_html)
        .replace("{{INTRO}}", intro_html)
        .replace("{{BODY}}", body_html)
        .replace("{{BUILD_TIME}}", time.strftime("%Y-%m-%d"))
    )

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    OUT_FILE.write_text(out, encoding="utf-8")
    print(f"wrote {OUT_FILE} ({len(out):,} bytes) from {TUTORIAL.relative_to(ROOT)}")
    print(f"  {len(body_parser.headings)} headings, "
          f"{sum(1 for l, _, _ in body_parser.headings if l == 2)} chapters")


def watch():
    print(f"watching {TUTORIAL} — Ctrl-C to stop")
    last = None
    while True:
        try:
            mtime = TUTORIAL.stat().st_mtime
            if mtime != last:
                build()
                last = mtime
            time.sleep(0.5)
        except KeyboardInterrupt:
            print()
            return


if __name__ == "__main__":
    if "--watch" in sys.argv:
        build()
        watch()
    else:
        build()

#!/usr/bin/env python3
"""Sync the three.js boot-intro from the report template into existing reports.

WHY ANCHOR-BASED INSTEAD OF DIFF-REPLAY
---------------------------------------
`reports/*/full-report.html` are generated artifacts: the template with
`{{PLACEHOLDER}}` substituted and `data-page-node-id` attributes stripped.
They were produced by *different template revisions*, so replaying the
template's `git diff HEAD` onto them fails on ~half the hunks (stale context)
and — worse — silently drops the critical 8.5KB boot script, leaving the
overlay permanently on screen (black page forever).

Instead we splice the three well-delimited boot blocks straight out of the
*current* template, using stable text anchors:

  1. BOOT CSS   `/* ═══ BOOT SEQUENCE OVERLAY ... ═══ */` → `/* ═══ RESPONSIVE · TABLET ═══ */`
  2. BOOT HTML  `<!-- ─── BOOT OVERLAY ... ─── -->`       → `<div class="container"`
  3. SCRIPT     the whole inline `<script>` block (boot IIFE + content IIFE), verbatim

The template's preview layer is a *separate* `<script src=...>` tag, not part of
the inline block, so it never enters `boot_js`; it is stripped from reports in
step 0.5 below.

Note the boot-HTML end anchor is `<div class="container"`, not `</noscript>`:
older reports predate the `<noscript>` guard, so anchoring on it would miss.

The script block is replaced wholesale rather than patched, because the boot
IIFE and the content IIFE share one `<script>` tag and the content IIFE's
entrance timeline is coupled to the boot's dismissal — they must move together.

Also normalises the 7 `image-rendering: pixelated` declarations to `auto`
(avatars are now photos, not 8-bit sprites) and strips any `data-page-node-id`
that leaked in.

The operation is idempotent — re-running it is a no-op.

Usage: python3 tools/sync_boot_intro.py [report_dir ...]
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "assets" / "report-template.html"

NODE_ID = re.compile(r'\s*data-page-node-id="[^"]*"')
PIXELATED = re.compile(r'image-rendering:\s*pixelated\s*;')

CSS_START = "/* ═══════════════ BOOT SEQUENCE OVERLAY · three.js 多空对撞 ═══════════════ */"
CSS_END = "/* ═══════════════ RESPONSIVE · TABLET ═══════════════ */"
HTML_START = "<!-- ─── BOOT OVERLAY · three.js 多空对撞 ─── -->"
HTML_NEXT = '\n<div class="container"'

# The template references the preview layer as an external script so that the
# template file itself can be opened and previewed in place:
#
#     <script src="../tools/template_preview_layer.js"></script>
#     <script> ...boot IIFE + content IIFE... </script>
#
# Reports live in `reports/<dir>/`, where that relative path does not resolve, so
# the tag would only cost a 404 and dead bytes. Strip it — the report is fully
# substituted and has no use for the preview layer.
PREVIEW_TAG = re.compile(r'[ \t]*<script src="[^"]*template_preview_layer\.js"></script>[ \t]*\r?\n?')

# Legacy: reports generated while the layer was still inlined in the template's
# <script> block. Kept so those can still be normalised in one pass.
PREVIEW_INLINE_BEGIN = "/* ═══════════════ TEMPLATE PREVIEW LAYER ═══════════════ */"
PREVIEW_INLINE_END = "/* ═══════════════ /TEMPLATE PREVIEW LAYER ═══════════════ */"

# Report-side anchor: the content IIFE that the boot script must precede.
CONTENT_ANCHOR = "<script>\n(function() {\n  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;"


def strip_preview_layer(html: str) -> str:
    """Remove the preview-layer `<script src>` tag from generated HTML.

    Also handles the older inline form (`PREVIEW_BEGIN`..`PREVIEW_END`) so that
    reports produced before the external-script refactor can still be normalised.
    """
    html = PREVIEW_TAG.sub("", html)
    if PREVIEW_INLINE_BEGIN in html:
        i = html.index(PREVIEW_INLINE_BEGIN)
        if i > 0 and html[i - 1] == "\n":
            i -= 1
        j = html.index(PREVIEW_INLINE_END, i) + len(PREVIEW_INLINE_END)
        if html[j:j + 1] == "\n":
            j += 1
        html = html[:i] + html[j:]
    return html


def slice_between(text: str, start_mark: str, end_mark: str) -> str:
    i = text.index(start_mark)
    j = text.index(end_mark, i)
    return text[i:j]


def extract_script(tpl: str) -> str:
    """The template's *inline* `<script>` block = boot IIFE + content IIFE.

    The template also has a `<script src=...>` tag for the preview layer, so
    `</script>` appears twice — take the inline block via the last close tag.
    """
    if tpl.count("<script>") != 1:
        raise SystemExit(
            f"template should have exactly one inline <script> block, found {tpl.count('<script>')}"
        )
    i = tpl.index("<script>")
    j = tpl.rindex("</script>") + len("</script>")
    return tpl[i:j]


def strip_node_ids(html: str) -> str:
    return NODE_ID.sub("", html)


def patch(html: str, boot_css: str, boot_html: str, tpl_script: str) -> tuple[str, list[str]]:
    notes: list[str] = []

    # 0 · drop editor metadata that leaked into generated reports
    n_before = len(NODE_ID.findall(html))
    html = strip_node_ids(html)
    if n_before:
        notes.append(f"stripped {n_before} data-page-node-id")

    # 0.5 · drop the preview-layer reference — a report has no use for it, and in
    #       reports/<dir>/ the relative path does not resolve anyway.
    if PREVIEW_TAG.search(html) or PREVIEW_INLINE_BEGIN in html:
        html = strip_preview_layer(html)
        notes.append("preview layer stripped")

    # 1 · boot CSS — replace the whole BOOT..RESPONSIVE region
    if CSS_START in html and CSS_END in html:
        old = slice_between(html, CSS_START, CSS_END)
        if old != boot_css:
            html = html.replace(old, boot_css, 1)
            notes.append("boot CSS replaced")
        else:
            notes.append("boot CSS already current")
    else:
        notes.append("!! boot CSS anchors missing")

    # 2 · boot overlay HTML
    if HTML_START in html and HTML_NEXT in html:
        i = html.index(HTML_START)
        j = html.index(HTML_NEXT, i)
        if html[i:j].rstrip() != boot_html:
            html = html[:i] + boot_html + "\n" + html[j:]
            notes.append("boot HTML replaced")
        else:
            notes.append("boot HTML already current")
    else:
        notes.append("!! boot HTML anchors missing")

    # 3 · the script block (boot IIFE + content IIFE) — take the template's verbatim
    if html.count("<script>") != 1 or html.count("</script>") != 1:
        notes.append(f"!! expected exactly one <script> block, got "
                     f"{html.count('<script>')}/{html.count('</script>')}")
    else:
        i = html.index("<script>")
        j = html.index("</script>", i) + len("</script>")
        if html[i:j] != tpl_script:
            html = html[:i] + tpl_script + html[j:]
            notes.append(f"script block replaced ({len(tpl_script)}B)")
        else:
            notes.append("script block already current")

    # 4 · avatars are photos now, not 8-bit sprites
    n_px = len(PIXELATED.findall(html))
    if n_px:
        html = PIXELATED.sub("image-rendering: auto;", html)
        notes.append(f"pixelated→auto ×{n_px}")

    return html, notes


def main(argv: list[str]) -> int:
    tpl = TEMPLATE.read_text(encoding="utf-8")
    boot_css = slice_between(tpl, CSS_START, CSS_END)
    hi = tpl.index(HTML_START)
    hj = tpl.index(HTML_NEXT, hi)
    boot_html = strip_node_ids(tpl[hi:hj]).rstrip()
    boot_js = strip_preview_layer(extract_script(tpl))
    print(f"template blocks: css={len(boot_css)}B html={len(boot_html)}B script={len(boot_js)}B")

    if argv:
        reports = [Path(a) / "full-report.html" if Path(a).is_dir() else Path(a) for a in argv]
    else:
        reports = sorted((ROOT / "reports").glob("*/full-report.html"))
    if not reports:
        print("no reports found", file=sys.stderr)
        return 1

    bad = 0
    for rp in reports:
        if not rp.exists():
            print(f"SKIP {rp} (missing)")
            continue
        html = rp.read_text(encoding="utf-8")
        out, notes = patch(html, boot_css, boot_html, boot_js)
        rp.write_text(out, encoding="utf-8")
        broke = any(n.startswith("!!") for n in notes)
        bad += broke
        print(f"{'FAIL' if broke else 'OK  '} {rp.parent.name}: " + " · ".join(notes))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

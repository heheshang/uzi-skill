#!/usr/bin/env python3
"""Lint the template preview layer and its wiring into the report template.

Replaces the old `apply_template_preview.py`, which *inlined* the layer into the
template's `<script>` block. The template now references it externally:

    <script src="../tools/template_preview_layer.js"></script>
    <script> ...boot IIFE + content IIFE... </script>

so the layer lives in exactly one place, the template stays small, and
`sync_boot_intro.py` can strip the one tag to produce a pristine report.

Three checks, all of which have bitten us:

  1. NO LITERAL TOKEN IN THE LAYER
     The generator does a *blind* global text substitution over the whole file,
     JS comments included. A comment that spelled out a token literally was
     rewritten to "Bitcoin" in every report. Braces must be built at runtime
     (`String.fromCharCode(123, 123)`), never written out.

  2. TEMPLATE WIRING
     Exactly one `<script src=...template_preview_layer.js>` and exactly one
     inline `<script>`, with the src tag first — the layer must run before the
     content IIFE, which collects the seats/slots the layer builds.

  3. TOKEN COVERAGE
     Every `{{TOKEN}}` in the template needs a key in the layer's `D` map, or
     the standalone preview renders a raw placeholder. Extra keys are a
     warning (drift), not an error.

Usage: python3 tools/lint_preview_layer.py
Exit:  0 = clean (warnings allowed) · 1 = error
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TPL = ROOT / "assets" / "report-template.html"
LAYER = ROOT / "tools" / "template_preview_layer.js"

LITERAL_TOKEN = re.compile(r"\{\{[A-Z_0-9]+\}\}")
ANY_TOKEN = re.compile(r"\{\{([A-Z_0-9]+)\}\}")
SRC_TAG = re.compile(r'<script src="[^"]*template_preview_layer\.js"></script>')
D_BLOCK = re.compile(r"\bvar\s+D\s*=\s*(\{.*?\n\s*\};)", re.S)
# Keys sit after `{` or `,`; several share a line. Anchoring on the line start
# instead would silently see only the first key of each line (29 of 58).
D_KEY = re.compile(r"[{,]\s*([A-Z_0-9]+)\s*:")


def main() -> int:
    errors: list[str] = []
    warnings: list[str] = []

    layer = LAYER.read_text(encoding="utf-8")
    tpl = TPL.read_text(encoding="utf-8")

    # ── 1 · no literal token in the layer ────────────────────────────────
    stray = LITERAL_TOKEN.search(layer)
    if stray:
        line = layer.count("\n", 0, stray.start()) + 1
        errors.append(
            f"layer line {line}: literal token {stray.group(0)!r} — the generator would "
            "substitute it in every report; build braces with String.fromCharCode"
        )

    # ── 2 · template wiring ──────────────────────────────────────────────
    n_src = len(SRC_TAG.findall(tpl))
    n_inline = tpl.count("<script>")
    if n_src != 1:
        errors.append(f"template has {n_src} preview-layer <script src> tags, expected 1")
    if n_inline != 1:
        errors.append(f"template has {n_inline} inline <script> blocks, expected 1")
    if n_src == 1 and n_inline == 1:
        if SRC_TAG.search(tpl).start() > tpl.index("<script>"):
            errors.append("preview-layer <script src> must precede the inline <script>")

    # ── 3 · token coverage ───────────────────────────────────────────────
    # Only the *renderable* part of the template: the inline script is code, and
    # tokens there would be a different bug (the generator would rewrite it).
    script_start = tpl.index("<script>")
    script_end = tpl.rindex("</script>")
    markup = tpl[:script_start] + tpl[script_end:]

    tpl_tokens = set(ANY_TOKEN.findall(markup))
    in_script = set(ANY_TOKEN.findall(tpl[script_start:script_end]))
    if in_script:
        errors.append(f"inline script contains tokens: {sorted(in_script)}")

    m = D_BLOCK.search(layer)
    if not m:
        errors.append("could not find the layer's `var D = { ... };` map")
        d_keys: set[str] = set()
    else:
        d_keys = set(D_KEY.findall(m.group(1)))

    missing = sorted(tpl_tokens - d_keys)
    if missing:
        errors.append(
            f"{len(missing)} template token(s) have no demo value in D — the preview "
            f"would render them raw: {', '.join(missing)}"
        )
    unused = sorted(d_keys - tpl_tokens)
    if unused:
        warnings.append(f"{len(unused)} unused D key(s): {', '.join(unused)}")

    # ── report ───────────────────────────────────────────────────────────
    print(f"template tokens: {len(tpl_tokens)} · D keys: {len(d_keys)}")
    for w in warnings:
        print(f"warn: {w}")
    for e in errors:
        print(f"!! {e}")
    if errors:
        return 1
    print("preview layer OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())

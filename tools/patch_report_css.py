#!/usr/bin/env python3
"""Apply targeted CSS fixes to generated reports.

WHY THIS EXISTS
---------------
`tools/sync_boot_intro.py` only syncs three blocks out of the template: the boot
CSS region, the boot HTML region, and the whole `<script>`. Any *other* template
CSS change therefore never reaches reports that were generated from an older
template revision — the template and its reports silently drift apart.

This tool closes that gap for small, surgical rules. Each entry is an explicit
(old, new) pair so it can never rewrite something unintended, and it is
idempotent: a report already carrying the fix is left alone.

Usage: python3 tools/patch_report_css.py [report_dir ...]
"""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (label, must_not_already_contain, old, new)
PATCHES: list[tuple[str, str, str, str]] = [
    (
        "msg-reasoning: preserve the generator's newlines",
        "white-space: pre-wrap;",
        """.chat-msg .msg-reasoning {
  color: var(--text-main);
  margin-bottom: 6px;
}""",
        """.chat-msg .msg-reasoning {
  color: var(--text-main);
  margin-bottom: 6px;
  /* 生成器往这里写的是 \\n 分隔的 ✅/❌ 条目；没有这条规则时换行会被折叠，
     整段挤成一行。pre-wrap 既保留换行和缩进，又允许长行自动折行。 */
  white-space: pre-wrap;
}""",
    ),
]


def main(argv: list[str]) -> int:
    if argv:
        reports = [Path(a) / "full-report.html" if Path(a).is_dir() else Path(a) for a in argv]
    else:
        reports = sorted((ROOT / "reports").glob("*/full-report.html"))
    if not reports:
        print("no reports found", file=sys.stderr)
        return 1

    problems = 0
    for rp in reports:
        if not rp.exists():
            print(f"SKIP {rp} (missing)")
            continue
        html = rp.read_text(encoding="utf-8")
        notes: list[str] = []
        for label, sentinel, old, new in PATCHES:
            if sentinel in html and old not in html:
                notes.append(f"{label}: already applied")
            elif old in html:
                html = html.replace(old, new, 1)
                notes.append(f"{label}: applied")
            else:
                notes.append(f"!! {label}: anchor not found")
                problems += 1
        rp.write_text(html, encoding="utf-8")
        print(f"{rp.parent.name}: " + " · ".join(notes))

    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

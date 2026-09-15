#!/usr/bin/env python3
"""Rebuild full-report-standalone.html by inlining avatars/*.svg as base64 data URIs.

Mirrors crates/uzi-report/src/inline.rs :: inline_assets.
"""
import base64
import re
import sys
from pathlib import Path

RE = re.compile(r'src="(avatars/[^"]+)"')


def rebuild(report_dir: Path) -> tuple[int, int]:
    html_path = report_dir / "full-report.html"
    avatars_dir = report_dir / "avatars"
    html = html_path.read_text(encoding="utf-8")

    replaced = 0
    missing = 0

    def sub(m: re.Match) -> str:
        nonlocal replaced, missing
        src = m.group(1)
        name = src[len("avatars/"):]
        avatar_path = avatars_dir / name
        if avatar_path.exists():
            b64 = base64.b64encode(avatar_path.read_bytes()).decode("ascii")
            replaced += 1
            return f'src="data:image/svg+xml;base64,{b64}"'
        missing += 1
        return m.group(0)

    inlined = RE.sub(sub, html)
    out = report_dir / "full-report-standalone.html"
    out.write_text(inlined, encoding="utf-8")
    return replaced, missing


if __name__ == "__main__":
    dirs = sys.argv[1:]
    if not dirs:
        print("usage: rebuild_standalone.py <report_dir> [...]", file=sys.stderr)
        sys.exit(1)
    for d in dirs:
        rd = Path(d)
        if not (rd / "full-report.html").exists():
            print(f"SKIP (no full-report.html): {rd}")
            continue
        replaced, missing = rebuild(rd)
        print(f"OK  {rd.name}: inlined={replaced} missing={missing}")

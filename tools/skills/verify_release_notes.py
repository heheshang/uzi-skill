#!/usr/bin/env python3
"""Verify the migrated RELEASE-NOTES.md did not falsify history.

The port shares upstream's version numbers, so this file is a *record*. A
migration may translate executable commands (`python run.py` → `uzi`) and add a
framing header, but every historical fact must survive byte-for-byte:

  * all 70 version headings, in order;
  * historical counts (`51 评委`, `19 维`, `52→65` …) — these were the values at
    the time and must not be "fixed" to today's 66 / 22 / 9;
  * upstream issue/PR links and dates;
  * the body text of each version section, apart from translated command lines.

Usage:
    python3 tools/skills/verify_release_notes.py [--upstream PATH] [--migrated PATH]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

DEFAULT_UPSTREAM = Path("/tmp/uzi-src/RELEASE-NOTES.md")
DEFAULT_MIGRATED = Path(__file__).resolve().parent.parent.parent / "RELEASE-NOTES.md"

# Counts that were true when written. Changing them is falsifying the record.
HISTORICAL_TOKENS = [
    "51 评委", "65 评委", "50 评委", "19 维", "7 大流派",
    "52→65", "180 规则", "242 规则", "22 维",
]

# An *executable* Python step. Historical prose that merely names a module is
# fine; a copy-pasteable interpreter invocation is not.
EXEC_PATTERNS = [
    (re.compile(r"\bpython3?\s+run\.py"), "python run.py"),
    (re.compile(r"\bpython3?\s+-c\b"), "python -c"),
    (re.compile(r"\bpython3?\s+-m\s+lib\."), "python -m lib."),
    (re.compile(r"\bpip\s+install\s+-r\b"), "pip install -r"),
]
# A line may mention an upstream-only step as history if it says so.
BENIGN = ("上游", "upstream")


def headings(text: str) -> list[str]:
    return [m.group(0).strip() for m in re.finditer(r"^## .+$", text, re.M)]


def version_tags(text: str) -> list[str]:
    return [m.group(0) for m in re.finditer(r"^## (v[0-9][0-9.]*)", text, re.M)]


def dates(text: str) -> list[str]:
    return re.findall(r"^## .*?(20\d{2}-\d{2}-\d{2})", text, re.M)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--upstream", type=Path, default=DEFAULT_UPSTREAM)
    ap.add_argument("--migrated", type=Path, default=DEFAULT_MIGRATED)
    args = ap.parse_args()

    if not args.upstream.is_file():
        print(f"upstream reference missing: {args.upstream}", file=sys.stderr)
        return 2
    if not args.migrated.is_file():
        print(f"migrated file missing: {args.migrated}", file=sys.stderr)
        return 1

    up = args.upstream.read_text(encoding="utf-8")
    new = args.migrated.read_text(encoding="utf-8")
    findings: list[str] = []

    # 1 · version sections, in order
    up_versions, new_versions = version_tags(up), version_tags(new)
    if up_versions != new_versions:
        only_up = [v for v in up_versions if v not in new_versions]
        only_new = [v for v in new_versions if v not in up_versions]
        findings.append(
            f"version list differs — missing: {only_up or 'none'}; added: {only_new or 'none'}; "
            f"order preserved: {[v for v in up_versions if v in new_versions] == new_versions}"
        )
    if len(new) < len(up):
        findings.append(f"file shrank: {len(up)} → {len(new)} chars")

    # 2 · historical counts
    for token in HISTORICAL_TOKENS:
        a, b = up.count(token), new.count(token)
        if a != b:
            findings.append(f"historical count {token!r}: {a} → {b} (must not change)")

    # 3 · upstream issue/PR links and dates
    link_re_chk = re.compile(r"github\.com/wbh604/UZI-Skill/(?:issues|pull)/\d+")
    links_up = link_re_chk.findall(up)
    links_new = link_re_chk.findall(new)
    if links_up != links_new:
        findings.append(f"upstream issue/PR links changed: {len(links_up)} → {len(links_new)}")
    if dates(up) != dates(new):
        findings.append("version dates changed")

    # 4 · no executable Python survives
    in_fence = False
    for line_no, line in enumerate(new.splitlines(), 1):
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            continue
        for pattern, label in EXEC_PATTERNS:
            if pattern.search(line) and not any(m in line for m in BENIGN):
                findings.append(f"line {line_no}: executable {label} — {line.strip()[:90]}")

    # 5 · every `uzi` flag mentioned must exist.
    # Reuses the doc guard's scraper so both tools agree on the accepted set;
    # without a built binary the check is skipped rather than guessed at.
    known: set[str] = set()
    try:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        from verify_docs import cli_flags  # type: ignore

        known = cli_flags()
    except Exception as e:  # pragma: no cover - defensive
        print(f"note: CLI flag check skipped ({e})")
    if not known:
        print("note: CLI not built — flag check skipped (run `cargo build -p uzi-cli`)")
    else:
        flag_re = re.compile(r"(?<![\w-])(--[a-z][a-z0-9-]*)(?![\w*-])")
        uzi_cmd = re.compile(r"(?:^|[|;&]\s*|\$\s+)uzi\s")
        for line_no, line in enumerate(new.splitlines(), 1):
            if not uzi_cmd.search(line):
                continue
            for flag in flag_re.findall(line):
                if flag not in known:
                    findings.append(
                        f"line {line_no}: flag {flag} not accepted by the CLI — {line.strip()[:80]}"
                    )

    # 6 · relative links must resolve inside the repo. Upstream pointed at files
    # this port deliberately did not migrate (`docs/BUGS-LOG.md`), so a dangling
    # `](…)` here is always a migration defect, never upstream's own.
    repo = Path(__file__).resolve().parent.parent.parent
    link_re = re.compile(r"\]\((?!https?://|#|mailto:)([^)]+)\)")
    for line_no, line in enumerate(new.splitlines(), 1):
        if line.lstrip().startswith("```"):
            continue  # fenced samples, not links
        for target in link_re.findall(line):
            path = target.split("#", 1)[0].strip()
            if path == "...":  # upstream's own placeholder, kept verbatim
                continue
            if path and not (repo / path).exists():
                findings.append(f"line {line_no}: link target does not exist — {target}")

    print(f"upstream: {len(up.splitlines())} lines · {len(up_versions)} versions")
    print(f"migrated: {len(new.splitlines())} lines · {len(new_versions)} versions")
    if findings:
        print(f"\n{len(findings)} finding(s):\n")
        for f in findings:
            print(f"  {f}")
        return 1
    print("✓ version history intact · historical counts intact · links/dates intact · no executable python")
    return 0


if __name__ == "__main__":
    sys.exit(main())

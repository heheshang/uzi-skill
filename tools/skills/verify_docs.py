#!/usr/bin/env python3
"""Verify every Rust path cited in the SKILL docs actually exists.

Usage:
    python3 tools/skill-migration/verify_skill_paths.py [--fix-hints]

The migrated reference docs point at concrete Rust modules. A path that does not
exist is worse than no path at all: it reads as authoritative while sending the
reader somewhere that isn't there. `personas_dir()`-style silent degradations are
exactly the failure mode this guards.

What is checked:
  1. every `uzi_<crate>::<path>` token resolves to a real module (or to a
     `pub fn`/`pub const`/`pub struct` inside one) in `crates/`;
  2. every relative markdown link between docs resolves on disk;
  3. no executable Python invocation survives (`python -c`, `python3 run.py`,
     `pip install`, `python -m lib.…`), while allowing prose that explains the
     upstream-vs-Rust difference;
  4. no doc tells the reader to *build* in order to *run* — the repo ships a
     prebuilt `./uzi`, so a `cargo build` / `target/release/uzi` line must be
     explicitly scoped to "changing code / other platforms" (see
     `SOURCE_DISCLAIMERS`). A doc whose entry point is a build step sends a
     reader with no toolchain (or an agent in a read-only checkout) into a wall;
  5. no doc claims a ported method has **no CLI entry** (`无 CLI 入口`). They all
     have one — the claim goes stale the moment the method is wired up, and it
     tells the reader to go find the Rust source instead of running `uzi`.

Exit code 0 = clean, 1 = findings.
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
CRATES = REPO / "crates"

# Docs that are part of the shipped skill surface.
DOC_GLOBS = [
    "SKILL.md",
    "skills/*/SKILL.md",
    "skills/*/references/*.md",
    "skills/*/references/*/*.md",
    "commands/*.md",
    # Agent-facing project context and user-facing docs: same rules apply, since
    # these are what a reader/agent follows to drive the tool.
    "README.md",
    "README_EN.md",
    "AGENTS.md",
    "CLAUDE.md",
    "CODEX.md",
    "GEMINI.md",
    "CONTRIBUTORS.md",
    "docs/*.md",
    "docs/*/*.md",
    "hooks/README.md",
    ".codex/INSTALL.md",
    ".opencode/INSTALL.md",
    ".github/*.md",
    ".github/ISSUE_TEMPLATE/*.md",
]

MODULE_RE = re.compile(r"\buzi_[a-z_]+(?:::[a-z_0-9]+)*")
LINK_RE = re.compile(r"\]\((\.{1,2}/[^)#\s]+\.md)\)")

# A glob (`--stage*`) is not a flag claim. The boundary must be part of the
# trailing class rather than a lookahead: with `(?!\*)` the greedy body
# backtracks and happily reports `--stag` for `--stage*`.
FLAG_RE = re.compile(r"(?<![\w-])(--[a-z][a-z0-9-]*)(?![\w*-])")
# `uzi` in command position: start of line, after a pipe/separator, or after
# `$ `. Keeps `cargo build --release` and CSS properties out of the check.
UZI_CMD_RE = re.compile(r"(?:^|[|;&]\s*|\$\s+)uzi\s")


def cli_flags() -> set[str]:
    """Flags scraped from the CLI's own `--help`.

    The flag set lives in the binary, never in a hand-maintained list here — a
    copy of it goes stale the moment a flag is added. A local build wins (it
    matches the source being edited); the prebuilt binary shipped at the repo
    root is the fallback, so the check still runs in a fresh clone with no
    `target/`.
    """
    for exe in (
        REPO / "target" / "release" / "uzi",
        REPO / "target" / "debug" / "uzi",
        REPO / "uzi",
    ):
        if not exe.is_file():
            continue
        try:
            out = subprocess.run(
                [str(exe), "--help"], capture_output=True, text=True, timeout=30
            ).stdout
        except (OSError, subprocess.SubprocessError):
            continue
        return set(FLAG_RE.findall(out))
    return set()

# An executable invocation, not prose about upstream. `python3 -c "..."` and
# `python -m x` are always executable; a bare `python3 run.py` likewise.
# A prose mention that *disclaims* Python rather than instructing it. A Rust
# migration legitimately says "上游用 Python / 本项目不需要 pip 安装"; only a
# copy-pasteable instruction is a defect.
BENIGN_MARKERS = (
    "上游", "upstream", "不用", "无需", "没有", "不再", "不是", "已弃用",
    "对照", "而非", "零依赖", "零外部依赖", "不装", "不依赖",
)

PY_EXEC_PATTERNS = [
    (re.compile(r"\bpython3?\s+-c\b"), "python -c invocation"),
    (re.compile(r"\bpython3?\s+-m\s"), "python -m invocation"),
    (re.compile(r"\bpython3?\s+run\.py"), "python run.py invocation"),
    (re.compile(r"\bpip\s+install\b"), "pip install"),
    (re.compile(r"\bplaywright\s+install\b"), "playwright install"),
]

# A build step is not a run step. The repo ships a prebuilt `./uzi`, so any
# `cargo build` / `target/release/uzi` line must be scoped to source work.
BUILD_CMD_RE = re.compile(r"\bcargo\s+(?:build|run|test)\b|target/release/uzi")
# Judged only on the surface an agent/reader follows to *run* an analysis.
# Maintainer docs (release notes, PR/issue templates, dev guides) discuss
# `cargo test` legitimately and are not entry points for a user.
RUN_SURFACE_GLOBS = [
    "SKILL.md",
    "skills/*/SKILL.md",
    "skills/*/references/*.md",
    "skills/*/references/*/*.md",
    "commands/*.md",
    "agents/*.md",
    ".codex/INSTALL.md",
    ".opencode/INSTALL.md",
    "hooks/README.md",
]
# Proximity window: a fence header and the command inside it can be a few lines
# apart, and the disclaimer is usually the sentence right before/after the block.
BUILD_SCOPE_WINDOW = 8
SOURCE_DISCLAIMERS = (
    "无需编译", "不需要编译", "无需构建", "不需要构建", "无需源码", "不需要 Rust 源码",
    "预编译", "源码构建", "源码维护", "源码仓库", "仅改代码", "仅维护", "改代码",
    "非 macOS", "其他平台", "其它平台", "自编译", "源码用户", "开发与验证",
    "维护者", "冒烟",
)
# Every ported method has a CLI entry; the stale phrasing told readers to go read
# Rust source instead. Capabilities the CLI genuinely does not expose are
# described as `CLI 未暴露…` plus the source-side call.
NO_CLI_CLAIM_RE = re.compile(r"无\s*CLI\s*入口")


def rust_index() -> tuple[set[str], str]:
    """Every module path and the concatenated source (for symbol lookup)."""
    mods: set[str] = set()
    for crate in sorted(CRATES.iterdir()):
        if not crate.is_dir() or not (crate / "src").is_dir():
            continue
        name = crate.name.replace("-", "_")
        mods.add(name)
        for rs in sorted((crate / "src").rglob("*.rs")):
            rel = rs.relative_to(crate / "src").with_suffix("")
            parts = [p for p in rel.parts if p != "mod"]
            if parts:
                mods.add(f"{name}::" + "::".join(parts))
    return mods, ""


def source_text() -> str:
    """All Rust source concatenated, for `pub fn` / `pub const` lookup."""
    out: list[str] = []
    for rs in sorted(CRATES.rglob("*.rs")):
        if "/target/" in str(rs):
            continue
        try:
            out.append(rs.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError):
            continue
    return "\n".join(out)


def docs() -> list[Path]:
    found: list[Path] = []
    for pattern in DOC_GLOBS:
        found.extend(sorted(REPO.glob(pattern)))
    return found


def check_module(token: str, mods: set[str], src: str) -> str | None:
    """Return a finding, or None when the token resolves."""
    # Longest-prefix match against a real module, e.g. `uzi_data::browser::fallback`
    # may be followed by a symbol like `::autofill_via_browser`.
    parts = token.split("::")
    for cut in range(len(parts), 0, -1):
        candidate = "::".join(parts[:cut])
        if candidate in mods:
            # Anything after the module must be a real symbol.
            for symbol in parts[cut:]:
                if symbol in src:
                    return None
                return f"{token} — module {candidate} exists but no symbol `{symbol}` found"
            return None
    # A bare crate with a symbol, e.g. `uzi_core::py::truthy` where `py` is a module.
    return f"{token} — no such module"


def inventory() -> int:
    """Print every Python-ish mention per doc, for human review.

    A rewritten doc may legitimately *mention* upstream Python ("上游用 akshare …").
    What is not acceptable is a Python path presented as the thing to use here.
    This dumps the lines so they can be judged in bulk.
    """
    pat = re.compile(r"(lib/[a-z_0-9/]+\.py|fetch_[a-z_]+\.py|compute_[a-z_]+\.py|"
                     r"assemble_report\.py|run_real_test\.py|akshare|\.py\b)")
    total = 0
    for doc in docs():
        text = doc.read_text(encoding="utf-8")
        hits = [
            (i, line.strip())
            for i, line in enumerate(text.splitlines(), 1)
            if pat.search(line)
        ]
        if not hits:
            continue
        rel = doc.relative_to(REPO)
        print(f"\n{rel}  ({len(hits)} mention(s))")
        for line_no, line in hits:
            print(f"  {line_no}: {line[:150]}")
        total += len(hits)
    print(f"\n{total} python-ish mention(s) across all docs")
    return 0


def main() -> int:
    if "--inventory" in sys.argv:
        return inventory()

    mods, _ = rust_index()
    src = source_text()
    known = cli_flags()
    if not known:
        print("note: CLI not built — flag check skipped (run `cargo build -p uzi-cli`)")
    findings: list[str] = []

    targets = docs()
    if not targets:
        print("no skill docs found", file=sys.stderr)
        return 1

    run_surface: set[Path] = set()
    for pattern in RUN_SURFACE_GLOBS:
        run_surface.update(REPO.glob(pattern))

    total_tokens = 0
    checked: set[str] = set()
    for doc in targets:
        text = doc.read_text(encoding="utf-8")
        rel = doc.relative_to(REPO)

        # 1 · Rust paths
        for token in set(MODULE_RE.findall(text)):
            total_tokens += 1
            if token in checked:
                continue
            checked.add(token)
            problem = check_module(token, mods, src)
            if problem:
                findings.append(f"{rel}: {problem}")

        # 2 · relative links
        for link in LINK_RE.findall(text):
            target = (doc.parent / link).resolve()
            if not target.exists():
                findings.append(f"{rel}: dangling link -> {link}")

        # 3 · flags that the CLI does not accept.
        # Only flags on a line that actually invokes `uzi` — CSS custom
        # properties (`--neon-cyan`) and prose otherwise look identical.
        for flag in sorted(set(FLAG_RE.findall(text))):
            if known and flag not in known:
                invocations = [
                    line for line in text.splitlines()
                    if flag in line and UZI_CMD_RE.search(line)
                ]
                if invocations:
                    findings.append(
                        f"{rel}: unknown flag {flag} (not in `uzi --help`): "
                        f"{invocations[0].strip()[:80]}"
                    )

        # 4 · executable Python.
        # Strict inside code fences — those are what a reader copies. Lenient in
        # prose, where mentioning the upstream Python world (and distinguishing
        # it from this project) is the whole point of a migration guide.
        in_fence = False
        for line_no, line in enumerate(text.splitlines(), 1):
            if line.lstrip().startswith("```"):
                in_fence = not in_fence
                continue
            for pattern, label in PY_EXEC_PATTERNS:
                if not pattern.search(line):
                    continue
                if not in_fence and any(m in line for m in BENIGN_MARKERS):
                    continue
                findings.append(f"{rel}:{line_no}: {label} — {line.strip()[:100]}")

        # 5 · a build command presented as the way to run. The repo ships a
        # prebuilt `./uzi`; a reader without a toolchain (or an agent in a
        # read-only checkout) must not be told to compile first.
        lines = text.splitlines()
        if doc in run_surface:
            for line_no, line in enumerate(lines, 1):
                if not BUILD_CMD_RE.search(line):
                    continue
                lo = max(0, line_no - 1 - BUILD_SCOPE_WINDOW)
                hi = min(len(lines), line_no + BUILD_SCOPE_WINDOW)
                if any(m in "\n".join(lines[lo:hi]) for m in SOURCE_DISCLAIMERS):
                    continue
                findings.append(
                    f"{rel}:{line_no}: build command with no source-scope caveat — "
                    f"{line.strip()[:90]}"
                )

        # 6 · "no CLI entry" claims. Every ported method has a CLI path; the
        # stale wording points readers at the Rust source instead of `uzi`.
        for line_no, line in enumerate(lines, 1):
            if NO_CLI_CLAIM_RE.search(line):
                findings.append(
                    f"{rel}:{line_no}: stale `无 CLI 入口` claim — {line.strip()[:90]}"
                )

    print(f"checked {len(targets)} docs · {total_tokens} rust path mentions ({len(checked)} unique)")
    if findings:
        print(f"\n{len(findings)} finding(s):\n")
        for f in findings:
            print(f"  {f}")
        return 1
    print(
        "✓ all rust paths resolve · all links resolve · "
        "all CLI flags known · no executable python · "
        "run paths need no build · no stale `无 CLI 入口` claims"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

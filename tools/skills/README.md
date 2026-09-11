# Skill doc guards

The `SKILL.md` / `references/` tree under `skills/` is shipped to agents, so a
wrong path in it is worse than no path: it reads as authoritative while sending
the reader somewhere that isn't there.

## `verify_docs.py`

```bash
python3 tools/skills/verify_docs.py              # check (exit 1 on findings)
python3 tools/skills/verify_docs.py --inventory   # list every Python-ish mention
```

Checks four things across `SKILL.md`, `skills/**/*.md`, and `commands/*.md`:

1. **Every `uzi_<crate>::<path>` token resolves** — the longest module prefix must
   exist under `crates/`, and any trailing segment must be a real symbol in the
   source. Catches paths invented by a doc rewrite.
2. **Every relative markdown link resolves** on disk. Catches the common
   `../` vs `../../` mistake when a doc sits in a `references/` subdirectory.
3. **Every flag is one the CLI accepts** — validated against `uzi --help` (so a
   built binary is required; otherwise the check is skipped with a note). Only
   flags on a line that actually invokes `uzi` in command position are examined,
   which keeps `cargo build --release` and CSS custom properties
   (`--neon-cyan`) out of the results. This is what catches a doc that documents
   an entry point nobody implemented.
4. **No executable Python survives** — `python -c`, `python -m`, `python run.py`,
   `pip install`, `playwright install`. This project is a Rust binary; telling a
   reader to run Python is a defect. Prose that *compares* against upstream is
   fine, so the check matches invocation shapes rather than the word "python".

`--inventory` prints every `lib/*.py` / `akshare` / `.py` mention per file for
human review — useful after a bulk rewrite, since a legitimate contrast
("上游用 akshare，本项目改用 HTTP 端点") reads differently from a live
instruction.

Run it after editing any skill doc. It is the doc counterpart to
`tools/golden/`'s behavioural parity tests.

## Why it exists

Writing these docs surfaced the failure repeatedly: modules that are ported,
compile, and pass their own tests while being **unreachable** — no CLI flag, no
caller, or a path that resolves relative to a directory the user is not in.
`verify_docs.py` only guards the documentation half; reachability of the code
itself is covered by the per-crate tests (see `tools/golden/README.md`).

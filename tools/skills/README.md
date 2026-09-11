# Skill doc guards

The `SKILL.md` / `references/` tree under `skills/` is shipped to agents, so a
wrong path in it is worse than no path: it reads as authoritative while sending
the reader somewhere that isn't there.

## `verify_docs.py`

```bash
python3 tools/skills/verify_docs.py              # check (exit 1 on findings)
python3 tools/skills/verify_docs.py --inventory   # list every Python-ish mention
```

Checks seven things across `SKILL.md`, `skills/**/*.md`, and `commands/*.md`:

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
5. **No doc makes a build step the entry point** — the repo ships a prebuilt
   `./uzi`, so a `cargo build` / `target/release/uzi` line inside the run surface
   (`SKILL.md`, `skills/**`, `commands/**`, `agents/**`, the two `INSTALL.md`
   files, `hooks/README.md`) must sit next to a source-scope caveat ("仅改代码 /
   其他平台 / 无需编译" …). Maintainer docs — release notes, PR/issue templates,
   dev guides — are exempt, since they discuss `cargo test` legitimately.
6. **No stale `无 CLI 入口` claim** — every ported method has a CLI path. The
   phrase sent readers hunting for the Rust source instead of running `uzi`;
   capabilities the CLI genuinely does not expose are described as "CLI 未暴露…"
   plus the source-side call.
7. **Source pointers declare themselves** — inside the run surface, an imperative
   `实现见 uzi_data::fetch::kline` reads as "go read this module". Naming the
   module is fine (it is provenance), but the line must say so ("移植出处 /
   源码 / 运行时不读"), or a reader with no checkout cannot tell an explanation
   from an instruction.

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

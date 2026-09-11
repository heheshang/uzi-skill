#!/usr/bin/env python3
"""Capture the mock artifacts `preview_with_mock.py` builds, as JSON assets.

Usage:
    python3 tools/golden/dump_mock.py [out_dir]

`preview_with_mock.py` is a 500-line data fixture: it constructs mock
`raw_data` / `dimensions` / `panel` / `synthesis` payloads, writes them to the
task cache, and calls `assemble_report.assemble`. The Rust port reproduces the
orchestration; the four payloads live here as checked-in assets so the mock data
is not re-transcribed by hand into Rust source.

The upstream script is executed with `write_task_output` and `assemble` stubbed,
so it runs to completion offline and hands back exactly the payloads it would
have written.

Default output: assets/mock/
"""
from __future__ import annotations

import json
import sys
import types
from pathlib import Path

UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")
HERE = Path(__file__).resolve().parent
DEFAULT_OUT = HERE.parent.parent / "assets" / "mock"

ARTIFACTS = ("raw_data", "dimensions", "panel", "synthesis")


def extract_literals(source: str, names: set[str]) -> dict:
    """Return the module-level literal assignments named in `names`."""
    import ast

    tree = ast.parse(source)
    found: dict = {}
    for node in tree.body:
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id in names:
                    found[target.id] = ast.literal_eval(node.value)
    return found


def main() -> int:
    out_dir = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else DEFAULT_OUT
    script = UPSTREAM / "preview_with_mock.py"
    if not script.exists():
        print(f"upstream script not found: {script}", file=sys.stderr)
        return 2

    sys.path.insert(0, str(UPSTREAM))
    import lib.cache as real_cache

    captured: dict[str, object] = {}

    def fake_write_task_output(ticker, task_name, data):
        captured[task_name] = data
        return None

    real_cache.write_task_output = fake_write_task_output

    # The script's final act is `from assemble_report import assemble`, then
    # `assemble(TICKER)`; stub the module so nothing renders.
    fake_assemble = types.ModuleType("assemble_report")
    fake_assemble.assemble = lambda ticker: f"reports/{ticker}/full-report.html"
    sys.modules["assemble_report"] = fake_assemble

    source = script.read_text(encoding="utf-8")
    exec(compile(source, str(script), "exec"), {"__name__": "__main__", "__file__": str(script)})

    missing = [name for name in ARTIFACTS if name not in captured]
    if missing:
        print(f"upstream did not produce: {', '.join(missing)}", file=sys.stderr)
        return 1

    out_dir.mkdir(parents=True, exist_ok=True)
    for name in ARTIFACTS:
        payload = json.dumps(captured[name], ensure_ascii=False, indent=2, default=str)
        (out_dir / f"{name}.json").write_text(payload + "\n", encoding="utf-8")
        print(f"  wrote {name}.json ({len(payload)} bytes)")

    # The panel loop's prose tables are data: capture them so the Rust port only
    # has to reproduce the seeded selection, not re-type 30 Chinese sentences.
    extracted = extract_literals(source, {"SAMPLE_COMMENTS", "VERDICTS"})
    if set(extracted) != {"SAMPLE_COMMENTS", "VERDICTS"}:
        print("could not extract SAMPLE_COMMENTS / VERDICTS", file=sys.stderr)
        return 1
    tables = {
        "comments": extracted["SAMPLE_COMMENTS"],
        "verdicts": extracted["VERDICTS"],
    }
    payload = json.dumps(tables, ensure_ascii=False, indent=2)
    (out_dir / "panel_tables.json").write_text(payload + "\n", encoding="utf-8")
    print(f"  wrote panel_tables.json ({len(payload)} bytes)")

    print(f"→ {out_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

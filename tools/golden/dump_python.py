#!/usr/bin/env python3
"""Dump reference artifacts from the upstream Python implementation.

Usage:
    python3 tools/golden/dump_python.py <raw_data.json> <out_dir> [--case NAME]

The upstream scripts communicate through JSON dicts, so every Rust stage can be
differentially tested against the Python output produced here: same input tree
-> byte-comparable output tree.

`--case` selects which artifacts to dump (default `all`):
    features     extract_features + sanitize_features
    dims         score_dimensions
    panel        generate_panel
    synthesis    generate_synthesis
    all          everything above
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")


def write(path: Path, data) -> None:
    path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2, default=str),
        encoding="utf-8",
    )


def main() -> int:
    argv = [a for a in sys.argv[1:] if not a.startswith("--")]
    case = "all"
    if "--case" in sys.argv:
        case = sys.argv[sys.argv.index("--case") + 1]
    if len(argv) != 2:
        print(__doc__)
        return 2
    raw_path = Path(argv[0]).resolve()
    out_dir = Path(argv[1]).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    sys.path.insert(0, str(UPSTREAM))
    raw = json.loads(raw_path.read_text(encoding="utf-8"))

    from lib.investor_db import INVESTORS

    if case in ("all", "features", "dims", "panel", "synthesis"):
        write(out_dir / "investors.json", INVESTORS)

    if case in ("all", "features"):
        from lib.stock_features import extract_features, sanitize_features

        features = extract_features(raw, raw.get("dimensions", {}))
        write(out_dir / "features.json", features)
        write(out_dir / "features_sanitized.json", sanitize_features(features))

    dims = None
    if case in ("all", "dims", "panel", "synthesis"):
        from lib.pipeline.score_fns import score_dimensions

        dims = score_dimensions(raw)
        write(out_dir / "dimensions.json", dims)

    panel = None
    if case in ("all", "panel", "synthesis"):
        from lib.pipeline.score_fns import generate_panel

        panel = generate_panel(dims, raw)
        write(out_dir / "panel.json", panel)

    if case in ("all", "synthesis"):
        from lib.pipeline.score_fns import generate_synthesis

        synthesis = generate_synthesis(raw, dims, panel, agent_analysis=None)
        write(out_dir / "synthesis.json", synthesis)

    if case in ("all", "panel", "personas"):
        dump_persona_pools(out_dir)

    print(f"dumped {case} to {out_dir}")
    return 0


def dump_persona_pools(out_dir: Path) -> None:
    """Persona lines are chosen with an unseeded `random.choice`; dump the pool so
    the Rust port can be checked for membership + substitution instead of identity."""
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "investor_personas", UPSTREAM / "lib" / "investor_personas.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    pools = {
        pid: {
            signal: list(lines)
            for signal, lines in entry.items()
            if isinstance(lines, list)
        }
        for pid, entry in mod.PERSONAS.items()
    }
    write(out_dir / "persona_pools.json", pools)


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Dump upstream `lib/name_matcher.fuzzy_match` results for a fixed index.

Usage:
    python3 tools/golden/dump_name_matcher.py [fixture.json] [out.json]

`fuzzy_match` normally pulls its A-share index from akshare behind a 7-day
cache. To keep the golden deterministic and network-free, the index from the
fixture is injected by patching `build_a_share_index` — the matching rules under
test (`levenshtein` / `char_set_jaccard` / ranking) are still upstream's own.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")
HERE = Path(__file__).resolve().parent

DEFAULT_FIXTURE = HERE / "fixtures" / "name_matcher.json"
DEFAULT_OUT = HERE / "expected" / "name_matcher" / "queries.json"


def main() -> int:
    fixture_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_FIXTURE
    out_path = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_OUT

    fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
    index = fixture["index"]
    queries = fixture["queries"]

    sys.path.insert(0, str(UPSTREAM))
    import lib.name_matcher as nm

    # Serve the fixture index instead of hitting akshare.
    nm.build_a_share_index = lambda: index

    out = {}
    for q in queries:
        out[q] = nm.fuzzy_match(q, top_k=5, max_distance=2)

    # The primitives are pure and worth pinning directly.
    primitives = []
    for a, b in [
        ("", ""),
        ("abc", "abc"),
        ("", "abc"),
        ("abc", ""),
        ("kitten", "sitting"),
        ("北部港湾", "北部湾港"),
        ("水晶光电", "水晶光电"),
        ("ab", "bc"),
        ("ab", ""),
        ("abc", "bca"),
        ("ab", "cd"),
        ("贵州茅台", "贵州茅太"),
    ]:
        primitives.append(
            {
                "a": a,
                "b": b,
                "levenshtein": nm.levenshtein(a, b),
                "jaccard": nm.char_set_jaccard(a, b),
            }
        )

    payload = {
        "fixture": fixture_path.name,
        "queries": out,
        "primitives": primitives,
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"wrote {out_path} ({len(out)} queries, {len(primitives)} primitives)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

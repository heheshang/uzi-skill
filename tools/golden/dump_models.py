#!/usr/bin/env python3
"""Dump the institutional-modeling dimensions (20/21/22) from upstream Python.

Usage:
    python3 tools/golden/dump_models.py <raw_data.json> <out_dir>

Mirrors `run_real_test._run_modeling_and_scoring`:
    features = sanitize_features(extract_features(raw, raw["dimensions"]))
    d20 = compute_dim_20(features, raw)
    d21 = compute_dim_21(features, raw, d20)
    d22 = compute_dim_22(features, raw, d20, d21)
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")


def write(path: Path, data) -> None:
    path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2, default=str), encoding="utf-8"
    )


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    raw = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    out_dir = Path(sys.argv[2]).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    sys.path.insert(0, str(UPSTREAM))
    from compute_deep_methods import compute_dim_20, compute_dim_21, compute_dim_22
    from lib.stock_features import extract_features, sanitize_features

    features = sanitize_features(extract_features(raw, raw.get("dimensions", {})))
    write(out_dir / "features_sanitized.json", features)

    # Mirror run_real_test._run_modeling_and_scoring exactly: compute_dim_21 and
    # compute_dim_22 receive the INNER `data` dicts (d20["data"] / d21["data"]),
    # never the full {data, source, fallback} dimension dicts.
    raw["dimensions"]["20_valuation_models"] = compute_dim_20(features, raw)
    d20 = raw["dimensions"]["20_valuation_models"]["data"]
    raw["dimensions"]["21_research_workflow"] = compute_dim_21(features, raw, d20)
    d21 = raw["dimensions"]["21_research_workflow"]["data"]
    raw["dimensions"]["22_deep_methods"] = compute_dim_22(features, raw, d20, d21)

    write(out_dir / "dim_20.json", raw["dimensions"]["20_valuation_models"])
    write(out_dir / "dim_21.json", raw["dimensions"]["21_research_workflow"])
    write(out_dir / "dim_22.json", raw["dimensions"]["22_deep_methods"])

    print(f"dumped models to {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

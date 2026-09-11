#!/usr/bin/env python3
"""Dump `_auto_summarize_dim` output for every dimension of a fixture.

Usage:
    python3 tools/golden/dump_summarize.py <raw_data.json> <out.json>

The score for each dimension comes from the upstream `score_dimensions` run on
the same fixture, which is exactly what `generate_synthesis` passes.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")

DIM_LABELS = {
    "0_basic": "基础信息",
    "1_financials": "财报",
    "2_kline": "K线技术面",
    "3_macro": "宏观环境",
    "4_peers": "同行对比",
    "5_chain": "产业链",
    "6_research": "券商研报",
    "7_industry": "行业景气",
    "8_materials": "原材料",
    "9_futures": "期货关联",
    "10_valuation": "估值分位",
    "11_governance": "治理/减持",
    "12_capital_flow": "资金面",
    "13_policy": "政策与监管",
    "14_moat": "护城河",
    "15_events": "事件驱动",
    "16_lhb": "龙虎榜",
    "17_sentiment": "舆情",
    "18_trap": "杀猪盘",
    "19_contests": "实盘比赛",
}


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    raw = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    out_path = Path(sys.argv[2])

    sys.path.insert(0, str(UPSTREAM))
    from lib.pipeline.score_fns import _auto_summarize_dim, score_dimensions

    dims_scored = score_dimensions(raw)
    out = {}
    for dim_key, label in DIM_LABELS.items():
        dim = (raw.get("dimensions", {}) or {}).get(dim_key) or {}
        score = (dims_scored.get("dimensions", {}).get(dim_key) or {}).get("score", 0)
        out[dim_key] = _auto_summarize_dim(dim_key, label, dim, score)

    out_path.write_text(
        json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

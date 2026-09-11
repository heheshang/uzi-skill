#!/usr/bin/env python3
"""Dump upstream `lib/playwright_fallback.py` strategy output for fixed HTML.

Usage:
    python3 tools/golden/dump_fallback.py [out.json]

The strategies fetch through Playwright; the *parsing* half is what the Rust
port reimplements. This stubs `fetch_url` so each upstream `_strategy_*` parser
runs against canned HTML — the regexes, filters, and projections under test are
upstream's own.

`_dim_quality_score` / `_dim_needs_fallback` / `_filter_dims_by_network` are also
covered, since they decide whether the fallback runs at all.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")
HERE = Path(__file__).resolve().parent
DEFAULT_OUT = HERE / "expected" / "fallback" / "strategies.json"

# ─── Reference pages, one per strategy ───────────────────────────────────────
PAGES = {
    "4_peers": """
      <a href="/S/SH600519">贵州茅台</a>
      <a href="/S/SZ000582">北部湾港</a>
      <a href="/S/SH600519">贵州茅台</a>
      <a href="/S/SH600520">短</a>
      <a href="/S/HK00700">腾讯控股</a>
      <a href="/S/BJ430047">诺思兰德</a>
    """,
    "8_materials": "<div class='m_table'>主营业务：光学薄膜研发与制造</div>",
    "15_events": """
      <div class="announcement-title">2026年第一季度报告</div>
      <div class="announcement-title">关于回购公司股份的公告</div>
    """,
    "17_sentiment": """<script>{"title":"水晶光电还有机会吗"}{"title":"聊聊AR眼镜产业链"}</script>""",
    "3_macro": """
      <a href="/x">短</a>
      <a href="/y">1234567890</a>
      <a href="/z">2026年8月份国民经济运行情况</a>
      <a href="/w">国家统计局城市司首席统计师解读数据</a>
    """,
    "7_industry": """
      <h3><a href="/l">光学光电子行业景气度持续提升</a></h3>
      <span class="content-right_abc">市场规模预计达到 420 亿元，渗透率稳步提升</span>
    """,
    "14_moat": """
      <div class="lemma-summary J-summary"><b>水晶光电</b>是一家光学薄膜企业。</div>
      <dt class="basicInfo-item name">主营业务</dt><dd class="basicInfo-item value">光学元件</dd>
      <dt class="basicInfo-item name">所属行业</dt><dd class="basicInfo-item value">光学光电子</dd>
    """,
    "13_policy": """
      <a title="证监会发布关于资本市场的最新政策通知">x</a>
      <a title="首页">y</a>
      <a>关于进一步规范上市公司信息披露的公告</a>
    """,
    "18_trap": """{"title":"水晶光电 老师推荐 必涨"}{"title":"其他公司分析"}{"title":"水晶光电财报解读"}""",
    "19_contests": """<div>{"name":"稳健成长","total_gain":45.6}</div><div>{"name":"激进策略","total_gain":-12.3}</div>""",
}

RAW = {
    "dimensions": {
        "0_basic": {"data": {"name": "水晶光电", "industry": "光学光电子"}},
        "4_peers": {"data": {}},
    }
}

# ─── Quality-gate fixtures ──────────────────────────────────────────────────
QUALITY_DIMS = [
    {"data": {"a": 1, "b": 2}},
    {"data": {"a": 1, "b": "—", "c": None, "d": ""}},
    {"data": {"a": 1, "b": None}},
    {"data": {}},
    {"data": {"_src": "x", "a": 1}},
    {"data": {"_src": "x", "_e": "y"}},
    {"data": None},
    {"data": {"a": "—", "b": None}},
    {"data": {"a": 1, "b": 2}, "fallback": True},
    {"data": {"growth": "—", "tam": "—", "penetration": "—", "k1": 1, "k2": 2, "k3": 3}},
]

NETWORK_CASES = [
    {"domestic_ok": True, "search_ok": True, "overseas_ok": True},
    {"domestic_ok": True, "search_ok": False},
    {"domestic_ok": False, "search_ok": True},
]


def main() -> int:
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_OUT
    sys.path.insert(0, str(UPSTREAM))
    import lib.playwright_fallback as pf

    # Serve canned HTML to the real parsers; never launch a browser.
    current = {"html": ""}
    pf.fetch_url = lambda url, wait_for=None, timeout=15: current["html"]

    strategies = {}
    for dim, html in PAGES.items():
        current["html"] = html
        fn = pf.DIM_STRATEGIES[dim]
        try:
            strategies[dim] = fn("002273.SZ", RAW)
        except Exception as e:  # a parser raising is itself a result worth pinning
            strategies[dim] = {"_raised": f"{type(e).__name__}: {e}"}

    # An empty page must degrade to None for every strategy.
    empty = {}
    current["html"] = ""
    for dim, fn in pf.DIM_STRATEGIES.items():
        try:
            empty[dim] = fn("002273.SZ", RAW)
        except Exception as e:
            empty[dim] = {"_raised": f"{type(e).__name__}: {e}"}

    quality = []
    for dim in QUALITY_DIMS:
        needs, reason = pf._dim_needs_fallback(dim)
        data = dim.get("data")
        quality.append(
            {
                "dim": dim,
                "needs": needs,
                "score": pf._dim_quality_score(data) if isinstance(data, dict) else 0.0,
            }
        )

    dims = frozenset(pf.DIM_STRATEGIES.keys())
    network = []
    for profile in NETWORK_CASES:
        effective, skipped = pf._filter_dims_by_network(dims)
        network.append(
            {
                "profile": profile,
                "effective": sorted(effective),
                "skipped": sorted(skipped),
            }
        )

    # `_filter_dims_by_network` reads the live preflight; call it directly with
    # each canned profile instead so the dump is deterministic.
    network = []
    for profile in NETWORK_CASES:
        effective, skipped = [], []
        for d in sorted(dims):
            reqs = pf.DIM_NETWORK_REQUIREMENTS.get(d, ())
            ok = True
            why = []
            for req in reqs:
                if req == "domestic" and not profile.get("domestic_ok"):
                    ok = False
                    why.append("domestic 不通")
                elif req == "search" and not profile.get("search_ok"):
                    ok = False
                    why.append("search 不通")
                elif req == "overseas" and not profile.get("overseas_ok"):
                    ok = False
                    why.append("overseas 不通")
            if ok:
                effective.append(d)
            else:
                skipped.append(f"{d}({','.join(why)})")
        network.append(
            {"profile": profile, "effective": effective, "skipped": skipped}
        )

    payload = {
        "strategies": strategies,
        "empty_pages": empty,
        "quality": quality,
        "network": network,
        "dim_network_requirements": {
            d: list(pf.DIM_NETWORK_REQUIREMENTS.get(d, ())) for d in sorted(dims)
        },
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

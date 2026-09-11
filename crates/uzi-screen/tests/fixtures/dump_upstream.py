#!/usr/bin/env python3
"""Generate golden artifacts for `crates/uzi-screen` from the UPSTREAM Python code.

Run exactly like this (the upstream checkout must be present at /tmp/uzi-src):

    cd /tmp/uzi-src/skills/deep-analysis/scripts
    python3 /Users/shang/Documents/workspace/personal/ai/UZI-Skill/crates/uzi-screen/tests/fixtures/dump_upstream.py

It reads `daily_screen_input.json` / `holdings.csv` / `.cache/...` from this
directory and writes `expected_*.json` + `expected_screen.html` next to them.
Nothing here is part of the Rust build; it exists so the Rust port can be
differentially verified against the reference implementation.
"""
from __future__ import annotations

import json
import sys
import tempfile
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

import pandas as pd

FIX = Path(__file__).resolve().parent
SHANGHAI = ZoneInfo("Asia/Shanghai")

# The upstream package layout: `lib/daily_screen/...`.  When this script is run
# from `skills/deep-analysis/scripts`, plain `import lib...` resolves.
sys.path.insert(0, str(Path.cwd()))


def dump(name: str, payload) -> None:
    text = json.dumps(payload, ensure_ascii=False, indent=2, default=str)
    (FIX / name).write_text(text, encoding="utf-8")
    print(f"wrote {name} ({len(text)} bytes)")


def load_input() -> dict:
    return json.loads((FIX / "daily_screen_input.json").read_text(encoding="utf-8"))


def build_snapshots(inp: dict):
    """Raw frame rows -> StockSnapshot list, with fixture overlays applied."""
    from lib.daily_screen.universe import apply_hard_filters, normalize_universe_frame

    snaps_by_market = {}
    for market in inp["markets"]:
        frame = pd.DataFrame(inp["market_rows"][market])
        snaps = normalize_universe_frame(
            frame, market, inp["observed_at"][market], inp["source"][market]
        )
        snaps_by_market[market] = snaps

    for market, snaps in snaps_by_market.items():
        for s in snaps:
            overlay = inp["overlays"].get(s.code)
            if overlay:
                s.extra.update(overlay.get("extra", {}))
                # `enrich_intraday` rewrites the snapshot's quote-identity fields
                # once a live quote is accepted; fixtures model that explicitly.
                for key in ("source", "observed_at"):
                    if overlay.get(key):
                        setattr(s, key, overlay[key])

    filtered, stats = {}, {}
    for market, snaps in snaps_by_market.items():
        filtered[market], stats[market] = apply_hard_filters(snaps, inp["min_turnover"])
    return snaps_by_market, filtered, stats


def golden_daily_screen() -> None:
    from datetime import datetime as _datetime

    import lib.daily_screen.runner as runner_mod
    from lib.daily_screen.personas import build_features
    from lib.daily_screen.ranker import build_candidate, preselect, rank_candidates
    from lib.daily_screen.runner import run_daily_screen
    from lib.daily_screen.themes import build_theme_context
    from lib.daily_screen.renderer import render_report

    inp = load_input()
    snaps_by_market, filtered, stats = build_snapshots(inp)

    # Freeze the runner's wall clock so the report (report_id / generated_at /
    # execution freshness) is reproducible in the Rust port.
    frozen_now = _datetime.fromisoformat(inp["runner_now"])

    class FrozenDateTime(_datetime):
        @classmethod
        def now(cls, tz=None):
            return frozen_now.astimezone(tz) if tz else frozen_now.replace(tzinfo=None)

    runner_mod.datetime = FrozenDateTime

    theme_universe = [s for m in inp["markets"] for s in snaps_by_market[m]]
    all_stocks = [s for m in inp["markets"] for s in filtered[m]]
    themes = build_theme_context(theme_universe)
    shortlisted = preselect(all_stocks, limit=40)

    evaluated_at = datetime.fromisoformat(inp["evaluated_at"])

    out = {
        "universe_stats": stats,
        "snapshots": {m: [s.to_dict() for s in snaps_by_market[m]] for m in inp["markets"]},
        "themes": themes,
        "shortlist_codes": [s.code for s in shortlisted],
        "features": {
            s.code: build_features(s, themes.get(s.code, {}), inp["overlays"].get(s.code, {}).get("evidence", []))
            for s in shortlisted
        },
    }

    candidates = []
    for s in shortlisted:
        overlay = inp["overlays"].get(s.code, {})
        evidence = overlay.get("evidence", [])
        gaps = [] if overlay else ["snapshot_only"]
        candidates.append(build_candidate(s, themes.get(s.code, {}), evidence, gaps, evaluated_at=evaluated_at))
    picks, rejected = rank_candidates(candidates, top_n=inp["top_n"])

    out["candidates"] = [c.to_dict() for c in candidates]
    out["picks"] = [c.to_dict() for c in picks]
    out["rejected"] = [c.to_dict() for c in rejected]
    dump("expected_daily_screen.json", out)

    # ---- runner report (frozen snapshots, enrichment disabled) --------------
    frozen = [s for m in inp["markets"] for s in snaps_by_market[m]]
    with tempfile.TemporaryDirectory() as tmp:
        report = run_daily_screen(
            mode=inp["mode"],
            markets=tuple(inp["markets"]),
            top_n=inp["top_n"],
            min_turnover_local=inp["min_turnover"],
            enrich=False,
            track=False,
            output_root=Path(tmp),
            now=datetime.fromisoformat(inp["runner_now"]),
            frozen_stocks=frozen,
        )
    report.pop("report_path", None)  # tempdir-local; the port returns its own
    dump("expected_daily_screen_runner.json", report)

    # ---- byte-exact HTML ----------------------------------------------------
    as_of = {}
    for market in inp["markets"]:
        stamps = [s.observed_at for s in theme_universe if s.market == market]
        as_of[market] = max(stamps)
    html_report = {
        "report_id": "TEST-20260910-close-ah-143105000000",
        "mode": inp["mode"],
        "generated_at": "2026-09-10T14:31:05+08:00",
        "as_of_by_market": as_of,
        "snapshot_kind": "frozen",
        "analysis_basis": "rule_only",
        "filters": {
            "exclude_st": True,
            "exclude_suspended": True,
            "min_turnover_local": inp["min_turnover"],
            "top_n": inp["top_n"],
            "min_research_confidence": 70,
        },
        "universe_stats": stats,
        "market_errors": {},
        "action_summary": {},
        "picks": [c.to_dict() for c in picks],
        "rejected": [c.to_dict() for c in rejected[:20]],
        "data_quality": {
            "markets_requested": list(inp["markets"]),
            "markets_available": list(inp["markets"]),
            "enrichment_enabled": False,
            "shortlisted": len(shortlisted),
            "removed_after_quote_refresh": 0,
            "industry_sources": {},
            "intraday_available": 2,
        },
        "performance_contract": {
            "entry": "first_executable_price_after_publish",
            "horizons": ["close", "next_open", "next_close", "3d"],
            "status": "paper_trading",
        },
    }
    from collections import Counter

    html_report["action_summary"] = dict(Counter(c.action for c in picks))
    dump("expected_report_input.json", html_report)
    with tempfile.TemporaryDirectory() as tmp:
        target = Path(tmp) / "index.html"
        render_report(html_report, target, FIX / "_no_avatars")
        (FIX / "expected_screen.html").write_bytes(target.read_bytes())
        print(f"wrote expected_screen.html ({len(target.read_bytes())} bytes)")


def golden_portfolio_and_versus() -> None:
    import lib.versus_runner as vs
    import lib.portfolio_runner as pf
    from lib.fund_holdings_runner import _estimate_runtime, confirm_and_run_holdings

    # `_load_cache` resolves `<SCRIPTS_DIR>/.cache/<ticker>/...`; point it at the
    # fixture tree so the synthetic cache below is what gets read.
    vs.SCRIPTS_DIR = FIX
    pf.SCRIPTS_DIR = FIX

    tickers = ["600519.SH", "000858.SZ", "300750.SZ", "002594.SZ", "603501.SH"]
    metrics = []
    for t in tickers:
        bundle = vs._load_cache(t)
        assert bundle is not None, f"fixture cache missing for {t}"
        metrics.append(vs._extract_metrics(bundle))

    rows = pf._parse_csv(FIX / "holdings.csv")
    normalized = pf._normalize_weights([dict(r) for r in rows])
    by_ticker = {h["ticker"]: h for h in normalized}
    for m in metrics:
        h = by_ticker[m["ticker"]]
        m["_weight"] = h["weight"]
        m["_note"] = h.get("note", "")
        m["report_path"] = None
    health = pf._portfolio_health(metrics)

    # headerless CSV path
    headerless = pf._parse_csv(FIX / "holdings_headerless.csv")
    mixed_rows = pf._parse_csv(FIX / "holdings_mixed.csv")
    normalized_mixed = pf._normalize_weights([dict(r) for r in mixed_rows])

    out = {
        "metrics": metrics,
        "csv_rows": rows,
        "normalized": normalized,
        "normalized_mixed": normalized_mixed,
        "headerless_rows": headerless,
        "health": health,
        "comparison_grid": vs._render_comparison_grid(metrics),
        "verdict_cards": vs._render_verdict_cards(metrics),
        "estimate_runtime": {
            "lite_10": _estimate_runtime(10, "lite"),
            "medium_10": _estimate_runtime(10, "medium"),
            "deep_3": _estimate_runtime(3, "deep"),
            "unknown_120": _estimate_runtime(120, "nope"),
        },
        "holdings_cancel": confirm_and_run_holdings("510300.SH", "ETF", [
            {"rank": 1, "code": "600519.SH", "name": "贵州茅台", "weight_pct": 5.5},
            {"rank": 2, "code": "000858.SZ", "name": "五粮液", "weight_pct": 4.2},
        ], depth="medium", auto_yes=False, interactive=False),
        "holdings_empty": confirm_and_run_holdings("510300.SH", "ETF", [], auto_yes=True),
    }
    dump("expected_portfolio.json", out)

    # versus/portfolio HTML is a pure function of the metrics/health once the
    # wall-clock timestamp is normalised: the Rust port substitutes its own
    # `now` and does the same, so the comparison stays byte-exact.
    from datetime import datetime as _dt

    now = _dt.now().strftime("%Y-%m-%d %H:%M")
    versus_html = vs._render_html(metrics, "lite").replace(now, "{{NOW}}")
    (FIX / "expected_versus.html").write_text(versus_html, encoding="utf-8")
    print(f"wrote expected_versus.html ({len(versus_html)} bytes)")

    portfolio_html = pf._render_html("测试组合", metrics, health, "lite").replace(now, "{{NOW}}")
    (FIX / "expected_portfolio.html").write_text(portfolio_html, encoding="utf-8")
    print(f"wrote expected_portfolio.html ({len(portfolio_html)} bytes)")


def golden_source_parsers() -> None:
    """`sources.py` quote/minute parsers over a synthetic provider payload."""
    from lib.daily_screen.sources import parse_minutes, parse_tencent, parse_tencent_minutes

    # Tencent `v_sz300308="..."` payload: 38+ `~`-separated fields, amount at 37
    # in 万元 (A-share) / 元 (HK), volume at 6 in lots (A) / shares (HK).
    fields = [""] * 60
    fields[1] = "中际旭创"
    fields[2] = "300308"
    fields[3] = "156.80"
    fields[4] = "142.70"
    fields[5] = "145.00"
    fields[6] = "242000"
    fields[9] = "156.70"
    fields[10] = "1200"
    fields[19] = "156.90"
    fields[20] = "800"
    fields[30] = "20250910143000"
    fields[32] = "9.90"
    fields[33] = "158.00"
    fields[34] = "144.50"
    fields[37] = "380000"
    a_text = 'v_sz300308="' + "~".join(fields) + '";'
    hk_fields = list(fields)
    hk_fields[2] = "00700"
    hk_fields[30] = "2025/09/10 15:30:00"
    hk_fields[37] = "3200000000"
    hk_fields[6] = "8400000"
    hk_text = 'v_hk00700="' + "~".join(hk_fields) + '";'

    a_quote = parse_tencent(a_text, "300308.SZ")
    hk_quote = parse_tencent(hk_text, "00700.HK")

    trends = [
        "2025-09-10 14:29,0,156.70,0,0,0,130000000,0",
        "2025-09-10 14:30,0,156.80,0,0,0,100000000,0",
        # out-of-order duplicate (later value wins) + a post-cutoff + a pre-day bar
        "2025-09-10 14:30,0,156.85,0,0,0,101000000,0",
        "2025-09-10 15:01,0,157.00,0,0,0,90000000,0",
        "2025-09-09 14:30,0,150.00,0,0,0,80000000,0",
        "2025-09-10 14:28,0,0,0,0,0,70000000,0",  # close <= 0 → dropped
        "bad,row",
    ]
    minutes = parse_minutes(
        {"data": {"code": "300308", "trends": trends}},
        "300308.SZ",
        "2025-09-10T14:30:30+08:00",
    )

    tencent_minutes_payload = {
        "data": {
            "sz300308": {
                "data": {
                    "date": "20250910",
                    "data": [
                        "1430 156.80 1200 380000000",
                        "1431 157.00 800 380090000",
                    ],
                }
            }
        }
    }
    tencent_minutes = parse_tencent_minutes(
        tencent_minutes_payload, "300308.SZ", "2025-09-10T14:31:30+08:00"
    )

    dump(
        "expected_sources.json",
        {
            "a_quote": a_quote,
            "hk_quote": hk_quote,
            "minutes": minutes,
            "tencent_minutes": tencent_minutes,
        },
    )


if __name__ == "__main__":
    golden_daily_screen()
    golden_portfolio_and_versus()
    golden_source_parsers()

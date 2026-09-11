#!/usr/bin/env python3
"""Build a deterministic synthetic `raw_data.json` covering every dimension.

Real network snapshots are noisy and non-reproducible; differential testing needs
a fixture that (a) is stable and (b) exercises every field `stock_features` and
`investor_criteria` read. Values are chosen to hit boundary branches (zero vs
null, negative ROE, missing optional fields, empty collections).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path


def dim(data: dict, source: str = "fixture", quality: str = "full") -> dict:
    return {
        "data": data,
        "source": source,
        "fallback": quality in ("error", "missing"),
        "_pipeline": {
            "dim_key": source,
            "quality": quality,
            "data_gaps": [],
            "latency_ms": 12,
            "fetched_at": 1770000000.0,
            "cached": False,
            "top_level_fields": {},
            "error": None,
        },
    }


def build() -> dict:
    dims = {}

    dims["0_basic"] = dim(
        {
            "code": "002273",
            "name": "水晶光电",
            "industry": "光学光电子",
            "price": 23.45,
            "change_pct": 2.13,
            "market_cap_yi": 326.5,
            "circulating_cap_yi": 300.1,
            "listed_date": "2008-09-19",
            "chairman": "林敏",
            "actual_controller": "林敏",
            "staff_num": 5732,
            "pe_ttm": 34.2,
            "pb": 3.11,
            "eps": 0.69,
            "dividend_yield_ttm": 0.85,
            "market": "A",
            "market_status": {"is_open": True, "label": "交易中"},
        },
        "fixture:fetch_basic",
    )

    dims["1_financials"] = dim(
        {
            "roe": 11.2,
            "roe_history": [9.1, 10.4, 8.7, -2.3, 11.2],
            "revenue_history": [32.1, 38.6, 41.2, 45.7, 52.3],
            "net_profit_history": [2.9, 3.6, 3.1, -0.8, 5.4],
            "revenue_ttm": 54.8,
            "net_profit_ttm": 6.1,
            "revenue_growth_yoy": 14.5,
            "net_profit_growth_yoy": 22.0,
            "revenue_growth_period": "2025A",
            "revenue_growth_basis": "annual",
            "revenue_growth_source": "eastmoney",
            "net_profit_growth_period": "2025A",
            "net_profit_growth_basis": "annual",
            "net_profit_growth_source": "eastmoney",
            "net_margin": 11.1,
            "dividend_years": [2020, 2021, 2022, 2023, 2024],
            "dividend_amounts": [1.2, 1.5, 1.5, 2.0, 2.5],
            "financial_health": {
                "current_ratio": 1.85,
                "debt_ratio": 38.4,
                "fcf_margin": 8.2,
                "roic": 9.6,
                "ocf_to_net_income_ratio": 1.12,
            },
            "ocf_to_net_income_ratio": 1.12,
            "dupont": {
                "net_margin_pct": 11.1,
                "asset_turnover": 0.62,
                "equity_multiplier": 1.63,
                "roe_reconstructed_pct": 11.2,
                "roe_quality": "margin_driven",
            },
        },
        "fixture:fetch_financials",
    )

    dims["2_kline"] = dim(
        {
            "stage": "Stage 2 · 上升期",
            "ma_align": "多头排列",
            "macd": "水上金叉",
            "rsi": 62.4,
            "kline_stats": {
                "ytd_return": "+18.4%",
                "volatility": "32.1%",
                "max_drawdown": "-24.5%",
            },
            "indicators": {
                "kdj_k": 71.2,
                "kdj_d": 65.8,
                "kdj_j": 82.0,
                "obv_trend_up": True,
                "williams_r": -28.4,
            },
            "candles_60d": [
                {"date": "2026-06-01", "open": 18.0, "high": 18.6, "low": 17.8, "close": 18.4},
                {"date": "2026-07-01", "open": 18.4, "high": 21.2, "low": 18.1, "close": 20.9},
                {"date": "2026-08-03", "open": 21.0, "high": 24.8, "low": 20.2, "close": 24.1},
                {"date": "2026-09-10", "open": 23.0, "high": 23.9, "low": 22.6, "close": 23.45},
            ],
        },
        "fixture:fetch_kline",
    )

    dims["3_macro"] = dim(
        {
            "rate_cycle": "降息周期 · 流动性宽松",
            "commodity": "原油震荡",
            "summary": "宏观流动性友好",
        },
        "fixture:fetch_macro",
    )

    dims["4_peers"] = dim(
        {
            "peer_table": [
                {"code": "002273", "name": "水晶光电", "pe": 34.2, "is_self": True},
                {"code": "002241", "name": "歌尔股份", "pe": 28.5, "is_self": False},
                {"code": "300433", "name": "蓝思科技", "pe": 41.0, "is_self": False},
            ],
            "global_peer_comparison": {
                "peer_count": 4,
                "peers": [
                    {"name": "Sunny Optical", "pe": 22.0},
                    {"name": "Largan", "pe": 18.5},
                ],
            },
        },
        "fixture:fetch_peers",
    )

    dims["5_chain"] = dim(
        {
            "main_business_breakdown": [
                {"name": "光学元器件", "revenue_pct": 62.5},
                {"name": "半导体材料", "revenue_pct": 24.1},
                {"name": "其他", "revenue_pct": 13.4},
            ],
            "upstream": ["光学玻璃", "树脂"],
            "downstream": ["消费电子", "汽车电子"],
        },
        "fixture:fetch_chain",
    )

    dims["6_research"] = dim(
        {
            "report_count": 18,
            "coverage_count": 18,
            "rating_distribution": {"买入": 11, "增持": 5, "中性": 2},
            "buy_rating_pct": 88.9,
            "target_price_avg": 28.6,
            "consensus_eps_2026": 0.92,
            "consensus_pe_2026": 25.5,
        },
        "fixture:fetch_research",
    )

    dims["7_industry"] = dim(
        {"growth": 12.5, "lifecycle": "成长期", "summary": "光学需求复苏"},
        "fixture:fetch_industry",
    )

    dims["8_materials"] = dim(
        {
            "materials_detail": [{"name": "光学玻璃", "price_change_pct": -3.2}],
            "summary": "原材料成本平稳",
        },
        "fixture:fetch_materials",
    )

    dims["9_futures"] = dim({"related": [], "summary": "无强关联期货品种"}, "fixture:fetch_futures")

    dims["10_valuation"] = dim(
        {
            "pe": 34.2,
            "pb": 3.11,
            "pe_quantile": "5 年 42 分位",
            "industry_pe": 31.8,
            "dcf": "估值 298.4 亿",
        },
        "fixture:fetch_valuation",
    )

    dims["11_governance"] = dim(
        {
            "pledge": [],
            "insider_trades_1y": [{"name": "高管A", "action": "增持"}],
        },
        "fixture:fetch_governance",
    )

    dims["12_capital_flow"] = dim(
        {
            "main_fund_flow_20d": [
                {"date": "2026-09-10", "主力净流入-净额": 1.2e8},
                {"date": "2026-09-09", "主力净流入-净额": -4.5e7},
                {"date": "2026-09-08", "主力净流入-净额": 8.8e7},
                {"date": "2026-09-05", "主力净流入-净额": 2.1e7},
                {"date": "2026-09-04", "主力净流入-净额": -1.1e7},
            ],
            "margin_trend": "融资余额上升",
            "holders_trend": "股东户数下降 6.2%",
            "unlock_schedule": [{"date": "2026-12-01", "ratio": 1.2}],
        },
        "fixture:fetch_capital_flow",
    )

    dims["13_policy"] = dim({"policy_dir": "积极支持高端制造"}, "fixture:fetch_policy")

    dims["14_moat"] = dim(
        {
            "scores": {"intangible": 7, "switching": 6, "network": 4, "scale": 8},
            "summary": "技术壁垒明显",
        },
        "fixture:fetch_moat",
    )

    dims["15_events"] = dim(
        {
            "news": [
                {"title": "公司发布新品", "date": "2026-09-01"},
                {"title": "获大订单", "date": "2026-08-20"},
                {"title": "股东减持计划", "date": "2026-08-05"},
            ],
            "recent_notices": [{"title": "2026 半年报"}],
            "event_timeline": ["新品发布 增长", "大订单 合作"],
        },
        "fixture:fetch_events",
    )

    dims["16_lhb"] = dim(
        {
            "lhb_count_30d": 4,
            "matched_youzi": ["章盟主", "赵老哥"],
            "inst_vs_youzi": {"institutional_net": 3.2e7, "youzi_net": -1.1e7},
        },
        "fixture:fetch_lhb",
    )

    dims["17_sentiment"] = dim(
        {
            "thermometer_value": 72,
            "positive_pct": 64.5,
            "sentiment_label": "偏乐观",
            "hot_rank": {"rank_history": [12, 18, 9]},
        },
        "fixture:fetch_sentiment",
    )

    dims["18_trap"] = dim(
        {"signals_hit_count": 0, "trap_level": "🟢 安全"},
        "fixture:fetch_trap_signals",
    )

    dims["19_contests"] = dim(
        {
            "summary": {"xueqiu_cubes_total": 12, "high_return_cubes": 2},
            "cubes": [],
        },
        "fixture:fetch_contests",
    )

    raw = {
        "ticker": "002273.SZ",
        "code": "002273",
        "market": "A",
        "dimensions": dims,
        "fund_managers": [
            {"name": "张三", "fund": "某某成长混合", "return_5y": 128.4, "holding_quarters": 4},
            {"name": "李四", "fund": "某某精选", "return_5y": 62.1, "holding_quarters": 2},
        ],
        "similar_stocks": [{"code": "300433", "name": "蓝思科技"}],
    }
    return raw


def build_sparse() -> dict:
    """Markets/edge cases: everything missing. Exercises the degraded path."""
    return {
        "ticker": "AAPL",
        "code": "AAPL",
        "market": "U",
        "dimensions": {
            "0_basic": dim({"code": "AAPL", "name": "Apple Inc.", "price": 0}, "fixture:fetch_basic", "partial"),
            "1_financials": dim({}, "fixture:fetch_financials", "missing"),
            "2_kline": dim({}, "fixture:fetch_kline", "error"),
        },
    }


def build_empty() -> dict:
    """No dimensions at all — exercises every fallback branch in scoring."""
    return {"ticker": "EMPTY.SZ", "code": "EMPTY", "market": "A", "dimensions": {}}


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("tools/golden/fixtures")
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "raw_data_synthetic.json").write_text(
        json.dumps(build(), ensure_ascii=False, indent=2), encoding="utf-8"
    )
    (out_dir / "raw_data_sparse.json").write_text(
        json.dumps(build_sparse(), ensure_ascii=False, indent=2), encoding="utf-8"
    )
    (out_dir / "raw_data_empty.json").write_text(
        json.dumps(build_empty(), ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"wrote fixtures to {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

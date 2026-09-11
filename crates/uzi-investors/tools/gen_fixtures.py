#!/usr/bin/env python3
"""Regenerate every data table and differential fixture of `uzi-investors`.

The upstream checkout must exist at `/tmp/uzi-src/skills/deep-analysis/scripts`
(same prerequisite as `tools/golden/dump_python.py`).

    python3 crates/uzi-investors/tools/gen_fixtures.py            # everything
    python3 crates/uzi-investors/tools/gen_fixtures.py data       # src/data/*.json
    python3 crates/uzi-investors/tools/gen_fixtures.py probes     # rule probes
    python3 crates/uzi-investors/tools/gen_fixtures.py seats      # is_in_range
    python3 crates/uzi-investors/tools/gen_fixtures.py knowledge  # affinity/scope

Data tables (`data`) are `json.dumps(..., ensure_ascii=False)` of the runtime
Python objects, so ids/names/weights/messages and key order are byte-faithful.
The test fixtures record upstream *behaviour* (check outcomes, `is_in_range`
verdicts, affinity scores) so the Rust port can be differentially tested rule by
rule without depending on the Python install at test time.
"""
from __future__ import annotations

import ast
import importlib
import inspect
import json
import random
import re
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]  # crates/uzi-investors
UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")
sys.path.insert(0, str(UPSTREAM))

DATA = ROOT / "src" / "data"
FIXTURES = ROOT / "tests" / "fixtures"


def write_json(path: Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(data, ensure_ascii=False, indent=2, default=str)
    path.write_text(text + "\n", encoding="utf-8")
    print(f"{path.relative_to(ROOT)}: {len(text)} bytes")


# ────────────────────────────────────────────────────────────────
# data tables
# ────────────────────────────────────────────────────────────────


def gen_data() -> None:
    from lib.investor_db import INVESTORS
    from lib.seat_db import SEATS
    from lib.investor_personas import PERSONAS
    from lib import investor_profile as P
    from lib import investor_knowledge as K
    from lib.investor_criteria import INVESTOR_RULES
    from lib.personas import FRAMEWORK_INSTRUCTIONS_ZH

    write_json(DATA / "investors.json", INVESTORS)
    write_json(DATA / "seats.json", SEATS)
    write_json(DATA / "persona_pools.json", PERSONAS)
    write_json(DATA / "profiles.json", {
        "profiles": P.PROFILES,
        "group_default": P.GROUP_DEFAULT,
        "generic_fallback": P.GENERIC_FALLBACK,
    })
    write_json(DATA / "knowledge.json", {
        "market_scope": K.MARKET_SCOPE,
        "known_holdings": {k: [list(t) for t in v] for k, v in K.KNOWN_HOLDINGS.items()},
        "industry_affinity": K.INDUSTRY_AFFINITY,
    })
    write_json(DATA / "criteria_meta.json", {
        inv["id"]: [
            {
                "rule_id": r.rule_id,
                "name": r.name,
                "weight": r.weight,
                "pass_msg": r.pass_msg,
                "fail_msg": r.fail_msg,
            }
            for r in INVESTOR_RULES[inv["id"]]
        ]
        for inv in INVESTORS
    })
    framework = DATA / "framework_zh.txt"
    framework.write_text(FRAMEWORK_INSTRUCTIONS_ZH, encoding="utf-8")
    print(f"framework_zh.txt: {len(FRAMEWORK_INSTRUCTIONS_ZH)} bytes")


# ────────────────────────────────────────────────────────────────
# per-rule differential probes
# ────────────────────────────────────────────────────────────────

NUM = [0, 0.0, 1, 2, 3, 4, 5, 8, 10, 12, 15, 18, 19, 20, 21, 22.5, 24, 25, 26, 28, 30, 32, 35, 38,
       40, 45, 49, 50, 55, 60, 65, 70, 75, 80, 90, 100, 150, 200, 300, 500, 999, 5000, 20000,
       -1, -2, -5, -10, -15, -20, -25, -30, -40, -50, -100, 0.1, 0.2, 0.5, 1.5, 2.5, 1e9]
STR = ["AI", "新能源", "光模块", "白酒", "软件", "半导体", "消费电子", "China", "US", "A", "测试",
       "博彩", "锂电", "机器人", "核电", "数据中心", "marketplace", "SaaS", "订阅", "银行", "钢铁",
       "化工原料", "航运", "量子", "加密", "比特币", "NVIDIA", "台积电", "互联网", "创新药", "游戏",
       "社交", "传统汽车", "传统制造", "重型机械", "新能源车"]
BOOL = [True, False]
SPECIAL = {
    "market": ["A", "A", "A", "A", "US", "HK"],
    "fcf_known": [True, False],
    "fcf_positive": [True, False],
    "matched_youzi": [[], ["赵老哥"], ["章盟主"]],
}


def _const_strings(fn):
    """(string constants, called function names) of a function's source."""
    try:
        tree = ast.parse(textwrap.dedent(inspect.getsource(fn)))
    except Exception:
        return set(), set()
    found, called = set(), set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            found.add(node.value)
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
            called.add(node.func.id)
    return found, called


def referenced_keys(check, keys, mod):
    """Feature keys a rule's check reads, following calls into local helpers."""
    helpers = {
        name: obj
        for name, obj in vars(mod).items()
        if callable(obj) and hasattr(obj, "__code__")
        and obj.__code__.co_filename.endswith("investor_criteria.py")
    }
    found, seen, stack = set(), set(), [check]
    while stack:
        fn = stack.pop()
        if id(fn) in seen:
            continue
        seen.add(id(fn))
        consts, called = _const_strings(fn)
        found |= consts
        stack.extend(helpers[name] for name in called if name in helpers)
    return found & set(keys)


def gen_probes() -> None:
    from lib.investor_db import INVESTORS
    from lib.investor_criteria import INVESTOR_RULES
    import lib.investor_criteria as mod

    src = (UPSTREAM / "lib" / "investor_criteria.py").read_text(encoding="utf-8")
    keys = set(re.findall(r'f\.get\(\s*"([^"]+)"', src))
    keys |= set(re.findall(r'features\.get\(\s*"([^"]+)"', src))
    keys |= {"market", "ticker", "name", "industry", "market_cap", "market_cap_yi",
             "matched_youzi", "fcf_known", "fcf_positive"}
    keys = sorted(keys)

    rng = random.Random(20260911)
    corpus = []
    for _ in range(30000):
        f = {}
        for k in keys:
            if k in SPECIAL:
                f[k] = rng.choice(SPECIAL[k])
                continue
            r = rng.random()
            if r < 0.18:
                continue
            if r < 0.30:
                f[k] = None
            elif r < 0.55:
                f[k] = rng.choice(NUM)
            elif r < 0.80:
                f[k] = rng.choice(STR)
            else:
                f[k] = rng.choice(BOOL)
        corpus.append(f)
    for scalar in NUM + STR + BOOL + [None]:
        corpus.append({k: scalar for k in keys if k not in SPECIAL})
    for _ in range(2000):
        corpus.append({k: rng.choice(NUM) for k in keys if k not in SPECIAL})

    results = {}
    both = 0
    for inv in INVESTORS:
        entry = {}
        for rule in INVESTOR_RULES[inv["id"]]:
            want = {"pass": None, "fail": None, "raise": None}
            for f in corpus:
                try:
                    key = "pass" if bool(rule.check(f)) else "fail"
                except Exception:
                    key = "raise"
                if want[key] is None:
                    want[key] = f
                if all(v is not None for v in want.values()):
                    break
            refs = referenced_keys(rule.check, keys, mod)
            entry[rule.rule_id] = {
                kind: {k: v for k, v in feats.items() if k in refs}
                for kind, feats in want.items()
                if feats is not None
            }
            if "pass" in entry[rule.rule_id] and "fail" in entry[rule.rule_id]:
                both += 1
        results[inv["id"]] = entry
    print("rule probes: both directions", both, "/ 242")
    write_json(FIXTURES / "rule_probes.json", results)


# ────────────────────────────────────────────────────────────────
# is_in_range differential cases
# ────────────────────────────────────────────────────────────────

CAPS = [None, 0, 1_000_000_000, 2_000_000_000, 5_000_000_000, 8_000_000_000,
        10_000_000_000, 15_000_000_000, 19_999_999_999, 20_000_000_000, 30_000_000_000,
        50_000_000_000, 50_000_000_001, 100_000_000_000, 9_000_000_000_000]
FLAGS = [
    {},
    {"trend": "up"}, {"trend": "down"},
    {"is_sector_leader": True}, {"is_sector_leader": False},
    {"is_first_or_second_board": True},
    {"is_hot_theme": True},
    {"is_hottest_in_sector": True},
    {"is_oversold": True},
    {"sentiment_cycle": True},
    {"style_match": "trend"}, {"style_match": "value"},
    {"short_term_only": True},
    {"is_accelerating": True},
    {"is_continuous_limit_up": True},
    {"is_first_board": True},
    {"is_ai_theme": True},
    {"min_fundamental_score": 70},
    {"min_turnover": 1_000_000_000},
    {"max_institution_pct": 10},
]


def gen_seats() -> None:
    from lib.seat_db import SEATS, is_in_range

    rows = []
    for nick in list(SEATS) + ["不存在"]:
        for ci in range(len(CAPS)):
            rows.append((nick, ci, 0))
        for fi in range(1, len(FLAGS)):
            for ci in (1, 8, 11, 14):
                rows.append((nick, ci, fi))
    cases = []
    for nick, ci, fi in rows:
        features = dict(FLAGS[fi])
        if CAPS[ci] is not None:
            features["market_cap"] = CAPS[ci]
        cases.append([nick, ci, fi, is_in_range(nick, features)])
    write_json(FIXTURES / "seat_range.json",
               {"caps": CAPS, "flags": FLAGS, "cases": cases})


# ────────────────────────────────────────────────────────────────
# market scope / affinity / holdings differential cases
# ────────────────────────────────────────────────────────────────

INDUSTRIES = ["AI", "半导体量子", "消费电子白酒", "银行保险", "化工钢铁", "生物科技mRNA",
              "互联网平台", "新能源电池", "黄金加密", "量子通信", "煤炭", "", "石油石化"]
NAMES = ["", "中国石化", "苹果", "贵州茅台"]
MARKETS = ["A", "HK", "US", "XX", ""]
TICKERS = ["AAPL", "苹果", "600519", "贵州茅台", "TSLA", "NVDA", "00700", "腾讯", "ZZZ", ""]


def gen_knowledge() -> None:
    from lib.investor_db import INVESTORS
    from lib.investor_knowledge import (
        KNOWN_HOLDINGS, INDUSTRY_AFFINITY,
        market_match, check_known_holdings, compute_affinity,
    )

    def holdings(inv, tk, nm):
        hit = check_known_holdings(inv, tk, nm)
        return list(hit) if hit else None

    write_json(FIXTURES / "knowledge_cases.json", {
        "industries": INDUSTRIES,
        "names": NAMES,
        "affinity": [
            [inv, ind, nm, compute_affinity(inv, ind, nm)]
            for inv in sorted(INDUSTRY_AFFINITY)
            for ind in INDUSTRIES
            for nm in NAMES
        ],
        "markets": [
            [inv["id"], mkt, market_match(inv["id"], mkt)]
            for inv in INVESTORS
            for mkt in MARKETS
        ],
        "holdings": [
            [inv, tk, nm, holdings(inv, tk, nm)]
            for inv in sorted(KNOWN_HOLDINGS)
            for tk in TICKERS
            for nm in NAMES
        ],
    })


def main() -> int:
    which = sys.argv[1:] or ["data", "probes", "seats", "knowledge"]
    for step in which:
        {"data": gen_data, "probes": gen_probes, "seats": gen_seats, "knowledge": gen_knowledge}[step]()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

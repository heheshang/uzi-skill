#!/usr/bin/env python3
"""uzi 字段覆盖审计 · 只读

清点 `.cache/<ticker>/raw_data.json` 里每个维度的字段落地情况，把"空"分成三类：

  missing           真缺 —— null / "" / "—" / "待补充" 等占位
  empty_collection  空集合 —— [] / {}（可能是"确实没有"，也可能是没抓到，需人工判读）
  error_string      取数失败留下的错误串 —— "ImportError: akshare not installed" /
                    "HTTP 502" / "endpoint empty" 之类。**非空，但不是数据。**

`_` 开头的内部诊断字段（上游残留错误、fallback 快照等）**不计入覆盖率的分子分母** ——
它们是诊断信息，不是指标。它们单独列出来，是定位"某维度为什么恒空"的最快线索。

用法:
    python3 tools/audit_field_coverage.py                     # 扫 .cache 全部标的
    python3 tools/audit_field_coverage.py --ticker BTC-USD    # 只看一个
    python3 tools/audit_field_coverage.py --json /tmp/audit.json
    python3 tools/audit_field_coverage.py --gaps-json        # 见下方"gaps-json"
    python3 tools/audit_field_coverage.py --strict            # 见下方"退出码"

`--gaps-json`:
    为每个标的写 `.cache/<ticker>/_data_gaps.json` —— 把诊断字段(`_` 前缀)与取数失败
    错误串从 `raw_data.json` 的"内嵌诊断"**降级**为独立的机器可读 sidecar，供看板/CI 直接
    消费，而主产物 `raw_data.json` 保持字节级忠实（上游 Python 同样内嵌这些诊断）。
    只读：绝不改动 `raw_data.json`，只新增 sidecar。

退出码:
    0  正常
    1  --strict 命中：存在 100% 空的维度，或存在 error_string 字段
    2  找不到任何 raw_data.json

历史教训（别重犯）:
    本脚本 v1 只统计"每个维度的顶层字段名"，把加密空率报成股票的 3.3 倍 —— 实际是
    1.6–2.3 倍。v2 改成递归到叶子。v2 又漏掉了 error_string 这一类，导致
    `1_financials` 明明整块没取到却显示 0.0% 空。v3 补上。
"""

from __future__ import annotations

import argparse
import collections
import glob
import json
import os
import re
import sys
from datetime import datetime, timezone

PLACEHOLDERS = {
    None, "", "—", "-", "--", "N/A", "n/a", "NA", "nan", "NaN",
    "待补充", "无", "未知", "数据缺失",
}

# 取数失败留下的哨兵串。命中即视为"没有数据"，不算 present。
SENTINEL_PATTERNS = [
    re.compile(p) for p in (
        r"^ImportError\b",
        r"^ModuleNotFoundError\b",
        r"^AttributeError\b",
        r"^NameError\b",
        r"\bnot installed\b",
        r"\bno module named\b",
        r"\bendpoint empty\b",
        r"\bempty (?:report|response|result)\b",
        r"\bHTTP\s+[45]\d\d\b",
        r"^HTTP\s+[45]\d\d",
        r"\btimed? ?out\b",
        r"^Traceback\b",
        r"没有返回内容",
        r"\bfetch failed\b",
        r"\brequest failed\b",
        r"^error:",
    )
]

DIM_ORDER_FALLBACK = 999


def dim_sort_key(dim: str) -> tuple[int, str]:
    head = dim.split("_", 1)[0]
    return (int(head) if head.isdigit() else DIM_ORDER_FALLBACK, dim)


def is_sentinel(text: str) -> bool:
    return any(p.search(text) for p in SENTINEL_PATTERNS)


def classify(value) -> str | None:
    """返回 'missing' / 'empty_collection' / 'error_string' / None(有值)"""
    if isinstance(value, str):
        stripped = value.strip()
        if stripped in PLACEHOLDERS:
            return "missing"
        return "error_string" if is_sentinel(stripped) else None
    if value is None:
        return "missing"
    if isinstance(value, (list, dict)):
        return "empty_collection" if len(value) == 0 else None
    return None


def walk_leaves(node, prefix: str = "", diag_out: list | None = None):
    """产出 (路径, 值)。空集合与标量都算叶子，非空容器继续下钻。

    `_` 开头的键被视作诊断信息，收进 `diag_out` 并**不再下钻、不计入覆盖率**。
    """
    if isinstance(node, dict):
        if not node:
            yield prefix or "<root>", node
            return
        for key, val in node.items():
            if key.startswith("_"):
                if diag_out is not None:
                    diag_out.append({"path": f"{prefix}.{key}" if prefix else key,
                                     "value": str(val)[:200]})
                continue
            child = f"{prefix}.{key}" if prefix else key
            yield from walk_leaves(val, child, diag_out)
    elif isinstance(node, list):
        if not node:
            yield prefix or "<root>", node
            return
        # 列表只看前 3 个元素的结构，避免长序列把字段数刷爆
        for item in node[:3]:
            yield from walk_leaves(item, f"{prefix}[]", diag_out)
    else:
        yield prefix or "<root>", node


def audit_ticker(raw_path: str) -> dict:
    with open(raw_path, encoding="utf-8") as fh:
        raw = json.load(fh)

    ticker = os.path.basename(os.path.dirname(raw_path))
    dims_report: dict[str, dict] = {}
    diagnostics: list[dict] = []
    errors: list[dict] = []

    for dim, node in raw.get("dimensions", {}).items():
        data = node.get("data", node) if isinstance(node, dict) else node
        missing: set[str] = set()
        empty_coll: set[str] = set()
        errs: set[str] = set()
        present: set[str] = set()
        total = 0
        local_diag: list[dict] = []

        for path, value in walk_leaves(data, "", local_diag):
            total += 1
            present.add(path)
            kind = classify(value)
            if kind == "missing":
                missing.add(path)
            elif kind == "empty_collection":
                empty_coll.add(path)
            elif kind == "error_string":
                errs.add(path)
                errors.append({"dim": dim, "path": path, "value": str(value)[:200]})

        for item in local_diag:
            diagnostics.append({"dim": dim, **item})

        dims_report[dim] = {
            "leaf_fields": total,
            "missing": sorted(missing),
            "empty_collection": sorted(empty_coll),
            "error_string": sorted(errs),
            "present": sorted(present),
            "empty_total": len(missing) + len(empty_coll) + len(errs),
            "empty_pct": round((len(missing) + len(empty_coll) + len(errs)) / total * 100, 1) if total else 0.0,
            "source": (node.get("source") if isinstance(node, dict) else None),
            "fallback": (node.get("fallback") if isinstance(node, dict) else None),
        }

    leaf_total = sum(d["leaf_fields"] for d in dims_report.values())
    empty_total = sum(d["empty_total"] for d in dims_report.values())

    return {
        "ticker": ticker,
        "market": raw.get("market"),
        "dimensions": dims_report,
        "diagnostics": diagnostics,
        "errors": errors,
        "leaf_fields": leaf_total,
        "empty_total": empty_total,
        "empty_pct": round(empty_total / leaf_total * 100, 1) if leaf_total else 0.0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="uzi raw_data.json 字段覆盖审计（只读）")
    parser.add_argument("--cache", default=".cache", help="缓存根目录（默认 .cache）")
    parser.add_argument("--ticker", action="append", help="只看指定标的，可重复")
    parser.add_argument("--json", dest="json_out", help="把结果写到该 JSON 文件")
    parser.add_argument("--gaps-json", action="store_true",
                        help="为每个标的写 _data_gaps.json（诊断字段 + 错误串 sidecar）")
    parser.add_argument("--strict", action="store_true",
                        help="存在 100%% 空的维度或 error_string 字段时 exit 1")
    parser.add_argument("--quiet", action="store_true", help="只打印汇总")
    args = parser.parse_args()

    paths = sorted(glob.glob(os.path.join(args.cache, "*", "raw_data.json")))
    if args.ticker:
        wanted = set(args.ticker)
        paths = [p for p in paths if os.path.basename(os.path.dirname(p)) in wanted]
    if not paths:
        print(f"!! 在 {args.cache} 下没找到任何 raw_data.json", file=sys.stderr)
        return 2

    reports = [audit_ticker(p) for p in paths]

    for rep in reports:
        print(f"\n{'=' * 84}")
        print(f"{rep['ticker']}  market={rep['market']}  "
              f"字段 {rep['leaf_fields']} · 空 {rep['empty_total']} ({rep['empty_pct']}%)")
        print(f"{'=' * 84}")
        if not args.quiet:
            print(f"  {'dim':<24}{'字段':>6}{'真缺':>6}{'空集':>6}{'错串':>6}{'空率':>8}  source")
            for dim in sorted(rep["dimensions"], key=dim_sort_key):
                d = rep["dimensions"][dim]
                print(f"  {dim:<24}{d['leaf_fields']:>6}{len(d['missing']):>6}"
                      f"{len(d['empty_collection']):>6}{len(d['error_string']):>6}"
                      f"{d['empty_pct']:>7}%  {str(d['source'])[:28]}")
        if rep["errors"]:
            print(f"\n  -- 取数失败留下的错误串 ({len(rep['errors'])}) --")
            for item in rep["errors"]:
                print(f"    {item['dim']}.{item['path']} = {item['value']}")
        if rep["diagnostics"]:
            print(f"\n  -- 内部诊断字段泄漏 ({len(rep['diagnostics'])}) [不计入覆盖率] --")
            for item in rep["diagnostics"]:
                print(f"    {item['dim']}.{item['path']} = {item['value']}")

    print(f"\n{'=' * 84}\n结构性恒空字段（凡是有该字段的标的，它都是空的）\n{'=' * 84}")
    per_dim: dict[str, dict[str, dict[str, set]]] = collections.defaultdict(
        lambda: {"present": collections.defaultdict(set), "empty": collections.defaultdict(set)}
    )
    tickers = [r["ticker"] for r in reports]
    for rep in reports:
        for dim, d in rep["dimensions"].items():
            for path in d["present"]:
                per_dim[dim]["present"][path].add(rep["ticker"])
            for path in d["missing"]:
                per_dim[dim]["empty"][path].add(rep["ticker"])
            for path in d["empty_collection"]:
                per_dim[dim]["empty"][path].add(rep["ticker"])
            for path in d["error_string"]:
                per_dim[dim]["empty"][path].add(rep["ticker"])

    always_empty: dict[str, list[str]] = {}
    for dim in sorted(per_dim, key=dim_sort_key):
        hits = []
        for path, present_in in per_dim[dim]["present"].items():
            empty_in = per_dim[dim]["empty"].get(path, set())
            if empty_in and empty_in == present_in:
                hits.append(f"{path}  (空于 {len(present_in)}/{len(tickers)} 标的)")
        if hits:
            always_empty[dim] = sorted(hits)

    for dim, hits in always_empty.items():
        print(f"\n  [{dim}] {len(hits)} 个字段结构性恒空")
        for h in hits:
            print(f"      - {h}")
    if not always_empty:
        print("  （无）")

    if args.json_out:
        payload = {
            "generated_from": os.path.abspath(args.cache),
            "tickers": reports,
            "always_empty": always_empty,
        }
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump(payload, fh, ensure_ascii=False, indent=2)
        print(f"\n-> 明细已写入 {args.json_out}")

    if args.gaps_json:
        for rep in reports:
            # 把诊断字段 + 取数失败错误串降级为独立 sidecar，主产物 raw_data.json 不动。
            gaps = {
                "ticker": rep["ticker"],
                "market": rep["market"],
                "generated_at": datetime.now(timezone.utc).isoformat(),
                "diagnostics": [
                    {"dim": item["dim"], "path": item["path"], "value": item["value"]}
                    for item in rep["diagnostics"]
                ],
                "error_strings": [
                    {"dim": item["dim"], "path": item["path"], "value": item["value"]}
                    for item in rep["errors"]
                ],
                "missing_summary": {
                    dim: {
                        "leaf_fields": d["leaf_fields"],
                        "empty_total": d["empty_total"],
                        "empty_pct": d["empty_pct"],
                    }
                    for dim, d in rep["dimensions"].items()
                },
            }
            ticker_dir = os.path.join(args.cache, rep["ticker"])
            os.makedirs(ticker_dir, exist_ok=True)
            gaps_path = os.path.join(ticker_dir, "_data_gaps.json")
            with open(gaps_path, "w", encoding="utf-8") as fh:
                json.dump(gaps, fh, ensure_ascii=False, indent=2)
            print(f"-> gaps 已写入 {gaps_path}")

    if args.strict:
        empty_dims = [
            (r["ticker"], dim)
            for r in reports
            for dim, d in r["dimensions"].items()
            if d["leaf_fields"] and d["empty_total"] == d["leaf_fields"]
        ]
        sentinels = [(r["ticker"], e["dim"], e["path"]) for r in reports for e in r["errors"]]
        if empty_dims or sentinels:
            print("\n!! --strict 命中:", file=sys.stderr)
            for ticker, dim in empty_dims:
                print(f"     [100% 空] {ticker} · {dim}", file=sys.stderr)
            for ticker, dim, path in sentinels:
                print(f"     [错误串 ] {ticker} · {dim}.{path}", file=sys.stderr)
            return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())

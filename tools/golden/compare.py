#!/usr/bin/env python3
"""Deep-compare two JSON trees produced by the Python reference and the Rust port.

Usage:
    python3 tools/golden/compare.py <expected.json> <actual.json> [--float-tol 1e-6]

Exits non-zero and prints the first N differences. Key ORDER is compared too:
the port must reproduce Python's insertion order because renderers walk keys in
order.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

MAX_DIFFS = 25


def compare(exp, act, path: str, tol: float, diffs: list[str]) -> None:
    if len(diffs) >= MAX_DIFFS:
        return
    if isinstance(exp, dict) and isinstance(act, dict):
        for key in exp:
            if key not in act:
                diffs.append(f"{path}.{key}: MISSING in actual")
            else:
                compare(exp[key], act[key], f"{path}.{key}", tol, diffs)
        for key in act:
            if key not in exp:
                diffs.append(f"{path}.{key}: EXTRA in actual")
        exp_order = list(exp.keys())
        act_order = [k for k in act if k in exp]
        if exp_order != act_order:
            diffs.append(f"{path}: KEY ORDER {exp_order} != {act_order}")
        return
    if isinstance(exp, list) and isinstance(act, list):
        if len(exp) != len(act):
            diffs.append(f"{path}: LENGTH {len(exp)} != {len(act)}")
            return
        for i, (e, a) in enumerate(zip(exp, act)):
            compare(e, a, f"{path}[{i}]", tol, diffs)
        return
    if isinstance(exp, bool) or isinstance(act, bool):
        if exp != act:
            diffs.append(f"{path}: {exp!r} != {act!r}")
        return
    if isinstance(exp, (int, float)) and isinstance(act, (int, float)):
        if abs(float(exp) - float(act)) > tol:
            diffs.append(f"{path}: {exp!r} != {act!r} (delta {float(act) - float(exp)!r})")
        return
    if exp != act:
        diffs.append(f"{path}: {exp!r} != {act!r}")


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    tol = 1e-6
    if "--float-tol" in sys.argv:
        tol = float(sys.argv[sys.argv.index("--float-tol") + 1])
    if len(args) != 2:
        print(__doc__)
        return 2
    exp = json.loads(Path(args[0]).read_text(encoding="utf-8"))
    act = json.loads(Path(args[1]).read_text(encoding="utf-8"))
    diffs: list[str] = []
    compare(exp, act, "$", tol, diffs)
    if diffs:
        print(f"FAIL {args[1]} · {len(diffs)} difference(s):")
        for d in diffs:
            print("  " + d)
        return 1
    print(f"OK   {args[1]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

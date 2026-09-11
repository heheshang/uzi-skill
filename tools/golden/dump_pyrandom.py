#!/usr/bin/env python3
"""Dump CPython `random` streams for seats the Rust port must reproduce.

Usage:
    python3 tools/golden/dump_pyrandom.py [out.json]

`preview_with_mock.py` seeds with a fixed integer and then draws `random()`,
`randint`, `randrange`, and `choice`, so the port has to match CPython's MT19937
stream exactly — not merely be deterministic. Several seeds are covered, each
exercised past the 624-word state regeneration boundary.
"""
from __future__ import annotations

import json
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_OUT = HERE / "expected" / "pyrandom" / "streams.json"

SEEDS = [0, 1, 42, 43, 12345, 2**31, 2**32 - 1, 2**40 + 7]
CHOICE_SEQ = ["a", "b", "c", "d", "e"]


def main() -> int:
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_OUT
    cases = []

    for seed in SEEDS:
        random.seed(seed)
        draws = {
            "random": [random.random() for _ in range(24)],
            "randint_55_95": [random.randint(55, 95) for _ in range(24)],
            "randrange_5_96": [random.randrange(5, 96) for _ in range(24)],
            "randint_-8_5": [random.randint(-8, 5) for _ in range(24)],
            "choice": [random.choice(CHOICE_SEQ) for _ in range(24)],
        }

        # Word-level checks straddling the 624-word regeneration boundary.
        random.seed(seed)
        words = [random.getrandbits(32) for _ in range(700)]

        random.seed(seed)
        below = [random.randrange(0, 1_000_000_007) for _ in range(24)]

        cases.append(
            {
                "seed": seed,
                "draws": draws,
                "getrandbits32": words,
                "randbelow_1e9": below,
            }
        )

    payload = {"choice_seq": CHOICE_SEQ, "cases": cases}
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"wrote {out_path} ({len(cases)} seeds)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

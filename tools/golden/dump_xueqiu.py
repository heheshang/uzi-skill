#!/usr/bin/env python3
"""Dump upstream XueQiu parser output for fixed page HTML.

Usage:
    python3 tools/golden/dump_xueqiu.py [out.json]

`lib/xueqiu_browser.py` fetches through Playwright, which needs a login and the
network. The *parsing* half is what the Rust port reimplements, so this feeds
both implementations the same HTML with `fetch_with_browser` monkeypatched — the
regexes, field projection, normalization, dedup, and caps under test are still
upstream's own.

`xq_symbol` is exercised directly, including the branch interactions.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

UPSTREAM = Path("/tmp/uzi-src/skills/deep-analysis/scripts")
HERE = Path(__file__).resolve().parent
DEFAULT_OUT = HERE / "expected" / "xueqiu" / "parsers.json"

# ─── Cubes ───────────────────────────────────────────────────────────────────
CUBES_HTML = (
    '<html><body>{"list":['
    '{"name":"稳健组合","symbol":"ZH001","daily_gain":1.2,"monthly_gain":3.4,'
    '"total_gain":56.7,"annualized_gain_rate":12.3,"stocks_count":8,'
    '"view_rebalancing_count":4,"owner":{"screen_name":"张三"}},'
    '{"name":"激进组合","symbol":"ZH002","daily_gain":-0.5,"monthly_gain":9.1,'
    '"total_gain":120.4,"annualized_gain_rate":31.7,"stocks_count":15,'
    '"view_rebalancing_count":22,"owner":{"screen_name":"李四"}},'
    '{"name":"无主组合","symbol":"ZH003"}'
    ']}</body></html>'
)
CUBES_ALT_KEY_HTML = '<html>{"cubes":[{"name":"A","symbol":"ZH009","total_gain":5}]}</html>'
PEERS_CUBES_INVALID = '<html><body>{"list": not valid json}</body></html>'
NO_JSON_HTML = "<html><body>no json here</body></html>"
EMPTY_HTML = ""

# ─── Peers ───────────────────────────────────────────────────────────────────
PEERS_HTML = """
<div class="peers">
  <a href="/S/SH600519" class="x">贵州茅台</a>
  <a href="/S/SZ000582">北部湾港</a>
  <a href="/S/SH600519">贵州茅台</a>
  <a href="/S/SH600520">短</a>
  <a href="/S/HK00700">腾讯控股</a>
  <a href="/S/BJ430047">诺思兰德</a>
  <a href="/S/US/AAPL">苹果</a>
</div>
"""
PEERS_CAPPED_HTML = "".join(
    f'<a href="/S/SH6005{i:02d}">公司{i:02d}</a>' for i in range(50)
)
PEERS_UNCLOSED = "<a href='/S/SH600519'>unclosed"

SYMBOLS = [
    "600519", "900001", "000582", "300750", "430047", "830799",
    "00700", "700", "1", "500", "aapl", " 600519 ",
]


def main() -> int:
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_OUT
    sys.path.insert(0, str(UPSTREAM))
    import lib.xueqiu_browser as xq

    # `fetch_peers_via_browser` early-returns unless login is enabled; the browser
    # itself is never started because `fetch_with_browser` is stubbed below.
    import os

    os.environ["UZI_XQ_LOGIN"] = "1"

    # Never start a browser; serve canned HTML to the real parsers.
    pages = {
        "cubes": CUBES_HTML,
        "cubes_alt_key": CUBES_ALT_KEY_HTML,
        "cubes_invalid": PEERS_CUBES_INVALID,
        "no_json": NO_JSON_HTML,
        "empty": EMPTY_HTML,
    }
    current = {"html": ""}
    xq.fetch_with_browser = lambda url, timeout=15: current["html"]

    def cubes_for(name: str):
        current["html"] = pages[name]
        return xq.fetch_cubes_via_browser("SH600519", limit=50)

    peer_cases = {}
    for case, (html, cap) in {
        "mixed": (PEERS_HTML, 20),
        "capped": (PEERS_CAPPED_HTML, 5),
        "unclosed": (PEERS_UNCLOSED, 20),
        "empty": (EMPTY_HTML, 20),
    }.items():
        current["html"] = html
        peer_cases[case] = xq.fetch_peers_via_browser("600519", max_peers=cap)

    payload = {
        "cubes": {name: cubes_for(name) for name in pages},
        "peers": peer_cases,
    }

    # `xq_symbol` is defined inline in both call sites; rebuild it faithfully
    # from the module source so the dump tests the real branch order.
    def xq_symbol(stock_code: str) -> str:
        code_str = str(stock_code).strip()
        if code_str.startswith("6") or code_str.startswith("9"):
            return f"SH{code_str}"
        elif code_str.startswith(("0", "3")):
            return f"SZ{code_str}"
        elif code_str.startswith(("4", "8")):
            return f"BJ{code_str}"
        elif code_str.isdigit() and len(code_str) <= 5:
            return f"HK{code_str.zfill(5)}"
        return code_str.upper()

    payload["symbols"] = {s: xq_symbol(s) for s in SYMBOLS}

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

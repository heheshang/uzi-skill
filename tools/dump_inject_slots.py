#!/usr/bin/env python3
"""Extract one sample of the markup Rust injects at each <!-- INJECT_* --> marker.

Writes /tmp/inject/<MARKER>.html so a template preview layer can reproduce the
exact design (class names, nesting) without guessing.
"""
import re
import sys
from pathlib import Path
from bs4 import BeautifulSoup

ROOT = Path("/Users/shang/Documents/workspace/personal/ai/uzi-skill")
REPORT = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "reports/BTC-USD_20260913/full-report.html")
OUT = Path("/tmp/inject")
if not OUT.exists():          # note: mkdir(exist_ok=True) raises EEXIST under the WB python shim
    OUT.mkdir(parents=True)

html = REPORT.read_text(encoding="utf-8")
soup = BeautifulSoup(html, "lxml")

# marker -> (selector, index)
SLOTS = {
    "INJECT_JURY_SEATS":             ("#jury-seats", 0),
    "INJECT_CHAT_MESSAGES":          ("#chat-messages", 0),
    "INJECT_FRIENDLY_LAYER":         (".friendly-trio", 0),
    "INJECT_FUND_MANAGERS":          (".fund-mgr-section", 0),
    "INJECT_DEBATE_ROUNDS":          (".debate-rounds", 0),
    "INJECT_PANEL_INSIGHTS":         (".panel-insights", 0),
    "INJECT_SCHOOL_SCORES":          (".school-scores", 0),
    "INJECT_INSTITUTIONAL_MODELING": (".inst-modeling-wrap", 0),
    "INJECT_RISKS":                  (".risk-box", 0),
    "INJECT_VOTE_BARS":              (".sc-votes", 0),
    "INJECT_TOP3_BULLS":             (".sc-best", 0),
    "INJECT_TOP3_BEARS":             (".sc-best", 1),
}
for i, name in enumerate(["FINANCIAL", "MARKET", "INDUSTRY", "COMPANY", "ENV", "SAFETY"]):
    SLOTS[f"INJECT_DIM_{name}"] = (".dim-row", i)

for marker, (sel, idx) in SLOTS.items():
    nodes = soup.select(sel)
    if idx >= len(nodes):
        print(f"  {marker:<30} {sel}[{idx}] -> MISSING (found {len(nodes)})")
        continue
    frag = nodes[idx].decode_contents()
    (OUT / f"{marker}.html").write_text(frag, encoding="utf-8")
    print(f"  {marker:<30} {sel}[{idx}] {len(frag):>7}B  children={len(nodes[idx].find_all(recursive=False))}")

# positional ones: no wrapper element of their own
nav_end = html.index("</nav>")
hero = html.index('<div class="bento-hero"')
(OUT / "_TOPBANNERS.html").write_text(html[nav_end + 6:hero], encoding="utf-8")
print(f"  {'_TOPBANNERS (banner+chip)':<30} positional  {hero - nav_end:>7}B")

imw = soup.select_one(".inst-modeling-wrap")
risks_head = soup.select_one("#section-risks")
if imw and risks_head:
    seg = html[html.index(imw.decode()[-80:]) + 80: html.index('id="section-risks"')]
    (OUT / "_SEGMENTAL.html").write_text(seg, encoding="utf-8")
    print(f"  {'_SEGMENTAL':<30} positional  {len(seg):>7}B")

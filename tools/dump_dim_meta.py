#!/usr/bin/env python3
"""Print the dimension-card metadata found in a generated report (for template demo data)."""
import sys
from pathlib import Path
from bs4 import BeautifulSoup

report = Path(sys.argv[1] if len(sys.argv) > 1 else
              "/Users/shang/Documents/workspace/personal/ai/uzi-skill/reports/BTC-USD_20260913/full-report.html")
soup = BeautifulSoup(report.read_text(encoding="utf-8"), "lxml")

cards = soup.select(".dim-card")
print("dim cards found:", len(cards))
for card in cards:
    num_el = card.select_one(".dim-num")
    title_el = card.select_one(".dim-title")
    en_el = card.select_one(".dim-en")
    score_el = card.select_one(".dim-score .num")
    label_el = card.select_one(".dim-label")
    get = lambda el: el.get_text(strip=True) if el else ""
    print(f"  {get(num_el):<24} {get(title_el):<12} {get(en_el):<18} "
          f"{get(score_el):<4} {get(label_el)[:46]}")

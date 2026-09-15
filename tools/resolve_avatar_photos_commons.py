#!/usr/bin/env python3
"""Stage 1b: fill gaps via Wikimedia Commons search.

For each investor still missing a portrait, search Commons (namespace 6 = File)
and accept only files whose name actually contains the person's surname/full name.
"""
import json
import re
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

UA = {
    "User-Agent": "uzi-skill-avatar-research/1.0 "
    "(https://github.com/uzi-skill; educational research; contact: local)"
}

# investors still needing a portrait -> (search terms, required name tokens)
NEEDED = {
    "fisher": (["Philip Arthur Fisher", "Philip Fisher economist"], ["fisher"]),
    "lynch": (["Peter Lynch investor", "Peter Lynch Fidelity"], ["lynch"]),
    "druck": (["Stanley Druckenmiller"], ["druckenmiller"]),
    "burry": (["Michael Burry"], ["burry"]),
    "chanos": (["Jim Chanos"], ["chanos"]),
    "minervini": (["Mark Minervini"], ["minervini"]),
    "darvas": (["Nicolas Darvas"], ["darvas"]),
    "gann": (["William Delbert Gann", "W. D. Gann"], ["gann"]),
    "thorp": (["Edward Thorp", "Ed Thorp"], ["thorp"]),
    "shaw": (["David E. Shaw", "David Shaw computer scientist"], ["shaw"]),
    "templeton": (["John Templeton investor", "Sir John Templeton businessman"], ["templeton"]),
    "robertson": (["Julian Robertson investor", "Julian H. Robertson Jr."], ["robertson"]),
    "marks": (["Howard Marks Oaktree", "Howard Stanley Marks investor"], ["marks"]),
    "oneill": (["William O'Neil Investor's Business Daily", "William J. O'Neil"], ["o'neil", "oneil"]),
    "duan": (["段永平", "Duan Yongping"], ["段永平", "duan", "yongping"]),
    "zhangkun": (["张坤 基金经理", "Zhang Kun E Fund"], ["张坤", "zhang kun"]),
    "zhushaoxing": (["朱少醒"], ["朱少醒", "zhushaoxing", "zhu shaoxing"]),
    "xiezhiyu": (["谢治宇"], ["谢治宇", "xiezhiyu", "xie zhiyu"]),
    "fengliu": (["冯柳"], ["冯柳", "feng liu"]),
    "dengxiaofeng": (["邓晓峰"], ["邓晓峰", "deng xiaofeng"]),
    "zhang_lei": (["张磊 高瓴", "Zhang Lei Hillhouse"], ["张磊", "zhang lei"]),
}

BAD = re.compile(r"(logo|icon|flag|map|signature|grave|tomb|book|cover|"
                 r"building|library|lodge|house|hotel|street|city|"
                 r"commons-|wiki|disambig|\.svg$|\.pdf$|\.ogg$)", re.I)


def api(host: str, params: dict, retries: int = 4) -> dict:
    q = urllib.parse.urlencode(params)
    url = f"https://{host}/w/api.php?{q}"
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers=UA)
            return json.load(urllib.request.urlopen(req, timeout=30))
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(8 * (attempt + 1))
                continue
            raise
    raise RuntimeError("retries exhausted")


def search_commons(terms: list[str], tokens: list[str], size: int = 512):
    """Return (file_title, thumb_url) or None."""
    for term in terms:
        d = api("commons.wikimedia.org", {
            "action": "query",
            "generator": "search",
            "gsrsearch": term,
            "gsrnamespace": 6,
            "gsrlimit": 20,
            "prop": "imageinfo",
            "iiprop": "url|mime|size",
            "iiurlwidth": size,
            "format": "json",
        })
        pages = d.get("query", {}).get("pages", {})
        cands = []
        for p in pages.values():
            title = p.get("title", "")
            ii = p.get("imageinfo")
            if not ii:
                continue
            mime = ii[0].get("mime", "")
            if not mime.startswith("image/") or "svg" in mime:
                continue
            if BAD.search(title):
                continue
            low = title.lower()
            if not any(tok.lower() in low for tok in tokens):
                continue
            # prefer larger originals (portraits are usually >= 300px)
            w = ii[0].get("width", 0)
            cands.append((w, title, ii[0].get("thumburl") or ii[0].get("url")))
        if cands:
            cands.sort(key=lambda c: -c[0])
            return cands[0][1], cands[0][2]
        time.sleep(1.5)
    return None


if __name__ == "__main__":
    base = json.loads(
        (Path(__file__).with_name("avatar_photo_urls.json")).read_text(encoding="utf-8")
    )
    added, still = [], []
    for iid, (terms, tokens) in NEEDED.items():
        try:
            r = search_commons(terms, tokens)
        except Exception as e:
            print(f"  {iid}: error {e}", file=sys.stderr)
            r = None
        if r:
            base[iid] = {"url": r[1], "title": r[0], "lang": "commons", "via": f"commons:{r[0]}"}
            added.append(iid)
            print(f"OK   {iid} -> {r[0]}")
        else:
            still.append(iid)
            print(f"MISS {iid}")
        time.sleep(1.5)

    out = Path(__file__).with_name("avatar_photo_urls.json")
    out.write_text(json.dumps(base, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nadded={len(added)} still_missing={len(still)} total={len(base)}", file=sys.stderr)
    print("STILL:", still, file=sys.stderr)

#!/usr/bin/env python3
"""Resolve Wikipedia/Wikimedia portrait URLs for the uzi-skill investor roster.

Stage 1 of the "real photo avatars" pipeline: map each investor id to a
Wikipedia title, then resolve a portrait image URL.

Strategy:
  1. `prop=pageimages` (batched, 50 titles/request) -> canonical lead image.
  2. Fallback `prop=images` -> pick the first plausible portrait file, then
     resolve its URL via `prop=imageinfo`.

Writes `tools/avatar_photo_urls.json` = {id: {"url":..., "title":..., "lang":...}}
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

# id -> (lang, wikipedia title)
MAPPING = {
    # ── A 价值 ──
    "buffett": ("en", "Warren Buffett"),
    "graham": ("en", "Benjamin Graham"),
    "fisher": ("en", "Philip Fisher"),
    "munger": ("en", "Charlie Munger"),
    "templeton": ("en", "John Templeton"),
    "klarman": ("en", "Seth Klarman"),
    # ── B 成长 ──
    "lynch": ("en", "Peter Lynch"),
    "oneill": ("en", "William O'Neil"),
    "thiel": ("en", "Peter Thiel"),
    "wood": ("en", "Cathie Wood"),
    "andreessen": ("en", "Marc Andreessen"),
    "gurley": ("en", "Bill Gurley"),
    "naval": ("en", "Naval Ravikant"),
    "gerstner": ("en", "Brad Gerstner"),
    "chamath": ("en", "Chamath Palihapitiya"),
    # ── C 宏观 ──
    "soros": ("en", "George Soros"),
    "dalio": ("en", "Ray Dalio"),
    "marks": ("en", "Howard Marks"),
    "druck": ("en", "Stanley Druckenmiller"),
    "robertson": ("en", "Julian Robertson"),
    "burry": ("en", "Michael Burry"),
    "chanos": ("en", "Jim Chanos"),
    # ── D 技术 ──
    "livermore": ("en", "Jesse Livermore"),
    "minervini": ("en", "Mark Minervini"),
    "darvas": ("en", "Nicolas Darvas"),
    "gann": ("en", "W. D. Gann"),
    # ── E 中国价投 ──
    "duan": ("zh", "段永平"),
    "zhangkun": ("zh", "张坤"),
    "zhushaoxing": ("zh", "朱少醒"),
    "xiezhiyu": ("zh", "谢治宇"),
    "fengliu": ("zh", "冯柳"),
    "dengxiaofeng": ("zh", "邓晓峰"),
    "zhang_lei": ("zh", "张磊"),
    # ── G 量化 ──
    "simons": ("en", "Jim Simons"),
    "thorp": ("en", "Edward O. Thorp"),
    "shaw": ("en", "David E. Shaw"),
    "asness": ("en", "Cliff Asness"),
    # ── H 科技领袖 ──
    "jensen_huang": ("en", "Jensen Huang"),
    "musk": ("en", "Elon Musk"),
    "altman": ("en", "Sam Altman"),
    "saylor": ("en", "Michael Saylor"),
}

BAD_FILE_HINTS = re.compile(
    r"(logo|icon|flag|map|signature|grave|tomb|headstone|book|cover|"
    r"commons-|wikimedia|wiki|disambig|edit-|ambox|portal|"
    r"\.svg$|\.ogg$|\.webm$|\.pdf$)",
    re.I,
)


def api(lang: str, params: dict, retries: int = 4) -> dict:
    q = urllib.parse.urlencode(params)
    url = f"https://{lang}.wikipedia.org/w/api.php?{q}"
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers=UA)
            return json.load(urllib.request.urlopen(req, timeout=30))
        except urllib.error.HTTPError as e:
            if e.code == 429:
                wait = 8 * (attempt + 1)
                print(f"    429 rate-limited, waiting {wait}s...", file=sys.stderr)
                time.sleep(wait)
                continue
            raise
    raise RuntimeError("too many retries")


def batch_pageimages(lang: str, titles: list[str], size: int = 512) -> dict[str, str | None]:
    """titles -> thumbnail url (or None)."""
    out: dict[str, str | None] = {}
    for i in range(0, len(titles), 40):
        chunk = titles[i : i + 40]
        d = api(
            lang,
            {
                "action": "query",
                "titles": "|".join(chunk),
                "prop": "pageimages",
                "pithumbsize": size,
                "format": "json",
                "redirects": 1,
            },
        )
        # map normalised/redirected titles back
        norm = {}
        for key in ("normalized", "redirects"):
            for entry in d.get("query", {}).get(key, []):
                norm[entry["from"]] = entry["to"]
        pages = d.get("query", {}).get("pages", {})
        by_title = {}
        for p in pages.values():
            t = p.get("title")
            by_title[t] = p.get("thumbnail", {}).get("source")
        for t in chunk:
            resolved = norm.get(t, t)
            out[t] = by_title.get(resolved) or by_title.get(t)
        time.sleep(1.5)
    return out


def page_image_files(lang: str, title: str) -> list[str]:
    d = api(
        lang,
        {
            "action": "query",
            "titles": title,
            "prop": "images",
            "imlimit": 60,
            "format": "json",
            "redirects": 1,
        },
    )
    files: list[str] = []
    for p in d.get("query", {}).get("pages", {}).values():
        for im in p.get("images", []):
            files.append(im["title"])
    return files


def imageinfo_url(lang: str, file_titles: list[str], size: int = 512) -> dict[str, str | None]:
    out: dict[str, str | None] = {}
    for i in range(0, len(file_titles), 40):
        chunk = file_titles[i : i + 40]
        d = api(
            lang,
            {
                "action": "query",
                "titles": "|".join(chunk),
                "prop": "imageinfo",
                "iiprop": "url|mime",
                "iiurlwidth": size,
                "format": "json",
            },
        )
        for p in d.get("query", {}).get("pages", {}).values():
            ii = p.get("imageinfo")
            if ii:
                out[p["title"]] = ii[0].get("thumburl") or ii[0].get("url")
            else:
                out[p["title"]] = None
        time.sleep(1.5)
    return out


def resolve() -> dict:
    result: dict[str, dict] = {}
    by_lang: dict[str, list[tuple[str, str]]] = {}
    for iid, (lang, title) in MAPPING.items():
        by_lang.setdefault(lang, []).append((iid, title))

    for lang, pairs in by_lang.items():
        titles = [t for _, t in pairs]
        print(f"[{lang}] batch pageimages for {len(titles)} titles...", file=sys.stderr)
        thumbs = batch_pageimages(lang, titles)

        missing = [(iid, t) for iid, t in pairs if not thumbs.get(t)]
        print(f"[{lang}] {len(missing)} missing -> fallback prop=images", file=sys.stderr)

        for iid, title in pairs:
            url = thumbs.get(title)
            if url:
                result[iid] = {"url": url, "title": title, "lang": lang, "via": "pageimages"}
                continue
            # fallback: list page images, pick a plausible portrait
            try:
                files = page_image_files(lang, title)
            except Exception as e:
                print(f"    {iid}: page_image_files failed: {e}", file=sys.stderr)
                files = []
            candidates = [f for f in files if not BAD_FILE_HINTS.search(f)]
            # prefer jpg/jpeg/png
            candidates = [f for f in candidates if re.search(r"\.(jpe?g|png)$", f, re.I)]
            if not candidates:
                print(f"    {iid}: no candidate files", file=sys.stderr)
                continue
            # prefer files whose name shares a token with the person's name
            surname = title.split()[-1].lower()
            candidates.sort(key=lambda f: (surname not in f.lower(), len(f)))
            picked = candidates[:3]
            info = imageinfo_url(lang, picked)
            for f in picked:
                if info.get(f):
                    result[iid] = {
                        "url": info[f],
                        "title": title,
                        "lang": lang,
                        "via": f"images:{f}",
                    }
                    break
            if iid not in result:
                print(f"    {iid}: imageinfo gave nothing", file=sys.stderr)
    return result


if __name__ == "__main__":
    res = resolve()
    out = Path(__file__).with_name("avatar_photo_urls.json")
    out.write_text(json.dumps(res, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nresolved {len(res)}/{len(MAPPING)} -> {out}", file=sys.stderr)
    for iid in MAPPING:
        print(("OK  " if iid in res else "MISS"), iid, res.get(iid, {}).get("via", ""))

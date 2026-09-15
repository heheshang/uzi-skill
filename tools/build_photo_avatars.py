#!/usr/bin/env python3
"""Stage 2: download verified portraits and wrap them as <id>.svg.

Why SVG wrappers instead of plain .jpg?
  The whole report pipeline hardcodes `avatars/{id}.svg` (template placeholders,
  panel_cards.rs, avatars.rs, inline.rs). Wrapping the JPEG in an SVG keeps every
  existing reference valid — no Rust changes, no recompile.

The <image> uses preserveAspectRatio="xMidYMin slice" so the photo is cropped to a
square anchored at the top-centre (where a portrait's head usually sits).
"""
import base64
import json
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UA = {
    "User-Agent": "uzi-skill-avatar-research/1.0 "
    "(https://github.com/uzi-skill; educational research; contact: local)"
}
CACHE = ROOT / "tools" / "photo_cache"
AVATARS = ROOT / "assets" / "avatars"
SIZE = 128


def fetch(url: str) -> tuple[bytes, str]:
    url = url.split("?")[0]  # strip utm_* tracking params
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=60) as r:
        ctype = r.headers.get("Content-Type", "image/jpeg").split(";")[0].strip()
        return r.read(), ctype


def wrap(data: bytes, mime: str) -> str:
    b64 = base64.b64encode(data).decode("ascii")
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}" '
        f'width="{SIZE}" height="{SIZE}">'
        f'<image width="{SIZE}" height="{SIZE}" preserveAspectRatio="xMidYMin slice" '
        f'href="data:{mime};base64,{b64}"/></svg>'
    )


if __name__ == "__main__":
    urls = json.loads((ROOT / "tools" / "avatar_photo_urls.json").read_text(encoding="utf-8"))
    CACHE.mkdir(parents=True, exist_ok=True)
    ok, fail = [], []
    for iid, meta in sorted(urls.items()):
        try:
            data, mime = fetch(meta["url"])
            ext = {"image/jpeg": "jpg", "image/png": "png", "image/webp": "webp"}.get(mime, "jpg")
            (CACHE / f"{iid}.{ext}").write_bytes(data)
            (AVATARS / f"{iid}.svg").write_text(wrap(data, mime), encoding="utf-8")
            ok.append(iid)
            print(f"OK   {iid:14s} {len(data):>7d} B  {mime}")
        except Exception as e:
            fail.append(iid)
            print(f"FAIL {iid:14s} {e}", file=sys.stderr)
        time.sleep(0.4)
    print(f"\nwrote {len(ok)} avatars, {len(fail)} failed -> {AVATARS}", file=sys.stderr)
    if fail:
        print("FAILED:", fail, file=sys.stderr)

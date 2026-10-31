#!/usr/bin/env python3
"""
Build engine/data/eg_freq.txt: colloquial (mostly Egyptian) Arabic word
counts, used as an extra ranking signal next to OpenSubtitles.

Sources (both GPL-2.0, same licence as the bundled Tashkeela data):
  - ASTD, Arabic Sentiment Tweets Dataset (Nabil et al., 2015)
    https://github.com/mahmoudnabil/ASTD
  - LABR, Large-scale Arabic Book Reviews (Aly & Atiya, 2013)
    https://github.com/mohamedadaly/LABR

Output: `word count` lines, most frequent first, words seen at least
MIN_COUNT times, diacritics and tatweel removed.

Run:
    python build_eg_freq.py
"""

import collections
import re
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "data" / "eg_freq.txt"
SOURCES = [
    "https://raw.githubusercontent.com/mahmoudnabil/ASTD/master/data/Tweets.txt",
    "https://raw.githubusercontent.com/mohamedadaly/LABR/master/data/reviews.tsv",
]
MIN_COUNT = 3
TOKEN_RE = re.compile(r"[ء-ي]+")
STRIP_RE = re.compile(r"[ً-ْـ]")


def main():
    counts = collections.Counter()
    for url in SOURCES:
        print(f"[download] {url}")
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req) as r:
            text = r.read().decode("utf-8", errors="ignore")
        for token in TOKEN_RE.findall(STRIP_RE.sub("", text)):
            if len(token) >= 2:
                counts[token] += 1
    items = [(w, n) for w, n in counts.most_common() if n >= MIN_COUNT]
    OUTPUT.write_text("".join(f"{w} {n}\n" for w, n in items), encoding="utf-8")
    print(f"[wrote] {OUTPUT} ({len(items):,} words)")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
Build engine/data/tashkeela_vocalized.txt from the Tashkeela corpus.

Source: https://sourceforge.net/projects/tashkeela/ (Taha Zerrouki, GPL).
75M+ fully-vocalized words from Shamela classical books + MSA.

Pipeline:
  1. Locate or download a Tashkeela archive (.zip).
  2. Walk all .txt files in the archive, tokenize on whitespace/punct.
  3. Keep only tokens whose base (tashkeel-stripped) is pure Arabic letters
     AND that carry at least one tashkeel mark (so we don't dilute the file
     with already-unvocalized noise).
  4. For each base form, pick the most-frequent vocalized variant.
  5. Sort by base-form frequency descending, cap at MAX_ENTRIES.
  6. Write `vocalized<TAB>count` lines.

Run:
    python build_tashkeela.py                # downloads to ./.cache/
    python build_tashkeela.py --input PATH   # use a local archive
"""

import argparse
import collections
import os
import re
import sys
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = ROOT / "data"
CACHE_DIR = ROOT / ".cache"
OUTPUT = DATA_DIR / "tashkeela_vocalized.txt"

# HuggingFace mirror of the Tashkeela corpus as parquet (GPL-2.0).
# SourceForge's direct download requires a JS-rendered mirror redirect that
# breaks plain curl/urllib — this mirror is byte-stable and CDN-backed.
HF_PARQUET_URLS = [
    "https://huggingface.co/datasets/community-datasets/tashkeela/resolve/main/plain_text/train-00000-of-00003.parquet",
    "https://huggingface.co/datasets/community-datasets/tashkeela/resolve/main/plain_text/train-00001-of-00003.parquet",
    "https://huggingface.co/datasets/community-datasets/tashkeela/resolve/main/plain_text/train-00002-of-00003.parquet",
]

MAX_ENTRIES = 500_000
TASHKEEL_CHARS = set("ًٌٍَُِّْٰ")
# Allow Arabic letters (incl. hamza forms) and combining marks.
ARABIC_LETTER_RE = re.compile(r"^[ء-ي٠-٩ٰ-ۓً-ْ]+$")
TOKEN_SPLIT_RE = re.compile(r"[^ء-ي٠-٩ٰ-ۓً-ْ]+")


def strip_tashkeel(s: str) -> str:
    return "".join(c for c in s if c not in TASHKEEL_CHARS)


def has_tashkeel(s: str) -> bool:
    return any(c in TASHKEEL_CHARS for c in s)


def download(url: str, dest: Path) -> Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() and dest.stat().st_size > 1_000_000:
        print(f"[cache] {dest} ({dest.stat().st_size:,} bytes)", file=sys.stderr)
        return dest
    print(f"[download] {url}", file=sys.stderr)
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req) as r, open(dest, "wb") as f:
        total = 0
        while True:
            chunk = r.read(1 << 20)
            if not chunk:
                break
            f.write(chunk)
            total += len(chunk)
            if total % (1 << 24) < (1 << 20):
                print(f"  ... {total:,} bytes", file=sys.stderr)
    print(f"[done] {total:,} bytes", file=sys.stderr)
    return dest


def iter_text_from_zip(zip_path: Path):
    with zipfile.ZipFile(zip_path) as zf:
        names = [n for n in zf.namelist() if n.lower().endswith(".txt")]
        print(f"[archive] {len(names)} .txt files", file=sys.stderr)
        for i, name in enumerate(names):
            if i % 200 == 0:
                print(f"  reading {i}/{len(names)} ({name})", file=sys.stderr)
            try:
                with zf.open(name) as f:
                    raw = f.read()
            except Exception as e:
                print(f"  skip {name}: {e}", file=sys.stderr)
                continue
            for enc in ("utf-8", "windows-1256", "cp1252"):
                try:
                    yield raw.decode(enc)
                    break
                except UnicodeDecodeError:
                    continue


def iter_text_from_parquets(paths):
    import pyarrow.parquet as pq
    for path in paths:
        print(f"[parquet] {path}", file=sys.stderr)
        pf = pq.ParquetFile(str(path))
        for rg in range(pf.num_row_groups):
            tbl = pf.read_row_group(rg)
            # The dataset's text column is named 'text'; fall back to last col
            # (which is the document body in this schema).
            names = tbl.column_names
            col = "text" if "text" in names else names[-1]
            for v in tbl.column(col).to_pylist():
                if v:
                    yield v


def tokenize_and_count(text_stream):
    """Yield (base, vocalized) pairs and tally counts."""
    base_counts = collections.Counter()
    # For each base form, track which vocalized variant occurred most often.
    variant_counts = collections.defaultdict(collections.Counter)

    n_tokens = 0
    n_vocal = 0
    for text in text_stream:
        for tok in TOKEN_SPLIT_RE.split(text):
            if not tok or len(tok) < 2:
                continue
            n_tokens += 1
            if not has_tashkeel(tok):
                continue
            if not ARABIC_LETTER_RE.match(tok):
                continue
            base = strip_tashkeel(tok)
            if len(base) < 2:
                continue
            n_vocal += 1
            base_counts[base] += 1
            variant_counts[base][tok] += 1

    print(f"[stats] {n_tokens:,} tokens scanned, {n_vocal:,} vocalized kept, "
          f"{len(base_counts):,} unique bases",
          file=sys.stderr)
    return base_counts, variant_counts


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--input", help="Path to a local Tashkeela .zip (skips HF download)")
    ap.add_argument("--max-entries", type=int, default=MAX_ENTRIES)
    args = ap.parse_args()

    DATA_DIR.mkdir(parents=True, exist_ok=True)

    if args.input:
        archive = Path(args.input)
        if not archive.exists():
            sys.exit(f"input not found: {archive}")
        stream = iter_text_from_zip(archive)
    else:
        parquet_paths = []
        for url in HF_PARQUET_URLS:
            dest = CACHE_DIR / Path(url).name
            parquet_paths.append(download(url, dest))
        stream = iter_text_from_parquets(parquet_paths)

    base_counts, variant_counts = tokenize_and_count(stream)

    # Pick the most common vocalized variant for each base.
    pairs = []
    for base, count in base_counts.most_common(args.max_entries):
        best_vocal = variant_counts[base].most_common(1)[0][0]
        pairs.append((best_vocal, count))

    OUTPUT.write_text(
        "\n".join(f"{v}\t{c}" for v, c in pairs) + "\n",
        encoding="utf-8",
    )
    print(f"[wrote] {OUTPUT} ({len(pairs):,} entries, "
          f"{OUTPUT.stat().st_size:,} bytes)",
          file=sys.stderr)


if __name__ == "__main__":
    main()

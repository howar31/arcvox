#!/usr/bin/env python3
"""Builds the two Chinese tables the core ships, from their sources.

usage: zh-tables.py <Unihan_Readings.txt> <out dir> <dict.txt>...

Writes `zh-readings.tsv` (one line per CJK character of the basic block:
the character, its customary Taiwan reading, its customary mainland
reading, and its dictionary readings, all as toneless pinyin) and
`zh-words.txt` (one word per line: the words of two to four characters
any of the dictionaries lists with a frequency of at least MIN_FREQ).

Sources: the Unihan database (Unicode licence; fields kMandarin,
kHanyuPinyin, kTGHZ2013); the jieba-tw dictionary (MIT) and the jieba
dictionary (MIT) converted to Traditional characters with OpenCC s2tw
beforehand (the core's own conversion crate does it).
"""

import sys
import unicodedata
from collections import defaultdict
from pathlib import Path

MIN_FREQ = 5
BASIC = range(0x4E00, 0xA000)


def toneless(syllable: str) -> str:
    """`quán` -> `quan`, `lǜ` -> `lv`."""
    out = []
    for c in unicodedata.normalize("NFD", syllable):
        if unicodedata.combining(c):
            continue
        out.append("v" if c == "ü" else c)
    return "".join(out).lower()


def cjk(text: str) -> bool:
    return all(ord(c) in BASIC for c in text)


def readings(path: Path) -> dict[int, tuple[str, str, list[str]]]:
    mandarin: dict[int, list[str]] = {}
    every: dict[int, list[str]] = defaultdict(list)
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.startswith("U+"):
            continue
        code, field, value = line.split("\t", 2)
        cp = int(code[2:], 16)
        if cp not in BASIC:
            continue
        if field == "kMandarin":
            mandarin[cp] = [toneless(v) for v in value.split()]
        elif field in ("kHanyuPinyin", "kTGHZ2013"):
            # `10602.010:hè,hé,huò` or `134.020:hé 135.090:hè`
            for entry in value.split():
                _, _, pinyins = entry.partition(":")
                for p in pinyins.split(","):
                    every[cp].append(toneless(p))
    table = {}
    for cp, values in mandarin.items():
        cn = values[0]
        tw = values[1] if len(values) > 1 else values[0]
        seen: list[str] = []
        for r in every.get(cp, []):
            if r and r not in seen:
                seen.append(r)
        table[cp] = (tw, cn, seen)
    return table


def words(paths: list[Path]) -> list[str]:
    out = set()
    for path in paths:
        for line in path.read_text(encoding="utf-8").splitlines():
            parts = line.split()
            if len(parts) < 2:
                continue
            word, freq = parts[0], parts[1]
            if not freq.isdigit() or int(freq) < MIN_FREQ:
                continue
            if 2 <= len(word) <= 4 and cjk(word):
                out.add(word)
    return sorted(out)


def main() -> None:
    unihan, out = Path(sys.argv[1]), Path(sys.argv[2])
    dictionaries = [Path(a) for a in sys.argv[3:]]
    table = readings(unihan)
    lines = [f"{chr(cp)}\t{tw}\t{cn}\t{','.join(every)}" for cp, (tw, cn, every) in sorted(table.items())]
    (out / "zh-readings.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")
    listed = words(dictionaries)
    (out / "zh-words.txt").write_text("\n".join(listed) + "\n", encoding="utf-8")
    print(f"{len(lines)} characters, {len(listed)} words")


if __name__ == "__main__":
    main()

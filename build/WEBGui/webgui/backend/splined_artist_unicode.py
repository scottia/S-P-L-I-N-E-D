"""ART-004 text operations using the same Unicode data as Windows."""

from bisect import bisect_left
from functools import cache
from pathlib import Path

from unicodedata2 import normalize, unidata_version


if unidata_version != "18.0.0":
    raise RuntimeError("ART-004 requires the packaged Unicode 18.0.0 data")


@cache
def _tables():
    folds = {}
    ranges = {"A": [], "G": []}
    path = Path(__file__).resolve().parents[1] / "artist_unicode_data.txt"
    for line in path.read_text(encoding="ascii").splitlines():
        if line.startswith("#"):
            continue
        kind, start, *rest = line.split()
        start = int(start, 16)
        if kind == "F":
            folds[start] = "".join(chr(int(value, 16)) for value in rest)
        else:
            ranges[kind].append((start, int(rest[0], 16)))
    return folds, {kind: tuple(zip(*values)) for kind, values in ranges.items()}


def casefold(value: str) -> str:
    if value.isascii():
        return value.lower()
    return value.translate(_tables()[0])


def _has_category(character: str, kind: str) -> bool:
    starts, ends = _tables()[1][kind]
    value = ord(character)
    index = bisect_left(ends, value)
    return index < len(starts) and starts[index] <= value


def alphanumeric(character: str) -> bool:
    return character.isalnum() if character.isascii() else _has_category(character, "A")


def normalized_fold(value: str) -> str:
    return casefold(normalize("NFKC", value))


def search_literal(value: str) -> str:
    return normalized_fold(value).strip()


def sort_key(value: str) -> str:
    return casefold("".join(
        character for character in normalize("NFKC", value)
        if not _has_category(character, "G")
    )).strip()

"""Portable ART-004 delimiters. Independent COMP expressions stay native."""

from dataclasses import dataclass
from functools import lru_cache
import re
import string


_SPACE = "\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000"
_ASCII_FOLD = str.maketrans(string.ascii_uppercase, string.ascii_lowercase)
_MAX_PROFILE_BYTES = 4096
_MAX_GROUP_DEPTH = 32
REMEDIATION = (
    "Incompatible Artist separator profile. Configured credit splitting and catalog refresh are paused. "
    "In Settings → Library & Files → Advanced, edit or import a portable profile, save it, "
    "then follow the catalog reset and refresh instructions."
)


@dataclass(frozen=True)
class ArtistSeparator:
    regex: re.Pattern[str] | None = None
    ignore_ascii_case: bool = False

    def matches(self, value: str):
        if self.regex is not None:
            if self.ignore_ascii_case:
                value = value.translate(_ASCII_FOLD)
            for match in self.regex.finditer(value):
                yield match.span()

    def split(self, value: str) -> list[str]:
        parts = []
        last = 0
        for start, end in self.matches(value):
            parts.append(value[last:start])
            last = end
        parts.append(value[last:])
        return parts

    def full_match(self, value: str) -> bool:
        if self.ignore_ascii_case:
            value = value.translate(_ASCII_FOLD)
        return self.regex is not None and self.regex.fullmatch(value) is not None


def _escaped(ch: str) -> str:
    if ch in ("t", "n", "r"):
        return {"t": "\t", "n": "\n", "r": "\r"}[ch]
    if ch and (ch in string.punctuation or ch == " "):
        return ch
    raise ValueError(REMEDIATION)


@lru_cache(maxsize=32)
def parse(profile: str) -> ArtistSeparator:
    try:
        profile_bytes = len(profile.encode("utf-8"))
    except UnicodeEncodeError as error:
        raise ValueError(REMEDIATION) from error
    if profile_bytes > _MAX_PROFILE_BYTES:
        raise ValueError(REMEDIATION)
    fold = profile.startswith("(?i)")
    source = profile[4:] if fold else profile
    cursor = 0

    def peek() -> str:
        return source[cursor:cursor + 1]

    def take() -> str:
        nonlocal cursor
        value = peek()
        cursor += 1
        return value

    def literal(ch: str) -> str:
        if fold and not ch.isascii():
            raise ValueError(REMEDIATION)
        return re.escape(ch.lower() if fold else ch)

    def character_class() -> str:
        def member(ch: str) -> str:
            if ch == "\\":
                return _escaped(take())
            if not ch or not ch.isascii() or ch in "[^":
                raise ValueError(REMEDIATION)
            return ch

        def encode(ch: str) -> str:
            return rf"\x{ord(ch.lower() if fold else ch):02x}"

        members = []
        while (ch := take()):
            if ch == "]":
                if members:
                    return "[" + "".join(members) + "]"
                break
            if ch in "&|~-" and peek() == ch:
                raise ValueError(REMEDIATION)
            if ch == "\\" and peek() == "s":
                take()
                members.append(_SPACE)
                continue
            first = member(ch)
            if peek() != "-":
                members.append(encode(first))
                continue
            take()
            if peek() == "-":
                raise ValueError(REMEDIATION)
            if peek() == "]":
                members.extend((encode(first), encode("-")))
                continue
            last = member(take())
            if first > last:
                raise ValueError(REMEDIATION)
            members.extend(encode(chr(code)) for code in range(ord(first), ord(last) + 1))
        raise ValueError(REMEDIATION)

    portable = []
    depth = 0
    can_repeat = can_repeat_many = has_atom = False
    while (ch := take()):
        if ch == "(":
            if depth == _MAX_GROUP_DEPTH:
                raise ValueError(REMEDIATION)
            if peek() == "?":
                take()
                if take() != ":":
                    raise ValueError(REMEDIATION)
            portable.append("(?:")
            depth += 1
            has_atom = can_repeat = False
        elif ch in ")|":
            if not has_atom or (ch == ")" and depth == 0):
                raise ValueError(REMEDIATION)
            portable.append(ch)
            if ch == ")":
                depth -= 1
            has_atom = can_repeat = ch == ")"
            can_repeat_many = False
        elif ch in "?*+":
            if not can_repeat or (ch != "?" and not can_repeat_many):
                raise ValueError(REMEDIATION)
            portable.append(ch)
            can_repeat = False
        elif ch in ".^{}$]":
            raise ValueError(REMEDIATION)
        else:
            if ch == "[":
                portable.append(character_class())
            elif ch == "\\":
                escaped = take()
                portable.append(f"[{_SPACE}]" if escaped == "s" else literal(_escaped(escaped)))
            else:
                portable.append(literal(ch))
            has_atom = can_repeat = can_repeat_many = True
    if depth or not has_atom:
        raise ValueError(REMEDIATION)
    try:
        regex = re.compile("".join(portable))
    except (re.error, RecursionError) as error:
        raise ValueError(REMEDIATION) from error
    if regex.match("") is not None:
        raise ValueError(REMEDIATION)
    return ArtistSeparator(regex, fold)


def warning(profile: str) -> str:
    try:
        parse(profile)
    except ValueError:
        return REMEDIATION
    return ""

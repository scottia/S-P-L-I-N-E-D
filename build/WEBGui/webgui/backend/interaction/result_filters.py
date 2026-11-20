"""Pure result filtering and ordering used by the Python processing engine.

The engine remains authoritative for discovery and policy.  These helpers only
control which already-discovered rows are presented during an interactive
review; the CURRENT ALBUM authority is deliberately outside these filters.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from difflib import SequenceMatcher
import re
from typing import Any, Iterable


MUSICBRAINZ_BASE_RELEASE_TYPES = (
    "Album",
    "Single",
    "EP",
    "Compilation",
    "Soundtrack",
)


def _value(item: Any, name: str, default: str = "") -> str:
    if isinstance(item, dict):
        return str(item.get(name, default) or default).strip()
    return str(getattr(item, name, default) or default).strip()


def musicbrainz_artist(item: dict[str, Any]) -> str:
    return _value(item, "artist") or _value(item, "release_artist") or _value(
        item, "recording_artist"
    )


def musicbrainz_release_type(item: dict[str, Any]) -> str:
    raw = _value(item, "release_class", "Release")
    aliases = {
        "album": "Album",
        "single": "Single",
        "ep": "EP",
        "compilation": "Compilation",
        "soundtrack": "Soundtrack",
    }
    return aliases.get(raw.casefold(), raw.title() or "Release")


def _date_sort_value(value: str) -> int:
    parts = value.strip().split("-")
    try:
        year = int(parts[0])
    except (IndexError, ValueError):
        return -1
    if year <= 0:
        return -1

    def part(index: int, fallback: int, maximum: int) -> int:
        try:
            parsed = int(parts[index])
        except (IndexError, ValueError):
            return fallback
        return parsed if 0 <= parsed <= maximum else fallback

    return year * 10000 + part(1, 0, 12) * 100 + part(2, 0, 31)


def _family_rank(item: dict[str, Any]) -> int:
    release_type = musicbrainz_release_type(item).casefold()
    artist = musicbrainz_artist(item)
    if release_type == "soundtrack":
        return 1
    if release_type == "compilation":
        return 2
    if artist.casefold() == "various artists":
        return 3
    if not artist:
        return 4
    return 0


def _integer_value(item: Any, name: str, default: int = 0) -> int:
    try:
        return int(_value(item, name, str(default)) or default)
    except (TypeError, ValueError):
        return default


def musicbrainz_title_match(query: str, title: str) -> int:
    """Estimate a display percentage when MusicBrainz did not supply one."""
    def normalized(value: str) -> str:
        return " ".join(re.findall(r"\w+", value.casefold()))

    left, right = normalized(query), normalized(title)
    if not left or not right:
        return 0
    return round(100 * SequenceMatcher(None, left, right).ratio())


def musicbrainz_match_score(item: dict[str, Any]) -> int:
    return max(0, min(100, _integer_value(item, "score")))


def musicbrainz_artist_album_preferred(item: dict[str, Any]) -> bool:
    if musicbrainz_release_type(item).casefold() not in {"album", "compilation"}:
        return False
    release_artist = _value(item, "release_artist") or musicbrainz_artist(item)
    recording_artist = _value(item, "recording_artist") or _value(item, "query_artist")
    def key(value: str) -> str:
        compact = "".join(character for character in value.casefold().replace("&", "and") if character.isalnum())
        return compact.removeprefix("the")
    return bool(release_artist and recording_artist and key(release_artist) == key(recording_artist))


def musicbrainz_match_section(item: dict[str, Any]) -> int:
    if _value(item, "selection_state") == "current" or musicbrainz_match_score(item) >= 90:
        return 0
    return 1 if musicbrainz_artist_album_preferred(item) else 2


def musicbrainz_sort_key(item: dict[str, Any]) -> tuple[Any, ...]:
    """Order scored results by section and percentage, then by release detail."""

    family = _family_rank(item)
    artist = musicbrainz_artist(item)
    country = _value(item, "country")
    country_rank = 0 if country.casefold() == "us" else 2 if not country else 1
    release_id = _value(item, "id") or _value(item, "release_mbid")
    title = _value(item, "title") or _value(item, "release_title")
    date_value = _value(item, "date") or _value(item, "release_date")
    scored = _value(item, "score") != ""
    return (
        musicbrainz_match_section(item) if scored else 0,
        -musicbrainz_match_score(item) if scored else 0,
        family,
        artist.casefold() if family == 0 else "",
        -_date_sort_value(date_value),
        country_rank,
        country.casefold(),
        title.casefold(),
        release_id.casefold(),
        _integer_value(item, "original_index"),
    )


def order_musicbrainz_results(
    items: Iterable[dict[str, Any]],
) -> list[dict[str, Any]]:
    return sorted(items, key=musicbrainz_sort_key)


@dataclass
class MusicBrainzFilter:
    """Combined Artist and Release Type filter for result rows."""

    artists: set[str] = field(default_factory=set)
    release_types: set[str] = field(default_factory=set)

    @classmethod
    def from_items(cls, items: Iterable[dict[str, Any]]) -> "MusicBrainzFilter":
        rows = list(items)
        return cls(
            {musicbrainz_artist(item) for item in rows},
            {musicbrainz_release_type(item) for item in rows},
        )

    def available_artists(self, items: Iterable[dict[str, Any]]) -> list[str]:
        return sorted(
            {musicbrainz_artist(item) for item in items}, key=str.casefold
        )

    def available_release_types(
        self, items: Iterable[dict[str, Any]]
    ) -> list[str]:
        rows = [
            item
            for item in items
            if musicbrainz_artist(item) in self.artists
        ]
        present = {musicbrainz_release_type(item) for item in rows}
        base = [value for value in MUSICBRAINZ_BASE_RELEASE_TYPES if value in present]
        return base + sorted(present.difference(base), key=str.casefold)

    def normalize(self, items: Iterable[dict[str, Any]]) -> None:
        rows = list(items)
        available_artists = set(self.available_artists(rows))
        if not self.artists:
            self.artists = available_artists
        else:
            self.artists.intersection_update(available_artists)
            if not self.artists:
                self.artists = available_artists
        available_types = set(self.available_release_types(rows))
        if not self.release_types:
            self.release_types = available_types
        else:
            self.release_types.intersection_update(available_types)
            if not self.release_types:
                self.release_types = available_types

    def visible(self, items: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
        rows = list(items)
        self.normalize(rows)
        return order_musicbrainz_results(
            item
            for item in rows
            if musicbrainz_artist(item) in self.artists
            and musicbrainz_release_type(item) in self.release_types
        )


def candidate_image_type(candidate: Any) -> str:
    source = _value(candidate, "source")
    provenance = _value(candidate, "provenance")
    if source.casefold() == "local" or provenance.casefold() == "[local]":
        return "Local"
    acceptable = bool(
        candidate.get("acceptable", False)
        if isinstance(candidate, dict)
        else getattr(candidate, "acceptable", False)
    )
    approved = bool(
        candidate.get("approved", False)
        if isinstance(candidate, dict)
        else getattr(candidate, "approved", False)
    )
    range_type = _value(candidate, "range_type").casefold()
    if not acceptable:
        return "Rejected"
    if range_type in {"belowminimum", "lowerrange"}:
        return "Upscalable"
    return "Recommended" if approved or acceptable else "Rejected"


def candidate_policy(candidate: Any) -> str:
    approved = bool(
        candidate.get("approved", False)
        if isinstance(candidate, dict)
        else getattr(candidate, "approved", False)
    )
    return "Strict" if approved else "Acceptable"


@dataclass
class CandidateFilter:
    """Interactive, non-destructive source/type/policy/range filter."""

    sources: set[str] = field(default_factory=set)
    image_types: set[str] = field(default_factory=set)
    policies: set[str] = field(default_factory=set)
    ranges: set[str] = field(default_factory=set)

    @classmethod
    def from_items(cls, items: Iterable[Any]) -> "CandidateFilter":
        rows = list(items)
        return cls(
            {_value(item, "source") for item in rows},
            {candidate_image_type(item) for item in rows},
            {candidate_policy(item) for item in rows},
            {_value(item, "range_type") for item in rows},
        )

    def reset(self, items: Iterable[Any]) -> None:
        replacement = self.from_items(items)
        self.sources = replacement.sources
        self.image_types = replacement.image_types
        self.policies = replacement.policies
        self.ranges = replacement.ranges

    def visible(self, items: Iterable[Any]) -> list[Any]:
        return [
            item
            for item in items
            if _value(item, "source") in self.sources
            and candidate_image_type(item) in self.image_types
            and candidate_policy(item) in self.policies
            and _value(item, "range_type") in self.ranges
        ]

    @property
    def active_count(self) -> int:
        return sum(
            bool(values)
            for values in (
                self.sources,
                self.image_types,
                self.policies,
                self.ranges,
            )
        )


def comparison_pair(items: Iterable[Any], selected_index: int) -> tuple[int, int] | None:
    """Return stable indexes for local-vs-selected visual comparison."""

    rows = list(items)
    if len(rows) < 2:
        return None
    selected = max(0, min(selected_index, len(rows) - 1))
    local = next(
        (
            index
            for index, item in enumerate(rows)
            if candidate_image_type(item) == "Local" and index != selected
        ),
        -1,
    )
    if local >= 0:
        return local, selected
    suggested = next(
        (
            index
            for index, item in enumerate(rows)
            if bool(
                item.get("suggested", False)
                if isinstance(item, dict)
                else getattr(item, "suggested", False)
            )
            and index != selected
        ),
        -1,
    )
    if suggested >= 0:
        return suggested, selected
    alternate = 0 if selected != 0 else 1
    return alternate, selected

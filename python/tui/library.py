"""In-memory Select Media model for the Ratatui workspace.

The engine supplies stable folder identities from the disposable picker index
and authoritative history states. This module filters and manages transient
selection only; it never reads the filesystem or persists status authority.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Iterable


class AlbumStatus(str, Enum):
    UNPROCESSED = "unprocessed"
    PROCESSED = "processed"
    BYPASSED = "bypassed"
    TIMEOUT = "timeout"


class ArtistStatus(str, Enum):
    UNPROCESSED = "unprocessed"
    PARTIAL = "partial"
    COMPLETE = "complete"
    CONTAINS_BYPASS = "contains-bypass"


STATUS_LABELS = {
    AlbumStatus.UNPROCESSED: "Unprocessed",
    AlbumStatus.PROCESSED: "Processed",
    AlbumStatus.BYPASSED: "Bypass",
    AlbumStatus.TIMEOUT: "Timeout active",
    ArtistStatus.UNPROCESSED: "Unprocessed",
    ArtistStatus.PARTIAL: "Partial",
    ArtistStatus.COMPLETE: "Complete",
    ArtistStatus.CONTAINS_BYPASS: "Contains bypass",
}


@dataclass
class AlbumItem:
    path: str
    artist: str
    title: str
    status: AlbumStatus
    formats: tuple[str, ...] = ()
    timeout_remaining: str = ""
    selected: bool = False
    bypass_override: bool = False

    @property
    def auto_eligible(self) -> bool:
        return self.status is AlbumStatus.UNPROCESSED


@dataclass(frozen=True)
class ArtistItem:
    path: str
    name: str
    status: ArtistStatus | None
    album_count: int
    selected_count: int
    indexed: bool = False
    loaded: bool = False


def artist_status(albums: Iterable[AlbumItem]) -> ArtistStatus:
    """Apply the locked Blue > Green > Purple > White precedence."""
    items = list(albums)
    if any(item.status is AlbumStatus.BYPASSED for item in items):
        return ArtistStatus.CONTAINS_BYPASS
    protected = {AlbumStatus.PROCESSED, AlbumStatus.TIMEOUT}
    # Match Windows LibraryInventory.Load: timeout-active albums are retained,
    # protected history entries, so an artist with no remaining unprocessed
    # album is complete. A timeout mixed with unprocessed work is partial.
    if items and all(item.status in protected for item in items):
        return ArtistStatus.COMPLETE
    if any(item.status in protected for item in items):
        return ArtistStatus.PARTIAL
    return ArtistStatus.UNPROCESSED


@dataclass
class LibraryModel:
    root: str
    albums: list[AlbumItem]
    artists: list[ArtistItem] = field(default_factory=list)
    artist_filter: str = ""
    album_filter: str = ""
    status_filters: set[AlbumStatus] = field(
        default_factory=lambda: set(AlbumStatus)
    )
    artist_status_filters: set[ArtistStatus] = field(
        default_factory=lambda: set(ArtistStatus)
    )
    active_artist: str = ""
    inventory_loads: int = 1
    select_new: bool = False
    picker_index: str = ""
    _album_by_path: dict[str, AlbumItem] = field(
        default_factory=dict,
        init=False,
        repr=False,
    )

    def __post_init__(self) -> None:
        self._album_by_path = {item.path: item for item in self.albums}
        if not self.artists:
            grouped = self._artist_groups()
            self.artists = [
                ArtistItem(
                    name,
                    name,
                    artist_status(children),
                    len(children),
                    sum(child.selected for child in children),
                    True,
                    True,
                )
                for name, children in sorted(
                    grouped.items(), key=lambda pair: pair[0].casefold()
                )
            ]
            if self.artists and not self.active_artist:
                self.active_artist = self.artists[0].name

    @classmethod
    def from_payload(cls, payload: dict[str, Any]) -> "LibraryModel":
        items: list[AlbumItem] = []
        for raw in payload.get("albums", []):
            if not isinstance(raw, dict):
                continue
            try:
                status = AlbumStatus(str(raw.get("status", "unprocessed")))
            except ValueError:
                status = AlbumStatus.UNPROCESSED
            item = AlbumItem(
                path=str(raw.get("path", "")),
                artist=str(raw.get("artist", "Unknown Artist")) or "Unknown Artist",
                title=str(raw.get("album", "Unknown Album")) or "Unknown Album",
                status=status,
                formats=tuple(str(value).upper() for value in raw.get("formats", [])),
                timeout_remaining=str(raw.get("timeout_remaining", "")),
                selected=bool(raw.get("selected", False)),
                bypass_override=bool(raw.get("bypass_override", False)),
            )
            # Protected states are never selected merely because malformed
            # presentation payload claimed they were.
            if not item.auto_eligible and not item.bypass_override:
                item.selected = False
            items.append(item)
        grouped: dict[str, list[AlbumItem]] = {}
        for item in items:
            grouped.setdefault(item.artist, []).append(item)
        artists: list[ArtistItem] = []
        for raw in payload.get("artists", []):
            if not isinstance(raw, dict):
                continue
            name = str(raw.get("name", "Unknown Artist")) or "Unknown Artist"
            children = grouped.get(name, [])
            indexed = bool(raw.get("indexed", True))
            artists.append(
                ArtistItem(
                    str(raw.get("path", "")),
                    name,
                    artist_status(children) if children else None,
                    int(raw.get("album_count", len(children))),
                    sum(child.selected for child in children),
                    indexed,
                    bool(raw.get("loaded", True)),
                )
            )
        if not artists:
            artists = [
                ArtistItem(
                    name,
                    name,
                    artist_status(children),
                    len(children),
                    sum(child.selected for child in children),
                    True,
                    True,
                )
                for name, children in sorted(grouped.items(), key=lambda pair: pair[0].casefold())
            ]
        model = cls(
            str(payload.get("root", "")),
            items,
            artists,
            select_new=bool(payload.get("select_new", False)),
            picker_index=str(payload.get("picker_index", "")),
        )
        artists = model.visible_artists()
        if artists:
            model.active_artist = artists[0].name
        return model

    def merge_payload(
        self,
        payload: dict[str, Any],
        *,
        preserve_selection: bool = False,
    ) -> None:
        replacement = self.from_payload(payload)
        replacement.artist_filter = self.artist_filter
        replacement.album_filter = self.album_filter
        replacement.status_filters = set(self.status_filters)
        replacement.artist_status_filters = set(self.artist_status_filters)
        replacement.select_new = self.select_new
        if preserve_selection:
            existing = {
                item.path: (item.selected, item.bypass_override)
                for item in self.albums
            }
            for item in replacement.albums:
                if item.path in existing:
                    item.selected, item.bypass_override = existing[item.path]
        names = {item.name for item in replacement.artists}
        replacement.active_artist = (
            self.active_artist if self.active_artist in names else replacement.active_artist
        )
        self.root = replacement.root
        self.albums = replacement.albums
        self.artists = replacement.artists
        self.artist_filter = replacement.artist_filter
        self.album_filter = replacement.album_filter
        self.status_filters = replacement.status_filters
        self.artist_status_filters = replacement.artist_status_filters
        self.active_artist = replacement.active_artist
        self.select_new = replacement.select_new
        self.picker_index = replacement.picker_index
        self._album_by_path = replacement._album_by_path

    def _text_matches(self, item: AlbumItem) -> bool:
        return (
            self.artist_filter.casefold() in item.artist.casefold()
            and self.album_filter.casefold() in item.title.casefold()
        )

    def visible_albums(self, *, active_artist_only: bool = False) -> list[AlbumItem]:
        return [
            item
            for item in self.albums
            if item.status in self.status_filters
            and self._text_matches(item)
            and (not active_artist_only or not self.active_artist or item.artist == self.active_artist)
        ]

    def active_albums(self) -> list[AlbumItem]:
        """Return the known Album topology for the active Artist only."""
        if not self.active_artist:
            return []
        return [item for item in self.albums if item.artist == self.active_artist]

    def active_status_counts(self) -> dict[AlbumStatus, int]:
        items = self.active_albums()
        return {
            status: sum(item.status is status for item in items)
            for status in AlbumStatus
        }

    def album_status_counts(self) -> dict[AlbumStatus, int]:
        """Return authoritative counts across the complete picker snapshot."""
        return {
            status: sum(item.status is status for item in self.albums)
            for status in AlbumStatus
        }

    def indexed_artist_status_counts(self) -> dict[ArtistStatus, int]:
        groups = self._artist_groups()
        counts = {status: 0 for status in ArtistStatus}
        for item in self.artists:
            children = groups.get(item.name, [])
            if not children:
                continue
            counts[artist_status(children)] += 1
        return counts

    def _artist_groups(self) -> dict[str, list[AlbumItem]]:
        groups: dict[str, list[AlbumItem]] = {}
        for item in self.albums:
            groups.setdefault(item.artist, []).append(item)
        return groups

    def visible_artists(self) -> list[ArtistItem]:
        groups = self._artist_groups()
        rows: list[ArtistItem] = []
        for base in sorted(self.artists, key=lambda item: item.name.casefold()):
            if self.artist_filter.casefold() not in base.name.casefold():
                continue
            children = groups.get(base.name, [])
            aggregate = artist_status(children) if children else None
            if aggregate is not None and aggregate not in self.artist_status_filters:
                continue
            rows.append(
                ArtistItem(
                    path=base.path,
                    name=base.name,
                    status=aggregate,
                    album_count=len(children),
                    selected_count=sum(child.selected for child in children),
                    indexed=True,
                    loaded=True,
                )
            )
        return rows

    def set_filters(self, *, artist: str | None = None, album: str | None = None) -> None:
        if artist is not None:
            self.artist_filter = artist
        if album is not None:
            self.album_filter = album
        visible = self.visible_artists()
        if visible and self.active_artist not in {row.name for row in visible}:
            self.active_artist = visible[0].name

    def toggle_status(self, status: AlbumStatus) -> None:
        if status in self.status_filters:
            self.status_filters.remove(status)
        else:
            self.status_filters.add(status)

    def toggle_artist_status(self, status: ArtistStatus) -> None:
        if status in self.artist_status_filters:
            self.artist_status_filters.remove(status)
        else:
            self.artist_status_filters.add(status)

    def select_none(self) -> None:
        self.select_new = False
        for item in self.albums:
            item.selected = False
            item.bypass_override = False

    def select_all(self, *, filtered: bool = False) -> None:
        if not filtered:
            self.select_new = True
        scope = (
            self.visible_albums()
            if filtered
            else self.albums
        )
        paths = {item.path for item in scope}
        for item in self.albums:
            if item.path in paths and item.auto_eligible:
                item.selected = True

    def toggle_artist(self, name: str) -> None:
        eligible = [
            item for item in self.albums if item.artist == name and item.auto_eligible
        ]
        select = any(not item.selected for item in eligible)
        for item in eligible:
            item.selected = select

    def artist(self, name: str) -> ArtistItem | None:
        return next((item for item in self.artists if item.name == name), None)

    def selection_state(self) -> dict[str, Any]:
        return {
            "selected": [item.path for item in self.albums if item.selected],
            "bypass_overrides": [
                item.path for item in self.albums if item.bypass_override
            ],
            "select_new": self.select_new,
        }

    def toggle_album(
        self,
        item: AlbumItem,
        *,
        bypass_override: bool = False,
        timeout_override: bool = False,
    ) -> str:
        """Toggle one album and report whether an explicit override is needed."""
        if item.status is AlbumStatus.BYPASSED and not (bypass_override or item.bypass_override):
            return "bypass-confirmation-required"
        if item.status is AlbumStatus.TIMEOUT and not timeout_override:
            return "timeout-active"
        item.selected = not item.selected
        if item.status is AlbumStatus.BYPASSED:
            item.bypass_override = item.selected
        return "selected" if item.selected else "cleared"

    def selection_payload(self, scan_mode: str) -> dict[str, Any]:
        if scan_mode in {"filtered-read", "filtered-write"}:
            # Windows AUTO LAUNCH is path-exact: visibility narrows the checked
            # set, but ordinary eligibility never checks a row implicitly.
            visible = {item.path for item in self.visible_albums()}
            selected = [
                item.path
                for item in self.albums
                if item.path in visible and item.selected
            ]
        elif scan_mode == "auto-all":
            selected = [
                item.path
                for item in self.albums
                if item.auto_eligible or item.selected
            ]
        else:
            selected = [item.path for item in self.albums if item.selected]
        return {
            "action": "launch",
            "scan_mode": scan_mode,
            "selected": selected,
            "bypass_overrides": [
                item.path
                for item in self.albums
                if item.path in selected and item.bypass_override
            ],
        }

    def statistics(self) -> dict[str, Any]:
        formats: dict[str, int] = {}
        for item in self.albums:
            for value in item.formats:
                formats[value] = formats.get(value, 0) + 1
        active = self.active_albums()
        active_visible = self.visible_albums(active_artist_only=True)
        return {
            "path": self.root,
            "artists": len(self.artists),
            "albums": len(self.albums),
            "visible_artists": len(self.visible_artists()),
            "active_albums": len(active),
            "active_visible_albums": len(active_visible),
            "selected": sum(item.selected for item in self.albums),
            "formats": formats,
            "cache": "COMPLETE",
        }

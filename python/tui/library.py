"""In-memory Select Media model for the Ratatui workspace.

The engine supplies authoritative album states after its single inventory and
history load.  This module only filters and manages transient selections; it
never reads the filesystem or creates a parallel history database.
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
    tagged: bool = False
    album_mbid: str = ""

    @property
    def auto_eligible(self) -> bool:
        return self.status is AlbumStatus.UNPROCESSED


@dataclass(frozen=True)
class ArtistItem:
    name: str
    status: ArtistStatus
    album_count: int
    selected_count: int


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
    _album_by_path: dict[str, AlbumItem] = field(
        default_factory=dict,
        init=False,
        repr=False,
    )

    def __post_init__(self) -> None:
        self._album_by_path = {item.path: item for item in self.albums}

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
                selected=bool(raw.get("selected", status is AlbumStatus.UNPROCESSED)),
                bypass_override=bool(raw.get("bypass_override", False)),
                tagged=bool(raw.get("tagged", False)),
                album_mbid=str(raw.get("album_mbid", "")),
            )
            # Protected states are never selected merely because malformed
            # presentation payload claimed they were.
            if not item.auto_eligible and not item.bypass_override:
                item.selected = False
            items.append(item)
        model = cls(str(payload.get("root", "")), items)
        artists = model.visible_artists()
        if artists:
            model.active_artist = artists[0].name
        return model

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

    def _artist_groups(self) -> dict[str, list[AlbumItem]]:
        groups: dict[str, list[AlbumItem]] = {}
        for item in self.albums:
            groups.setdefault(item.artist, []).append(item)
        return groups

    def visible_artists(self) -> list[ArtistItem]:
        groups = self._artist_groups()
        visible_paths = {item.path for item in self.visible_albums()}
        rows: list[ArtistItem] = []
        for name in sorted(groups, key=str.casefold):
            children = groups[name]
            aggregate = artist_status(children)
            if aggregate not in self.artist_status_filters:
                continue
            if not any(child.path in visible_paths for child in children):
                continue
            rows.append(
                ArtistItem(
                    name=name,
                    status=aggregate,
                    album_count=len(children),
                    selected_count=sum(child.selected for child in children),
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

    def apply_tag_enrichment(
        self,
        path: str,
        *,
        artist: str = "",
        album: str = "",
        album_mbid: str = "",
    ) -> bool:
        item = self._album_by_path.get(path)
        if item is None:
            return False
        previous_artist = item.artist
        if artist.strip():
            item.artist = artist.strip()
        if album.strip():
            item.title = album.strip()
        item.album_mbid = album_mbid.strip()
        item.tagged = True
        if self.active_artist == previous_artist and item.artist != previous_artist:
            self.active_artist = item.artist
        return True

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
        for item in self.albums:
            item.selected = False
            item.bypass_override = False

    def select_all(self, *, filtered: bool = False) -> None:
        scope = self.visible_albums() if filtered else self.albums
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
            visible = {item.path for item in self.visible_albums()}
            selected = [
                item.path
                for item in self.albums
                if item.path in visible and (item.auto_eligible or item.selected)
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
        return {
            "path": self.root,
            "artists": len(self._artist_groups()),
            "albums": len(self.albums),
            "visible_artists": len(self.visible_artists()),
            "visible_albums": len(self.visible_albums()),
            "selected": sum(item.selected for item in self.albums),
            "tagged": sum(item.tagged for item in self.albums),
            "musicbrainz": sum(bool(item.album_mbid) for item in self.albums),
            "formats": formats,
        }

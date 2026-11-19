"""Field ownership for catalogs synchronized from authoritative Windows SQLite."""

from __future__ import annotations

from typing import Any


WINDOWS_MANAGED_TABLES = frozenset({"artists", "albums", "tracks"})
TABLE_KEYS = {
    "artists": "artist_key",
    "albums": "album_key",
    "tracks": "track_key",
}

# These fields are shared with Windows schema v2 and are written only by
# reconciliation once an entity has established Windows ownership.
WINDOWS_SYNCHRONIZED_FIELDS = {
    "artists": frozenset(
        {
            "artist_key",
            "artist_name",
            "artist_sort",
            "musicbrainz_artistid",
            "primary_path",
            "created_at",
            "updated_at",
            "last_seen_at",
            "splined_version",
        }
    ),
    "albums": frozenset(
        {
            "album_key",
            "artist_key",
            "album_name",
            "album_sort",
            "musicbrainz_albumid",
            "musicbrainz_releasegroupid",
            "release_year",
            "compilation",
            "path",
            "representative_file",
            "representative_size",
            "representative_mtime_ns",
            "tag_signature",
            "track_count",
            "inventory_fingerprint",
            "cover_required",
            "cover_found",
            "cover_path",
            "cover_name",
            "cover_format",
            "cover_width",
            "cover_height",
            "artwork_jpeg",
            "artwork_png",
            "artwork_webp",
            "artwork_other",
            "root_files",
            "cover_files",
            "cover_names_json",
            "local_art_json",
            "other_filenames_json",
            "webp_found",
            "webp_size_mb",
            "webp_resolution",
            "webp_conversion",
            "processed_at",
            "bypassed",
            "timeout_until",
            "selected_source",
            "created_at",
            "updated_at",
            "last_seen_at",
            "splined_version",
        }
    ),
    "tracks": frozenset(
        {
            "track_key",
            "album_key",
            "path",
            "title",
            "artist_name",
            "musicbrainz_recordingid",
            "musicbrainz_artistid",
            "file_size",
            "file_mtime_ns",
            "updated_at",
            "splined_version",
        }
    ),
}

# These remain writable by WebGUI even when the synchronized entity is owned
# by Windows. Artist/Album status is derived from authoritative processing
# state; embedded validation has no Windows schema-v2 counterpart.
DERIVED_PRESENTATION_FIELDS = {
    "artists": frozenset(
        {
            "status",
            "album_count",
            "unprocessed_count",
            "processed_count",
            "bypassed_count",
            "timeout_count",
        }
    ),
    "albums": frozenset({"status"}),
    "tracks": frozenset(),
}
POSTGRESQL_EXCLUSIVE_FIELDS = {
    "artists": frozenset(),
    "albums": frozenset(
        {
            "embedded_tracks",
            "embedded_validation_status",
            "embedded_validated_at",
        }
    ),
    "tracks": frozenset(),
}


def mark_windows_authority(
    raw: Any,
    table: str,
    entity_key: Any,
    source_fingerprint: str,
    source_label: str,
) -> None:
    """Record a proven Windows association without modifying catalog values."""
    if table not in WINDOWS_MANAGED_TABLES or not str(entity_key or ""):
        return
    raw.execute(
        "INSERT INTO catalog_sync_authority(entity_table,entity_key,authority,"
        "source_fingerprint,source_label) VALUES(%s,%s,'windows',%s,%s) "
        "ON CONFLICT(entity_table,entity_key) DO UPDATE SET authority='windows',"
        "source_fingerprint=excluded.source_fingerprint,"
        "source_label=excluded.source_label,last_confirmed_at=now()",
        (table, str(entity_key), source_fingerprint, source_label),
    )


def windows_managed(raw: Any, table: str, entity_key: Any) -> bool:
    if table not in WINDOWS_MANAGED_TABLES or not str(entity_key or ""):
        return False
    return bool(
        raw.execute(
            "SELECT 1 FROM catalog_sync_authority WHERE entity_table=%s "
            "AND entity_key=%s AND authority='windows'",
            (table, str(entity_key)),
        ).fetchone()
    )


def windows_managed_keys(raw: Any, table: str) -> set[str]:
    if table not in WINDOWS_MANAGED_TABLES:
        return set()
    return {
        str(row["entity_key"])
        for row in raw.execute(
            "SELECT entity_key FROM catalog_sync_authority "
            "WHERE entity_table=%s AND authority='windows'",
            (table,),
        )
    }


def windows_managed_track_keys(raw: Any, album_key: Any) -> set[str]:
    """Return only managed tracks belonging to one indexed Album."""
    return {
        str(row["entity_key"])
        for row in raw.execute(
            "SELECT authority.entity_key FROM catalog_sync_authority authority "
            "JOIN tracks ON tracks.track_key=authority.entity_key "
            "WHERE authority.entity_table='tracks' AND authority.authority='windows' "
            "AND tracks.album_key=%s",
            (str(album_key),),
        )
    }


def index_update_columns(table: str, row: dict[str, Any], managed: bool) -> list[str]:
    """Return columns an indexing UPSERT may update for this entity."""
    key = TABLE_KEYS[table]
    if not managed:
        return [name for name in row if name != key]
    protected = WINDOWS_SYNCHRONIZED_FIELDS[table]
    permitted = DERIVED_PRESENTATION_FIELDS[table] | POSTGRESQL_EXCLUSIVE_FIELDS[table]
    return [name for name in row if name != key and name not in protected and name in permitted]

"""Client-specific adaptive batching for the PostgreSQL catalog indexer.

The controller is created only by an explicit index operation. Decisions are
made after durable PostgreSQL checkpoints, so startup catalog reads never wait
for calibration or filesystem work.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
import json


DEFAULT_BATCH_SIZE = 16
MIN_BATCH_SIZE = 1
MAX_BATCH_SIZE = 64
TARGET_PAYLOAD_BYTES = 16 * 1024 * 1024
TARGET_CHECKPOINT_SECONDS = 0.250


@dataclass(frozen=True)
class CatalogWorkload:
    artists: int = 0
    albums: int = 0
    tracks: int = 0
    average_tracks_per_album: float = 0.0
    maximum_tracks_per_album: int = 0


@dataclass(frozen=True)
class BatchDecision:
    checkpoint: int
    previous_size: int
    next_size: int
    reason: str
    albums: int
    tracks: int
    payload_bytes: int
    processing_seconds: float
    checkpoint_seconds: float
    interactive_pressure: bool


class AdaptiveBatchController:
    def __init__(
        self,
        *,
        enabled: bool,
        configured_size: int,
        workload: CatalogWorkload,
        prior_size: int | None = None,
    ) -> None:
        self.enabled = bool(enabled)
        self.configured_size = _bounded(configured_size)
        self.workload = workload
        self.initial_size = (
            _bounded(prior_size or DEFAULT_BATCH_SIZE)
            if self.enabled
            else self.configured_size
        )
        self.current_size = self.initial_size
        self.checkpoints = 0
        self.observed_albums = 0
        self.stable_checkpoints = 0
        self.peak_payload_bytes = 0
        self.decisions: list[BatchDecision] = []

    @classmethod
    def from_database(cls, connection, *, enabled: bool, configured_size: int):
        workload = read_workload(connection)
        return cls(
            enabled=enabled,
            configured_size=configured_size,
            workload=workload,
            prior_size=prior_adaptive_size(connection, workload),
        )

    def observe_checkpoint(
        self,
        *,
        albums: int,
        tracks: int,
        payload_bytes: int,
        processing_seconds: float,
        checkpoint_seconds: float,
        interactive_pressure: bool,
    ) -> None:
        self.checkpoints += 1
        self.observed_albums += albums
        self.peak_payload_bytes = max(self.peak_payload_bytes, payload_bytes)
        if not self.enabled or albums <= 0:
            return

        previous = self.current_size
        average_tracks = tracks / albums
        bytes_per_album = payload_bytes // albums
        processing_per_album = processing_seconds / albums
        severe_memory_pressure = (
            payload_bytes > TARGET_PAYLOAD_BYTES or bytes_per_album > 1024 * 1024
        )
        heavy_album = average_tracks >= 128 or processing_per_album >= 0.5
        slow_checkpoint = checkpoint_seconds > TARGET_CHECKPOINT_SECONDS * 4

        if interactive_pressure:
            candidate, reason, immediate = lower_step(previous), "interactive activity", True
        elif severe_memory_pressure:
            candidate, reason, immediate = lower_step(previous), "batch memory pressure", True
        elif slow_checkpoint:
            candidate, reason, immediate = lower_step(previous), "PostgreSQL checkpoint latency", True
        elif heavy_album:
            candidate, reason, immediate = lower_step(previous), "heavy per-Album workload", False
        elif (
            (self.workload.albums >= 512 or self.observed_albums >= 512)
            and checkpoint_seconds < TARGET_CHECKPOINT_SECONDS / 2
            and payload_bytes < TARGET_PAYLOAD_BYTES / 2
            and average_tracks < 64
            and processing_per_album < 0.3
        ):
            candidate, reason, immediate = higher_step(previous), "low-cost checkpoint", False
        else:
            candidate, reason, immediate = previous, "within target", False

        if candidate == previous:
            self.stable_checkpoints = 0
            return
        self.stable_checkpoints += 1
        if not immediate and self.stable_checkpoints < 2:
            return
        self.stable_checkpoints = 0
        self.current_size = candidate
        self.decisions.append(
            BatchDecision(
                checkpoint=self.checkpoints,
                previous_size=previous,
                next_size=candidate,
                reason=reason,
                albums=albums,
                tracks=tracks,
                payload_bytes=payload_bytes,
                processing_seconds=processing_seconds,
                checkpoint_seconds=checkpoint_seconds,
                interactive_pressure=interactive_pressure,
            )
        )

    def should_checkpoint(self, albums: int, payload_bytes: int) -> bool:
        return albums >= self.current_size or (
            self.enabled and payload_bytes >= TARGET_PAYLOAD_BYTES
        )

    def summary(self) -> dict[str, object]:
        return {
            "enabled": self.enabled,
            "configured_size": self.configured_size,
            "initial_size": self.initial_size,
            "final_size": self.current_size,
            "minimum_size": MIN_BATCH_SIZE,
            "maximum_size": MAX_BATCH_SIZE,
            "checkpoints_observed": self.checkpoints,
            "peak_batch_payload_bytes": self.peak_payload_bytes,
            "catalog_artists": self.workload.artists,
            "catalog_albums": self.workload.albums,
            "catalog_tracks": self.workload.tracks,
            "average_tracks_per_album": self.workload.average_tracks_per_album,
            "maximum_tracks_per_album": self.workload.maximum_tracks_per_album,
            "decisions": [asdict(decision) for decision in self.decisions],
        }


def read_workload(connection) -> CatalogWorkload:
    row = connection.raw.execute(
        "SELECT (SELECT count(*) FROM artists) AS artists,"
        "(SELECT count(*) FROM albums) AS albums,"
        "(SELECT count(*) FROM tracks) AS tracks,"
        "COALESCE((SELECT avg(track_count) FROM albums),0.0) AS average_tracks,"
        "COALESCE((SELECT max(track_count) FROM albums),0) AS maximum_tracks"
    ).fetchone()
    return CatalogWorkload(
        artists=max(0, int(row["artists"] or 0)),
        albums=max(0, int(row["albums"] or 0)),
        tracks=max(0, int(row["tracks"] or 0)),
        average_tracks_per_album=float(row["average_tracks"] or 0.0),
        maximum_tracks_per_album=max(0, int(row["maximum_tracks"] or 0)),
    )


def prior_adaptive_size(connection, workload: CatalogWorkload) -> int | None:
    row = connection.raw.execute(
        "SELECT timings FROM catalog_generations "
        "WHERE backend='postgresql' AND state='published' "
        "ORDER BY published_at DESC NULLS LAST LIMIT 1"
    ).fetchone()
    if row is None:
        return None
    timings = dict(row["timings"] or {})
    adaptive = dict(timings.get("adaptive_batching") or {})
    if not adaptive.get("enabled"):
        return None
    prior_albums = int(adaptive.get("catalog_albums") or timings.get("albums") or 0)
    if prior_albums <= 0 or workload.albums <= 0:
        return None
    ratio = workload.albums / prior_albums
    if not 0.5 <= ratio <= 2.0:
        return None
    value = int(adaptive.get("final_size") or 0)
    return value if MIN_BATCH_SIZE <= value <= MAX_BATCH_SIZE else None


def checkpoint_payload_bytes(rows) -> int:
    return sum(
        len(json.dumps(row, default=str, separators=(",", ":")).encode("utf-8"))
        for row in rows
    )


def competing_interactive_activity(connection) -> bool:
    row = connection.raw.execute(
        "SELECT EXISTS(SELECT 1 FROM web_jobs "
        "WHERE kind<>'index' AND state IN ('queued','running','waiting')) AS active"
    ).fetchone()
    return bool(row and row["active"])


def _bounded(value: int) -> int:
    return max(MIN_BATCH_SIZE, min(MAX_BATCH_SIZE, int(value)))


def lower_step(value: int) -> int:
    if value <= 1:
        return 1
    if value <= 2:
        return 1
    if value <= 4:
        return 2
    if value <= 8:
        return 4
    if value <= 16:
        return 8
    if value <= 32:
        return 16
    return 32


def higher_step(value: int) -> int:
    if value <= 1:
        return 2
    if value <= 2:
        return 4
    if value <= 4:
        return 8
    if value <= 8:
        return 16
    if value <= 16:
        return 32
    return 64

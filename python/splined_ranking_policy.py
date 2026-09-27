"""Shared candidate ranking policy installed by the operational entrypoint.

The projected final image remains the primary authority.  When two candidates
project to the same acceptable result, SPLINED prefers the stronger original
source material before configured provider order.  A benign downscale is not a
quality penalty; upscaling, cropping, and format conversion remain penalties.
"""

from __future__ import annotations

from typing import Any


_RANGE_RANK = {
    "Ideal": 0,
    "UpperRange": 1,
    "LowerRange": 2,
    "Ladder": 3,
    "BelowMinimum": 4,
    "AboveLadder": 5,
}
_POLICY_RANK = {"accept": 0, "fallback": 1, "reject": 2}


def _source_resolution_key(candidate: Any) -> tuple[int, int]:
    width = max(0, int(candidate.width))
    height = max(0, int(candidate.height))
    return (-min(width, height), -(width * height))


def _format_rank(projected: dict[str, Any], format_order: list[str]) -> int:
    value = str(projected["format"])
    return format_order.index(value) if value in format_order else 999999


def operational_candidate_key(
    scan: Any,
    candidate: Any,
    cfg: dict[str, Any],
    format_order: list[str],
) -> tuple[Any, ...]:
    projected = scan.project_candidate(candidate, cfg, format_order)
    # Downscaling a slightly larger source to the same target is benign.  The
    # old generic `resized` penalty made a 3000px iTunes image beat a 3024px
    # Cover Art Archive image even though both finished at the same 3000px
    # Ideal target.
    transform_penalty = (
        1 if projected["upscaled"] else 0,
        1 if projected["cropped"] else 0,
        1 if projected["converted"] else 0,
    )
    return (
        _POLICY_RANK.get(str(projected["policy_status"]), 2),
        int(projected["distance"]),
        _RANGE_RANK.get(str(projected["range_type"]), 99),
        transform_penalty,
        *_source_resolution_key(candidate),
        int(candidate.source_priority),
        _format_rank(projected, format_order),
        str(scan.provider_label(candidate.source)).casefold(),
        str(candidate.ref.id),
    )


def operational_fallback_sort_key(
    scan: Any,
    candidate: Any,
    cfg: dict[str, Any],
    format_order: list[str],
) -> tuple[Any, ...]:
    projected = scan.project_candidate(candidate, cfg, format_order)
    return (
        -int(projected["short_side"]),
        0 if projected["square"] else 1,
        0 if candidate.ref.approved else 1,
        *_source_resolution_key(candidate),
        int(candidate.source_priority),
        _format_rank(projected, format_order),
        str(candidate.ref.id),
    )


def operational_fallback_suggested(
    scan: Any,
    candidates: list[Any],
    cfg: dict[str, Any],
    format_order: list[str],
) -> Any | None:
    if not candidates:
        return None

    def key(candidate: Any) -> tuple[Any, ...]:
        projected = scan.project_candidate(candidate, cfg, format_order)
        return (
            int(projected["distance"]),
            0 if projected["square"] else 1,
            0 if candidate.ref.approved else 1,
            *_source_resolution_key(candidate),
            int(candidate.source_priority),
            _format_rank(projected, format_order),
            str(candidate.ref.id),
        )

    return min(candidates, key=key)


def core_candidate_key(
    core: Any,
    candidate: Any,
    cfg: dict[str, Any],
    format_order: list[str],
) -> tuple[Any, ...]:
    projected = core.project_candidate(
        candidate,
        cfg,
        core.target_format_for_candidate(candidate, format_order),
    )
    transform_penalty = (
        1 if projected["upscaled"] else 0,
        1 if projected["cropped"] else 0,
        1 if projected["converted"] else 0,
    )
    return (
        _POLICY_RANK.get(str(projected["policy_status"]), 2),
        int(projected["distance"]),
        _RANGE_RANK.get(str(projected["range_type"]), 99),
        transform_penalty,
        *_source_resolution_key(candidate),
        int(candidate.source_priority),
        _format_rank(projected, format_order),
        str(core.provider_label(candidate.source)).casefold(),
        str(candidate.ref.id),
    )


def install(core: Any, scan: Any) -> None:
    """Install one ranking policy across core and operational scan surfaces."""

    def scan_candidate_key(
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> tuple[Any, ...]:
        return operational_candidate_key(scan, candidate, cfg, format_order)

    def scan_fallback_sort_key(
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> tuple[Any, ...]:
        return operational_fallback_sort_key(scan, candidate, cfg, format_order)

    def scan_fallback_suggested(
        candidates: list[Any],
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> Any | None:
        return operational_fallback_suggested(
            scan,
            candidates,
            cfg,
            format_order,
        )

    def shared_core_candidate_key(
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> tuple[Any, ...]:
        return core_candidate_key(core, candidate, cfg, format_order)

    scan.candidate_key = scan_candidate_key
    scan.fallback_sort_key = scan_fallback_sort_key
    scan.fallback_suggested = scan_fallback_suggested
    core.candidate_key = shared_core_candidate_key

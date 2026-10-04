"""Content-evidence gate for sources that may return non-cover product images.

Strict policy deliberately does not hide candidates.  It annotates them so
interactive selection can still expose every downloaded image while Preferred
and unattended Auto selection can exclude content that has not been matched to
authoritative, exact-release front artwork.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from PIL import Image, ImageOps


_FRONT_TYPES = {"front", "front cover"}
_NON_FRONT_TYPES = {
    "back",
    "booklet",
    "medium",
    "disc",
    "tray",
    "obi",
    "spine",
    "track",
}
_LOCAL_SOURCES = {"local", "local-library", "embedded", "webpstill"}
_EXACT_RELEASE_SOURCES = {"coverartarchive"}
_SIGNATURE_CACHE: dict[tuple[str, int, int], "VisualSignature"] = {}


@dataclass(frozen=True)
class VisualSignature:
    width: int
    height: int
    dhash: int
    ahash: int


def _bits(comparisons: list[bool]) -> int:
    result = 0
    for enabled in comparisons:
        result = (result << 1) | int(enabled)
    return result


def image_signature(path: Path) -> VisualSignature:
    """Return a small, deterministic full-frame perceptual signature."""
    stat = path.stat()
    cache_key = (str(path.resolve()), int(stat.st_size), int(stat.st_mtime_ns))
    cached = _SIGNATURE_CACHE.get(cache_key)
    if cached is not None:
        return cached
    with Image.open(path) as opened:
        image = ImageOps.exif_transpose(opened).convert("L")
        width, height = image.size
        dhash_image = image.resize((9, 8), Image.Resampling.LANCZOS)
        pixels = list(dhash_image.getdata())
        comparisons = [
            pixels[(row * 9) + column] > pixels[(row * 9) + column + 1]
            for row in range(8)
            for column in range(8)
        ]
        average_image = image.resize((8, 8), Image.Resampling.LANCZOS)
        average_pixels = list(average_image.getdata())
        average = sum(average_pixels) / max(1, len(average_pixels))
        average_comparisons = [value >= average for value in average_pixels]
    signature = VisualSignature(
        width=width,
        height=height,
        dhash=_bits(comparisons),
        ahash=_bits(average_comparisons),
    )
    if len(_SIGNATURE_CACHE) >= 2048:
        _SIGNATURE_CACHE.clear()
    _SIGNATURE_CACHE[cache_key] = signature
    return signature


def _hamming(left: int, right: int) -> int:
    return (left ^ right).bit_count()


def visual_distance(left: VisualSignature, right: VisualSignature) -> tuple[int, int]:
    return _hamming(left.dhash, right.dhash), _hamming(left.ahash, right.ahash)


def full_frame_match(left: VisualSignature, right: VisualSignature) -> tuple[bool, int, int]:
    """Match the same complete artwork despite ordinary scan/encode variance.

    Aspect-ratio agreement is intentionally mandatory.  A photograph of a CD
    case on a table can contain the correct pixels, but it is not the complete
    front image and therefore must not pass the strict Preferred/Auto gate.
    """
    left_ratio = left.width / max(1, left.height)
    right_ratio = right.width / max(1, right.height)
    ratio_delta = abs(left_ratio - right_ratio) / max(left_ratio, right_ratio)
    dhash_distance, ahash_distance = visual_distance(left, right)
    matched = ratio_delta <= 0.06 and (
        dhash_distance <= 10
        or (dhash_distance <= 16 and ahash_distance <= 9)
    )
    return matched, dhash_distance, ahash_distance


def _types(candidate: Any) -> set[str]:
    return {
        str(value).strip().casefold()
        for value in getattr(candidate.ref, "types", [])
        if str(value).strip()
    }


def _is_front(candidate: Any) -> bool:
    types = _types(candidate)
    return bool(getattr(candidate.ref, "front", False)) or bool(types & _FRONT_TYPES)


def _is_non_front(candidate: Any) -> bool:
    types = _types(candidate)
    return not _is_front(candidate) and bool(types & _NON_FRONT_TYPES)


def _best_match(
    signature: VisualSignature,
    references: list[tuple[Any, VisualSignature]],
) -> tuple[Any | None, int | None, int | None]:
    best: tuple[Any | None, int | None, int | None] = (None, None, None)
    for candidate, reference in references:
        matched, dhash_distance, ahash_distance = full_frame_match(signature, reference)
        if not matched:
            continue
        if best[0] is None or (dhash_distance, ahash_distance) < (
            int(best[1]),
            int(best[2]),
        ):
            best = candidate, dhash_distance, ahash_distance
    return best


def preferred_eligible(candidate: Any) -> bool:
    return bool(getattr(candidate, "strict_preferred_eligible", True))


def auto_eligible(candidate: Any) -> bool:
    return bool(getattr(candidate, "strict_auto_eligible", True))


def apply(
    candidates: list[Any],
    cfg: dict[str, Any],
    policy_for: Callable[[dict[str, Any], str], dict[str, Any]],
    *,
    debug: Callable[[str], None] | None = None,
    emit: Callable[..., None] | None = None,
) -> None:
    """Annotate a complete candidate pool with strict-content decisions."""
    for candidate in candidates:
        candidate.strict_status = "not-applicable"
        candidate.strict_reason = ""
        candidate.strict_preferred_eligible = True
        candidate.strict_auto_eligible = True
        candidate.strict_match_distance = None
        candidate.strict_match_source = ""
        candidate.strict_local_validated = False

    strict_candidates = [
        candidate
        for candidate in candidates
        if bool(policy_for(cfg, str(candidate.source))["strict_override"])
    ]
    if not strict_candidates:
        return

    signatures: dict[int, VisualSignature] = {}
    for candidate in candidates:
        try:
            signatures[id(candidate)] = image_signature(Path(candidate.path))
        except Exception as exc:
            if candidate in strict_candidates:
                candidate.strict_status = "analysis-error"
                candidate.strict_reason = f"content analysis failed: {exc}"
                candidate.strict_preferred_eligible = False
                candidate.strict_auto_eligible = False

    exact_front = [
        (candidate, signatures[id(candidate)])
        for candidate in candidates
        if id(candidate) in signatures
        and str(candidate.source).casefold() in _EXACT_RELEASE_SOURCES
        and _is_front(candidate)
    ]
    exact_non_front = [
        (candidate, signatures[id(candidate)])
        for candidate in candidates
        if id(candidate) in signatures
        and str(candidate.source).casefold() in _EXACT_RELEASE_SOURCES
        and _is_non_front(candidate)
    ]
    flexible_front = [
        (candidate, signatures[id(candidate)])
        for candidate in candidates
        if id(candidate) in signatures
        and str(candidate.source).casefold() not in _LOCAL_SOURCES
        and not bool(policy_for(cfg, str(candidate.source))["strict_override"])
        and _is_front(candidate)
    ]

    local_candidates = [
        candidate
        for candidate in candidates
        if id(candidate) in signatures
        and str(candidate.source).casefold() in _LOCAL_SOURCES
    ]
    validated_local: list[tuple[Any, VisualSignature]] = []
    for local in local_candidates:
        signature = signatures[id(local)]
        exact_match = _best_match(signature, exact_front)[0]
        agreeing_sources = {
            str(reference.source).casefold()
            for reference, reference_signature in flexible_front
            if full_frame_match(signature, reference_signature)[0]
        }
        if exact_match is not None or len(agreeing_sources) >= 2:
            local.strict_local_validated = True
            local.strict_status = "validated-local"
            local.strict_reason = (
                "matches exact-release CAA Front"
                if exact_match is not None
                else "matches multiple independent front-art sources"
            )
            validated_local.append((local, signature))

    for candidate in strict_candidates:
        signature = signatures.get(id(candidate))
        if signature is None:
            continue
        front_match, front_dhash, _front_ahash = _best_match(signature, exact_front)
        local_match, local_dhash, _local_ahash = _best_match(signature, validated_local)
        wrong_type, wrong_dhash, _wrong_ahash = _best_match(signature, exact_non_front)
        agreeing_sources = {
            str(reference.source).casefold()
            for reference, reference_signature in flexible_front
            if reference is not candidate
            and full_frame_match(signature, reference_signature)[0]
        }

        if front_match is not None:
            candidate.strict_status = "validated-front"
            candidate.strict_reason = "matches exact-release CAA Front artwork"
            candidate.strict_match_distance = front_dhash
            candidate.strict_match_source = str(front_match.source)
            candidate.strict_preferred_eligible = True
            candidate.strict_auto_eligible = bool(
                getattr(candidate.ref, "approved", False) and _is_front(candidate)
            )
        elif local_match is not None:
            candidate.strict_status = "validated-local"
            candidate.strict_reason = "matches locally cached, source-validated front artwork"
            candidate.strict_match_distance = local_dhash
            candidate.strict_match_source = str(local_match.source)
            candidate.strict_preferred_eligible = True
            candidate.strict_auto_eligible = False
        elif len(agreeing_sources) >= 2:
            candidate.strict_status = "validated-consensus"
            candidate.strict_reason = "matches multiple independent front-art sources"
            candidate.strict_preferred_eligible = True
            candidate.strict_auto_eligible = False
        elif wrong_type is not None:
            candidate.strict_status = "wrong-type"
            candidate.strict_reason = "matches exact-release non-Front artwork"
            candidate.strict_match_distance = wrong_dhash
            candidate.strict_match_source = str(wrong_type.source)
            candidate.strict_preferred_eligible = False
            candidate.strict_auto_eligible = False
        else:
            candidate.strict_status = "unverified"
            candidate.strict_reason = "no full-frame exact-release Front or trusted consensus match"
            candidate.strict_preferred_eligible = False
            candidate.strict_auto_eligible = False

        message = (
            f"strict.content source={candidate.source} status={candidate.strict_status} "
            f"preferred={candidate.strict_preferred_eligible} "
            f"auto={candidate.strict_auto_eligible} reason={candidate.strict_reason!r}"
        )
        if debug is not None:
            debug(message)
        if emit is not None:
            emit(
                "activity",
                category="strict-policy",
                state=candidate.strict_status,
                source=str(candidate.source),
                message=(
                    f"{candidate.source} strict check: {candidate.strict_status} · "
                    f"{candidate.strict_reason}"
                ),
            )


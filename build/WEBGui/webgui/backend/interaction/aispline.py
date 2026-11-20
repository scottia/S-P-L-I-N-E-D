"""SPLINED-side AISPLINE state contract.

There is intentionally no transport, model call, or image-generation logic in
this module.  Future adapters may publish truthful review/activity results into
these structures; absent an adapter, capability remains explicitly unavailable.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any

from PIL import Image, UnidentifiedImageError


SPLINED_TITLE = "S:P:L:I:N:E:D"
AISPLINE_TITLE = "A:I:S:P:L:I:N:E:D"


class AiPhase(str, Enum):
    ASSESSING = "ASSESSING"
    RESTORING = "RESTORING"
    UPSCALING = "UPSCALING"
    VALIDATING = "VALIDATING"
    SUCCESS = "SUCCESS"
    REJECTED = "REJECTED"


@dataclass(frozen=True)
class AiCandidate:
    key: str
    short_side: int
    provenance: str
    review: bool | None = None


@dataclass
class EnhancementSelection:
    enabled: bool
    runtime_available: bool
    minimum_short_side: int = 600
    allow_below_minimum_override: bool = False
    ideal: int = 1800
    candidates: dict[str, AiCandidate] = field(default_factory=dict)
    selected_key: str | None = None
    pending_key: str | None = None
    runtime_upscale_override: str | None = None

    def register(self, candidate: AiCandidate) -> None:
        self.candidates[candidate.key] = candidate

    def delta(self, key: str) -> int:
        candidate = self.candidates[key]
        return max(0, self.ideal - candidate.short_side)

    def label(self, key: str) -> str:
        candidate = self.candidates[key]
        if not self.enabled or not self.runtime_available or candidate.review is not True:
            return "N/A"
        if self.delta(key) <= 0:
            return "N/A"
        if (
            candidate.short_side < self.minimum_short_side
            and not self.allow_below_minimum_override
        ):
            return "N/A"
        marker = "☑" if self.selected_key == key else "☐"
        return f"{marker} +{self.delta(key)} EH"

    def disabled(self, key: str) -> bool:
        return self.selected_key is not None and self.selected_key != key

    def request(
        self,
        key: str,
        *,
        upscale_below_ideal: bool,
        below_floor_confirmed: bool = False,
    ) -> str:
        if not self.enabled:
            return "disabled"
        if not self.runtime_available:
            return "runtime-unavailable"
        if self.selected_key is not None and self.selected_key != key:
            return "selection-locked"
        candidate = self.candidates[key]
        if candidate.review is not True or self.delta(key) <= 0:
            return "not-offered"
        if candidate.short_side < self.minimum_short_side:
            if not self.allow_below_minimum_override:
                return "below-floor"
            if not below_floor_confirmed:
                self.pending_key = key
                return "below-floor-confirmation-required"
        if not upscale_below_ideal:
            self.pending_key = key
            return "upscale-confirmation-required"
        self.selected_key = key
        self.pending_key = None
        self.runtime_upscale_override = None
        return "selected"

    def confirm_upscale(self, accepted: bool) -> str:
        key = self.pending_key
        self.pending_key = None
        if not accepted or key is None:
            return "cancelled"
        self.selected_key = key
        self.runtime_upscale_override = key
        return "selected-runtime-override"

    def clear(self) -> None:
        self.selected_key = None
        self.pending_key = None
        self.runtime_upscale_override = None

    def finish_attempt(self) -> None:
        """One-attempt override never leaks into a later candidate/album."""
        self.clear()


@dataclass
class AiActivityState:
    active: bool = False
    phase: AiPhase | None = None
    message: str = ""
    reason: str = ""

    def apply(self, payload: dict[str, Any]) -> None:
        raw_phase = str(payload.get("phase", "")).upper()
        self.phase = AiPhase(raw_phase)
        self.active = bool(payload.get("active", True))
        self.message = str(payload.get("message", ""))
        self.reason = str(payload.get("reason", ""))

    @property
    def title(self) -> str:
        return AISPLINE_TITLE if self.active else SPLINED_TITLE


def validated_enhanced_results(
    history_entry: dict[str, Any] | None,
    album_path: Path,
) -> list[dict[str, Any]]:
    """Return only retained Enhanced files that still exist and validate.

    The optional history field is deliberately small: ``enhanced_results`` is
    a list of records with ``path`` and ``validated=true``.  It is provenance,
    not an AISPLINE endpoint/model schema.
    """
    if not isinstance(history_entry, dict):
        return []
    records = history_entry.get("enhanced_results", [])
    if not isinstance(records, list):
        return []
    try:
        album_root = album_path.resolve()
    except OSError:
        return []
    valid: list[dict[str, Any]] = []
    for record in records:
        if not isinstance(record, dict) or record.get("validated") is not True:
            continue
        raw_path = str(record.get("path", "")).strip()
        if not raw_path:
            continue
        candidate = Path(raw_path)
        if not candidate.is_absolute():
            candidate = album_root / candidate
        try:
            if candidate.is_symlink():
                continue
            resolved = candidate.resolve(strict=True)
            resolved.relative_to(album_root)
            if not resolved.is_file():
                continue
            with Image.open(resolved) as image:
                width, height = image.size
                image_format = str(image.format or "").lower()
                image.verify()
            if width <= 0 or height <= 0 or image_format not in {"jpeg", "png", "webp"}:
                continue
        except (OSError, ValueError, UnidentifiedImageError):
            continue
        valid.append(
            {
                "path": resolved,
                "width": width,
                "height": height,
                "format": image_format,
                "source_candidate": str(record.get("source_candidate", "")),
            }
        )
    return valid

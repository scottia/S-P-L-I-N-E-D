"""Terminal-cell artwork previews for already-acquired candidates."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from PIL import Image, UnidentifiedImageError


Rgb = tuple[int, int, int]


@dataclass(frozen=True)
class ArtworkPreview:
    """Two image pixels per terminal row, rendered with a ``▀`` cell."""

    rows: tuple[tuple[tuple[Rgb, Rgb], ...], ...]

    @property
    def width(self) -> int:
        return len(self.rows[0]) if self.rows else 0

    @property
    def height(self) -> int:
        return len(self.rows)


def generate_preview(
    path: str | Path,
    *,
    width: int = 8,
    height: int = 4,
) -> ArtworkPreview | None:
    """Decode one existing candidate file once and downsample it safely.

    Candidate discovery/evaluation has already acquired this path.  Failure is
    presentation-only and deliberately cannot affect selection or ranking.
    """
    candidate = Path(path)
    if not str(path) or not candidate.is_file() or candidate.is_symlink():
        return None
    try:
        with Image.open(candidate) as image:
            pixels = image.convert("RGB").resize(
                (max(1, width), max(1, height) * 2),
                Image.Resampling.LANCZOS,
            )
            rows: list[tuple[tuple[Rgb, Rgb], ...]] = []
            for row in range(height):
                pairs: list[tuple[Rgb, Rgb]] = []
                for column in range(width):
                    top = tuple(int(value) for value in pixels.getpixel((column, row * 2)))
                    bottom = tuple(
                        int(value) for value in pixels.getpixel((column, row * 2 + 1))
                    )
                    pairs.append((top, bottom))  # type: ignore[arg-type]
                rows.append(tuple(pairs))
            return ArtworkPreview(tuple(rows))
    except (OSError, ValueError, UnidentifiedImageError):
        return None

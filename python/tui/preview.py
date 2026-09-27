"""Terminal-cell artwork previews for already-acquired candidates."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageOps, UnidentifiedImageError


Rgb = tuple[int, int, int]


@dataclass(frozen=True)
class PreviewCell:
    """One two-colour Unicode quadrant cell prepared by Pillow."""

    glyph: str
    foreground: Rgb
    background: Rgb


@dataclass(frozen=True)
class ArtworkPreview:
    """Two-by-two sampled image quadrants encoded into terminal cells."""

    rows: tuple[tuple[PreviewCell, ...], ...]

    @property
    def width(self) -> int:
        return len(self.rows[0]) if self.rows else 0

    @property
    def height(self) -> int:
        return len(self.rows)


_QUADRANT_GLYPHS = {
    0b0000: " ",
    0b0001: "▘",
    0b0010: "▝",
    0b0011: "▀",
    0b0100: "▖",
    0b0101: "▌",
    0b0110: "▞",
    0b0111: "▛",
    0b1000: "▗",
    0b1001: "▚",
    0b1010: "▐",
    0b1011: "▜",
    0b1100: "▄",
    0b1101: "▙",
    0b1110: "▟",
    0b1111: "█",
}


def _distance(left: Rgb, right: Rgb) -> int:
    return sum((left[index] - right[index]) ** 2 for index in range(3))


def _mean(colors: list[Rgb]) -> Rgb:
    return tuple(
        round(sum(color[channel] for color in colors) / len(colors))
        for channel in range(3)
    )  # type: ignore[return-value]


def _quadrant_cell(colors: tuple[Rgb, Rgb, Rgb, Rgb]) -> PreviewCell:
    """Approximate four Pillow samples with a terminal's foreground/background.

    Unicode quadrant blocks retain twice the horizontal sampling of the old
    half-block renderer.  A tiny two-colour clustering pass selects the best
    foreground/background pair available to a terminal cell without modifying
    or quantizing the cached source artwork.
    """
    pairs = [
        (first, second)
        for first in range(len(colors))
        for second in range(first + 1, len(colors))
    ]
    first, second = max(
        pairs,
        key=lambda pair: _distance(colors[pair[0]], colors[pair[1]]),
    )
    foreground = colors[first]
    background = colors[second]
    if foreground == background:
        return PreviewCell(" ", foreground, background)

    assignments = [
        _distance(color, foreground) <= _distance(color, background)
        for color in colors
    ]
    foreground_colors = [
        color for color, assigned in zip(colors, assignments) if assigned
    ]
    background_colors = [
        color for color, assigned in zip(colors, assignments) if not assigned
    ]
    foreground = _mean(foreground_colors)
    background = _mean(background_colors)

    # Reassign once against the cluster means. This improves antialiased text
    # edges while deliberately avoiding sharpening halos.
    assignments = [
        _distance(color, foreground) <= _distance(color, background)
        for color in colors
    ]
    mask = sum(1 << index for index, assigned in enumerate(assignments) if assigned)
    if mask == 0 or mask == 0b1111:
        average = _mean(list(colors))
        return PreviewCell(" ", average, average)
    return PreviewCell(_QUADRANT_GLYPHS[mask], foreground, background)


def _contained_sample_size(
    source_width: int,
    source_height: int,
    cell_width: int,
    cell_height: int,
) -> tuple[int, int]:
    """Return a pixel-aspect-correct 2x2 sample size for terminal cells."""
    # One terminal cell is approximately twice as tall as it is wide. Work in
    # square display pixels first, then double only the horizontal samples used
    # by quadrant glyphs. This preserves artwork aspect ratio on screen.
    display_width = max(1, cell_width)
    display_height = max(1, cell_height) * 2
    scale = min(display_width / source_width, display_height / source_height)
    contained_width = max(1, min(display_width, round(source_width * scale)))
    contained_height = max(1, min(display_height, round(source_height * scale)))
    return contained_width * 2, contained_height


def generate_preview(
    path: str | Path,
    *,
    width: int = 28,
    height: int = 12,
) -> ArtworkPreview | None:
    """Decode one existing candidate file once and prepare it safely.

    Candidate discovery/evaluation has already acquired this path.  Failure is
    presentation-only and deliberately cannot affect selection or ranking.
    """
    candidate = Path(path)
    if not str(path) or not candidate.is_file() or candidate.is_symlink():
        return None
    try:
        with Image.open(candidate) as image:
            target_width = max(1, width)
            target_height = max(1, height) * 2
            oriented = ImageOps.exif_transpose(image).convert("RGB")
            sample_size = _contained_sample_size(
                oriented.width,
                oriented.height,
                target_width,
                max(1, height),
            )

            # Pillow remains authoritative through the final sample grid. The
            # two horizontal samples per cell are the highest colour-detail
            # density representable by widely supported Unicode quadrant
            # blocks. LANCZOS with a reducing gap performs staged high-quality
            # reduction for large provider artwork; no sharpening is applied.
            source = oriented.resize(
                sample_size,
                resample=Image.Resampling.LANCZOS,
                reducing_gap=3.0,
            )

            # Each cell consumes a 2x2 sample block. Centered letterboxing is
            # calculated in that same grid and preserves the displayed aspect.
            pixels = Image.new(
                "RGB",
                (target_width * 2, target_height),
                (0, 0, 0),
            )
            pixels.paste(
                source,
                (
                    (target_width * 2 - source.width) // 2,
                    (target_height - source.height) // 2,
                ),
            )
            rows: list[tuple[PreviewCell, ...]] = []
            for row in range(max(1, height)):
                cells: list[PreviewCell] = []
                for column in range(target_width):
                    x = column * 2
                    y = row * 2
                    colors = tuple(
                        tuple(int(value) for value in pixels.getpixel(point))
                        for point in (
                            (x, y),
                            (x + 1, y),
                            (x, y + 1),
                            (x + 1, y + 1),
                        )
                    )
                    cells.append(_quadrant_cell(colors))  # type: ignore[arg-type]
                rows.append(tuple(cells))
            return ArtworkPreview(tuple(rows))
    except (OSError, ValueError, UnidentifiedImageError):
        return None

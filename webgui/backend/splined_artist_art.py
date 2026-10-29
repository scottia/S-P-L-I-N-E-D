"""Artist-specific portrait and banner files shared with the Windows runtime.

An Artist that lives under Soundtracks or Various Artists must never reuse the
shared folder's unqualified ``artist.*``. The managed names here match Windows:
``artist-mbid-<uuid>.png``/``banner-mbid-<uuid>.png`` or a stable name hash.
The live SQLite catalog contains identities and paths, not image payloads.
"""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from io import BytesIO
import os
from pathlib import Path
import tempfile
from typing import Iterable
from uuid import UUID

from PIL import Image, ImageDraw, ImageEnhance, ImageFilter, ImageOps, ImageStat


SUPPORTED_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}


@dataclass(frozen=True)
class ArtistAlbum:
    artist: str
    path: Path
    album_mbid: str = ""
    artist_mbid: str = ""


@dataclass(frozen=True)
class ArtistArtTarget:
    directory: Path
    stem: str
    managed: bool = False
    album_count: int = 0

    def for_banner(self) -> "ArtistArtTarget":
        stem = f"banner-{self.stem.removeprefix('artist-')}" if self.managed else "banner"
        return ArtistArtTarget(self.directory, stem, self.managed, self.album_count)


@dataclass(frozen=True)
class ArtistPackage:
    portrait: Path | None = None
    banner: Path | None = None


@dataclass
class AdjustmentProfile:
    picture: int = 0
    sharpen: int = 0
    softness: int = 0
    contrast: int = 0
    exposure: int = 0
    brightness: int = 0
    gamma: int = 0
    color: int = 0
    hue: int = 0
    darkness: int = 0
    tint: int = 0
    tint_color: str = ""


ADJUSTMENT_FIELDS = (
    "picture", "sharpen", "softness", "contrast", "exposure", "brightness",
    "gamma", "color", "hue", "darkness", "tint",
)


def _dominant_color(image: Image.Image) -> tuple[int, int, int]:
    # A tiny in-memory sample is sufficient for a per-photo tint starting
    # point. Nothing is cached or persisted until the user writes the image.
    sample = image.convert("RGB").resize((32, 32), Image.Resampling.BILINEAR)
    return tuple(round(value) for value in ImageStat.Stat(sample).mean[:3])


def apply_adjustments(source: Path | bytes | Image.Image, profile: AdjustmentProfile) -> Image.Image:
    """Preview and write share one pixel pipeline; preserve alpha throughout."""
    image = source.copy().convert("RGBA") if isinstance(source, Image.Image) else _decode(source)
    alpha = image.getchannel("A")
    rgb = image.convert("RGB")
    if profile.picture:
        rgb = ImageEnhance.Sharpness(rgb).enhance(max(0.0, 1 + profile.picture / 100))
    if profile.sharpen > 0:
        rgb = ImageEnhance.Sharpness(rgb).enhance(1 + profile.sharpen / 30)
    if profile.softness > 0:
        softened = rgb.filter(ImageFilter.GaussianBlur(radius=profile.softness / 40))
        rgb = Image.blend(rgb, softened, min(1.0, profile.softness / 100))
    if profile.contrast:
        rgb = ImageEnhance.Contrast(rgb).enhance(max(0.0, 1 + profile.contrast / 100))
    if profile.exposure:
        rgb = ImageEnhance.Brightness(rgb).enhance(max(0.0, 2 ** (profile.exposure / 100)))
    if profile.brightness:
        rgb = ImageEnhance.Brightness(rgb).enhance(max(0.0, 1 + profile.brightness / 100))
    if profile.gamma:
        exponent = 1 / (1 + profile.gamma / 100) if profile.gamma >= 0 else 1 - profile.gamma / 100
        table = [max(0, min(255, round((value / 255) ** exponent * 255))) for value in range(256)]
        rgb = rgb.point(table * 3)
    if profile.color:
        rgb = ImageEnhance.Color(rgb).enhance(max(0.0, 1 + profile.color / 100))
    if profile.hue:
        hsv = rgb.convert("HSV")
        hue, saturation, value = hsv.split()
        shift = round(profile.hue / 360 * 255)
        hue = hue.point([((channel + shift) % 256) for channel in range(256)])
        rgb = Image.merge("HSV", (hue, saturation, value)).convert("RGB")
    if profile.tint:
        if profile.tint_color:
            color = profile.tint_color.lstrip("#")
            if len(color) != 6 or any(character not in "0123456789abcdefABCDEF" for character in color):
                raise ValueError("Tint color must be #RRGGBB")
            tint = tuple(int(color[index:index + 2], 16) for index in (0, 2, 4))
        else:
            tint = _dominant_color(rgb)
        strength = min(1.0, abs(profile.tint) / 100)
        tint_image = Image.new("RGB", rgb.size, tint)
        if profile.tint < 0:
            # Negative tint reduces the dominant color instead of adding an
            # unrelated complementary hue.
            tint_image = Image.new("RGB", rgb.size, _dominant_color(rgb.convert("L").convert("RGB")))
        rgb = Image.blend(rgb, tint_image, strength)
    if profile.darkness > 0:
        rgb = ImageEnhance.Brightness(rgb).enhance(1 - min(100, profile.darkness) / 100)
    result = rgb.convert("RGBA")
    result.putalpha(alpha)
    return result


def _artist_key(value: str) -> str:
    compact = "".join(character for character in value.casefold().replace("&", "and") if character.isalnum())
    return compact.removeprefix("the")


def _valid_mbid(value: str) -> str:
    try:
        return str(UUID(value.strip()))
    except (ValueError, AttributeError):
        return ""


def artist_targets(
    albums: Iterable[ArtistAlbum], artist: str, managed_directory: Path
) -> list[ArtistArtTarget]:
    """Return Artist folder targets, then one managed shared-folder target."""
    if not artist.strip():
        return []
    eligible = [
        album for album in albums
        if album.artist.strip().casefold() == artist.strip().casefold()
        and album.album_mbid.strip()
    ]
    if not eligible:
        return []
    direct: dict[str, tuple[Path, int]] = {}
    artist_ids: set[str] = set()
    for album in eligible:
        valid_id = _valid_mbid(album.artist_mbid)
        if valid_id:
            artist_ids.add(valid_id)
        parent = album.path.parent
        if _artist_key(parent.name) != _artist_key(artist):
            continue
        key = str(parent).casefold()
        prior = direct.get(key)
        direct[key] = (parent, (prior[1] if prior else 0) + 1)
    targets = [
        ArtistArtTarget(directory, "artist", False, count)
        for directory, count in direct.values()
    ]
    targets.sort(key=lambda item: (-item.album_count, str(item.directory).casefold()))
    identity = (
        f"mbid-{next(iter(artist_ids))}"
        if len(artist_ids) == 1
        else f"name-{sha256(artist.strip().casefold().encode('utf-8')).digest()[:16].hex()}"
    )
    targets.append(ArtistArtTarget(managed_directory, f"artist-{identity}", True))
    return targets


def artist_albums_from_db(connection: object, artist: str) -> list[ArtistAlbum]:
    """Read only catalog identities; callers own the database connection."""
    rows = connection.execute(
        "SELECT artists.artist_name, artists.musicbrainz_artistid, "
        "albums.musicbrainz_albumid, albums.path "
        "FROM albums JOIN artists ON artists.artist_key=albums.artist_key "
        "WHERE artists.artist_name=? COLLATE NOCASE ORDER BY albums.path COLLATE NOCASE",
        (artist,),
    )
    return [
        ArtistAlbum(str(name), Path(str(path)), str(album_id or ""), str(artist_id or ""))
        for name, artist_id, album_id, path in rows
    ]


def _matches_stem(path: Path, stem: str) -> bool:
    name = path.stem.casefold()
    stem = stem.casefold()
    return name == stem or (name.startswith(f"{stem}-(") and name.endswith(")"))


def local_files(target: ArtistArtTarget) -> list[Path]:
    if not target.directory.is_dir():
        return []
    files = [
        path for path in target.directory.iterdir()
        if path.is_file() and not path.is_symlink()
        and path.suffix.casefold() in SUPPORTED_EXTENSIONS
        and _matches_stem(path, target.stem)
    ]
    return sorted(files, key=lambda path: (-path.stat().st_mtime_ns, path.name.casefold()))


def local_candidates(targets: Iterable[ArtistArtTarget]) -> list[tuple[str, Path]]:
    result: list[tuple[str, Path]] = []
    seen: set[str] = set()
    for target in targets:
        for label, selected in (("Artist", target), ("Banner", target.for_banner())):
            for path in local_files(selected):
                key = str(path).casefold()
                if key not in seen:
                    seen.add(key)
                    result.append((label, path))
    return result


def saved_package(targets: Iterable[ArtistArtTarget]) -> ArtistPackage:
    targets = list(targets)
    return ArtistPackage(
        next((files[0] for target in targets if (files := local_files(target))), None),
        next((files[0] for target in targets if (files := local_files(target.for_banner()))), None),
    )


def choose_banner(
    saved: Path | None, navidrome: Path | None, portrait: Path | None
) -> tuple[Path | None, bool]:
    """Return background and whether portrait needs landscape expansion."""
    if saved is not None:
        return saved, False
    if navidrome is not None:
        return navidrome, False
    return portrait, portrait is not None


def _decode(source: Path | bytes) -> Image.Image:
    with Image.open(BytesIO(source) if isinstance(source, bytes) else source) as image:
        image.load()
        return ImageOps.exif_transpose(image).convert("RGBA")


def _fit(image: Image.Image, width: int, height: int, zoom: float, x: float, y: float) -> Image.Image:
    if width < 1 or height < 1 or not 1.0 <= zoom <= 4.0 or not -1.0 <= x <= 1.0 or not -1.0 <= y <= 1.0:
        raise ValueError("Artwork width, height, zoom, or position is out of range")
    scale = max(width / image.width, height / image.height) * zoom
    resized = image.resize(
        (max(width, round(image.width * scale)), max(height, round(image.height * scale))),
        Image.Resampling.LANCZOS,
    )
    overflow_x = resized.width - width
    overflow_y = resized.height - height
    left = round(overflow_x * (x + 1.0) / 2.0)
    top = round(overflow_y * (y + 1.0) / 2.0)
    return resized.crop((left, top, left + width, top + height))


def prepare_portrait(
    source: Path | bytes, side: int = 1200, *, zoom: float = 1.0,
    x: float = 0.0, y: float = 0.0, adjustments: AdjustmentProfile | None = None,
) -> bytes:
    image = _fit(apply_adjustments(source, adjustments or AdjustmentProfile()), side, side, zoom, x, y)
    # Keep artist.* square. Only the overview thumbnail is circular.
    output = BytesIO()
    image.save(output, format="PNG")
    return output.getvalue()


def prepare_banner(
    source: Path | bytes, *, width: int = 1600, height: int = 600,
    zoom: float = 1.0, x: float = 0.0, y: float = 0.0,
    adjustments: AdjustmentProfile | None = None,
) -> bytes:
    """Fit a banner to a reproducible frame with bounded zoom/position."""
    image = _fit(apply_adjustments(source, adjustments or AdjustmentProfile()), width, height, zoom, x, y)
    output = BytesIO()
    image.save(output, format="PNG")
    return output.getvalue()


def compose_package(portrait_png: bytes | None, banner_png: bytes | None, *, width: int = 960, height: int = 360) -> bytes:
    """Render the Artist Overview-style banner plus round portrait preview."""
    background = Image.new("RGBA", (width, height), (10, 24, 42, 255))
    if banner_png:
        banner = _fit(_decode(banner_png), width, height, 1.0, 0.0, 0.0)
        background.alpha_composite(banner)
    if portrait_png:
        side = round(height * 0.7)
        portrait = _fit(_decode(portrait_png), side, side, 1.0, 0.0, 0.0)
        mask = Image.new("L", (side, side), 0)
        ImageDraw.Draw(mask).ellipse((0, 0, side - 1, side - 1), fill=255)
        portrait.putalpha(mask)
        background.alpha_composite(portrait, (round(height * 0.15), round(height * 0.15)))
    output = BytesIO()
    background.save(output, format="PNG")
    return output.getvalue()


def write_image(
    target: ArtistArtTarget, prepared_png: bytes, *, preserve_file: bool = False
) -> Path:
    """Write approved bytes atomically; leave existing art until validation passes."""
    with Image.open(BytesIO(prepared_png)) as image:
        image.verify()
    target.directory.mkdir(parents=True, exist_ok=True)
    canonical = target.directory / f"{target.stem}.png"
    destination = canonical
    if preserve_file:
        number = 1
        while destination.exists():
            if destination.read_bytes() == prepared_png:
                return destination
            number += 1
            destination = target.directory / f"{target.stem}-({number}).png"
    handle, temporary = tempfile.mkstemp(prefix=f".{target.stem}-", suffix=".png", dir=target.directory)
    try:
        with os.fdopen(handle, "wb") as file:
            file.write(prepared_png)
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary, destination)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    if not preserve_file:
        for old in local_files(target):
            if old != destination:
                old.unlink()
    return destination

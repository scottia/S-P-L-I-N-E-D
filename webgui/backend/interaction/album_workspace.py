"""Browser-backed Album cover preview with an explicit, atomic write."""

from __future__ import annotations

from dataclasses import dataclass, field
from io import BytesIO
import os
from pathlib import Path
import shutil
import tempfile

from PIL import Image, ImageOps

from splined_artist_art import ADJUSTMENT_FIELDS, AdjustmentProfile, apply_adjustments


STATIC_EXTENSIONS = (".jpg", ".jpeg", ".png")
VIEW_EXTENSIONS = STATIC_EXTENSIONS + (".webp",)


def find_cover(album: Path, stem: str = "cover") -> Path | None:
    for suffix in VIEW_EXTENSIONS:
        candidate = album / f"{stem}{suffix}"
        if candidate.is_file() and not candidate.is_symlink():
            return candidate
    return None


@dataclass
class AlbumWorkspace:
    album: Path
    cache_dir: Path
    writable: bool
    cover_stem: str = "cover"
    source: Path | None = None
    saved: Path | None = None
    profile: AdjustmentProfile = field(default_factory=AdjustmentProfile)
    selected_adjustment: int = 0
    upscale: bool = False
    confirm_write: bool = False
    input_path: str | None = None
    preview_dir: Path | None = None
    notice: str = ""

    @classmethod
    def load(cls, album: Path, cache_dir: Path, *, writable: bool, cover_stem: str = "cover") -> "AlbumWorkspace":
        if not album.is_dir():
            raise FileNotFoundError(f"Album directory is missing: {album}")
        saved = find_cover(album, cover_stem)
        session = cls(album, cache_dir, writable, cover_stem, saved, saved)
        session.preview_dir = Path(tempfile.mkdtemp(prefix="album-preview-", dir=cache_dir))
        session.refresh_preview()
        return session

    def choose_local(self, path: Path) -> None:
        if not path.is_file() or path.is_symlink():
            raise ValueError("Choose an existing regular image file")
        with Image.open(path) as image:
            image.verify()
        self.source = path
        self.profile = AdjustmentProfile()
        self.confirm_write = False
        self.refresh_preview()

    def refresh_preview(self) -> None:
        if self.preview_dir is None:
            return
        if self.saved is not None:
            with Image.open(self.saved) as image:
                image = ImageOps.exif_transpose(image)
                image.thumbnail((900, 900), Image.Resampling.LANCZOS)
                image.convert("RGB").save(self.preview_dir / "original.png", format="PNG")
        if self.source is None:
            return
        image = apply_adjustments(self.source, self.profile)
        if self.upscale:
            image = image.resize((image.width * 2, image.height * 2), Image.Resampling.LANCZOS)
        output = BytesIO()
        image.save(output, format="PNG")
        (self.preview_dir / "enhanced.png").write_bytes(output.getvalue())

    def adjust(self, delta: int) -> None:
        field = ADJUSTMENT_FIELDS[self.selected_adjustment]
        value = getattr(self.profile, field)
        setattr(self.profile, field, max(-100, min(100, value + delta)))
        self.confirm_write = False
        self.refresh_preview()

    def reset(self) -> None:
        self.profile = AdjustmentProfile()
        self.upscale = False
        self.confirm_write = False
        self.refresh_preview()

    def write_approved(self) -> Path | None:
        if not self.writable:
            raise PermissionError("LIVE WRITE is disabled in this Python configuration")
        if self.source is None or self.preview_dir is None:
            raise ValueError("Choose an Album cover first")
        if not self.confirm_write:
            self.confirm_write = True
            self.notice = "Confirm the write in the browser to approve the Album cover write"
            return None
        prepared = self.preview_dir / "enhanced.png"
        with Image.open(prepared) as image:
            image.load()
            image = image.convert("RGB")
            suffix = self.saved.suffix.casefold() if self.saved and self.saved.suffix.casefold() in STATIC_EXTENSIONS else ".jpg"
            destination = self.album / f"{self.cover_stem}{suffix}"
            output = BytesIO()
            image.save(output, format="PNG" if suffix == ".png" else "JPEG", quality=95)
        descriptor, temporary = tempfile.mkstemp(prefix=f".{self.cover_stem}-", suffix=suffix, dir=self.album)
        try:
            with os.fdopen(descriptor, "wb") as file:
                file.write(output.getvalue())
                file.flush()
                os.fsync(file.fileno())
            os.replace(temporary, destination)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
        self.saved = destination
        self.source = destination
        self.profile = AdjustmentProfile()
        self.upscale = False
        self.confirm_write = False
        self.notice = f"Saved {destination.name}; Album index refreshes on the next scan"
        self.refresh_preview()
        return destination

    def close(self) -> None:
        directory = self.preview_dir
        self.preview_dir = None
        if directory is None or not directory.exists():
            return
        if directory.resolve().parent != self.cache_dir.resolve() or not directory.name.startswith("album-preview-"):
            raise ValueError("Refusing to remove an unexpected Album preview directory")
        shutil.rmtree(directory)

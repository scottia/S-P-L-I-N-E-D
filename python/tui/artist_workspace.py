"""Terminal-native Artist Art package decisions and preview state."""

from __future__ import annotations

from dataclasses import dataclass, field
from contextlib import closing
from io import BytesIO
from pathlib import Path
import shutil
import tempfile

from PIL import Image

from splined_artist_art import (
    ADJUSTMENT_FIELDS, AdjustmentProfile, ArtistArtTarget, ArtistPackage,
    apply_adjustments, artist_albums_from_db, artist_targets, choose_banner,
    compose_package, local_candidates, prepare_banner, prepare_portrait,
    saved_package, write_image,
)
from splined_db_audit import connect_sqlite


@dataclass
class ComponentFit:
    zoom: float = 1.0
    x: float = 0.0
    y: float = 0.0


@dataclass
class ArtistWorkspace:
    artist: str
    targets: list[ArtistArtTarget]
    saved: ArtistPackage
    candidates: list[tuple[str, Path]]
    cache_dir: Path
    writable: bool
    portrait_source: Path | None = None
    banner_source: Path | None = None
    portrait_active: bool = True
    banner_active: bool = False
    theme_enabled: bool = False
    selected_candidate: int = 0
    selected_adjustment: int = 0
    portrait_adjustments: AdjustmentProfile = field(default_factory=AdjustmentProfile)
    banner_adjustments: AdjustmentProfile = field(default_factory=AdjustmentProfile)
    theme_adjustments: AdjustmentProfile = field(default_factory=AdjustmentProfile)
    portrait_fit: ComponentFit = field(default_factory=ComponentFit)
    banner_fit: ComponentFit = field(default_factory=ComponentFit)
    upscale: bool = False
    notice: str = ""
    confirm_write: bool = False
    input_path: str | None = None
    preview_dir: Path | None = None
    artist_mbid: str = ""

    @classmethod
    def load(
        cls, artist: str, database: Path, managed_directory: Path,
        cache_dir: Path, *, writable: bool,
    ) -> "ArtistWorkspace":
        if not database.is_file():
            raise FileNotFoundError(f"SPLINED index is missing: {database}")
        with closing(connect_sqlite(database)) as connection:
            connection.execute("PRAGMA query_only=ON")
            albums = artist_albums_from_db(connection, artist)
        targets = artist_targets(albums, artist, managed_directory)
        if not targets:
            raise ValueError("This Artist has no indexed non-fallback Album ID")
        saved = saved_package(targets)
        session = cls(
            artist, targets, saved, local_candidates(targets),
            cache_dir, writable, saved.portrait, saved.banner,
        )
        session.artist_mbid = next((item.artist_mbid for item in albums if item.artist_mbid), "")
        session.preview_dir = Path(tempfile.mkdtemp(prefix="artist-preview-", dir=cache_dir))
        session.refresh_preview()
        return session

    @property
    def selected_component(self) -> str:
        if self.portrait_active and self.banner_active:
            return "package"
        if self.banner_active:
            return "banner"
        return "portrait"

    @property
    def selected_path(self) -> Path | None:
        if self.preview_dir is None:
            return None
        return self.preview_dir / f"{self.selected_component}.png"

    def select_radios(self, portrait: bool, banner: bool, *, current: bool = False) -> None:
        if current:
            self.portrait_source = self.saved.portrait
            self.banner_source = self.saved.banner
            portrait = banner = True
        if not portrait and not banner:
            raise ValueError("Select an Artist image, banner, or both")
        self.portrait_active, self.banner_active = portrait, banner
        if not (portrait and banner):
            self.theme_enabled = False
        self.confirm_write = False
        self.refresh_preview()

    def choose_candidate(self, kind: str) -> None:
        if not self.candidates:
            raise ValueError("No local Artist Art candidate is available")
        path = self.candidates[self.selected_candidate][1]
        if kind == "portrait":
            self.portrait_source = path
            self.select_radios(True, False)
        elif kind == "banner":
            self.banner_source = path
            self.select_radios(False, True)
        else:
            raise ValueError(f"Unknown Artist Art component: {kind}")

    def add_local_candidate(self, source: Path) -> None:
        if not source.is_file() or source.is_symlink():
            raise ValueError("Candidate must be an existing regular image file")
        with Image.open(source) as image:
            image.verify()
        self.candidates.append(("Added local image", source))
        self.selected_candidate = len(self.candidates) - 1
        self.notice = f"Local candidate ready: {source.name}"

    def add_discovered_candidates(self, found: list[tuple[str, bytes]]) -> int:
        if self.preview_dir is None:
            return 0
        first = len(self.candidates)
        for label, image in found:
            destination = self.preview_dir / f"remote-{len(self.candidates) + 1:02d}.png"
            destination.write_bytes(image)
            self.candidates.append((label, destination))
        if len(self.candidates) > first:
            self.selected_candidate = first
        return len(self.candidates) - first

    def _theme(self, prepared: bytes) -> bytes:
        if not self.theme_enabled:
            return prepared
        with Image.open(BytesIO(prepared)) as image:
            adjusted = apply_adjustments(image, self.theme_adjustments)
        out = BytesIO()
        adjusted.save(out, format="PNG")
        return out.getvalue()

    def _final_portrait(self) -> bytes:
        assert self.portrait_source is not None
        with Image.open(self.portrait_source) as source:
            side = min(source.size)
        if self.upscale:
            side *= 2
        fit = self.portrait_fit
        return self._theme(prepare_portrait(
            self.portrait_source, side, zoom=fit.zoom, x=fit.x, y=fit.y,
            adjustments=self.portrait_adjustments,
        ))

    def _final_banner(self) -> bytes:
        assert self.banner_source is not None
        with Image.open(self.banner_source) as source:
            source_width, source_height = source.size
        ratio = 8 / 3
        if source_width / source_height > ratio:
            width, height = round(source_height * ratio), source_height
        else:
            width, height = source_width, round(source_width / ratio)
        if self.upscale:
            width, height = width * 2, height * 2
        fit = self.banner_fit
        return self._theme(prepare_banner(
            self.banner_source, width=max(1, width), height=max(1, height),
            zoom=fit.zoom, x=fit.x, y=fit.y,
            adjustments=self.banner_adjustments,
        ))

    def refresh_preview(self) -> None:
        if self.preview_dir is None:
            return
        side = 1600 if self.upscale else 800
        width, height = (2400, 900) if self.upscale else (1200, 450)
        portrait: bytes | None = None
        banner: bytes | None = None
        if self.portrait_source:
            fit = self.portrait_fit
            portrait = self._theme(prepare_portrait(
                self.portrait_source, side, zoom=fit.zoom, x=fit.x, y=fit.y,
                adjustments=self.portrait_adjustments,
            ))
            (self.preview_dir / "portrait.png").write_bytes(portrait)
        if self.banner_source:
            fit = self.banner_fit
            banner = self._theme(prepare_banner(
                self.banner_source, width=width, height=height,
                zoom=fit.zoom, x=fit.x, y=fit.y,
                adjustments=self.banner_adjustments,
            ))
            (self.preview_dir / "banner.png").write_bytes(banner)
        if portrait or banner:
            display_banner = banner
            if display_banner is None and self.portrait_source is not None:
                display_banner = prepare_banner(
                    self.portrait_source, width=width, height=height,
                    adjustments=self.portrait_adjustments,
                )
            (self.preview_dir / "package.png").write_bytes(compose_package(portrait, display_banner))
        current_portrait = self.saved.portrait.read_bytes() if self.saved.portrait else None
        current_banner_path, expansion = choose_banner(
            self.saved.banner, None, self.saved.portrait,
        )
        current_banner = current_banner_path.read_bytes() if current_banner_path else None
        if current_banner and expansion:
            current_banner = prepare_banner(current_banner, width=1200, height=450)
        if current_portrait or current_banner:
            (self.preview_dir / "current.png").write_bytes(
                compose_package(current_portrait, current_banner)
            )

    def adjust(self, delta: int) -> None:
        if self.portrait_active and self.banner_active and not self.theme_enabled:
            raise ValueError("Enable Themed Packaging to edit both components together")
        field = ADJUSTMENT_FIELDS[self.selected_adjustment]
        profile = (
            self.theme_adjustments if self.theme_enabled
            else self.banner_adjustments if self.banner_active
            else self.portrait_adjustments
        )
        value = getattr(profile, field)
        setattr(profile, field, max(-100, min(100, value + delta)))
        self.refresh_preview()

    def fit(self, *, zoom: float = 0.0, x: float = 0.0, y: float = 0.0) -> None:
        if self.portrait_active and self.banner_active and not self.theme_enabled:
            raise ValueError("Enable Themed Packaging to move the package")
        targets = (
            [self.portrait_fit, self.banner_fit] if self.portrait_active and self.banner_active
            else [self.banner_fit if self.banner_active else self.portrait_fit]
        )
        for target in targets:
            target.zoom = max(1.0, min(4.0, round(target.zoom + zoom, 3)))
            target.x = max(-1.0, min(1.0, round(target.x + x, 3)))
            target.y = max(-1.0, min(1.0, round(target.y + y, 3)))
        self.refresh_preview()

    def reset_preview(self) -> None:
        if self.portrait_active and self.banner_active:
            self.portrait_adjustments = AdjustmentProfile()
            self.banner_adjustments = AdjustmentProfile()
            self.theme_adjustments = AdjustmentProfile()
            self.portrait_fit = ComponentFit()
            self.banner_fit = ComponentFit()
        elif self.banner_active:
            self.banner_adjustments = AdjustmentProfile()
            self.banner_fit = ComponentFit()
        else:
            self.portrait_adjustments = AdjustmentProfile()
            self.portrait_fit = ComponentFit()
        self.upscale = False
        self.refresh_preview()

    def write_approved(self) -> list[Path]:
        if not self.writable:
            raise PermissionError("LIVE WRITE is disabled in this Python configuration")
        if not self.confirm_write:
            self.confirm_write = True
            self.notice = "Press W again to approve the selected Artist Art write"
            return []
        self.confirm_write = False
        if self.preview_dir is None:
            raise RuntimeError("Artist Art preview is unavailable")
        written: list[Path] = []
        target = self.targets[0]
        # Prepare and validate both images before replacing either file.
        portrait = self._final_portrait() if self.portrait_active and self.portrait_source else None
        banner = self._final_banner() if self.banner_active and self.banner_source else None
        if portrait is None and banner is None:
            raise ValueError("Select a portrait or banner candidate before writing")
        if portrait is not None:
            written.append(write_image(target, portrait))
        if banner is not None:
            written.append(write_image(target.for_banner(), banner))
        self.saved = saved_package(self.targets)
        self.candidates = local_candidates(self.targets)
        self.notice = "Saved " + ", ".join(path.name for path in written)
        self.refresh_preview()
        return written

    def close(self) -> None:
        directory = self.preview_dir
        self.preview_dir = None
        if directory is None or not directory.exists():
            return
        resolved_cache = self.cache_dir.resolve()
        resolved_directory = directory.resolve()
        if resolved_directory.parent != resolved_cache or not directory.name.startswith("artist-preview-"):
            raise ValueError("Refusing to remove an unexpected Artist preview directory")
        shutil.rmtree(directory)

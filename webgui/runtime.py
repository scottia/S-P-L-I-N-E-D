"""Validated operational configuration and headless processing integration."""

from __future__ import annotations

from contextlib import closing
from copy import deepcopy
from pathlib import Path
import json
import os
import tempfile
import threading
import tomllib

import tomlkit

import splined as core
import splined_scan as scan
import splined_compilation_authority_policy as compilation
import splined_ranking_policy as ranking

from webgui.database import Database


class CatalogReference:
    """A catalog capability for policy code that previously accepted a file."""

    def is_file(self):
        return True

    def exists(self):
        return True

    def __str__(self):
        return "PostgreSQL catalog"


class Runtime:
    def __init__(self, config_file: Path, database: Database, cfg=None):
        self.config_file = config_file.resolve()
        self.database = database
        self.lock = threading.RLock()
        self.processing = threading.RLock()
        self.cfg = deepcopy(cfg) if cfg is not None else self.load()
        self.validate(self.cfg)
        self._ensure_cache_directories(self.cfg)
        compilation.install(core)
        ranking.install(core, scan)
        self._bindings = (
            compilation.connect_sqlite,
            compilation._database_path,
            core._load_cache_state,
            core._save_cache_state,
            core.compilation_prepare_embedded,
        )
        core.compilation_prepare_embedded = self.prepare_comp_artwork
        compilation.connect_sqlite = lambda *_args, **_kwargs: self.database.connect()
        compilation._database_path = lambda *_args, **_kwargs: CatalogReference()
        core._load_cache_state = self.load_cache
        core._save_cache_state = self.save_cache

    def load(self):
        if not self.config_file.is_file():
            example = Path(__file__).resolve().parents[1] / "docker/config.example.toml"
            return tomllib.loads(example.read_text(encoding="utf-8"))
        cfg = tomllib.loads(self.config_file.read_text(encoding="utf-8"))
        self.validate(cfg)
        return cfg

    @staticmethod
    def validate(cfg):
        core.validate_config_v5(cfg)
        side = cfg.get("output", {}).get("embedded_thumbnail_size", 1200)
        if (
            isinstance(side, bool)
            or not isinstance(side, int)
            or not 300 <= side <= 4000
        ):
            raise ValueError(
                "Embedded thumbnail size must be an integer from 300 through 4000"
            )

    @property
    def music(self):
        return core.resolve_path(
            self.config_file, str(self.cfg.get("library", {}).get("music_library", ""))
        ).resolve()

    @property
    def cache(self):
        return core.runtime_cache_dir(self.config_file, self.cfg).resolve()

    @property
    def temporary_cache(self):
        return self._temporary_cache_dir(self.cfg)

    def _temporary_cache_dir(self, cfg):
        configured = str(cfg.get("scan", {}).get("temporary_cache_dir", "")).strip()
        if configured:
            return core.resolve_path(self.config_file, configured).resolve()
        return core.runtime_cache_dir(self.config_file, cfg).resolve()

    def _ensure_cache_directories(self, cfg):
        persistent = core.runtime_cache_dir(self.config_file, cfg).resolve()
        temporary = self._temporary_cache_dir(cfg)
        try:
            persistent.mkdir(parents=True, exist_ok=True)
            temporary.mkdir(parents=True, exist_ok=True)
        except OSError as error:
            raise ValueError(
                "Unable to create the persistent or temporary cache directory; "
                "check the configured paths and container volume permissions"
            ) from error

    @property
    def artist_art(self):
        return core.runtime_artist_art_dir(self.config_file, self.cfg).resolve()

    @property
    def credentials(self):
        return core.runtime_credential_dir(self.config_file, self.cfg).resolve()

    def require_write(self):
        if self.cfg.get("mode", "read").lower() != "write":
            raise PermissionError(
                "Enable Live Write in Settings before writing artwork or track tags"
            )

    def settings(self):
        result = deepcopy(self.cfg)
        for table in ("webgui", "database"):
            if table in result:
                for key in ("token", "password", "url"):
                    if key in result[table]:
                        result[table][key] = ""
        return result

    def save_settings(self, cfg):
        if not isinstance(cfg, dict):
            raise ValueError("Settings must be an object")
        cfg = deepcopy(cfg)
        self.validate(cfg)
        name = str(cfg.get("output", {}).get("file_name", "cover"))
        core.validate_filename(name)
        library = core.resolve_path(
            self.config_file, str(cfg.get("library", {}).get("music_library", ""))
        )
        if not library.is_dir():
            raise ValueError("The music library must be an existing directory")
        self._ensure_cache_directories(cfg)
        with self.lock:
            if not self.processing.acquire(blocking=False):
                raise ValueError(
                    "Wait for active artwork work to finish before changing settings"
                )
            try:
                self.config_file.parent.mkdir(parents=True, exist_ok=True)
                descriptor, temporary = tempfile.mkstemp(
                    prefix=".splined-config-",
                    suffix=".toml",
                    dir=self.config_file.parent,
                )
                try:
                    with os.fdopen(
                        descriptor, "w", encoding="utf-8", newline="\n"
                    ) as handle:
                        handle.write(tomlkit.dumps(cfg))
                        handle.flush()
                        os.fsync(handle.fileno())
                    os.replace(temporary, self.config_file)
                finally:
                    Path(temporary).unlink(missing_ok=True)
                self.cfg = cfg
            finally:
                self.processing.release()
        return self.settings()

    def load_cache(self, _path, cache_key):
        with closing(self.database.connect()) as db:
            row = db.execute(
                "SELECT payload_json FROM cache_entries WHERE cache_key=?", (cache_key,)
            ).fetchone()
            return json.loads(row["payload_json"]) if row else None

    def save_cache(self, _path, cache_key, cache_type, state):
        from splined_media_tags import utc_now

        with closing(self.database.connect()) as db, db:
            now = utc_now()
            db.execute(
                "INSERT INTO cache_entries(cache_key,cache_type,payload_json,created_at,updated_at,splined_version) VALUES(?,?,?,?,?,?) ON CONFLICT(cache_key) DO UPDATE SET payload_json=excluded.payload_json,updated_at=excluded.updated_at,splined_version=excluded.splined_version",
                (cache_key, cache_type, json.dumps(state), now, now, core.VERSION),
            )

    def prepare_comp_artwork(self, candidate, cfg):
        from PIL import Image, ImageOps
        from io import BytesIO
        from webgui.imaging import encode

        content, info, target = compilation.prepare_embedded_artwork(
            core, candidate, cfg
        )
        side = int(cfg.get("output", {}).get("embedded_thumbnail_size", 1200))
        with Image.open(BytesIO(content)) as image:
            body = encode(
                ImageOps.fit(image, (side, side), method=Image.Resampling.LANCZOS),
                "jpeg",
            )
        return body, dict(info, width=side, height=side, bytes=len(body)), "jpeg"

    def close(self):
        (
            compilation.connect_sqlite,
            compilation._database_path,
            core._load_cache_state,
            core._save_cache_state,
            core.compilation_prepare_embedded,
        ) = self._bindings

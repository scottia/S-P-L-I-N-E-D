"""Persistent status-only Select Media inventory cache.

This cache accelerates the lightweight Artist-status readiness pass without
restoring the removed SQLite picker snapshot. It stores only filesystem
inventory needed to derive folder status; it never becomes selection, history,
provider, ranking, or final-write authority.
"""

from __future__ import annotations

import atexit
import fnmatch
import hashlib
import importlib.abc
import importlib.machinery
import json
import os
import sys
import threading
import time
import tomllib
from pathlib import Path
from types import ModuleType
from typing import Any, Callable


CACHE_VERSION = 1
CACHE_FILE = "select-media-status.json"
SAVE_BATCH = 16
SAVE_INTERVAL_SECONDS = 2.0

_LOCK = threading.RLock()
_PAYLOAD: dict[str, Any] | None = None
_CACHE_PATH: Path | None = None
_DIRTY = 0
_LAST_SAVE = 0.0
_CURRENT_BASELINE = 0
_CURRENT_TOTAL = 0
_INSTALLED = False


def _config_path() -> Path:
    return Path(os.environ.get("SPLINED_CONFIG", "/config/config.toml"))


def _load_config() -> dict[str, Any]:
    path = _config_path()
    try:
        with path.open("rb") as handle:
            raw = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return {}
    return raw if isinstance(raw, dict) else {}


def _section(cfg: dict[str, Any], name: str) -> dict[str, Any]:
    value = cfg.get(name, {})
    return value if isinstance(value, dict) else {}


def _resolve_config_path(raw: str, default: Path) -> Path:
    value = raw.strip()
    if not value:
        return default
    path = Path(value)
    return path if path.is_absolute() else (_config_path().parent / path).resolve()


def _runtime_settings() -> tuple[Path, Path | None, list[str], str]:
    cfg = _load_config()
    scan = _section(cfg, "scan")
    library = _section(cfg, "library")
    output = _section(cfg, "output")
    history_env = os.environ.get("SPLINED_HISTORY_DIR", "").strip()
    history_dir = (
        Path(history_env)
        if history_env
        else _resolve_config_path(
            str(scan.get("history_dir", "")),
            Path("/_logs/_history"),
        )
    )
    raw_library = str(library.get("music_library", "")).strip()
    library_root = (
        _resolve_config_path(raw_library, Path("/music"))
        if raw_library
        else None
    )
    ignored = [str(value) for value in library.get("ignored_subs", [])]
    cover_name = str(output.get("file_name", "cover")).strip() or "cover"
    return history_dir, library_root, ignored, cover_name


def _inventory_key(ignored: list[str], configured_file_name: str) -> str:
    body = json.dumps(
        {
            "ignored": sorted(str(value) for value in ignored),
            "cover_name": configured_file_name.casefold(),
        },
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(body).hexdigest()


def _empty_payload() -> dict[str, Any]:
    return {
        "version": CACHE_VERSION,
        "updated_at_unix": 0.0,
        "artists": {},
    }


def _ensure_loaded() -> tuple[dict[str, Any], Path]:
    global _PAYLOAD, _CACHE_PATH
    with _LOCK:
        history_dir, _library_root, _ignored, _cover_name = _runtime_settings()
        path = history_dir / CACHE_FILE
        if _PAYLOAD is not None and _CACHE_PATH == path:
            return _PAYLOAD, path

        payload = _empty_payload()
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            raw = None
        if isinstance(raw, dict) and raw.get("version") == CACHE_VERSION:
            artists = raw.get("artists")
            if isinstance(artists, dict):
                payload = raw
        payload.setdefault("artists", {})
        _PAYLOAD = payload
        _CACHE_PATH = path
        return payload, path


def _atomic_write(path: Path, body: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(
        f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp"
    )
    try:
        with temp.open("wb") as handle:
            handle.write(body)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp, path)
    finally:
        try:
            temp.unlink()
        except FileNotFoundError:
            pass


def _save_locked(*, force: bool = False) -> None:
    global _DIRTY, _LAST_SAVE
    if _PAYLOAD is None or _CACHE_PATH is None or not _DIRTY:
        return
    now = time.monotonic()
    if (
        not force
        and _DIRTY < SAVE_BATCH
        and now - _LAST_SAVE < SAVE_INTERVAL_SECONDS
    ):
        return
    _PAYLOAD["version"] = CACHE_VERSION
    _PAYLOAD["updated_at_unix"] = time.time()
    body = (json.dumps(_PAYLOAD, indent=2, sort_keys=True) + "\n").encode(
        "utf-8"
    )
    _atomic_write(_CACHE_PATH, body)
    _DIRTY = 0
    _LAST_SAVE = now


def flush() -> None:
    with _LOCK:
        try:
            _save_locked(force=True)
        except (OSError, TypeError, ValueError):
            # Status acceleration must never make SPLINED fail.
            pass


atexit.register(flush)


def _inside(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def _entry_albums(
    core: ModuleType,
    root: Path,
    entry: dict[str, Any],
    fingerprint_paths: set[str],
) -> list[Any] | None:
    directories = entry.get("directories")
    raw_albums = entry.get("albums")
    if not isinstance(directories, dict) or not isinstance(raw_albums, list):
        return None

    for raw_path, raw_mtime in directories.items():
        directory = Path(str(raw_path))
        if not _inside(directory, root):
            return None
        try:
            current = directory.stat().st_mtime_ns
            expected = int(raw_mtime)
        except (OSError, TypeError, ValueError):
            return None
        if current != expected:
            return None

    albums: list[Any] = []
    for raw in raw_albums:
        if not isinstance(raw, dict):
            return None
        album_path = Path(str(raw.get("path", "")))
        if not str(album_path) or not _inside(album_path, root):
            return None
        audio = [Path(str(value)) for value in raw.get("audio", [])]
        local_art = [Path(str(value)) for value in raw.get("local_art", [])]
        if any(not _inside(path, album_path) for path in (*audio, *local_art)):
            return None
        fingerprint = raw.get("inventory_fingerprint")
        fingerprint = str(fingerprint) if fingerprint else None

        if str(album_path) in fingerprint_paths:
            values: list[tuple[Path, int | None, int | None]] = []
            for path in audio:
                try:
                    stat = path.stat()
                    values.append((path, stat.st_size, stat.st_mtime_ns))
                except OSError:
                    values.append((path, None, None))
            current_fingerprint = core._fingerprint_from_metadata(values)
            if fingerprint is not None and fingerprint != current_fingerprint:
                return None
            fingerprint = current_fingerprint

        albums.append(
            core.AlbumDir(
                album_path,
                audio,
                local_art,
                fingerprint,
            )
        )
    albums.sort(key=lambda item: str(item.path).casefold())
    return albums


def _directory_snapshot(root: Path, albums: list[Any]) -> dict[str, int]:
    paths: set[Path] = {root}
    for album in albums:
        album_path = Path(album.path)
        if not _inside(album_path, root):
            continue
        current = album_path
        while True:
            paths.add(current)
            if current == root:
                break
            current = current.parent

    # Preserve empty first-level category folders as change sentinels. This is
    # important for layouts such as [Various Artists]/<category>/<album>.
    try:
        with os.scandir(root) as iterator:
            for item in iterator:
                try:
                    if not item.is_symlink() and item.is_dir(follow_symlinks=False):
                        paths.add(Path(item.path))
                except OSError:
                    continue
    except OSError:
        pass

    snapshot: dict[str, int] = {}
    for path in sorted(paths, key=lambda value: str(value).casefold()):
        try:
            snapshot[str(path)] = path.stat().st_mtime_ns
        except OSError:
            # An unstable tree should not be reused on the next run.
            return {}
    return snapshot


def _cache_entry(
    root: Path,
    albums: list[Any],
    inventory_key: str,
) -> dict[str, Any]:
    return {
        "inventory_key": inventory_key,
        "complete": True,
        "directories": _directory_snapshot(root, albums),
        "albums": [
            {
                "path": str(album.path),
                "audio": [str(path) for path in album.audio_files],
                "local_art": [str(path) for path in album.local_art_files],
                "inventory_fingerprint": album.inventory_fingerprint,
            }
            for album in albums
        ],
        "updated_at_unix": time.time(),
    }


def _should_cache_status_inventory(
    root: Path,
    workers: int,
    progress: Callable[[int, int], None] | None,
) -> bool:
    if workers != 1 or progress is not None:
        return False
    _history_dir, library_root, _ignored, _cover_name = _runtime_settings()
    if library_root is None:
        return True
    try:
        return root.parent.resolve() == library_root.resolve()
    except OSError:
        return root.parent == library_root


def _matching_cached_count(total: int) -> int:
    payload, _path = _ensure_loaded()
    _history_dir, library_root, ignored, cover_name = _runtime_settings()
    if library_root is None or total <= 0:
        return 0
    key = _inventory_key(ignored, cover_name)
    artists = payload.get("artists", {})
    if not isinstance(artists, dict):
        return 0

    count = 0
    try:
        with os.scandir(library_root) as iterator:
            entries = list(iterator)
    except OSError:
        return 0
    for item in entries:
        name = item.name
        if name.startswith("."):
            continue
        if any(fnmatch.fnmatchcase(name.casefold(), value.casefold()) for value in ignored):
            continue
        try:
            if item.is_symlink() or not item.is_dir(follow_symlinks=False):
                continue
        except OSError:
            continue
        cached = artists.get(str(Path(item.path)))
        if (
            isinstance(cached, dict)
            and cached.get("complete") is True
            and cached.get("inventory_key") == key
        ):
            count += 1
    return min(total, count)


def install_core_patch(core: ModuleType) -> None:
    if getattr(core, "_splined_status_cache_installed", False):
        return
    original_inventory = core.inventory
    original_emit_ui = core.emit_ui

    def cached_inventory(
        root: Path,
        ignored_subs: list[str],
        configured_file_name: str = "cover",
        *,
        fingerprint_paths: set[str] | None = None,
        progress: Callable[[int, int], None] | None = None,
        workers: int = 8,
        cancelled: Callable[[], bool] | None = None,
    ):
        root = Path(root)
        if not _should_cache_status_inventory(root, workers, progress):
            return original_inventory(
                root,
                ignored_subs,
                configured_file_name,
                fingerprint_paths=fingerprint_paths,
                progress=progress,
                workers=workers,
                cancelled=cancelled,
            )

        payload, _path = _ensure_loaded()
        inventory_key = _inventory_key(ignored_subs, configured_file_name)
        fingerprints = fingerprint_paths or set()
        with _LOCK:
            artists = payload.setdefault("artists", {})
            entry = artists.get(str(root)) if isinstance(artists, dict) else None
        if (
            isinstance(entry, dict)
            and entry.get("complete") is True
            and entry.get("inventory_key") == inventory_key
        ):
            cached = _entry_albums(core, root, entry, fingerprints)
            if cached is not None:
                core.debug_log(
                    f"picker.status_cache.hit artist={str(root)!r} albums={len(cached)}"
                )
                return cached, []

        albums, ignored = original_inventory(
            root,
            ignored_subs,
            configured_file_name,
            fingerprint_paths=fingerprints,
            progress=progress,
            workers=workers,
            cancelled=cancelled,
        )
        entry = _cache_entry(root, albums, inventory_key)
        if entry["directories"]:
            global _DIRTY
            with _LOCK:
                payload.setdefault("artists", {})[str(root)] = entry
                _DIRTY += 1
                try:
                    _save_locked(force=False)
                except (OSError, TypeError, ValueError):
                    pass
        core.debug_log(
            f"picker.status_cache.miss artist={str(root)!r} albums={len(albums)}"
        )
        return albums, ignored

    def cached_emit_ui(event: str, **payload: Any) -> None:
        global _CURRENT_BASELINE, _CURRENT_TOTAL
        if event == "folder_status_progress":
            try:
                total = max(0, int(payload.get("total", 0) or 0))
                processed = max(0, int(payload.get("processed", 0) or 0))
            except (TypeError, ValueError):
                total = processed = 0
            if processed == 0 and total > 0 and not bool(payload.get("done", False)):
                _CURRENT_TOTAL = total
                _CURRENT_BASELINE = _matching_cached_count(total)
            baseline = _CURRENT_BASELINE if total == _CURRENT_TOTAL else 0
            effective = min(total, max(processed, baseline)) if total else processed
            payload["processed"] = effective
            payload["percent"] = effective / total * 100.0 if total else 100.0
            if total and baseline >= total:
                # A complete prior snapshot makes Select Media immediately
                # usable. Filesystem-difference validation continues in the
                # normal status worker and updates changed Artists in place.
                payload["done"] = True
        original_emit_ui(event, **payload)

    core.inventory = cached_inventory
    core.emit_ui = cached_emit_ui
    core._splined_status_cache_installed = True


def _patch_pyratatui_line() -> None:
    """Accept text-only blank lines across pyratatui binding versions."""
    try:
        import pyratatui
    except ImportError:
        return
    original = pyratatui.Line
    if getattr(original, "_splined_compatible", False):
        return

    def compatible_line(spans: Any = None, *args: Any, **kwargs: Any):
        if spans is None:
            spans = []
        elif isinstance(spans, str):
            spans = [pyratatui.Span(spans)]
        return original(spans, *args, **kwargs)

    compatible_line._splined_compatible = True  # type: ignore[attr-defined]
    pyratatui.Line = compatible_line


class _SplinedLoader(importlib.abc.Loader):
    def __init__(self, wrapped: importlib.abc.Loader) -> None:
        self.wrapped = wrapped

    def create_module(self, spec):  # type: ignore[no-untyped-def]
        creator = getattr(self.wrapped, "create_module", None)
        return creator(spec) if callable(creator) else None

    def exec_module(self, module: ModuleType) -> None:
        self.wrapped.exec_module(module)
        install_core_patch(module)


class _SplinedFinder(importlib.abc.MetaPathFinder):
    def find_spec(self, fullname: str, path, target=None):  # type: ignore[no-untyped-def]
        if fullname != "splined":
            return None
        spec = importlib.machinery.PathFinder.find_spec(fullname, path)
        if spec is None or spec.loader is None:
            return None
        spec.loader = _SplinedLoader(spec.loader)
        try:
            sys.meta_path.remove(self)
        except ValueError:
            pass
        return spec


def install() -> None:
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True
    _patch_pyratatui_line()
    existing = sys.modules.get("splined")
    if isinstance(existing, ModuleType):
        install_core_patch(existing)
        return
    sys.meta_path.insert(0, _SplinedFinder())

"""Ownership-aware lifecycle for disposable WebGUI operation files."""

from __future__ import annotations

from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4
import json
import logging
import os
import shutil
import socket
import threading


LOGGER = logging.getLogger("uvicorn.error")
_RECOVERY_ROOTS = (
    "web-candidates",
    "comp-work",
    "prepared-previews",
    "prepared-comp",
    "uploads",
    "web-images",
)
_CONVERSION_MARKER_SUFFIX = ".splined-conversion.json"


def _process_is_active(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        import ctypes

        process = ctypes.windll.kernel32.OpenProcess(0x1000, False, pid)
        if not process:
            # Access denied means an existing protected process; an invalid
            # parameter means the PID no longer exists.
            return ctypes.windll.kernel32.GetLastError() == 5
        try:
            exit_code = ctypes.c_ulong()
            return bool(
                ctypes.windll.kernel32.GetExitCodeProcess(
                    process, ctypes.byref(exit_code)
                )
                and exit_code.value == 259
            )
        finally:
            ctypes.windll.kernel32.CloseHandle(process)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except (OSError, PermissionError):
        # An inaccessible process is still an active owner.
        return True
    return True


class TemporaryWorkspaces:
    """Create, track, and recover only SPLINED-owned temporary directories."""

    def __init__(self, root: Path, *, stale_after_seconds: int = 24 * 60 * 60):
        self.root = root.resolve()
        self.session_id = f"{os.getpid()}-{uuid4().hex}"
        self.hostname = socket.gethostname()
        self.stale_after_seconds = stale_after_seconds
        self._owned: set[Path] = set()
        self._session_workspaces: dict[str, Path] = {}
        self._lock = threading.RLock()
        self.root.mkdir(parents=True, exist_ok=True)
        self.recover_abandoned()

    @staticmethod
    def _remove(path: Path, reason: str) -> bool:
        try:
            shutil.rmtree(path)
            LOGGER.info("Removed SPLINED temporary workspace path=%s reason=%s", path, reason)
            return True
        except FileNotFoundError:
            return True
        except OSError as error:
            LOGGER.warning(
                "Unable to remove SPLINED temporary workspace path=%s reason=%s error=%s",
                path,
                reason,
                error,
            )
            return False

    def _write_owner(self, path: Path) -> None:
        owner = {
            "application": "SPLINED",
            "kind": "disposable-workspace",
            "session_id": self.session_id,
            "pid": os.getpid(),
            "hostname": self.hostname,
            "created_at": datetime.now(timezone.utc).isoformat(),
        }
        (path / ".splined-owner.json").write_text(
            json.dumps(owner, sort_keys=True), encoding="utf-8"
        )

    def session_workspace(self, kind: str) -> Path:
        """Return one session-owned directory retained until orderly shutdown."""
        if kind not in _RECOVERY_ROOTS:
            raise ValueError(f"Unknown SPLINED temporary workspace kind: {kind}")
        with self._lock:
            existing = self._session_workspaces.get(kind)
            if existing is not None and existing.is_dir():
                return existing
            path = self.root / kind / self.session_id
            path.mkdir(parents=True, exist_ok=False)
            self._write_owner(path)
            self._session_workspaces[kind] = path
            self._owned.add(path)
            return path

    @contextmanager
    def workspace(self, kind: str):
        if kind not in _RECOVERY_ROOTS:
            raise ValueError(f"Unknown SPLINED temporary workspace kind: {kind}")
        path = self.root / kind / f"{self.session_id}-{uuid4().hex}"
        path.mkdir(parents=True, exist_ok=False)
        self._write_owner(path)
        with self._lock:
            self._owned.add(path)
        try:
            yield path
        finally:
            if self._remove(path, "operation-finished"):
                with self._lock:
                    self._owned.discard(path)

    def recover_abandoned(self) -> int:
        """Remove dead-session and recognized legacy workspaces, never live owners."""
        recovered = 0
        for kind in _RECOVERY_ROOTS:
            parent = self.root / kind
            if not parent.is_dir():
                continue
            for path in parent.iterdir():
                if path.is_file():
                    if not _legacy_owned_file(kind, path):
                        continue
                    try:
                        path.unlink()
                        recovered += 1
                        LOGGER.info(
                            "Removed legacy SPLINED temporary file path=%s reason=startup-recovery",
                            path,
                        )
                    except OSError as error:
                        LOGGER.warning(
                            "Unable to remove legacy SPLINED temporary file path=%s "
                            "reason=startup-recovery error=%s",
                            path,
                            error,
                        )
                    continue
                if not path.is_dir():
                    continue
                marker = path / ".splined-owner.json"
                owner = None
                try:
                    owner = json.loads(marker.read_text(encoding="utf-8"))
                except (FileNotFoundError, OSError, ValueError, TypeError):
                    pass
                active = False
                local_owner = False
                if isinstance(owner, dict) and owner.get("application") == "SPLINED":
                    local_owner = str(owner.get("hostname", "")) == self.hostname
                    active = local_owner and _process_is_active(
                        int(owner.get("pid", 0) or 0)
                    )
                # A marker proves ownership. Pre-marker releases used exact UUID
                # directory names under these application-owned disposable roots;
                # retaining them defeats crash recovery and cannot protect a live
                # owner because the old layout recorded no ownership at all.
                valid_owner = (
                    isinstance(owner, dict)
                    and owner.get("application") == "SPLINED"
                    and owner.get("kind") == "disposable-workspace"
                )
                # A valid marker from another host may still represent live
                # work on shared storage. Preserve it because this backend
                # cannot prove that remote process has exited.
                eligible = (valid_owner and local_owner and not active) or (
                    not valid_owner and _legacy_owned_directory(kind, path)
                )
                if eligible and self._remove(path, "startup-recovery"):
                    recovered += 1
        return recovered

    def close(self) -> None:
        with self._lock:
            owned = list(self._owned)
        for path in owned:
            if self._remove(path, "orderly-shutdown"):
                with self._lock:
                    self._owned.discard(path)


def _legacy_owned_file(kind: str, path: Path) -> bool:
    """Recognize only pre-owner-marker files created by known SPLINED caches."""
    stem = path.stem.casefold()
    suffix = path.suffix.casefold()
    if kind == "web-images":
        return (
            suffix in {".jpg", ".jpeg", ".png", ".webp"}
            and len(stem) == 64
            and all(character in "0123456789abcdef" for character in stem)
        )
    if kind in {"uploads", "prepared-previews"}:
        return (
            suffix == ".png"
            and len(stem) == 32
            and all(character in "0123456789abcdef" for character in stem)
        )
    return False


def _legacy_owned_directory(kind: str, path: Path) -> bool:
    """Recognize exact pre-marker UUID operation directories."""
    if kind not in {
        "web-candidates",
        "comp-work",
        "prepared-previews",
        "prepared-comp",
        "uploads",
        "web-images",
    }:
        return False
    name = path.name.casefold()
    return len(name) == 32 and all(character in "0123456789abcdef" for character in name)


def conversion_marker_path(path: Path) -> Path:
    return path.with_name(path.name + _CONVERSION_MARKER_SUFFIX)


def mark_conversion_backup(path: Path, *, committed: bool) -> None:
    """Record whether an owned recovery backup is safe to discard."""
    marker = {
        "application": "SPLINED",
        "kind": "conversion-backup",
        "status": "committed" if committed else "recovery-required",
        "path": path.name,
        "pid": os.getpid(),
        "hostname": socket.gethostname(),
        "updated_at": datetime.now(timezone.utc).isoformat(),
    }
    conversion_marker_path(path).write_text(
        json.dumps(marker, sort_keys=True), encoding="utf-8"
    )


def discard_committed_conversion(path: Path) -> bool:
    """Delete one committed backup and its marker, retaining both on failure."""
    marker = conversion_marker_path(path)
    try:
        path.unlink(missing_ok=True)
        marker.unlink(missing_ok=True)
        return True
    except OSError as error:
        LOGGER.warning(
            "Unable to remove committed SPLINED conversion intermediate path=%s error=%s",
            path,
            error,
        )
        return False


def recover_conversion_intermediates(cache: Path) -> int:
    """Remove committed backups and unusable pre-marker conversion leftovers."""
    root = cache.resolve() / "embedded-backups"
    if not root.is_dir() or root.is_symlink():
        return 0
    recovered = 0
    for path in root.iterdir():
        if (
            not path.is_file()
            or path.suffix.casefold() != ".image"
            or len(path.stem) != 64
            or any(character not in "0123456789abcdef" for character in path.stem.casefold())
        ):
            continue
        try:
            marker = json.loads(
                conversion_marker_path(path).read_text(encoding="utf-8")
            )
        except FileNotFoundError:
            # Older builds created one-way hashed backups with no target-path
            # metadata. They cannot be restored and are safe to discard.
            try:
                path.unlink()
                recovered += 1
                LOGGER.info(
                    "Removed legacy SPLINED conversion intermediate path=%s "
                    "reason=missing-recovery-metadata",
                    path,
                )
            except OSError as error:
                LOGGER.warning(
                    "Unable to remove legacy SPLINED conversion intermediate "
                    "path=%s error=%s",
                    path,
                    error,
                )
            continue
        except (OSError, ValueError, TypeError):
            continue
        eligible = (
            isinstance(marker, dict)
            and marker.get("application") == "SPLINED"
            and marker.get("kind") == "conversion-backup"
            and marker.get("status") == "committed"
            and marker.get("path") == path.name
        )
        if eligible and discard_committed_conversion(path):
            recovered += 1
            LOGGER.info(
                "Removed abandoned SPLINED conversion intermediate path=%s reason=committed-retry",
                path,
            )
    # If deletion of the image succeeded but marker deletion was interrupted,
    # remove the now-useless committed marker on the next startup as well.
    for marker_path in root.glob("*" + _CONVERSION_MARKER_SUFFIX):
        image_path = marker_path.with_name(
            marker_path.name.removesuffix(_CONVERSION_MARKER_SUFFIX)
        )
        if image_path.exists():
            continue
        try:
            marker = json.loads(marker_path.read_text(encoding="utf-8"))
        except (OSError, ValueError, TypeError):
            continue
        eligible = (
            isinstance(marker, dict)
            and marker.get("application") == "SPLINED"
            and marker.get("kind") == "conversion-backup"
            and marker.get("status") == "committed"
            and marker.get("path") == image_path.name
        )
        if eligible:
            try:
                marker_path.unlink()
            except OSError as error:
                LOGGER.warning(
                    "Unable to remove committed SPLINED conversion marker path=%s error=%s",
                    marker_path,
                    error,
                )
    return recovered

from __future__ import annotations

from pathlib import Path
from tempfile import TemporaryDirectory
import asyncio
import json
import os
import socket
import time
import unittest

from webgui.tempfiles import (
    TemporaryWorkspaces,
    mark_conversion_backup,
    recover_conversion_intermediates,
)


class TemporaryWorkspaceTests(unittest.TestCase):
    def test_operation_cleanup_runs_after_success_and_failure(self):
        with TemporaryDirectory() as directory:
            manager = TemporaryWorkspaces(Path(directory), stale_after_seconds=60)
            with manager.workspace("web-candidates") as path:
                (path / "candidate.img").write_bytes(b"owned")
                successful = path
            self.assertFalse(successful.exists())
            failed = None
            with self.assertRaisesRegex(RuntimeError, "fixture"):
                with manager.workspace("comp-work") as path:
                    failed = path
                    (path / "candidate.img").write_bytes(b"owned")
                    raise RuntimeError("fixture")
            self.assertFalse(failed.exists())

    def test_operation_cleanup_runs_after_cancellation(self):
        with TemporaryDirectory() as directory:
            manager = TemporaryWorkspaces(Path(directory), stale_after_seconds=60)
            cancelled = None
            with self.assertRaises(asyncio.CancelledError):
                with manager.workspace("comp-work") as path:
                    cancelled = path
                    (path / "candidate.img").write_bytes(b"owned")
                    raise asyncio.CancelledError()
            self.assertFalse(cancelled.exists())

    def test_startup_preserves_live_owner_and_recovers_dead_owner(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            live = root / "comp-work" / "live"
            dead = root / "comp-work" / "dead"
            for path, pid in ((live, os.getpid()), (dead, 999_999_999)):
                path.mkdir(parents=True)
                (path / ".splined-owner.json").write_text(
                    json.dumps(
                        {
                            "application": "SPLINED",
                            "kind": "disposable-workspace",
                            "hostname": __import__("socket").gethostname(),
                            "pid": pid,
                        }
                    ),
                    encoding="utf-8",
                )
            manager = TemporaryWorkspaces(root, stale_after_seconds=3600)
            self.assertTrue(live.exists())
            self.assertFalse(dead.exists())
            manager.close()

    def test_orderly_close_retries_owned_workspace_and_preserves_remote_owner(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            manager = TemporaryWorkspaces(root, stale_after_seconds=0)
            owned_context = manager.workspace("prepared-previews")
            owned = owned_context.__enter__()
            remote = root / "comp-work" / "remote"
            remote.mkdir(parents=True)
            (remote / ".splined-owner.json").write_text(
                json.dumps(
                    {
                        "application": "SPLINED",
                        "kind": "disposable-workspace",
                        "hostname": socket.gethostname() + "-other",
                        "pid": 1,
                    }
                ),
                encoding="utf-8",
            )
            manager.recover_abandoned()
            self.assertTrue(remote.exists())
            manager.close()
            self.assertFalse(owned.exists())
            owned_context.__exit__(None, None, None)

    def test_session_workspace_is_reused_and_removed_on_close(self):
        with TemporaryDirectory() as directory:
            manager = TemporaryWorkspaces(Path(directory), stale_after_seconds=60)
            first = manager.session_workspace("web-images")
            second = manager.session_workspace("web-images")
            self.assertEqual(first, second)
            (first / ("a" * 64 + ".png")).write_bytes(b"fixture")
            manager.close()
            self.assertFalse(first.exists())

    def test_startup_recovers_only_known_legacy_temp_files(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            managed = root / "uploads" / ("a" * 32 + ".png")
            unrelated = root / "uploads" / "source.png"
            managed.parent.mkdir(parents=True)
            managed.write_bytes(b"managed")
            unrelated.write_bytes(b"keep")
            old = time.time() - 120
            os.utime(managed, (old, old))
            os.utime(unrelated, (old, old))
            TemporaryWorkspaces(root, stale_after_seconds=60).close()
            self.assertFalse(managed.exists())
            self.assertTrue(unrelated.exists())

    def test_conversion_recovery_removes_only_committed_owned_backups(self):
        with TemporaryDirectory() as directory:
            cache = Path(directory)
            root = cache / "embedded-backups"
            root.mkdir()
            orphan = root / ("a" * 64 + ".image")
            recent = root / ("b" * 64 + ".image")
            unrelated = root / "source.image"
            for path in (orphan, recent, unrelated):
                path.write_bytes(b"fixture")
            mark_conversion_backup(orphan, committed=True)
            mark_conversion_backup(recent, committed=False)
            self.assertEqual(recover_conversion_intermediates(cache), 1)
            self.assertFalse(orphan.exists())
            self.assertTrue(recent.exists())
            self.assertTrue(unrelated.exists())

    def test_conversion_recovery_removes_abandoned_committed_marker(self):
        with TemporaryDirectory() as directory:
            cache = Path(directory)
            root = cache / "embedded-backups"
            root.mkdir()
            path = root / ("c" * 64 + ".image")
            path.write_bytes(b"fixture")
            mark_conversion_backup(path, committed=True)
            marker = path.with_name(path.name + ".splined-conversion.json")
            path.unlink()
            self.assertEqual(recover_conversion_intermediates(cache), 0)
            self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main()

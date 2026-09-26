#!/usr/bin/env python3
"""SPLINED Python/TUI specification and acceptance harness.

This runner validates documented product contracts and then delegates to the
existing regression suites. It is intentionally separate from application
behavior: adding or changing a specification check must not silently change the
runtime being checked.

Exit status:
  0 = no FAIL results
  1 = one or more FAIL results
  2 = harness usage/internal error
"""

from __future__ import annotations

import argparse
import contextlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from typing import Callable, Iterable

ROOT = Path(__file__).resolve().parents[1]
PYTHON_DIR = ROOT / "python"
if str(PYTHON_DIR) not in sys.path:
    sys.path.insert(0, str(PYTHON_DIR))

import splined  # noqa: E402
from tui.library import (  # noqa: E402
    AlbumItem,
    AlbumStatus,
    ArtistStatus,
    LibraryModel,
    artist_status,
)


@dataclass(frozen=True)
class Result:
    spec_id: str
    status: str
    summary: str
    detail: str = ""


class Audit:
    def __init__(self, *, verbose: bool = false) -> None:
        self.verbose = verbose
        self.results: list[Result] = []

    def add(self, spec_id: str, status: str, summary: str, detail: str = "") -> None:
        self.results.append(Result(spec_id, status, summary, detail))

    def check(self, spec_id: str, summary: str, fn: Callable[[], None]) -> None:
        try:
            fn()
        except AssertionError as exc:
            self.add(spec_id, "FAIL", summary, str(exc) or "assertion failed")
        except Exception as exc:
            self.add(spec_id, "FAIL", summary, f"{type(exc).__name__}: {exc}")
        else:
            self.add(spec_id, "PASS", summary)

    def warn(self, spec_id: str, summary: str, detail: str) -> None:
        self.add(spec_id, "WARN", summary, detail)

    def skip(self, spec_id: str, summary: str, detail: str) -> None:
        self.add(spec_id, "SKIP", summary, detail)

    def command(
        self,
        spec_id: str,
        summary: str,
        argv: list[str],
        *,
        cwd: Path = ROOT,
        timeout: int = 300,
    ) -> None:
        if shutil.which(argv[0]) is None:
            self.skip(spec_id, summary, f"required executable unavailable: {argv[0]}")
            return
        try:
            completed = subprocess.run(
                argv,
                cwd=cwd,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=timeout,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            self.add(spec_id, "FAIL", summary, f"timed out after {exc.timeout}s")
            return

        output = completed.stdout.strip()
        if completed.returncode == 0:
            self.add(spec_id, "PASS", summary, output if self.verbose else "")
            return

        tail = "\n".join(output.splitlines()[-30:]) if output else ""
        detail = f"exit={completed.returncode}"
        if tail:
            detail += "\n" + tail
        self.add(spec_id, "FAIL", summary, detail)

    @property
    def failures(self) -> int:
        return sum(result.status == "FAIL" for result in self.results)

    def render(self) -> None:
        width = max((len(result.spec_id) for result in self.results), default=8)
        print("SPLINED SPECIFICATION AUDIT")
        print("=" * 78)
        for result in self.results:
            print(f"[{result.status:4}] {result.spec_id:<{width}}  {result.summary}")
            if result.detail:
                for line in result.detail.splitlines():
                    print(f"       {' ' * width}  {line}")
        print("=" * 78)
        counts = {
            status: sum(result.status == status for result in self.results)
            for status in ("PASS", "FAIL", "WARN", "SKIP")
        }
        result = "FAIL" if counts["FAIL"] else "PASS"
        totals = "  ".join(f"{name}={count}" for name, count in counts.items())
        print(f"RESULT: {result}  {totals}")


REQUIRED_DOC_ANCHORS: dict[str, tuple[str, ...]] = {
    "docs/config-v5-reference.md": (
        "scan_mode_timeout",
        "history.enabled",
        "[aisplined]",
    ),
    "docs/history-retention-bypass-timeout.md": (
        "Manual reprocessing",
        "persistent history / bypass / timeout authority",
        "Orange album",
    ),
    "docs/media-filter-status-colors.md": (
        "Select [ALL]",
        "Select [FILTERED]",
        "filtering never auto-adds an unchecked Album",
        "Orange albums may be deliberately reselected",
    ),
    "docs/ratatui-tui.md": (
        "complete persistent Select Media snapshot",
        "validate filesystem changes in the background",
        "AUTO SELECTED uses all checked Albums",
    ),
    "docs/source-policies-range-types.md": (
        "Global Resolution Range",
        "Source Override",
    ),
}

CONFIG_EXPECTED_KEYS: dict[str, tuple[str, ...]] = {
    "": ("config_version", "mode", "verbosity"),
    "library": ("music_library", "ignored_subs"),
    "scan": (
        "scan_library_dir",
        "scan_mode",
        "library_scan",
        "scan_mode_timeout",
        "cache_dir",
        "log_dir",
        "history_dir",
    ),
    "samples": ("sample_write",),
    "credentials": ("credential_dir",),
    "output": (
        "preserve_file",
        "file_formats",
        "file_name",
        "square",
        "square_mode",
        "square_round_to",
        "upscale_below_ideal",
        "evaluate_final_image",
    ),
    "range": ("min", "ideal", "max", "ladder"),
    "sources": ("cover_sources", "exclude_cover_sources"),
    "logging": ("retention_days",),
    "history": ("enabled", "retention_days"),
    "aisplined": (
        "enabled",
        "endpoint",
        "minimum_short_side",
        "allow_below_minimum_override",
    ),
}

HELP_EXPECTED_TOKENS: dict[str, str] = {
    "config_version": "Config version",
    "mode": "Mode",
    "verbosity": "Verbosity",
    "library.music_library": "Library:",
    "library.ignored_subs": "Ignore Sub-Directories:",
    "scan.scan_library_dir": "Scan Directory:",
    "scan.scan_mode": "scan_mode",
    "scan.library_scan": "library_scan",
    "scan.scan_mode_timeout": "scan_mode_timeout",
    "scan.cache_dir": "Cache Directory",
    "scan.log_dir": "Log Directory",
    "scan.history_dir": "History Directory",
    "samples.sample_write": "sample_write",
    "credentials.credential_dir": "Credential Directory",
    "output.preserve_file": "preserve_file",
    "output.file_formats": "file_formats",
    "output.file_name": "file_name",
    "output.square": "square",
    "output.square_mode": "square_mode",
    "output.square_round_to": "square_round_to",
    "output.upscale_below_ideal": "upscale_below_ideal",
    "output.evaluate_final_image": "evaluate_final_image",
    "range.min": "range.min",
    "range.ideal": "range.ideal",
    "range.max": "range.max",
    "range.ladder": "range.ladder",
    "sources.cover_sources": "--cover-sources",
    "sources.exclude_cover_sources": "--exclude-cover-sources",
    "logging.retention_days": "logging.retention_days",
    "history.enabled": "history.enabled",
    "history.retention_days": "history.retention_days",
    "aisplined.enabled": "aisplined.enabled",
    "aisplined.endpoint": "aisplined.endpoint",
    "aisplined.minimum_short_side": "aisplined.minimum_short_side",
    "aisplined.allow_below_minimum_override": "aisplined.allow_below_minimum_override",
}


def _load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def _missing_config_keys(config: dict[str, object]) -> list[str]:
    missing: list[str] = []
    for section_name, keys in CONFIG_EXPECTED_KEYS.items():
        table: object = config if not section_name else config.get(section_name)
        if not isinstance(table, dict):
            missing.extend(
                f"{section_name}.{key}" if section_name else key for key in keys
            )
            continue
        for key in keys:
            if key not in table:
                missing.append(f"{section_name}.{key}" if section_name else key)
    return missing


def check_docs(audit: Audit) -> None:
    def anchors() -> None:
        missing: list[str] = []
        for relative, phrases in REQUIRED_DOC_ANCHORS.items():
            path = ROOT / relative
            assert path.is_file(), f"missing document: {relative}"
            body = path.read_text(encoding="utf-8")
            for phrase in phrases:
                if phrase.casefold() not in body.casefold():
                    missing.append(f"{relative}: {phrase}")
        assert not missing, "missing contract anchors:\n" + "\n".join(missing)

    audit.check("DOC-001", "required /docs contract anchors are present", anchors)

    history_body = (ROOT / "docs/history-retention-bypass-timeout.md").read_text(
        encoding="utf-8"
    )
    media_body = (ROOT / "docs/media-filter-status-colors.md").read_text(
        encoding="utf-8"
    )
    if (
        "retained a successful prior processing state" in history_body
        and "detected local artwork state" in media_body
    ):
        audit.warn(
            "DOC-002",
            "Processed/Orange authority needs explicit reconciliation",
            "The history document defines Orange around retained successful processing "
            "history while the media-status document also permits Windows-equivalent "
            "detected local artwork state. Resolve this against intended Windows/Python "
            "authority before changing implementation behavior.",
        )
    else:
        audit.add("DOC-002", "PASS", "Processed/Orange authority is textually consistent")


def check_config(audit: Audit) -> None:
    examples = (
        ROOT / "config.example.toml",
        ROOT / "docker/config.example.toml",
        ROOT / "windows/config.example.toml",
    )

    def examples_validate() -> None:
        errors: list[str] = []
        for path in examples:
            cfg = _load_toml(path)
            if cfg.get("config_version") != 5:
                errors.append(f"{path.relative_to(ROOT)}: config_version != 5")
                continue
            try:
                splined.validate_config_v5(cfg)
            except Exception as exc:
                errors.append(f"{path.relative_to(ROOT)}: {exc}")
        assert not errors, "\n".join(errors)

    audit.check("CFG-001", "all public Config v5 examples validate", examples_validate)

    def examples_cover_surface() -> None:
        errors: list[str] = []
        for path in examples:
            missing = _missing_config_keys(_load_toml(path))
            if missing:
                errors.append(f"{path.relative_to(ROOT)} missing: " + ", ".join(missing))
        assert not errors, "\n".join(errors)

    audit.check(
        "CFG-002",
        "public examples cover the documented Config v5 surface",
        examples_cover_surface,
    )


def check_timeout(audit: Audit) -> None:
    def disabled_forms() -> None:
        for raw in (False, 0, 0.0, "off"):
            actual = splined.scan_timeout_hours({"scan": {"scan_mode_timeout": raw}})
            assert actual == 0.0, f"{raw!r} resolved to {actual}, expected 0.0"
        actual = splined.scan_timeout_hours({"scan": {"scan_mode_timeout": 24}})
        assert actual == 24.0, f"24 resolved to {actual}"

    audit.check(
        "TIMEOUT-001",
        "false/off/0 disable scan timeout; numeric hours remain numeric",
        disabled_forms,
    )


def check_status_and_selection(audit: Audit) -> None:
    def precedence() -> None:
        u = AlbumItem("/U", "A", "U", AlbumStatus.UNPROCESSED)
        p = AlbumItem("/P", "A", "P", AlbumStatus.PROCESSED)
        b = AlbumItem("/B", "A", "B", AlbumStatus.BYPASSED)
        t = AlbumItem("/T", "A", "T", AlbumStatus.TIMEOUT)
        assert artist_status([u]) is ArtistStatus.UNPROCESSED
        assert artist_status([p]) is ArtistStatus.COMPLETE
        assert artist_status([p, u]) is ArtistStatus.PARTIAL
        assert artist_status([t]) is ArtistStatus.COMPLETE
        assert artist_status([t, u]) is ArtistStatus.PARTIAL
        assert artist_status([p, b, u]) is ArtistStatus.CONTAINS_BYPASS

    audit.check(
        "STATUS-001",
        "artist status precedence is Blue > Green > Purple > White",
        precedence,
    )

    def manual_processed() -> None:
        item = AlbumItem("/music/A/P", "A", "Processed", AlbumStatus.PROCESSED)
        model = LibraryModel("/music", [item])
        result = model.toggle_album(item)
        assert result == "selected", f"toggle returned {result!r}"
        assert item.selected, "Processed Album was not manually selectable"

    audit.check(
        "SELECT-001",
        "Orange/Processed Album can be manually reselected without clearing history",
        manual_processed,
    )

    def protected_states() -> None:
        bypassed = AlbumItem("/music/A/B", "A", "Bypassed", AlbumStatus.BYPASSED)
        timed = AlbumItem("/music/A/T", "A", "Timed", AlbumStatus.TIMEOUT)
        model = LibraryModel("/music", [bypassed, timed])
        assert model.toggle_album(bypassed) == "bypass-confirmation-required"
        assert not bypassed.selected
        assert model.toggle_album(timed) == "timeout-active"
        assert not timed.selected
        assert model.toggle_album(bypassed, bypass_override=True) == "selected"
        assert bypassed.selected and bypassed.bypass_override

    audit.check(
        "SELECT-002",
        "Bypass/timeout protection requires deliberate supported actions",
        protected_states,
    )

    def filtered_launch() -> None:
        kept = AlbumItem(
            "/music/A/Keep", "A", "Keep Me", AlbumStatus.UNPROCESSED, selected=True
        )
        unchecked = AlbumItem(
            "/music/A/Other", "A", "Other", AlbumStatus.UNPROCESSED, selected=False
        )
        model = LibraryModel("/music", [kept, unchecked])
        before = [item.selected for item in model.albums]
        model.set_filters(album="Keep")
        assert before == [item.selected for item in model.albums], "filter mutated selection"
        payload = model.selection_payload("filtered-read")
        assert payload["selected"] == [kept.path], payload["selected"]

    audit.check(
        "SELECT-003",
        "filtering preserves selection; filtered launch is visible ∩ checked",
        filtered_launch,
    )

    def select_all() -> None:
        white = AlbumItem("/music/A/U", "A", "U", AlbumStatus.UNPROCESSED)
        orange = AlbumItem("/music/A/P", "A", "P", AlbumStatus.PROCESSED)
        red = AlbumItem("/music/A/B", "A", "B", AlbumStatus.BYPASSED)
        purple = AlbumItem("/music/A/T", "A", "T", AlbumStatus.TIMEOUT)
        model = LibraryModel("/music", [white, orange, red, purple])
        model.select_all(filtered=False)
        selected = {item.path for item in model.albums if item.selected}
        assert selected == {white.path}, selected

    audit.check(
        "SELECT-004",
        "normal Select ALL auto-selects White only",
        select_all,
    )

    if LibraryModel.__dataclass_fields__["select_new"].default is True:
        audit.warn(
            "SELECT-005",
            "initial/new-Album auto-selection policy is not explicit enough in /docs",
            "LibraryModel.select_new currently defaults true. The docs define explicit "
            "selection actions and path-exact launch behavior, but do not unambiguously "
            "state whether a newly opened complete picker begins with White Albums "
            "prechecked. Resolve against intended Windows behavior before changing it.",
        )
    else:
        audit.add("SELECT-005", "PASS", "initial/new-Album selection is non-automatic")


def check_help(audit: Audit) -> None:
    cfg_path = ROOT / "docker/config.example.toml"
    cfg = _load_toml(cfg_path)

    def surface() -> None:
        stream = io.StringIO()
        previous = os.environ.get("SPLINED_COLOR")
        os.environ["SPLINED_COLOR"] = "always"
        try:
            with contextlib.redirect_stdout(stream):
                splined.print_help(cfg_path, cfg)
        finally:
            if previous is None:
                os.environ.pop("SPLINED_COLOR", None)
            else:
                os.environ["SPLINED_COLOR"] = previous
        output = stream.getvalue()
        missing = [
            f"{key} ({token})"
            for key, token in HELP_EXPECTED_TOKENS.items()
            if token.casefold() not in output.casefold()
        ]
        assert not missing, "help omits Config v5 values:\n" + "\n".join(missing)

    audit.check(
        "HELP-001",
        "-h exposes the complete user-facing Config v5 surface",
        surface,
    )


def check_targeted_regressions(audit: Audit) -> None:
    tests: tuple[tuple[str, str, str], ...] = (
        (
            "CACHE-001",
            "warm first render is SQLite-only before filesystem validation",
            "test_tui_inventory.CompletePickerSnapshotTests."
            "test_warm_first_render_is_sqlite_only_when_media_access_fails",
        ),
        (
            "CACHE-002",
            "failed promotion retains the last complete snapshot",
            "test_tui_inventory.CompletePickerSnapshotTests."
            "test_failed_generation_promotion_retains_last_complete_snapshot",
        ),
        (
            "CACHE-003",
            "background validation promotes one coherent snapshot",
            "test_tui_inventory.CompletePickerSnapshotTests."
            "test_background_validation_promotes_one_coherent_snapshot",
        ),
        (
            "CACHE-004",
            "same-session return avoids SQLite/filesystem reinitialization",
            "test_tui_inventory.CompletePickerSnapshotTests."
            "test_same_session_return_uses_memory_without_sqlite_or_filesystem",
        ),
        (
            "SESSION-001",
            "final report returns to preserved resident picker state",
            "test_tui_session_corrections.MultiBatchSessionTests."
            "test_windows_style_report_returns_to_preserved_library_state",
        ),
        (
            "REPORT-001",
            "final report pauses, scrolls, and waits for explicit exit",
            "test_tui_session_corrections.MultiBatchSessionTests."
            "test_report_pauses_scrolls_and_exit_waits_for_worker_result",
        ),
    )
    for spec_id, summary, target in tests:
        audit.command(
            spec_id,
            summary,
            [sys.executable, "-m", "unittest", "-q", target],
            cwd=PYTHON_DIR,
            timeout=120,
        )


def check_version(audit: Audit) -> None:
    version = str(splined.VERSION)
    if version == "1.0.9":
        audit.warn(
            "VERSION-001",
            "dev identifier is indistinguishable from released 1.0.9",
            "Establish a dev-version convention before final acceptance so -V proves "
            "which image is running.",
        )
    else:
        audit.add("VERSION-001", "PASS", f"dev identifier is distinct: {version}")


def run_full_suites(audit: Audit) -> None:
    audit.command(
        "PY-ALL",
        "complete Python unittest suite",
        [sys.executable, "-m", "unittest", "discover", "-s", "python", "-p", "test_*.py"],
        timeout=300,
    )
    audit.command(
        "PY-COMPILE",
        "Python compileall",
        [sys.executable, "-m", "compileall", "-q", "python"],
        timeout=120,
    )
    audit.command(
        "PY-PIP",
        "Python dependency consistency",
        [sys.executable, "-m", "pip", "check"],
        timeout=120,
    )
    audit.command(
        "RUST-TEST",
        "root Rust test suite",
        ["cargo", "test", "--locked"],
        timeout=600,
    )
    audit.command(
        "RUST-FMT",
        "root Rust formatting",
        ["cargo", "fmt", "--all", "--", "--check"],
        timeout=120,
    )

    extension = ROOT / "python/pyratatui_input"
    audit.command(
        "INPUT-TEST",
        "pyratatui input-extension tests",
        ["cargo", "test", "--locked"],
        cwd=extension,
        timeout=300,
    )
    audit.command(
        "INPUT-FMT",
        "pyratatui input-extension formatting",
        ["cargo", "fmt", "--all", "--", "--check"],
        cwd=extension,
        timeout=120,
    )
    audit.command(
        "INPUT-CLIPPY",
        "pyratatui input-extension clippy -D warnings",
        ["cargo", "clippy", "--locked", "--", "-D", "warnings"],
        cwd=extension,
        timeout=300,
    )

    audit.skip(
        "WIN-GUI",
        "Windows GUI self-tests",
        "Run windows/gui/TEST-WINDOWS-GUI.cmd on the Windows acceptance host; "
        "an unavailable platform is never reported as PASS.",
    )


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run SPLINED documented-contract and acceptance checks."
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--quick",
        action="store_true",
        help="Run contract checks plus focused regressions (default).",
    )
    mode.add_argument(
        "--full",
        action="store_true",
        help="Run quick checks plus complete Python/Rust suites.",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Show successful command output.",
    )
    return parser.parse_args(argv)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    audit = Audit(verbose=bool(args.verbose))

    check_docs(audit)
    check_config(audit)
    check_timeout(audit)
    check_status_and_selection(audit)
    check_help(audit)
    check_targeted_regressions(audit)
    check_version(audit)

    if args.full:
        run_full_suites(audit)

    audit.render()
    return 1 if audit.failures else 0


if __name__ == "__main__":
    raise SystemExit(main())

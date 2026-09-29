"""Advanced-runtime corrections for preferred ranking and TUI lifecycle.

This policy is deliberately installed after the shared ranking, SQLite, and TUI
policies.  It keeps three user-visible contracts together:

* ``(s) preferred`` is the candidate with the shortest projected distance from
  the configured Ideal; source-policy acceptance remains visible information
  and automatic non-interactive selection keeps its existing safeguards.
* every SQLite startup phase uses the same spectral progress rail as the
  one-time database build;
* the library status card is named ``ALBUM STATUS`` and terminal mouse/raw mode
  is restored defensively after every TUI operational run.
"""

from __future__ import annotations

import sys
from typing import Any

import splined_ranking_policy as ranking


_INSTALLED = False


def _distance_first_preferred(
    scan: Any,
    candidates: list[Any],
    cfg: dict[str, Any],
    format_order: list[str],
) -> Any | None:
    """Choose the visible/manual ``(s)`` candidate by projected distance.

    ``operational_fallback_suggested`` already implements the approved
    distance-first order with deterministic square, approval, resolution,
    provider, format, and identity tie-breaks.  Reusing it here changes only
    the interactive preferred candidate; policy-safe automatic selection
    continues to use the normal candidate key.
    """
    return ranking.operational_fallback_suggested(
        scan,
        candidates,
        cfg,
        format_order,
    )


def _is_sqlite_progress(payload: dict[str, Any]) -> bool:
    phase = str(payload.get("phase", "")).strip().casefold()
    status = str(payload.get("status", "")).strip().casefold()
    if phase in {"load", "status", "commit", "ready"}:
        return any(
            token in status
            for token in (
                "sqlite",
                "indexed",
                "select media",
                "folder status",
                "artist rows",
                "album rows",
            )
        )
    return False


def _patch_tui(module: Any) -> None:
    if getattr(module, "_splined_advanced_runtime_policy", False):
        return

    original_apply = module.TuiState.apply
    original_card = module.card

    def apply(state: Any, event: str, payload: dict[str, Any]) -> None:
        original_apply(state, event, payload)
        # The original spectral progress extension keyed only from
        # cache_build_start. Warm starts emit cache_progress directly, so mark
        # those SQLite phases as the same media-index surface.
        if event == "cache_progress" and _is_sqlite_progress(payload):
            state.cache_kind = "media-index"

    def card(
        theme: Any,
        title: str,
        semantic: Any,
        *args: Any,
        **kwargs: Any,
    ) -> Any:
        if str(title).strip().casefold() == "folder status":
            title = "ALBUM STATUS"
        return original_card(theme, title, semantic, *args, **kwargs)

    module.TuiState.apply = apply
    module.card = card
    module._splined_advanced_runtime_policy = True


def install(core: Any, scan: Any) -> None:
    """Install the final Advanced runtime correction layer."""
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    # The shared local-comparison prompt resolves this module global at call
    # time, so replacing it here updates every subsequent ``(s)`` decision
    # without weakening the separate automatic select_best path.
    ranking.operational_preferred_candidate = _distance_first_preferred

    original_run = core.run_operational_interface

    def run_operational_interface(args: Any, worker: Any) -> int:
        likely_tui = (
            not bool(getattr(args, "no_tui", False))
            and bool(getattr(sys.stdin, "isatty", lambda: False)())
            and bool(getattr(sys.stdout, "isatty", lambda: False)())
        )
        tui_module: Any | None = None
        if likely_tui:
            try:
                from tui import splined_tui

                tui_module = splined_tui
                _patch_tui(tui_module)
            except ImportError:
                tui_module = None

        try:
            return int(original_run(args, worker))
        finally:
            # A normal EventReader exit already disables mouse capture.  This
            # outer idempotent restore also covers exceptions/SystemExit and
            # prevents SGR mouse reports from being interpreted by the shell as
            # commands such as ``6M35`` after the alternate screen closes.
            if likely_tui and tui_module is not None:
                restore = getattr(tui_module, "emergency_terminal_restore", None)
                if callable(restore):
                    try:
                        restore()
                        core.debug_log("tui.terminal_restore.finalized")
                    except Exception as exc:
                        core.debug_log(
                            "tui.terminal_restore.error "
                            f"error={type(exc).__name__}: {exc}"
                        )

    core.run_operational_interface = run_operational_interface
    core._splined_advanced_runtime_policy_installed = True

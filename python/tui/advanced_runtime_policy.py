"""Advanced-runtime corrections for preferred ranking and TUI lifecycle.

This policy is deliberately installed after the shared ranking, SQLite, and TUI
policies. It keeps five user-visible contracts together:

* ``(s) preferred`` is the candidate with the shortest projected distance from
  the configured Ideal; source-policy acceptance remains visible information
  and automatic non-interactive selection keeps its existing safeguards.
* SQLite warm starts retain the compact ``LIBRARY CACHE`` panel but use the
  same spectral progress rail as the one-time database build.
* native terminal polling never monopolizes the Python interpreter while the
  SQLite worker hydrates the in-memory Artist/Album picker model.
* the library status card is named ``ALBUM STATUS``;
* terminal mouse/raw mode is restored defensively after every TUI operational
  run, and successful summaries containing ``Failed [0]`` are not mislabeled
  as runtime errors.
"""

from __future__ import annotations

import sys
import time
from typing import Any

import splined_media_build_policy as build_policy
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
    provider, format, and identity tie-breaks. Reusing it here changes only the
    interactive preferred candidate; policy-safe automatic selection continues
    to use the normal candidate key.
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
    if phase not in {"load", "status", "commit", "ready"}:
        return False
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


def _spectral_bar(module: Any, theme: Any, width: int, ratio: float) -> list[Any]:
    bar_width = min(64, max(24, width - 10))
    filled = min(bar_width, round(bar_width * max(0.0, min(1.0, ratio))))
    spans: list[Any] = [
        module.Span("[", module.style(theme, module.Semantic.MUTED))
    ]
    for index in range(bar_width):
        if index < filled:
            red, green, blue = module._status_gradient_rgb(
                index / max(1, bar_width - 1)
            )
            spans.append(
                module.Span(
                    "█",
                    module.Style().fg(module.Color.rgb(red, green, blue)),
                )
            )
        else:
            spans.append(
                module.Span(
                    "░",
                    module.style(theme, module.Semantic.MUTED),
                )
            )
    spans.append(module.Span("]", module.style(theme, module.Semantic.MUTED)))
    return spans


def _install_cooperative_input_reader(module: Any) -> None:
    """Prevent the native 80 ms terminal poll from starving Python workers.

    The PyO3 ``poll_event`` method currently blocks inside Rust while still
    attached to the Python interpreter. The TUI calls it every frame, which can
    leave the SQLite hydration thread only tiny scheduling windows between
    polls. A non-blocking native probe plus ``time.sleep`` preserves the same
    input latency while releasing the interpreter for the worker thread.
    """
    if getattr(module, "_splined_cooperative_input_reader", False):
        return
    native_reader = getattr(module, "InputEventReader", None)
    if native_reader is None:
        return

    class CooperativeInputEventReader:
        def __init__(self) -> None:
            self._reader = native_reader()

        def __enter__(self) -> "CooperativeInputEventReader":
            self._reader.__enter__()
            return self

        def __exit__(
            self,
            exc_type: Any,
            exc_value: Any,
            traceback: Any,
        ) -> bool:
            return bool(self._reader.__exit__(exc_type, exc_value, traceback))

        def __getattr__(self, name: str) -> Any:
            return getattr(self._reader, name)

        def poll_event(self, timeout_ms: int = 0) -> Any:
            # Probe the native queue without blocking while attached to Python.
            event = self._reader.poll_event(0)
            if event is not None or timeout_ms <= 0:
                return event

            # Python's sleep releases the interpreter so the SQLite/session
            # worker can fetch and materialize all rows at native speed.
            time.sleep(float(timeout_ms) / 1000.0)
            return self._reader.poll_event(0)

    CooperativeInputEventReader.__name__ = "CooperativeInputEventReader"
    module.InputEventReader = CooperativeInputEventReader
    module._splined_cooperative_input_reader = True


def _patch_tui(module: Any) -> None:
    if getattr(module, "_splined_advanced_runtime_policy", False):
        return

    original_apply = module.TuiState.apply
    original_render = module._render_startup
    original_card = module.card

    def apply(state: Any, event: str, payload: dict[str, Any]) -> None:
        original_apply(state, event, payload)
        # First-build presentation is already keyed as ``media-index``. Warm
        # starts emit cache_progress directly, so give them a separate marker
        # that does not trigger the BUILDING/REFRESHING database banner.
        if event == "cache_progress" and _is_sqlite_progress(payload):
            state.cache_kind = "sqlite-warm"

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

    def render_startup(frame: Any, state: Any, theme: Any) -> None:
        original_render(frame, state, theme)
        if getattr(state, "cache_kind", "") != "sqlite-warm":
            return

        area = frame.area
        brand_height = min(
            int(area.height),
            module.startup_brand_height(area.width, area.height),
        )
        _brand_area, inventory_area = module._split_vertical(
            area,
            [module.Constraint.length(brand_height), module.Constraint.fill(1)],
        )
        panel_width = min(max(48, int(inventory_area.width) - 8), 86)
        panel_height = min(10, int(inventory_area.height))
        panel = module.Rect(
            int(inventory_area.x)
            + max(0, (int(inventory_area.width) - panel_width) // 2),
            int(inventory_area.y)
            + max(0, (int(inventory_area.height) - panel_height) // 2),
            panel_width,
            panel_height,
        )
        status_rows = module._split_vertical(
            panel,
            [
                module.Constraint.length(3),
                module.Constraint.length(1),
                module.Constraint.length(2),
                module.Constraint.fill(1),
            ],
        )
        progress_area = status_rows[2]
        total = int(getattr(state, "cache_total", 0) or 0)
        percent = float(getattr(state, "cache_percent", 0.0) or 0.0)
        ratio = percent / 100.0 if total else 0.0

        if total:
            spans = _spectral_bar(module, theme, int(progress_area.width), ratio)
        else:
            # A rare denominator-free warm-maintenance phase gets the same
            # moving spectral pulse used by first-run discovery.
            bar_width = min(64, max(24, int(progress_area.width) - 10))
            pulse_width = max(8, min(16, bar_width // 4))
            elapsed = max(
                0.0,
                time.monotonic()
                - float(getattr(state, "started_at", time.monotonic())),
            )
            head = (int(elapsed * 12.0) % (bar_width + pulse_width)) - pulse_width
            spans = [
                module.Span("[", module.style(theme, module.Semantic.MUTED))
            ]
            for index in range(bar_width):
                offset = index - head
                if 0 <= offset < pulse_width:
                    red, green, blue = module._status_gradient_rgb(
                        offset / max(1, pulse_width - 1)
                    )
                    spans.append(
                        module.Span(
                            "█",
                            module.Style().fg(
                                module.Color.rgb(red, green, blue)
                            ),
                        )
                    )
                else:
                    spans.append(
                        module.Span(
                            "░",
                            module.style(theme, module.Semantic.MUTED),
                        )
                    )
            spans.append(
                module.Span("]", module.style(theme, module.Semantic.MUTED))
            )

        frame.render_widget(module.Clear(), progress_area)
        frame.render_widget(
            module.Paragraph(
                module.Text(
                    [
                        module.Line(spans).centered(),
                        module.Line([]),
                    ]
                )
            ).style(module.panel_style(theme)),
            progress_area,
        )

    def publish(writer: Any, line: str) -> None:
        clean = module.OSC_RE.sub("", module.ANSI_RE.sub("", line)).strip()
        if not clean:
            return
        level = writer.level
        upper = clean.upper()
        failed_zero = bool(
            module.re.search(r"\bFAILED\s*\[\s*0\s*\]", upper)
        )
        if "ERROR" in upper or ("FAILED" in upper and not failed_zero):
            level = "ERROR"
        elif "WARN" in upper or "FALLBACK" in upper:
            level = "WARN"
        core_module = sys.modules.get("splined")
        logger = (
            getattr(core_module, "runtime_log", None)
            if core_module is not None
            else None
        )
        if callable(logger):
            logger(level, clean)
        writer.adapter.emit("log", {"level": level, "message": clean})

    _install_cooperative_input_reader(module)
    module.TuiState.apply = apply
    module.card = card
    module._render_startup = render_startup
    module.EventWriter._publish = publish
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

    # Media-build owns the authoritative TUI patch boundary. Extend that
    # boundary so these final presentation fixes are applied after the build,
    # progress, and native-overlay policies have installed their wrappers.
    original_patch_tui = build_policy._patch_tui

    def patch_tui(module: Any) -> None:
        original_patch_tui(module)
        _patch_tui(module)

    build_policy._patch_tui = patch_tui

    original_run = core.run_operational_interface

    def run_operational_interface(args: Any, worker: Any) -> int:
        likely_tui = (
            not bool(getattr(args, "no_tui", False))
            and bool(getattr(sys.stdin, "isatty", lambda: False)())
            and bool(getattr(sys.stdout, "isatty", lambda: False)())
        )
        try:
            return int(original_run(args, worker))
        finally:
            # A normal EventReader exit already disables mouse capture. This
            # outer idempotent restore also covers exceptions/SystemExit and
            # prevents SGR mouse reports from being interpreted by the shell as
            # commands such as ``6M35`` after the alternate screen closes.
            if likely_tui:
                try:
                    from tui import splined_tui

                    restore = getattr(
                        splined_tui,
                        "emergency_terminal_restore",
                        None,
                    )
                    if callable(restore):
                        restore()
                        core.debug_log("tui.terminal_restore.finalized")
                except Exception as exc:
                    core.debug_log(
                        "tui.terminal_restore.error "
                        f"error={type(exc).__name__}: {exc}"
                    )

    core.run_operational_interface = run_operational_interface
    core._splined_advanced_runtime_policy_installed = True

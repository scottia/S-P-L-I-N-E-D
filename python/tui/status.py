"""Tiny engine-to-presentation boundary.

When no adapter is active these helpers are no-ops (or ordinary ``input``), so
the existing CLI remains the behavioral baseline.  The TUI installs one
adapter only around its worker thread.
"""

from __future__ import annotations

import builtins
import threading
from contextlib import contextmanager
from typing import Any, Iterator, Protocol


class Adapter(Protocol):
    def emit(self, event: str, payload: dict[str, Any]) -> None: ...
    def read(self, prompt: str, context: dict[str, Any]) -> str: ...


_lock = threading.RLock()
_adapter: Adapter | None = None


@contextmanager
def use_adapter(adapter: Adapter) -> Iterator[None]:
    global _adapter
    with _lock:
        if _adapter is not None:
            raise RuntimeError("A SPLINED UI adapter is already active.")
        _adapter = adapter
    try:
        yield
    finally:
        with _lock:
            if _adapter is adapter:
                _adapter = None


def active() -> bool:
    with _lock:
        return _adapter is not None


def emit(event: str, **payload: Any) -> None:
    with _lock:
        adapter = _adapter
    if adapter is not None:
        adapter.emit(event, payload)


def read_input(prompt: str = "", **context: Any) -> str:
    with _lock:
        adapter = _adapter
    if adapter is None:
        return builtins.input(prompt)
    return adapter.read(prompt, context)

"""TTY-safe TUI activation rules."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Activation:
    enabled: bool
    explicit: bool
    reason: str


def decide_activation(
    *,
    tui: bool,
    no_tui: bool,
    operational: bool,
    stdin_tty: bool,
    stdout_tty: bool,
) -> Activation:
    if tui and no_tui:
        raise ValueError("--tui and --no-tui cannot be used together.")
    if not operational:
        if tui:
            raise ValueError("--tui is available only for an operational scan.")
        return Activation(False, False, "non-operational-command")
    if no_tui:
        return Activation(False, False, "disabled")
    if not (stdin_tty and stdout_tty):
        if tui:
            raise ValueError("--tui requires interactive stdin and stdout terminals.")
        return Activation(False, False, "non-tty")
    return Activation(True, tui, "explicit" if tui else "interactive-default")

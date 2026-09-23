"""Small dialog models kept testable without a live terminal."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class ConfirmDialog:
    title: str
    question: str
    accept_label: str = "Yes"
    reject_label: str = "No"


BYPASS_DIALOG = ConfirmDialog("Confirm", "Bypass this album?")


def confirm_key(code: str) -> bool | None:
    value = code.lower()
    if value in {"y", "enter", "return"}:
        return True
    if value in {"n", "esc", "escape"}:
        return False
    return None

"""Key-to-action mapping; engine command letters remain authoritative."""

from __future__ import annotations

from enum import Enum


class Action(str, Enum):
    NONE = "none"
    UP = "up"
    DOWN = "down"
    LEFT = "left"
    RIGHT = "right"
    NEXT_REGION = "next-region"
    PREVIOUS_REGION = "previous-region"
    ACTIVATE = "activate"
    BACK = "back"
    HELP = "help"
    QUIT = "quit"
    SUGGESTED = "suggested"
    FALLBACK_EDIT = "fallback-edit"
    MUSICBRAINZ = "musicbrainz"
    BYPASS = "bypass"
    KEEP = "keep"
    DIGIT = "digit"
    DELETE = "delete"
    TOGGLE = "toggle"
    SETTINGS = "settings"
    SAVE = "save"
    URL = "url"


def map_key(code: str, *, ctrl: bool = False, shift: bool = False) -> Action:
    value = code.lower()
    if ctrl and value == "c":
        return Action.QUIT
    if ctrl and value == "s":
        return Action.SAVE
    if value == "up":
        return Action.UP
    if value == "down":
        return Action.DOWN
    if value == "left":
        return Action.LEFT
    if value == "right":
        return Action.RIGHT
    if value in {"backtab", "shift+tab"} or (value == "tab" and shift):
        return Action.PREVIOUS_REGION
    if value == "tab":
        return Action.NEXT_REGION
    if value in {"enter", "return"}:
        return Action.ACTIVATE
    if value in {"esc", "escape"}:
        return Action.BACK
    if value == "?":
        return Action.HELP
    if value == "q":
        return Action.QUIT
    if value == "s":
        return Action.SUGGESTED
    if value == "f":
        return Action.FALLBACK_EDIT
    if value == "m":
        return Action.MUSICBRAINZ
    if value == "b":
        return Action.BYPASS
    if value == "k":
        return Action.KEEP
    if value in {"space", " "}:
        return Action.TOGGLE
    if value == "p":
        return Action.SETTINGS
    if value == "u":
        return Action.URL
    if value in {"backspace", "delete"}:
        return Action.DELETE
    if len(value) == 1 and value.isdigit():
        return Action.DIGIT
    return Action.NONE


def picker_response(action: Action, selected_index: int | None = None) -> str | None:
    return {
        Action.SUGGESTED: "s",
        Action.FALLBACK_EDIT: "f",
        Action.MUSICBRAINZ: "m",
        Action.BYPASS: "b",
        Action.KEEP: "k",
        Action.BACK: "b",
    }.get(action) or (
        str(selected_index + 1)
        if action is Action.ACTIVATE and selected_index is not None
        else None
    )

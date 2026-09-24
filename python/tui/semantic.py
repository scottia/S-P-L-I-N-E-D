"""Stable state-to-presentation meanings shared by both themes."""

from __future__ import annotations

from enum import Enum


class Semantic(str, Enum):
    ACCEPTED = "accepted"
    ACTIVE = "active"
    FALLBACK = "fallback"
    WARNING = "warning"
    REJECTED = "rejected"
    HISTORY = "history"
    SPECIAL = "special"
    DISABLED = "disabled"
    TEXT = "text"
    MUTED = "muted"
    INFO = "info"
    DEBUG = "debug"


def range_semantic(range_type: str) -> Semantic:
    return {
        "BelowMinimum": Semantic.REJECTED,
        "LowerRange": Semantic.FALLBACK,
        "Ideal": Semantic.ACCEPTED,
        "UpperRange": Semantic.WARNING,
        "Ladder": Semantic.SPECIAL,
        "AboveLadder": Semantic.HISTORY,
    }.get(range_type, Semantic.TEXT)


def outcome_semantic(outcome: str) -> Semantic:
    value = outcome.lower()
    if "fail" in value or "error" in value or "unresolved" in value:
        return Semantic.REJECTED
    if "bypass" in value or "fallback" in value:
        return Semantic.FALLBACK
    if "postpon" in value or "history" in value:
        return Semantic.HISTORY
    if any(
        marker in value
        for marker in (
            "selected",
            "kept",
            "complete",
            "install",
            "written",
            "replace",
            "unchanged",
            "retain",
            "preserve",
            "read-only",
        )
    ):
        return Semantic.ACCEPTED
    return Semantic.ACTIVE


def log_semantic(level: str) -> Semantic:
    return {
        "DEBUG": Semantic.DEBUG,
        "INFO": Semantic.INFO,
        "WARN": Semantic.WARNING,
        "WARNING": Semantic.WARNING,
        "ERROR": Semantic.REJECTED,
    }.get(level.upper(), Semantic.TEXT)

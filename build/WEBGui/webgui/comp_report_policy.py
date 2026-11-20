"""Shared COMP report matching, batching, and coordination policy.

PostgreSQL owns the revision for WebGUI and coordinated Windows clients.  A
standalone client uses the repository default or its validated local mirror.
Dynamic track identity, artwork, and completion evidence remains in the
database; this document contains only deterministic workflow rules.
"""

from __future__ import annotations
from webgui import source_root

from contextlib import closing, contextmanager
from contextvars import ContextVar
from functools import lru_cache
import json
import os
from pathlib import Path
import re
import tempfile
import threading
import tomllib
import unicodedata
from typing import Iterator


POLICY_ENV = "SPLINED_COMP_REPORT_POLICY"
DEFAULT_POLICY = source_root() / "schema" / "comp-report-policy.json"
POLICY_FILENAME = "comp-report-policy.json"
CURRENT_POLICY_ID = "splined-comp-report-v1"
_DATABASE_POLICY: dict | None = None
_DATABASE_POLICY_REVISION = 0
_DATABASE_POLICY_LOCK = threading.RLock()
_PINNED_POLICY: ContextVar[tuple[dict, int] | None] = ContextVar(
    "splined_comp_report_policy", default=None
)


def _configured_policy_path() -> Path | None:
    config_value = os.environ.get("SPLINED_CONFIG", "").strip()
    if not config_value:
        return None
    config_file = Path(config_value)
    try:
        config = tomllib.loads(config_file.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None
    configured = str(config.get("credentials", {}).get("credential_dir", "")).strip()
    if not configured:
        return None
    directory = Path(configured)
    if not directory.is_absolute():
        directory = config_file.parent / directory
    return directory / POLICY_FILENAME


def _candidate_paths() -> tuple[Path, ...]:
    configured = os.environ.get(POLICY_ENV, "").strip()
    values: list[Path] = []
    if configured:
        values.append(Path(configured))
    path = _configured_policy_path()
    if path is not None:
        values.append(path)
    values.append(DEFAULT_POLICY)
    return tuple(dict.fromkeys(values))


def _integer(value: object, name: str, minimum: int, maximum: int) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or not minimum <= value <= maximum:
        raise ValueError(f"{name} must be from {minimum} through {maximum}")
    return value


def validate_comp_report_policy(value: object) -> dict:
    if not isinstance(value, dict):
        raise ValueError("COMP report policy must be a JSON object")
    if int(value.get("schema_version", 0)) != 1:
        raise ValueError("unsupported COMP report policy schema_version")
    if str(value.get("policy_id", "")) != CURRENT_POLICY_ID:
        raise ValueError(f"policy_id must be {CURRENT_POLICY_ID}")

    artist = value.get("artist_identity")
    if not isinstance(artist, dict) or artist.get("selected_artist_precedence") is not True:
        raise ValueError("selected Artist precedence is required")
    separators = artist.get("credit_separators")
    if not isinstance(separators, list) or not separators or any(
        not isinstance(item, str) or not item for item in separators
    ):
        raise ValueError("artist_identity.credit_separators are required")
    normalization = artist.get("normalization")
    if not isinstance(normalization, dict):
        raise ValueError("artist_identity.normalization is required")
    for flag in ("case_fold", "unicode_nfkd", "collapse_whitespace"):
        if normalization.get(flag) is not True:
            raise ValueError(f"artist_identity.normalization.{flag} must be true")
    local = artist.get("local_artist_alias")
    remote = artist.get("musicbrainz_artist_search")
    if not isinstance(local, dict) or not isinstance(remote, dict):
        raise ValueError("local and MusicBrainz Artist matching rules are required")
    _integer(local.get("minimum_score"), "local Artist minimum_score", 1, 100)
    _integer(local.get("exact_score"), "local Artist exact_score", 1, 100)
    _integer(local.get("limit"), "local Artist limit", 1, 100)
    if remote.get("enabled") is not True:
        raise ValueError("MusicBrainz Artist-name recovery must be enabled")
    _integer(remote.get("minimum_score"), "MusicBrainz Artist minimum_score", 1, 100)
    _integer(remote.get("limit"), "MusicBrainz Artist limit", 1, 25)

    recording = value.get("recording_search")
    if not isinstance(recording, dict):
        raise ValueError("recording_search is required")
    for key in (
        "minimum_score",
        "exact_recording_score",
        "related_work_score",
        "maximum_non_exact_score",
    ):
        _integer(recording.get(key), f"recording_search.{key}", 1, 100)
    _integer(recording.get("limit"), "recording_search.limit", 1, 100)
    _integer(recording.get("detail_lookup_limit"), "recording_search.detail_lookup_limit", 0, 25)

    grouping = value.get("result_grouping")
    if not isinstance(grouping, dict) or grouping.get("official_releases_only") is not True:
        raise ValueError("official release grouping is required")
    bands = grouping.get("percentage_bands")
    if not isinstance(bands, list) or not bands or any(
        not isinstance(item, int) or not 1 <= item <= 100 for item in bands
    ) or bands != sorted(set(bands), reverse=True):
        raise ValueError("result_grouping.percentage_bands must be unique and descending")
    order = grouping.get("release_family_order")
    required_order = {"album", "single", "ep", "soundtrack", "compilation", "broadcast", "other"}
    if not isinstance(order, list) or set(order) != required_order:
        raise ValueError("result_grouping.release_family_order is incomplete")

    existing = value.get("existing_artwork")
    batch = value.get("batch")
    coordination = value.get("coordination")
    synchronization = value.get("synchronization")
    if not isinstance(existing, dict) or not all(
        existing.get(key) is True
        for key in (
            "show_before_remote_search",
            "reuse_if_identity_pending",
            "completion_requires_authoritative_ids",
        )
    ):
        raise ValueError("existing_artwork rules are required")
    if not isinstance(batch, dict) or batch.get("completion_states") != [
        "completed", "unresolved", "failed"
    ] or batch.get("pin_policy_revision") is not True or batch.get(
        "refresh_report_after_each_track"
    ) is not True:
        raise ValueError("batch completion, pinning, and refresh rules are required")
    if not isinstance(coordination, dict) or not all(
        coordination.get(key) is True
        for key in (
            "publish_verified_artwork_write",
            "publish_unresolved_identity_write",
            "completion_requires_authoritative_ids",
        )
    ):
        raise ValueError("coordination publishing rules are required")
    if not isinstance(synchronization, dict) or synchronization != {
        "startup_source": "local_mirror",
        "async_revision_check": True,
        "activate_only_when_idle": True,
    }:
        raise ValueError("deterministic synchronization rules are required")
    return value


def editable_policy_path() -> Path:
    configured = os.environ.get(POLICY_ENV, "").strip()
    if configured:
        return Path(configured)
    path = _configured_policy_path()
    if path is None:
        raise RuntimeError("Provider Credential Directory is unavailable for the COMP report policy")
    return path


def save_comp_report_policy(value: object) -> dict:
    policy = validate_comp_report_policy(value)
    target = editable_policy_path()
    target.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(
        prefix=".comp-report-policy-", suffix=".json", dir=target.parent
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            json.dump(policy, output, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, target)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    return activate_comp_report_policy(policy, revision=active_policy_revision())


def activate_comp_report_policy(value: object, *, revision: int = 0) -> dict:
    policy = validate_comp_report_policy(value)
    with _DATABASE_POLICY_LOCK:
        global _DATABASE_POLICY, _DATABASE_POLICY_REVISION
        _DATABASE_POLICY = policy
        _DATABASE_POLICY_REVISION = max(0, int(revision))
        _active_or_file_policy.cache_clear()
    return policy


def active_policy_revision() -> int:
    pinned = _PINNED_POLICY.get()
    if pinned is not None:
        return pinned[1]
    with _DATABASE_POLICY_LOCK:
        return _DATABASE_POLICY_REVISION


@contextmanager
def pin_comp_report_policy(
    value: object | None = None, revision: int | None = None
) -> Iterator[tuple[dict, int]]:
    """Pin one immutable policy snapshot for an entire COMP batch."""
    existing = _PINNED_POLICY.get()
    if existing is not None:
        yield existing
        return
    selected = comp_report_policy() if value is None else validate_comp_report_policy(value)
    snapshot = (
        json.loads(json.dumps(selected)),
        active_policy_revision() if revision is None else max(0, int(revision)),
    )
    token = _PINNED_POLICY.set(snapshot)
    try:
        yield snapshot
    finally:
        _PINNED_POLICY.reset(token)


def pinned_comp_report_batch(function):
    """Decorate a synchronous COMP batch with one revision snapshot."""
    def wrapped(*args, **kwargs):
        with pin_comp_report_policy():
            return function(*args, **kwargs)
    wrapped.__name__ = function.__name__
    wrapped.__doc__ = function.__doc__
    wrapped.__module__ = function.__module__
    return wrapped


def database_comp_report_policy(database, *, seed_if_missing: bool = True) -> dict:
    with closing(database.connect()) as db, db:
        db.raw.execute("SELECT pg_advisory_xact_lock(741520215)")
        row = db.raw.execute(
            "SELECT policy,revision,updated_at,updated_by FROM comp_report_policy WHERE singleton=TRUE"
        ).fetchone()
        if row is None:
            if not seed_if_missing:
                raise RuntimeError("PostgreSQL COMP report policy has not been initialized")
            seed = _file_comp_report_policy()
            row = db.raw.execute(
                "INSERT INTO comp_report_policy(singleton,policy,revision,updated_by) "
                "VALUES(TRUE,%s::jsonb,1,'webgui-seed') "
                "RETURNING policy,revision,updated_at,updated_by",
                (json.dumps(seed),),
            ).fetchone()
    policy = activate_comp_report_policy(row["policy"], revision=int(row["revision"]))
    return {
        "policy": policy,
        "revision": int(row["revision"]),
        "updated_at": str(row["updated_at"]),
        "updated_by": str(row["updated_by"]),
    }


def save_database_comp_report_policy(database, value: object, *, updated_by: str) -> dict:
    policy = validate_comp_report_policy(value)
    with closing(database.connect()) as db, db:
        db.raw.execute("SELECT pg_advisory_xact_lock(741520215)")
        row = db.raw.execute(
            "INSERT INTO comp_report_policy(singleton,policy,revision,updated_by) "
            "VALUES(TRUE,%s::jsonb,1,%s) ON CONFLICT(singleton) DO UPDATE SET "
            "policy=excluded.policy,revision=comp_report_policy.revision+1,"
            "updated_at=now(),updated_by=excluded.updated_by "
            "RETURNING policy,revision,updated_at,updated_by",
            (json.dumps(policy), updated_by),
        ).fetchone()
    active = activate_comp_report_policy(row["policy"], revision=int(row["revision"]))
    return {
        "policy": active,
        "revision": int(row["revision"]),
        "updated_at": str(row["updated_at"]),
        "updated_by": str(row["updated_by"]),
    }


def comp_report_policy() -> dict:
    pinned = _PINNED_POLICY.get()
    if pinned is not None:
        return pinned[0]
    return _active_or_file_policy()


@lru_cache(maxsize=1)
def _active_or_file_policy() -> dict:
    with _DATABASE_POLICY_LOCK:
        if _DATABASE_POLICY is not None:
            return _DATABASE_POLICY
    return _file_comp_report_policy()


def _file_comp_report_policy() -> dict:
    errors = []
    for path in _candidate_paths():
        try:
            return validate_comp_report_policy(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, ValueError, TypeError) as error:
            errors.append(f"{path}: {error}")
    raise RuntimeError("Unable to load COMP report policy: " + "; ".join(errors))


def normalize_artist_name(value: object) -> str:
    rules = comp_report_policy()["artist_identity"]["normalization"]
    text = str(value or "").strip()
    if rules["unicode_nfkd"]:
        text = unicodedata.normalize("NFKD", text)
    for source, replacement in rules.get("word_equivalents", {}).items():
        text = text.replace(str(source), f" {replacement} ")
    for symbol in rules.get("strip_leading_symbols", []):
        text = text.lstrip(str(symbol))
    if rules["case_fold"]:
        text = text.casefold()
    if rules["collapse_whitespace"]:
        text = " ".join(text.split())
    for article in rules.get("remove_leading_articles", []):
        prefix = str(article).casefold().strip() + " "
        if text.startswith(prefix):
            text = text[len(prefix):]
            break
    return "".join(character for character in text if character.isalnum())


def split_artist_credit(value: object) -> tuple[str, ...]:
    text = str(value or "").strip()
    if not text:
        return ()
    separators = sorted(
        (
            "\0" if separator == "\\u0000" else separator
            for separator in comp_report_policy()["artist_identity"]["credit_separators"]
        ),
        key=len,
        reverse=True,
    )
    pattern = "|".join(re.escape(separator) for separator in separators)
    return tuple(part.strip() for part in re.split(pattern, text, flags=re.IGNORECASE) if part.strip())


def focused_artist_name(track_credit: object, selected_artist: object = "") -> str:
    selected = str(selected_artist or "").strip()
    credit = str(track_credit or "").strip()
    if selected and comp_report_policy()["artist_identity"]["selected_artist_precedence"]:
        selected_key = normalize_artist_name(selected)
        if selected_key and (
            normalize_artist_name(credit) == selected_key
            or selected_key in {normalize_artist_name(part) for part in split_artist_credit(credit)}
        ):
            return selected
    parts = split_artist_credit(credit)
    return parts[0] if parts else credit


def artist_names_compatible(left: object, right: object) -> bool:
    left_key = normalize_artist_name(left)
    right_key = normalize_artist_name(right)
    if not left_key or not right_key:
        return False
    if left_key == right_key:
        return True
    return right_key in {normalize_artist_name(part) for part in split_artist_credit(left)}

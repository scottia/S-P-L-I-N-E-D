"""Resolve the previous release-note baseline across official release stamps."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path

from source_version import _normalized_release_content, current_source_version


NUMERIC_VERSION = re.compile(r"^\d+\.\d+\.\d+$")
PRODUCT_ROOTS = ("build/Windows", "build/WEBGui", "schema")
VERSION_FILES = {
    "build/Windows/Cargo.toml",
    "build/Windows/Cargo.lock",
    "build/Windows/gui/app.manifest",
    "build/Windows/package/Package.appxmanifest",
    "build/Windows/package/SPLINED.appinstaller.template",
    "build/WEBGui/webgui/backend/splined.py",
}


@dataclass(frozen=True)
class ReleaseNoteBaseline:
    previous_tag: str
    baseline_commit: str
    range: str
    mode: str


def _git(repository: Path, *args: str, text: bool = True) -> str | bytes:
    result = subprocess.run(
        ["git", "-C", str(repository), *args],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=text,
    )
    return result.stdout.strip() if text else result.stdout


def _is_ancestor(repository: Path, candidate: str, descendant: str) -> bool:
    result = subprocess.run(
        [
            "git",
            "-C",
            str(repository),
            "merge-base",
            "--is-ancestor",
            candidate,
            descendant,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    if result.returncode == 0:
        return True
    if result.returncode == 1:
        return False
    raise ValueError(
        "Unable to inspect release ancestry: "
        + result.stderr.decode(errors="replace").strip()
    )


def _commit_bytes(repository: Path, commit: str, path: str) -> bytes:
    return _git(repository, "show", f"{commit}:{path}", text=False)  # type: ignore[return-value]


def _normalized_product_digest(repository: Path, commit: str) -> str:
    paths = str(
        _git(repository, "ls-tree", "-r", "--name-only", commit, "--", *PRODUCT_ROOTS)
    ).splitlines()
    if not paths:
        raise ValueError(f"Release source {commit} has no product tree")
    digest = hashlib.sha256()
    for path in sorted(paths):
        data = _commit_bytes(repository, commit, path)
        digest.update(path.encode("utf-8"))
        digest.update(b"\0")
        digest.update(_normalized_release_content(path, data))
        digest.update(b"\0")
    return digest.hexdigest()


def _version_at_commit(repository: Path, commit: str) -> str:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        for relative in VERSION_FILES:
            destination = root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(_commit_bytes(repository, commit, relative))
        return current_source_version(root)


def _numeric_tags_before(repository: Path, version: str) -> list[str]:
    current = tuple(map(int, version.split(".")))
    numeric: list[tuple[tuple[int, int, int], str]] = []
    for tag in str(_git(repository, "tag", "--list")).splitlines():
        if NUMERIC_VERSION.fullmatch(tag):
            value = tuple(map(int, tag.split(".")))
            if value < current:
                numeric.append((value, tag))
    return [tag for _, tag in sorted(numeric, reverse=True)]


def resolve_release_note_baseline(
    repository: Path, version: str
) -> ReleaseNoteBaseline:
    """Return the immediate prior release and the safe git-cliff baseline."""
    repository = repository.resolve()
    if not NUMERIC_VERSION.fullmatch(version):
        raise ValueError("Version must be an existing numeric release tag")
    try:
        release_commit = str(
            _git(repository, "rev-parse", "--verify", f"refs/tags/{version}^{{commit}}")
        )
    except subprocess.CalledProcessError as exc:
        raise ValueError(f"Release tag does not exist: {version}") from exc

    candidates = _numeric_tags_before(repository, version)
    if not candidates:
        raise ValueError(f"No previous numeric release tag exists before {version}")
    previous_tag = candidates[0]
    previous_commit = str(
        _git(
            repository,
            "rev-parse",
            "--verify",
            f"refs/tags/{previous_tag}^{{commit}}",
        )
    )

    if _is_ancestor(repository, previous_commit, release_commit):
        return ReleaseNoteBaseline(
            previous_tag=previous_tag,
            baseline_commit=previous_commit,
            range=f"{previous_tag}..{version}",
            mode="direct-tag-ancestor",
        )

    parent_line = str(_git(repository, "rev-list", "--parents", "-n", "1", previous_commit))
    parent_parts = parent_line.split()
    if len(parent_parts) != 2:
        raise ValueError(
            f"Previous release {previous_tag} is not an ancestor and is not a "
            "single-parent temporary version stamp"
        )
    stamp_parent = parent_parts[1]
    if not _is_ancestor(repository, stamp_parent, release_commit):
        raise ValueError(
            f"Previous release {previous_tag} is unrelated to release {version}"
        )

    changed = set(
        str(_git(repository, "diff", "--name-only", stamp_parent, previous_commit)).splitlines()
    )
    if changed != VERSION_FILES:
        raise ValueError(
            f"Previous release {previous_tag} is not a valid temporary version stamp; "
            f"changed files were {sorted(changed)}"
        )
    if _version_at_commit(repository, previous_commit) != previous_tag:
        raise ValueError(
            f"Previous release {previous_tag} does not contain matching synchronized versions"
        )
    if _normalized_product_digest(repository, stamp_parent) != _normalized_product_digest(
        repository, previous_commit
    ):
        raise ValueError(
            f"Previous release {previous_tag} changed product source beyond synchronized versions"
        )

    return ReleaseNoteBaseline(
        previous_tag=previous_tag,
        baseline_commit=stamp_parent,
        range=f"{stamp_parent}..{version}",
        mode="temporary-stamp-parent",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    try:
        result = resolve_release_note_baseline(args.repository, args.version)
    except (ValueError, subprocess.CalledProcessError) as exc:
        raise SystemExit(str(exc)) from exc
    print(json.dumps(asdict(result), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

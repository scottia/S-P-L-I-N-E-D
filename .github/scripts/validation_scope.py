"""Select validation lanes from an authorized boundary-check result."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Mapping


LANES = (
    "infrastructure",
    "root_native",
    "windows",
    "ubuntu_webgui",
    "release_contract",
    "documentation",
)


def is_validation_infrastructure(path: str) -> bool:
    return (
        path == ".github/workflows/validate.yml"
        or path.startswith(".github/scripts/")
        or path.startswith(".github/boundaries/")
    )


def select_lanes(boundary: Mapping[str, object]) -> dict[str, bool]:
    """Map authorized policy validations to their actual CI consumers."""

    if boundary.get("allowed") is not True:
        raise ValueError("Boundary authorization must succeed before validation selection")

    categories = {str(value) for value in boundary.get("categories", [])}
    validations = {str(value) for value in boundary.get("validations", [])}
    classified = boundary.get("classified_paths", {})
    if not isinstance(classified, Mapping):
        raise ValueError("Boundary result is missing classified paths")

    paths = {str(path) for path in classified}
    selected = dict.fromkeys(LANES, False)
    selected["root_native"] = "root-native-other" in validations
    selected["windows"] = "windows" in validations
    selected["ubuntu_webgui"] = "ubuntu-webgui" in validations
    selected["release_contract"] = "release-version" in categories
    selected["documentation"] = "terminology-docs" in categories
    selected["infrastructure"] = (
        "boundary-policy" in categories
        or any(is_validation_infrastructure(path) for path in paths)
    )
    return selected


def github_outputs(boundary: Mapping[str, object]) -> list[str]:
    selection = select_lanes(boundary)
    compact_boundary = json.dumps(boundary, sort_keys=True, separators=(",", ":"))
    compact_selection = json.dumps(selection, sort_keys=True, separators=(",", ":"))
    lines = [
        f"boundary_json={compact_boundary}",
        f"selection_json={compact_selection}",
    ]
    lines.extend(f"{lane}={str(selection[lane]).lower()}" for lane in LANES)
    return lines


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("boundary_json", type=Path)
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args(argv)

    try:
        boundary = json.loads(args.boundary_json.read_text(encoding="utf-8"))
        lines = github_outputs(boundary)
        text = "\n".join(lines) + "\n"
        if args.github_output is None:
            sys.stdout.write(text)
        else:
            with args.github_output.open("a", encoding="utf-8", newline="\n") as output:
                output.write(text)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"validation selection failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

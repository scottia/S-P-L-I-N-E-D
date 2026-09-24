#!/usr/bin/env python3
from __future__ import annotations

"""SPLINED operational scan entrypoint.

The core provider/authority engine remains in splined.py.  This module owns the
operational album scan surface so local artwork can be evaluated before remote
artwork discovery without coupling the provider engine to filesystem policy.
"""

import hashlib
import io
import sys
from pathlib import Path
from typing import Any

from mutagen.flac import FLAC
from mutagen.id3 import ID3
from mutagen.mp4 import MP4
from PIL import Image

import splined as core
from tui.aispline import validated_enhanced_results


APP_NAME = core.APP_NAME
VERSION = core.VERSION


def provider_label(source: str) -> str:
    mapping = {
        "local": "Local",
        "webpstill": "WebP",
        "embedded": "Embedded",
    }
    if source.lower() in mapping:
        return mapping[source.lower()]
    return core.provider_label(source)


def target_format_for_candidate(candidate: core.Candidate, format_order: list[str]) -> str:
    # WEBP is preservation/source material during local preflight.  SPLINED's
    # canonical still extracted from it is JPEG.  Embedded artwork follows the
    # same canonical JPEG rule when it becomes the installed album cover.
    if candidate.source == "embedded":
        return "jpeg"
    if candidate.source == "local" and candidate.path.suffix.lower() == ".webp":
        return "jpeg"
    return core.target_format_for_candidate(candidate, format_order)


def project_candidate(candidate: core.Candidate, cfg: dict[str, Any], format_order: list[str]) -> dict[str, Any]:
    return core.project_candidate(
        candidate,
        cfg,
        target_format_for_candidate(candidate, format_order),
    )


def candidate_key(candidate: core.Candidate, cfg: dict[str, Any], format_order: list[str]):
    projected = project_candidate(candidate, cfg, format_order)
    range_rank = {
        "Ideal": 0,
        "UpperRange": 1,
        "LowerRange": 2,
        "Ladder": 3,
        "BelowMinimum": 4,
        "AboveLadder": 5,
    }.get(projected["range_type"], 99)
    transform_penalty = (
        1 if projected["upscaled"] else 0,
        1 if projected["cropped"] else 0,
        1 if projected["resized"] else 0,
        1 if projected["converted"] else 0,
    )
    return (
        {"accept": 0, "fallback": 1, "reject": 2}.get(projected["policy_status"], 2),
        projected["distance"],
        range_rank,
        transform_penalty,
        candidate.source_priority,
        format_order.index(projected["format"])
        if projected["format"] in format_order
        else 999999,
        -projected["short_side"],
        provider_label(candidate.source).lower(),
        str(candidate.ref.id),
    )


def select_best(candidates: list[core.Candidate], cfg: dict[str, Any], format_order: list[str]) -> core.Candidate | None:
    acceptable = [candidate for candidate in candidates if project_candidate(candidate, cfg, format_order)["acceptable"]]
    return min(acceptable, key=lambda candidate: candidate_key(candidate, cfg, format_order)) if acceptable else None


def fallback_sort_key(candidate: core.Candidate, cfg: dict[str, Any], format_order: list[str]):
    projected = project_candidate(candidate, cfg, format_order)
    return (
        -projected["short_side"],
        0 if projected["square"] else 1,
        0 if candidate.ref.approved else 1,
        candidate.source_priority,
        format_order.index(projected["format"])
        if projected["format"] in format_order
        else 999999,
        str(candidate.ref.id),
    )


def fallback_top_candidates(
    candidates: list[core.Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
    limit: int = 10,
) -> list[core.Candidate]:
    return sorted(candidates, key=lambda candidate: fallback_sort_key(candidate, cfg, format_order))[:limit]


def fallback_suggested(
    candidates: list[core.Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
) -> core.Candidate | None:
    if not candidates:
        return None

    def key(candidate: core.Candidate):
        projected = project_candidate(candidate, cfg, format_order)
        return (
            projected["distance"],
            0 if projected["square"] else 1,
            0 if candidate.ref.approved else 1,
            candidate.source_priority,
            format_order.index(projected["format"])
            if projected["format"] in format_order
            else 999999,
            -projected["short_side"],
            str(candidate.ref.id),
        )

    return min(candidates, key=key)


def candidate_link(candidate: core.Candidate) -> str:
    if candidate.source in {"local", "webpstill", "embedded"}:
        return core.bracketed_text("LOCAL", core.green)
    if candidate.source == "enhanced":
        return core.bracketed_text("Enhanced", core.magenta)
    return core.format_source_url(candidate.ref.url)


def candidate_provenance(candidate: core.Candidate) -> str:
    if candidate.source in {"local", "webpstill", "embedded"}:
        return "[LOCAL]"
    if candidate.source == "enhanced":
        return "[Enhanced]"
    return "[URL]"


def render_candidate_table(
    candidates: list[core.Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
    selected: core.Candidate | None = None,
    suggested: core.Candidate | None = None,
    manual_fallback: bool = False,
) -> None:
    candidate_items: list[dict[str, Any]] = []
    local_candidate = next(
        (
            item
            for item in candidates
            if item.source in {"local", "webpstill", "embedded"}
        ),
        None,
    )
    local_projected = (
        project_candidate(local_candidate, cfg, format_order)
        if local_candidate is not None
        else None
    )
    for index, candidate in enumerate(candidates, 1):
        projected = project_candidate(candidate, cfg, format_order)
        comparison = ""
        crop_risk = "equal"
        if local_candidate is not None and candidate is not local_candidate and local_projected is not None:
            delta = int(local_projected["distance"]) - int(projected["distance"])
            if delta > 0:
                resolution = f"{provider_label(candidate.source)} +{delta}px toward Ideal"
            elif delta < 0:
                resolution = f"Local +{-delta}px toward Ideal"
            else:
                resolution = "Equal distance to Ideal"
            shape = (
                "equal"
                if bool(local_projected["square"]) == bool(projected["square"])
                else "changed"
            )
            local_crop = candidate_crop_text(local_candidate, local_projected)
            remote_crop = candidate_crop_text(candidate, projected)
            crop_risk = (
                "equal"
                if local_crop == remote_crop
                else f"Local {local_crop} / {provider_label(candidate.source)} {remote_crop}"
            )
            comparison = (
                f"{resolution} · Shape: {shape} · Crop risk: {crop_risk} · "
                "Source: remote replacement"
            )
        candidate_items.append(
            {
                "number": index,
                "source": provider_label(candidate.source),
                "width": candidate.width,
                "height": candidate.height,
                "format": candidate.format,
                "range_type": projected["range_type"],
                "distance": projected["distance"],
                "square": projected["square"],
                "acceptable": projected["acceptable"],
                "approved": candidate.ref.approved,
                "id": str(candidate.ref.id),
                "url": candidate.ref.url,
                "provenance": candidate_provenance(candidate),
                "comparison": comparison,
                "crop_risk": crop_risk,
                "selected": candidate is selected,
                "suggested": candidate is suggested,
            }
        )
    core.emit_ui(
        "candidates",
        items=candidate_items,
        manual_fallback=manual_fallback,
        aisplined=core.aisplined_settings(cfg),
        ai_runtime_available=False,
        ideal=int(core.section(cfg, "range").get("ideal", 1800)),
    )

    columns = [
        ("[#]", 5, "right"),
        ("Source", 8, "left"),
        ("Resolution", 12, "left"),
        ("Type", 6, "left"),
        ("Range Type", 13, "left"),
        ("Distance", 10, "left"),
        ("Square", 8, "left"),
        ("Acceptable", 12, "left"),
        ("Approved", 10, "left"),
        ("URL", 0, "left"),
    ]
    header_cells: list[str] = []
    for label, width, align in columns:
        colored = core.cyan(label)
        if width:
            colored = (
                core.rjust_color(colored, width)
                if align == "right"
                else core.ljust_color(colored, width)
            )
        header_cells.append(colored)
    print("  " + " ".join(header_cells))
    print("  " + core.gray("-" * 128))

    for index, candidate in enumerate(candidates, 1):
        projected = project_candidate(candidate, cfg, format_order)
        if manual_fallback and candidate is suggested:
            marker = core.paint("PURPLE", "*[s]")
        elif candidate is selected:
            marker = core.green(f"*[{index}]")
        else:
            marker = core.white(f"[{index}]")

        cells = [
            core.rjust_color(marker, 5),
            core.ljust_color(core.magenta(provider_label(candidate.source)), 8),
            core.ljust_color(core.white(f"{candidate.width}x{candidate.height}"), 12),
            core.ljust_color(core.white(candidate.format.upper()), 6),
            core.ljust_color(core.color_range_type(projected["range_type"]), 13),
            core.ljust_color(core.white(str(projected["distance"])), 10),
            core.ljust_color(core.bool_color(projected["square"]), 8),
            core.ljust_color(core.bool_color(projected["acceptable"]), 12),
            core.ljust_color(core.bool_color(candidate.ref.approved), 10),
            candidate_link(candidate),
        ]
        row = "  " + " ".join(cells)
        print(core.bold(row) if candidate is selected else row)


def inspect_candidate_file(path: Path, source: str, priority: int) -> core.Candidate:
    with Image.open(path) as image:
        fmt = core.image_format(image)
        width, height = image.size
        image.verify()
    return core.Candidate(
        core.Ref(source, path.name, "", front=True, approved=True, types=["Front"]),
        path,
        width,
        height,
        fmt,
        priority,
    )


def _front_picture(items: list[Any]) -> Any | None:
    if not items:
        return None
    for item in items:
        if int(getattr(item, "type", 0) or 0) == 3:
            return item
    return items[0]


def embedded_candidate(audio_path: Path, cache: Path) -> core.Candidate | None:
    data: bytes | None = None
    try:
        suffix = audio_path.suffix.lower()
        if suffix == ".mp3":
            picture = _front_picture(list(ID3(audio_path).getall("APIC")))
            if picture is not None:
                data = bytes(picture.data)
        elif suffix == ".flac":
            picture = _front_picture(list(FLAC(audio_path).pictures))
            if picture is not None:
                data = bytes(picture.data)
        elif suffix in {".m4a", ".mp4"}:
            covers = list((MP4(audio_path).tags or {}).get("covr") or [])
            if covers:
                data = bytes(covers[0])
    except Exception as exc:
        core.debug_log(
            f"local.embedded.error file={str(audio_path)!r} "
            f"error={type(exc).__name__}: {exc}"
        )
        return None

    if not data:
        return None

    try:
        with Image.open(io.BytesIO(data)) as image:
            fmt = core.image_format(image)
            width, height = image.size
            image.verify()
    except Exception as exc:
        core.debug_log(
            f"local.embedded.invalid file={str(audio_path)!r} "
            f"error={type(exc).__name__}: {exc}"
        )
        return None

    digest = hashlib.sha256(str(audio_path).encode("utf-8", "surrogateescape") + data[:4096]).hexdigest()[:24]
    path = cache / f"splined-local-embedded-{digest}.{core.EXTENSIONS[fmt]}"
    path.write_bytes(data)
    return core.Candidate(
        core.Ref("embedded", audio_path.name, "", front=True, approved=True, types=["Front"]),
        path,
        width,
        height,
        fmt,
        -1,
    )


def _canonical_local_files(album: core.AlbumDir, file_name: str) -> dict[str, list[Path]]:
    wanted = {
        f"{file_name}.webp": "webp",
        f"{file_name}.jpg": "jpeg",
        f"{file_name}.jpeg": "jpeg",
        f"{file_name}.png": "png",
    }
    found: dict[str, list[Path]] = {"webp": [], "jpeg": [], "png": []}
    for path in sorted(album.path.iterdir(), key=lambda item: item.name.lower()):
        if path.is_symlink() or not path.is_file():
            continue
        kind = wanted.get(path.name.lower())
        if kind:
            found[kind].append(path)
    return found


def inspect_local_preflight(
    album: core.AlbumDir,
    cfg: dict[str, Any],
    cache: Path,
    format_order: list[str],
) -> dict[str, Any]:
    output = core.section(cfg, "output")
    file_name = str(output.get("file_name", "cover")).strip()
    core.validate_file_name(file_name)
    files = _canonical_local_files(album, file_name)
    diagnostics: list[str] = []

    # Preserve WEBP as source material, but do not assume its still is the best
    # static artwork available. Generate/update the canonical JPEG using the
    # same aspect-ratio/resize/output safety, then compare that still against
    # normal provider discovery.
    for path in files["webp"]:
        try:
            webp_candidate = inspect_candidate_file(path, "local", -2)
            content, info = core.prepare_final(
                webp_candidate,
                cfg,
                "jpeg",
                allow_out_of_range=True,
            )
            destination = album.path / f"{file_name}.jpg"
            mode = str(cfg.get("mode", "read")).strip().lower()
            unchanged = destination.exists() and destination.read_bytes() == content

            if mode == "write":
                if not unchanged:
                    core.atomic_write(destination, content)
                still_path = destination
            else:
                digest = hashlib.sha256(
                    str(path).encode("utf-8", "surrogateescape") + content[:4096]
                ).hexdigest()[:24]
                still_path = cache / f"splined-local-webp-still-{digest}.jpg"
                still_path.write_bytes(content)

            still = inspect_candidate_file(still_path, "webpstill", -2)
            return {
                "action": "fallback",
                "candidate": still,
                "fallback": [still],
                "cleanup": [],
                "diagnostics": diagnostics,
                "webp_still": True,
                "webp_source": webp_candidate,
                "still_info": info,
                "still_unchanged": unchanged,
            }
        except Exception as exc:
            diagnostics.append(f"{path.name}: {exc}")

    static_candidates: list[core.Candidate] = []
    for path in files["jpeg"] + files["png"]:
        try:
            static_candidates.append(inspect_candidate_file(path, "local", -2))
        except Exception as exc:
            diagnostics.append(f"{path.name}: {exc}")

    if static_candidates:
        best = min(
            static_candidates,
            key=lambda candidate: candidate_key(candidate, cfg, format_order),
        )
        cleanup = [
            candidate.path
            for candidate in static_candidates
            if candidate.path != best.path
        ]
        projected = project_candidate(best, cfg, format_order)
        is_ideal = projected["range_type"] == "Ideal"
        return {
            "action": "local-ideal" if is_ideal else "fallback",
            "candidate": best,
            "fallback": [] if is_ideal else [best],
            "cleanup": cleanup,
            "diagnostics": diagnostics,
        }

    if album.audio_files:
        candidate = embedded_candidate(sorted(album.audio_files)[0], cache)
        if candidate is not None:
            projected = project_candidate(candidate, cfg, format_order)
            is_ideal = projected["range_type"] == "Ideal"
            return {
                "action": "embedded-ideal" if is_ideal else "fallback",
                "candidate": candidate,
                "fallback": [] if is_ideal else [candidate],
                "cleanup": [],
                "diagnostics": diagnostics,
            }

    return {
        "action": "none",
        "candidate": None,
        "fallback": [],
        "cleanup": [],
        "diagnostics": diagnostics,
    }

def cleanup_competing_static(paths: list[Path], mode: str) -> bool:
    if not paths:
        return True
    if mode != "write":
        print(
            f"  {core.cyan('Local Cleanup:'):13} "
            f"{core.yellow('READ-ONLY')} would remove "
            + ", ".join(path.name for path in paths)
        )
        return True
    try:
        for path in paths:
            path.unlink()
            print(f"  {core.cyan('Local Cleanup:'):13} removed {core.orange(path.name)}")
        return True
    except OSError as exc:
        print(f"  {core.red('ERROR: unable to remove competing local artwork: ' + str(exc))}")
        return False


def _write_sample_if_enabled(
    sample_dir: Path,
    sample_release: core.Release,
    candidate: core.Candidate,
    preserve: bool,
    samples_enabled: bool,
    summary: core.Summary,
) -> Path:
    if not samples_enabled:
        return sample_dir / (
            f"{core.sanitize(sample_release.artist_credit)}."
            f"{core.sanitize(sample_release.title)}.sample.{core.EXTENSIONS[candidate.format]}"
        )
    sample_path, unchanged = core.write_sample(sample_dir, sample_release, candidate, preserve)
    summary.samples_unchanged += int(unchanged)
    summary.samples_written += int(not unchanged)
    return sample_path


def apply_local_preflight(
    album: core.AlbumDir,
    sample_release: core.Release,
    preflight: dict[str, Any],
    cfg: dict[str, Any],
    format_order: list[str],
    sample_dir: Path,
    preserve: bool,
    samples_enabled: bool,
    summary: core.Summary,
) -> bool:
    candidate: core.Candidate = preflight["candidate"]
    action_kind = str(preflight["action"])
    mode = str(cfg.get("mode", "read")).strip().lower()
    output = core.section(cfg, "output")
    file_name = str(output.get("file_name", "cover")).strip()

    try:
        sample_path = _write_sample_if_enabled(
            sample_dir,
            sample_release,
            candidate,
            preserve,
            samples_enabled,
            summary,
        )

        if action_kind == "local-ideal":
            target = target_format_for_candidate(candidate, format_order)
            content, info = core.prepare_final(candidate, cfg, target)
            destination = candidate.path
            unchanged = destination.read_bytes() == content
            if mode == "write" and not unchanged:
                core.atomic_write(destination, content)
            action = "UNCHANGED" if unchanged else ("INSTALLED" if mode == "write" else "READ-ONLY (would normalize)")

        elif action_kind == "webp":
            content, info = core.prepare_final(candidate, cfg, "jpeg", allow_out_of_range=True)
            destination = album.path / f"{file_name}.jpg"
            unchanged = destination.exists() and destination.read_bytes() == content
            if mode == "write" and not unchanged:
                core.atomic_write(destination, content)
            action = "UNCHANGED" if unchanged else ("INSTALLED" if mode == "write" else "READ-ONLY (would install)")

        elif action_kind == "embedded-ideal":
            content, info = core.prepare_final(candidate, cfg, "jpeg")
            destination = album.path / f"{file_name}.jpg"
            unchanged = destination.exists() and destination.read_bytes() == content
            if mode == "write" and not unchanged:
                core.atomic_write(destination, content)
            action = "UNCHANGED" if unchanged else ("INSTALLED" if mode == "write" else "READ-ONLY (would install)")

        else:
            raise core.SplinedError(f"Unsupported local preflight action: {action_kind}")

    except Exception as exc:
        summary.failed += 1
        print(f"  {core.red('ERROR: local artwork preflight failed: ' + str(exc))}")
        return False

    summary.selected += 1
    if action == "INSTALLED":
        summary.installed += 1
        display = core.bracketed_text(action, core.green)
    elif action == "UNCHANGED":
        summary.unchanged += 1
        display = core.bracketed_text(action, core.cyan)
    else:
        summary.read_only += 1
        display = core.bracketed_text(action, core.yellow)

    print()
    print(f"  {core.cyan('Artwork:'):13} {display}")
    print(f"  {core.cyan('Sample:'):13} {core.orange(sample_path.name)}")
    print(
        f"  {core.cyan('Selected:'):13} "
        f"{core.magenta(provider_label(candidate.source))} {core.white('·')} "
        f"{core.orange(f'{candidate.width}x{candidate.height} {candidate.format.upper()}')} "
        f"{core.white('/')} {core.orange(destination.name)} {candidate_link(candidate)}"
    )
    aspect = core.aspect_ratio(candidate.width, candidate.height)
    off = core.aspect_deviation(candidate.width, candidate.height) * 100.0
    deviation = core.aspect_deviation(candidate.width, candidate.height)
    shape = (
        "square"
        if deviation == 0
        else (
            "square-equivalent"
            if deviation <= core.SQUARE_EQUIVALENT_TOLERANCE
            else "source-ratio"
        )
    )
    print(
        f"  {core.cyan('Conversion:'):13} "
        f"{core.white('shape=')}{core.orange(shape)}"
        f"{core.white(' · aspect=')}{core.orange(f'{aspect:.4f}')}"
        f"{core.white(' · off-square=')}{core.orange(f'{off:.2f}%')}"
        f"{core.white(' · cropped=')}{core.bool_color(bool(info.get('cropped', False)))}"
        f"{core.white(' · resized=')}{core.bool_color(bool(info.get('resized', False)))}"
        f"{core.white(' · converted=')}{core.bool_color(bool(info.get('converted', False)))}"
        f"{core.white(' · final=')}{core.orange(str(info['width']) + 'x' + str(info['height']))}"
    )
    return True


def preview_destination(
    album: core.AlbumDir,
    candidate: core.Candidate,
    cfg: dict[str, Any],
    format_order: list[str],
    allow_out_of_range: bool = False,
) -> tuple[Path, dict[str, Any]]:
    output = core.section(cfg, "output")
    preserve = bool(output.get("preserve_file", True))
    name = str(output.get("file_name", "cover")).strip()
    core.validate_file_name(name)
    target = target_format_for_candidate(candidate, format_order)
    content, info = core.prepare_final(
        candidate,
        cfg,
        target,
        allow_out_of_range=allow_out_of_range,
    )
    canonical = album.path / f"{name}.{core.EXTENSIONS[target]}"
    if not preserve:
        return canonical, info
    destination, _ = core.choose_destination(canonical, content, True)
    return destination, info


def finalize(
    album: core.AlbumDir,
    candidate: core.Candidate,
    cfg: dict[str, Any],
    format_order: list[str],
    allow_out_of_range: bool = False,
) -> tuple[str, Path, dict[str, Any]]:
    output = core.section(cfg, "output")
    preserve = bool(output.get("preserve_file", True))
    name = str(output.get("file_name", "cover")).strip()
    core.validate_file_name(name)
    target = target_format_for_candidate(candidate, format_order)
    content, info = core.prepare_final(
        candidate,
        cfg,
        target,
        allow_out_of_range=allow_out_of_range,
    )
    canonical = album.path / f"{name}.{core.EXTENSIONS[target]}"
    mode = str(cfg.get("mode", "read")).strip().lower()

    if not preserve:
        if mode == "read":
            unchanged = canonical.exists() and canonical.read_bytes() == content
            return ("UNCHANGED" if unchanged else "READ-ONLY (would install)"), canonical, info
        core.atomic_write(canonical, content)
        core.remove_numbered_cover_variants(album.path, name)
        return "INSTALLED", canonical, info

    destination, unchanged = core.choose_destination(canonical, content, True)
    if unchanged:
        return "UNCHANGED", destination, info
    if mode == "read":
        return "READ-ONLY (would install)", destination, info
    core.atomic_write(destination, content)
    return "INSTALLED", destination, info


def apply_selected_candidate(
    album: core.AlbumDir,
    sample_release: core.Release,
    candidate: core.Candidate,
    cfg: dict[str, Any],
    format_order: list[str],
    sample_dir: Path,
    preserve: bool,
    samples_enabled: bool,
    summary: core.Summary,
    allow_out_of_range: bool = False,
) -> bool:
    target = target_format_for_candidate(candidate, format_order)
    try:
        core.prepare_final(candidate, cfg, target, allow_out_of_range=allow_out_of_range)
    except Exception as exc:
        summary.failed += 1
        print(f"  {core.red('ERROR: final artwork validation failed: ' + str(exc))}")
        return False

    try:
        sample_path = _write_sample_if_enabled(
            sample_dir,
            sample_release,
            candidate,
            preserve,
            samples_enabled,
            summary,
        )
    except Exception as exc:
        summary.failed += 1
        print(f"  {core.red('ERROR: selected sample failed: ' + str(exc))}")
        return False

    try:
        action, destination, info = finalize(
            album,
            candidate,
            cfg,
            format_order,
            allow_out_of_range=allow_out_of_range,
        )
    except Exception as exc:
        summary.failed += 1
        print(f"  {core.red('ERROR: final artwork failed: ' + str(exc))}")
        return False

    summary.selected += 1
    if action == "INSTALLED":
        summary.installed += 1
        display = core.bracketed_text(action, core.green)
    elif action == "UNCHANGED":
        summary.unchanged += 1
        display = core.bracketed_text(action, core.cyan)
    else:
        summary.read_only += 1
        display = core.bracketed_text(action, core.yellow)

    print()
    print(f"  {core.cyan('Artwork:'):13} {display}")
    print(f"  {core.cyan('Sample:'):13} {core.orange(sample_path.name)}")
    print(
        f"  {core.cyan('Selected:'):13} "
        f"{core.magenta(provider_label(candidate.source))} {core.white('·')} "
        f"{core.orange(f'{candidate.width}x{candidate.height} {candidate.format.upper()}')} "
        f"{core.white('/')} {core.orange(destination.name)} {candidate_link(candidate)}"
    )
    aspect = core.aspect_ratio(candidate.width, candidate.height)
    off = core.aspect_deviation(candidate.width, candidate.height) * 100.0
    deviation = core.aspect_deviation(candidate.width, candidate.height)
    shape = (
        "square"
        if deviation == 0
        else (
            "square-equivalent"
            if deviation <= core.SQUARE_EQUIVALENT_TOLERANCE
            else "source-ratio"
        )
    )
    print(
        f"  {core.cyan('Conversion:'):13} "
        f"{core.white('shape=')}{core.orange(shape)}"
        f"{core.white(' · aspect=')}{core.orange(f'{aspect:.4f}')}"
        f"{core.white(' · off-square=')}{core.orange(f'{off:.2f}%')}"
        f"{core.white(' · cropped=')}{core.bool_color(bool(info.get('cropped', False)))}"
        f"{core.white(' · resized=')}{core.bool_color(bool(info.get('resized', False)))}"
        f"{core.white(' · converted=')}{core.bool_color(bool(info.get('converted', False)))}"
        f"{core.white(' · final=')}{core.orange(str(info['width']) + 'x' + str(info['height']))}"
    )
    return True


def _sample_release(record: dict[str, Any], album: core.AlbumDir) -> core.Release:
    release = record.get("release")
    if isinstance(release, core.Release):
        return release
    return core.Release(
        mbid=str(record.get("mbid") or ""),
        title=str(record.get("tag_album") or album.path.name),
        artist_credit=str(record.get("tag_artist") or "Unknown Artist"),
        release_group_id=None,
        release_group_title=None,
        track_count=None,
    )



BYPASS_HISTORY_VERSION = 1
BYPASS_HISTORY_FILE = "bypass-source-history.json"
_BYPASS_OVERRIDE = False


def bypass_history_path(history_dir: Path) -> Path:
    return history_dir / BYPASS_HISTORY_FILE


def load_bypass_history(path: Path) -> dict[str, Any]:
    empty = {"version": BYPASS_HISTORY_VERSION, "albums": {}}
    if not path.exists():
        return empty
    try:
        raw = core.json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, core.json.JSONDecodeError):
        return empty
    if not isinstance(raw, dict) or not isinstance(raw.get("albums"), dict):
        return empty
    return {"version": BYPASS_HISTORY_VERSION, "albums": {str(k): v for k, v in raw["albums"].items() if isinstance(v, dict)}}


def save_bypass_history(path: Path, history: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    history["version"] = BYPASS_HISTORY_VERSION
    body = (core.json.dumps(history, indent=2, sort_keys=True) + "\n").encode("utf-8")
    core.atomic_write(path, body)


def album_bypass_key(album: core.AlbumDir) -> str:
    return str(album.path)


def is_album_bypassed(history: dict[str, Any], album: core.AlbumDir) -> bool:
    return isinstance(history.get("albums", {}).get(album_bypass_key(album)), dict)


def record_album_bypass(path: Path, history: dict[str, Any], album: core.AlbumDir, mbid: str | None, artist: str, title: str, reason: str) -> None:
    history.setdefault("albums", {})[album_bypass_key(album)] = {
        "path": str(album.path),
        "album_mbid": str(mbid or ""),
        "artist": str(artist or ""),
        "album": str(title or album.path.name),
        "bypassed_at_unix": core.time.time(),
        "reason": str(reason),
    }
    save_bypass_history(path, history)


def _value(text: str, formatter=core.green) -> str:
    return core.white("[") + formatter(str(text)) + core.white("]")


def _field(label: str, text: str, formatter=core.green) -> str:
    return core.white(label + " ") + _value(text, formatter)


def candidate_crop_text(candidate: core.Candidate, projected: dict[str, Any]) -> str:
    pct = float(projected.get("aspect_deviation", core.aspect_deviation(candidate.width, candidate.height))) * 100.0
    if projected.get("crop_guarded"):
        return f"destructive {pct:.2f}%"
    if projected.get("cropped"):
        return f"minor {pct:.2f}%"
    return "none"


def candidate_compare_line(prefix: str, candidate: core.Candidate, cfg: dict[str, Any], format_order: list[str], local_file: str | None = None) -> str:
    projected = project_candidate(candidate, cfg, format_order)
    ideal = int(core.section(cfg, "range").get("ideal", 1800))
    source_text = "LOCAL" if candidate.source == "local" else provider_label(candidate.source)
    file_text = local_file or (str(core.section(cfg, "output").get("file_name", "cover")).strip() + "." + core.EXTENSIONS[target_format_for_candidate(candidate, format_order)])
    aspect = core.aspect_ratio(candidate.width, candidate.height)
    off = core.aspect_deviation(candidate.width, candidate.height) * 100.0
    crop = candidate_crop_text(candidate, projected)
    return (
        core.ljust_color(core.cyan(prefix), 12)
        + _field("File", file_text)
        + " " + _field("Source", source_text, core.magenta if candidate.source != "local" else core.green)
        + " " + _field("Res", f"{candidate.width}x{candidate.height}")
        + " " + _field("Range", projected["range_type"], core.color_range_type)
        + " " + _field("Ideal", f"{ideal}x{ideal}")
        + " " + _field("Distance", str(projected["distance"]))
        + " " + _field("Aspect", f"{aspect:.4f}")
        + " " + _field("Off-square", f"{off:.2f}%")
        + " " + _field("Crop", crop, core.red if projected.get("crop_guarded") else core.green)
    )


def render_local_suggested_comparison(local_candidate: core.Candidate, suggested: core.Candidate | None, cfg: dict[str, Any], format_order: list[str]) -> None:
    print()
    print(core.bold(core.cyan("Local & Suggested (s) Artwork Comparison")))
    print()
    print(candidate_compare_line("Local:", local_candidate, cfg, format_order, local_candidate.path.name))
    if suggested is None:
        print(core.ljust_color(core.cyan("Source (s):"), 12) + core.yellow("No provider candidate returned"))
        print(core.gray("-" * 150))
        print(core.ljust_color(core.cyan("Result:"), 12) + _field("Source", "local only", core.yellow))
        return

    print(candidate_compare_line("Source (s):", suggested, cfg, format_order))
    print(core.gray("-" * 150))
    lp = project_candidate(local_candidate, cfg, format_order)
    sp = project_candidate(suggested, cfg, format_order)
    delta = int(lp["distance"]) - int(sp["distance"])
    if delta > 0:
        resolution = f"{provider_label(suggested.source)} +{delta} px toward Ideal"
    elif delta < 0:
        resolution = f"Local +{-delta} px toward Ideal"
    else:
        resolution = "equal distance to Ideal"
    local_square = bool(lp.get("square"))
    source_square = bool(sp.get("square"))
    shape = "equal" if local_square == source_square else ("Local square-equivalent" if local_square else f"{provider_label(suggested.source)} square-equivalent")
    local_crop = candidate_crop_text(local_candidate, lp)
    source_crop = candidate_crop_text(suggested, sp)
    crop_risk = "equal" if local_crop == source_crop else f"Local {local_crop} / {provider_label(suggested.source)} {source_crop}"
    print(
        core.ljust_color(core.cyan("Result:"), 12)
        + _field("Res", resolution)
        + " " + _field("Shape", shape)
        + " " + _field("Crop risk", crop_risk, core.red if "destructive" in crop_risk else core.green)
        + " " + _field("Source", "remote replacement")
    )


def keep_existing_local(album: core.AlbumDir, sample_release: core.Release, local_candidate: core.Candidate, sample_dir: Path, preserve: bool, samples_enabled: bool, summary: core.Summary) -> bool:
    try:
        sample_path = _write_sample_if_enabled(sample_dir, sample_release, local_candidate, preserve, samples_enabled, summary)
    except Exception as exc:
        summary.failed += 1
        print(f"  {core.red('ERROR: local artwork keep failed: ' + str(exc))}")
        return False
    summary.selected += 1
    summary.unchanged += 1
    print()
    print(f"  {core.cyan('Artwork:'):13} {core.bracketed_text('UNCHANGED', core.cyan)}")
    print(f"  {core.cyan('Sample:'):13} {core.orange(sample_path.name)}")
    print(f"  {core.cyan('Selected:'):13} {core.magenta('Local')} {core.white('·')} {core.orange(f'{local_candidate.width}x{local_candidate.height} {local_candidate.format.upper()}')} {core.white('/')} {core.orange(local_candidate.path.name)} {candidate_link(local_candidate)}")
    return True


def choose_remote_suggestion(remote: list[core.Candidate], cfg: dict[str, Any], format_order: list[str]) -> core.Candidate | None:
    suggested = select_best(remote, cfg, format_order)
    return suggested if suggested is not None else fallback_suggested(remote, cfg, format_order)


def local_comparison_prompt(local_candidate: core.Candidate, remote: list[core.Candidate], cfg: dict[str, Any], format_order: list[str], mb_retry_available: bool = False) -> tuple[str, core.Candidate | None]:
    suggested = choose_remote_suggestion(remote, cfg, format_order)
    all_candidates = [local_candidate] + list(remote)
    render_local_suggested_comparison(local_candidate, suggested, cfg, format_order)
    if all_candidates:
        print()
        render_candidate_table(all_candidates, cfg, format_order, suggested=suggested, manual_fallback=True)
    while True:
        print()
        menu = (
            f"  {core.paint('PURPLE', '[s]')} use suggested replacement   "
            f"{core.cyan('[k]')} keep local {local_candidate.path.name}   "
            f"{core.cyan('[#]')} choose another candidate   "
            f"{core.cyan('[b]')} bypass album"
        )
        if mb_retry_available:
            menu += f"   {core.cyan('[m]')} MusicBrainz retry"
        print(menu)
        answer = core.read_input(
            "  Choice: ",
            kind="local-comparison",
            musicbrainz=mb_retry_available,
        ).strip().lower()
        if answer == "s":
            if suggested is None:
                print(f"  {core.yellow('No suggested provider candidate is available.')}")
                continue
            return "selected", suggested
        if answer == "k":
            return "keep", local_candidate
        if answer == "b":
            return "bypass", None
        if answer == "m" and mb_retry_available:
            return "mb-retry", None
        if answer.isdigit():
            number = int(answer)
            if 1 <= number <= len(all_candidates):
                chosen = all_candidates[number - 1]
                return ("keep", chosen) if chosen is local_candidate else ("selected", chosen)
            print(f"  {core.yellow('Choose a listed candidate number.')}")
            continue
        choices = "s, k, a listed number, b" + (", or m" if mb_retry_available else "")
        print(f"  Choose {choices}.")


def enhanced_history_candidates(
    history_entry: dict[str, Any] | None,
    album: core.AlbumDir,
) -> list[core.Candidate]:
    candidates: list[core.Candidate] = []
    for item in validated_enhanced_results(history_entry, album.path):
        path = item["path"]
        candidates.append(
            core.Candidate(
                core.Ref(
                    "enhanced",
                    path.name,
                    "",
                    front=True,
                    approved=True,
                    types=["Front", "Enhanced"],
                ),
                path,
                int(item["width"]),
                int(item["height"]),
                str(item["format"]),
                -3,
            )
        )
    return candidates


def run_scan_dir(
    config_file: Path,
    cfg: dict[str, Any],
    sources: list[str],
    scan_words: list[str] | None = None,
) -> int:
    scan = core.section(cfg, "scan")
    library = core.section(cfg, "library")
    output = core.section(cfg, "output")
    samples_cfg = core.section(cfg, "samples")
    mbcfg = core.section(cfg, "musicbrainz")

    root, library_root = core.resolve_scan_root(config_file, cfg, scan_words)
    cache = core.runtime_cache_dir(config_file, cfg)
    ignored = [str(item) for item in library.get("ignored_subs", [])]
    format_order = core.formats(cfg)
    preserve = bool(output.get("preserve_file", True))
    mode = str(cfg.get("mode", "read")).lower()
    samples_enabled = bool(samples_cfg.get("sample_write", True))

    core.output_settings(cfg)
    _, history_dir = core.ensure_runtime_directories(config_file, cfg, cache)
    core.prepare_run_cache(cache)
    inventory_started = core.time.perf_counter()
    core.emit_ui(
        "activity",
        category="inventory",
        state="start",
        source="filesystem",
        message="Lightweight library inventory started",
    )
    discovered_albums, ignored_dirs = core.inventory(
        root,
        ignored,
        str(output.get("file_name", "cover")),
    )
    inventory_elapsed = core.time.perf_counter() - inventory_started
    core.emit_ui(
        "activity",
        category="inventory",
        state="done",
        source="filesystem",
        message=(
            f"Lightweight inventory complete: {len(discovered_albums)} album(s) "
            f"in {inventory_elapsed:.3f}s"
        ),
    )

    if not discovered_albums:
        print(core.red("ERROR: No supported audio files were found in this directory or any sub-directory:"), file=sys.stderr)
        print(core.red(str(root)), file=sys.stderr)
        print(file=sys.stderr)
        print_help(config_file, cfg)
        return 2

    sample_dir = core.prepare_samples(cache)
    timeout_hours = core.scan_timeout_hours(cfg)
    completion_path = core.scan_completion_history_path(history_dir)
    completion_history = core.load_scan_completion_history(completion_path, cfg)
    completion_path.parent.mkdir(parents=True, exist_ok=True)

    bypass_path = bypass_history_path(history_dir)
    bypass_history = load_bypass_history(bypass_path)
    bypass_path.parent.mkdir(parents=True, exist_ok=True)

    albums: list[core.AlbumDir] = []
    postponed_albums: list[tuple[core.AlbumDir, float]] = []
    track_cache: dict[str, list[core.Track]] = {}
    if core.tui_active():
        bypassed_paths = (
            set()
            if _BYPASS_OVERRIDE
            else {str(value) for value in bypass_history.get("albums", {})}
        )
        albums, track_cache, bypass_overrides, timeout_paths, sources = (
            core.prepare_tui_library_selection(
                config_file,
                cfg,
                sources,
                root,
                discovered_albums,
                completion_history,
                timeout_hours,
                bypassed_paths=bypassed_paths,
            )
        )
        # The TUI can override bypass only through its explicit confirmation;
        # timeout-active rows remain unselected by the shared model.
        albums = [
            album
            for album in albums
            if str(album.path) not in bypassed_paths
            or str(album.path) in bypass_overrides
        ]
        postponed_albums = [
            (album, 0.0)
            for album in discovered_albums
            if str(album.path) in timeout_paths
        ]
        mode = str(cfg.get("mode", "read")).lower()
    else:
        for album in discovered_albums:
            if is_album_bypassed(bypass_history, album) and not _BYPASS_OVERRIDE:
                print(core.red(f"BYPASSED ALBUM: {album.path} · saved bypass is active; use -bp to override for this run."))
                continue

            postponed, age_hours = core.scan_completion_status(
                completion_history,
                album,
                cfg,
                sources,
                timeout_hours,
            )
            if postponed:
                postponed_albums.append((album, age_hours))
                core.debug_log(
                    f"scan.postponed album={str(album.path)!r} "
                    f"age_hours={age_hours:.3f} timeout_hours={timeout_hours:g}"
                )
            else:
                albums.append(album)

    http = core.Http()
    _, mbmode = core.mb_headers(config_file, cfg)
    summary = core.Summary(albums=len(discovered_albums), postponed=len(postponed_albums))
    history_path = core.source_history_path(history_dir)
    source_history = (
        core.load_source_history(history_path)
        if bool(core.section(cfg, "history").get("enabled", True))
        else core.empty_source_history()
    )
    history_path.parent.mkdir(parents=True, exist_ok=True)
    api_queried: set[str] = set()

    core.emit_ui(
        "scan_start",
        root=str(root),
        total=len(albums),
        discovered=len(discovered_albums),
        postponed=len(postponed_albums),
        mode=mode,
        phase="authority",
    )

    records: list[dict[str, Any]] = []
    for inventory_index, album in enumerate(albums, 1):
        core.emit_ui(
            "album",
            index=inventory_index,
            total=len(albums),
            path=str(album.path),
            artist="",
            album=album.path.name,
            authority="Reading tags and MusicBrainz authority",
            fallback_reason="",
            phase="authority",
        )
        record: dict[str, Any] = {
            "album": album,
            "tracks": None,
            "file_count": 0,
            "compilation": "Standard",
            "tag_album": None,
            "tag_artist": None,
            "valid": {},
            "missing": [],
            "invalid": [],
            "mbid": None,
            "release": None,
            "fallback_reason": None,
        }
        try:
            tracks = track_cache.get(str(album.path)) or [
                core.read_track(path) for path in album.audio_files
            ]
            record["tracks"] = tracks
            record["file_count"] = len(tracks)
            record["compilation"] = (
                "Compilation"
                if any((track.compilation or "").strip() == "1" for track in tracks)
                else "Standard"
            )
            record["tag_album"] = core.tagged_album(tracks)
            record["tag_artist"] = core.tagged_artist(tracks)
            valid, missing, invalid = core.audit_album_ids(tracks)
            record["valid"] = valid
            record["missing"] = missing
            record["invalid"] = invalid

            if len(valid) != 1:
                if len(valid) > 1:
                    record["fallback_reason"] = "MULTIPLE MBIDS"
                elif invalid:
                    record["fallback_reason"] = "INVALID MBID"
                else:
                    record["fallback_reason"] = "NO MBID FOUND"
            else:
                mbid = next(iter(valid))
                record["mbid"] = mbid
                try:
                    release = core.lookup_release(http, config_file, cfg, mbid)
                    record["release"] = release
                    if record["tag_album"] and core.normalize_text(record["tag_album"]) != core.normalize_text(release.title):
                        record["fallback_reason"] = "TAG / RELEASE MISMATCH"
                except Exception as exc:
                    record["fallback_reason"] = core.fallback_reason_from_error(exc)
                    record["mb_error"] = str(exc)
        except Exception as exc:
            record["fatal_error"] = str(exc)
            record["fallback_reason"] = "TAG READ FAILURE"
        records.append(record)

    fallback_records = [record for record in records if record.get("fallback_reason")]
    normal_records = [record for record in records if not record.get("fallback_reason")]
    ordered_records = fallback_records + normal_records

    provider_list = [source for source in sources if source != "discogs"]
    output_list = [
        f"{str(output.get('file_name', 'cover')).strip()}.{core.EXTENSIONS[fmt]}"
        for fmt in format_order
    ]

    print(core.bold(core.cyan("SPLINED LIBRARY SCAN")))
    print()
    print(f"  {core.cyan('Mode:'):14} {core.orange(mode.capitalize())}")
    print(f"  {core.cyan('Mutation:'):14} {core.red('enabled') if mode == 'write' else core.green('disabled')}")
    print()
    print(f"  {core.cyan('Library:'):14} {core.white(str(library_root))}")
    print(f"  {core.cyan('Library Scan:'):14} {core.white(str(root))}")
    print()
    print(f"  {core.cyan('Samples:'):14} {core.green('enabled') if samples_enabled else core.red('disabled')}")
    print(f"  {core.cyan('Cache:'):14} {core.white(str(cache))}")
    print(f"  {core.cyan('Sample Dir:'):14} {core.white(str(sample_dir))}")
    print(f"  {core.cyan('History:'):14} {core.white(str(history_dir))}")
    if core._DEBUG_ENABLED and core._DEBUG_PATH is not None:
        print(f"  {core.cyan('Debug Log:'):14} {core.white(str(core._DEBUG_PATH))}")
    print()
    print(
        core.ljust_color(core.cyan("Run:"), 14)
        + core.ljust_color(core.white("Preserve"), 11)
        + core.bracketed_text(core.bool_text(preserve), core.green if preserve else core.red)
        + " "
        + core.white("Albums") + " " + core.bracketed_text(str(len(discovered_albums)), core.white)
        + " "
        + core.white("Postponed") + " " + core.bracketed_text(
            str(len(postponed_albums)), core.cyan if postponed_albums else core.white
        )
        + " "
        + core.white("Ignored") + " " + core.bracketed_text(str(len(ignored_dirs)), core.white)
    )
    print(
        core.ljust_color(core.cyan("Timeout:"), 14)
        + core.bracketed_text(core.format_timeout_hours(timeout_hours) + ("" if timeout_hours <= 0 else "h"), core.green)
        + core.white(" completed-album reprocessing window")
    )
    print(core.ljust_color(core.cyan("Providers:"), 14) + core.bracketed_list(provider_list, core.green))
    print(core.ljust_color(core.cyan("Source:"), 14) + core.bracketed_list(sources, core.green))
    print(core.cyan("Authentication:"))
    for provider, auth_mode in core.authentication_statuses(config_file, cfg):
        print(
            "  "
            + core.ljust_color(core.cyan(provider), 14)
            + core.white(auth_mode)
        )
    mb_rhs = (
        core.magenta(mbmode + ", Retry ")
        + core.white("[") + core.magenta(str(int(mbcfg.get("retry_max", 2)))) + core.white("]")
        + core.magenta(" Delay ") + core.white("[") + core.magenta(f'{float(mbcfg.get("mb_min_delay", 1.05)):.2f}s') + core.white("]")
        + core.magenta(" Timeout ") + core.white("[") + core.magenta(f'{float(mbcfg.get("mb_recording_timeout", 7.0)):.1f}s') + core.white("]")
    )
    print(core.ljust_color(core.cyan("MusicBrainz:"), 14) + mb_rhs)
    if output_list:
        rest = " / ".join(output_list[1:])
        print(
            core.ljust_color(core.cyan("Output:"), 14)
            + core.white("[") + core.orange(output_list[0])
            + core.white(" / " + rest if rest else "") + core.white("]")
        )
    else:
        print(core.ljust_color(core.cyan("Output:"), 14) + core.white("[]"))
    print()

    for run_index, record in enumerate(ordered_records, 1):
        album: core.AlbumDir = record["album"]
        file_count = int(record.get("file_count", 0))
        compilation = str(record.get("compilation") or "Standard")
        tag_album = str(record.get("tag_album") or album.path.name)
        tag_artist = str(record.get("tag_artist") or "")
        valid = record.get("valid") or {}
        missing = record.get("missing") or []
        invalid = record.get("invalid") or []
        mbid = record.get("mbid")
        release: core.Release | None = record.get("release")
        fallback_reason = record.get("fallback_reason")

        core.emit_ui(
            "album",
            index=run_index,
            total=len(ordered_records),
            path=str(album.path),
            artist=tag_artist,
            album=tag_album,
            authority="Fallback" if fallback_reason else "ExactAlbumId",
            fallback_reason=str(fallback_reason or ""),
            track_count=file_count,
            compilation=compilation,
            mbid=str(mbid or ""),
            tag_state="UNMATCHED" if fallback_reason else "MATCHED",
            phase="processing",
        )

        print(core.bold(core.cyan(f"[{run_index}/{len(ordered_records)}] {core.album_path_text(album.path)}")))

        if record.get("fatal_error"):
            summary.failed += 1
            print(f"  {core.red('ERROR: ' + str(record['fatal_error']))}\n")
            continue

        preflight = inspect_local_preflight(album, cfg, cache, format_order)
        for diagnostic in preflight["diagnostics"]:
            print(f"  {core.cyan('Local Art:'):13} {core.yellow(diagnostic)}")

        if preflight.get("webp_still"):
            webp_source: core.Candidate = preflight["webp_source"]
            still_candidate: core.Candidate = preflight["candidate"]
            still_projected = project_candidate(still_candidate, cfg, format_order)
            info = preflight["still_info"]
            aspect = core.aspect_ratio(webp_source.width, webp_source.height)
            off = core.aspect_deviation(webp_source.width, webp_source.height) * 100.0
            deviation = core.aspect_deviation(webp_source.width, webp_source.height)
            shape = (
                "square"
                if deviation == 0
                else (
                    "square-equivalent"
                    if deviation <= core.SQUARE_EQUIVALENT_TOLERANCE
                    else "source-ratio"
                )
            )
            still_status = (
                "UNCHANGED"
                if bool(preflight.get("still_unchanged"))
                else ("INSTALLED" if mode == "write" else "READ-ONLY")
            )
            print(
                f"  {core.cyan('Local Art:'):13} "
                f"{core.magenta('WEBP preserved')} {core.white('·')} "
                f"{core.orange(f'{webp_source.width}x{webp_source.height} WEBP')} "
                f"{candidate_link(webp_source)}"
            )
            print(
                f"  {core.cyan('Still:'):13} "
                f"{core.orange(still_candidate.path.name)} {core.white('·')} "
                f"{core.orange(f'{still_candidate.width}x{still_candidate.height} JPEG')} "
                f"{core.white('·')} "
                f"{core.color_range_type(still_projected['range_type'])} "
                f"{core.bracketed_text(still_status, core.green if still_status in {'UNCHANGED', 'INSTALLED'} else core.yellow)}"
            )
            print(
                f"  {core.cyan('Conversion:'):13} "
                f"{core.white('shape=')}{core.orange(shape)}"
                f"{core.white(' · aspect=')}{core.orange(f'{aspect:.4f}')}"
                f"{core.white(' · off-square=')}{core.orange(f'{off:.2f}%')}"
                f"{core.white(' · cropped=')}{core.bool_color(bool(info.get('cropped', False)))}"
                f"{core.white(' · resized=')}{core.bool_color(bool(info.get('resized', False)))}"
                f"{core.white(' · converted=')}{core.bool_color(bool(info.get('converted', False)))}"
                f"{core.white(' · final=')}{core.orange(str(info['width']) + 'x' + str(info['height']))}"
            )

        if not cleanup_competing_static(preflight["cleanup"], mode):
            summary.failed += 1
            print()
            continue

        if preflight["action"] in {"webp", "local-ideal", "embedded-ideal"}:
            candidate: core.Candidate = preflight["candidate"]
            projected = project_candidate(candidate, cfg, format_order)
            print(
                f"  {core.cyan('Local Art:'):13} "
                f"{core.magenta(provider_label(candidate.source))} {core.white('·')} "
                f"{core.orange(f'{candidate.width}x{candidate.height} {candidate.format.upper()}')} "
                f"{core.white('·')} {core.color_range_type(projected['range_type'])} {candidate_link(candidate)}"
            )
            if apply_local_preflight(
                album,
                _sample_release(record, album),
                preflight,
                cfg,
                format_order,
                sample_dir,
                preserve,
                samples_enabled,
                summary,
            ):
                summary.resolved += 1
                core.record_scan_completion(
                    completion_path,
                    completion_history,
                    album,
                    cfg,
                    sources,
                    f"local-preflight-{preflight['action']}",
                )
            else:
                summary.unresolved += 1
            print()
            continue

        local_fallback: list[core.Candidate] = list(preflight["fallback"])
        history_entry = completion_history.get("albums", {}).get(str(album.path))
        enhanced_candidates = enhanced_history_candidates(
            history_entry if isinstance(history_entry, dict) else None,
            album,
        )
        if local_fallback:
            candidate = local_fallback[0]
            projected = project_candidate(candidate, cfg, format_order)
            print(
                f"  {core.cyan('Local Art:'):13} retained for source comparison · "
                f"{core.magenta(provider_label(candidate.source))} "
                f"{core.orange(f'{candidate.width}x{candidate.height}')} "
                f"{core.color_range_type(projected['range_type'])} {candidate_link(candidate)}"
            )

        if fallback_reason:
            print(f"  {core.cyan('Tracks:'):13} {core.bracketed_text(str(file_count), core.red)}")
            print(f"  {core.cyan('Compilation:'):13} {core.white(compilation)}")
            print()
            print(f"  {core.cyan('Authority:'):13} {core.paint('PURPLE', 'N/A · FALLBACK MODE')}")
            if release is not None:
                mb_count_text = "?" if release.track_count is None else str(release.track_count)
                count_fmt = core.green if release.track_count == file_count else core.red
                print(f"  {core.cyan('MB Artist:'):13} {core.magenta(release.artist_credit or 'N/A')}")
                print(f"  {core.cyan('MB Release:'):13} {core.orange(release.title)} {core.bracketed_text(mb_count_text, count_fmt)}")
            else:
                print(f"  {core.cyan('MB Artist:'):13} {core.gray('N/A')}")
                print(f"  {core.cyan('MB Release:'):13} {core.gray('N/A')}")

            reason_text = str(fallback_reason)
            mbid_display = core.magenta(mbid) if mbid else core.gray("N/A")
            print(
                f"  {core.cyan('Tagged Album:'):13} "
                f"{core.orange(tag_album)} {core.bracketed_text('UNMATCHED', core.red)} / "
                f"{mbid_display} {core.paint('PURPLE', '· ' + reason_text)}"
            )

            if len(valid) != 1 or missing or invalid:
                print(f"  {core.cyan('MBID Evidence:')}")
                for evidence_mbid, paths in sorted(valid.items(), key=lambda item: (-len(item[1]), item[0])):
                    print(f"    - {len(paths)}/{file_count} tracks: {core.magenta(evidence_mbid)}")
                    print(f"      Files: {core.gray(core.compact(paths))}")
                if missing:
                    print(f"    - Missing/blank: {len(missing)}/{file_count} tracks")
                    print(f"      Files: {core.gray(core.compact(missing))}")
                for path_value, raw_value in invalid:
                    print(f"    - {core.yellow('Invalid MBID')}: {path_value.name}: {core.yellow(raw_value)}")

            if not tag_artist or not tag_album:
                summary.unresolved += 1
                print(f"  {core.red('Fallback requires tagged Artist + Album values.')}")
                print(f"  {core.cyan('Artwork:'):13} {core.bracketed_text('SKIPPED', core.yellow)}\n")
                continue

            search_artist = tag_artist
            search_album = tag_album
            recovered_release: core.Release | None = None
            diagnostics: list[tuple[str, str]] = []
            completion_outcome: str | None = None
            mb_retry_available = bool(record.get("mb_error"))

            while True:
                if recovered_release is not None:
                    refs, diagnostics = core.discover_all(
                        http,
                        config_file,
                        cfg,
                        recovered_release,
                        sources,
                        queried_sources=api_queried,
                    )
                    sample_release = recovered_release
                else:
                    refs, diagnostics = core.discover_fallback(
                        http,
                        config_file,
                        cfg,
                        search_artist,
                        search_album,
                        sources,
                        release_mbid=mbid,
                        queried_sources=api_queried,
                    )
                    sample_release = core.Release(
                        mbid=mbid or "",
                        title=search_album,
                        artist_credit=search_artist,
                        release_group_id=None,
                        release_group_title=None,
                        track_count=None,
                    )

                remote, download_diag = core.download_candidates(http, refs, sources, cache, cfg)
                diagnostics += download_diag
                candidates = local_fallback + enhanced_candidates + remote

                if local_fallback:
                    local_candidate = local_fallback[0]
                    comparison_action, comparison_candidate = local_comparison_prompt(local_candidate, remote, cfg, format_order, mb_retry_available=mb_retry_available)

                    if comparison_action == "mb-retry":
                        recovered = core.musicbrainz_picker(http, config_file, cfg, search_artist, search_album, exact_mbid=mbid if mbid and release is None else None)
                        if recovered is not None:
                            recovered_release = recovered
                            mbid = recovered.mbid
                            search_artist = recovered.artist_credit or search_artist
                            search_album = recovered.title or search_album
                            mb_retry_available = False
                        continue

                    if comparison_action == "bypass":
                        record_album_bypass(bypass_path, bypass_history, album, mbid, search_artist, search_album, "local-source-comparison")
                        summary.unresolved += 1
                        completion_outcome = "fallback-bypassed-persistent"
                        print(f"  {core.cyan('Artwork:'):13} {core.bracketed_text('BYPASSED', core.yellow)}")
                        break

                    if comparison_action == "keep":
                        if keep_existing_local(album, sample_release, local_candidate, sample_dir, preserve, samples_enabled, summary):
                            summary.resolved += 1
                            completion_outcome = "fallback-local-kept"
                        else:
                            summary.unresolved += 1
                        break

                    if comparison_action == "selected" and comparison_candidate is not None:
                        if apply_selected_candidate(album, sample_release, comparison_candidate, cfg, format_order, sample_dir, preserve, samples_enabled, summary, allow_out_of_range=True):
                            summary.resolved += 1
                            completion_outcome = "fallback-local-source-selected"
                        else:
                            summary.unresolved += 1
                        break

                normal_best = select_best(candidates, cfg, format_order)
                if normal_best is not None:
                    print()
                    authority_text = "ExactAlbumId · RECOVERED" if recovered_release else "ArtistAlbumFallback · AUTO"
                    print(f"  {core.cyan('Authority:'):13} {core.green(authority_text)}")
                    preview_dest, _ = preview_destination(album, normal_best, cfg, format_order)
                    print(
                        f"  {core.cyan('Candidates:'):13} "
                        f"{core.white(str(len(candidates)))} {core.white('Chose')} "
                        f"{core.bracketed_text(provider_label(normal_best.source), core.magenta)} "
                        f"{core.white('File')} {core.bracketed_text(preview_dest.name, core.orange)} "
                        f"{candidate_link(normal_best)}"
                    )
                    render_candidate_table(candidates, cfg, format_order, selected=normal_best)
                    if diagnostics:
                        print(f"  {core.cyan('Diagnostics:'):13}")
                        for source, message in diagnostics:
                            print(f"    - {core.magenta(provider_label(source))}: {message}")
                    if apply_selected_candidate(
                        album,
                        sample_release,
                        normal_best,
                        cfg,
                        format_order,
                        sample_dir,
                        preserve,
                        samples_enabled,
                        summary,
                    ):
                        summary.resolved += 1
                        completion_outcome = "fallback-auto-selected"
                    else:
                        summary.unresolved += 1
                    break

                top = fallback_top_candidates(candidates, cfg, format_order, 10)
                suggested = fallback_suggested(top, cfg, format_order)
                print()
                preview_name = str(output.get("file_name", "cover")).strip() + "." + core.EXTENSIONS[format_order[0]]
                print(
                    f"  {core.cyan('Candidates:'):13} "
                    f"{core.white(str(len(top)))} {core.paint('PURPLE', '· FALLBACK PICKER')} "
                    f"{core.white('File')} {core.bracketed_text(preview_name, core.orange)} "
                    f"{candidate_link(suggested) if suggested else ''}"
                )
                if top:
                    render_candidate_table(top, cfg, format_order, suggested=suggested, manual_fallback=True)
                else:
                    print(f"  {core.yellow('No fallback artwork candidates were returned.')}")

                if diagnostics:
                    print(f"  {core.cyan('Diagnostics:'):13}")
                    for source, message in diagnostics:
                        print(f"    - {core.magenta(provider_label(source))}: {message}")

                print()
                menu = (
                    f"  {core.paint('PURPLE', '[s]')} suggested exception   "
                    f"{core.cyan('[#]')} choose number   "
                    f"{core.cyan('[f]')} fuzzy search   "
                )
                if mb_retry_available:
                    menu += f"{core.cyan('[m]')} MusicBrainz retry   "
                menu += f"{core.cyan('[b]')} bypass"
                print(menu)
                answer = core.read_input(
                    "  Choice: ",
                    kind="fallback-picker",
                ).strip().lower()

                if answer == "b":
                    record_album_bypass(bypass_path, bypass_history, album, mbid, search_artist, search_album, "fallback-recovery")
                    summary.unresolved += 1
                    completion_outcome = "fallback-bypassed-persistent"
                    print(f"  {core.cyan('Artwork:'):13} {core.bracketed_text('BYPASSED', core.yellow)}")
                    break

                if answer == "s":
                    if suggested is None:
                        print(f"  {core.yellow('No suggested fallback candidate is available.')}")
                        continue
                    if apply_selected_candidate(
                        album,
                        sample_release,
                        suggested,
                        cfg,
                        format_order,
                        sample_dir,
                        preserve,
                        samples_enabled,
                        summary,
                        allow_out_of_range=True,
                    ):
                        summary.resolved += 1
                        completion_outcome = "fallback-manual-suggested"
                    else:
                        summary.unresolved += 1
                    break

                if answer.isdigit():
                    number = int(answer)
                    if 1 <= number <= len(top):
                        selected = top[number - 1]
                        if apply_selected_candidate(
                            album,
                            sample_release,
                            selected,
                            cfg,
                            format_order,
                            sample_dir,
                            preserve,
                            samples_enabled,
                            summary,
                            allow_out_of_range=True,
                        ):
                            summary.resolved += 1
                            completion_outcome = "fallback-manual-number"
                        else:
                            summary.unresolved += 1
                        break
                    print(f"  {core.yellow('Choose a listed candidate number.')}")
                    continue

                if answer == "f":
                    entered_artist = core.read_input(
                        f"  Artist [{search_artist}]: ",
                        kind="artist",
                    ).strip()
                    entered_album = core.read_input(
                        f"  Album  [{search_album}]: ",
                        kind="album",
                    ).strip()
                    if entered_artist:
                        search_artist = entered_artist
                    if entered_album:
                        search_album = entered_album
                    recovered_release = None
                    continue

                if answer == "m" and mb_retry_available:
                    recovered = core.musicbrainz_picker(
                        http,
                        config_file,
                        cfg,
                        search_artist,
                        search_album,
                        exact_mbid=mbid if mbid and release is None else None,
                    )
                    if recovered is not None:
                        recovered_release = recovered
                        mbid = recovered.mbid
                        search_artist = recovered.artist_credit or search_artist
                        search_album = recovered.title or search_album
                        mb_retry_available = False
                    continue

                choices = "s, a listed number, f, m, or b" if mb_retry_available else "s, a listed number, f, or b"
                print(f"  Choose {choices}.")

            if completion_outcome is not None:
                core.record_scan_completion(
                    completion_path,
                    completion_history,
                    album,
                    cfg,
                    sources,
                    completion_outcome,
                )
            print()
            continue

        if release is None or mbid is None:
            raise core.SplinedError("Internal error: exact MusicBrainz release state is incomplete.")
        mb_count_text = "?" if release.track_count is None else str(release.track_count)
        count_match = release.track_count is not None and release.track_count == file_count
        count_fmt = core.green if count_match else core.red

        print(f"  {core.cyan('Tracks:'):13} {core.bracketed_text(str(file_count), count_fmt)}")
        print(f"  {core.cyan('Compilation:'):13} {core.white(compilation)}")
        print()
        print(f"  {core.cyan('Authority:'):13} {core.green('ExactAlbumId')}")
        print(f"  {core.cyan('MB Artist:'):13} {core.green(release.artist_credit)}")
        print(f"  {core.cyan('MB Release:'):13} {core.orange(release.title)} {core.bracketed_text(mb_count_text, count_fmt)}")
        print(
            f"  {core.cyan('Tagged Album:'):13} "
            f"{core.orange(tag_album or release.title)} {core.bracketed_text('MATCHED', core.green)} / {core.magenta(mbid)}"
        )
        print()

        remote, diagnostics, _ = core.discover_normal_ranked(
            http,
            config_file,
            cfg,
            release,
            sources,
            cache,
            format_order,
            source_history,
            queried_sources=api_queried,
        )
        candidates = local_fallback + enhanced_candidates + remote

        if local_fallback:
            local_candidate = local_fallback[0]
            comparison_action, comparison_candidate = local_comparison_prompt(local_candidate, remote, cfg, format_order, mb_retry_available=False)

            if comparison_action == "bypass":
                record_album_bypass(bypass_path, bypass_history, album, mbid, release.artist_credit, release.title, "local-source-comparison")
                summary.unresolved += 1
                print(f"  {core.cyan('Artwork:'):13} {core.bracketed_text('BYPASSED', core.yellow)}")
                core.record_scan_completion(completion_path, completion_history, album, cfg, sources, "normal-bypassed-persistent")
                print()
                continue

            if comparison_action == "keep":
                if keep_existing_local(album, release, local_candidate, sample_dir, preserve, samples_enabled, summary):
                    summary.resolved += 1
                    core.record_scan_completion(completion_path, completion_history, album, cfg, sources, "normal-local-kept")
                else:
                    summary.unresolved += 1
                print()
                continue

            if comparison_action == "selected" and comparison_candidate is not None:
                if apply_selected_candidate(album, release, comparison_candidate, cfg, format_order, sample_dir, preserve, samples_enabled, summary, allow_out_of_range=True):
                    summary.resolved += 1
                    core.record_source_selection(history_path, source_history, comparison_candidate, cfg, format_order)
                    core.record_scan_completion(completion_path, completion_history, album, cfg, sources, "normal-local-source-selected")
                else:
                    summary.unresolved += 1
                print()
                continue

        best = select_best(candidates, cfg, format_order)

        if best is None:
            acceptable_count = sum(1 for candidate in candidates if project_candidate(candidate, cfg, format_order)["acceptable"])
            core.debug_log(
                f"normal.selection album={release.title!r} "
                f"candidates={len(candidates)} acceptable={acceptable_count} chosen=none"
            )

            if not candidates:
                summary.resolved += 1
                print(
                    f"  {core.cyan('Candidates:'):13} {core.white('0')} "
                    f"{core.white('Chose')} {core.bracketed_text('none', core.magenta)}"
                )
                print(
                    f"  {core.cyan('Discovery:'):13} "
                    f"{core.yellow('No artwork candidates were returned by queried sources')}"
                )
                if diagnostics:
                    print(f"  {core.cyan('Diagnostics:'):13}")
                    for source, message in diagnostics:
                        print(f"    - {core.magenta(provider_label(source))}: {message}")
                print(f"  {core.cyan('Artwork:'):13} {core.bracketed_text('SKIPPED', core.yellow)}")
                core.record_scan_completion(
                    completion_path,
                    completion_history,
                    album,
                    cfg,
                    sources,
                    "normal-no-candidates",
                )
                print()
                continue

            manual_pool = sorted(candidates, key=lambda candidate: fallback_sort_key(candidate, cfg, format_order))
            suggested = fallback_suggested(manual_pool, cfg, format_order)
            preview_name = str(output.get("file_name", "cover")).strip() + "." + core.EXTENSIONS[format_order[0]]
            print(
                f"  {core.cyan('Candidates:'):13} {core.white(str(len(manual_pool)))} "
                f"{core.paint('PURPLE', '· OUT-OF-RANGE PICKER')} "
                f"{core.white('File')} {core.bracketed_text(preview_name, core.orange)} "
                f"{candidate_link(suggested) if suggested else ''}"
            )
            print(
                f"  {core.cyan('Picker Reason:'):13} "
                f"{core.yellow('no candidate projects inside Artwork Resolution Range')}"
            )
            print(
                f"  {core.cyan('Range:'):13} minimum={core.orange(str(int(core.section(cfg, 'range').get('min', 1200))))} "
                f"ideal={core.green(str(int(core.section(cfg, 'range').get('ideal', 1800))))}"
            )
            render_candidate_table(
                manual_pool,
                cfg,
                format_order,
                suggested=suggested,
                manual_fallback=True,
            )

            if diagnostics:
                print(f"  {core.cyan('Diagnostics:'):13}")
                for source, message in diagnostics:
                    print(f"    - {core.magenta(provider_label(source))}: {message}")

            while True:
                print()
                print(
                    f"  {core.paint('PURPLE', '[s]')} suggested closest candidate   "
                    f"{core.cyan('[#]')} choose exact candidate   "
                    f"{core.cyan('[b]')} bypass"
                )
                answer = core.read_input(
                    "  Choice: ",
                    kind="out-of-range-picker",
                ).strip().lower()

                if answer == "b":
                    record_album_bypass(bypass_path, bypass_history, album, mbid, release.artist_credit, release.title, "normal-out-of-range")
                    summary.resolved += 1
                    print(f"  {core.cyan('Artwork:'):13} {core.bracketed_text('BYPASSED', core.yellow)}")
                    core.record_scan_completion(
                        completion_path,
                        completion_history,
                        album,
                        cfg,
                        sources,
                        "normal-out-of-range-bypassed",
                    )
                    break

                if answer == "s":
                    if suggested is None:
                        print(f"  {core.yellow('No suggested candidate is available.')}")
                        continue
                    if apply_selected_candidate(
                        album,
                        release,
                        suggested,
                        cfg,
                        format_order,
                        sample_dir,
                        preserve,
                        samples_enabled,
                        summary,
                        allow_out_of_range=True,
                    ):
                        summary.resolved += 1
                        core.record_scan_completion(
                            completion_path,
                            completion_history,
                            album,
                            cfg,
                            sources,
                            "normal-out-of-range-suggested",
                        )
                    else:
                        summary.unresolved += 1
                    break

                if answer.isdigit():
                    number = int(answer)
                    if 1 <= number <= len(manual_pool):
                        selected = manual_pool[number - 1]
                        if apply_selected_candidate(
                            album,
                            release,
                            selected,
                            cfg,
                            format_order,
                            sample_dir,
                            preserve,
                            samples_enabled,
                            summary,
                            allow_out_of_range=True,
                        ):
                            summary.resolved += 1
                            core.record_scan_completion(
                                completion_path,
                                completion_history,
                                album,
                                cfg,
                                sources,
                                "normal-out-of-range-number",
                            )
                        else:
                            summary.unresolved += 1
                        break
                    print(f"  {core.yellow('Choose a listed candidate number.')}")
                    continue

                print("  Choose s, a listed number, or b.")

            print()
            continue

        summary.resolved += 1
        core.debug_log(
            f"normal.selection album={release.title!r} "
            f"candidates={len(candidates)} chosen_source={best.source} "
            f"chosen_id={best.ref.id}"
        )
        preview_dest, _ = preview_destination(album, best, cfg, format_order)
        print(
            f"  {core.cyan('Candidates:'):13} "
            f"{core.white(str(len(candidates)))} {core.white('Chose')} "
            f"{core.bracketed_text(provider_label(best.source), core.magenta)} "
            f"{core.white('File')} {core.bracketed_text(preview_dest.name, core.orange)} "
            f"{candidate_link(best)}"
        )
        render_candidate_table(candidates, cfg, format_order, selected=best)

        if diagnostics:
            print(f"  {core.cyan('Diagnostics:'):13}")
            for source, message in diagnostics:
                print(f"    - {core.magenta(provider_label(source))}: {message}")

        normal_apply_ok = apply_selected_candidate(
            album,
            release,
            best,
            cfg,
            format_order,
            sample_dir,
            preserve,
            samples_enabled,
            summary,
        )
        if normal_apply_ok:
            core.record_source_selection(history_path, source_history, best, cfg, format_order)
            core.record_scan_completion(
                completion_path,
                completion_history,
                album,
                cfg,
                sources,
                "normal-selected",
            )
        print()

    print(core.bold(core.cyan(f"SPLINED SCAN LIBRARY {mode.upper()} SUMMARY")))
    print()
    album_cells = [
        core.ljust_color(core.cyan("Albums:"), 10),
        core.ljust_color(core.white("Selected"), 11) + core.bracketed_text(str(summary.selected), core.green if summary.selected else core.white),
        core.ljust_color(core.white("Resolved"), 11) + core.bracketed_text(str(summary.resolved), core.green if summary.resolved else core.white),
        core.ljust_color(core.white("Postponed"), 12) + core.bracketed_text(str(summary.postponed), core.cyan if summary.postponed else core.white),
        core.ljust_color(core.white("Unresolved"), 11) + core.bracketed_text(str(summary.unresolved), core.green if summary.unresolved == 0 else core.red),
        core.ljust_color(core.white("Failed"), 10) + core.bracketed_text(str(summary.failed), core.green if summary.failed == 0 else core.red),
    ]
    print("  ".join(album_cells))

    sample_total = summary.samples_written + summary.samples_unchanged
    sample_cells = [
        core.ljust_color(core.cyan("Samples:"), 10),
        core.ljust_color(core.white("Count"), 11) + core.bracketed_text(str(sample_total), core.green if sample_total else core.white),
        core.ljust_color(core.white("Installed"), 11) + core.bracketed_text(str(summary.installed), core.green if summary.installed else core.white),
        core.ljust_color(core.white("Unchanged"), 11) + core.bracketed_text(str(summary.unchanged), core.cyan if summary.unchanged else core.white),
    ]
    print("  ".join(sample_cells))

    queried_list = [source for source in sources if source in api_queried]
    skipped_list = [source for source in sources if source not in api_queried]
    print(
        core.ljust_color(core.cyan("API:"), 10)
        + core.white("Queried") + " " + core.bracketed_list(queried_list, core.green)
        + " "
        + core.white("Skipped") + " " + core.bracketed_list(skipped_list, core.gray)
    )
    core.emit_ui(
        "summary",
        **vars(summary),
        api_queried=queried_list,
        api_skipped=skipped_list,
        mode=mode,
    )
    return 0 if summary.failed == 0 else 1


def validate_aisplined_placeholder(cfg: dict[str, Any]) -> None:
    core.aisplined_settings(cfg)


def print_help(path: Path, cfg: dict[str, Any]) -> None:
    core.print_help(path, cfg)
    ai = core.aisplined_settings(cfg)
    print()
    print("A:I:S:P:L:I:N:E:D companion boundary (placeholder only):")
    core.help_row("      enabled", core.green(f"[{str(bool(ai.get('enabled', False))).lower()}]"))
    core.help_row("      endpoint", core.green(f"[{str(ai.get('endpoint', ''))}]"))
    core.help_row("      minimum_short_side", core.green(f"[{int(ai.get('minimum_short_side', 600))}]"))
    core.help_row("      below-floor override", core.green(f"[{str(bool(ai.get('allow_below_minimum_override', False))).lower()}]"))
    core.help_row("", "No AI image processing is enabled in this release")
    core.help_row("      -bp", "Override saved [b] album bypass history for this run")
    core.help_row("      [k]", "Keep the existing local cover during Local & Suggested artwork comparison")
    core.help_row("      WEBP still", "Preserve cover.webp; generate a safe still, then compare the still against provider artwork")
    core.help_row("      [b]", "Persistently bypass the unresolved album and continue the scan")
    core.help_row("      [m] picker", "Shown only when the album entered fallback because a MusicBrainz lookup actually failed")
    core.help_row("      Manual picks", "Explicit s/number selections may be outside the normal range; aspect-ratio/resize/output safety still applies")


def print_config(path: Path, cfg: dict[str, Any]) -> None:
    core.print_config(path, cfg)
    ai = core.aisplined_settings(cfg)
    print(f"AISPLINE enabled: {bool(ai.get('enabled', False))}")
    print(f"AISPLINE endpoint: {str(ai.get('endpoint', ''))}")
    print(f"AISPLINE minimum short side: {int(ai.get('minimum_short_side', 600))}")
    print(f"AISPLINE below-minimum override: {bool(ai.get('allow_below_minimum_override', False))}")


def main() -> int:
    global _BYPASS_OVERRIDE
    _BYPASS_OVERRIDE = "-bp" in sys.argv[1:]
    if _BYPASS_OVERRIDE:
        sys.argv = [sys.argv[0]] + [arg for arg in sys.argv[1:] if arg != "-bp"]
    args = core.parser().parse_args()
    interface_only = True
    skip_theme_value = False
    for argument in sys.argv[1:]:
        if skip_theme_value:
            skip_theme_value = False
            continue
        if argument == "--tui-theme":
            skip_theme_value = True
            continue
        if argument.startswith("--tui-theme=") or argument in {"--tui", "--no-tui"}:
            continue
        interface_only = False
        break
    if args.version:
        print(f"{APP_NAME} {VERSION}")
        return 0
    if args.idle:
        return core.idle()

    try:
        path, cfg = core.load_config()
        validate_aisplined_placeholder(cfg)
        if args.oauth_validation:
            if len(sys.argv) != 2:
                raise core.SplinedError(
                    "--oauth-validation does not accept additional command options."
                )
            return core.run_oauth_validation_command(path, cfg)

        scan_cfg = core.section(cfg, "scan")
        runtime_cache = core.runtime_cache_dir(path, cfg)
        core.ensure_runtime_directories(path, cfg, runtime_cache)
        core.init_debug_log(path, cfg)

        if args.preserve_file is not None:
            core.section(cfg, "output")["preserve_file"] = args.preserve_file == "true"
        if args.help:
            print_help(path, cfg)
            return 0
        if args.config:
            print_config(path, cfg)
            return 0
        if args.config_check:
            print(f"OK: {path}\nconfig_version = {core.CONFIG_VERSION}")
            return 0
        if args.config_edit:
            return core.run_config_edit(path)
        if args.fanarttv_credentials:
            return core.configure_fanarttv_credentials(path, cfg)
        if args.lastfm_credentials:
            return core.configure_lastfm_credentials(path, cfg)
        if args.lastfm_login:
            return core.run_lastfm_login(path, cfg)
        if args.mb_oauth_login:
            return core.run_musicbrainz_login(path, cfg)

        sources = core.resolve_sources(
            cfg,
            core.parse_sources(args.cover_sources),
            core.parse_sources(args.only_cover_sources),
            core.parse_sources(args.exclude_cover_sources) or [],
        )
        if not sources:
            raise core.SplinedError("No SPLINED cover sources remain after exclusions.")

        if args.scan:
            return core.run_scan_preview(path, cfg, sources)

        # Bare `splined` is intentionally the operational command.  It scans
        # the current working directory recursively, making it suitable for
        # shell/batch automation without another required subcommand.
        if len(sys.argv) == 1 or interface_only:
            return core.run_operational_interface(
                args,
                lambda: run_scan_dir(path, cfg, sources, [str(Path.cwd())]),
            )

        if args.scan_dir is not None:
            return core.run_operational_interface(
                args,
                lambda: run_scan_dir(path, cfg, sources, args.scan_dir),
            )
        if args.release_mbid:
            return core.run_release_discovery(path, cfg, sources, args.release_mbid)

        if args.tui:
            raise core.SplinedError(
                "--tui is available only for an operational scan."
            )
        print_help(path, cfg)
        return 0

    except core.SplinedError as exc:
        print(str(exc), file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("\nSPLINED interrupted.", file=sys.stderr)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())

"""SPLINED operational scan entrypoint.

The core provider/authority engine remains in splined.py.  This module owns the
operational album scan surface so local artwork can be evaluated before remote
artwork discovery without coupling the provider engine to filesystem policy.
"""

from __future__ import annotations

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
import splined_strict_source_policy as strict_policy
from interaction.aispline import validated_enhanced_results


APP_NAME = core.APP_NAME
VERSION = core.VERSION


class QueuedArtworkVerificationError(ValueError):
    """A queued write could not be proven by reopening its target audio file."""


def _iter_lazy_album_records(albums, prepare_album):
    """Yield prepared Albums one at a time without batch-wide prefetch."""
    for index, album in enumerate(albums, 1):
        yield index, prepare_album(album, index)


def provider_label(source: str) -> str:
    mapping = {
        "local": "Local",
        "local-library": "Local Library",
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
    if candidate.source in {"local", "local-library"} and candidate.path.suffix.lower() == ".webp":
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
        -projected["short_side"],
        candidate.source_priority,
        format_order.index(projected["format"])
        if projected["format"] in format_order
        else 999999,
        provider_label(candidate.source).lower(),
        str(candidate.ref.id),
    )


def select_best(candidates: list[core.Candidate], cfg: dict[str, Any], format_order: list[str]) -> core.Candidate | None:
    acceptable = [
        candidate
        for candidate in candidates
        if strict_policy.preferred_eligible(candidate)
        and project_candidate(candidate, cfg, format_order)["acceptable"]
    ]
    return min(acceptable, key=lambda candidate: candidate_key(candidate, cfg, format_order)) if acceptable else None


def select_auto_ideal(
    candidates: list[core.Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
) -> core.Candidate | None:
    """Choose only a policy-acceptable Ideal candidate for unattended use."""
    ideal: list[core.Candidate] = []
    for candidate in candidates:
        projected = project_candidate(candidate, cfg, format_order)
        if (
            strict_policy.auto_eligible(candidate)
            and projected["acceptable"]
            and projected["range_type"] == "Ideal"
            and not bool(projected.get("upscaled", False))
            and not bool(projected.get("cropped", False))
        ):
            ideal.append(candidate)
    return (
        min(ideal, key=lambda candidate: candidate_key(candidate, cfg, format_order))
        if ideal
        else None
    )


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
    candidates = [
        candidate
        for candidate in candidates
        if strict_policy.preferred_eligible(candidate)
    ]
    if not candidates:
        return None

    def key(candidate: core.Candidate):
        projected = project_candidate(candidate, cfg, format_order)
        return (
            projected["distance"],
            0 if projected["square"] else 1,
            0 if candidate.ref.approved else 1,
            -projected["short_side"],
            candidate.source_priority,
            format_order.index(projected["format"])
            if projected["format"] in format_order
            else 999999,
            str(candidate.ref.id),
        )

    return min(candidates, key=key)


def _candidate_resolution_label(
    candidates: list[core.Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
) -> str:
    candidate = select_best(candidates, cfg, format_order) or fallback_suggested(
        candidates, cfg, format_order
    )
    return f"{candidate.width}x{candidate.height}" if candidate is not None else ""


def candidate_browser_url(candidate: core.Candidate) -> str:
    """Return the successful remote resource without changing fetch identity."""
    return str(candidate.ref.browser_url or candidate.ref.url).strip()


def candidate_link(candidate: core.Candidate) -> str:
    if candidate.source in {"local", "local-library", "webpstill", "embedded"}:
        return core.bracketed_text("LOCAL", core.green)
    if candidate.source == "enhanced":
        return core.bracketed_text("Enhanced", core.magenta)
    return core.format_source_url(candidate_browser_url(candidate))


def candidate_provenance(candidate: core.Candidate) -> str:
    if candidate.source in {"local", "local-library", "webpstill", "embedded"}:
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
            if item.source in {"local", "local-library", "webpstill", "embedded"}
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
        strict_eligible = strict_policy.preferred_eligible(candidate)
        strict_status = str(getattr(candidate, "strict_status", "not-applicable"))
        strict_reason = str(getattr(candidate, "strict_reason", ""))
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
        if strict_status != "not-applicable":
            strict_text = f"Strict: {strict_status}"
            if strict_reason:
                strict_text += f" · {strict_reason}"
            comparison = f"{comparison} · {strict_text}" if comparison else strict_text
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
                "acceptable": projected["acceptable"] and strict_eligible,
                "approved": candidate.ref.approved,
                "id": str(candidate.ref.id),
                "url": candidate_browser_url(candidate),
                "path": str(candidate.path),
                "provenance": candidate_provenance(candidate),
                "comparison": comparison,
                "crop_risk": crop_risk,
                "strict_status": strict_status,
                "strict_reason": strict_reason,
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
            core.ljust_color(
                core.bool_color(projected["acceptable"] and strict_eligible),
                12,
            ),
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
    preserve = bool(output.get("preserve_file", True))
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
        # preserve_file=false is an explicit request to allow replacement.
        # Keep the current cover in the comparison, but never let an Ideal
        # local file short-circuit provider discovery in overwrite mode.
        is_ideal = projected["range_type"] == "Ideal" and preserve
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
            is_ideal = projected["range_type"] == "Ideal" and preserve
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
        return (
            sample_dir
            / core.sanitize(sample_release.artist_credit)
            / core.sanitize(sample_release.title)
            / f"{core.sanitize(candidate.source)}.sample.{core.EXTENSIONS[candidate.format]}"
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
            destination_existed = destination.exists()
            unchanged = destination.read_bytes() == content
            if mode == "write" and not unchanged:
                core.atomic_write(destination, content)
            action = "UNCHANGED" if unchanged else ("INSTALLED" if mode == "write" else "READ-ONLY (would normalize)")

        elif action_kind == "webp":
            content, info = core.prepare_final(candidate, cfg, "jpeg", allow_out_of_range=True)
            destination = album.path / f"{file_name}.jpg"
            destination_existed = destination.exists()
            unchanged = destination.exists() and destination.read_bytes() == content
            if mode == "write" and not unchanged:
                core.atomic_write(destination, content)
            action = "UNCHANGED" if unchanged else ("INSTALLED" if mode == "write" else "READ-ONLY (would install)")

        elif action_kind == "embedded-ideal":
            content, info = core.prepare_final(candidate, cfg, "jpeg")
            destination = album.path / f"{file_name}.jpg"
            destination_existed = destination.exists()
            unchanged = destination.exists() and destination.read_bytes() == content
            if mode == "write" and not unchanged:
                core.atomic_write(destination, content)
            action = "UNCHANGED" if unchanged else ("INSTALLED" if mode == "write" else "READ-ONLY (would install)")

        else:
            raise core.SplinedError(f"Unsupported local preflight action: {action_kind}")

    except Exception as exc:
        summary.failed += 1
        core.emit_ui(
            "album_material_result",
            outcome="Failed",
            file_action="Failed",
            detail=str(exc),
        )
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

    projected = project_candidate(candidate, cfg, format_order)
    file_action = (
        "Unchanged"
        if action == "UNCHANGED"
        else "Replaced"
        if action == "INSTALLED" and destination_existed
        else "Written"
        if action == "INSTALLED"
        else "Preserved"
    )
    core.emit_ui(
        "album_material_result",
        outcome=action.title(),
        destination=str(destination),
        file_action=file_action,
        source=provider_label(candidate.source),
        width=candidate.width,
        height=candidate.height,
        format=candidate.format,
        range_type=str(projected["range_type"]),
        distance=int(projected["distance"]),
    )

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
) -> tuple[str, Path, dict[str, Any], bool]:
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
        existed = canonical.exists()
        if mode == "read":
            unchanged = existed and canonical.read_bytes() == content
            return ("UNCHANGED" if unchanged else "READ-ONLY (would install)"), canonical, info, existed
        core.atomic_write(canonical, content)
        core.remove_numbered_cover_variants(album.path, name)
        return "INSTALLED", canonical, info, existed

    destination, unchanged = core.choose_destination(canonical, content, True)
    existed = destination.exists()
    if unchanged:
        return "UNCHANGED", destination, info, existed
    if mode == "read":
        return "READ-ONLY (would install)", destination, info, existed
    core.atomic_write(destination, content)
    return "INSTALLED", destination, info, existed


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
        core.emit_ui("album_material_result", outcome="Failed", file_action="Failed", detail=str(exc))
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
        core.emit_ui("album_material_result", outcome="Failed", file_action="Failed", detail=str(exc))
        print(f"  {core.red('ERROR: selected sample failed: ' + str(exc))}")
        return False

    try:
        action, destination, info, destination_existed = finalize(
            album,
            candidate,
            cfg,
            format_order,
            allow_out_of_range=allow_out_of_range,
        )
    except Exception as exc:
        summary.failed += 1
        core.emit_ui("album_material_result", outcome="Failed", file_action="Failed", detail=str(exc))
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

    projected = project_candidate(candidate, cfg, format_order)
    file_action = (
        "Unchanged"
        if action == "UNCHANGED"
        else "Replaced"
        if action == "INSTALLED" and destination_existed
        else "Written"
        if action == "INSTALLED"
        else "Preserved"
    )
    core.emit_ui(
        "album_material_result",
        outcome=action.title(),
        destination=str(destination),
        file_action=file_action,
        source=provider_label(candidate.source),
        width=candidate.width,
        height=candidate.height,
        format=candidate.format,
        range_type=str(projected["range_type"]),
        distance=int(projected["distance"]),
    )

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


def _manual_unresolved(
    track: Any,
    reason: str,
    unresolved_items: list[dict[str, str]],
) -> None:
    item = {
        "path": str(track.path),
        "artist": str(track.artist or ""),
        "title": str(track.title or track.path.name),
        "reason": str(reason),
    }
    unresolved_items.append(item)
    core.debug_log(
        "compilation.manual.unresolved "
        f"file={str(track.path)!r} reason={str(reason)!r}"
    )
    print(
        f"    {core.red('Unresolved:')} "
        f"{core.white(item['artist'])} · {core.orange(item['title'])} · "
        f"{core.yellow(item['reason'])}"
    )


def _manual_local_candidates(
    rows: list[dict[str, Any]],
) -> list[core.Candidate]:
    candidates: list[core.Candidate] = []
    seen: set[str] = set()
    for row in rows:
        path = Path(str(row.get("cover_path") or ""))
        identity = str(path).casefold()
        if not identity or identity in seen:
            continue
        seen.add(identity)
        try:
            candidate = inspect_candidate_file(path, "local-library", -3)
        except Exception:
            continue
        candidate.ref.id = (
            f"{str(row.get('album_artist') or '')} · "
            f"{str(row.get('album_name') or path.parent.name)}"
        )
        candidates.append(candidate)
    return candidates


def _manual_emit_existing_artwork(
    rows: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Expose authorized local Album matches beside native-style MB results."""
    items: list[dict[str, Any]] = []
    eligible_rows: list[dict[str, Any]] = []
    seen: set[str] = set()
    for row in rows:
        cover = Path(str(row.get("cover_path") or ""))
        identity = str(cover).casefold()
        if not identity or identity in seen or not cover.is_file():
            continue
        seen.add(identity)
        eligible_rows.append(row)
        items.append(
            {
                "path": str(cover),
                "title": str(row.get("album_name") or cover.parent.name),
                "artist": str(row.get("album_artist") or ""),
                "release_id": str(row.get("musicbrainz_albumid") or ""),
                "release_group_id": str(
                    row.get("musicbrainz_releasegroupid") or ""
                ),
                "kind": (
                    "recording-match"
                    if row.get("recording_match")
                    else "artist-title-match"
                ),
            }
        )
    core.emit_ui("musicbrainz_existing", items=items)
    return eligible_rows


def _manual_remote_candidates(
    http: Any,
    config_file: Path,
    cfg: dict[str, Any],
    release: core.Release,
    sources: list[str],
    cache: Path,
    api_queried: set[str],
    state: dict[str, Any],
    *,
    force_refresh: bool = False,
) -> tuple[list[core.Candidate], list[tuple[str, str]]]:
    key = str(release.mbid).casefold()
    release_cache = state.setdefault("release_candidates", {})
    diagnostic_cache = state.setdefault("release_diagnostics", {})
    cached = release_cache.get(key)
    if cached is not None and not force_refresh:
        core.debug_log(
            "compilation.release_results.cache_hit "
            f"mbid={key!r} candidates={len(cached)}"
        )
        return list(cached), list(diagnostic_cache.get(key, []))
    refs, diagnostics = core.discover_all(
        http,
        config_file,
        cfg,
        release,
        sources,
        queried_sources=api_queried,
    )
    downloaded, download_diagnostics = core.download_candidates(
        http,
        refs,
        sources,
        cache,
        cfg,
        # Keep inspected releases available throughout the active compilation
        # so green/blue MusicBrainz rows never repeat provider work.
        clean_first=not bool(release_cache),
    )
    diagnostics.extend(download_diagnostics)
    release_cache[key] = list(downloaded)
    diagnostic_cache[key] = list(diagnostics)
    state.setdefault("release_resolutions", {})[key] = _candidate_resolution_label(
        downloaded,
        cfg,
        core.formats(cfg),
    )
    return downloaded, diagnostics


def _manual_discovery_options(
    items: list[Any],
    *,
    current_release_id: str = "",
    inspected_resolutions: dict[str, str] | None = None,
) -> list[dict[str, str]]:
    options: list[dict[str, str]] = []
    for item in items:
        family_title = str(
            getattr(item, "release_group_title", "")
            or item.release_title
        )
        family_id = str(item.release_group_mbid or item.release_mbid)
        options.append(
            {
                "id": str(item.release_mbid),
                "musicbrainz_url": (
                    "https://musicbrainz.org/release/" + str(item.release_mbid)
                ),
                "recording_id": str(item.recording_mbid),
                "artist": str(item.recording_artist),
                "recording_artist": str(item.recording_artist),
                "release_artist": str(item.release_artist),
                "track": str(item.recording_title),
                "title": str(item.release_title),
                "date": str(item.release_date),
                "country": str(item.country),
                "release_class": str(item.release_class).title(),
                "release_group_id": str(item.release_group_mbid),
                "release_group_title": family_title,
                "family_id": family_id,
                "family_title": family_title,
                "group": f"Release Family · {family_title}",
                "score": str(item.score),
                "match_basis": str(
                    getattr(item, "match_basis", "exact-recording")
                ),
                "match_label": {
                    "exact-recording": "Tagged Recording ID",
                    "related-work": "Related Work · validated Artist",
                    "same-artist-title": "Same-Artist title suggestion",
                }.get(
                    str(getattr(item, "match_basis", "")),
                    "MusicBrainz match",
                ),
            }
        )
    return core.decorate_musicbrainz_results(
        options,
        current_release_id=current_release_id,
        inspected_resolutions=inspected_resolutions,
    )


def _manual_choose_release(
    items: list[Any],
    *,
    current_release_id: str = "",
    inspected_resolutions: dict[str, str] | None = None,
    authority: dict[str, str] | None = None,
    friendly: dict[str, str] | None = None,
    identity: Any | None = None,
    current_album: dict[str, str] | None = None,
    existing_rows: list[dict[str, Any]] | None = None,
    can_back: bool = False,
) -> tuple[str, Any | None, dict[str, Any]]:
    options = _manual_discovery_options(
        items,
        current_release_id=current_release_id,
        inspected_resolutions=inspected_resolutions,
    )
    print()
    print(f"  {core.cyan('MusicBrainz matches')}")
    for number, option in enumerate(options, 1):
        print(
            f"    {core.magenta(str(number).rjust(2))}  "
            f"{core.white(option['artist'])} · "
            f"{core.green(option['score'] + '%')} · "
            f"{core.orange(option['track'])} · "
            f"{core.cyan(option['release_class'])} · "
            f"{core.white(option['date'] or 'Unknown year')} · "
            f"{core.orange(option['title'])}"
        )
    while True:
        raw_answer = core.read_input(
            "    MusicBrainz release #: ",
            kind="musicbrainz-results",
            options=options,
            authority={
                "artist": str((friendly or {}).get("artist", "")),
                "album": str((friendly or {}).get("album", "")),
                "artist_mbids": str((authority or {}).get("artist_id", "")),
                "release_mbid": str((authority or {}).get("release_id", "")),
                "recording_mbid": str((authority or {}).get("recording_id", "")),
            },
            identity={
                "status": str(getattr(identity, "identity_status", "pending")),
                "message": str(getattr(identity, "identity_message", "")),
                "recording_status": str(
                    getattr(identity, "recording_status", "pending")
                ),
                "artist_status": str(
                    getattr(identity, "artist_status", "pending")
                ),
                "friendly_artist": str(
                    getattr(identity, "friendly_artist", "")
                ),
                "friendly_title": str(
                    getattr(identity, "friendly_title", "")
                ),
                "focused_artist_mbid": str(
                    getattr(identity, "focused_artist_mbid", "")
                ),
                "used_fuzzy_search": bool(
                    getattr(identity, "used_fuzzy_search", False)
                ),
            },
            current_album=dict(current_album or {}),
            can_back=bool(can_back),
        ).strip()
        answer = raw_answer.lower()
        if answer in {"__cancel__", "__manual_album_exit__"}:
            return "exit-album", None, {}
        if answer == "__manual_candidates__":
            return "back-candidates", None, {}
        if answer == "b":
            return "unchanged", None, {}
        try:
            payload = core.json.loads(raw_answer)
        except (TypeError, ValueError):
            payload = None
        if (
            isinstance(payload, dict)
            and payload.get("action") == "manual-authority-query"
        ):
            fields = {
                key: str(payload.get(key) or "").strip().casefold()
                for key in ("artist_id", "recording_id", "edited")
            }
            return "requery", None, fields
        if (
            isinstance(payload, dict)
            and payload.get("action") == "use-existing"
        ):
            try:
                existing_number = int(payload.get("number", 0))
            except (TypeError, ValueError):
                existing_number = 0
            rows = list(existing_rows or [])
            if 1 <= existing_number <= len(rows):
                return "existing", rows[existing_number - 1], {}
        if answer.isdigit() and 1 <= int(answer) <= len(items):
            selected_id = options[int(answer) - 1].get("id", "")
            selected = next(
                (
                    item
                    for item in items
                    if str(item.release_mbid).casefold() == selected_id.casefold()
                ),
                None,
            )
            if selected is not None:
                return "selected", selected, {}
        print("    Choose a listed MusicBrainz release number, Apply IDs, Back, or Esc.")


def _manual_choose_candidate(
    candidates: list[core.Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
    *,
    has_musicbrainz_results: bool = False,
    authority: dict[str, str] | None = None,
    friendly: dict[str, str] | None = None,
) -> tuple[str, core.Candidate | None, dict[str, Any]]:
    top = fallback_top_candidates(candidates, cfg, format_order, 10)
    suggested = fallback_suggested(top, cfg, format_order)
    render_candidate_table(
        top,
        cfg,
        format_order,
        suggested=suggested,
        manual_fallback=True,
    )
    while True:
        menu = (
            f"    {core.paint('PURPLE', '[s]')} suggested   "
            f"{core.cyan('[#]')} choose number   "
            f"{core.cyan('[m]')} MusicBrainz Artist/Track search   "
            f"{core.cyan('[b]')} leave unchanged"
        )
        print(menu)
        raw_answer = core.read_input(
            "    Choice: ",
            kind="fallback-picker",
            manual_fallback=True,
            musicbrainz_back=has_musicbrainz_results,
            authority={
                "artist": str((friendly or {}).get("artist", "")),
                "album": str((friendly or {}).get("album", "")),
                "track": str((friendly or {}).get("track", "")),
                "artist_mbids": str((authority or {}).get("artist_id", "")),
                "release_mbid": str((authority or {}).get("release_id", "")),
                "recording_mbid": str((authority or {}).get("recording_id", "")),
            },
        ).strip()
        answer = raw_answer.lower()
        if answer == "__manual_mb_results__" and has_musicbrainz_results:
            return "mb-results", None, {}
        if answer == "__batch_return__":
            raise core.InteractionReturn()
        if answer in {"__cancel__", "__manual_album_exit__"}:
            return "exit-album", None, {}
        try:
            payload = core.json.loads(raw_answer)
        except (TypeError, ValueError):
            payload = None
        if (
            isinstance(payload, dict)
            and payload.get("action") == "webgui-embed-track"
        ):
            try:
                number = int(payload.get("number", 0))
            except (TypeError, ValueError):
                number = 0
            artwork = payload.get("artwork")
            if 1 <= number <= len(top) and isinstance(artwork, dict):
                return "selected", top[number - 1], {
                    "webgui_artwork": {
                        "image": str(artwork.get("image") or ""),
                        "adjustments": artwork.get("adjustments") or {},
                        "upscale": bool(artwork.get("upscale")),
                        "prepared_id": str(artwork.get("prepared_id") or ""),
                        "prepared_sha256": str(artwork.get("prepared_sha256") or ""),
                    }
                }
        if (
            isinstance(payload, dict)
            and payload.get("action") == "manual-authority-query"
        ):
            fields = {
                key: str(payload.get(key) or "").strip().casefold()
                for key in ("artist_id", "recording_id", "edited")
            }
            return "requery", None, fields
        if answer == "m":
            return (
                "mb-results" if has_musicbrainz_results else "discover",
                None,
                {},
            )
        if answer == "b":
            return "unchanged", None, {}
        if answer == "s" and suggested is not None:
            return "selected", suggested, {}
        if answer.isdigit():
            number = int(answer)
            if 1 <= number <= len(top):
                return "selected", top[number - 1], {}
        print("    Choose s, a listed number, m, or b.")


def _manual_track_authority(
    track: Any,
    *,
    artist_id: str,
    recording_id: str,
) -> core.Track:
    """Build a session-only authority view; never alter the file's tags."""
    return core.Track(
        track.path,
        str(track.title or ""),
        str(track.artist or ""),
        getattr(track, "album", None),
        getattr(track, "album_artist", None),
        getattr(track, "album_mbid", None),
        recording_id,
        getattr(track, "compilation", None),
        artist_id,
    )


def _manual_musicbrainz_options(
    config_file: Path,
    cfg: dict[str, Any],
    state: dict[str, Any],
) -> Any | None:
    options = state.get("mb_options")
    if options is None and "mb_options_error" not in state:
        try:
            options = core.compilation_credential_options(config_file, cfg)
            state["mb_options"] = options
            core.debug_log(
                "compilation.manual.mb_options "
                f"retry_max={options.retry_max} "
                f"min_delay={options.min_delay:g} "
                f"recording_timeout={options.recording_timeout:g}"
            )
        except Exception as exc:
            state["mb_options_error"] = str(exc)
    return options


def _manual_progress(
    album: core.AlbumDir,
    tracks: list[Any],
    completed_paths: set[str],
    *,
    config_file: Path,
    cfg: dict[str, Any],
) -> None:
    completed = len(completed_paths)
    total = len(tracks)
    status = "complete" if total > 0 and completed >= total else "incomplete"
    try:
        core.compilation_record_progress(
            config_file,
            cfg,
            str(album.path),
            total,
            completed,
        )
    except Exception as exc:
        core.debug_log(
            "compilation.manual.progress_write_failed "
            f"album={str(album.path)!r} error={type(exc).__name__!r}"
        )
    core.emit_ui(
        "album_progress",
        path=str(album.path),
        completed=completed,
        total=total,
        status=status,
    )


def run_manual_compilation_album(
    album: core.AlbumDir,
    tracks: list[Any],
    *,
    http: Any,
    config_file: Path,
    cfg: dict[str, Any],
    sources: list[str],
    cache: Path,
    format_order: list[str],
    summary: core.Summary,
    api_queried: set[str],
    unresolved_items: list[dict[str, str]],
    state: dict[str, Any],
    target_track_paths: set[str] | None = None,
    track_context: Any | None = None,
    prepare_selected_artwork: Any | None = None,
    persist_track_authority: Any | None = None,
    verify_embedded_artwork: Any | None = None,
    musicbrainz_results_first: bool = False,
) -> bool:
    """Resolve and approve embedded artwork independently for every track."""
    if not core.compilation_manual_album_eligible(tracks):
        summary.unresolved += 1
        reason = (
            "Fallback embedded artwork requires a representative track "
            "with no Album MBID"
        )
        for track in tracks[:1]:
            _manual_unresolved(track, reason, unresolved_items)
        return True

    print()
    print(
        f"  {core.cyan('Track Artwork:'):13} "
        f"{core.orange('fallback embedded artwork')}"
    )
    print(
        f"  {core.cyan('Folder Art:'):13} "
        f"{core.green('untouched')}"
    )
    mode = str(cfg.get("mode", "read")).strip().casefold()
    resolved_tracks = 0
    completed_paths = (
        core.compilation_completed_track_paths(
            config_file,
            cfg,
            str(album.path),
            tracks,
        )
        if mode == "write"
        else set()
    )
    if completed_paths:
        print(
            f"  {core.cyan('Resume:'):13} "
            f"{core.green(str(len(completed_paths)))} / "
            f"{core.white(str(len(tracks)))} tracks already complete"
        )
        core.debug_log(
            "compilation.manual.resume "
            f"album={str(album.path)!r} "
            f"completed={len(completed_paths)} total={len(tracks)}"
        )
    if mode == "write":
        _manual_progress(
            album,
            tracks,
            completed_paths,
            config_file=config_file,
            cfg=cfg,
        )

    explicit_targets = {str(value) for value in (target_track_paths or set())}
    for track_number, track in enumerate(tracks, 1):
        if core.ui_cancelled(): raise core.InteractionCancelled()
        print()
        print(
            f"  {core.cyan(f'Track {track_number}:'):13} "
            f"{core.white(str(track.artist or ''))} · "
            f"{core.orange(str(track.title or track.path.name))}"
        )
        if explicit_targets and str(track.path) not in explicit_targets:
            print(
                f"    {core.cyan('Target:'):11} "
                f"{core.gray('not explicitly selected')} · skipped"
            )
            continue
        if track_context is not None:
            track_context(track)
        if str(track.path) in completed_paths and str(track.path) not in explicit_targets:
            print(
                f"    {core.cyan('Progress:'):11} "
                f"{core.green('already complete')} · skipped"
            )
            continue
        eligible, reason = core.compilation_manual_track_eligible(track)
        if not eligible:
            summary.unresolved += 1
            _manual_unresolved(track, reason, unresolved_items)
            continue
        artist_ids = sorted(
            core.compilation_mbids(getattr(track, "artist_mbid", None))
        )
        recording_ids = sorted(
            core.compilation_mbids(getattr(track, "recording_mbid", None))
        )
        authority = {
            "artist_id": artist_ids[0] if artist_ids else "",
            "release_id": "",
            "recording_id": recording_ids[0] if recording_ids else "",
        }
        friendly = {
            "artist": str(track.artist or ""),
            "album": "",
            "track": str(track.title or track.path.name),
        }
        force_musicbrainz = False
        force_sources = False
        interactive_retry = False
        # Windows always presents the current track's ranked MusicBrainz
        # releases before querying artwork providers.  Browser COMP review opts
        # into that sequence so one Recording cannot fan out into provider work
        # (and diagnostics) for every containing release before the operator
        # chooses the intended release.
        show_discovery = (
            musicbrainz_results_first
            or not authority["artist_id"]
            or not authority["recording_id"]
        )
        discovery_results: list[Any] = []
        discovery_identity: Any | None = None
        discovery_existing_rows: list[dict[str, Any]] = []
        selected_release_item: Any | None = None
        selected_local_row: dict[str, Any] | None = None
        selected: core.Candidate | None = None
        selected_artwork_request: dict[str, Any] | None = None
        release: core.Release | None = None
        authority_edit_pending = False
        # Completion authority belongs to the Recording/Artist validation,
        # not to a subsequently selected release. A same-Artist title
        # suggestion may supply artwork without validating the track's tagged
        # Recording ID.
        authority_completion_validated = False
        leave_track_unchanged = False

        while True:
            if show_discovery:
                options = _manual_musicbrainz_options(config_file, cfg, state)
                search_error = ""
                cache_key = (
                    str(track.artist or "").strip().casefold(),
                    str(track.title or "").strip().casefold(),
                    authority["artist_id"],
                    authority["recording_id"],
                )
                discovery_cache = state.setdefault("mb_discovery_results", {})
                identity_cache = state.setdefault("mb_discovery_identity", {})
                used_discovery_cache = False
                if not discovery_results and cache_key in discovery_cache:
                    discovery_results = list(discovery_cache[cache_key])
                    discovery_identity = identity_cache.get(cache_key)
                    used_discovery_cache = True
                authority_track = _manual_track_authority(
                    track,
                    artist_id=authority["artist_id"],
                    recording_id=authority["recording_id"],
                )
                discovery_existing_rows = core.compilation_local_artwork_rows(
                    config_file,
                    cfg,
                    authority_track,
                    current_album_path=album.path,
                    release_mbid="",
                )
                discovery_existing_rows = _manual_emit_existing_artwork(
                    discovery_existing_rows
                )
                if not discovery_results and options is not None:
                    if musicbrainz_results_first:
                        discovery_identity = (
                            core.compilation_resolve_interactive_identity(
                                http,
                                config_file,
                                cfg,
                                authority_track,
                                options,
                            )
                        )
                        discovered = discovery_identity.candidates
                        search_error = discovery_identity.error
                    else:
                        discovered, search_error = (
                            core.compilation_discover_releases(
                                http,
                                config_file,
                                cfg,
                                authority_track,
                                options,
                            )
                        )
                    discovery_results = list(discovered)
                    if discovery_results:
                        discovery_cache[cache_key] = list(discovery_results)
                    if discovery_identity is not None:
                        identity_cache[cache_key] = discovery_identity
                    core.debug_log(
                        "compilation.manual.discovery "
                        f"file={str(track.path)!r} results={len(discovery_results)} "
                        f"cached={used_discovery_cache}"
                    )
                elif options is None:
                    search_error = str(
                        state.get("mb_options_error")
                        or "Invalid MusicBrainz options"
                    )
                if (
                    getattr(discovery_identity, "identity_status", "") == "valid"
                    and getattr(discovery_identity, "recording_status", "") == "valid"
                    and getattr(discovery_identity, "artist_status", "") == "valid"
                ):
                    authority_completion_validated = True
                core.emit_ui("candidates", items=[])
                core.emit_ui(
                    "fallback_authority",
                    artist=friendly["artist"],
                    artist_id=authority["artist_id"],
                    album=friendly["album"],
                    album_id=authority["release_id"],
                    track=friendly["track"],
                    track_id=authority["recording_id"],
                )
                core.emit_ui(
                    "diagnostics",
                    items=[
                        ("musicbrainz", message)
                        for message in dict.fromkeys(
                            [
                                *(getattr(discovery_identity, "warnings", ()) or ()),
                                *([search_error] if search_error else []),
                            ]
                        )
                    ],
                )
                if discovery_results or musicbrainz_results_first:
                    if (
                        authority_edit_pending
                        and getattr(discovery_identity, "identity_status", "")
                        == "valid"
                        and persist_track_authority is not None
                    ):
                        persist_track_authority(
                            track,
                            authority["recording_id"],
                            authority["artist_id"],
                        )
                        track.recording_mbid = authority["recording_id"]
                        track.artist_mbid = authority["artist_id"]
                        authority_edit_pending = False
                        authority_completion_validated = True
                    decision, discovered_item, changes = _manual_choose_release(
                        discovery_results,
                        current_release_id=authority["release_id"],
                        inspected_resolutions=state.setdefault(
                            "release_resolutions", {}
                        ),
                        authority=authority,
                        friendly=friendly,
                        identity=discovery_identity,
                        current_album={
                            "artist": str(
                                getattr(track, "album_artist", None)
                                or "Various Artists"
                            ),
                            "title": str(
                                getattr(track, "album", None)
                                or album.path.name
                            ),
                            "release_id": "",
                        },
                        existing_rows=discovery_existing_rows,
                        can_back=bool(
                            selected_release_item is not None
                            or selected_local_row is not None
                            or authority["release_id"]
                        ),
                    )
                    if decision == "exit-album":
                        if mode == "write":
                            _manual_progress(
                                album,
                                tracks,
                                completed_paths,
                                config_file=config_file,
                                cfg=cfg,
                            )
                        return False
                    if decision == "back-candidates":
                        if not (
                            selected_release_item is not None
                            or selected_local_row is not None
                            or authority["release_id"]
                        ):
                            core.emit_ui(
                                "diagnostics",
                                items=[(
                                    "musicbrainz",
                                    "Select a release before opening artwork candidates.",
                                )],
                            )
                            continue
                        show_discovery = False
                        interactive_retry = True
                        continue
                    if decision == "unchanged":
                        summary.unresolved += 1
                        _manual_unresolved(
                            track,
                            "Operator left existing artwork unchanged",
                            unresolved_items,
                        )
                        leave_track_unchanged = True
                        break
                    if decision == "requery":
                        new_values = {
                            key: changes.get(key, authority[key])
                            for key in ("artist_id", "release_id", "recording_id")
                        }
                        if any(
                            len(core.compilation_mbids(value)) != 1
                            for value in (
                                new_values["artist_id"],
                                new_values["recording_id"],
                            )
                        ):
                            # Keep rejected text in the Advanced IDs fields so
                            # the operator can correct it.  It is session-only:
                            # no tag or PostgreSQL authority write occurs until
                            # the real resolver validates Artist + Recording.
                            authority.update(new_values)
                            authority_completion_validated = False
                            discovery_results = []
                            discovery_identity = None
                            core.emit_ui(
                                "diagnostics",
                                items=[(
                                    "musicbrainz",
                                    "Recording and Artist IDs must be valid MusicBrainz UUIDs.",
                                )],
                            )
                            continue
                        authority.update(new_values)
                        selected_release_item = None
                        selected_local_row = None
                        discovery_results = []
                        discovery_identity = None
                        state.setdefault("mb_discovery_results", {}).pop(
                            cache_key, None
                        )
                        state.setdefault("mb_discovery_identity", {}).pop(
                            cache_key, None
                        )
                        show_discovery = musicbrainz_results_first
                        force_musicbrainz = not musicbrainz_results_first
                        force_sources = True
                        interactive_retry = True
                        authority_edit_pending = True
                        authority_completion_validated = False
                        continue
                    if decision == "existing":
                        selected_local_row = discovered_item
                        selected_release_item = None
                        authority["release_id"] = str(
                            selected_local_row.get("musicbrainz_albumid") or ""
                        ).casefold()
                        friendly["album"] = str(
                            selected_local_row.get("album_name") or ""
                        )
                        show_discovery = False
                        force_musicbrainz = False
                        force_sources = False
                        interactive_retry = True
                        continue
                    if discovered_item is None:
                        continue
                    selected_local_row = None
                    selected_release_item = (
                        core.compilation_release_candidate_from_discovery(
                            discovered_item
                        )
                    )
                    artist_ids = sorted(
                        core.compilation_mbids(
                            discovered_item.artist_mbids_key
                        )
                    )
                    focused_artist_id = authority["artist_id"]
                    authority.update(
                        artist_id=(
                            focused_artist_id
                            if focused_artist_id in artist_ids
                            else artist_ids[0] if artist_ids else ""
                        ),
                        recording_id=discovered_item.recording_mbid,
                        release_id=discovered_item.release_mbid,
                    )
                    friendly.update(
                        artist=discovered_item.recording_artist,
                        track=discovered_item.recording_title,
                        album=discovered_item.release_title,
                    )
                    show_discovery = False
                    force_musicbrainz = False
                    force_sources = False
                    interactive_retry = True
                    continue
                show_discovery = False
                interactive_retry = True

            if leave_track_unchanged:
                break

            authority_track = _manual_track_authority(
                track,
                artist_id=authority["artist_id"],
                recording_id=authority["recording_id"],
            )
            exact_release = (
                authority["release_id"]
                if force_musicbrainz or selected_release_item is not None
                else ""
            )
            local_rows = (
                [selected_local_row]
                if selected_local_row is not None
                else core.compilation_local_artwork_rows(
                    config_file,
                    cfg,
                    authority_track,
                    current_album_path=album.path,
                    release_mbid=exact_release,
                )
            )
            local_match = local_rows[0] if local_rows else {}
            local_candidates: list[core.Candidate] = []
            remote: list[core.Candidate] = []
            diagnostics: list[tuple[str, str]] = []
            release = None
            release_source = ""
            resolution = None
            lookup_error = ""

            if local_rows and not force_musicbrainz:
                release = core.compilation_release_from_local(local_rows[0])
                release_source = "local-library"
                local_candidates = _manual_local_candidates(local_rows)
            elif selected_release_item is not None and not force_musicbrainz:
                release = core.compilation_release_from_candidate(
                    selected_release_item
                )
                release_source = "musicbrainz-search"
            else:
                options = _manual_musicbrainz_options(
                    config_file, cfg, state
                )
                if options is None:
                    lookup_error = str(
                        state.get("mb_options_error")
                        or "Invalid MusicBrainz options"
                    )
                elif exact_release:
                    resolution = core.compilation_resolve_release_id(
                        http,
                        config_file,
                        cfg,
                        authority_track,
                        exact_release,
                        options,
                    )
                else:
                    resolution = core.compilation_resolve_releases(
                        http,
                        config_file,
                        cfg,
                        authority_track,
                        options,
                        force_refresh=force_musicbrainz,
                    )
                if resolution is not None and not resolution.candidates:
                    lookup_error = (
                        resolution.error or "No artwork release was resolved"
                    )
                if resolution is not None:
                    if (
                        authority_edit_pending
                        and resolution.candidates
                        and persist_track_authority is not None
                    ):
                        persist_track_authority(
                            track,
                            authority["recording_id"],
                            authority["artist_id"],
                        )
                        track.recording_mbid = authority["recording_id"]
                        track.artist_mbid = authority["artist_id"]
                        authority_edit_pending = False
                        authority_completion_validated = True
                        core.emit_ui(
                            "authority_saved",
                            recording_mbid=authority["recording_id"],
                            artist_mbids=authority["artist_id"],
                            message="Validated MusicBrainz Recording and Artist IDs saved to this track.",
                        )
                    for release_item in resolution.candidates:
                        attempted_release = (
                            core.compilation_release_from_candidate(release_item)
                        )
                        attempted, attempted_diagnostics = (
                            _manual_remote_candidates(
                                http,
                                config_file,
                                cfg,
                                attempted_release,
                                sources,
                                cache,
                                api_queried,
                                state,
                                force_refresh=force_sources,
                            )
                        )
                        diagnostics.extend(attempted_diagnostics)
                        if attempted:
                            release = attempted_release
                            remote = attempted
                            release_source = resolution.source
                            break
                    if release is None and resolution.candidates and not lookup_error:
                        lookup_error = (
                            "No acceptable local or remote artwork candidate "
                            "was returned"
                        )
                if exact_release and local_rows and release is not None:
                    local_candidates = _manual_local_candidates(local_rows)

            if release is not None and not remote:
                remote, remote_diagnostics = _manual_remote_candidates(
                    http,
                    config_file,
                    cfg,
                    release,
                    sources,
                    cache,
                    api_queried,
                    state,
                    force_refresh=force_sources,
                )
                diagnostics.extend(remote_diagnostics)

            candidates = local_candidates + remote
            strict_policy.apply(
                candidates,
                cfg,
                core.source_policy,
                debug=core.debug_log,
                emit=core.emit_ui,
            )
            if release is not None:
                authority["release_id"] = str(release.mbid or "").casefold()
                friendly["album"] = str(release.title or "")
                print(
                    f"    {core.cyan('Authority:'):11} "
                    f"{core.green(release_source)} · "
                    f"{core.white(release.artist_credit or '')} · "
                    f"{core.orange(release.title or '')}"
                )
            if resolution is not None:
                friendly["artist"] = (
                    str(getattr(resolution, "recording_artist", "") or "")
                    or friendly["artist"]
                )
                friendly["track"] = (
                    str(getattr(resolution, "recording_title", "") or "")
                    or friendly["track"]
                )
            if local_match:
                friendly["artist"] = (
                    str(local_match.get("track_artist") or "")
                    or friendly["artist"]
                )
                friendly["track"] = (
                    str(local_match.get("track_title") or "")
                    or friendly["track"]
                )
            core.emit_ui(
                "fallback_authority",
                artist=friendly["artist"],
                artist_id=authority["artist_id"],
                album=friendly["album"],
                album_id=authority["release_id"],
                track=friendly["track"],
                track_id=authority["recording_id"],
            )
            if diagnostics:
                for source, message in diagnostics:
                    print(
                        f"    {core.magenta(provider_label(source))}: "
                        f"{core.yellow(message)}"
                    )
            # A release can expose many references rejected for the same policy
            # reason.  Surface each actionable diagnostic once, matching the
            # concise Windows review rather than flooding the candidate dock.
            visible_diagnostics = list(dict.fromkeys(diagnostics))
            if lookup_error:
                visible_diagnostics.insert(
                    0, ("musicbrainz", lookup_error)
                )
            core.emit_ui("diagnostics", items=visible_diagnostics)
            if not candidates and not interactive_retry:
                show_discovery = True
                interactive_retry = True
                continue
            if lookup_error:
                print(
                    f"    {core.red('Authority lookup:')} "
                    f"{core.yellow(lookup_error)}"
                )

            decision, chosen, changes = _manual_choose_candidate(
                candidates,
                cfg,
                format_order,
                has_musicbrainz_results=bool(discovery_results),
                authority=authority,
                friendly=friendly,
            )
            if decision == "exit-album":
                if mode == "write":
                    _manual_progress(
                        album,
                        tracks,
                        completed_paths,
                        config_file=config_file,
                        cfg=cfg,
                    )
                core.debug_log(
                    "compilation.manual.album_exit "
                    f"album={str(album.path)!r} "
                    f"completed={len(completed_paths)} total={len(tracks)}"
                )
                return False
            if decision in {"discover", "mb-results"}:
                show_discovery = True
                if decision == "discover":
                    discovery_results = []
                    state.setdefault("mb_discovery_results", {}).pop(
                        (
                            str(track.artist or "").strip().casefold(),
                            str(track.title or "").strip().casefold(),
                        ),
                        None,
                    )
                core.debug_log(
                    "compilation.manual.discovery_open "
                    f"file={str(track.path)!r} refresh={decision == 'discover'}"
                )
                continue
            if decision == "requery":
                edited = changes.get("edited", "")
                if changes:
                    new_values = {
                        key: changes.get(key, authority[key])
                        for key in ("artist_id", "release_id", "recording_id")
                    }
                    if any(
                        len(core.compilation_mbids(value)) != 1
                        for value in (
                            new_values["artist_id"],
                            new_values["recording_id"],
                        )
                    ):
                        print(
                            f"    {core.yellow('Each edited ID must be a valid MusicBrainz UUID.')}"
                        )
                        continue
                    authority.update(new_values)
                    authority_edit_pending = True
                    selected_release_item = None
                    if edited in {"artist", "recording"}:
                        authority["release_id"] = ""
                        friendly["album"] = ""
                force_musicbrainz = True
                force_sources = True
                interactive_retry = True
                core.debug_log(
                    "compilation.manual.authority_requery "
                    f"file={str(track.path)!r} edited={edited or 'retry'!r}"
                )
                continue
            if decision == "unchanged":
                summary.unresolved += 1
                _manual_unresolved(
                    track,
                    "Operator left existing artwork unchanged",
                    unresolved_items,
                )
                break
            selected = chosen
            selected_artwork_request = changes.get("webgui_artwork")
            break

        if leave_track_unchanged:
            continue

        if selected is None:
            continue
        if core.ui_cancelled(): raise core.InteractionCancelled()
        try:
            if selected_artwork_request and prepare_selected_artwork is not None:
                artwork, info, image_format = prepare_selected_artwork(
                    selected_artwork_request
                )
            else:
                artwork, info, image_format = core.compilation_prepare_embedded(
                    selected, cfg
                )
            if mode == "write":
                core.compilation_replace_embedded(
                    track.path, artwork, image_format, info
                )
                if verify_embedded_artwork is not None:
                    verify_embedded_artwork(track, artwork, info)
                next_completed = set(completed_paths)
                next_completed.add(str(track.path))
                completion_recorded = core.compilation_record_application(
                    config_file,
                    cfg,
                    track,
                    source_kind=selected.source,
                    source_locator=(
                        str(selected.path)
                        if selected.source == "local-library"
                        else str(
                            selected.ref.browser_url
                            or selected.ref.url
                            or selected.ref.id
                        )
                    ),
                    release_mbid=str(release.mbid if release else ""),
                    artwork=artwork,
                    outcome="embedded-replaced",
                    authority_validated=authority_completion_validated,
                    album_path=str(album.path),
                    total_tracks=len(tracks),
                    completed_tracks=len(next_completed),
                )
                if completion_recorded:
                    completed_paths = next_completed
                else:
                    summary.unresolved += 1
                    _manual_unresolved(
                        track,
                        "Artwork was embedded and verified, but authoritative "
                        "Recording and Artist IDs were not saved; the track remains pending",
                        unresolved_items,
                    )
                summary.installed += 1
                action = (
                    "EMBEDDED ART REPLACED"
                    if completion_recorded
                    else "ARTWORK EMBEDDED · AUTHORITY REQUIRED"
                )
                semantic = core.green if completion_recorded else core.yellow
            else:
                completion_recorded = True
                summary.read_only += 1
                action = "READ-ONLY (would replace embedded art)"
                semantic = core.yellow
        except QueuedArtworkVerificationError:
            # A queued WebGUI write that cannot be read back must fail the job,
            # not fall through as an ordinary per-track provider miss.
            raise
        except Exception as exc:
            summary.failed += 1
            summary.unresolved += 1
            _manual_unresolved(
                track,
                f"Embedded artwork preparation/write failed: {exc}",
                unresolved_items,
            )
            continue

        summary.selected += 1
        if completion_recorded:
            summary.resolved += 1
            resolved_tracks += 1
        core.emit_ui(
            "album_material_result",
            outcome=action.title(),
            destination=str(track.path),
            file_action=("Replaced" if mode == "write" else "Preserved"),
            source=provider_label(selected.source),
            width=int(info.get("width", selected.width)),
            height=int(info.get("height", selected.height)),
            format=image_format,
            range_type="Ladder",
            distance=0,
            manual_compilation=(mode == "write" and completion_recorded),
            progress_completed=len(completed_paths),
            progress_total=len(tracks),
            progress_status=(
                "complete"
                if len(completed_paths) >= len(tracks)
                else "incomplete"
            ),
        )
        core.debug_log(
            "compilation.manual.track_done "
            f"file={str(track.path)!r} source={selected.source!r} "
            f"mode={mode!r}"
        )
        print(
            f"    {core.cyan('Artwork:'):11} "
            f"{semantic(action)} · {core.magenta(provider_label(selected.source))} "
            f"{candidate_provenance(selected)}"
        )

    core.debug_log(
        "compilation.manual.album_done "
        f"album={str(album.path)!r} tracks={len(tracks)} "
        f"resolved_this_run={resolved_tracks} "
        f"completed={len(completed_paths)} "
        f"unresolved={len(tracks) - len(completed_paths)}"
    )
    if mode == "write":
        _manual_progress(
            album,
            tracks,
            completed_paths,
            config_file=config_file,
            cfg=cfg,
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
BYPASS_HISTORY_CACHE_KEY = "album-bypass-state"
_BYPASS_OVERRIDE = False


def bypass_history_path(cache_dir: Path) -> Path:
    return Path(cache_dir) / "splined.db"


def load_bypass_history(path: Path) -> dict[str, Any]:
    empty = {"version": BYPASS_HISTORY_VERSION, "albums": {}}
    raw = core._load_cache_state(path, BYPASS_HISTORY_CACHE_KEY)
    if raw is None:
        return empty
    if not isinstance(raw, dict) or not isinstance(raw.get("albums"), dict):
        return empty
    return {"version": BYPASS_HISTORY_VERSION, "albums": {str(k): v for k, v in raw["albums"].items() if isinstance(v, dict)}}


def save_bypass_history(path: Path, history: dict[str, Any]) -> None:
    history["version"] = BYPASS_HISTORY_VERSION
    core._save_cache_state(
        path,
        BYPASS_HISTORY_CACHE_KEY,
        "bypass_history",
        history,
    )


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


def remove_album_bypass(
    path: Path,
    history: dict[str, Any],
    album_path: str | Path,
) -> bool:
    """Remove one persistent bypass record and save atomically."""
    key = str(album_path)
    albums = history.setdefault("albums", {})
    if key not in albums:
        return False
    del albums[key]
    save_bypass_history(path, history)
    return True


def remove_album_bypass_state(
    bypass_path: Path,
    bypass_history: dict[str, Any],
    completion_path: Path,
    completion_history: dict[str, Any],
    album_path: str | Path,
) -> bool:
    """Remove persistent bypass authority from both saved history surfaces."""
    key = str(album_path)
    changed = remove_album_bypass(bypass_path, bypass_history, key)
    albums = completion_history.setdefault("albums", {})
    entry = albums.get(key)
    if isinstance(entry, dict) and "bypass" in str(entry.get("outcome", "")).casefold():
        del albums[key]
        core.save_scan_completion_history(completion_path, completion_history)
        changed = True
    return changed


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
        core.emit_ui("album_material_result", outcome="Failed", file_action="Failed", detail=str(exc))
        print(f"  {core.red('ERROR: local artwork keep failed: ' + str(exc))}")
        return False
    summary.selected += 1
    summary.unchanged += 1
    core.emit_ui(
        "album_material_result",
        outcome="Unchanged",
        destination=str(local_candidate.path),
        file_action="Retained",
        source="Local",
        width=local_candidate.width,
        height=local_candidate.height,
        format=local_candidate.format,
    )
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
        if answer == "__batch_return__":
            raise core.InteractionReturn()
        if answer == "__cancel__":
            raise core.InteractionCancelled()
        if answer == "unbypass":
            return "unbypass", None
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

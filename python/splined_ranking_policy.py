"""Shared candidate ranking and local-art selection policy.

The projected final image remains the primary authority. When two candidates
project to the same acceptable result, SPLINED prefers the stronger original
source material before configured provider order. Local and embedded artwork
participate in the same scale/distance ranking as provider artwork.
"""

from __future__ import annotations

from typing import Any


_RANGE_RANK = {
    "Ideal": 0,
    "UpperRange": 1,
    "LowerRange": 2,
    "Ladder": 3,
    "BelowMinimum": 4,
    "AboveLadder": 5,
}
_POLICY_RANK = {"accept": 0, "fallback": 1, "reject": 2}


def _source_resolution_key(candidate: Any) -> tuple[int, int]:
    width = max(0, int(candidate.width))
    height = max(0, int(candidate.height))
    return (-min(width, height), -(width * height))


def _format_rank(projected: dict[str, Any], format_order: list[str]) -> int:
    value = str(projected["format"])
    return format_order.index(value) if value in format_order else 999999


def operational_candidate_key(
    scan: Any,
    candidate: Any,
    cfg: dict[str, Any],
    format_order: list[str],
) -> tuple[Any, ...]:
    projected = scan.project_candidate(candidate, cfg, format_order)
    # Downscaling a larger source to the same target is benign. Upscaling,
    # cropping, and conversion remain meaningful penalties.
    transform_penalty = (
        1 if projected["upscaled"] else 0,
        1 if projected["cropped"] else 0,
        1 if projected["converted"] else 0,
    )
    return (
        _POLICY_RANK.get(str(projected["policy_status"]), 2),
        int(projected["distance"]),
        _RANGE_RANK.get(str(projected["range_type"]), 99),
        transform_penalty,
        *_source_resolution_key(candidate),
        int(candidate.source_priority),
        _format_rank(projected, format_order),
        str(scan.provider_label(candidate.source)).casefold(),
        str(candidate.ref.id),
    )


def operational_fallback_sort_key(
    scan: Any,
    candidate: Any,
    cfg: dict[str, Any],
    format_order: list[str],
) -> tuple[Any, ...]:
    projected = scan.project_candidate(candidate, cfg, format_order)
    return (
        -int(projected["short_side"]),
        0 if projected["square"] else 1,
        0 if candidate.ref.approved else 1,
        *_source_resolution_key(candidate),
        int(candidate.source_priority),
        _format_rank(projected, format_order),
        str(candidate.ref.id),
    )


def operational_fallback_suggested(
    scan: Any,
    candidates: list[Any],
    cfg: dict[str, Any],
    format_order: list[str],
) -> Any | None:
    if not candidates:
        return None

    def key(candidate: Any) -> tuple[Any, ...]:
        projected = scan.project_candidate(candidate, cfg, format_order)
        return (
            int(projected["distance"]),
            0 if projected["square"] else 1,
            0 if candidate.ref.approved else 1,
            *_source_resolution_key(candidate),
            int(candidate.source_priority),
            _format_rank(projected, format_order),
            str(candidate.ref.id),
        )

    return min(candidates, key=key)


def operational_preferred_candidate(
    scan: Any,
    candidates: list[Any],
    cfg: dict[str, Any],
    format_order: list[str],
) -> Any | None:
    """Choose (s) across local/embedded and provider candidates together."""
    if not candidates:
        return None
    acceptable = [
        candidate
        for candidate in candidates
        if bool(scan.project_candidate(candidate, cfg, format_order)["acceptable"])
    ]
    if acceptable:
        return min(
            acceptable,
            key=lambda candidate: operational_candidate_key(
                scan,
                candidate,
                cfg,
                format_order,
            ),
        )
    return operational_fallback_suggested(
        scan,
        candidates,
        cfg,
        format_order,
    )


def local_comparison_selection_action(
    candidate: Any,
    local_candidate: Any,
) -> str:
    """Only an existing canonical local file is a keep action.

    Embedded/WebP-still candidates are source material. Selecting them by (s)
    or number must materialize the configured cover file.
    """
    if candidate is local_candidate and str(candidate.source).casefold() == "local":
        return "keep"
    return "selected"


def operational_output_extension(core: Any, candidate: Any, target: str) -> str:
    """Use .jpeg for selected embedded JPEG while preserving other policy."""
    if str(candidate.source).casefold() == "embedded" and target == "jpeg":
        return "jpeg"
    return str(core.EXTENSIONS[target])


def core_candidate_key(
    core: Any,
    candidate: Any,
    cfg: dict[str, Any],
    format_order: list[str],
) -> tuple[Any, ...]:
    projected = core.project_candidate(
        candidate,
        cfg,
        core.target_format_for_candidate(candidate, format_order),
    )
    transform_penalty = (
        1 if projected["upscaled"] else 0,
        1 if projected["cropped"] else 0,
        1 if projected["converted"] else 0,
    )
    return (
        _POLICY_RANK.get(str(projected["policy_status"]), 2),
        int(projected["distance"]),
        _RANGE_RANK.get(str(projected["range_type"]), 99),
        transform_penalty,
        *_source_resolution_key(candidate),
        int(candidate.source_priority),
        _format_rank(projected, format_order),
        str(core.provider_label(candidate.source)).casefold(),
        str(candidate.ref.id),
    )


def install(core: Any, scan: Any) -> None:
    """Install one ranking/output policy across core and operational scan."""
    if getattr(scan, "_splined_ranking_policy_installed", False):
        return

    original_render_comparison = scan.render_local_suggested_comparison
    original_preview_destination = scan.preview_destination
    original_finalize = scan.finalize
    original_apply_local_preflight = scan.apply_local_preflight

    def scan_candidate_key(
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> tuple[Any, ...]:
        return operational_candidate_key(scan, candidate, cfg, format_order)

    def scan_fallback_sort_key(
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> tuple[Any, ...]:
        return operational_fallback_sort_key(scan, candidate, cfg, format_order)

    def scan_fallback_suggested(
        candidates: list[Any],
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> Any | None:
        return operational_fallback_suggested(
            scan,
            candidates,
            cfg,
            format_order,
        )

    def shared_core_candidate_key(
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> tuple[Any, ...]:
        return core_candidate_key(core, candidate, cfg, format_order)

    def shared_render_local_suggested_comparison(
        local_candidate: Any,
        suggested: Any | None,
        cfg: dict[str, Any],
        format_order: list[str],
    ) -> None:
        if suggested is not local_candidate:
            original_render_comparison(
                local_candidate,
                suggested,
                cfg,
                format_order,
            )
            return

        projected = scan.project_candidate(local_candidate, cfg, format_order)
        source = scan.provider_label(local_candidate.source)
        crop = scan.candidate_crop_text(local_candidate, projected)
        print()
        print(core.bold(core.cyan("Local & Suggested (s) Artwork Comparison")))
        print()
        print(
            scan.candidate_compare_line(
                "Local:",
                local_candidate,
                cfg,
                format_order,
                local_candidate.path.name,
            )
        )
        print(core.gray("-" * 150))
        print(
            core.ljust_color(core.cyan("Result:"), 12)
            + scan._field("Source", f"{source} preferred", core.green)
            + " "
            + scan._field(
                "Reason",
                "best projected scale / distance",
                core.green,
            )
            + " "
            + scan._field("Distance", str(projected["distance"]))
            + " "
            + scan._field(
                "Crop risk",
                crop,
                core.red if projected.get("crop_guarded") else core.green,
            )
        )

    def shared_local_comparison_prompt(
        local_candidate: Any,
        remote: list[Any],
        cfg: dict[str, Any],
        format_order: list[str],
        mb_retry_available: bool = False,
    ) -> tuple[str, Any | None]:
        all_candidates = [local_candidate] + list(remote)
        suggested = operational_preferred_candidate(
            scan,
            all_candidates,
            cfg,
            format_order,
        )
        shared_render_local_suggested_comparison(
            local_candidate,
            suggested,
            cfg,
            format_order,
        )
        if all_candidates:
            print()
            scan.render_candidate_table(
                all_candidates,
                cfg,
                format_order,
                suggested=suggested,
                manual_fallback=True,
            )

        while True:
            print()
            menu = (
                f"  {core.paint('PURPLE', '[s]')} use preferred candidate   "
                f"{core.cyan('[k]')} keep current source only   "
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
            if answer == "__cancel__":
                raise core.TuiSessionExit()
            if answer == "unbypass":
                return "unbypass", None
            if answer == "s":
                if suggested is None:
                    print(core.yellow("  No preferred candidate is available."))
                    continue
                return (
                    local_comparison_selection_action(
                        suggested,
                        local_candidate,
                    ),
                    suggested,
                )
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
                    return (
                        local_comparison_selection_action(
                            chosen,
                            local_candidate,
                        ),
                        chosen,
                    )
                print(core.yellow("  Choose a listed candidate number."))
                continue
            choices = "s, k, a listed number, b" + (
                ", or m" if mb_retry_available else ""
            )
            print(f"  Choose {choices}.")

    def shared_preview_destination(
        album: Any,
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
        allow_out_of_range: bool = False,
    ) -> tuple[Any, dict[str, Any]]:
        if str(candidate.source).casefold() != "embedded":
            return original_preview_destination(
                album,
                candidate,
                cfg,
                format_order,
                allow_out_of_range,
            )
        output = core.section(cfg, "output")
        preserve = bool(output.get("preserve_file", True))
        name = str(output.get("file_name", "cover")).strip()
        core.validate_file_name(name)
        target = scan.target_format_for_candidate(candidate, format_order)
        content, info = core.prepare_final(
            candidate,
            cfg,
            target,
            allow_out_of_range=allow_out_of_range,
        )
        extension = operational_output_extension(core, candidate, target)
        canonical = album.path / f"{name}.{extension}"
        if not preserve:
            return canonical, info
        destination, _ = core.choose_destination(canonical, content, True)
        return destination, info

    def shared_finalize(
        album: Any,
        candidate: Any,
        cfg: dict[str, Any],
        format_order: list[str],
        allow_out_of_range: bool = False,
    ) -> tuple[str, Any, dict[str, Any], bool]:
        if str(candidate.source).casefold() != "embedded":
            return original_finalize(
                album,
                candidate,
                cfg,
                format_order,
                allow_out_of_range,
            )
        output = core.section(cfg, "output")
        preserve = bool(output.get("preserve_file", True))
        name = str(output.get("file_name", "cover")).strip()
        core.validate_file_name(name)
        target = scan.target_format_for_candidate(candidate, format_order)
        content, info = core.prepare_final(
            candidate,
            cfg,
            target,
            allow_out_of_range=allow_out_of_range,
        )
        extension = operational_output_extension(core, candidate, target)
        canonical = album.path / f"{name}.{extension}"
        mode = str(cfg.get("mode", "read")).strip().lower()

        if not preserve:
            existed = canonical.exists()
            if mode == "read":
                unchanged = existed and canonical.read_bytes() == content
                return (
                    "UNCHANGED" if unchanged else "READ-ONLY (would install)",
                    canonical,
                    info,
                    existed,
                )
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

    def shared_apply_local_preflight(
        album: Any,
        sample_release: Any,
        preflight: dict[str, Any],
        cfg: dict[str, Any],
        format_order: list[str],
        sample_dir: Any,
        preserve: bool,
        samples_enabled: bool,
        summary: Any,
    ) -> bool:
        if str(preflight.get("action", "")) != "embedded-ideal":
            return original_apply_local_preflight(
                album,
                sample_release,
                preflight,
                cfg,
                format_order,
                sample_dir,
                preserve,
                samples_enabled,
                summary,
            )
        return scan.apply_selected_candidate(
            album,
            sample_release,
            preflight["candidate"],
            cfg,
            format_order,
            sample_dir,
            preserve,
            samples_enabled,
            summary,
        )

    scan.candidate_key = scan_candidate_key
    scan.fallback_sort_key = scan_fallback_sort_key
    scan.fallback_suggested = scan_fallback_suggested
    core.candidate_key = shared_core_candidate_key
    scan.render_local_suggested_comparison = (
        shared_render_local_suggested_comparison
    )
    scan.local_comparison_prompt = shared_local_comparison_prompt
    scan.preview_destination = shared_preview_destination
    scan.finalize = shared_finalize
    scan.apply_local_preflight = shared_apply_local_preflight
    scan._splined_ranking_policy_installed = True

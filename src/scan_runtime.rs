use crate::candidate::{Candidate, StaticFormat};
use crate::config::{Config, Mode, OutputConfig, samples_dir};
use crate::final_artwork::{
    FinalArtworkAction, FinalArtworkResult, PreparedArtwork, destination_matches_prepared,
    install_prepared_artwork, prepare_configured_artwork, project_configured_artwork,
};
use crate::gui_events;
use crate::history::{
    AlbumHistoryState, album_history_status, load_completion_history, record_scan_completion,
    unix_now,
};
use crate::inspect::inspect_image;
use crate::local_artwork::{
    LocalPreflightAction, cleanup_competing_static, cleanup_replaced_static_covers,
    inspect_local_preflight,
};
use crate::musicbrainz::MusicBrainzClient;
use crate::pipeline::{
    PipelineCandidate, RegistryPipelineOptions, candidate_summary, prepare_persistent_cache_dir,
    run_registry_pipeline_with_cache_dir,
};
use crate::range::Range;
use crate::safe_write::replace_binary_file;
use crate::scan::inventory_album_directories;
use crate::scan_musicbrainz::{
    AlbumReleaseDecision, LocalTrackEvidence, TaggedAlbumIdAudit, compilation_context,
    resolve_album_release_with_fallback, tagged_album_id_audit, tagged_album_title,
    tagged_album_title_matches_release,
};
use crate::scan_tags::read_album_track_evidence;
use crate::source::{ArtworkQuery, ProviderContext, ProviderRegistry};
use crate::source_history::{load_source_history, record_source_selection};
use crate::source_policy::{
    SourcePolicyDecision, SourcePolicyStatus, active_policy, global_range_decision,
    source_override_decision,
};
use crossterm::style::{Color, Stylize};
use serde_json::json;
use std::cmp::Ordering;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const ORANGE: Color = Color::AnsiValue(208);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadScanSummary {
    pub albums: usize,
    pub postponed: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub failed: usize,
    pub selected: usize,
    pub samples_written: usize,
    pub samples_unchanged: usize,
    pub installed: usize,
    pub unchanged: usize,
    pub read_only: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SampleWriteResult {
    path: PathBuf,
    unchanged: bool,
}

pub async fn run_scan_library_read_report(
    config: &Config,
    resolved_sources: &[String],
) -> Result<ReadScanSummary, String> {
    if config.scan.scan_library_dir.trim().is_empty() {
        return Err(
            "SPLINED [scan].scan_library_dir is not configured; refusing to scan the current working directory."
                .to_string(),
        );
    }

    let root = Path::new(&config.scan.scan_library_dir);
    let cache_dir = Path::new(&config.scan.cache_dir);
    let sample_dir = samples_dir(&config.scan.cache_dir);

    prepare_persistent_cache_dir(cache_dir)?;
    prepare_samples_dir(&sample_dir)?;

    let inventory = inventory_album_directories(root, &config.library.ignored_subs)?;
    let musicbrainz = MusicBrainzClient::new(&config.musicbrainz)?;
    let discogs_credential_file = Path::new(&config.credentials.credential_dir)
        .join("discogs.json")
        .to_string_lossy()
        .into_owned();

    let registry = ProviderRegistry::from_source_order_with_credentials(
        resolved_sources,
        &config.fanarttv.credential_file,
        &config.lastfm.credential_file,
        &discogs_credential_file,
    )?;

    if registry.is_empty() {
        return Err("No implemented artwork provider is enabled for scan testing.".to_string());
    }

    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let format_order = configured_output_formats(&config.output.file_formats)?;
    let mut completion_history = load_completion_history(config);
    let mut source_history = load_source_history(config);

    let ignored_count = inventory.ignored_directories.len();
    let ignored_display = if ignored_count == 0 {
        ignored_count.to_string().green().bold()
    } else if inventory.albums.is_empty() {
        ignored_count.to_string().red().bold()
    } else {
        ignored_count.to_string().with(ORANGE).bold()
    };

    let (mode_label, mutation_label, detail_label) = match config.mode {
        Mode::Read => (
            "Read",
            "disabled".green().bold(),
            "verbose diagnostics / review cycle (Read mode)".cyan(),
        ),
        Mode::Write => (
            "Write",
            "enabled".red().bold(),
            "live artwork mutation (Write mode)".with(ORANGE).bold(),
        ),
    };

    println!(
        "{}",
        format!(
            "SPLINED SCAN LIBRARY {} TEST",
            mode_label.to_ascii_uppercase()
        )
        .cyan()
        .bold()
    );
    println!();
    println!("Mode:        {}", mode_label.with(ORANGE).bold());
    println!("Mutation:    {mutation_label}");
    println!("Detail:      {detail_label}");
    println!(
        "Directory:   {}",
        inventory.root.display().to_string().yellow().bold()
    );
    println!(
        "Cache:       {}",
        cache_dir.display().to_string().cyan().bold()
    );
    println!(
        "Samples:     {}",
        if config.samples.sample_write {
            "enabled".green().bold()
        } else {
            "disabled".yellow().bold()
        }
    );
    println!(
        "Sample Dir:  {}",
        sample_dir.display().to_string().cyan().bold()
    );
    println!(
        "Preserve:    {}",
        config.output.preserve_file.to_string().cyan().bold()
    );
    println!(
        "Albums:      {}",
        inventory.albums.len().to_string().with(ORANGE).bold()
    );
    println!("Ignored:     {ignored_display}");
    println!(
        "MusicBrainz: {}",
        format!("{:?}", musicbrainz.request_mode()).cyan().bold()
    );
    println!(
        "MB Retry Max: {}",
        musicbrainz.retry_max().to_string().cyan().bold()
    );
    println!(
        "MB Min Delay: {}",
        format!("{:.2}s", musicbrainz.min_delay()).cyan().bold()
    );
    println!(
        "MB Rec Timeout: {}",
        format!("{:.1}s", musicbrainz.recording_timeout_seconds())
            .cyan()
            .bold()
    );
    println!(
        "Providers:   {}{}{}",
        "[".white(),
        registry.names().join(", ").magenta().bold(),
        "]".white()
    );
    println!("Sources:     [{}]", resolved_sources.join(", "));
    println!(
        "Output:      {}",
        configured_output_label(&config.output.file_name, &format_order)?
            .yellow()
            .bold()
    );
    println!();

    let mut summary = ReadScanSummary {
        albums: inventory.albums.len(),
        ..ReadScanSummary::default()
    };

    for (index, album) in inventory.albums.iter().enumerate() {
        if gui_events::cancelled() {
            return Err("SPLINED scan stopped by user.".to_string());
        }
        let history_status = album_history_status(
            &completion_history,
            album,
            config,
            resolved_sources,
            unix_now(),
        );
        if history_status.state == AlbumHistoryState::Bypassed
            && !gui_events::bypass_allowed(&album.path)
        {
            gui_events::emit(json!({
                "event": "album_skipped",
                "album_path": album.path,
                "reason": "bypassed",
            }));
            continue;
        }
        if history_status.state == AlbumHistoryState::TimeoutActive {
            summary.postponed += 1;
            gui_events::emit(json!({
                "event": "album_postponed",
                "album_path": album.path,
                "eligible_at_unix": history_status.eligible_at_unix,
                "remaining_seconds": history_status.remaining.map(|value| value.as_secs()),
            }));
            continue;
        }
        gui_events::emit(json!({
            "event": "album_started",
            "index": index + 1,
            "total": inventory.albums.len(),
            "album_path": album.path,
        }));
        println!(
            "{}",
            format!(
                "[{}/{}] {}",
                index + 1,
                inventory.albums.len(),
                album.path.display()
            )
            .cyan()
            .bold()
        );

        let tracks = match read_album_track_evidence(album) {
            Ok(tracks) => tracks,
            Err(error) => {
                summary.failed += 1;
                println!("  {}", format!("ERROR: {error}").red().bold());
                println!();
                continue;
            }
        };

        let compilation = compilation_context(&tracks);
        let tagged_album = tagged_album_title(&tracks);
        let mbid_audit = tagged_album_id_audit(&tracks);

        println!(
            "  Tracks:      {}",
            tracks.len().to_string().with(ORANGE).bold()
        );
        println!("  Compilation: {}", format!("{compilation:?}").blue());
        println!(
            "  Tagged Album: {}",
            tagged_album
                .as_deref()
                .unwrap_or("unavailable")
                .yellow()
                .bold()
        );

        let mut local_preflight = inspect_local_preflight(album, config, &range, cache_dir);
        for diagnostic in &local_preflight.diagnostics {
            println!("  {}", format!("Local artwork: {diagnostic}").yellow());
        }
        if local_preflight.webp_source.is_some() {
            gui_events::emit(json!({
                "event": "local_preflight",
                "album_path": album.path,
                "action": "webp-still",
                "message": "Preserved cover.webp and generated a safe JPEG still for Local & Suggested comparison.",
                "destination": local_preflight.generated_destination,
            }));
        }

        if matches!(
            local_preflight.action,
            LocalPreflightAction::LocalIdeal | LocalPreflightAction::EmbeddedIdeal
        ) {
            let Some(best) = local_preflight.candidate.take() else {
                summary.failed += 1;
                println!("  ERROR: local artwork preflight did not return its selected candidate.");
                continue;
            };
            let candidate = &best.downloaded.candidate;
            if config.samples.sample_write
                && let Some(sample_context) =
                    fallback_provider_context(&tracks, tagged_album.as_deref())
            {
                match write_selected_sample(
                    &sample_dir,
                    &sample_context.artist_credit,
                    &sample_context.release_title,
                    candidate,
                    best.downloaded.path(),
                    config.output.preserve_file,
                ) {
                    Ok(sample) if sample.unchanged => summary.samples_unchanged += 1,
                    Ok(_) => summary.samples_written += 1,
                    Err(error) => {
                        summary.failed += 1;
                        println!(
                            "  {}",
                            format!("ERROR: local sample failed: {error}").red().bold()
                        );
                        continue;
                    }
                }
            }
            let target_format = match target_format_for_candidate(candidate.format, &format_order) {
                Ok(format) => format,
                Err(error) => {
                    summary.failed += 1;
                    println!("  {}", format!("ERROR: {error}").red().bold());
                    continue;
                }
            };
            let destination = if local_preflight.action == LocalPreflightAction::LocalIdeal {
                best.downloaded.path().to_path_buf()
            } else {
                match output_destination(&album.path, &config.output.file_name, target_format) {
                    Ok(path) => path,
                    Err(error) => {
                        summary.failed += 1;
                        println!("  {}", format!("ERROR: {error}").red().bold());
                        continue;
                    }
                }
            };
            match finalize_selected_with_preserve(
                config.mode,
                candidate,
                best.downloaded.path(),
                &destination,
                &range,
                target_format,
                FinalizationOptions {
                    preserve_file: false,
                    explicit_manual_selection: false,
                    output: &config.output,
                },
            ) {
                Ok((final_result, destination)) => {
                    if let Err(error) = cleanup_competing_static(
                        &local_preflight.cleanup,
                        config.mode,
                        config.output.preserve_file,
                    ) {
                        summary.failed += 1;
                        println!("  {}", format!("ERROR: {error}").red().bold());
                        continue;
                    }
                    summary.selected += 1;
                    match final_result.action {
                        FinalArtworkAction::Installed => summary.installed += 1,
                        FinalArtworkAction::Unchanged => summary.unchanged += 1,
                        FinalArtworkAction::ReadOnly => summary.read_only += 1,
                        FinalArtworkAction::NoSelection => {}
                    }
                    let action = if local_preflight.action == LocalPreflightAction::LocalIdeal {
                        "local-ideal"
                    } else {
                        "embedded-ideal"
                    };
                    let _ = record_scan_completion(
                        config,
                        &mut completion_history,
                        album,
                        resolved_sources,
                        action,
                    );
                    gui_events::emit(json!({
                        "event": "local_preflight",
                        "album_path": album.path,
                        "action": action,
                        "message": if action == "local-ideal" {
                            "Ideal canonical local artwork accepted before provider discovery."
                        } else {
                            "Ideal embedded front cover accepted before provider discovery."
                        },
                        "destination": destination,
                    }));
                    gui_events::emit(json!({
                        "event": "album_completed",
                        "album_path": album.path,
                        "destination": destination,
                        "action": format!("{:?}", final_result.action),
                        "mode": format!("{:?}", config.mode).to_ascii_lowercase(),
                    }));
                    println!("  Artwork:     {}", action.green().bold());
                }
                Err(error) => {
                    summary.failed += 1;
                    println!(
                        "  {}",
                        format!("ERROR: local artwork preflight failed: {error}")
                            .red()
                            .bold()
                    );
                }
            }
            println!();
            continue;
        }

        let local_comparison_candidate = if local_preflight.action == LocalPreflightAction::Compare
        {
            local_preflight.candidate.take()
        } else {
            None
        };

        if mbid_audit.valid.len() != 1
            || !mbid_audit.missing.is_empty()
            || !mbid_audit.invalid.is_empty()
        {
            print_album_id_diagnostics(&musicbrainz, &tracks, &mbid_audit).await;
        }

        let decision = match resolve_album_release_with_fallback(&musicbrainz, &tracks).await {
            Ok(decision) => decision,
            Err(error) => {
                summary.failed += 1;
                println!(
                    "  {}",
                    format!("ERROR: MusicBrainz release resolution failed: {error}")
                        .red()
                        .bold()
                );
                println!();
                continue;
            }
        };

        let mut query_and_context: Option<(ArtworkQuery, ProviderContext)> = None;
        let mut fallback_reason: Option<String> = None;
        let mut fallback_release_mbid: Option<String> = None;
        let mut musicbrainz_retry_available = false;

        match decision {
            AlbumReleaseDecision::Resolved { release_mbid, .. } if !musicbrainz.is_enabled() => {
                fallback_reason = Some("MUSICBRAINZ SOURCE DISABLED".to_string());
                fallback_release_mbid = Some(release_mbid);
            }
            AlbumReleaseDecision::Resolved {
                release_mbid,
                authority,
            } => match musicbrainz.lookup_release(&release_mbid).await {
                Ok(release) => {
                    let title_match = tagged_album_title_matches_release(&tracks, &release.title);
                    print_tagged_release_result(
                        tagged_album.as_deref(),
                        &release.title,
                        &release_mbid,
                        title_match,
                    );
                    println!("  Authority:   {}", format!("{authority:?}").green().bold());
                    if title_match == Some(false) {
                        fallback_reason = Some("TAG / RELEASE MISMATCH".to_string());
                        fallback_release_mbid = Some(release_mbid);
                    } else {
                        summary.resolved += 1;
                        println!(
                            "  MB Artist:   {}",
                            release.artist_credit.as_str().green().bold()
                        );
                        println!("  MB Release:  {}", release.title.as_str().yellow().bold());
                        gui_events::emit(json!({
                            "event": "release_resolved",
                            "album_path": album.path,
                            "artist": release.artist_credit,
                            "release": release.title,
                            "release_mbid": release_mbid,
                            "evidence_track": mbid_audit.valid.first()
                                .and_then(|item| item.track_paths.first()),
                            "authority": format!("{authority:?}"),
                            "fallback": false,
                        }));
                        query_and_context = Some((
                            ArtworkQuery::release(&release_mbid),
                            ProviderContext {
                                artist_credit: release.artist_credit,
                                release_title: release.title,
                                release_group_mbid: release.release_group_id,
                                release_group_title: release.release_group_title,
                            },
                        ));
                    }
                }
                Err(error) => {
                    fallback_reason = Some(fallback_reason_from_error(&error));
                    fallback_release_mbid = Some(release_mbid);
                    musicbrainz_retry_available = true;
                }
            },
            AlbumReleaseDecision::Fallback { .. } => {
                fallback_reason = Some(if mbid_audit.valid.len() > 1 {
                    "MULTIPLE MBIDS".to_string()
                } else if !mbid_audit.invalid.is_empty() {
                    "INVALID MBID".to_string()
                } else {
                    "NO MBID FOUND".to_string()
                });
            }
        }

        if query_and_context.is_none() {
            summary.unresolved += 1;
            print_unresolved_tagged_release(&tagged_album, &mbid_audit);
            let Some(context) = fallback_provider_context(&tracks, tagged_album.as_deref()) else {
                println!(
                    "  Artwork:     {}",
                    "fallback unavailable: artist/title tags missing"
                        .yellow()
                        .bold()
                );
                println!();
                continue;
            };
            println!(
                "  Fallback:    {}",
                fallback_reason
                    .as_deref()
                    .unwrap_or("TAG FALLBACK")
                    .yellow()
                    .bold()
            );
            gui_events::emit(json!({
                "event": "release_resolved",
                "album_path": album.path,
                "artist": context.artist_credit,
                "release": context.release_title,
                "release_mbid": "",
                "authority": "TaggedFallback",
                "fallback": true,
                "reason": fallback_reason,
            }));
            query_and_context = Some((
                ArtworkQuery::release(fallback_release_mbid.clone().unwrap_or_default()),
                context,
            ));
        }

        let (query, context) = query_and_context.expect("normal or fallback provider context");
        let fallback_sources: Vec<String> = resolved_sources
            .iter()
            .filter(|source| {
                source.as_str() != "fanarttv"
                    && (source.as_str() != "coverartarchive"
                        || !query.release_mbid.trim().is_empty())
            })
            .cloned()
            .collect();
        let fallback_registry = if fallback_reason.is_some() {
            Some(ProviderRegistry::from_source_order_with_credentials(
                &fallback_sources,
                &config.fanarttv.credential_file,
                &config.lastfm.credential_file,
                &discogs_credential_file,
            )?)
        } else {
            None
        };
        let active_registry = fallback_registry.as_ref().unwrap_or(&registry);

        if active_registry.is_empty() {
            summary.failed += 1;
            println!("  ERROR: no configured provider can participate in fallback.");
            continue;
        }

        let mut result = match run_registry_pipeline_with_cache_dir(
            active_registry,
            &query,
            &context,
            RegistryPipelineOptions {
                source_order: resolved_sources,
                range: &range,
                format_order: &format_order,
                source_policies: &config.source_policies,
            },
            cache_dir,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                summary.failed += 1;
                println!(
                    "  {}",
                    format!("ERROR: artwork pipeline failed: {error}")
                        .red()
                        .bold()
                );
                println!();
                continue;
            }
        };

        if let Some(local) = local_comparison_candidate {
            if let Some(best_index) = result.best_index.as_mut() {
                *best_index += 1;
            }
            result.candidates.insert(0, local);
        }

        let automatic_index = result
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let candidate = &item.downloaded.candidate;
                if matches!(candidate.source.as_str(), "local" | "webpstill") {
                    return None;
                }
                let projected = project_configured_artwork(candidate, &range, &config.output);
                let policy = configured_candidate_policy(candidate, &projected, &range, config);
                if policy.status == SourcePolicyStatus::Reject {
                    return None;
                }
                let target = target_format_for_candidate(candidate.format, &format_order).ok()?;
                let format_priority = format_order
                    .iter()
                    .position(|format| *format == target)
                    .unwrap_or(usize::MAX);
                Some((
                    index,
                    (
                        if policy.status == SourcePolicyStatus::Accept {
                            0
                        } else {
                            1
                        },
                        projected.width.min(projected.height).abs_diff(range.ideal),
                        projected.upscaled,
                        projected.cropped,
                        projected.resized,
                        candidate.format != target,
                        candidate.source_priority,
                        format_priority,
                        std::cmp::Reverse(projected.width.min(projected.height)),
                    ),
                ))
            })
            .min_by_key(|(_, key)| *key)
            .map(|(index, _)| index);
        let mut display_indices: Vec<usize> = (0..result.candidates.len()).collect();
        display_indices.retain(|candidate_index| {
            let candidate = &result.candidates[*candidate_index].downloaded.candidate;
            candidate_visible_for_review(candidate, &range, config)
        });
        let hidden_by_source_policy = result.candidates.len() - display_indices.len();
        if fallback_reason.is_some() {
            display_indices.sort_by(|left_index, right_index| {
                compare_fallback_candidates(
                    &result.candidates[*left_index],
                    &result.candidates[*right_index],
                    &range,
                    &format_order,
                    &config.output,
                )
            });
            display_indices.truncate(10);
        }
        let suggested_index = automatic_index.or_else(|| {
            display_indices
                .iter()
                .copied()
                .filter(|index| {
                    !matches!(
                        result.candidates[*index]
                            .downloaded
                            .candidate
                            .source
                            .as_str(),
                        "local" | "webpstill"
                    )
                })
                .min_by(|left_index, right_index| {
                    compare_fallback_suggestions(
                        &result.candidates[*left_index],
                        &result.candidates[*right_index],
                        &range,
                        &format_order,
                        &config.output,
                    )
                })
        });
        let mut selected_index = if gui_events::review_required() {
            None
        } else {
            automatic_index
        };
        let mut manually_selected = false;

        let gui_candidates: Vec<_> = display_indices
            .iter()
            .map(|candidate_index| {
                let item = &result.candidates[*candidate_index];
                let candidate = &item.downloaded.candidate;
                let projected = project_configured_artwork(candidate, &range, &config.output);
                let policy = configured_candidate_policy(candidate, &projected, &range, config);
                let source_override_active =
                    active_policy(&config.source_policies, &candidate.source).is_some();
                json!({
                    "index": *candidate_index + 1,
                    "source": candidate.source,
                    "width": candidate.width,
                    "height": candidate.height,
                    "format": format!("{:?}", candidate.format).to_ascii_lowercase(),
                    "range_class": projected_range_class(projected.width.min(projected.height), &range),
                    "distance_from_ideal": projected.width.min(projected.height).abs_diff(range.ideal),
                    "square": candidate.is_square(),
                    "acceptable": policy.status != SourcePolicyStatus::Reject,
                    "policy_status": policy.status,
                    "policy_reason": policy.reason,
                    "source_override_active": source_override_active,
                    "projected_width": projected.width,
                    "projected_height": projected.height,
                    "cropped": projected.cropped,
                    "resized": projected.resized,
                    "upscaled": projected.upscaled,
                    "approved": item.reference.approved,
                    "reference_id": item.reference.id,
                    "local_origin": if item.reference.types.iter().any(|value| value == "EmbeddedTrack") {
                        "embedded-track"
                    } else if item.reference.types.iter().any(|value| value == "CoverFile") {
                        "cover-file"
                    } else {
                        ""
                    },
                    "local_reference": item.reference.id,
                    "url": item.reference.url,
                    "cache_path": item.downloaded.path(),
                    "recommended": Some(*candidate_index) == suggested_index,
                })
            })
            .collect();
        gui_events::emit(json!({
            "event": "candidates",
            "album_path": album.path,
            "items": gui_candidates,
            "recommended_index": suggested_index.map(|value| value + 1),
            "hidden_by_source_policy": hidden_by_source_policy,
            "fallback": fallback_reason.is_some(),
            "fallback_reason": fallback_reason,
            "musicbrainz_retry_available": musicbrainz_retry_available,
        }));

        if selected_index.is_none()
            && (!result.candidates.is_empty() || fallback_reason.is_some())
            && gui_events::decisions_available()
        {
            gui_events::emit(json!({
                "event": "decision_required",
                "album_path": album.path,
                "reason": if fallback_reason.is_some() { "fallback" } else if gui_events::review_required() { "review" } else { "outside-range" },
                "fallback_reason": fallback_reason,
                "suggested_index": suggested_index.map(|value| value + 1),
                "allow_bypass": true,
                "musicbrainz_retry_available": musicbrainz_retry_available,
            }));
            if !gui_events::enabled() {
                print!("  Choose a candidate number or b to bypass: ");
                let _ = io::stdout().flush();
            }
            match gui_events::wait_for_candidate_decision() {
                Ok(gui_events::CandidateDecision::Use(index))
                    if index < result.candidates.len() =>
                {
                    let candidate = &result.candidates[index].downloaded.candidate;
                    if !candidate_visible_for_review(candidate, &range, config) {
                        summary.failed += 1;
                        println!(
                            "  {}",
                            "ERROR: candidate is rejected by the active source policy."
                                .red()
                                .bold()
                        );
                        gui_events::emit(json!({
                            "event": "album_error",
                            "album_path": album.path,
                            "message": "Candidate is rejected by the active source policy."
                        }));
                        continue;
                    }
                    selected_index = Some(index);
                    manually_selected = true;
                }
                Ok(gui_events::CandidateDecision::Use(index)) => {
                    summary.failed += 1;
                    println!("  ERROR: candidate {} does not exist.", index + 1);
                    continue;
                }
                Ok(gui_events::CandidateDecision::Bypass) => {
                    let outcome = if fallback_reason.is_some() {
                        "fallback-bypassed"
                    } else {
                        "normal-out-of-range-bypassed"
                    };
                    let _ = record_scan_completion(
                        config,
                        &mut completion_history,
                        album,
                        resolved_sources,
                        outcome,
                    );
                    gui_events::emit(json!({
                        "event": "album_completed",
                        "album_path": album.path,
                        "destination": "",
                        "action": "Bypassed",
                        "mode": format!("{:?}", config.mode).to_ascii_lowercase(),
                    }));
                    println!("  Artwork:     {}", "BYPASSED".yellow().bold());
                    println!();
                    continue;
                }
                Ok(gui_events::CandidateDecision::RetryMusicBrainz)
                    if musicbrainz_retry_available =>
                {
                    gui_events::emit(json!({
                        "event": "album_musicbrainz_retry_requested",
                        "album_path": album.path,
                    }));
                    println!("  MusicBrainz lookup will retry for the current album.");
                    println!();
                    continue;
                }
                Ok(gui_events::CandidateDecision::RetryMusicBrainz) => {
                    summary.failed += 1;
                    println!(
                        "  ERROR: MusicBrainz retry is available only after an actual lookup failure."
                    );
                    continue;
                }
                Ok(gui_events::CandidateDecision::Retry {
                    artist,
                    album: retry_album,
                }) if fallback_reason.is_some() => {
                    gui_events::emit(json!({
                        "event": "album_retry_requested",
                        "album_path": album.path,
                        "artist": artist,
                        "release": retry_album,
                    }));
                    println!("  Fallback search will retry with edited Artist / Album values.");
                    println!();
                    continue;
                }
                Ok(gui_events::CandidateDecision::Retry { .. }) => {
                    summary.failed += 1;
                    println!("  ERROR: fallback retry is only available in fallback mode.");
                    continue;
                }
                Err(error) => {
                    summary.failed += 1;
                    println!("  ERROR: {error}");
                    continue;
                }
            }
        }

        let chosen_source = selected_index
            .and_then(|index| result.candidates.get(index))
            .map(|best| best.downloaded.candidate.source.as_str());
        match chosen_source {
            Some(source) => println!(
                "  Candidates:  {}  {}{}{}",
                result.candidates.len().to_string().with(ORANGE).bold(),
                "[".white(),
                source.magenta().bold(),
                "]".white()
            ),
            None => println!(
                "  Candidates:  {}  {}{}{}",
                result.candidates.len().to_string().with(ORANGE).bold(),
                "[".white(),
                "none".yellow().bold(),
                "]".white()
            ),
        }

        for (candidate_index, item) in result.candidates.iter().enumerate() {
            let selected = Some(candidate_index) == selected_index;
            let marker = if selected { "*" } else { " " };
            let line = format!(
                "    {marker} [{}] {}",
                candidate_index + 1,
                candidate_summary(item, &range, &format_order)
            );

            if selected {
                println!("{}", line.green().bold());
            } else {
                println!("{}", line.dark_grey());
            }
        }

        if !result.diagnostics.is_empty() {
            gui_events::emit(json!({
                "event": "provider_diagnostics",
                "album_path": album.path,
                "items": result.diagnostics.iter().map(|item| json!({
                    "source": item.source,
                    "url": item.url,
                    "message": item.message,
                })).collect::<Vec<_>>(),
            }));
            println!("  {}", "Provider diagnostics:".yellow().bold());
            for diagnostic in &result.diagnostics {
                if diagnostic.url.is_empty() {
                    println!("    - {}: {}", diagnostic.source, diagnostic.message);
                } else {
                    println!(
                        "    - {}: {} ({})",
                        diagnostic.source, diagnostic.message, diagnostic.url
                    );
                }
            }
        }

        match selected_index.and_then(|index| result.candidates.get(index)) {
            Some(best) => {
                summary.selected += 1;
                let candidate = &best.downloaded.candidate;
                println!(
                    "  Selected:    {}",
                    format!(
                        "{}x{} {:?} from {}",
                        candidate.width, candidate.height, candidate.format, candidate.source
                    )
                    .with(ORANGE)
                    .bold()
                );
                println!("  URL:         {}", best.reference.url.as_str().blue());

                if config.samples.sample_write {
                    match write_selected_sample(
                        &sample_dir,
                        &context.artist_credit,
                        &context.release_title,
                        candidate,
                        best.downloaded.path(),
                        config.output.preserve_file,
                    ) {
                        Ok(sample) => {
                            if sample.unchanged {
                                summary.samples_unchanged += 1;
                            } else {
                                summary.samples_written += 1;
                            }
                            println!(
                                "  Sample:      {}{}",
                                sample.path.display().to_string().cyan().bold(),
                                if sample.unchanged {
                                    " (UNCHANGED)".cyan().bold()
                                } else {
                                    "".reset()
                                }
                            );
                        }
                        Err(error) => {
                            summary.failed += 1;
                            println!(
                                "  {}",
                                format!("ERROR: selected sample failed: {error}")
                                    .red()
                                    .bold()
                            );
                            println!();
                            continue;
                        }
                    }
                }

                if matches!(candidate.source.as_str(), "local" | "webpstill") {
                    summary.unchanged += 1;
                    gui_events::emit(json!({
                        "event": "album_completed",
                        "album_path": album.path,
                        "destination": best.downloaded.path(),
                        "action": "KeptLocal",
                        "mode": format!("{:?}", config.mode).to_ascii_lowercase(),
                    }));
                    let _ = record_scan_completion(
                        config,
                        &mut completion_history,
                        album,
                        resolved_sources,
                        "local-kept",
                    );
                    println!("  Artwork:     {}", "KEPT LOCAL".cyan().bold());
                    println!();
                    continue;
                }

                let target_format =
                    match target_format_for_candidate(candidate.format, &format_order) {
                        Ok(target_format) => target_format,
                        Err(error) => {
                            summary.failed += 1;
                            println!("  {}", format!("ERROR: {error}").red().bold());
                            println!();
                            continue;
                        }
                    };

                let canonical_destination = match output_destination(
                    &album.path,
                    &config.output.file_name,
                    target_format,
                ) {
                    Ok(destination) => destination,
                    Err(error) => {
                        summary.failed += 1;
                        println!("  {}", format!("ERROR: {error}").red().bold());
                        println!();
                        continue;
                    }
                };

                match finalize_selected_with_preserve(
                    config.mode,
                    candidate,
                    best.downloaded.path(),
                    &canonical_destination,
                    &range,
                    target_format,
                    FinalizationOptions {
                        preserve_file: config.output.preserve_file,
                        explicit_manual_selection: manually_selected
                            || active_policy(&config.source_policies, &candidate.source).is_some()
                            || configured_candidate_policy(
                                candidate,
                                &project_configured_artwork(candidate, &range, &config.output),
                                &range,
                                config,
                            )
                            .status
                                == SourcePolicyStatus::Fallback
                            || result.best_index.is_none(),
                        output: &config.output,
                    },
                ) {
                    Ok((final_result, destination)) => {
                        if final_result.action == FinalArtworkAction::Installed {
                            match cleanup_replaced_static_covers(
                                &album.path,
                                &config.output.file_name,
                                &destination,
                                config.mode,
                                config.output.preserve_file,
                            ) {
                                Ok(removed) => {
                                    for removed_path in removed {
                                        println!(
                                            "  Local Cleanup: removed {}",
                                            removed_path.display()
                                        );
                                    }
                                }
                                Err(error) => {
                                    summary.failed += 1;
                                    println!("  {}", format!("ERROR: {error}").red().bold());
                                }
                            }
                        }
                        println!(
                            "  Destination: {}",
                            destination.display().to_string().yellow().bold()
                        );

                        if let Some(info) = final_result.info {
                            println!(
                                "  Final:       {}x{} {:?} resized={} converted={}",
                                info.width, info.height, info.format, info.resized, info.converted
                            );
                        }

                        match final_result.action {
                            FinalArtworkAction::Installed => {
                                summary.installed += 1;
                                println!("  Artwork:     {}", "INSTALLED".green().bold());
                            }
                            FinalArtworkAction::Unchanged => {
                                summary.unchanged += 1;
                                println!("  Artwork:     {}", "UNCHANGED".cyan().bold());
                            }
                            FinalArtworkAction::ReadOnly => {
                                summary.read_only += 1;
                                println!(
                                    "  Artwork:     {}",
                                    "READ-ONLY (would install)".yellow().bold()
                                );
                            }
                            FinalArtworkAction::NoSelection => {
                                println!("  Artwork:     {}", "skipped".yellow().bold());
                            }
                        }
                        gui_events::emit(json!({
                            "event": "album_completed",
                            "album_path": album.path,
                            "destination": destination,
                            "action": format!("{:?}", final_result.action),
                            "mode": format!("{:?}", config.mode).to_ascii_lowercase(),
                        }));
                        if final_result.action != FinalArtworkAction::NoSelection {
                            let outcome = if fallback_reason.is_some() {
                                if manually_selected
                                    && Some(selected_index.unwrap_or(usize::MAX)) == suggested_index
                                {
                                    "fallback-manual-suggested".to_string()
                                } else if manually_selected {
                                    "fallback-manual-number".to_string()
                                } else {
                                    "fallback-auto-selected".to_string()
                                }
                            } else {
                                format!("{:?}", final_result.action).to_ascii_lowercase()
                            };
                            let _ = record_scan_completion(
                                config,
                                &mut completion_history,
                                album,
                                resolved_sources,
                                &outcome,
                            );
                            // Python's chosen-source history is count-only and
                            // intentionally excludes every fallback choice.
                            if fallback_reason.is_none() {
                                let _ = record_source_selection(
                                    config,
                                    &mut source_history,
                                    candidate,
                                    &range,
                                    unix_now(),
                                    false,
                                );
                            }
                        }
                    }
                    Err(error) => {
                        summary.failed += 1;
                        println!(
                            "  {}",
                            format!("ERROR: final artwork failed: {error}").red().bold()
                        );
                    }
                }
            }
            None => println!("  Selected:    {}", "none".yellow().bold()),
        }

        println!();
    }

    println!(
        "{}",
        format!(
            "SPLINED SCAN LIBRARY {} SUMMARY",
            mode_label.to_ascii_uppercase()
        )
        .cyan()
        .bold()
    );
    println!("Albums:      {}", summary.albums);
    println!(
        "Resolved:    {}",
        summary.resolved.to_string().green().bold()
    );
    println!(
        "Unresolved:  {}",
        if summary.unresolved == 0 {
            summary.unresolved.to_string().green().bold()
        } else {
            summary.unresolved.to_string().yellow().bold()
        }
    );
    println!(
        "Failed:      {}",
        if summary.failed == 0 {
            summary.failed.to_string().green().bold()
        } else {
            summary.failed.to_string().red().bold()
        }
    );
    println!(
        "Selected:    {}",
        summary.selected.to_string().green().bold()
    );
    if config.samples.sample_write {
        println!(
            "Samples:     {}",
            (summary.samples_written + summary.samples_unchanged)
                .to_string()
                .green()
                .bold()
        );
    }
    match config.mode {
        Mode::Read => {
            println!(
                "Would Write: {}",
                summary.read_only.to_string().yellow().bold()
            );
            println!(
                "Unchanged:   {}",
                summary.unchanged.to_string().cyan().bold()
            );
            println!("Mutation:    {}", "disabled".green().bold());
        }
        Mode::Write => {
            println!(
                "Installed:   {}",
                summary.installed.to_string().green().bold()
            );
            println!(
                "Unchanged:   {}",
                summary.unchanged.to_string().cyan().bold()
            );
            println!("Mutation:    {}", "enabled".red().bold());
        }
    }

    gui_events::emit(json!({
        "event": "scan_completed",
        "albums": summary.albums,
        "postponed": summary.postponed,
        "resolved": summary.resolved,
        "unresolved": summary.unresolved,
        "failed": summary.failed,
        "selected": summary.selected,
        "installed": summary.installed,
        "unchanged": summary.unchanged,
        "read_only": summary.read_only,
    }));

    Ok(summary)
}

fn prepare_samples_dir(sample_dir: &Path) -> Result<(), String> {
    if sample_dir.file_name().and_then(|value| value.to_str()) != Some("samples") {
        return Err(format!(
            "Refusing to clean unexpected SPLINED samples path: {}",
            sample_dir.display()
        ));
    }

    if sample_dir.exists() {
        let metadata = fs::symlink_metadata(sample_dir).map_err(|error| {
            format!(
                "Unable to inspect SPLINED samples directory {}: {error}",
                sample_dir.display()
            )
        })?;

        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Refusing to clean symlinked SPLINED samples directory: {}",
                sample_dir.display()
            ));
        }

        if !metadata.is_dir() {
            return Err(format!(
                "SPLINED samples path exists but is not a directory: {}",
                sample_dir.display()
            ));
        }

        fs::remove_dir_all(sample_dir).map_err(|error| {
            format!(
                "Unable to clear SPLINED samples directory {}: {error}",
                sample_dir.display()
            )
        })?;
    }

    fs::create_dir_all(sample_dir).map_err(|error| {
        format!(
            "Unable to create SPLINED samples directory {}: {error}",
            sample_dir.display()
        )
    })
}

fn write_selected_sample(
    sample_dir: &Path,
    artist: &str,
    album: &str,
    candidate: &Candidate,
    source_path: &Path,
    preserve_file: bool,
) -> Result<SampleWriteResult, String> {
    let bytes = fs::read(source_path).map_err(|error| {
        format!(
            "Unable to read selected sample source {}: {error}",
            source_path.display()
        )
    })?;
    let file_name = format!(
        "{}.{}.sample.{}",
        sanitize_sample_component(artist),
        sanitize_sample_component(album),
        output_extension(candidate.format)
    );
    let canonical = sample_dir.join(file_name);
    let (destination, unchanged) = resolve_bytes_destination(&canonical, &bytes, preserve_file)?;

    if unchanged {
        return Ok(SampleWriteResult {
            path: destination,
            unchanged: true,
        });
    }

    let expected = candidate.clone();
    replace_binary_file(
        &destination,
        &bytes,
        "selected sample",
        move |staged_path| {
            let inspected = inspect_image(staged_path)?;
            if inspected.width != expected.width
                || inspected.height != expected.height
                || inspected.format != expected.format
            {
                return Err(format!(
                    "Selected sample validation failed: expected {}x{} {:?}, found {}x{} {:?}.",
                    expected.width,
                    expected.height,
                    expected.format,
                    inspected.width,
                    inspected.height,
                    inspected.format
                ));
            }
            Ok(())
        },
    )?;

    Ok(SampleWriteResult {
        path: destination,
        unchanged: false,
    })
}

fn resolve_bytes_destination(
    canonical: &Path,
    bytes: &[u8],
    preserve_file: bool,
) -> Result<(PathBuf, bool), String> {
    if !preserve_file {
        return Ok((canonical.to_path_buf(), false));
    }

    resolve_preserved_destination(canonical, |path| {
        let existing = fs::read(path)
            .map_err(|error| format!("Unable to read existing file {}: {error}", path.display()))?;
        Ok(existing == bytes)
    })
}

struct FinalizationOptions<'a> {
    preserve_file: bool,
    explicit_manual_selection: bool,
    output: &'a OutputConfig,
}

fn finalize_selected_with_preserve(
    mode: Mode,
    candidate: &Candidate,
    source_path: &Path,
    canonical_destination: &Path,
    range: &Range,
    target_format: StaticFormat,
    options: FinalizationOptions<'_>,
) -> Result<(FinalArtworkResult, PathBuf), String> {
    let prepared = prepare_configured_artwork(
        candidate,
        source_path,
        range,
        target_format,
        options.output,
        options.explicit_manual_selection,
    )?;

    if !options.preserve_file {
        let destination = canonical_destination.to_path_buf();

        if mode == Mode::Read {
            let action = if destination_matches_prepared(&destination, &prepared)? {
                FinalArtworkAction::Unchanged
            } else {
                FinalArtworkAction::ReadOnly
            };
            return Ok((
                FinalArtworkResult {
                    action,
                    info: Some(prepared.info),
                },
                destination,
            ));
        }

        install_prepared_artwork(&destination, &prepared)?;
        return Ok((
            FinalArtworkResult {
                action: FinalArtworkAction::Installed,
                info: Some(prepared.info),
            },
            destination,
        ));
    }

    let (destination, unchanged) = resolve_prepared_destination(canonical_destination, &prepared)?;
    let action = if unchanged {
        FinalArtworkAction::Unchanged
    } else if mode == Mode::Read {
        FinalArtworkAction::ReadOnly
    } else {
        install_prepared_artwork(&destination, &prepared)?;
        FinalArtworkAction::Installed
    };

    Ok((
        FinalArtworkResult {
            action,
            info: Some(prepared.info),
        },
        destination,
    ))
}

fn resolve_prepared_destination(
    canonical: &Path,
    prepared: &PreparedArtwork,
) -> Result<(PathBuf, bool), String> {
    resolve_preserved_destination(canonical, |path| {
        destination_matches_prepared(path, prepared)
    })
}

fn resolve_preserved_destination<F>(
    canonical: &Path,
    mut matches_existing: F,
) -> Result<(PathBuf, bool), String>
where
    F: FnMut(&Path) -> Result<bool, String>,
{
    if !canonical.exists() {
        return Ok((canonical.to_path_buf(), false));
    }

    if matches_existing(canonical)? {
        return Ok((canonical.to_path_buf(), true));
    }

    for number in 2..=1_000_000usize {
        let candidate = numbered_destination(canonical, number)?;
        if !candidate.exists() {
            return Ok((candidate, false));
        }
        if matches_existing(&candidate)? {
            return Ok((candidate, true));
        }
    }

    Err(format!(
        "Unable to find an available preserved artwork filename for {}.",
        canonical.display()
    ))
}

fn numbered_destination(canonical: &Path, number: usize) -> Result<PathBuf, String> {
    let parent = canonical.parent().ok_or_else(|| {
        format!(
            "Unable to determine parent directory for {}.",
            canonical.display()
        )
    })?;
    let stem = canonical
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("Invalid destination filename: {}", canonical.display()))?;
    let extension = canonical
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("Destination has no extension: {}", canonical.display()))?;

    Ok(parent.join(format!("{stem}-({number}).{extension}")))
}

fn sanitize_sample_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());

    for character in value.trim().chars() {
        if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        {
            sanitized.push('_');
        } else {
            sanitized.push(character);
        }
    }

    let sanitized = sanitized.trim().trim_end_matches([' ', '.']).to_string();

    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn configured_output_formats(file_formats: &[String]) -> Result<Vec<StaticFormat>, String> {
    let mut formats = Vec::new();

    for configured in file_formats {
        let format = match configured.as_str() {
            "jpeg" => StaticFormat::Jpeg,
            "png" => StaticFormat::Png,
            "webp" => StaticFormat::Webp,
            other => return Err(format!("Unsupported SPLINED static output format: {other}")),
        };

        if !formats.contains(&format) {
            formats.push(format);
        }
    }

    if formats.is_empty() {
        return Err("SPLINED output file_formats cannot be empty.".to_string());
    }

    Ok(formats)
}

fn configured_candidate_policy(
    candidate: &Candidate,
    projected: &crate::final_artwork::ProjectedArtwork,
    range: &Range,
    config: &Config,
) -> SourcePolicyDecision {
    match active_policy(&config.source_policies, &candidate.source) {
        Some(policy) => source_override_decision(policy, candidate.width, candidate.height, range),
        None => global_range_decision(projected.width, projected.height, range),
    }
}

fn candidate_visible_for_review(candidate: &Candidate, range: &Range, config: &Config) -> bool {
    match active_policy(&config.source_policies, &candidate.source) {
        Some(_) => {
            let projected = project_configured_artwork(candidate, range, &config.output);
            configured_candidate_policy(candidate, &projected, range, config).status
                != SourcePolicyStatus::Reject
        }
        // Source Override = No retains the existing global/manual-review display.
        None => true,
    }
}

fn target_format_for_candidate(
    candidate_format: StaticFormat,
    configured_formats: &[StaticFormat],
) -> Result<StaticFormat, String> {
    if configured_formats.contains(&candidate_format) {
        return Ok(candidate_format);
    }

    configured_formats
        .first()
        .copied()
        .ok_or_else(|| "SPLINED output file_formats cannot be empty.".to_string())
}

fn configured_output_label(
    file_name: &str,
    configured_formats: &[StaticFormat],
) -> Result<String, String> {
    validate_output_file_name(file_name)?;

    if configured_formats.is_empty() {
        return Err("SPLINED output file_formats cannot be empty.".to_string());
    }

    Ok(configured_formats
        .iter()
        .map(|format| format!("{}.{}", file_name.trim(), output_extension(*format)))
        .collect::<Vec<_>>()
        .join(" / "))
}

fn output_extension(format: StaticFormat) -> &'static str {
    match format {
        StaticFormat::Jpeg => "jpg",
        StaticFormat::Png => "png",
        StaticFormat::Webp => "webp",
    }
}

fn validate_output_file_name(file_name: &str) -> Result<(), String> {
    let file_name = file_name.trim();

    if file_name.is_empty() {
        return Err("SPLINED output file_name cannot be empty.".to_string());
    }

    if file_name == "." || file_name == ".." || file_name.contains('/') || file_name.contains('\\')
    {
        return Err(
            "SPLINED output file_name must be a single filename stem without directories."
                .to_string(),
        );
    }

    if Path::new(file_name).extension().is_some() {
        return Err(
            "SPLINED output file_name must not include an extension; use output.file_formats instead."
                .to_string(),
        );
    }

    Ok(())
}

fn output_destination(
    album_dir: &Path,
    file_name: &str,
    target_format: StaticFormat,
) -> Result<PathBuf, String> {
    validate_output_file_name(file_name)?;

    Ok(album_dir.join(format!(
        "{}.{}",
        file_name.trim(),
        output_extension(target_format)
    )))
}

fn print_tagged_release_result(
    tagged_album: Option<&str>,
    release_title: &str,
    release_mbid: &str,
    title_match: Option<bool>,
) {
    match title_match {
        Some(true) => println!(
            "  Tagged / Release [{}] {} / {}",
            "MATCHED".green().bold(),
            release_title.cyan().bold(),
            release_mbid.magenta().bold()
        ),
        Some(false) => println!(
            "  Tagged / Release [{}] {} -> {} / {}",
            "MIS-MATCHED".red().bold(),
            tagged_album.unwrap_or("unavailable").with(ORANGE).bold(),
            release_title.cyan().bold(),
            release_mbid.magenta().bold()
        ),
        None => println!(
            "  Tagged / Release [{}] {} / {}",
            "UNVERIFIED".yellow().bold(),
            release_title.cyan().bold(),
            release_mbid.magenta().bold()
        ),
    }
}

fn print_unresolved_tagged_release(tagged_album: &Option<String>, audit: &TaggedAlbumIdAudit) {
    let evidence = if audit.valid.len() > 1 {
        format!("{} valid MBIDs", audit.valid.len())
    } else {
        "no valid MBID".to_string()
    };

    println!(
        "  Tagged / Release [{}] {} / {}",
        "UNRESOLVED".red().bold(),
        tagged_album
            .as_deref()
            .unwrap_or("unavailable")
            .with(ORANGE)
            .bold(),
        evidence.magenta().bold()
    );
}

async fn print_album_id_diagnostics(
    musicbrainz: &MusicBrainzClient,
    tracks: &[LocalTrackEvidence],
    audit: &TaggedAlbumIdAudit,
) {
    println!("  {}", "MBID evidence:".cyan().bold());

    for evidence in &audit.valid {
        println!(
            "    - {}/{} tracks: {}",
            evidence.track_paths.len(),
            tracks.len(),
            evidence.release_mbid.as_str().cyan().bold()
        );

        if audit.valid.len() > 1 && musicbrainz.is_enabled() {
            match musicbrainz.lookup_release(&evidence.release_mbid).await {
                Ok(release) => println!(
                    "      MB release: {} — {}",
                    release.artist_credit, release.title
                ),
                Err(error) => println!(
                    "      MB release: {}",
                    format!("lookup failed: {error}").yellow()
                ),
            }
        }

        println!(
            "      Files: {}",
            compact_track_names(&evidence.track_paths).dark_grey()
        );
    }

    if !audit.missing.is_empty() {
        println!(
            "    - Missing/blank: {}/{} tracks",
            audit.missing.len(),
            tracks.len()
        );
        println!(
            "      Files: {}",
            compact_track_names(&audit.missing).dark_grey()
        );
    }

    if !audit.invalid.is_empty() {
        println!(
            "    - {}",
            format!(
                "Invalid MBID syntax: {}/{} tracks",
                audit.invalid.len(),
                tracks.len()
            )
            .yellow()
            .bold()
        );
        for (path, value) in &audit.invalid {
            println!(
                "      {}: {}",
                track_name(path).dark_grey(),
                value.as_str().yellow()
            );
        }
    }
}

fn compact_track_names(paths: &[PathBuf]) -> String {
    const MAX_NAMES: usize = 5;

    let mut names: Vec<String> = paths
        .iter()
        .take(MAX_NAMES)
        .map(|path| track_name(path))
        .collect();

    if paths.len() > MAX_NAMES {
        names.push(format!("+{} more", paths.len() - MAX_NAMES));
    }

    names.join(", ")
}

fn track_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn fallback_provider_context(
    tracks: &[LocalTrackEvidence],
    tagged_album: Option<&str>,
) -> Option<ProviderContext> {
    let release_title = std::env::var("SPLINED_FALLBACK_ALBUM")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            tagged_album
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    tracks
                        .iter()
                        .filter_map(|track| track.album.as_deref())
                        .map(str::trim)
                        .find(|value| !value.is_empty())
                        .map(str::to_string)
                })
        })?;
    let artist_credit = std::env::var("SPLINED_FALLBACK_ARTIST")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            tracks
                .iter()
                .filter_map(|track| track.album_artist.as_deref())
                .map(str::trim)
                .find(|value| !value.is_empty())
                .or_else(|| {
                    tracks
                        .iter()
                        .map(|track| track.artist.trim())
                        .find(|value| !value.is_empty())
                })
                .map(str::to_string)
        })?;
    Some(ProviderContext {
        artist_credit,
        release_title,
        release_group_mbid: None,
        release_group_title: None,
    })
}

fn fallback_reason_from_error(error: &str) -> String {
    let upper = error.to_ascii_uppercase();
    if error.contains("503") {
        "TEMPORARY MB FAILURE [503]".to_string()
    } else if error.contains("429") {
        "TEMPORARY MB FAILURE [429]".to_string()
    } else if upper.contains("TIMEOUT") || upper.contains("TIMED OUT") {
        "TEMPORARY MB FAILURE [timeout]".to_string()
    } else if error.contains("401") || upper.contains("AUTHENTICATION") {
        "MB AUTH FAILURE".to_string()
    } else if error.contains("404") || upper.contains("NOT FOUND") {
        "MB RELEASE NOT FOUND".to_string()
    } else {
        "TEMPORARY MB FAILURE".to_string()
    }
}

fn fallback_format_priority(candidate: &Candidate, format_order: &[StaticFormat]) -> usize {
    target_format_for_candidate(candidate.format, format_order)
        .ok()
        .and_then(|target| format_order.iter().position(|format| *format == target))
        .unwrap_or(usize::MAX)
}

fn compare_fallback_candidates(
    left: &PipelineCandidate,
    right: &PipelineCandidate,
    range: &Range,
    format_order: &[StaticFormat],
    output: &OutputConfig,
) -> Ordering {
    let left_candidate = &left.downloaded.candidate;
    let right_candidate = &right.downloaded.candidate;
    let left_projected = project_configured_artwork(left_candidate, range, output);
    let right_projected = project_configured_artwork(right_candidate, range, output);
    right_projected
        .width
        .min(right_projected.height)
        .cmp(&left_projected.width.min(left_projected.height))
        .then_with(|| (!left_candidate.is_square()).cmp(&(!right_candidate.is_square())))
        .then_with(|| (!left.reference.approved).cmp(&(!right.reference.approved)))
        .then_with(|| {
            left_candidate
                .source_priority
                .cmp(&right_candidate.source_priority)
        })
        .then_with(|| {
            fallback_format_priority(left_candidate, format_order)
                .cmp(&fallback_format_priority(right_candidate, format_order))
        })
        .then_with(|| left.reference.id.cmp(&right.reference.id))
}

fn compare_fallback_suggestions(
    left: &PipelineCandidate,
    right: &PipelineCandidate,
    range: &Range,
    format_order: &[StaticFormat],
    output: &OutputConfig,
) -> Ordering {
    let left_candidate = &left.downloaded.candidate;
    let right_candidate = &right.downloaded.candidate;
    let left_projected = project_configured_artwork(left_candidate, range, output);
    let right_projected = project_configured_artwork(right_candidate, range, output);
    left_projected
        .width
        .min(left_projected.height)
        .abs_diff(range.ideal)
        .cmp(
            &right_projected
                .width
                .min(right_projected.height)
                .abs_diff(range.ideal),
        )
        .then_with(|| (!left_candidate.is_square()).cmp(&(!right_candidate.is_square())))
        .then_with(|| (!left.reference.approved).cmp(&(!right.reference.approved)))
        .then_with(|| {
            left_candidate
                .source_priority
                .cmp(&right_candidate.source_priority)
        })
        .then_with(|| {
            fallback_format_priority(left_candidate, format_order)
                .cmp(&fallback_format_priority(right_candidate, format_order))
        })
        .then_with(|| {
            right_projected
                .width
                .min(right_projected.height)
                .cmp(&left_projected.width.min(left_projected.height))
        })
        .then_with(|| left.reference.id.cmp(&right.reference.id))
}

fn projected_range_class(short_side: u32, range: &Range) -> &'static str {
    use crate::range::RangeClass;

    match range.classify(short_side) {
        RangeClass::BelowMinimum => "below-minimum",
        RangeClass::LowerRange => "lower-range",
        RangeClass::Ideal => "ideal",
        RangeClass::UpperRange => "upper-range",
        RangeClass::Ladder => "ladder",
        RangeClass::AboveLadder => "above-ladder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn configured_output_formats_preserve_configured_order() {
        assert_eq!(
            configured_output_formats(&["jpeg".to_string(), "png".to_string()]),
            Ok(vec![StaticFormat::Jpeg, StaticFormat::Png])
        );
    }

    #[test]
    fn selected_candidate_keeps_its_configured_format() {
        let configured = vec![StaticFormat::Jpeg, StaticFormat::Png];
        assert_eq!(
            target_format_for_candidate(StaticFormat::Png, &configured),
            Ok(StaticFormat::Png)
        );
    }

    #[test]
    fn unconfigured_candidate_format_falls_back_to_first_output_format() {
        let configured = vec![StaticFormat::Jpeg, StaticFormat::Png];
        assert_eq!(
            target_format_for_candidate(StaticFormat::Webp, &configured),
            Ok(StaticFormat::Jpeg)
        );
    }

    #[test]
    fn output_destination_rejects_directory_escape() {
        assert!(output_destination(Path::new("album"), "../cover", StaticFormat::Jpeg).is_err());
        assert!(
            output_destination(Path::new("album"), "nested/cover", StaticFormat::Jpeg).is_err()
        );
    }

    #[test]
    fn preserved_destination_reuses_identical_and_numbers_different_files() {
        let dir = TempDir::new().unwrap();
        let canonical = dir.path().join("cover.jpg");
        fs::write(&canonical, b"old").unwrap();
        fs::write(dir.path().join("cover-(2).jpg"), b"new").unwrap();

        let (path, unchanged) = resolve_bytes_destination(&canonical, b"new", true).unwrap();
        assert_eq!(path, dir.path().join("cover-(2).jpg"));
        assert!(unchanged);
    }

    #[test]
    fn samples_directory_is_cleared_without_touching_cache_parent() {
        let dir = TempDir::new().unwrap();
        let samples = dir.path().join("samples");
        fs::create_dir_all(&samples).unwrap();
        fs::write(samples.join("old.sample.jpg"), b"old").unwrap();
        fs::write(dir.path().join("unrelated.cache"), b"keep").unwrap();

        prepare_samples_dir(&samples).unwrap();
        assert_eq!(fs::read_dir(&samples).unwrap().count(), 0);
        assert!(dir.path().join("unrelated.cache").exists());
    }

    #[test]
    fn sample_name_sanitizes_unsafe_characters() {
        assert_eq!(sanitize_sample_component("Artist/Live?"), "Artist_Live_");
        assert_eq!(sanitize_sample_component(" Album. "), "Album");
    }

    #[test]
    fn projected_range_labels_match_configured_range() {
        let range = Range::default();
        assert_eq!(projected_range_class(1199, &range), "below-minimum");
        assert_eq!(projected_range_class(1800, &range), "ideal");
        assert_eq!(projected_range_class(3601, &range), "above-ladder");
    }

    #[test]
    fn active_source_override_hides_rejected_candidates_from_review() {
        let range = Range::default();
        let mut config = Config::default();
        config.source_policies.insert(
            "discogs".to_string(),
            crate::source_policy::SourcePolicyConfig {
                source_override: true,
                minimum_range_type: crate::source_policy::MinimumRangeType::Ideal,
                ..crate::source_policy::SourcePolicyConfig::default()
            },
        );
        let rejected_discogs = Candidate {
            source: "discogs".to_string(),
            width: 600,
            height: 600,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        let accepted_discogs = Candidate {
            width: 1800,
            height: 1800,
            ..rejected_discogs.clone()
        };
        let legacy_global_source = Candidate {
            source: "itunes".to_string(),
            ..rejected_discogs.clone()
        };

        assert!(!candidate_visible_for_review(
            &rejected_discogs,
            &range,
            &config
        ));
        assert!(candidate_visible_for_review(
            &accepted_discogs,
            &range,
            &config
        ));
        assert!(candidate_visible_for_review(
            &legacy_global_source,
            &range,
            &config
        ));
    }
}

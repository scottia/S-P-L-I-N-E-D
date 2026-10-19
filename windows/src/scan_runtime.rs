use crate::candidate::{Candidate, StaticFormat};
use crate::compilation::{
    CompilationMatchCache, MusicBrainzMatch, extract_mbids, manual_album_eligible, track_authority,
};
use crate::config::{Config, Mode, OutputConfig, configured_samples_dir};
use crate::download::DownloadedCandidate;
use crate::embedded_artwork::{
    embedded_front_matches, replace_embedded_front, replace_musicbrainz_identity,
};
use crate::final_artwork::{
    FinalArtworkAction, FinalArtworkResult, PreparedArtwork, PreparedArtworkInfo,
    assess_artwork_quality, destination_matches_prepared, install_prepared_artwork,
    prepare_configured_artwork, prepare_embedded_thumbnail, prepare_existing_cover_edit,
    prepare_selected_artwork_edit, project_configured_artwork,
};
use crate::gui_events;
use crate::history::{AlbumHistoryState, album_history_status, load_completion_history, unix_now};
use crate::inspect::inspect_image;
use crate::local_artwork::{
    LocalPreflightAction, cleanup_competing_static, cleanup_replaced_static_covers,
    embedded_candidate, embedded_preview_cache_dir, inspect_local_preflight,
};
use crate::media_database::{
    CompilationArtworkApplication, RuntimeArtworkMaterial, cached_artist_musicbrainz_identity,
    cached_track_musicbrainz_identity, clear_album_bypass_from_runtime, compilation_resume_track,
    completed_compilation_track_paths, find_existing_compilation_artwork,
    find_local_compilation_artwork, find_local_compilation_artworks,
    persist_track_musicbrainz_identity, record_album_outcome_from_runtime,
    record_compilation_artwork_application, record_compilation_progress,
    record_compilation_resume_track,
};
use crate::musicbrainz::MusicBrainzClient;
use crate::pipeline::{
    PipelineCandidate, PipelineResult, RegistryPipelineOptions, candidate_summary,
    prepare_persistent_cache_dir, run_registry_pipeline_with_cache_dir,
};
use crate::range::{Range, RangeClass};
use crate::safe_write::replace_binary_file;
use crate::scan::{AlbumDirectory, inventory_album_directories};
use crate::scan_musicbrainz::{
    AlbumReleaseDecision, LocalTrackEvidence, TaggedAlbumIdAudit, compilation_context,
    resolve_album_release_with_fallback, tagged_album_id_audit, tagged_album_title,
    tagged_album_title_matches_release,
};
use crate::scan_tags::read_album_track_evidence;
use crate::source::{ArtworkQuery, ArtworkReference, ProviderContext, ProviderRegistry};
use crate::source_history::{load_source_history, record_source_selection};
use crate::source_policy::{
    SourcePolicyDecision, SourcePolicyStatus, active_policy, best_candidate_index,
    global_range_decision, source_override_decision,
};
use crate::strict_source_policy::{StrictContentDecision, apply as apply_strict_source_policy};
use crossterm::style::{Color, Stylize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    let cache_dir = Path::new(&config.scan.temporary_cache_dir);
    let sample_dir = configured_samples_dir(config);

    prepare_persistent_cache_dir(cache_dir)?;
    let _run_cache_cleanup = RunCacheCleanup::new(cache_dir);
    if config.samples.sample_write {
        prepare_samples_dir(&sample_dir)?;
    }

    let inventory = inventory_album_directories(root, &config.library.ignored_subs)?;
    let musicbrainz = MusicBrainzClient::new(&config.musicbrainz)?;
    let discogs_credential_file = Path::new(&config.credentials.credential_dir)
        .join("discogs.json")
        .to_string_lossy()
        .into_owned();
    let navidrome_credential_file = Path::new(&config.credentials.credential_dir)
        .join("navidrome.json")
        .to_string_lossy()
        .into_owned();

    let registry = ProviderRegistry::from_source_order_with_credentials(
        resolved_sources,
        &config.fanarttv.credential_file,
        &config.lastfm.credential_file,
        &discogs_credential_file,
        &navidrome_credential_file,
        Duration::from_secs_f64(musicbrainz.recording_timeout_seconds()),
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
    let completion_history = load_completion_history(config);
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
        "Run Cache:   {}",
        cache_dir.display().to_string().cyan().bold()
    );
    println!(
        "State DB:    {}",
        crate::media_database::database_path(&config.scan.cache_dir)
            .display()
            .to_string()
            .cyan()
            .bold()
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

    let mut compilation_match_cache = CompilationMatchCache::default();
    'album_loop: for (index, album) in inventory.albums.iter().enumerate() {
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
            && !gui_events::musicbrainz_requested()
        {
            gui_events::emit(json!({
                "event": "album_skipped",
                "album_path": album.path,
                "reason": "bypassed",
            }));
            continue;
        }
        if history_status.state == AlbumHistoryState::TimeoutActive
            && !gui_events::musicbrainz_requested()
        {
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

        if manual_album_eligible(&tracks) {
            let compilation_context = CompilationRunContext {
                config,
                resolved_sources,
                registry: &registry,
                musicbrainz: &musicbrainz,
                range: &range,
                format_order: &format_order,
                cache_dir,
            };
            match run_compilation_album(
                &compilation_context,
                &mut compilation_match_cache,
                album,
                &tracks,
            )
            .await
            {
                Ok(result) => {
                    summary.selected += result.selected;
                    summary.installed += result.installed;
                    summary.read_only += result.read_only;
                    summary.unresolved += result.unresolved;
                    if result.failed {
                        summary.failed += 1;
                    }
                }
                Err(error) => {
                    summary.failed += 1;
                    println!(
                        "  {}",
                        format!("ERROR: compilation track-art workflow failed: {error}")
                            .red()
                            .bold()
                    );
                    gui_events::emit(
                        json!({ "event": "album_error", "album_path": album.path, "message": error }),
                    );
                }
            }
            println!();
            continue;
        }

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

        let review_existing_cover = should_offer_existing_cover_for_review(
            local_preflight.action,
            gui_events::review_required(),
            gui_events::auto_ideal_enabled(),
        );

        if matches!(
            local_preflight.action,
            LocalPreflightAction::LocalIdeal | LocalPreflightAction::EmbeddedIdeal
        ) && !review_existing_cover
        {
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
                    apply_edit_profile: false,
                    edit_existing_cover: false,
                    output: &config.output,
                },
            ) {
                Ok((final_result, destination)) => {
                    let post_cover_started = Instant::now();
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
                    record_runtime_completion(
                        config,
                        album,
                        action,
                        Some("local"),
                        runtime_artwork_material(&destination, final_result.info),
                        post_cover_started,
                        post_cover_started,
                    )?;
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

        let local_comparison_candidate =
            if local_preflight.action == LocalPreflightAction::Compare || review_existing_cover {
                local_preflight.candidate.take()
            } else {
                None
            };
        let existing_album_artwork = match normal_album_existing_artwork(
            config,
            album,
            &tracks,
            &mbid_audit,
            local_comparison_candidate.as_ref(),
        ) {
            Ok(found) => found,
            Err(error) => {
                gui_events::emit(json!({"event": "activity", "category": "existing-artwork",
                    "state": "warning", "message": format!("Existing Artist Album lookup unavailable: {error}")}));
                None
            }
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
                        let apple_collection_ids = release
                            .external_urls
                            .iter()
                            .filter_map(|url| {
                                crate::source::itunes::apple_collection_id_from_url(url)
                            })
                            .collect::<Vec<_>>();
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
                                apple_collection_ids,
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

        let Some((query, mut context)) = query_and_context else {
            summary.unresolved += 1;
            gui_events::emit(json!({
                "event": "album_error",
                "album_path": album.path,
                "message": "SPLINED could not establish either tagged or fallback provider authority."
            }));
            continue;
        };
        let fallback_sources: Vec<String> = resolved_sources
            .iter()
            .filter(|source| {
                source.as_str() != "fanarttv"
                    && (source.as_str() != "musicbrainz"
                        || context.release_group_mbid.is_some()
                        || !query.release_mbid.trim().is_empty())
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
                &navidrome_credential_file,
                Duration::from_secs_f64(musicbrainz.recording_timeout_seconds()),
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

        for timing in &result.provider_timings {
            gui_events::emit(json!({
                "event": "provider_timing",
                "album_path": album.path,
                "source": timing.source,
                "status": timing.status(),
                "discovery_ms": timing.discovery_ms,
                "download_ms": timing.download_ms,
                "references": timing.references,
                "candidates": timing.candidates,
                "errors": timing.errors,
                "error": timing.error,
            }));
        }

        if let Some(local) = local_comparison_candidate.filter(|local| {
            existing_album_artwork
                .as_ref()
                .is_none_or(|existing| !local.downloaded.path().eq(existing.1.as_path()))
        }) {
            if let Some(best_index) = result.best_index.as_mut() {
                *best_index += 1;
            }
            result.candidates.insert(0, local);
        }
        if let Some(existing) = existing_album_artwork.as_ref()
            && let Ok(mut existing_pipeline) =
                existing_compilation_pipeline(&existing.1, &existing.0, existing.2)
        {
            if let Some(best_index) = result.best_index.as_mut() {
                *best_index += 1;
            }
            result
                .candidates
                .insert(0, existing_pipeline.candidates.remove(0));
        }

        apply_strict_source_policy(&mut result.candidates, &config.source_policies);
        for item in result
            .candidates
            .iter()
            .filter(|item| item.strict.status != "not-applicable")
        {
            gui_events::emit(json!({
                "event": "activity",
                "category": "strict-policy",
                "state": item.strict.status,
                "source": item.downloaded.candidate.source,
                "message": format!(
                    "{} strict check: {} · {}",
                    item.downloaded.candidate.source,
                    item.strict.status,
                    item.strict.reason
                ),
            }));
        }

        if config.samples.sample_write {
            match append_saved_sample_candidates(
                &mut result,
                &sample_dir,
                &context.artist_credit,
                &context.release_title,
            ) {
                Ok(count) if count > 0 => gui_events::emit(json!({
                    "event": "activity",
                    "category": "samples",
                    "state": "ready",
                    "source": "samples",
                    "message": format!("Loaded {count} saved sample candidate(s) for this album."),
                })),
                Ok(_) => {}
                Err(error) => gui_events::emit(json!({
                    "event": "activity",
                    "category": "samples",
                    "state": "warning",
                    "source": "samples",
                    "message": error,
                })),
            }
        }

        let automatic_index = result
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let candidate = &item.downloaded.candidate;
                if !item.strict.preferred_eligible {
                    return None;
                }
                if matches!(
                    candidate.source.as_str(),
                    "local" | "webpstill" | "existing"
                ) {
                    return None;
                }
                let projected = project_configured_artwork(candidate, &range, &config.output);
                if !candidate_meets_upscale_limit(candidate, &range, config) {
                    return None;
                }
                if !candidate_quality_eligible_for_preferred(item, &range, config) {
                    return None;
                }
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
        let automatic_ideal_index = automatic_index.filter(|index| {
            let item = &result.candidates[*index];
            item.strict.auto_eligible
                && candidate_is_auto_ideal(&item.downloaded.candidate, &range, config)
        });
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
                    result.candidates[*index].strict.preferred_eligible
                        && candidate_meets_upscale_limit(
                            &result.candidates[*index].downloaded.candidate,
                            &range,
                            config,
                        )
                        && candidate_quality_eligible_for_preferred(
                            &result.candidates[*index],
                            &range,
                            config,
                        )
                        && !matches!(
                            result.candidates[*index]
                                .downloaded
                                .candidate
                                .source
                                .as_str(),
                            "local" | "webpstill" | "existing"
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
            if gui_events::auto_ideal_enabled() {
                automatic_ideal_index
            } else {
                None
            }
        } else {
            automatic_index
        };
        result.best_index = suggested_index;
        let mut manually_selected = false;
        let mut explicit_write = false;
        let mut selected_output = config.output.clone();
        let mut apply_edit_profile = false;
        let mut edit_existing_cover = false;
        let mut embed_thumbnails = false;

        emit_normal_source_results(NormalSourceResultsEvent {
            album,
            result: &result,
            display_indices: &display_indices,
            suggested_index,
            hidden_by_source_policy,
            fallback_reason: fallback_reason.as_deref(),
            musicbrainz_retry_available,
            musicbrainz_matches_available: musicbrainz.is_enabled() && !tracks.is_empty(),
            range: &range,
            config,
        });

        if selected_index.is_none()
            && (!result.candidates.is_empty()
                || fallback_reason.is_some()
                || gui_events::musicbrainz_requested())
            && gui_events::decisions_available()
        {
            gui_events::emit(json!({
                "event": "decision_required",
                "album_path": album.path,
                "reason": if fallback_reason.is_some() { "fallback" } else if gui_events::auto_ideal_enabled() { "auto-no-ideal" } else if gui_events::review_required() { "review" } else { "outside-range" },
                "fallback_reason": fallback_reason,
                "suggested_index": suggested_index.map(|value| value + 1),
                "allow_bypass": true,
                "musicbrainz_retry_available": musicbrainz_retry_available,
                "musicbrainz_matches_available": musicbrainz.is_enabled() && !tracks.is_empty(),
            }));
            if !gui_events::enabled() {
                print!("  Choose a candidate number or b to bypass: ");
                let _ = io::stdout().flush();
            }
            'candidate_review: loop {
                match gui_events::wait_for_candidate_decision() {
                    Ok(gui_events::CandidateDecision::Use {
                        index,
                        upscale,
                        explicit_write: write,
                    }) if index < result.candidates.len() => {
                        embed_thumbnails =
                            upscale.as_ref().is_some_and(|value| value.embed_thumbnails);
                        if let Some(upscale) = upscale {
                            if let Err(error) =
                                apply_upscale_overrides(&mut selected_output, &upscale)
                            {
                                println!("  {}", format!("ERROR: {error}").red().bold());
                                continue 'candidate_review;
                            }
                            apply_edit_profile =
                                !upscale.preserve_source_resolution && upscale.apply_edit_profile;
                            edit_existing_cover =
                                !upscale.preserve_source_resolution && upscale.edit_existing_cover;
                        }
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
                            continue 'album_loop;
                        }
                        selected_index = Some(index);
                        manually_selected = true;
                        explicit_write = write;
                        break 'candidate_review;
                    }
                    Ok(gui_events::CandidateDecision::Use { index, .. }) => {
                        summary.failed += 1;
                        println!("  ERROR: candidate {} does not exist.", index + 1);
                        continue 'album_loop;
                    }
                    Ok(gui_events::CandidateDecision::Skip) => {
                        gui_events::emit(json!({
                            "event": "album_skipped",
                            "album_path": album.path,
                            "reason": "operator",
                        }));
                        println!("  Artwork:     {}", "SKIPPED".yellow().bold());
                        println!();
                        continue 'album_loop;
                    }
                    Ok(gui_events::CandidateDecision::Bypass) => {
                        let post_cover_started = Instant::now();
                        let outcome = if fallback_reason.is_some() {
                            "fallback-bypassed"
                        } else {
                            "normal-out-of-range-bypassed"
                        };
                        record_runtime_completion(
                            config,
                            album,
                            outcome,
                            None,
                            None,
                            post_cover_started,
                            post_cover_started,
                        )?;
                        gui_events::emit(json!({
                            "event": "album_completed",
                            "album_path": album.path,
                            "destination": "",
                            "action": "Bypassed",
                            "mode": format!("{:?}", config.mode).to_ascii_lowercase(),
                        }));
                        println!("  Artwork:     {}", "BYPASSED".yellow().bold());
                        println!();
                        continue 'album_loop;
                    }
                    Ok(gui_events::CandidateDecision::ClearBypass) => {
                        clear_runtime_bypass(config, album)?;
                        continue 'candidate_review;
                    }
                    Ok(gui_events::CandidateDecision::RetryMusicBrainz) => {
                        match run_normal_musicbrainz_browser(
                            &CompilationRunContext {
                                config,
                                resolved_sources,
                                registry: &registry,
                                musicbrainz: &musicbrainz,
                                range: &range,
                                format_order: &format_order,
                                cache_dir,
                            },
                            &mut compilation_match_cache,
                            album,
                            &tracks[0],
                            existing_album_artwork.as_ref(),
                        )
                        .await?
                        {
                            NormalBrowserOutcome::Use {
                                pipeline,
                                selected,
                                provider_context,
                                explicit_write: write,
                                upscale,
                            } => {
                                result = *pipeline;
                                selected_index = Some(selected);
                                context = *provider_context;
                                fallback_reason = None;
                                manually_selected = true;
                                explicit_write = write;
                                if let Some(upscale) = upscale {
                                    embed_thumbnails = upscale.embed_thumbnails;
                                    apply_upscale_overrides(&mut selected_output, &upscale)?;
                                    apply_edit_profile = !upscale.preserve_source_resolution
                                        && upscale.apply_edit_profile;
                                    edit_existing_cover = !upscale.preserve_source_resolution
                                        && upscale.edit_existing_cover;
                                }
                                break 'candidate_review;
                            }
                            NormalBrowserOutcome::ReturnToSources => {
                                emit_normal_source_results(NormalSourceResultsEvent {
                                    album,
                                    result: &result,
                                    display_indices: &display_indices,
                                    suggested_index,
                                    hidden_by_source_policy,
                                    fallback_reason: fallback_reason.as_deref(),
                                    musicbrainz_retry_available,
                                    musicbrainz_matches_available: musicbrainz.is_enabled()
                                        && !tracks.is_empty(),
                                    range: &range,
                                    config,
                                });
                                gui_events::emit(
                                    json!({ "event": "source_results_restored", "album_path": album.path }),
                                );
                                continue 'candidate_review;
                            }
                            NormalBrowserOutcome::Skip => {
                                gui_events::emit(json!({
                                    "event": "album_skipped",
                                    "album_path": album.path,
                                    "reason": "operator",
                                }));
                                continue 'album_loop;
                            }
                            NormalBrowserOutcome::Bypass => {
                                let post_cover_started = Instant::now();
                                record_runtime_completion(
                                    config,
                                    album,
                                    "normal-out-of-range-bypassed",
                                    None,
                                    None,
                                    post_cover_started,
                                    post_cover_started,
                                )?;
                                gui_events::emit(
                                    json!({ "event": "album_completed", "album_path": album.path,
                                "destination": "", "action": "Bypassed",
                                "mode": format!("{:?}", config.mode).to_ascii_lowercase() }),
                                );
                                continue 'album_loop;
                            }
                        }
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
                        continue 'album_loop;
                    }
                    Ok(gui_events::CandidateDecision::Retry { .. }) => {
                        summary.failed += 1;
                        println!("  ERROR: fallback retry is only available in fallback mode.");
                        continue 'album_loop;
                    }
                    Ok(gui_events::CandidateDecision::BackToMusicBrainz) => {
                        summary.failed += 1;
                        println!(
                            "  ERROR: MusicBrainz match navigation is available only for curated compilation tracks."
                        );
                        continue 'album_loop;
                    }
                    Err(error) => {
                        summary.failed += 1;
                        println!("  ERROR: {error}");
                        continue 'album_loop;
                    }
                }
            }
        }

        let effective_mode = if explicit_write {
            Mode::Write
        } else {
            config.mode
        };
        let mut final_config = config.clone();
        final_config.mode = effective_mode;
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

                if config.samples.sample_write && !candidate.source.eq_ignore_ascii_case("samples")
                {
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

                let same_album_existing = candidate.source == "existing"
                    && best.downloaded.path().parent() == Some(album.path.as_path());
                if (matches!(candidate.source.as_str(), "local" | "webpstill")
                    || same_album_existing)
                    && !edit_existing_cover
                    && !apply_edit_profile
                    && !embed_thumbnails
                {
                    let post_cover_started = Instant::now();
                    summary.unchanged += 1;
                    record_runtime_completion(
                        &final_config,
                        album,
                        "local-kept",
                        Some(candidate.source.as_str()),
                        Some(RuntimeArtworkMaterial {
                            path: best.downloaded.path().to_path_buf(),
                            format: format!("{:?}", candidate.format).to_ascii_uppercase(),
                            width: candidate.width,
                            height: candidate.height,
                        }),
                        post_cover_started,
                        post_cover_started,
                    )?;
                    gui_events::emit(json!({
                        "event": "album_completed",
                        "album_path": album.path,
                        "destination": best.downloaded.path(),
                        "action": "KeptLocal",
                        "mode": format!("{:?}", effective_mode).to_ascii_lowercase(),
                    }));
                    if effective_mode == Mode::Write {
                        match remove_completed_album_samples(
                            &sample_dir,
                            &context.artist_credit,
                            &context.release_title,
                        ) {
                            Ok(true) => println!(
                                "  Sample Cleanup: removed saved samples for {} — {}",
                                context.artist_credit, context.release_title
                            ),
                            Ok(false) => {}
                            Err(error) => println!(
                                "  {}",
                                format!("WARNING: sample cleanup failed: {error}")
                                    .yellow()
                                    .bold()
                            ),
                        }
                    }
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
                    &selected_output.file_name,
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
                    effective_mode,
                    candidate,
                    best.downloaded.path(),
                    &canonical_destination,
                    &range,
                    target_format,
                    FinalizationOptions {
                        preserve_file: selected_output.preserve_file && !edit_existing_cover,
                        explicit_manual_selection: manually_selected
                            || active_policy(&config.source_policies, &candidate.source).is_some()
                            || configured_candidate_policy(
                                candidate,
                                &project_configured_artwork(candidate, &range, &selected_output),
                                &range,
                                config,
                            )
                            .status
                                == SourcePolicyStatus::Fallback
                            || result.best_index.is_none(),
                        apply_edit_profile,
                        edit_existing_cover,
                        output: &selected_output,
                    },
                ) {
                    Ok((final_result, destination)) => {
                        if embed_thumbnails && effective_mode == Mode::Write {
                            let full_resolution = fs::read(&destination).map_err(|error| {
                                format!("Unable to read saved full-resolution cover for embedding: {error}")
                            })?;
                            let thumbnail_format = match inspect_image(&destination)?.format {
                                StaticFormat::Png => StaticFormat::Png,
                                _ => StaticFormat::Jpeg,
                            };
                            let thumbnail = prepare_embedded_thumbnail(
                                &full_resolution,
                                selected_output.embedded_thumbnail_size,
                                thumbnail_format,
                            )?;
                            for track in &tracks {
                                if !embedded_front_matches(&track.path, &thumbnail).unwrap_or(false)
                                {
                                    replace_embedded_front(&track.path, &thumbnail)?;
                                }
                            }
                            gui_events::emit(json!({
                                "event": "album_thumbnails_embedded",
                                "album_path": album.path,
                                "track_count": tracks.len(),
                                "size": selected_output.embedded_thumbnail_size,
                            }));
                        }
                        let post_cover_started = Instant::now();
                        if final_result.action == FinalArtworkAction::Installed {
                            match cleanup_replaced_static_covers(
                                &album.path,
                                &selected_output.file_name,
                                &destination,
                                effective_mode,
                                selected_output.preserve_file,
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
                        if effective_mode == Mode::Write
                            && matches!(
                                final_result.action,
                                FinalArtworkAction::Installed | FinalArtworkAction::Unchanged
                            )
                        {
                            match remove_completed_album_samples(
                                &sample_dir,
                                &context.artist_credit,
                                &context.release_title,
                            ) {
                                Ok(true) => println!(
                                    "  Sample Cleanup: removed saved samples for {} — {}",
                                    context.artist_credit, context.release_title
                                ),
                                Ok(false) => {}
                                Err(error) => println!(
                                    "  {}",
                                    format!("WARNING: sample cleanup failed: {error}")
                                        .yellow()
                                        .bold()
                                ),
                            }
                        }
                        println!(
                            "  Destination: {}",
                            destination.display().to_string().yellow().bold()
                        );

                        if let Some(info) = final_result.info {
                            println!(
                                "  Final:       {}x{} {:?} resized={} converted={} upscale_backend={} adaptive={} picture={:+}% brightness={:+}% contrast={:+}% exposure={:+}% sharpen={} softness={} gamma={:+}% color_temperature={:+} quality_eligible={}",
                                info.width,
                                info.height,
                                info.format,
                                info.resized,
                                info.converted,
                                info.upscale_backend.as_str(),
                                info.adaptive_defaults,
                                info.picture_percent,
                                info.brightness_percent,
                                info.contrast_percent,
                                info.exposure_percent,
                                info.sharpen_percent,
                                info.softness_percent,
                                info.gamma_percent,
                                info.color_temperature,
                                info.quality_eligible,
                            );
                        }
                        let final_logged_started = Instant::now();

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
                            record_runtime_completion(
                                &final_config,
                                album,
                                &outcome,
                                Some(&candidate.source),
                                runtime_artwork_material(&destination, final_result.info),
                                post_cover_started,
                                final_logged_started,
                            )?;
                            // Python's chosen-source history is count-only and
                            // intentionally excludes every fallback choice.
                            if fallback_reason.is_none()
                                && !matches!(candidate.source.as_str(), "local" | "webpstill")
                            {
                                let _ = record_source_selection(
                                    &final_config,
                                    &mut source_history,
                                    candidate,
                                    &range,
                                    unix_now(),
                                    false,
                                );
                            }
                        }
                        gui_events::emit(json!({
                            "event": "album_completed",
                            "album_path": album.path,
                            "destination": destination,
                            "action": format!("{:?}", final_result.action),
                            "mode": format!("{:?}", effective_mode).to_ascii_lowercase(),
                            "source": candidate.source,
                            "source_width": candidate.width,
                            "source_height": candidate.height,
                            "final_width": final_result.info.map(|info| info.width),
                            "final_height": final_result.info.map(|info| info.height),
                            "resized": final_result.info.is_some_and(|info| info.resized),
                            "converted": final_result.info.is_some_and(|info| info.converted),
                            "upscale_backend": final_result.info
                                .map(|info| info.upscale_backend.as_str())
                                .unwrap_or("none"),
                            "brightness_percent": final_result.info.map(|info| info.brightness_percent).unwrap_or(0),
                            "contrast_percent": final_result.info.map(|info| info.contrast_percent).unwrap_or(0),
                            "exposure_percent": final_result.info.map(|info| info.exposure_percent).unwrap_or(0),
                            "picture_percent": final_result.info.map(|info| info.picture_percent).unwrap_or(0),
                            "sharpen_percent": final_result.info.map(|info| info.sharpen_percent).unwrap_or(0),
                            "softness_percent": final_result.info.map(|info| info.softness_percent).unwrap_or(0),
                            "gamma_percent": final_result.info.map(|info| info.gamma_percent).unwrap_or(0),
                            "color_temperature": final_result.info.map(|info| info.color_temperature).unwrap_or(0),
                            "upscale_adaptive_defaults": final_result.info.is_some_and(|info| info.adaptive_defaults),
                            "quality_eligible": final_result.info.is_some_and(|info| info.quality_eligible),
                        }));
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
    }

    fs::create_dir_all(sample_dir).map_err(|error| {
        format!(
            "Unable to create SPLINED samples directory {}: {error}",
            sample_dir.display()
        )
    })
}

struct RunCacheCleanup<'a> {
    path: &'a Path,
}

impl<'a> RunCacheCleanup<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path }
    }
}

impl Drop for RunCacheCleanup<'_> {
    fn drop(&mut self) {
        crate::pipeline::cleanup_run_cache_files(self.path);
    }
}

fn append_saved_sample_candidates(
    result: &mut PipelineResult,
    sample_root: &Path,
    artist: &str,
    album: &str,
) -> Result<usize, String> {
    let album_dir = sample_root
        .join(sanitize_sample_component(artist))
        .join(sanitize_sample_component(album));
    let entries = match fs::read_dir(&album_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(format!(
                "Unable to read saved Samples directory {}: {error}",
                album_dir.display()
            ));
        }
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    });

    let initial_len = result.candidates.len();
    for (offset, path) in paths.into_iter().enumerate() {
        let provider = sample_provider_label(&path);
        let source_priority = initial_len.saturating_add(offset);
        let Ok(downloaded) =
            DownloadedCandidate::from_existing_path("samples", path.clone(), source_priority, "")
        else {
            continue;
        };
        result.candidates.push(PipelineCandidate {
            reference: ArtworkReference {
                source: "samples".to_string(),
                id: provider.clone(),
                url: String::new(),
                front: true,
                approved: true,
                types: vec!["Front".to_string(), "Sample".to_string()],
            },
            downloaded,
            strict: StrictContentDecision {
                status: "saved-sample".to_string(),
                reason: format!("Saved from {provider}"),
                preferred_eligible: false,
                auto_eligible: false,
                ..StrictContentDecision::default()
            },
        });
    }
    Ok(result.candidates.len().saturating_sub(initial_len))
}

fn sample_provider_label(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Saved source");
    let provider = stem
        .split_once(".sample")
        .map_or(stem, |(provider, _)| provider)
        .replace(['_', '-'], " ");
    let provider = provider.trim();
    if provider.is_empty() {
        "Saved source".to_string()
    } else {
        provider.to_string()
    }
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
    let album_dir = sample_dir
        .join(sanitize_sample_component(artist))
        .join(sanitize_sample_component(album));
    fs::create_dir_all(&album_dir).map_err(|error| {
        format!(
            "Unable to create album sample directory {}: {error}",
            album_dir.display()
        )
    })?;
    let file_name = format!(
        "{}.sample.{}",
        sanitize_sample_component(&candidate.source),
        output_extension(candidate.format)
    );
    let canonical = album_dir.join(file_name);
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

#[derive(Default)]
struct CompilationRunResult {
    selected: usize,
    installed: usize,
    read_only: usize,
    unresolved: usize,
    failed: bool,
}

struct NormalSourceResultsEvent<'a> {
    album: &'a AlbumDirectory,
    result: &'a PipelineResult,
    display_indices: &'a [usize],
    suggested_index: Option<usize>,
    hidden_by_source_policy: usize,
    fallback_reason: Option<&'a str>,
    musicbrainz_retry_available: bool,
    musicbrainz_matches_available: bool,
    range: &'a Range,
    config: &'a Config,
}

fn emit_normal_source_results(event: NormalSourceResultsEvent<'_>) {
    let items = event.display_indices
        .iter()
        .map(|candidate_index| {
            let item = &event.result.candidates[*candidate_index];
            let candidate = &item.downloaded.candidate;
            let projected = project_configured_artwork(candidate, event.range, &event.config.output);
            let policy = configured_candidate_policy(candidate, &projected, event.range, event.config);
            json!({
                "index": *candidate_index + 1, "source": candidate.source,
                "width": candidate.width, "height": candidate.height,
                "format": format!("{:?}", candidate.format).to_ascii_lowercase(),
                "source_range_class": projected_range_class(candidate.short_side(), event.range),
                "source_distance_from_ideal": candidate.short_side().abs_diff(event.range.ideal),
                "range_class": projected_range_class(projected.width.min(projected.height), event.range),
                "distance_from_ideal": projected.width.min(projected.height).abs_diff(event.range.ideal),
                "square": candidate.is_square(),
                "acceptable": policy.status != SourcePolicyStatus::Reject,
                "policy_status": policy.status, "policy_reason": policy.reason,
                "source_override_active": active_policy(&event.config.source_policies, &candidate.source).is_some(),
                "strict_override_active": event.config.source_policies
                    .get(&candidate.source.to_ascii_lowercase())
                    .is_some_and(|source_policy| source_policy.strict_override),
                "strict_status": item.strict.status,
                "strict_reason": item.strict.reason,
                "strict_preferred_eligible": item.strict.preferred_eligible,
                "strict_auto_eligible": item.strict.auto_eligible,
                "strict_match_distance": item.strict.match_distance,
                "strict_match_source": item.strict.match_source,
                "projected_width": projected.width, "projected_height": projected.height,
                "cropped": projected.cropped, "resized": projected.resized,
                "upscaled": projected.upscaled, "approved": item.reference.approved,
                "reference_id": item.reference.id,
                "local_origin": if item.reference.types.iter().any(|value| value == "EmbeddedTrack") {
                    "embedded-track"
                } else if item.reference.types.iter().any(|value| value == "ExistingCover") {
                    "artist-album-cover"
                } else if item.reference.types.iter().any(|value| value == "CoverFile") {
                    "cover-file"
                } else { "" },
                "local_reference": item.reference.id, "url": item.reference.url,
                "image_type": if candidate.source.eq_ignore_ascii_case("samples") {
                    format!("Sample · {}", item.reference.id)
                } else {
                    candidate_image_category(&item.reference.types).to_string()
                },
                "cache_path": item.downloaded.path(),
                "recommended": Some(*candidate_index) == event.suggested_index,
            })
        })
        .collect::<Vec<_>>();
    gui_events::emit(json!({
        "event": "candidates", "album_path": event.album.path, "items": items,
        "recommended_index": event.suggested_index.map(|value| value + 1),
        "hidden_by_source_policy": event.hidden_by_source_policy,
        "fallback": event.fallback_reason.is_some(), "fallback_reason": event.fallback_reason,
        "musicbrainz_retry_available": event.musicbrainz_retry_available,
        "musicbrainz_matches_available": event.musicbrainz_matches_available,
    }));
}

struct CompilationRunContext<'a> {
    config: &'a Config,
    resolved_sources: &'a [String],
    registry: &'a ProviderRegistry,
    musicbrainz: &'a MusicBrainzClient,
    range: &'a Range,
    format_order: &'a [StaticFormat],
    cache_dir: &'a Path,
}

#[derive(Clone)]
struct CachedPipelineCandidate {
    reference: ArtworkReference,
    path: PathBuf,
    source_priority: usize,
    strict: StrictContentDecision,
}

#[derive(Clone)]
struct CachedPipelineResult {
    candidates: Vec<CachedPipelineCandidate>,
    best_index: Option<usize>,
    diagnostics: Vec<crate::pipeline::PipelineDiagnostic>,
}

impl CachedPipelineResult {
    fn capture(result: &PipelineResult) -> Self {
        Self {
            candidates: result
                .candidates
                .iter()
                .map(|item| CachedPipelineCandidate {
                    reference: item.reference.clone(),
                    path: item.downloaded.path().to_path_buf(),
                    source_priority: item.downloaded.candidate.source_priority,
                    strict: item.strict.clone(),
                })
                .collect(),
            best_index: result.best_index,
            diagnostics: result.diagnostics.clone(),
        }
    }

    fn restore(&self) -> Result<PipelineResult, String> {
        let candidates = self
            .candidates
            .iter()
            .map(|item| {
                Ok(PipelineCandidate {
                    reference: item.reference.clone(),
                    downloaded: DownloadedCandidate::from_existing_path(
                        item.reference.source.clone(),
                        item.path.clone(),
                        item.source_priority,
                        item.reference.url.clone(),
                    )?,
                    strict: item.strict.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(PipelineResult {
            candidates,
            best_index: self.best_index,
            diagnostics: self.diagnostics.clone(),
            provider_timings: Vec::new(),
        })
    }
}

enum NormalBrowserOutcome {
    Use {
        pipeline: Box<PipelineResult>,
        selected: usize,
        provider_context: Box<ProviderContext>,
        explicit_write: bool,
        upscale: Option<gui_events::UpscaleOverrides>,
    },
    ReturnToSources,
    Skip,
    Bypass,
}

struct MusicBrainzSessionAuthority {
    recording_mbid: Option<String>,
    artist_mbids: Vec<String>,
    release_mbid: String,
    release_title: String,
    release_artist: String,
}

struct MusicBrainzBrowserDisplay<'a> {
    visited_releases: &'a std::collections::HashSet<String>,
    current_release: Option<&'a str>,
    searched: bool,
    resolutions: &'a std::collections::HashMap<String, String>,
    notice: Option<&'a str>,
}

impl MusicBrainzSessionAuthority {
    fn from_track(track: &LocalTrackEvidence) -> Self {
        let (recording_mbid, artist_mbids) = track_authority(track);
        Self {
            recording_mbid,
            artist_mbids,
            release_mbid: track.musicbrainz_album_id.clone().unwrap_or_default(),
            release_title: track.album.clone().unwrap_or_default(),
            release_artist: track
                .album_artist
                .clone()
                .unwrap_or_else(|| track.artist.clone()),
        }
    }

    fn apply(
        &mut self,
        recording_mbid: &str,
        artist_mbids: &str,
        release_mbid: &str,
        release_title: &str,
        release_artist: &str,
    ) {
        let recording_mbid = recording_mbid.trim();
        self.recording_mbid =
            (!recording_mbid.is_empty()).then(|| recording_mbid.to_ascii_lowercase());
        self.artist_mbids = artist_mbids
            .split(|character: char| {
                character == ',' || character == ';' || character.is_whitespace()
            })
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        self.release_mbid = release_mbid.trim().to_ascii_lowercase();
        self.release_title = release_title.to_string();
        self.release_artist = release_artist.to_string();
    }
}

fn resolved_track_authority(
    config: &Config,
    track: &LocalTrackEvidence,
) -> (Option<String>, Vec<String>) {
    let (mut recording_mbid, mut artist_mbids) = track_authority(track);
    if (recording_mbid.is_none() || artist_mbids.is_empty())
        && let Ok(Some((cached_recording, cached_artist))) =
            cached_track_musicbrainz_identity(config, &track.path)
    {
        if recording_mbid.is_none() {
            recording_mbid = extract_mbids(&cached_recording).into_iter().next();
        }
        if artist_mbids.is_empty() {
            artist_mbids = extract_mbids(&cached_artist);
        }
    }
    if artist_mbids.is_empty()
        && let Ok(Some(cached_artist)) = cached_artist_musicbrainz_identity(config, &track.artist)
    {
        artist_mbids = extract_mbids(&cached_artist);
    }
    (recording_mbid, artist_mbids)
}

fn persist_validated_track_identity(
    config: &Config,
    track: &LocalTrackEvidence,
    recording_mbid: &str,
    artist_mbids: &str,
) -> Result<(), String> {
    let artists = extract_mbids(artist_mbids);
    if artists.is_empty() {
        return Err("A validated Artist ID is required before saving track identity.".to_string());
    }
    let artist_ids = artists.join(", ");
    replace_musicbrainz_identity(&track.path, recording_mbid, &artist_ids)?;
    persist_track_musicbrainz_identity(config, &track.path, recording_mbid, &artist_ids)
}

fn focused_track_artist(track_artist: &str) -> String {
    let focused = gui_events::scan_context().fallback_artist;
    focused_track_artist_name(track_artist, focused.as_deref())
}

fn focused_track_artist_name(track_artist: &str, focused: Option<&str>) -> String {
    if let Some(focused) = focused.filter(|focused| {
        track_artist
            .split(['\0', ';'])
            .any(|credit| credit.trim().eq_ignore_ascii_case(focused.trim()))
    }) {
        return focused.to_string();
    }
    track_artist
        .split(['\0', ';'])
        .find(|credit| !credit.trim().is_empty())
        .unwrap_or(track_artist)
        .trim()
        .to_string()
}

fn normal_album_existing_artwork(
    config: &Config,
    album: &AlbumDirectory,
    tracks: &[LocalTrackEvidence],
    audit: &TaggedAlbumIdAudit,
    local_candidate: Option<&PipelineCandidate>,
) -> Result<Option<(PathBuf, PathBuf, &'static str)>, String> {
    if audit.valid.len() != 1 || !audit.missing.is_empty() || !audit.invalid.is_empty() {
        return Ok(None);
    }
    let release_mbid = audit.valid[0].release_mbid.as_str();
    let local_cover = local_candidate
        .map(|candidate| candidate.downloaded.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("cover"))
        });
    for track in tracks {
        let (Some(recording), artist_mbids) = resolved_track_authority(config, track) else {
            continue;
        };
        if artist_mbids.is_empty() {
            continue;
        }
        if let Some(cover) = local_cover {
            return Ok(Some((
                track.path.clone(),
                cover.to_path_buf(),
                "artist-album-cover",
            )));
        }
        if let Some(found) = find_local_compilation_artwork(
            config,
            &recording,
            &artist_mbids,
            &album.path,
            &focused_track_artist(&track.artist),
            &track.title,
            Some(release_mbid),
        )? {
            return Ok(Some((
                found.track_path,
                found.cover_path,
                "artist-album-cover",
            )));
        }
    }
    Ok(None)
}

async fn run_normal_musicbrainz_browser(
    context: &CompilationRunContext<'_>,
    match_cache: &mut CompilationMatchCache,
    album: &AlbumDirectory,
    track: &LocalTrackEvidence,
    existing_source: Option<&(PathBuf, PathBuf, &'static str)>,
) -> Result<NormalBrowserOutcome, String> {
    let CompilationRunContext {
        config,
        resolved_sources,
        registry,
        musicbrainz,
        range,
        format_order,
        cache_dir,
    } = context;
    let manual_musicbrainz = if musicbrainz.is_enabled() {
        None
    } else {
        let mut options = config.musicbrainz.clone();
        options.enabled = true;
        Some(MusicBrainzClient::new(&options)?)
    };
    let musicbrainz = manual_musicbrainz.as_ref().unwrap_or(musicbrainz);
    let (recording_mbid, artist_mbids) = resolved_track_authority(config, track);
    let mut authority = MusicBrainzSessionAuthority::from_track(track);
    authority.recording_mbid = recording_mbid.clone();
    authority.artist_mbids = artist_mbids.clone();
    let use_search = recording_mbid.is_none();
    let search_artist = focused_track_artist(&track.artist);
    let mut matches = match_cache
        .browser_matches(
            musicbrainz,
            recording_mbid.as_deref(),
            &artist_mbids,
            &search_artist,
            &track.title,
        )
        .await?;
    let mut visited = std::collections::HashSet::<String>::new();
    let mut current_release = None::<String>;
    let mut source_results = std::collections::HashMap::<String, CachedPipelineResult>::new();
    let mut resolutions = std::collections::HashMap::<String, String>::new();
    let mut browser_notice = None::<String>;

    loop {
        emit_normal_musicbrainz_matches(
            track,
            &matches,
            &authority,
            existing_source,
            MusicBrainzBrowserDisplay {
                visited_releases: &visited,
                current_release: current_release.as_deref(),
                searched: use_search,
                resolutions: &resolutions,
                notice: browser_notice.take().as_deref(),
            },
        );
        let (selected, selected_from_authority, preview_write) =
            match gui_events::wait_for_musicbrainz_match_decision()? {
                gui_events::MusicBrainzMatchDecision::Use(index) if index < matches.len() => {
                    (matches[index].clone(), false, None)
                }
                gui_events::MusicBrainzMatchDecision::Use(_) => {
                    return Err("Selected MusicBrainz match does not exist.".to_string());
                }
                gui_events::MusicBrainzMatchDecision::UseExisting { upscale, .. } => {
                    let (source_track, image_path, kind) = existing_source.ok_or_else(|| {
                        "Existing artwork is no longer available for this Album.".to_string()
                    })?;
                    let pipeline = existing_compilation_pipeline(image_path, source_track, kind)?;
                    return Ok(NormalBrowserOutcome::Use {
                        pipeline: Box::new(pipeline),
                        selected: 0,
                        provider_context: Box::new(ProviderContext {
                            artist_credit: track
                                .album_artist
                                .clone()
                                .unwrap_or_else(|| track.artist.clone()),
                            release_title: track.album.clone().unwrap_or_default(),
                            release_group_mbid: None,
                            release_group_title: track.album.clone(),
                            apple_collection_ids: Vec::new(),
                        }),
                        explicit_write: true,
                        upscale,
                    });
                }
                gui_events::MusicBrainzMatchDecision::WritePreview {
                    index,
                    cover_path,
                    upscale,
                } if index < matches.len() => {
                    (matches[index].clone(), false, Some((cover_path, upscale)))
                }
                gui_events::MusicBrainzMatchDecision::WritePreview { .. } => {
                    return Err("Selected MusicBrainz preview release does not exist.".to_string());
                }
                gui_events::MusicBrainzMatchDecision::UseAuthority {
                    recording_mbid,
                    artist_mbids,
                    release_mbid,
                } => {
                    let edited = match match_cache
                        .edited_authority_matches(
                            musicbrainz,
                            &recording_mbid,
                            &artist_mbids,
                            &release_mbid,
                            &search_artist,
                            &track.title,
                        )
                        .await
                    {
                        Ok(resolution) if resolution.identity_status == "valid" => resolution,
                        Ok(resolution) => {
                            gui_events::emit(json!({ "event": "musicbrainz_authority_error",
                            "message": resolution.identity_message }));
                            continue;
                        }
                        Err(error) => {
                            gui_events::emit(
                                json!({ "event": "musicbrainz_authority_error", "message": error }),
                            );
                            continue;
                        }
                    };
                    let _ = release_mbid;
                    if let Err(error) = persist_validated_track_identity(
                        config,
                        track,
                        &recording_mbid,
                        &artist_mbids,
                    ) {
                        gui_events::emit(
                            json!({ "event": "musicbrainz_authority_error", "message": error }),
                        );
                        continue;
                    }
                    matches = edited.matches;
                    authority.apply(&recording_mbid, &artist_mbids, "", "", "");
                    visited.clear();
                    current_release = None;
                    browser_notice = Some(format!(
                        "Accepted MusicBrainz identity returned {} release results.",
                        matches.len()
                    ));
                    continue;
                }
                gui_events::MusicBrainzMatchDecision::Skip => {
                    return Ok(NormalBrowserOutcome::Skip);
                }
                gui_events::MusicBrainzMatchDecision::Bypass => {
                    return Ok(NormalBrowserOutcome::Bypass);
                }
                gui_events::MusicBrainzMatchDecision::ClearBypass => {
                    clear_runtime_bypass(config, album)?;
                    continue;
                }
                gui_events::MusicBrainzMatchDecision::LeaveUnchanged => {
                    return Ok(NormalBrowserOutcome::ReturnToSources);
                }
            };

        let provider_context = ProviderContext {
            artist_credit: selected.release_artist.clone(),
            release_title: selected.release_title.clone(),
            release_group_mbid: selected.release_group_mbid.clone(),
            release_group_title: Some(selected.release_title.clone()),
            apple_collection_ids: Vec::new(),
        };
        if let Some((cover_path, upscale)) = preview_write {
            let pipeline = musicbrainz_preview_pipeline(Path::new(&cover_path), &selected)?;
            return Ok(NormalBrowserOutcome::Use {
                pipeline: Box::new(pipeline),
                selected: 0,
                provider_context: Box::new(provider_context),
                explicit_write: true,
                upscale,
            });
        }
        let mut pipeline = if let Some(cached) = source_results.get(&selected.release_mbid) {
            gui_events::emit(json!({ "event": "source_results_cache_hit",
                "album_path": album.path, "release_mbid": selected.release_mbid,
                "candidates": cached.candidates.len() }));
            cached.restore()?
        } else {
            let discovered = run_registry_pipeline_with_cache_dir(
                registry,
                &ArtworkQuery::release(&selected.release_mbid),
                &provider_context,
                RegistryPipelineOptions {
                    source_order: resolved_sources,
                    range,
                    format_order,
                    source_policies: &config.source_policies,
                },
                cache_dir,
            )
            .await?;
            for timing in &discovered.provider_timings {
                gui_events::emit(
                    json!({ "event": "provider_timing", "album_path": album.path,
                    "source": timing.source, "status": timing.status(),
                    "discovery_ms": timing.discovery_ms, "download_ms": timing.download_ms,
                    "references": timing.references, "candidates": timing.candidates,
                    "errors": timing.errors, "error": timing.error }),
                );
            }
            discovered
        };
        apply_strict_candidate_policy(&mut pipeline, range, format_order, config);
        if let Some(best) = pipeline
            .best_index
            .and_then(|index| pipeline.candidates.get(index))
        {
            resolutions.insert(
                selected.release_mbid.clone(),
                format!(
                    "{}x{}",
                    best.downloaded.candidate.width, best.downloaded.candidate.height
                ),
            );
        }
        source_results.insert(
            selected.release_mbid.clone(),
            CachedPipelineResult::capture(&pipeline),
        );
        let suggested_index = pipeline.best_index;
        let display_indices = (0..pipeline.candidates.len())
            .filter(|index| {
                candidate_visible_for_review(
                    &pipeline.candidates[*index].downloaded.candidate,
                    range,
                    config,
                )
            })
            .collect::<Vec<_>>();
        if display_indices.is_empty() {
            visited.insert(selected.release_mbid.clone());
            current_release = Some(selected.release_mbid.clone());
            browser_notice = Some(format!(
                "No usable artwork is available for {} · choose another MusicBrainz release",
                selected.release_title
            ));
            continue;
        }
        let gui_candidates = display_indices
            .iter()
            .map(|candidate_index| {
                let item = &pipeline.candidates[*candidate_index];
                let candidate = &item.downloaded.candidate;
                let projected = project_configured_artwork(candidate, range, &config.output);
                let policy = configured_candidate_policy(candidate, &projected, range, config);
                json!({ "index": candidate_index + 1, "source": candidate.source,
                    "width": candidate.width, "height": candidate.height,
                    "format": format!("{:?}", candidate.format).to_ascii_lowercase(),
                    "source_range_class": projected_range_class(candidate.short_side(), range),
                    "source_distance_from_ideal": candidate.short_side().abs_diff(range.ideal),
                    "range_class": projected_range_class(projected.width.min(projected.height), range),
                    "distance_from_ideal": projected.width.min(projected.height).abs_diff(range.ideal),
                    "square": candidate.is_square(), "acceptable": policy.status != SourcePolicyStatus::Reject,
                    "policy_status": policy.status, "policy_reason": policy.reason,
                    "source_override_active": active_policy(&config.source_policies, &candidate.source).is_some(),
                    "strict_override_active": config.source_policies.get(&candidate.source.to_ascii_lowercase()).is_some_and(|source_policy| source_policy.strict_override),
                    "strict_status": item.strict.status, "strict_reason": item.strict.reason,
                    "strict_preferred_eligible": item.strict.preferred_eligible,
                    "strict_auto_eligible": item.strict.auto_eligible,
                    "projected_width": projected.width, "projected_height": projected.height,
                    "cropped": projected.cropped, "resized": projected.resized,
                    "upscaled": projected.upscaled, "approved": item.reference.approved,
                    "reference_id": item.reference.id, "local_origin": "",
                    "local_reference": item.reference.id, "url": item.reference.url,
                    "cache_path": item.downloaded.path(),
                    "recommended": Some(*candidate_index) == suggested_index })
            })
            .collect::<Vec<_>>();
        gui_events::emit(json!({ "event": "candidates", "album_path": album.path,
            "items": gui_candidates, "recommended_index": suggested_index.map(|value| value + 1),
            "hidden_by_source_policy": pipeline.candidates.len() - display_indices.len(),
            "fallback": false, "musicbrainz_back_available": true,
            "musicbrainz_matches_available": false }));
        gui_events::emit(
            json!({ "event": "decision_required", "album_path": album.path,
            "reason": "musicbrainz-release", "suggested_index": suggested_index.map(|value| value + 1),
            "allow_bypass": true, "musicbrainz_back_available": true,
            "musicbrainz_matches_available": false }),
        );

        loop {
            match gui_events::wait_for_candidate_decision()? {
                gui_events::CandidateDecision::Use {
                    index,
                    explicit_write,
                    ..
                } if index < pipeline.candidates.len() => {
                    if !candidate_visible_for_review(
                        &pipeline.candidates[index].downloaded.candidate,
                        range,
                        config,
                    ) {
                        return Err(
                            "Candidate is rejected by the active source policy.".to_string()
                        );
                    }
                    return Ok(NormalBrowserOutcome::Use {
                        pipeline: Box::new(pipeline),
                        selected: index,
                        provider_context: Box::new(provider_context),
                        explicit_write,
                        upscale: None,
                    });
                }
                gui_events::CandidateDecision::BackToMusicBrainz => {
                    if !selected_from_authority {
                        visited.insert(selected.release_mbid.clone());
                        current_release = Some(selected.release_mbid);
                    }
                    break;
                }
                gui_events::CandidateDecision::Skip => {
                    return Ok(NormalBrowserOutcome::Skip);
                }
                gui_events::CandidateDecision::Bypass => {
                    return Ok(NormalBrowserOutcome::Bypass);
                }
                gui_events::CandidateDecision::ClearBypass => {
                    clear_runtime_bypass(config, album)?;
                }
                _ => return Err("Invalid MusicBrainz artwork decision.".to_string()),
            }
        }
    }
}

fn emit_normal_musicbrainz_matches(
    track: &LocalTrackEvidence,
    matches: &[MusicBrainzMatch],
    authority: &MusicBrainzSessionAuthority,
    existing_source: Option<&(PathBuf, PathBuf, &'static str)>,
    display: MusicBrainzBrowserDisplay<'_>,
) {
    let identity_valid = authority.recording_mbid.is_some()
        && !authority.artist_mbids.is_empty()
        && !matches.is_empty();
    let used_fuzzy_search = matches.iter().any(|item| item.score < 95);
    let friendly_artist = matches
        .first()
        .map(|item| item.recording_artist.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&track.artist);
    let friendly_title = matches
        .first()
        .map(|item| item.recording_title.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&track.title);
    let items = matches
        .iter()
        .enumerate()
        .map(|(index, item)| {
            json!({ "index": index + 1, "recording_mbid": item.recording_mbid,
                "recording_title": item.recording_title, "recording_artist": item.recording_artist,
                "artist_mbids": item.artist_mbids, "release_mbid": item.release_mbid,
                "release_group_mbid": item.release_group_mbid,
                "release_group_title": item.release_group_title,
                "release_class": item.release_class,
                "release_title": item.release_title, "release_artist": item.release_artist,
                "release_date": item.release_date, "country": item.country, "score": item.score,
                "url": item.url,
                "resolution": display.resolutions.get(&item.release_mbid),
                "visited": display.visited_releases.contains(&item.release_mbid),
                "current": display.current_release.is_some_and(|value| value.eq_ignore_ascii_case(&item.release_mbid)) })
        })
        .collect::<Vec<_>>();
    gui_events::emit(
        json!({ "event": "musicbrainz_matches", "track_path": track.path,
        "artist": track.artist, "title": track.title, "searched": display.searched,
        "album_artist": track.album_artist.as_deref().unwrap_or(&track.artist),
        "album": track.album.as_deref().unwrap_or(""),
        "authority_recording_mbid": authority.recording_mbid.as_deref().unwrap_or(""),
        "authority_artist_mbids": authority.artist_mbids.join(", "),
        "authority_release_mbid": authority.release_mbid,
        "authority_release_title": authority.release_title,
        "authority_release_artist": authority.release_artist,
        "identity_status": if identity_valid { "valid" } else { "failed" },
        "identity_message": if identity_valid { "MusicBrainz Recording and Artist identity verified." } else { "MusicBrainz identity could not be resolved." },
        "recording_status": if authority.recording_mbid.is_some() { "valid" } else { "invalid" },
        "artist_status": if authority.artist_mbids.len() == 1 { "valid" } else { "invalid" },
        "focused_artist_mbid": if authority.artist_mbids.len() == 1 { authority.artist_mbids[0].as_str() } else { "" },
        "friendly_artist": friendly_artist,
        "friendly_title": friendly_title,
        "existing_artwork_path": existing_source.map(|(_, path, _)| path.to_string_lossy().into_owned()).unwrap_or_default(),
        "existing_artwork_kind": existing_source.map(|(_, _, kind)| *kind).unwrap_or(""),
        "existing_artwork_title": existing_source.and_then(|(track, _, _)| track.parent()).and_then(|folder| folder.file_name()).and_then(|name| name.to_str()).unwrap_or(""),
        "used_fuzzy_search": used_fuzzy_search,
        "compilation_track": false, "notice": display.notice.unwrap_or(""), "items": items }),
    );
}

async fn run_compilation_album(
    context: &CompilationRunContext<'_>,
    match_cache: &mut CompilationMatchCache,
    album: &AlbumDirectory,
    tracks: &[LocalTrackEvidence],
) -> Result<CompilationRunResult, String> {
    let CompilationRunContext {
        config,
        resolved_sources,
        registry,
        musicbrainz,
        range,
        format_order,
        cache_dir,
    } = context;
    let manual_musicbrainz = if !musicbrainz.is_enabled() && gui_events::musicbrainz_requested() {
        let mut options = config.musicbrainz.clone();
        options.enabled = true;
        Some(MusicBrainzClient::new(&options)?)
    } else {
        None
    };
    let musicbrainz = manual_musicbrainz.as_ref().unwrap_or(musicbrainz);
    let requested_track = gui_events::scan_context().compilation_track_path;
    let tracks = if let Some(requested) = requested_track.as_ref() {
        let targeted = tracks
            .iter()
            .filter(|track| {
                track
                    .path
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&requested.to_string_lossy())
            })
            .cloned()
            .collect::<Vec<_>>();
        if targeted.is_empty() {
            return Err(format!(
                "The selected compilation track is not present in this Album: {}",
                requested.display()
            ));
        }
        targeted
    } else {
        tracks.to_vec()
    };
    let targeted_edit = requested_track.is_some();
    // Track previews are GUI state, not provider/run-cache state. The Windows
    // GUI supplies a portable, writable preview cache so a read-only shared
    // temporary cache cannot suppress the current track's embedded artwork.
    let embedded_preview_cache = embedded_preview_cache_dir(cache_dir);
    let resume_track = if targeted_edit {
        None
    } else {
        compilation_resume_track(config, &album.path)?
    };
    gui_events::emit(json!({
        "event": "compilation_started", "album_path": album.path,
        "total_tracks": tracks.len(),
        "targeted_track": requested_track,
        "resume_track": resume_track,
        "message": if targeted_edit {
            "Reopening one selected fallback track for embedded-art review; folder cover files are ignored and untouched."
        } else {
            "Missing Album MBID: approved artwork will be embedded per track; folder cover files are ignored and untouched."
        }
    }));
    let identities = tracks
        .iter()
        .map(|track| {
            let (recording, artists) = track_authority(track);
            (
                track.path.clone(),
                recording.unwrap_or_default(),
                artists.join(","),
            )
        })
        .collect::<Vec<_>>();
    let already_completed = if targeted_edit {
        std::collections::HashSet::new()
    } else {
        completed_compilation_track_paths(config, &album.path, &identities)?
    };
    let resume_index = compilation_resume_start_index(&tracks, resume_track.as_deref());
    let mut completed = already_completed.len();
    let mut result = CompilationRunResult::default();
    // Keep every inspected release available for the duration of this Album.
    // Returning to MusicBrainz Matches must not repeat provider discovery or
    // downloads, and ranking is restored from the original deterministic run.
    let mut source_results_cache = std::collections::HashMap::<String, CachedPipelineResult>::new();

    for (track_index, track) in tracks.iter().enumerate().skip(resume_index) {
        if gui_events::cancelled() {
            break;
        }
        if !targeted_edit {
            // The current track remains the restart point until processing advances
            // to the next loop iteration. Stop/error therefore resumes at the exact
            // decision that was visible, while the completion ledger remains the
            // authority for already-written tracks.
            record_compilation_resume_track(config, &album.path, Some(&track.path))?;
        }
        if compilation_track_is_already_complete(&track.path, &already_completed, targeted_edit) {
            gui_events::emit(
                json!({ "event": "compilation_track_skipped", "album_path": album.path,
                "track_path": track.path, "index": track_index + 1, "total": tracks.len(), "reason": "already-complete" }),
            );
            continue;
        }
        // The GUI preview is deliberately sourced only from this track's
        // embedded bytes. Folder-level cover files are not candidates for the
        // fallback-compilation preview. A preview extraction failure must not
        // alter normal resume or provider discovery; explicit targeted edits
        // retain the existing strict error behavior.
        let embedded_preview = if targeted_edit {
            embedded_candidate(&track.path, &embedded_preview_cache)?
        } else {
            embedded_candidate(&track.path, &embedded_preview_cache).unwrap_or(None)
        };
        let embedded_artwork_path = embedded_preview
            .as_ref()
            .map(|candidate| candidate.path().to_path_buf());
        gui_events::emit(
            json!({ "event": "compilation_track_started", "album_path": album.path,
            "track_path": track.path, "artist": track.artist, "title": track.title,
            "embedded_artwork_path": embedded_artwork_path,
            "index": track_index + 1, "total": tracks.len() }),
        );

        let (recording_mbid, artist_mbids) = resolved_track_authority(config, track);
        let artist_album_covers = match recording_mbid.as_deref() {
            Some(recording) => match find_local_compilation_artworks(
                config,
                recording,
                &artist_mbids,
                &album.path,
                &focused_track_artist(&track.artist),
                &track.title,
                None,
            ) {
                Ok(found) => found
                    .into_iter()
                    .map(|item| {
                        let kind = if item.recording_match {
                            "artist-album-cover"
                        } else {
                            "artist-album-cover-title"
                        };
                        (item.track_path, item.cover_path, kind)
                    })
                    .collect::<Vec<_>>(),
                Err(error) => {
                    gui_events::emit(json!({"event": "compilation_lookup_warning",
                        "track_path": track.path, "message": format!("Artist Album cover lookup unavailable: {error}")}));
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        let embedded_existing = match recording_mbid.as_deref() {
            Some(recording) => match find_existing_compilation_artwork(
                config,
                recording,
                &artist_mbids,
                &track.path,
                &track.title,
            ) {
                Ok(Some(existing)) => {
                    match embedded_candidate(&existing.track_path, &embedded_preview_cache) {
                        Ok(Some(candidate)) => fs::read(candidate.path()).ok().and_then(|bytes| {
                            let digest = Sha256::digest(&bytes);
                            let actual_hash = digest
                                .iter()
                                .map(|byte| format!("{byte:02x}"))
                                .collect::<String>();
                            (actual_hash.eq_ignore_ascii_case(&existing.artwork_sha256)).then(
                                || {
                                    (
                                        existing.track_path,
                                        candidate.path().to_path_buf(),
                                        "embedded-track",
                                    )
                                },
                            )
                        }),
                        Ok(None) | Err(_) => None,
                    }
                }
                Ok(None) => None,
                Err(error) => {
                    gui_events::emit(json!({"event": "compilation_lookup_warning",
                        "track_path": track.path, "message": format!("Existing artwork lookup unavailable: {error}")}));
                    None
                }
            },
            None => None,
        };
        let mut existing_sources = artist_album_covers;
        if let Some(source) = embedded_existing
            && !existing_sources.iter().any(|item| {
                item.0
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&source.0.to_string_lossy())
            })
        {
            existing_sources.push(source);
        }
        let mut existing_source = existing_sources.first().cloned();
        if let Some((source_track, _, kind)) = &existing_source {
            gui_events::emit(json!({"event": "compilation_existing_artwork",
            "track_path": track.path, "source_track_path": source_track,
            "message": if *kind == "artist-album-cover" {
                "Existing Artist Album cover verified for the same Recording and Artist IDs."
            } else if *kind == "artist-album-cover-title" {
                "Existing Artist Album cover found by Artist ID and exact song title; Recording ID differs."
            } else {
                "Existing embedded art verified for the same Recording and Artist IDs."
            }}));
        }
        let mut use_existing = false;
        let mut lookup_pending = true;
        let search_artist = focused_track_artist(&track.artist);
        let mut authority = MusicBrainzSessionAuthority::from_track(track);
        authority.recording_mbid = recording_mbid.clone();
        authority.artist_mbids = artist_mbids.clone();
        let targeted_embedded = if targeted_edit {
            embedded_artwork_path
        } else {
            None
        };
        let mut selected_match: Option<MusicBrainzMatch> = None;
        let mut direct_preview: Option<(String, Option<gui_events::UpscaleOverrides>)> = None;
        let mut existing_upscale: Option<gui_events::UpscaleOverrides> = None;
        let mut selected_from_authority = false;
        let mut visited_releases = std::collections::HashSet::<String>::new();
        let mut current_release: Option<String> = None;
        let mut matches = Vec::<MusicBrainzMatch>::new();
        let mut used_search = false;
        let mut identity_status = "pending".to_string();
        let mut identity_message = String::new();
        let mut recording_status = "pending".to_string();
        let mut artist_status = "pending".to_string();
        let mut friendly_artist = track.artist.clone();
        let mut friendly_title = track.title.clone();
        let mut used_fuzzy_search = false;
        let mut focused_artist_mbid = None::<String>;
        // The same Recording and Artist IDs may offer verified embedded art
        // immediately. MusicBrainz remains available if the operator wants a
        // different release; folder-level cover.* is never borrowed here.
        let needs_musicbrainz = true;

        'match_selection: loop {
            if lookup_pending {
                used_search = true;
                lookup_pending = false;
                let resolution = match_cache
                    .automatic_matches(
                        musicbrainz,
                        config,
                        recording_mbid.as_deref(),
                        &artist_mbids,
                        &search_artist,
                        &track.title,
                    )
                    .await;
                matches = resolution.matches;
                identity_status = resolution.identity_status;
                identity_message = resolution.identity_message;
                recording_status = resolution.recording_status;
                artist_status = resolution.artist_status;
                friendly_artist = resolution.friendly_artist;
                friendly_title = resolution.friendly_title;
                used_fuzzy_search = resolution.used_fuzzy_search;
                focused_artist_mbid = resolution.focused_artist_mbid;
                for warning in resolution.warnings {
                    gui_events::emit(json!({
                        "event": "compilation_lookup_warning", "track_path": track.path,
                        "message": warning
                    }));
                }
                if matches.is_empty() {
                    gui_events::emit(json!({
                        "event": "compilation_lookup_warning",
                        "track_path": track.path,
                        "message": "No matching MusicBrainz release was found. Correct the Recording and Artist IDs with Apply IDs, search manually, or leave the track unchanged."
                    }));
                }
            }
            if needs_musicbrainz && selected_match.is_none() && !use_existing {
                emit_musicbrainz_matches(
                    track,
                    &matches,
                    &visited_releases,
                    current_release.as_deref(),
                    used_search,
                    &authority,
                    MusicBrainzIdentityDisplay {
                        status: &identity_status,
                        message: &identity_message,
                        recording_status: &recording_status,
                        artist_status: &artist_status,
                        friendly_artist: &friendly_artist,
                        friendly_title: &friendly_title,
                        used_fuzzy_search,
                        focused_artist_mbid: focused_artist_mbid.as_deref(),
                        focused_artist: &search_artist,
                        existing_sources: &existing_sources,
                    },
                );
                match gui_events::wait_for_musicbrainz_match_decision()? {
                    gui_events::MusicBrainzMatchDecision::Use(index) if index < matches.len() => {
                        selected_match = Some(matches[index].clone());
                        selected_from_authority = false;
                    }
                    gui_events::MusicBrainzMatchDecision::Use(_) => {
                        return Err("Selected MusicBrainz match does not exist.".to_string());
                    }
                    gui_events::MusicBrainzMatchDecision::UseExisting { index, upscale }
                        if let Some(source) = existing_sources.get(index) =>
                    {
                        existing_source = Some(source.clone());
                        use_existing = true;
                        selected_match = None;
                        existing_upscale = upscale;
                    }
                    gui_events::MusicBrainzMatchDecision::UseExisting { .. } => {
                        return Err("Existing artwork is no longer available.".to_string());
                    }
                    gui_events::MusicBrainzMatchDecision::WritePreview {
                        index,
                        cover_path,
                        upscale,
                    } if index < matches.len() => {
                        selected_match = Some(matches[index].clone());
                        selected_from_authority = false;
                        direct_preview = Some((cover_path, upscale));
                    }
                    gui_events::MusicBrainzMatchDecision::WritePreview { .. } => {
                        return Err(
                            "Selected MusicBrainz preview release does not exist.".to_string()
                        );
                    }
                    gui_events::MusicBrainzMatchDecision::UseAuthority {
                        recording_mbid,
                        artist_mbids,
                        release_mbid,
                    } => {
                        let edited = match match_cache
                            .edited_authority_matches(
                                musicbrainz,
                                &recording_mbid,
                                &artist_mbids,
                                &release_mbid,
                                &search_artist,
                                &track.title,
                            )
                            .await
                        {
                            Ok(resolution) if resolution.identity_status == "valid" => resolution,
                            Ok(resolution) => {
                                gui_events::emit(json!({ "event": "musicbrainz_authority_error",
                                    "message": resolution.identity_message }));
                                continue 'match_selection;
                            }
                            Err(error) => {
                                gui_events::emit(
                                    json!({ "event": "musicbrainz_authority_error", "message": error }),
                                );
                                continue 'match_selection;
                            }
                        };
                        let _ = release_mbid;
                        if let Err(error) = persist_validated_track_identity(
                            config,
                            track,
                            &recording_mbid,
                            &artist_mbids,
                        ) {
                            gui_events::emit(json!({
                                "event": "musicbrainz_authority_error",
                                "message": error
                            }));
                            continue 'match_selection;
                        }
                        identity_status = edited.identity_status;
                        identity_message = edited.identity_message;
                        recording_status = edited.recording_status;
                        artist_status = edited.artist_status;
                        friendly_artist = edited.friendly_artist;
                        friendly_title = edited.friendly_title;
                        used_fuzzy_search = edited.used_fuzzy_search;
                        focused_artist_mbid = edited.focused_artist_mbid;
                        matches = edited.matches;
                        authority.apply(&recording_mbid, &artist_mbids, "", "", "");
                        existing_source = None;
                        existing_sources.clear();
                        selected_match = None;
                        selected_from_authority = false;
                        visited_releases.clear();
                        current_release = None;
                        continue 'match_selection;
                    }
                    gui_events::MusicBrainzMatchDecision::Skip => {
                        gui_events::emit(json!({
                            "event": "album_skipped",
                            "album_path": album.path,
                            "reason": "operator",
                        }));
                        return Ok(result);
                    }
                    gui_events::MusicBrainzMatchDecision::Bypass => {
                        bypass_compilation_album(config, album)?;
                        return Ok(result);
                    }
                    gui_events::MusicBrainzMatchDecision::ClearBypass => {
                        clear_runtime_bypass(config, album)?;
                        continue 'match_selection;
                    }
                    gui_events::MusicBrainzMatchDecision::LeaveUnchanged => {
                        result.unresolved += 1;
                        record_compilation_progress(config, &album.path, tracks.len(), completed)?;
                        gui_events::emit(
                            json!({ "event": "compilation_track_unresolved", "track_path": track.path,
                            "artist": track.artist, "title": track.title, "reason": "Operator left embedded artwork unchanged" }),
                        );
                        break 'match_selection;
                    }
                }
            }

            let mut pipeline = if let Some((cover_path, _)) = direct_preview.as_ref() {
                musicbrainz_preview_pipeline(
                    Path::new(cover_path),
                    selected_match.as_ref().ok_or_else(|| {
                        "The selected MusicBrainz release is no longer available.".to_string()
                    })?,
                )?
            } else if use_existing {
                let (source_track, image_path, kind) =
                    existing_source.as_ref().ok_or_else(|| {
                        "The verified existing artwork source is no longer available.".to_string()
                    })?;
                existing_compilation_pipeline(image_path, source_track, kind)?
            } else {
                let Some(selected) = selected_match.as_ref() else {
                    result.unresolved += 1;
                    gui_events::emit(json!({
                        "event": "compilation_track_unresolved",
                        "track_path": track.path,
                        "artist": track.artist,
                        "title": track.title,
                        "reason": "MusicBrainz selection state ended without a selected release"
                    }));
                    break 'match_selection;
                };
                if let Some(cached) = source_results_cache.get(&selected.release_mbid) {
                    gui_events::emit(json!({ "event": "source_results_cache_hit",
                    "album_path": album.path, "track_path": track.path,
                    "release_mbid": selected.release_mbid,
                    "candidates": cached.candidates.len() }));
                    cached.restore()?
                } else {
                    let discovered = run_registry_pipeline_with_cache_dir(
                        registry,
                        &ArtworkQuery::release(&selected.release_mbid),
                        &ProviderContext {
                            artist_credit: selected.release_artist.clone(),
                            release_title: selected.release_title.clone(),
                            release_group_mbid: selected.release_group_mbid.clone(),
                            release_group_title: Some(selected.release_title.clone()),
                            apple_collection_ids: Vec::new(),
                        },
                        RegistryPipelineOptions {
                            source_order: resolved_sources,
                            range,
                            format_order,
                            source_policies: &config.source_policies,
                        },
                        cache_dir,
                    )
                    .await?;
                    source_results_cache.insert(
                        selected.release_mbid.clone(),
                        CachedPipelineResult::capture(&discovered),
                    );
                    discovered
                }
            };
            for timing in &pipeline.provider_timings {
                gui_events::emit(
                    json!({ "event": "provider_timing", "album_path": album.path, "track_path": track.path,
                    "source": timing.source, "status": timing.status(), "discovery_ms": timing.discovery_ms,
                    "download_ms": timing.download_ms, "references": timing.references, "candidates": timing.candidates,
                    "errors": timing.errors, "error": timing.error }),
                );
            }
            if !use_existing && let Some(embedded_path) = targeted_embedded.as_ref() {
                prepend_embedded_compilation_candidate(&mut pipeline, embedded_path, &track.path)?;
            }
            if pipeline.candidates.is_empty() {
                if let Some(selected) = selected_match.take() {
                    if !selected_from_authority {
                        visited_releases.insert(selected.release_mbid.clone());
                        current_release = Some(selected.release_mbid);
                    }
                    selected_from_authority = false;
                    continue;
                }
                result.unresolved += 1;
                record_compilation_progress(config, &album.path, tracks.len(), completed)?;
                break 'match_selection;
            }
            apply_strict_candidate_policy(&mut pipeline, range, format_order, config);
            let suggested_index = if use_existing {
                Some(0)
            } else {
                pipeline.best_index.or_else(|| {
                    pipeline
                        .candidates
                        .iter()
                        .position(|item| item.strict.preferred_eligible)
                })
            };
            let display_indices = (0..pipeline.candidates.len())
                .filter(|index| {
                    let item = &pipeline.candidates[*index];
                    item.reference
                        .types
                        .iter()
                        .any(|value| value == "EmbeddedTrack" || value == "ExistingEmbedded")
                        || item
                            .reference
                            .types
                            .iter()
                            .any(|value| value == "ExistingCover")
                        || candidate_visible_for_review(&item.downloaded.candidate, range, config)
                })
                .collect::<Vec<_>>();
            let gui_candidates = display_indices.iter().map(|candidate_index| {
                let item = &pipeline.candidates[*candidate_index];
                let candidate = &item.downloaded.candidate;
                let projected = project_configured_artwork(candidate, range, &config.output);
                let policy = configured_candidate_policy(candidate, &projected, range, config);
                json!({ "index": candidate_index + 1, "source": candidate.source, "width": candidate.width,
                    "height": candidate.height, "format": format!("{:?}", candidate.format).to_ascii_lowercase(),
                    "source_range_class": projected_range_class(candidate.short_side(), range),
                    "source_distance_from_ideal": candidate.short_side().abs_diff(range.ideal),
                    "range_class": projected_range_class(projected.width.min(projected.height), range),
                    "distance_from_ideal": projected.width.min(projected.height).abs_diff(range.ideal),
                    "square": candidate.is_square(), "acceptable": policy.status != SourcePolicyStatus::Reject,
                    "policy_status": policy.status, "policy_reason": policy.reason,
                    "source_override_active": active_policy(&config.source_policies, &candidate.source).is_some(),
                    "strict_override_active": config.source_policies.get(&candidate.source.to_ascii_lowercase()).is_some_and(|source_policy| source_policy.strict_override),
                    "strict_status": item.strict.status, "strict_reason": item.strict.reason,
                    "strict_preferred_eligible": item.strict.preferred_eligible,
                    "strict_auto_eligible": item.strict.auto_eligible,
                    "projected_width": projected.width, "projected_height": projected.height,
                    "cropped": projected.cropped, "resized": projected.resized, "upscaled": projected.upscaled,
                    "approved": item.reference.approved, "reference_id": item.reference.id,
                    "local_origin": if item.reference.types.iter().any(|value| value == "ExistingCover") {
                        "artist-album-cover"
                    } else if item.reference.types.iter().any(|value| value == "ExistingEmbedded") {
                        "existing-track"
                    } else if item.reference.types.iter().any(|value| value == "EmbeddedTrack") {
                        "embedded-track"
                    } else if candidate.source == "local" { "cover-file" } else { "" },
                    "local_reference": item.reference.id, "url": item.reference.url,
                    "image_type": candidate_image_category(&item.reference.types),
                    "cache_path": item.downloaded.path(), "recommended": Some(*candidate_index) == suggested_index })
            }).collect::<Vec<_>>();
            gui_events::emit(
                json!({ "event": "candidates", "album_path": album.path, "track_path": track.path,
                "items": gui_candidates, "recommended_index": suggested_index.map(|value| value + 1),
                "hidden_by_source_policy": pipeline.candidates.len() - display_indices.len(), "fallback": false,
                "compilation_track": true, "musicbrainz_back_available": needs_musicbrainz }),
            );
            gui_events::emit(
                json!({ "event": "decision_required", "album_path": album.path, "track_path": track.path,
                "reason": "compilation-track", "suggested_index": suggested_index.map(|value| value + 1),
                "allow_bypass": true, "musicbrainz_back_available": needs_musicbrainz }),
            );

            let writing_mb_preview = direct_preview.is_some();
            let (chosen_index, upscale_overrides, explicit_write) = if use_existing {
                (0, existing_upscale.take(), true)
            } else if let Some((_, upscale)) = direct_preview.take() {
                (0, upscale, true)
            } else {
                loop {
                    match gui_events::wait_for_candidate_decision()? {
                        gui_events::CandidateDecision::Use {
                            index,
                            upscale,
                            explicit_write,
                        } if index < pipeline.candidates.len() => {
                            break (index, upscale, explicit_write);
                        }
                        gui_events::CandidateDecision::BackToMusicBrainz if needs_musicbrainz => {
                            if use_existing {
                                use_existing = false;
                                lookup_pending = true;
                            }
                            if let Some(selected) = selected_match.take()
                                && !selected_from_authority
                            {
                                visited_releases.insert(selected.release_mbid.clone());
                                current_release = Some(selected.release_mbid);
                            }
                            selected_from_authority = false;
                            continue 'match_selection;
                        }
                        gui_events::CandidateDecision::Skip => {
                            gui_events::emit(json!({
                                "event": "album_skipped",
                                "album_path": album.path,
                                "reason": "operator",
                            }));
                            return Ok(result);
                        }
                        gui_events::CandidateDecision::Bypass => {
                            bypass_compilation_album(config, album)?;
                            return Ok(result);
                        }
                        gui_events::CandidateDecision::ClearBypass => {
                            clear_runtime_bypass(config, album)?;
                        }
                        _ => return Err("Invalid compilation artwork decision.".to_string()),
                    }
                }
            };
            let chosen = pipeline.candidates.swap_remove(chosen_index);
            let effective_mode = if explicit_write {
                Mode::Write
            } else {
                config.mode
            };
            let mut final_config = (*config).clone();
            final_config.mode = effective_mode;
            result.selected += 1;
            let target_format = match chosen.downloaded.candidate.format {
                StaticFormat::Png => StaticFormat::Png,
                _ => StaticFormat::Jpeg,
            };
            let embedding_range = Range {
                min: config.range.min.min(config.range.ladder),
                ideal: config.range.ladder,
                max: config.range.ladder,
                ladder: config.range.ladder,
            };
            let mut embedding_output = config.output.clone();
            embedding_output.upscale_below_ideal = false;
            if let Some(overrides) = upscale_overrides.as_ref() {
                apply_upscale_overrides(&mut embedding_output, overrides)?;
                if (writing_mb_preview || use_existing)
                    && !overrides.preserve_source_resolution
                    && overrides.apply_edit_profile
                {
                    embedding_output.upscale_below_ideal = true;
                }
            }
            let prepared = prepare_configured_artwork(
                &chosen.downloaded.candidate,
                chosen.downloaded.path(),
                &embedding_range,
                target_format,
                &embedding_output,
                true,
            )?;
            let embedded_bytes = if upscale_overrides
                .as_ref()
                .is_some_and(|overrides| overrides.embed_thumbnails)
            {
                prepare_embedded_thumbnail(
                    &prepared.bytes,
                    embedding_output.embedded_thumbnail_size,
                    target_format,
                )?
            } else {
                prepared.bytes.clone()
            };
            let selected_release = selected_match
                .as_ref()
                .map(|item| item.release_mbid.as_str());
            // Apply IDs is durable authority. Keep the completion ledger in
            // lockstep with the safely written track tags and splined.db.
            let track_recording = authority.recording_mbid.clone().unwrap_or_default();
            let track_artists = authority.artist_mbids.join(",");
            let already_has_artwork = effective_mode == Mode::Write
                && embedded_front_matches(&track.path, &embedded_bytes).unwrap_or(false);
            if effective_mode == Mode::Write {
                if !already_has_artwork {
                    replace_embedded_front(&track.path, &embedded_bytes)?;
                }
                completed += 1;
                record_compilation_artwork_application(
                    &final_config,
                    CompilationArtworkApplication {
                        album_path: &album.path,
                        track_path: &track.path,
                        recording_mbid: &track_recording,
                        artist_mbids_key: &track_artists,
                        source_kind: &chosen.downloaded.candidate.source,
                        source_locator: &chosen.reference.url,
                        release_mbid: selected_release,
                        artwork: &embedded_bytes,
                        outcome: if already_has_artwork {
                            "embedded-existing"
                        } else {
                            "embedded-replaced"
                        },
                        total_tracks: tracks.len(),
                        completed_tracks: completed,
                    },
                )?;
                result.installed += 1;
            } else {
                result.read_only += 1;
            }
            gui_events::emit(
                json!({ "event": "compilation_track_completed", "album_path": album.path,
                "track_path": track.path, "artist": track.artist, "title": track.title,
                "action": if effective_mode == Mode::Write {
                    if already_has_artwork { "EmbeddedExisting" } else { "EmbeddedReplaced" }
                } else { "ReadOnly" },
                "mode": format!("{:?}", effective_mode).to_ascii_lowercase(),
                "source": chosen.downloaded.candidate.source, "release_mbid": selected_release,
                "width": if upscale_overrides.as_ref().is_some_and(|overrides| overrides.embed_thumbnails) { embedding_output.embedded_thumbnail_size } else { prepared.info.width },
                "height": if upscale_overrides.as_ref().is_some_and(|overrides| overrides.embed_thumbnails) { embedding_output.embedded_thumbnail_size } else { prepared.info.height },
                "upscale_backend": prepared.info.upscale_backend.as_str() }),
            );
            break 'match_selection;
        }
    }

    record_compilation_progress(config, &album.path, tracks.len(), completed)?;
    if !targeted_edit && !gui_events::cancelled() {
        record_compilation_resume_track(config, &album.path, None)?;
    }
    result.failed = gui_events::cancelled();
    gui_events::emit(
        json!({ "event": "album_completed", "album_path": album.path,
        "destination": format!("{completed}/{} embedded track artwork", tracks.len()),
        "action": if completed == tracks.len() { "CompilationComplete" } else { "CompilationIncomplete" },
        "mode": format!("{:?}", config.mode).to_ascii_lowercase(),
        "unresolved_tracks": tracks.len().saturating_sub(completed) }),
    );
    Ok(result)
}

fn candidate_image_category(types: &[String]) -> &'static str {
    if types.iter().any(|value| value.eq_ignore_ascii_case("Back")) {
        "BackCover"
    } else if types.iter().any(|value| {
        value.eq_ignore_ascii_case("Artist") || value.eq_ignore_ascii_case("ArtistImage")
    }) {
        "ArtistImage"
    } else if types.iter().any(|value| {
        value.eq_ignore_ascii_case("Media")
            || value.eq_ignore_ascii_case("Booklet")
            || value.eq_ignore_ascii_case("Medium")
    }) {
        "MediaBooklet"
    } else if types
        .iter()
        .any(|value| value.eq_ignore_ascii_case("Front"))
    {
        "FrontCover"
    } else {
        "Other"
    }
}

fn compilation_track_is_already_complete(
    track_path: &Path,
    completed_paths: &std::collections::HashSet<PathBuf>,
    targeted_edit: bool,
) -> bool {
    !targeted_edit && completed_paths.contains(track_path)
}

fn compilation_resume_start_index(
    tracks: &[LocalTrackEvidence],
    resume_track: Option<&Path>,
) -> usize {
    let Some(resume_track) = resume_track else {
        return 0;
    };
    tracks
        .iter()
        .position(|track| {
            track
                .path
                .to_string_lossy()
                .eq_ignore_ascii_case(&resume_track.to_string_lossy())
        })
        .unwrap_or(0)
}

struct MusicBrainzIdentityDisplay<'a> {
    status: &'a str,
    message: &'a str,
    recording_status: &'a str,
    artist_status: &'a str,
    friendly_artist: &'a str,
    friendly_title: &'a str,
    used_fuzzy_search: bool,
    focused_artist_mbid: Option<&'a str>,
    focused_artist: &'a str,
    existing_sources: &'a [(PathBuf, PathBuf, &'static str)],
}

fn emit_musicbrainz_matches(
    track: &LocalTrackEvidence,
    matches: &[MusicBrainzMatch],
    visited_releases: &std::collections::HashSet<String>,
    current_release: Option<&str>,
    searched: bool,
    authority: &MusicBrainzSessionAuthority,
    identity: MusicBrainzIdentityDisplay<'_>,
) {
    let items = matches.iter().enumerate().map(|(index, item)| json!({
        "index": index + 1, "recording_mbid": item.recording_mbid, "recording_title": item.recording_title,
        "recording_artist": item.recording_artist, "artist_mbids": item.artist_mbids,
        "release_mbid": item.release_mbid, "release_group_mbid": item.release_group_mbid,
        "release_group_title": item.release_group_title,
        "release_class": item.release_class, "release_title": item.release_title,
        "release_artist": item.release_artist, "release_date": item.release_date,
        "country": item.country, "score": item.score, "url": item.url,
        "visited": visited_releases.contains(&item.release_mbid),
        "current": current_release.is_some_and(|value| value.eq_ignore_ascii_case(&item.release_mbid)),
    })).collect::<Vec<_>>();
    gui_events::emit(
        json!({ "event": "musicbrainz_matches", "track_path": track.path,
        "artist": track.artist, "title": track.title, "searched": searched,
        "focused_artist": identity.focused_artist,
        "album_artist": track.album_artist.as_deref().unwrap_or(&track.artist),
        "album": track.album.as_deref().unwrap_or(""),
        "authority_recording_mbid": authority.recording_mbid.as_deref().unwrap_or(""),
        "authority_artist_mbids": authority.artist_mbids.join(", "),
        "authority_release_mbid": authority.release_mbid,
        "authority_release_title": authority.release_title,
        "authority_release_artist": authority.release_artist,
        "identity_status": identity.status,
        "identity_message": identity.message,
        "recording_status": identity.recording_status,
        "artist_status": identity.artist_status,
        "focused_artist_mbid": identity.focused_artist_mbid.unwrap_or(""),
        "friendly_artist": identity.friendly_artist,
        "friendly_title": identity.friendly_title,
        "used_fuzzy_search": identity.used_fuzzy_search,
        "compilation_track": true,
        "existing_artwork_path": identity.existing_sources.first().map(|(_, cover, _)| cover.to_string_lossy().into_owned()).unwrap_or_default(),
        "existing_artwork_kind": identity.existing_sources.first().map(|(_, _, kind)| *kind).unwrap_or(""),
        "existing_artwork_title": identity.existing_sources.first().and_then(|(track, _, _)| track.parent()).and_then(|folder| folder.file_name()).and_then(|name| name.to_str()).unwrap_or(""),
        "existing_artwork_items": identity.existing_sources.iter().map(|(track, cover, kind)| json!({
            "track_path": track, "artwork_path": cover, "kind": kind,
            "title": track.parent().and_then(|folder| folder.file_name()).and_then(|name| name.to_str()).unwrap_or("")
        })).collect::<Vec<_>>(),
        "notice": if matches.is_empty() && identity.status != "valid" {
            "No matching release was found. Correct the Recording and Artist IDs with Apply IDs, search manually, or leave the track unchanged."
        } else {
            ""
        },
        "items": items }),
    );
}

struct FinalizationOptions<'a> {
    preserve_file: bool,
    explicit_manual_selection: bool,
    apply_edit_profile: bool,
    edit_existing_cover: bool,
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
    let prepared = if options.edit_existing_cover {
        prepare_existing_cover_edit(
            candidate,
            source_path,
            range,
            target_format,
            options.output,
            options.explicit_manual_selection,
        )?
    } else if options.apply_edit_profile {
        prepare_selected_artwork_edit(
            candidate,
            source_path,
            range,
            target_format,
            options.output,
            options.explicit_manual_selection,
        )?
    } else {
        prepare_configured_artwork(
            candidate,
            source_path,
            range,
            target_format,
            options.output,
            options.explicit_manual_selection,
        )?
    };

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

fn remove_completed_album_samples(
    sample_root: &Path,
    artist: &str,
    album: &str,
) -> Result<bool, String> {
    let album_dir = sample_root
        .join(sanitize_sample_component(artist))
        .join(sanitize_sample_component(album));
    if !album_dir.exists() {
        return Ok(false);
    }
    let verified_root = sample_root.canonicalize().map_err(|error| {
        format!(
            "Unable to verify the configured Samples root {}: {error}",
            sample_root.display()
        )
    })?;
    let verified_album = album_dir.canonicalize().map_err(|error| {
        format!(
            "Unable to verify the completed album Samples directory {}: {error}",
            album_dir.display()
        )
    })?;
    if verified_album == verified_root || !verified_album.starts_with(&verified_root) {
        return Err(
            "Refusing to remove a completed Samples directory outside the configured root."
                .to_string(),
        );
    }
    let artist_dir = verified_album.parent().map(Path::to_path_buf);
    fs::remove_dir_all(&verified_album).map_err(|error| {
        format!(
            "Unable to remove completed album Samples directory {}: {error}",
            verified_album.display()
        )
    })?;
    if let Some(artist_dir) = artist_dir
        && artist_dir != verified_root
    {
        let _ = fs::remove_dir(&artist_dir);
    }
    Ok(true)
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

fn prepend_embedded_compilation_candidate(
    result: &mut PipelineResult,
    embedded_path: &Path,
    track_path: &Path,
) -> Result<(), String> {
    let downloaded = DownloadedCandidate::from_existing_path(
        "local",
        embedded_path.to_path_buf(),
        0,
        track_path.to_string_lossy(),
    )?;
    if let Some(best_index) = result.best_index.as_mut() {
        *best_index += 1;
    }
    result.candidates.insert(
        0,
        PipelineCandidate {
            reference: ArtworkReference {
                source: "local".to_string(),
                id: track_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("embedded track")
                    .to_string(),
                url: track_path.to_string_lossy().into_owned(),
                front: true,
                approved: true,
                types: vec!["EmbeddedTrack".to_string()],
            },
            downloaded,
            strict: StrictContentDecision::default(),
        },
    );
    Ok(())
}

fn existing_compilation_pipeline(
    image_path: &Path,
    source_track: &Path,
    kind: &str,
) -> Result<PipelineResult, String> {
    let downloaded = DownloadedCandidate::from_existing_path(
        "existing",
        image_path.to_path_buf(),
        0,
        source_track.to_string_lossy(),
    )?;
    Ok(PipelineResult {
        candidates: vec![PipelineCandidate {
            reference: ArtworkReference {
                source: "existing".to_string(),
                id: source_track
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("matching recording")
                    .to_string(),
                url: source_track.to_string_lossy().into_owned(),
                front: true,
                approved: true,
                types: vec![
                    if kind.starts_with("artist-album-cover") {
                        "ExistingCover"
                    } else {
                        "ExistingEmbedded"
                    }
                    .to_string(),
                ],
            },
            downloaded,
            strict: StrictContentDecision::default(),
        }],
        best_index: Some(0),
        diagnostics: Vec::new(),
        provider_timings: Vec::new(),
    })
}

fn musicbrainz_preview_pipeline(
    image_path: &Path,
    selected: &MusicBrainzMatch,
) -> Result<PipelineResult, String> {
    if !image_path.is_file() {
        return Err("The selected MusicBrainz cover preview is no longer available.".to_string());
    }
    let downloaded = DownloadedCandidate::from_existing_path(
        "coverartarchive",
        image_path.to_path_buf(),
        0,
        selected.url.clone(),
    )?;
    Ok(PipelineResult {
        candidates: vec![PipelineCandidate {
            reference: ArtworkReference {
                source: "coverartarchive".to_string(),
                id: selected.release_mbid.clone(),
                url: selected.url.clone(),
                front: true,
                approved: true,
                types: vec!["Front".to_string()],
            },
            downloaded,
            strict: StrictContentDecision::default(),
        }],
        best_index: Some(0),
        diagnostics: Vec::new(),
        provider_timings: Vec::new(),
    })
}

fn apply_strict_candidate_policy(
    result: &mut PipelineResult,
    range: &Range,
    format_order: &[StaticFormat],
    config: &Config,
) {
    apply_strict_source_policy(&mut result.candidates, &config.source_policies);
    let eligible_indices = result
        .candidates
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            (item.strict.preferred_eligible
                && candidate_quality_eligible_for_preferred(item, range, config))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let eligible = eligible_indices
        .iter()
        .map(|index| result.candidates[*index].downloaded.candidate.clone())
        .collect::<Vec<_>>();
    result.best_index =
        best_candidate_index(&eligible, range, format_order, &config.source_policies)
            .map(|index| eligible_indices[index]);
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

fn should_offer_existing_cover_for_review(
    action: LocalPreflightAction,
    review_required: bool,
    auto_ideal_enabled: bool,
) -> bool {
    review_required && !auto_ideal_enabled && action == LocalPreflightAction::LocalIdeal
}

fn candidate_is_auto_ideal(candidate: &Candidate, range: &Range, config: &Config) -> bool {
    let projected = project_configured_artwork(candidate, range, &config.output);
    let policy = configured_candidate_policy(candidate, &projected, range, config);
    policy.status == SourcePolicyStatus::Accept
        && candidate_meets_upscale_limit(candidate, range, config)
        && range.classify(projected.width.min(projected.height)) == RangeClass::Ideal
}

fn candidate_meets_upscale_limit(candidate: &Candidate, range: &Range, config: &Config) -> bool {
    candidate.short_side() >= range.ideal
        || !config.output.upscale_below_ideal
        || (candidate.short_side() > 0
            && u64::from(range.ideal) * 100
                <= u64::from(candidate.short_side()) * u64::from(config.output.upscale_max_percent))
}

fn apply_upscale_overrides(
    output: &mut OutputConfig,
    overrides: &gui_events::UpscaleOverrides,
) -> Result<(), String> {
    if overrides.preserve_source_resolution {
        output.evaluate_final_image = false;
        output.upscale_below_ideal = false;
        output.upscale_adaptive_defaults = false;
        output.upscale_picture_percent = 0;
        output.upscale_sharpen_percent = 0;
        output.upscale_softness_percent = 0;
        output.upscale_contrast_percent = 0;
        output.upscale_exposure_percent = 0;
        output.upscale_brightness_percent = 0;
        output.upscale_gamma_percent = 0;
        output.upscale_color_temperature = 0;
        return Ok(());
    }
    for (name, value) in [
        ("sharpen", overrides.sharpen_percent),
        ("softness", overrides.softness_percent),
    ] {
        if !(0..=20).contains(&value) {
            return Err(format!("Upscale {name} override must be between 0 and 20."));
        }
    }
    for (name, value) in [
        ("picture", overrides.picture_percent),
        ("contrast", overrides.contrast_percent),
        ("exposure", overrides.exposure_percent),
        ("brightness", overrides.brightness_percent),
        ("gamma", overrides.gamma_percent),
    ] {
        if !(-20..=20).contains(&value) {
            return Err(format!(
                "Upscale {name} override must be between -20 and 20."
            ));
        }
    }
    if !(-100..=100).contains(&overrides.color_temperature) {
        return Err("Upscale color correction override must be between -100 and 100.".to_string());
    }
    output.upscale_adaptive_defaults = overrides.adaptive_defaults;
    output.upscale_picture_percent = overrides.picture_percent;
    output.upscale_sharpen_percent = overrides.sharpen_percent;
    output.upscale_softness_percent = overrides.softness_percent;
    output.upscale_contrast_percent = overrides.contrast_percent;
    output.upscale_exposure_percent = overrides.exposure_percent;
    output.upscale_brightness_percent = overrides.brightness_percent;
    output.upscale_gamma_percent = overrides.gamma_percent;
    output.upscale_color_temperature = overrides.color_temperature;
    Ok(())
}

fn candidate_quality_eligible_for_preferred(
    item: &PipelineCandidate,
    range: &Range,
    config: &Config,
) -> bool {
    let projected = project_configured_artwork(&item.downloaded.candidate, range, &config.output);
    if !projected.upscaled {
        return true;
    }
    assess_artwork_quality(item.downloaded.path())
        .map(|quality| quality.automatic_eligible)
        .unwrap_or(false)
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
    let bridge = gui_events::scan_context();
    let release_title = bridge
        .fallback_album
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
    let artist_credit = bridge
        .fallback_artist
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
        apple_collection_ids: Vec::new(),
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

fn record_runtime_completion(
    config: &Config,
    album: &AlbumDirectory,
    outcome: &str,
    selected_source: Option<&str>,
    material: Option<RuntimeArtworkMaterial>,
    post_cover_started: Instant,
    final_logged_started: Instant,
) -> Result<(), String> {
    let pre_persistence_ms = elapsed_ms(post_cover_started);
    let bridge = gui_events::scan_context();
    let indexed_album_path = bridge.indexed_album_path;
    let indexed_album_key = bridge.indexed_album_key;
    let timing = record_album_outcome_from_runtime(
        config,
        &album.path,
        indexed_album_path.as_deref(),
        indexed_album_key.as_deref(),
        outcome,
        selected_source,
        material.as_ref(),
    )?;
    gui_events::emit(json!({
        "event": "post_cover_timing",
        "album_path": album.path,
        "pre_persistence_ms": pre_persistence_ms,
        "database_ms": timing.database_ms,
        "filesystem_ms": timing.filesystem_ms,
        "identity_ms": timing.identity_ms,
        "update_ms": timing.update_ms,
        "aggregate_ms": timing.aggregate_ms,
        "audit_ms": timing.audit_ms,
        "commit_ms": timing.commit_ms,
        "persistence_ms": timing.total_ms,
        "post_cover_total_ms": elapsed_ms(post_cover_started),
        "final_to_album_completed_ms": elapsed_ms(final_logged_started),
    }));
    Ok(())
}

fn clear_runtime_bypass(config: &Config, album: &AlbumDirectory) -> Result<(), String> {
    let bridge = gui_events::scan_context();
    let removed = clear_album_bypass_from_runtime(
        config,
        &album.path,
        bridge.indexed_album_path.as_deref(),
        bridge.indexed_album_key.as_deref(),
    )?;
    gui_events::emit(json!({
        "event": "album_bypass_cleared",
        "album_path": album.path,
        "removed": removed,
    }));
    Ok(())
}

fn bypass_compilation_album(config: &Config, album: &AlbumDirectory) -> Result<(), String> {
    let now = Instant::now();
    record_runtime_completion(config, album, "compilation-bypassed", None, None, now, now)?;
    gui_events::emit(json!({
        "event": "album_completed",
        "album_path": album.path,
        "destination": "",
        "action": "Bypassed",
        "mode": format!("{:?}", config.mode).to_ascii_lowercase(),
    }));
    Ok(())
}

fn runtime_artwork_material(
    destination: &Path,
    info: Option<PreparedArtworkInfo>,
) -> Option<RuntimeArtworkMaterial> {
    info.map(|info| RuntimeArtworkMaterial {
        path: destination.to_path_buf(),
        format: format!("{:?}", info.format).to_ascii_uppercase(),
        width: info.width,
        height: info.height,
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
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
    fn matching_embedded_recording_is_offered_as_existing_candidate() {
        let directory = TempDir::new().unwrap();
        let image_path = directory.path().join("art.png");
        let source_track = directory.path().join("source.mp3");
        image::RgbImage::from_pixel(100, 100, image::Rgb([20, 80, 140]))
            .save_with_format(&image_path, image::ImageFormat::Png)
            .unwrap();
        let result =
            existing_compilation_pipeline(&image_path, &source_track, "embedded-track").unwrap();
        assert_eq!(result.best_index, Some(0));
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].downloaded.candidate.source, "existing");
        assert_eq!(result.candidates[0].reference.types, ["ExistingEmbedded"]);
        assert_eq!(
            result.candidates[0].reference.url,
            source_track.to_string_lossy()
        );
    }

    #[test]
    fn artist_album_cover_is_an_existing_candidate_and_mb_preview_uses_selected_pixels() {
        let directory = TempDir::new().unwrap();
        let image_path = directory.path().join("cover.png");
        let source_track = directory.path().join("source.mp3");
        image::RgbImage::from_pixel(100, 100, image::Rgb([20, 80, 140]))
            .save_with_format(&image_path, image::ImageFormat::Png)
            .unwrap();
        let existing =
            existing_compilation_pipeline(&image_path, &source_track, "artist-album-cover")
                .unwrap();
        assert_eq!(existing.candidates[0].reference.types, ["ExistingCover"]);
        assert_eq!(
            existing.candidates[0].downloaded.candidate.source,
            "existing"
        );
        let selected = MusicBrainzMatch {
            recording_mbid: "recording-id".to_string(),
            recording_title: "Song".to_string(),
            recording_artist: "Artist".to_string(),
            artist_mbids: vec!["artist-id".to_string()],
            release_mbid: "release-id".to_string(),
            release_group_mbid: None,
            release_group_title: None,
            release_class: crate::compilation::ReleaseClass::Album,
            release_title: "Album".to_string(),
            release_artist: "Artist".to_string(),
            release_date: None,
            country: None,
            score: 100,
            url: "https://musicbrainz.org/release/release-id".to_string(),
        };
        let direct = musicbrainz_preview_pipeline(&image_path, &selected).unwrap();
        assert_eq!(direct.candidates.len(), 1);
        assert_eq!(direct.candidates[0].downloaded.path(), image_path.as_path());
        assert_eq!(
            direct.candidates[0].downloaded.candidate.source,
            "coverartarchive"
        );
    }

    #[test]
    fn artist_album_review_offers_its_verified_local_cover_as_existing() {
        let directory = TempDir::new().unwrap();
        let album_path = directory
            .path()
            .join("Mariah Carey")
            .join("Merry Christmas");
        fs::create_dir_all(&album_path).unwrap();
        let cover = album_path.join("cover.png");
        image::RgbImage::from_pixel(100, 100, image::Rgb([20, 80, 140]))
            .save_with_format(&cover, image::ImageFormat::Png)
            .unwrap();
        let track_path = album_path.join("01.mp3");
        let track = LocalTrackEvidence {
            path: track_path.clone(),
            title: "All I Want for Christmas Is You".to_string(),
            artist: "Mariah Carey".to_string(),
            album: Some("Merry Christmas".to_string()),
            album_artist: Some("Mariah Carey".to_string()),
            musicbrainz_album_id: Some("11111111-1111-4111-8111-111111111111".to_string()),
            musicbrainz_track_id: Some("22222222-2222-4222-8222-222222222222".to_string()),
            musicbrainz_artist_id: Some("33333333-3333-4333-8333-333333333333".to_string()),
            compilation: None,
        };
        let local =
            existing_compilation_pipeline(&cover, &track_path, "artist-album-cover").unwrap();
        let source = normal_album_existing_artwork(
            &Config::default(),
            &AlbumDirectory {
                path: album_path,
                audio_files: vec![track_path.clone()],
            },
            std::slice::from_ref(&track),
            &tagged_album_id_audit(std::slice::from_ref(&track)),
            local.candidates.first(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(source.0, track_path);
        assert_eq!(source.1, cover);

        let remix = LocalTrackEvidence {
            title: "All I Want for Christmas Is You (Remix)".to_string(),
            ..track
        };
        assert!(
            normal_album_existing_artwork(
                &Config::default(),
                &AlbumDirectory {
                    path: directory.path().to_path_buf(),
                    audio_files: Vec::new(),
                },
                std::slice::from_ref(&remix),
                &tagged_album_id_audit(std::slice::from_ref(&remix)),
                local.candidates.first(),
            )
            .unwrap()
            .is_some()
        );
    }

    #[test]
    fn multi_credit_fallback_uses_focused_artist_without_discarding_primary_credit() {
        assert_eq!(
            focused_track_artist_name("Jason Derulo\x002 Chainz", Some("2 Chainz")),
            "2 Chainz"
        );
        assert_eq!(
            focused_track_artist_name("Jason Derulo\x002 Chainz", Some("Jason Derulo")),
            "Jason Derulo"
        );
        assert_eq!(
            focused_track_artist_name("Jason Derulo\x002 Chainz", Some("Other")),
            "Jason Derulo"
        );
    }

    #[test]
    fn candidate_image_categories_preserve_filterable_provider_evidence() {
        assert_eq!(
            candidate_image_category(&["Front".to_string()]),
            "FrontCover"
        );
        assert_eq!(candidate_image_category(&["Back".to_string()]), "BackCover");
        assert_eq!(
            candidate_image_category(&["ArtistImage".to_string()]),
            "ArtistImage"
        );
        assert_eq!(
            candidate_image_category(&["Booklet".to_string()]),
            "MediaBooklet"
        );
        assert_eq!(candidate_image_category(&[]), "Other");
    }

    #[test]
    fn completed_album_sample_cleanup_removes_album_and_empty_artist_directories() {
        let directory = TempDir::new().unwrap();
        let album = directory.path().join("Bob Dylan").join("Blonde on Blonde");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("itunes.sample.jpg"), b"sample").unwrap();

        assert!(
            remove_completed_album_samples(directory.path(), "Bob Dylan", "Blonde on Blonde")
                .unwrap()
        );
        assert!(!album.exists());
        assert!(!directory.path().join("Bob Dylan").exists());
        assert!(!remove_completed_album_samples(directory.path(), "Bob Dylan", "Missing").unwrap());
    }

    #[test]
    fn saved_samples_join_the_runtime_candidate_authority() {
        let directory = TempDir::new().unwrap();
        let album = directory.path().join("Bob Dylan").join("Blonde on Blonde");
        fs::create_dir_all(&album).unwrap();
        for provider in ["itunes", "discogs"] {
            let path = album.join(format!("{provider}.sample.png"));
            image::RgbaImage::from_pixel(12, 12, image::Rgba([40, 90, 150, 255]))
                .save_with_format(path, image::ImageFormat::Png)
                .unwrap();
        }
        fs::write(album.join("ignore.txt"), b"not artwork").unwrap();
        let mut result = PipelineResult {
            candidates: Vec::new(),
            best_index: None,
            diagnostics: Vec::new(),
            provider_timings: Vec::new(),
        };

        assert_eq!(
            append_saved_sample_candidates(
                &mut result,
                directory.path(),
                "Bob Dylan",
                "Blonde on Blonde"
            )
            .unwrap(),
            2
        );
        assert_eq!(result.candidates.len(), 2);
        assert!(
            result
                .candidates
                .iter()
                .all(|candidate| candidate.downloaded.candidate.source == "samples")
        );
        assert!(
            result
                .candidates
                .iter()
                .all(|candidate| !candidate.strict.preferred_eligible)
        );
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|candidate| candidate.reference.id.as_str())
                .collect::<Vec<_>>(),
            vec!["discogs", "itunes"]
        );
    }

    #[test]
    fn manual_gui_review_keeps_existing_ideal_cover_in_candidate_results() {
        assert!(should_offer_existing_cover_for_review(
            LocalPreflightAction::LocalIdeal,
            true,
            false,
        ));
        assert!(!should_offer_existing_cover_for_review(
            LocalPreflightAction::EmbeddedIdeal,
            true,
            false,
        ));
        assert!(!should_offer_existing_cover_for_review(
            LocalPreflightAction::LocalIdeal,
            true,
            true,
        ));
        assert!(!should_offer_existing_cover_for_review(
            LocalPreflightAction::LocalIdeal,
            false,
            false,
        ));
    }

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
    fn samples_directory_is_preserved_between_runs() {
        let dir = TempDir::new().unwrap();
        let samples = dir.path().join("samples");
        fs::create_dir_all(&samples).unwrap();
        fs::write(samples.join("old.sample.jpg"), b"old").unwrap();
        fs::write(dir.path().join("unrelated.cache"), b"keep").unwrap();

        prepare_samples_dir(&samples).unwrap();
        assert_eq!(fs::read_dir(&samples).unwrap().count(), 1);
        assert!(samples.join("old.sample.jpg").exists());
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

    #[test]
    fn unattended_selection_accepts_only_policy_accepted_ideal_candidates() {
        let range = Range::default();
        let config = Config::default();
        let lower = Candidate {
            source: "itunes".to_string(),
            width: range.ideal - 1,
            height: range.ideal - 1,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        let ideal = Candidate {
            width: range.ideal,
            height: range.ideal,
            ..lower.clone()
        };

        assert!(!candidate_is_auto_ideal(&lower, &range, &config));
        assert!(candidate_is_auto_ideal(&ideal, &range, &config));
    }

    #[test]
    fn upscaling_promotes_only_sources_within_the_configured_maximum() {
        let range = Range::default();
        let mut config = Config::default();
        config.output.upscale_below_ideal = true;
        config.output.upscale_max_percent = 200;
        let within_limit = Candidate {
            source: "itunes".to_string(),
            width: range.ideal.div_ceil(2),
            height: range.ideal.div_ceil(2),
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        let exceeds_limit = Candidate {
            width: within_limit.width - 1,
            height: within_limit.height - 1,
            ..within_limit.clone()
        };

        assert!(candidate_meets_upscale_limit(
            &within_limit,
            &range,
            &config
        ));
        assert!(candidate_is_auto_ideal(&within_limit, &range, &config));
        assert!(!candidate_meets_upscale_limit(
            &exceeds_limit,
            &range,
            &config
        ));
        assert!(!candidate_is_auto_ideal(&exceeds_limit, &range, &config));
    }

    #[test]
    fn candidate_decision_profile_overrides_the_current_album_output() {
        let mut output = OutputConfig::default();
        let overrides = gui_events::UpscaleOverrides {
            preserve_source_resolution: false,
            adaptive_defaults: false,
            picture_percent: 6,
            sharpen_percent: 4,
            softness_percent: 2,
            contrast_percent: 7,
            exposure_percent: -3,
            brightness_percent: 5,
            gamma_percent: -2,
            color_temperature: -25,
            apply_edit_profile: true,
            edit_existing_cover: true,
            embed_thumbnails: false,
        };
        apply_upscale_overrides(&mut output, &overrides).unwrap();
        assert!(!output.upscale_adaptive_defaults);
        assert_eq!(output.upscale_picture_percent, 6);
        assert_eq!(output.upscale_sharpen_percent, 4);
        assert_eq!(output.upscale_softness_percent, 2);
        assert_eq!(output.upscale_contrast_percent, 7);
        assert_eq!(output.upscale_exposure_percent, -3);
        assert_eq!(output.upscale_brightness_percent, 5);
        assert_eq!(output.upscale_gamma_percent, -2);
        assert_eq!(output.upscale_color_temperature, -25);

        let invalid = gui_events::UpscaleOverrides {
            sharpen_percent: 21,
            ..overrides
        };
        assert!(apply_upscale_overrides(&mut output, &invalid).is_err());
    }

    #[test]
    fn preview_only_selection_preserves_source_resolution_and_disables_editing() {
        let mut output = OutputConfig {
            evaluate_final_image: true,
            upscale_below_ideal: true,
            upscale_picture_percent: 12,
            upscale_sharpen_percent: 8,
            ..OutputConfig::default()
        };
        let overrides = gui_events::UpscaleOverrides {
            preserve_source_resolution: true,
            adaptive_defaults: true,
            picture_percent: 12,
            sharpen_percent: 8,
            softness_percent: 4,
            contrast_percent: 3,
            exposure_percent: 2,
            brightness_percent: 1,
            gamma_percent: -1,
            color_temperature: 20,
            apply_edit_profile: false,
            edit_existing_cover: false,
            embed_thumbnails: false,
        };
        apply_upscale_overrides(&mut output, &overrides).unwrap();
        assert!(!output.evaluate_final_image);
        assert!(!output.upscale_below_ideal);
        assert!(!output.upscale_adaptive_defaults);
        assert_eq!(output.upscale_picture_percent, 0);
        assert_eq!(output.upscale_sharpen_percent, 0);
        assert_eq!(output.upscale_color_temperature, 0);
    }

    #[test]
    fn applying_ids_updates_only_the_musicbrainz_authority() {
        let ordinary_results = vec!["ordinary-release-a", "ordinary-release-b"];
        let mut authority = MusicBrainzSessionAuthority {
            recording_mbid: Some("old-recording".into()),
            artist_mbids: vec!["old-artist".into()],
            release_mbid: "old-release".into(),
            release_title: "Old Album".into(),
            release_artist: "Old Artist".into(),
        };
        authority.apply(
            "NEW-RECORDING",
            "NEW-ARTIST",
            "NEW-RELEASE",
            "Applied Album",
            "Applied Artist",
        );
        assert_eq!(authority.recording_mbid.as_deref(), Some("new-recording"));
        assert_eq!(authority.artist_mbids, ["new-artist"]);
        assert_eq!(authority.release_mbid, "new-release");
        assert_eq!(authority.release_title, "Applied Album");
        assert_eq!(authority.release_artist, "Applied Artist");
        assert_eq!(
            ordinary_results,
            ["ordinary-release-a", "ordinary-release-b"]
        );
    }

    #[test]
    fn compilation_resume_skips_completed_tracks_but_targeted_edit_reopens_them() {
        let completed_track = PathBuf::from("album/01-complete.mp3");
        let unfinished_track = PathBuf::from("album/02-unfinished.mp3");
        let completed = std::collections::HashSet::from([completed_track.clone()]);

        assert!(compilation_track_is_already_complete(
            &completed_track,
            &completed,
            false,
        ));
        assert!(!compilation_track_is_already_complete(
            &unfinished_track,
            &completed,
            false,
        ));
        assert!(!compilation_track_is_already_complete(
            &completed_track,
            &completed,
            true,
        ));
    }

    #[test]
    fn compilation_resume_cursor_restarts_at_the_interrupted_track() {
        let tracks = ["01.mp3", "02.mp3", "03.mp3", "04.mp3"]
            .into_iter()
            .map(|name| LocalTrackEvidence {
                path: PathBuf::from("album").join(name),
                title: String::new(),
                artist: String::new(),
                album: None,
                album_artist: None,
                musicbrainz_album_id: None,
                musicbrainz_track_id: None,
                musicbrainz_artist_id: None,
                compilation: None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            compilation_resume_start_index(
                &tracks,
                Some(PathBuf::from("album").join("03.mp3").as_path()),
            ),
            2
        );
        assert_eq!(
            compilation_resume_start_index(
                &tracks,
                Some(PathBuf::from("album").join("missing.mp3").as_path()),
            ),
            0
        );
    }

    #[test]
    fn compilation_resume_cursor_and_completion_ledger_advance_to_track_52() {
        let tracks = (1..=100)
            .map(|index| LocalTrackEvidence {
                path: PathBuf::from("1961").join(format!("{index:02}.mp3")),
                title: String::new(),
                artist: String::new(),
                album: None,
                album_artist: None,
                musicbrainz_album_id: None,
                musicbrainz_track_id: None,
                musicbrainz_artist_id: None,
                compilation: Some("1".to_string()),
            })
            .collect::<Vec<_>>();
        let completed = tracks
            .iter()
            .take(51)
            .map(|track| track.path.clone())
            .collect::<std::collections::HashSet<_>>();
        let resume_index = compilation_resume_start_index(&tracks, Some(&tracks[2].path));
        let next = tracks
            .iter()
            .skip(resume_index)
            .find(|track| !compilation_track_is_already_complete(&track.path, &completed, false))
            .expect("track 52 should remain unfinished");

        assert_eq!(next.path, tracks[51].path);
    }

    #[test]
    fn targeted_embedded_art_is_added_without_replacing_provider_results() {
        let dir = TempDir::new().unwrap();
        let embedded_path = dir.path().join("embedded.png");
        let provider_path = dir.path().join("provider.png");
        image::RgbImage::from_pixel(32, 32, image::Rgb([12, 34, 56]))
            .save(&embedded_path)
            .unwrap();
        image::RgbImage::from_pixel(64, 64, image::Rgb([65, 43, 21]))
            .save(&provider_path)
            .unwrap();
        let downloaded = DownloadedCandidate::from_existing_path(
            "itunes",
            provider_path,
            3,
            "https://example.invalid/provider.png",
        )
        .unwrap();
        let mut pipeline = PipelineResult {
            candidates: vec![PipelineCandidate {
                reference: ArtworkReference {
                    source: "itunes".to_string(),
                    id: "provider".to_string(),
                    url: "https://example.invalid/provider.png".to_string(),
                    front: true,
                    approved: true,
                    types: vec!["Front".to_string()],
                },
                downloaded,
                strict: StrictContentDecision::default(),
            }],
            best_index: Some(0),
            diagnostics: Vec::new(),
            provider_timings: Vec::new(),
        };

        prepend_embedded_compilation_candidate(
            &mut pipeline,
            &embedded_path,
            Path::new("album/54 - Track.mp3"),
        )
        .unwrap();

        assert_eq!(pipeline.candidates.len(), 2);
        assert_eq!(pipeline.candidates[0].downloaded.candidate.source, "local");
        assert!(
            pipeline.candidates[0]
                .reference
                .types
                .iter()
                .any(|value| value == "EmbeddedTrack")
        );
        assert_eq!(pipeline.candidates[1].downloaded.candidate.source, "itunes");
        assert_eq!(pipeline.best_index, Some(1));
    }

    #[test]
    fn compilation_source_results_cache_restores_original_ranking() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("candidate.png");
        image::RgbImage::from_pixel(32, 32, image::Rgb([12, 34, 56]))
            .save(&path)
            .unwrap();
        let downloaded = DownloadedCandidate::from_existing_path(
            "itunes",
            path.clone(),
            3,
            "https://example.invalid/art.png",
        )
        .unwrap();
        let original = PipelineResult {
            candidates: vec![PipelineCandidate {
                reference: ArtworkReference {
                    source: "itunes".to_string(),
                    id: "fixture".to_string(),
                    url: "https://example.invalid/art.png".to_string(),
                    front: true,
                    approved: true,
                    types: vec!["Front".to_string()],
                },
                downloaded,
                strict: StrictContentDecision::default(),
            }],
            best_index: Some(0),
            diagnostics: Vec::new(),
            provider_timings: Vec::new(),
        };

        let restored = CachedPipelineResult::capture(&original).restore().unwrap();
        assert_eq!(restored.best_index, Some(0));
        assert_eq!(
            restored.candidates[0].reference,
            original.candidates[0].reference
        );
        assert_eq!(restored.candidates[0].downloaded.path(), path);
        assert_eq!(
            restored.candidates[0].downloaded.candidate.source_priority,
            3
        );
    }
}

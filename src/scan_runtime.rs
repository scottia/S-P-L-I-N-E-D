use crate::candidate::{Candidate, StaticFormat};
use crate::config::{Config, Mode, samples_dir};
use crate::final_artwork::{
    FinalArtworkAction, FinalArtworkResult, PreparedArtwork, destination_matches_prepared,
    install_prepared_artwork, prepare_final_artwork,
};
use crate::inspect::inspect_image;
use crate::musicbrainz::{MusicBrainzClient, RequestMode};
use crate::pipeline::{
    candidate_summary, prepare_persistent_cache_dir, run_registry_pipeline_with_cache_dir,
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
use crossterm::style::{Color, Stylize};
use std::fs;
use std::path::{Path, PathBuf};

const ORANGE: Color = Color::AnsiValue(208);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadScanSummary {
    pub albums: usize,
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
    require_musicbrainz_oauth(config.musicbrainz.oauth)?;

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

    if musicbrainz.request_mode() != &RequestMode::OAuthBearer {
        return Err(
            "Operational scan testing requires MusicBrainz OAuth Bearer mode; anonymous fallback is not permitted."
                .to_string(),
        );
    }

    let registry = ProviderRegistry::from_source_order_with_credentials(
        resolved_sources,
        &config.fanarttv.credential_file,
        &config.lastfm.credential_file,
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
        config.musicbrainz.retry_max.to_string().cyan().bold()
    );
    println!(
        "MB Min Delay: {}",
        format!("{:.2}s", config.musicbrainz.mb_min_delay)
            .cyan()
            .bold()
    );
    println!(
        "MB Rec Timeout: {}",
        format!("{:.1}s", config.musicbrainz.mb_recording_timeout)
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

        let (release_mbid, authority) = match decision {
            AlbumReleaseDecision::Resolved {
                release_mbid,
                authority,
            } => (release_mbid, authority),
            AlbumReleaseDecision::Fallback { .. } => {
                summary.unresolved += 1;
                print_unresolved_tagged_release(&tagged_album, &mbid_audit);
                if mbid_audit.valid.len() > 1 {
                    println!(
                        "  Reason:      {}",
                        "multiple valid MUSICBRAINZ_ALBUMID values; reconcile album tags"
                            .yellow()
                            .bold()
                    );
                } else if mbid_audit.valid.is_empty() {
                    println!(
                        "  Reason:      {}",
                        "no valid MUSICBRAINZ_ALBUMID is available".yellow()
                    );
                }
                println!("  Artwork:     {}", "skipped".yellow().bold());
                println!();
                continue;
            }
        };

        let release = match musicbrainz.lookup_release(&release_mbid).await {
            Ok(release) => release,
            Err(error) => {
                summary.failed += 1;
                println!(
                    "  {}",
                    format!("ERROR: MusicBrainz release lookup failed: {error}")
                        .red()
                        .bold()
                );
                println!();
                continue;
            }
        };

        let title_match = tagged_album_title_matches_release(&tracks, &release.title);
        print_tagged_release_result(
            tagged_album.as_deref(),
            &release.title,
            &release_mbid,
            title_match,
        );
        println!("  Authority:   {}", format!("{authority:?}").green().bold());

        if title_match == Some(false) {
            summary.unresolved += 1;
            println!("  MB Release:  {}", release.title.as_str().yellow().bold());
            println!(
                "  {}",
                "ERROR: tagged ALBUM does not match the MusicBrainz release title."
                    .red()
                    .bold()
            );
            println!("  Artwork:     {}", "skipped".yellow().bold());
            println!();
            continue;
        }

        summary.resolved += 1;
        println!(
            "  MB Artist:   {}",
            release.artist_credit.as_str().green().bold()
        );
        println!("  MB Release:  {}", release.title.as_str().yellow().bold());

        let query = ArtworkQuery::release(&release_mbid);
        let context = ProviderContext {
            artist_credit: release.artist_credit,
            release_title: release.title,
            release_group_mbid: release.release_group_id,
            release_group_title: release.release_group_title,
        };

        let result = match run_registry_pipeline_with_cache_dir(
            &registry,
            &query,
            &context,
            resolved_sources,
            &range,
            &format_order,
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

        let chosen_source = result
            .best()
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
            let selected = Some(candidate_index) == result.best_index;
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

        match result.best() {
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
                    config.output.preserve_file,
                ) {
                    Ok((final_result, destination)) => {
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

fn finalize_selected_with_preserve(
    mode: Mode,
    candidate: &Candidate,
    source_path: &Path,
    canonical_destination: &Path,
    range: &Range,
    target_format: StaticFormat,
    preserve_file: bool,
) -> Result<(FinalArtworkResult, PathBuf), String> {
    let prepared = prepare_final_artwork(candidate, source_path, range, target_format)?;

    if !preserve_file {
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

        if audit.valid.len() > 1 {
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

fn require_musicbrainz_oauth(oauth: bool) -> Result<(), String> {
    if oauth {
        Ok(())
    } else {
        Err(
            "Operational scan testing requires [musicbrainz] oauth = true; anonymous MusicBrainz fallback is not permitted."
                .to_string(),
        )
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
    fn scan_library_requires_musicbrainz_oauth() {
        assert_eq!(require_musicbrainz_oauth(true), Ok(()));
        assert!(require_musicbrainz_oauth(false).is_err());
    }
}

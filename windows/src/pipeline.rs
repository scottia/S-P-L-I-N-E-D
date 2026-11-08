use crate::candidate::{Candidate, StaticFormat};
use crate::config::Mode;
use crate::download::{DownloadCache, DownloadedCandidate, temporary_owner_token};
use crate::evaluate::{best_candidate, evaluate, is_acceptable};
use crate::final_artwork::{FinalArtworkResult, finalize_selected_candidate};
use crate::range::Range;
use crate::source::coverartarchive::CoverArtArchive;
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderRegistry,
};
use crate::source_policy::{SourcePolicyConfig, best_candidate_index, reference_allowed};
use crate::strict_source_policy::StrictContentDecision;
use futures_util::future::join_all;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Semaphore;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

static CLEANED_PERSISTENT_CACHE_DIRS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
const CANDIDATE_DOWNLOAD_CONCURRENCY: usize = 4;

#[derive(Debug)]
pub struct PipelineCandidate {
    pub reference: ArtworkReference,
    pub downloaded: DownloadedCandidate,
    pub strict: StrictContentDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineDiagnostic {
    pub source: String,
    pub url: String,
    pub message: String,
}

#[derive(Debug)]
pub struct PipelineResult {
    pub candidates: Vec<PipelineCandidate>,
    pub best_index: Option<usize>,
    pub diagnostics: Vec<PipelineDiagnostic>,
    pub provider_timings: Vec<ProviderTiming>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTiming {
    pub source: String,
    pub discovery_ms: u64,
    pub download_ms: u64,
    pub references: usize,
    pub candidates: usize,
    pub errors: usize,
    pub error: Option<String>,
}

impl ProviderTiming {
    pub fn status(&self) -> &'static str {
        if self.errors > 0 {
            "error"
        } else if self.candidates > 0 {
            "success"
        } else {
            "zero"
        }
    }
}

#[derive(Clone, Copy)]
pub struct RegistryPipelineOptions<'a> {
    pub source_order: &'a [String],
    pub range: &'a Range,
    pub format_order: &'a [StaticFormat],
    pub source_policies: &'a BTreeMap<String, SourcePolicyConfig>,
}

impl PipelineResult {
    pub fn best(&self) -> Option<&PipelineCandidate> {
        self.best_index.and_then(|index| self.candidates.get(index))
    }

    pub fn finalize_best(
        &self,
        mode: Mode,
        destination: &Path,
        range: &Range,
        target_format: StaticFormat,
    ) -> Result<FinalArtworkResult, String> {
        let selected = self
            .best()
            .map(|item| (&item.downloaded.candidate, item.downloaded.path()));

        finalize_selected_candidate(mode, selected, destination, range, target_format)
    }
}

pub async fn discover_provider(
    provider: &dyn ArtworkProvider,
    query: &ArtworkQuery,
    context: &ProviderContext,
) -> Result<Vec<ArtworkReference>, String> {
    let references = provider.discover(query, context).await?;

    if let Some(reference) = references
        .iter()
        .find(|reference| reference.source != provider.name())
    {
        return Err(format!(
            "Artwork provider {} returned a reference owned by {}.",
            provider.name(),
            reference.source
        ));
    }

    Ok(references)
}

pub async fn discover_all_providers(
    registry: &ProviderRegistry,
    query: &ArtworkQuery,
    context: &ProviderContext,
) -> (
    Vec<ArtworkReference>,
    Vec<PipelineDiagnostic>,
    Vec<ProviderTiming>,
) {
    let mut references = Vec::new();
    let mut diagnostics = Vec::new();
    let discovery = join_all(registry.providers().map(|provider| async move {
        let started = Instant::now();
        let result = discover_provider(provider, query, context).await;
        (provider.name(), elapsed_ms(started), result)
    }))
    .await;
    let mut timings = Vec::with_capacity(discovery.len());

    // join_all preserves input order, so concurrent I/O cannot change source
    // priority or the deterministic reference order consumed by ranking.
    for (source, discovery_ms, result) in discovery {
        match result {
            Ok(mut discovered) => {
                let reference_count = discovered.len();
                references.append(&mut discovered);
                timings.push(ProviderTiming {
                    source: source.to_string(),
                    discovery_ms,
                    download_ms: 0,
                    references: reference_count,
                    candidates: 0,
                    errors: 0,
                    error: None,
                });
            }
            Err(error) => {
                let error = compact_safe_error(&error);
                diagnostics.push(PipelineDiagnostic {
                    source: source.to_string(),
                    url: String::new(),
                    message: error.clone(),
                });
                timings.push(ProviderTiming {
                    source: source.to_string(),
                    discovery_ms,
                    download_ms: 0,
                    references: 0,
                    candidates: 0,
                    errors: 1,
                    error: Some(error),
                });
            }
        }
    }

    (references, diagnostics, timings)
}

pub async fn run_registry_pipeline(
    registry: &ProviderRegistry,
    query: &ArtworkQuery,
    context: &ProviderContext,
    source_order: &[String],
    range: &Range,
    format_order: &[StaticFormat],
    source_policies: &BTreeMap<String, SourcePolicyConfig>,
) -> Result<PipelineResult, String> {
    let cache = DownloadCache::new()?;
    run_registry_pipeline_with_cache(
        registry,
        query,
        context,
        RegistryPipelineOptions {
            source_order,
            range,
            format_order,
            source_policies,
        },
        &cache,
    )
    .await
}

pub fn prepare_persistent_cache_dir(cache_dir: &Path) -> Result<(), String> {
    if cache_dir.as_os_str().is_empty() || cache_dir.file_name().is_none() {
        return Err(format!(
            "Refusing to clean unsafe SPLINED persistent cache path: {}",
            cache_dir.display()
        ));
    }

    fs::create_dir_all(cache_dir).map_err(|error| {
        format!(
            "Unable to create SPLINED persistent cache directory {}: {error}",
            cache_dir.display()
        )
    })?;

    let cache_key = fs::canonicalize(cache_dir).unwrap_or_else(|_| cache_dir.to_path_buf());
    let cleaned_dirs = CLEANED_PERSISTENT_CACHE_DIRS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut cleaned_dirs = cleaned_dirs
        .lock()
        .map_err(|_| "SPLINED persistent cache cleanup lock was poisoned.".to_string())?;

    if cleaned_dirs.contains(&cache_key) {
        return Ok(());
    }

    cleaned_dirs.insert(cache_key);
    Ok(())
}

/// Best-effort removal of disposable artwork created by a completed or
/// interrupted run. The SQLite database, samples, and unrelated files are
/// intentionally outside this filename contract and are never touched.
pub fn cleanup_run_cache_files(cache_dir: &Path) {
    let candidate_prefix = format!("splined-candidate-{}-", temporary_owner_token());
    let local_prefix = format!("splined-local-{}-", temporary_owner_token());
    for directory in [
        cache_dir.to_path_buf(),
        cache_dir.join("artist-art-candidates"),
        cache_dir.join("musicbrainz-cover-previews"),
    ] {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if (name.starts_with(&candidate_prefix) || name.starts_with(&local_prefix))
                && path.is_file()
                && let Err(error) = fs::remove_file(&path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "SPLINED temporary cleanup failed path={} error={error}",
                    path.display()
                );
            }
        }
    }
}

fn process_scoped_owner_pid(name: &str) -> Option<u32> {
    for prefix in ["splined-candidate-", "splined-local-"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest.split('-').next()?.parse().ok();
        }
    }
    None
}

#[cfg(windows)]
fn process_is_active(pid: u32) -> bool {
    // SAFETY: OpenProcess returns an owned handle which is closed on every
    // successful path. Query-only access cannot mutate the target process.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or_default() as u32;
        return error == ERROR_ACCESS_DENIED || error != ERROR_INVALID_PARAMETER;
    }
    let mut exit_code = 0_u32;
    // STILL_ACTIVE is 259. A failed query is treated as active so cleanup
    // never guesses that another live SPLINED process has exited.
    let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    unsafe { CloseHandle(handle) };
    !queried || exit_code == 259
}

#[cfg(not(windows))]
fn process_is_active(_pid: u32) -> bool {
    true
}

/// Prune only known disposable Windows artwork from the configured run cache.
/// A grace period protects another client actively reading the same shared
/// cache; SQLite, saved samples, and the persistent Artist images directory
/// are outside this filename-and-directory contract.
pub fn cleanup_stale_artwork_on_startup(cache_dir: &Path) -> Result<(usize, u64), String> {
    if cache_dir.as_os_str().is_empty() || cache_dir.file_name().is_none() {
        return Err(format!(
            "Refusing unsafe cache cleanup path: {}",
            cache_dir.display()
        ));
    }
    if !cache_dir.exists() {
        return Ok((0, 0));
    }
    if !cache_dir.is_dir()
        || fs::symlink_metadata(cache_dir).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(format!(
            "Refusing non-directory or linked cache path: {}",
            cache_dir.display()
        ));
    }
    let cutoff = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    let mut removed = 0usize;
    let mut bytes = 0u64;
    let locations = [
        (cache_dir.to_path_buf(), "root"),
        (cache_dir.join("artist-art-candidates"), "candidates"),
        (cache_dir.join("musicbrainz-cover-previews"), "candidates"),
        (
            cache_dir.join("navidrome-thumbnails").join("artists"),
            "navidrome",
        ),
        (
            cache_dir.join("navidrome-thumbnails").join("albums"),
            "navidrome",
        ),
        (
            cache_dir
                .join("navidrome-thumbnails")
                .join("similar-artists"),
            "navidrome",
        ),
        (
            cache_dir
                .join("navidrome-thumbnails")
                .join("artist-banners"),
            "navidrome",
        ),
    ];
    for (directory, kind) in locations {
        if !directory.exists() {
            continue;
        }
        if !directory.is_dir()
            || fs::symlink_metadata(&directory).is_ok_and(|meta| meta.file_type().is_symlink())
        {
            continue;
        }
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("Unable to inspect cache {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("Unable to inspect cache entry: {error}"))?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Unable to inspect cache file type: {error}"))?;
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let legacy_navidrome_image = kind == "navidrome"
                && entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| {
                        value.eq_ignore_ascii_case("img") || value.eq_ignore_ascii_case("image")
                    });
            if legacy_navidrome_image {
                let metadata = entry.metadata().map_err(|error| {
                    format!(
                        "Unable to inspect cache file {}: {error}",
                        entry.path().display()
                    )
                })?;
                let discarded = normalize_legacy_navidrome_image(&entry.path())?;
                removed += 1;
                if discarded {
                    bytes = bytes.saturating_add(metadata.len());
                }
                continue;
            }
            let managed = match kind {
                "root" => {
                    name.starts_with("splined-candidate-") || name.starts_with("splined-local-")
                }
                "candidates" => name.starts_with("splined-candidate-"),
                _ => {
                    name.ends_with(".img")
                        || ((name.starts_with("circle-v1-")
                            || name.starts_with("banner-v2-")
                            || name.starts_with("banner-v3-")
                            || name.starts_with("banner-v4-")
                            || name.starts_with("banner-v5-"))
                            && name.ends_with(".png"))
                }
            };
            if !managed {
                continue;
            }
            let metadata = entry.metadata().map_err(|error| {
                format!(
                    "Unable to inspect cache file {}: {error}",
                    entry.path().display()
                )
            })?;
            let abandoned_process_file =
                process_scoped_owner_pid(&name).is_some_and(|pid| !process_is_active(pid));
            if !abandoned_process_file
                && !metadata.modified().is_ok_and(|modified| modified <= cutoff)
            {
                continue;
            }
            fs::remove_file(entry.path()).map_err(|error| {
                format!(
                    "Unable to remove stale cache file {}: {error}",
                    entry.path().display()
                )
            })?;
            removed += 1;
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    Ok((removed, bytes))
}

/// Replace the generic cache extension used by older builds with the detected
/// static format. Invalid or duplicate generic files are discarded.
fn normalize_legacy_navidrome_image(path: &Path) -> Result<bool, String> {
    let inspected = match crate::inspect::inspect_image(path) {
        Ok(inspected) => inspected,
        Err(_) => {
            fs::remove_file(path).map_err(|error| {
                format!(
                    "Unable to remove invalid Navidrome cache {}: {error}",
                    path.display()
                )
            })?;
            return Ok(true);
        }
    };
    let extension = match inspected.format {
        StaticFormat::Jpeg => "jpg",
        StaticFormat::Png => "png",
        StaticFormat::Webp => "webp",
    };
    let target = path.with_extension(extension);
    if target.is_file() && crate::inspect::inspect_image(&target).is_ok() {
        fs::remove_file(path).map_err(|error| {
            format!(
                "Unable to remove duplicate Navidrome cache {}: {error}",
                path.display()
            )
        })?;
        return Ok(true);
    }
    if target.exists() {
        fs::remove_file(&target).map_err(|error| {
            format!(
                "Unable to replace invalid Navidrome cache {}: {error}",
                target.display()
            )
        })?;
    }
    fs::rename(path, &target).map_err(|error| {
        format!(
            "Unable to normalize Navidrome cache {} to {}: {error}",
            path.display(),
            target.display()
        )
    })?;
    Ok(false)
}

pub async fn run_registry_pipeline_with_cache_dir(
    registry: &ProviderRegistry,
    query: &ArtworkQuery,
    context: &ProviderContext,
    options: RegistryPipelineOptions<'_>,
    cache_dir: &Path,
) -> Result<PipelineResult, String> {
    prepare_persistent_cache_dir(cache_dir)?;
    let cache = DownloadCache::new_persistent(cache_dir)?;

    run_registry_pipeline_with_cache(registry, query, context, options, &cache).await
}

async fn run_registry_pipeline_with_cache(
    registry: &ProviderRegistry,
    query: &ArtworkQuery,
    context: &ProviderContext,
    options: RegistryPipelineOptions<'_>,
    cache: &DownloadCache,
) -> Result<PipelineResult, String> {
    let (references, mut discovery_diagnostics, mut provider_timings) =
        discover_all_providers(registry, query, context).await;

    let mut result = run_artwork_pipeline_with_cache(
        references,
        options.source_order,
        options.range,
        options.format_order,
        Some(options.source_policies),
        cache,
    )
    .await?;
    discovery_diagnostics.append(&mut result.diagnostics);
    result.diagnostics = discovery_diagnostics;
    merge_download_timings(&mut provider_timings, &result.provider_timings);
    result.provider_timings = provider_timings;

    Ok(result)
}

pub async fn run_provider_pipeline(
    provider: &dyn ArtworkProvider,
    query: &ArtworkQuery,
    context: &ProviderContext,
    source_order: &[String],
    range: &Range,
    format_order: &[StaticFormat],
) -> Result<PipelineResult, String> {
    let references = discover_provider(provider, query, context).await?;
    run_artwork_pipeline(references, source_order, range, format_order).await
}

pub async fn run_artwork_pipeline(
    references: Vec<ArtworkReference>,
    source_order: &[String],
    range: &Range,
    format_order: &[StaticFormat],
) -> Result<PipelineResult, String> {
    let cache = DownloadCache::new()?;
    run_artwork_pipeline_with_cache(references, source_order, range, format_order, None, &cache)
        .await
}

async fn run_artwork_pipeline_with_cache(
    references: Vec<ArtworkReference>,
    source_order: &[String],
    range: &Range,
    format_order: &[StaticFormat],
    source_policies: Option<&BTreeMap<String, SourcePolicyConfig>>,
    cache: &DownloadCache,
) -> Result<PipelineResult, String> {
    let mut pending = Vec::new();
    let mut diagnostics = Vec::new();
    let mut provider_timings = Vec::<ProviderTiming>::new();
    let download_slots = Arc::new(Semaphore::new(CANDIDATE_DOWNLOAD_CONCURRENCY));

    for reference in references {
        let allowed = source_policies
            .map(|policies| reference_allowed(policies, &reference.source, reference.front))
            .unwrap_or(reference.front);
        if !allowed {
            continue;
        }

        let Some(source_priority) = source_order
            .iter()
            .position(|source| source == &reference.source)
        else {
            diagnostics.push(PipelineDiagnostic {
                source: reference.source.clone(),
                url: reference.url.clone(),
                message: "provider is not enabled in the resolved source list".to_string(),
            });
            continue;
        };

        let download_slots = Arc::clone(&download_slots);
        pending.push(async move {
            let started = Instant::now();
            let _slot = download_slots
                .acquire_owned()
                .await
                .expect("candidate download semaphore was closed");
            let result = cache
                .download_candidate(reference.source.clone(), &reference.url, source_priority)
                .await;
            (reference, elapsed_ms(started), result)
        });
    }

    let mut candidates = Vec::new();
    // Candidate downloads are independent network operations. Results are
    // consumed in the original reference order to preserve stable ranking.
    for (reference, download_ms, result) in join_all(pending).await {
        let timing = provider_timing_mut(&mut provider_timings, &reference.source);
        timing.download_ms = timing.download_ms.max(download_ms);
        match result {
            Ok(downloaded) => {
                timing.candidates += 1;
                candidates.push(PipelineCandidate {
                    reference,
                    downloaded,
                    strict: StrictContentDecision::default(),
                });
            }
            Err(error) => {
                let error = compact_safe_error(&error);
                timing.errors += 1;
                if timing.error.is_none() {
                    timing.error = Some(error.clone());
                }
                diagnostics.push(PipelineDiagnostic {
                    source: reference.source.clone(),
                    url: reference.url.clone(),
                    message: error,
                });
            }
        }
    }

    let candidate_values: Vec<Candidate> = candidates
        .iter()
        .map(|item| item.downloaded.candidate.clone())
        .collect();

    let best_index = match source_policies {
        Some(policies) => best_candidate_index(&candidate_values, range, format_order, policies),
        None => best_candidate(&candidate_values, range, format_order).and_then(|best| {
            candidate_values
                .iter()
                .position(|candidate| candidate == best)
        }),
    };

    Ok(PipelineResult {
        candidates,
        best_index,
        diagnostics,
        provider_timings,
    })
}

fn provider_timing_mut<'a>(
    timings: &'a mut Vec<ProviderTiming>,
    source: &str,
) -> &'a mut ProviderTiming {
    if let Some(index) = timings.iter().position(|timing| timing.source == source) {
        return &mut timings[index];
    }
    timings.push(ProviderTiming {
        source: source.to_string(),
        discovery_ms: 0,
        download_ms: 0,
        references: 0,
        candidates: 0,
        errors: 0,
        error: None,
    });
    timings.last_mut().expect("provider timing was inserted")
}

fn merge_download_timings(discovery: &mut Vec<ProviderTiming>, downloads: &[ProviderTiming]) {
    for downloaded in downloads {
        let timing = provider_timing_mut(discovery, &downloaded.source);
        timing.download_ms = downloaded.download_ms;
        timing.candidates = downloaded.candidates;
        timing.errors += downloaded.errors;
        if timing.error.is_none() {
            timing.error.clone_from(&downloaded.error);
        }
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn compact_safe_error(message: &str) -> String {
    let mut safe = String::with_capacity(message.len().min(240));
    let mut remaining = message;
    while let Some(offset) = remaining
        .find("http://")
        .or_else(|| remaining.find("https://"))
    {
        safe.push_str(&remaining[..offset]);
        remaining = &remaining[offset..];
        let end = remaining
            .find(|character: char| character.is_whitespace() || matches!(character, ')' | ']'))
            .unwrap_or(remaining.len());
        let url = &remaining[..end];
        if let Some(query) = url.find('?') {
            safe.push_str(&url[..query]);
            safe.push_str("?<redacted>");
        } else {
            safe.push_str(url);
        }
        remaining = &remaining[end..];
    }
    safe.push_str(remaining);
    let mut safe = safe.replace(['\r', '\n'], " ");
    if safe.chars().count() > 240 {
        safe = safe.chars().take(237).collect::<String>();
        safe.push_str("...");
    }
    safe
}

pub async fn run_coverartarchive_pipeline(
    release_mbid: &str,
    source_order: &[String],
    range: &Range,
    format_order: &[StaticFormat],
) -> Result<PipelineResult, String> {
    let provider = CoverArtArchive::new(std::time::Duration::from_secs(20))?;
    let query = ArtworkQuery::release(release_mbid);
    let context = ProviderContext {
        artist_credit: String::new(),
        release_title: String::new(),
        release_group_mbid: None,
        release_group_title: None,
        apple_collection_ids: Vec::new(),
    };

    run_provider_pipeline(
        &provider,
        &query,
        &context,
        source_order,
        range,
        format_order,
    )
    .await
}

pub fn candidate_summary(
    item: &PipelineCandidate,
    range: &Range,
    format_order: &[StaticFormat],
) -> String {
    let candidate = &item.downloaded.candidate;
    let evaluation = evaluate(candidate, range, format_order);

    format!(
        "{}x{} {:?} {:?} distance={} square={} acceptable={} approved={} id={} priority={}",
        candidate.width,
        candidate.height,
        candidate.format,
        evaluation.range_class,
        evaluation.distance_from_ideal,
        candidate.is_square(),
        is_acceptable(candidate, range),
        item.reference.approved,
        item.reference.id,
        candidate.source_priority
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ProviderDiscoveryFuture;
    use tokio::sync::Barrier;
    use tokio::time::{Duration, timeout};

    fn reference(source: &str, id: &str) -> ArtworkReference {
        ArtworkReference {
            source: source.to_string(),
            id: id.to_string(),
            url: "https://example.invalid/art.jpg".to_string(),
            front: true,
            approved: true,
            types: vec!["Front".to_string()],
        }
    }

    struct FixtureProvider {
        source: &'static str,
        returned_source: &'static str,
    }

    struct CoordinatedProvider {
        source: &'static str,
        barrier: Arc<Barrier>,
    }

    impl ArtworkProvider for CoordinatedProvider {
        fn name(&self) -> &'static str {
            self.source
        }

        fn discover<'a>(
            &'a self,
            _query: &'a ArtworkQuery,
            _context: &'a ProviderContext,
        ) -> ProviderDiscoveryFuture<'a> {
            Box::pin(async move {
                self.barrier.wait().await;
                Ok(vec![reference(self.source, self.source)])
            })
        }
    }

    impl ArtworkProvider for FixtureProvider {
        fn name(&self) -> &'static str {
            self.source
        }

        fn discover<'a>(
            &'a self,
            _query: &'a ArtworkQuery,
            _context: &'a ProviderContext,
        ) -> ProviderDiscoveryFuture<'a> {
            Box::pin(async move { Ok(vec![reference(self.returned_source, "fixture")]) })
        }
    }

    fn fixture_context() -> ProviderContext {
        ProviderContext {
            artist_credit: "Fixture Artist".to_string(),
            release_title: "Fixture Release".to_string(),
            release_group_mbid: Some("978d88db-60ec-41d9-ade7-beea020941b0".to_string()),
            release_group_title: Some("Fixture Release".to_string()),
            apple_collection_ids: Vec::new(),
        }
    }

    #[tokio::test]
    async fn provider_identity_mismatch_is_rejected() {
        let provider = FixtureProvider {
            source: "coverartarchive",
            returned_source: "deezer",
        };
        let query = ArtworkQuery::release("f60a6a1c-56cf-4dd9-a6ad-c47450d1b132");
        let error = discover_provider(&provider, &query, &fixture_context())
            .await
            .expect_err("mismatched provider identity must fail");
        assert!(error.contains("returned a reference owned by deezer"));
    }

    #[tokio::test]
    async fn provider_discovery_overlaps_without_changing_source_order() {
        let barrier = Arc::new(Barrier::new(2));
        let registry = ProviderRegistry::from_providers(vec![
            Box::new(CoordinatedProvider {
                source: "itunes",
                barrier: Arc::clone(&barrier),
            }),
            Box::new(CoordinatedProvider {
                source: "amazon",
                barrier,
            }),
        ]);
        let query = ArtworkQuery::release("f60a6a1c-56cf-4dd9-a6ad-c47450d1b132");
        let (references, diagnostics, timings) = timeout(
            Duration::from_secs(1),
            discover_all_providers(&registry, &query, &fixture_context()),
        )
        .await
        .expect("provider discovery did not overlap");

        assert!(diagnostics.is_empty());
        assert_eq!(
            references
                .iter()
                .map(|reference| reference.source.as_str())
                .collect::<Vec<_>>(),
            vec!["itunes", "amazon"]
        );
        assert_eq!(
            timings
                .iter()
                .map(|timing| timing.source.as_str())
                .collect::<Vec<_>>(),
            vec!["itunes", "amazon"]
        );
    }

    #[test]
    fn provider_errors_remove_query_values_and_newlines() {
        let safe = compact_safe_error(
            "request failed: https://example.test/api?api_key=secret&token=private\nupstream 503",
        );
        assert_eq!(
            safe,
            "request failed: https://example.test/api?<redacted> upstream 503"
        );
        assert!(!safe.contains("secret"));
        assert!(!safe.contains("private"));
    }

    #[test]
    fn persistent_cache_preparation_preserves_other_session_candidates() {
        use tempfile::TempDir;
        let parent = TempDir::new().unwrap();
        let cache_dir = parent.path().join("cache-root");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("splined-candidate-stale.jpg"), b"stale").unwrap();
        fs::write(cache_dir.join("keep.txt"), b"keep").unwrap();

        prepare_persistent_cache_dir(&cache_dir).unwrap();

        assert!(cache_dir.join("splined-candidate-stale.jpg").exists());
        assert!(cache_dir.join("keep.txt").exists());
    }

    #[test]
    fn completed_run_cleanup_removes_only_disposable_artwork() {
        let root = tempfile::TempDir::new().expect("run cache fixture should create");
        let artist_candidates = root.path().join("artist-art-candidates");
        let musicbrainz_candidates = root.path().join("musicbrainz-cover-previews");
        fs::create_dir_all(&artist_candidates).unwrap();
        fs::create_dir_all(&musicbrainz_candidates).unwrap();
        fs::write(
            root.path().join(format!(
                "splined-candidate-{}-current.jpg",
                temporary_owner_token()
            )),
            b"candidate",
        )
        .unwrap();
        fs::write(
            root.path().join(format!(
                "splined-local-{}-embedded-current.jpg",
                temporary_owner_token()
            )),
            b"local",
        )
        .unwrap();
        let nested_artist = artist_candidates.join(format!(
            "splined-candidate-{}-artist.jpg",
            temporary_owner_token()
        ));
        let nested_musicbrainz = musicbrainz_candidates.join(format!(
            "splined-candidate-{}-release.jpg",
            temporary_owner_token()
        ));
        let other_session = artist_candidates.join("splined-candidate-999999-1-other.jpg");
        for path in [&nested_artist, &nested_musicbrainz, &other_session] {
            fs::write(path, b"candidate").unwrap();
        }
        fs::write(root.path().join("splined.db"), b"database").unwrap();
        fs::create_dir(root.path().join("samples")).unwrap();

        cleanup_run_cache_files(root.path());

        assert!(
            !root
                .path()
                .join(format!(
                    "splined-candidate-{}-current.jpg",
                    temporary_owner_token()
                ))
                .exists()
        );
        assert!(
            !root
                .path()
                .join(format!(
                    "splined-local-{}-embedded-current.jpg",
                    temporary_owner_token()
                ))
                .exists()
        );
        assert!(!nested_artist.exists());
        assert!(!nested_musicbrainz.exists());
        assert!(other_session.exists());
        assert!(root.path().join("splined.db").exists());
        assert!(root.path().join("samples").is_dir());
    }

    #[test]
    fn persistent_cache_cleanup_rejects_root_like_path() {
        assert!(prepare_persistent_cache_dir(Path::new("")).is_err());
    }

    #[test]
    fn startup_cleanup_prunes_only_old_known_artwork_and_preserves_database_and_samples() {
        let root = tempfile::TempDir::new().unwrap();
        let cache = root.path().join("_cache");
        let candidates = cache.join("artist-art-candidates");
        let musicbrainz = cache.join("musicbrainz-cover-previews");
        let navidrome = cache.join("navidrome-thumbnails").join("artist-banners");
        fs::create_dir_all(&candidates).unwrap();
        fs::create_dir_all(&musicbrainz).unwrap();
        fs::create_dir_all(&navidrome).unwrap();
        fs::create_dir_all(cache.join("samples")).unwrap();
        let old_candidate = candidates.join("splined-candidate-old.jpg");
        let old_banner = navidrome.join("banner-v2-old.png");
        let recent_candidate = candidates.join("splined-candidate-recent.png");
        let old_musicbrainz = musicbrainz.join("splined-candidate-old-release.jpg");
        let database = cache.join("splined.db");
        for path in [
            &old_candidate,
            &old_banner,
            &recent_candidate,
            &old_musicbrainz,
            &database,
        ] {
            fs::write(path, b"fixture").unwrap();
        }
        fs::write(candidates.join("Thumbs.db"), b"keep").unwrap();
        fs::write(cache.join("samples").join("sample.jpg"), b"keep").unwrap();
        let old_time = SystemTime::now() - Duration::from_secs(48 * 60 * 60);
        for path in [&old_candidate, &old_banner, &old_musicbrainz] {
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(old_time))
                .unwrap();
        }
        let (removed, bytes) = cleanup_stale_artwork_on_startup(&cache).unwrap();
        assert_eq!((removed, bytes), (3, 21));
        assert!(!old_candidate.exists());
        assert!(!old_banner.exists());
        assert!(!old_musicbrainz.exists());
        assert!(recent_candidate.exists());
        assert!(database.exists());
        assert!(candidates.join("Thumbs.db").exists());
        assert!(cache.join("samples").join("sample.jpg").exists());
    }

    #[test]
    fn startup_cleanup_normalizes_recent_legacy_navidrome_images() {
        let root = tempfile::TempDir::new().unwrap();
        let cache = root.path().join("_cache");
        let artists = cache.join("navidrome-thumbnails").join("artists");
        fs::create_dir_all(&artists).unwrap();
        let valid = artists.join("valid.img");
        let invalid = artists.join("invalid.image");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 80, 120, 255]))
            .save_with_format(&valid, image::ImageFormat::Png)
            .unwrap();
        fs::write(&invalid, b"not an image").unwrap();

        let (removed, bytes) = cleanup_stale_artwork_on_startup(&cache).unwrap();

        assert_eq!(removed, 2);
        assert_eq!(bytes, 12);
        assert!(!valid.exists());
        assert!(artists.join("valid.png").is_file());
        assert!(!invalid.exists());
    }
}

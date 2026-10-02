use crate::candidate::{Candidate, StaticFormat};
use crate::config::Mode;
use crate::download::{DownloadCache, DownloadedCandidate};
use crate::evaluate::{best_candidate, evaluate, is_acceptable};
use crate::final_artwork::{FinalArtworkResult, finalize_selected_candidate};
use crate::range::Range;
use crate::source::coverartarchive::CoverArtArchive;
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderRegistry,
};
use crate::source_policy::{SourcePolicyConfig, best_candidate_index, reference_allowed};
use futures_util::future::join_all;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio::sync::Semaphore;

static CLEANED_PERSISTENT_CACHE_DIRS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
const CANDIDATE_DOWNLOAD_CONCURRENCY: usize = 4;

#[derive(Debug)]
pub struct PipelineCandidate {
    pub reference: ArtworkReference,
    pub downloaded: DownloadedCandidate,
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

    for entry in fs::read_dir(cache_dir).map_err(|error| {
        format!(
            "Unable to enumerate SPLINED persistent cache {}: {error}",
            cache_dir.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Unable to inspect SPLINED persistent cache entry in {}: {error}",
                cache_dir.display()
            )
        })?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if !file_name.starts_with("splined-candidate-") {
            continue;
        }

        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "Unable to inspect SPLINED persistent cache entry {}: {error}",
                path.display()
            )
        })?;

        if file_type.is_dir() && !file_type.is_symlink() {
            continue;
        }

        fs::remove_file(&path).map_err(|error| {
            format!(
                "Unable to remove SPLINED persistent cache file {}: {error}",
                path.display()
            )
        })?;
    }

    cleaned_dirs.insert(cache_key);
    Ok(())
}

/// Best-effort removal of disposable artwork created by a completed or
/// interrupted run. The SQLite database, samples, and unrelated files are
/// intentionally outside this filename contract and are never touched.
pub fn cleanup_run_cache_files(cache_dir: &Path) {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if (name.starts_with("splined-candidate-") || name.starts_with("splined-local-"))
            && path.is_file()
        {
            let _ = fs::remove_file(path);
        }
    }
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
    fn persistent_cache_cleanup_removes_only_splined_candidates() {
        use tempfile::TempDir;
        let parent = TempDir::new().unwrap();
        let cache_dir = parent.path().join("cache-root");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("splined-candidate-stale.jpg"), b"stale").unwrap();
        fs::write(cache_dir.join("keep.txt"), b"keep").unwrap();

        prepare_persistent_cache_dir(&cache_dir).unwrap();

        assert!(!cache_dir.join("splined-candidate-stale.jpg").exists());
        assert!(cache_dir.join("keep.txt").exists());
    }

    #[test]
    fn completed_run_cleanup_removes_only_disposable_artwork() {
        let root = tempfile::TempDir::new().expect("run cache fixture should create");
        fs::write(
            root.path().join("splined-candidate-current.jpg"),
            b"candidate",
        )
        .unwrap();
        fs::write(
            root.path().join("splined-local-embedded-current.jpg"),
            b"local",
        )
        .unwrap();
        fs::write(root.path().join("splined.db"), b"database").unwrap();
        fs::create_dir(root.path().join("samples")).unwrap();

        cleanup_run_cache_files(root.path());

        assert!(!root.path().join("splined-candidate-current.jpg").exists());
        assert!(
            !root
                .path()
                .join("splined-local-embedded-current.jpg")
                .exists()
        );
        assert!(root.path().join("splined.db").exists());
        assert!(root.path().join("samples").is_dir());
    }

    #[test]
    fn persistent_cache_cleanup_rejects_root_like_path() {
        assert!(prepare_persistent_cache_dir(Path::new("")).is_err());
    }
}

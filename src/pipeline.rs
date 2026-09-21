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
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static CLEANED_PERSISTENT_CACHE_DIRS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

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
) -> (Vec<ArtworkReference>, Vec<PipelineDiagnostic>) {
    let mut references = Vec::new();
    let mut diagnostics = Vec::new();

    for provider in registry.providers() {
        match discover_provider(provider, query, context).await {
            Ok(mut discovered) => references.append(&mut discovered),
            Err(error) => diagnostics.push(PipelineDiagnostic {
                source: provider.name().to_string(),
                url: String::new(),
                message: error,
            }),
        }
    }

    (references, diagnostics)
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
    let (references, mut discovery_diagnostics) =
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
    let mut candidates = Vec::new();
    let mut diagnostics = Vec::new();

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

        let downloaded = match cache
            .download_candidate(reference.source.clone(), &reference.url, source_priority)
            .await
        {
            Ok(downloaded) => downloaded,
            Err(error) => {
                diagnostics.push(PipelineDiagnostic {
                    source: reference.source.clone(),
                    url: reference.url.clone(),
                    message: error,
                });
                continue;
            }
        };

        candidates.push(PipelineCandidate {
            reference,
            downloaded,
        });
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
    })
}

pub async fn run_coverartarchive_pipeline(
    release_mbid: &str,
    source_order: &[String],
    range: &Range,
    format_order: &[StaticFormat],
) -> Result<PipelineResult, String> {
    let provider = CoverArtArchive::new()?;
    let query = ArtworkQuery::release(release_mbid);
    let context = ProviderContext {
        artist_credit: String::new(),
        release_title: String::new(),
        release_group_mbid: None,
        release_group_title: None,
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
    fn persistent_cache_cleanup_rejects_root_like_path() {
        assert!(prepare_persistent_cache_dir(Path::new("")).is_err());
    }
}

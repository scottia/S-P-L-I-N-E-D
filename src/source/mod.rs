use std::future::Future;
use std::pin::Pin;

pub mod coverartarchive;
pub mod deezer;
pub mod discogs;
pub mod fanarttv;
pub mod itunes;
pub mod lastfm;

use coverartarchive::CoverArtArchive;
use deezer::Deezer;
use discogs::Discogs;
use fanarttv::FanartTv;
use itunes::ITunes;
use lastfm::LastFm;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkQuery {
    pub release_mbid: String,
}

impl ArtworkQuery {
    pub fn release(release_mbid: impl Into<String>) -> Self {
        Self {
            release_mbid: release_mbid.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContext {
    pub artist_credit: String,
    pub release_title: String,
    pub release_group_mbid: Option<String>,
    pub release_group_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkReference {
    pub source: String,
    pub id: String,
    pub url: String,
    pub front: bool,
    pub approved: bool,
    pub types: Vec<String>,
}

pub type ProviderDiscoveryFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ArtworkReference>, String>> + Send + 'a>>;

pub trait ArtworkProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn discover<'a>(
        &'a self,
        query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a>;
}

pub struct ProviderRegistry {
    providers: Vec<Box<dyn ArtworkProvider>>,
}

impl ProviderRegistry {
    pub fn from_source_order(source_order: &[String]) -> Result<Self, String> {
        Self::from_source_order_with_credentials(source_order, "", "", "")
    }

    pub fn from_source_order_with_credentials(
        source_order: &[String],
        fanarttv_credential_file: &str,
        lastfm_credential_file: &str,
        discogs_credential_file: &str,
    ) -> Result<Self, String> {
        let mut providers: Vec<Box<dyn ArtworkProvider>> = Vec::new();

        for source in source_order {
            match source.as_str() {
                "deezer" => providers.push(Box::new(Deezer::new()?)),
                "itunes" => providers.push(Box::new(ITunes::new()?)),
                "fanarttv" => providers.push(Box::new(FanartTv::new(fanarttv_credential_file)?)),
                "lastfm" => providers.push(Box::new(LastFm::new(lastfm_credential_file)?)),
                "coverartarchive" => providers.push(Box::new(CoverArtArchive::new()?)),
                "discogs" => providers.push(Box::new(Discogs::new(discogs_credential_file)?)),
                _ => {
                    return Err(format!(
                        "Unsupported SPLINED cover source in provider registry: {source}"
                    ));
                }
            }
        }

        Ok(Self { providers })
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.providers
            .iter()
            .map(|provider| provider.name())
            .collect()
    }

    pub fn providers(&self) -> impl Iterator<Item = &dyn ArtworkProvider> {
        self.providers.iter().map(Box::as_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_query_preserves_mbid() {
        let query = ArtworkQuery::release("f60a6a1c-56cf-4dd9-a6ad-c47450d1b132");
        assert_eq!(query.release_mbid, "f60a6a1c-56cf-4dd9-a6ad-c47450d1b132");
    }

    #[test]
    fn registry_resolves_implemented_enabled_providers() {
        let source_order = vec![
            "deezer".to_string(),
            "itunes".to_string(),
            "lastfm".to_string(),
            "coverartarchive".to_string(),
            "discogs".to_string(),
        ];
        let registry = ProviderRegistry::from_source_order_with_credentials(
            &source_order,
            "fanarttv.json",
            "lastfm.json",
            "discogs.json",
        )
        .unwrap();
        assert_eq!(
            registry.names(),
            vec!["deezer", "itunes", "lastfm", "coverartarchive", "discogs"]
        );
    }

    #[test]
    fn registry_can_be_empty_when_no_live_provider_is_enabled() {
        let source_order = Vec::new();
        let registry = ProviderRegistry::from_source_order(&source_order).unwrap();
        assert!(registry.is_empty());
    }
}

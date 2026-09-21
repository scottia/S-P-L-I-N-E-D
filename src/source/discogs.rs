use crate::credentials::{load_discogs_credential, resolve_credential_path};
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

const SEARCH_URL: &str = "https://api.discogs.com/database/search";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    id: u64,
    #[serde(default)]
    resource_url: String,
    #[serde(default)]
    cover_image: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    #[serde(default)]
    images: Vec<ReleaseImage>,
}

#[derive(Debug, Deserialize)]
struct ReleaseImage {
    id: Option<u64>,
    #[serde(rename = "type", default)]
    image_type: String,
    #[serde(default)]
    uri: String,
    #[serde(default)]
    resource_url: String,
}

pub struct Discogs {
    client: Client,
    credential_path: PathBuf,
}

impl Discogs {
    pub fn new(credential_file: &str) -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| format!("Unable to create Discogs HTTP client: {error}"))?;
        Ok(Self {
            client,
            credential_path: resolve_credential_path(credential_file, "Discogs")?,
        })
    }

    async fn discover_album(
        &self,
        artist: &str,
        album: &str,
    ) -> Result<Vec<ArtworkReference>, String> {
        if artist.trim().is_empty() || album.trim().is_empty() {
            return Ok(Vec::new());
        }

        let token = load_discogs_credential(&self.credential_path)?.token;
        let authorization = format!("Discogs token={}", token.trim());
        let response = self
            .client
            .get(SEARCH_URL)
            .header("Authorization", &authorization)
            .query(&[
                ("artist", artist),
                ("release_title", album),
                ("type", "release"),
                ("per_page", "10"),
            ])
            .send()
            .await
            .map_err(|error| format!("Unable to search Discogs for {artist} - {album}: {error}"))?;

        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err("Discogs personal access token was rejected.".to_string());
        }
        if status != StatusCode::OK {
            return Err(format!("Discogs search returned HTTP {status}"));
        }
        let search = response
            .json::<SearchResponse>()
            .await
            .map_err(|error| format!("Invalid Discogs search response: {error}"))?;

        let mut references = Vec::new();
        let mut seen_releases = HashSet::new();
        for result in search.results.into_iter().take(6) {
            if !seen_releases.insert(result.id) {
                continue;
            }
            let release_url = if result.resource_url.trim().is_empty() {
                format!("https://api.discogs.com/releases/{}", result.id)
            } else {
                result.resource_url.clone()
            };
            let release_response = match self
                .client
                .get(&release_url)
                .header("Authorization", &authorization)
                .send()
                .await
            {
                Ok(response) if response.status() == StatusCode::OK => response,
                _ => continue,
            };
            let release = match release_response.json::<ReleaseResponse>().await {
                Ok(release) => release,
                Err(_) => continue,
            };
            let primary: Vec<&ReleaseImage> = release
                .images
                .iter()
                .filter(|image| image.image_type.eq_ignore_ascii_case("primary"))
                .collect();
            let images: Vec<&ReleaseImage> = if primary.is_empty() {
                release.images.iter().collect()
            } else {
                primary
            };
            let mut emitted = false;
            for (image_index, image) in images.into_iter().take(2).enumerate() {
                let url = if image.uri.trim().is_empty() {
                    image.resource_url.trim()
                } else {
                    image.uri.trim()
                };
                if url.is_empty() {
                    continue;
                }
                references.push(ArtworkReference {
                    source: "discogs".to_string(),
                    id: format!(
                        "{}:{}",
                        result.id,
                        image
                            .id
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| (image_index + 1).to_string())
                    ),
                    url: url.to_string(),
                    front: true,
                    approved: true,
                    types: vec!["Front".to_string()],
                });
                emitted = true;
            }
            if !emitted && !result.cover_image.trim().is_empty() {
                references.push(ArtworkReference {
                    source: "discogs".to_string(),
                    id: result.id.to_string(),
                    url: result.cover_image,
                    front: true,
                    approved: true,
                    types: vec!["Front".to_string()],
                });
            }
        }
        Ok(references)
    }
}

impl ArtworkProvider for Discogs {
    fn name(&self) -> &'static str {
        "discogs"
    }

    fn discover<'a>(
        &'a self,
        _query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move {
            self.discover_album(&context.artist_credit, &context.release_title)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_fixture_parses() {
        let payload: SearchResponse = serde_json::from_str(
            r#"{"results":[{"id":123,"resource_url":"https://api.discogs.com/releases/123","cover_image":"https://example.test/cover.jpg"}]}"#,
        )
        .expect("fixture should parse");
        assert_eq!(payload.results.len(), 1);
        assert_eq!(payload.results[0].id, 123);
    }

    #[test]
    fn release_fixture_parses_primary_image() {
        let payload: ReleaseResponse = serde_json::from_str(
            r#"{"images":[{"id":45,"type":"primary","uri":"https://example.test/original.jpg"}]}"#,
        )
        .expect("fixture should parse");
        assert_eq!(payload.images[0].image_type, "primary");
        assert_eq!(payload.images[0].id, Some(45));
    }
}

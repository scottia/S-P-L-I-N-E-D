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
    title: String,
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
struct ArtistResponse {
    #[serde(default)]
    name: String,
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

    pub async fn discover_artist(&self, artist: &str) -> Result<Vec<ArtworkReference>, String> {
        let artist = artist.trim();
        if artist.is_empty() {
            return Ok(Vec::new());
        }
        let token = load_discogs_credential(&self.credential_path)?.token;
        let authorization = format!("Discogs token={}", token.trim());
        let response = self
            .client
            .get(SEARCH_URL)
            .header("Authorization", &authorization)
            .query(&[("q", artist), ("type", "artist"), ("per_page", "15")])
            .send()
            .await
            .map_err(|error| format!("Unable to search Discogs artists for {artist}: {error}"))?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err("Discogs personal access token was rejected.".to_string());
        }
        if response.status() != StatusCode::OK {
            return Err(format!(
                "Discogs artist search returned HTTP {}",
                response.status()
            ));
        }
        let search = response
            .json::<SearchResponse>()
            .await
            .map_err(|error| format!("Invalid Discogs artist search response: {error}"))?;
        let mut references = Vec::new();
        let mut seen = HashSet::new();
        for result in search
            .results
            .into_iter()
            .filter(|result| result.id != 0 && same_artist(&result.title, artist))
            .take(5)
        {
            if !seen.insert(result.id) {
                continue;
            }
            // Build this URL from the numeric ID; do not follow a result-supplied URL.
            let response = match self
                .client
                .get(format!("https://api.discogs.com/artists/{}", result.id))
                .header("Authorization", &authorization)
                .send()
                .await
            {
                Ok(response) if response.status() == StatusCode::OK => response,
                _ => continue,
            };
            let found = match response.json::<ArtistResponse>().await {
                Ok(found) if same_artist(&found.name, artist) => found,
                _ => continue,
            };
            references.extend(artist_image_references(result.id, found));
        }
        Ok(references)
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
                if !usable_artwork_url(url) {
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
            if !emitted && usable_artwork_url(&result.cover_image) {
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

fn same_artist(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .flat_map(char::to_lowercase)
            .filter(|character| character.is_alphanumeric())
            .collect::<String>()
    };
    !left.trim().is_empty() && normalize(left) == normalize(right)
}

fn artist_image_references(id: u64, artist: ArtistResponse) -> Vec<ArtworkReference> {
    let mut images = artist.images;
    images.sort_by_key(|image| !image.image_type.eq_ignore_ascii_case("primary"));
    images
        .into_iter()
        .filter_map(|image| {
            let url = if image.uri.trim().is_empty() {
                image.resource_url.trim()
            } else {
                image.uri.trim()
            };
            usable_artwork_url(url).then(|| ArtworkReference {
                source: "discogs".to_string(),
                id: format!("{id}:{}", image.id.unwrap_or_default()),
                url: url.to_string(),
                front: true,
                approved: true,
                types: vec!["ArtistImage".to_string()],
            })
        })
        .take(4)
        .collect()
}

fn usable_artwork_url(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !lower.ends_with("/images/spacer.gif")
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

    #[test]
    fn discogs_placeholder_is_not_an_artwork_candidate() {
        assert!(!usable_artwork_url(
            "https://st.discogs.com/build/images/spacer.gif"
        ));
        assert!(usable_artwork_url(
            "https://i.discogs.com/example/release-cover.jpg"
        ));
    }

    #[test]
    fn artist_images_are_distinct_from_release_covers() {
        let search: SearchResponse = serde_json::from_str(
            r#"{"results":[{"id":42,"title":"10cc"},{"id":43,"title":"10cc Tribute"}]}"#,
        )
        .unwrap();
        assert!(same_artist(&search.results[0].title, "10cc"));
        assert!(!same_artist(&search.results[1].title, "10cc"));
        let artist: ArtistResponse = serde_json::from_str(
            r#"{"name":"10cc","images":[{"id":2,"type":"secondary","uri":"https://example.test/secondary.jpg"},{"id":1,"type":"primary","uri":"https://example.test/primary.jpg"}]}"#,
        )
        .unwrap();
        let images = artist_image_references(42, artist);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].url, "https://example.test/primary.jpg");
        assert_eq!(images[0].types, vec!["ArtistImage"]);
    }
}

use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

const SEARCH_URL: &str = "https://itunes.apple.com/search";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const SEARCH_LIMIT: usize = 25;
const ARTWORK_SIZE: u32 = 3000;

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchAlbum>,
}

#[derive(Debug, Deserialize)]
struct SearchAlbum {
    #[serde(rename = "collectionId")]
    collection_id: u64,
    #[serde(rename = "collectionName")]
    collection_name: String,
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(rename = "artworkUrl100", default)]
    artwork_url_100: String,
}

pub struct ITunes {
    client: Client,
}

impl ITunes {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| format!("Unable to create iTunes HTTP client: {error}"))?;

        Ok(Self { client })
    }

    async fn search_query(
        &self,
        artist: &str,
        album: &str,
        term: &str,
    ) -> Result<Vec<SearchAlbum>, String> {
        let limit = SEARCH_LIMIT.to_string();

        let response = self
            .client
            .get(SEARCH_URL)
            .query(&[
                ("term", term),
                ("media", "music"),
                ("entity", "album"),
                ("limit", limit.as_str()),
            ])
            .send()
            .await
            .map_err(|error| format!("Unable to query iTunes for {artist} - {album}: {error}"))?;

        let status = response.status();
        let final_url = response.url().clone();

        if status != StatusCode::OK {
            return Err(format!(
                "iTunes search for {artist} - {album} returned HTTP {status} from {final_url}"
            ));
        }

        let body = response.text().await.map_err(|error| {
            format!(
                "Unable to read iTunes response for {artist} - {album} from {final_url}: {error}"
            )
        })?;

        let payload = serde_json::from_str::<SearchResponse>(&body).map_err(|error| {
            let preview: String = body.chars().take(500).collect();
            format!(
                "Invalid iTunes JSON for {artist} - {album} from {final_url}: {error}\nResponse preview: {preview}"
            )
        })?;

        Ok(payload.results)
    }

    async fn search_album(
        &self,
        artist: &str,
        album: &str,
    ) -> Result<Vec<ArtworkReference>, String> {
        let terms = [format!("{artist} {album}"), album.to_string()];
        let mut references = Vec::new();
        let mut seen = HashSet::new();

        for term in terms {
            for item in self.search_query(artist, album, &term).await? {
                if item.artwork_url_100.trim().is_empty()
                    || !same_text(&item.artist_name, artist)
                    || !title_matches(&item.collection_name, album)
                    || !seen.insert(item.collection_id)
                {
                    continue;
                }

                references.push(ArtworkReference {
                    source: "itunes".to_string(),
                    id: item.collection_id.to_string(),
                    url: high_resolution_artwork_url(&item.artwork_url_100, ARTWORK_SIZE),
                    front: true,
                    approved: true,
                    types: vec!["Front".to_string()],
                });
            }

            if !references.is_empty() {
                break;
            }
        }

        Ok(references)
    }

    async fn discover_release_context(
        &self,
        context: &ProviderContext,
    ) -> Result<Vec<ArtworkReference>, String> {
        let artist = context.artist_credit.trim();
        let release_title = context.release_title.trim();

        if artist.is_empty() || release_title.is_empty() {
            return Ok(Vec::new());
        }

        let mut titles = Vec::new();

        if let Some(group_title) = context.release_group_title.as_deref() {
            let group_title = group_title.trim();
            if !group_title.is_empty() {
                titles.push(group_title.to_string());
            }
        }

        if !titles.iter().any(|title| same_text(title, release_title)) {
            titles.push(release_title.to_string());
        }

        let mut references = Vec::new();
        let mut seen = HashSet::new();

        for title in titles {
            for reference in self.search_album(artist, &title).await? {
                if seen.insert(reference.id.clone()) {
                    references.push(reference);
                }
            }
        }

        Ok(references)
    }
}

impl ArtworkProvider for ITunes {
    fn name(&self) -> &'static str {
        "itunes"
    }

    fn discover<'a>(
        &'a self,
        _query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move { self.discover_release_context(context).await })
    }
}

fn high_resolution_artwork_url(url: &str, size: u32) -> String {
    let replacement = format!("{size}x{size}bb");
    url.replacen("100x100bb", &replacement, 1)
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn same_text(left: &str, right: &str) -> bool {
    normalized(left) == normalized(right)
}

fn title_matches(found: &str, requested: &str) -> bool {
    let found = normalized(found);
    let requested = normalized(requested);

    !found.is_empty()
        && !requested.is_empty()
        && (found == requested || found.starts_with(&requested) || requested.starts_with(&found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_response_parses_album_identity_and_artwork() {
        let json = r#"
        {
            "resultCount": 1,
            "results": [
                {
                    "collectionId": 573962245,
                    "collectionName": "Unorthodox Jukebox",
                    "artistName": "Bruno Mars",
                    "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/Music/v4/example/100x100bb.jpg"
                }
            ]
        }
        "#;

        let response: SearchResponse = serde_json::from_str(json).expect("fixture should parse");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].collection_id, 573962245);
        assert_eq!(response.results[0].artist_name, "Bruno Mars");
        assert_eq!(response.results[0].collection_name, "Unorthodox Jukebox");
    }

    #[test]
    fn missing_results_defaults_to_empty() {
        let response: SearchResponse = serde_json::from_str("{}").expect("fixture should parse");
        assert!(response.results.is_empty());
    }

    #[test]
    fn high_resolution_artwork_url_replaces_apple_size_token() {
        assert_eq!(
            high_resolution_artwork_url(
                "https://is1-ssl.mzstatic.com/image/thumb/Music/v4/example/100x100bb.jpg",
                3000,
            ),
            "https://is1-ssl.mzstatic.com/image/thumb/Music/v4/example/3000x3000bb.jpg"
        );
    }

    #[test]
    fn unknown_artwork_url_shape_is_preserved() {
        let url = "https://example.invalid/cover.jpg";
        assert_eq!(high_resolution_artwork_url(url, 3000), url);
    }

    #[test]
    fn matching_ignores_case_space_and_punctuation() {
        assert!(same_text("Bruno Mars", "BRUNO MARS"));
        assert!(same_text("AC/DC", "ACDC"));
    }

    #[test]
    fn title_match_allows_provider_edition_suffix() {
        assert!(title_matches(
            "Unorthodox Jukebox (Deluxe Edition)",
            "Unorthodox Jukebox"
        ));
    }

    #[test]
    fn title_match_allows_musicbrainz_edition_suffix() {
        assert!(title_matches(
            "Unorthodox Jukebox",
            "Unorthodox Jukebox (Hi-Res Version)"
        ));
    }

    #[test]
    fn title_match_rejects_unrelated_album() {
        assert!(!title_matches("24K Magic", "Unorthodox Jukebox"));
    }
}

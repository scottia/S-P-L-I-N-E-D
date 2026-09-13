use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

const SEARCH_URL: &str = "https://api.deezer.com/search/album";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const SEARCH_LIMIT: usize = 25;

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<SearchAlbum>,
}

#[derive(Debug, Deserialize)]
struct SearchAlbum {
    id: u64,
    title: String,
    artist: SearchArtist,
}

#[derive(Debug, Deserialize)]
struct SearchArtist {
    name: String,
}

pub struct Deezer {
    client: Client,
}

impl Deezer {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| format!("Unable to create Deezer HTTP client: {error}"))?;

        Ok(Self { client })
    }

    async fn search_query(
        &self,
        artist: &str,
        album: &str,
        query: &str,
    ) -> Result<Vec<SearchAlbum>, String> {
        let limit = SEARCH_LIMIT.to_string();

        let response = self
            .client
            .get(SEARCH_URL)
            .query(&[("q", query), ("limit", limit.as_str())])
            .send()
            .await
            .map_err(|error| format!("Unable to query Deezer for {artist} - {album}: {error}"))?;

        let status = response.status();
        let final_url = response.url().clone();

        if status != StatusCode::OK {
            return Err(format!(
                "Deezer search for {artist} - {album} returned HTTP {status} from {final_url}"
            ));
        }

        let body = response.text().await.map_err(|error| {
            format!(
                "Unable to read Deezer response for {artist} - {album} from {final_url}: {error}"
            )
        })?;

        let payload = serde_json::from_str::<SearchResponse>(&body).map_err(|error| {
            let preview: String = body.chars().take(500).collect();
            format!(
                "Invalid Deezer JSON for {artist} - {album} from {final_url}: {error}\nResponse preview: {preview}"
            )
        })?;

        Ok(payload.data)
    }

    async fn search_album(
        &self,
        artist: &str,
        album: &str,
    ) -> Result<Vec<ArtworkReference>, String> {
        let queries = [
            format!("artist:\"{artist}\" album:\"{album}\""),
            format!("{artist} {album}"),
            album.to_string(),
        ];

        let mut references = Vec::new();
        let mut seen = HashSet::new();

        for query in queries {
            for item in self.search_query(artist, album, &query).await? {
                if !same_text(&item.artist.name, artist)
                    || !title_matches(&item.title, album)
                    || !seen.insert(item.id)
                {
                    continue;
                }

                references.push(ArtworkReference {
                    source: "deezer".to_string(),
                    id: item.id.to_string(),
                    url: album_image_url(item.id),
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

impl ArtworkProvider for Deezer {
    fn name(&self) -> &'static str {
        "deezer"
    }

    fn discover<'a>(
        &'a self,
        _query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move { self.discover_release_context(context).await })
    }
}

fn album_image_url(album_id: u64) -> String {
    format!("https://api.deezer.com/album/{album_id}/image?size=xl")
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
    fn search_response_parses_album_identity() {
        let json = r#"
        {
            "data": [
                {
                    "id": 6157080,
                    "title": "Unorthodox Jukebox",
                    "cover_xl": "https://cdn-images.dzcdn.net/images/cover/example/1000x1000.jpg",
                    "artist": { "name": "Bruno Mars" }
                }
            ]
        }
        "#;

        let response: SearchResponse = serde_json::from_str(json).expect("fixture should parse");

        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id, 6157080);
        assert_eq!(response.data[0].artist.name, "Bruno Mars");
    }

    #[test]
    fn album_image_url_uses_deezer_redirect_endpoint() {
        assert_eq!(
            album_image_url(6157080),
            "https://api.deezer.com/album/6157080/image?size=xl"
        );
    }

    #[test]
    fn missing_data_defaults_to_empty() {
        let response: SearchResponse = serde_json::from_str("{}").expect("fixture should parse");
        assert!(response.data.is_empty());
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

    #[test]
    fn known_deezer_album_fixture_matches_musicbrainz_context() {
        let item = SearchAlbum {
            id: 6157080,
            title: "Unorthodox Jukebox".to_string(),
            artist: SearchArtist {
                name: "Bruno Mars".to_string(),
            },
        };

        assert!(same_text(&item.artist.name, "Bruno Mars"));
        assert!(title_matches(
            &item.title,
            "Unorthodox Jukebox (Hi-Res Version)"
        ));
    }
}

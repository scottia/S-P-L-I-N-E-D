use crate::credentials::{FANARTTV_API_VERSION, load_fanarttv_credential, resolve_credential_path};
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

const API_BASE_URL: &str = "https://webservice.fanart.tv/v3.2/music/albums";
const ARTIST_API_BASE_URL: &str = "https://webservice.fanart.tv/v3.2/music";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct AlbumResponse {
    #[serde(default)]
    albums: Vec<Album>,
}

#[derive(Debug, Deserialize)]
struct Album {
    release_group_id: String,
    #[serde(default)]
    albumcover: Vec<AlbumCover>,
}

#[derive(Debug, Deserialize)]
struct AlbumCover {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct ArtistResponse {
    #[serde(default)]
    artistthumb: Vec<ArtistImage>,
    #[serde(default)]
    artistbackground: Vec<ArtistImage>,
}

#[derive(Debug, Deserialize)]
struct ArtistImage {
    id: String,
    url: String,
}

pub struct FanartTv {
    client: Client,
    credential_path: PathBuf,
}

impl FanartTv {
    pub fn new(configured_path: &str) -> Result<Self, String> {
        let credential_path = resolve_credential_path(configured_path, "Fanart.tv")?;

        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| format!("Unable to create Fanart.tv HTTP client: {error}"))?;

        Ok(Self {
            client,
            credential_path,
        })
    }

    async fn discover_release_group(
        &self,
        release_group_mbid: &str,
    ) -> Result<Vec<ArtworkReference>, String> {
        let credential = load_fanarttv_credential(&self.credential_path)?;
        let url = format!("{API_BASE_URL}/{release_group_mbid}");
        let mut request = self.client.get(&url).header("api-key", credential.api_key);

        if !credential.client_key.trim().is_empty() {
            request = request.header("client-key", credential.client_key);
        }

        let response = request.send().await.map_err(|error| {
            format!(
                "Unable to query Fanart.tv album artwork for release group {release_group_mbid}: {error}"
            )
        })?;

        let status = response.status();
        let final_url = response.url().clone();

        if status == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(format!(
                "Fanart.tv {FANARTTV_API_VERSION} credentials were rejected."
            ));
        }

        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .map(|value| format!(" Retry after {value} seconds."))
                .unwrap_or_default();
            return Err(format!(
                "Fanart.tv {FANARTTV_API_VERSION} rate limit was reached.{retry_after}"
            ));
        }

        if status != StatusCode::OK {
            return Err(format!(
                "Fanart.tv album artwork for release group {release_group_mbid} returned HTTP {status} from {final_url}"
            ));
        }

        let body = response.text().await.map_err(|error| {
            format!(
                "Unable to read Fanart.tv response for release group {release_group_mbid} from {final_url}: {error}"
            )
        })?;

        let payload = serde_json::from_str::<AlbumResponse>(&body).map_err(|error| {
            let preview: String = body.chars().take(500).collect();
            format!(
                "Invalid Fanart.tv JSON for release group {release_group_mbid} from {final_url}: {error}\nResponse preview: {preview}"
            )
        })?;

        Ok(payload
            .albums
            .into_iter()
            .filter(|album| {
                album
                    .release_group_id
                    .eq_ignore_ascii_case(release_group_mbid)
            })
            .flat_map(|album| album.albumcover)
            .filter(|cover| !cover.url.trim().is_empty())
            .map(|cover| ArtworkReference {
                source: "fanarttv".to_string(),
                id: cover.id,
                url: cover.url,
                front: true,
                approved: true,
                types: vec!["Front".to_string()],
            })
            .collect())
    }

    pub async fn discover_artist(
        &self,
        artist_mbid: &str,
    ) -> Result<Vec<ArtworkReference>, String> {
        let artist_mbid = artist_mbid.trim();
        if artist_mbid.is_empty() {
            return Ok(Vec::new());
        }
        let credential = load_fanarttv_credential(&self.credential_path)?;
        let url = format!("{ARTIST_API_BASE_URL}/{artist_mbid}");
        let mut request = self.client.get(&url).header("api-key", credential.api_key);
        if !credential.client_key.trim().is_empty() {
            request = request.header("client-key", credential.client_key);
        }
        let response = request.send().await.map_err(|error| {
            format!("Unable to query Fanart.tv Artist Art for {artist_mbid}: {error}")
        })?;
        let status = response.status();
        let final_url = response.url().clone();
        if status == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(format!(
                "Fanart.tv {FANARTTV_API_VERSION} credentials were rejected."
            ));
        }
        if status != StatusCode::OK {
            return Err(format!(
                "Fanart.tv Artist Art for {artist_mbid} returned HTTP {status} from {final_url}"
            ));
        }
        let body = response.text().await.map_err(|error| {
            format!("Unable to read Fanart.tv Artist Art from {final_url}: {error}")
        })?;
        let payload = serde_json::from_str::<ArtistResponse>(&body).map_err(|error| {
            format!("Invalid Fanart.tv Artist Art JSON from {final_url}: {error}")
        })?;
        let mut seen = HashSet::new();
        Ok(payload
            .artistthumb
            .into_iter()
            .map(|image| ("Artist Thumb", image))
            .chain(
                payload
                    .artistbackground
                    .into_iter()
                    .map(|image| ("Artist Background", image)),
            )
            .filter(|(_, image)| !image.url.trim().is_empty() && seen.insert(image.url.clone()))
            .map(|(kind, image)| ArtworkReference {
                source: "fanarttv".to_string(),
                id: image.id,
                url: image.url,
                front: true,
                approved: true,
                types: vec![kind.to_string(), "ArtistImage".to_string()],
            })
            .collect())
    }
}

impl ArtworkProvider for FanartTv {
    fn name(&self) -> &'static str {
        "fanarttv"
    }

    fn discover<'a>(
        &'a self,
        _query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move {
            let Some(release_group_mbid) = context.release_group_mbid.as_deref() else {
                return Ok(Vec::new());
            };

            let release_group_mbid = release_group_mbid.trim();
            if release_group_mbid.is_empty() {
                return Ok(Vec::new());
            }

            self.discover_release_group(release_group_mbid).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_credential_path_is_rejected() {
        assert!(FanartTv::new("").is_err());
    }

    #[test]
    fn album_response_parses_album_covers() {
        let json = r#"
        {
            "name": "Fixture Artist",
            "image_count": 1,
            "albums": [
                {
                    "release_group_id": "1b022e01-4da6-387b-8658-8678046e4cef",
                    "albumcover": [
                        {
                            "id": "12345",
                            "url": "https://assets.fanart.tv/fanart/music/example/albumcover/example.jpg",
                            "width": "1000",
                            "height": "1000"
                        }
                    ]
                }
            ]
        }
        "#;

        let response: AlbumResponse = serde_json::from_str(json).expect("fixture should parse");

        assert_eq!(response.albums.len(), 1);
        assert_eq!(response.albums[0].albumcover.len(), 1);
        assert_eq!(response.albums[0].albumcover[0].id, "12345");
        assert!(response.albums[0].albumcover[0].url.contains("fanart.tv"));
    }

    #[test]
    fn missing_albums_defaults_to_empty() {
        let response: AlbumResponse = serde_json::from_str("{}").expect("fixture should parse");
        assert!(response.albums.is_empty());
    }

    #[test]
    fn artist_response_parses_thumbnails_and_backgrounds() {
        let response: ArtistResponse = serde_json::from_str(
            r#"{"artistthumb":[{"id":"1","url":"https://assets.fanart.tv/thumb.jpg"}],"artistbackground":[{"id":"2","url":"https://assets.fanart.tv/background.jpg"}]}"#,
        )
        .expect("fixture should parse");
        assert_eq!(response.artistthumb.len(), 1);
        assert_eq!(response.artistbackground.len(), 1);
    }

    #[test]
    fn v3_object_shaped_albums_are_not_accepted_as_v32() {
        let json = r#"{"albums":{"release-group-id":{"albumcover":[]}}}"#;
        assert!(serde_json::from_str::<AlbumResponse>(json).is_err());
    }
}

use crate::credentials::{load_fanarttv_credential, resolve_credential_path};
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

const API_BASE_URL: &str = "https://webservice.fanart.tv/v3/music/albums";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct AlbumResponse {
    #[serde(default)]
    albumcover: Vec<AlbumCover>,
}

#[derive(Debug, Deserialize)]
struct AlbumCover {
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
            return Err("Fanart.tv credentials were rejected.".to_string());
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
            .albumcover
            .into_iter()
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
            "albumcover": [
                {
                    "id": "12345",
                    "url": "https://assets.fanart.tv/fanart/music/example/albumcover/example.jpg"
                }
            ]
        }
        "#;

        let response: AlbumResponse = serde_json::from_str(json).expect("fixture should parse");

        assert_eq!(response.albumcover.len(), 1);
        assert_eq!(response.albumcover[0].id, "12345");
        assert!(response.albumcover[0].url.contains("fanart.tv"));
    }

    #[test]
    fn missing_albumcover_defaults_to_empty() {
        let response: AlbumResponse = serde_json::from_str("{}").expect("fixture should parse");
        assert!(response.albumcover.is_empty());
    }
}

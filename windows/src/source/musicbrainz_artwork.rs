use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;

const BASE_URL: &str = "https://coverartarchive.org";

#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    images: Vec<ApiImage>,
}

#[derive(Debug, Deserialize)]
struct ApiImage {
    #[serde(deserialize_with = "deserialize_id")]
    id: String,
    image: String,
    front: bool,
    approved: bool,
    #[serde(default)]
    types: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringOrNumber {
    String(String),
    Number(u64),
}

fn deserialize_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(value) => value,
        StringOrNumber::Number(value) => value.to_string(),
    })
}

pub struct MusicBrainzArtwork {
    client: Client,
}

impl MusicBrainzArtwork {
    pub fn new(request_timeout: Duration) -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!(
                "SPLINED/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/scottia/S-P-L-I-N-E-D)"
            ))
            // Python requests and the successful Windows curl path both use
            // HTTP/1.1 across CAA's archive.org redirect chain. Keep the native
            // client on that proven path instead of negotiating HTTP/2 with a
            // rotating archive host.
            .http1_only()
            .timeout(request_timeout)
            .build()
            .map_err(|error| {
                format!("Unable to create MusicBrainz artwork HTTP client: {error}")
            })?;
        Ok(Self { client })
    }

    async fn discover_authority(
        &self,
        query: &ArtworkQuery,
        context: &ProviderContext,
    ) -> Result<Vec<ArtworkReference>, String> {
        let (entity, identity) = context
            .release_group_mbid
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| ("release-group", value))
            .unwrap_or(("release", query.release_mbid.as_str()));
        validate_mbid(identity)?;
        let url = format!("{BASE_URL}/{entity}/{identity}/");
        let response = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| format!("Unable to query MusicBrainz artwork {identity}: {error}"))?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Ok(Vec::new()),
            status => {
                return Err(format!(
                    "MusicBrainz artwork {identity} returned HTTP {status}"
                ));
            }
        }
        let payload = response
            .json::<ApiResponse>()
            .await
            .map_err(|error| format!("Invalid MusicBrainz artwork JSON for {identity}: {error}"))?;
        Ok(payload
            .images
            .into_iter()
            .filter(|image| !image.image.trim().is_empty())
            .map(|image| ArtworkReference {
                source: self.name().to_string(),
                id: image.id,
                url: image.image,
                front: image.front,
                approved: image.approved,
                types: image.types,
            })
            .collect())
    }
}

impl ArtworkProvider for MusicBrainzArtwork {
    fn name(&self) -> &'static str {
        "musicbrainz"
    }

    fn discover<'a>(
        &'a self,
        query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move { self.discover_authority(query, context).await })
    }
}

fn validate_mbid(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(format!("Invalid MusicBrainz artwork MBID: {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_name_is_canonical() {
        let provider =
            MusicBrainzArtwork::new(Duration::from_secs(7)).expect("provider should create");
        assert_eq!(provider.name(), "musicbrainz");
    }

    #[test]
    fn validates_release_or_release_group_mbid() {
        assert!(validate_mbid("76df3287-6cda-33eb-8e9a-044b5e15ffdd").is_ok());
        assert!(validate_mbid("not-an-mbid").is_err());
    }
}

use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

const BASE_URL: &str = "https://coverartarchive.org";

#[derive(Debug, Deserialize)]
struct ApiResponse {
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
    let value = StringOrNumber::deserialize(deserializer)?;

    Ok(match value {
        StringOrNumber::String(value) => value,
        StringOrNumber::Number(value) => value.to_string(),
    })
}

pub struct CoverArtArchive {
    client: Client,
}

impl CoverArtArchive {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("Unable to create Cover Art Archive HTTP client: {error}"))?;

        Ok(Self { client })
    }

    pub async fn discover_release(
        &self,
        release_mbid: &str,
    ) -> Result<Vec<ArtworkReference>, String> {
        validate_release_mbid(release_mbid)?;

        let url = format!("{BASE_URL}/release/{release_mbid}/");

        let response = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| {
                format!("Unable to query Cover Art Archive release {release_mbid}: {error}")
            })?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Ok(Vec::new()),
            status => {
                return Err(format!(
                    "Cover Art Archive release {release_mbid} returned HTTP {status}"
                ));
            }
        }

        let final_url = response.url().clone();

        let body = response.text().await.map_err(|error| {
            format!(
                "Unable to read Cover Art Archive response for release {release_mbid} from {final_url}: {error}"
            )
        })?;

        let payload = serde_json::from_str::<ApiResponse>(&body).map_err(|error| {
            let preview: String = body.chars().take(500).collect();

            format!(
                "Invalid Cover Art Archive JSON for release {release_mbid} from {final_url}: {error}\nResponse preview: {preview}"
            )
        })?;

        Ok(payload
            .images
            .into_iter()
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

impl ArtworkProvider for CoverArtArchive {
    fn name(&self) -> &'static str {
        "coverartarchive"
    }

    fn discover<'a>(
        &'a self,
        query: &'a ArtworkQuery,
        _context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move { self.discover_release(&query.release_mbid).await })
    }
}

fn validate_release_mbid(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();

    let valid_shape = bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit());

    if valid_shape {
        Ok(())
    } else {
        Err(format!("Invalid MusicBrainz release MBID: {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_name_is_canonical() {
        let provider = CoverArtArchive::new().expect("provider should create");
        assert_eq!(provider.name(), "coverartarchive");
    }

    #[test]
    fn valid_release_mbid_is_accepted() {
        assert!(validate_release_mbid("76df3287-6cda-33eb-8e9a-044b5e15ffdd").is_ok());
    }

    #[test]
    fn uppercase_release_mbid_is_accepted() {
        assert!(validate_release_mbid("76DF3287-6CDA-33EB-8E9A-044B5E15FFDD").is_ok());
    }

    #[test]
    fn malformed_release_mbid_is_rejected() {
        assert!(validate_release_mbid("not-an-mbid").is_err());
    }

    #[test]
    fn release_mbid_with_invalid_hex_is_rejected() {
        assert!(validate_release_mbid("76df3287-6cda-33eb-8e9a-044b5e15ffdz").is_err());
    }

    #[test]
    fn api_response_parses_string_image_id() {
        let json = r#"
        {
            "images": [
                {
                    "id": "829521842",
                    "image": "https://example.invalid/image.jpg",
                    "front": true,
                    "approved": true,
                    "types": ["Front"]
                }
            ]
        }
        "#;

        let response: ApiResponse = serde_json::from_str(json).expect("fixture should parse");
        assert_eq!(response.images[0].id, "829521842");
    }

    #[test]
    fn api_response_parses_numeric_image_id() {
        let json = r#"
        {
            "images": [
                {
                    "id": 829521842,
                    "image": "https://example.invalid/image.jpg",
                    "front": true,
                    "approved": true,
                    "types": ["Front"]
                }
            ]
        }
        "#;

        let response: ApiResponse =
            serde_json::from_str(json).expect("numeric id fixture should parse");
        assert_eq!(response.images[0].id, "829521842");
    }

    #[test]
    fn missing_types_defaults_to_empty() {
        let json = r#"
        {
            "images": [
                {
                    "id": 1,
                    "image": "https://example.invalid/image.jpg",
                    "front": false,
                    "approved": true
                }
            ]
        }
        "#;

        let response: ApiResponse = serde_json::from_str(json).expect("fixture should parse");
        assert!(response.images[0].types.is_empty());
    }
}

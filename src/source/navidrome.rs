use crate::download::MAX_DOWNLOAD_BYTES;
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use rand::Rng;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const CLIENT_NAME: &str = "SPLINED";
const PROTOCOL_VERSION: &str = "1.16.1";
const INTERNAL_SCHEME: &str = "splined+navidrome://";

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static DOWNLOAD_SESSIONS: OnceLock<Mutex<HashMap<u64, NavidromeCredential>>> = OnceLock::new();

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct NavidromeCredential {
    server_url: String,
    auth_method: String,
    username: String,
    password: String,
    api_key: String,
    bearer_token: String,
}

impl NavidromeCredential {
    fn load(path: &str) -> Result<Self, String> {
        let path = Path::new(path);
        let body = std::fs::read_to_string(path).map_err(|error| {
            format!(
                "Navidrome credentials are unavailable at {}: {error}",
                path.display()
            )
        })?;
        let mut value: Self = serde_json::from_str(&body).map_err(|error| {
            format!(
                "Navidrome credential file is invalid at {}: {error}",
                path.display()
            )
        })?;
        value.server_url = value.server_url.trim().trim_end_matches('/').to_string();
        value.auth_method = value.auth_method.trim().to_ascii_lowercase();
        value.username = value.username.trim().to_string();
        value.api_key = value.api_key.trim().to_string();
        value.bearer_token = value.bearer_token.trim().to_string();
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), String> {
        let valid_url =
            self.server_url.starts_with("https://") || self.server_url.starts_with("http://");
        if !valid_url {
            return Err("Navidrome server_url must be an absolute HTTP or HTTPS URL.".to_string());
        }
        if self.uses_api_key() {
            if self.api_key.is_empty() {
                return Err(
                    "Navidrome api_key is required for the selected authentication method."
                        .to_string(),
                );
            }
        } else if self.username.is_empty() || self.password.is_empty() {
            return Err(
                "Navidrome username and password are required, or select API key authentication."
                    .to_string(),
            );
        }
        Ok(())
    }

    fn uses_api_key(&self) -> bool {
        if self.auth_method.eq_ignore_ascii_case("api_key") {
            return true;
        }
        if self.auth_method.eq_ignore_ascii_case("password") {
            return false;
        }
        !self.api_key.is_empty() && (self.username.is_empty() || self.password.is_empty())
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "subsonic-response")]
    response: SubsonicResponse,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct SubsonicResponse {
    status: String,
    error: Option<ApiError>,
    #[serde(rename = "searchResult3")]
    search_result: Option<SearchResult>,
}

impl Default for SubsonicResponse {
    fn default() -> Self {
        Self {
            status: String::new(),
            error: None,
            search_result: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct ApiError {
    message: String,
}

impl Default for ApiError {
    fn default() -> Self {
        Self {
            message: String::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchResult {
    album: Vec<RemoteAlbum>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct RemoteAlbum {
    id: String,
    title: String,
    album: String,
    name: String,
    artist: String,
    #[serde(rename = "coverArt")]
    cover_art: String,
    #[serde(rename = "musicBrainzId")]
    musicbrainz_id: String,
}

impl RemoteAlbum {
    fn title(&self) -> &str {
        [&self.album, &self.title, &self.name]
            .into_iter()
            .find(|value| !value.trim().is_empty())
            .map(String::as_str)
            .unwrap_or("")
    }
}

pub struct Navidrome {
    client: Client,
    credential: Result<NavidromeCredential, String>,
    session_id: Option<u64>,
}

impl Navidrome {
    pub fn new(credential_file: &str) -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("Unable to create Navidrome HTTP client: {error}"))?;
        let credential = NavidromeCredential::load(credential_file);
        let session_id = credential.as_ref().ok().map(|value| {
            let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
            download_sessions()
                .lock()
                .expect("Navidrome download session lock poisoned")
                .insert(session, value.clone());
            session
        });
        Ok(Self {
            client,
            credential,
            session_id,
        })
    }

    async fn discover_album(
        &self,
        query: &ArtworkQuery,
        context: &ProviderContext,
    ) -> Result<Vec<ArtworkReference>, String> {
        let credential = self.credential.as_ref().map_err(Clone::clone)?;
        let search = format!("{} {}", context.artist_credit, context.release_title);
        let request = authenticated_request(
            &self.client,
            credential,
            "search3",
            &[
                ("query", search),
                ("artistCount", "0".to_string()),
                ("albumCount", "50".to_string()),
                ("songCount", "0".to_string()),
            ],
        );
        let response = request
            .send()
            .await
            .map_err(|_| "Unable to query Navidrome/OpenSubsonic.".to_string())?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err("Navidrome rejected the saved authentication credentials.".to_string());
        }
        let response = response
            .error_for_status()
            .map_err(|_| "Navidrome/OpenSubsonic search failed.".to_string())?;
        let envelope = response
            .json::<Envelope>()
            .await
            .map_err(|error| format!("Navidrome/OpenSubsonic returned invalid JSON: {error}"))?;
        validate_response(&envelope.response)?;
        let albums = envelope
            .response
            .search_result
            .map(|result| result.album)
            .unwrap_or_default();
        let matches = match_albums(query, context, albums);
        let session_id = self
            .session_id
            .ok_or_else(|| "Navidrome credential session is unavailable.".to_string())?;
        Ok(matches
            .into_iter()
            .filter(|album| !album.cover_art.trim().is_empty())
            .map(|album| ArtworkReference {
                source: self.name().to_string(),
                id: if album.id.is_empty() {
                    album.cover_art.clone()
                } else {
                    album.id
                },
                url: internal_cover_url(session_id, &album.cover_art),
                front: true,
                approved: true,
                types: vec!["Front".to_string()],
            })
            .collect())
    }
}

impl ArtworkProvider for Navidrome {
    fn name(&self) -> &'static str {
        "navidrome"
    }

    fn discover<'a>(
        &'a self,
        query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move { self.discover_album(query, context).await })
    }
}

pub fn is_internal_url(url: &str) -> bool {
    url.starts_with(INTERNAL_SCHEME)
}

pub async fn download_cover(client: &Client, internal_url: &str) -> Result<Vec<u8>, String> {
    let path = internal_url
        .strip_prefix(INTERNAL_SCHEME)
        .ok_or_else(|| "Invalid internal Navidrome artwork reference.".to_string())?;
    let (session, cover_hex) = path
        .split_once('/')
        .ok_or_else(|| "Invalid internal Navidrome artwork reference.".to_string())?;
    let session = session
        .parse::<u64>()
        .map_err(|_| "Invalid internal Navidrome artwork session.".to_string())?;
    let cover_id = decode_hex(cover_hex)?;
    let credential = download_sessions()
        .lock()
        .map_err(|_| "Navidrome download session is unavailable.".to_string())?
        .get(&session)
        .cloned()
        .ok_or_else(|| "Navidrome download session has expired.".to_string())?;
    let response = authenticated_request(client, &credential, "getCoverArt", &[("id", cover_id)])
        .send()
        .await
        .map_err(|_| "Unable to download Navidrome cover art.".to_string())?;
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return Err("Navidrome rejected the saved authentication credentials.".to_string());
    }
    if response.content_length().unwrap_or(0) > MAX_DOWNLOAD_BYTES as u64 {
        return Err("Navidrome cover art exceeded the download size limit.".to_string());
    }
    let response = response
        .error_for_status()
        .map_err(|_| "Navidrome cover-art download failed.".to_string())?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Unable to read Navidrome cover art: {error}"))?;
    if bytes.len() > MAX_DOWNLOAD_BYTES {
        return Err("Navidrome cover art exceeded the download size limit.".to_string());
    }
    Ok(bytes.to_vec())
}

fn authenticated_request(
    client: &Client,
    credential: &NavidromeCredential,
    method: &str,
    parameters: &[(impl AsRef<str>, String)],
) -> RequestBuilder {
    let mut query = vec![
        ("v".to_string(), PROTOCOL_VERSION.to_string()),
        ("c".to_string(), CLIENT_NAME.to_string()),
        ("f".to_string(), "json".to_string()),
    ];
    if credential.uses_api_key() {
        query.push(("apiKey".to_string(), credential.api_key.clone()));
    } else {
        let mut salt_bytes = [0_u8; 6];
        rand::rng().fill_bytes(&mut salt_bytes);
        let salt = encode_hex(&salt_bytes);
        let token =
            crate::source::lastfm::md5_hex(format!("{}{}", credential.password, salt).as_bytes());
        query.push(("u".to_string(), credential.username.clone()));
        query.push(("s".to_string(), salt));
        query.push(("t".to_string(), token));
    }
    for (key, value) in parameters {
        query.push((key.as_ref().to_string(), value.clone()));
    }
    // X-ND-Authorization is for Navidrome's separate native Web API. The
    // OpenSubsonic /rest contract uses exactly one query authentication mode.
    client
        .get(format!("{}/rest/{method}.view", credential.server_url))
        .query(&query)
}

fn validate_response(response: &SubsonicResponse) -> Result<(), String> {
    if response.status.eq_ignore_ascii_case("ok") {
        return Ok(());
    }
    let detail = response
        .error
        .as_ref()
        .map(|error| error.message.trim())
        .filter(|message| !message.is_empty())
        .map(|message| format!(": {message}"))
        .unwrap_or_default();
    Err(format!("Navidrome/OpenSubsonic request failed{detail}"))
}

fn match_albums(
    query: &ArtworkQuery,
    context: &ProviderContext,
    albums: Vec<RemoteAlbum>,
) -> Vec<RemoteAlbum> {
    if !query.release_mbid.trim().is_empty() {
        let by_mbid: Vec<_> = albums
            .iter()
            .filter(|album| {
                album
                    .musicbrainz_id
                    .eq_ignore_ascii_case(&query.release_mbid)
            })
            .cloned()
            .collect();
        if !by_mbid.is_empty() {
            return by_mbid;
        }
    }
    let artist = normalize_identity(&context.artist_credit);
    let title = normalize_identity(&context.release_title);
    let exact: Vec<_> = albums
        .into_iter()
        .filter(|album| {
            normalize_identity(&album.artist) == artist
                && normalize_identity(album.title()) == title
        })
        .collect();
    if exact.len() == 1 { exact } else { Vec::new() }
}

fn normalize_identity(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn download_sessions() -> &'static Mutex<HashMap<u64, NavidromeCredential>> {
    DOWNLOAD_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn internal_cover_url(session_id: u64, cover_id: &str) -> String {
    format!(
        "{INTERNAL_SCHEME}{session_id}/{}",
        encode_hex(cover_id.as_bytes())
    )
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Result<String, String> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid internal Navidrome cover-art identity.".to_string());
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Invalid internal Navidrome cover-art identity.".to_string())?;
    String::from_utf8(bytes)
        .map_err(|_| "Invalid internal Navidrome cover-art identity.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_match_prefers_release_mbid() {
        let query = ArtworkQuery::release("release-id");
        let context = ProviderContext {
            artist_credit: "The Artist".to_string(),
            release_title: "The Album".to_string(),
            ..ProviderContext::default()
        };
        let albums = vec![
            RemoteAlbum {
                id: "wrong".to_string(),
                artist: "The Artist".to_string(),
                album: "The Album".to_string(),
                ..RemoteAlbum::default()
            },
            RemoteAlbum {
                id: "right".to_string(),
                musicbrainz_id: "release-id".to_string(),
                ..RemoteAlbum::default()
            },
        ];
        assert_eq!(match_albums(&query, &context, albums)[0].id, "right");
    }

    #[test]
    fn fallback_requires_one_exact_identity() {
        let query = ArtworkQuery::release("");
        let context = ProviderContext {
            artist_credit: "Beyoncé".to_string(),
            release_title: "Renaissance".to_string(),
            ..ProviderContext::default()
        };
        let album = RemoteAlbum {
            id: "album".to_string(),
            artist: "Beyoncé".to_string(),
            album: "Renaissance".to_string(),
            ..RemoteAlbum::default()
        };
        assert_eq!(match_albums(&query, &context, vec![album.clone()]).len(), 1);
        assert!(match_albums(&query, &context, vec![album.clone(), album]).is_empty());
    }

    #[test]
    fn internal_reference_never_contains_credential_data() {
        let reference = internal_cover_url(7, "cover/album");
        assert_eq!(reference, "splined+navidrome://7/636f7665722f616c62756d");
        assert_eq!(
            decode_hex(reference.rsplit('/').next().unwrap()).unwrap(),
            "cover/album"
        );
    }

    #[test]
    fn api_key_authentication_does_not_require_bearer_or_username() {
        let credential = NavidromeCredential {
            server_url: "https://music.example".to_string(),
            auth_method: "api_key".to_string(),
            api_key: "api-secret".to_string(),
            ..NavidromeCredential::default()
        };
        credential.validate().unwrap();
        let parameters: Vec<(String, String)> = Vec::new();
        let request = authenticated_request(&Client::new(), &credential, "ping", &parameters)
            .build()
            .unwrap();
        assert!(request.url().as_str().contains("apiKey=api-secret"));
        assert!(!request.url().as_str().contains("u="));
        assert!(request.headers().get("X-ND-Authorization").is_none());
    }

    #[test]
    fn password_authentication_does_not_require_bearer() {
        let credential = NavidromeCredential {
            server_url: "https://music.example".to_string(),
            auth_method: "password".to_string(),
            username: "listener".to_string(),
            password: "secret".to_string(),
            ..NavidromeCredential::default()
        };
        credential.validate().unwrap();
        let parameters: Vec<(String, String)> = Vec::new();
        let request = authenticated_request(&Client::new(), &credential, "ping", &parameters)
            .build()
            .unwrap();
        assert!(request.url().as_str().contains("u=listener"));
        assert!(request.url().as_str().contains("s="));
        assert!(request.url().as_str().contains("t="));
        assert!(!request.url().as_str().contains("secret"));
        assert!(request.headers().get("X-ND-Authorization").is_none());
    }

    #[test]
    fn legacy_native_bearer_is_not_sent_to_opensubsonic() {
        let credential = NavidromeCredential {
            server_url: "https://music.example".to_string(),
            auth_method: "api_key".to_string(),
            api_key: "api-secret".to_string(),
            bearer_token: "legacy-native-jwt".to_string(),
            ..NavidromeCredential::default()
        };
        let parameters: Vec<(String, String)> = Vec::new();
        let request = authenticated_request(&Client::new(), &credential, "ping", &parameters)
            .build()
            .unwrap();
        assert!(request.headers().get("X-ND-Authorization").is_none());
    }
}

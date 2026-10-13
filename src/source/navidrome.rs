use crate::download::MAX_DOWNLOAD_BYTES;
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use rand::Rng;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::error::Error as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const CLIENT_NAME: &str = "SPLINED";
const PROTOCOL_VERSION: &str = "1.16.1";
const INTERNAL_SCHEME: &str = "splined+navidrome://";

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static DOWNLOAD_SESSIONS: OnceLock<Mutex<HashMap<u64, NavidromeCredential>>> = OnceLock::new();

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct NavidromeCredential {
    pub server_url: String,
    pub auth_method: String,
    pub username: String,
    pub password: String,
    pub api_key: String,
    pub bearer_token: String,
}

impl NavidromeCredential {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
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

    pub fn validate(&self) -> Result<(), String> {
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

    pub fn uses_api_key(&self) -> bool {
        if self.auth_method.eq_ignore_ascii_case("api_key") {
            return true;
        }
        if self.auth_method.eq_ignore_ascii_case("password") {
            return false;
        }
        !self.api_key.is_empty() && (self.username.is_empty() || self.password.is_empty())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavidromeCapabilities {
    pub connected: bool,
    pub library_available: bool,
    pub artist_artwork_available: bool,
    pub album_artwork_available: bool,
    pub path_enrichment_available: bool,
    pub open_subsonic: bool,
    pub server_type: String,
    pub server_version: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavidromeArtist {
    pub id: String,
    pub name: String,
    pub cover_art_id: String,
    pub artist_image_url: String,
    pub musicbrainz_id: String,
    pub album_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavidromeAlbum {
    pub id: String,
    pub artist_id: String,
    pub artist: String,
    pub title: String,
    pub cover_art_id: String,
    pub musicbrainz_id: String,
    pub year: String,
    pub song_count: u32,
}

/// Read-only OpenSubsonic-first companion projection. SPLINED's SQLite model
/// remains authoritative; this client only supplies optional server metadata
/// and artwork for matching local artists/albums.
#[derive(Clone)]
pub struct NavidromeCompanion {
    client: Client,
    credential: NavidromeCredential,
}

impl NavidromeCompanion {
    pub fn new(credential: NavidromeCredential) -> Result<Self, String> {
        credential.validate()?;
        let client = navidrome_client(&credential.server_url)?;
        Ok(Self { client, credential })
    }

    pub async fn capabilities(&self) -> Result<NavidromeCapabilities, String> {
        let ping = self.request_value("ping", &[]).await?;
        let mut result = NavidromeCapabilities {
            connected: true,
            open_subsonic: json_bool(&ping, "openSubsonic"),
            server_type: json_text(&ping, "type"),
            server_version: json_text(&ping, "serverVersion"),
            ..NavidromeCapabilities::default()
        };
        let folders = self.request_value("getMusicFolders", &[]).await?;
        result.library_available = nested_array(&folders, "musicFolders", "musicFolder")
            .is_some_and(|items| !items.is_empty());

        if let Ok(recent) = self
            .request_value(
                "getAlbumList2",
                &[("type", "recent".to_string()), ("size", "1".to_string())],
            )
            .await
            && let Some(album) =
                nested_array(&recent, "albumList2", "album").and_then(|items| items.first())
        {
            result.album_artwork_available = !json_text(album, "coverArt").is_empty();
            let artist_id = json_text(album, "artistId");
            if !artist_id.is_empty()
                && let Ok(info) = self
                    .request_value("getArtistInfo2", &[("id", artist_id)])
                    .await
                && let Some(info) = info.get("artistInfo2")
            {
                result.artist_artwork_available =
                    ["smallImageUrl", "mediumImageUrl", "largeImageUrl"]
                        .iter()
                        .any(|key| !json_text(info, key).is_empty());
            }
            let album_id = json_text(album, "id");
            if !album_id.is_empty()
                && let Ok(details) = self.request_value("getAlbum", &[("id", album_id)]).await
                && let Some(album) = details.get("album")
            {
                result.path_enrichment_available = album
                    .get("song")
                    .and_then(Value::as_array)
                    .is_some_and(|songs| {
                        songs.iter().any(|song| !json_text(song, "path").is_empty())
                    });
            }
        }
        Ok(result)
    }

    pub async fn artists(&self) -> Result<Vec<NavidromeArtist>, String> {
        let response = self.request_value("getArtists", &[]).await?;
        let mut result = Vec::new();
        if let Some(indexes) = response
            .get("artists")
            .and_then(|artists| artists.get("index"))
            .and_then(Value::as_array)
        {
            for index in indexes {
                for value in index
                    .get("artist")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    result.push(NavidromeArtist {
                        id: json_text(value, "id"),
                        name: json_text(value, "name"),
                        cover_art_id: json_text(value, "coverArt"),
                        artist_image_url: json_text(value, "artistImageUrl"),
                        musicbrainz_id: json_text(value, "musicBrainzId"),
                        album_count: json_u32(value, "albumCount"),
                    });
                }
            }
        }
        Ok(result)
    }

    pub async fn artist_albums(&self, artist_id: &str) -> Result<Vec<NavidromeAlbum>, String> {
        if artist_id.trim().is_empty() {
            return Err("Navidrome artist identity is required.".to_string());
        }
        let response = self
            .request_value("getArtist", &[("id", artist_id.to_string())])
            .await?;
        Ok(response
            .get("artist")
            .and_then(|artist| artist.get("album"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|value| NavidromeAlbum {
                id: json_text(value, "id"),
                artist_id: json_text(value, "artistId"),
                artist: json_text(value, "artist"),
                title: ["album", "title", "name"]
                    .iter()
                    .map(|key| json_text(value, key))
                    .find(|text| !text.is_empty())
                    .unwrap_or_default(),
                cover_art_id: json_text(value, "coverArt"),
                musicbrainz_id: json_text(value, "musicBrainzId"),
                year: json_text(value, "year"),
                song_count: json_u32(value, "songCount"),
            })
            .collect())
    }

    pub async fn cache_cover_art(
        &self,
        cover_art_id: &str,
        cache_directory: &Path,
        size: u32,
    ) -> Result<Option<PathBuf>, String> {
        if cover_art_id.trim().is_empty() {
            return Ok(None);
        }
        std::fs::create_dir_all(cache_directory).map_err(|error| {
            format!(
                "Unable to create Navidrome artwork cache {}: {error}",
                cache_directory.display()
            )
        })?;
        let identity = Sha256::digest(
            format!("{}\n{}\n{size}", self.credential.server_url, cover_art_id).as_bytes(),
        );
        let filename = identity
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let path = cache_directory.join(format!("{filename}.img"));
        if path.is_file() && crate::inspect::inspect_image(&path).is_ok() {
            return Ok(Some(path));
        }
        let mut parameters = vec![("id", cover_art_id.to_string())];
        if size > 0 {
            parameters.push(("size", size.to_string()));
        }
        let response =
            authenticated_request(&self.client, &self.credential, "getCoverArt", &parameters)
                .send()
                .await
                .map_err(|error| safe_request_error("download Navidrome cover art", error))?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err("Navidrome rejected the saved authentication credentials.".to_string());
        }
        if response.content_length().unwrap_or(0) > MAX_DOWNLOAD_BYTES as u64 {
            return Err("Navidrome cover art exceeded the download size limit.".to_string());
        }
        let bytes = response
            .error_for_status()
            .map_err(|_| "Navidrome cover-art download failed.".to_string())?
            .bytes()
            .await
            .map_err(|error| format!("Unable to read Navidrome cover art: {error}"))?;
        if bytes.is_empty() || bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err("Navidrome returned invalid cover-art bytes.".to_string());
        }
        crate::safe_write::replace_binary_file(
            &path,
            &bytes,
            "Navidrome artwork cache",
            |staged| crate::inspect::inspect_image(staged).map(|_| ()),
        )?;
        Ok(Some(path))
    }

    async fn request_value(
        &self,
        method: &str,
        parameters: &[(&str, String)],
    ) -> Result<Value, String> {
        let response = authenticated_request(&self.client, &self.credential, method, parameters)
            .send()
            .await
            .map_err(|error| {
                safe_request_error(&format!("query Navidrome/OpenSubsonic {method}"), error)
            })?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err("Navidrome rejected the saved authentication credentials.".to_string());
        }
        let document = response
            .error_for_status()
            .map_err(|_| format!("Navidrome/OpenSubsonic {method} failed."))?
            .json::<Value>()
            .await
            .map_err(|error| format!("Navidrome/OpenSubsonic returned invalid JSON: {error}"))?;
        let response = document
            .get("subsonic-response")
            .ok_or_else(|| "Navidrome/OpenSubsonic response envelope is missing.".to_string())?;
        validate_value_response(response)?;
        Ok(response.clone())
    }
}

pub fn match_companion_album<'a>(
    artist: &str,
    title: &str,
    release_mbid: &str,
    release_year: &str,
    albums: &'a [NavidromeAlbum],
) -> Option<&'a NavidromeAlbum> {
    if !release_mbid.trim().is_empty()
        && let Some(album) = albums.iter().find(|album| {
            album
                .musicbrainz_id
                .eq_ignore_ascii_case(release_mbid.trim())
        })
    {
        return Some(album);
    }
    let artist = normalize_identity(artist);
    let title = normalize_identity(title);
    let mut exact = albums.iter().filter(|album| {
        normalize_identity(&album.artist) == artist && normalize_identity(&album.title) == title
    });
    let candidate = exact.next()?;
    if exact.next().is_some()
        || (!release_year.trim().is_empty()
            && !candidate.year.trim().is_empty()
            && !candidate.year.eq_ignore_ascii_case(release_year.trim()))
    {
        return None;
    }
    Some(candidate)
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "subsonic-response")]
    response: SubsonicResponse,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SubsonicResponse {
    status: String,
    error: Option<ApiError>,
    #[serde(rename = "searchResult3")]
    search_result: Option<SearchResult>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ApiError {
    message: String,
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
        let credential = NavidromeCredential::load(credential_file);
        let client = navidrome_client(
            credential
                .as_ref()
                .map(|value| value.server_url.as_str())
                .unwrap_or_default(),
        )?;
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
            .map_err(|error| safe_request_error("query Navidrome/OpenSubsonic", error))?;
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

fn navidrome_client(server_url: &str) -> Result<Client, String> {
    let mut builder = Client::builder().user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")));
    // Local Navidrome installations are common. Never route a loopback server
    // through a machine-wide HTTP proxy; doing so both breaks valid local
    // connections and exposes authentication query parameters to that proxy.
    if reqwest::Url::parse(server_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
    {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|error| format!("Unable to create Navidrome HTTP client: {error}"))
}

pub fn save_credential(path: &Path, credential: &NavidromeCredential) -> Result<(), String> {
    let mut credential = credential.clone();
    credential.server_url = credential
        .server_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    credential.auth_method = credential.auth_method.trim().to_ascii_lowercase();
    credential.username = credential.username.trim().to_string();
    credential.api_key = credential.api_key.trim().to_string();
    credential.bearer_token = credential.bearer_token.trim().to_string();
    credential.validate()?;
    let json = serde_json::to_string_pretty(&credential)
        .map_err(|error| format!("Unable to serialize Navidrome credential: {error}"))?;
    crate::safe_write::replace_text_file(
        path,
        &format!("{json}\n"),
        "Navidrome credential",
        |candidate| {
            let parsed: NavidromeCredential = serde_json::from_str(candidate)
                .map_err(|error| format!("Invalid staged Navidrome credential: {error}"))?;
            parsed.validate()
        },
    )
}

pub async fn test_credential(credential: &NavidromeCredential) -> Result<String, String> {
    let capabilities = NavidromeCompanion::new(credential.clone())
        .map_err(|error| format!("Navidrome credential is invalid: {error}"))?
        .capabilities()
        .await
        .map_err(|error| format!("Unable to verify Navidrome/OpenSubsonic access: {error}"))?;
    Ok(format!(
        "Connected to Navidrome/OpenSubsonic. Library access: {}; album artwork: {}; artist artwork: {}; path enrichment: {}.",
        yes_no(capabilities.library_available),
        yes_no(capabilities.album_artwork_available),
        yes_no(capabilities.artist_artwork_available),
        yes_no(capabilities.path_enrichment_available),
    ))
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
        .map_err(|error| safe_request_error("download Navidrome cover art", error))?;
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

fn safe_request_error(action: &str, error: reqwest::Error) -> String {
    // OpenSubsonic API-key authentication is carried in the query string.
    // Strip the URL before returning diagnostics so failures cannot leak it.
    let error = error.without_url();
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let next = cause.source();
        let description = cause.to_string().replace(['\r', '\n'], " ");
        if !description.is_empty() && !detail.contains(&description) {
            detail.push_str(": ");
            detail.push_str(&description);
        }
        source = next;
    }
    format!("Unable to {action}: {detail}")
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

fn validate_value_response(response: &Value) -> Result<(), String> {
    if json_text(response, "status").eq_ignore_ascii_case("ok") {
        return Ok(());
    }
    let detail = response
        .get("error")
        .map(|error| json_text(error, "message"))
        .filter(|message| !message.is_empty())
        .map(|message| format!(": {message}"))
        .unwrap_or_default();
    Err(format!("Navidrome/OpenSubsonic request failed{detail}"))
}

fn nested_array<'a>(value: &'a Value, object: &str, array: &str) -> Option<&'a Vec<Value>> {
    value.get(object)?.get(array)?.as_array()
}

fn json_text(value: &Value, name: &str) -> String {
    value
        .get(name)
        .map(|value| match value {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            Value::Bool(value) => value.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

fn json_u32(value: &Value, name: &str) -> u32 {
    value
        .get(name)
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn json_bool(value: &Value, name: &str) -> bool {
    value.get(name).is_some_and(|value| {
        value.as_bool().unwrap_or_else(|| {
            value
                .as_str()
                .is_some_and(|text| text.eq_ignore_ascii_case("true"))
        })
    })
}

fn yes_no(value: bool) -> &'static str {
    if value { "available" } else { "not reported" }
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
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
    use std::thread;
    use std::time::Duration;

    struct MockNavidrome {
        url: String,
        requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        address: std::net::SocketAddr,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl MockNavidrome {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let worker_requests = Arc::clone(&requests);
            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || {
                while !worker_stop.load(AtomicOrdering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // Windows can inherit the listener's nonblocking
                            // mode on accepted sockets. The mock must wait for
                            // the complete request instead of treating an
                            // initial WouldBlock as an empty HTTP request.
                            stream.set_nonblocking(false).unwrap();
                            let request = read_request(&mut stream);
                            worker_requests.lock().unwrap().push(request.clone());
                            write_response(&mut stream, &request);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                url: format!("http://{address}"),
                requests,
                stop,
                address,
                worker: Some(worker),
            }
        }

        fn credential(&self) -> NavidromeCredential {
            NavidromeCredential {
                server_url: self.url.clone(),
                auth_method: "password".to_string(),
                username: "listener".to_string(),
                password: "private-password".to_string(),
                ..NavidromeCredential::default()
            }
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for MockNavidrome {
        fn drop(&mut self) {
            self.stop.store(true, AtomicOrdering::Release);
            let _ = TcpStream::connect(self.address);
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 2048];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).unwrap_or(0);
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn write_response(stream: &mut TcpStream, request: &str) {
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or_default();
        let (content_type, body) = if target.contains("/rest/ping.view") {
            ("application/json", ping_response().into_bytes())
        } else if target.contains("/rest/getMusicFolders.view") {
            (
                "application/json",
                ok_response(r#""musicFolders":{"musicFolder":[{"id":"1","name":"Music"}]}"#)
                    .into_bytes(),
            )
        } else if target.contains("/rest/getAlbumList2.view") {
            (
                "application/json",
                ok_response(
                    r#""albumList2":{"album":[{"id":"album-1","artistId":"artist-1","coverArt":"cover-1"}]}"#,
                )
                .into_bytes(),
            )
        } else if target.contains("/rest/getArtistInfo2.view") {
            (
                "application/json",
                ok_response(
                    r#""artistInfo2":{"mediumImageUrl":"https://example.invalid/artist.jpg"}"#,
                )
                .into_bytes(),
            )
        } else if target.contains("/rest/getAlbum.view") {
            (
                "application/json",
                ok_response(r#""album":{"song":[{"path":"Artist/Album/01.flac"}]}"#).into_bytes(),
            )
        } else if target.contains("/rest/getArtists.view") {
            (
                "application/json",
                ok_response(
                    r#""artists":{"index":[{"name":"B","artist":[{"id":"artist-1","name":"Beyoncé","coverArt":"artist-cover","musicBrainzId":"artist-mbid","albumCount":2}]}]}"#,
                )
                .into_bytes(),
            )
        } else if target.contains("/rest/getArtist.view") {
            (
                "application/json",
                ok_response(
                    r#""artist":{"album":[{"id":"album-1","artistId":"artist-1","artist":"Beyoncé","album":"Renaissance","coverArt":"cover-1","musicBrainzId":"release-mbid","year":2022,"songCount":"16"}]}"#,
                )
                .into_bytes(),
            )
        } else if target.contains("/rest/getCoverArt.view") {
            ("image/png", one_pixel_png())
        } else {
            ("application/json", Vec::new())
        };
        let status = if body.is_empty() {
            "404 Not Found"
        } else {
            "200 OK"
        };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
        stream.flush().unwrap();
    }

    fn ping_response() -> String {
        r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.54.4","openSubsonic":true}}"#.to_string()
    }

    fn ok_response(fields: &str) -> String {
        format!(r#"{{"subsonic-response":{{"status":"ok",{fields}}}}}"#)
    }

    fn one_pixel_png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

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

    #[tokio::test]
    async fn capability_probe_verifies_supported_server_features() {
        let server = MockNavidrome::start();
        let companion = NavidromeCompanion::new(server.credential()).unwrap();
        let capabilities = companion.capabilities().await.unwrap();
        assert!(capabilities.connected);
        assert!(capabilities.library_available);
        assert!(capabilities.artist_artwork_available);
        assert!(capabilities.album_artwork_available);
        assert!(capabilities.path_enrichment_available);
        assert!(capabilities.open_subsonic);
        assert_eq!(capabilities.server_type, "navidrome");
        assert_eq!(capabilities.server_version, "0.54.4");

        let requests = server.requests().join("\n");
        assert!(requests.contains("/rest/ping.view"));
        assert!(requests.contains("/rest/getMusicFolders.view"));
        assert!(requests.contains("/rest/getArtistInfo2.view"));
        assert!(requests.contains("/rest/getAlbum.view"));
        assert!(!requests.contains("private-password"));
        assert!(!requests.contains("X-ND-Authorization"));
    }

    #[tokio::test]
    async fn companion_projects_artist_and_album_enrichment() {
        let server = MockNavidrome::start();
        let companion = NavidromeCompanion::new(server.credential()).unwrap();
        let artists = companion.artists().await.unwrap();
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Beyoncé");
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].musicbrainz_id, "artist-mbid");

        let albums = companion.artist_albums(&artists[0].id).await.unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Renaissance");
        assert_eq!(albums[0].song_count, 16);
        assert_eq!(
            match_companion_album("Beyoncé", "Renaissance", "release-mbid", "2022", &albums)
                .unwrap()
                .cover_art_id,
            "cover-1"
        );
    }

    #[tokio::test]
    async fn companion_caches_validated_cover_art_once() {
        let server = MockNavidrome::start();
        let companion = NavidromeCompanion::new(server.credential()).unwrap();
        let cache = tempfile::tempdir().unwrap();
        let first = companion
            .cache_cover_art("cover-1", cache.path(), 512)
            .await
            .unwrap()
            .unwrap();
        let second = companion
            .cache_cover_art("cover-1", cache.path(), 512)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
        assert!(crate::inspect::inspect_image(&first).is_ok());
        let cover_requests = server
            .requests()
            .iter()
            .filter(|request| request.contains("/rest/getCoverArt.view"))
            .count();
        assert_eq!(cover_requests, 1);
    }

    #[tokio::test]
    async fn unavailable_server_is_reported_instead_of_claiming_connected() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let credential = NavidromeCredential {
            server_url: format!("http://{address}"),
            auth_method: "password".to_string(),
            username: "listener".to_string(),
            password: "private-password".to_string(),
            ..NavidromeCredential::default()
        };
        let error = test_credential(&credential).await.unwrap_err();
        assert!(error.contains("Unable to verify Navidrome/OpenSubsonic access"));
        assert!(!error.contains("private-password"));
    }
}

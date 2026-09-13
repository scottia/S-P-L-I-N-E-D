use crate::safe_write::{recover_backup_if_needed, replace_text_file};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep};

const API_BASE_URL: &str = "https://musicbrainz.org/ws/2";
const AUTHORIZE_URL: &str = "https://musicbrainz.org/oauth2/authorize";
const TOKEN_URL: &str = "https://musicbrainz.org/oauth2/token";
const EXPIRY_SAFETY_SECONDS: u64 = 30;
const OAUTH_ERROR_PREVIEW_CHARS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicBrainzConfig {
    pub retry_max: u8,
    pub mb_min_delay: f64,
    pub mb_recording_timeout: f64,
    pub oauth: bool,
    pub client_id: String,
    pub callback_uri: String,
    pub token_file: String,
    pub scope: String,
}

impl Default for MusicBrainzConfig {
    fn default() -> Self {
        Self {
            retry_max: 2,
            mb_min_delay: 1.05,
            mb_recording_timeout: 7.0,
            oauth: false,
            client_id: String::new(),
            callback_uri: "urn:ietf:wg:oauth:2.0:oob".to_string(),
            token_file: String::new(),
            scope: "profile".to_string(),
        }
    }
}

impl PartialEq for MusicBrainzConfig {
    fn eq(&self, other: &Self) -> bool {
        self.retry_max == other.retry_max
            && self.mb_min_delay.to_bits() == other.mb_min_delay.to_bits()
            && self.mb_recording_timeout.to_bits() == other.mb_recording_timeout.to_bits()
            && self.oauth == other.oauth
            && self.client_id == other.client_id
            && self.callback_uri == other.callback_uri
            && self.token_file == other.token_file
            && self.scope == other.scope
    }
}

impl Eq for MusicBrainzConfig {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OAuthCredential {
    pub client_secret: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub token_type: Option<String>,
    pub expires_at_unix: Option<u64>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestMode {
    Anonymous,
    OAuthBearer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationSession {
    pub authorization_url: String,
    pub state: String,
    code_verifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub artist_credit: String,
    pub release_group_id: Option<String>,
    pub release_group_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingRelease {
    pub id: String,
    pub title: String,
    pub artist_credit: String,
    pub status: Option<String>,
    pub date: Option<String>,
    pub release_group_id: Option<String>,
    pub release_group_title: Option<String>,
    pub release_group_primary_type: Option<String>,
    pub release_group_secondary_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingLookup {
    pub id: String,
    pub title: String,
    pub artist_credit: String,
    pub video: bool,
    pub disambiguation: Option<String>,
    pub releases: Vec<RecordingRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingSearchHit {
    pub id: String,
    pub score: u16,
    pub title: String,
    pub artist_credit: String,
    pub video: bool,
    pub disambiguation: Option<String>,
    pub releases: Vec<RecordingRelease>,
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    id: String,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ApiArtistCredit>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(rename = "release-group")]
    release_group: Option<ApiReleaseGroup>,
}

#[derive(Debug, Deserialize)]
struct ApiArtistCredit {
    #[serde(default)]
    name: String,
    #[serde(default)]
    joinphrase: String,
}

#[derive(Debug, Deserialize)]
struct ApiReleaseGroup {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(rename = "primary-type", default)]
    primary_type: Option<String>,
    #[serde(rename = "secondary-types", default)]
    secondary_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiRecording {
    id: String,
    title: String,
    #[serde(default)]
    score: u16,
    #[serde(default)]
    video: Option<bool>,
    #[serde(default)]
    disambiguation: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ApiArtistCredit>,
    #[serde(default)]
    releases: Vec<ApiRelease>,
}

#[derive(Debug, Deserialize)]
struct ApiRecordingSearch {
    #[serde(default)]
    recordings: Vec<ApiRecording>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

pub struct MusicBrainzClient {
    client: Client,
    config: MusicBrainzConfig,
    mode: RequestMode,
    token_path: Option<PathBuf>,
    credential: Mutex<Option<OAuthCredential>>,
    last_request: Mutex<Option<Instant>>,
}

impl MusicBrainzClient {
    pub fn new(config: &MusicBrainzConfig) -> Result<Self, String> {
        validate_config(config)?;

        let client = Client::builder()
            .user_agent(format!(
                "SPLINED/{} (https://github.com/scottia/S-P-L-I-N-E-D)",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|error| format!("Unable to create MusicBrainz HTTP client: {error}"))?;

        if config.oauth {
            let token_path = resolve_token_path(&config.token_file)?;
            let credential = load_credential(&token_path)?;

            if credential.client_secret.trim().is_empty() {
                return Err(format!(
                    "MusicBrainz OAuth credential file contains no client_secret: {}",
                    token_path.display()
                ));
            }

            Ok(Self {
                client,
                config: config.clone(),
                mode: RequestMode::OAuthBearer,
                token_path: Some(token_path),
                credential: Mutex::new(Some(credential)),
                last_request: Mutex::new(None),
            })
        } else {
            Ok(Self {
                client,
                config: config.clone(),
                mode: RequestMode::Anonymous,
                token_path: None,
                credential: Mutex::new(None),
                last_request: Mutex::new(None),
            })
        }
    }

    pub fn request_mode(&self) -> &RequestMode {
        &self.mode
    }

    pub fn token_path(&self) -> Option<&Path> {
        self.token_path.as_deref()
    }

    pub fn begin_authorization(&self) -> Result<AuthorizationSession, String> {
        if !self.config.oauth {
            return Err("MusicBrainz OAuth is disabled in SPLINED configuration.".to_string());
        }

        let code_verifier = random_urlsafe(48);
        let state = random_urlsafe(32);
        let digest = Sha256::digest(code_verifier.as_bytes());
        let code_challenge = URL_SAFE_NO_PAD.encode(digest);

        let authorization_url = reqwest::Url::parse_with_params(
            AUTHORIZE_URL,
            &[
                ("response_type", "code"),
                ("client_id", self.config.client_id.as_str()),
                ("redirect_uri", self.config.callback_uri.as_str()),
                ("scope", self.config.scope.as_str()),
                ("state", state.as_str()),
                ("code_challenge", code_challenge.as_str()),
                ("code_challenge_method", "S256"),
            ],
        )
        .map_err(|error| {
            format!("Unable to construct MusicBrainz OAuth authorization URL: {error}")
        })?
        .to_string();

        Ok(AuthorizationSession {
            authorization_url,
            state,
            code_verifier,
        })
    }

    pub async fn exchange_authorization_code(
        &self,
        session: &AuthorizationSession,
        code: &str,
    ) -> Result<(), String> {
        if !self.config.oauth {
            return Err("MusicBrainz OAuth is disabled in SPLINED configuration.".to_string());
        }
        if code.trim().is_empty() {
            return Err("MusicBrainz OAuth authorization code is empty.".to_string());
        }

        let token_path = self
            .token_path
            .as_ref()
            .ok_or_else(|| "MusicBrainz OAuth token path is unavailable.".to_string())?;
        let current = self
            .credential
            .lock()
            .await
            .clone()
            .ok_or_else(|| "MusicBrainz OAuth credential state is unavailable.".to_string())?;

        let response = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code.trim()),
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", current.client_secret.as_str()),
                ("redirect_uri", self.config.callback_uri.as_str()),
                ("code_verifier", session.code_verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|error| {
                format!("Unable to exchange MusicBrainz OAuth authorization code: {error}")
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("Unable to read MusicBrainz OAuth token response: {error}"))?;

        if !status.is_success() {
            return Err(format!(
                "MusicBrainz OAuth authorization-code exchange returned HTTP {status}: {}",
                sanitized_oauth_error_preview(&body)
            ));
        }

        let token: TokenResponse = serde_json::from_str(&body)
            .map_err(|error| format!("Invalid MusicBrainz OAuth token response: {error}"))?;
        let updated = credential_from_token_response(current, token)?;

        save_credential(token_path, &updated)?;
        *self.credential.lock().await = Some(updated);
        Ok(())
    }

    pub async fn lookup_release(&self, release_mbid: &str) -> Result<Release, String> {
        validate_mbid(release_mbid)?;
        let url = format!("{API_BASE_URL}/release/{release_mbid}");
        let access_token = if self.config.oauth {
            Some(self.valid_access_token().await?)
        } else {
            None
        };

        for attempt in 0..=self.config.retry_max {
            self.wait_for_rate_limit().await;
            let mut request = self
                .client
                .get(&url)
                .query(&[("fmt", "json"), ("inc", "artist-credits+release-groups")]);
            if let Some(token) = access_token.as_deref() {
                request = request.bearer_auth(token);
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(error) if attempt < self.config.retry_max => {
                    let _ = error;
                    continue;
                }
                Err(error) => {
                    return Err(format!(
                        "Unable to query MusicBrainz release {release_mbid} after {} attempt(s): {error}",
                        attempt + 1
                    ));
                }
            };

            let status = response.status();
            match status {
                StatusCode::OK => {}
                StatusCode::NOT_FOUND => {
                    return Err(format!("MusicBrainz release not found: {release_mbid}"));
                }
                StatusCode::UNAUTHORIZED => {
                    return Err("MusicBrainz OAuth authentication was rejected.".to_string());
                }
                status if is_retryable_status(status) && attempt < self.config.retry_max => {
                    continue;
                }
                status => {
                    return Err(format!(
                        "MusicBrainz release {release_mbid} returned HTTP {status} after {} attempt(s)",
                        attempt + 1
                    ));
                }
            }

            let final_url = response.url().clone();
            let body = response.text().await.map_err(|error| {
                format!("Unable to read MusicBrainz response for release {release_mbid}: {error}")
            })?;
            let release: ApiRelease = serde_json::from_str(&body).map_err(|error| {
                let preview: String = body.chars().take(500).collect();
                format!(
                    "Invalid MusicBrainz JSON for release {release_mbid} from {final_url}: {error}\nResponse preview: {preview}"
                )
            })?;

            return Ok(Release {
                id: release.id,
                title: release.title,
                artist_credit: render_artist_credit(&release.artist_credit),
                release_group_id: release.release_group.as_ref().map(|group| group.id.clone()),
                release_group_title: release.release_group.map(|group| group.title),
            });
        }

        unreachable!("MusicBrainz retry loop always performs at least one attempt")
    }

    pub async fn lookup_recording_releases(
        &self,
        recording_mbid: &str,
    ) -> Result<RecordingLookup, String> {
        validate_mbid(recording_mbid)?;
        let url = format!("{API_BASE_URL}/recording/{recording_mbid}");
        let access_token = if self.config.oauth {
            Some(self.valid_access_token().await?)
        } else {
            None
        };

        for attempt in 0..=self.config.retry_max {
            self.wait_for_rate_limit().await;
            let mut request = self
                .client
                .get(&url)
                .timeout(self.recording_timeout())
                .query(&[
                    ("fmt", "json"),
                    ("inc", "artist-credits+releases+release-groups"),
                ]);
            if let Some(token) = access_token.as_deref() {
                request = request.bearer_auth(token);
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(error) if attempt < self.config.retry_max => {
                    let _ = error;
                    continue;
                }
                Err(error) => {
                    return Err(format!(
                        "Unable to query MusicBrainz recording {recording_mbid} after {} attempt(s): {error}",
                        attempt + 1
                    ));
                }
            };

            let status = response.status();
            match status {
                StatusCode::OK => {}
                StatusCode::NOT_FOUND => {
                    return Err(format!("MusicBrainz recording not found: {recording_mbid}"));
                }
                StatusCode::UNAUTHORIZED => {
                    return Err("MusicBrainz OAuth authentication was rejected.".to_string());
                }
                status if is_retryable_status(status) && attempt < self.config.retry_max => {
                    continue;
                }
                status => {
                    return Err(format!(
                        "MusicBrainz recording {recording_mbid} returned HTTP {status} after {} attempt(s)",
                        attempt + 1
                    ));
                }
            }

            let final_url = response.url().clone();
            let body = response.text().await.map_err(|error| {
                format!(
                    "Unable to read MusicBrainz response for recording {recording_mbid}: {error}"
                )
            })?;
            let recording: ApiRecording = serde_json::from_str(&body).map_err(|error| {
                let preview: String = body.chars().take(500).collect();
                format!(
                    "Invalid MusicBrainz JSON for recording {recording_mbid} from {final_url}: {error}\nResponse preview: {preview}"
                )
            })?;

            return Ok(recording_lookup_from_api(recording));
        }

        unreachable!("MusicBrainz retry loop always performs at least one attempt")
    }

    pub async fn search_recordings(
        &self,
        query: &str,
        limit: u8,
    ) -> Result<Vec<RecordingSearchHit>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Err("MusicBrainz recording search query cannot be empty.".to_string());
        }

        let limit = limit.clamp(1, 100);
        let url = format!("{API_BASE_URL}/recording");
        let access_token = if self.config.oauth {
            Some(self.valid_access_token().await?)
        } else {
            None
        };

        for attempt in 0..=self.config.retry_max {
            self.wait_for_rate_limit().await;
            let limit_text = limit.to_string();
            let mut request = self
                .client
                .get(&url)
                .timeout(self.recording_timeout())
                .query(&[
                    ("fmt", "json"),
                    ("query", query),
                    ("limit", limit_text.as_str()),
                ]);
            if let Some(token) = access_token.as_deref() {
                request = request.bearer_auth(token);
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(error) if attempt < self.config.retry_max => {
                    let _ = error;
                    continue;
                }
                Err(error) => {
                    return Err(format!(
                        "Unable to search MusicBrainz recordings after {} attempt(s): {error}",
                        attempt + 1
                    ));
                }
            };

            let status = response.status();
            match status {
                StatusCode::OK => {}
                StatusCode::UNAUTHORIZED => {
                    return Err("MusicBrainz OAuth authentication was rejected.".to_string());
                }
                status if is_retryable_status(status) && attempt < self.config.retry_max => {
                    continue;
                }
                status => {
                    return Err(format!(
                        "MusicBrainz recording search returned HTTP {status} after {} attempt(s)",
                        attempt + 1
                    ));
                }
            }

            let final_url = response.url().clone();
            let body = response.text().await.map_err(|error| {
                format!("Unable to read MusicBrainz recording search response: {error}")
            })?;
            let search: ApiRecordingSearch = serde_json::from_str(&body).map_err(|error| {
                let preview: String = body.chars().take(500).collect();
                format!(
                    "Invalid MusicBrainz recording-search JSON from {final_url}: {error}\nResponse preview: {preview}"
                )
            })?;

            return Ok(search
                .recordings
                .into_iter()
                .map(recording_search_hit_from_api)
                .collect());
        }

        unreachable!("MusicBrainz retry loop always performs at least one attempt")
    }

    async fn valid_access_token(&self) -> Result<String, String> {
        let credential = self
            .credential
            .lock()
            .await
            .clone()
            .ok_or_else(|| "MusicBrainz OAuth credential state is unavailable.".to_string())?;

        if let Some(access_token) = credential.access_token.as_deref()
            && !access_token.trim().is_empty()
            && !credential_is_expired(&credential)?
        {
            return Ok(access_token.to_string());
        }

        self.refresh_access_token().await
    }

    async fn refresh_access_token(&self) -> Result<String, String> {
        let token_path = self
            .token_path
            .as_ref()
            .ok_or_else(|| "MusicBrainz OAuth token path is unavailable.".to_string())?;
        let current = self
            .credential
            .lock()
            .await
            .clone()
            .ok_or_else(|| "MusicBrainz OAuth credential state is unavailable.".to_string())?;
        let refresh_token = current
            .refresh_token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                "MusicBrainz OAuth access token is unavailable or expired and no refresh token is stored."
                    .to_string()
            })?;

        let response = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", current.client_secret.as_str()),
                ("token_type", "bearer"),
            ])
            .send()
            .await
            .map_err(|error| {
                format!("Unable to refresh MusicBrainz OAuth access token: {error}")
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|error| {
            format!("Unable to read MusicBrainz OAuth refresh response: {error}")
        })?;

        if !status.is_success() {
            return Err(format!(
                "MusicBrainz OAuth refresh returned HTTP {status}: {}",
                sanitized_oauth_error_preview(&body)
            ));
        }

        let token: TokenResponse = serde_json::from_str(&body)
            .map_err(|error| format!("Invalid MusicBrainz OAuth refresh response: {error}"))?;
        let updated = credential_from_token_response(current, token)?;
        let access_token = updated
            .access_token
            .clone()
            .ok_or_else(|| "MusicBrainz OAuth refresh returned no access token.".to_string())?;

        save_credential(token_path, &updated)?;
        *self.credential.lock().await = Some(updated);
        Ok(access_token)
    }

    async fn wait_for_rate_limit(&self) {
        let request_interval = self.request_interval();
        let mut last_request = self.last_request.lock().await;

        if let Some(previous) = *last_request {
            let elapsed = previous.elapsed();
            if elapsed < request_interval {
                sleep(request_interval - elapsed).await;
            }
        }

        *last_request = Some(Instant::now());
    }

    fn request_interval(&self) -> Duration {
        Duration::from_secs_f64(self.config.mb_min_delay)
    }

    fn recording_timeout(&self) -> Duration {
        Duration::from_secs_f64(self.config.mb_recording_timeout)
    }
}

fn recording_lookup_from_api(recording: ApiRecording) -> RecordingLookup {
    RecordingLookup {
        id: recording.id,
        title: recording.title,
        artist_credit: render_artist_credit(&recording.artist_credit),
        video: recording.video.unwrap_or(false),
        disambiguation: nonempty(recording.disambiguation),
        releases: recording
            .releases
            .into_iter()
            .map(recording_release_from_api)
            .collect(),
    }
}

fn recording_search_hit_from_api(recording: ApiRecording) -> RecordingSearchHit {
    RecordingSearchHit {
        id: recording.id,
        score: recording.score.min(100),
        title: recording.title,
        artist_credit: render_artist_credit(&recording.artist_credit),
        video: recording.video.unwrap_or(false),
        disambiguation: nonempty(recording.disambiguation),
        releases: recording
            .releases
            .into_iter()
            .map(recording_release_from_api)
            .collect(),
    }
}

fn recording_release_from_api(release: ApiRelease) -> RecordingRelease {
    let release_group_id = release.release_group.as_ref().map(|group| group.id.clone());
    let release_group_title = release
        .release_group
        .as_ref()
        .map(|group| group.title.clone());
    let release_group_primary_type = release
        .release_group
        .as_ref()
        .and_then(|group| group.primary_type.clone());
    let release_group_secondary_types = release
        .release_group
        .as_ref()
        .map(|group| group.secondary_types.clone())
        .unwrap_or_default();

    RecordingRelease {
        id: release.id,
        title: release.title,
        artist_credit: render_artist_credit(&release.artist_credit),
        status: release.status,
        date: release.date,
        release_group_id,
        release_group_title,
        release_group_primary_type,
        release_group_secondary_types,
    }
}

fn render_artist_credit(credits: &[ApiArtistCredit]) -> String {
    credits
        .iter()
        .map(|credit| format!("{}{}", credit.name, credit.joinphrase))
        .collect()
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

pub fn resolve_token_path(configured_path: &str) -> Result<PathBuf, String> {
    let configured_path = configured_path.trim();
    if configured_path.is_empty() {
        Err("SPLINED MusicBrainz OAuth token path is not configured.".to_string())
    } else {
        Ok(PathBuf::from(configured_path))
    }
}

pub fn load_credential(path: &Path) -> Result<OAuthCredential, String> {
    recover_backup_if_needed(path, "MusicBrainz OAuth credential")?;
    let body = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "Unable to read MusicBrainz OAuth credential file {}: {error}",
            path.display()
        )
    })?;

    serde_json::from_str(&body).map_err(|error| {
        format!(
            "Invalid MusicBrainz OAuth credential file {}: {error}",
            path.display()
        )
    })
}

pub fn save_credential(path: &Path, credential: &OAuthCredential) -> Result<(), String> {
    let json = serde_json::to_string_pretty(credential)
        .map_err(|error| format!("Unable to serialize MusicBrainz OAuth credential: {error}"))?;
    let body = format!("{json}\n");

    replace_text_file(path, &body, "MusicBrainz OAuth credential", |candidate| {
        serde_json::from_str::<OAuthCredential>(candidate)
            .map(|_| ())
            .map_err(|error| format!("Invalid staged MusicBrainz OAuth credential: {error}"))
    })
}

fn credential_from_token_response(
    mut credential: OAuthCredential,
    response: TokenResponse,
) -> Result<OAuthCredential, String> {
    let now = unix_now()?;
    credential.access_token = Some(response.access_token);
    if response.refresh_token.is_some() {
        credential.refresh_token = response.refresh_token;
    }
    credential.token_type = response.token_type;
    credential.scope = response.scope;
    credential.expires_at_unix = Some(now.saturating_add(response.expires_in));
    Ok(credential)
}

fn credential_is_expired(credential: &OAuthCredential) -> Result<bool, String> {
    let expires_at = match credential.expires_at_unix {
        Some(value) => value,
        None => return Ok(true),
    };
    let now = unix_now()?;
    Ok(now.saturating_add(EXPIRY_SAFETY_SECONDS) >= expires_at)
}

fn unix_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("System clock is before Unix epoch: {error}"))
}

fn random_urlsafe(byte_count: usize) -> String {
    let mut bytes = vec![0_u8; byte_count];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn sanitized_oauth_error_preview(body: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(body) else {
        return "non-JSON response body omitted".to_string();
    };

    redact_sensitive_json(&mut value);
    let rendered = serde_json::to_string(&value)
        .unwrap_or_else(|_| "OAuth error body unavailable".to_string());
    rendered.chars().take(OAUTH_ERROR_PREVIEW_CHARS).collect()
}

fn redact_sensitive_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let key = key.to_ascii_lowercase();
                if key.contains("token")
                    || key.contains("secret")
                    || key == "code"
                    || key.contains("authorization")
                {
                    *child = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    redact_sensitive_json(child);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                redact_sensitive_json(child);
            }
        }
        _ => {}
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn validate_config(config: &MusicBrainzConfig) -> Result<(), String> {
    if !config.mb_min_delay.is_finite() || config.mb_min_delay <= 0.0 {
        return Err("MusicBrainz mb_min_delay must be a finite number greater than 0.".to_string());
    }
    if !config.mb_recording_timeout.is_finite() || config.mb_recording_timeout <= 0.0 {
        return Err(
            "MusicBrainz mb_recording_timeout must be a finite number greater than 0.".to_string(),
        );
    }
    if !config.oauth {
        return Ok(());
    }
    if config.client_id.trim().is_empty() {
        return Err("MusicBrainz OAuth is enabled but client_id is empty.".to_string());
    }
    if config.callback_uri.trim().is_empty() {
        return Err("MusicBrainz OAuth is enabled but callback_uri is empty.".to_string());
    }
    if config.scope.trim().is_empty() {
        return Err("MusicBrainz OAuth is enabled but scope is empty.".to_string());
    }
    if config.token_file.trim().is_empty() {
        return Err("MusicBrainz OAuth is enabled but token_file is empty.".to_string());
    }
    Ok(())
}

fn validate_mbid(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit());

    if valid {
        Ok(())
    } else {
        Err(format!("Invalid MusicBrainz MBID: {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn secure_fixture() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("temporary secure directory should create");
        let path = dir.path().join("musicbrainz.json");
        let credential = OAuthCredential {
            client_secret: "fixture-secret".to_string(),
            access_token: Some("fixture-access".to_string()),
            refresh_token: Some("fixture-refresh".to_string()),
            token_type: Some("Bearer".to_string()),
            expires_at_unix: Some(u64::MAX),
            scope: Some("profile".to_string()),
        };
        save_credential(&path, &credential).expect("fixture credential should save");
        (dir, path)
    }

    #[test]
    fn oauth_is_optional_by_default() {
        let config = MusicBrainzConfig::default();
        assert_eq!(config.retry_max, 2);
        assert_eq!(config.mb_min_delay, 1.05);
        assert_eq!(config.mb_recording_timeout, 7.0);
        assert!(!config.oauth);
        assert!(config.client_id.is_empty());
        assert!(config.token_file.is_empty());
    }

    #[test]
    fn empty_token_path_has_no_os_fallback() {
        assert!(resolve_token_path("").is_err());
    }

    #[test]
    fn explicit_token_path_wins_exactly() {
        let configured = PathBuf::from("credentials").join("musicbrainz.json");
        let text = configured.to_string_lossy();
        assert_eq!(resolve_token_path(&text).unwrap(), configured);
    }

    #[test]
    fn anonymous_client_requires_no_credentials() {
        let client = MusicBrainzClient::new(&MusicBrainzConfig::default())
            .expect("anonymous client should create");
        assert_eq!(client.request_mode(), &RequestMode::Anonymous);
    }

    #[test]
    fn oauth_client_uses_configured_credential_file() {
        let (_dir, path) = secure_fixture();
        let config = MusicBrainzConfig {
            oauth: true,
            client_id: "fixture-client".to_string(),
            token_file: path.to_string_lossy().into_owned(),
            ..MusicBrainzConfig::default()
        };
        let client = MusicBrainzClient::new(&config).expect("OAuth client should create");
        assert_eq!(client.request_mode(), &RequestMode::OAuthBearer);
        assert_eq!(client.token_path(), Some(path.as_path()));
    }

    #[test]
    fn invalid_request_timing_is_rejected() {
        let zero_delay = MusicBrainzConfig {
            mb_min_delay: 0.0,
            ..MusicBrainzConfig::default()
        };
        assert!(MusicBrainzClient::new(&zero_delay).is_err());
    }

    #[test]
    fn valid_mbid_is_accepted() {
        assert!(validate_mbid("f60a6a1c-56cf-4dd9-a6ad-c47450d1b132").is_ok());
    }

    #[test]
    fn malformed_mbid_is_rejected() {
        assert!(validate_mbid("bad-mbid").is_err());
    }

    #[test]
    fn null_video_maps_to_false() {
        let search: ApiRecordingSearch = serde_json::from_str(
            r#"{"recordings":[{"id":"7ede458d-64dd-40e9-9c0b-e028eb1ba00f","title":"Fixture","video":null}]}"#,
        )
        .expect("fixture should parse");
        let hit = recording_search_hit_from_api(search.recordings.into_iter().next().unwrap());
        assert!(!hit.video);
    }

    #[test]
    fn oauth_error_preview_redacts_sensitive_fields() {
        let body = r#"{"error":"invalid_grant","access_token":"secret-access","refresh_token":"secret-refresh","code":"secret-code"}"#;
        let preview = sanitized_oauth_error_preview(body);
        assert!(preview.contains("invalid_grant"));
        assert!(!preview.contains("secret-access"));
        assert!(!preview.contains("secret-refresh"));
        assert!(!preview.contains("secret-code"));
        assert!(preview.contains("[REDACTED]"));
    }

    #[test]
    fn non_json_oauth_error_body_is_not_echoed() {
        assert_eq!(
            sanitized_oauth_error_preview("secret raw response body"),
            "non-JSON response body omitted"
        );
    }
}

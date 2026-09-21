use crate::safe_write::{recover_backup_if_needed, replace_text_file};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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
    #[serde(skip)]
    pub enabled: bool,
    #[serde(skip)]
    pub source_override: bool,
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
            enabled: true,
            source_override: false,
            retry_max: 4,
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
            && self.enabled == other.enabled
            && self.source_override == other.source_override
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicBrainzOptions {
    pub retry_max: u8,
    pub min_delay: f64,
    pub recording_timeout: u64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for MusicBrainzOptions {
    fn default() -> Self {
        Self {
            retry_max: 4,
            min_delay: 1.05,
            recording_timeout: 7,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OAuthCredential {
    pub oauth_enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    pub callback_uri: String,
    pub oauth_scope: String,
    pub options: MusicBrainzOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for OAuthCredential {
    fn default() -> Self {
        Self {
            oauth_enabled: false,
            client_id: String::new(),
            client_secret: String::new(),
            callback_uri: "urn:ietf:wg:oauth:2.0:oob".to_string(),
            oauth_scope: "profile".to_string(),
            options: MusicBrainzOptions::default(),
            access_token: None,
            refresh_token: None,
            token_type: None,
            expires_at_unix: None,
            scope: None,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestMode {
    Disabled,
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

        let token_path = if config.token_file.trim().is_empty() {
            None
        } else {
            Some(resolve_token_path(&config.token_file)?)
        };
        let credential = token_path
            .as_ref()
            .filter(|path| path.exists())
            .map(|path| load_credential(path))
            .transpose()?;
        let mut effective_config = config.clone();
        if config.source_override
            && let Some(credential) = credential.as_ref()
        {
            validate_options(&credential.options)?;
            effective_config.retry_max = credential.options.retry_max;
            effective_config.mb_min_delay = credential.options.min_delay;
            effective_config.mb_recording_timeout = credential.options.recording_timeout as f64;
        }
        let oauth_enabled = config.oauth
            || credential
                .as_ref()
                .map(|value| value.oauth_enabled || credential_access_token(value).is_some())
                .unwrap_or(false);

        if oauth_enabled {
            let token_path = token_path.ok_or_else(|| {
                "MusicBrainz OAuth is enabled but its fixed credential path is unavailable."
                    .to_string()
            })?;
            let credential = credential.ok_or_else(|| {
                format!(
                    "MusicBrainz OAuth is enabled but the credential file does not exist: {}",
                    token_path.display()
                )
            })?;
            effective_config.oauth = true;
            if !credential.client_id.trim().is_empty() {
                effective_config.client_id = credential.client_id.trim().to_string();
            }
            if !credential.callback_uri.trim().is_empty() {
                effective_config.callback_uri = credential.callback_uri.trim().to_string();
            }
            if !credential.oauth_scope.trim().is_empty() {
                effective_config.scope = credential.oauth_scope.trim().to_string();
            }
            validate_config(&effective_config)?;

            if credential.client_secret.trim().is_empty()
                && credential_access_token(&credential).is_none()
            {
                return Err(format!(
                    "MusicBrainz OAuth credential file contains neither a usable token nor client_secret: {}",
                    token_path.display()
                ));
            }

            let mode = if effective_config.enabled {
                RequestMode::OAuthBearer
            } else {
                RequestMode::Disabled
            };
            Ok(Self {
                client,
                config: effective_config,
                mode,
                token_path: Some(token_path),
                credential: Mutex::new(Some(credential)),
                last_request: Mutex::new(None),
            })
        } else {
            let mode = if effective_config.enabled {
                RequestMode::Anonymous
            } else {
                RequestMode::Disabled
            };
            Ok(Self {
                client,
                config: effective_config,
                mode,
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

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn retry_max(&self) -> u8 {
        self.config.retry_max
    }

    pub fn min_delay(&self) -> f64 {
        self.config.mb_min_delay
    }

    pub fn recording_timeout_seconds(&self) -> f64 {
        self.config.mb_recording_timeout
    }

    pub fn begin_authorization(&self) -> Result<AuthorizationSession, String> {
        if !self.config.oauth {
            return Err("MusicBrainz OAuth is disabled in SPLINED configuration.".to_string());
        }
        validate_oauth_application(&self.config)?;

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
        self.ensure_enabled()?;
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
        self.ensure_enabled()?;
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
        self.ensure_enabled()?;
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

        if let Some(access_token) = credential_access_token(&credential)
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
        validate_oauth_application(&self.config)?;
        if current.client_secret.trim().is_empty() {
            return Err(
                "MusicBrainz OAuth refresh requires client_secret in the credential file."
                    .to_string(),
            );
        }
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

    fn ensure_enabled(&self) -> Result<(), String> {
        if self.config.enabled {
            Ok(())
        } else {
            Err("MusicBrainz source is disabled by source policy.".to_string())
        }
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

    let credential: OAuthCredential = serde_json::from_str(&body).map_err(|error| {
        format!(
            "Invalid MusicBrainz OAuth credential file {}: {error}",
            path.display()
        )
    })?;
    validate_options(&credential.options)?;
    Ok(credential)
}

pub fn save_credential(path: &Path, credential: &OAuthCredential) -> Result<(), String> {
    validate_options(&credential.options)?;
    let update = serde_json::to_value(credential)
        .map_err(|error| format!("Unable to serialize MusicBrainz OAuth credential: {error}"))?;
    let mut update = update.as_object().cloned().ok_or_else(|| {
        "MusicBrainz OAuth credential did not serialize as an object.".to_string()
    })?;
    let mut merged = read_existing_object(path)?.unwrap_or_default();

    // Token refresh/authentication writes must never reset source options.
    if merged.contains_key("options") {
        update.remove("options");
    }
    for (key, value) in update {
        let empty_string = value.as_str().is_some_and(|text| text.is_empty());
        if (value.is_null() || empty_string) && merged.contains_key(&key) {
            continue;
        }
        merged.insert(key, value);
    }

    if !merged.contains_key("options") {
        merged.insert(
            "options".to_string(),
            serde_json::to_value(&credential.options).map_err(|error| {
                format!("Unable to serialize MusicBrainz runtime options: {error}")
            })?,
        );
    }
    let json = serde_json::to_string_pretty(&merged)
        .map_err(|error| format!("Unable to serialize MusicBrainz OAuth credential: {error}"))?;
    let body = format!("{json}\n");

    replace_text_file(path, &body, "MusicBrainz OAuth credential", |candidate| {
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(candidate)
            .and_then(|_| serde_json::from_str::<OAuthCredential>(candidate).map(|_| ()))
            .map_err(|error| format!("Invalid staged MusicBrainz OAuth credential: {error}"))
    })
}

pub fn merge_credential_options(path: &Path, options: &MusicBrainzOptions) -> Result<(), String> {
    validate_options(options)?;
    let mut document = read_existing_object(path)?.ok_or_else(|| {
        format!(
            "MusicBrainz credential file does not exist; refusing to create an options-only credential: {}",
            path.display()
        )
    })?;
    let mut option_document = document
        .get("options")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    option_document.insert(
        "retry_max".to_string(),
        serde_json::json!(options.retry_max),
    );
    option_document.insert(
        "min_delay".to_string(),
        serde_json::json!(options.min_delay),
    );
    option_document.insert(
        "recording_timeout".to_string(),
        serde_json::json!(options.recording_timeout),
    );
    document.insert(
        "options".to_string(),
        serde_json::Value::Object(option_document),
    );
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&document)
            .map_err(|error| format!("Unable to serialize MusicBrainz runtime options: {error}"))?
    );
    replace_text_file(path, &body, "MusicBrainz OAuth credential", |candidate| {
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(candidate)
            .map(|_| ())
            .map_err(|error| format!("Invalid staged MusicBrainz OAuth credential: {error}"))
    })
}

fn read_existing_object(
    path: &Path,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    if !path.exists() {
        return Ok(None);
    }
    recover_backup_if_needed(path, "MusicBrainz OAuth credential")?;
    let body = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "Unable to read MusicBrainz OAuth credential file {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&body)
        .map(Some)
        .map_err(|error| {
            format!(
                "Invalid MusicBrainz OAuth credential file {}: {error}",
                path.display()
            )
        })
}

fn validate_options(options: &MusicBrainzOptions) -> Result<(), String> {
    if options.retry_max > 20 {
        return Err("MusicBrainz retry_max must be between 0 and 20.".to_string());
    }
    if !options.min_delay.is_finite() || options.min_delay <= 0.0 {
        return Err("MusicBrainz min_delay must be a finite number greater than 0.".to_string());
    }
    if options.recording_timeout == 0 {
        return Err("MusicBrainz recording_timeout must be greater than 0.".to_string());
    }
    Ok(())
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
        None => return Ok(false),
    };
    let now = unix_now()?;
    Ok(now.saturating_add(EXPIRY_SAFETY_SECONDS) >= expires_at)
}

fn credential_access_token(credential: &OAuthCredential) -> Option<&str> {
    credential
        .access_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            credential
                .extra
                .get("token")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
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
    if config.token_file.trim().is_empty() && config.oauth {
        return Err("MusicBrainz OAuth is enabled but token_file is empty.".to_string());
    }
    Ok(())
}

fn validate_oauth_application(config: &MusicBrainzConfig) -> Result<(), String> {
    if config.client_id.trim().is_empty() {
        return Err(
            "MusicBrainz OAuth authorization requires client_id in the credential file."
                .to_string(),
        );
    }
    if config.callback_uri.trim().is_empty() {
        return Err(
            "MusicBrainz OAuth authorization requires callback_uri in the credential file."
                .to_string(),
        );
    }
    if config.scope.trim().is_empty() {
        return Err(
            "MusicBrainz OAuth authorization requires oauth_scope in the credential file."
                .to_string(),
        );
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
            oauth_enabled: true,
            client_id: "fixture-client".to_string(),
            client_secret: "fixture-secret".to_string(),
            ..OAuthCredential::default()
        };
        save_credential(&path, &credential).expect("fixture credential should save");
        (dir, path)
    }

    #[test]
    fn oauth_is_optional_by_default() {
        let config = MusicBrainzConfig::default();
        assert_eq!(config.retry_max, 4);
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
    fn oauth_client_uses_fixed_credential_json_without_toml_oauth_fields() {
        let (_dir, path) = secure_fixture();
        let config = MusicBrainzConfig {
            token_file: path.to_string_lossy().into_owned(),
            ..MusicBrainzConfig::default()
        };
        let client = MusicBrainzClient::new(&config).expect("OAuth client should create");
        assert_eq!(client.request_mode(), &RequestMode::OAuthBearer);
        assert_eq!(client.token_path(), Some(path.as_path()));
    }

    #[test]
    fn adding_default_options_preserves_existing_token_and_unknown_fields() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("musicbrainz.json");
        std::fs::write(
            &path,
            r#"{"token":"fixture-token","future_auth":{"nonce":"keep-me"}}"#,
        )
        .unwrap();

        merge_credential_options(&path, &MusicBrainzOptions::default()).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["token"], "fixture-token");
        assert_eq!(saved["future_auth"]["nonce"], "keep-me");
        assert_eq!(saved["options"]["retry_max"], 4);
        assert_eq!(saved["options"]["min_delay"], 1.05);
        assert_eq!(saved["options"]["recording_timeout"], 7);
    }

    #[test]
    fn legacy_token_only_document_remains_an_authenticated_client() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("musicbrainz.json");
        std::fs::write(&path, r#"{"token":"fixture-token"}"#).unwrap();
        let config = MusicBrainzConfig {
            token_file: path.to_string_lossy().into_owned(),
            ..MusicBrainzConfig::default()
        };
        let client = MusicBrainzClient::new(&config).unwrap();
        assert_eq!(client.request_mode(), &RequestMode::OAuthBearer);
    }

    #[test]
    fn custom_options_and_unknown_nested_option_survive_authentication_save() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("musicbrainz.json");
        std::fs::write(
            &path,
            r#"{
                "oauth_enabled": true,
                "client_id": "fixture-client",
                "client_secret": "fixture-secret",
                "access_token": "fixture-access",
                "options": {
                    "retry_max": 9,
                    "min_delay": 1.75,
                    "recording_timeout": 13,
                    "future_option": "keep-me"
                },
                "future_root": "keep-root"
            }"#,
        )
        .unwrap();

        let mut credential = load_credential(&path).unwrap();
        assert_eq!(credential.options.retry_max, 9);
        assert_eq!(credential.options.min_delay, 1.75);
        assert_eq!(credential.options.recording_timeout, 13);
        credential.refresh_token = Some("fixture-refresh".to_string());
        save_credential(&path, &credential).unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["access_token"], "fixture-access");
        assert_eq!(saved["options"]["retry_max"], 9);
        assert_eq!(saved["options"]["min_delay"], 1.75);
        assert_eq!(saved["options"]["recording_timeout"], 13);
        assert_eq!(saved["options"]["future_option"], "keep-me");
        assert_eq!(saved["future_root"], "keep-root");
    }

    #[test]
    fn changing_only_retry_max_preserves_other_options_and_token() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("musicbrainz.json");
        std::fs::write(
            &path,
            r#"{"token":"fixture-token","options":{"retry_max":6,"min_delay":1.25,"recording_timeout":11}}"#,
        )
        .unwrap();
        let mut options = load_credential(&path).unwrap().options;
        options.retry_max = 8;
        merge_credential_options(&path, &options).unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["token"], "fixture-token");
        assert_eq!(saved["options"]["retry_max"], 8);
        assert_eq!(saved["options"]["min_delay"], 1.25);
        assert_eq!(saved["options"]["recording_timeout"], 11);
    }

    #[test]
    fn source_override_activates_json_runtime_options() {
        let (_dir, path) = secure_fixture();
        let mut credential = load_credential(&path).unwrap();
        credential.options.retry_max = 7;
        credential.options.min_delay = 1.45;
        credential.options.recording_timeout = 12;
        merge_credential_options(&path, &credential.options).unwrap();
        let config = MusicBrainzConfig {
            source_override: true,
            token_file: path.to_string_lossy().into_owned(),
            ..MusicBrainzConfig::default()
        };
        let client = MusicBrainzClient::new(&config).unwrap();
        assert_eq!(client.retry_max(), 7);
        assert_eq!(client.min_delay(), 1.45);
        assert_eq!(client.recording_timeout_seconds(), 12.0);
    }

    #[test]
    fn disabled_source_policy_prevents_musicbrainz_queries_without_touching_credentials() {
        let (_dir, path) = secure_fixture();
        let config = MusicBrainzConfig {
            enabled: false,
            token_file: path.to_string_lossy().into_owned(),
            ..MusicBrainzConfig::default()
        };
        let client = MusicBrainzClient::new(&config).unwrap();
        assert_eq!(client.request_mode(), &RequestMode::Disabled);
        assert!(!client.is_enabled());
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

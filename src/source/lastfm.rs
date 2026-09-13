use crate::credentials::{
    LastFmCredential, load_lastfm_credential, resolve_credential_path, save_lastfm_credential,
};
use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_URL: &str = "https://www.last.fm/api/auth/";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct AlbumResponse {
    album: Option<Album>,
    error: Option<i64>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Album {
    name: String,
    artist: String,
    #[serde(default)]
    image: Vec<AlbumImage>,
}

#[derive(Debug, Deserialize)]
struct AlbumImage {
    #[serde(rename = "#text")]
    url: String,
    size: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    error: Option<i64>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    session: Option<Session>,
    error: Option<i64>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Session {
    name: String,
    key: String,
    subscriber: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastFmAuthorization {
    pub token: String,
    pub authorization_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastFmSessionIdentity {
    pub username: String,
    pub subscriber: bool,
}

pub struct LastFm {
    client: Client,
    credential_file: String,
}

impl LastFm {
    pub fn new(configured_path: &str) -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| format!("Unable to create Last.fm HTTP client: {error}"))?;

        Ok(Self {
            client,
            credential_file: configured_path.to_string(),
        })
    }

    fn credential_path(&self) -> Result<PathBuf, String> {
        resolve_credential_path(&self.credential_file, "Last.fm")
    }

    fn credential(&self) -> Result<LastFmCredential, String> {
        load_lastfm_credential(&self.credential_path()?)
    }

    fn api_key(&self) -> Result<String, String> {
        Ok(self.credential()?.api_key)
    }

    fn require_auth_material(&self) -> Result<LastFmCredential, String> {
        let credential = self.credential()?;

        if credential.shared_secret.trim().is_empty() {
            return Err(
                "Last.fm credential file contains no shared_secret. Run --lastfm-credentials first."
                    .to_string(),
            );
        }

        Ok(credential)
    }

    pub async fn begin_authorization(&self) -> Result<LastFmAuthorization, String> {
        let credential = self.require_auth_material()?;
        let signature = api_signature(
            &[
                ("api_key", credential.api_key.as_str()),
                ("method", "auth.getToken"),
            ],
            &credential.shared_secret,
        );

        let response = self
            .client
            .get(API_URL)
            .query(&[
                ("method", "auth.getToken"),
                ("api_key", credential.api_key.as_str()),
                ("api_sig", signature.as_str()),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|error| format!("Unable to request Last.fm authorization token: {error}"))?;

        let status = response.status();
        let body = response.text().await.map_err(|error| {
            format!("Unable to read Last.fm authorization token response: {error}")
        })?;

        let payload = serde_json::from_str::<TokenResponse>(&body)
            .map_err(|error| format!("Invalid Last.fm authorization token response: {error}"))?;

        if let Some(code) = payload.error {
            let message = payload
                .message
                .unwrap_or_else(|| "unknown error".to_string());
            return Err(format!(
                "Last.fm authorization token request failed with API error {code}: {message}"
            ));
        }

        if status != StatusCode::OK {
            return Err(format!(
                "Last.fm authorization token request returned HTTP {status}"
            ));
        }

        let token = payload
            .token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                "Last.fm authorization token response contained no token.".to_string()
            })?;

        let mut authorization_url = Url::parse(AUTH_URL)
            .map_err(|error| format!("Unable to construct Last.fm authorization URL: {error}"))?;
        authorization_url
            .query_pairs_mut()
            .append_pair("api_key", &credential.api_key)
            .append_pair("token", &token);

        Ok(LastFmAuthorization {
            token,
            authorization_url: authorization_url.to_string(),
        })
    }

    pub async fn complete_authorization(
        &self,
        authorization: &LastFmAuthorization,
    ) -> Result<LastFmSessionIdentity, String> {
        let mut credential = self.require_auth_material()?;
        let signature = api_signature(
            &[
                ("api_key", credential.api_key.as_str()),
                ("method", "auth.getSession"),
                ("token", authorization.token.as_str()),
            ],
            &credential.shared_secret,
        );

        let response = self
            .client
            .get(API_URL)
            .query(&[
                ("method", "auth.getSession"),
                ("api_key", credential.api_key.as_str()),
                ("token", authorization.token.as_str()),
                ("api_sig", signature.as_str()),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|error| format!("Unable to create Last.fm authenticated session: {error}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("Unable to read Last.fm session response: {error}"))?;

        let payload = serde_json::from_str::<SessionResponse>(&body)
            .map_err(|error| format!("Invalid Last.fm session response: {error}"))?;

        if let Some(code) = payload.error {
            let message = payload
                .message
                .unwrap_or_else(|| "unknown error".to_string());
            return Err(format!(
                "Last.fm session creation failed with API error {code}: {message}"
            ));
        }

        if status != StatusCode::OK {
            return Err(format!("Last.fm session creation returned HTTP {status}"));
        }

        let session = payload
            .session
            .ok_or_else(|| "Last.fm session response contained no session.".to_string())?;

        if session.key.trim().is_empty() || session.name.trim().is_empty() {
            return Err("Last.fm session response was incomplete.".to_string());
        }

        credential.username = session.name.trim().to_string();
        credential.session_key = session.key.trim().to_string();
        credential.subscriber = session.subscriber != 0;
        save_lastfm_credential(&self.credential_path()?, &credential)?;

        Ok(LastFmSessionIdentity {
            username: credential.username,
            subscriber: credential.subscriber,
        })
    }

    async fn request_album(
        &self,
        api_key: &str,
        params: &[(&str, &str)],
        description: &str,
    ) -> Result<Option<Album>, String> {
        let mut query = vec![
            ("method", "album.getinfo"),
            ("api_key", api_key),
            ("format", "json"),
            ("autocorrect", "1"),
        ];
        query.extend_from_slice(params);

        let response = self
            .client
            .get(API_URL)
            .query(&query)
            .send()
            .await
            .map_err(|error| format!("Unable to query Last.fm for {description}: {error}"))?;

        let status = response.status();
        let final_url = response.url().clone();
        let body = response.text().await.map_err(|error| {
            format!("Unable to read Last.fm response for {description} from {final_url}: {error}")
        })?;

        let payload = serde_json::from_str::<AlbumResponse>(&body).map_err(|error| {
            let preview: String = body.chars().take(500).collect();
            format!(
                "Invalid Last.fm JSON for {description} from {final_url}: {error}\nResponse preview: {preview}"
            )
        })?;

        if let Some(code) = payload.error {
            let message = payload
                .message
                .unwrap_or_else(|| "unknown error".to_string());

            if code == 6 || code == 7 {
                return Ok(None);
            }

            if code == 10 || status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                return Err("Last.fm API credentials were rejected.".to_string());
            }

            return Err(format!(
                "Last.fm album lookup for {description} failed with API error {code}: {message}"
            ));
        }

        if status != StatusCode::OK {
            return Err(format!(
                "Last.fm album lookup for {description} returned HTTP {status} from {final_url}"
            ));
        }

        Ok(payload.album)
    }

    fn references_from_album(album: Album) -> Vec<ArtworkReference> {
        let mut references = Vec::new();
        let mut seen = HashSet::new();

        for image in album.image {
            let url = image.url.trim();
            if url.is_empty() || !seen.insert(url.to_string()) {
                continue;
            }

            references.push(ArtworkReference {
                source: "lastfm".to_string(),
                id: format!("{}:{}:{}", album.artist, album.name, image.size),
                url: url.to_string(),
                front: true,
                approved: true,
                types: vec!["Front".to_string()],
            });
        }

        references
    }

    async fn discover_release(
        &self,
        query: &ArtworkQuery,
        context: &ProviderContext,
    ) -> Result<Vec<ArtworkReference>, String> {
        let api_key = self.api_key()?;
        let release_mbid = query.release_mbid.trim();

        if !release_mbid.is_empty()
            && let Some(album) = self
                .request_album(
                    &api_key,
                    &[("mbid", release_mbid)],
                    &format!("MusicBrainz release {release_mbid}"),
                )
                .await?
        {
            let references = Self::references_from_album(album);
            if !references.is_empty() {
                return Ok(references);
            }
        }

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

        for title in titles {
            let description = format!("{artist} - {title}");
            if let Some(album) = self
                .request_album(
                    &api_key,
                    &[("artist", artist), ("album", title.as_str())],
                    &description,
                )
                .await?
                && same_text(&album.artist, artist)
                && title_matches(&album.name, &title)
            {
                let references = Self::references_from_album(album);
                if !references.is_empty() {
                    return Ok(references);
                }
            }
        }

        Ok(Vec::new())
    }
}

impl ArtworkProvider for LastFm {
    fn name(&self) -> &'static str {
        "lastfm"
    }

    fn discover<'a>(
        &'a self,
        query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move { self.discover_release(query, context).await })
    }
}

fn api_signature(params: &[(&str, &str)], shared_secret: &str) -> String {
    let mut params = params.to_vec();
    params.sort_unstable_by(|left, right| left.0.cmp(right.0));

    let mut material = String::new();
    for (name, value) in params {
        material.push_str(name);
        material.push_str(value);
    }
    material.push_str(shared_secret);

    md5_hex(material.as_bytes())
}

fn md5_hex(input: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut data = input.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_le_bytes());

    let mut a0 = 0x67452301u32;
    let mut b0 = 0xefcdab89u32;
    let mut c0 = 0x98badcfeu32;
    let mut d0 = 0x10325476u32;

    for chunk in data.as_chunks::<64>().0 {
        let mut m = [0u32; 16];
        for (index, word) in m.iter_mut().enumerate() {
            let offset = index * 4;
            *word = u32::from_le_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }

        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;

        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | ((!b) & d), i)
            } else if i < 32 {
                ((d & b) | ((!d) & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };

            let next_d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
            a = d;
            d = next_d;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = Vec::with_capacity(16);
    digest.extend_from_slice(&a0.to_le_bytes());
    digest.extend_from_slice(&b0.to_le_bytes());
    digest.extend_from_slice(&c0.to_le_bytes());
    digest.extend_from_slice(&d0.to_le_bytes());

    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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
    fn blank_credential_path_has_no_os_fallback() {
        let client = LastFm::new("").expect("client construction should not touch credentials");
        assert!(client.credential_path().is_err());
    }

    #[test]
    fn album_response_parses_images() {
        let json = r##"
        {
            "album": {
                "name": "Unorthodox Jukebox",
                "artist": "Bruno Mars",
                "image": [
                    {"#text": "https://lastfm.freetls.fastly.net/i/u/770x0/example.jpg", "size": "extralarge"}
                ]
            }
        }
        "##;

        let response: AlbumResponse = serde_json::from_str(json).expect("fixture should parse");
        let album = response.album.expect("album should exist");

        assert_eq!(album.name, "Unorthodox Jukebox");
        assert_eq!(album.artist, "Bruno Mars");
        assert_eq!(album.image.len(), 1);
        assert_eq!(album.image[0].size, "extralarge");
    }

    #[test]
    fn api_error_response_parses_without_album() {
        let json = r#"{"error":6,"message":"Album not found"}"#;
        let response: AlbumResponse = serde_json::from_str(json).expect("fixture should parse");

        assert_eq!(response.error, Some(6));
        assert!(response.album.is_none());
    }

    #[test]
    fn empty_image_urls_are_not_emitted() {
        let album = Album {
            name: "Example".to_string(),
            artist: "Artist".to_string(),
            image: vec![AlbumImage {
                url: String::new(),
                size: "large".to_string(),
            }],
        };

        assert!(LastFm::references_from_album(album).is_empty());
    }

    #[test]
    fn matching_ignores_case_space_and_punctuation() {
        assert!(same_text("Bruno Mars", "BRUNO MARS"));
        assert!(same_text("AC/DC", "ACDC"));
    }

    #[test]
    fn title_match_allows_edition_suffixes() {
        assert!(title_matches(
            "Unorthodox Jukebox",
            "Unorthodox Jukebox (Hi-Res Version)"
        ));
    }

    #[test]
    fn md5_matches_standard_test_vector() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn api_signature_sorts_parameter_names() {
        let signature = api_signature(
            &[
                ("token", "bbbb"),
                ("api_key", "aaaa"),
                ("method", "auth.getSession"),
            ],
            "secret",
        );

        assert_eq!(
            signature,
            md5_hex(b"api_keyaaaamethodauth.getSessiontokenbbbbsecret")
        );
    }

    #[test]
    fn token_response_parses() {
        let response: TokenResponse =
            serde_json::from_str(r#"{"token":"abc123"}"#).expect("token fixture should parse");
        assert_eq!(response.token.as_deref(), Some("abc123"));
    }

    #[test]
    fn session_response_parses_subscriber_status() {
        let response: SessionResponse = serde_json::from_str(
            r#"{"session":{"name":"FixtureUser","key":"session-key","subscriber":1}}"#,
        )
        .expect("session fixture should parse");
        let session = response.session.expect("session should exist");

        assert_eq!(session.name, "FixtureUser");
        assert_eq!(session.key, "session-key");
        assert_eq!(session.subscriber, 1);
    }
}

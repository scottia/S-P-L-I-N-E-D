//! Curated-compilation authority and MusicBrainz match projection.
//!
//! Eligibility is explicit: the representative track has no Album/Release
//! MBID. The local Album title is never used as MusicBrainz identity. Local
//! Recording/Artist IDs are the only fallback identity. Filename, Artist text,
//! and Track Title text are never used to invent MusicBrainz matches.

use crate::config::Config;
use crate::media_database::{RecordingReleaseCacheRow, cache_recording_releases};
use crate::musicbrainz::{MusicBrainzClient, RecordingRelease, WorkRecording};
use crate::scan_musicbrainz::LocalTrackEvidence;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MAX_WORK_RECORDING_LOOKUPS: usize = 8;
const MUSICBRAINZ_EMBEDDED_RELEASE_LIMIT: usize = 25;
const MAX_FUZZY_DETAIL_LOOKUPS: usize = 8;
const MIN_FUZZY_SCORE: u16 = 55;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseClass {
    Album,
    Single,
    Ep,
    Soundtrack,
    Compilation,
    Broadcast,
    Other,
}

impl ReleaseClass {
    pub fn rank(self) -> u8 {
        match self {
            Self::Album => 0,
            Self::Single => 1,
            Self::Ep => 2,
            Self::Soundtrack => 3,
            Self::Compilation => 4,
            Self::Broadcast => 5,
            Self::Other => 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicBrainzMatch {
    pub recording_mbid: String,
    pub recording_title: String,
    pub recording_artist: String,
    pub artist_mbids: Vec<String>,
    pub release_mbid: String,
    pub release_group_mbid: Option<String>,
    pub release_group_title: Option<String>,
    pub release_class: ReleaseClass,
    pub release_title: String,
    pub release_artist: String,
    pub release_date: Option<String>,
    pub country: Option<String>,
    pub score: u16,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicBrainzResolution {
    pub matches: Vec<MusicBrainzMatch>,
    pub warnings: Vec<String>,
    pub identity_status: String,
    pub identity_message: String,
    pub recording_status: String,
    pub artist_status: String,
    pub friendly_artist: String,
    pub friendly_title: String,
    pub used_fuzzy_search: bool,
    pub focused_artist_mbid: Option<String>,
}

impl MusicBrainzResolution {
    fn failed(
        message: impl Into<String>,
        recording_status: &str,
        artist_status: &str,
        friendly_artist: impl Into<String>,
        friendly_title: impl Into<String>,
    ) -> Self {
        let message = message.into();
        Self {
            matches: Vec::new(),
            warnings: vec![message.clone()],
            identity_status: "failed".to_string(),
            identity_message: message,
            recording_status: recording_status.to_string(),
            artist_status: artist_status.to_string(),
            friendly_artist: friendly_artist.into(),
            friendly_title: friendly_title.into(),
            used_fuzzy_search: false,
            focused_artist_mbid: None,
        }
    }
}

#[derive(Default)]
pub struct CompilationMatchCache {
    recording: HashMap<String, Vec<MusicBrainzMatch>>,
    browser: HashMap<String, Vec<MusicBrainzMatch>>,
}

impl CompilationMatchCache {
    pub async fn automatic_matches(
        &mut self,
        client: &MusicBrainzClient,
        config: &Config,
        recording_mbid: Option<&str>,
        local_artist_mbids: &[String],
        artist: &str,
        title: &str,
    ) -> MusicBrainzResolution {
        self.resolve_identity_matches(
            client,
            Some(config),
            recording_mbid,
            local_artist_mbids,
            artist,
            title,
        )
        .await
    }

    async fn resolve_identity_matches(
        &mut self,
        client: &MusicBrainzClient,
        config: Option<&Config>,
        recording_mbid: Option<&str>,
        local_artist_mbids: &[String],
        artist: &str,
        title: &str,
    ) -> MusicBrainzResolution {
        if local_artist_mbids.is_empty() {
            return MusicBrainzResolution::failed(
                "A valid MusicBrainz Artist ID is required before SPLINED can search this fallback track.",
                if recording_mbid.is_some() {
                    "pending"
                } else {
                    "invalid"
                },
                "invalid",
                artist,
                title,
            );
        }

        let mut validated_artist = None;
        let mut last_lookup_error = None;
        for artist_mbid in local_artist_mbids {
            match client.lookup_artist(artist_mbid).await {
                Ok(lookup) if artist_names_compatible(artist, &lookup.name) => {
                    validated_artist = Some((artist_mbid.clone(), lookup.name));
                    break;
                }
                Ok(_) => {}
                Err(error) => last_lookup_error = Some(error),
            }
        }
        let Some((focused_artist_mbid, friendly_artist)) = validated_artist else {
            return MusicBrainzResolution::failed(
                last_lookup_error.unwrap_or_else(|| {
                    format!(
                        "None of the tagged Artist IDs resolves to the selected artist {artist}."
                    )
                }),
                if recording_mbid.is_some() {
                    "pending"
                } else {
                    "invalid"
                },
                "invalid",
                artist,
                title,
            );
        };
        let focused_artist_ids = vec![focused_artist_mbid.clone()];

        let exact_error = if let Some(recording_mbid) =
            recording_mbid.filter(|value| !value.trim().is_empty())
        {
            let exact = if let Some(config) = config {
                self.recording_matches(client, config, recording_mbid, &focused_artist_ids)
                    .await
            } else {
                self.browser_matches_exact(client, recording_mbid, &focused_artist_ids)
                    .await
            };
            match exact {
                Ok(mut matches) if !matches.is_empty() => {
                    finalize_interactive_matches(&mut matches);
                    let friendly_title = matches
                        .first()
                        .map(|item| item.recording_title.clone())
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| title.to_string());
                    return MusicBrainzResolution {
                        matches,
                        warnings: Vec::new(),
                        identity_status: "valid".to_string(),
                        identity_message: format!(
                            "Validated Recording and Artist identity: {friendly_artist} — {friendly_title}"
                        ),
                        recording_status: "valid".to_string(),
                        artist_status: "valid".to_string(),
                        friendly_artist,
                        friendly_title,
                        used_fuzzy_search: false,
                        focused_artist_mbid: Some(focused_artist_mbid),
                    };
                }
                Ok(_) => {
                    return MusicBrainzResolution {
                        matches: Vec::new(),
                        warnings: vec!["The Recording and Artist IDs are valid, but no eligible official release was found through the Recording or its related Work.".to_string()],
                        identity_status: "valid".to_string(),
                        identity_message: "Recording and Artist IDs validated; no eligible official release is available. Use MB INFO to review MusicBrainz.".to_string(),
                        recording_status: "valid".to_string(),
                        artist_status: "valid".to_string(),
                        friendly_artist,
                        friendly_title: title.to_string(),
                        used_fuzzy_search: false,
                        focused_artist_mbid: Some(focused_artist_mbid),
                    };
                }
                Err(error) => error,
            }
        } else {
            "The track has no valid MusicBrainz Recording ID.".to_string()
        };

        match fuzzy_artist_title_matches(client, &focused_artist_mbid, &friendly_artist, title)
            .await
        {
            Ok(mut matches) if !matches.is_empty() => {
                finalize_interactive_matches(&mut matches);
                let warning = exact_error.clone();
                MusicBrainzResolution {
                    matches,
                    warnings: vec![warning.clone()],
                    identity_status: "invalid".to_string(),
                    identity_message: format!(
                        "{warning} Artist-restricted title recovery is shown below; choose a release or correct Advanced IDs."
                    ),
                    recording_status: "invalid".to_string(),
                    artist_status: "valid".to_string(),
                    friendly_artist,
                    friendly_title: title.to_string(),
                    used_fuzzy_search: true,
                    focused_artist_mbid: Some(focused_artist_mbid),
                }
            }
            Ok(_) => {
                let mut result = MusicBrainzResolution::failed(
                    exact_error,
                    "invalid",
                    "valid",
                    friendly_artist,
                    title,
                );
                result.focused_artist_mbid = Some(focused_artist_mbid);
                result
            }
            Err(error) => {
                let mut result = MusicBrainzResolution::failed(
                    format!(
                        "{} Artist-restricted recovery also failed: {error}",
                        exact_error
                    ),
                    "invalid",
                    "valid",
                    friendly_artist,
                    title,
                );
                result.focused_artist_mbid = Some(focused_artist_mbid);
                result
            }
        }
    }

    pub async fn recording_matches(
        &mut self,
        client: &MusicBrainzClient,
        config: &Config,
        recording_mbid: &str,
        local_artist_mbids: &[String],
    ) -> Result<Vec<MusicBrainzMatch>, String> {
        if local_artist_mbids.is_empty() {
            return Err(
                "Track has no valid embedded MusicBrainz Artist ID for its Recording ID."
                    .to_string(),
            );
        }
        let cache_key = format!(
            "{}|{}",
            recording_mbid.trim().to_ascii_lowercase(),
            artist_key(local_artist_mbids)
        );
        if let Some(cached) = self.recording.get(&cache_key) {
            return Ok(cached.clone());
        }
        let artist_mbids_key = artist_key(local_artist_mbids);
        let lookup = client.lookup_recording_releases(recording_mbid).await?;
        let canonical_recording_mbid = lookup.id.clone();
        let work_mbids = lookup.work_mbids.clone();
        let direct_artist_matches = ids_subset(local_artist_mbids, &lookup.artist_mbids);
        let mut matches = if direct_artist_matches {
            project_releases(
                &lookup.id,
                &lookup.title,
                &lookup.artist_credit,
                &lookup.artist_mbids,
                lookup.releases,
                100,
                true,
            )
        } else {
            Vec::new()
        };
        let used_work_fallback = release_family_count(&matches) <= 1 && !work_mbids.is_empty();
        if used_work_fallback {
            matches.extend(
                work_fallback_matches(
                    client,
                    &canonical_recording_mbid,
                    &work_mbids,
                    local_artist_mbids,
                    true,
                )
                .await?,
            );
            finalize_interactive_matches(&mut matches);
        }
        if matches.is_empty() && !direct_artist_matches {
            return Err(
                "MusicBrainz Recording Artist IDs do not match the locally tagged Artist IDs, and its Work has no Recording for that Artist ID."
                    .to_string(),
            );
        }
        // MusicBrainz can redirect a merged Recording MBID to its canonical
        // identity. The SQL parent row is keyed by the requested/tagged MBID,
        // while projected children correctly carry the canonical MBID; mixing
        // those identities would violate the candidate foreign key. Return the
        // canonical live result and leave that redirected lookup uncached.
        if !used_work_fallback
            && recording_cache_identity_matches(recording_mbid, &canonical_recording_mbid)
        {
            let cache_rows = matches
                .iter()
                .enumerate()
                .map(|(rank, item)| RecordingReleaseCacheRow {
                    recording_mbid: item.recording_mbid.clone(),
                    release_mbid: item.release_mbid.clone(),
                    release_group_mbid: item.release_group_mbid.clone(),
                    release_class: format!("{:?}", item.release_class).to_ascii_lowercase(),
                    class_rank: item.release_class.rank(),
                    candidate_rank: rank,
                    release_title: item.release_title.clone(),
                    release_artist: item.release_artist.clone(),
                    artist_mbids_key: artist_key(&item.artist_mbids),
                    release_date: item.release_date.clone(),
                })
                .collect::<Vec<_>>();
            cache_recording_releases(
                config,
                &canonical_recording_mbid,
                &artist_mbids_key,
                &cache_rows,
            )?;
        }
        self.recording.insert(cache_key, matches.clone());
        Ok(matches)
    }

    pub async fn browser_matches(
        &mut self,
        client: &MusicBrainzClient,
        recording_mbid: Option<&str>,
        local_artist_mbids: &[String],
        artist: &str,
        title: &str,
    ) -> Result<Vec<MusicBrainzMatch>, String> {
        Ok(self
            .resolve_identity_matches(
                client,
                None,
                recording_mbid,
                local_artist_mbids,
                artist,
                title,
            )
            .await
            .matches)
    }

    async fn browser_matches_exact(
        &mut self,
        client: &MusicBrainzClient,
        recording_mbid: &str,
        local_artist_mbids: &[String],
    ) -> Result<Vec<MusicBrainzMatch>, String> {
        let key = format!(
            "{}|{}",
            recording_mbid.trim().to_ascii_lowercase(),
            artist_key(local_artist_mbids)
        );
        if let Some(cached) = self.browser.get(&key) {
            return Ok(cached.clone());
        }
        if local_artist_mbids.is_empty() {
            return Err(
                "Track has no valid embedded MusicBrainz Artist ID for its Recording ID."
                    .to_string(),
            );
        }
        let lookup = client.lookup_recording_releases(recording_mbid).await?;
        let canonical_recording_mbid = lookup.id.clone();
        let work_mbids = lookup.work_mbids.clone();
        let direct_artist_matches = ids_subset(local_artist_mbids, &lookup.artist_mbids);
        let mut matches = if direct_artist_matches {
            project_releases(
                &lookup.id,
                &lookup.title,
                &lookup.artist_credit,
                &lookup.artist_mbids,
                lookup.releases,
                100,
                true,
            )
        } else {
            Vec::new()
        };
        if release_family_count(&matches) <= 1 && !work_mbids.is_empty() {
            matches.extend(
                work_fallback_matches(
                    client,
                    &canonical_recording_mbid,
                    &work_mbids,
                    local_artist_mbids,
                    false,
                )
                .await?,
            );
        }
        if matches.is_empty() && !direct_artist_matches {
            return Err(
                "MusicBrainz Recording Artist IDs do not match the locally tagged Artist IDs, and its Work has no Recording for that Artist ID."
                    .to_string(),
            );
        }
        finalize_interactive_matches(&mut matches);
        self.browser.insert(key, matches.clone());
        Ok(matches)
    }

    pub async fn edited_authority_matches(
        &mut self,
        client: &MusicBrainzClient,
        recording_value: &str,
        artist_value: &str,
        _release_value: &str,
        artist: &str,
        title: &str,
    ) -> Result<MusicBrainzResolution, String> {
        let recording_ids = extract_mbids(recording_value);
        if recording_ids.len() != 1 {
            return Err("Recording ID must be one canonical MusicBrainz UUID.".to_string());
        }
        let artist_ids = extract_mbids(artist_value);
        if artist_ids.is_empty()
            || !artist_value
                .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '/' | '\\' | '|'))
                .filter(|part| !part.is_empty())
                .all(|part| one_mbid(part).is_some())
        {
            return Err("Artist IDs must be canonical MusicBrainz UUIDs.".to_string());
        }
        Ok(self
            .resolve_identity_matches(
                client,
                None,
                recording_ids.first().map(String::as_str),
                &artist_ids,
                artist,
                title,
            )
            .await)
    }
}

async fn work_fallback_matches(
    client: &MusicBrainzClient,
    tagged_recording_mbid: &str,
    work_mbids: &[String],
    local_artist_mbids: &[String],
    strict_compilation_policy: bool,
) -> Result<Vec<MusicBrainzMatch>, String> {
    let mut combined = Vec::new();
    let mut last_error = None;

    for work_mbid in work_mbids {
        let recordings = match client.lookup_work_recordings(work_mbid).await {
            Ok(recordings) => {
                matching_work_recordings(recordings, tagged_recording_mbid, local_artist_mbids)
            }
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };

        for recording in recordings.into_iter().take(MAX_WORK_RECORDING_LOOKUPS) {
            let lookup = match client.lookup_recording_releases(&recording.id).await {
                Ok(lookup) => lookup,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            if !ids_subset(local_artist_mbids, &lookup.artist_mbids) {
                continue;
            }
            let release_count = lookup.releases.len();
            let projected = project_releases(
                &lookup.id,
                &lookup.title,
                &lookup.artist_credit,
                &lookup.artist_mbids,
                lookup.releases,
                95,
                strict_compilation_policy,
            );
            combined.extend(projected);
            // MusicBrainz embeds at most 25 releases in one Recording lookup.
            // One full page plus all earlier same-Artist recordings is enough
            // to avoid an unbounded Work crawl while retaining every distinct
            // release already discovered.
            if release_count >= MUSICBRAINZ_EMBEDDED_RELEASE_LIMIT && !combined.is_empty() {
                finalize_interactive_matches(&mut combined);
                return Ok(combined);
            }
        }
        if !combined.is_empty() {
            finalize_interactive_matches(&mut combined);
            return Ok(combined);
        }
    }

    if let Some(error) = last_error {
        Err(error)
    } else {
        Ok(Vec::new())
    }
}

async fn fuzzy_artist_title_matches(
    client: &MusicBrainzClient,
    artist_mbid: &str,
    artist_name: &str,
    title: &str,
) -> Result<Vec<MusicBrainzMatch>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(Vec::new());
    }
    let query = format!(
        "recording:\"{}\" AND arid:{}",
        escape_lucene_phrase(title),
        artist_mbid
    );
    let hits = client.search_recordings(&query, 50).await?;
    let mut matches = Vec::new();
    let mut detailed_lookups = 0usize;
    for hit in hits
        .into_iter()
        .filter(|hit| hit.score >= MIN_FUZZY_SCORE)
        .filter(|hit| {
            ids_subset(
                std::slice::from_ref(&artist_mbid.to_string()),
                &hit.artist_mbids,
            )
        })
    {
        let score = hit.score;
        if hit.releases.is_empty() {
            if detailed_lookups >= MAX_FUZZY_DETAIL_LOOKUPS {
                continue;
            }
            detailed_lookups += 1;
            let lookup = client.lookup_recording_releases(&hit.id).await?;
            if !ids_subset(
                std::slice::from_ref(&artist_mbid.to_string()),
                &lookup.artist_mbids,
            ) {
                continue;
            }
            matches.extend(project_releases(
                &lookup.id,
                &lookup.title,
                if lookup.artist_credit.trim().is_empty() {
                    artist_name
                } else {
                    &lookup.artist_credit
                },
                &lookup.artist_mbids,
                lookup.releases,
                score,
                false,
            ));
        } else {
            matches.extend(project_releases(
                &hit.id,
                &hit.title,
                if hit.artist_credit.trim().is_empty() {
                    artist_name
                } else {
                    &hit.artist_credit
                },
                &hit.artist_mbids,
                hit.releases,
                score,
                false,
            ));
        }
    }
    finalize_interactive_matches(&mut matches);
    Ok(matches)
}

fn escape_lucene_phrase(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\\' | '"' => vec!['\\', character],
            _ => vec![character],
        })
        .collect()
}

fn artist_names_compatible(local: &str, musicbrainz: &str) -> bool {
    fn normalized(value: &str) -> String {
        let expanded = value.replace('&', "and");
        let compact = expanded
            .chars()
            .flat_map(char::to_lowercase)
            .filter(|character| character.is_alphanumeric())
            .collect::<String>();
        compact.strip_prefix("the").unwrap_or(&compact).to_string()
    }
    let local = normalized(local);
    let musicbrainz = normalized(musicbrainz);
    !local.is_empty() && local == musicbrainz
}

fn matching_work_recordings(
    recordings: Vec<WorkRecording>,
    tagged_recording_mbid: &str,
    local_artist_mbids: &[String],
) -> Vec<WorkRecording> {
    let mut recordings = recordings
        .into_iter()
        .filter(|recording| {
            !recording.id.eq_ignore_ascii_case(tagged_recording_mbid)
                && ids_subset(local_artist_mbids, &recording.artist_mbids)
        })
        .collect::<Vec<_>>();
    // MusicBrainz relationship order is meaningful enough to retain within a
    // tier. Prefer unqualified recordings over live/remix/partial relations,
    // but never relax the exact Artist-MBID constraint.
    recordings.sort_by_key(|recording| usize::from(!recording.attributes.is_empty()));
    recordings
}

pub fn manual_album_eligible(tracks: &[LocalTrackEvidence]) -> bool {
    tracks.first().is_some_and(|track| {
        track
            .musicbrainz_album_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    })
}

pub fn track_authority(track: &LocalTrackEvidence) -> (Option<String>, Vec<String>) {
    (
        track.musicbrainz_track_id.as_deref().and_then(one_mbid),
        extract_mbids(track.musicbrainz_artist_id.as_deref().unwrap_or("")),
    )
}

pub fn classify_release(release: &RecordingRelease) -> Option<ReleaseClass> {
    if !release
        .status
        .as_deref()
        .unwrap_or("")
        .eq_ignore_ascii_case("official")
    {
        return None;
    }
    let secondary = release
        .release_group_secondary_types
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    if secondary.iter().any(|value| {
        matches!(
            value.as_str(),
            "dj-mix" | "live" | "mixtape/street" | "remix"
        )
    }) {
        return None;
    }
    if secondary.contains("soundtrack") {
        return Some(ReleaseClass::Soundtrack);
    }
    if secondary.contains("compilation") {
        return Some(ReleaseClass::Compilation);
    }
    if release
        .release_group_primary_type
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("album"))
        && secondary.is_empty()
    {
        return Some(ReleaseClass::Album);
    }
    None
}

pub fn classify_release_for_browser(release: &RecordingRelease) -> Option<ReleaseClass> {
    if !release
        .status
        .as_deref()
        .unwrap_or("")
        .eq_ignore_ascii_case("official")
    {
        return None;
    }
    let secondary = release
        .release_group_secondary_types
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    if secondary.contains("soundtrack") {
        return Some(ReleaseClass::Soundtrack);
    }
    if secondary.contains("compilation") {
        return Some(ReleaseClass::Compilation);
    }
    Some(
        match release
            .release_group_primary_type
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "album" => ReleaseClass::Album,
            "single" => ReleaseClass::Single,
            "ep" => ReleaseClass::Ep,
            "broadcast" => ReleaseClass::Broadcast,
            _ => ReleaseClass::Other,
        },
    )
}

pub fn decade(date: Option<&str>) -> String {
    date.and_then(|value| value.get(0..4))
        .and_then(|year| year.parse::<u16>().ok())
        .map(|year| format!("{}0s", year / 10))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn project_releases(
    recording_mbid: &str,
    recording_title: &str,
    recording_artist: &str,
    recording_artist_mbids: &[String],
    releases: Vec<RecordingRelease>,
    score: u16,
    strict_compilation_policy: bool,
) -> Vec<MusicBrainzMatch> {
    let mut projected = releases
        .into_iter()
        .filter_map(|release| {
            let release_class = if strict_compilation_policy {
                classify_release(&release)?
            } else {
                classify_release_for_browser(&release)?
            };
            let artist_mbids = if recording_artist_mbids.is_empty() {
                release.artist_mbids.clone()
            } else {
                recording_artist_mbids.to_vec()
            };
            let release_group_mbid = release
                .release_group_id
                .map(|value| value.to_ascii_lowercase());
            let release_group_title = release.release_group_title;
            Some(MusicBrainzMatch {
                recording_mbid: recording_mbid.to_ascii_lowercase(),
                recording_title: recording_title.to_string(),
                recording_artist: recording_artist.to_string(),
                artist_mbids,
                release_mbid: release.id.to_ascii_lowercase(),
                release_group_mbid,
                release_group_title,
                release_class,
                release_title: release.title,
                release_artist: release.artist_credit,
                release_date: release.date,
                country: release.country,
                score,
                url: format!("https://musicbrainz.org/release/{}", release.id),
            })
        })
        .collect::<Vec<_>>();
    sort_and_deduplicate(&mut projected);
    projected
}

fn sort_and_deduplicate(matches: &mut Vec<MusicBrainzMatch>) {
    matches.sort_by(|left, right| {
        decade(right.release_date.as_deref())
            .cmp(&decade(left.release_date.as_deref()))
            .then(left.release_class.rank().cmp(&right.release_class.rank()))
            .then(
                left.release_date
                    .as_deref()
                    .unwrap_or("9999-99-99")
                    .cmp(right.release_date.as_deref().unwrap_or("9999-99-99")),
            )
            .then(right.score.cmp(&left.score))
            .then(
                left.release_title
                    .to_ascii_lowercase()
                    .cmp(&right.release_title.to_ascii_lowercase()),
            )
            .then(left.release_mbid.cmp(&right.release_mbid))
    });
    let mut seen = HashSet::new();
    // A release edition is one user choice even when several same-Artist
    // recordings of the Work lead to it. Keep the highest-ranked path and do
    // not repeat the same edition under every related Recording.
    matches.retain(|item| seen.insert(item.release_mbid.clone()));
}

fn finalize_interactive_matches(matches: &mut Vec<MusicBrainzMatch>) {
    sort_and_deduplicate(matches);
    let fuzzy = matches.iter().any(|item| item.score < 95);
    matches.sort_by(|left, right| {
        (if fuzzy {
            right.score.cmp(&left.score)
        } else {
            std::cmp::Ordering::Equal
        })
        .then_with(|| left.release_class.rank().cmp(&right.release_class.rank()))
        .then_with(|| release_family_title(left).cmp(&release_family_title(right)))
        .then_with(|| release_family_id(left).cmp(&release_family_id(right)))
        .then_with(|| release_country_rank(left).cmp(&release_country_rank(right)))
        .then_with(|| {
            left.release_date
                .as_deref()
                .unwrap_or("9999-99-99")
                .cmp(right.release_date.as_deref().unwrap_or("9999-99-99"))
        })
        .then_with(|| left.release_mbid.cmp(&right.release_mbid))
    });
}

fn release_family_count(matches: &[MusicBrainzMatch]) -> usize {
    matches
        .iter()
        .map(release_family_id)
        .collect::<HashSet<_>>()
        .len()
}

fn release_family_title(item: &MusicBrainzMatch) -> String {
    item.release_group_title
        .as_deref()
        .unwrap_or(&item.release_title)
        .trim()
        .to_ascii_lowercase()
}

fn release_family_id(item: &MusicBrainzMatch) -> String {
    item.release_group_mbid
        .as_deref()
        .unwrap_or(&item.release_mbid)
        .to_ascii_lowercase()
}

fn release_country_rank(item: &MusicBrainzMatch) -> u8 {
    match item.country.as_deref().unwrap_or("").trim() {
        country if country.eq_ignore_ascii_case("US") => 0,
        country if country.eq_ignore_ascii_case("XW") => 1,
        "" => 3,
        _ => 2,
    }
}

pub fn extract_mbids(value: &str) -> Vec<String> {
    let mut ids = value
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | '/' | '\\' | '|')
        })
        .filter_map(one_mbid)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn one_mbid(value: &str) -> Option<String> {
    let value = value.trim();
    (value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23) && byte.is_ascii_hexdigit()
        }))
    .then(|| value.to_ascii_lowercase())
}

fn artist_key(values: &[String]) -> String {
    let mut values = values
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values.join(",")
}

fn ids_subset(expected: &[String], actual: &[String]) -> bool {
    let actual = actual
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    expected
        .iter()
        .all(|value| actual.contains(&value.to_ascii_lowercase()))
}

fn recording_cache_identity_matches(requested: &str, resolved: &str) -> bool {
    requested.trim().eq_ignore_ascii_case(resolved.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn featured_artist_id_can_validate_a_multi_credit_recording() {
        let jason = "6de0f914-3e60-4418-be3b-42e0feb6eb4d".to_string();
        let chainz = "dff0d392-4cd5-4052-9fbb-f485df3891e5".to_string();
        let credits = vec![jason.clone(), chainz.clone()];
        assert!(ids_subset(&[chainz], &credits));
        assert!(artist_names_compatible("2 Chainz", "2 Chainz"));
        assert!(!artist_names_compatible(
            "Jason Derulo\x002 Chainz",
            "2 Chainz"
        ));
        assert_eq!(extract_mbids(&credits.join(", ")).len(), 2);
    }

    #[test]
    fn release_policy_is_album_then_soundtrack_then_compilation_only() {
        let release = |primary: Option<&str>, secondary: &[&str]| RecordingRelease {
            id: "5d05694f-2b0f-427e-9df8-78dbc0983681".into(),
            title: "Fixture".into(),
            artist_credit: "Artist".into(),
            status: Some("Official".into()),
            date: Some("2017".into()),
            country: Some("US".into()),
            artist_mbids: Vec::new(),
            release_group_id: None,
            release_group_title: None,
            release_group_primary_type: primary.map(str::to_string),
            release_group_secondary_types: secondary
                .iter()
                .map(|value| value.to_string())
                .collect(),
        };
        assert_eq!(
            classify_release(&release(Some("Album"), &[])),
            Some(ReleaseClass::Album)
        );
        assert_eq!(
            classify_release(&release(Some("Album"), &["Soundtrack"])),
            Some(ReleaseClass::Soundtrack)
        );
        assert_eq!(
            classify_release(&release(Some("Album"), &["Compilation"])),
            Some(ReleaseClass::Compilation)
        );
        assert_eq!(classify_release(&release(Some("Album"), &["Live"])), None);
        assert_eq!(
            classify_release_for_browser(&release(Some("Single"), &[])),
            Some(ReleaseClass::Single)
        );
        assert_eq!(
            classify_release_for_browser(&release(Some("EP"), &[])),
            Some(ReleaseClass::Ep)
        );
        assert_eq!(
            classify_release_for_browser(&release(Some("Album"), &["Soundtrack"])),
            Some(ReleaseClass::Soundtrack)
        );
    }

    #[test]
    fn album_eligibility_uses_only_representative_track() {
        let mut track = LocalTrackEvidence {
            path: "fixture.mp3".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: Some("Curated".into()),
            album_artist: Some("Various Artists".into()),
            musicbrainz_album_id: None,
            musicbrainz_track_id: None,
            musicbrainz_artist_id: None,
            compilation: Some("1".into()),
        };
        assert!(manual_album_eligible(std::slice::from_ref(&track)));
        track.musicbrainz_album_id = Some("823b3eeb-e62b-42c6-9029-5fc47279b7fc".to_string());
        assert!(!manual_album_eligible(&[track]));
    }

    #[test]
    fn shared_cross_runtime_album_policy_fixture_matches_rust() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/cross-runtime-album-policy.json"
        ))
        .unwrap();
        for item in fixture["albums"].as_array().unwrap() {
            let track = LocalTrackEvidence {
                path: "fixture.mp3".into(),
                title: "Track".into(),
                artist: "Artist".into(),
                album: Some(item["name"].as_str().unwrap().into()),
                album_artist: Some("Artist".into()),
                musicbrainz_album_id: item["album_mbid"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                musicbrainz_track_id: None,
                musicbrainz_artist_id: None,
                compilation: Some(item["compilation"].as_str().unwrap().into()),
            };
            let eligible = manual_album_eligible(&[track]);
            assert_eq!(eligible, item["manual_compilation"].as_bool().unwrap());
            assert_eq!(
                if eligible { "embedded-track" } else { "folder" },
                item["output_target"].as_str().unwrap()
            );
        }
        for item in fixture["automatic_release_classes"].as_array().unwrap() {
            let release = RecordingRelease {
                id: "5d05694f-2b0f-427e-9df8-78dbc0983681".into(),
                title: "Fixture".into(),
                artist_credit: "Artist".into(),
                status: Some("Official".into()),
                date: None,
                country: None,
                artist_mbids: Vec::new(),
                release_group_id: None,
                release_group_title: None,
                release_group_primary_type: Some(item["primary"].as_str().unwrap().into()),
                release_group_secondary_types: item["secondary"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap().to_string())
                    .collect(),
            };
            let actual =
                classify_release(&release).map(|value| format!("{value:?}").to_ascii_lowercase());
            assert_eq!(actual.as_deref(), item["expected"].as_str());
        }
    }

    #[test]
    fn recording_id_projection_preserves_exact_recording_and_artist_authority() {
        let recording_artist_id = "837555ba-012e-45f1-9a9c-9628da13ee54".to_string();
        let release_artist_id = "89ad4ac3-39f7-470e-963a-56509c546377".to_string();
        let projected = project_releases(
            "29b6fe53-6c45-4328-ad87-b4b3334a1436",
            "Spanish Harlem",
            "Ben E. King",
            std::slice::from_ref(&recording_artist_id),
            vec![RecordingRelease {
                id: "823b3eeb-e62b-42c6-9029-5fc47279b7fc".into(),
                title: "Spanish Harlem".into(),
                artist_credit: "Various Artists".into(),
                status: Some("Official".into()),
                date: Some("2012-10-03".into()),
                country: Some("XW".into()),
                artist_mbids: vec![release_artist_id],
                release_group_id: None,
                release_group_title: None,
                release_group_primary_type: Some("Album".into()),
                release_group_secondary_types: Vec::new(),
            }],
            100,
            true,
        );
        assert_eq!(projected.len(), 1);
        assert_eq!(
            projected[0].recording_mbid,
            "29b6fe53-6c45-4328-ad87-b4b3334a1436"
        );
        assert_eq!(projected[0].artist_mbids, vec![recording_artist_id]);
        assert_eq!(projected[0].release_artist, "Various Artists");
    }

    #[test]
    fn redirected_recording_identity_is_not_sql_cache_compatible() {
        assert!(recording_cache_identity_matches(
            "29b6fe53-6c45-4328-ad87-b4b3334a1436",
            "29B6FE53-6C45-4328-AD87-B4B3334A1436"
        ));
        assert!(!recording_cache_identity_matches(
            "b5386435-a43c-4fe5-ae3c-337176914ca6",
            "29b6fe53-6c45-4328-ad87-b4b3334a1436"
        ));
    }

    #[test]
    fn work_fallback_rejects_other_artists_and_prefers_unqualified_recordings() {
        let local_artist = "f37c537b-3557-4031-bfd6-ab63ced32854".to_string();
        let other_artist = "11111111-1111-4111-8111-111111111111".to_string();
        let recording = |id: &str, artist: String, attributes: &[&str]| WorkRecording {
            id: id.to_string(),
            title: "I'm Not in Love".to_string(),
            artist_credit: "10cc".to_string(),
            artist_mbids: vec![artist],
            attributes: attributes.iter().map(|value| value.to_string()).collect(),
        };
        let recordings = matching_work_recordings(
            vec![
                recording("tagged", local_artist.clone(), &[]),
                recording("other", other_artist, &[]),
                recording("live", local_artist.clone(), &["live"]),
                recording("studio", local_artist.clone(), &[]),
            ],
            "tagged",
            std::slice::from_ref(&local_artist),
        );

        assert_eq!(
            recordings
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["studio", "live"]
        );
    }

    #[test]
    fn track_authority_accepts_only_valid_embedded_musicbrainz_ids() {
        let mut track = LocalTrackEvidence {
            path: "fixture.mp3".into(),
            title: "Best of My Love".into(),
            artist: "The Emotions".into(),
            album: Some("Billboard".into()),
            album_artist: Some("Various Artists".into()),
            musicbrainz_album_id: None,
            musicbrainz_track_id: Some("not-a-recording-id".into()),
            musicbrainz_artist_id: Some("also-not-an-id".into()),
            compilation: Some("1".into()),
        };
        assert_eq!(track_authority(&track), (None, Vec::new()));

        track.musicbrainz_track_id = Some("29b6fe53-6c45-4328-ad87-b4b3334a1436".into());
        track.musicbrainz_artist_id = Some("837555ba-012e-45f1-9a9c-9628da13ee54".into());
        assert_eq!(
            track_authority(&track),
            (
                Some("29b6fe53-6c45-4328-ad87-b4b3334a1436".into()),
                vec!["837555ba-012e-45f1-9a9c-9628da13ee54".into()]
            )
        );
    }

    #[test]
    fn interactive_results_keep_release_editions_grouped_with_us_primary() {
        let edition = |release: &str, group: &str, title: &str, country: &str| MusicBrainzMatch {
            recording_mbid: "29b6fe53-6c45-4328-ad87-b4b3334a1436".to_string(),
            recording_title: "Song".to_string(),
            recording_artist: "Artist".to_string(),
            artist_mbids: Vec::new(),
            release_mbid: release.to_string(),
            release_group_mbid: Some(group.to_string()),
            release_group_title: Some(title.to_string()),
            release_class: ReleaseClass::Compilation,
            release_title: title.to_string(),
            release_artist: "Various Artists".to_string(),
            release_date: Some("2020".to_string()),
            country: Some(country.to_string()),
            score: 100,
            url: String::new(),
        };
        let mut matches = vec![
            edition("group-b-gb", "group-b", "Second Family", "GB"),
            edition("group-a-gb", "group-a", "First Family", "GB"),
            edition("group-a-us", "group-a", "First Family", "US"),
        ];

        finalize_interactive_matches(&mut matches);

        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].release_mbid, "group-a-us");
        assert_eq!(matches[1].release_mbid, "group-a-gb");
        assert_eq!(matches[2].release_mbid, "group-b-gb");
    }

    #[test]
    fn work_expansion_deduplicates_the_same_release_across_recordings() {
        let candidate = |recording: &str, score: u16| MusicBrainzMatch {
            recording_mbid: recording.to_string(),
            recording_title: "The Things We Do for Love".to_string(),
            recording_artist: "10cc".to_string(),
            artist_mbids: vec!["f37c537b-3557-4031-bfd6-ab63ced32854".to_string()],
            release_mbid: "02d5630b-52e3-42e6-a420-6d0e41c3d572".to_string(),
            release_group_mbid: Some("family".to_string()),
            release_group_title: Some("100x Liefde 2009".to_string()),
            release_class: ReleaseClass::Compilation,
            release_title: "100x Liefde 2009".to_string(),
            release_artist: "Various Artists".to_string(),
            release_date: Some("2009-01-30".to_string()),
            country: Some("NL".to_string()),
            score,
            url: String::new(),
        };
        let mut matches = vec![candidate("exact", 100), candidate("work-recording", 95)];

        finalize_interactive_matches(&mut matches);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].recording_mbid, "exact");
        assert_eq!(matches[0].score, 100);
    }

    #[test]
    fn fuzzy_recovery_groups_higher_percentage_before_release_family() {
        let candidate = |release: &str, family: &str, score: u16| MusicBrainzMatch {
            recording_mbid: format!("recording-{score}"),
            recording_title: "The Things We Do for Love".to_string(),
            recording_artist: "10cc".to_string(),
            artist_mbids: vec!["f37c537b-3557-4031-bfd6-ab63ced32854".to_string()],
            release_mbid: release.to_string(),
            release_group_mbid: Some(family.to_string()),
            release_group_title: Some(family.to_string()),
            release_class: ReleaseClass::Compilation,
            release_title: family.to_string(),
            release_artist: "Various Artists".to_string(),
            release_date: Some("2009".to_string()),
            country: Some("US".to_string()),
            score,
            url: String::new(),
        };
        let mut matches = vec![
            candidate("low", "A Family", 61),
            candidate("high", "Z Family", 94),
            candidate("middle", "M Family", 78),
        ];

        finalize_interactive_matches(&mut matches);

        assert_eq!(
            matches.iter().map(|item| item.score).collect::<Vec<_>>(),
            vec![94, 78, 61]
        );
    }

    #[test]
    fn artist_identity_name_validation_rejects_the_wrong_performer() {
        assert!(artist_names_compatible("The Beatles", "Beatles"));
        assert!(artist_names_compatible(
            "Earth, Wind & Fire",
            "Earth Wind and Fire"
        ));
        assert!(!artist_names_compatible("10cc", "Elton John"));
    }
}

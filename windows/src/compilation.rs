//! Curated-compilation authority and MusicBrainz match projection.
//!
//! Eligibility is explicit: the representative track has no Album/Release
//! MBID and is tagged `compilation=1`. The curated Album title is never used
//! as MusicBrainz identity. Local Recording/Artist IDs are preferred; a
//! bounded Artist/Title Recording search is available when those IDs are
//! absent or need operator correction.

use crate::config::Config;
use crate::media_database::{
    RecordingReleaseCacheRow, cache_recording_releases, cached_recording_releases,
};
use crate::musicbrainz::{MusicBrainzClient, RecordingRelease};
use crate::scan_musicbrainz::LocalTrackEvidence;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    pub release_class: ReleaseClass,
    pub release_title: String,
    pub release_artist: String,
    pub release_date: Option<String>,
    pub country: Option<String>,
    pub score: u16,
    pub url: String,
}

#[derive(Default)]
pub struct CompilationMatchCache {
    recording: HashMap<String, Vec<MusicBrainzMatch>>,
    search: HashMap<String, Vec<MusicBrainzMatch>>,
    browser: HashMap<String, Vec<MusicBrainzMatch>>,
}

impl CompilationMatchCache {
    pub async fn recording_matches(
        &mut self,
        client: &MusicBrainzClient,
        config: &Config,
        recording_mbid: &str,
        local_artist_mbids: &[String],
    ) -> Result<Vec<MusicBrainzMatch>, String> {
        let cache_key = format!(
            "{}|{}",
            recording_mbid.trim().to_ascii_lowercase(),
            artist_key(local_artist_mbids)
        );
        if let Some(cached) = self.recording.get(&cache_key) {
            return Ok(cached.clone());
        }
        let artist_mbids_key = artist_key(local_artist_mbids);
        let sql_cached = cached_recording_releases(config, recording_mbid, &artist_mbids_key)?;
        if !sql_cached.is_empty() {
            let matches = sql_cached
                .into_iter()
                .filter_map(match_from_cache_row)
                .collect::<Vec<_>>();
            self.recording.insert(cache_key, matches.clone());
            return Ok(matches);
        }
        let lookup = client.lookup_recording_releases(recording_mbid).await?;
        if !local_artist_mbids.is_empty()
            && !ids_intersect(local_artist_mbids, &lookup.artist_mbids)
        {
            return Err(
                "MusicBrainz Recording Artist IDs do not match the locally tagged Artist IDs."
                    .to_string(),
            );
        }
        let matches = project_releases(
            &lookup.id,
            &lookup.title,
            &lookup.artist_credit,
            &lookup.artist_mbids,
            lookup.releases,
            100,
            true,
        );
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
                artist_mbids_key: artist_mbids_key.clone(),
                release_date: item.release_date.clone(),
            })
            .collect::<Vec<_>>();
        cache_recording_releases(config, recording_mbid, &artist_mbids_key, &cache_rows)?;
        self.recording.insert(cache_key, matches.clone());
        Ok(matches)
    }

    pub async fn artist_title_matches(
        &mut self,
        client: &MusicBrainzClient,
        artist: &str,
        title: &str,
    ) -> Result<Vec<MusicBrainzMatch>, String> {
        if artist.trim().is_empty() || title.trim().is_empty() {
            return Err("MusicBrainz search requires local Artist and Track Title.".to_string());
        }
        let query = format!(
            "recording:\"{}\" AND artist:\"{}\" AND status:official",
            lucene_phrase(title),
            lucene_phrase(artist)
        );
        if let Some(cached) = self.search.get(&query) {
            return Ok(cached.clone());
        }
        let mut matches = Vec::new();
        for hit in client.search_recordings(&query, 100).await? {
            matches.extend(project_releases(
                &hit.id,
                &hit.title,
                &hit.artist_credit,
                &hit.artist_mbids,
                hit.releases,
                hit.score,
                true,
            ));
        }
        sort_and_deduplicate(&mut matches);
        self.search.insert(query, matches.clone());
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
        let key = format!(
            "{}|{}|{}|{}",
            recording_mbid.unwrap_or("").trim().to_ascii_lowercase(),
            artist_key(local_artist_mbids),
            artist.trim().to_ascii_lowercase(),
            title.trim().to_ascii_lowercase()
        );
        if let Some(cached) = self.browser.get(&key) {
            return Ok(cached.clone());
        }
        let mut matches = Vec::new();
        if let Some(recording_mbid) = recording_mbid.filter(|value| !value.trim().is_empty()) {
            let lookup = client.lookup_recording_releases(recording_mbid).await?;
            if !local_artist_mbids.is_empty()
                && !ids_intersect(local_artist_mbids, &lookup.artist_mbids)
            {
                return Err(
                    "MusicBrainz Recording Artist IDs do not match the locally tagged Artist IDs."
                        .to_string(),
                );
            }
            matches.extend(project_releases(
                &lookup.id,
                &lookup.title,
                &lookup.artist_credit,
                &lookup.artist_mbids,
                lookup.releases,
                100,
                false,
            ));
        } else {
            if artist.trim().is_empty() || title.trim().is_empty() {
                return Err("MusicBrainz search requires local Artist and Track Title.".to_string());
            }
            let query = format!(
                "recording:\"{}\" AND artist:\"{}\" AND status:official",
                lucene_phrase(title),
                lucene_phrase(artist)
            );
            for hit in client.search_recordings(&query, 100).await? {
                matches.extend(project_releases(
                    &hit.id,
                    &hit.title,
                    &hit.artist_credit,
                    &hit.artist_mbids,
                    hit.releases,
                    hit.score,
                    false,
                ));
            }
        }
        sort_and_deduplicate(&mut matches);
        self.browser.insert(key, matches.clone());
        Ok(matches)
    }

    pub async fn edited_authority_matches(
        &mut self,
        client: &MusicBrainzClient,
        recording_value: &str,
        artist_value: &str,
        release_value: &str,
        artist: &str,
        title: &str,
    ) -> Result<Vec<MusicBrainzMatch>, String> {
        let recording_ids = extract_mbids(recording_value);
        if !recording_value.trim().is_empty() && recording_ids.len() != 1 {
            return Err("Recording ID must be one canonical MusicBrainz UUID.".to_string());
        }
        let artist_ids = extract_mbids(artist_value);
        if !artist_value.trim().is_empty() && artist_ids.is_empty() {
            return Err(
                "Artist ID must contain at least one canonical MusicBrainz UUID.".to_string(),
            );
        }
        let release_ids = extract_mbids(release_value);
        if !release_value.trim().is_empty() && release_ids.len() != 1 {
            return Err("Release ID must be one canonical MusicBrainz UUID.".to_string());
        }
        let recording_id = recording_ids.first().map(String::as_str);
        if let Some(release_id) = release_ids.first() {
            if let Some(recording_id) = recording_id {
                let lookup = client.lookup_recording_releases(recording_id).await?;
                if !artist_ids.is_empty() && !ids_intersect(&artist_ids, &lookup.artist_mbids) {
                    return Err(
                        "Edited Recording and Artist IDs do not identify the same MusicBrainz authority."
                            .to_string(),
                    );
                }
                let release = lookup
                    .releases
                    .into_iter()
                    .find(|item| item.id.eq_ignore_ascii_case(release_id))
                    .ok_or_else(|| {
                        "Edited Release does not contain the selected MusicBrainz Recording."
                            .to_string()
                    })?;
                return Ok(project_releases(
                    &lookup.id,
                    &lookup.title,
                    &lookup.artist_credit,
                    &lookup.artist_mbids,
                    vec![release],
                    100,
                    false,
                ));
            }
            let release = client.lookup_release(release_id).await?;
            return Ok(vec![MusicBrainzMatch {
                recording_mbid: String::new(),
                recording_title: title.to_string(),
                recording_artist: artist.to_string(),
                artist_mbids: artist_ids,
                release_mbid: release.id.to_ascii_lowercase(),
                release_group_mbid: release.release_group_id,
                release_class: ReleaseClass::Other,
                release_title: release.title,
                release_artist: release.artist_credit,
                release_date: None,
                country: None,
                score: 100,
                url: format!("https://musicbrainz.org/release/{}", release.id),
            }]);
        }

        let mut matches = self
            .browser_matches(client, recording_id, &artist_ids, artist, title)
            .await?;
        if !artist_ids.is_empty() {
            matches.retain(|item| ids_intersect(&artist_ids, &item.artist_mbids));
        }
        Ok(matches)
    }
}

fn match_from_cache_row(row: RecordingReleaseCacheRow) -> Option<MusicBrainzMatch> {
    let release_class = match row.release_class.as_str() {
        "album" => ReleaseClass::Album,
        "single" => ReleaseClass::Single,
        "ep" => ReleaseClass::Ep,
        "soundtrack" => ReleaseClass::Soundtrack,
        "compilation" => ReleaseClass::Compilation,
        "broadcast" => ReleaseClass::Broadcast,
        "other" => ReleaseClass::Other,
        _ => return None,
    };
    Some(MusicBrainzMatch {
        recording_mbid: row.recording_mbid,
        recording_title: String::new(),
        recording_artist: row.release_artist.clone(),
        artist_mbids: row
            .artist_mbids_key
            .split(',')
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        release_mbid: row.release_mbid.clone(),
        release_group_mbid: row.release_group_mbid,
        release_class,
        release_title: row.release_title,
        release_artist: row.release_artist,
        release_date: row.release_date,
        country: None,
        score: 100,
        url: format!("https://musicbrainz.org/release/{}", row.release_mbid),
    })
}

pub fn manual_album_eligible(tracks: &[LocalTrackEvidence]) -> bool {
    tracks.first().is_some_and(|track| {
        track
            .musicbrainz_album_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
            && truthy(track.compilation.as_deref())
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
            let artist_mbids = if release.artist_mbids.is_empty() {
                recording_artist_mbids.to_vec()
            } else {
                release.artist_mbids.clone()
            };
            Some(MusicBrainzMatch {
                recording_mbid: recording_mbid.to_ascii_lowercase(),
                recording_title: recording_title.to_string(),
                recording_artist: recording_artist.to_string(),
                artist_mbids,
                release_mbid: release.id.to_ascii_lowercase(),
                release_group_mbid: release
                    .release_group_id
                    .map(|value| value.to_ascii_lowercase()),
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
    matches.retain(|item| seen.insert((item.recording_mbid.clone(), item.release_mbid.clone())));
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

fn truthy(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        )
    })
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

fn ids_intersect(left: &[String], right: &[String]) -> bool {
    let right = right
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    left.iter()
        .any(|value| right.contains(&value.to_ascii_lowercase()))
}

fn lucene_phrase(value: &str) -> String {
    value.trim().replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let track = LocalTrackEvidence {
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
        assert!(manual_album_eligible(&[track]));
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
}

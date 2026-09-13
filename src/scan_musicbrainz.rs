use crate::musicbrainz::MusicBrainzClient;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTrackEvidence {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub musicbrainz_album_id: Option<String>,
    pub musicbrainz_track_id: Option<String>,
    pub compilation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationContext {
    Standard,
    Compilation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseAuthority {
    ExactAlbumId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlbumReleaseDecision {
    Resolved {
        release_mbid: String,
        authority: ReleaseAuthority,
    },
    Fallback {
        compilation: CompilationContext,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedAlbumIdEvidence {
    pub release_mbid: String,
    pub track_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaggedAlbumIdAudit {
    pub valid: Vec<TaggedAlbumIdEvidence>,
    pub missing: Vec<PathBuf>,
    pub invalid: Vec<(PathBuf, String)>,
}

pub fn compilation_context(tracks: &[LocalTrackEvidence]) -> CompilationContext {
    if tracks.iter().any(|track| {
        track
            .compilation
            .as_deref()
            .is_some_and(|value| value.trim() == "1")
    }) {
        CompilationContext::Compilation
    } else {
        CompilationContext::Standard
    }
}

pub fn tagged_album_id_audit(tracks: &[LocalTrackEvidence]) -> TaggedAlbumIdAudit {
    let mut grouped = BTreeMap::<String, Vec<PathBuf>>::new();
    let mut missing = Vec::new();
    let mut invalid = Vec::new();

    for track in tracks {
        let raw = track
            .musicbrainz_album_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let Some(raw) = raw else {
            missing.push(track.path.clone());
            continue;
        };

        if let Some(release_mbid) = normalize_mbid(Some(raw)) {
            grouped
                .entry(release_mbid)
                .or_default()
                .push(track.path.clone());
        } else {
            invalid.push((track.path.clone(), raw.to_string()));
        }
    }

    let mut valid: Vec<TaggedAlbumIdEvidence> = grouped
        .into_iter()
        .map(|(release_mbid, mut track_paths)| {
            track_paths.sort();
            TaggedAlbumIdEvidence {
                release_mbid,
                track_paths,
            }
        })
        .collect();

    valid.sort_by(|left, right| {
        right
            .track_paths
            .len()
            .cmp(&left.track_paths.len())
            .then_with(|| left.release_mbid.cmp(&right.release_mbid))
    });
    missing.sort();
    invalid.sort_by(|left, right| left.0.cmp(&right.0));

    TaggedAlbumIdAudit {
        valid,
        missing,
        invalid,
    }
}

pub fn tagged_album_release(tracks: &[LocalTrackEvidence]) -> Option<String> {
    let audit = tagged_album_id_audit(tracks);

    if audit.valid.len() == 1 {
        audit.valid.first().map(|item| item.release_mbid.clone())
    } else {
        None
    }
}

pub fn unanimous_album_release(tracks: &[LocalTrackEvidence]) -> Option<String> {
    tagged_album_release(tracks)
}

pub fn tagged_album_title(tracks: &[LocalTrackEvidence]) -> Option<String> {
    let mut selected: Option<String> = None;

    for track in tracks {
        let Some(album) = track
            .album
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        match selected.as_deref() {
            None => selected = Some(album.to_string()),
            Some(current) if normalized_equal(current, album) => {}
            Some(_) => return None,
        }
    }

    selected
}

pub fn tagged_album_title_matches_release(
    tracks: &[LocalTrackEvidence],
    release_title: &str,
) -> Option<bool> {
    tagged_album_title(tracks).map(|album| normalized_equal(&album, release_title))
}

pub async fn resolve_album_release_workflow(
    _client: &MusicBrainzClient,
    tracks: &[LocalTrackEvidence],
) -> Result<AlbumReleaseDecision, String> {
    if let Some(release_mbid) = tagged_album_release(tracks) {
        return Ok(AlbumReleaseDecision::Resolved {
            release_mbid,
            authority: ReleaseAuthority::ExactAlbumId,
        });
    }

    Ok(AlbumReleaseDecision::Fallback {
        compilation: compilation_context(tracks),
    })
}

pub async fn resolve_album_release_with_fallback(
    client: &MusicBrainzClient,
    tracks: &[LocalTrackEvidence],
) -> Result<AlbumReleaseDecision, String> {
    resolve_album_release_workflow(client, tracks).await
}

fn normalize_mbid(value: Option<&str>) -> Option<String> {
    let value = value?.trim().to_ascii_lowercase();
    let bytes = value.as_bytes();

    let valid = bytes.len() == 36
        && [8, 13, 18, 23]
            .iter()
            .all(|index| bytes.get(*index).is_some_and(|byte| *byte == b'-'))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());

    valid.then_some(value)
}

fn normalized_equal(left: &str, right: &str) -> bool {
    normalize_text(left) == normalize_text(right)
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musicbrainz::MusicBrainzConfig;

    const RELEASE_A: &str = "11111111-1111-1111-1111-111111111111";
    const RELEASE_B: &str = "22222222-2222-2222-2222-222222222222";

    fn track(path: &str, album_id: Option<&str>) -> LocalTrackEvidence {
        LocalTrackEvidence {
            path: PathBuf::from(path),
            title: "Fixture Track".to_string(),
            artist: "Fixture Artist".to_string(),
            album: Some("Fixture Album".to_string()),
            album_artist: Some("Fixture Artist".to_string()),
            musicbrainz_album_id: album_id.map(str::to_string),
            musicbrainz_track_id: None,
            compilation: None,
        }
    }

    #[test]
    fn one_unique_tagged_album_id_is_authority_even_when_some_tracks_are_blank() {
        let tracks = vec![
            track("1.mp3", Some(RELEASE_A)),
            track("2.mp3", None),
            track("3.mp3", Some(RELEASE_A)),
        ];

        assert_eq!(tagged_album_release(&tracks).as_deref(), Some(RELEASE_A));
        assert_eq!(unanimous_album_release(&tracks).as_deref(), Some(RELEASE_A));
    }

    #[test]
    fn tagged_album_id_audit_preserves_conflicts_missing_and_invalid_values() {
        let tracks = vec![
            track("1.mp3", Some(RELEASE_A)),
            track("2.mp3", Some(RELEASE_B)),
            track("3.mp3", Some(RELEASE_A)),
            track("4.mp3", None),
            track("5.mp3", Some("not-an-mbid")),
        ];

        let audit = tagged_album_id_audit(&tracks);
        assert_eq!(audit.valid.len(), 2);
        assert_eq!(audit.missing, vec![PathBuf::from("4.mp3")]);
        assert_eq!(
            audit.invalid,
            vec![(PathBuf::from("5.mp3"), "not-an-mbid".to_string())]
        );
    }

    #[test]
    fn conflicting_tagged_album_ids_are_unresolved() {
        let tracks = vec![
            track("1.mp3", Some(RELEASE_A)),
            track("2.mp3", Some(RELEASE_B)),
        ];
        assert_eq!(tagged_album_release(&tracks), None);
    }

    #[test]
    fn compilation_tag_one_selects_compilation_context() {
        let mut tracks = vec![track("1.mp3", None), track("2.mp3", None)];
        tracks[1].compilation = Some("1".to_string());
        assert_eq!(
            compilation_context(&tracks),
            CompilationContext::Compilation
        );
    }

    #[tokio::test]
    async fn track_ids_never_drive_album_resolution() {
        let mut tracks = vec![track("1.mp3", None), track("2.mp3", None)];
        tracks[0].musicbrainz_track_id = Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string());
        tracks[1].musicbrainz_track_id = Some("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string());

        let client = MusicBrainzClient::new(&MusicBrainzConfig::default()).unwrap();
        let decision = resolve_album_release_with_fallback(&client, &tracks)
            .await
            .unwrap();

        assert_eq!(
            decision,
            AlbumReleaseDecision::Fallback {
                compilation: CompilationContext::Standard,
            }
        );
    }

    #[test]
    fn coherent_album_title_is_validation_only() {
        let tracks = vec![track("1.mp3", Some(RELEASE_A)), track("2.mp3", None)];
        assert_eq!(
            tagged_album_title_matches_release(&tracks, "Fixture Album"),
            Some(true)
        );
        assert_eq!(
            tagged_album_title_matches_release(&tracks, "Different Album"),
            Some(false)
        );
    }
}

use crate::scan::AlbumDirectory;
use crate::scan_musicbrainz::LocalTrackEvidence;
use lofty::file::{TaggedFile, TaggedFileExt};
use lofty::tag::ItemKey;
use std::path::Path;

pub fn read_album_track_evidence(
    album: &AlbumDirectory,
) -> Result<Vec<LocalTrackEvidence>, String> {
    album
        .audio_files
        .iter()
        .map(|path| read_local_track_evidence(path))
        .collect()
}

pub fn read_local_track_evidence(path: &Path) -> Result<LocalTrackEvidence, String> {
    let tagged_file = lofty::read_from_path(path).map_err(|error| {
        format!(
            "Unable to read SPLINED audio tags from {}: {error}",
            path.display()
        )
    })?;

    evidence_from_tagged_file(path, &tagged_file)
}

fn evidence_from_tagged_file(
    path: &Path,
    tagged_file: &TaggedFile,
) -> Result<LocalTrackEvidence, String> {
    let title = read_required(tagged_file, &ItemKey::TrackTitle, "TITLE", path)?;
    let artist = read_required(tagged_file, &ItemKey::TrackArtist, "ARTIST", path)?;

    Ok(LocalTrackEvidence {
        path: path.to_path_buf(),
        title,
        artist,
        album: read_optional(tagged_file, &ItemKey::AlbumTitle),
        album_artist: read_optional(tagged_file, &ItemKey::AlbumArtist),
        musicbrainz_album_id: read_optional(tagged_file, &ItemKey::MusicBrainzReleaseId),
        musicbrainz_track_id: read_optional(tagged_file, &ItemKey::MusicBrainzRecordingId),
        compilation: read_optional(tagged_file, &ItemKey::FlagCompilation),
    })
}

fn read_required(
    tagged_file: &TaggedFile,
    key: &ItemKey,
    field_name: &str,
    path: &Path,
) -> Result<String, String> {
    read_optional(tagged_file, key).ok_or_else(|| {
        format!(
            "SPLINED scan requires {field_name} in actual file tags: {}",
            path.display()
        )
    })
}

fn read_optional(tagged_file: &TaggedFile, key: &ItemKey) -> Option<String> {
    if let Some(primary) = tagged_file.primary_tag()
        && let Some(value) = clean_value(primary.get_string(*key))
    {
        return Some(value);
    }

    tagged_file
        .tags()
        .iter()
        .find_map(|tag| clean_value(tag.get_string(*key)))
}

fn clean_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::file::FileType;
    use lofty::tag::{ItemKey, Tag, TagType};

    const RELEASE_ID: &str = "11111111-1111-1111-1111-111111111111";
    const RECORDING_ID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

    fn populated_tag(tag_type: TagType) -> Tag {
        let mut tag = Tag::new(tag_type);
        assert!(tag.insert_text(ItemKey::TrackTitle, "Track Title".to_string()));
        assert!(tag.insert_text(ItemKey::TrackArtist, "Track Artist".to_string()));
        assert!(tag.insert_text(ItemKey::AlbumTitle, "Album Title".to_string()));
        assert!(tag.insert_text(ItemKey::AlbumArtist, "Album Artist".to_string()));
        assert!(tag.insert_text(ItemKey::MusicBrainzReleaseId, RELEASE_ID.to_string()));

        if tag_type != TagType::Id3v2 {
            assert!(tag.insert_text(ItemKey::MusicBrainzRecordingId, RECORDING_ID.to_string()));
        }

        assert!(tag.insert_text(ItemKey::FlagCompilation, "1".to_string()));
        tag
    }

    fn tagged_file_for(tag: Tag, file_type: FileType) -> TaggedFile {
        TaggedFile::new(file_type, Default::default(), vec![tag])
    }

    fn assert_expected_evidence(
        tag_type: TagType,
        file_type: FileType,
        expected_recording_id: Option<&str>,
    ) {
        let tagged_file = tagged_file_for(populated_tag(tag_type), file_type);
        let evidence = evidence_from_tagged_file(Path::new("fixture.audio"), &tagged_file)
            .expect("tag evidence should map");

        assert_eq!(evidence.title, "Track Title");
        assert_eq!(evidence.artist, "Track Artist");
        assert_eq!(evidence.album.as_deref(), Some("Album Title"));
        assert_eq!(evidence.album_artist.as_deref(), Some("Album Artist"));
        assert_eq!(evidence.musicbrainz_album_id.as_deref(), Some(RELEASE_ID));
        assert_eq!(
            evidence.musicbrainz_track_id.as_deref(),
            expected_recording_id
        );
        assert_eq!(evidence.compilation.as_deref(), Some("1"));
    }

    #[test]
    fn maps_id3v2_common_musicbrainz_and_compilation_fields() {
        assert_expected_evidence(TagType::Id3v2, FileType::Mpeg, None);
    }

    #[test]
    fn maps_vorbis_musicbrainz_and_compilation_fields() {
        assert_expected_evidence(TagType::VorbisComments, FileType::Flac, Some(RECORDING_ID));
    }

    #[test]
    fn maps_mp4_musicbrainz_and_compilation_fields() {
        assert_expected_evidence(TagType::Mp4Ilst, FileType::Mp4, Some(RECORDING_ID));
    }

    #[test]
    fn missing_required_title_is_rejected_without_guessing() {
        let mut tag = Tag::new(TagType::Id3v2);
        assert!(tag.insert_text(ItemKey::TrackArtist, "Artist".to_string()));
        let tagged_file = tagged_file_for(tag, FileType::Mpeg);

        let error = evidence_from_tagged_file(Path::new("missing-title.mp3"), &tagged_file)
            .expect_err("missing title should be rejected");
        assert!(error.contains("TITLE"));
        assert!(error.contains("missing-title.mp3"));
    }

    #[test]
    fn physical_release_track_id_is_not_used_as_recording_authority() {
        let mut tag = Tag::new(TagType::Id3v2);
        assert!(tag.insert_text(ItemKey::TrackTitle, "Track".to_string()));
        assert!(tag.insert_text(ItemKey::TrackArtist, "Artist".to_string()));
        assert!(tag.insert_text(
            ItemKey::MusicBrainzTrackId,
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string()
        ));
        let tagged_file = tagged_file_for(tag, FileType::Mpeg);

        let evidence = evidence_from_tagged_file(Path::new("track.mp3"), &tagged_file)
            .expect("required tags should map");
        assert_eq!(evidence.musicbrainz_track_id, None);
    }
}

use crate::scan::AlbumDirectory;
use crate::scan_musicbrainz::LocalTrackEvidence;
use lofty::config::ParseOptions;
use lofty::file::{TaggedFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAX_ID3V2_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Id3v2Fields {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    artist_sort: Option<String>,
    album_sort: Option<String>,
    release_date: Option<String>,
    musicbrainz_album_id: Option<String>,
    musicbrainz_release_group_id: Option<String>,
    musicbrainz_album_artist_id: Option<String>,
    musicbrainz_track_id: Option<String>,
    musicbrainz_artist_id: Option<String>,
    compilation: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlbumIndexTags {
    pub album: String,
    pub album_artist: String,
    pub artist_sort: String,
    pub album_sort: String,
    pub musicbrainz_album_id: String,
    pub musicbrainz_release_group_id: String,
    pub musicbrainz_album_artist_id: String,
    pub year: String,
    pub compilation: bool,
}

/// Read only the representative-track fields that define the media index.
/// Normal Select Media indexing must continue to call this once per Album.
pub fn read_album_index_tags(path: &Path) -> Result<AlbumIndexTags, String> {
    if is_mpeg_path(path)
        && let Some(fields) = read_id3v2_fields(path)?
    {
        return Ok(album_index_from_id3(fields));
    }

    let tagged_file = read_tags_only(path).map_err(|error| {
        format!(
            "Unable to read SPLINED representative tags from {}: {error}",
            path.display()
        )
    })?;
    let album = read_optional(&tagged_file, &ItemKey::AlbumTitle).unwrap_or_default();
    let album_artist = read_optional(&tagged_file, &ItemKey::AlbumArtist)
        .or_else(|| read_optional(&tagged_file, &ItemKey::TrackArtist))
        .unwrap_or_default();
    let date = read_optional(&tagged_file, &ItemKey::RecordingDate)
        .or_else(|| read_optional(&tagged_file, &ItemKey::OriginalReleaseDate))
        .unwrap_or_default();
    let year = date
        .as_bytes()
        .windows(4)
        .find(|value| value.iter().all(u8::is_ascii_digit))
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .unwrap_or_default();
    let musicbrainz_album_id =
        read_optional(&tagged_file, &ItemKey::MusicBrainzReleaseId).unwrap_or_default();
    let compilation = read_optional(&tagged_file, &ItemKey::FlagCompilation).is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y"
        )
    });

    Ok(AlbumIndexTags {
        album,
        album_artist,
        artist_sort: read_optional(&tagged_file, &ItemKey::AlbumArtistSortOrder)
            .or_else(|| read_optional(&tagged_file, &ItemKey::TrackArtistSortOrder))
            .unwrap_or_default(),
        album_sort: read_optional(&tagged_file, &ItemKey::AlbumTitleSortOrder).unwrap_or_default(),
        musicbrainz_album_id,
        musicbrainz_release_group_id: read_optional(
            &tagged_file,
            &ItemKey::MusicBrainzReleaseGroupId,
        )
        .unwrap_or_default(),
        musicbrainz_album_artist_id: read_optional(
            &tagged_file,
            &ItemKey::MusicBrainzReleaseArtistId,
        )
        .or_else(|| read_optional(&tagged_file, &ItemKey::MusicBrainzArtistId))
        .unwrap_or_default(),
        year,
        compilation,
    })
}

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
    if is_mpeg_path(path)
        && let Some(fields) = read_id3v2_fields(path)?
    {
        return evidence_from_id3(path, fields);
    }

    let tagged_file = read_tags_only(path).map_err(|error| {
        format!(
            "Unable to read SPLINED audio tags from {}: {error}",
            path.display()
        )
    })?;

    evidence_from_tagged_file(path, &tagged_file)
}

/// Album identity needs tags only. Lofty's default path reader also reads
/// audio properties and cover art; on an SMB library that can turn a large
/// box set into minutes of avoidable network I/O. Keep the same authoritative
/// per-track evidence while limiting reads to tag blocks.
fn read_tags_only(path: &Path) -> Result<TaggedFile, lofty::error::FileParseError> {
    Probe::open(path)?
        .options(
            ParseOptions::new()
                .read_properties(false)
                .read_cover_art(false),
        )
        .read()
}

fn evidence_from_tagged_file(
    path: &Path,
    tagged_file: &TaggedFile,
) -> Result<LocalTrackEvidence, String> {
    let title = read_required(tagged_file, &ItemKey::TrackTitle, "TITLE", path)?;
    let artist = read_required(tagged_file, &ItemKey::TrackArtist, "ARTIST", path)?;
    let musicbrainz_album_id = read_optional(tagged_file, &ItemKey::MusicBrainzReleaseId);

    Ok(LocalTrackEvidence {
        path: path.to_path_buf(),
        title,
        artist,
        album: read_optional(tagged_file, &ItemKey::AlbumTitle),
        album_artist: read_optional(tagged_file, &ItemKey::AlbumArtist),
        musicbrainz_album_id,
        musicbrainz_track_id: read_optional(tagged_file, &ItemKey::MusicBrainzRecordingId),
        musicbrainz_artist_id: read_optional(tagged_file, &ItemKey::MusicBrainzArtistId),
        compilation: read_optional(tagged_file, &ItemKey::FlagCompilation),
    })
}

fn is_mpeg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("mp3"))
}

fn album_index_from_id3(fields: Id3v2Fields) -> AlbumIndexTags {
    let date = fields.release_date.unwrap_or_default();
    let year = date
        .as_bytes()
        .windows(4)
        .find(|value| value.iter().all(u8::is_ascii_digit))
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .unwrap_or_default();
    let compilation = fields.compilation.as_deref().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y"
        )
    });

    AlbumIndexTags {
        album: fields.album.unwrap_or_default(),
        album_artist: fields.album_artist.or(fields.artist).unwrap_or_default(),
        artist_sort: fields.artist_sort.unwrap_or_default(),
        album_sort: fields.album_sort.unwrap_or_default(),
        musicbrainz_album_id: fields.musicbrainz_album_id.unwrap_or_default(),
        musicbrainz_release_group_id: fields.musicbrainz_release_group_id.unwrap_or_default(),
        musicbrainz_album_artist_id: fields.musicbrainz_album_artist_id.unwrap_or_default(),
        year,
        compilation,
    }
}

fn evidence_from_id3(path: &Path, fields: Id3v2Fields) -> Result<LocalTrackEvidence, String> {
    let title = required_id3_value(fields.title, "TITLE", path)?;
    let artist = required_id3_value(fields.artist, "ARTIST", path)?;
    Ok(LocalTrackEvidence {
        path: path.to_path_buf(),
        title,
        artist,
        album: fields.album,
        album_artist: fields.album_artist,
        musicbrainz_album_id: fields.musicbrainz_album_id,
        musicbrainz_track_id: fields.musicbrainz_track_id,
        musicbrainz_artist_id: fields.musicbrainz_artist_id,
        compilation: fields.compilation,
    })
}

fn required_id3_value(value: Option<String>, field: &str, path: &Path) -> Result<String, String> {
    value.ok_or_else(|| {
        format!(
            "SPLINED scan requires {field} in actual file tags: {}",
            path.display()
        )
    })
}

/// Read only the leading ID3v2 tag. Unlike a whole MPEG container parse this
/// never seeks to the end of an MP3 for ID3v1, Lyrics3 or APE tags. That keeps
/// the all-track authority audit bounded to one small sequential SMB read per
/// track, matching Mutagen's ID3-only behavior without shipping Python in the
/// native Windows package.
fn read_id3v2_fields(path: &Path) -> Result<Option<Id3v2Fields>, String> {
    let mut file = File::open(path).map_err(|error| {
        format!(
            "Unable to open SPLINED audio tags {}: {error}",
            path.display()
        )
    })?;
    let mut header = [0_u8; 10];
    file.read_exact(&mut header).map_err(|error| {
        format!(
            "Unable to read SPLINED ID3 header {}: {error}",
            path.display()
        )
    })?;
    if &header[..3] != b"ID3" {
        return Ok(None);
    }
    let version = header[3];
    if !(2..=4).contains(&version) {
        return Err(format!(
            "Unsupported ID3v2.{version} tag in {}",
            path.display()
        ));
    }
    // Rare globally-unsynchronised tags need Lofty's full parser. Do not risk
    // frame-boundary drift in the bounded fast path.
    if header[5] & 0x80 != 0 {
        return Ok(None);
    }
    let size = syncsafe_u32(&header[6..10]) as usize;
    if size > MAX_ID3V2_BYTES {
        return Err(format!(
            "ID3v2 tag in {} exceeds the {} byte safety limit",
            path.display(),
            MAX_ID3V2_BYTES
        ));
    }
    let mut bytes = vec![0_u8; size];
    file.read_exact(&mut bytes).map_err(|error| {
        format!(
            "Unable to read SPLINED ID3v2 tag {}: {error}",
            path.display()
        )
    })?;
    Ok(Some(parse_id3v2_frames(version, header[5], &bytes)))
}

fn parse_id3v2_frames(version: u8, flags: u8, bytes: &[u8]) -> Id3v2Fields {
    let mut result = Id3v2Fields::default();
    let mut offset = extended_header_size(version, flags, bytes).min(bytes.len());
    let header_size = if version == 2 { 6 } else { 10 };
    while offset + header_size <= bytes.len() {
        let (id, frame_size, content_offset) = if version == 2 {
            let id = &bytes[offset..offset + 3];
            if id.iter().all(|value| *value == 0) {
                break;
            }
            let size = ((bytes[offset + 3] as usize) << 16)
                | ((bytes[offset + 4] as usize) << 8)
                | bytes[offset + 5] as usize;
            (String::from_utf8_lossy(id).into_owned(), size, offset + 6)
        } else {
            let id = &bytes[offset..offset + 4];
            if id.iter().all(|value| *value == 0) {
                break;
            }
            let size = if version == 4 {
                syncsafe_u32(&bytes[offset + 4..offset + 8]) as usize
            } else {
                u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize
            };
            (String::from_utf8_lossy(id).into_owned(), size, offset + 10)
        };
        let Some(end) = content_offset.checked_add(frame_size) else {
            break;
        };
        if frame_size == 0 || end > bytes.len() {
            break;
        }
        apply_id3_frame(&mut result, &id, &bytes[content_offset..end]);
        offset = end;
    }
    result
}

fn extended_header_size(version: u8, flags: u8, bytes: &[u8]) -> usize {
    if flags & 0x40 == 0 || bytes.len() < 4 {
        return 0;
    }
    if version == 3 {
        4_usize.saturating_add(u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize)
    } else if version == 4 {
        syncsafe_u32(&bytes[..4]) as usize
    } else {
        0
    }
}

fn apply_id3_frame(fields: &mut Id3v2Fields, id: &str, payload: &[u8]) {
    match id {
        "TIT2" | "TT2" => fields.title = decode_text_frame(payload),
        "TPE1" | "TP1" => fields.artist = decode_text_frame(payload),
        "TALB" | "TAL" => fields.album = decode_text_frame(payload),
        "TPE2" | "TP2" => fields.album_artist = decode_text_frame(payload),
        "TSOP" | "TSP" => fields.artist_sort = decode_text_frame(payload),
        "TSO2" => fields.artist_sort = decode_text_frame(payload),
        "TSOA" | "TSA" => fields.album_sort = decode_text_frame(payload),
        "TDRC" | "TYER" | "TYE" => fields.release_date = decode_text_frame(payload),
        "TCMP" | "TCP" => fields.compilation = decode_text_frame(payload),
        "TXXX" | "TXX" => apply_user_text(fields, payload),
        "UFID" | "UFI" => apply_unique_file_id(fields, payload),
        _ => {}
    }
}

fn apply_user_text(fields: &mut Id3v2Fields, payload: &[u8]) {
    let Some(text) = decode_text_frame(payload) else {
        return;
    };
    let mut parts = text.splitn(2, '\0');
    let description = normalize_description(parts.next().unwrap_or_default());
    let value = parts.next().and_then(|value| clean_value(Some(value)));
    match description.as_str() {
        "musicbrainzalbumid" | "musicbrainzreleaseid" => fields.musicbrainz_album_id = value,
        "musicbrainzreleasegroupid" => fields.musicbrainz_release_group_id = value,
        "musicbrainzalbumartistid" | "musicbrainzreleaseartistid" => {
            fields.musicbrainz_album_artist_id = value
        }
        "musicbrainztrackid" | "musicbrainzrecordingid" => fields.musicbrainz_track_id = value,
        "musicbrainzartistid" => fields.musicbrainz_artist_id = value,
        "compilation" => fields.compilation = value,
        _ => {}
    }
}

fn apply_unique_file_id(fields: &mut Id3v2Fields, payload: &[u8]) {
    let Some(separator) = payload.iter().position(|value| *value == 0) else {
        return;
    };
    let owner = String::from_utf8_lossy(&payload[..separator]);
    if owner.eq_ignore_ascii_case("http://musicbrainz.org") {
        fields.musicbrainz_track_id = clean_value(Some(
            String::from_utf8_lossy(&payload[separator + 1..]).as_ref(),
        ));
    }
}

fn decode_text_frame(payload: &[u8]) -> Option<String> {
    let (&encoding, content) = payload.split_first()?;
    let decoded = match encoding {
        0 => content.iter().map(|value| char::from(*value)).collect(),
        1 => decode_utf16(content, None),
        2 => decode_utf16(content, Some(true)),
        3 => String::from_utf8_lossy(content).into_owned(),
        _ => return None,
    };
    clean_value(Some(decoded.trim_end_matches('\0')))
}

fn decode_utf16(bytes: &[u8], forced_big_endian: Option<bool>) -> String {
    let (big_endian, content) = if let Some(value) = forced_big_endian {
        (value, bytes)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        (true, &bytes[2..])
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (false, &bytes[2..])
    } else {
        (false, bytes)
    };
    let units = content.as_chunks::<2>().0.iter().map(|pair| {
        if big_endian {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_le_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(units)
        .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn syncsafe_u32(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0_u32, |value, byte| (value << 7) | u32::from(byte & 0x7F))
}

fn normalize_description(description: &str) -> String {
    description
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
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
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    const RELEASE_ID: &str = "11111111-1111-1111-1111-111111111111";
    const RECORDING_ID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn id3_text_frame(id: &str, value: &str) -> Vec<u8> {
        let mut payload = vec![3];
        payload.extend_from_slice(value.as_bytes());
        let mut frame = Vec::new();
        frame.extend_from_slice(id.as_bytes());
        let size = payload.len() as u32;
        frame.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&payload);
        frame
    }

    fn id3_user_text_frame(description: &str, value: &str) -> Vec<u8> {
        id3_text_frame("TXXX", &format!("{description}\0{value}"))
    }

    fn write_id3_fixture(frames: &[Vec<u8>]) -> std::path::PathBuf {
        let body = frames.iter().flatten().copied().collect::<Vec<_>>();
        let size = body.len() as u32;
        let mut bytes = b"ID3\x04\x00\x00".to_vec();
        bytes.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        bytes.extend_from_slice(&body);
        // A large-looking audio tail proves the reader only needs the leading
        // ID3 block; its contents are deliberately not a valid MPEG stream.
        bytes.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        let path = std::env::temp_dir().join(format!(
            "splined-id3-prefix-{}-{}.mp3",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        path
    }

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
    fn reads_required_mp3_evidence_from_the_leading_id3_tag_only() {
        let path = write_id3_fixture(&[
            id3_text_frame("TIT2", "Track Title"),
            id3_text_frame("TPE1", "Track Artist"),
            id3_text_frame("TALB", "Album Title"),
            id3_text_frame("TPE2", "Album Artist"),
            id3_user_text_frame("MUSICBRAINZ_ALBUMID", RELEASE_ID),
            id3_user_text_frame("MusicBrainz Track Id", RECORDING_ID),
            id3_text_frame("TCMP", "1"),
        ]);
        let evidence = read_local_track_evidence(&path).unwrap();
        assert_eq!(evidence.title, "Track Title");
        assert_eq!(evidence.artist, "Track Artist");
        assert_eq!(evidence.album.as_deref(), Some("Album Title"));
        assert_eq!(evidence.album_artist.as_deref(), Some("Album Artist"));
        assert_eq!(evidence.musicbrainz_album_id.as_deref(), Some(RELEASE_ID));
        assert_eq!(evidence.musicbrainz_track_id.as_deref(), Some(RECORDING_ID));
        assert_eq!(evidence.compilation.as_deref(), Some("1"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn maps_case_and_separator_variants_without_accepting_release_group_id() {
        for description in [
            "MusicBrainz Album Id",
            "MusicBrainz Album ID",
            "musicbrainz_albumid",
            "MUSICBRAINZ-RELEASE-ID",
        ] {
            let parsed = parse_id3v2_frames(4, 0, &id3_user_text_frame(description, RELEASE_ID));
            assert_eq!(
                parsed.musicbrainz_album_id.as_deref(),
                Some(RELEASE_ID),
                "description {description:?} should be recognized"
            );
        }

        let release_group = parse_id3v2_frames(
            4,
            0,
            &id3_user_text_frame("MUSICBRAINZ_RELEASEGROUPID", RELEASE_ID),
        );
        assert_eq!(release_group.musicbrainz_album_id, None);
        assert_eq!(
            release_group.musicbrainz_release_group_id.as_deref(),
            Some(RELEASE_ID)
        );
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

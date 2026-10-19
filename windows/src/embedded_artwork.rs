//! Safe per-track front-cover replacement used only by curated compilations.
//!
//! Normal Album processing remains folder-artwork based. This module stages a
//! complete audio-file copy beside the source, changes only the front picture,
//! verifies the staged tag, and then uses SPLINED's recoverable replacement.

use crate::safe_write::transform_existing_file;
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{ItemKey, Tag, TagExt};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::Path;

pub fn embedded_front_sha256s(track_path: &Path) -> Result<Vec<String>, String> {
    let tagged = lofty::read_from_path(track_path).map_err(|error| {
        format!(
            "Unable to read embedded artwork tags from {}: {error}",
            track_path.display()
        )
    })?;
    Ok(tagged
        .tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .filter(|picture| picture.pic_type() == PictureType::CoverFront)
        .map(|picture| sha256_hex(picture.data()))
        .collect())
}

pub fn embedded_front_matches(track_path: &Path, artwork: &[u8]) -> Result<bool, String> {
    let expected = sha256_hex(artwork);
    Ok(embedded_front_sha256s(track_path)?
        .iter()
        .any(|hash| hash == &expected))
}

/// Read-only dimensions and encoded byte count of the track's current front
/// picture. This never consults a folder-level cover.* file.
pub fn embedded_front_metadata(track_path: &Path) -> Result<Option<(u32, u32, u64)>, String> {
    let tagged = lofty::read_from_path(track_path).map_err(|error| {
        format!(
            "Unable to read embedded artwork tags from {}: {error}",
            track_path.display()
        )
    })?;
    let picture = tagged
        .tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .find(|picture| picture.pic_type() == PictureType::CoverFront);
    let Some(picture) = picture else {
        return Ok(None);
    };
    let dimensions = image::ImageReader::new(Cursor::new(picture.data()))
        .with_guessed_format()
        .map_err(|error| format!("Unable to identify embedded artwork: {error}"))?
        .into_dimensions()
        .map_err(|error| format!("Unable to inspect embedded artwork dimensions: {error}"))?;
    Ok(Some((
        dimensions.0,
        dimensions.1,
        picture.data().len() as u64,
    )))
}

fn sha256_hex(value: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(value);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub fn replace_embedded_front(track_path: &Path, artwork: &[u8]) -> Result<(), String> {
    let mut reader = Cursor::new(artwork);
    let mut picture = Picture::from_reader(&mut reader)
        .map_err(|error| format!("Embedded artwork is not a supported image: {error}"))?;
    match picture.mime_type() {
        Some(MimeType::Jpeg | MimeType::Png) => {}
        _ => return Err("Embedded artwork must be prepared as JPEG or PNG.".to_string()),
    }
    picture.set_pic_type(PictureType::CoverFront);
    let expected_hash = Sha256::digest(artwork).to_vec();

    transform_existing_file(
        track_path,
        "compilation embedded artwork",
        move |staged| {
            let tagged = lofty::read_from_path(staged).map_err(|error| {
                format!(
                    "Unable to read staged audio tags from {}: {error}",
                    staged.display()
                )
            })?;
            let mut tag = tagged
                .primary_tag()
                .cloned()
                .unwrap_or_else(|| Tag::new(tagged.primary_tag_type()));
            tag.remove_picture_type(PictureType::CoverFront);
            tag.push_picture(picture);
            tag.save_to_path(staged, WriteOptions::default())
                .map_err(|error| {
                    format!(
                        "Unable to save staged embedded artwork to {}: {error}",
                        staged.display()
                    )
                })
        },
        move |staged| {
            let tagged = lofty::read_from_path(staged).map_err(|error| {
                format!(
                    "Unable to verify staged audio tags from {}: {error}",
                    staged.display()
                )
            })?;
            let matches = tagged
                .tags()
                .iter()
                .flat_map(|tag| tag.pictures())
                .any(|item| {
                    item.pic_type() == PictureType::CoverFront
                        && Sha256::digest(item.data()).as_slice() == expected_hash.as_slice()
                });
            if matches {
                Ok(())
            } else {
                Err("Staged audio file did not retain the selected front artwork.".to_string())
            }
        },
    )
}

/// Persist a user-validated MusicBrainz Recording/Artist identity without
/// touching audio frames, embedded artwork, or the Album/Release identity.
/// The same recoverable staging path used for artwork writes protects the
/// source track from a partial tag save.
pub fn replace_musicbrainz_identity(
    track_path: &Path,
    recording_mbid: &str,
    artist_mbid: &str,
) -> Result<(), String> {
    let recording_mbid = recording_mbid.trim().to_ascii_lowercase();
    let artist_mbid = artist_mbid.trim().to_ascii_lowercase();
    if recording_mbid.is_empty() || artist_mbid.is_empty() {
        return Err("Recording and Artist MBIDs are both required.".to_string());
    }
    let expected_recording = recording_mbid.clone();
    let expected_artist = artist_mbid.clone();
    transform_existing_file(
        track_path,
        "MusicBrainz track identity",
        move |staged| {
            let tagged = lofty::read_from_path(staged).map_err(|error| {
                format!(
                    "Unable to read staged MusicBrainz tags from {}: {error}",
                    staged.display()
                )
            })?;
            let mut tag = tagged
                .primary_tag()
                .cloned()
                .unwrap_or_else(|| Tag::new(tagged.primary_tag_type()));
            tag.remove_key(ItemKey::MusicBrainzRecordingId);
            tag.remove_key(ItemKey::MusicBrainzArtistId);
            tag.insert_text(ItemKey::MusicBrainzRecordingId, recording_mbid.clone());
            tag.insert_text(ItemKey::MusicBrainzArtistId, artist_mbid.clone());
            tag.save_to_path(staged, WriteOptions::default())
                .map_err(|error| {
                    format!(
                        "Unable to save staged MusicBrainz tags to {}: {error}",
                        staged.display()
                    )
                })
        },
        move |staged| {
            let tagged = lofty::read_from_path(staged).map_err(|error| {
                format!(
                    "Unable to verify staged MusicBrainz tags from {}: {error}",
                    staged.display()
                )
            })?;
            let matches = tagged.tags().iter().any(|tag| {
                tag.get_string(ItemKey::MusicBrainzRecordingId)
                    .is_some_and(|value| value.eq_ignore_ascii_case(&expected_recording))
                    && tag
                        .get_string(ItemKey::MusicBrainzArtistId)
                        .is_some_and(|value| value.eq_ignore_ascii_case(&expected_artist))
            });
            matches.then_some(()).ok_or_else(|| {
                "Staged audio file did not retain the validated MusicBrainz identity.".to_string()
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_image_is_rejected_before_track_mutation() {
        let error = replace_embedded_front(Path::new("missing.mp3"), b"not-an-image")
            .expect_err("invalid image should fail first");
        assert!(error.contains("supported image"));
    }

    #[test]
    fn embedded_artwork_digest_uses_lowercase_sha256() {
        assert_eq!(
            sha256_hex(b"SPLINED embedded artwork"),
            "15bb1cd87155169d373d5c56732c7e98a0149f12a49f47d8ef06fdd2d1982f5b"
        );
    }
}

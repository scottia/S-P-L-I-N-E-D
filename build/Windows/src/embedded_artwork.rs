//! Safe per-track front-cover replacement for album queues and compilations.
//!
//! Normal Album processing remains folder-artwork based. This module stages a
//! complete audio-file copy beside the source, changes only the front picture,
//! verifies the staged tag, and then uses SPLINED's recoverable replacement.

use crate::safe_write::transform_existing_file_with_trace;
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Tag, TagExt};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

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
    replace_embedded_front_with_trace(track_path, artwork, &mut |_, _| {})
}

pub fn replace_embedded_front_with_trace(
    track_path: &Path,
    artwork: &[u8],
    trace: &mut dyn FnMut(&'static str, Duration),
) -> Result<(), String> {
    let mut reader = Cursor::new(artwork);
    let mut picture = Picture::from_reader(&mut reader)
        .map_err(|error| format!("Embedded artwork is not a supported image: {error}"))?;
    match picture.mime_type() {
        Some(MimeType::Jpeg | MimeType::Png) => {}
        _ => return Err("Embedded artwork must be prepared as JPEG or PNG.".to_string()),
    }
    picture.set_pic_type(PictureType::CoverFront);
    let expected_hash = Sha256::digest(artwork).to_vec();

    transform_existing_file_with_trace(
        track_path,
        "compilation embedded artwork",
        move |staged| {
            if staged.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("mp3")) {
                let mime = match picture.mime_type() {
                    Some(MimeType::Png) => "image/png",
                    _ => "image/jpeg",
                };
                return crate::artwork_integrity::write_mp3_front(staged, artwork, mime);
            }
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
            // Protect ID3 version, each non-front frame (including MusicBrainz),
            // other audio metadata and the complete audio stream before commit.
            crate::artwork_integrity::verify_only_front_artwork(track_path, staged)?;
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
        trace,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id3v24_front_picture_never_alters_multivalue_artist_or_mb_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credits.mp3");
        let mut tag = Vec::new();
        fn frame(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut encoded = Vec::new();
            encoded.extend_from_slice(id);
            encoded.extend_from_slice(&encode_synchsafe(payload.len()).unwrap());
            encoded.extend_from_slice(&[0, 0]);
            encoded.extend_from_slice(payload);
            encoded
        }
        tag.extend_from_slice(&frame(b"TPE1", b"\x03Sammy Hagar\x00Taj Mahal"));
        tag.extend_from_slice(&frame(b"TPE2", b"\x03Sammy Hagar\x00Sammy Hagar & Friends"));
        tag.extend_from_slice(&frame(
            b"TXXX", b"\x03MusicBrainz Artist Id\x00aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa",
        ));
        let mut original = b"ID3\x04\x00\x00".to_vec();
        original.extend_from_slice(&encode_synchsafe(tag.len()).unwrap());
        original.extend_from_slice(&tag);
        // Existing MPEG payload is copied byte-for-byte, without decoding.
        let mut audio = vec![0u8; 417 * 8];
        for frame in audio.chunks_exact_mut(417) {
            frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
        }
        original.extend_from_slice(&audio);
        std::fs::write(&path, &original).unwrap();

        let before = protected_mp3_bytes(&path).unwrap();
        // Exercise the production staged writer and lofty readback, not
        // merely a mocked or internal byte-stream helper.
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut encoded, image::ImageFormat::Png).unwrap();
        replace_embedded_front(&path, &encoded.into_inner()).unwrap();
        assert_eq!(protected_mp3_bytes(&path).unwrap(), before);
        assert!(std::fs::read(&path).unwrap().ends_with(&audio));

        let mut tampered = std::fs::read(&path).unwrap();
        let index = tampered.windows(5).position(|w| w == b"Sammy").unwrap();
        tampered[index] = b'X';
        std::fs::write(&path, tampered).unwrap();
        assert_ne!(protected_mp3_bytes(&path).unwrap(), before);
    }

    #[test]
    fn id3_major_version_conversion_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("version.mp3");
        let mut input = b"ID3\x04\x00\x00\x00\x00\x00\x00".to_vec();
        input.extend_from_slice(b"\xff\xfb\x90\x64");
        std::fs::write(&path, &input).unwrap();
        let baseline = protected_mp3_bytes(&path).unwrap();
        input[3] = 3;
        std::fs::write(&path, input).unwrap();
        assert_ne!(protected_mp3_bytes(&path).unwrap(), baseline);
    }

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

//! Safe per-track front-cover replacement used only by curated compilations.
//!
//! Normal Album processing remains folder-artwork based. This module stages a
//! complete audio-file copy beside the source, changes only the front picture,
//! verifies the staged tag, and then uses SPLINED's recoverable replacement.

use crate::safe_write::transform_existing_file;
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Tag, TagExt};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_image_is_rejected_before_track_mutation() {
        let error = replace_embedded_front(Path::new("missing.mp3"), b"not-an-image")
            .expect_err("invalid image should fail first");
        assert!(error.contains("supported image"));
    }
}

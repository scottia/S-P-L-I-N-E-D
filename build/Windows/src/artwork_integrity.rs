//! Fail-closed, byte-level verification of artwork-only audio-file changes.
//! The original music file is never modified until this verification succeeds.

use std::fs;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
struct Mp3View {
    version: Option<[u8; 3]>,
    non_front_frames: Vec<Vec<u8>>,
    audio: Vec<u8>,
}

fn syncsafe(input: &[u8]) -> Result<usize, String> {
    if input.len() != 4 || input.iter().any(|value| value & 0x80 != 0) {
        return Err("Invalid ID3 synchsafe length".into());
    }
    Ok((usize::from(input[0]) << 21)
        | (usize::from(input[1]) << 14)
        | (usize::from(input[2]) << 7)
        | usize::from(input[3]))
}

fn encode_syncsafe(value: usize) -> Result<[u8; 4], String> {
    if value >= (1 << 28) {
        return Err("Embedded ID3 artwork exceeds supported tag size".into());
    }
    Ok([
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ])
}

fn front_apic(frame: &[u8]) -> Result<bool, String> {
    if &frame[..4] != b"APIC" {
        return Ok(false);
    }
    if frame[8..10] != [0, 0] {
        return Err("Unsupported flagged ID3 artwork frame".into());
    }
    let data = &frame[10..];
    let mime_end = data
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, value)| **value == 0)
        .map(|(index, _)| index)
        .ok_or("Invalid ID3 artwork MIME type")?;
    let picture_type = *data.get(mime_end + 1).ok_or("Missing ID3 picture type")?;
    Ok(picture_type == 3)
}

fn parse_mp3(bytes: &[u8]) -> Result<Mp3View, String> {
    if !bytes.starts_with(b"ID3") {
        return Ok(Mp3View {
            version: None,
            non_front_frames: Vec::new(),
            audio: bytes.to_vec(),
        });
    }
    if bytes.len() < 10 || !matches!(bytes[3], 3 | 4) || bytes[4] != 0 || bytes[5] != 0 {
        return Err("Unsupported ID3 version or flags; media is unchanged".into());
    }
    let tag_end = 10usize
        .checked_add(syncsafe(&bytes[6..10])?)
        .ok_or("Invalid ID3 tag length")?;
    if tag_end > bytes.len() {
        return Err("ID3 tag exceeds the media file".into());
    }
    let mut offset = 10;
    let mut frames = Vec::new();
    while offset + 10 <= tag_end {
        let frame = &bytes[offset..offset + 10];
        if frame.iter().all(|value| *value == 0) {
            if bytes[offset..tag_end].iter().any(|value| *value != 0) {
                return Err("Invalid ID3 padding".into());
            }
            break;
        }
        if !frame[..4].iter().all(|value| value.is_ascii_uppercase() || value.is_ascii_digit()) {
            return Err("Unsupported ID3 frame identifier".into());
        }
        let size = if bytes[3] == 4 {
            syncsafe(&frame[4..8])?
        } else {
            u32::from_be_bytes(frame[4..8].try_into().expect("four bytes")) as usize
        };
        let end = offset
            .checked_add(10)
            .and_then(|pos| pos.checked_add(size))
            .ok_or("Invalid ID3 frame size")?;
        if end > tag_end {
            return Err("ID3 frame extends beyond the tag".into());
        }
        if !front_apic(&bytes[offset..end])? {
            frames.push(bytes[offset..end].to_vec());
        }
        offset = end;
    }
    if bytes[offset..tag_end].iter().any(|value| *value != 0) {
        return Err("Unverifiable ID3 frame boundary".into());
    }
    Ok(Mp3View {
        version: Some([bytes[3], bytes[4], bytes[5]]),
        non_front_frames: frames,
        audio: bytes[tag_end..].to_vec(),
    })
}

pub fn write_mp3_front(path: &Path, image: &[u8], mime: &str) -> Result<(), String> {
    if !matches!(mime, "image/png" | "image/jpeg") {
        return Err("Only JPEG or PNG front artwork is supported".into());
    }
    let original = fs::read(path).map_err(|error| format!("Unable to read staged MP3: {error}"))?;
    let parsed = parse_mp3(&original)?;
    let version = parsed.version.map_or(4, |value| value[0]);
    let mut data = Vec::with_capacity(image.len() + 32);
    data.push(0); // Latin-1 ASCII description is valid for both ID3v2.3 and v2.4.
    data.extend_from_slice(mime.as_bytes());
    data.extend_from_slice(&[0, 3]); // MIME terminator and front-cover picture type.
    data.extend_from_slice(b"Cover\0");
    data.extend_from_slice(image);
    let mut front = b"APIC".to_vec();
    if version == 4 {
        front.extend_from_slice(&encode_syncsafe(data.len())?);
    } else {
        let length = u32::try_from(data.len()).map_err(|_| "ID3 front image is too large")?;
        front.extend_from_slice(&length.to_be_bytes());
    }
    front.extend_from_slice(&[0, 0]); // No compression/encryption or unsynchronization.
    front.extend_from_slice(&data);
    let payload_len: usize = parsed.non_front_frames.iter().map(Vec::len).sum::<usize>()
        + front.len() + 128;
    let mut updated = vec![b'I', b'D', b'3', version, 0, 0];
    updated.extend_from_slice(&encode_syncsafe(payload_len)?);
    for frame in parsed.non_front_frames {
        updated.extend_from_slice(&frame);
    }
    updated.extend_from_slice(&front);
    updated.resize(updated.len() + 128, 0); // ID3 padding is not a text tag.
    updated.extend_from_slice(&parsed.audio);
    fs::write(path, updated).map_err(|error| format!("Unable to save staged front artwork: {error}"))
}

fn parse_flac(bytes: &[u8]) -> Result<(Vec<(u8, Vec<u8>)>, Vec<u8>), String> {
    if !bytes.starts_with(b"fLaC") {
        return Err("Unsupported FLAC header".into());
    }
    let mut offset = 4usize;
    let mut metadata = Vec::new();
    loop {
        if offset + 4 > bytes.len() {
            return Err("Truncated FLAC metadata header".into());
        }
        let marker = bytes[offset];
        let kind = marker & 0x7f;
        let len = (usize::from(bytes[offset + 1]) << 16)
            | (usize::from(bytes[offset + 2]) << 8)
            | usize::from(bytes[offset + 3]);
        let end = offset + 4 + len;
        if end > bytes.len() {
            return Err("Truncated FLAC metadata block".into());
        }
        let payload = &bytes[offset + 4..end];
        let front = kind == 6
            && payload.len() >= 4
            && u32::from_be_bytes(payload[..4].try_into().expect("four bytes")) == 3;
        if kind != 1 && !front {
            metadata.push((kind, payload.to_vec()));
        }
        offset = end;
        if marker & 0x80 != 0 {
            break;
        }
    }
    Ok((metadata, bytes[offset..].to_vec()))
}

/// Fail closed for any unverified audio container or non-front metadata change.
/// Check *before* the recoverable staged-file installation operation.
pub fn verify_only_front_artwork(original: &Path, staged: &Path) -> Result<(), String> {
    let extension = original.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    if extension != staged.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase() {
        return Err("Staged audio container differs from the original".into());
    }
    let before = fs::read(original).map_err(|error| format!("Unable to inspect original: {error}"))?;
    let after = fs::read(staged).map_err(|error| format!("Unable to inspect staged audio: {error}"))?;
    let unchanged = match extension.as_str() {
        "mp3" => {
            let original = parse_mp3(&before)?;
            let staged = parse_mp3(&after)?;
            let versions_match = original.version == staged.version
                || (original.version.is_none()
                    && staged.version == Some([4, 0, 0])
                    && staged.non_front_frames.is_empty());
            versions_match
                && original.non_front_frames == staged.non_front_frames
                && original.audio == staged.audio
        }
        "flac" => parse_flac(&before)? == parse_flac(&after)?,
        _ => return Err(format!("Artwork-only integrity is not verified for .{extension}; original preserved")),
    };
    if unchanged {
        Ok(())
    } else {
        Err("Non-artwork tag, ID3 version, or audio bytes changed; original preserved".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn frame(version: u8, id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut frame = id.to_vec();
        if version == 4 {
            frame.extend_from_slice(&encode_syncsafe(payload.len()).unwrap());
        } else {
            frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        }
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(payload);
        frame
    }

    fn fixture(version: u8) -> Vec<u8> {
        let artist = if version == 4 {
            b"\x03Sammy Hagar\0Taj Mahal".as_slice()
        } else {
            b"\x00Sammy Hagar/Taj Mahal".as_slice()
        };
        let frames = [
            frame(version, b"TPE1", artist),
            frame(version, b"TXXX", b"\x03MusicBrainz Artist Id\0example-id"),
            frame(version, b"TIT2", b"\x03Fooey"),
        ]
        .concat();
        let mut result = vec![b'I', b'D', b'3', version, 0, 0];
        result.extend_from_slice(&encode_syncsafe(frames.len()).unwrap());
        result.extend_from_slice(&frames);
        result.extend_from_slice(b"\xff\xfb\0test-audio-bytes");
        result
    }

    #[test]
    fn id3v24_multivalue_tags_survive_front_artwork_replacement() {
        let dir = tempdir().unwrap();
        let original = dir.path().join("original.mp3");
        let staged = dir.path().join("staged.mp3");
        fs::write(&original, fixture(4)).unwrap();
        fs::copy(&original, &staged).unwrap();
        write_mp3_front(&staged, b"artwork-only", "image/jpeg").unwrap();
        verify_only_front_artwork(&original, &staged).unwrap();
        assert_eq!(parse_mp3(&fs::read(&staged).unwrap()).unwrap().version, Some([4, 0, 0]));
    }

    #[test]
    fn id3v23_version_is_not_upgraded() {
        let dir = tempdir().unwrap();
        let original = dir.path().join("original.mp3");
        let staged = dir.path().join("staged.mp3");
        fs::write(&original, fixture(3)).unwrap();
        fs::copy(&original, &staged).unwrap();
        write_mp3_front(&staged, b"artwork-only", "image/jpeg").unwrap();
        verify_only_front_artwork(&original, &staged).unwrap();
        assert_eq!(parse_mp3(&fs::read(&staged).unwrap()).unwrap().version, Some([3, 0, 0]));
    }

    #[test]
    fn non_artwork_mutation_is_rejected_without_changing_original() {
        let dir = tempdir().unwrap();
        let original = dir.path().join("original.mp3");
        let staged = dir.path().join("staged.mp3");
        let source = fixture(4);
        fs::write(&original, &source).unwrap();
        fs::write(&staged, &source).unwrap();
        let mut changed = fs::read(&staged).unwrap();
        let index = changed.windows(5).position(|part| part == b"Sammy").unwrap();
        changed[index] = b'T';
        fs::write(&staged, changed).unwrap();
        assert!(verify_only_front_artwork(&original, &staged).is_err());
        assert_eq!(fs::read(&original).unwrap(), source);
    }
}

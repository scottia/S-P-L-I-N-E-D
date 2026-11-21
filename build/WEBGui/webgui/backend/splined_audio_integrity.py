"""Fail-closed verification that an audio write changes only front artwork.

Never authorize ID3 version conversion, MusicBrainz edits, or text-frame
normalization. Compare original and staged files *before* atomic publication.
"""
from __future__ import annotations

from pathlib import Path
import hashlib

from mutagen.mp4 import MP4


class AudioMetadataChanged(ValueError):
    """The staged audio file is not an artwork-only transformation."""


def _synchsafe(data: bytes) -> int:
    if len(data) != 4 or any(x & 0x80 for x in data):
        raise AudioMetadataChanged("Invalid ID3 synchsafe size")
    return (data[0] << 21) | (data[1] << 14) | (data[2] << 7) | data[3]


def _id3_snapshot(path: Path) -> tuple:
    raw = path.read_bytes()
    if not raw.startswith(b"ID3"):
        return (None, (), raw)
    if len(raw) < 10 or raw[3] not in (3, 4) or raw[4] != 0 or raw[5] != 0:
        raise AudioMetadataChanged("Unverifiable ID3 version/flags; original preserved")
    tag_end = 10 + _synchsafe(raw[6:10])
    if tag_end > len(raw):
        raise AudioMetadataChanged("ID3 tag extends past the file")
    pos, unchanged_frames = 10, []
    while pos + 10 <= tag_end:
        header = raw[pos:pos + 10]
        if header == b"\0" * 10:
            if any(raw[pos:tag_end]):
                raise AudioMetadataChanged("Invalid ID3 padding")
            break
        fid = header[:4]
        if not all(48 <= x <= 57 or 65 <= x <= 90 for x in fid):
            raise AudioMetadataChanged("Unsupported ID3 frame identifier")
        size = _synchsafe(header[4:8]) if raw[3] == 4 else int.from_bytes(header[4:8], "big")
        end = pos + 10 + size
        if end > tag_end:
            raise AudioMetadataChanged("Invalid ID3 frame boundary")
        payload = raw[pos + 10:end]
        front = False
        if fid == b"APIC":
            if header[8:10] != b"\0\0":
                raise AudioMetadataChanged("Unverifiable flagged ID3 APIC frame")
            delimiter = payload.find(b"\0", 1)
            if delimiter < 0 or delimiter + 1 >= len(payload):
                raise AudioMetadataChanged("Invalid ID3 APIC picture header")
            front = payload[delimiter + 1] == 3
        if not front:
            unchanged_frames.append(raw[pos:end])
        pos = end
    if pos != tag_end and any(raw[pos:tag_end]):
        raise AudioMetadataChanged("Incomplete ID3 frame header")
    return (raw[3:6], tuple(unchanged_frames), raw[tag_end:])


def _flac_snapshot(path: Path) -> tuple:
    raw = path.read_bytes()
    if not raw.startswith(b"fLaC"):
        raise AudioMetadataChanged("Unsupported FLAC metadata header")
    pos, unchanged = 4, []
    while True:
        if pos + 4 > len(raw):
            raise AudioMetadataChanged("Invalid FLAC metadata block")
        marker = raw[pos]
        kind = marker & 0x7F
        size = int.from_bytes(raw[pos + 1:pos + 4], "big")
        end = pos + 4 + size
        if end > len(raw):
            raise AudioMetadataChanged("Truncated FLAC metadata block")
        payload = raw[pos + 4:end]
        if kind != 1:  # Padding is not user metadata.
            front = kind == 6 and len(payload) >= 4 and int.from_bytes(payload[:4], "big") == 3
            if not front:
                unchanged.append((kind, payload))
        pos = end
        if marker & 0x80:
            break
    return tuple(unchanged), hashlib.sha256(raw[pos:]).digest()


def _mp4_snapshot(path: Path) -> tuple:
    track = MP4(path)
    tags = track.tags or {}
    unchanged = tuple(sorted(
        (str(key), tuple(bytes(v) if isinstance(v, (bytearray, memoryview)) else v for v in values))
        for key, values in tags.items() if key != "covr"
    ))
    raw = path.read_bytes()
    pos, audio = 0, []
    while pos < len(raw):
        if len(raw) - pos < 8:
            raise AudioMetadataChanged("Malformed MP4 top-level box")
        size = int.from_bytes(raw[pos:pos + 4], "big")
        kind = raw[pos + 4:pos + 8]
        header = 8
        if size == 1:
            if len(raw) - pos < 16:
                raise AudioMetadataChanged("Malformed MP4 64-bit box")
            size = int.from_bytes(raw[pos + 8:pos + 16], "big")
            header = 16
        elif size == 0:
            size = len(raw) - pos
        if size < header or pos + size > len(raw):
            raise AudioMetadataChanged("Invalid MP4 box boundaries")
        if kind == b"mdat":
            audio.append(hashlib.sha256(raw[pos + header:pos + size]).digest())
        pos += size
    if not audio:
        raise AudioMetadataChanged("MP4 audio data is missing")
    return unchanged, tuple(audio)


def assert_artwork_only(original: Path, staged: Path) -> None:
    """Reject any non-artwork audio modification before source replacement.

    Unsupported/unverifiable formats fail closed rather than silently
    permitting a writer to change another audio tag.
    """
    original, staged = Path(original), Path(staged)
    if original.suffix.lower() != staged.suffix.lower():
        raise AudioMetadataChanged("Audio container changed during embedding")
    ext = original.suffix.lower()
    snapshot = {".mp3": _id3_snapshot, ".flac": _flac_snapshot,
                ".m4a": _mp4_snapshot, ".mp4": _mp4_snapshot}.get(ext)
    if snapshot is None:
        raise AudioMetadataChanged(f"Artwork-only preservation not proven for {ext}; original preserved")
    before, after = snapshot(original), snapshot(staged)
    if ext == ".mp3" and before[0] is None:
        # A file without ID3 tags may acquire an artwork-only ID3 tag;
        # all pre-existing audio bytes must remain identical.
        allowed = after[0] is not None and after[1] == () and before[2] == after[2]
    else:
        allowed = before == after
    if not allowed:
        raise AudioMetadataChanged("Non-artwork metadata, ID3 version, or audio stream changed; original preserved")

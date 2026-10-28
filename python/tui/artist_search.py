"""Small, disposable remote Artist Art search for the Python TUI."""

from __future__ import annotations

from io import BytesIO
from pathlib import Path
from urllib.parse import urlparse
from uuid import UUID
import json
import re

from PIL import Image, ImageOps
import requests


def _same_artist(left: str, right: str) -> bool:
    return re.sub(r"[^\w]+", "", left.casefold()) == re.sub(r"[^\w]+", "", right.casefold())


def _allowed_image_url(url: str) -> bool:
    parsed = urlparse(url)
    host = (parsed.hostname or "").casefold()
    return parsed.scheme == "https" and parsed.username is None and parsed.password is None and any(
        host == domain or host.endswith("." + domain)
        for domain in ("fanart.tv", "dzcdn.net")
    )


def _image_png(response: requests.Response) -> bytes:
    response.raise_for_status()
    raw = response.content
    if len(raw) > 8 * 1024 * 1024:
        raise ValueError("Artist image exceeds the 8 MB preview limit")
    with Image.open(BytesIO(raw)) as source:
        if source.width * source.height > 24_000_000:
            raise ValueError("Artist image exceeds the preview pixel limit")
        image = ImageOps.exif_transpose(source)
        image.thumbnail((2400, 2400), Image.Resampling.LANCZOS)
        output = BytesIO()
        image.save(output, format="PNG")
        return output.getvalue()


def _fanart_references(artist_mbid: str, credential_dir: Path, http: requests.Session) -> list[tuple[str, str]]:
    path = credential_dir / "fanarttv.json"
    if not artist_mbid or not path.is_file():
        return []
    try:
        artist_mbid = str(UUID(artist_mbid))
    except ValueError:
        return []
    credential = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(credential, dict):
        raise ValueError("Fanart.tv credential file is invalid")
    api_key = str(credential.get("api_key", "") or "").strip()
    if not api_key:
        return []
    headers = {"api-key": api_key}
    client_key = str(credential.get("client_key", "") or "").strip()
    if client_key:
        headers["client-key"] = client_key
    response = http.get(
        f"https://webservice.fanart.tv/v3.2/music/{artist_mbid}",
        headers=headers, timeout=15,
    )
    if response.status_code == 404:
        return []
    response.raise_for_status()
    payload = response.json()
    if not isinstance(payload, dict):
        raise ValueError("Fanart.tv returned an invalid Artist Art response")
    result: list[tuple[str, str]] = []
    for key, label, limit in (("artistbackground", "Fanart banner", 3), ("artistthumb", "Fanart portrait", 6)):
        values = payload.get(key, [])
        if isinstance(values, list):
            result.extend((label, str(item.get("url", ""))) for item in values[:limit] if isinstance(item, dict))
    return result


def _deezer_references(artist: str, http: requests.Session) -> list[tuple[str, str]]:
    response = http.get(
        "https://api.deezer.com/search/artist",
        params={"q": artist, "limit": 15}, timeout=15,
    )
    response.raise_for_status()
    payload = response.json()
    values = payload.get("data", []) if isinstance(payload, dict) else []
    if not isinstance(values, list):
        return []
    return [
        ("Deezer portrait", str(item.get("picture_xl", "")))
        for item in values if isinstance(item, dict)
        and _same_artist(str(item.get("name", "")), artist)
        and item.get("picture_xl")
    ][:3]


def discover_artist_images(
    artist: str, artist_mbid: str, credential_dir: Path,
    *, http: requests.Session | None = None,
) -> tuple[list[tuple[str, bytes]], list[str]]:
    """Download a bounded set of verified images; caller owns their preview life."""
    session = http or requests.Session()
    session.headers.update({"User-Agent": "SPLINED/2.0.2 (https://github.com/scottia/S-P-L-I-N-E-D)"})
    references: list[tuple[str, str]] = []
    warnings: list[str] = []
    for source, search in (
        ("Fanart.tv", lambda: _fanart_references(artist_mbid, credential_dir, session)),
        ("Deezer", lambda: _deezer_references(artist, session)),
    ):
        try:
            references.extend(search())
        except (OSError, ValueError, requests.RequestException) as error:
            warnings.append(f"{source}: {error}")
    candidates: list[tuple[str, bytes]] = []
    seen: set[str] = set()
    for label, url in references:
        if len(candidates) >= 10:
            break
        if not _allowed_image_url(url) or url in seen:
            continue
        seen.add(url)
        try:
            response = session.get(url, timeout=20)
            candidates.append((label, _image_png(response)))
        except (OSError, ValueError, requests.RequestException) as error:
            warnings.append(f"{label}: {error}")
    return candidates, warnings

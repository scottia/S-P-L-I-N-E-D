"""Small, disposable remote Artist Art search for the Python backend."""

from __future__ import annotations

from io import BytesIO
from html import unescape
from pathlib import Path
from urllib.parse import urlparse
from uuid import UUID
import json
import re

from PIL import Image, ImageOps
import requests
import splined as core


MUSICBRAINZ_API = "https://musicbrainz.org/ws/2"
LASTFM_API = "https://ws.audioscrobbler.com/2.0/"
DISCOGS_API = "https://api.discogs.com/database/search"
ITUNES_API = "https://itunes.apple.com/search"


def _same_artist(left: str, right: str) -> bool:
    return re.sub(r"[^\w]+", "", left.casefold()) == re.sub(r"[^\w]+", "", right.casefold())


def _plain_text(value: object) -> str:
    return " ".join(unescape(re.sub(r"<[^>]+>", " ", str(value or ""))).split())


def _musicbrainz_artist_match(
    artist: str, artist_mbid: str, http: requests.Session
) -> tuple[str, str]:
    try:
        return artist, str(UUID(artist_mbid.strip()))
    except (ValueError, AttributeError):
        pass
    response = http.get(
        f"{MUSICBRAINZ_API}/artist",
        params={"query": f'artist:"{artist}"', "fmt": "json", "limit": "10"},
        timeout=15,
    )
    response.raise_for_status()
    payload = response.json()
    values = payload.get("artists", []) if isinstance(payload, dict) else []
    if not isinstance(values, list):
        return artist, ""
    exact = [
        value
        for value in values
        if isinstance(value, dict)
        and _same_artist(str(value.get("name", "")), artist)
        and str(value.get("id", ""))
    ]
    choices = exact or [
        value
        for value in values
        if isinstance(value, dict)
        and int(value.get("score", 0) or 0) >= 95
        and str(value.get("id", ""))
    ]
    if not choices:
        return artist, ""
    selected = max(choices, key=lambda value: int(value.get("score", 0) or 0))
    try:
        matched_id = str(UUID(str(selected["id"])))
    except (ValueError, KeyError):
        return artist, ""
    return str(selected.get("name") or artist), matched_id


def discover_artist_profile(
    artist: str,
    artist_mbid: str,
    credential_dir: Path,
    *,
    http: requests.Session | None = None,
) -> dict[str, str] | None:
    """Resolve MusicBrainz identity, then obtain biography text from Last.fm."""
    path = credential_dir / "lastfm.json"
    if not path.is_file():
        return None
    credential = json.loads(path.read_text(encoding="utf-8"))
    api_key = str(credential.get("api_key", "") if isinstance(credential, dict) else "").strip()
    if not api_key:
        return None
    session = http or requests.Session()
    session.headers.update({"User-Agent": core.USER_AGENT})
    matched_name, matched_id = _musicbrainz_artist_match(artist.strip(), artist_mbid, session)
    parameters = {
        "method": "artist.getinfo",
        "api_key": api_key,
        "format": "json",
        "autocorrect": "1",
    }
    parameters["mbid" if matched_id else "artist"] = matched_id or matched_name
    response = session.get(LASTFM_API, params=parameters, timeout=15)
    response.raise_for_status()
    payload = response.json()
    found = payload.get("artist") if isinstance(payload, dict) else None
    if not isinstance(found, dict):
        return None
    bio = found.get("bio") if isinstance(found.get("bio"), dict) else {}
    biography = _plain_text(bio.get("content")) or _plain_text(bio.get("summary"))
    if not biography:
        return None
    return {
        "lookup_name": str(found.get("name") or matched_name),
        "musicbrainz_id": matched_id,
        "biography": biography,
        "external_url": str(found.get("url") or ""),
        "source": "MusicBrainz + Last.fm",
    }


def _allowed_image_url(url: str) -> bool:
    parsed = urlparse(url)
    host = (parsed.hostname or "").casefold()
    return parsed.scheme == "https" and parsed.username is None and parsed.password is None and any(
        host == domain or host.endswith("." + domain)
        for domain in (
            "fanart.tv",
            "dzcdn.net",
            "lastfm.freetls.fastly.net",
            "lastfm-img2.akamaized.net",
            "i.discogs.com",
            "img.discogs.com",
            "discogs.com",
            "mzstatic.com",
        )
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
    # Keep the browser and native clients on the same bounded Fanart result
    # contract. Windows reserves two backgrounds and retains up to ten
    # portraits; the former WebGUI limit of three portraits made identical
    # provider selections return fewer usable Artist Art candidates.
    for key, label, limit in (("artistbackground", "Fanart banner", 2), ("artistthumb", "Fanart portrait", 10)):
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
    references: list[tuple[str, str]] = []
    seen_ids: set[int] = set()
    for item in values:
        if not isinstance(item, dict) or not _same_artist(str(item.get("name", "")), artist):
            continue
        try:
            artist_id = int(item.get("id") or 0)
        except (TypeError, ValueError):
            continue
        url = str(item.get("picture_xl") or "").strip()
        if artist_id <= 0 or artist_id in seen_ids or not url.startswith("https://"):
            continue
        seen_ids.add(artist_id)
        references.append(("Deezer portrait", url))
        if len(references) == 5:
            break
    return references


def _lastfm_references(
    artist: str, artist_mbid: str, credential_dir: Path, http: requests.Session
) -> list[tuple[str, str]]:
    path = credential_dir / "lastfm.json"
    if not path.is_file():
        return []
    credential = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(credential, dict):
        raise ValueError("Last.fm credential file is invalid")
    api_key = str(credential.get("api_key") or "").strip()
    if not api_key:
        return []
    parameters = {
        "method": "artist.getinfo",
        "api_key": api_key,
        "format": "json",
        "autocorrect": "1",
    }
    # Windows uses the selected Artist identity here. Using an MBID only in the
    # browser could select a different Last.fm response and made identical
    # source policies yield different candidate sets.
    parameters["artist"] = artist
    response = http.get(LASTFM_API, params=parameters, timeout=15)
    response.raise_for_status()
    payload = response.json()
    found = payload.get("artist") if isinstance(payload, dict) else None
    if not isinstance(found, dict) or not _same_artist(str(found.get("name") or artist), artist):
        return []
    images = found.get("image") or []
    if isinstance(images, dict):
        images = [images]
    values: list[str] = []
    seen: set[str] = set()
    for item in images:
        if not isinstance(item, dict):
            continue
        url = str(item.get("#text") or "").strip()
        if not url or "2a96cbd8b46e442fc41c2b86b821562f" in url or url in seen:
            continue
        seen.add(url)
        values.append(url)
        if len(values) == 8:
            break
    return [("Last.fm portrait", url) for url in values]


def _discogs_references(
    artist: str, credential_dir: Path, http: requests.Session
) -> list[tuple[str, str]]:
    path = credential_dir / "discogs.json"
    if not path.is_file():
        return []
    credential = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(credential, dict):
        raise ValueError("Discogs credential file is invalid")
    token = str(credential.get("token") or "").strip()
    if not token:
        return []
    response = http.get(
        DISCOGS_API,
        params={"q": artist, "type": "artist", "per_page": "15"},
        headers={"Authorization": f"Discogs token={token}"},
        timeout=15,
    )
    response.raise_for_status()
    payload = response.json()
    values = payload.get("results", []) if isinstance(payload, dict) else []
    if not isinstance(values, list):
        return []
    references: list[tuple[str, str]] = []
    seen_ids: set[int] = set()
    exact = [
        item for item in values
        if isinstance(item, dict)
        and _same_artist(str(item.get("title") or ""), artist)
        and int(item.get("id") or 0) > 0
    ][:5]
    for item in exact:
        try:
            artist_id = int(item.get("id") or 0)
        except (TypeError, ValueError):
            continue
        if artist_id <= 0 or artist_id in seen_ids:
            continue
        seen_ids.add(artist_id)
        detail = http.get(
            f"https://api.discogs.com/artists/{artist_id}",
            headers={"Authorization": f"Discogs token={token}"},
            timeout=15,
        )
        if detail.status_code != 200:
            continue
        found = detail.json()
        if not isinstance(found, dict) or not _same_artist(str(found.get("name") or ""), artist):
            continue
        images = found.get("images") or []
        if not isinstance(images, list):
            continue
        images = sorted(
            (image for image in images if isinstance(image, dict)),
            key=lambda image: str(image.get("type") or "").casefold() != "primary",
        )
        for image in images[:4]:
            url = str(image.get("uri") or image.get("resource_url") or "").strip()
            if url and not url.casefold().endswith("/images/spacer.gif"):
                references.append(("Discogs portrait", url))
    return references


def _itunes_artist_artwork_url(html: str) -> str:
    match = re.search(
        r'<meta\s+[^>]*property=["\']og:image["\'][^>]*content=["\']([^"\']+)',
        html,
        re.IGNORECASE,
    )
    if not match:
        return ""
    parsed = urlparse(unescape(match.group(1)))
    if (
        parsed.scheme != "https"
        or not (parsed.hostname or "").casefold().endswith(".mzstatic.com")
        or not parsed.path.startswith("/image/thumb/")
    ):
        return ""
    prefix, separator, sized = parsed.path.rpartition("/")
    if not separator or "." not in sized:
        return ""
    extension = sized.rsplit(".", 1)[1].casefold()
    if extension not in {"jpg", "jpeg", "png", "webp"}:
        return ""
    return parsed._replace(path=f"{prefix}/1200x1200bb.{extension}").geturl()


def _itunes_references(artist: str, http: requests.Session) -> list[tuple[str, str]]:
    response = http.get(
        ITUNES_API,
        params={
            "term": artist,
            "media": "music",
            "entity": "musicArtist",
            "country": "us",
            "limit": "10",
        },
        timeout=15,
    )
    response.raise_for_status()
    payload = response.json()
    values = payload.get("results", []) if isinstance(payload, dict) else []
    references: list[tuple[str, str]] = []
    for item in values if isinstance(values, list) else []:
        if (
            not isinstance(item, dict)
            or not _same_artist(str(item.get("artistName") or ""), artist)
        ):
            continue
        profile = str(item.get("artistLinkUrl") or "")
        parsed = urlparse(profile)
        if parsed.scheme != "https" or parsed.hostname != "music.apple.com":
            continue
        page = http.get(profile, timeout=15)
        page.raise_for_status()
        artwork = _itunes_artist_artwork_url(page.text)
        if artwork:
            references.append(("iTunes portrait", artwork))
        if len(references) >= 3:
            break
    return references


def discover_artist_images(
    artist: str, artist_mbid: str, credential_dir: Path,
    *, http: requests.Session | None = None,
    enabled=("fanarttv", "deezer", "lastfm", "discogs", "itunes"),
) -> tuple[list[tuple[str, bytes]], list[str]]:
    """Download provider-bounded verified images; caller owns their preview life."""
    session = http or requests.Session()
    session.headers.update({"User-Agent": core.USER_AGENT})
    references: list[tuple[str, str]] = []
    warnings: list[str] = []
    for source, search in (
        ("Fanart.tv", lambda: _fanart_references(artist_mbid, credential_dir, session)),
        ("Deezer", lambda: _deezer_references(artist, session)),
        ("Last.fm", lambda: _lastfm_references(artist, artist_mbid, credential_dir, session)),
        ("Discogs", lambda: _discogs_references(artist, credential_dir, session)),
        ("iTunes", lambda: _itunes_references(artist, session)),
    ):
        source_key = {"Fanart.tv": "fanarttv", "Deezer": "deezer", "Last.fm": "lastfm", "Discogs": "discogs", "iTunes": "itunes"}[source]
        if source_key not in enabled:
            continue
        try:
            references.extend(search())
        except (OSError, ValueError, requests.RequestException) as error:
            warnings.append(f"{source}: request failed ({type(error).__name__})")
    candidates: list[tuple[str, bytes]] = []
    seen: set[str] = set()
    for label, url in references:
        if not _allowed_image_url(url) or url in seen:
            continue
        seen.add(url)
        try:
            response = session.get(url, timeout=20)
            candidates.append((label, _image_png(response)))
        except (OSError, ValueError, requests.RequestException) as error:
            warnings.append(f"{label}: image request failed ({type(error).__name__})")
    return candidates, warnings

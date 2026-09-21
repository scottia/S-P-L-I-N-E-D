from __future__ import annotations

import random
from pathlib import Path
from typing import Any

import requests

import splined as core


# Curated public records used only to prove provider connectivity. One target is
# chosen at random per configured provider so validation does not depend on the
# user's library or require interactive artist/album input.
DISCOGS_TARGETS = (
    ("B.B. King", "Live at the Regal"),
    ("Etta James", "At Last!"),
    ("Nirvana", "Nevermind"),
)

LASTFM_TARGETS = (
    ("B.B. King", "Live at the Regal"),
    ("Etta James", "At Last!"),
    ("Nirvana", "Nevermind"),
)

MUSICBRAINZ_TARGETS = (
    ("B.B. King", "Live at the Regal", "93852aca-b6d4-3320-8eaa-4e9c70d0302f"),
    ("Nirvana", "Nevermind", "201d5e28-2de4-4e6f-830a-d55c98e57641"),
    ("Etta James", "At Last!", "c0f1351c-b6c8-4c91-9a20-28d6c360eb08"),
)

FANARTTV_TARGETS = (
    ("B.B. King", "Live at the Regal", "a4d2a86c-bbd6-352b-b9fa-f9da86df842c"),
    ("Nirvana", "Nevermind", "1b022e01-4da6-387b-8658-8678046e4cef"),
)


def _randomized(values: tuple[Any, ...]) -> list[Any]:
    items = list(values)
    random.SystemRandom().shuffle(items)
    return items


def _read_credential(path: Path, label: str) -> tuple[str, dict[str, Any] | None]:
    if not path.exists():
        print(f"SKIP: {label}: credential file not found: {path}")
        return "skip", None
    try:
        return "configured", core.load_json(path, label)
    except core.SplinedError as exc:
        print(f"FAIL: {label}: {exc}")
        return "fail", None


def _request_json(method: str, url: str, **kwargs: Any) -> tuple[requests.Response, dict[str, Any]]:
    headers = dict(kwargs.pop("headers", {}) or {})
    headers.setdefault("User-Agent", core.USER_AGENT)
    response = requests.request(method, url, headers=headers, timeout=core.REQUEST_TIMEOUT, **kwargs)
    try:
        data = response.json()
    except ValueError:
        data = {}
    return response, data if isinstance(data, dict) else {}


def _validate_discogs(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "Discogs"
    path = core.credential_file(config_file, cfg, "discogs")
    state, cred = _read_credential(path, label)
    if state != "configured":
        return state
    token = str((cred or {}).get("token") or "").strip()
    if not token:
        print(f"FAIL: {label}: configured credential contains no token")
        return "fail"

    for artist, album in _randomized(DISCOGS_TARGETS):
        print(f"Random test: {label}: {artist} - {album}")
        try:
            response, data = _request_json(
                "GET",
                "https://api.discogs.com/database/search",
                headers={"Authorization": f"Discogs token={token}"},
                params={
                    "artist": artist,
                    "release_title": album,
                    "type": "release",
                    "per_page": 1,
                },
            )
        except requests.RequestException as exc:
            print(f"FAIL: {label}: network error: {exc}")
            return "fail"
        if response.status_code in {401, 403}:
            print(f"FAIL: {label}: authentication rejected (HTTP {response.status_code})")
            return "fail"
        if response.status_code != 200:
            print(f"FAIL: {label}: provider returned HTTP {response.status_code}")
            return "fail"
        results = data.get("results")
        if isinstance(results, list) and results:
            result = results[0] if isinstance(results[0], dict) else {}
            print(
                "PASS: Discogs personal token accepted; "
                f"result={result.get('title') or 'release'}; "
                f"id={result.get('id') or 'n/a'}; "
                f"year={result.get('year') or 'n/a'}; "
                f"cover={result.get('cover_image') or 'n/a'}"
            )
            return "pass"
        print("NO RESULT: Discogs authentication succeeded but this random target returned no release.")
    print("FAIL: Discogs authentication succeeded, but no curated random target returned a result.")
    return "fail"


def _validate_fanarttv(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "Fanart.tv"
    path = core.credential_file(config_file, cfg, "fanarttv")
    state, cred = _read_credential(path, label)
    if state != "configured":
        return state
    cred = cred or {}
    api_key = str(cred.get("api_key") or "").strip()
    client_key = str(cred.get("client_key") or "").strip()
    api_version = str(cred.get("api_version") or "").strip()
    if not api_key:
        print(f"FAIL: {label}: configured credential contains no api_key")
        return "fail"
    if api_version and api_version != "v3.2":
        print(f"FAIL: {label}: credential api_version must be v3.2")
        return "fail"

    headers = {"api-key": api_key}
    if client_key:
        headers["client-key"] = client_key

    for artist, album, release_group_mbid in _randomized(FANARTTV_TARGETS):
        print(
            f"Random test: {label}: {artist} - {album} "
            f"(release-group {release_group_mbid})"
        )
        try:
            response, data = _request_json(
                "GET",
                f"https://webservice.fanart.tv/v3.2/music/albums/{release_group_mbid}",
                headers=headers,
            )
        except requests.RequestException as exc:
            print(f"FAIL: {label}: network error: {exc}")
            return "fail"
        if response.status_code in {401, 403}:
            print(f"FAIL: {label}: authentication rejected (HTTP {response.status_code})")
            return "fail"
        if response.status_code not in {200, 404}:
            print(f"FAIL: {label}: provider returned HTTP {response.status_code}")
            return "fail"
        covers = data.get("albumcover")
        if isinstance(covers, list) and covers:
            cover = covers[0] if isinstance(covers[0], dict) else {}
            print(
                "PASS: Fanart.tv credentials accepted; "
                f"id={cover.get('id') or 'n/a'}; "
                f"artwork={cover.get('url') or 'n/a'}"
            )
            return "pass"
        if response.status_code == 200 and data and "error" not in data:
            print("PASS: Fanart.tv credentials accepted; album record returned without albumcover artwork.")
            return "pass"
        print("NO RESULT: Fanart.tv accepted the request but this random release-group was not available.")
    print("FAIL: Fanart.tv credentials were not rejected, but no curated random target returned album data.")
    return "fail"


def _musicbrainz_artist(data: dict[str, Any]) -> str:
    parts: list[str] = []
    for item in data.get("artist-credit", []) or []:
        if isinstance(item, str):
            parts.append(item)
        elif isinstance(item, dict):
            parts.append(str(item.get("name") or (item.get("artist") or {}).get("name") or ""))
            parts.append(str(item.get("joinphrase") or ""))
    return "".join(parts).strip()


def _validate_musicbrainz(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "MusicBrainz"
    path = core.credential_file(config_file, cfg, "musicbrainz")
    state, cred = _read_credential(path, label)
    if state != "configured":
        return state
    cred = cred or {}
    token = str(cred.get("access_token") or "").strip()
    if not token:
        print(f"FAIL: {label}: configured credential contains no access_token")
        return "fail"

    try:
        response, identity = _request_json(
            "GET",
            "https://musicbrainz.org/oauth2/userinfo",
            headers={"Authorization": f"Bearer {token}"},
        )
    except requests.RequestException as exc:
        print(f"FAIL: {label}: OAuth identity network error: {exc}")
        return "fail"
    if response.status_code in {401, 403}:
        print(f"FAIL: {label}: saved OAuth access token rejected (HTTP {response.status_code})")
        return "fail"
    if response.status_code != 200:
        print(f"FAIL: {label}: OAuth identity returned HTTP {response.status_code}")
        return "fail"
    identity_name = identity.get("username") or identity.get("sub") or identity.get("name") or "authorized user"
    print(f"PASS: MusicBrainz saved OAuth access token accepted; identity={identity_name}")

    for artist, album, release_mbid in _randomized(MUSICBRAINZ_TARGETS):
        print(f"Random test: {label}: {artist} - {album} (release {release_mbid})")
        try:
            metadata_response, data = _request_json(
                "GET",
                f"{core.MB_BASE}/release/{release_mbid}",
                headers={"Authorization": f"Bearer {token}"},
                params={"fmt": "json", "inc": "artist-credits+release-groups"},
            )
        except requests.RequestException as exc:
            print(f"FAIL: {label}: metadata network error: {exc}")
            return "fail"
        if metadata_response.status_code in {401, 403}:
            print(f"FAIL: {label}: OAuth accepted by userinfo but rejected by metadata endpoint")
            return "fail"
        if metadata_response.status_code == 200 and data.get("id"):
            rg = data.get("release-group") if isinstance(data.get("release-group"), dict) else {}
            print(
                "PASS: MusicBrainz metadata lookup succeeded; "
                f"artist={_musicbrainz_artist(data) or artist}; "
                f"release={data.get('title') or album}; "
                f"release_id={data.get('id')}; "
                f"release_group_id={rg.get('id') or 'n/a'}"
            )
            return "pass"
        print(f"NO RESULT: MusicBrainz metadata target returned HTTP {metadata_response.status_code}.")
    print("FAIL: MusicBrainz OAuth succeeded, but no curated random metadata target resolved.")
    return "fail"


def _validate_lastfm(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "Last.fm"
    path = core.credential_file(config_file, cfg, "lastfm")
    state, cred = _read_credential(path, label)
    if state != "configured":
        return state
    api_key = str((cred or {}).get("api_key") or "").strip()
    if not api_key:
        print(f"FAIL: {label}: configured credential contains no api_key")
        return "fail"

    for artist, album in _randomized(LASTFM_TARGETS):
        print(f"Random test: {label}: {artist} - {album}")
        try:
            response, data = _request_json(
                "GET",
                core.LASTFM_API_URL,
                params={
                    "method": "album.getinfo",
                    "api_key": api_key,
                    "artist": artist,
                    "album": album,
                    "autocorrect": "1",
                    "format": "json",
                },
            )
        except requests.RequestException as exc:
            print(f"FAIL: {label}: network error: {exc}")
            return "fail"
        error = data.get("error")
        if error == 10 or response.status_code in {401, 403}:
            print(f"FAIL: {label}: API key rejected")
            return "fail"
        if error:
            if error in {6, 7}:
                print(f"NO RESULT: Last.fm accepted the API key but this random target was not found.")
                continue
            print(f"FAIL: {label}: API error {error}: {data.get('message') or 'unknown error'}")
            return "fail"
        album_data = data.get("album") if isinstance(data.get("album"), dict) else None
        if album_data:
            artwork = "n/a"
            images = album_data.get("image")
            if isinstance(images, list):
                urls = [
                    str(item.get("#text") or "").strip()
                    for item in images
                    if isinstance(item, dict) and str(item.get("#text") or "").strip()
                ]
                if urls:
                    artwork = urls[-1]
            print(
                "PASS: Last.fm API key accepted; "
                f"artist={album_data.get('artist') or artist}; "
                f"album={album_data.get('name') or album}; "
                f"mbid={album_data.get('mbid') or 'n/a'}; "
                f"artwork={artwork}"
            )
            return "pass"
        print("NO RESULT: Last.fm request succeeded but contained no album object.")
    print("FAIL: Last.fm API key was not rejected, but no curated random target returned an album.")
    return "fail"


def run_oauth_validation(config_file: Path, cfg: dict[str, Any]) -> int:
    credential_dir = core.runtime_credential_dir(config_file, cfg)
    print("SPLINED API/OAuth Validation")
    print()
    print(f"Config: {config_file}")
    print(f"Credential Directory: {credential_dir}")
    print("Targets: random curated public artist/album records; one representative result per configured provider")
    print("Secrets: never displayed")
    print()

    results: list[tuple[str, str]] = []
    validators = (
        ("Discogs", _validate_discogs),
        ("Fanart.tv", _validate_fanarttv),
        ("MusicBrainz", _validate_musicbrainz),
        ("Last.fm", _validate_lastfm),
    )
    for label, validator in validators:
        print(f"=== {label} ===")
        result = validator(config_file, cfg)
        results.append((label, result))
        print()

    print("=== API/OAuth Validation Summary ===")
    for label, result in results:
        print(f"{label}: {result.upper()}")

    configured = [result for _, result in results if result != "skip"]
    failures = [result for result in configured if result != "pass"]
    if not configured:
        print("No configured provider credential files were found.")
        return 0
    if failures:
        print("RESULT: FAIL - one or more configured providers did not complete validation.")
        return 1
    print("RESULT: PASS - all configured provider credentials validated successfully.")
    return 0

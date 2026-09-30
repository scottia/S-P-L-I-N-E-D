from __future__ import annotations

import random
from pathlib import Path
from typing import Any

import requests

import splined as core


STATUS_OK = "PASS"
# Public compatibility constant used by the validation regression harness.
PASS = STATUS_OK  # nosec B105
FAIL = "FAIL"
SKIP = "SKIP"


# These are stable public catalog records. The release identifiers were checked
# against MusicBrainz; Fanart.tv receives release-group identifiers, never
# release identifiers.
DISCOGS_TARGETS = (
    ("B.B. King", "Live at the Regal"),
    ("Etta James", "At Last!"),
    ("Nirvana", "Nevermind"),
)

FANARTTV_TARGETS = (
    ("B.B. King", "Live at the Regal", "a4d2a86c-bbd6-352b-b9fa-f9da86df842c"),
    ("Nirvana", "Nevermind", "1b022e01-4da6-387b-8658-8678046e4cef"),
)

MUSICBRAINZ_TARGETS = (
    ("B.B. King", "Live at the Regal", "93852aca-b6d4-3320-8eaa-4e9c70d0302f"),
    ("Nirvana", "Nevermind", "201d5e28-2de4-4e6f-830a-d55c98e57641"),
    ("Etta James", "At Last!", "c0f1351c-b6c8-4c91-9a20-28d6c360eb08"),
)

LASTFM_TARGETS = (
    ("B.B. King", "Live at the Regal"),
    ("Etta James", "At Last!"),
    ("Nirvana", "Nevermind"),
)


class ProviderRequestError(RuntimeError):
    """A deliberately secret-free provider transport/response failure."""


def _randomized(values: tuple[Any, ...]) -> list[Any]:
    items = list(values)
    random.SystemRandom().shuffle(items)
    return items


def _print_recovery(
    config_file: Path,
    cfg: dict[str, Any],
    provider: str,
    label: str,
    *,
    connectivity_first: bool = False,
) -> None:
    path = core.credential_file(config_file, cfg, provider)
    print("Next steps:")
    if connectivity_first:
        print("  1. Check network connectivity and provider availability.")
        print("  2. Run: splined --oauth-validation")
        print(f"  3. If only {label} continues to fail, repair its credential as follows:")
        indent = "     "
    else:
        indent = "  "

    if provider == "discogs":
        print(f"{indent}Replace or repair {path.name} with a valid Discogs personal access token.")
        print(f"{indent}Credential file: {path}")
        print(f"{indent}Then run: splined --oauth-validation")
        return

    if provider == "fanarttv":
        print(f"{indent}Run: splined --fanarttv-credentials")
        print(f"{indent}Enter a valid Fanart.tv API key; the client key is optional.")
        print(f"{indent}Then run: splined --oauth-validation")
        return

    if provider == "musicbrainz":
        print(f"{indent}Run: splined --mb-oauth-login")
        print(f"{indent}Complete MusicBrainz browser authorization.")
        print(f"{indent}Then run: splined --oauth-validation")
        print("Resolution check:")
        print("  Future splined --oauth-validation runs automatically refresh an expired")
        print("  MusicBrainz access token and advance expires_at_unix before testing it.")
        print("  If expires_at_unix does not advance or validation still fails, the saved")
        print("  refresh grant is no longer usable; run --mb-oauth-login again.")
        return

    if provider == "lastfm":
        print(f"{indent}Run: splined --lastfm-credentials")
        print(f"{indent}Enter a valid Last.fm API key and shared secret.")
        print(f"{indent}Then run: splined --oauth-validation")
        print(f"{indent}If user-account authorization is also needed, run: splined --lastfm-login")


def _read_credential(
    config_file: Path,
    cfg: dict[str, Any],
    provider: str,
    label: str,
) -> tuple[str, dict[str, Any] | None]:
    path = core.credential_file(config_file, cfg, provider)
    if not path.is_file():
        print(f"{SKIP}: {label}: {path.name} is not configured")
        return SKIP, None
    try:
        return "CONFIGURED", core.load_json(path, label)
    except core.SplinedError as exc:
        print(f"{FAIL}: {label}: {exc}")
        _print_recovery(config_file, cfg, provider, label)
        return FAIL, None


def _request_json(
    method: str,
    url: str,
    **kwargs: Any,
) -> tuple[int, dict[str, Any]]:
    headers = dict(kwargs.pop("headers", {}) or {})
    headers.setdefault("User-Agent", core.USER_AGENT)
    try:
        response = requests.request(
            method,
            url,
            headers=headers,
            timeout=core.REQUEST_TIMEOUT,
            **kwargs,
        )
    except requests.RequestException as exc:
        # Exception messages may contain prepared URLs or headers. Keep the
        # diagnostic useful without risking credential disclosure.
        raise ProviderRequestError(
            f"request failed ({type(exc).__name__})"
        ) from None

    status = int(response.status_code)
    try:
        try:
            payload = response.json()
        except ValueError:
            raise ProviderRequestError(
                f"provider returned invalid JSON (HTTP {status})"
            ) from None
    finally:
        response.close()

    if not isinstance(payload, dict):
        raise ProviderRequestError(
            f"provider returned an unexpected JSON value (HTTP {status})"
        )
    return status, payload


def _request_failed(
    config_file: Path,
    cfg: dict[str, Any],
    provider: str,
    label: str,
    exc: ProviderRequestError,
) -> str:
    print(f"{FAIL}: {label}: {exc}")
    _print_recovery(config_file, cfg, provider, label, connectivity_first=True)
    return FAIL


def _validate_discogs(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "Discogs"
    state, credential = _read_credential(config_file, cfg, "discogs", label)
    if state != "CONFIGURED":
        return state

    token = str((credential or {}).get("token") or "").strip()
    if not token:
        print(f"{FAIL}: {label}: configured credential contains no token")
        _print_recovery(config_file, cfg, "discogs", label)
        return FAIL

    for artist, album in _randomized(DISCOGS_TARGETS):
        print(f"Random test: {label}: {artist} - {album}")
        try:
            status, data = _request_json(
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
        except ProviderRequestError as exc:
            return _request_failed(config_file, cfg, "discogs", label, exc)

        if status in {401, 403}:
            print(f"{FAIL}: {label}: personal access token was rejected")
            _print_recovery(config_file, cfg, "discogs", label)
            return FAIL
        if status != 200:
            print(f"{FAIL}: {label}: provider returned HTTP {status}")
            _print_recovery(config_file, cfg, "discogs", label, connectivity_first=True)
            return FAIL

        results = data.get("results")
        if isinstance(results, list) and results:
            result = results[0] if isinstance(results[0], dict) else {}
            print(
                f"{STATUS_OK}: Discogs personal token accepted; "
                f"result={result.get('title') or 'release'}; "
                f"id={result.get('id') or 'n/a'}; "
                f"year={result.get('year') or 'n/a'}"
            )
            return STATUS_OK

    print(
        f"{STATUS_OK}: Discogs personal token accepted; "
        "the curated records returned no representative search result"
    )
    return STATUS_OK


def _fanart_cover(
    data: dict[str, Any],
    release_group_mbid: str,
) -> dict[str, Any] | None:
    albums = data.get("albums")
    if not isinstance(albums, list):
        return None
    for item in albums:
        if not isinstance(item, dict):
            continue
        if str(item.get("release_group_id") or "").lower() != release_group_mbid.lower():
            continue
        covers = item.get("albumcover")
        if not isinstance(covers, list):
            return None
        for cover in covers:
            if isinstance(cover, dict) and str(cover.get("url") or "").strip():
                return cover
    return None


def _validate_fanarttv(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "Fanart.tv"
    state, credential = _read_credential(config_file, cfg, "fanarttv", label)
    if state != "CONFIGURED":
        return state

    credential = credential or {}
    api_key = str(credential.get("api_key") or "").strip()
    client_key = str(credential.get("client_key") or "").strip()
    api_version = str(credential.get("api_version") or "").strip()
    if not api_key:
        print(f"{FAIL}: {label}: configured credential contains no api_key")
        _print_recovery(config_file, cfg, "fanarttv", label)
        return FAIL
    if api_version.lower() != core.FANARTTV_API_VERSION:
        print(
            f"WARN: {label}: saved api_version marker is missing or not v3.2; "
            "testing the saved credentials against the live v3.2 endpoint"
        )

    headers = {"api-key": api_key}
    if client_key:
        headers["client-key"] = client_key

    for artist, album, release_group_mbid in _randomized(FANARTTV_TARGETS):
        print(
            f"Random test: {label}: {artist} - {album} "
            f"(release-group {release_group_mbid})"
        )
        try:
            status, data = _request_json(
                "GET",
                f"{core.FANARTTV_API_BASE}/music/albums/{release_group_mbid}",
                headers=headers,
            )
        except ProviderRequestError as exc:
            return _request_failed(config_file, cfg, "fanarttv", label, exc)

        if status in {401, 403}:
            print(f"{FAIL}: {label}: project/client key combination was rejected")
            _print_recovery(config_file, cfg, "fanarttv", label)
            return FAIL
        if status == 404:
            continue
        if status != 200:
            print(f"{FAIL}: {label}: provider returned HTTP {status}")
            _print_recovery(config_file, cfg, "fanarttv", label, connectivity_first=True)
            return FAIL

        albums = data.get("albums")
        if not isinstance(albums, list):
            print(f"{FAIL}: {label}: v3.2 response did not contain an albums array")
            _print_recovery(config_file, cfg, "fanarttv", label, connectivity_first=True)
            return FAIL
        cover = _fanart_cover(data, release_group_mbid)
        if cover is not None:
            print(
                f"{STATUS_OK}: Fanart.tv v3.2 credentials accepted; "
                f"release_group_mbid={release_group_mbid}; "
                f"artwork_id={cover.get('id') or 'n/a'}; "
                f"artwork={cover.get('url')}"
            )
            return STATUS_OK

    print(
        f"{STATUS_OK}: Fanart.tv v3.2 credentials accepted; "
        "the curated release groups returned no album-cover result"
    )
    return STATUS_OK


def _musicbrainz_artist(data: dict[str, Any]) -> str:
    parts: list[str] = []
    artist_credit = data.get("artist-credit")
    if not isinstance(artist_credit, list):
        return ""
    for item in artist_credit:
        if isinstance(item, str):
            parts.append(item)
        elif isinstance(item, dict):
            artist = item.get("artist")
            artist_name = artist.get("name") if isinstance(artist, dict) else ""
            parts.append(str(item.get("name") or artist_name or ""))
            parts.append(str(item.get("joinphrase") or ""))
    return "".join(parts).strip()


def _validate_musicbrainz(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "MusicBrainz"
    state, _credential = _read_credential(config_file, cfg, "musicbrainz", label)
    if state != "CONFIGURED":
        return state

    try:
        headers, _mode = core.mb_headers(config_file, cfg)
    except core.SplinedError as exc:
        print(f"{FAIL}: {label}: automatic OAuth refresh failed: {exc}")
        _print_recovery(config_file, cfg, "musicbrainz", label)
        return FAIL

    try:
        status, _identity = _request_json(
            "GET",
            "https://musicbrainz.org/oauth2/userinfo",
            headers=headers,
        )
    except ProviderRequestError as exc:
        return _request_failed(config_file, cfg, "musicbrainz", label, exc)
    if status in {401, 403}:
        try:
            headers, _mode = core.mb_headers(config_file, cfg, force_refresh=True)
            status, _identity = _request_json(
                "GET",
                "https://musicbrainz.org/oauth2/userinfo",
                headers=headers,
            )
        except core.SplinedError as exc:
            print(f"{FAIL}: {label}: OAuth refresh after token rejection failed: {exc}")
            _print_recovery(config_file, cfg, "musicbrainz", label)
            return FAIL
        except ProviderRequestError as exc:
            return _request_failed(config_file, cfg, "musicbrainz", label, exc)
        if status in {401, 403}:
            print(f"{FAIL}: {label}: refreshed OAuth access token was rejected")
            _print_recovery(config_file, cfg, "musicbrainz", label)
            return FAIL
    if status != 200:
        print(f"{FAIL}: {label}: OAuth userinfo returned HTTP {status}")
        _print_recovery(config_file, cfg, "musicbrainz", label, connectivity_first=True)
        return FAIL
    print(f"{STATUS_OK}: MusicBrainz OAuth bearer token was accepted")

    for artist, album, release_mbid in _randomized(MUSICBRAINZ_TARGETS):
        print(f"Random test: {label}: {artist} - {album} (release {release_mbid})")
        try:
            metadata_status, data = _request_json(
                "GET",
                f"{core.MB_BASE}/release/{release_mbid}",
                headers=headers,
                params={"fmt": "json", "inc": "artist-credits+release-groups"},
            )
        except ProviderRequestError as exc:
            return _request_failed(config_file, cfg, "musicbrainz", label, exc)
        if metadata_status in {401, 403}:
            print(f"{FAIL}: {label}: metadata request rejected the saved OAuth token")
            _print_recovery(config_file, cfg, "musicbrainz", label)
            return FAIL
        if metadata_status == 404:
            continue
        if metadata_status != 200:
            print(f"{FAIL}: {label}: metadata endpoint returned HTTP {metadata_status}")
            _print_recovery(config_file, cfg, "musicbrainz", label, connectivity_first=True)
            return FAIL
        if data.get("id"):
            release_group = data.get("release-group")
            release_group = release_group if isinstance(release_group, dict) else {}
            print(
                f"{STATUS_OK}: MusicBrainz metadata lookup succeeded; "
                f"artist={_musicbrainz_artist(data) or artist}; "
                f"release={data.get('title') or album}; "
                f"release_mbid={data.get('id')}; "
                f"release_group_mbid={release_group.get('id') or 'n/a'}"
            )
            return STATUS_OK

    print(
        f"{STATUS_OK}: MusicBrainz OAuth bearer token was accepted; "
        "the curated releases returned no metadata result"
    )
    return STATUS_OK


def _validate_lastfm(config_file: Path, cfg: dict[str, Any]) -> str:
    label = "Last.fm"
    state, credential = _read_credential(config_file, cfg, "lastfm", label)
    if state != "CONFIGURED":
        return state

    api_key = str((credential or {}).get("api_key") or "").strip()
    if not api_key:
        print(f"{FAIL}: {label}: configured credential contains no api_key")
        _print_recovery(config_file, cfg, "lastfm", label)
        return FAIL

    for artist, album in _randomized(LASTFM_TARGETS):
        print(f"Random test: {label}: {artist} - {album}")
        try:
            status, data = _request_json(
                "GET",
                core.LASTFM_API_URL,
                params={
                    "method": "album.getInfo",
                    "api_key": api_key,
                    "artist": artist,
                    "album": album,
                    "autocorrect": "1",
                    "format": "json",
                },
            )
        except ProviderRequestError as exc:
            return _request_failed(config_file, cfg, "lastfm", label, exc)

        error = str(data.get("error") or "")
        if status in {401, 403} or error in {"10", "26"}:
            print(f"{FAIL}: {label}: API key was rejected")
            _print_recovery(config_file, cfg, "lastfm", label)
            return FAIL
        if status != 200:
            print(f"{FAIL}: {label}: provider returned HTTP {status}")
            _print_recovery(config_file, cfg, "lastfm", label, connectivity_first=True)
            return FAIL
        if error in {"6", "7"}:
            continue
        if error:
            print(f"{FAIL}: {label}: provider returned API error {error}")
            _print_recovery(config_file, cfg, "lastfm", label, connectivity_first=True)
            return FAIL

        album_data = data.get("album")
        if not isinstance(album_data, dict):
            continue
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
            f"{STATUS_OK}: Last.fm API key accepted; "
            f"artist={album_data.get('artist') or artist}; "
            f"album={album_data.get('name') or album}; "
            f"artwork={artwork}"
        )
        return STATUS_OK

    print(
        f"{STATUS_OK}: Last.fm API key accepted; "
        "the curated albums returned no representative result"
    )
    return STATUS_OK


def run_oauth_validation(config_file: Path, cfg: dict[str, Any]) -> int:
    credential_dir = core.runtime_credential_dir(config_file, cfg)
    print("SPLINED API/OAuth Validation")
    print(f"Config: {config_file}")
    print(f"Credential directory: {credential_dir}")
    print("Random curated public records are used; the music library is not inspected.")
    print("Saved credentials are tested and secrets are never displayed.")
    print("Expired or rejected MusicBrainz access tokens are refreshed before retesting.")
    print()

    validators = (
        ("Discogs", _validate_discogs),
        ("Fanart.tv", _validate_fanarttv),
        ("MusicBrainz", _validate_musicbrainz),
        ("Last.fm", _validate_lastfm),
    )
    results: list[tuple[str, str]] = []
    for label, validator in validators:
        result = validator(config_file, cfg)
        results.append((label, result))

    print()
    print("SPLINED API/OAuth Validation Summary")
    for label, result in results:
        print(f"{label:<14}{result}")

    return 1 if any(result == FAIL for _, result in results) else 0

# Credentials and Provider Setup

All supported S:P:L:I:N:E:D runtimes use Config v5 and the same
credential-directory contract. Python/Docker `config.toml` and Windows'
portable `data/config.toml` record stores only the directory:

```toml
[credentials]
credential_dir = "credentials"
```

Docker normally uses `/credentials`. Windows structured Setup requires the
operator to choose a credential directory before the first save. Existing
portable or packaged Config v5 values are loaded automatically and are not
replaced with a legacy path. Standard filenames are fixed beneath the selected
directory; `credential_file` and `token_file` are not Config v5 settings.

| Provider | File | Normal artwork access |
| --- | --- | --- |
| Fanart.tv | `fanarttv.json` | API key required |
| Last.fm | `lastfm.json` | API key required; account session optional |
| Discogs | `discogs.json` | Personal access token required |
| MusicBrainz / Cover Art Archive | `musicbrainz.json` | Anonymous release authority and artwork; OAuth optional |
| Navidrome / OpenSubsonic | `navidrome.json` | Server URL plus either a dedicated API key or username/password |
| iTunes / Apple | none | Anonymous |
| Cover Art Archive | none | Anonymous |
| Deezer | none | Anonymous |
| Amazon Store | none | Anonymous, opt-in best-effort search |

Credential JSON is sensitive. Never commit it, attach it to an issue, or copy secrets into `config.toml`.

## Filesystem protection

Credential writes are atomic. Windows applies a non-inherited ACL for the current user and Local System where supported; Unix-like runtimes request mode `0600`. Config and GUI-state files do not receive credential ACL rules.

If the selected filesystem cannot apply user-specific protection, SPLINED reports:

```text
Credential file created; user-specific filesystem ACL protection
is unavailable on this storage location.
```

SPLINED does not add proprietary encryption, require DPAPI, or store secrets in the Registry. Use BitLocker, EFS, encrypted NAS/cloud storage, or another suitable encrypted volume when encryption at rest is required.

## Fanart.tv v3.2

Fanart.tv uses the v3.2 album endpoint. The credential contract is:

```json
{
  "api_version": "v3.2",
  "api_key": "synthetic-api-key",
  "client_key": "synthetic-optional-client-key"
}
```

`api_version` must be `v3.2` for the canonical saved credential format. `client_key` is optional. The Windows credential editor always saves the v3.2 marker and validates against the v3.2 album endpoint.

Python/Docker `splined --oauth-validation` is intentionally tolerant of legacy or manually created Fanart.tv files whose `api_version` marker is missing or stale. It prints `WARN`, then tests the saved API key and optional client key against the live v3.2 release-group album endpoint. A missing or stale marker alone is not treated as an authentication failure; the live request determines credential `PASS` or `FAIL`. New or rewritten credential files should still use `"api_version": "v3.2"`.

Operational provider discovery also always uses the v3.2 release-group album
endpoint and its `albums` response shape. A credential-file marker never
downgrades the runtime request to v3.

## Last.fm

Normal `album.getInfo` artwork reads require only `api_key`. They do not require a username or an authorized session.

```json
{
  "api_key": "synthetic-api-key",
  "shared_secret": "synthetic-shared-secret",
  "username": "synthetic-user",
  "session_key": "synthetic-session-key",
  "subscriber": false
}
```

Account authorization is a separate workflow:

1. request a temporary token with `auth.getToken`;
2. open the Last.fm browser authorization URL;
3. exchange the authorized token with `auth.getSession`;
4. retain `username`, `session_key`, and `subscriber` without losing the API key or shared secret.

On Windows, save `lastfm.json`, choose **Authorize** in Credentials, approve the
browser request, and choose **Complete**. Linux/macOS and Python/container
commands expose `--lastfm-login` and perform the same API sequence.

## Discogs

Discogs uses a personal access token:

```json
{
  "token": "synthetic-personal-access-token"
}
```

All runtimes send:

```text
Authorization: Discogs token=<token>
```

The Windows saved-credential test validates the token with the Discogs API v2 `/oauth/identity` endpoint. This is not a Python-only contract and does not require a Discogs application OAuth exchange.

## MusicBrainz

MusicBrainz is release authority and does not host image bytes. Its Artwork
Source Priority entry resolves Cover Art Archive images from MusicBrainz
release identity; anonymous requests remain available when OAuth is disabled.
The standard `musicbrainz.json` can contain:

```json
{
  "oauth_enabled": true,
  "client_id": "synthetic-client-id",
  "client_secret": "synthetic-client-secret",
  "callback_uri": "urn:ietf:wg:oauth:2.0:oob",
  "oauth_scope": "profile",
  "access_token": "synthetic-access-token",
  "refresh_token": "synthetic-refresh-token",
  "token_type": "Bearer",
  "expires_at_unix": 2000000000,
  "scope": "profile",
  "options": {
    "retry_max": 4,
    "min_delay": 1.05,
    "recording_timeout": 7
  }
}
```

The runtime supports OAuth2 authorization code with PKCE S256, token expiry tracking, refresh-token renewal, and non-destructive JSON updates. See [MusicBrainz OAuth](musicbrainz-oauth.md).

The native Windows runtime also applies `recording_timeout` to MusicBrainz/CAA
release-group and exact-release artwork discovery. This bounds the external
CAA redirect chain without changing source priority or candidate ranking.

Python/Docker `splined --oauth-validation` refreshes an expired MusicBrainz
access token before checking `/oauth2/userinfo`, then performs one normal
metadata lookup. If a nominally current bearer token is rejected, it forces one
refresh and retries once. Successful renewal atomically updates only the
MusicBrainz credential while preserving its other fields.

## Navidrome / OpenSubsonic

Navidrome is supplemental library and current-server artwork enrichment. It
does not replace MusicBrainz release authority, the SPLINED media index, or
canonical path mapping. The shared credential contract is:

```json
{
  "server_url": "https://music.example.invalid",
  "auth_method": "api_key",
  "username": "",
  "password": "",
  "api_key": "synthetic-opensubsonic-api-key"
}
```

`server_url` must be an absolute HTTP or HTTPS URL. `auth_method` is either
`api_key` or `password`; the two methods are mutually exclusive on the wire.
API-key mode sends `apiKey` and deliberately omits the OpenSubsonic `u`
parameter. Password mode sends `u`, a fresh random `s`, and
`t = md5(password + salt)`; it never places the password in a request URL.

For current Navidrome servers, create a dedicated SPLINED key in **Settings >
Players & API keys**, choose or create the SPLINED player, select **Generate API
key**, copy the one-time value, and save the player. A native Navidrome Web-API
JWT/bearer token is a separate, unstable API surface and is neither requested
nor sent for normal OpenSubsonic library/artwork access. Older JSON files that
contain `bearer_token` remain readable, but that value is ignored by `/rest/*`
requests.

SPLINED uses compatible `ping`, library, `search3`, and `getCoverArt` endpoints.
Secrets are never copied into Config v5, SQLite, candidate references, logs, or
screenshots.

Windows **Credentials > Navidrome** masks the active secret field and performs
a live ping/library/artwork capability test. `SAVED` and `TESTED` are distinct:
only a successful request marks the credential tested. Python/Docker can create
the same fixed file with:

```text
splined --navidrome-credentials
```

Candidate discovery prefers an exact MusicBrainz Release ID. Without that ID,
one and only one normalized exact Artist + Album result is accepted. Ambiguous
server matches return no candidate rather than guessing.

Navtagger's post-write targeted scan is intentionally separate. Navtagger
v15.8.5 calls its configured restricted Docker Engine endpoint
(`navidrome_docker_api`) and runs `navidrome scan -t <library>:<target>` inside
the configured container. SPLINED does not reinterpret that transport as an
OpenSubsonic credential and does not invent a second targeted-scan API.

## Windows authorization

Use **Credentials** to edit provider JSON. For MusicBrainz, save the application
fields, choose **Authorize**, approve the request, paste the returned code, and
choose **Complete**. The in-process Rust backend performs PKCE exchange and
atomically updates the same credential file. Root native and Python/container
commands expose the corresponding `--mb-oauth-login` option.

## Moving or backing up credentials

Changing `credential_dir` does not move existing JSON files. Close SPLINED,
move the five standard files deliberately, update Config v5, verify filesystem
permissions, then test provider status before deleting the old copy.

On Windows, use selective `.spl` export to include credentials, internal
settings, and SQLite as needed. On file-backed runtimes, back up the credential
directory, Config v5, and `<scan.cache_dir>/splined.db`. Other candidate and
sample cache files are disposable.

## Related documentation

- [API/OAuth credential validation](oauth-validation.md)
- [MusicBrainz OAuth](musicbrainz-oauth.md)
- [Config v5 reference](config-v5-reference.md)
- [Source policies and Range Types](source-policies-range-types.md)
- [Installation and first run](installation-first-run.md)

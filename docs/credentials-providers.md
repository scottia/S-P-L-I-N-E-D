# Credentials and Provider Setup

All supported S:P:L:I:N:E:D runtimes use Config v5 and the same credential-directory contract. `config.toml` stores only the directory:

```toml
[credentials]
credential_dir = "credentials"
```

Docker normally uses `/credentials`. Standard filenames are fixed beneath that directory; `credential_file` and `token_file` are not Config v5 settings.

| Provider | File | Normal artwork access |
| --- | --- | --- |
| Fanart.tv | `fanarttv.json` | API key required |
| Last.fm | `lastfm.json` | API key required; account session optional |
| Discogs | `discogs.json` | Personal access token required |
| MusicBrainz | `musicbrainz.json` | Anonymous metadata supported; OAuth optional |
| iTunes / Apple | none | Anonymous |
| Cover Art Archive | none | Anonymous |
| Deezer | none | Anonymous |

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

The native Windows/root command is `splined.exe --lastfm-login` on Windows or `./splined --lastfm-login` on Linux/macOS. It polls the pending authorization for up to 60 seconds. Python/Docker exposes `splined --lastfm-login` and performs the same Last.fm API sequence after the user confirms browser authorization.

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

MusicBrainz is metadata authority, not an artwork provider. Anonymous metadata requests remain available when OAuth is disabled. The standard `musicbrainz.json` can contain:

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

Python/Docker `splined --oauth-validation` refreshes an expired MusicBrainz
access token before checking `/oauth2/userinfo`, then performs one normal
metadata lookup. If a nominally current bearer token is rejected, it forces one
refresh and retries once. Successful renewal atomically updates only the
MusicBrainz credential while preserving its other fields.

## Windows GUI versus runtime authorization

Use **File > Credentials...** or **Settings > Advanced > Library, Paths & Processing > Credentials / Status...** to edit provider fields and test saved credentials.

The MusicBrainz GUI test validates an already-present access token through `/oauth2/userinfo`. It does not perform the browser authorization-code exchange. Use the core command:

```text
splined.exe --mb-oauth-login
```

The root native and Python/Docker commands expose the corresponding `--mb-oauth-login` option.

## Moving or backing up credentials

Changing `credential_dir` does not move existing JSON files. Close SPLINED, move the four standard files deliberately, update Config v5, verify filesystem permissions, then test provider status before deleting the old copy.

Back up `credentials/`, `config/`, and the configured history location separately. `_cache/` is disposable.

## Related documentation

- [API/OAuth credential validation](oauth-validation.md)
- [MusicBrainz OAuth](musicbrainz-oauth.md)
- [Config v5 reference](config-v5-reference.md)
- [Source policies and Range Types](source-policies-range-types.md)
- [Installation and first run](installation-first-run.md)

# API/OAuth validation

WebGUI exposes **Settings → Provider Credentials → Test Connection** for each saved provider. Configuration and credential editing take place in the browser. Connection tests load credentials from the configured directory, mask saved secrets, and report success or failure. MusicBrainz tests can refresh saved OAuth tokens automatically.

`/credentials` is the existing Docker Config v5 default, not a validator
override. Relative `credential_dir` values are resolved from the directory
containing `config.toml` through the same `runtime_credential_dir()` and
`credential_file()` path used by normal Python/Docker operation.

## What is tested

Only configured credential files are tested. Missing provider credential files are reported as `SKIP` rather than failures.

- **Discogs** — reads `discogs.json`, validates the saved personal token with an authenticated database search, and returns one representative release result. Discogs application OAuth is not required.
- **Fanart.tv** — reads `fanarttv.json`, tests the saved API key and optional client key against the live Fanart.tv v3.2 MusicBrainz release-group album endpoint, verifies the v3.2 `albums` response, and returns one representative album-cover result when available. The canonical credential format still records `"api_version": "v3.2"`; if that marker is missing or stale, the validator prints `WARN` and continues with the live v3.2 request. The live request determines credential `PASS` or `FAIL`.
- **MusicBrainz** — reads `musicbrainz.json`, refreshes an expired access token before testing it, validates the resulting OAuth bearer token against `/oauth2/userinfo`, then performs a normal MusicBrainz release metadata lookup and reports one representative release plus release-group MBID. If a nominally current token is rejected, validation forces one refresh and retries once. A successful refresh atomically updates the credential and advances `expires_at_unix`.
- **Last.fm** — reads `lastfm.json`, validates the API key using `album.getInfo`, and returns one representative album/artwork result. A Last.fm user session is not required for normal artwork reads.

## Random validation targets

The user does not need to enter an artist, album, or MBID. For each configured provider, SPLINED randomly selects from a small internal set of curated public artist/album records. The selected target is shown as `Random test:` before the provider request.

If a provider accepts the credential but the selected public record is
unavailable in that provider's catalog, SPLINED distinguishes that condition
from an authentication failure and may try another curated target internally.
An accepted credential is not reported as failed merely because a provider
lacks a particular test album.

## Results and recovery

The browser reports PASS, FAIL, or SKIP for the selected provider. Missing credentials are SKIP; a rejected saved credential is FAIL. Secret values are not returned by the editor or included in the test result. Blank secret fields retain saved values.

For Discogs, save a personal token in Settings and test it. For Fanart.tv, save the API key and optional client key; the live v3.2 check determines success even if an old file has no API version marker. For Last.fm, save the API key; normal artwork lookup does not require a user session. For MusicBrainz, save the client fields and choose **Authorize MusicBrainz**, approve in the provider's browser page, paste the returned code, and complete authorization. Test Connection renews saved refreshable tokens.

If a connection fails, check server network access and the saved provider fields, then test again. Reauthorize MusicBrainz when its refresh grant is no longer usable.

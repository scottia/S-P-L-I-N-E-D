# API/OAuth validation

Python/Docker exposes a single non-destructive credential validation command:

```text
splined --oauth-validation
```

The command loads the active SPLINED Config v5 file and resolves the credential directory from:

```toml
[credentials]
credential_dir = "/credentials"
```

No credential path is hard-coded into the validator. Relative paths are resolved the same way as the rest of the Python runtime.

## What is tested

Only configured credential files are tested. Missing provider credential files are reported as `SKIP` rather than failures.

- **Discogs** — reads `discogs.json`, validates the saved personal token with an authenticated database search, and returns one representative release result. Discogs application OAuth is not required.
- **Fanart.tv** — reads `fanarttv.json`, validates the v3.2 API key and optional client key against a release-group artwork lookup, and returns one representative artwork result when available.
- **MusicBrainz** — reads `musicbrainz.json`, validates the saved OAuth bearer token against `/oauth2/userinfo`, then performs a normal MusicBrainz release metadata lookup and reports one representative release plus release-group MBID.
- **Last.fm** — reads `lastfm.json`, validates the API key using `album.getInfo`, and returns one representative album/artwork result. A Last.fm user session is not required for normal artwork reads.

## Random validation targets

The user does not need to enter an artist, album, or MBID. For each configured provider, SPLINED randomly selects from a small internal set of curated public artist/album records. The selected target is shown as `Random test:` before the provider request.

If a provider accepts the credential but the selected public record is unavailable in that provider's catalog, SPLINED distinguishes that condition from an authentication failure and may try another curated target.

## Output and exit status

Each provider ends in one of these states:

- `PASS` — the configured credential was accepted and the provider validation completed.
- `FAIL` — the configured credential was rejected, malformed, unreachable, or the provider test could not complete.
- `SKIP` — the provider credential file is not configured.
- `NO RESULT` — authentication was not rejected, but the selected provider record returned no usable data; SPLINED may continue with another curated target.

The final summary returns exit code `0` when every configured provider passes, or when no provider credentials are configured. It returns exit code `1` when one or more configured providers fail validation.

## Secret handling

The validation command never prints API keys, OAuth bearer tokens, refresh tokens, shared secrets, Fanart.tv client keys, or Discogs personal tokens. It prints only credential file status, provider result metadata, public artwork URLs, and public account identity fields returned by MusicBrainz.

The validator does not rewrite credentials and does not refresh or replace saved tokens. It is intended as an explicit smoke test of the credentials currently stored on disk.

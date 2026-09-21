# API/OAuth validation

Python/Docker exposes a single non-destructive credential validation command:

```text
splined --oauth-validation
```

The command loads the active SPLINED Config v5 file normally. In Docker that
file defaults to `/config/config.toml`. SPLINED then resolves the credential
directory exclusively from:

```toml
[credentials]
credential_dir = "/credentials"
```

`/credentials` is the existing Docker Config v5 default, not a validator
override. Relative `credential_dir` values are resolved from the directory
containing `config.toml` through the same `runtime_credential_dir()` and
`credential_file()` path used by normal Python/Docker operation.

## What is tested

Only configured credential files are tested. Missing provider credential files are reported as `SKIP` rather than failures.

- **Discogs** — reads `discogs.json`, validates the saved personal token with an authenticated database search, and returns one representative release result. Discogs application OAuth is not required.
- **Fanart.tv** — reads `fanarttv.json`, validates the v3.2 API key and optional client key against a MusicBrainz release-group artwork lookup, verifies the v3.2 `albums` response, and returns one representative album-cover result when available.
- **MusicBrainz** — reads `musicbrainz.json`, validates the saved OAuth bearer token against `/oauth2/userinfo`, then performs a normal MusicBrainz release metadata lookup and reports one representative release plus release-group MBID.
- **Last.fm** — reads `lastfm.json`, validates the API key using `album.getInfo`, and returns one representative album/artwork result. A Last.fm user session is not required for normal artwork reads.

## Random validation targets

The user does not need to enter an artist, album, or MBID. For each configured provider, SPLINED randomly selects from a small internal set of curated public artist/album records. The selected target is shown as `Random test:` before the provider request.

If a provider accepts the credential but the selected public record is
unavailable in that provider's catalog, SPLINED distinguishes that condition
from an authentication failure and may try another curated target internally.
An accepted credential is not reported as failed merely because a provider
lacks a particular test album.

## Output and exit status

Each provider ends in one of these states:

- `PASS` — the configured credential was accepted and the provider validation completed. This also covers an accepted credential when every curated target is absent from that provider.
- `FAIL` — the configured credential was rejected, malformed, unreachable, or the provider test could not complete.
- `SKIP` — the provider credential file is not configured.

The final summary returns exit code `0` when every configured provider passes, or when no provider credentials are configured. It returns exit code `1` when one or more configured providers fail validation.

## Secret handling

The validation command never prints API keys, OAuth bearer tokens, refresh
tokens, shared secrets, Fanart.tv client keys, or Discogs personal tokens. It
prints only credential-file status and public provider-result metadata or
artwork URLs.

The validator does not rewrite credentials and does not refresh or replace saved tokens. It is intended as an explicit smoke test of the credentials currently stored on disk.

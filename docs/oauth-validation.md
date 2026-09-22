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
- **Fanart.tv** — reads `fanarttv.json`, tests the saved API key and optional client key against the live Fanart.tv v3.2 MusicBrainz release-group album endpoint, verifies the v3.2 `albums` response, and returns one representative album-cover result when available. The canonical credential format still records `"api_version": "v3.2"`; if that marker is missing or stale, the validator prints `WARN` and continues with the live v3.2 request. The live request determines credential `PASS` or `FAIL`.
- **MusicBrainz** — reads `musicbrainz.json`, validates the currently stored OAuth bearer token against `/oauth2/userinfo`, then performs a normal MusicBrainz release metadata lookup and reports one representative release plus release-group MBID. This validation path is read-only: it does not refresh, replace, or rewrite the saved token.
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

`WARN` is advisory and does not itself change the provider result. For example,
a Fanart.tv credential with a missing or stale `api_version` marker is warned
about, then tested against the live v3.2 endpoint.

The final summary returns exit code `0` when every configured provider passes, or when no provider credentials are configured. It returns exit code `1` when one or more configured providers fail validation.

## Secret handling

The validation command never prints API keys, OAuth bearer tokens, refresh
tokens, shared secrets, Fanart.tv client keys, or Discogs personal tokens. It
prints only credential-file status and public provider-result metadata or
artwork URLs.

The validator does not rewrite credentials and does not refresh or replace saved tokens. It is intended as an explicit smoke test of the credentials currently stored on disk.

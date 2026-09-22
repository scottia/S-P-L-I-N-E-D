# API/OAuth validation

Python/Docker exposes a single credential validation command:

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
- **MusicBrainz** — reads `musicbrainz.json`, refreshes an expired access token before testing it, validates the resulting OAuth bearer token against `/oauth2/userinfo`, then performs a normal MusicBrainz release metadata lookup and reports one representative release plus release-group MBID. If a nominally current token is rejected, validation forces one refresh and retries once. A successful refresh atomically updates the credential and advances `expires_at_unix`.
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

When a configured provider fails, the command prints `Next steps:` immediately
under that failure. Recovery guidance is provider-specific so a user does not
need external documentation to determine which command or credential must be
repaired.

The final summary returns exit code `0` when every configured provider passes, or when no provider credentials are configured. It returns exit code `1` when one or more configured providers fail validation.

## Provider recovery guidance

### Discogs

A rejected or malformed Discogs token tells the user to replace or repair the
configured `discogs.json` personal access token, shows the credential path, and
then rerun:

```text
splined --oauth-validation
```

Discogs does not require an application OAuth exchange.

### Fanart.tv

A rejected or malformed Fanart.tv credential tells the user to run:

```text
splined --fanarttv-credentials
splined --oauth-validation
```

The API key is required and the client key remains optional.

### MusicBrainz

A rejected or malformed MusicBrainz OAuth credential tells the user to run:

```text
splined --mb-oauth-login
splined --oauth-validation
```

After successful reauthorization, the validation command participates in the
automatic refresh lifecycle. Once the short-lived access token expires, run:

```text
splined --oauth-validation
```

The validator refreshes the token, atomically updates `musicbrainz.json`,
advances `expires_at_unix`, and tests the new bearer token. If the provider
rejects a nominally current token, the validator forces one refresh and retry.
If renewal still fails, reauthorize with `splined --mb-oauth-login` and rerun
the validator.

### Last.fm

A rejected or malformed Last.fm API credential tells the user to run:

```text
splined --lastfm-credentials
splined --oauth-validation
```

If user-account authorization is also required for that user's workflow, the
recovery text additionally points to:

```text
splined --lastfm-login
```

A Last.fm user session is still not required for normal artwork reads.

For transport failures or provider-side HTTP failures, SPLINED first tells the
user to check network connectivity and provider availability and rerun the
validator before replacing credentials.

## Secret handling

The validation command never prints API keys, OAuth bearer tokens, refresh
tokens, shared secrets, Fanart.tv client keys, or Discogs personal tokens. It
prints only credential-file status, recovery commands, configured file paths,
and public provider-result metadata or artwork URLs.

Discogs, Fanart.tv, and Last.fm validation remains read-only. MusicBrainz may
atomically rewrite `musicbrainz.json` only when it renews an expired or rejected
access token. Other credential fields and unknown future fields are preserved.

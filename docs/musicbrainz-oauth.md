# MusicBrainz OAuth

All supported Config v5 runtimes use `musicbrainz.json` beneath the configured
`credential_dir`. MusicBrainz supplies release and recording metadata; it is
not an artwork provider and its policy does not use artwork range or dimension
controls.

Anonymous metadata requests remain available while OAuth is disabled. Never
put OAuth tokens, client secrets, or authorization codes in `config.toml`,
logs, screenshots, commits, or support reports.

## Credential contract

The credential document may contain:

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

Secrets belong only in this credential JSON. `config.toml` contains the
credential directory and `[source_policies.musicbrainz]` only.

## Authorization-code flow

The native Windows/root and Python runtimes implement OAuth2 authorization
code with PKCE S256 and a random state value:

1. create the verifier and S256 challenge;
2. open the MusicBrainz authorization URL;
3. receive or prompt for the returned authorization code;
4. exchange the code using the configured client ID, client secret, callback
   URI, and scope;
5. save the access token, refresh token, token type, scope, and expiry;
6. validate authenticated requests as required.

The default callback is `urn:ietf:wg:oauth:2.0:oob` and the default scope is
`profile`. A MusicBrainz application registration must agree with the callback.

Supported login entry points are:

```text
splined.exe --mb-oauth-login        # Windows core
./splined --mb-oauth-login          # Linux/macOS native
splined --mb-oauth-login            # Python/Docker
```

## Windows GUI versus runtime login

The Windows credential editor under **File > Credentials...** (also reachable
from Settings) edits credential fields and tests an already-present bearer
token against `/oauth2/userinfo`. It does not run the browser authorization
exchange. Use `splined.exe --mb-oauth-login` to authorize, then return to the
GUI to inspect or test the saved credential.

## Read-only credential validation

Python/Docker also exposes:

```text
splined --oauth-validation
```

For MusicBrainz, this command validates the **currently stored** access token
against `/oauth2/userinfo`, then performs one normal release metadata lookup
using a curated public release. It is a diagnostic smoke test, not a login or
refresh path.

`--oauth-validation` does **not** refresh an expired token, exchange a refresh
token, or rewrite `musicbrainz.json`. A rejected bearer token is reported as
`FAIL` even when a refresh token is present. Use normal runtime access or
`--mb-oauth-login` when renewal or reauthorization is required.

The command never prints the access token, refresh token, client secret, or
other saved credential values. See [API/OAuth credential validation](oauth-validation.md)
for the shared provider behavior and `PASS` / `FAIL` / `SKIP` semantics.

## Refresh and non-destructive updates

When an access token is expired or within the runtime safety window, SPLINED
uses the saved refresh token and client credentials to renew it. A response
that omits a new refresh token retains the current one. Updates use
read-modify-write behavior so authentication fields, `options`, and unknown
future fields are preserved, then the credential file is atomically replaced.

This automatic refresh behavior applies to normal runtime MusicBrainz access;
it is intentionally not invoked by the read-only `--oauth-validation` smoke
test.

Credential files receive the filesystem protection described in
[Credentials and provider setup](credentials-providers.md).

## Runtime options

| Option | Default | Meaning |
| --- | ---: | --- |
| `retry_max` | `4` | Maximum retry count for retryable requests |
| `min_delay` | `1.05` | Minimum seconds between MusicBrainz requests |
| `recording_timeout` | `7` | Recording-query timeout in seconds |

When `[source_policies.musicbrainz].source_override` is `false`, the standard
defaults are active while saved custom options remain retained. When it is
`true`, the saved `options` values become active. `enabled = false` disables
MusicBrainz participation without deleting credentials.

## Recovery

- If the access token expires, allow automatic refresh during normal runtime access or run the login command again.
- If `--oauth-validation` reports the stored bearer token as rejected, use normal runtime refresh or reauthorize; the validation command itself will not modify credentials.
- If refresh fails because consent or the refresh token was revoked,
  reauthorize; do not invent token values.
- If JSON is malformed, close SPLINED, make a private backup, repair only known
  data or reauthorize, then run the GUI credential test or an authenticated
  command.
- Securely remove obsolete credential backups after recovery.

## Related documentation

- [API/OAuth credential validation](oauth-validation.md)
- [Credentials and provider setup](credentials-providers.md)
- [Config v5 reference](config-v5-reference.md)
- [Source policies and Range Types](source-policies-range-types.md)

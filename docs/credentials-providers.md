# Credentials and provider configuration

This guide describes supported credential-directory contracts and
provider connection setup from the current Rust/WebGUI source.
Secrets must never appear in source, issues, diagnostics, release
notes or unredacted configuration exports.

## Credential location

Config v5 stores the configured directory, not the secrets:

```toml
[credentials]
credential_dir = "credentials"
```

Windows portable or packaged state and WebGUI container volumes may
resolve that relative directory differently. WebGUI normally mounts
a persistent `/credentials` volume; Windows Setup chooses a
persistent local, mapped or UNC directory. Back up these files
separately from Config v5 and the music library.

| Provider | Credential source |
| --- | --- |
| Fanart.tv | `fanarttv.json`: API key |
| Last.fm | `lastfm.json`: API key, optional authorized session |
| Discogs | `discogs.json`: personal access token |
| MusicBrainz | `musicbrainz.json`: optional OAuth for supported actions; basic public lookup can be anonymous |
| Navidrome / OpenSubsonic | `navidrome.json`: service URL and supported API key or username/password |
| Optional Album coordination | `album-coordination.json`: explicit Windows connection to WebGUI PostgreSQL |
| Anonymous providers | No credential JSON when no authenticated operation is required |

Use the current WebGUI
[`credentials.py`](../build/WEBGui/webgui/credentials.py)
and Windows
[`credentials.rs`](../build/Windows/src/credentials.rs)
for accepted file schemas and validation. A saved secret is not
returned in plaintext to the WebGUI browser.

## Windows SQLite setup and coordination

**Standalone Windows SQLite** needs neither a PostgreSQL credential
nor a separately managed media-state policy. **Shared / Network
SQLite** uses the same approved behavioral JSON as standalone mode;
it merely changes how one physical `splined.db` is accessed.

If the operator explicitly chooses optional PostgreSQL coordination,
Setup must test the connection and preserve exact credentials.
The PostgreSQL catalog is **not** a replacement authority for
Windows-owned Artist/Album/Track fields. Any persisted policy
revision must agree with the canonical
[`schema/media-state-classification.json`](../schema/media-state-classification.json)
and
[`schema/comp-report-policy.json`](../schema/comp-report-policy.json).
An existing conflicting local or PostgreSQL revision requires an
explicit error/review path, not silent replacement, a second
default, or an inferred mode-based policy change.

`scan.postgres_coordination` is a distinct Config v5 option. A
legacy installed coordination credential is not permission to
create a new connection without an intentional setup decision.

## Provider validation and authentication

- **Fanart.tv:** use the supported API key and request version;
  validate with the provider's connection test.
- **Last.fm:** API-key discovery and optional account authorization
  are separate. Authorize through the browser and save a validated
  user session; do not log session keys.
- **Discogs:** use a personal access token from the account;
  preserve its contents as a secret.
- **MusicBrainz:** public release/artwork lookup and account-scoped
  OAuth are different capabilities. OAuth tokens must be checked,
  refreshed or reauthorized through the supported interface.
- **Navidrome/OpenSubsonic:** test the server address and
  authentication using the selected mode; do not confuse a
  server-ready response with an approved Artist identity.

The exact availability of OAuth/setup controls in Windows and WebGUI
must be verified against each current UI, rather than inferred from
older command-line instructions.

## Filesystem and backup safeguards

Where supported, Windows writes files with appropriate user ACLs
and file-backed runtimes apply restrictive permissions. Storage
that cannot enforce those guarantees needs an explicit warning;
SPLINED does not independently encrypt every credential JSON.
Use an encrypted persistent volume or protected NAS share as needed.

Changing `credential_dir` does **not** relocate existing files;
move and verify the data deliberately before deleting the original.
Never include credentials in a public issue or an unencrypted log.

See [Config v5](config-v5-reference.md),
[Windows application](windows-guide.md), and
[WebGUI workspace](browser-workspace-guide.md).
Credential connection behavior remains subject to runtime testing.

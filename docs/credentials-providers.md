# Credentials and Provider Setup

This page documents how S:P:L:I:N:E:D stores provider credentials, how those credentials relate to Config v5, and what users should expect when enabling artwork providers.

> **Windows target:** v3.0.0 Stable with Config v5.
>
> **Repository note:** the finalized Windows source has not yet been integrated into the repository. Exact GUI labels should be verified against the final v3.0.0 source before publication, but the credential architecture described here is the intended public behavior.

---

## Core rule: credentials are separate from config

`config.toml` identifies the **credential directory**. Provider secrets live in separate JSON files beneath that directory.

Conceptually:

```toml
[credentials]
credential_dir = "credentials"
```

Typical Windows portable layout:

```text
SPLINED/
├── splined.exe
├── config/
│   └── config.toml
├── credentials/
│   ├── musicbrainz.json
│   ├── lastfm.json
│   ├── fanarttv.json
│   └── discogs.json
├── _cache/
└── _logs/
```

Typical Docker mapping:

```text
/config/config.toml
/credentials/
```

The normal Config v5 workflow should not require users to repeat standard provider filenames in `config.toml`. Standard filenames are resolved beneath the configured credential directory unless an intentionally supported compatibility override exists.

---

## Why credentials are separate

Keeping credentials outside `config.toml` provides several benefits:

- configuration can be shared without sharing secrets;
- provider credentials can be replaced independently;
- backups can apply different protection policies to config and credentials;
- cloud/NAS/encrypted-volume storage can protect credential files without changing SPLINED configuration;
- provider authentication can be refreshed without reconstructing the main config.

Credential files are **sensitive data**. Never commit them to GitHub, attach them to issue reports, paste them into logs, or include them in public examples.

---

## Filesystem protection

When SPLINED creates a credential file, it should apply restrictive user-specific filesystem permissions where the selected storage supports them.

If the storage location cannot provide user-specific ACL protection, SPLINED should warn clearly rather than claiming stronger protection than the filesystem can provide.

Expected wording is intentionally simple:

```text
Credential file created; user-specific filesystem ACL protection
is unavailable on this storage location.
```

SPLINED does not require its own heavy encryption layer. Users may choose operating-system, OneDrive, Google Drive, NAS, encrypted-volume, BitLocker, EFS, ZFS, Btrfs, or other storage protection appropriate to their environment.

---

# Provider overview

SPLINED currently supports artwork discovery from:

- iTunes / Apple artwork
- Fanart.tv
- Last.fm
- Cover Art Archive
- Deezer
- Discogs

Not every provider requires credentials.

| Provider | Credentials normally required | Standard credential file |
| --- | --- | --- |
| iTunes / Apple artwork | No | — |
| Cover Art Archive | No | — |
| Deezer | No for normal artwork lookup | — |
| Fanart.tv | Yes | `fanarttv.json` |
| Last.fm | Yes | `lastfm.json` |
| Discogs | Yes | `discogs.json` |
| MusicBrainz metadata/OAuth | Optional or feature-dependent | `musicbrainz.json` |

MusicBrainz is metadata authority/resolution infrastructure rather than an artwork source in the same sense as the providers above. Its OAuth flow is documented separately in [MusicBrainz OAuth](musicbrainz-oauth.md).

---

# Fanart.tv

Fanart.tv uses a provider credential file beneath the configured credential directory.

The current repository-side credential model includes:

```json
{
  "api_key": "example-api-key",
  "client_key": "example-client-key"
}
```

Use synthetic values in examples and tests only.

The Windows GUI should create or update the provider credential without exposing the secret in logs after entry.

If authentication fails:

1. confirm the provider is enabled;
2. confirm `fanarttv.json` exists beneath the resolved credential directory;
3. verify the key values were entered correctly;
4. verify the host has outbound HTTPS access;
5. retry after checking the provider account/key status.

Do not place the API key directly in `config.toml`.

---

# Last.fm

The current credential model supports fields including:

```json
{
  "api_key": "example-api-key",
  "shared_secret": "example-shared-secret",
  "username": "example-user"
}
```

A particular operation may not require every stored field, but credential updates should preserve existing fields unless the user explicitly replaces them.

Expected file:

```text
credentials/lastfm.json
```

If Last.fm discovery fails, verify the credential file, provider enablement, network access, and account/API-key status.

---

# Discogs

Discogs uses a provider token stored in `discogs.json`.

The Python implementation currently expects a `token` value when Discogs authentication is used:

```json
{
  "token": "example-token"
}
```

Never expose the token in diagnostic output.

The repository currently contains implementation differences between the native/root source and Python source around Discogs provider execution. Until the finalized Windows v3.0.0 source is integrated, treat Windows and Python/Docker Discogs behavior as separate supported implementation details rather than assuming identical execution paths.

---

# MusicBrainz

MusicBrainz may operate with unauthenticated metadata requests or authenticated OAuth behavior depending on the feature and configuration.

The standard credential file is:

```text
credentials/musicbrainz.json
```

Authentication data and runtime request options belong in the MusicBrainz credential document and must be updated **non-destructively**.

Changing runtime options must not erase:

- access token;
- refresh token;
- client ID;
- client secret;
- other recognized or future authentication fields.

See [MusicBrainz OAuth](musicbrainz-oauth.md).

---

# Providers that do not normally require credentials

## iTunes / Apple artwork

Normal artwork discovery does not require a user credential file.

## Cover Art Archive

Normal Cover Art Archive artwork discovery does not require a user credential file.

## Deezer

Normal public artwork lookup does not require a user credential file.

A provider can still fail because of network connectivity, rate limiting, upstream availability, malformed responses, regional behavior, or changed provider APIs. A missing credential file is therefore not the only possible provider failure.

---

# Creating credentials in the Windows GUI

The finalized Windows GUI should expose credential creation/status from its Paths/credential area rather than requiring users to hand-edit JSON for normal setup.

Expected workflow:

1. Open the relevant Settings/Paths credential control.
2. Choose the provider.
3. Enter or authorize the provider credential.
4. SPLINED writes the provider JSON beneath the configured credential directory.
5. SPLINED applies restrictive user ACL protection where supported.
6. The GUI reports credential status without printing the secret.

If a provider credential already exists, the update must preserve unrelated fields unless the operation explicitly replaces the whole credential.

---

# Manual credential editing

Manual JSON editing is supported for advanced recovery/troubleshooting but should not be the normal first-run path.

When editing manually:

- stop SPLINED first;
- keep a backup of the existing credential file;
- preserve valid JSON syntax;
- do not remove unknown fields merely because they are not currently understood;
- do not paste secrets into support requests;
- restart SPLINED and verify provider status afterward.

JSON files must use double quotes around object keys and string values.

---

# Credential directory changes

Changing `credential_dir` changes where SPLINED looks for provider credential files. It does **not** automatically migrate the old files.

If moving credentials:

1. close SPLINED;
2. copy or move the credential files to the new directory;
3. update Config v5 to the new credential directory;
4. verify filesystem permissions;
5. restart SPLINED;
6. confirm provider status before deleting the old copy.

For portable Windows installs, relative application-owned paths are preferred where practical.

---

# Backup guidance

Back up credentials separately from disposable cache data.

Recommended importance:

| Data | Backup priority |
| --- | --- |
| `credentials/` | High |
| `config/` | High |
| `_logs/_history/` | High if preserving scan authority/state matters |
| `_cache/` | Low / disposable |

If a credential is revoked or compromised, replace/revoke it at the provider and update the local JSON file.

---

# Troubleshooting checklist

If a provider reports missing or invalid credentials:

1. confirm the provider actually requires credentials;
2. confirm the configured credential directory resolves to the intended location;
3. confirm the expected JSON file exists;
4. confirm the file contains valid JSON;
5. confirm required fields are non-empty;
6. confirm the application user can read the file;
7. verify outbound HTTPS/network access;
8. check provider account/key/token validity;
9. restart or retry the provider after correction.

Do not solve a credential error by embedding secrets into `config.toml`.

---

# Security rules

Never commit or publish:

- API keys;
- shared secrets;
- OAuth client secrets;
- access tokens;
- refresh tokens;
- personal provider usernames when not required for an example;
- real credential JSON files.

Repository examples must use obviously synthetic placeholders only.

---

# Related documentation

- [Config v5 reference](config-v5-reference.md)
- [MusicBrainz OAuth](musicbrainz-oauth.md)
- [Installation and first run](installation-first-run.md)
- [Docker installation](../docker/README.md)

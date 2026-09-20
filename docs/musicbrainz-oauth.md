# MusicBrainz OAuth

This page documents the S:P:L:I:N:E:D MusicBrainz authentication model, credential storage, token refresh expectations, and recovery steps.

> **Windows target:** v3.0.0 Stable with Config v5.
>
> **Repository note:** exact Windows GUI button labels should be verified against the finalized v3.0.0 source when it is integrated. The credential and preservation rules below are the intended public contract.

---

## MusicBrainz in SPLINED

MusicBrainz is used as release/recording metadata authority during album resolution. SPLINED may make MusicBrainz requests without OAuth for operations that do not require authenticated access, while OAuth can provide authenticated request behavior where configured.

MusicBrainz authentication data is stored separately from `config.toml`.

Standard credential file:

```text
credentials/musicbrainz.json
```

Config v5 identifies the credential directory rather than embedding tokens in the main config.

---

## What OAuth stores

A MusicBrainz OAuth credential may contain fields such as:

```json
{
  "client_id": "example-client-id",
  "client_secret": "example-client-secret",
  "access_token": "example-access-token",
  "refresh_token": "example-refresh-token"
}
```

The exact document may contain additional recognized or future fields.

SPLINED must treat the credential document as persistent sensitive data and update it **non-destructively**.

Changing one field or runtime option must not erase unrelated authentication data.

---

## Runtime options

The current Windows Config v5 behavior keeps MusicBrainz request tuning in the MusicBrainz credential document rather than scattering those values through provider source policy.

Current intended defaults:

```json
{
  "options": {
    "retry_max": 4,
    "min_delay": 1.05,
    "recording_timeout": 7
  }
}
```

Meaning:

| Option | Purpose |
| --- | --- |
| `retry_max` | Maximum retry attempts for eligible transient MusicBrainz request failures |
| `min_delay` | Minimum delay between requests, in seconds |
| `recording_timeout` | Recording-oriented request timeout, in seconds |

Updating `options` must preserve existing tokens, client credentials, and unknown fields.

---

# OAuth authorization flow

The exact visual sequence depends on the Windows GUI implementation, but the flow is conceptually:

1. Open the MusicBrainz credential/authentication control.
2. Enter or confirm the MusicBrainz OAuth application information required by the current build.
3. Start authorization.
4. SPLINED opens or provides the MusicBrainz authorization URL.
5. Sign in to MusicBrainz and approve the requested scope.
6. Return the authorization result/code to SPLINED if the flow requires it.
7. SPLINED exchanges the authorization code for token data.
8. SPLINED writes the updated `musicbrainz.json` credential file.
9. SPLINED applies restrictive user-specific filesystem protection where supported.
10. Verify authenticated status before starting a production scan.

The current codebase includes an authorization-code exchange path. An empty authorization code is treated as an error rather than silently accepted.

---

## Callback behavior

Older repository configuration examples may contain callback values associated with earlier/native flows. Do not assume an old Config v4 callback field is the final Windows v3.0.0 GUI presentation.

The finalized v3.0.0 source should define the actual callback/user interaction used by the Windows GUI.

The documentation principle remains:

> OAuth application/client information and token data must remain separate from normal artwork source policy and must never be printed in logs.

---

# Token refresh

When an access token expires and a refresh token is available, SPLINED should refresh authentication without requiring the user to reconstruct the credential document.

A successful refresh should update only the token-related fields that changed while preserving:

- client ID;
- client secret;
- runtime `options`;
- unrelated recognized fields;
- unknown/future fields where practical.

If the provider returns a new refresh token, store it. If no replacement refresh token is returned, preserve the existing valid refresh token rather than clearing it accidentally.

---

# Reauthorization

Reauthorization may be necessary when:

- the refresh token has expired or been revoked;
- the OAuth application credentials changed;
- the MusicBrainz account revoked access;
- the credential JSON is incomplete/corrupt;
- the requested scope changes;
- the local credential was intentionally reset.

Before reauthorizing, keep a backup of the current `musicbrainz.json` if troubleshooting preservation behavior.

Do not publish that backup.

---

# Non-destructive credential updates

This is a critical rule.

Incorrect behavior:

```text
read only the access token
-> create a new minimal JSON document
-> overwrite musicbrainz.json
```

That can destroy refresh tokens, client data, runtime options, or future fields.

Correct behavior:

```text
load full JSON document
-> update only intended fields
-> preserve everything else
-> write atomically
```

Where the platform supports it, use atomic replacement semantics so an interrupted write does not leave a truncated credential file.

---

# Filesystem security

`musicbrainz.json` may contain secrets and bearer credentials.

SPLINED should apply restrictive user ACLs where supported.

If user-specific protection is unavailable on the selected storage, report that limitation explicitly.

Do not log:

- client secrets;
- access tokens;
- refresh tokens;
- authorization codes.

Diagnostic messages should report status and sanitized provider errors only.

---

# Request mode and fallback behavior

The application may operate in different MusicBrainz request modes depending on whether OAuth is configured and valid.

Authenticated mode should use bearer authorization when valid token data is available.

If OAuth is optional for a particular operation, lack of OAuth should not automatically be interpreted as a broken MusicBrainz installation. The current operation/configuration determines whether authentication is required.

Do not confuse:

```text
MusicBrainz metadata resolution
```

with:

```text
artwork-provider enable/disable policy
```

MusicBrainz is not simply another artwork source toggle.

---

# Common OAuth failures

## Authorization code is empty

Cause:
- authorization was cancelled;
- no code/result was returned;
- an empty value was submitted.

Action:
- restart the authorization flow and complete it fully.

## Authorization-code exchange fails

Possible causes:
- invalid/expired code;
- wrong client credentials;
- network failure;
- upstream MusicBrainz/OAuth error;
- callback mismatch where applicable.

Action:
- retry authorization;
- verify client configuration;
- confirm outbound HTTPS access;
- inspect the sanitized error status without exposing credentials.

## Access token rejected

Possible causes:
- expired token;
- revoked token;
- malformed credential JSON.

Action:
- allow refresh if possible;
- otherwise reauthorize.

## Credential file exists but authentication is unavailable

Check:

1. valid JSON syntax;
2. expected credential directory;
3. readable file permissions;
4. non-empty client/token fields needed by the selected flow;
5. no accidental overwrite that removed fields;
6. runtime options did not replace the authentication object.

---

# Manual recovery

If `musicbrainz.json` becomes malformed:

1. close SPLINED;
2. make a private backup of the file;
3. validate/fix JSON syntax if the credential data is still trustworthy;
4. if the token state is unknown, reauthorize instead of guessing token values;
5. reopen SPLINED and verify MusicBrainz status;
6. remove obsolete private backups after confirming recovery, according to your own security policy.

Never paste a live token into a public support request.

---

# Relationship to Config v5

Config v5 should contain the credential directory, not OAuth secrets.

Conceptually:

```toml
[credentials]
credential_dir = "credentials"
```

Normal users should not need to add:

```text
token_file = "musicbrainz.json"
```

because the standard filename can be resolved beneath the credential directory.

Backward-compatible parsing of older fields may exist, but the GUI should not emit obsolete fields into a clean Config v5 file unless they remain intentionally required.

---

# Related documentation

- [Credentials and provider setup](credentials-providers.md)
- [Config v5 reference](config-v5-reference.md)
- [Troubleshooting](troubleshooting.md) *(planned)*

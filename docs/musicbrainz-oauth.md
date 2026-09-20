# MusicBrainz OAuth

> **Windows target:** v3.0.0 Stable with Config v5.

MusicBrainz supplies release and recording metadata. It is not an artwork
source and does not appear in `cover_sources`.

## Storage

Config v5 stores only the credential directory. MusicBrainz authentication,
OAuth fields, validation metadata, and runtime options remain in the standard
MusicBrainz JSON beneath that directory.

Never put OAuth tokens, client secrets, or authorization codes in
`config.toml`, logs, screenshots, commits, or support reports.

## Windows credential screen

Open **File > Credentials... > MusicBrainz OAuth** or use **Credentials /
Status...** from Settings.

The Windows credential editor exposes:

- Enable MusicBrainz OAuth;
- Client ID;
- Client secret;
- Callback URI;
- OAuth scope;
- Access token;
- Refresh token;
- a live credential test against MusicBrainz OAuth user information.

The screen edits and validates saved credential data. It does not claim that a
saved file is authenticated until the live test succeeds. Public metadata
lookups can remain anonymous when OAuth is disabled.

The current Python command line retains its own `--mb-oauth-login`
authorization workflow. Windows and Python/Docker are separate supported
interfaces.

## Runtime options

Windows v3.0.0 Stable stores request tuning under the credential document's
`options` object:

```json
{
  "options": {
    "retry_max": 4,
    "min_delay": 1.05,
    "recording_timeout": 7
  }
}
```

| Option | Type | Default | Meaning |
| --- | --- | ---: | --- |
| `retry_max` | integer | 4 | Maximum retry count for retryable requests |
| `min_delay` | number | 1.05 | Minimum seconds between requests |
| `recording_timeout` | integer | 7 | Recording-query timeout in seconds |

When `options` is absent, these defaults are used. Existing custom values
round-trip unchanged until the user edits them.

## Non-destructive updates

Every credential update follows read-modify-write behavior:

1. read the existing JSON;
2. preserve authentication fields and unknown/future fields;
3. merge only the fields the user changed;
4. preserve `options` when authentication changes;
5. preserve authentication when options or source policy change;
6. atomically replace the credential file.

Changing only `retry_max` does not change `min_delay`,
`recording_timeout`, tokens, or other fields. Source Enabled and Source
Override are stored in Config v5 and do not rewrite the credential JSON.

## Source policy

`[source_policies.musicbrainz]` contains:

```toml
[source_policies.musicbrainz]
enabled = true
source_override = false
```

Artwork Range Type and image-dimension controls do not apply to MusicBrainz
metadata queries. Enabling Source Override activates the saved MusicBrainz
runtime options; disabling it retains those values.

## Status meanings

- **Disabled:** OAuth is off and public metadata remains available where the
  operation permits anonymous access.
- **Incomplete/Auth needed:** required OAuth fields or a usable access token
  are missing.
- **Saved:** data exists but has not necessarily passed a live test.
- **Validated:** MusicBrainz accepted the saved bearer token through its OAuth
  user-information endpoint.

An expired or revoked token requires refresh or reauthorization through the
supported runtime workflow. SPLINED never fabricates or logs replacement
tokens.

## Recovery

If the credential JSON is malformed:

1. close SPLINED;
2. make a private backup;
3. repair JSON only if the existing credential data is trusted;
4. otherwise reauthorize instead of guessing token values;
5. reopen SPLINED and run the saved-credential test;
6. securely remove obsolete backups after recovery.

## Related documentation

- [Credentials and provider setup](credentials-providers.md)
- [Config v5 reference](config-v5-reference.md)
- [Source policies and Range Types](source-policies-range-types.md)

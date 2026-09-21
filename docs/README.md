# S:P:L:I:N:E:D Documentation

These versioned repository pages are the canonical public documentation for
[S:P:L:I:N:E:D](../README.md).

## Supported implementations

S:P:L:I:N:E:D has separate supported runtimes:

- **Windows GUI:** v3.0.0 Stable, using Config v5.
- **Windows source:** [`../windows/`](../windows/README.md), containing the
  finalized GUI and its Config v5 processing core.
- **Repository-root native command line:** Config v5 implementation used by
  Linux and macOS; its application version follows repository releases.
- **Python/Docker:** Config v5 runtime whose application version follows
  repository releases.

Application release versions and configuration schema versions are independent.
Do not infer one from another.

## Start here

- [Installation and first run](installation-first-run.md)
- [Config v5 reference](config-v5-reference.md)
- [Credentials and provider setup](credentials-providers.md)
- [API/OAuth credential validation](oauth-validation.md)
- [MusicBrainz OAuth](musicbrainz-oauth.md)
- [Source policies and Range Types](source-policies-range-types.md)
- [Select Media and status colors](media-filter-status-colors.md)
- [History, retention, bypass, and timeout](history-retention-bypass-timeout.md)
- [Docker installation](../docker/README.md)

The Windows GUI opens this page from both **Help > Help** and the **Help**
button in **About S:P:L:I:N:E:D**.

## Configuration examples

- [`../config.example.toml`](../config.example.toml) is the native/Windows
  Config v5 example.
- [`../docker/config.example.toml`](../docker/config.example.toml) is the
  Python/Docker Config v5 example with container paths.

Both examples use the central credential-directory architecture. Provider
secrets and normal provider credential filenames do not belong in
`config.toml`.

## Windows status colors

| Color | Meaning |
| --- | --- |
| White | Default / unprocessed |
| Orange | Processed; manually reprocessable |
| Red | Bypassed; explicit override required |
| Purple | Partial artist or timeout-active album, depending on row type |
| Green | Artist complete |
| Blue | Artist contains at least one bypassed album |

The tree derives these states from the same album folder, history, bypass, and
timeout authority used by execution. It does not maintain a separate persistent
GUI status database.

## Python command equivalence

A dedicated cross-runtime command-equivalence page is not yet published.
Current Python command behavior remains authoritative in:

- `python/splined.py --help`
- `python/splined.py`
- `python/splined_scan.py`

The Windows Settings tools retain Config v5 validation and direct access to the
configured config and log folders without embedding a duplicate Python-help
window.

## Security and safe operation

- Never commit credential JSON, API keys, OAuth tokens, or secrets.
- Use Read mode to evaluate without changing artwork in album folders.
- Test Write mode against a copy, backup, snapshot, or staging library first.
- Back up `config/`, `credentials/`, and the configured log/history location.
  `_cache/` is disposable.

## Project links

- [Repository README](../README.md)
- [Windows portable instructions](../release/README-WINDOWS.txt)
- [Linux portable instructions](../release/README-LINUX.txt)
- [macOS portable instructions](../release/README-MACOS.txt)
- [Latest releases](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest)

# PR #133 PC acceptance checkpoint

Date: 2026-11-02  
Branch: `feat/windows-backup-snapshot-lifecycle`  
Revision: `1dd01f3208278af9b9b0e1216f79f277e6287e23`

## Repository checkpoint

- Draft PR: <https://github.com/scottia/S-P-L-I-N-E-D/pull/133>
- GitHub Actions run: `41544714647`
- Relevant Windows Rust/Slint advisory job: passed
- WebGUI and native Linux jobs: skipped by scope selection

## PC installation

The unchanged quick local updater built and installed the validated revision at
`E:\apps\SPLINED\SPLINED.exe`.

| Artifact | SHA-256 |
| --- | --- |
| Previous PC test executable | `E8B34906F671F1F2EA8960AF3A6B91BFC31C58B7C6B8CAD0F7EE73D3019E807F` |
| PR #133 PC test executable | `6A17DDC142E02A4C80279F5C3281BE1827110912F70F49CBE262C08DE9DD522E` |

The previous executable, `data/config.toml`, `data/ui.toml`, and isolated
`test-state` were backed up under
`E:\apps\SPLINED\backups\pr133-preinstall-20261102-172826` before installation.
The installed executable reports version `2.0.22` and the exact PR revision
above. Version `2.0.22` is the current repository metadata inherited from
WebGUI-only releases; PR #133 does not change release metadata.

## Isolated artifact validation

The PC test used only `E:\apps\SPLINED\test-state` and the minimal local music
library. No production library or database was modified.

- Saved Settings archive: `.spl` produced and parsed; Config v5 library root
  preserved; interface settings included; credentials deliberately excluded
  from this synthetic acceptance artifact.
- SQLite Database Backup: online backup produced a standalone SQLite file;
  `PRAGMA quick_check` returned `ok`; schema version `2` and the canonical
  library root were preserved.
- Database restore: restored into a disposable directory; restored SHA-256
  exactly matched the backup artifact.
- Diagnostic Summary: valid JSON produced and loaded read-only; malformed JSON
  failed safely with a validation error.
- Authoritative SQLite Snapshot: two standalone snapshots and their provenance
  sidecars were published; both had identical SHA-256 values; `quick_check`
  returned `ok`; schema and canonical library root were preserved.
- Source safety: the live isolated SQLite source had the same SHA-256 before
  and after all exports.
- Preference migration: the old shared paths were retained for Settings,
  Database, and Diagnostic types; the authoritative Snapshot received its own
  safe directory; all four schedule/max-file fields persisted across restart.

Machine-readable results are in
[`pc-artifact-validation.json`](pc-artifact-validation.json).

## PR #130 reference — status validation for every Album type

Owner acceptance of the Windows-created snapshot exposed the same
unknown-versus-confirmed classification defect that PR #130 corrected for COMP
thumbnail validation. PR #130 keeps a temporary or failed validation separate
from a confirmed purple repair condition; the regular Album-card path still
treated `embedded_tracks = 0` as confirmed missing even when reconciliation had
not inspected the mapped media.

- PR #133 remains the current draft and acceptance checkpoint.
- The Windows snapshot is valid and contains the committed authoritative SQLite
  state.
- PostgreSQL successfully received the applicable safe differences.
- After safe apply, WebGUI now verifies every mapped target track through the
  existing embedded-artwork reader and requires PR #130's exact square
  preferred-thumbnail dimensions before assigning complete state.
- Albums reconciled before this verifier existed are checked lazily when their
  Artist Overview opens; a repeated SQLite difference is not required.
- Migration 013 no longer promotes historical `embedded_tracks` counters to
  verified. Migration 014 invalidates the optimistic verification produced by
  earlier PR-branch deployments without changing those counters, so the first
  Artist Overview readback supplies the actual evidence.
- A read/access failure records `retry` and preserves the prior counter instead
  of presenting a false purple repair condition.
- A successful read that finds missing, partial, corrupt, or wrong-size artwork
  is confirmed attention evidence; only every track at the configured preferred
  embedded-thumbnail side is verified complete/green.
- The rule applies to regular Artist Albums, Various Artists, Soundtracks, and
  Featured appearances because they share the Album-card state authority.
- COMP fallback retains PR #130's lazy per-track `checking` / `check-error`
  behavior and never converts a failed validation into purple.

The PostgreSQL field `embedded_validation_status` records `unknown`, `verified`,
or `retry`. Only `verified` evidence can make an identified Album green or
purple. This is deliberately derived from the mapped WebGUI media after
reconciliation; the Windows snapshot remains unchanged and authoritative for
its committed SQLite fields.

## Remaining owner-visible checks

The current Remotix/virtual-monitor surface rejected Windows Graphics Capture
(`CreateForMonitor` error `0x80070057`) and input capture (`GetCursorPos` access
denied). The installed application itself started normally, loaded 210 albums,
and logged revision `1dd01f3`, but automated screenshots and canvas interaction
could not be captured honestly on this display.

The following owner-visible checks therefore remain open and are not claimed:

- four Backup Type screens and their yellow summary rows;
- manual Save Backup Preferences behavior, independent schedules, retention,
  and counts;
- SQL Database Snapshots panel;
- four Restore Type screens and explicit restore confirmation behavior;
- Diagnostic JSON rendered in Verbose / Run Details;
- Library & Files yellow WebGUI Runtime row;
- WebGUI Compare/Dry Run visibility, because no WebGUI test service was
  listening on this PC during this checkpoint.


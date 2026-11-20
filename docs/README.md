# SPLINED documentation — restructuring checkpoint

**Status: functional documentation and behavioral policy under review.**
This branch does not yet publish an approved application-wide policy register.
Source observations, old test expectations, PR acceptance records, and archived
documents are evidence, **not** automatic instructions for a future code change.

## Verified release structure

[Build layout](build-layout.md) records the two Workflow 01 release sources:
`build/WEBGui/` for the GHCR Python/browser application and `build/Windows/`
for the native Rust/Slint application. Shared runtime schemas remain under
`schema/`; deployment inputs remain under `docker/`; release automation
remains under `.github/workflows/` and `release/`.

## Documentation quarantine

The prior guides, policy Markdown, architecture narratives, screenshots,
dated comparisons, and PR acceptance records have been **preserved, not
destroyed**, on the [dev-archive branch](https://github.com/scottia/S-P-L-I-N-E-D/tree/dev-archive/historical/2026-11-19-pre-policy-docs-cleanup/docs).
They were withdrawn from active `/dev/docs` because their approval and
consistency have not been established. Their presence in the archive does
not grant authority.

WebGUI and Windows retain runtime policy modules, active JSON configuration
contracts, SQL schemas, and tests needed to build or operate. These are
technical dependencies, not independently approved behavioral policy.

## Pending review

Before publishing new Help or approving an application policy, verify both
clients against the owner's intended rules, resolve cross-document conflicts,
and revalidate Help content. Changes must not be justified solely by an
existing implementation or historical branch.

Application links intended as official Help must target **`/main`**, never
`/dev`; the current `/main` Help content also requires owner review and is
not being changed during this checkpoint.

For current public materials, consult the repository
[README](../README.md), [Support](../SUPPORT.md),
[Security](../SECURITY.md), and [Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases).

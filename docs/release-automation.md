# Release automation

The former production release workflows numbered 10 and 21–24 are retired.
They are intentionally absent from `.github/workflows` while their official
single-run replacements are reviewed independently.

Workflow 20 remains a `dev` evaluation path only. Its Windows executable and
WebGUI image are QA artifacts, are marked `release_eligible: false`, and must
not be promoted or published as an official `main` release.

Until the replacement release workflows are merged, no supported automated
production release path is available. Do not reconstruct a release by manually
combining artifacts from the retired workflows or from Workflow 20.

The shared release-note configuration remains
[`release/cliff.toml`](../release/cliff.toml).

# Workflow 01 build source map

**Verified source structure only**; application behavior and deployment
acceptance are separate from the owner-approved [Policy Register](POLICY-REGISTER.md).

## Active targets

| Target | Source | Primary build/packaging input |
| --- | --- | --- |
| GHCR WebGUI | `build/WEBGui/webgui/` | `build/WEBGui/Dockerfile`; Python package `webgui` |
| Windows Portable + Store | `build/Windows/` | `build/Windows/Cargo.toml`; `build/Windows/build.rs` |

`.github/workflows/01-full-release.yml` remains the source release entry
point. It builds native Windows Rust/Slint and the WebGUI GHCR image from
these distinct roots, and the WebGUI source includes migrations, tests,
backend and browser/static assets.

The `build/` root contains **exactly two** immediate directories:
`WEBGui` and `Windows`. No Markdown policy/help guides belong inside
`build/`.

## Shared source dependencies, kept outside both targets

- `schema/media-state-classification.json`: **approved,
  protected** Artist/Album name rankings, status dots and palette;
  bundled into WebGUI and embedded by Windows for offline use.
  Standalone/shared mode does not select a different policy.
- `schema/comp-report-policy.json`: **protected** COMP matching,
  grouping and coordination policy; preserve its accepted working
  baseline while independently verifying detailed runtime parity.
- `schema/media.sql`: SQLite catalog/coordinator DDL.
- `schema/settings-ui.json`: Config v5 settings-presentation contract.
- `build/WEBGui/docker/config.example.toml` and
  `build/WEBGui/docker/compose.yaml`: WebGUI-only deployment inputs.
  The operator guide is [webgui-docker-deployment.md](webgui-docker-deployment.md).
  Release-wide scripts and Windows manifests remain in
  `release/` and `build/Windows/package/`.

These four shared contracts are referenced together in
[POLICY-INDEX.md](POLICY-INDEX.md) but do **not** require separate
copies inside the Windows and WebGUI build roots.

The root [`fixtures/cross-runtime-album-policy.json`](../fixtures/cross-runtime-album-policy.json)
is **test input**, not a third runtime policy or deployable schema. Windows
and WebGUI both test their Album/compilation classification against it.

## Active release boundary

Unsupported root-native Rust/CLI sources are not Workflow 01 release
targets. Only the two designated `build/` application roots are active;
policy and design documentation belong under `docs/`.

The two-source layout is on `dev` within
[draft PR #216](https://github.com/scottia/S-P-L-I-N-E-D/pull/216).
Do not promote to `main` or publish before resolving branch
divergence, policy-review authorization and complete Windows/WebGUI
validation. Public app Help must point only to reviewed `main` docs;
`dev` documentation is for implementation and owner review.

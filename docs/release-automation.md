# Official release automation

Official releases are single-run workflows dispatched from `main`. Operators
do not copy candidate IDs or build run IDs between workflows.

| Workflow | Official output |
| --- | --- |
| `01. Build ALL [Release]` | Windows portable + unsigned MSIX draft, WebGUI source archive, GitHub Release, GHCR image, git-cliff notes, and offline Microsoft Store review bundle. |
| `02. Build Windows [Release]` | Windows portable + unsigned MSIX draft, GitHub Release, git-cliff notes, and offline Microsoft Store review bundle. No GHCR publication. |
| `03. Build WebGUI [Release]` | WebGUI source archive, GitHub Release, GHCR image, and git-cliff notes. No Windows or MSIX work. |

Each workflow automatically performs the same ordered release contract:

1. Pin the exact dispatch commit on `main` and allocate the next available
   numeric release tag. Workflow 20 artifacts and `/dev` candidate versions
   are never reused as official release inputs.
2. Confirm the exact `main` source/version identity and retain
   `splined-official-candidate-report`. This stage creates no MSIX, reruns no
   source tests, and records the internal candidate ID for the same run.
3. Prepare an immutable release commit when the version fields need a
   temporary stamp, then build every selected target from that exact commit.
4. Validate the selected build results and retain
   `splined-official-build-report`. No manual ID handoff is involved.
5. Verify that `main` and the release tag are still unchanged, tag the exact
   built commit, generate incremental notes with pinned git-cliff 2.14.2, and
   publish the GitHub Release.
6. Publish the exact smoke-tested WebGUI image to GHCR when WebGUI is selected.
   When Windows is selected, create the unsigned production MSIX and, after
   publication, retain the offline Microsoft Store draft/review bundle.

The official workflows never submit to Partner Center. The Store result is a
validated local draft artifact for independent review. Existing recovery tools
remain separate from this normal release path.

Workflow 20 remains a `/dev` and PR evaluation path only. Its artifacts are
marked `release_eligible: false` and cannot satisfy any official-release stage.

Source validation belongs to the topic-branch → `dev` cycle. Once that exact
tree is promoted to `main`, official Workflows 01–03 trust the promotion rather
than repeating those suites. Release runs still build the selected production
assets, verify package identity, perform native/container smoke checks, and
require a valid final Build Report before tagging or publication.

Windows and WebGUI releases first identify the current `/dev` head and restore
only compatible compiler/Docker state keyed to that SHA when available. This
accelerates the build without accepting a Workflow 20 artifact: Cargo and
Buildx still evaluate the exact `main` release source, invalidate changed
outputs/layers, and produce newly validated release assets.

Workflows 01–03 do not run a Rust/Slint strict-validation suite. A Windows
selection performs the production release `cargo build`, followed only by the
asset identity, portable/native smoke, MSIX and isolated package validations
required for its Build Report.

Release and Store notes share
[`release/cliff.toml`](../release/cliff.toml) and the checked-in templates.

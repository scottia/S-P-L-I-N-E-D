<p align="center">
  <img src=assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png alt="S:P:L:I:N:E:D" width="820">
</p>

<h2 align="center">SƎARCHABLƎ:PІXƎL:LІNKS:ІDƎNTІFІƎD:NORMALІZƎD:ƎNRІCHƎD:DƎFІNƎD</h2>

## 🎨 What is S:P:L:I:N:E:D?

**S:P:L:I:N:E:D** is a utility for finding and managing Album artwork in a
music library.

It checks the Album, asks several artwork sources for possible covers, compares
the results, and selects artwork against the configured size, shape, source,
and output policy.

Use **Read** mode to review what SPLINED would choose or **Write** mode to save
the selected artwork into the Album folder.

---

## ✨ Highlights

- 🎯 **Chooses one best cover** from available candidates
- 🧠 **Uses MusicBrainz Album information** to confirm the release
- 🖼️ **Checks real image dimensions**, not only provider metadata
- 📐 **Protects geometry** with aspect-ratio-aware crop/resize safety
- 🗂️ **Evaluates local and embedded artwork** before replacement
- 🛡️ **Preserves WebP source artwork** while allowing a static JPEG companion
- 👀 **Read mode** evaluates without modifying Album folders
- ✍️ **Write mode** can install the selected artwork
- 🧪 **Sample output** writes one selected image per Album for review
- 🧭 **Persistent history and bypass state** live in the authoritative SQLite database
- 🗃️ **Tag-identified Select Media database** provides stable Album authority, physical Artist-folder grouping, and fast warm startup in Python/Docker
- ⚙️ **Config driven** — library, scan, cache, credential, and output paths remain configurable
- 🪟 **Windows v4 internal settings** — Config v5 and interface layout no longer require external TOML files
- 💾 **Selective `.spl` backup** — export/import settings, interface state, credentials, SQLite, or diagnostics with optional password protection
- 🐳 **Docker image** provides a Linux/server deployment path
- 🐀 **Ratatui TUI** provides OLED and CHALK interactive views, local `cover.*` preview/resolution, URL-backed candidate preview, and mouse/touch operation
- 📦 **Portable Windows, Linux, and macOS releases** keep application-owned files together

---

## 📦 Installation

Choose the installation method that matches where SPLINED will run.

| Install type | Intended use | Installation files |
| --- | --- | --- |
| **Windows Portable** | Windows desktop / workstation | [`release/README-WINDOWS.txt`](release/README-WINDOWS.txt) |
| **Linux Portable** | Native Linux installation | [`release/README-LINUX.txt`](release/README-LINUX.txt) |
| **macOS Portable** | Native macOS installation | [`release/README-MACOS.txt`](release/README-MACOS.txt) |
| **Docker** | Linux servers, NAS, and container deployments | [`docker/README.md`](docker/README.md) · [`python/Dockerfile`](python/Dockerfile) |

Portable users should download the appropriate archive from the
[latest GitHub release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest),
extract it into the final application directory, and run `splined.exe` on
Windows or `./splined` on Linux/macOS.

Docker image:

```text
ghcr.io/scottia/splined:latest
```

Docker users should start with the [Docker installation guide](docker/README.md)
for Compose, persistent mounts, container paths, and usage.

---

## 📚 Documentation

The versioned documentation under [`docs/`](docs/README.md) is the canonical
public help system.

Start with:

- [Documentation home](docs/README.md)
- [Installation and first run](docs/installation-first-run.md)
- [Config v5 reference](docs/config-v5-reference.md)
- [Python Ratatui TUI](docs/ratatui-tui.md)
- [SPLINED media database](docs/splined-media-database.md)

Windows GUI v3.0.0 Stable opens the documentation home from **Help > Help** and
from the Help button in its About dialog.

The Windows v4 source layout adds a single Launch surface, resizable panel
presets, spectrum panel framing, a warm-cream Light theme, multicolor wordmark,
internal settings, and selective `.spl` backup/restore. The application version,
source-layout generation, and Config v5 schema remain independent identifiers.

The Windows updater is compiled into `splined.exe`. Stable builds discover
verified updater assets from official versioned releases; `dev` builds use the
isolated rolling test channel. Both expose **Help > Check for Update...** and
notify the operator when their channel advances. See
[Windows upgrade](docs/installation-first-run.md#windows-upgrade).

---

## 🧩 Source implementation layout

SPLINED keeps its supported implementations separate:

- [`windows/`](windows/README.md) contains the finalized Windows GUI v3.0.0
  Stable, its Config v5 processing core, and the reproducible Windows build.
- Root [`Cargo.toml`](Cargo.toml), [`Cargo.lock`](Cargo.lock), and [`src/`](src/)
  remain the native command-line implementation used by Linux and macOS.
- [`python/`](python/) is the supported Python/Docker Config v5 implementation.
  Its operational TUI is rendered by Ratatui through `pyratatui`; non-TTY
  execution remains plain CLI. A packaged crossterm/PyO3 extension supplies
  mouse/touch input and terminal-image lifecycle handling missing from the
  published `pyratatui==0.3.0` wheel.

The Windows application version, repository release version, and configuration
schema versions are independent.

---

## 🗃️ Python/Docker Select Media database

Python/Docker and Windows store the persistent Artist/Album read model at:

```text
<scan.cache_dir>/splined.db
```

Default Docker path:

```text
/_cache/splined.db
```

Windows derives the same filename from its browsable Cache directory. The
Rust core owns SQLite access and supplies the embedded GUI with a compact JSON
projection; the GUI does not maintain a second database or recursively rebuild
Select Media before first paint. Set `[scan].sqlite_shared = true` only when
the same physical database is intentionally opened through multiple OS or
network filesystem views. In shared mode Python/Docker owns inventory and
exclusion policy; Windows reloads that inventory while retaining independent
local configuration and read/write runtime updates. Raw UNC cache paths are
supported without requiring a persistent mapped drive.

The first interactive launch inventories Artist/Album folders and reads one
representative audio file per Album (Mutagen in Python, Lofty in Rust). The
build saves resumable SQLite checkpoints and publishes the picker only after
the completed snapshot
validates.

Artist and Album SQL identity is based on MusicBrainz/tag values; filesystem
paths are stored only as current locations and are not SQL identity indexes.
The visible Artist Picker is intentionally folder-based: every Album is grouped
under the first physical directory below the configured library root. Tagged
Album Artist identity remains artwork/search authority, but it does not move a
solo Album into `[Soundtracks]` or duplicate an OST/Various Artists Album under
each credited performer.

Warm launches load the complete picker and stable Album Status from SQLite.
There is no Artist-by-Artist background validation and no color change merely
because an Artist was opened. External tagger/filesystem changes
are reconciled after the explicit Refresh action.

See [SPLINED media database](docs/splined-media-database.md).

When a selected curated compilation is tagged `compilation=1` but its
representative track has no MusicBrainz Album/Release ID, the ordinary scan
automatically routes that Album to per-track embedded-artwork decisions. The
yellow warning remains visible before launch. SPLINED searches the local SQL
cache first, performs bounded Recording-ID recovery only after a local miss,
and offers an operator-reviewed Artist/Track MusicBrainz list when IDs are
missing or the recovered release is unsuitable. A chosen release enters the
same artwork-source preview and approval path used by every Album; Escape
returns to the cached MusicBrainz choices without another request. The current
release is green, inspected alternatives are blue, and reselecting either
restores cached source results without rerunning artwork providers. Each match
also exposes a direct Cover Art Archive URL for Artwork-pane preview. This path
replaces only operator-approved embedded track artwork. LIVE
WRITE progress is durable per track: an interrupted compilation is blue
`Incomplete` in the picker even if the operator leaves before the first
approval, and the next scan skips already verified writes. No folder-level
`cover.*` is created, changed, or removed for this compilation path. See
[Source policies and range types](docs/source-policies-range-types.md).

The MusicBrainz list uses release-group artwork only as a quick visual preview;
its Resolution field is separate, Release-MBID-specific evidence from the best
candidate returned by enabled SPLINED sources. See
[Ratatui TUI: Reading MusicBrainz match evidence](docs/ratatui-tui.md#reading-musicbrainz-match-evidence).

MusicBrainz is available in Artwork Source Priority as release authority backed
by Cover Art Archive image bytes. Amazon Store artwork is also available as an
opt-in, credential-free source; SPLINED normalizes accepted
`m.media-amazon.com/images/I/` primary-image URLs to the untransformed original
before download and preview.

---

## 📏 Artwork size defaults

SPLINED prefers artwork near a practical target rather than simply choosing the
largest file.

| Setting | Size |
| --- | ---: |
| Minimum | 1200 px |
| Ideal | 1800 px |
| Maximum | 2400 px |
| Ladder | 3600 px |

The selected acceptable image closest to the **1800 px ideal** wins first.
Shape, source history, approval state, original source quality, and output
safety also participate.

---

## 🔎 Artwork sources

Current provider support includes:

- 🍎 iTunes / Apple artwork
- 🎨 Fanart.tv
- 🎧 Last.fm
- 🧠 MusicBrainz / Cover Art Archive release-group artwork
- 💿 Cover Art Archive
- 🔵 Deezer
- 🟠 Discogs
- 🛒 Amazon Store original primary images (opt-in)

Sources can be reordered, enabled, or excluded. Source priority breaks otherwise
equal scoring decisions.

---

## 🚦 Read vs Write

### 👀 Read

Read mode can search providers, evaluate artwork, build transient candidate
files, and save samples, but it does **not** modify artwork inside Album folders
or persist a no-cover Album as processed merely because a candidate was found.

### ✍️ Write

Write mode performs the same selection process and may save selected artwork
into the Album directory. Existing artwork can be preserved, retained, or
replaced according to current policy and the operational picker.

---

## 🧪 Samples

When enabled, each resolved Album receives one sample named:

```text
<artists>.<album>.sample.jpg
```

Samples live under the configured cache sample directory and are recreated for
the current operational scan.

---

## 🛡️ Existing artwork and persistent bypass

SPLINED evaluates existing local and embedded artwork before remote replacement.
When a local cover is retained, no unnecessary replacement is written.

The operational picker can save a persistent Album bypass. Bypass state remains
under persistent history authority until explicitly removed.

---

## ⚙️ Configuration

All supported runtimes use **Config v5**:

- [Native/Windows Config v5 example](config.example.toml)
- [Docker Config v5 example](docker/config.example.toml)
- [Config v5 reference](docs/config-v5-reference.md)

Portable native/Windows paths are application-relative by default. Docker uses
container-specific absolute paths while preserving the schema.

---

## ▶️ Basic usage

Show help and configured directories:

```text
splined --help
```

Scan the configured directory:

```text
splined --scan-dir
```

Interactive Python scans use OLED by default. Use `--tui-theme CHALK`, or
`--no-tui` for plain output. See the
[Python Ratatui TUI guide](docs/ratatui-tui.md).

A bare native invocation scans the caller's current directory recursively.
Windows portable users can invoke `splined.exe`.

---

## 🔐 Credentials

Provider credentials are normal provider JSON files under the configured
credential directory. SPLINED applies restrictive user-only filesystem
protection where the selected storage supports it. Additional encryption at
rest may be supplied by the operating system, cloud storage, NAS, or encrypted
volume.

Credential files contain sensitive information and should **never** be
committed to GitHub.

---

## 🧭 Project status

The current stable version is identified by the
[latest GitHub release tag](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).

Artwork discovery, MusicBrainz-assisted resolution, candidate evaluation,
local/embedded comparison, Read/Write scanning, samples, persistent
history/bypass, credential handling, Docker packaging, and portable foundations
are implemented.

---

## ⚠️ Use safely

Write mode changes files in Album directories. Test against a copy, staging
library, backup, or snapshot first.

For backups, preserve `config/`, `credentials/`, and `_cache/splined.db`. The
database contains durable completion, bypass, timeout, and source-selection
state. Diagnostic logs and other candidate/sample cache data remain disposable.

---

## 📄 License

SPLINED is licensed under the [GNU General Public License v3](LICENSE).

Copyright 2010-2026

<p align="center">
  <img src="assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png"
       alt="S:P:L:I:N:E:D"
       width="820"><br>
  <img src="assets/branding/splined-slogan.svg?v=4"
     alt="SƎARCHABLƎ : PІXƎL : LІNKS : ІDƎNTІFІƎD : NORMALІZƎD : ƎNRІCHƎD : DƎFІNƎD"
     width="100%">
</p>

---
## 🎨 What is S:P:L:I:N:E:D?

**S:P:L:I:N:E:D** helps people with organized music libraries find, review,
and manage Album and Artist artwork. It combines local and embedded images with
results from configured artwork providers, verifies the actual image, and
applies the selected identity, size, shape, source, and output policies before
choosing a result.

Use **Read** mode to inspect decisions without installing artwork into the music
library. Use **Write** mode when the configured workflow should be allowed to
save covers, Artist Art, or embedded track artwork.

---

## ✨ Highlights

- 🎯 **Best-cover selection** — compares eligible candidates against the active
  artwork policy instead of simply choosing the largest image.
- 🧠 **MusicBrainz release authority** — uses Album and Recording identity to
  keep artwork attached to the intended release.
- 🖼️ **Real image inspection** — checks downloaded pixels and dimensions rather
  than trusting provider metadata alone.
- 📐 **Safe image preparation** — supports square, fitment, crop, resize, and
  configurable upscale limits while protecting source quality.
- 🗂️ **Local and embedded artwork awareness** — evaluates existing art before
  unnecessary replacement and preserves supported WebP sources.
- 👀 **Read mode** — reviews candidates and reports decisions without installing
  music-library artwork.
- ✍️ **Write mode** — permits configured cover, Artist Art, and embedded-track
  changes; runtime and policy determine when a manual decision is needed.
- 🧪 **Review samples** — can write optional sample images outside Album folders
  for comparison.
- 🛡️ **Persistent history and bypass** — retains completed, incomplete, timeout,
  and operator-bypass state in the runtime's own database.
- 🗃️ **Durable library catalogs** — Windows uses SQLite; Ubuntu WebGUI uses a
  separate PostgreSQL catalog and persistent work queue.
- ⚙️ **Config v5** — keeps library, cache, credential, source, range, and output
  policy configurable across supported runtimes.
- 🪟 **Native Windows application** — available through Microsoft Store or a
  portable ZIP, with Windows-owned settings and SQLite backup tools.
- 🌐 **Ubuntu WebGUI** — provides browser-based review and Docker deployment,
  with PostgreSQL-backed catalog and queue state.
- 💻 **Native Linux/macOS CLI** — remains a separate root Rust implementation;
  artifact availability follows the selected release targets.

---

## 📦 Installation

Choose the runtime and distribution that match where SPLINED will run. Release
assets vary by release, so confirm that the desired artifact is present before
following its portable instructions.

| Install type | Application and persistent state | Start here |
| --- | --- | --- |
| **Microsoft Store** | Native Windows application; Store-managed installation and updates; SQLite | [Install SPLINED](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US) · [Store guide](docs/windows-store.md) |
| **Windows Portable** | Native Windows application in an extracted folder; portable settings; SQLite | [Windows portable instructions](release/README-WINDOWS.txt) · [Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases) |
| **Ubuntu WebGUI / Docker** | Python/browser application; PostgreSQL and persistent mounted files | [Docker deployment](docker/README.md) · [WebGUI guide](docs/browser-workspace-guide.md) |
| **Native Linux** | Separate root Rust command-line implementation when a matching artifact or source build is available | [Linux instructions](release/README-LINUX.txt) · [Installation guide](docs/installation-first-run.md) |
| **Native macOS** | Separate root Rust command-line implementation when selected for a release | [macOS instructions](release/README-MACOS.txt) · [Installation guide](docs/installation-first-run.md) |

After installation, configure the runtime's music-library view, persistent
state, cache, and provider credentials. Build or refresh the catalog, then
start with a small Read-mode selection before enabling Write. See
[Installation and first run](docs/installation-first-run.md).

---

## 📚 Documentation

The documentation index separates current user guides, authoritative technical
references, maintainer procedures, and historical audits:

- [Documentation home](docs/README.md)
- [Installation and first run](docs/installation-first-run.md)
- [Windows application guide](docs/windows-guide.md)
- [Ubuntu WebGUI guide](docs/browser-workspace-guide.md)
- [Config v5 reference](docs/config-v5-reference.md)
- [Support](SUPPORT.md), [security reporting](SECURITY.md), and
  [release history](https://github.com/scottia/S-P-L-I-N-E-D/releases)

---

## 🧩 Source implementation layout

SPLINED keeps its supported implementations separate:

- [`windows/`](windows/README.md) contains the native Rust/Slint Windows
  application, its processing core, and Windows packaging.
- Root [`Cargo.toml`](Cargo.toml), [`Cargo.lock`](Cargo.lock), and [`src/`](src/)
  contain the separate native command-line implementation for Linux/macOS.
- [`webgui/`](webgui/) contains the Python server and browser interface;
  [`docker/`](docker/README.md) contains its PostgreSQL deployment.

They share configuration and artwork-policy concepts, but do not guarantee
identical interfaces or use one shared live database.

---

## 🗃️ Library catalog

Windows keeps its native catalog and durable work state in
`<scan.cache_dir>/splined.db`. Ubuntu WebGUI keeps its catalog, completion
ledger, and persistent queue in PostgreSQL. An explicit WebGUI import can copy
recognized rows from a read-only SQLite snapshot before destination Album rows
exist, but Windows and WebGUI do not open the same live database.

Catalog refreshes reconcile tagged identity with current library locations and
publish the Artist/Album views used for artwork review. Albums without a
MusicBrainz Release ID can enter the runtime's reviewed per-track COMP workflow
without treating a folder cover as embedded track art. See the
[media database guide](docs/splined-media-database.md),
[Windows guide](docs/windows-guide.md), and
[WebGUI guide](docs/browser-workspace-guide.md).

---

## 📏 Artwork size defaults

The checked-in Config v5 examples use this starting range:

| Setting | Size |
| --- | ---: |
| Minimum | 1200 px |
| Ideal | 1800 px |
| Maximum | 2400 px |
| Ladder | 3600 px |

These values are configurable. Source overrides, final-image evaluation, and
runtime settings can change which candidates are eligible; the table is not a
universal scoring rule. See [Config v5](docs/config-v5-reference.md#range) and
[Source policies and Range Types](docs/source-policies-range-types.md).

---

## 🔎 Artwork sources

Recognized artwork and enrichment sources include:

- 🍎 **iTunes / Apple artwork**
- 🎨 **Fanart.tv**
- 🎧 **Last.fm**
- 🧠 **MusicBrainz / Cover Art Archive authority**
- 💿 **Cover Art Archive**
- 🔵 **Deezer**
- 🟠 **Discogs**
- 🛒 **Amazon Store artwork** — opt-in, best-effort discovery
- 🟢 **Navidrome / OpenSubsonic** — supplemental current-server artwork and
  library enrichment, not release authority

Availability depends on the runtime, enabled source order, credentials, and
the current Album identity. See [Credentials and provider setup](docs/credentials-providers.md)
and [source policy](docs/source-policies-range-types.md).

---

## 🚦 Read vs Write

Both modes can inspect the library, query enabled providers, evaluate images,
and use runtime state. Their important difference is permission to change music
artwork.

### 👀 Read

Read mode does not install covers or embedded artwork into the music library.
It can still create or update the runtime's catalog, temporary candidates,
logs, and explicitly enabled sample output. Read reports what the active policy
would choose so paths and decisions can be checked safely.

### ✍️ Write

Write mode permits the configured workflow to save Album covers, Artist Art,
or embedded track thumbnails. Some operating modes present explicit review
steps; others may accept an eligible automatic result under the saved policy.
Review the relevant [Windows](docs/windows-guide.md) or
[WebGUI](docs/browser-workspace-guide.md) workflow before enabling it.

---

## 🧪 Samples

When sample output is enabled, SPLINED can create one review image per resolved
Album using a recognizable name such as:

```text
<artists>.<album>.sample.jpg
```

Samples belong in the configured sample/cache location, not the Album folder,
and can be recreated. See the [Config v5 reference](docs/config-v5-reference.md#samples).

---

## 🛡️ Existing artwork and persistent bypass

SPLINED evaluates supported local and embedded artwork before remote
replacement. An acceptable existing image can remain in place. A saved Album
bypass is durable runtime state and remains active until the operator explicitly
removes it; timeout and incomplete states remain distinct from a successful
completion. See [source policy](docs/source-policies-range-types.md#local-artwork)
and [database state](docs/splined-media-database.md#runtime-state).

---

## ⚙️ Configuration

Supported runtimes use **Config v5** concepts for library paths, scanning,
output, ranges, source policy, samples, credentials, logging, and history.
Windows presents its configuration through native Setup/Settings and keeps
portable or packaged Windows state in the appropriate Windows-owned location.
WebGUI presents its server-visible settings in the browser and keeps PostgreSQL
connection authority with the deployment.

Start with the [Config v5 reference](docs/config-v5-reference.md) and the
checked-in [native](config.example.toml) or
[Docker](docker/config.example.toml) example for the selected runtime.

---

## ▶️ Native Linux/macOS command-line usage

The root Rust project is a separate native command-line implementation. When a
matching artifact or source build is available, begin with:

```text
splined --help
splined --scan-dir
```

Supply the intended Config v5 file and verify its paths before scanning. These
commands do not describe Windows GUI or WebGUI operation. See
[Installation and first run](docs/installation-first-run.md).

---

## 🔐 Credentials

Provider credentials use fixed JSON filenames beneath the configured credential
directory. Keep that directory outside source control, restrict filesystem
access, and never paste keys, tokens, passwords, or credential files into an
issue or diagnostic report. Some sources are anonymous; others require their
own provider setup.

See [Credentials and provider setup](docs/credentials-providers.md) for the
supported provider files, safe tests, relocation, and backup guidance.

---

## 🧭 Project status

The source tree contains the native Windows application, Ubuntu WebGUI, and the
separate root-native command-line implementation. Distribution targets are
selected per release, so the presence of source or instructions does not mean
every release contains every platform artifact.

Use [GitHub Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases) for
published versions and change history. Current behavior is documented in the
[documentation index](docs/README.md).

---

## ⚠️ Use safely

Write mode can change Album-folder images and embedded audio artwork. Test with
a copy, snapshot, staging library, or small selection first, and keep a backup
of the music files before enabling writes.

Also back up the runtime's configuration, credential directory, and persistent
database. Windows uses SQLite and Windows backup/restore tools; WebGUI requires
a consistent PostgreSQL dump plus its persistent mounted files. Temporary
candidates, samples, and logs are not substitutes for those backups. Avoid
simultaneous Windows/WebGUI writers to the same music files.

---

## 📄 License

SPLINED is licensed under the [GNU General Public License v3](LICENSE).

Copyright 2010-2026

<p align="center">
  <img src=assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png alt="S:P:L:I:N:E:D" width="820">
</p>

<h2 align="center">SƎARCHABLƎ:PІXƎL:LІNKS:ІDƎNTІFІƎD:NORMALІZƎD:ƎNRІCHƎD:DƎFІNƎD</h2>

## 🎨 What is S:P:L:I:N:E:D?

**S:P:L:I:N:E:D** is a utility for finding and managing album artwork in a music library.

It checks the album, asks several artwork sources for possible covers, compares the results, and selects artwork against the configured size, shape, source, and output policy.

You can use it in **Read** mode to review what it would choose, or in **Write** mode to save selected artwork into the album folder.

---

## ✨ Highlights

- 🎯 **Chooses one best cover** from available candidates
- 🧠 **Uses MusicBrainz album information** to help confirm the correct release
- 🖼️ **Checks the real downloaded image**, not just provider-reported dimensions
- 📐 **Protects image geometry** with aspect-ratio-aware resize/crop safety
- 🗂️ **Evaluates existing local artwork** before replacing it
- 🛡️ **Preserves WebP source artwork** while allowing a static JPEG companion when required
- 👀 **Read mode** lets you review results without changing album folders
- ✍️ **Write mode** can install selected artwork
- 🧪 **Sample output** saves one chosen image per album for easy review
- 🧭 **Persistent scan history and bypass state** are stored separately from disposable cache data
- ⚙️ **Config driven** — library, scan, cache, credential, and output locations remain configurable
- 🐳 **Docker image** provides a Linux/server deployment path
- 🐀 **Ratatui TUI** provides OLED and CHALK interactive Python scan views while preserving plain automation
- 📦 **Portable Windows, Linux, and macOS releases** keep application-owned files together

---

## 📦 Installation

Choose the installation method that matches where S:P:L:I:N:E:D will run.

| Install type | Intended use | Installation files |
| --- | --- | --- |
| **Windows Portable** | Windows desktop / workstation | [`release/README-WINDOWS.txt`](release/README-WINDOWS.txt) |
| **Linux Portable** | Native Linux installation | [`release/README-LINUX.txt`](release/README-LINUX.txt) |
| **macOS Portable** | Native macOS installation | [`release/README-MACOS.txt`](release/README-MACOS.txt) |
| **Docker** | Linux servers, NAS, and container deployments | [`docker/README.md`](docker/README.md) · [`python/Dockerfile`](python/Dockerfile) |

Portable users should download the appropriate archive from the [latest GitHub release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest), extract it into the final application directory, and run `splined.exe` on Windows or `./splined` on Linux/macOS. No setup-launcher rename step is required.

Docker image:

```text
ghcr.io/scottia/splined:latest
```

Docker users should start with the [Docker installation guide](docker/README.md) for the minimal Compose example, persistent mounts, container paths, and basic usage.

---

## 📚 Documentation

The versioned repository documentation under [`docs/`](docs/README.md) is
the canonical public help system.

Start with:

- [Documentation home](docs/README.md)
- [Installation and first run](docs/installation-first-run.md)
- [Config v5 reference](docs/config-v5-reference.md)

Windows GUI v3.0.0 Stable opens the documentation home from **Help > Help**
and from the Help button in its About dialog.

---

## 🧩 Source implementation layout

S:P:L:I:N:E:D keeps its supported implementations separate:

- [`windows/`](windows/README.md) contains the finalized Windows GUI
  v3.0.0 Stable, its Config v5 processing core, and the reproducible Windows
  build entry point.
- Root [`Cargo.toml`](Cargo.toml), [`Cargo.lock`](Cargo.lock), and [`src/`](src/)
  remain the native command-line implementation used by Linux and macOS. It
  uses Config v5 and the repository release version.
- [`python/`](python/) is the supported Python/Docker Config v5 implementation
  and shares the repository release version. Its interactive operational scan
  UI is rendered by Ratatui through `pyratatui`; non-TTY execution remains the
  plain CLI. A small packaged crossterm/PyO3 input extension supplies
  mouse/touch events and capture lifecycle missing from the published
  `pyratatui==0.3.0` wheel; production images do not contain a Rust toolchain.

The Windows application version, repository release version, and configuration
schema versions are independent.

---

## 📏 Artwork size defaults

S:P:L:I:N:E:D is designed to prefer artwork close to a practical target rather than simply choosing the largest file available.

| Setting | Size |
| --- | ---: |
| Minimum | 1200 px |
| Ideal | 1800 px |
| Maximum | 2400 px |
| Ladder | 3600 px |

The selected acceptable image closest to the **1800 px ideal** wins first. Shape, source history, approval state, and output safety are also considered.

---

## 🔎 Artwork sources

Current provider support includes:

- 🍎 iTunes / Apple artwork
- 🎨 Fanart.tv
- 🎧 Last.fm
- 💿 Cover Art Archive
- 🔵 Deezer
- 🟠 Discogs

Sources can be reordered, enabled, or excluded. Source priority breaks
otherwise equal scoring decisions; configured sources may still be queried
before the final candidate ranking.

---

## 🚦 Read vs Write

### 👀 Read

Use Read mode when you want to inspect results safely.

S:P:L:I:N:E:D can still search providers, evaluate artwork, build disposable cache files, and save selected samples, but it **does not modify artwork inside album folders**.

### ✍️ Write

Write mode performs the same selection process and can save selected artwork into the album directory.

Existing artwork can be preserved, retained, or replaced according to configuration and the operational picker.

---

## 🧪 Samples

When enabled, each resolved album gets one selected sample using the following naming pattern:

```text
<artists>.<album>.sample.jpg
```

Samples are stored under the configured cache sample directory and are recreated for the current operational scan.

---

## 🛡️ Existing artwork and persistent bypass

S:P:L:I:N:E:D evaluates existing local artwork before remote replacement. When a local cover is retained, no unnecessary replacement is written.

The operational picker can also save a persistent album bypass. A saved bypass is stored in persistent history, not disposable cache, and remains active across later runs until explicitly overridden for a run.

---

## ⚙️ Configuration

S:P:L:I:N:E:D uses a TOML config file.

All supported runtimes use **Config v5**:

- [Native/Windows Config v5 example](config.example.toml)
- [Docker Config v5 example](docker/config.example.toml)
- [Config v5 reference](docs/config-v5-reference.md)

Portable native/Windows paths are application-relative by default. Docker uses
container-specific absolute paths while preserving the same schema.

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

Interactive Python scans select the OLED Ratatui theme by default. Use
`--tui-theme CHALK`, or use `--no-tui` to keep plain output in a terminal. See
the [Python Ratatui TUI guide](docs/ratatui-tui.md).

A bare native invocation scans the caller's current directory recursively.

Windows portable users can invoke the executable as `splined.exe`.

---

## 🔐 Credentials

Provider credentials are stored as normal provider JSON files in the configured credential location.

When S:P:L:I:N:E:D creates a credential file, it applies restrictive user-only filesystem protection where the selected storage location supports it. Additional encryption at rest can be provided by the operating system, cloud storage, NAS, encrypted volume, or other storage selected by the user.

Credential files contain sensitive information and should **never be committed to GitHub**.

---

## 🧭 Project status

The current stable S:P:L:I:N:E:D version is identified by the [latest GitHub release tag](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).

Artwork discovery, MusicBrainz-assisted album resolution, candidate evaluation, local-art comparison, Read / Write scanning, samples, persistent bypass/history, credential handling, Docker packaging, and portable application foundations are implemented.

---

## ⚠️ Use safely

Write mode changes files in album directories.

Test against a copy, staging library, backup, or snapshot first. Review selected samples and scan output before using a new configuration against a production library.

---

## 📄 License

S:P:L:I:N:E:D is licensed under the [GNU General Public License v3](LICENSE).

Copyright 2010-2025

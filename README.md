<p align="center">
  <img src=assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png alt="S:P:L:I:N:E:D" width="820">
</p>

  <h2 align="center">SƎARCHABLƎ:PІXƎL:LІNKS:ІDƎNTІFІƎD:NORMALІZƎD:ƎNRІCHƎD:DƎFІNƎD</h2>
  

## 🎨 What is S:P:L:I:N:E:D?

**S:P:L:I:N:E:D** is a small Rust utility for finding and managing album artwork in a music library.

It checks the album, asks several artwork sources for possible covers, compares the results, and picks the image that best matches your preferred size and quality range.

You can use it in **Read** mode to review what it would choose, or in **Write** mode to save the selected artwork into the album folder.

That is the core idea — intentionally simple.

---

## ✨ Highlights

- 🎯 **Chooses one best cover** from all available candidates
- 🧠 **Uses MusicBrainz album information** to help confirm the correct release
- 🖼️ **Checks the real downloaded image**, not just provider-reported dimensions
- 👀 **Read mode** lets you review results without changing album folders
- ✍️ **Write mode** can install the selected artwork
- 🧪 **Sample output** saves one chosen image per album for easy review
- 🛡️ **Preserve mode** can keep existing artwork instead of overwriting it
- ⚙️ **Config driven** — library, scan, cache, credential, and output locations are changeable
- 📦 **Portable Windows, Linux, and macOS releases** keep application-owned files together while paths remain configurable
- 🔁 **Upgrade-safe bootstrap** replaces the program executable while preserving config, cache, and credentials

---

## 📏 Artwork size defaults

S:P:L:I:N:E:D is designed to prefer artwork close to a practical target rather than simply choosing the largest file available.

| Setting | Size |
| --- | ---: |
| Minimum | 1200 px |
| Ideal | 1800 px |
| Maximum | 2400 px |
| Ladder | 3600 px |

The selected acceptable image closest to the **1800 px ideal** wins first. Other qualities such as squareness and source preference help break ties.

---

## 🔎 Artwork sources

Current provider support includes:

- 🍎 iTunes / Apple artwork
- 🎨 Fanart.tv
- 🎧 Last.fm
- 💿 Cover Art Archive
- 🔵 Deezer

Discogs remains available in configuration for future provider work.

Sources can be reordered or excluded in the config.

---

## 🚦 Read vs Write

### 👀 Read

Use Read mode when you want to inspect results safely.

S:P:L:I:N:E:D can still search providers, evaluate artwork, build cache files, and save selected samples, but it **does not modify artwork inside album folders**.

### ✍️ Write

Write mode performs the same selection process and can save the selected artwork into the album directory.

Existing artwork can either be replaced or preserved depending on your configuration.

---

## 🧪 Samples

Samples are an easy way to see what S:P:L:I:N:E:D selected without digging through the candidate cache.

When enabled, each resolved album gets **one selected sample**:

```text
<artist>.<album>.sample.<extension>
```

Example:

```text
10,000 Maniacs.Our Time in Eden.sample.jpg
```

Samples are stored under:

```text
<cache_dir>\samples
```

The samples folder is refreshed at the beginning of each scan run so it represents the current results.

---

## 🛡️ Preserve existing artwork

S:P:L:I:N:E:D can avoid replacing a different existing cover.

With preserve enabled:

```text
cover.jpg
cover-(2).jpg
cover-(3).jpg
```

If the selected image already exists, it is reported as **UNCHANGED** instead of creating another copy.

Preserve can be controlled in the config or for a single run with:

```text
-p, --preserve-file <true|false>
```

---

## ⚙️ Configuration

S:P:L:I:N:E:D uses a TOML config file.

The important idea is that **your directories are yours to choose**. The application should not assume a production library path.

```toml
config_version = 4
mode = "read"
verbosity = "info"

[scan]
cache_dir = "cache"
scan_library_dir = ""

[library]
music_library = ""

[samples]
sample_write = true

[credentials]
credential_dir = "credentials"

[output]
preserve_file = true
file_formats = ["jpeg", "png", "webp"]
file_name = "cover"

[range]
min = 1200
ideal = 1800
max = 2400
ladder = 3600
```

Your real library, test library, cache, and credential paths can all be changed without changing S:P:L:I:N:E:D itself.

---

## ▶️ Basic usage

Show the current help and configured directories:

```powershell
splined.exe --help
```

Scan the configured test / album directory:

```powershell
splined.exe --scan-dir
```

Build from source:

```powershell
cargo build --release
```

Development validation:

```powershell
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

---

## 📦 Portable release

Release archives contain a temporary platform setup launcher plus a platform-specific `README-*.txt` instruction file. The repository homepage `README.md` is not included in release archives.

Run setup from the final application directory. A successful fresh setup installs the permanent executable and creates the portable application-owned directories. A successful upgrade replaces the permanent executable while preserving existing portable data.

After setup completes, the temporary setup launcher and platform instruction file are removed automatically. On Windows, deletion of the running setup executable is completed immediately after that process exits.

The resulting Windows layout is:

```text
splined\
├─ splined.exe
├─ config\
│  └─ config.toml
├─ cache\
│  └─ samples\
└─ credentials\
```

For an upgrade, extract the new archive into the existing SPLINED application directory and run the new setup executable. Setup stages the replacement executable and attempts to restore the previous executable if replacement fails. Existing `config`, `cache`, and `credentials` data is left in place.

S:P:L:I:N:E:D does not automatically discover, import, or move another installation. Move or copy portable data manually when changing application directories.

---

## 🔐 Credentials

Provider credentials are stored as normal provider JSON files in the configured credential location.

When S:P:L:I:N:E:D creates a credential file, it applies restrictive user-only filesystem protection where the selected storage location supports it. Additional encryption at rest can be provided by the operating system, cloud storage, NAS, encrypted volume, or other storage selected by the user.

Credential files contain sensitive information and should **never be committed to GitHub**.

---

## 🐧 Linux / Docker

S:P:L:I:N:E:D ships a native Linux release built from the same Rust engine as Windows and macOS.

Docker or other container deployments can use the Linux build with configuration, cache, credentials, and media exposed through user-selected mounted storage.

---

## 🧭 Project status

S:P:L:I:N:E:D 1.0.0 establishes the portable release foundation.

Artwork discovery, MusicBrainz-assisted album resolution, candidate evaluation, Read / Write scanning, samples, preserve behavior, portable application bootstrap, credential file protection, and native release packaging are implemented.

Current focus:

- 📦 platform-specific release packaging and installer validation
- 🧪 fresh-install and executable-only upgrade acceptance testing
- 🔎 privacy/provenance auditing before public publication

---

## ⚠️ Use safely

Write mode changes files in album directories.

Test against a copy, staging library, backup, or snapshot first. Review the selected samples and scan output before using a new configuration against a production library.

---

## 📄 License

A project license will be finalized before the first public release.

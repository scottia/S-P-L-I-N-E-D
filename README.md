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
- 📦 **Portable Windows, Linux, and macOS releases** keep application-owned files together

---

## 📏 Artwork size defaults

S:P:L:I:N:E:D is designed to prefer artwork close to a practical target rather than simply choosing the largest file available.

| Setting | Size |
| --- | ---: |
| Minimum | 1200 px |
| Ideal | 1800 px |
| Maximum | 2400 px |
| Ladder | 3600 px |

The selected acceptable image closest to the **1800 px ideal** wins first. Shape, source preference, approval state, and output safety are also considered.

---

## 🔎 Artwork sources

Current provider support includes:

- 🍎 iTunes / Apple artwork
- 🎨 Fanart.tv
- 🎧 Last.fm
- 💿 Cover Art Archive
- 🔵 Deezer
- 🟠 Discogs

Sources can be reordered or excluded in the config.

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

When enabled, each resolved album gets one selected sample:

```text
<artist>.<album>.sample.<extension>
```

Example:

```text
10,000 Maniacs.Our Time in Eden.sample.jpg
```

Samples are stored under:

```text
<cache_dir>/samples
```

The disposable cache is prepared for each operational scan and the samples directory is recreated for the current run.

---

## 🛡️ Existing artwork and persistent bypass

S:P:L:I:N:E:D evaluates existing local artwork before remote replacement. When a local cover is retained, no unnecessary replacement is written.

The operational picker can also save a persistent album bypass. A saved bypass is stored in history, not cache, and remains active across later runs until explicitly overridden for a run.

---

## ⚙️ Configuration

S:P:L:I:N:E:D uses a TOML config file.

The application does not assume a production music-library path. Relative portable paths are resolved from the application root; Docker deployments normally use the container paths shown below.

```toml
config_version = 4
mode = "read"
verbosity = "info"

[scan]
cache_dir = "_cache"
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

For Docker, `cache_dir` should normally resolve to `/_cache` and `credential_dir` to `/credentials`.

---

## 🗂️ Runtime data layout

S:P:L:I:N:E:D separates disposable runtime data from persistent state.

| Purpose | Docker path | Lifecycle |
| --- | --- | --- |
| Config | `/config` | Persistent |
| Credentials | `/credentials` | Persistent |
| Cache | `/_cache` | Disposable; cleared/prepared for each operational scan |
| Samples | `/_cache/samples` | Disposable; recreated for the current scan |
| Logs | `/_logs` | Persistent |
| History | `/logs/_history` | Persistent |

The current persistent history files include:

```text
chosen-source-history.json
scan-completed-history.json
bypass-source-history.json
```

Debug output is written independently of cache:

```text
/_logs/splined_debug.log
```

---

## 🐳 Docker — recommended Linux/server deployment

The stable Docker image is published as:

```text
scottia/splined:latest
```

Use `latest` to follow the current stable image. For a reproducible deployment, use the tag shown on the [latest GitHub release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest) and pin the image as `scottia/splined:<release-tag>`.

### Minimal Docker Compose

```yaml
services:
  splined:
    image: scottia/splined:latest
    container_name: splined
    restart: unless-stopped
    environment:
      SPLINED_CONFIG: /config/config.toml
    volumes:
      - /path/to/music:/music:rw
      - /path/to/splined/_cache:/_cache:rw
      - /path/to/splined/_logs:/_logs:rw
      - /path/to/splined/_logs/_history:/logs/_history:rw
      - /path/to/splined/config:/config:rw
      - /path/to/splined/credentials:/credentials:rw
```

The image defaults to:

```text
SPLINED_CONFIG=/config/config.toml
```

Put `config.toml` in the host directory mounted at `/config`. Paths inside the config refer to **container paths**, not host paths.

A typical Docker configuration therefore uses:

```toml
[scan]
cache_dir = "/_cache"

[library]
music_library = "/music"

[credentials]
credential_dir = "/credentials"
```

### Mount semantics

| Container path | Purpose | Typical access |
| --- | --- | --- |
| `/music` | Music library | `ro` for Read mode, `rw` for Write mode |
| `/config` | `config.toml` | `rw` |
| `/_cache` | Disposable candidate/cache data and samples | `rw` |
| `/_logs` | Persistent log files | `rw` |
| `/logs/_history` | Persistent completion/source/bypass history | `rw` |
| `/credentials` | Provider credential JSON files | `rw` |

If S:P:L:I:N:E:D will only run in Read mode, mounting the library read-only is a useful additional safeguard:

```yaml
      - /path/to/music:/music:ro
```

Write mode requires `/music` to be writable.

### Example host application layout

A server deployment can keep all SPLINED-owned data under one application root:

```text
/path/to/splined/
├── config/
├── credentials/
├── docker_builds/
├── _cache/
│   └── samples/
└── _logs/
    ├── splined_debug.log
    └── _history/
        ├── chosen-source-history.json
        ├── scan-completed-history.json
        └── bypass-source-history.json
```

`docker_builds` is host-side organization and does not need to be mounted for normal scanning unless the deployment intentionally exposes it to the container.

### Private Docker Hub authentication

If the Docker Hub repository requires authentication, log in on the Docker host before pulling:

```bash
docker login -u scottia
```

A Docker Hub Personal Access Token is preferred over the account password.

If normal Docker commands use `sudo`, perform the login with the same privilege level so Docker reads the same credential store:

```bash
sudo docker login -u scottia
```

### Common Docker commands

Verify the running container:

```bash
docker compose exec splined splined -V
```

Show help:

```bash
docker compose exec splined splined --help
```

Run the configured operational scan:

```bash
docker compose exec splined splined --scan-dir
```

Follow container output:

```bash
docker compose logs -f splined
```

### One-shot `docker run`

```bash
docker run --rm \
  -v /path/to/music:/music:rw \
  -v /path/to/splined/_cache:/_cache:rw \
  -v /path/to/splined/_logs:/_logs:rw \
  -v /path/to/splined/_logs/_history:/logs/_history:rw \
  -v /path/to/splined/config:/config:rw \
  -v /path/to/splined/credentials:/credentials:rw \
  scottia/splined:latest -V
```

The Docker image starts S:P:L:I:N:E:D in idle mode when no command is supplied, allowing normal CLI commands through `docker compose exec`.

---

## ▶️ Basic usage

Show help and configured directories:

```powershell
splined.exe --help
```

Scan the configured directory:

```powershell
splined.exe --scan-dir
```

A bare native invocation scans the caller's current directory recursively.

Build the native Rust/reference implementation from source:

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

Run setup from the final application directory. A successful fresh setup installs the permanent executable and creates the portable application-owned directories. A successful upgrade replaces the permanent executable while preserving persistent application data.

The portable layout uses the same separation between disposable cache and persistent logs/history:

```text
splined/
├── splined[.exe]
├── config/
│   └── config.toml
├── credentials/
├── _cache/
│   └── samples/
└── _logs/
    └── _history/
```

`_cache` is disposable runtime data. Configuration, credentials, logs, and history are persistent application data.

S:P:L:I:N:E:D does not automatically discover, import, or move another installation. Move or copy portable data manually when changing application directories.

---

## 🔐 Credentials

Provider credentials are stored as normal provider JSON files in the configured credential location.

When S:P:L:I:N:E:D creates a credential file, it applies restrictive user-only filesystem protection where the selected storage location supports it. Additional encryption at rest can be provided by the operating system, cloud storage, NAS, encrypted volume, or other storage selected by the user.

Credential files contain sensitive information and should **never be committed to GitHub**.

---

## 🐧 Linux

For Linux and server deployments, Docker Compose is the recommended starting point. The stable container image uses the validated Python runtime associated with the current GitHub release tag under `python/` and keeps configuration, credentials, persistent logs/history, and media outside the image. Disposable scan cache is mounted separately.

The repository also retains the native Rust/reference implementation and portable release tooling.

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

A project license will be finalized before the first public release.

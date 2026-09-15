<p align="center">
  <img src=assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png alt="S:P:L:I:N:E:D" width="820">
</p>

  <h2 align="center">SƎARCHABLƎ:PІXƎL:LІNKS:ІDƎNTІFІƎD:NORMALІZƎD:ƎNRІCHƎD:DƎFІNƎD</h2>
  

## 🎨 What is S:P:L:I:N:E:D?

**S:P:L:I:N:E:D** is a small utility for finding and managing album artwork in a music library.

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
- 🐳 **Docker image** provides a simple Linux/server deployment path
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
- 🟠 Discogs

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

## 🐳 Docker — recommended Linux/server deployment

The stable Docker image is published as:

```text
scottia/splined:latest
```

Use `latest` to follow the current stable image. For a reproducible deployment, use the tag shown on the [latest GitHub release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest) and pin the image as `scottia/splined:<release-tag>`.

### Minimal Docker Compose

For most Docker hosts, this is the recommended starting point:

```yaml
services:
  splined:
    image: scottia/splined:latest
    container_name: splined
    restart: unless-stopped
    volumes:
      - /path/to/music:/music:rw
      - /path/to/splined/config:/config:rw
      - /path/to/splined/cache:/cache:rw
      - /path/to/splined/credentials:/credentials:rw
```

The image already defaults to:

```text
SPLINED_CONFIG=/config/config.toml
```

Put `config.toml` in the host directory mounted at `/config`. Paths inside that config should refer to the **container paths** such as `/music`, `/cache`, and `/credentials`, not the host-side paths.

### Persistent mounts

| Container path | Purpose | Typical access |
| --- | --- | --- |
| `/music` | Music library | `ro` for Read mode, `rw` for Write mode |
| `/config` | `config.toml` | `rw` |
| `/cache` | cache, history, debug logs, samples | `rw` |
| `/credentials` | provider credential JSON files | `rw` |

If S:P:L:I:N:E:D will only run in Read mode, mounting the library read-only is a useful additional safeguard:

```yaml
      - /path/to/music:/music:ro
```

Write mode requires `/music` to be writable.

### Private Docker Hub authentication

If the Docker Hub repository requires authentication, log in on the Docker host before pulling:

```bash
docker login -u scottia
```

A Docker Hub Personal Access Token is preferred over the account password.

If your normal Docker commands use `sudo`, perform the login with the same privilege level so Docker reads the same credential store:

```bash
sudo docker login -u scottia
```

Then pull and start the service:

```bash
docker compose pull splined
docker compose up -d splined
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

Run a scan using the configured scan directory:

```bash
docker compose exec splined splined --scan-dir
```

Follow container output:

```bash
docker compose logs -f splined
```

Update a `latest` deployment:

```bash
docker compose pull splined
docker compose up -d splined
```

For a pinned deployment, change the image tag in Compose first, then pull and recreate the service.

### Standard hardening / host-permission options

These are optional and depend on the Docker host:

```yaml
services:
  splined:
    image: scottia/splined:latest
    container_name: splined
    restart: unless-stopped
    user: ""
    security_opt:
      - no-new-privileges:true
    environment:
      TZ: ""
      SPLINED_CONFIG: /config/config.toml
    volumes:
      - /path/to/music:/music:rw
      - /path/to/splined/config:/config:rw
      - /path/to/splined/cache:/cache:rw
      - /path/to/splined/credentials:/credentials:rw
```

Use a UID/GID that has the required access to the mounted host directories. `no-new-privileges:true` is appropriate for normal operation and prevents the container process from gaining additional privileges.

### One-shot `docker run`

Compose is the normal deployment method, but the image can also be run directly:

```bash
docker run --rm \
  -v /path/to/music:/music:rw \
  -v /path/to/splined/config:/config:rw \
  -v /path/to/splined/cache:/cache:rw \
  -v /path/to/splined/credentials:/credentials:rw \
  scottia/splined:latest -V
```

The Docker image starts S:P:L:I:N:E:D in idle mode when no command is supplied, which allows `docker compose exec` to be used for normal CLI commands.

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

## 🐧 Linux

For Linux and server deployments, Docker Compose is the recommended starting point. The stable container image uses the validated Python runtime associated with the current GitHub release tag under `python/` and keeps configuration, cache, credentials, and media outside the image through mounted storage.

The repository also retains the native Rust/reference implementation and portable release tooling.

---

## 🧭 Project status

The current stable S:P:L:I:N:E:D version is identified by the [latest GitHub release tag](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).

Artwork discovery, MusicBrainz-assisted album resolution, candidate evaluation, Read / Write scanning, samples, preserve behavior, scan completion timeout/history, credential handling, Docker packaging, and portable application foundations are implemented.

The stable Docker release workflow validates the committed Python source, builds the image, verifies the container version, and publishes both the numbered release tag and `latest`.

---

## ⚠️ Use safely

Write mode changes files in album directories.

Test against a copy, staging library, backup, or snapshot first. Review the selected samples and scan output before using a new configuration against a production library.

---

## 📄 License

A project license will be finalized before the first public release.

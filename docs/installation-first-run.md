# Installation and First Run

This guide covers the supported installation models for S:P:L:I:N:E:D and the first-run checks that should be completed before using Write mode against a production music library.

## Choose an installation model

S:P:L:I:N:E:D currently supports two primary deployment styles:

| Installation | Best for | Runtime |
| --- | --- | --- |
| **Windows portable GUI** | Desktop/workstation use, interactive candidate review, GUI configuration | Native Windows executable |
| **Python / Docker** | Linux servers, NAS systems, unattended or CLI-driven operation | GHCR container image |

The implementations share the same project purpose and many common policies, but they are separate supported runtimes. Do not assume that application version numbers are synchronized across Windows and Python/Docker.

---

# Windows portable installation

## 1. Download the Windows release

Download the current Windows release archive from the project Releases page after the repository is public and the v3.0.0 Stable package is published.

Use the archive supplied by the project. Do not rename an unrelated executable to `splined.exe`.

## 2. Extract into the final application directory

Choose the permanent directory where S:P:L:I:N:E:D should live, then extract the full archive there.

S:P:L:I:N:E:D is designed around a portable application-owned layout. Do not extract the package to one directory, run setup, and then move only the executable elsewhere.

For the current portable release model, setup produces an application layout conceptually like:

```text
SPLINED/
├── splined.exe
├── config/
│   └── config.toml
├── credentials/
├── docker_builds/
├── _cache/
│   └── samples/
└── _logs/
    └── _history/
```

The final v3.0.0 Stable package should be checked against this documented layout before publication.

## 3. Run the supplied setup launcher when present

The existing portable release model uses:

```text
setup-splined.exe
```

Setup installs or replaces the permanent `splined.exe` while preserving application-owned persistent data during upgrades.

After successful setup, use:

```text
splined.exe
```

for normal launches.

See [`../release/README-WINDOWS.txt`](../release/README-WINDOWS.txt) for the current packaging notes. The final v3.0.0 Stable release package remains authoritative if its packaging differs.

---

# Windows first run

## 1. Open Settings before writing artwork

On first launch, confirm at minimum:

- Music Library path
- Config path / portable layout
- Credential directory
- Read vs Write behavior
- Artwork output formats
- Global artwork Resolution Range
- Enabled artwork sources
- Per-source policy overrides, if any
- History / retention behavior
- Bypass / timeout behavior

The Windows GUI uses **Config v5** for the primary application configuration. Appearance/UI-only preferences may be stored separately from Config v5.

See [Config v5 reference](config-v5-reference.md).

## 2. Configure credentials

Credentials are stored separately from `config.toml`.

The configuration identifies the credential directory; provider credential JSON files live beneath that directory.

Do not paste provider secrets directly into public configuration examples or commit credential JSON files to Git.

A typical portable layout is:

```text
credentials/
├── musicbrainz.json
├── lastfm.json
├── fanarttv.json
└── ...
```

Only files for providers that actually require credentials need to exist.

When S:P:L:I:N:E:D creates a credential file, it should apply restrictive user-specific filesystem protection when the selected storage supports it. If the storage location cannot support that protection, use an appropriately private or encrypted storage location.

Detailed provider setup will be documented in the credentials/provider guide.

## 3. Start with a safe library target

Before using Write mode against the entire library, use one of:

- a copied album
- a staging library
- a filesystem snapshot
- a tested backup
- a small artist directory

The goal is to validate provider results, candidate ranking, output format, file naming, and history behavior before large-scale writes.

## 4. Review the library tree

The Windows GUI loads the media-library folder tree and derives row state from the same processing/history/bypass/timeout authority used by execution.

Typical states:

| Color | Meaning |
| --- | --- |
| White | Unprocessed/default |
| Orange | Processed; manually reprocessable |
| Red | Bypassed |
| Purple | Partial artist state or timeout-active album, depending on row/context |
| Green | Artist fully processed |
| Blue | Artist contains bypassed album(s) |

Selecting an Artist normally selects eligible unprocessed albums. Processed albums remain available for deliberate reprocessing, and bypassed albums require explicit bypass confirmation/override.

## 5. Use Media Filter for a small first run

The Media Filter can narrow the in-memory library tree without changing history or filesystem state.

Use:

- Artist text filtering
- Album text filtering
- status/color filters

Filtering should not trigger a new filesystem scan on every keystroke and should not silently change the underlying selected/history/bypass state.

## 6. Launch in the desired mode

The GUI exposes a reusable execution lifecycle:

```text
LAUNCH -> STOP while active -> LAUNCH when complete/stopped
```

Review the active execution mode before launching.

For the first production test, prefer the safest mode that still exercises the intended provider and candidate pipeline.

## 7. Review Scan Activity and Decisions

During execution, inspect the live log for:

- local artwork discovery
- MusicBrainz resolution
- provider queries
- source-policy filtering
- downloaded/evaluated candidate counts
- review-required decisions
- installed/retained artwork outcomes
- bypass or timeout decisions

Do not treat a visually good thumbnail as sufficient evidence by itself; S:P:L:I:N:E:D evaluates the actual downloaded image and the configured source/range/output policy.

## 8. Review candidates before broad Write-mode use

When review is required, inspect candidate source, dimensions, effective Range Type, and final output suitability.

S:P:L:I:N:E:D uses the image short side for Range Type classification.

Default scale:

| Range Type | Short side |
| --- | ---: |
| BelowMinimum | `< 1200` |
| LowerRange | `1200–1799` |
| Ideal | `1800` |
| UpperRange | `1801–2400` |
| Ladder | `2401–3600` |
| AboveLadder | `> 3600` |

See [Config v5 reference](config-v5-reference.md) for policy details.

---

# Docker installation

The container image is published through GHCR:

```text
ghcr.io/scottia/splined:latest
```

A minimal Compose deployment uses persistent mounts for:

```text
/music
/config
/credentials
/_cache
/_logs
```

Example:

```yaml
services:
  splined:
    image: ghcr.io/scottia/splined:latest
    container_name: splined
    security_opt:
      - no-new-privileges:true
    restart: unless-stopped
    volumes:
      - /path/to/music:/music:rw
      - /path/to/splined/config:/config:rw
      - /path/to/splined/credentials:/credentials:rw
      - /path/to/splined/_cache:/_cache:rw
      - /path/to/splined/_logs:/_logs:rw
```

The container uses:

```text
/config/config.toml
```

as its main configuration path by default.

Provider credential JSON files belong under:

```text
/credentials
```

not inside `/config`.

For current Docker details and commands, see [`../docker/README.md`](../docker/README.md).

---

# Docker first run

## 1. Create persistent host directories

Example:

```text
/path/to/splined/
├── config/
├── credentials/
├── _cache/
└── _logs/
```

## 2. Place `config.toml` under the mounted config directory

The host file should resolve inside the container as:

```text
/config/config.toml
```

## 3. Keep credentials separate

The host credential directory should resolve to:

```text
/credentials
```

The config should identify the credential directory rather than embedding provider secrets.

## 4. Validate the container

Show the runtime version:

```bash
docker compose exec splined splined -V
```

Show help/resolved runtime information:

```bash
docker compose exec splined splined --help
```

Validate configuration where the runtime supports it:

```bash
docker compose exec splined splined --config-check
```

## 5. Test a small scan

Run a limited target before scanning the entire library.

Example:

```bash
docker compose exec splined splined --scan-dir "10,000 Maniacs"
```

Follow output:

```bash
docker compose logs -f splined
```

---

# What to back up before first Write-mode use

At minimum, preserve:

- the music library or a filesystem snapshot
- `config/`
- `credentials/`
- `_logs/_history/` if existing history must survive rebuild/reinstall

`_cache/` is intended to be disposable runtime data.

A full backup/portable-layout guide will document recovery and migration scenarios in detail.

---

# Upgrade behavior

## Windows portable

An upgrade should preserve:

```text
config/
credentials/
_logs/
```

while replacing the application executable and recreating disposable cache data as needed.

Do not assume that moving only `splined.exe` moves the installation state.

## Docker

Update the image while keeping persistent host mounts unchanged.

Do not store durable credentials, history, or configuration only inside the container filesystem.

---

# Safety checklist

Before a broad Write-mode run, verify:

- [ ] A backup, copy, or snapshot exists.
- [ ] The intended music-library path is loaded.
- [ ] Config v5 settings are reviewed on Windows.
- [ ] Credential files are stored outside public/source-controlled paths.
- [ ] Enabled providers are intentional.
- [ ] Global Range Type policy is understood.
- [ ] Per-source overrides are intentional.
- [ ] History/retention/bypass/timeout behavior is understood.
- [ ] A small test artist/album has completed successfully.
- [ ] Candidate/output results have been manually reviewed.

Once these checks pass, expand to larger selections.

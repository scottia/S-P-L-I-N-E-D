<p align="center">
  <img src="assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png"
       alt="S:P:L:I:N:E:D"
       width="820">
  <img src="assets/branding/splined-slogan.svg?v=4"
       alt="S:P:L:I:N:E:D"
       width="100%">
</p>

---

## 🎨 What is S:P:L:I:N:E:D?

**S:P:L:I:N:E:D** helps you organize a music library and find, inspect,
and manage Album covers, Artist Art, and embedded track artwork. It compares
existing images with results from configured sources, checks image quality and
MusicBrainz identity, and lets you review a candidate before allowing a
music-file write.

SPLINED currently has **two supported application targets**: a native
**Windows Rust/Slint** application and a **Python/browser WebGUI** deployed
with Docker and PostgreSQL. Their interfaces and storage are distinct; both
follow the same approved artwork and media-state policy.

---

## ✨ Highlights

### 🖼️ Artwork search and quality
- 🎯 **Candidate selection** — evaluates identity, image quality, source
  and configured resolution bands, not just the largest download.
- 🧠 **MusicBrainz guidance** — uses release and recording evidence to
  keep candidate artwork associated with the intended Album or COMP track.
- 🔎 **Real-image inspection** — checks decoded image dimensions and
  source evidence before applying supported output and enhancement options.
- 🗂️ **Existing artwork** — considers local covers, Artist Art and
  embedded thumbnails; animated `cover.webp` has its own indicator.

### 📚 Library and workflow
- 🗃️ **Persistent catalogs** — keeps index, processing and history data
  across application sessions instead of relying on temporary JSON state.
- 🎧 **Album and COMP workflows** — supports ordinary Albums and
  separately reviewed, track-level fallback/curated compilation artwork.
- 🚦 **Clear library states** — uses approved Artist/Album name
  classifications, applicable Album status dots, and independent COMP rows.
- 🛠️ **Config v5** — configures libraries, source preferences, image
  ranges, credentials and output handling.

### 🛡️ Review and control
- 👀 **Inspect before writing** — browsing, candidate previews and
  reports do not themselves install new music-library artwork.
- ✍️ **Intentional writes** — permitted cover, Artist Art and
  track-embedding actions can change files; verify your selection first.
- 🔐 **Separate credentials and backups** — protects provider secrets,
  persistent catalogs and artwork files using runtime-specific storage.

---

## 📦 Installation and access

Choose the application you want to run. **Published release assets may differ
from the current development branch**, so check the release before installing.

| Option | What it runs | Start here |
| --- | --- | --- |
| **Microsoft Store** | Native Windows app; Store-managed updates and SQLite catalog | [Microsoft Store](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US) · [Store guide](docs/windows-store.md) |
| **Windows Portable** | Native Windows app from a release ZIP; persistent SQLite and application settings | [GitHub Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases) · [Portable instructions](release/README-WINDOWS.txt) |
| **WebGUI / Docker** | Browser interface backed by a Python server, PostgreSQL and persistent volumes | [Docker deployment](docs/webgui-docker-deployment.md) · [WebGUI guide](docs/browser-workspace-guide.md) |

### 🚀 First steps

- **Select your music library** in Windows Setup or the WebGUI Settings.
  WebGUI paths must exist on the **server/container**, not just the
  computer or tablet opening the browser.
- **Configure persistent storage.** Windows maintains `splined.db` under
  its configured `scan.cache_dir`; WebGUI uses a separate PostgreSQL
  database, plus persistent configuration and credential volumes.
- **Set up artwork sources** and any required credentials. A provider
  may be enabled without guaranteeing a suitable image.
- **Index or refresh** the library, then open an Artist or Album to inspect
  local artwork, candidates, image quality and the applicable state.
- **Start with review.** Use a small, backed-up selection before allowing
  cover, Artist Art or embedded-track writes.

---

## 📚 Documentation

The guides below describe the **current `dev` application source**.
Some user-interface and cross-client behavior still requires live
acceptance testing; documentation is not a production-certification claim.

### 👤 Installation and everyday use
- [Documentation home and guide index](docs/README.md)
- [Windows application guide](docs/windows-guide.md)
- [WebGUI browser workspace](docs/browser-workspace-guide.md)
- [WebGUI Docker setup](docs/webgui-docker-deployment.md)
- [Windows Store distribution](docs/windows-store.md)

### 🎨 Artwork, settings and providers
- [Config v5 and Settings](docs/config-v5-reference.md)
- [Artwork source policy and Resolution Range Types](docs/source-policies-range-types.md)
- [Provider credentials and connections](docs/credentials-providers.md)
- [Approved Artist/Album classification rules](docs/POLICY-REGISTER.md)

### 🗃️ Catalog, synchronization and recovery
- [Windows SQLite catalog and shared-file operation](docs/splined-media-database.md)
- [SQLite and PostgreSQL field authority](docs/catalog-authority.md)
- [Windows SQLite v2 → v3 upgrade safeguards](docs/windows-schema-upgrade-preparedness.md)

### 🧰 Development and release reference
- [Build/source layout](docs/build-layout.md)
- [Policy and schema source index](docs/POLICY-INDEX.md)
- [Implementation validation and remaining checks](docs/SCHEMA-VALIDATION.md)
- [Workflows 01–03 and release automation](docs/release-automation.md)
- [Repository contribution and PR instructions](AGENTS.md)

---

## 🗃️ How the catalogs work

- **Windows — SQLite:** `<scan.cache_dir>/splined.db` stores the
  authoritative synchronized Artist, Album and Track catalog fields,
  processing history and native indexing evidence.
- **WebGUI — PostgreSQL:** maintains its own browser catalog, jobs,
  queues, audit and WebGUI-owned state. WebGUI does **not** replace
  Windows SQLite as the authority for synchronized fields.
- **Optional coordination:** multiple Windows clients may deliberately
  share one physical SQLite database using the shared/network setting.
  Optional WebGUI/PostgreSQL coordination uses defined field ownership
  and writer evidence; it is not a shared live database file.
- **One media-state policy:** choosing standalone/shared SQLite or
  enabling coordination never creates a separate Artist/Album color
  or COMP policy. Installed policy copies may need deliberate revision
  reconciliation.

See [catalog authority](docs/catalog-authority.md) and
[the media database guide](docs/splined-media-database.md).

---

## 📐 Image sizes and artwork sources

The current Config v5 examples start with these configurable resolution
bands; a provider-specific rule or actual image evidence may change which
candidate is eligible.

| Band | Example default |
| --- | ---: |
| Minimum | 1200 px |
| Ideal | 1800 px |
| Maximum | 2400 px |
| Ladder | 3600 px |

Supported integrations include **MusicBrainz / Cover Art Archive**,
**iTunes / Apple**, **Fanart.tv**, **Last.fm**, **Deezer**, **Discogs**,
**Amazon artwork discovery** and **Navidrome / OpenSubsonic**. Actual
availability depends on configuration, credentials, runtime and
release identity. See [source policies](docs/source-policies-range-types.md)
and [provider setup](docs/credentials-providers.md).

---

## 🧩 Source layout

Workflow 01's current development and release inputs have **two application
roots**; shared policies and release tooling remain outside both.

~~~text
build/
  Windows/          Rust/Slint application, native UI and packaging
  WEBGui/
    webgui/          Python server, browser interface, migrations, tests
    Dockerfile       GHCR image build
    docker/          Compose, environment and Config v5 example
schema/              Shared media-state/COMP JSON, SQLite SQL, Settings UI
release/             Windows portable instructions, release scripts and tests
docs/                User, catalog, configuration and maintainer guides
.github/workflows/   PR validation and numbered release workflows
~~~

The active release workflow is
[01 — Full Release](.github/workflows/01-full-release.yml).
[Workflow 02](.github/workflows/02-store-recovery.yml) handles tagged Store
recovery; [Workflow 03](.github/workflows/03-ghcr-recovery.yml) handles
verified GHCR tag recovery. These are publishing/recovery workflows,
**not** commands to run for documentation review.

---

## ⚠️ Use safely and understand development status

- **Back up music files before writing artwork.** Cover replacement and
  embedded thumbnail changes can modify library content.
- **Back up persistent state separately:** native SQLite, WebGUI
  PostgreSQL, Config v5, credentials and any custom artwork directory.
  A PostgreSQL dump is not a `splined.db` backup.
- **Preserve established policy.** The two protected JSON contracts in
  [`schema/`](schema/) and the [policy register](docs/POLICY-REGISTER.md)
  govern media-state and COMP behavior; a standalone install does not
  define another policy.
- **Do not treat `dev` as a published release.** On-device Windows
  compilation/UI behavior, live synchronization and deployed policy
  revision parity still need their stated validation before promotion.
  Application Help intended for the public uses reviewed `main`
  documentation, not unreleased `dev` revisions.

For published versions see [GitHub Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases).
For assistance see [Support](SUPPORT.md), [Security](SECURITY.md),
[Privacy](PRIVACY.md) and [License](LICENSE).

<p align="center">
  <img src="assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png"
       alt="S:P:L:I:N:E:D"
       width="820"><br>
  <img src="assets/branding/splined-slogan.svg?v=4"
     alt="SƎARCHABLƎ : PІXƎL : LІNKS : ІDƎNTІFІƎD : NORMALІZƎD : ƎNRІCHƎD : DƎFІNƎD"
     width="100%">
</p>

---

## What SPLINED does

**S:P:L:I:N:E:D** helps people with organized music libraries find, review,
and manage album and artist artwork. It combines local and embedded images with
results from configured artwork providers, checks the actual image, and applies
the selected identity, size, shape, source, and output policies before proposing
a result.

SPLINED is designed for deliberate library maintenance. **Read** mode lets you
inspect decisions without writing artwork into the music library. **Write** mode
enables explicit cover, artist-art, or embedded-track changes after review.

## Important capabilities

- Uses MusicBrainz and local tags to keep artwork tied to the intended release.
- Compares real image dimensions and evaluates local or embedded artwork before
  unnecessary replacement.
- Supports configurable source priority, resolution ranges, fitment, retention,
  and output behavior through Config v5.
- Handles ordinary album covers, artist portraits and banners, and reviewed
  per-track artwork for albums without a release ID.
- Keeps durable catalog and work state appropriate to each runtime, including
  resumable review and embedding work where the interface supports it.

The Windows application and Ubuntu WebGUI are separate implementations. They
share policy concepts, but they do not promise identical interfaces or use one
shared live database.

## Install and get started

| Platform | Application and state | Start here |
| --- | --- | --- |
| **Windows** | Native Rust/Slint application, distributed through Microsoft Store or as a portable ZIP; persistent catalog in SQLite | [Installation and first run](docs/installation-first-run.md) · [Windows guide](docs/windows-guide.md) · [Store channel](docs/windows-store.md) |
| **Ubuntu WebGUI** | Python/browser application deployed with Docker; PostgreSQL owns its catalog and persistent queue | [Docker deployment](docker/README.md) · [WebGUI guide](docs/browser-workspace-guide.md) |
| **Native Linux/macOS** | Separate root Rust command-line implementation; availability follows the source and artifacts published for a release | [Installation guide](docs/installation-first-run.md) · [Linux notes](release/README-LINUX.txt) · [macOS notes](release/README-MACOS.txt) |

Choose the guide for the runtime you will actually use. Configure its view of
the music library, persistent state, cache, and provider credentials. Keep those
paths outside source control. Build or refresh the catalog, then use Read mode
with a small selection to confirm identity, candidate order, and destinations.
Enable Write only after the proposed results match your policy.

References to **PC** in maintainer documentation mean the development
workstation. PC is not a SPLINED platform, media server role, or separate
product.

## Safety and documentation

Write mode changes files in album directories and can replace embedded artwork.
Back up or snapshot the music library before enabling it. Also preserve the
runtime's configuration, credential directory, and persistent database. Windows
uses SQLite; WebGUI uses PostgreSQL and its mounted persistent files. They must
be backed up and restored with their own platform procedures.

Detailed behavior lives in the authoritative guides instead of this summary:

- [Documentation home](docs/README.md)
- [Config v5 reference](docs/config-v5-reference.md)
- [Artwork and COMP source policy](docs/source-policies-range-types.md)
- [Persistent state and database boundaries](docs/splined-media-database.md)
- [Credentials and provider setup](docs/credentials-providers.md)
- [Support](SUPPORT.md) and [security reporting](SECURITY.md)
- [Release history](https://github.com/scottia/S-P-L-I-N-E-D/releases) and
  [maintainer release procedures](docs/release-automation.md)

## License

SPLINED is licensed under the [GNU General Public License v3](LICENSE).

Copyright 2010-2026

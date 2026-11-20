<p align="center">
  <img src="assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png"
       alt="S:P:L:I:N:E:D"
       width="820">
  <img src="assets/branding/splined-slogan.svg?v=4"
       alt="S:P:L:I:N:E:D"
       width="100%">
</p>

# S:P:L:I:N:E:D

S:P:L:I:N:E:D is a music artwork and library management application with
a native Windows client and a Python/browser WebGUI.

## Active release-source layout

```text
build/
  WEBGui/
    Dockerfile
    docker/       WebGUI Compose, .env and Config v5 example
    webgui/       Python backend, browser UI, migrations, validation
  Windows/
    Cargo.toml
    src/          Native Rust application
    ui/           Slint interface
    gui/          Windows GUI resources
    package/      Store/portable packaging
schema/           Shared SQL and JSON runtime contracts
release/          Release automation support
.github/workflows/01-full-release.yml
```

Workflow 01 uses `build/WEBGui/Dockerfile` for the published
[GHCR image](https://github.com/scottia/S-P-L-I-N-E-D/pkgs/container/splined)
and `build/Windows/Cargo.toml` for the native Windows executable.
Windows uses its configured `splined.db` SQLite catalog as authority for
synchronized Artist, Album and Track fields; standalone or shared SQLite
is a storage/coordination choice, **not a separate media-state policy**.
The WebGUI uses its own PostgreSQL catalog and operational queues. It does
not replace the authoritative Windows SQLite catalog. Production artifacts
and release history are available under [GitHub Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases).

## Documentation status

Current, owner-approved classification requirements live in the
[policy register](docs/POLICY-REGISTER.md), with source and validation
traceability in the [policy index](docs/POLICY-INDEX.md).
See the [documentation index](docs/README.md),
[WebGUI Docker deployment](docs/webgui-docker-deployment.md), and
[Workflow 01 build layout](docs/build-layout.md) for the active sources.

Application Help intended for public use belongs on `main` after review.

## Project information

[Microsoft Store](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US)
 · [Support](SUPPORT.md)
 · [Security](SECURITY.md)
 · [Privacy](PRIVACY.md)
 · [License](LICENSE)

**Development note:** The restructuring on `dev` has not been declared
a validated production release. Do not promote it to `main` without
the required build and test evidence.

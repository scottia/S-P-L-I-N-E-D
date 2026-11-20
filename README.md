<p align="center">
  <img src="assets/branding/2CD0B55C-328E-48AF-A1A5-DD11147C9977.png"
       alt="S:P:L:I:N:E:D"
       width="820">
  <img src="assets/branding/splined-slogan.svg?v=4"
       alt="S:P:L:I:N:E:D"
       width="100%">
</p>

# S:P:L:I:N:E:D

Cross-platform music artwork and library management, with a native Windows
application and a Python/browser WebGUI.

## Active release-source layout

```text
build/
  WEBGui/
    Dockerfile
    webgui/       Python backend, browser UI, migrations, validation
  Windows/
    Cargo.toml
    src/          Native Rust application
    ui/           Slint interface
    gui/          Windows GUI resources
    package/      Store/portable packaging
schema/           Shared SQL and JSON runtime contracts
docker/           WebGUI deployment definition
release/          Release automation support
.github/workflows/01-full-release.yml
```

Workflow 01 uses `build/WEBGui/Dockerfile` for the published
[GHCR image](https://github.com/scottia/S-P-L-I-N-E-D/pkgs/container/splined)
and `build/Windows/Cargo.toml` for Windows. Production artifacts and
release history are available under [GitHub Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases).

## Documentation status

The source tree is undergoing structural consolidation. **Historical guides
and behavioral policy Markdown are not currently approved as an application
policy.** See [documentation status](docs/README.md) and the
[verified build layout](docs/build-layout.md).

The complete original tree and withdrawn historical documents remain on
[`dev-archive`](https://github.com/scottia/S-P-L-I-N-E-D/tree/dev-archive).
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

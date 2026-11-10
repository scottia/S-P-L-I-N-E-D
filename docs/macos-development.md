# macOS development branch

macOS work is isolated on `dev-macos`. It is experimental, unsupported, and
not part of SPLINED's versioned release workflow. The normal `main`/`dev`
release workflow does not offer a macOS target and does not publish macOS
assets.

The branch-specific **macOS Development - dev-macos** workflow runs on
`macos-latest` for pushes and pull requests targeting `dev-macos`, and it can
also be started manually from that branch. It performs:

1. Rust formatting validation.
2. Clippy with warnings denied.
3. The locked root-native Rust test suite.
4. An optimized locked build and command-line startup smoke test.
5. A short-lived Actions artifact containing `splined` and the experimental
   macOS instructions.

The workflow has read-only repository permissions. It does not create or move
tags, publish GitHub Releases, push container images, contact Microsoft Store,
or change `main` or `dev`.

## Local development

Install stable Rust with `rustfmt` and `clippy`, then run:

```text
cargo +stable fmt -- --check
cargo +stable clippy --locked --all-targets -- -D warnings
cargo +stable test --locked
cargo +stable build --locked --release
./target/release/splined --help
```

Use only backed-up or disposable media while testing write operations. The
remaining macOS implementation is planned for removal from SPLINED by December
31, 2026 unless active maintainers establish a reviewed path forward. Apple
developers interested in testing and developing this branch are welcome to ask
about joining the SPLINED project team.

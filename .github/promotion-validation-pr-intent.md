# PR intent: exact dev-to-main promotion validation

This draft will make the exact same-repository `dev` to `main` promotion run a fast provenance check instead of repeating the source and audio-integrity suites that already passed before merge to `dev`.

Feature pull requests and Workflow 05 candidate validation remain subject to the full validation workflow. No application behavior, protected policy JSON, release metadata, or branch contents are changed by this intent commit.

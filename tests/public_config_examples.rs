use std::fs;

use splined::config::{CURRENT_CONFIG_VERSION, parse_config};

const REQUIRED_SECTIONS: &[&str] = &[
    "library",
    "scan",
    "output",
    "range",
    "sources",
    "source_policies",
    "samples",
    "credentials",
    "logging",
    "history",
    "aisplined",
];

const POLICY_FIELDS: &[&str] = &[
    "enabled",
    "source_override",
    "minimum_range_type",
    "allow_below_minimum_fallback",
    "primary_image_only",
];

fn load_example(path: &str, expected_credential_dir: &str) -> toml::Value {
    let text = fs::read_to_string(path).expect("public configuration example should be readable");
    let value: toml::Value =
        toml::from_str(&text).expect("public configuration example should be valid TOML");

    assert_eq!(
        value["config_version"].as_integer(),
        Some(i64::from(CURRENT_CONFIG_VERSION)),
        "unexpected config version in {path}"
    );
    assert_eq!(
        value["credentials"]["credential_dir"].as_str(),
        Some(expected_credential_dir),
        "unexpected credential directory in {path}"
    );

    for section in REQUIRED_SECTIONS {
        assert!(
            value
                .get(*section)
                .and_then(toml::Value::as_table)
                .is_some(),
            "{path} is missing [{section}]"
        );
    }

    for provider in [
        "deezer",
        "itunes",
        "fanarttv",
        "lastfm",
        "coverartarchive",
        "discogs",
    ] {
        let policy = value["source_policies"][provider]
            .as_table()
            .unwrap_or_else(|| panic!("{path} is missing source policy {provider}"));
        for field in POLICY_FIELDS {
            assert!(
                policy.contains_key(*field),
                "{path} source policy {provider} is missing {field}"
            );
        }
    }
    let musicbrainz = value["source_policies"]["musicbrainz"]
        .as_table()
        .expect("MusicBrainz policy should exist");
    assert_eq!(musicbrainz.len(), 2, "MusicBrainz is metadata-only");
    assert!(musicbrainz.contains_key("enabled"));
    assert!(musicbrainz.contains_key("source_override"));

    for forbidden in [
        "credential_file =",
        "token_file =",
        "access_token =",
        "refresh_token =",
        "api_key =",
        "shared_secret =",
        "client_secret =",
    ] {
        assert!(
            !text.contains(forbidden),
            "{path} must not publish {forbidden}"
        );
    }

    parse_config(&text).unwrap_or_else(|error| panic!("{path} must parse as Config v5: {error}"));
    value
}

#[test]
fn all_public_examples_are_config_v5_and_secret_free() {
    assert_eq!(CURRENT_CONFIG_VERSION, 5);
    let python = fs::read_to_string("python/splined.py").expect("Python runtime source");
    assert!(python.contains("CONFIG_VERSION = 5"));
    assert!(!python.contains("CONFIG_VERSION = 4"));
    load_example("config.example.toml", "credentials");
    load_example("windows/config.example.toml", "credentials");
    let docker = load_example("docker/config.example.toml", "/credentials");
    assert_eq!(docker["library"]["music_library"].as_str(), Some("/music"));
    assert_eq!(docker["scan"]["cache_dir"].as_str(), Some("/_cache"));
    assert_eq!(docker["scan"]["log_dir"].as_str(), Some("/_logs"));
}

#[test]
fn windows_identity_and_release_domains_stay_separate() {
    let manifest = fs::read_to_string("windows/Cargo.toml").expect("Windows manifest");
    let manifest: toml::Value = toml::from_str(&manifest).expect("Windows manifest TOML");
    assert_eq!(manifest["package"]["version"].as_str(), Some("3.0.0"));

    let release_info = fs::read_to_string("windows/gui/ReleaseInfo.cs").expect("ReleaseInfo.cs");
    assert!(release_info.contains("NumericVersion = \"3.0.0\""));
    assert!(release_info.contains("Channel = \"Stable\""));
    assert!(release_info.contains("HelpUrl = RepositoryUrl + \"/blob/main/docs/README.md\""));

    let workflow =
        fs::read_to_string(".github/workflows/release-next-patch.yml").expect("release workflow");
    assert!(workflow.contains("git diff --exit-code -- windows/Cargo.toml windows/Cargo.lock"));
    assert!(!workflow.contains("cargo set-version --manifest-path windows/Cargo.toml"));
    assert!(workflow.contains("ref: ${{ needs.prepare-release.outputs.commit }}"));
    assert!(workflow.contains("packages: write"));

    let ghcr_job = workflow
        .split("  publish-ghcr:")
        .nth(1)
        .expect("publish-ghcr job")
        .split("  release-summary:")
        .next()
        .expect("publish-ghcr job body");
    assert!(ghcr_job.contains("always() &&"));
    assert!(ghcr_job.contains("needs.publish-release.result == 'success'"));

    for platform in ["Windows", "Ubuntu", "macOS", "All"] {
        assert!(workflow.contains(&format!("- \"{platform}\"")));
    }
}

#[test]
fn current_public_docs_do_not_claim_config_v4_runtime() {
    let forbidden = [
        "Python/Docker Config v4",
        "root native Config v4",
        "native Config v4",
        "currently requires Config v4",
        "currently uses Config v4",
        "remains Config v4",
    ];
    for path in [
        "README.md",
        "docs/README.md",
        "docs/config-v5-reference.md",
        "docs/credentials-providers.md",
        "docs/musicbrainz-oauth.md",
        "docs/installation-first-run.md",
        "docker/README.md",
        "windows/README.md",
    ] {
        let text = fs::read_to_string(path).unwrap_or_else(|_| panic!("missing {path}"));
        for phrase in forbidden {
            assert!(
                !text.contains(phrase),
                "{path} contains stale current-runtime claim: {phrase}"
            );
        }
    }
}

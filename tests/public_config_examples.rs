use std::fs;

fn load_example(path: &str, expected_version: i64, expected_credential_dir: &str) {
    let text = fs::read_to_string(path).expect("public configuration example should be readable");
    let value: toml::Value =
        toml::from_str(&text).expect("public configuration example should be valid TOML");

    assert_eq!(
        value["config_version"].as_integer(),
        Some(expected_version),
        "unexpected config version in {path}"
    );
    assert_eq!(
        value["credentials"]["credential_dir"].as_str(),
        Some(expected_credential_dir),
        "unexpected credential directory in {path}"
    );

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
}

#[test]
fn public_config_examples_are_valid_and_secret_free() {
    load_example("config.example.toml", 5, "credentials");
    load_example("docker/config.example.toml", 4, "/credentials");
}

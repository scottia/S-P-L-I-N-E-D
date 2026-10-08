use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use std::env;
use std::fs;

fn required_path(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required for release signature validation"))
}

fn decode_signed_text(value: &str, label: &str) -> String {
    let decoded = STANDARD
        .decode(value.trim())
        .unwrap_or_else(|error| panic!("invalid base64 {label}: {error}"));
    String::from_utf8(decoded).unwrap_or_else(|error| panic!("invalid UTF-8 {label}: {error}"))
}

fn verify_encoded_signature(
    artifact: &[u8],
    encoded_signature: &str,
    encoded_public_key: &str,
) -> Result<(), String> {
    let public_key = PublicKey::decode(&decode_signed_text(
        encoded_public_key,
        "updater public key",
    ))
    .map_err(|error| format!("invalid updater public key: {error}"))?;
    let signature = Signature::decode(&decode_signed_text(encoded_signature, "updater signature"))
        .map_err(|error| format!("invalid updater signature: {error}"))?;
    public_key
        .verify(artifact, &signature, true)
        .map_err(|error| format!("updater signature verification failed: {error}"))
}

#[test]
fn altered_update_package_fails_closed() {
    let public_key = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    let signature = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
    let encoded_public_key = STANDARD.encode(public_key);
    let encoded_signature = STANDARD.encode(signature);

    verify_encoded_signature(b"test", &encoded_signature, &encoded_public_key)
        .expect("known updater signature should verify");
    assert!(
        verify_encoded_signature(b"tampered", &encoded_signature, &encoded_public_key).is_err(),
        "a modified updater package must never pass signature verification"
    );
}

#[test]
#[ignore = "requires the final release artifact and updater key"]
fn verifies_release_updater_signature() {
    let artifact_path = required_path("SPLINED_UPDATE_ARTIFACT");
    let signature_path = required_path("SPLINED_UPDATE_SIGNATURE");
    let public_key = required_path("SPLINED_UPDATER_PUBLIC_KEY");

    let artifact = fs::read(&artifact_path)
        .unwrap_or_else(|error| panic!("unable to read update artifact {artifact_path}: {error}"));
    let encoded_signature = fs::read_to_string(&signature_path).unwrap_or_else(|error| {
        panic!("unable to read update signature {signature_path}: {error}")
    });
    verify_encoded_signature(&artifact, &encoded_signature, &public_key)
        .unwrap_or_else(|error| panic!("invalid update signature {signature_path}: {error}"));
}

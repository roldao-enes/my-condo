//! Ed25519 signing and verification of manifests.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
pub use ed25519_dalek::VerifyingKey;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};

use crate::manifest::{Manifest, SCHEMA};
use crate::UpdateError;

/// The public keys compiled into this build (`update-public-key.txt`). Empty means this build
/// does not check for updates.
pub fn embedded_keys() -> Vec<VerifyingKey> {
    parse_public_keys(include_str!("../update-public-key.txt")).unwrap_or_default()
}

/// One base64 key per line; blank lines and `#` comments are ignored.
pub fn parse_public_keys(text: &str) -> Result<Vec<VerifyingKey>, UpdateError> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let bytes = STANDARD
                .decode(line)
                .map_err(|e| UpdateError::InvalidKey(e.to_string()))?;
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| UpdateError::InvalidKey("a key is 32 bytes".to_owned()))?;
            VerifyingKey::from_bytes(&bytes).map_err(|e| UpdateError::InvalidKey(e.to_string()))
        })
        .collect()
}

/// Checks `signature` (base64) over the exact `manifest_bytes` against every key, and only then
/// parses them. Nothing in the manifest is looked at before the signature is accepted.
pub fn open(
    manifest_bytes: &[u8],
    signature: &str,
    keys: &[VerifyingKey],
) -> Result<Manifest, UpdateError> {
    if keys.is_empty() {
        return Err(UpdateError::NoKeys);
    }
    let signature = STANDARD
        .decode(signature.trim())
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or(UpdateError::BadSignature)?;
    if !keys
        .iter()
        .any(|key| key.verify(manifest_bytes, &signature).is_ok())
    {
        return Err(UpdateError::BadSignature);
    }
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| UpdateError::Malformed(e.to_string()))?;
    if manifest.schema > SCHEMA {
        return Err(UpdateError::UnsupportedSchema {
            found: manifest.schema,
            supported: SCHEMA,
        });
    }
    Ok(manifest)
}

/// A new key pair: the private key as base64 (keep it secret) and the public key as base64.
pub fn generate_keypair() -> (String, String) {
    let key = SigningKey::generate(&mut rand_core::OsRng);
    (
        STANDARD.encode(key.to_bytes()),
        STANDARD.encode(key.verifying_key().to_bytes()),
    )
}

/// Signs `bytes` with a private key from [`generate_keypair`]; returns the base64 signature.
pub fn sign(private_key: &str, bytes: &[u8]) -> Result<String, UpdateError> {
    let invalid = |why: &str| UpdateError::InvalidPrivateKey(why.to_owned());
    let seed = STANDARD
        .decode(private_key.trim())
        .map_err(|_| invalid("not base64"))?;
    let seed: [u8; 32] = seed.try_into().map_err(|_| invalid("a key is 32 bytes"))?;
    let signature = SigningKey::from_bytes(&seed).sign(bytes);
    Ok(STANDARD.encode(signature.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str =
        r#"{"schema":1,"version":"0.2.0","published":"2026-12-01T00:00:00Z","assets":[]}"#;

    fn keys(public: &str) -> Vec<VerifyingKey> {
        parse_public_keys(public).unwrap()
    }

    #[test]
    fn a_signed_manifest_opens() {
        let (private, public) = generate_keypair();
        let signature = sign(&private, MANIFEST.as_bytes()).unwrap();
        let manifest = open(MANIFEST.as_bytes(), &signature, &keys(&public)).unwrap();
        assert_eq!(manifest.version, "0.2.0");
    }

    #[test]
    fn any_change_to_the_manifest_breaks_the_signature() {
        let (private, public) = generate_keypair();
        let signature = sign(&private, MANIFEST.as_bytes()).unwrap();
        let tampered = MANIFEST.replace("0.2.0", "9.9.9");
        assert!(matches!(
            open(tampered.as_bytes(), &signature, &keys(&public)),
            Err(UpdateError::BadSignature)
        ));
        // Even a trailing newline: the signature covers the exact bytes.
        let padded = format!("{MANIFEST}\n");
        assert!(matches!(
            open(padded.as_bytes(), &signature, &keys(&public)),
            Err(UpdateError::BadSignature)
        ));
    }

    #[test]
    fn a_signature_from_another_key_is_refused() {
        let (_, trusted) = generate_keypair();
        let (attacker_private, _) = generate_keypair();
        let forged = sign(&attacker_private, MANIFEST.as_bytes()).unwrap();
        assert!(matches!(
            open(MANIFEST.as_bytes(), &forged, &keys(&trusted)),
            Err(UpdateError::BadSignature)
        ));
    }

    #[test]
    fn garbage_signatures_are_refused_not_panicked_on() {
        let (_, public) = generate_keypair();
        for garbage in ["", "not base64!!", "AAAA", &"A".repeat(200)] {
            assert!(matches!(
                open(MANIFEST.as_bytes(), garbage, &keys(&public)),
                Err(UpdateError::BadSignature)
            ));
        }
    }

    #[test]
    fn no_configured_key_means_nothing_is_trusted() {
        let (private, _) = generate_keypair();
        let signature = sign(&private, MANIFEST.as_bytes()).unwrap();
        assert!(matches!(
            open(MANIFEST.as_bytes(), &signature, &[]),
            Err(UpdateError::NoKeys)
        ));
    }

    #[test]
    fn a_rotated_key_list_accepts_either_key() {
        let (old_private, old_public) = generate_keypair();
        let (new_private, new_public) = generate_keypair();
        let both = keys(&format!("# old\n{old_public}\n\n{new_public}\n"));
        for private in [old_private, new_private] {
            let signature = sign(&private, MANIFEST.as_bytes()).unwrap();
            assert!(open(MANIFEST.as_bytes(), &signature, &both).is_ok());
        }
    }

    #[test]
    fn a_newer_manifest_format_is_reported_as_unsupported() {
        let (private, public) = generate_keypair();
        let future = MANIFEST.replace("\"schema\":1", "\"schema\":2");
        let signature = sign(&private, future.as_bytes()).unwrap();
        assert!(matches!(
            open(future.as_bytes(), &signature, &keys(&public)),
            Err(UpdateError::UnsupportedSchema { found: 2, .. })
        ));
    }

    #[test]
    fn bad_keys_are_rejected() {
        assert!(parse_public_keys("not a key").is_err());
        assert!(parse_public_keys("AAAA").is_err());
        assert!(sign("AAAA", b"x").is_err());
    }

    #[test]
    fn the_shipped_key_file_parses() {
        // Empty until the maintainer adds a key; it must always be valid.
        parse_public_keys(include_str!("../update-public-key.txt")).unwrap();
    }
}

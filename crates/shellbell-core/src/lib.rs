//! Domain primitives that do not depend on HTTP or storage.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{Rng, distr::Alphanumeric};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const PAIRING_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

pub fn random_token(prefix: &str) -> String {
    let bytes: [u8; 32] = rand::random();
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes))
}

pub fn random_id() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

pub fn pairing_code() -> String {
    let mut rng = rand::rng();
    let chars: String = (0..8)
        .map(|_| PAIRING_ALPHABET[rng.random_range(0..PAIRING_ALPHABET.len())] as char)
        .collect();
    format!("{}-{}", &chars[..4], &chars[4..])
}

pub fn hash_secret(secret: &str) -> String {
    hex_digest(Sha256::digest(secret.as_bytes()).as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn verify_secret(secret: &str, expected_hash: &str) -> bool {
    let actual = hash_secret(secret);
    actual.as_bytes().ct_eq(expected_hash.as_bytes()).into()
}

pub fn redact_secret(value: &str) -> String {
    let prefix: String = value.chars().take(7).collect();
    format!("{prefix}…[redacted]")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_are_unique_and_prefixed() {
        let a = random_token("sb_");
        let b = random_token("sb_");
        assert!(a.starts_with("sb_"));
        assert_ne!(a, b);
        assert!(verify_secret(&a, &hash_secret(&a)));
    }
    #[test]
    fn pairing_codes_are_unambiguous() {
        for _ in 0..100 {
            let c = pairing_code();
            assert_eq!(c.len(), 9);
            assert_eq!(&c[4..5], "-");
            assert!(!c.contains(['0', '1', 'I', 'O']));
        }
    }
    #[test]
    fn redaction_never_contains_full_value() {
        let s = "sb_src_really-secret";
        assert!(!redact_secret(s).contains(s));
    }
}

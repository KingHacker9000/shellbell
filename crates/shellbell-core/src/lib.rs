//! Domain primitives that do not depend on HTTP or storage.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{Rng, distr::Alphanumeric};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const PAIRING_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const PASSWORD_KDF_ITERATIONS: u32 = 600_000;
const PASSWORD_SALT_BYTES: usize = 16;
const SHA256_BYTES: usize = 32;
const SHA256_BLOCK_BYTES: usize = 64;

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

pub fn hash_password(password: &str) -> String {
    let salt: [u8; PASSWORD_SALT_BYTES] = rand::random();
    let derived = pbkdf2_sha256(password.as_bytes(), &salt, PASSWORD_KDF_ITERATIONS);
    format!(
        "pbkdf2-sha256${}${}${}",
        PASSWORD_KDF_ITERATIONS,
        URL_SAFE_NO_PAD.encode(salt),
        URL_SAFE_NO_PAD.encode(derived)
    )
}

pub fn verify_password(password: &str, encoded: &str) -> bool {
    let mut parts = encoded.split('$');
    if parts.next() != Some("pbkdf2-sha256") {
        return false;
    }
    let Some(iterations) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
        return false;
    };
    if !(100_000..=1_000_000).contains(&iterations) {
        return false;
    }
    let Some(salt) = parts
        .next()
        .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
    else {
        return false;
    };
    let Some(expected) = parts
        .next()
        .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
    else {
        return false;
    };
    if parts.next().is_some() || salt.len() < 8 || expected.len() != SHA256_BYTES {
        return false;
    }
    let actual = pbkdf2_sha256(password.as_bytes(), &salt, iterations);
    actual.as_slice().ct_eq(expected.as_slice()).into()
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; SHA256_BYTES] {
    debug_assert!(iterations > 0);
    let mut initial = Vec::with_capacity(salt.len() + 4);
    initial.extend_from_slice(salt);
    initial.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password, &initial);
    let mut derived = u;
    for _ in 1..iterations {
        u = hmac_sha256(password, &u);
        for (target, value) in derived.iter_mut().zip(u) {
            *target ^= value;
        }
    }
    derived
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; SHA256_BYTES] {
    let mut key_block = [0u8; SHA256_BLOCK_BYTES];
    if key.len() > SHA256_BLOCK_BYTES {
        let digest = Sha256::digest(key);
        key_block[..SHA256_BYTES].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0x36u8; SHA256_BLOCK_BYTES];
    let mut outer_pad = [0x5cu8; SHA256_BLOCK_BYTES];
    for index in 0..SHA256_BLOCK_BYTES {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }

    let inner = Sha256::new()
        .chain_update(inner_pad)
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(outer_pad)
        .chain_update(inner)
        .finalize()
        .into()
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
    fn password_kdf_matches_known_sha256_vector() {
        assert_eq!(
            hex_digest(&pbkdf2_sha256(b"password", b"salt", 1)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }
    #[test]
    fn password_hashes_are_salted_and_verified() {
        let first = hash_password("a memorable owner password");
        let second = hash_password("a memorable owner password");
        assert_ne!(first, second);
        assert!(verify_password("a memorable owner password", &first));
        assert!(!verify_password("wrong password", &first));
        assert!(!verify_password("anything", "malformed"));
    }
    #[test]
    fn redaction_never_contains_full_value() {
        let s = "sb_src_really-secret";
        assert!(!redact_secret(s).contains(s));
    }
}

//! Security helpers (invite codes, creator keys, constant-time compare)

use rand::Rng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Generate a human-friendly invite code (e.g. "7K2P-9QXH").
/// - Uppercase only
/// - Excludes confusing chars (O/0, I/1, etc.)
pub fn generate_invite_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();

    let mut raw = String::with_capacity(8);
    for _ in 0..8 {
        let idx = rng.random_range(0..CHARSET.len());
        raw.push(CHARSET[idx] as char);
    }

    format!("{}-{}", &raw[0..4], &raw[4..8])
}

/// Generate a random creator key (host secret). Returned ONCE to the host.
pub fn generate_creator_key() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();

    (0..32)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Generate a random salt (hex) for hashing secrets.
pub fn generate_salt_hex() -> String {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    hex::encode(bytes)
}

/// Hash `secret` with a `salt_hex` using SHA-256.
/// Output is hex-encoded.
pub fn hash_secret_sha256_hex(secret: &str, salt_hex: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt_hex.as_bytes());
    hasher.update(b":");
    hasher.update(secret.as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)
}

/// Constant-time equality for hex strings.
pub fn ct_eq_hex(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

//! One-time recovery codes: what a user types at the login challenge when
//! their phone is gone. They're shown once, stored hashed (SHA-256: the
//! codes are random, so a slow password hash isn't needed), and each works
//! once.

use sha2::{Digest, Sha256};

/// How many codes a user gets.
pub const COUNT: usize = 8;

/// Letters and digits that can't be mistaken for each other (no `0`/`o`,
/// `1`/`l`/`i`).
const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// `COUNT` new codes, such as `k7mqp-x2ndr`: ten random characters, about
/// 49 bits each.
pub fn generate() -> Vec<String> {
    (0..COUNT)
        .map(|_| {
            let chars: String = (0..10)
                .map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char)
                .collect();
            format!("{}-{}", &chars[..5], &chars[5..])
        })
        .collect()
}

/// A typed code as it's stored: lowercase, without spaces or dashes, then
/// hashed.
pub fn hash(code: &str) -> String {
    let normal: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let digest = Sha256::digest(normal.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `typed` looks like a recovery code rather than a six-digit code.
pub fn looks_like_one(typed: &str) -> bool {
    typed.chars().filter(|c| c.is_ascii_alphanumeric()).count() == 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_readable() {
        let codes = generate();
        assert_eq!(codes.len(), COUNT);
        let mut unique = codes.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), COUNT);
        for code in &codes {
            assert_eq!(code.len(), 11, "{code}");
            assert_eq!(&code[5..6], "-");
            assert!(looks_like_one(code));
        }
        assert!(!looks_like_one("123456"));
    }

    #[test]
    fn typing_is_forgiving() {
        assert_eq!(hash("k7mqp-x2ndr"), hash(" K7MQP X2NDR "));
        assert_ne!(hash("k7mqp-x2ndr"), hash("k7mqp-x2nds"));
    }
}

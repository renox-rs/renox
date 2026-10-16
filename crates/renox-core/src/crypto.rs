use anyhow::{Context, ensure};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use cookie::Key;

/// Generates a new `APP_KEY` value, e.g. `base64:3q2+7w==...`.
pub fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    format!("base64:{}", STANDARD.encode(bytes))
}

/// Parses an `APP_KEY` value (`base64:...` or at least 32 raw bytes).
pub(crate) fn parse_key(value: &str) -> anyhow::Result<Key> {
    let bytes = match value.strip_prefix("base64:") {
        Some(encoded) => STANDARD
            .decode(encoded)
            .context("APP_KEY is not valid base64")?,
        None => value.as_bytes().to_vec(),
    };
    ensure!(
        bytes.len() >= 32,
        "APP_KEY must be at least 32 bytes; generate one with `rnx key:generate`"
    );
    Ok(Key::derive_from(&bytes))
}

/// Associated data for [`seal`], so its values can't pass as an encrypted
/// cookie or the other way round.
const SEALED: &str = "renox.encrypted";

/// Encrypts `plain` with AES-256-GCM under `key` (via the cookie crate's
/// private jar); base64 text.
pub(crate) fn seal(key: &Key, plain: &str) -> String {
    let mut jar = cookie::CookieJar::new();
    jar.private_mut(key)
        .add(cookie::Cookie::new(SEALED, plain.to_owned()));
    jar.get(SEALED)
        .map(|sealed| sealed.value().to_owned())
        .unwrap_or_default()
}

/// Reads a value from [`seal`]; fails if it was changed or sealed with
/// another key.
pub(crate) fn open(key: &Key, sealed: &str) -> anyhow::Result<String> {
    cookie::CookieJar::new()
        .private(key)
        .decrypt(cookie::Cookie::new(SEALED, sealed.to_owned()))
        .map(|plain| plain.value().to_owned())
        .context("the value can't be decrypted with this APP_KEY")
}

/// First bytes of a file written by [`encrypt_file`].
const FILE_MAGIC: &[u8; 4] = b"RNXE";
/// Version of the encrypted file format (byte 5).
const FILE_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const DATA_KEY_LEN: usize = 32;
/// Magic, version, the wrapped data key (nonce, key, tag) and the data nonce.
const FILE_HEADER_LEN: usize = 5 + NONCE_LEN + DATA_KEY_LEN + TAG_LEN + NONCE_LEN;

/// Whether `data` starts like a file from [`encrypt_file`].
pub(crate) fn is_encrypted_file(data: &[u8]) -> bool {
    data.len() >= FILE_HEADER_LEN + TAG_LEN && data.starts_with(FILE_MAGIC)
}

/// Encrypts a file with AES-256-GCM under a fresh data key, which is itself
/// sealed (AES-256-GCM) with a key derived from `APP_KEY`. Layout:
/// `RNXE`, version, wrap nonce (12), wrapped data key (32 + 16 tag), data
/// nonce (12), ciphertext + tag. Rotating `APP_KEY` only needs the header
/// re-wrapped, see [`rewrap_file`].
pub(crate) fn encrypt_file(key: &Key, plain: &[u8]) -> anyhow::Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let mut data_key = [0u8; DATA_KEY_LEN];
    let mut wrap_nonce = [0u8; NONCE_LEN];
    let mut data_nonce = [0u8; NONCE_LEN];
    rand::fill(&mut data_key);
    rand::fill(&mut wrap_nonce);
    rand::fill(&mut data_nonce);
    let aad = file_aad(b"renox.file-key");
    let wrapped = Aes256Gcm::new_from_slice(key.encryption())
        .map_err(|_| anyhow::anyhow!("invalid key"))?
        .encrypt(
            Nonce::from_slice(&wrap_nonce),
            Payload {
                msg: &data_key,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("could not wrap the file key"))?;
    let aad = file_aad(b"renox.file");
    let body = Aes256Gcm::new_from_slice(&data_key)
        .map_err(|_| anyhow::anyhow!("invalid key"))?
        .encrypt(
            Nonce::from_slice(&data_nonce),
            Payload {
                msg: plain,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("could not encrypt the file"))?;
    let mut out = Vec::with_capacity(FILE_HEADER_LEN + body.len());
    out.extend_from_slice(FILE_MAGIC);
    out.push(FILE_VERSION);
    out.extend_from_slice(&wrap_nonce);
    out.extend_from_slice(&wrapped);
    out.extend_from_slice(&data_nonce);
    out.extend_from_slice(&body);
    Ok(out)
}

fn file_aad(purpose: &[u8]) -> Vec<u8> {
    let mut aad = purpose.to_vec();
    aad.extend_from_slice(FILE_MAGIC);
    aad.push(FILE_VERSION);
    aad
}

/// Unwraps the data key in `data`'s header with `key`.
fn unwrap_data_key(key: &Key, data: &[u8]) -> anyhow::Result<[u8; DATA_KEY_LEN]> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    ensure!(is_encrypted_file(data), "not an encrypted Renox file");
    ensure!(
        data[4] == FILE_VERSION,
        "unknown encrypted file version {}",
        data[4]
    );
    let wrap_nonce = &data[5..5 + NONCE_LEN];
    let wrapped = &data[5 + NONCE_LEN..5 + NONCE_LEN + DATA_KEY_LEN + TAG_LEN];
    let aad = file_aad(b"renox.file-key");
    let plain = Aes256Gcm::new_from_slice(key.encryption())
        .map_err(|_| anyhow::anyhow!("invalid key"))?
        .decrypt(
            Nonce::from_slice(wrap_nonce),
            Payload {
                msg: wrapped,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("the file can't be decrypted with this APP_KEY"))?;
    plain
        .try_into()
        .map_err(|_| anyhow::anyhow!("the file's key has the wrong length"))
}

/// Reads a file from [`encrypt_file`]; fails if it was changed or sealed
/// with another `APP_KEY`.
pub(crate) fn decrypt_file(key: &Key, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let data_key = unwrap_data_key(key, data)?;
    let nonce_at = 5 + NONCE_LEN + DATA_KEY_LEN + TAG_LEN;
    let nonce = &data[nonce_at..nonce_at + NONCE_LEN];
    let aad = file_aad(b"renox.file");
    Aes256Gcm::new_from_slice(&data_key)
        .map_err(|_| anyhow::anyhow!("invalid key"))?
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: &data[FILE_HEADER_LEN..],
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("the file was changed or is damaged"))
}

/// Seals `data`'s file key again under `new` (opened with `old`), leaving the
/// content as it is: the cheap step of an `APP_KEY` rotation.
pub(crate) fn rewrap_file(old: &Key, new: &Key, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let data_key = unwrap_data_key(old, data)?;
    let mut wrap_nonce = [0u8; NONCE_LEN];
    rand::fill(&mut wrap_nonce);
    let aad = file_aad(b"renox.file-key");
    let wrapped = Aes256Gcm::new_from_slice(new.encryption())
        .map_err(|_| anyhow::anyhow!("invalid key"))?
        .encrypt(
            Nonce::from_slice(&wrap_nonce),
            Payload {
                msg: &data_key,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("could not wrap the file key"))?;
    let mut out = data.to_vec();
    out[5..5 + NONCE_LEN].copy_from_slice(&wrap_nonce);
    out[5 + NONCE_LEN..5 + NONCE_LEN + DATA_KEY_LEN + TAG_LEN].copy_from_slice(&wrapped);
    Ok(out)
}

/// A random, URL-safe token with 256 bits of entropy (43 characters), e.g.
/// for an invitation link or an API key.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compares two strings in time independent of where they differ.
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_parse() {
        let key = generate_key();
        assert!(key.starts_with("base64:"));
        assert!(parse_key(&key).is_ok());
    }

    #[test]
    fn short_keys_are_rejected() {
        assert!(parse_key("too-short").is_err());
    }

    #[test]
    fn files_round_trip_and_reject_other_keys_and_tampering() {
        let key = parse_key(&generate_key()).unwrap();
        let other = parse_key(&generate_key()).unwrap();
        let sealed = encrypt_file(&key, b"passport scan").unwrap();
        assert!(is_encrypted_file(&sealed));
        assert!(!sealed.windows(8).any(|w| w == b"passport"));
        assert_eq!(decrypt_file(&key, &sealed).unwrap(), b"passport scan");
        // A fresh data key and nonce each time.
        assert_ne!(sealed, encrypt_file(&key, b"passport scan").unwrap());
        assert!(decrypt_file(&other, &sealed).is_err());
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(decrypt_file(&key, &tampered).is_err());
        assert!(decrypt_file(&key, b"plain text").is_err());
        assert_eq!(
            decrypt_file(&key, &encrypt_file(&key, b"").unwrap()).unwrap(),
            b""
        );
    }

    #[test]
    fn rotation_rewraps_the_file_key_only() {
        let old = parse_key(&generate_key()).unwrap();
        let new = parse_key(&generate_key()).unwrap();
        let sealed = encrypt_file(&old, b"scan").unwrap();
        let moved = rewrap_file(&old, &new, &sealed).unwrap();
        assert_eq!(decrypt_file(&new, &moved).unwrap(), b"scan");
        assert!(decrypt_file(&old, &moved).is_err());
        assert_eq!(sealed[FILE_HEADER_LEN..], moved[FILE_HEADER_LEN..]);
        assert!(rewrap_file(&new, &old, &sealed).is_err());
    }

    #[test]
    fn compares_tokens() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}

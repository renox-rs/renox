//! Time-based one-time passwords (RFC 6238): the six-digit codes an
//! authenticator app shows, from a secret shared once through a QR code.
//! HMAC-SHA1, 30-second steps, six digits: what Google Authenticator, Authy,
//! 1Password and the others expect by default.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

/// How long one code lasts.
pub const STEP_SECONDS: i64 = 30;
/// Digits in a code.
pub const DIGITS: u32 = 6;

/// A new shared secret: 20 random bytes (160 bits, RFC 4226's
/// recommendation), as base32 for the QR code and for typing in.
pub fn new_secret() -> String {
    let bytes: [u8; 20] = rand::random();
    base32_encode(&bytes)
}

/// The time step `unix_seconds` falls in.
pub fn step_at(unix_seconds: i64) -> i64 {
    unix_seconds.div_euclid(STEP_SECONDS)
}

/// The code for `step` (RFC 4226's HOTP with the step as the counter), or
/// `None` when `secret` isn't valid base32.
pub fn code_at(secret: &str, step: i64) -> Option<String> {
    let key = base32_decode(secret)?;
    Some(hotp(&key, step as u64, DIGITS))
}

/// The step whose code is `code`: the current step, the one before or the
/// one after (clocks drift), else `None`. A code for a step at or before
/// `last_used` is refused, so a code works once.
pub fn verify(secret: &str, code: &str, unix_seconds: i64, last_used: Option<i64>) -> Option<i64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let key = base32_decode(secret)?;
    let now = step_at(unix_seconds);
    (now - 1..=now + 1)
        .filter(|step| last_used.is_none_or(|used| *step > used))
        .find(|step| constant_time_eq(&hotp(&key, *step as u64, DIGITS), &code))
}

/// The `otpauth://` URI authenticator apps read from the QR code.
pub fn otpauth_uri(issuer: &str, account: &str, secret: &str) -> String {
    let label = format!("{}:{}", encode(issuer), encode(account));
    format!(
        "otpauth://totp/{label}?secret={secret}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
        encode(issuer)
    )
}

/// HOTP (RFC 4226): HMAC-SHA1 of the counter, dynamically truncated.
fn hotp(key: &[u8], counter: u64, digits: u32) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC takes keys of any length");
    mac.update(&counter.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[hash.len() - 1] & 0x0f);
    let binary = (u32::from(hash[offset] & 0x7f) << 24)
        | (u32::from(hash[offset + 1]) << 16)
        | (u32::from(hash[offset + 2]) << 8)
        | u32::from(hash[offset + 3]);
    format!(
        "{:0width$}",
        binary % 10u32.pow(digits),
        width = digits as usize
    )
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Base32 (RFC 4648), without padding: how authenticator apps take a secret.
pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    for chunk in bytes.chunks(5) {
        let mut buffer = [0u8; 5];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let bits = buffer
            .iter()
            .fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
        let chars = (chunk.len() * 8).div_ceil(5);
        for i in 0..chars {
            let index = (bits >> (35 - i * 5)) & 0x1f;
            out.push(char::from(ALPHABET[index as usize]));
        }
    }
    out
}

/// Reads base32, ignoring case, spaces, dashes and padding (people type it
/// in groups); `None` for anything else.
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut bits, mut count) = (0u64, 0u32);
    for c in text.chars().filter(|c| !matches!(c, ' ' | '-' | '=')) {
        let value = ALPHABET
            .iter()
            .position(|a| char::from(*a) == c.to_ascii_uppercase())?;
        bits = (bits << 5) | value as u64;
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Percent-encodes a label part of the URI.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'@' => {
                out.push(char::from(byte))
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238, Appendix B (SHA-1): the shared secret is the ASCII bytes
    /// "12345678901234567890"; codes are 8 digits there.
    #[test]
    fn rfc_6238_test_vectors() {
        let key = b"12345678901234567890";
        for (time, code) in [
            (59, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(hotp(key, step_at(time) as u64, 8), code, "time {time}");
        }
        // Six digits are the last six.
        let secret = base32_encode(key);
        assert_eq!(code_at(&secret, step_at(59)).unwrap(), "287082");
    }

    /// RFC 4648, section 10 (without padding).
    #[test]
    fn base32_test_vectors() {
        for (plain, encoded) in [
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foob", "MZXW6YQ"),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32_encode(plain.as_bytes()), encoded);
            assert_eq!(base32_decode(encoded).unwrap(), plain.as_bytes());
        }
        // How people type it: lower case, groups, padding.
        assert_eq!(base32_decode("mzxw 6ytb-oi======").unwrap(), b"foobar");
        assert!(base32_decode("not base32!").is_none());
        assert!(base32_decode("").is_none());
    }

    #[test]
    fn codes_work_once_and_within_one_step() {
        let secret = new_secret();
        assert_eq!(secret.len(), 32);
        let now = 1_700_000_000;
        let step = step_at(now);
        let code = code_at(&secret, step).unwrap();
        assert_eq!(verify(&secret, &code, now, None), Some(step));
        // A step before and after still work (clock drift)…
        let before = code_at(&secret, step - 1).unwrap();
        assert_eq!(verify(&secret, &before, now, None), Some(step - 1));
        let after = code_at(&secret, step + 1).unwrap();
        assert_eq!(verify(&secret, &after, now, None), Some(step + 1));
        // …two steps away don't.
        let old = code_at(&secret, step - 2).unwrap();
        assert_eq!(verify(&secret, &old, now, None), None);
        // A code (or an older one) can't be used twice.
        assert_eq!(verify(&secret, &code, now, Some(step)), None);
        assert_eq!(verify(&secret, &before, now, Some(step)), None);
        assert_eq!(verify(&secret, &after, now, Some(step)), Some(step + 1));
        // Spaces are fine, other text isn't.
        let spaced = format!("{} {}", &code[..3], &code[3..]);
        assert_eq!(verify(&secret, &spaced, now, None), Some(step));
        assert_eq!(verify(&secret, "12345", now, None), None);
        assert_eq!(verify(&secret, "abcdef", now, None), None);
    }

    #[test]
    fn the_uri_names_the_app_and_the_account() {
        let uri = otpauth_uri("Acme Shop", "ana@example.com", "JBSWY3DPEHPK3PXP");
        assert_eq!(
            uri,
            "otpauth://totp/Acme%20Shop:ana@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Acme%20Shop&algorithm=SHA1&digits=6&period=30"
        );
    }
}

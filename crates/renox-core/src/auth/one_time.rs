//! One-time codes: a short number (or a long random token) that proves
//! someone received a message, to use once before it expires, with a limit
//! on wrong tries. Passwordless login ([`LoginCodes`](super::LoginCodes),
//! [`Auth::magic_link`](super::Auth::magic_link)) is built on it; use it
//! yourself to confirm any other action (a payout, a phone number) the same
//! way.
//!
//! ```
//! use std::time::Duration;
//! use renox::auth::{CodeCheck, OneTimeCode};
//! # use renox::prelude::*;
//!
//! # async fn demo(state: AppState, user_id: i64) -> Result {
//! // Send the code by SMS, mail... (only the hash is stored).
//! let code = OneTimeCode::issue(&state, "payout", &user_id.to_string(), Duration::from_secs(300), 5).await?;
//! # let _ = code;
//!
//! // Later, when the user types it back:
//! match OneTimeCode::verify(&state, "payout", &user_id.to_string(), "123456").await? {
//!     CodeCheck::Valid => { /* do it; the code is used up */ }
//!     CodeCheck::Wrong { tries_left } => { /* "wrong code, {tries_left} left" */ let _ = tries_left; }
//!     CodeCheck::Missing => { /* expired, used, or burned by wrong tries: ask for a new one */ }
//! }
//! # Ok(()) }
//! ```
//!
//! The codes live in the `one_time_codes` table, which the [`Auth`](super::Auth)
//! module's migrations create, so the app needs that module.
//!
//! Only a keyed hash of the code is stored (HMAC with `APP_KEY`, so a leaked
//! table doesn't let someone work out a six-digit code). A new code for the
//! same `purpose` and `subject` replaces the old one. Each check of a live
//! code first uses up a try, in one statement, so parallel guesses can't
//! exceed `max_tries`; the code that is right is deleted in the same step
//! that accepts it, so two requests with it can't both succeed. After
//! `max_tries` wrong codes the code is burned: a new one must be issued.
//! Account-level lockout is not this type's job (`LoginThrottle` does that
//! for logins).

use std::time::Duration;

use super::tokens::sha256_hex;
use crate::crypto::{constant_time_eq, random_token};
use crate::db::{Db, sql};
use crate::webhook::hmac_sha256_hex;
use crate::{AppState, Result};

/// The result of [`OneTimeCode::verify`]. Not `#[non_exhaustive]`: a `match` should
/// handle every outcome on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCheck {
    /// Right: the code is used up.
    Valid,
    /// Wrong. `tries_left` is how many more wrong codes are allowed; `0`
    /// means this was the last, and the code is burned.
    Wrong {
        /// Tries left before the code is burned.
        tries_left: u32,
    },
    /// There is no live code to check against: none was issued, it expired,
    /// it was used, or wrong tries burned it.
    Missing,
}

/// Issues and checks one-time codes; see the [module](self).
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct OneTimeCode;

/// Longest code [`OneTimeCode::issue_digits`] makes.
const MAX_DIGITS: u32 = 12;

impl OneTimeCode {
    /// Issues a six-digit code for `subject` (who or what it confirms) under
    /// `purpose` (what for: `"payout"`), valid for `ttl` and for `max_tries`
    /// checks. Replaces the live code for the same purpose and subject.
    /// Returns the code to send; it is not stored.
    pub async fn issue(
        state: &AppState,
        purpose: &str,
        subject: &str,
        ttl: Duration,
        max_tries: u32,
    ) -> Result<String> {
        Self::issue_digits(state, purpose, subject, 6, ttl, max_tries).await
    }

    /// [`OneTimeCode::issue`] with `digits` digits (4 to 12).
    pub async fn issue_digits(
        state: &AppState,
        purpose: &str,
        subject: &str,
        digits: u32,
        ttl: Duration,
        max_tries: u32,
    ) -> Result<String> {
        let code = random_digits(digits);
        store(state, purpose, subject, &code, None, ttl, max_tries).await?;
        Ok(code)
    }

    /// Checks `code` against the live code for `purpose` and `subject`; see
    /// [`CodeCheck`]. Whitespace and dashes in what the user typed
    /// (`123 456`) are ignored.
    pub async fn verify(
        state: &AppState,
        purpose: &str,
        subject: &str,
        code: &str,
    ) -> Result<CodeCheck> {
        check(state, purpose, subject, code, None).await
    }

    /// Removes the live code for `purpose` and `subject`, e.g. when what it
    /// was for is cancelled; returns whether there was one.
    pub async fn discard(state: &AppState, purpose: &str, subject: &str) -> Result<bool> {
        let done = sql("DELETE FROM one_time_codes WHERE purpose = ? AND subject = ?")
            .bind(purpose)
            .bind(subject)
            .execute(&state.db)
            .await?;
        Ok(done > 0)
    }
}

/// Deletes codes that expired; returns how many. Issuing does it too, and
/// `rnx tokens:prune` runs it.
pub async fn prune_expired_codes(db: &Db) -> Result<u64> {
    Ok(sql("DELETE FROM one_time_codes WHERE expires_at < ?")
        .bind(crate::clock::unix_secs())
        .execute(db)
        .await?)
}

/// `digits` random decimal digits, leading zeros kept.
pub(crate) fn random_digits(digits: u32) -> String {
    let digits = digits.clamp(4, MAX_DIGITS);
    let mut out = String::with_capacity(digits as usize);
    let mut bytes = [0u8; 16];
    rand::fill(&mut bytes);
    // One byte per digit, rejecting values that would bias the result.
    let mut pool = bytes.to_vec();
    while out.len() < digits as usize {
        let Some(byte) = pool.pop() else {
            rand::fill(&mut bytes);
            pool = bytes.to_vec();
            continue;
        };
        if byte < 250 {
            out.push(char::from(b'0' + byte % 10));
        }
    }
    out
}

/// What the user typed, without spaces and dashes.
fn clean(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

/// The keyed hash that is stored for a secret.
fn keyed(state: &AppState, purpose: &str, subject: &str, secret: &str) -> String {
    hmac_sha256_hex(
        state.key.signing(),
        format!("renox.one-time\0{purpose}\0{subject}\0{secret}"),
    )
}

/// Hash of a browser-session nonce, as stored in `binding`.
pub(crate) fn bind_hash(nonce: &str) -> String {
    sha256_hex(nonce)
}

/// Stores a code (replacing the live one) and returns its row id.
pub(crate) async fn store(
    state: &AppState,
    purpose: &str,
    subject: &str,
    secret: &str,
    binding: Option<&str>,
    ttl: Duration,
    max_tries: u32,
) -> Result<i64> {
    let now = crate::clock::unix_secs();
    // Expired rows go now and then, with the next issue.
    sql("DELETE FROM one_time_codes WHERE expires_at < ?")
        .bind(now)
        .execute(&state.db)
        .await?;
    let row = sql("INSERT INTO one_time_codes \
         (purpose, subject, code, binding, attempts, max_attempts, expires_at, created_at) \
         VALUES (?, ?, ?, ?, 0, ?, ?, ?) \
         ON CONFLICT (purpose, subject) DO UPDATE SET code = excluded.code, \
         binding = excluded.binding, attempts = 0, max_attempts = excluded.max_attempts, \
         expires_at = excluded.expires_at, created_at = excluded.created_at \
         RETURNING id")
    .bind(purpose)
    .bind(subject)
    .bind(keyed(state, purpose, subject, secret))
    .bind(binding.map(bind_hash))
    .bind(i64::from(max_tries.max(1)))
    .bind(now + i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX / 2).max(1))
    .bind(now)
    .fetch_one(&state.db)
    .await?;
    Ok(row.try_get("id")?)
}

/// [`OneTimeCode::verify`], optionally for one browser session only: a
/// code bound to a session is never accepted (nor counted against) from
/// another.
pub(crate) async fn check(
    state: &AppState,
    purpose: &str,
    subject: &str,
    typed: &str,
    binding: Option<&str>,
) -> Result<CodeCheck> {
    let now = crate::clock::unix_secs();
    // Use up a try first, atomically; a code bound to another session
    // doesn't match, so it can't be used (or burned) from there.
    let Some(row) = sql("UPDATE one_time_codes SET attempts = attempts + 1 \
         WHERE purpose = ? AND subject = ? AND expires_at > ? AND attempts < max_attempts \
         AND (binding IS NULL OR binding = ?) \
         RETURNING id, code, attempts, max_attempts")
    .bind(purpose)
    .bind(subject)
    .bind(now)
    .bind(binding.map(bind_hash))
    .fetch_optional(&state.db)
    .await?
    else {
        return Ok(CodeCheck::Missing);
    };
    let id: i64 = row.try_get("id")?;
    let stored: String = row.try_get("code")?;
    let attempts: i64 = row.try_get("attempts")?;
    let max: i64 = row.try_get("max_attempts")?;
    if constant_time_eq(&stored, &keyed(state, purpose, subject, &clean(typed))) {
        // Accepted only by the request whose delete removes the row.
        let done = sql("DELETE FROM one_time_codes WHERE id = ? AND code = ?")
            .bind(id)
            .bind(&stored)
            .execute(&state.db)
            .await?;
        return Ok(if done == 1 {
            CodeCheck::Valid
        } else {
            CodeCheck::Missing
        });
    }
    let left = u32::try_from((max - attempts).max(0)).unwrap_or(0);
    if left == 0 {
        sql("DELETE FROM one_time_codes WHERE id = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(CodeCheck::Wrong { tries_left: left })
}

/// Issues a link token (`<id>.<secret>`, 256 bits) for `subject`.
pub(crate) async fn issue_token(
    state: &AppState,
    purpose: &str,
    subject: &str,
    binding: Option<&str>,
    ttl: Duration,
) -> Result<String> {
    let secret = random_token();
    let id = store(state, purpose, subject, &secret, binding, ttl, 1).await?;
    Ok(format!("{id}.{secret}"))
}

/// The subject a live link token is for, without using it up. A token
/// bound to another session than `binding` is not live for it.
pub(crate) async fn peek_token(
    state: &AppState,
    purpose: &str,
    token: &str,
    binding: Option<&str>,
) -> Result<Option<(i64, String, String)>> {
    let Some((id, secret)) = token.split_once('.') else {
        return Ok(None);
    };
    let Ok(id) = id.parse::<i64>() else {
        return Ok(None);
    };
    let Some(row) = sql("SELECT subject, code, binding FROM one_time_codes \
         WHERE id = ? AND purpose = ? AND expires_at > ?")
    .bind(id)
    .bind(purpose)
    .bind(crate::clock::unix_secs())
    .fetch_optional(&state.db)
    .await?
    else {
        return Ok(None);
    };
    let subject: String = row.try_get("subject")?;
    let stored: String = row.try_get("code")?;
    let bound: Option<String> = row.try_get("binding")?;
    let right_secret = constant_time_eq(&stored, &keyed(state, purpose, &subject, secret));
    let right_browser = match (&bound, binding) {
        (None, _) => true,
        (Some(bound), Some(nonce)) => constant_time_eq(bound, &bind_hash(nonce)),
        (Some(_), None) => false,
    };
    Ok((right_secret && right_browser).then_some((id, subject, stored)))
}

/// Uses a link token up: the subject it was issued for, once.
pub(crate) async fn consume_token(
    state: &AppState,
    purpose: &str,
    token: &str,
    binding: Option<&str>,
) -> Result<Option<String>> {
    let Some((id, subject, stored)) = peek_token(state, purpose, token, binding).await? else {
        return Ok(None);
    };
    let done = sql("DELETE FROM one_time_codes WHERE id = ? AND code = ?")
        .bind(id)
        .bind(&stored)
        .execute(&state.db)
        .await?;
    Ok((done == 1).then_some(subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_digits_of_the_right_length() {
        for digits in [4, 6, 8, 12] {
            let code = random_digits(digits);
            assert_eq!(code.len(), digits as usize);
            assert!(code.bytes().all(|b| b.is_ascii_digit()));
        }
        assert_eq!(random_digits(1).len(), 4);
        assert_eq!(random_digits(40).len(), 12);
    }

    #[test]
    fn typed_codes_lose_spaces_and_dashes() {
        assert_eq!(clean(" 123 456\n"), "123456");
        assert_eq!(clean("123-456"), "123456");
    }
}

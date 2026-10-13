use renox::db::Encrypted;
use renox::prelude::*;
use serde::Serialize;

use crate::recovery;

/// A user's two-factor authentication: their TOTP secret (sealed with
/// `APP_KEY`), whether they confirmed it, the last code used, and their
/// hashed recovery codes. A row without `confirmed_at` is a setup the user
/// hasn't finished: it doesn't count at login.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "two_factor")]
pub struct TwoFactorCredential {
    pub id: i64,
    pub user_id: i64,
    /// Never serialized: it isn't shown once confirmed.
    #[serde(skip)]
    pub secret: Encrypted<String>,
    pub confirmed_at: Option<DateTime>,
    pub last_used_step: Option<i64>,
    /// Hashed codes, as JSON.
    #[serde(skip)]
    pub recovery_codes: String,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl TwoFactorCredential {
    /// `user_id`'s row, confirmed or not.
    pub async fn of(db: &Db, user_id: i64) -> Result<Option<Self>> {
        Self::where_eq("user_id", user_id).first(db).await
    }

    /// Whether `user_id` has two-factor authentication on (confirmed).
    pub async fn enabled(db: &Db, user_id: i64) -> Result<bool> {
        Ok(Self::of(db, user_id)
            .await?
            .is_some_and(|credential| credential.confirmed_at.is_some()))
    }

    /// Whether it's on (the user confirmed a code).
    pub fn is_confirmed(&self) -> bool {
        self.confirmed_at.is_some()
    }

    /// The hashes of the recovery codes not used yet.
    fn hashes(&self) -> Vec<String> {
        renox::serde_json::from_str(&self.recovery_codes).unwrap_or_default()
    }

    /// How many recovery codes are left.
    pub fn recovery_codes_left(&self) -> usize {
        self.hashes().len()
    }

    /// New recovery codes in place of the old ones: returns them, for the
    /// user to see once; only their hashes are kept. Save the row after.
    pub fn new_recovery_codes(&mut self) -> Vec<String> {
        let codes = recovery::generate();
        let hashes: Vec<String> = codes.iter().map(|code| recovery::hash(code)).collect();
        self.recovery_codes = renox::serde_json::to_string(&hashes).unwrap_or_else(|_| "[]".into());
        codes
    }

    /// Uses up `typed` if it's one of the recovery codes left: true when it
    /// was (and is now gone). Save the row after.
    pub fn use_recovery_code(&mut self, typed: &str) -> bool {
        let typed = recovery::hash(typed);
        let mut hashes = self.hashes();
        let before = hashes.len();
        // Compared in constant time, like other secrets.
        hashes.retain(|hash| !same(hash.as_bytes(), typed.as_bytes()));
        if hashes.len() == before {
            return false;
        }
        self.recovery_codes = renox::serde_json::to_string(&hashes).unwrap_or_else(|_| "[]".into());
        true
    }
}

/// `a == b`, taking as long whatever the bytes are.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

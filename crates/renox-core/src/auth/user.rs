use std::future::Future;
use std::sync::LazyLock;

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use serde::{Deserialize, Serialize};

use super::Policy;
use crate::db::{
    ColumnKind, DateTime, Db, DbValue, Executor, FromRow, Model, ModelColumn, Row, ToDbValue, sql,
};
use crate::{Error, Result};

/// A row of the `users` table created by the `Auth` module.
///
/// Columns the app adds with its own migration (a `role`, a `phone`) are
/// kept in `extra`: read them with `user.get::<String>("role")`, change them
/// with `user.set(&db, "role", "admin")`, filter with
/// `User::where_eq("role", "admin")`. Templates and JSON see them as the
/// user's own fields (`{{ auth.user.role }}`). A typed model on the same
/// table (`#[model(table = "users")] struct Member`) works too.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct User {
    /// The `users` row id; `0` before it is saved.
    pub id: i64,
    /// The display name.
    pub name: String,
    /// The email address (Renox's forms store it trimmed and lowercased).
    pub email: String,
    /// The Argon2 hash; never serialized, so it can't leak into templates or JSON.
    #[serde(skip_serializing, default)]
    pub password: String,
    /// When the email address was confirmed; `None` while unverified.
    pub email_verified_at: Option<DateTime>,
    /// When the account was created.
    pub created_at: Option<DateTime>,
    /// When the row was last saved.
    pub updated_at: Option<DateTime>,
    /// The app's own columns, by name.
    #[serde(flatten, default)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Columns `extra` never holds: the struct's own, and secrets.
const NOT_EXTRA: &[&str] = &[
    "id",
    "name",
    "email",
    "password",
    "email_verified_at",
    "created_at",
    "updated_at",
    "sessions_revoked_at",
    "remember_token",
    "session_revoked",
];

impl FromRow for User {
    fn from_row(row: &Row) -> std::result::Result<Self, crate::db::DbError> {
        let extra = row
            .columns()
            .into_iter()
            .filter(|column| !NOT_EXTRA.contains(column))
            .map(|column| (column.to_owned(), row.json(column)))
            .collect();
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            email: row.try_get("email")?,
            password: row.try_get("password")?,
            email_verified_at: row.try_get("email_verified_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            extra,
        })
    }
}

impl Model for User {
    const TABLE: &'static str = "users";
    const SELECT_ALL: bool = true;
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "name",
        "email",
        "password",
        "email_verified_at",
        "created_at",
        "updated_at",
    ];

    type Key = i64;

    fn id(&self) -> i64 {
        self.id
    }

    fn set_id(&mut self, id: i64) {
        self.id = id;
    }

    fn values(&self) -> Vec<DbValue> {
        vec![
            self.name.to_db_value(),
            self.email.to_db_value(),
            self.password.to_db_value(),
            self.email_verified_at.to_db_value(),
            self.created_at.to_db_value(),
            self.updated_at.to_db_value(),
        ]
    }

    fn touch(&mut self, now: DateTime, creating: bool) {
        if creating && self.created_at.is_none() {
            self.created_at = Some(now);
        }
        self.updated_at = Some(now);
    }

    fn column_info() -> Vec<ModelColumn> {
        let col = ModelColumn::new;
        vec![
            col("id", "i64", ColumnKind::BigInt, false),
            col("name", "String", ColumnKind::Text, false),
            col("email", "String", ColumnKind::Text, false),
            col("password", "String", ColumnKind::Text, false),
            col(
                "email_verified_at",
                "Option<DateTime>",
                ColumnKind::DateTime,
                true,
            ),
            col("created_at", "Option<DateTime>", ColumnKind::DateTime, true),
            col("updated_at", "Option<DateTime>", ColumnKind::DateTime, true),
        ]
    }
}

impl User {
    /// Emails are matched case-insensitively.
    pub fn find_by_email<'c, E: Executor<'c>>(
        db: E,
        email: &str,
    ) -> impl Future<Output = Result<Option<Self>>> + Send {
        Self::query()
            .where_eq("email", normalize_email(email))
            .first(db)
    }

    /// Creates a user with a hashed password.
    pub async fn register(db: &Db, name: &str, email: &str, password: &str) -> Result<Self> {
        let user = Self {
            name: name.trim().to_owned(),
            email: normalize_email(email),
            password: hash_password(password).await?,
            ..Self::default()
        };
        let id = Self::create(db, user).await?.id;
        // Read back, with the defaults of the app's own columns (`extra`).
        Self::find_or_404(db, id).await
    }

    /// Changes the password. Every session of the user ends, this one too
    /// (sessions hold a fingerprint of the password hash); in a handler use
    /// [`crate::auth::change_password`], which logs this session in again.
    pub async fn set_password(&mut self, db: &Db, password: &str) -> Result {
        self.password = hash_password(password).await?;
        self.save(db).await
    }

    /// One of the app's own columns (see `extra`), e.g.
    /// `user.get::<String>("role")`; `None` if missing, null or of another type.
    /// A BOOLEAN column reads as `bool` on SQLite too, where it is stored as
    /// 0 or 1.
    pub fn get<T: serde::de::DeserializeOwned>(&self, column: &str) -> Option<T> {
        let value = self.extra.get(column)?;
        serde_json::from_value(value.clone()).ok().or_else(|| {
            let flag = value.as_i64().filter(|n| *n == 0 || *n == 1)?;
            serde_json::from_value(serde_json::Value::Bool(flag == 1)).ok()
        })
    }

    /// Sets one of the app's own columns in the database and in `extra`, e.g.
    /// `user.set(&db, "role", "admin")`.
    pub async fn set(&mut self, db: &Db, column: &str, value: impl ToDbValue) -> Result {
        let plain = !column.is_empty()
            && column
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain || NOT_EXTRA.contains(&column) {
            return Err(anyhow::anyhow!("User::set can't change `{column}`").into());
        }
        let value = value.to_db_value();
        sql(format!(
            "UPDATE users SET {} = ? WHERE id = ?",
            crate::db::quote(column)
        ))
        .bind(value.clone())
        .bind(self.id)
        .execute(db)
        .await?;
        self.extra.insert(column.to_owned(), value.to_json());
        Ok(())
    }

    /// Ends every session of the user, e.g. on logout or when an account
    /// may be compromised. API tokens stay; see [`User::revoke_tokens`].
    pub async fn revoke_sessions(&self, db: &Db) -> Result {
        revoke_sessions(db, self.id).await.map(|_| ())
    }

    /// The user, when their sessions were last revoked, and whether the
    /// session `session_id` was logged out, in one query.
    pub(crate) async fn find_with_revocation(
        db: &Db,
        id: i64,
        session_id: &str,
    ) -> Result<Option<(Self, i64, bool)>> {
        let row = sql(
            "SELECT users.*, (SELECT COUNT(*) FROM revoked_sessions WHERE id = ?) AS session_revoked \
             FROM users WHERE id = ?",
        )
        .bind(session_id)
        .bind(id)
        .fetch_optional(db)
        .await?;
        Ok(match row {
            Some(row) => {
                let revoked: i64 = row.try_get("session_revoked")?;
                Some((
                    Self::from_row(&row)?,
                    row.try_get("sessions_revoked_at")?,
                    revoked > 0,
                ))
            }
            None => None,
        })
    }

    /// The user with this email and password, e.g. to issue an API token.
    /// Takes as long for an unknown email as for a wrong password, so the
    /// answer doesn't reveal which emails have accounts.
    pub async fn attempt(db: &Db, email: &str, password: &str) -> Result<Option<Self>> {
        let user = Self::find_by_email(db, email).await?;
        let hash = user
            .as_ref()
            .map_or_else(dummy_hash, |u| u.password.clone());
        let valid = verify_password(password, &hash).await;
        let Some(mut user) = user.filter(|_| valid) else {
            return Ok(None);
        };
        user.rehash_if_needed(db, password).await?;
        Ok(Some(user))
    }

    /// After a successful login: replaces an older hash (bcrypt from an
    /// imported Laravel app) with Argon2id.
    pub(crate) async fn rehash_if_needed(&mut self, db: &Db, password: &str) -> Result {
        if needs_rehash(&self.password) {
            self.set_password(db, password).await?;
        }
        Ok(())
    }

    /// Whether the user has a password. Users made by a social login
    /// (`auth::register_verified`, the `renox-oauth` crate) don't: their
    /// `password` is empty, which no typed password matches, until they
    /// choose one with "Forgot your password?".
    pub fn has_password(&self) -> bool {
        !self.password.is_empty()
    }

    /// Whether `password` matches the stored hash (Argon2id or an imported bcrypt one).
    pub async fn check_password(&self, password: &str) -> bool {
        verify_password(password, &self.password).await
    }

    /// Whether `target`'s [`Policy`] allows this user `ability`.
    pub fn can(&self, ability: &str, target: &impl Policy) -> bool {
        target.allows(self, ability)
    }

    /// Like [`User::can`], but a refusal becomes a 403 error.
    pub fn authorize(&self, ability: &str, target: &impl Policy) -> Result {
        if self.can(ability, target) {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}

/// Ends every session of user `id` logged in until now; returns the cut-off.
pub(crate) async fn revoke_sessions(db: &Db, id: i64) -> Result<i64> {
    let now = super::unix_millis();
    sql("UPDATE users SET sessions_revoked_at = ? WHERE id = ?")
        .bind(now)
        .bind(id)
        .execute(db)
        .await?;
    Ok(now)
}

/// Emails are stored and looked up trimmed and lowercased, so they match
/// regardless of case on every database (SQLite's `COLLATE NOCASE` alone
/// wouldn't help on PostgreSQL).
///
/// Use it where app code compares or stores an address, so it matches what
/// Renox did at registration and login.
///
/// ```
/// assert_eq!(renox::auth::normalize_email("  Ana@Example.COM "), "ana@example.com");
/// ```
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// A real hash of a throwaway password, verified against when an email is
/// unknown so the response takes as long as for a real account.
pub(crate) fn dummy_hash() -> String {
    static HASH: LazyLock<String> = LazyLock::new(|| {
        Argon2::default()
            .hash_password(b"renox-timing-equaliser")
            .map(|hash| hash.to_string())
            .unwrap_or_default()
    });
    HASH.clone()
}

/// Hashes a password with Argon2id on a blocking thread.
pub async fn hash_password(password: &str) -> Result<String> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
            .map_err(|err| anyhow::anyhow!("could not hash the password: {err}"))
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(Error::from)
}

/// Checks a password against its hash on a blocking thread: Argon2 (what
/// Renox writes), or bcrypt (`$2y$…`, e.g. users imported from Laravel).
pub async fn verify_password(password: &str, hash: &str) -> bool {
    let (password, hash) = (password.to_owned(), hash.to_owned());
    tokio::task::spawn_blocking(move || {
        if is_bcrypt(&hash) {
            // PHP writes `$2y$`, the same algorithm as `$2b$`.
            let hash = hash.replacen("$2y$", "$2b$", 1);
            return bcrypt::verify(password.as_bytes(), &hash).unwrap_or(false);
        }
        PasswordHash::new(&hash).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

fn is_bcrypt(hash: &str) -> bool {
    ["$2y$", "$2b$", "$2a$"].iter().any(|p| hash.starts_with(p))
}

/// Whether a hash should be replaced by a fresh Argon2id one (e.g. a bcrypt
/// hash imported from Laravel). The login does it with the typed password.
pub fn needs_rehash(hash: &str) -> bool {
    !hash.starts_with("$argon2id$")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hashes_verify() {
        let hash = hash_password("rahasia123").await.unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("rahasia123", &hash).await);
        assert!(!verify_password("salah", &hash).await);
        assert!(!verify_password("rahasia123", "not a hash").await);
        assert!(!needs_rehash(&hash));
    }

    #[tokio::test]
    async fn laravel_bcrypt_hashes_verify() {
        // `Hash::make('password')` in Laravel (cost 4 to keep the test fast).
        let laravel = bcrypt::hash("password", 4)
            .unwrap()
            .replacen("$2b$", "$2y$", 1);
        assert!(laravel.starts_with("$2y$04$"));
        assert!(verify_password("password", &laravel).await);
        assert!(!verify_password("wrong", &laravel).await);
        assert!(needs_rehash(&laravel));
    }
}

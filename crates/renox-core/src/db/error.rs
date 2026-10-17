use std::fmt;

/// A database error: a query that failed, a lost connection, a broken
/// constraint. Converts into [`crate::Error`] with `?`.
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(db: Db) -> Result {
/// let inserted = renox::db::sql("INSERT INTO tags (name) VALUES (?)").bind("coffee").execute(&db).await;
/// match inserted {
///     Err(err) if err.is_unique_violation() => { /* already there */ }
///     other => { other?; }
/// }
/// # Ok(()) }
/// ```
pub struct DbError(sqlx::Error, Option<String>);

impl DbError {
    /// A filter or sort named the sensitive custom field `key`, whose values
    /// are sealed. Returned by `Query::with_custom_fields` and the grid; it
    /// answers 400.
    ///
    /// ```
    /// use renox::db::DbError;
    /// let err = DbError::sealed_custom_field("tax_id");
    /// assert_eq!(err.sealed_custom_field_key(), Some("tax_id"));
    /// ```
    pub fn sealed_custom_field(key: &str) -> Self {
        Self(
            sqlx::Error::Protocol(format!(
                "custom field `{key}` is sensitive: it cannot be filtered, sorted or searched"
            )),
            Some(key.to_owned()),
        )
    }

    /// The key of the sealed custom field a filter, sort or search named, if
    /// this error is [`DbError::sealed_custom_field`].
    pub fn sealed_custom_field_key(&self) -> Option<&str> {
        self.1.as_deref()
    }

    /// A `UNIQUE` constraint (or primary key) was violated.
    pub fn is_unique_violation(&self) -> bool {
        matches!(&self.0, sqlx::Error::Database(db) if db.is_unique_violation())
    }

    /// A `REFERENCES` constraint was violated.
    pub fn is_foreign_key_violation(&self) -> bool {
        matches!(&self.0, sqlx::Error::Database(db) if db.is_foreign_key_violation())
    }

    /// `fetch_one` or `scalar` found no row.
    pub fn is_row_not_found(&self) -> bool {
        matches!(self.0, sqlx::Error::RowNotFound)
    }

    /// Another transaction got in the way and trying again may work:
    /// SQLite's "database is locked" (busy), PostgreSQL's serialization
    /// failure or deadlock. `Db::transaction_retrying` retries on these.
    pub fn is_retryable(&self) -> bool {
        let sqlx::Error::Database(db) = &self.0 else {
            return false;
        };
        match db.code() {
            Some(code) if code == "40001" || code == "40P01" => true,
            // SQLite's primary codes 5 (BUSY) and 6 (LOCKED), extended or not.
            Some(code) => code.parse::<i32>().is_ok_and(|c| matches!(c & 0xff, 5 | 6)),
            None => false,
        }
    }

    /// No connection was free within `DATABASE_ACQUIRE_TIMEOUT`.
    pub fn is_timeout(&self) -> bool {
        matches!(self.0, sqlx::Error::PoolTimedOut)
    }

    /// The underlying sqlx error, for what Renox doesn't cover. sqlx's
    /// version isn't part of Renox's stability promise.
    pub fn sqlx(&self) -> &sqlx::Error {
        &self.0
    }
}

impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        Self(err, None)
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // The wrapped error's own cause; its message is already ours.
        self.0.source()
    }
}

/// A database error with a chosen code, for tests of the code checks.
#[cfg(test)]
pub(crate) mod fake {
    use std::borrow::Cow;
    use std::fmt;

    use sqlx::error::{DatabaseError, ErrorKind};

    #[derive(Debug)]
    struct Coded(Option<&'static str>, ErrorKind);

    impl fmt::Display for Coded {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "error {:?}", self.0)
        }
    }

    impl std::error::Error for Coded {}

    impl DatabaseError for Coded {
        fn message(&self) -> &str {
            "fake"
        }
        fn code(&self) -> Option<Cow<'_, str>> {
            self.0.map(Cow::Borrowed)
        }
        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
        fn kind(&self) -> ErrorKind {
            match self.1 {
                ErrorKind::UniqueViolation => ErrorKind::UniqueViolation,
                _ => ErrorKind::Other,
            }
        }
    }

    /// A database error answering `code`.
    pub(crate) fn coded(code: Option<&'static str>) -> sqlx::Error {
        sqlx::Error::Database(Box::new(Coded(code, ErrorKind::Other)))
    }

    /// PostgreSQL's unique violation.
    pub(crate) fn unique() -> sqlx::Error {
        sqlx::Error::Database(Box::new(Coded(Some("23505"), ErrorKind::UniqueViolation)))
    }
}

#[cfg(test)]
mod tests {
    use super::{DbError, fake};

    /// Retryable: PostgreSQL's serialization failure and deadlock, SQLite's
    /// busy and locked with their extended codes; nothing else.
    #[test]
    fn retryable_codes() {
        for code in ["40001", "40P01", "5", "517", "6", "262"] {
            assert!(
                DbError::from(fake::coded(Some(code))).is_retryable(),
                "{code}"
            );
        }
        for code in [Some("23505"), Some("19"), Some("XX000"), None] {
            assert!(!DbError::from(fake::coded(code)).is_retryable(), "{code:?}");
        }
        assert!(!DbError::from(sqlx::Error::PoolTimedOut).is_retryable());
        assert!(DbError::from(sqlx::Error::PoolTimedOut).is_timeout());
        assert!(!DbError::from(sqlx::Error::PoolClosed).is_unique_violation());
    }

    /// PostgreSQL's 23505 is a unique violation, through `DbError` and
    /// through `renox::Error`, also when the raw sqlx error was converted.
    #[test]
    fn unique_violations_through_every_wrapper() {
        assert!(DbError::from(fake::unique()).is_unique_violation());
        assert!(crate::Error::from(DbError::from(fake::unique())).is_unique_violation());
        let raw = crate::Error::from(anyhow::Error::from(fake::unique()));
        assert!(raw.is_unique_violation());
        assert!(!crate::Error::from(anyhow::anyhow!("no")).is_unique_violation());
        assert!(!crate::Error::NotFound.is_unique_violation());
    }
}

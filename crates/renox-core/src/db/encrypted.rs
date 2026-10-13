//! `Encrypted<T>`: a model field stored encrypted with `APP_KEY`.

use std::cell::RefCell;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use cookie::Key;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{DbValue, ToDbValue};

/// A field stored encrypted (AES-256-GCM under `APP_KEY`), read and written
/// as a plain `T` in Rust: a national id number, a bank account, a
/// third-party API secret. Laravel's `encrypted` cast.
///
/// ```
/// # use renox::prelude::*;
/// use renox::db::Encrypted;
///
/// #[derive(Model, serde::Serialize, Default)]
/// #[model(table = "suppliers")]
/// struct Supplier {
///     id: i64,
///     name: String,
///     bank_account: Encrypted<String>, // a TEXT column holding the sealed value
///     api_key: Option<Encrypted<String>>,
/// }
///
/// # async fn demo(db: Db) -> Result {
/// let supplier = Supplier::create(&db, Supplier {
///     name: "Corner Coffee".into(),
///     bank_account: Encrypted::new("BCA 123-456-789".into()),
///     ..Default::default()
/// }).await?;
/// assert_eq!(*supplier.bank_account, "BCA 123-456-789"); // `Deref` to the value
/// # Ok(()) }
/// ```
///
/// The value is sealed with the key of the [`Db`](super::Db) that writes it
/// and opened with the key of the one that reads it (the app's `APP_KEY`,
/// set at boot), so it works the same in handlers, jobs, commands, seeders
/// and tests. Each write uses a fresh nonce, so the column can't be searched
/// or indexed: look rows up by another column. Changing `APP_KEY` makes the
/// values unreadable. `T` is stored as JSON, so any serde type works.
///
/// `Debug` prints `Encrypted(..)`, never the value; `Serialize` writes the
/// plain value (for your JSON and templates, where you decide to show it).
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Encrypted<T>(T);

impl<T> Encrypted<T> {
    /// Wraps a plain value; it is encrypted when the model is saved.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// The plain value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Encrypted<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for Encrypted<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Encrypted<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> fmt::Debug for Encrypted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Encrypted(..)")
    }
}

impl<T: Serialize> Serialize for Encrypted<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Encrypted<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self)
    }
}

impl<T: Serialize> ToDbValue for Encrypted<T> {
    /// The plain value as JSON; the statement seals it with its database's
    /// key when it runs.
    fn to_db_value(&self) -> DbValue {
        DbValue::Encrypted(Unsealed(serde_json::to_string(&self.0).unwrap_or_default()))
    }
}

/// The plain value of an [`Encrypted`] field on its way to the database,
/// in [`DbValue::Encrypted`]. Its `Debug` doesn't show it.
#[derive(Clone, PartialEq)]
pub struct Unsealed(pub(crate) String);

impl fmt::Debug for Unsealed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("..")
    }
}

/// Associated data for sealed columns, so a sealed column can't pass as an
/// encrypted cookie or a `state.encrypt` value, or the other way round.
const COLUMN: &str = "renox.column";

/// Seals a column value (for statements, just before they run).
pub(crate) fn seal(key: &Key, plain: &str) -> String {
    let mut jar = cookie::CookieJar::new();
    jar.private_mut(key)
        .add(cookie::Cookie::new(COLUMN, plain.to_owned()));
    jar.get(COLUMN)
        .map(|sealed| sealed.value().to_owned())
        .unwrap_or_default()
}

fn open(key: &Key, sealed: &str) -> Option<String> {
    cookie::CookieJar::new()
        .private(key)
        .decrypt(cookie::Cookie::new(COLUMN, sealed.to_owned()))
        .map(|plain| plain.value().to_owned())
}

thread_local! {
    /// The key of the row being read, while `Row::try_get` decodes a column.
    static READING: RefCell<Option<Arc<Key>>> = const { RefCell::new(None) };
}

/// Runs `decode` with `key` as the one `Encrypted` columns are opened with.
pub(crate) fn reading<R>(key: Option<&Arc<Key>>, decode: impl FnOnce() -> R) -> R {
    let previous = READING.with(|current| current.replace(key.cloned()));
    let result = decode();
    READING.with(|current| *current.borrow_mut() = previous);
    result
}

fn decode_sealed<T: DeserializeOwned>(
    sealed: &str,
) -> Result<Encrypted<T>, sqlx::error::BoxDynError> {
    let key = READING
        .with(|current| current.borrow().clone())
        .ok_or("an Encrypted column was read without a key: read it through the app's Db")?;
    let plain = open(&key, sealed).ok_or(
        "an Encrypted column can't be decrypted with this APP_KEY (changed, or another key)",
    )?;
    Ok(Encrypted(serde_json::from_str(&plain)?))
}

macro_rules! encrypted_column {
    ($db:ty) => {
        impl<T> sqlx::Type<$db> for Encrypted<T> {
            fn type_info() -> <$db as sqlx::Database>::TypeInfo {
                <String as sqlx::Type<$db>>::type_info()
            }

            fn compatible(ty: &<$db as sqlx::Database>::TypeInfo) -> bool {
                <String as sqlx::Type<$db>>::compatible(ty)
            }
        }

        impl<'r, T: DeserializeOwned> sqlx::Decode<'r, $db> for Encrypted<T> {
            fn decode(
                value: <$db as sqlx::Database>::ValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let sealed = <String as sqlx::Decode<$db>>::decode(value)?;
                decode_sealed(&sealed)
            }
        }
    };
}

encrypted_column!(sqlx::sqlite::Sqlite);
#[cfg(feature = "postgres")]
encrypted_column!(sqlx::postgres::Postgres);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_values_open_with_their_key_only() {
        let key = Key::generate();
        let sealed = seal(&key, "\"BCA 123\"");
        assert!(!sealed.contains("BCA"));
        assert_ne!(sealed, seal(&key, "\"BCA 123\""), "a fresh nonce each time");
        let read = reading(Some(&Arc::new(key)), || decode_sealed::<String>(&sealed));
        assert_eq!(read.unwrap().into_inner(), "BCA 123");
        let other = reading(Some(&Arc::new(Key::generate())), || {
            decode_sealed::<String>(&sealed)
        });
        assert!(other.is_err());
        assert!(reading(None, || decode_sealed::<String>(&sealed)).is_err());
        assert_eq!(format!("{:?}", Encrypted::new("secret")), "Encrypted(..)");
    }
}

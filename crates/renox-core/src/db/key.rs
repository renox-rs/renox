//! Primary keys: the type of a model's `id` field.

use std::fmt;
use std::hash::Hash;
use std::str::FromStr;

use super::{DbValue, FromDb, ToDbValue};

/// A type a model's `id` can have: `i64` (the database numbers the rows),
/// [`Ulid`], `uuid::Uuid` (renox's `uuid` feature) or `String`.
/// `#[derive(Model)]` takes the key type from the `id` field:
///
/// ```
/// # use renox::prelude::*;
/// use renox::db::Ulid;
///
/// #[derive(Model, serde::Serialize, Default)]
/// #[model(table = "invoices")]
/// struct Invoice {
///     id: Ulid, // made on insert; TEXT PRIMARY KEY in the migration
///     total: i64,
/// }
///
/// # async fn demo(db: Db, id: Ulid) -> Result {
/// let invoice = Invoice::find_or_404(&db, id).await?;
/// # let _ = invoice; Ok(()) }
/// ```
///
/// A key's empty value (`0`, the nil UUID, an empty string) means "not
/// saved yet": `save` inserts such a model, making the key (a new ULID or
/// UUID v7) unless the database does (`i64`). A `String` key is yours to
/// set, so save a new row with [`Model::insert`](super::Model::insert) or
/// [`Model::create`](super::Model::create).
///
/// The trait is sealed: other types can't be keys.
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be a model's primary key",
    label = "the `id` field's type",
    note = "use `i64`, `renox::db::Ulid`, `uuid::Uuid` (renox's `uuid` feature) or `String`"
)]
pub trait ModelKey:
    sealed::Sealed
    + ToDbValue
    + FromDb
    + Clone
    + Eq
    + Ord
    + Hash
    + fmt::Debug
    + fmt::Display
    + FromStr
    + Default
    + Send
    + Sync
    + Unpin
    + 'static
{
    /// Whether this is the key of a model that isn't saved yet.
    fn is_unsaved(&self) -> bool;

    /// A new key for an insert, or `None` when the database makes it
    /// (auto-increment) or the app must set it (`String`).
    fn generate() -> Option<Self>;

    /// The database makes the key on insert (`RETURNING id`).
    const AUTO_INCREMENT: bool = false;
}

mod sealed {
    /// Only Renox's key types: the trait may grow without breaking apps.
    pub trait Sealed {}
    impl Sealed for i64 {}
    impl Sealed for String {}
    impl Sealed for super::Ulid {}
    #[cfg(feature = "uuid")]
    impl Sealed for uuid::Uuid {}
}

impl ModelKey for i64 {
    const AUTO_INCREMENT: bool = true;

    fn is_unsaved(&self) -> bool {
        *self == 0
    }

    fn generate() -> Option<Self> {
        None
    }
}

impl ModelKey for String {
    fn is_unsaved(&self) -> bool {
        self.is_empty()
    }

    fn generate() -> Option<Self> {
        None
    }
}

#[cfg(feature = "uuid")]
impl ModelKey for uuid::Uuid {
    fn is_unsaved(&self) -> bool {
        self.is_nil()
    }

    /// Version 7: time-ordered, so new rows land at the end of the index and
    /// cursor pages follow creation order.
    fn generate() -> Option<Self> {
        Some(uuid::Uuid::now_v7())
    }
}

impl ModelKey for Ulid {
    fn is_unsaved(&self) -> bool {
        self.0.is_empty()
    }

    fn generate() -> Option<Self> {
        Some(Ulid::new())
    }
}

/// A [ULID](https://github.com/ulid/spec): 26 characters (`01J9Z3…`), sortable
/// by creation time, stored as text. A good public id: short in URLs, and it
/// doesn't reveal how many rows a table has. `Ulid::default()` is the empty
/// value of an unsaved model.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ulid(String);

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl Ulid {
    /// A new ULID: the current time in milliseconds, then 80 random bits.
    /// Within one millisecond the random part counts up (the spec's
    /// monotonic mode), so ULIDs made by this process sort in the order they
    /// were made.
    pub fn new() -> Self {
        static LAST: std::sync::Mutex<(u128, u128)> = std::sync::Mutex::new((0, 0));
        const RANDOM_BITS: u128 = (1 << 80) - 1;
        let now = u128::try_from(crate::clock::unix_millis()).unwrap_or(0) & ((1 << 48) - 1);
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let (millis, random) = if now <= last.0 && last.1 < RANDOM_BITS {
            (last.0, last.1 + 1)
        } else {
            let mut bytes = [0u8; 10];
            rand::fill(&mut bytes);
            let random = bytes
                .iter()
                .fold(0u128, |acc, byte| (acc << 8) | u128::from(*byte));
            (now.max(last.0), random)
        };
        *last = (millis, random);
        drop(last);
        let value = (millis << 80) | random;
        let text = (0..26)
            .rev()
            .map(|i| CROCKFORD[((value >> (i * 5)) & 31) as usize] as char)
            .collect();
        Self(text)
    }

    /// The ULID as text; empty for an unsaved model.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The text isn't a ULID (26 Crockford base-32 characters).
#[derive(Debug)]
#[non_exhaustive]
pub struct InvalidUlid;

impl fmt::Display for InvalidUlid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a ULID")
    }
}

impl std::error::Error for InvalidUlid {}

impl FromStr for Ulid {
    type Err = InvalidUlid;

    /// Accepts lower case, as the spec asks; an empty string is the
    /// unsaved value.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Ok(Self::default());
        }
        let upper = text.to_ascii_uppercase();
        let valid = upper.len() == 26
            && upper.as_bytes()[0] <= b'7'
            && upper.bytes().all(|b| CROCKFORD.contains(&b));
        if valid {
            Ok(Self(upper))
        } else {
            Err(InvalidUlid)
        }
    }
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ulid({})", self.0)
    }
}

impl serde::Serialize for Ulid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Ulid {
    /// From a path segment, a form field or JSON: an invalid ULID is an
    /// error (a 404 through `renox::Path`).
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl ToDbValue for Ulid {
    fn to_db_value(&self) -> DbValue {
        DbValue::Text(self.0.clone())
    }
}

crate::__db_text_type_for!(Ulid, sqlx::sqlite::Sqlite);
#[cfg(feature = "postgres")]
crate::__db_text_type_for!(Ulid, sqlx::postgres::Postgres);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulids_are_26_sortable_characters() {
        let first = Ulid::new();
        assert_eq!(first.as_str().len(), 26);
        assert!(first.as_str().bytes().all(|b| CROCKFORD.contains(&b)));
        let second = Ulid::new(); // most likely within the same millisecond
        assert!(second > first, "{second} after {first}");
        let many: Vec<Ulid> = (0..1000).map(|_| Ulid::new()).collect();
        assert!(many.windows(2).all(|w| w[0] < w[1]), "monotonic");
        assert_eq!(first.to_string().parse::<Ulid>().unwrap(), first);
        assert_eq!(
            first.as_str().to_lowercase().parse::<Ulid>().unwrap(),
            first
        );
        assert!("not-a-ulid".parse::<Ulid>().is_err());
        assert!(
            "81J9Z3ABCDEFGHJKMNPQRSTVWX".parse::<Ulid>().is_err(),
            "past 2^48 ms"
        );
        assert!(Ulid::default().is_unsaved());
        assert!(!first.is_unsaved());
    }

    // #254: an empty key is the unsaved one; Display is the text form.
    #[test]
    fn empty_text_is_the_unsaved_ulid() {
        let empty: Ulid = "".parse().unwrap();
        assert_eq!(empty, Ulid::default());
        let id: Ulid = "01arz3ndektsv4rrffq69g5fav".parse().unwrap();
        assert_eq!(id.to_string(), "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }
}

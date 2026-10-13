use chrono::{FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

/// A value bound to a query parameter.
///
/// SQLite stores booleans as `0`/`1` and dates as text; PostgreSQL gets them
/// with their own types (`BOOLEAN`, `TIMESTAMPTZ`, `TIMESTAMP`, `DATE`,
/// `TIME`), so the same model works on both.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DbValue {
    /// SQL `NULL`.
    Null,
    /// A 64-bit integer (`INTEGER` on SQLite, `BIGINT` on PostgreSQL).
    Integer(i64),
    /// A 64-bit float (`REAL` on SQLite, `DOUBLE PRECISION` on PostgreSQL).
    Real(f64),
    /// Text.
    Text(String),
    /// Bytes (`BLOB` on SQLite, `BYTEA` on PostgreSQL).
    Blob(Vec<u8>),
    /// A boolean (`0`/`1` on SQLite, `BOOLEAN` on PostgreSQL).
    Bool(bool),
    /// A point in time with its offset (`TIMESTAMPTZ` on PostgreSQL).
    DateTime(chrono::DateTime<FixedOffset>),
    /// A date and time without a zone (`TIMESTAMP` on PostgreSQL).
    NaiveDateTime(NaiveDateTime),
    /// A date (`YYYY-MM-DD` text on SQLite, `DATE` on PostgreSQL).
    Date(NaiveDate),
    /// A time of day (text on SQLite, `TIME` on PostgreSQL).
    Time(NaiveTime),
    /// Text on SQLite; JSON on PostgreSQL (fits `JSONB` and `TEXT` columns).
    Json(serde_json::Value),
    /// A 16-byte BLOB on SQLite; `UUID` on PostgreSQL.
    #[cfg(feature = "uuid")]
    Uuid(uuid::Uuid),
    /// An [`Encrypted`](super::Encrypted) field's value, sealed with the
    /// database's key when the statement runs (text in the column).
    Encrypted(super::encrypted::Unsealed),
}

impl DbValue {
    /// The value as JSON, as templates and APIs would see it.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        use serde_json::Value;
        match self {
            DbValue::Null => Value::Null,
            DbValue::Integer(v) => Value::from(*v),
            DbValue::Real(v) => Value::from(*v),
            DbValue::Text(v) => Value::from(v.clone()),
            DbValue::Blob(v) => Value::from(v.clone()),
            DbValue::Bool(v) => Value::from(*v),
            DbValue::DateTime(v) => Value::from(v.to_rfc3339()),
            DbValue::NaiveDateTime(v) => Value::from(v.format("%Y-%m-%dT%H:%M:%S").to_string()),
            DbValue::Date(v) => Value::from(v.to_string()),
            DbValue::Time(v) => Value::from(v.to_string()),
            DbValue::Json(v) => v.clone(),
            #[cfg(feature = "uuid")]
            DbValue::Uuid(v) => Value::from(v.to_string()),
            DbValue::Encrypted(_) => Value::from("[encrypted]"),
        }
    }

    /// The value as SQLite stores it: booleans as integers, dates in the text
    /// formats sqlx uses, so values written through models and through raw
    /// sqlx queries compare equal.
    pub(crate) fn for_sqlite(self) -> DbValue {
        match self {
            DbValue::Bool(v) => DbValue::Integer(i64::from(v)),
            DbValue::DateTime(v) => DbValue::Text(v.format("%F %T%.f%:z").to_string()),
            DbValue::NaiveDateTime(v) => DbValue::Text(v.format("%F %T%.f").to_string()),
            DbValue::Date(v) => DbValue::Text(v.format("%F").to_string()),
            DbValue::Time(v) => DbValue::Text(v.format("%T%.f").to_string()),
            DbValue::Json(v) => DbValue::Text(v.to_string()),
            #[cfg(feature = "uuid")]
            DbValue::Uuid(v) => DbValue::Blob(v.as_bytes().to_vec()),
            other => other,
        }
    }
}

/// Converts a Rust value into a query parameter. Implemented for the usual
/// scalar types, `Option`, chrono dates and `serde_json::Value`; implement it
/// for your own types to use them in models and `where_*` filters.
pub trait ToDbValue {
    /// The value to bind for `self`.
    fn to_db_value(&self) -> DbValue;
}

macro_rules! integer {
    ($($t:ty),*) => {$(
        impl ToDbValue for $t {
            fn to_db_value(&self) -> DbValue {
                DbValue::Integer(i64::from(*self))
            }
        }
    )*};
}

integer!(i8, i16, i32, i64, u8, u16, u32);

impl ToDbValue for bool {
    fn to_db_value(&self) -> DbValue {
        DbValue::Bool(*self)
    }
}

impl ToDbValue for f32 {
    fn to_db_value(&self) -> DbValue {
        DbValue::Real(f64::from(*self))
    }
}

impl ToDbValue for f64 {
    fn to_db_value(&self) -> DbValue {
        DbValue::Real(*self)
    }
}

impl ToDbValue for str {
    fn to_db_value(&self) -> DbValue {
        DbValue::Text(self.to_owned())
    }
}

impl ToDbValue for String {
    fn to_db_value(&self) -> DbValue {
        DbValue::Text(self.clone())
    }
}

impl ToDbValue for Vec<u8> {
    fn to_db_value(&self) -> DbValue {
        DbValue::Blob(self.clone())
    }
}

impl ToDbValue for serde_json::Value {
    fn to_db_value(&self) -> DbValue {
        DbValue::Json(self.clone())
    }
}

#[cfg(feature = "uuid")]
impl ToDbValue for uuid::Uuid {
    fn to_db_value(&self) -> DbValue {
        DbValue::Uuid(*self)
    }
}

impl<Tz: TimeZone> ToDbValue for chrono::DateTime<Tz> {
    fn to_db_value(&self) -> DbValue {
        DbValue::DateTime(self.fixed_offset())
    }
}

impl ToDbValue for NaiveDateTime {
    fn to_db_value(&self) -> DbValue {
        DbValue::NaiveDateTime(*self)
    }
}

impl ToDbValue for NaiveDate {
    fn to_db_value(&self) -> DbValue {
        DbValue::Date(*self)
    }
}

impl ToDbValue for NaiveTime {
    fn to_db_value(&self) -> DbValue {
        DbValue::Time(*self)
    }
}

impl<T: ToDbValue> ToDbValue for Option<T> {
    fn to_db_value(&self) -> DbValue {
        self.as_ref().map_or(DbValue::Null, ToDbValue::to_db_value)
    }
}

impl<T: ToDbValue + ?Sized> ToDbValue for &T {
    fn to_db_value(&self) -> DbValue {
        (**self).to_db_value()
    }
}

impl ToDbValue for DbValue {
    fn to_db_value(&self) -> DbValue {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // #254: every variant as JSON (User's extra columns and the grid show
    // them this way), and the less common ToDbValue impls.
    #[test]
    fn every_value_has_a_json_form() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-01T09:30:00Z").unwrap();
        let local = at.naive_utc();
        let cases = [
            (DbValue::Null, json!(null)),
            (DbValue::Integer(7), json!(7)),
            (DbValue::Real(1.5), json!(1.5)),
            (DbValue::Text("a".into()), json!("a")),
            (DbValue::Blob(vec![1, 2]), json!([1, 2])),
            (DbValue::Bool(true), json!(true)),
            (DbValue::DateTime(at), json!("2026-10-01T09:30:00+00:00")),
            (DbValue::NaiveDateTime(local), json!("2026-10-01T09:30:00")),
            (DbValue::Date(local.date()), json!("2026-10-01")),
            (DbValue::Time(local.time()), json!("09:30:00")),
            (DbValue::Json(json!({"a": 1})), json!({"a": 1})),
            (
                DbValue::Encrypted(super::super::encrypted::Unsealed("secret".into())),
                json!("[encrypted]"),
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(value.to_json(), expected, "{value:?}");
        }
    }

    #[test]
    fn floats_times_and_bytes_become_values() {
        assert!(matches!(1.5f32.to_db_value(), DbValue::Real(v) if v == 1.5));
        let time = chrono::NaiveTime::from_hms_opt(9, 30, 0).unwrap();
        assert!(matches!(time.to_db_value(), DbValue::Time(t) if t == time));
        assert!(matches!(vec![1u8, 2].to_db_value(), DbValue::Blob(b) if b == [1, 2]));
    }
}

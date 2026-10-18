use std::ops::{Deref, DerefMut};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::error::BoxDynError;
use sqlx::sqlite::{Sqlite, SqliteTypeInfo, SqliteValueRef};

use super::column::{Condition, JsonOp};
use super::{Col, DbValue, ToDbValue};

/// A model field stored as JSON: `TEXT` on SQLite, `JSONB` (or `JSON`,
/// `TEXT`) on PostgreSQL. Serializes as the value itself, so templates and
/// APIs see the list or object.
///
/// ```
/// # use renox::prelude::*;
/// use renox::db::Json;
///
/// #[derive(Model, serde::Serialize, Default)]
/// #[model(table = "products")]
/// struct Product {
///     id: i64,
///     tags: Json<Vec<String>>, // tags TEXT (SQLite) / tags JSONB (PostgreSQL)
/// }
///
/// let product = Product { tags: Json(vec!["coffee".into()]), ..Default::default() };
/// assert_eq!(product.tags.len(), 1); // derefs to the Vec
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Json<T>(pub T);

impl<T> Deref for Json<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Json<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> From<T> for Json<T> {
    fn from(value: T) -> Self {
        Json(value)
    }
}

impl<T: Serialize> ToDbValue for Json<T> {
    fn to_db_value(&self) -> DbValue {
        DbValue::Json(serde_json::to_value(&self.0).unwrap_or(serde_json::Value::Null))
    }
}

impl<T> sqlx::Type<Sqlite> for Json<T> {
    fn type_info() -> SqliteTypeInfo {
        <String as sqlx::Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <String as sqlx::Type<Sqlite>>::compatible(ty)
    }
}

impl<'r, T: DeserializeOwned> sqlx::Decode<'r, Sqlite> for Json<T> {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = <String as sqlx::Decode<Sqlite>>::decode(value)?;
        Ok(Json(serde_json::from_str(&text)?))
    }
}

#[cfg(feature = "postgres")]
mod postgres {
    use serde::de::DeserializeOwned;
    use sqlx::error::BoxDynError;
    use sqlx::postgres::{PgTypeInfo, PgValueRef, Postgres};
    use sqlx::{TypeInfo, ValueRef};

    use super::Json;

    impl<T> sqlx::Type<Postgres> for Json<T> {
        fn type_info() -> PgTypeInfo {
            PgTypeInfo::with_name("jsonb")
        }

        fn compatible(ty: &PgTypeInfo) -> bool {
            matches!(ty.name(), "JSON" | "JSONB")
                || <String as sqlx::Type<Postgres>>::compatible(ty)
        }
    }

    impl<'r, T: DeserializeOwned> sqlx::Decode<'r, Postgres> for Json<T> {
        fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
            let is_json = matches!(value.type_info().name(), "JSON" | "JSONB");
            if is_json {
                let sqlx::types::Json(inner) =
                    <sqlx::types::Json<T> as sqlx::Decode<Postgres>>::decode(value)?;
                Ok(Json(inner))
            } else {
                let text = <String as sqlx::Decode<Postgres>>::decode(value)?;
                Ok(Json(serde_json::from_str(&text)?))
            }
        }
    }
}

/// The `CREATE INDEX` statement that speeds up filters and sorting on one key of
/// a JSON column ([`Col::json`]), in the SQL of `dialect`: an expression
/// index on `json_extract(column, '$.key')` (SQLite) or
/// `(column::jsonb ->> 'key')` (PostgreSQL), the expressions the query builder
/// writes. Run it from a migration; nothing creates it for you.
///
/// `table` and `column` must be plain identifiers and `key` must match
/// `[a-z][a-z0-9_]*`, else an error is returned.
///
/// ```
/// use renox::db::{Dialect, json_index};
///
/// let sql = json_index(Dialect::Sqlite, "parties", "extra", "segment").unwrap();
/// assert!(sql.contains("json_extract(\"extra\", '$.segment')"));
/// assert!(json_index(Dialect::Postgres, "parties", "extra", "a'b").is_err());
/// ```
pub fn json_index(
    dialect: super::Dialect,
    table: &str,
    column: &str,
    key: &str,
) -> crate::Result<String> {
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !plain(table) || !plain(column) || !crate::custom_fields::is_valid_key(key) {
        return Err(crate::Error::BadRequest(format!(
            "cannot index `{table}.{column}` key `{}`",
            key.escape_debug()
        )));
    }
    let expr = match dialect {
        super::Dialect::Sqlite => format!("json_extract(\"{column}\", '$.{key}')"),
        super::Dialect::Postgres => format!("((\"{column}\"::jsonb ->> '{key}'))"),
    };
    Ok(format!(
        "CREATE INDEX IF NOT EXISTS \"idx_{table}_{column}_{key}\" ON \"{table}\" ({expr})"
    ))
}

impl<M, T> Col<M, Json<T>> {
    /// One key of the JSON object stored in this column, to filter or sort by.
    ///
    /// The SQL is `json_extract(column, '$.key')` on SQLite and
    /// `column ->> 'key'` on PostgreSQL; values are always bound. A key that
    /// does not match `[a-z][a-z0-9_]*` makes the query return an error and
    /// is never put into SQL. Call
    /// [`Query::with_custom_fields`](super::Query::with_custom_fields) to check
    /// the key against the model's custom field definitions and to compare
    /// numbers and booleans with the right type on PostgreSQL.
    ///
    /// ```
    /// use renox::db::{Col, Json};
    /// use renox::custom_fields::CustomValues;
    ///
    /// struct Party;
    /// const EXTRA: Col<Party, Json<CustomValues>> = Col::new("extra");
    ///
    /// let retail = EXTRA.json("segment").eq("retail");
    /// # let _ = retail;
    /// ```
    pub fn json(self, key: &str) -> JsonPath<M> {
        JsonPath {
            column: self.name(),
            key: key.to_owned(),
            _p: std::marker::PhantomData,
        }
    }
}

/// One key of a JSON column of the model `M`, made by [`Col::json`].
pub struct JsonPath<M> {
    pub(super) column: &'static str,
    pub(super) key: String,
    _p: std::marker::PhantomData<fn() -> M>,
}

impl<M> std::fmt::Debug for JsonPath<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("JsonPath")
            .field(&self.column)
            .field(&self.key)
            .finish()
    }
}

macro_rules! json_comparison {
    ($($(#[$doc:meta])* $method:ident => $op:literal),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $method(self, value: impl ToDbValue) -> Condition<M> {
                Condition::json(
                    self.column,
                    self.key,
                    JsonOp::Cmp($op),
                    vec![value.to_db_value()],
                )
            }
        )*
    };
}

impl<M> JsonPath<M> {
    json_comparison! {
        /// `key = value`.
        eq => "=",
        /// `key != value`.
        ne => "!=",
        /// `key < value`.
        lt => "<",
        /// `key <= value`.
        lte => "<=",
        /// `key > value`.
        gt => ">",
        /// `key >= value`.
        gte => ">=",
    }

    /// `key LIKE pattern`, ignoring ASCII case on both databases.
    pub fn like(self, pattern: &str) -> Condition<M> {
        Condition::json(
            self.column,
            self.key,
            JsonOp::Cmp("like"),
            vec![pattern.to_db_value()],
        )
    }

    /// The key is missing or `null`.
    pub fn is_null(self) -> Condition<M> {
        Condition::json(self.column, self.key, JsonOp::Null(true), Vec::new())
    }

    /// The key holds a value.
    pub fn is_not_null(self) -> Condition<M> {
        Condition::json(self.column, self.key, JsonOp::Null(false), Vec::new())
    }

    /// `key IN (...)`; an empty list matches no rows.
    pub fn is_in<V: ToDbValue>(self, values: impl IntoIterator<Item = V>) -> Condition<M> {
        let values = values.into_iter().map(|v| v.to_db_value()).collect();
        Condition::json(self.column, self.key, JsonOp::In, values)
    }
}

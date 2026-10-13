use std::ops::{Deref, DerefMut};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::error::BoxDynError;
use sqlx::sqlite::{Sqlite, SqliteTypeInfo, SqliteValueRef};

use super::{DbValue, ToDbValue};

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

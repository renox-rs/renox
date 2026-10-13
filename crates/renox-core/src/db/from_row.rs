use super::{DbError, FromDb, Row};

/// A type read from a query's rows: models (`derive(Model)`), structs with
/// `#[derive(FromRow)]`, and tuples of columns by position.
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(db: Db) -> Result {
/// #[derive(renox::FromRow, serde::Serialize)]
/// struct Sales {
///     category: String,
///     total: i64,
/// }
///
/// let sales: Vec<Sales> = renox::db::sql(
///     "SELECT c.name AS category, CAST(SUM(p.price) AS BIGINT) AS total \
///      FROM products p JOIN categories c ON c.id = p.category_id GROUP BY c.name",
/// )
/// .fetch_as(&db)
/// .await?;
/// let pairs: Vec<(i64, String)> = renox::db::sql("SELECT id, name FROM products")
///     .fetch_as(&db)
///     .await?;
/// # let _ = (sales, pairs); Ok(()) }
/// ```
pub trait FromRow: Sized {
    /// Decodes one row, by column name or position.
    fn from_row(row: &Row) -> Result<Self, DbError>;
}

macro_rules! tuple {
    ($($t:ident $i:tt),+) => {
        impl<$($t: FromDb),+> FromRow for ($($t,)+) {
            fn from_row(row: &Row) -> Result<Self, DbError> {
                Ok(($(row.try_get::<$t>($i)?,)+))
            }
        }
    };
}

tuple!(A 0);
tuple!(A 0, B 1);
tuple!(A 0, B 1, C 2);
tuple!(A 0, B 1, C 2, D 3);
tuple!(A 0, B 1, C 2, D 3, E 4);
tuple!(A 0, B 1, C 2, D 3, E 4, F 5);

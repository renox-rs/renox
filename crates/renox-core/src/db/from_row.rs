use super::{DbError, FromDb, Row};
use crate::money::{Currency, Money};

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
tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);

impl Row {
    /// Reads a [`Money`] from two columns: the amount in minor units
    /// (`BIGINT`) and its ISO 4217 code (`TEXT`). A code that is not a known
    /// currency is a decode error.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # async fn demo(db: Db) -> Result {
    /// let row = renox::db::sql("SELECT 1250 AS total, 'USD' AS total_currency")
    ///     .fetch_one(&db)
    ///     .await?;
    /// let total = row.try_get_money("total", "total_currency")?;
    /// assert_eq!(total, renox::money::Money::usd(1250));
    /// # Ok(()) }
    /// ```
    pub fn try_get_money(&self, amount: &str, currency: &str) -> Result<Money, DbError> {
        let minor: i64 = self.try_get(amount)?;
        let code: String = self.try_get(currency)?;
        money_from_parts(minor, &code, currency)
    }

    /// Like [`Row::try_get_money`] for a nullable pair: `None` when both
    /// columns are `NULL`, an error when only one of them is.
    pub fn try_get_money_opt(
        &self,
        amount: &str,
        currency: &str,
    ) -> Result<Option<Money>, DbError> {
        let minor: Option<i64> = self.try_get(amount)?;
        let code: Option<String> = self.try_get(currency)?;
        match (minor, code) {
            (None, None) => Ok(None),
            (Some(minor), Some(code)) => money_from_parts(minor, &code, currency).map(Some),
            _ => Err(decode_error(format!(
                "`{amount}` and `{currency}` must be both set or both NULL"
            ))),
        }
    }
}

fn money_from_parts(minor: i64, code: &str, column: &str) -> Result<Money, DbError> {
    let currency =
        Currency::parse(code).map_err(|e| decode_error(format!("column `{column}`: {e}")))?;
    Ok(Money { minor, currency })
}

fn decode_error(message: String) -> DbError {
    DbError::from(sqlx::Error::Decode(message.into()))
}

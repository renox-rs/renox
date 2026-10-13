//! Typed columns: a [`Col`] names one column of a model and carries its Rust type, and its
//! comparison methods build a [`Condition`] for [`Query::where_`](super::Query::where_).

use std::fmt;
use std::marker::PhantomData;

use super::value::{DbValue, ToDbValue};

/// One column of the model `M`, whose Rust type is `T` (for example `Option<String>`).
///
/// A misspelled column or a value of the wrong type is a compile error:
///
/// ```
/// use renox::db::Col;
///
/// struct Product;
/// const PRICE: Col<Product, i64> = Col::new("price");
///
/// let cheap = PRICE.lt(20_000);
/// # let _ = cheap;
/// ```
pub struct Col<M, T> {
    name: &'static str,
    _p: PhantomData<fn() -> (M, T)>,
}

impl<M, T> Clone for Col<M, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, T> Copy for Col<M, T> {}

impl<M, T> fmt::Debug for Col<M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Col").field(&self.name).finish()
    }
}

impl<M, T> Col<M, T> {
    /// A column named `name`.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _p: PhantomData,
        }
    }

    /// The column's name.
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

/// Anything that names a column of the model `M`: a string, or a typed [`Col`] of `M`.
///
/// Taken by `order_by`, `order_by_desc` and `pluck`. Implemented for `&str`, `&&str`,
/// `String`, `&String` and `Col<M, T>`.
pub trait IntoColumn<M> {
    /// The column's name.
    fn column_name(&self) -> &str;
}

impl<M> IntoColumn<M> for &str {
    fn column_name(&self) -> &str {
        self
    }
}

impl<M> IntoColumn<M> for &&str {
    fn column_name(&self) -> &str {
        self
    }
}

impl<M> IntoColumn<M> for String {
    fn column_name(&self) -> &str {
        self
    }
}

impl<M> IntoColumn<M> for &String {
    fn column_name(&self) -> &str {
        self
    }
}

impl<M, T> IntoColumn<M> for Col<M, T> {
    fn column_name(&self) -> &str {
        self.name
    }
}

/// Says that a column of type `Self` can be compared with a value of type `V`.
///
/// Implemented by Renox only, for a type and itself, an `Option` and its inner type, and
/// `String` / `Option<String>` with `&str`.
pub trait Comparable<V> {}

impl<T: ToDbValue> Comparable<T> for T {}
impl<T: ToDbValue> Comparable<T> for Option<T> {}
impl Comparable<&str> for String {}
impl Comparable<&str> for Option<String> {}

/// A filter on the model `M`, made by a [`Col`] and given to
/// [`Query::where_`](super::Query::where_).
#[non_exhaustive]
pub struct Condition<M> {
    pub(super) kind: CondKind,
    _p: PhantomData<fn() -> M>,
}

pub(super) enum CondKind {
    Op(&'static str, &'static str, DbValue),
    Null(&'static str, bool),
    In(&'static str, Vec<DbValue>),
}

impl<M> fmt::Debug for Condition<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Condition")
    }
}

impl<M> Condition<M> {
    fn new(kind: CondKind) -> Self {
        Self {
            kind,
            _p: PhantomData,
        }
    }
}

macro_rules! comparison {
    ($($(#[$doc:meta])* $method:ident => $op:literal),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $method<V: ToDbValue>(self, value: V) -> Condition<M>
            where
                T: Comparable<V>,
            {
                Condition::new(CondKind::Op(self.name, $op, value.to_db_value()))
            }
        )*
    };
}

impl<M, T> Col<M, T> {
    comparison! {
        /// `column = value`.
        eq => "=",
        /// `column != value`.
        ne => "!=",
        /// `column < value`.
        lt => "<",
        /// `column <= value`.
        lte => "<=",
        /// `column > value`.
        gt => ">",
        /// `column >= value`.
        gte => ">=",
    }

    /// `column IS NULL`.
    pub fn is_null(self) -> Condition<M> {
        Condition::new(CondKind::Null(self.name, true))
    }

    /// `column IS NOT NULL`.
    pub fn is_not_null(self) -> Condition<M> {
        Condition::new(CondKind::Null(self.name, false))
    }

    /// `column IN (…)`; an empty list matches no rows.
    pub fn is_in<V: ToDbValue>(self, values: impl IntoIterator<Item = V>) -> Condition<M>
    where
        T: Comparable<V>,
    {
        let values = values.into_iter().map(|v| v.to_db_value()).collect();
        Condition::new(CondKind::In(self.name, values))
    }
}

impl<M> Col<M, String> {
    /// `column LIKE pattern`, ignoring ASCII case on both databases.
    pub fn like(self, pattern: &str) -> Condition<M> {
        Condition::new(CondKind::Op(self.name, "like", pattern.to_db_value()))
    }
}

impl<M> Col<M, Option<String>> {
    /// `column LIKE pattern`, ignoring ASCII case on both databases.
    pub fn like(self, pattern: &str) -> Condition<M> {
        Condition::new(CondKind::Op(self.name, "like", pattern.to_db_value()))
    }
}

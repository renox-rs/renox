//! A runtime description of a model's columns: the kind of each field's Rust
//! type, which `db:check` compares with the real table.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

use super::{Encrypted, Json, Ulid};

/// The kind of value a model field stores, as far as the database cares.
///
/// `Unknown` is a field type Renox has no [`ColumnType`] for (a custom type);
/// checks skip it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColumnKind {
    /// 64-bit integer (`i64`, `u32`).
    BigInt,
    /// 32-bit integer (`i32`, `u16`).
    Int,
    /// 16-bit integer or smaller (`i8`, `i16`, `u8`).
    SmallInt,
    /// `f64`.
    Double,
    /// `f32`.
    Real,
    /// Text (`String`, `Ulid`, `Encrypted<T>`, enums).
    Text,
    /// Bytes (`Vec<u8>`).
    Blob,
    /// `bool`.
    Bool,
    /// `chrono::DateTime<Tz>`.
    DateTime,
    /// `chrono::NaiveDateTime`.
    NaiveDateTime,
    /// `chrono::NaiveDate`.
    Date,
    /// `chrono::NaiveTime`.
    Time,
    /// `serde_json::Value` and `Json<T>`.
    Json,
    /// `uuid::Uuid`.
    Uuid,
    /// A type Renox doesn't know.
    Unknown,
}

impl ColumnKind {
    /// A lowercase name for messages.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BigInt => "bigint",
            Self::Int => "int",
            Self::SmallInt => "smallint",
            Self::Double => "double",
            Self::Real => "real",
            Self::Text => "text",
            Self::Blob => "blob",
            Self::Bool => "bool",
            Self::DateTime => "datetime",
            Self::NaiveDateTime => "naive datetime",
            Self::Date => "date",
            Self::Time => "time",
            Self::Json => "json",
            Self::Uuid => "uuid",
            Self::Unknown => "unknown",
        }
    }
}

/// A Rust type that can be a model field, and the kind of column it fills.
/// Implemented for every type with a `ToDbValue` impl, plus `Option<T>`,
/// `Json<T>`, `Encrypted<T>` and `Ulid`.
pub trait ColumnType {
    /// The kind of column this type is stored in.
    const KIND: ColumnKind;
}

macro_rules! kinds {
    ($($kind:ident => $($t:ty),*;)*) => {$($(
        impl ColumnType for $t {
            const KIND: ColumnKind = ColumnKind::$kind;
        }
    )*)*};
}

kinds! {
    BigInt => i64, u32;
    Int => i32, u16;
    SmallInt => i8, i16, u8;
    Double => f64;
    Real => f32;
    Text => String, Ulid;
    Blob => Vec<u8>;
    Bool => bool;
    NaiveDateTime => NaiveDateTime;
    Date => NaiveDate;
    Time => NaiveTime;
    Json => serde_json::Value;
}

#[cfg(feature = "uuid")]
impl ColumnType for uuid::Uuid {
    const KIND: ColumnKind = ColumnKind::Uuid;
}

impl<Tz: TimeZone> ColumnType for chrono::DateTime<Tz> {
    const KIND: ColumnKind = ColumnKind::DateTime;
}

impl<T: ColumnType> ColumnType for Option<T> {
    const KIND: ColumnKind = T::KIND;
}

impl<T> ColumnType for Json<T> {
    const KIND: ColumnKind = ColumnKind::Json;
}

impl<T> ColumnType for Encrypted<T> {
    const KIND: ColumnKind = ColumnKind::Text;
}

/// One column of a model, as the derive describes it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ModelColumn {
    /// The column's name.
    pub name: &'static str,
    /// The field's Rust type, as written.
    pub rust_type: &'static str,
    /// What kind of column the type fills.
    pub kind: ColumnKind,
    /// Whether the field is an `Option`.
    pub nullable: bool,
    /// The SQL default from `#[model(default = "…")]`, if any.
    pub default: Option<&'static str>,
    /// The table from `#[model(references = "…")]`, if any.
    pub references: Option<&'static str>,
}

impl ModelColumn {
    /// Describes one column.
    pub const fn new(
        name: &'static str,
        rust_type: &'static str,
        kind: ColumnKind,
        nullable: bool,
    ) -> Self {
        Self {
            name,
            rust_type,
            kind,
            nullable,
            default: None,
            references: None,
        }
    }

    /// Sets the column's SQL default (`#[model(default = "0")]`).
    pub const fn default_sql(mut self, sql: &'static str) -> Self {
        self.default = Some(sql);
        self
    }

    /// Sets the table the column points at (`#[model(references = "users")]`).
    pub const fn references(mut self, table: &'static str) -> Self {
        self.references = Some(table);
        self
    }
}

/// An index a model declares with `#[model(index(a, b))]` or
/// `#[model(unique(a))]`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ModelIndex {
    /// The indexed columns, in order.
    pub columns: &'static [&'static str],
    /// Whether the index is unique.
    pub unique: bool,
}

impl ModelIndex {
    /// Describes one index.
    pub const fn new(columns: &'static [&'static str], unique: bool) -> Self {
        Self { columns, unique }
    }
}

#[doc(hidden)]
pub struct ColumnProbe<T>(std::marker::PhantomData<fn() -> T>);
impl<T> ColumnProbe<T> {
    pub const fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}
impl<T> Default for ColumnProbe<T> {
    fn default() -> Self {
        Self::new()
    }
}
#[doc(hidden)]
pub trait KnownColumn {
    fn kind(&self) -> ColumnKind;
}
impl<T: ColumnType> KnownColumn for ColumnProbe<T> {
    fn kind(&self) -> ColumnKind {
        T::KIND
    }
}
#[doc(hidden)]
pub trait UnknownColumn {
    fn kind(&self) -> ColumnKind;
}
impl<T> UnknownColumn for &ColumnProbe<T> {
    fn kind(&self) -> ColumnKind {
        ColumnKind::Unknown
    }
}

/// A model registered with `App::model`, for `db:check`.
#[derive(Clone)]
#[allow(dead_code)] // read by the schema comparison (370.4)
pub(crate) struct ModelInfo {
    pub(crate) type_id: std::any::TypeId,
    pub(crate) name: &'static str,
    pub(crate) table: &'static str,
    pub(crate) columns: fn() -> Vec<ModelColumn>,
    pub(crate) indexes: fn() -> Vec<super::ModelIndex>,
}

/// Describes `M` for the registry.
pub(crate) fn model_info<M: super::Model>() -> ModelInfo {
    ModelInfo {
        type_id: std::any::TypeId::of::<M>(),
        name: std::any::type_name::<M>(),
        table: M::TABLE,
        columns: M::column_info,
        indexes: M::indexes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Custom;

    #[test]
    fn kinds_of_known_types() {
        assert_eq!(<i64 as ColumnType>::KIND, ColumnKind::BigInt);
        assert_eq!(<Option<String> as ColumnType>::KIND, ColumnKind::Text);
        assert_eq!(<Json<Vec<i64>> as ColumnType>::KIND, ColumnKind::Json);
        assert_eq!(ColumnKind::BigInt.as_str(), "bigint");
    }

    #[test]
    #[allow(clippy::needless_borrow)]
    fn probe_tells_known_from_unknown() {
        #[allow(unused_imports)]
        use super::{KnownColumn as _, UnknownColumn as _};
        assert_eq!((&ColumnProbe::<i64>::new()).kind(), ColumnKind::BigInt);
        assert_eq!((&ColumnProbe::<Custom>::new()).kind(), ColumnKind::Unknown);
        assert_eq!(
            (&ColumnProbe::<Option<Custom>>::new()).kind(),
            ColumnKind::Unknown
        );
    }
}

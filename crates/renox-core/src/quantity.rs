//! Exact decimal quantities (kilograms, metres, pieces) with the scale in
//! the type.
//!
//! [`Quantity<SCALE>`](Quantity) is a `Copy` newtype over an `i64` holding
//! the value in units of 10^-`SCALE`: `Quantity<3>` counts thousandths.
//! There are no floats. Like [`Money`](crate::money::Money), anything that
//! can lose digits takes an explicit [`Rounding`]; there is no default.
//! A quantity carries no unit: keep the unit in its own field.
//!
//! ```
//! use renox::money::Rounding;
//! use renox::quantity::Quantity;
//!
//! let q = Quantity::<3>::parse("2.5", Rounding::HalfEven).unwrap();
//! assert_eq!(q.raw(), 2500);
//! assert_eq!(q.to_string(), "2.500");
//! assert_eq!(Quantity::<3>::parse("0.0005", Rounding::HalfUp).unwrap().to_string(), "0.001");
//! assert!(Quantity::<3>::parse_exact("0.0005").is_err());
//! ```

use std::collections::BTreeMap;
use std::fmt;

use crate::money::{Rounding, div_round};

/// Why a quantity calculation or parse failed.
///
/// ```
/// use renox::quantity::{Quantity, QuantityError};
///
/// assert_eq!(Quantity::<3>::parse_exact("0.0005").unwrap_err(), QuantityError::Inexact);
/// assert!(matches!(Quantity::<3>::parse_exact("abc"), Err(QuantityError::Invalid(_))));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum QuantityError {
    /// The text is not a decimal number (or a factor).
    Invalid(String),
    /// The result does not fit in an `i64` of scaled units.
    Overflow,
    /// Digits would be lost and no rounding was allowed.
    Inexact,
    /// A unit code that is not in the unit table.
    UnknownUnit(String),
    /// A denominator of zero.
    DivisionByZero,
    /// A unit table that cannot be built (the reason is in the text).
    InvalidUnits(String),
    /// A conversion between units of different bases (kg to m).
    IncompatibleUnits {
        /// The unit converted from.
        from: String,
        /// The unit converted to.
        to: String,
    },
}

impl fmt::Display for QuantityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantityError::Invalid(t) => write!(f, "`{t}` is not a valid quantity"),
            QuantityError::Overflow => f.write_str("the quantity is too large"),
            QuantityError::Inexact => f.write_str("the quantity has more decimals than allowed"),
            QuantityError::UnknownUnit(u) => write!(f, "`{u}` is not a known unit"),
            QuantityError::DivisionByZero => f.write_str("the denominator is zero"),
            QuantityError::InvalidUnits(why) => write!(f, "invalid unit table: {why}"),
            QuantityError::IncompatibleUnits { from, to } => {
                write!(f, "`{from}` cannot be converted to `{to}`: different bases")
            }
        }
    }
}

impl std::error::Error for QuantityError {}

/// Splits `[+-]digits[.digits]` into sign, whole and fraction digits.
/// No exponent, no separators, no empty parts.
fn split_decimal(text: &str) -> Option<(bool, &str, &str)> {
    let t = text.trim();
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (whole, fraction) = t.split_once('.').unwrap_or((t, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) || t.ends_with('.') {
        return None;
    }
    Some((negative, whole, fraction))
}

/// Whether `text` reads as a decimal that fits an `i64` at `scale` digits with
/// nothing rounded (digits beyond the scale must all be zero). For the
/// validation rule, where the scale is only known at run time.
pub(crate) fn fits_scale(text: &str, scale: u32) -> bool {
    let Some((_, whole, fraction)) = split_decimal(text) else {
        return false;
    };
    let scale = scale.min(9) as usize;
    let (kept, rest) = fraction.split_at(fraction.len().min(scale));
    rest.bytes().all(|b| b == b'0') && format!("{whole}{kept:0<scale$}").parse::<i64>().is_ok()
}

/// A decimal quantity stored as an `i64` count of 10^-`SCALE` units.
/// `SCALE` is at most 9, checked at compile time. Adding and subtracting
/// return a `Result`; there are no panicking operators.
///
/// ```
/// use renox::money::Rounding;
/// use renox::quantity::Quantity;
///
/// let a = Quantity::<3>::parse_exact("1.250").unwrap();
/// let b = Quantity::<3>::parse_exact("0.750").unwrap();
/// assert_eq!(a.checked_add(b).unwrap().to_string(), "2.000");
/// assert_eq!(a.mul_decimal("1.15", Rounding::HalfUp).unwrap().to_string(), "1.438");
/// assert!(b < a);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quantity<const SCALE: u32>(i64);

impl<const SCALE: u32> Quantity<SCALE> {
    /// Compile-time check that the scale fits an `i64` (at most 9).
    const VALID: () = assert!(SCALE <= 9, "Quantity SCALE must be 0..=9");

    /// The zero quantity.
    pub const ZERO: Self = Quantity(0);

    /// The quantity of `raw` units of 10^-`SCALE` (`2500` is 2.500 for
    /// `Quantity<3>`).
    ///
    /// ```
    /// use renox::quantity::Quantity;
    ///
    /// assert_eq!(Quantity::<3>::from_raw(2500).to_string(), "2.500");
    /// ```
    pub const fn from_raw(raw: i64) -> Self {
        let () = Self::VALID;
        Quantity(raw)
    }

    /// The value in units of 10^-`SCALE`.
    ///
    /// ```
    /// use renox::quantity::Quantity;
    ///
    /// assert_eq!(Quantity::<2>::from_raw(-150).raw(), -150);
    /// ```
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Reads a decimal such as `"2.5"` or `"-0.125"` with `.` as the
    /// decimal point. Digits beyond the scale are rounded with `rounding`
    /// (`Down` is toward zero, `Up` away from zero). Empty text, signs
    /// other than a leading `-` or `+`, exponents and separators are
    /// `Invalid`; a value that does not fit is `Overflow`.
    ///
    /// ```
    /// use renox::money::Rounding;
    /// use renox::quantity::Quantity;
    ///
    /// let q = |t, r| Quantity::<3>::parse(t, r).unwrap().to_string();
    /// assert_eq!(q("2.5", Rounding::HalfEven), "2.500");
    /// assert_eq!(q("0.0005", Rounding::HalfUp), "0.001");
    /// assert_eq!(q("0.0005", Rounding::HalfEven), "0.000");
    /// assert_eq!(q("-0.0005", Rounding::Up), "-0.001");
    /// assert_eq!(q("0.0009", Rounding::Down), "0.000");
    /// assert!(Quantity::<3>::parse("1e3", Rounding::Down).is_err());
    /// ```
    pub fn parse(text: &str, rounding: Rounding) -> Result<Self, QuantityError> {
        let () = Self::VALID;
        let (negative, whole, fraction) =
            split_decimal(text).ok_or_else(|| QuantityError::Invalid(text.to_string()))?;
        let scale = SCALE as usize;
        let (kept, rest) = if fraction.len() > scale {
            fraction.split_at(scale)
        } else {
            (fraction, "")
        };
        let padded = format!("{whole}{kept:0<scale$}");
        let mut magnitude: i128 = padded.parse().map_err(|_| QuantityError::Overflow)?;
        if rest.bytes().any(|b| b != b'0') {
            let first = rest.as_bytes()[0];
            let beyond = rest[1..].bytes().any(|b| b != b'0');
            let round_away = match rounding {
                Rounding::Down => false,
                Rounding::Up => true,
                Rounding::HalfUp | Rounding::HalfEven => {
                    if first > b'5' || (first == b'5' && beyond) {
                        true
                    } else if first == b'5' {
                        rounding == Rounding::HalfUp || magnitude % 2 != 0
                    } else {
                        false
                    }
                }
            };
            if round_away {
                magnitude += 1;
            }
        }
        let signed = if negative { -magnitude } else { magnitude };
        i64::try_from(signed)
            .map(Quantity)
            .map_err(|_| QuantityError::Overflow)
    }

    /// Reads a decimal and fails with `Inexact` if digits beyond the scale
    /// are not all zero; nothing is rounded.
    ///
    /// ```
    /// use renox::quantity::{Quantity, QuantityError};
    ///
    /// assert_eq!(Quantity::<3>::parse_exact("12.50").unwrap().raw(), 12_500);
    /// assert_eq!(Quantity::<3>::parse_exact("1.2000").unwrap().raw(), 1_200);
    /// assert_eq!(Quantity::<3>::parse_exact("1.0001").unwrap_err(), QuantityError::Inexact);
    /// ```
    pub fn parse_exact(text: &str) -> Result<Self, QuantityError> {
        let down = Self::parse(text, Rounding::Down)?;
        let up = Self::parse(text, Rounding::Up)?;
        if down == up {
            Ok(down)
        } else {
            Err(QuantityError::Inexact)
        }
    }

    /// Like [`Quantity::parse`] with `HalfEven` (banker's rounding). It is
    /// a convenience, not a default: `parse` always asks for a rounding.
    ///
    /// ```
    /// use renox::quantity::Quantity;
    ///
    /// assert_eq!(Quantity::<0>::parse_half_even("2.5").unwrap().raw(), 2);
    /// assert_eq!(Quantity::<0>::parse_half_even("3.5").unwrap().raw(), 4);
    /// ```
    pub fn parse_half_even(text: &str) -> Result<Self, QuantityError> {
        Self::parse(text, Rounding::HalfEven)
    }

    /// The sum, or `Overflow` past the `i64` limits.
    ///
    /// ```
    /// use renox::quantity::Quantity;
    ///
    /// let one = Quantity::<3>::from_raw(1000);
    /// assert_eq!(one.checked_add(one).unwrap().raw(), 2000);
    /// assert!(Quantity::<3>::from_raw(i64::MAX).checked_add(one).is_err());
    /// ```
    pub fn checked_add(self, other: Self) -> Result<Self, QuantityError> {
        self.0
            .checked_add(other.0)
            .map(Quantity)
            .ok_or(QuantityError::Overflow)
    }

    /// The difference, or `Overflow` past the `i64` limits.
    ///
    /// ```
    /// use renox::quantity::Quantity;
    ///
    /// let one = Quantity::<3>::from_raw(1000);
    /// assert_eq!(one.checked_sub(Quantity::from_raw(1500)).unwrap().raw(), -500);
    /// ```
    pub fn checked_sub(self, other: Self) -> Result<Self, QuantityError> {
        self.0
            .checked_sub(other.0)
            .map(Quantity)
            .ok_or(QuantityError::Overflow)
    }

    /// The negated quantity, or `Overflow` for the smallest `i64`.
    ///
    /// ```
    /// use renox::quantity::Quantity;
    ///
    /// assert_eq!(Quantity::<3>::from_raw(5).checked_neg().unwrap().raw(), -5);
    /// assert!(Quantity::<3>::from_raw(i64::MIN).checked_neg().is_err());
    /// ```
    pub fn checked_neg(self) -> Result<Self, QuantityError> {
        self.0
            .checked_neg()
            .map(Quantity)
            .ok_or(QuantityError::Overflow)
    }

    /// The quantity times `num / den`, rounded to the scale with
    /// `rounding`. A zero `den` is `DivisionByZero`.
    ///
    /// ```
    /// use renox::money::Rounding;
    /// use renox::quantity::Quantity;
    ///
    /// let q = Quantity::<3>::from_raw(1000);
    /// assert_eq!(q.mul_ratio(1, 3, Rounding::Down).unwrap().raw(), 333);
    /// assert_eq!(q.mul_ratio(1, 3, Rounding::Up).unwrap().raw(), 334);
    /// ```
    pub fn mul_ratio(self, num: i64, den: i64, rounding: Rounding) -> Result<Self, QuantityError> {
        if den == 0 {
            return Err(QuantityError::DivisionByZero);
        }
        let n = self.0 as i128 * num as i128;
        i64::try_from(div_round(n, den as i128, rounding))
            .map(Quantity)
            .map_err(|_| QuantityError::Overflow)
    }

    /// The quantity times an exact decimal factor such as `"1.15"`,
    /// rounded with `rounding`. The factor has at most 18 decimals and no
    /// exponent; anything else is `Invalid`.
    ///
    /// ```
    /// use renox::money::Rounding;
    /// use renox::quantity::Quantity;
    ///
    /// let q = Quantity::<3>::from_raw(2000);
    /// assert_eq!(q.mul_decimal("1.15", Rounding::HalfUp).unwrap().raw(), 2300);
    /// assert_eq!(q.mul_decimal("-0.0005", Rounding::HalfUp).unwrap().raw(), -1);
    /// assert!(q.mul_decimal("1e3", Rounding::HalfUp).is_err());
    /// ```
    pub fn mul_decimal(self, factor: &str, rounding: Rounding) -> Result<Self, QuantityError> {
        let bad = || QuantityError::Invalid(factor.to_string());
        let (negative, whole, fraction) = split_decimal(factor).ok_or_else(bad)?;
        if fraction.len() > 18 {
            return Err(bad());
        }
        let magnitude: i128 = format!("{whole}{fraction}")
            .parse()
            .map_err(|_| QuantityError::Overflow)?;
        let num = if negative { -magnitude } else { magnitude };
        let den = 10i128.pow(fraction.len() as u32);
        let n = (self.0 as i128)
            .checked_mul(num)
            .ok_or(QuantityError::Overflow)?;
        i64::try_from(div_round(n, den, rounding))
            .map(Quantity)
            .map_err(|_| QuantityError::Overflow)
    }

    /// The same value at another scale. Going to a finer scale is exact
    /// (or `Overflow`); going coarser rounds with `rounding`.
    ///
    /// ```
    /// use renox::money::Rounding;
    /// use renox::quantity::Quantity;
    ///
    /// let q = Quantity::<3>::from_raw(2505);
    /// assert_eq!(q.rescale::<2>(Rounding::HalfUp).unwrap().raw(), 251);
    /// assert_eq!(q.rescale::<2>(Rounding::Down).unwrap().raw(), 250);
    /// assert_eq!(q.rescale::<4>(Rounding::Down).unwrap().raw(), 25_050);
    /// ```
    pub fn rescale<const TO: u32>(self, rounding: Rounding) -> Result<Quantity<TO>, QuantityError> {
        let () = Quantity::<TO>::VALID;
        let raw = if TO >= SCALE {
            (self.0 as i128) * 10i128.pow(TO - SCALE)
        } else {
            div_round(self.0 as i128, 10i128.pow(SCALE - TO), rounding)
        };
        i64::try_from(raw)
            .map(Quantity)
            .map_err(|_| QuantityError::Overflow)
    }
}

impl<const SCALE: u32> fmt::Display for Quantity<SCALE> {
    /// The exact decimal with all `SCALE` digits (`2.500`), `.` as the
    /// decimal point and no grouping.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scale = SCALE as usize;
        let digits = format!("{:0>width$}", self.0.unsigned_abs(), width = scale + 1);
        let (whole, fraction) = digits.split_at(digits.len() - scale);
        let sign = if self.0 < 0 { "-" } else { "" };
        if scale == 0 {
            write!(f, "{sign}{whole}")
        } else {
            write!(f, "{sign}{whole}.{fraction}")
        }
    }
}

impl<const SCALE: u32> serde::Serialize for Quantity<SCALE> {
    /// A decimal string such as `"2.500"`, so JSON never loses precision.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, const SCALE: u32> serde::Deserialize<'de> for Quantity<SCALE> {
    /// Accepts a decimal string, or a JSON number when it is exactly
    /// representable at the scale (its shortest decimal form is read).
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<const S: u32>;
        impl<const S: u32> serde::de::Visitor<'_> for V<S> {
            type Value = Quantity<S>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a decimal quantity as a string or an exact number")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Quantity::<S>::parse_exact(v).map_err(E::custom)
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Quantity::<S>::parse_exact(&v.to_string()).map_err(E::custom)
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Quantity::<S>::parse_exact(&v.to_string()).map_err(E::custom)
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                if !v.is_finite() {
                    return Err(E::custom(QuantityError::Invalid(v.to_string())));
                }
                Quantity::<S>::parse_exact(&format!("{v}")).map_err(E::custom)
            }
        }
        deserializer.deserialize_any(V::<SCALE>)
    }
}

/// How one unit relates to its base: 1 unit = `num / den` of the base.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UnitDef {
    base: String,
    num: i128,
    den: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Parses a positive exact decimal factor into a reduced fraction.
fn parse_factor(text: &str) -> Result<(i128, i128), QuantityError> {
    let bad = || QuantityError::InvalidUnits(format!("`{text}` is not a positive decimal factor"));
    let (negative, whole, fraction) = split_decimal(text).ok_or_else(bad)?;
    if negative || fraction.len() > 18 {
        return Err(bad());
    }
    let num: i128 = format!("{whole}{fraction}").parse().map_err(|_| bad())?;
    if num == 0 {
        return Err(bad());
    }
    let den = 10i128.pow(fraction.len() as u32);
    let g = gcd(num, den);
    Ok((num / g, den / g))
}

/// Builder for a unit conversion table, from strings with exact decimal
/// factors (never floats). Every unit points at a base unit:
/// `.unit("g", "kg", "0.001")` says 1 g is 0.001 kg. Units chain only
/// through the base (one level). [`Units::build`] checks the table and
/// returns a [`UnitTable`]; conversion takes an explicit [`Rounding`].
///
/// ```
/// use renox::money::Rounding;
/// use renox::quantity::{Quantity, Units};
///
/// let units = Units::new()
///     .base("kg")
///     .unit("g", "kg", "0.001")
///     .unit("t", "kg", "1000")
///     .build()
///     .unwrap();
/// let kg = Quantity::<3>::parse_exact("2.5").unwrap();
/// let g = units.convert(kg, "kg", "g", Rounding::HalfUp).unwrap();
/// assert_eq!(g.to_string(), "2500.000");
/// let grams = Quantity::<3>::parse_exact("1500").unwrap();
/// let back = units.convert(grams, "g", "kg", Rounding::HalfUp).unwrap();
/// assert_eq!(back.to_string(), "1.500");
/// ```
#[derive(Debug, Clone, Default)]
pub struct Units {
    /// Declarations in order: (code, base code or `None` for a base, factor text).
    declared: Vec<(String, Option<String>, String)>,
}

/// A checked unit table, ready to convert. Built with [`Units::build`];
/// plain data, `Clone` and `Debug`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitTable {
    defs: BTreeMap<String, UnitDef>,
}

impl Units {
    /// An empty table.
    pub fn new() -> Self {
        Units::default()
    }

    /// Declares a base unit (`"kg"`), the unit others are expressed in.
    pub fn base(mut self, code: &str) -> Self {
        self.declared.push((code.to_string(), None, String::new()));
        self
    }

    /// Declares `code` as `factor` units of the base `base`: 1 `code` =
    /// `factor` `base`. The factor is an exact positive decimal.
    pub fn unit(mut self, code: &str, base: &str, factor: &str) -> Self {
        self.declared
            .push((code.to_string(), Some(base.to_string()), factor.to_string()));
        self
    }

    /// Checks the table and returns it ready to convert. It fails with
    /// `InvalidUnits` on an empty or duplicate code, a factor that is
    /// zero, negative or malformed, and a unit whose base is missing or
    /// is not itself a base. Declaration order does not matter.
    ///
    /// ```
    /// use renox::quantity::Units;
    ///
    /// assert!(Units::new().base("kg").unit("g", "kg", "0.001").build().is_ok());
    /// assert!(Units::new().base("kg").unit("g", "kg", "0").build().is_err());
    /// let chained = Units::new().base("kg").unit("g", "kg", "0.001").unit("mg", "g", "0.001");
    /// assert!(chained.build().is_err());
    /// ```
    pub fn build(self) -> Result<UnitTable, QuantityError> {
        let fail = |why: String| Err(QuantityError::InvalidUnits(why));
        let mut defs: BTreeMap<String, UnitDef> = BTreeMap::new();
        for (code, base, factor) in &self.declared {
            if code.trim().is_empty() {
                return fail("a unit code is empty".into());
            }
            if defs.contains_key(code) {
                return fail(format!("`{code}` is declared twice"));
            }
            let def = match base {
                None => UnitDef {
                    base: code.clone(),
                    num: 1,
                    den: 1,
                },
                Some(base) => {
                    let (num, den) = parse_factor(factor)?;
                    UnitDef {
                        base: base.clone(),
                        num,
                        den,
                    }
                }
            };
            defs.insert(code.clone(), def);
        }
        for (code, def) in &defs {
            match defs.get(&def.base) {
                None => return fail(format!("`{code}` points at unknown base `{}`", def.base)),
                Some(target) if target.base != def.base => {
                    return fail(format!(
                        "`{code}` points at `{}`, which is not a base",
                        def.base
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(UnitTable { defs })
    }
}

impl UnitTable {
    /// Whether `code` is a unit of this table.
    ///
    /// ```
    /// use renox::quantity::Units;
    ///
    /// let t = Units::new().base("kg").unit("g", "kg", "0.001").build().unwrap();
    /// assert!(t.contains("g"));
    /// assert!(!t.contains("lb"));
    /// ```
    pub fn contains(&self, code: &str) -> bool {
        self.defs.contains_key(code)
    }

    /// Converts `q` from unit `from` to unit `to` at the same scale,
    /// rounding with `rounding` when digits are lost. An unknown unit is
    /// `UnknownUnit`, units of different bases are `IncompatibleUnits`,
    /// a result that does not fit is `Overflow`.
    ///
    /// ```
    /// use renox::money::Rounding;
    /// use renox::quantity::{Quantity, QuantityError, Units};
    ///
    /// let t = Units::new()
    ///     .base("kg")
    ///     .unit("g", "kg", "0.001")
    ///     .base("m")
    ///     .build()
    ///     .unwrap();
    /// let q = Quantity::<3>::from_raw(1);
    /// assert_eq!(t.convert(q, "g", "kg", Rounding::Down).unwrap().raw(), 0);
    /// assert_eq!(t.convert(q, "g", "kg", Rounding::Up).unwrap().raw(), 1);
    /// assert!(matches!(
    ///     t.convert(q, "kg", "m", Rounding::Up),
    ///     Err(QuantityError::IncompatibleUnits { .. })
    /// ));
    /// assert!(matches!(
    ///     t.convert(q, "kg", "lb", Rounding::Up),
    ///     Err(QuantityError::UnknownUnit(_))
    /// ));
    /// ```
    pub fn convert<const SCALE: u32>(
        &self,
        q: Quantity<SCALE>,
        from: &str,
        to: &str,
        rounding: Rounding,
    ) -> Result<Quantity<SCALE>, QuantityError> {
        let unknown = |c: &str| QuantityError::UnknownUnit(c.to_string());
        let f = self.defs.get(from).ok_or_else(|| unknown(from))?;
        let t = self.defs.get(to).ok_or_else(|| unknown(to))?;
        if f.base != t.base {
            return Err(QuantityError::IncompatibleUnits {
                from: from.to_string(),
                to: to.to_string(),
            });
        }
        // value * (f.num / f.den) / (t.num / t.den)
        let num = (q.raw() as i128)
            .checked_mul(f.num)
            .and_then(|n| n.checked_mul(t.den))
            .ok_or(QuantityError::Overflow)?;
        let den = f.den.checked_mul(t.num).ok_or(QuantityError::Overflow)?;
        i64::try_from(div_round(num, den, rounding))
            .map(Quantity::from_raw)
            .map_err(|_| QuantityError::Overflow)
    }
}

//! Currencies and rounding rules for money amounts.
//!
//! [`Currency`] is a `Copy` ISO 4217 code checked against one built-in
//! table (code, symbol and minor-unit digits). [`Rounding`] names how an
//! inexact result is rounded; there is no default, every operation that
//! rounds asks for one.
//!
//! ```
//! use renox::money::Currency;
//!
//! let aed = Currency::parse("aed").unwrap();
//! assert_eq!(aed, Currency::AED);
//! assert_eq!(aed.decimals(), 2);
//! assert_eq!(Currency::JPY.decimals(), 0);
//! assert_eq!(Currency::KWD.decimals(), 3);
//! assert!(Currency::parse("XX").is_err());
//! ```

use std::fmt;
use std::str::FromStr;

/// Code, symbol and minor-unit digits of every known currency. The symbol
/// is the code itself when the currency has no short one. This is the only
/// currency table in the framework.
const TABLE: &[(&str, &str, u32)] = &[
    ("AED", "AED", 2),
    ("AUD", "A$", 2),
    ("BHD", "BHD", 3),
    ("BRL", "R$", 2),
    ("CAD", "CA$", 2),
    ("CHF", "CHF", 2),
    ("CLP", "CLP", 0),
    ("CNY", "CN¥", 2),
    ("COP", "COP", 2),
    ("CZK", "CZK", 2),
    ("DKK", "DKK", 2),
    ("EGP", "EGP", 2),
    ("EUR", "€", 2),
    ("GBP", "£", 2),
    ("HKD", "HK$", 2),
    ("HUF", "HUF", 2),
    ("IDR", "Rp", 0),
    ("ILS", "₪", 2),
    ("INR", "₹", 2),
    ("IQD", "IQD", 3),
    ("JOD", "JOD", 3),
    ("JPY", "¥", 0),
    ("KRW", "₩", 0),
    ("KWD", "KWD", 3),
    ("LYD", "LYD", 3),
    ("MAD", "MAD", 2),
    ("MXN", "MX$", 2),
    ("MYR", "RM", 2),
    ("NGN", "₦", 2),
    ("NOK", "NOK", 2),
    ("NZD", "NZ$", 2),
    ("OMR", "OMR", 3),
    ("PHP", "₱", 2),
    ("PKR", "PKR", 2),
    ("PLN", "PLN", 2),
    ("QAR", "QAR", 2),
    ("RON", "RON", 2),
    ("RUB", "RUB", 2),
    ("SAR", "SAR", 2),
    ("SEK", "SEK", 2),
    ("SGD", "S$", 2),
    ("THB", "฿", 2),
    ("TND", "TND", 3),
    ("TRY", "₺", 2),
    ("TWD", "NT$", 2),
    ("UGX", "UGX", 0),
    ("USD", "$", 2),
    ("VND", "₫", 0),
    ("ZAR", "R", 2),
];

/// The table row for an exact upper-case `code`.
fn row(code: &str) -> Option<&'static (&'static str, &'static str, u32)> {
    TABLE.iter().find(|r| r.0 == code)
}

/// A currency's symbol and usual decimals (the code itself and 2 when it
/// isn't in the table). Backs `format_money` and `currency_decimals`.
pub(crate) fn info(code: &str) -> (&str, u32) {
    match row(code) {
        Some(&(_, symbol, digits)) => (symbol, digits),
        None => (code, 2),
    }
}

/// A currency: a three-letter ISO 4217 code known to the built-in table.
/// `Copy`, cheap to compare, and always valid: [`Currency::parse`] rejects
/// unknown codes.
///
/// ```
/// use renox::money::Currency;
///
/// assert_eq!("usd".parse::<Currency>().unwrap(), Currency::USD);
/// assert_eq!(Currency::BHD.code(), "BHD");
/// assert_eq!(Currency::USD.symbol(), "$");
/// assert_eq!(Currency::USD.to_string(), "USD");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Currency([u8; 3]);

macro_rules! currency_consts {
    ($($(#[$m:meta])* $name:ident),+ $(,)?) => {
        impl Currency {
            $(
                $(#[$m])*
                pub const $name: Currency = Currency::from_literal(stringify!($name));
            )+
        }
    };
}

currency_consts! {
    /// United Arab Emirates dirham (2 decimals).
    AED,
    /// Australian dollar (2 decimals).
    AUD,
    /// Bahraini dinar (3 decimals).
    BHD,
    /// Canadian dollar (2 decimals).
    CAD,
    /// Swiss franc (2 decimals).
    CHF,
    /// Chinese yuan (2 decimals).
    CNY,
    /// Euro (2 decimals).
    EUR,
    /// Pound sterling (2 decimals).
    GBP,
    /// Indonesian rupiah (0 decimals).
    IDR,
    /// Indian rupee (2 decimals).
    INR,
    /// Japanese yen (0 decimals).
    JPY,
    /// South Korean won (0 decimals).
    KRW,
    /// Kuwaiti dinar (3 decimals).
    KWD,
    /// Omani rial (3 decimals).
    OMR,
    /// Saudi riyal (2 decimals).
    SAR,
    /// Singapore dollar (2 decimals).
    SGD,
    /// United States dollar (2 decimals).
    USD,
}

impl Currency {
    /// Builds a constant from its code (only used for the constants above,
    /// which the tests check against the table).
    const fn from_literal(code: &str) -> Currency {
        let b = code.as_bytes();
        Currency([b[0], b[1], b[2]])
    }

    /// Reads an ISO 4217 code, ignoring case and surrounding spaces
    /// (`"aed"` is `AED`). An unknown or malformed code is an error.
    ///
    /// ```
    /// use renox::money::Currency;
    ///
    /// assert_eq!(Currency::parse(" kwd ").unwrap(), Currency::KWD);
    /// assert!(Currency::parse("ABCD").is_err());
    /// assert!(Currency::parse("123").is_err());
    /// ```
    pub fn parse(code: &str) -> Result<Currency, CurrencyError> {
        let upper = code.trim().to_ascii_uppercase();
        match row(&upper) {
            Some(r) => Ok(Currency::from_literal(r.0)),
            None => Err(CurrencyError {
                code: code.trim().to_string(),
            }),
        }
    }

    /// The upper-case code: `"USD"`.
    pub fn code(&self) -> &'static str {
        // The three bytes always come from a table row, so this finds it.
        TABLE
            .iter()
            .find(|r| r.0.as_bytes() == self.0)
            .expect("a Currency is always in the table")
            .0
    }

    /// The symbol (`$`, `€`, `Rp`), or the code when the currency has no
    /// short one.
    pub fn symbol(&self) -> &'static str {
        self.entry().1
    }

    /// The minor-unit digits: 2 for `USD` and `AED`, 0 for `JPY`, 3 for
    /// `BHD` and `KWD`. An amount in the minor unit is divided by
    /// `10^decimals` to give whole units.
    pub fn decimals(&self) -> u32 {
        self.entry().2
    }

    /// This currency's table row.
    fn entry(&self) -> &'static (&'static str, &'static str, u32) {
        TABLE
            .iter()
            .find(|r| r.0.as_bytes() == self.0)
            .expect("a Currency is always in the table")
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for Currency {
    type Err = CurrencyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Currency::parse(s)
    }
}

/// An unknown or malformed currency code.
///
/// ```
/// use renox::money::Currency;
///
/// let err = Currency::parse("XX").unwrap_err();
/// assert_eq!(err.to_string(), "`XX` is not a known ISO 4217 currency code");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencyError {
    code: String,
}

impl fmt::Display for CurrencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a known ISO 4217 currency code", self.code)
    }
}

impl std::error::Error for CurrencyError {}

/// How an inexact result is rounded to a whole minor unit. There is no
/// default: every operation that can round takes one explicitly.
///
/// ```
/// use renox::money::Rounding;
///
/// assert_eq!(Rounding::HalfEven.as_str(), "half_even");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Rounding {
    /// To the nearest; a tie goes away from zero (2.5 gives 3, -2.5 gives -3).
    HalfUp,
    /// To the nearest; a tie goes to the even neighbour (2.5 gives 2, 3.5 gives 4).
    HalfEven,
    /// Toward zero: the fraction is dropped (-2.9 gives -2).
    Down,
    /// Away from zero: any fraction rounds the magnitude up (2.1 gives 3, -2.1 gives -3).
    Up,
}

impl Rounding {
    /// The name in snake case: `half_up`, `half_even`, `down`, `up`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Rounding::HalfUp => "half_up",
            Rounding::HalfEven => "half_even",
            Rounding::Down => "down",
            Rounding::Up => "up",
        }
    }
}

impl fmt::Display for Rounding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for Currency {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

impl<'de> serde::Deserialize<'de> for Currency {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(d)?;
        Currency::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Why a money calculation or parse failed.
///
/// ```
/// use renox::money::{Money, MoneyError};
///
/// let err = Money::usd(1).checked_add(Money::eur(1)).unwrap_err();
/// assert!(matches!(err, MoneyError::CurrencyMismatch { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoneyError {
    /// The two amounts are in different currencies.
    CurrencyMismatch {
        /// The currency of the left-hand amount.
        left: Currency,
        /// The currency of the right-hand amount.
        right: Currency,
    },
    /// The result does not fit in an `i64` of minor units.
    Overflow,
    /// A denominator of zero.
    DivisionByZero,
    /// Nothing to split between: zero parts, no weights, or weights that
    /// add up to zero.
    NoParts,
    /// The exchange rate is not a positive exact decimal (or ratio).
    InvalidRate(String),
    /// The text is not an amount.
    InvalidAmount(String),
    /// The text has more decimals than the currency has.
    TooManyDecimals {
        /// The currency whose decimals were exceeded.
        currency: Currency,
        /// The decimals the currency allows.
        allowed: u32,
    },
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoneyError::CurrencyMismatch { left, right } => {
                write!(f, "cannot combine {left} and {right} amounts")
            }
            MoneyError::Overflow => f.write_str("the amount is too large"),
            MoneyError::DivisionByZero => f.write_str("the denominator is zero"),
            MoneyError::NoParts => f.write_str("there is nothing to split between"),
            MoneyError::InvalidRate(r) => write!(f, "`{r}` is not a valid exchange rate"),
            MoneyError::InvalidAmount(a) => write!(f, "`{a}` is not a valid amount"),
            MoneyError::TooManyDecimals { currency, allowed } => {
                write!(f, "{currency} amounts have at most {allowed} decimals")
            }
        }
    }
}

impl std::error::Error for MoneyError {}

/// `num / den` rounded to a whole number with `rounding`. `den` is not 0.
fn div_round(num: i128, den: i128, rounding: Rounding) -> i128 {
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    let q = num / den;
    let rem = num % den;
    if rem == 0 {
        return q;
    }
    let away = if num < 0 { q - 1 } else { q + 1 };
    match rounding {
        Rounding::Down => q,
        Rounding::Up => away,
        Rounding::HalfUp | Rounding::HalfEven => {
            let r = rem.abs();
            let other = den - r;
            if r > other {
                away
            } else if r < other {
                q
            } else if rounding == Rounding::HalfUp || q % 2 != 0 {
                away
            } else {
                q
            }
        }
    }
}

/// An exact, positive exchange rate: a ratio of two integers, built from a
/// decimal string (`"3.6725"`) or a pair. Never a float.
///
/// ```
/// use renox::money::Rate;
///
/// assert_eq!(Rate::parse("3.6725").unwrap(), Rate::new(36725, 10000).unwrap());
/// assert!(Rate::parse("0").is_err());
/// assert!(Rate::parse("1e3").is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    num: i64,
    den: i64,
}

impl Rate {
    /// The ratio `num / den`; both must be positive.
    pub fn new(num: i64, den: i64) -> Result<Rate, MoneyError> {
        if num <= 0 || den <= 0 {
            return Err(MoneyError::InvalidRate(format!("{num}/{den}")));
        }
        Ok(Rate { num, den })
    }

    /// Reads a positive decimal such as `"3.6725"` or `"0.0027"`: digits
    /// with an optional fraction of at most 18 digits, no sign, no exponent.
    pub fn parse(text: &str) -> Result<Rate, MoneyError> {
        let bad = || MoneyError::InvalidRate(text.to_string());
        let t = text.trim();
        let (whole, fraction) = t.split_once('.').unwrap_or((t, ""));
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty() || !digits(whole) || !digits(fraction) || fraction.len() > 18 {
            return Err(bad());
        }
        if t.ends_with('.') {
            return Err(bad());
        }
        let num: i64 = format!("{whole}{fraction}").parse().map_err(|_| bad())?;
        let den = 10i64.pow(fraction.len() as u32);
        Rate::new(num, den).map_err(|_| bad())
    }
}

/// Anything `Money::convert` accepts as a rate: a decimal string such as
/// `"3.6725"`, a `(numerator, denominator)` pair or a [`Rate`].
pub trait IntoRate {
    /// The exact rate, or why it is invalid.
    fn into_rate(self) -> Result<Rate, MoneyError>;
}

impl IntoRate for Rate {
    fn into_rate(self) -> Result<Rate, MoneyError> {
        Ok(self)
    }
}

impl IntoRate for &str {
    fn into_rate(self) -> Result<Rate, MoneyError> {
        Rate::parse(self)
    }
}

impl IntoRate for (i64, i64) {
    fn into_rate(self) -> Result<Rate, MoneyError> {
        Rate::new(self.0, self.1)
    }
}

/// An amount of money: whole minor units (cents, fils, yen) of one
/// [`Currency`]. Adding and subtracting need the same currency and return a
/// `Result`; there are no panicking operators. Anything that can round
/// takes an explicit [`Rounding`].
///
/// ```
/// use renox::money::{Money, Rounding};
///
/// let total = Money::usd(1999).checked_add(Money::usd(1)).unwrap();
/// assert_eq!(total.to_string(), "$20.00");
/// let parts: Vec<i64> = Money::usd(100).allocate(3).unwrap().iter().map(|m| m.minor).collect();
/// assert_eq!(parts, [34, 33, 33]);
/// assert_eq!(Money::usd(1999).percent(500, Rounding::HalfUp).unwrap(), Money::usd(100));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Money {
    /// The amount in minor units (cents for USD, yen for JPY).
    pub minor: i64,
    /// The currency of the amount.
    pub currency: Currency,
}

macro_rules! money_ctors {
    ($($(#[$m:meta])* $name:ident => $cur:ident),+ $(,)?) => {
        impl Money {
            $(
                $(#[$m])*
                pub const fn $name(minor: i64) -> Money {
                    Money { minor, currency: Currency::$cur }
                }
            )+
        }
    };
}

money_ctors! {
    /// An amount in US dollars (cents).
    usd => USD,
    /// An amount in euros (cents).
    eur => EUR,
    /// An amount in UAE dirhams (fils).
    aed => AED,
    /// An amount in Japanese yen.
    jpy => JPY,
    /// An amount in Bahraini dinars (thousandths).
    bhd => BHD,
    /// An amount in Kuwaiti dinars (thousandths).
    kwd => KWD,
}

impl Money {
    /// An amount of `minor` minor units in `currency`.
    pub const fn new(minor: i64, currency: Currency) -> Money {
        Money { minor, currency }
    }

    /// Whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.minor == 0
    }

    /// Whether the amount is below zero.
    pub const fn is_negative(&self) -> bool {
        self.minor < 0
    }

    fn same_currency(&self, other: &Money) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }

    /// The sum; a different currency is `CurrencyMismatch`, going past the
    /// `i64` limits is `Overflow`.
    pub fn checked_add(self, other: Money) -> Result<Money, MoneyError> {
        self.same_currency(&other)?;
        let minor = self
            .minor
            .checked_add(other.minor)
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(minor, self.currency))
    }

    /// The difference; a different currency is `CurrencyMismatch`, going
    /// past the `i64` limits is `Overflow`.
    pub fn checked_sub(self, other: Money) -> Result<Money, MoneyError> {
        self.same_currency(&other)?;
        let minor = self
            .minor
            .checked_sub(other.minor)
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(minor, self.currency))
    }

    /// Splits the amount between `parts` equal shares without losing a minor
    /// unit: the remainder goes to the first shares, one unit each
    /// (100 in 3 gives 34, 33, 33). Zero parts is `NoParts`.
    ///
    /// ```
    /// use renox::money::Money;
    ///
    /// let shares = Money::usd(-100).allocate(3).unwrap();
    /// assert_eq!(shares.iter().map(|m| m.minor).collect::<Vec<_>>(), [-34, -33, -33]);
    /// ```
    pub fn allocate(self, parts: usize) -> Result<Vec<Money>, MoneyError> {
        if parts == 0 {
            return Err(MoneyError::NoParts);
        }
        self.allocate_by(&vec![1; parts])
    }

    /// Splits the amount in proportion to `weights` without losing a minor
    /// unit; the leftover units go to the first non-zero weights. Weights
    /// adding up to zero (or none) is `NoParts`. The shares always add up
    /// to the original amount.
    ///
    /// ```
    /// use renox::money::Money;
    ///
    /// let shares = Money::usd(10).allocate_by(&[1, 1, 1]).unwrap();
    /// assert_eq!(shares.iter().map(|m| m.minor).collect::<Vec<_>>(), [4, 3, 3]);
    /// let shares = Money::usd(100).allocate_by(&[70, 30]).unwrap();
    /// assert_eq!(shares.iter().map(|m| m.minor).collect::<Vec<_>>(), [70, 30]);
    /// ```
    pub fn allocate_by(self, weights: &[i64]) -> Result<Vec<Money>, MoneyError> {
        let total: i128 = weights.iter().map(|w| *w as i128).sum();
        if weights.is_empty() || total == 0 {
            return Err(MoneyError::NoParts);
        }
        let mut shares: Vec<i128> = weights
            .iter()
            .map(|w| self.minor as i128 * *w as i128 / total)
            .collect();
        let left = self.minor as i128 - shares.iter().sum::<i128>();
        let step = left.signum();
        let mut to_give = left.abs();
        for (share, weight) in shares.iter_mut().zip(weights) {
            if to_give == 0 {
                break;
            }
            if *weight != 0 {
                *share += step;
                to_give -= 1;
            }
        }
        shares
            .into_iter()
            .map(|s| {
                i64::try_from(s)
                    .map(|minor| Money::new(minor, self.currency))
                    .map_err(|_| MoneyError::Overflow)
            })
            .collect()
    }

    /// The amount times `num / den`, rounded to a whole minor unit with
    /// `rounding`. A zero `den` is `DivisionByZero`.
    ///
    /// ```
    /// use renox::money::{Money, Rounding};
    ///
    /// assert_eq!(Money::usd(333).mul_ratio(3, 1, Rounding::Down).unwrap(), Money::usd(999));
    /// assert_eq!(Money::usd(100).mul_ratio(1, 3, Rounding::Up).unwrap(), Money::usd(34));
    /// ```
    pub fn mul_ratio(self, num: i64, den: i64, rounding: Rounding) -> Result<Money, MoneyError> {
        if den == 0 {
            return Err(MoneyError::DivisionByZero);
        }
        let n = self.minor as i128 * num as i128;
        let minor =
            i64::try_from(div_round(n, den as i128, rounding)).map_err(|_| MoneyError::Overflow)?;
        Ok(Money::new(minor, self.currency))
    }

    /// The amount times `basis_points / 10 000` (500 is 5 %), rounded with
    /// `rounding`.
    ///
    /// ```
    /// use renox::money::{Money, Rounding};
    ///
    /// assert_eq!(Money::usd(1050).percent(1500, Rounding::HalfEven).unwrap(), Money::usd(158));
    /// ```
    pub fn percent(self, basis_points: i64, rounding: Rounding) -> Result<Money, MoneyError> {
        self.mul_ratio(basis_points, 10_000, rounding)
    }

    /// The amount in another currency at an exact `rate` (units of `to` per
    /// one unit of this currency, a decimal string such as `"3.6725"` or a
    /// `(numerator, denominator)` pair), rounded with `rounding`. The minor
    /// digits of both currencies are taken into account. There are no
    /// rate tables: the caller supplies the rate.
    ///
    /// ```
    /// use renox::money::{Currency, Money, Rounding};
    ///
    /// let aed = Money::usd(1000).convert("3.6725", Currency::AED, Rounding::HalfUp).unwrap();
    /// assert_eq!(aed, Money::aed(3673));
    /// let yen = Money::usd(1000).convert("150", Currency::JPY, Rounding::HalfUp).unwrap();
    /// assert_eq!(yen, Money::jpy(1500));
    /// ```
    pub fn convert(
        self,
        rate: impl IntoRate,
        to: Currency,
        rounding: Rounding,
    ) -> Result<Money, MoneyError> {
        let rate = rate.into_rate()?;
        let mut num = self.minor as i128 * rate.num as i128;
        let mut den = rate.den as i128;
        let from_digits = self.currency.decimals();
        let to_digits = to.decimals();
        if to_digits >= from_digits {
            num = num
                .checked_mul(10i128.pow(to_digits - from_digits))
                .ok_or(MoneyError::Overflow)?;
        } else {
            den *= 10i128.pow(from_digits - to_digits);
        }
        let minor =
            i64::try_from(div_round(num, den, rounding)).map_err(|_| MoneyError::Overflow)?;
        Ok(Money::new(minor, to))
    }

    /// The amount written for people in `locale`: the symbol, the
    /// currency's own decimals and the locale's separators. Exact: no float
    /// is involved.
    ///
    /// ```
    /// use renox::money::Money;
    ///
    /// assert_eq!(Money::jpy(1250).format("en"), "¥1,250");
    /// assert_eq!(Money::kwd(-12345).format("en"), "-KWD 12.345");
    /// assert_eq!(Money::eur(125050).format("de"), "€1.250,50");
    /// ```
    pub fn format(&self, locale: &str) -> String {
        let (thousands, decimal) = crate::view_filters::separators(locale);
        let decimals = self.currency.decimals() as usize;
        let digits = format!(
            "{:0>width$}",
            self.minor.unsigned_abs(),
            width = decimals + 1
        );
        let (whole, fraction) = digits.split_at(digits.len() - decimals);
        let mut grouped = String::new();
        for (i, digit) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i) % 3 == 0 {
                grouped.push_str(thousands);
            }
            grouped.push(digit);
        }
        if decimals > 0 {
            grouped.push_str(decimal);
            grouped.push_str(fraction);
        }
        let sign = if self.minor < 0 { "-" } else { "" };
        let symbol = self.currency.symbol();
        let space = if symbol.ends_with(|c: char| c.is_ascii_alphabetic()) {
            " "
        } else {
            ""
        };
        format!("{sign}{symbol}{space}{grouped}")
    }

    /// Reads an amount in whole units (`"12.50"`, `"-3"`, `"1250"` for JPY)
    /// with `.` as the decimal point. More decimals than the currency has
    /// is `TooManyDecimals`; nothing is rounded silently.
    ///
    /// ```
    /// use renox::money::{Currency, Money};
    ///
    /// assert_eq!(Money::parse("12.50", Currency::USD).unwrap(), Money::usd(1250));
    /// assert_eq!(Money::parse("1250", Currency::JPY).unwrap(), Money::jpy(1250));
    /// assert!(Money::parse("10.5", Currency::JPY).is_err());
    /// assert!(Money::parse("1.234", Currency::USD).is_err());
    /// ```
    pub fn parse(text: &str, currency: Currency) -> Result<Money, MoneyError> {
        let bad = || MoneyError::InvalidAmount(text.to_string());
        let t = text.trim();
        let (negative, t) = match t.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, t.strip_prefix('+').unwrap_or(t)),
        };
        let (whole, fraction) = t.split_once('.').unwrap_or((t, ""));
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty() || !digits(whole) || !digits(fraction) || t.ends_with('.') {
            return Err(bad());
        }
        let allowed = currency.decimals();
        if fraction.len() > allowed as usize {
            return Err(MoneyError::TooManyDecimals { currency, allowed });
        }
        let padded = format!("{whole}{fraction:0<width$}", width = allowed as usize);
        let magnitude: i128 = padded.parse().map_err(|_| MoneyError::Overflow)?;
        let signed = if negative { -magnitude } else { magnitude };
        let minor = i64::try_from(signed).map_err(|_| MoneyError::Overflow)?;
        Ok(Money::new(minor, currency))
    }

    /// Like [`Money::parse`] for text written in `locale`: the locale's
    /// thousands separator is dropped and its decimal separator is the
    /// decimal point (`"1.250,50"` in `de`).
    ///
    /// ```
    /// use renox::money::{Currency, Money};
    ///
    /// assert_eq!(Money::parse_locale("1.250,50", Currency::EUR, "de").unwrap(), Money::eur(125050));
    /// assert_eq!(Money::parse_locale("1,250.50", Currency::USD, "en").unwrap(), Money::usd(125050));
    /// ```
    pub fn parse_locale(text: &str, currency: Currency, locale: &str) -> Result<Money, MoneyError> {
        let (thousands, decimal) = crate::view_filters::separators(locale);
        let plain = text
            .replace(thousands, "")
            .replace(' ', "")
            .replace(decimal, ".");
        Money::parse(&plain, currency).map_err(|e| match e {
            MoneyError::InvalidAmount(_) => MoneyError::InvalidAmount(text.to_string()),
            other => other,
        })
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format("en"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_the_table() {
        for (c, code, digits) in [
            (Currency::AED, "AED", 2),
            (Currency::BHD, "BHD", 3),
            (Currency::JPY, "JPY", 0),
            (Currency::KWD, "KWD", 3),
            (Currency::USD, "USD", 2),
            (Currency::IDR, "IDR", 0),
            (Currency::EUR, "EUR", 2),
        ] {
            assert_eq!(c.code(), code);
            assert_eq!(c.decimals(), digits);
            assert_eq!(Currency::parse(code).unwrap(), c);
        }
    }

    #[test]
    fn every_row_round_trips() {
        for &(code, symbol, digits) in TABLE {
            let c = Currency::parse(&code.to_lowercase()).unwrap();
            assert_eq!((c.code(), c.symbol(), c.decimals()), (code, symbol, digits));
        }
    }

    #[test]
    fn rejects_bad_codes() {
        for bad in ["", "XX", "ABCD", "123", "U$D", "XYZ"] {
            assert!(Currency::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn old_functions_agree_with_the_table() {
        for &(code, _, digits) in TABLE {
            assert_eq!(crate::currency_decimals(code), digits);
        }
        assert_eq!(crate::currency_decimals("ZZZ"), 2);
    }
}

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

//! `Quantity` (#69): exact parsing with explicit rounding, arithmetic,
//! rescaling, display and serde.

use renox::money::Rounding::{self, Down, HalfEven, HalfUp, Up};
use renox::quantity::{Quantity, QuantityError};

type Q3 = Quantity<3>;

fn p(text: &str, r: Rounding) -> i64 {
    Q3::parse(text, r).unwrap().raw()
}

#[test]
fn parse_pads_and_displays_every_digit() {
    assert_eq!(p("2.5", HalfEven), 2500);
    assert_eq!(Q3::parse("2.5", HalfEven).unwrap().to_string(), "2.500");
    assert_eq!(Q3::from_raw(-5).to_string(), "-0.005");
    assert_eq!(Quantity::<0>::from_raw(-7).to_string(), "-7");
    assert_eq!(Q3::parse("+3", Down).unwrap().raw(), 3000);
}

#[test]
fn ties_follow_each_rounding_for_both_signs() {
    assert_eq!((p("0.0005", HalfUp), p("-0.0005", HalfUp)), (1, -1));
    assert_eq!((p("0.0005", HalfEven), p("-0.0005", HalfEven)), (0, 0));
    assert_eq!((p("0.0015", HalfEven), p("-0.0015", HalfEven)), (2, -2));
    assert_eq!((p("0.0005", Down), p("-0.0005", Down)), (0, 0));
    assert_eq!((p("0.0005", Up), p("-0.0005", Up)), (1, -1));
    assert_eq!(p("0.00049", HalfUp), 0);
    assert_eq!(p("0.00051", HalfEven), 1);
    assert_eq!(p("0.00050001", HalfEven), 1);
    assert_eq!(p("0.0009", Down), 0);
    assert_eq!(p("0.00001", Up), 1);
    assert_eq!(p("1.0000", Up), 1000);
}

#[test]
fn parse_exact_refuses_lost_digits() {
    assert_eq!(
        Q3::parse_exact("0.0005").unwrap_err(),
        QuantityError::Inexact
    );
    assert_eq!(Q3::parse_exact("1.2340").unwrap().raw(), 1234);
    assert_eq!(Q3::parse_half_even("0.0005").unwrap().raw(), 0);
}

#[test]
fn bad_text_is_invalid_and_big_values_overflow() {
    for bad in [
        "", " ", "1e3", "1,5", "--1", "abc", "-", "1.", ".5", "+-1", "1 2",
    ] {
        assert!(
            matches!(Q3::parse(bad, HalfUp), Err(QuantityError::Invalid(_))),
            "{bad:?}"
        );
    }
    assert_eq!(
        Q3::parse("9223372036854775.808", Down).unwrap_err(),
        QuantityError::Overflow
    );
    assert_eq!(p("9223372036854775.807", Down), i64::MAX);
    assert_eq!(p("-9223372036854775.808", Down), i64::MIN);
    assert_eq!(
        Q3::parse(&"9".repeat(60), Down).unwrap_err(),
        QuantityError::Overflow
    );
}

#[test]
fn add_sub_neg_are_exact_and_checked() {
    let a = Q3::from_raw(1250);
    let b = Q3::from_raw(750);
    assert_eq!(a.checked_add(b).unwrap().raw(), 2000);
    assert_eq!(b.checked_sub(a).unwrap().raw(), -500);
    assert_eq!(a.checked_neg().unwrap().raw(), -1250);
    assert_eq!(
        Q3::from_raw(i64::MAX)
            .checked_add(Q3::from_raw(1))
            .unwrap_err(),
        QuantityError::Overflow
    );
    assert!(Q3::from_raw(i64::MIN).checked_sub(Q3::from_raw(1)).is_err());
    assert!(Q3::from_raw(i64::MIN).checked_neg().is_err());
    assert!(b < a && a.max(b) == a);
}

#[test]
fn mul_ratio_and_decimal_round_as_asked() {
    let q = Q3::from_raw(1000);
    assert_eq!(q.mul_ratio(1, 3, Down).unwrap().raw(), 333);
    assert_eq!(q.mul_ratio(1, 3, Up).unwrap().raw(), 334);
    assert_eq!(q.mul_ratio(-1, 3, Up).unwrap().raw(), -334);
    assert_eq!(Q3::from_raw(5).mul_ratio(1, 2, HalfUp).unwrap().raw(), 3);
    assert_eq!(Q3::from_raw(5).mul_ratio(1, 2, HalfEven).unwrap().raw(), 2);
    assert_eq!(Q3::from_raw(5).mul_ratio(1, -2, HalfUp).unwrap().raw(), -3);
    assert_eq!(
        q.mul_ratio(1, 0, Down).unwrap_err(),
        QuantityError::DivisionByZero
    );
    assert_eq!(
        Q3::from_raw(i64::MAX).mul_ratio(2, 1, Down).unwrap_err(),
        QuantityError::Overflow
    );
    assert_eq!(q.mul_decimal("1.15", HalfUp).unwrap().raw(), 1150);
    let x = Q3::from_raw(1250);
    assert_eq!(x.mul_decimal("1.15", HalfUp).unwrap().raw(), 1438);
    assert_eq!(x.mul_decimal("1.15", Down).unwrap().raw(), 1437);
    assert_eq!(x.mul_decimal("-1.15", Up).unwrap().raw(), -1438);
    for bad in ["", "x", "1e2", "1.", ".5", "1.1234567890123456789"] {
        assert!(
            matches!(q.mul_decimal(bad, Down), Err(QuantityError::Invalid(_))),
            "{bad:?}"
        );
    }
    assert!(
        Q3::from_raw(i64::MAX)
            .mul_decimal("99999999999999999999.5", Down)
            .is_err()
    );
}

#[test]
fn rescale_is_exact_up_and_rounds_down() {
    let q = Q3::from_raw(2505);
    assert_eq!(q.rescale::<2>(HalfUp).unwrap().raw(), 251);
    assert_eq!(q.rescale::<2>(HalfEven).unwrap().raw(), 250);
    assert_eq!(q.rescale::<2>(Down).unwrap().raw(), 250);
    assert_eq!(q.rescale::<2>(Up).unwrap().raw(), 251);
    assert_eq!(Q3::from_raw(-2505).rescale::<2>(Up).unwrap().raw(), -251);
    assert_eq!(q.rescale::<0>(Down).unwrap().raw(), 2);
    assert_eq!(q.rescale::<5>(Down).unwrap().raw(), 250_500);
    assert_eq!(
        Q3::from_raw(i64::MAX).rescale::<9>(Down).unwrap_err(),
        QuantityError::Overflow
    );
}

#[test]
fn serde_uses_decimal_strings() {
    let q = Q3::from_raw(2500);
    assert_eq!(serde_json::to_string(&q).unwrap(), "\"2.500\"");
    let back: Q3 = serde_json::from_str("\"2.5\"").unwrap();
    assert_eq!(back, q);
    assert_eq!(serde_json::from_str::<Q3>("2.5").unwrap(), q);
    assert_eq!(serde_json::from_str::<Q3>("3").unwrap().raw(), 3000);
    assert_eq!(serde_json::from_str::<Q3>("-3").unwrap().raw(), -3000);
    assert!(serde_json::from_str::<Q3>("0.0005").is_err());
    assert!(serde_json::from_str::<Q3>("\"0.0005\"").is_err());
    assert!(serde_json::from_str::<Q3>("\"abc\"").is_err());
    assert!(serde_json::from_str::<Q3>("null").is_err());
}

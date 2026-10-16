//! `Money` (#45): arithmetic within one currency, allocation, explicit
//! rounding, conversion with exact decimal rates, display and serde.

use renox::money::{Currency, Money, MoneyError, Rate, Rounding};

fn minors(v: Vec<Money>) -> Vec<i64> {
    v.into_iter().map(|m| m.minor).collect()
}

#[test]
fn add_and_sub_stay_in_one_currency() {
    let a = Money::usd(1000);
    assert_eq!(a.checked_add(Money::usd(250)).unwrap(), Money::usd(1250));
    assert_eq!(a.checked_sub(Money::usd(1500)).unwrap(), Money::usd(-500));
    assert_eq!(
        a.checked_add(Money::eur(1)).unwrap_err(),
        MoneyError::CurrencyMismatch {
            left: Currency::USD,
            right: Currency::EUR
        }
    );
    assert!(a.checked_sub(Money::aed(1)).is_err());
}

#[test]
fn add_and_sub_report_overflow() {
    assert_eq!(
        Money::usd(i64::MAX).checked_add(Money::usd(1)),
        Err(MoneyError::Overflow)
    );
    assert_eq!(
        Money::usd(i64::MIN).checked_sub(Money::usd(1)),
        Err(MoneyError::Overflow)
    );
    assert_eq!(
        Money::usd(i64::MAX)
            .checked_add(Money::usd(0))
            .unwrap()
            .minor,
        i64::MAX
    );
}

#[test]
fn allocate_loses_no_minor_unit() {
    assert_eq!(minors(Money::usd(100).allocate(3).unwrap()), [34, 33, 33]);
    assert_eq!(
        minors(Money::usd(-100).allocate(3).unwrap()),
        [-34, -33, -33]
    );
    assert_eq!(minors(Money::usd(2).allocate(5).unwrap()), [1, 1, 0, 0, 0]);
    assert_eq!(minors(Money::usd(0).allocate(2).unwrap()), [0, 0]);
    assert_eq!(Money::usd(1).allocate(0), Err(MoneyError::NoParts));
    for amount in [-1001, -7, 0, 1, 99, 100, 12345] {
        for parts in 1..9 {
            let shares = Money::usd(amount).allocate(parts).unwrap();
            assert_eq!(shares.iter().map(|m| m.minor).sum::<i64>(), amount);
            assert!(shares.iter().all(|m| m.currency == Currency::USD));
        }
    }
}

#[test]
fn allocate_by_weights() {
    assert_eq!(
        minors(Money::usd(100).allocate_by(&[70, 30]).unwrap()),
        [70, 30]
    );
    assert_eq!(
        minors(Money::usd(10).allocate_by(&[1, 1, 1]).unwrap()),
        [4, 3, 3]
    );
    assert_eq!(
        minors(Money::usd(5).allocate_by(&[0, 1, 1]).unwrap()),
        [0, 3, 2]
    );
    assert_eq!(
        minors(Money::usd(-10).allocate_by(&[2, 1]).unwrap()),
        [-7, -3]
    );
    // Negative weights keep the sum too.
    for (amount, weights) in [
        (100, vec![3, -1, 2]),
        (-77, vec![-2, -3, -5]),
        (9, vec![5, 5, -3]),
    ] {
        let shares = Money::usd(amount).allocate_by(&weights).unwrap();
        assert_eq!(
            shares.iter().map(|m| m.minor).sum::<i64>(),
            amount,
            "{weights:?}"
        );
    }
    assert_eq!(Money::usd(1).allocate_by(&[]), Err(MoneyError::NoParts));
    assert_eq!(
        Money::usd(1).allocate_by(&[1, -1]),
        Err(MoneyError::NoParts)
    );
}

/// The four rules on ties and non-ties, positive and negative:
/// `minor * 1 / 4` gives x.25 / x.5 / x.75 cases.
#[test]
fn rounding_rules_on_ties_and_non_ties() {
    use Rounding::*;
    // (minor, num, den, [HalfUp, HalfEven, Down, Up])
    let cases: [(i64, i64, i64, [i64; 4]); 12] = [
        (5, 1, 2, [3, 2, 2, 3]),       // 2.5
        (7, 1, 2, [4, 4, 3, 4]),       // 3.5
        (-5, 1, 2, [-3, -2, -2, -3]),  // -2.5
        (-7, 1, 2, [-4, -4, -3, -4]),  // -3.5
        (9, 1, 4, [2, 2, 2, 3]),       // 2.25
        (11, 1, 4, [3, 3, 2, 3]),      // 2.75
        (-9, 1, 4, [-2, -2, -2, -3]),  // -2.25
        (-11, 1, 4, [-3, -3, -2, -3]), // -2.75
        (10, 1, 2, [5, 5, 5, 5]),      // exact
        (5, -1, 2, [-3, -2, -2, -3]),  // negative ratio
        (5, 1, -2, [-3, -2, -2, -3]),  // negative denominator
        (0, 1, 3, [0, 0, 0, 0]),
    ];
    for (minor, num, den, expected) in cases {
        for (rounding, want) in [HalfUp, HalfEven, Down, Up].into_iter().zip(expected) {
            let got = Money::usd(minor).mul_ratio(num, den, rounding).unwrap();
            assert_eq!(got, Money::usd(want), "{minor}*{num}/{den} {rounding}");
        }
    }
}

#[test]
fn percent_uses_basis_points() {
    // 10.50 * 15 % = 1.575
    let m = Money::usd(1050);
    assert_eq!(m.percent(1500, Rounding::HalfUp).unwrap().minor, 158);
    assert_eq!(m.percent(1500, Rounding::HalfEven).unwrap().minor, 158);
    assert_eq!(m.percent(1500, Rounding::Down).unwrap().minor, 157);
    assert_eq!(m.percent(1500, Rounding::Up).unwrap().minor, 158);
    // 5 % of 1.10 = 0.055 -> tie
    let n = Money::usd(110);
    assert_eq!(n.percent(500, Rounding::HalfUp).unwrap().minor, 6);
    assert_eq!(n.percent(500, Rounding::HalfEven).unwrap().minor, 6);
    assert_eq!(
        Money::usd(130)
            .percent(500, Rounding::HalfEven)
            .unwrap()
            .minor,
        6
    );
    assert_eq!(
        Money::usd(-110).percent(500, Rounding::Down).unwrap().minor,
        -5
    );
    assert_eq!(
        Money::usd(-110)
            .percent(500, Rounding::HalfUp)
            .unwrap()
            .minor,
        -6
    );
}

#[test]
fn mul_ratio_errors() {
    assert_eq!(
        Money::usd(1).mul_ratio(1, 0, Rounding::Down),
        Err(MoneyError::DivisionByZero)
    );
    assert_eq!(
        Money::usd(i64::MAX).mul_ratio(2, 1, Rounding::Down),
        Err(MoneyError::Overflow)
    );
    // The intermediate product is i128, so a ratio that shrinks is fine.
    assert_eq!(
        Money::usd(i64::MAX)
            .mul_ratio(2, 4, Rounding::Down)
            .unwrap()
            .minor,
        i64::MAX / 2
    );
}

#[test]
fn convert_handles_minor_unit_digits() {
    use Rounding::*;
    // 10.00 USD at 3.6725 = 36.725 AED (tie at the third decimal -> 2 decimals)
    let usd = Money::usd(1000);
    assert_eq!(
        usd.convert("3.6725", Currency::AED, HalfUp).unwrap(),
        Money::aed(3673)
    );
    assert_eq!(
        usd.convert("3.6725", Currency::AED, HalfEven).unwrap(),
        Money::aed(3672)
    );
    assert_eq!(
        usd.convert("3.6725", Currency::AED, Down).unwrap(),
        Money::aed(3672)
    );
    assert_eq!(
        usd.convert("3.6725", Currency::AED, Up).unwrap(),
        Money::aed(3673)
    );
    // USD (2) to JPY (0): 10.00 * 149.5 = 1495 yen
    assert_eq!(
        usd.convert("149.5", Currency::JPY, HalfUp).unwrap(),
        Money::jpy(1495)
    );
    assert_eq!(
        Money::usd(1001)
            .convert("149.5", Currency::JPY, HalfUp)
            .unwrap(),
        Money::jpy(1496) // 1496.495
    );
    // KWD (3) to USD (2): 1.000 KWD at 3.25 = 3.25 USD; 0.001 KWD = 0.00325
    assert_eq!(
        Money::kwd(1000)
            .convert("3.25", Currency::USD, HalfUp)
            .unwrap(),
        Money::usd(325)
    );
    assert_eq!(
        Money::kwd(1)
            .convert("3.25", Currency::USD, HalfUp)
            .unwrap(),
        Money::usd(0)
    );
    assert_eq!(
        Money::kwd(1).convert("3.25", Currency::USD, Up).unwrap(),
        Money::usd(1)
    );
    // JPY (0) to BHD (3): 1000 yen at 0.0025 = 2.5 BHD
    assert_eq!(
        Money::jpy(1000)
            .convert("0.0025", Currency::BHD, HalfUp)
            .unwrap(),
        Money::bhd(2500)
    );
    // Negative amounts and a ratio pair.
    assert_eq!(
        Money::usd(-1000)
            .convert("3.6725", Currency::AED, HalfUp)
            .unwrap(),
        Money::aed(-3673)
    );
    assert_eq!(
        usd.convert((7, 2), Currency::EUR, Down).unwrap(),
        Money::eur(3500)
    );
}

#[test]
fn convert_rejects_bad_rates() {
    for bad in [
        "",
        "0",
        "0.0",
        "-1",
        "1e3",
        "1.2.3",
        "abc",
        ".5",
        "5.",
        "1,5",
        "0.1234567890123456789",
    ] {
        assert!(
            matches!(
                Money::usd(1).convert(bad, Currency::EUR, Rounding::Down),
                Err(MoneyError::InvalidRate(_))
            ),
            "{bad:?}"
        );
    }
    assert!(
        Money::usd(1)
            .convert((1, 0), Currency::EUR, Rounding::Down)
            .is_err()
    );
    assert!(
        Money::usd(1)
            .convert((0, 1), Currency::EUR, Rounding::Down)
            .is_err()
    );
    assert!(
        Money::usd(1)
            .convert((-1, 1), Currency::EUR, Rounding::Down)
            .is_err()
    );
    assert_eq!(Rate::parse(" 1.5 ").unwrap(), Rate::new(15, 10).unwrap());
    // A huge product is an error, not a panic.
    assert_eq!(
        Money::usd(i64::MAX).convert("9999999999999999.99", Currency::JPY, Rounding::Down),
        Err(MoneyError::Overflow)
    );
}

#[test]
fn formats_with_the_currencys_own_decimals() {
    assert_eq!(Money::jpy(1250).to_string(), "¥1,250");
    assert_eq!(Money::usd(125050).to_string(), "$1,250.50");
    assert_eq!(Money::aed(5).to_string(), "AED 0.05");
    assert_eq!(Money::bhd(1234567).to_string(), "BHD 1,234.567");
    assert_eq!(Money::kwd(-5).to_string(), "-KWD 0.005");
    assert_eq!(Money::usd(0).to_string(), "$0.00");
    assert_eq!(
        Money::usd(i64::MIN).to_string(),
        "-$92,233,720,368,547,758.08"
    );
    // Agrees with the existing float-based formatter.
    assert_eq!(
        Money::eur(125050).format("de"),
        renox::format_money(1250.5, "EUR", None, "de")
    );
    assert_eq!(Money::eur(125050).format("de"), "€1.250,50");
    assert_eq!(Money::usd(125050).format("fr"), "$1\u{202f}250,50");
}

#[test]
fn parses_amounts_without_silent_rounding() {
    assert_eq!(
        Money::parse("12.50", Currency::USD).unwrap(),
        Money::usd(1250)
    );
    assert_eq!(
        Money::parse("12.5", Currency::USD).unwrap(),
        Money::usd(1250)
    );
    assert_eq!(
        Money::parse("-0.07", Currency::USD).unwrap(),
        Money::usd(-7)
    );
    assert_eq!(
        Money::parse("1250", Currency::JPY).unwrap(),
        Money::jpy(1250)
    );
    assert_eq!(
        Money::parse("1.234", Currency::KWD).unwrap(),
        Money::kwd(1234)
    );
    assert_eq!(
        Money::parse("10.5", Currency::JPY),
        Err(MoneyError::TooManyDecimals {
            currency: Currency::JPY,
            allowed: 0
        })
    );
    assert!(matches!(
        Money::parse("1.234", Currency::USD),
        Err(MoneyError::TooManyDecimals { allowed: 2, .. })
    ));
    for bad in ["", "-", "abc", "1.", ".5", "1,5", "1e3", "--1"] {
        assert!(
            matches!(
                Money::parse(bad, Currency::USD),
                Err(MoneyError::InvalidAmount(_))
            ),
            "{bad:?}"
        );
    }
    assert_eq!(
        Money::parse("99999999999999999999", Currency::USD),
        Err(MoneyError::Overflow)
    );
    assert_eq!(
        Money::parse_locale("1.250,50", Currency::EUR, "de").unwrap(),
        Money::eur(125050)
    );
    assert_eq!(
        Money::parse_locale("1,250.50", Currency::USD, "en").unwrap(),
        Money::usd(125050)
    );
    // What `format` writes (without the symbol) reads back.
    let m = Money::usd(-123456789);
    assert_eq!(
        Money::parse_locale(&m.format("de").replace('$', ""), Currency::USD, "de").unwrap(),
        m
    );
}

#[test]
fn serde_round_trip() {
    let m = Money::aed(12500);
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(json, r#"{"minor":12500,"currency":"AED"}"#);
    assert_eq!(serde_json::from_str::<Money>(&json).unwrap(), m);
    let value = serde_json::to_value(Money::jpy(-3)).unwrap();
    assert_eq!(value, serde_json::json!({"minor": -3, "currency": "JPY"}));
    assert!(serde_json::from_str::<Money>(r#"{"minor":1,"currency":"XXX"}"#).is_err());
    assert_eq!(
        serde_json::from_str::<Money>(r#"{"minor":1,"currency":"usd"}"#).unwrap(),
        Money::usd(1)
    );
    assert_eq!(serde_json::to_string(&Currency::KWD).unwrap(), r#""KWD""#);
}

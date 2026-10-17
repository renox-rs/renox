//! `Quantity` (#69): exact parsing with explicit rounding, arithmetic,
//! rescaling, display and serde.

use renox::money::Rounding::{self, Down, HalfEven, HalfUp, Up};
use renox::quantity::{Quantity, QuantityError, UnitTable, Units};

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

fn table() -> UnitTable {
    Units::new()
        .base("kg")
        .unit("g", "kg", "0.001")
        .unit("t", "kg", "1000")
        .base("m")
        .build()
        .unwrap()
}

#[test]
fn units_convert_exactly() {
    let t = table();
    let kg = Q3::parse_exact("2.5").unwrap();
    assert_eq!(
        t.convert(kg, "kg", "g", HalfUp).unwrap().to_string(),
        "2500.000"
    );
    let g = Q3::parse_exact("1500").unwrap();
    assert_eq!(
        t.convert(g, "g", "kg", HalfUp).unwrap().to_string(),
        "1.500"
    );
    assert_eq!(t.convert(g, "g", "g", Down).unwrap(), g);
    assert_eq!(t.convert(kg, "t", "t", Down).unwrap(), kg);
    assert_eq!(
        t.convert(Q3::from_raw(1), "t", "g", Down).unwrap().raw(),
        1_000_000
    );
}

#[test]
fn units_round_as_asked_when_digits_are_lost() {
    let t = table();
    // Whole grams to whole kilograms: raw 500 g is 0.5 kg.
    for (raw, r, want) in [
        (1, Down, 0),
        (1, Up, 1),
        (1, HalfUp, 0),
        (-1, Up, -1),
        (-1, Down, 0),
        (500, HalfUp, 1),
        (500, HalfEven, 0),
        (1500, HalfEven, 2),
        (-500, HalfUp, -1),
        (-500, HalfEven, 0),
    ] {
        let got = t.convert(Quantity::<0>::from_raw(raw), "g", "kg", r);
        assert_eq!(got.unwrap().raw(), want, "{raw} {r:?}");
    }
}

#[test]
fn units_reject_bad_conversions() {
    let t = table();
    let q = Q3::from_raw(1000);
    assert_eq!(
        t.convert(q, "kg", "lb", Down).unwrap_err(),
        QuantityError::UnknownUnit("lb".into())
    );
    assert_eq!(
        t.convert(q, "lb", "kg", Down).unwrap_err(),
        QuantityError::UnknownUnit("lb".into())
    );
    assert!(matches!(
        t.convert(q, "kg", "m", Down),
        Err(QuantityError::IncompatibleUnits { .. })
    ));
    assert_eq!(
        t.convert(Q3::from_raw(i64::MAX), "t", "g", Down)
            .unwrap_err(),
        QuantityError::Overflow
    );
}

#[test]
fn units_table_is_validated_when_built() {
    let bad = |u: Units| matches!(u.build(), Err(QuantityError::InvalidUnits(_)));
    assert!(bad(Units::new().base("kg").unit("g", "kg", "0")));
    assert!(bad(Units::new().base("kg").unit("g", "kg", "0.000")));
    assert!(bad(Units::new().base("kg").unit("g", "kg", "-1")));
    assert!(bad(Units::new().base("kg").unit("g", "kg", "1e3")));
    assert!(bad(Units::new().base("kg").unit("g", "kg", "1,5")));
    assert!(bad(Units::new().base("kg").unit("g", "kg", "")));
    assert!(bad(Units::new().base("kg").unit(
        "g",
        "kg",
        "0.1234567890123456789"
    )));
    assert!(bad(Units::new().unit("g", "kg", "0.001")));
    assert!(bad(Units::new()
        .base("kg")
        .unit("g", "kg", "0.001")
        .unit("mg", "g", "0.001")));
    assert!(bad(Units::new().base("kg").base("kg")));
    assert!(bad(Units::new().base("")));
    assert!(
        Units::new()
            .unit("g", "kg", "0.001")
            .base("kg")
            .build()
            .is_ok()
    );
    assert!(table().contains("t"));
    assert!(!table().contains("lb"));
}

#[test]
fn units_handle_non_decimal_ratios_exactly() {
    // 1 box = 12 pieces, so pieces to boxes divides by 12.
    let t = Units::new()
        .base("pc")
        .unit("box", "pc", "12")
        .build()
        .unwrap();
    let pcs = Quantity::<0>::from_raw(30);
    assert_eq!(t.convert(pcs, "pc", "box", Down).unwrap().raw(), 2);
    assert_eq!(t.convert(pcs, "pc", "box", Up).unwrap().raw(), 3);
    assert_eq!(t.convert(pcs, "pc", "box", HalfEven).unwrap().raw(), 2);
    assert_eq!(t.convert(pcs, "pc", "box", HalfUp).unwrap().raw(), 3);
}

struct QuantityPage;

impl renox::prelude::Module for QuantityPage {
    fn name(&self) -> &'static str {
        "quantity_page"
    }
    fn routes(&self) -> renox::prelude::Routes {
        use renox::prelude::*;
        Routes::new().get("/q", || async {
            view(
                "q.html",
                context! {
                    q => Quantity::<3>::from_raw(1_234_567),
                    neg => Quantity::<3>::from_raw(-2500),
                    big => Quantity::<3>::from_raw(i64::MAX),
                },
            )
        })
    }
}

#[renox::test]
async fn the_quantity_filter_shows_exact_digits_units_and_locale_separators() {
    use renox::prelude::*;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("q.html"),
        "{{ q | quantity }}|{{ q | quantity(unit='kg') }}|{{ q | quantity(decimals=1) }}|{{ q | quantity(decimals=5) }}|{{ neg | quantity(decimals=0) }}|{{ 7 | quantity }}|{{ '0.0004' | quantity(decimals=3) }}|{{ '9.9996' | quantity(decimals=3) }}|{{ big | quantity }}",
    )
    .unwrap();
    for (locale, expected) in [
        (
            "en",
            "1,234.567|1,234.567 kg|1,234.6|1,234.56700|-3|7|0.000|10.000|9,223,372,036,854,775.807",
        ),
        (
            "es",
            "1.234,567|1.234,567 kg|1.234,6|1.234,56700|-3|7|0,000|10,000|9.223.372.036.854.775,807",
        ),
    ] {
        let path = dir.path().to_path_buf();
        let app = renox::testing::TestApp::with_config(App::new().module(QuantityPage), move |c| {
            c.views_path = path;
            c.locale = locale.into();
        })
        .await;
        app.get("/q").await.assert_see(expected);
    }
}

#[renox::test]
async fn the_quantity_filter_rejects_text_that_is_not_a_decimal() {
    use renox::prelude::*;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("q.html"), "{{ '1e3' | quantity }}").unwrap();
    let path = dir.path().to_path_buf();
    let app = renox::testing::TestApp::with_config(App::new().module(QuantityPage), move |c| {
        c.views_path = path
    })
    .await;
    app.get("/q").await.assert_status(500);
}

#[test]
fn quantity_binds_as_its_scaled_integer() {
    use renox::db::{DbValue, ToDbValue};
    assert_eq!(Q3::from_raw(-2500).to_db_value(), DbValue::Integer(-2500));
    assert_eq!(Some(Q3::from_raw(7)).to_db_value(), DbValue::Integer(7));
    assert_eq!(None::<Q3>.to_db_value(), DbValue::Null);
}

#[renox::test]
async fn quantities_round_trip_through_columns() {
    use renox::prelude::*;
    let app = renox::testing::TestApp::new(App::new()).await;
    let db = app.db();
    renox::db::sql("CREATE TABLE stock (id BIGINT PRIMARY KEY, amount BIGINT, spare BIGINT)")
        .execute(db)
        .await
        .unwrap();
    let cases = [
        (1_i64, Q3::from_raw(i64::MAX), Some(Q3::from_raw(i64::MIN))),
        (2, Q3::parse_exact("2.500").unwrap(), None),
        (3, Q3::from_raw(-1), Some(Q3::ZERO)),
    ];
    for (id, amount, spare) in cases {
        renox::db::sql("INSERT INTO stock (id, amount, spare) VALUES (?, ?, ?)")
            .bind(id)
            .bind(amount)
            .bind(spare)
            .execute(db)
            .await
            .unwrap();
    }
    for (id, amount, spare) in cases {
        let row = renox::db::sql("SELECT amount, spare FROM stock WHERE id = ?")
            .bind(id)
            .fetch_one(db)
            .await
            .unwrap();
        assert_eq!(row.try_get::<Q3>("amount").unwrap(), amount);
        assert_eq!(row.try_get::<Option<Q3>>("spare").unwrap(), spare);
    }
    let sums: Vec<(Q3,)> =
        renox::db::sql("SELECT CAST(SUM(amount) AS BIGINT) FROM stock WHERE id > 1")
            .fetch_as(db)
            .await
            .unwrap();
    assert_eq!(sums[0].0.to_string(), "2.499");
}

//! Money in Rust. Every amount is an integer in the smallest unit of
//! `APP_CURRENCY` (cents with `USD`, the shop's currency): what the models
//! store and what the `money` filter and the grids' money columns read, so
//! templates write `{{ price | money }}` and nothing else.
//!
//! Two places deal in whole units instead, and convert here with the
//! currency's decimals (`renox::currency_decimals`): text built in Rust
//! with `renox::format_money` (mails, notifications, JSON for the counter),
//! and the forms people type amounts into (`12.50`, not `1250`).

/// What one whole unit of `currency` is in its smallest unit: 100 for
/// `USD`, 1 for `IDR`.
pub fn scale(currency: &str) -> i64 {
    10i64.pow(renox::currency_decimals(currency).min(6))
}

/// `amount` (smallest unit) in whole units: 129999 → 1299.99 in `USD`.
pub fn to_whole(amount: i64, currency: &str) -> f64 {
    amount as f64 / scale(currency) as f64
}

/// A whole-unit amount (as typed in a form) in the smallest unit, rounded
/// to it: 12.5 → 1250 in `USD`.
pub fn from_whole(value: f64, currency: &str) -> i64 {
    (value * scale(currency) as f64).round() as i64
}

/// `amount` (smallest unit) as the `money` filter writes it: 129999 →
/// `$1,299.99` in English, `$1.299,99` in Spanish.
pub fn format(amount: i64, currency: &str, locale: &str) -> String {
    renox::format_money(to_whole(amount, currency), currency, None, locale)
}

/// An amount typed at the counter (`1,250.50`, `1.250,50`, `80`) in the
/// smallest unit. The last `.` or `,` followed by one or two digits is the
/// decimal point; other separators group thousands. `None` when it isn't
/// a number.
pub fn parse(text: &str, currency: &str) -> Option<i64> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }
    let (whole, fraction) = match text.rfind(['.', ',']) {
        Some(at) if (1..=2).contains(&(text.len() - at - 1)) => (&text[..at], &text[at + 1..]),
        _ => (text.as_str(), ""),
    };
    let digits: String = whole.chars().filter(char::is_ascii_digit).collect();
    let value: f64 = format!(
        "{}.{}",
        if digits.is_empty() { "0" } else { &digits },
        fraction
    )
    .trim_end_matches('.')
    .parse()
    .ok()?;
    Some(from_whole(value, currency))
}

/// An amount typed in a form (whole units, `Option<f64>`) in the smallest
/// unit of the current `APP_CURRENCY`; 0 when left out.
pub fn from_form(value: Option<f64>) -> i64 {
    from_whole(value.unwrap_or(0.0), &currency())
}

/// The `step` of a number input for an amount in whole units: `0.01` for
/// `USD`, `1` for `IDR` (shared with every view as `money_step`).
pub fn step(currency: &str) -> String {
    match renox::currency_decimals(currency).min(6) {
        0 => "1".to_owned(),
        decimals => format!("0.{}1", "0".repeat(decimals as usize - 1)),
    }
}

/// The currency's symbol, for a form field's prefix: `$`, `Rp` (shared
/// with every view as `money_symbol`).
pub fn symbol(currency: &str) -> String {
    renox::format_money(0.0, currency, Some(0), "en")
        .trim_end_matches('0')
        .trim()
        .to_owned()
}

/// `APP_CURRENCY` of the request, job or command running now (for code
/// without the state at hand, like an admin resource's `fill`); `USD`
/// outside one.
pub fn currency() -> String {
    renox::context::app().map_or_else(|| "USD".to_owned(), |state| state.config.currency.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_convert_with_the_currency_decimals() {
        assert_eq!(scale("USD"), 100);
        assert_eq!(scale("IDR"), 1);
        assert_eq!(to_whole(129_999, "USD"), 1_299.99);
        assert_eq!(from_whole(12.5, "USD"), 1_250);
        assert_eq!(from_whole(0.1 + 0.2, "USD"), 30);
        assert_eq!(from_whole(75_000.0, "IDR"), 75_000);
        assert_eq!(format(129_999, "USD", "en"), "$1,299.99");
        assert_eq!(format(129_999, "USD", "es"), "$1.299,99");
        assert_eq!(step("USD"), "0.01");
        assert_eq!(step("IDR"), "1");
        assert_eq!(symbol("USD"), "$");
        assert_eq!(symbol("IDR"), "Rp");
    }

    #[test]
    fn typed_amounts_take_either_decimal_point() {
        assert_eq!(parse("80", "USD"), Some(8_000));
        assert_eq!(parse("1,250.50", "USD"), Some(125_050));
        assert_eq!(parse("1.250,50", "USD"), Some(125_050));
        assert_eq!(parse("12,5", "USD"), Some(1_250));
        assert_eq!(parse("1,250", "USD"), Some(125_000));
        assert_eq!(parse(" 20.00 ", "USD"), Some(2_000));
        assert_eq!(parse("75.000", "IDR"), Some(75_000));
        assert_eq!(parse("", "USD"), None);
        assert_eq!(parse("ten", "USD"), None);
        assert_eq!(parse("-5", "USD"), None);
    }
}

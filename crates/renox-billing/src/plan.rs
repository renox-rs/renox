//! Plans: what an app sells, declared in code (there is no plans table).

use renox::Config;
use renox::chrono::{Days, Months};
use renox::prelude::*;
use serde::Serialize;

/// How often a plan is charged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Interval {
    /// Every day.
    Day,
    /// Every week.
    Week,
    /// Every month.
    Month,
    /// Every year.
    Year,
}

impl Interval {
    /// `"day"`, `"week"`, `"month"` or `"year"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    /// One interval after `from` (a month after 31 January is the last day
    /// of February).
    pub fn after(&self, from: DateTime) -> DateTime {
        let next = match self {
            Self::Day => from.checked_add_days(Days::new(1)),
            Self::Week => from.checked_add_days(Days::new(7)),
            Self::Month => from.checked_add_months(Months::new(1)),
            Self::Year => from.checked_add_months(Months::new(12)),
        };
        next.unwrap_or(from)
    }
}

/// A plan an app sells: a key that never changes once people subscribed
/// (it is stored in `subscriptions.plan`), a name for people, a price and
/// how often it's charged.
///
/// ```
/// use renox_billing::{Interval, Plan};
///
/// let pro = Plan::new("pro", "Pro")
///     .price(1_900, "USD", Interval::Month) // $19.00: amounts are in cents
///     .trial_days(14)
///     .description("For growing teams")
///     .feature("Unlimited projects")
///     .feature("Priority support");
/// assert_eq!(pro.amount, 1_900);
/// ```
///
/// The amount is in the currency's smallest unit as Renox counts it
/// ([`renox::currency_decimals`]): cents for `USD`, rupiah for `IDR`. Stripe
/// charges the Price set up in its dashboard ([`Plan::price_id`], or
/// `STRIPE_PRICE_PRO` in `.env`), so there the amount is what the plan
/// pages show; Xendit charges this amount.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct Plan {
    /// The plan's key: short, stable, e.g. `pro`.
    pub key: String,
    /// Its name for people: `Pro`.
    pub label: String,
    /// A line under the name on the plans page.
    pub description: Option<String>,
    /// The price per interval, in the currency's smallest unit.
    pub amount: i64,
    /// The ISO 4217 currency code (`USD`, `IDR`).
    pub currency: String,
    /// How often it's charged.
    pub interval: Interval,
    /// Free days before the first charge, for an owner's first
    /// subscription (0: none).
    pub trial_days: u32,
    /// What the plan includes, listed on the plans page.
    pub features: Vec<String>,
    /// The gateway that sells it (`stripe`, `xendit`); `None`: the module's
    /// first gateway that is set up.
    pub gateway: Option<String>,
    /// Price ids per gateway.
    #[serde(skip)]
    price_ids: Vec<(String, String)>,
}

impl Plan {
    /// A plan with this key and name, free until [`Plan::price`] is set,
    /// charged monthly in `USD`.
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: None,
            amount: 0,
            currency: "USD".into(),
            interval: Interval::Month,
            trial_days: 0,
            features: Vec::new(),
            gateway: None,
            price_ids: Vec::new(),
        }
    }

    /// Its price: `amount` in the smallest unit of `currency`, every
    /// `interval`.
    pub fn price(mut self, amount: i64, currency: &str, interval: Interval) -> Self {
        self.amount = amount;
        self.currency = currency.trim().to_ascii_uppercase();
        self.interval = interval;
        self
    }

    /// Free days before the first charge.
    pub fn trial_days(mut self, days: u32) -> Self {
        self.trial_days = days;
        self
    }

    /// A line under the name on the plans page.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Adds a line to what the plan includes.
    pub fn feature(mut self, text: impl Into<String>) -> Self {
        self.features.push(text.into());
        self
    }

    /// Sold through this gateway (by its name) rather than the module's
    /// first one, e.g. `IDR` plans through `xendit`.
    pub fn via(mut self, gateway: impl Into<String>) -> Self {
        self.gateway = Some(gateway.into());
        self
    }

    /// The plan's price id at `gateway` (Stripe's `price_…`). Without one,
    /// [`Plan::price_id_for`] reads `<GATEWAY>_PRICE_<KEY>` from the
    /// configuration, e.g. `STRIPE_PRICE_PRO`, so test and live keys can
    /// use different prices.
    pub fn price_id(mut self, gateway: impl Into<String>, id: impl Into<String>) -> Self {
        let gateway = gateway.into();
        self.price_ids.retain(|(g, _)| *g != gateway);
        self.price_ids.push((gateway, id.into()));
        self
    }

    /// The price id at `gateway`: given with [`Plan::price_id`], else
    /// `<GATEWAY>_PRICE_<KEY>` (upper case, `-` as `_`) from `config`.
    pub fn price_id_for(&self, gateway: &str, config: &Config) -> Option<String> {
        if let Some((_, id)) = self.price_ids.iter().find(|(g, _)| g == gateway) {
            return Some(id.clone());
        }
        let name = format!("{gateway}_PRICE_{}", self.key)
            .to_ascii_uppercase()
            .replace(['-', '.'], "_");
        config.var(&name)
    }

    /// The price for people, e.g. `$19.00 / month` (`Free` at 0), with the
    /// locale's separators.
    pub fn price_label(&self, locale: &str) -> String {
        if self.amount == 0 {
            return "Free".into();
        }
        let whole =
            self.amount as f64 / 10f64.powi(renox::currency_decimals(&self.currency) as i32);
        format!(
            "{} / {}",
            renox::format_money(whole, &self.currency, None, locale),
            self.interval.as_str()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_prices_in_whole_units() {
        let pro = Plan::new("pro", "Pro").price(1_900, "usd", Interval::Month);
        assert_eq!(pro.currency, "USD");
        assert_eq!(pro.price_label("en"), "$19.00 / month");
        let idr = Plan::new("basic", "Basic").price(99_000, "IDR", Interval::Year);
        assert_eq!(idr.price_label("en"), "Rp 99,000 / year");
        assert_eq!(Plan::new("free", "Free").price_label("en"), "Free");
        let daily = Plan::new("day", "Day pass").price(500, "USD", Interval::Day);
        assert_eq!(daily.price_label("en"), "$5.00 / day");
        assert_eq!(Interval::Week.as_str(), "week");
    }

    #[test]
    fn price_ids_come_from_code_or_the_configuration() {
        let mut config = Config::default();
        config
            .vars
            .insert("STRIPE_PRICE_TEAM_PLUS".into(), "price_env".into());
        let plan = Plan::new("team-plus", "Team plus");
        assert_eq!(
            plan.price_id_for("stripe", &config).as_deref(),
            Some("price_env")
        );
        let plan = plan.price_id("stripe", "price_code");
        assert_eq!(
            plan.price_id_for("stripe", &config).as_deref(),
            Some("price_code")
        );
        assert_eq!(plan.price_id_for("xendit", &config), None);
    }

    #[test]
    fn intervals_step_forward() {
        let start = renox::chrono::DateTime::parse_from_rfc3339("2026-01-31T10:00:00Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            Interval::Month.after(start).to_rfc3339(),
            "2026-02-28T10:00:00+00:00"
        );
        assert_eq!(
            Interval::Year.after(start).to_rfc3339(),
            "2027-01-31T10:00:00+00:00"
        );
    }
}

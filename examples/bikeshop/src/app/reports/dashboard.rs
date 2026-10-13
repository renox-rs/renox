//! `GET /staff/reports` (`reports.dashboard`): how the business is doing,
//! for any period, by stream and by store.
//!
//! The page reads three things from the address:
//! - `?period=` (`renox::chart::Period`: `7d`, `30d`, `90d`, `12w`, `12m`,
//!   `ytd`, or a custom range from the kit's `period_filter`);
//! - `?by=books|work` ([`By`]): income in the books of the store that owns
//!   the bike or goods, or at the store that did the work;
//! - `?store=` (one of the person's stores; all of them without it).
//!
//! The numbers come from [`Numbers::for_page`] (cached, see
//! `super::numbers`). The staff home page (`/staff`) shows a short
//! version for the active store: [`overview`].

use renox::chart::Period;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::model::STREAMS;
use super::numbers::Numbers;
use super::scope::{By, Reach};
use crate::app::access::{active_store, can_in, catalogue};

/// `?by=` and `?store=`.
#[derive(Deserialize, Default, Debug)]
pub struct DashboardQuery {
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub store: Option<i64>,
}

/// A named series for `chart(…)`.
#[derive(Serialize, Debug, Clone)]
pub struct Line {
    pub name: String,
    pub values: Vec<f64>,
}

/// The periods offered above the dashboard: the kit's five and 12 weeks.
pub fn periods(lang: &Lang) -> Vec<[String; 2]> {
    ["7d", "30d", "12w", "90d", "12m", "ytd"]
        .iter()
        .map(|key| {
            [
                (*key).to_owned(),
                lang.t(&format!("reports.period.{key}"), &[]),
            ]
        })
        .collect()
}

/// The stream's name in the visitor's language.
pub fn stream_name(lang: &Lang, key: &str) -> String {
    lang.t(&format!("reports.stream.{key}"), &[])
}

// [explain:reports.dashboard.handler]
/// `GET /staff/reports` (`reports.dashboard`).
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    period: Period,
    Query(query): Query<DashboardQuery>,
) -> Result<View> {
    let reach = Reach::of(&state.db, &user, query.store).await?;
    if reach.is_empty() {
        return Err(Error::Forbidden);
    }
    let by = By::parse(query.by.as_deref());
    let numbers = Numbers::for_page(&state, &reach, by, period).await?;
    // [/explain:reports.dashboard.handler]
    // The charts' series, named in the visitor's language.
    let revenue: Vec<Line> = numbers
        .streams
        .iter()
        .map(|s| Line {
            name: stream_name(&lang, &s.key),
            values: s.trend.clone(),
        })
        .collect();
    let stream_labels: Vec<String> = STREAMS.iter().map(|k| stream_name(&lang, k)).collect();
    let stream_amounts: Vec<i64> = numbers.streams.iter().map(|s| s.amount).collect();
    let comparison_labels: Vec<String> =
        numbers.comparison.iter().map(|s| s.name.clone()).collect();
    let comparison: Vec<Line> = STREAMS
        .iter()
        .enumerate()
        .map(|(i, key)| Line {
            name: stream_name(&lang, key),
            values: numbers
                .comparison
                .iter()
                .map(|s| s.streams[i] as f64)
                .collect(),
        })
        .collect();
    // [explain:reports.dashboard.handler]
    let store_name = reach.store.map(|id| reach.name_of(id));
    Ok(view(
        "reports/dashboard.html",
        context! {
            period,
            periods => periods(&lang),
            tabs => super::grids::tabs(),
            by => by.key(),
            reach,
            store_name,
            n => numbers,
            revenue,
            stream_labels,
            stream_amounts,
            comparison_labels,
            comparison,
        },
    ))
}
// [/explain:reports.dashboard.handler]

/// The staff home page's short version of the dashboard: the active
/// store's last 7 days, by the work it did. `None` when the person may not
/// see reports in the active store (then the home page shows its own
/// content).
#[derive(Serialize, Debug, Clone)]
pub struct Overview {
    pub store: String,
    pub numbers: Numbers,
}

/// The [`Overview`] for the person on the staff home page.
pub async fn overview(state: &AppState, user: &User) -> Result<Option<Overview>> {
    let Some(store) = active_store::current() else {
        return Ok(None);
    };
    if !can_in(user, catalogue::REPORTS_VIEW, store) {
        return Ok(None);
    }
    let reach = Reach::of(&state.db, user, Some(store)).await?;
    let numbers = Numbers::for_page(state, &reach, By::Work, Period::days(7)).await?;
    Ok(Some(Overview {
        store: reach.name_of(store),
        numbers,
    }))
}

//! Pages about the example itself: `/about/pages`, the index of every page
//! with the Renox features it uses (filtered by feature or by who uses the
//! page); `/about/data`, the data model explained ([`data`]); and
//! `/about/blocks`, the bike shop's own UI blocks working (`blocks.rs`);
//! `/about/fields`, every form input with its Rust and database types
//! (`fields.rs`); `/about/htmx`, htmx and Alpine recipes, live (`htmx.rs`); `/about/htmx/live`, the same checklist as a live component
//! (`live.rs`).
//!
//! The "About this page" mechanism is in `src/explain.rs`; the panel every
//! page shows is `resources/views/components/about_panel.html`.

pub mod blocks;
pub mod data;
pub mod explain;
pub mod fields;
pub mod htmx;
pub mod live;

use crate::app::staff::model::Store;
use crate::explain::{self as about_this_page, Audience};
use renox::prelude::*;
use std::collections::BTreeSet;

/// The about pages.
pub struct About;

impl Module for About {
    fn name(&self) -> &'static str {
        "about"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/about/pages", pages)
            .name("about.pages")
            .get("/about/data", data::show)
            .name("about.data")
            .merge(blocks::routes())
            .merge(fields::routes())
            .merge(htmx::routes())
            .merge(live::routes())
    }

    fn register(&self, app: &mut Registry) {
        app.live_component::<live::LiveChecklist>();
    }
}

/// `/about/pages?feature=renox::grid&audience=cashier`.
#[derive(Debug, Default, serde::Deserialize)]
struct Filters {
    feature: Option<String>,
    audience: Option<String>,
}

/// Every page with its purpose and features, filtered by a feature and by
/// who uses it.
// [explain:about.pages.handler]
async fn pages(
    State(state): State<AppState>,
    lang: Lang,
    Query(filters): Query<Filters>,
) -> Result<View> {
    let translate = about_this_page::translator(lang);
    let all = about_this_page::all();
    // [/explain:about.pages.handler]

    // The filters' options: every feature used anywhere, every audience.
    let features: BTreeSet<&str> = all
        .iter()
        .flat_map(|e| e.features.iter().map(|f| f.api))
        .collect();
    let feature = filters.feature.filter(|f| features.contains(f.as_str()));
    let audience = filters.audience.as_deref().and_then(Audience::from_key);

    // A store's page is on its own host: link to the first store's.
    let first_store = crate::app::staff::model::Store::query()
        .order_by(Store::ID)
        .first(&state.db)
        .await?
        .map(|store| crate::app::home::stores::site_url(&state.config, &store.slug));
    // [explain:about.pages.handler]
    let mut pages: Vec<about_this_page::Page> = all
        .iter()
        .filter(|e| {
            feature
                .as_deref()
                .is_none_or(|f| e.features.iter().any(|x| x.api == f))
        })
        .filter(|e| audience.is_none_or(|a| e.audience.contains(&a)))
        .map(|e| {
            let mut page = e.localize(&translate);
            // A link when the page needs no parameters.
            page.url = if e.route == crate::app::home::stores::ROUTE {
                first_store.clone()
            } else {
                state.url(e.route, &[]).ok()
            };
            page
        })
        .collect();
    pages.sort_by(|a, b| a.title.cmp(&b.title));
    // [/explain:about.pages.handler]

    // The audience filter's options, as the kit's `select` takes them.
    let audiences: Vec<_> = Audience::ALL
        .iter()
        .map(|a| {
            let label = translate(&format!("about.audience.{}", a.key()))
                .unwrap_or_else(|| a.label().to_owned());
            context! { value => a.key(), label }
        })
        .collect();

    Ok(view(
        "about/pages.html",
        context! {
            pages,
            total => all.len(),
            features => features.into_iter().collect::<Vec<_>>(),
            audiences,
            feature,
            audience => audience.map(Audience::key),
        },
    ))
}

//! The public home page, the language switch, and each store's own page
//! on its own host ([`stores`]).
//!
//! The app itself was made with `rnx new bikeshop`; this module is the home
//! page it wrote, moved onto the bike shop's public layout. The catalogue
//! (#233) gives the home page its products later.

pub mod explain;
pub mod stores;

use renox::prelude::*;

/// The home page and the language menu's route.
pub struct Home;

impl Module for Home {
    fn name(&self) -> &'static str {
        "home"
    }

    // [explain:home.routes]
    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", index)
            .name("home")
            // A 304 without the body when the page didn't change (#233).
            .etag()
            .post("/locale/{locale}", locale)
            .name("locale.update")
            // Each store's own page, on its own host (#351).
            .merge(stores::routes())
    }
    // [/explain:home.routes]
}

/// The languages the shop is written in (`resources/lang/<locale>.json`).
pub const LOCALES: [(&str, &str); 2] = [("en", "English"), ("es", "Español")];

// [explain:home.routes]
async fn index() -> View {
    view("home/index.html", context! {})
}
// [/explain:home.routes]

/// Remembers the visitor's language (the layout's language menu) and goes
/// back to the page they were on. An unknown language is ignored.
async fn locale(session: Session, back: Back, Path(locale): Path<String>) -> Result<Back> {
    if LOCALES.iter().any(|(code, _)| *code == locale) {
        renox::i18n::remember_locale(&session, &locale)?;
    }
    Ok(back)
}

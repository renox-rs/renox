//! Each store's own little site, on a host of its own (#351):
//! `north.localhost:3000`, `south.localhost:3000`… in development (browsers
//! send every `*.localhost` name to this machine), `north.example.com` in
//! production with `BIKESHOP_STORE_DOMAIN=example.com`.
//!
//! `Routes::domain("{store}.<domain>", …)` serves these routes only on
//! hosts that match the pattern, and those hosts get only these routes
//! (plus Renox's own: the scripts, `/health`, `public/`). The handler reads
//! the store from the host with the `DomainParams` extractor, and the
//! domain's `fallback` sends any other path back to the store's page. The
//! page's layout (`layouts/store.html`) links to the shop with absolute
//! addresses, since the shop's pages aren't on this host.
//!
//! The home page's store tiles link here ([`site_url`]).

use renox::DomainParams;
use renox::prelude::*;
use serde::Serialize;

use crate::app::accounts::model::FullAddress;
use crate::app::rentals::model::{BikeStatus, RentalBike};
use crate::app::staff::model::Store;

/// The variable naming the domain under which each store has a host.
pub const DOMAIN_VAR: &str = "BIKESHOP_STORE_DOMAIN";

/// The route name of a store's page.
pub const ROUTE: &str = "stores.site";

/// The domain the stores' hosts are under: `BIKESHOP_STORE_DOMAIN`, else
/// `localhost`. Read from the environment (where `.env` puts it), not from
/// the app's `Config`: a module's routes are built before the configuration
/// is loaded.
pub fn domain() -> String {
    std::env::var(DOMAIN_VAR)
        .ok()
        .map(|d| d.trim().trim_matches('.').to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "localhost".to_owned())
}

/// The host pattern of the stores' pages: `{store}.localhost`.
pub fn pattern() -> String {
    format!("{{store}}.{}", domain())
}

/// Each store's brand colour (#36): its page uses it for buttons, links and
/// focus rings. [`themes`] builds a theme from each through the kit's
/// contrast guard.
pub const BRANDS: [(&str, &str); 3] = [
    ("north", "#1E3A5F"),
    ("south", "#9A3412"),
    ("west", "#6D28D9"),
];

/// The brand themes of the stores, keyed by slug, built once at start.
/// A colour that cannot reach readable contrast is skipped (that store keeps
/// the shop's look) rather than stopping the app.
pub fn themes() -> std::collections::HashMap<&'static str, renox::theme::Theme> {
    BRANDS
        .iter()
        .filter_map(|(slug, color)| {
            renox::theme::Theme::from_brand(&renox::theme::Brand::new(*color))
                .ok()
                .map(|theme| (*slug, theme))
        })
        .collect()
}

/// The theme of the store whose host this is (`north.localhost` gives North's
/// brand); the shop's own host gets none.
///
/// ```
/// use bikeshop::app::home::stores::{theme_for_host, themes};
/// let themes = themes();
/// assert_eq!(themes.len(), 3, "every store brand passes the contrast guard");
/// assert!(theme_for_host(&themes, "north.localhost").is_some());
/// assert!(theme_for_host(&themes, "localhost").is_none());
/// ```
pub fn theme_for_host(
    themes: &std::collections::HashMap<&'static str, renox::theme::Theme>,
    host: &str,
) -> Option<renox::theme::Theme> {
    let slug = host.strip_suffix(&format!(".{}", domain()))?;
    themes.get(slug).cloned()
}

/// The routes on a store's host.
// [explain:stores.routes]
pub fn routes() -> Routes {
    Routes::new().domain(
        &pattern(),
        Routes::new()
            .get("/", site)
            .name(ROUTE)
            // Any other path on a store's host goes to its page.
            .fallback(|| async { Redirect::to("/") }),
    )
}
// [/explain:stores.routes]

/// The address of a store's page: `APP_URL`'s scheme and port with the
/// store's host, e.g. `http://north.localhost:3000/` for
/// `APP_URL=http://127.0.0.1:3000`.
///
/// ```
/// let mut config = renox::Config::default();
/// config.url = "http://127.0.0.1:3000".into();
/// assert_eq!(
///     bikeshop::app::home::stores::site_url(&config, "north"),
///     "http://north.localhost:3000/"
/// );
/// ```
pub fn site_url(config: &renox::Config, slug: &str) -> String {
    let url = config.url.trim_end_matches('/');
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let authority = rest.split('/').next().unwrap_or_default();
    let port = authority
        .rsplit_once(':')
        .map(|(_, port)| port)
        .filter(|port| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    format!("{scheme}://{slug}.{}{port}/", domain())
}

/// What a store's page shows.
#[derive(Serialize)]
struct Site {
    store: Store,
    address: Option<String>,
    /// Rental bikes standing at the store and free to rent now.
    bikes_free: i64,
    /// Rental bikes standing at the store, whatever their state (retired
    /// ones aside).
    bikes_here: i64,
    /// Variants of the catalogue the store has on its shelves.
    products_in_stock: i64,
}

/// `GET /` on `{store}.<domain>`: the store's name, photo, address, hours
/// and what it has, with links back to the shop. An unknown store is a 404.
// [explain:stores.handler]
async fn site(State(state): State<AppState>, domain: DomainParams) -> Result<View> {
    let db = &state.db;
    let slug = domain.get("store").unwrap_or_default();
    let store = Store::query()
        .where_(Store::SLUG.eq(slug))
        .first(db)
        .await?
        .ok_or(Error::NotFound)?;
    let address = FullAddress::load(db, vec![store.address_id])
        .await?
        .remove(&store.address_id)
        .map(|a| a.line());
    let here = || {
        RentalBike::query()
            .where_(RentalBike::LOCATION_STORE_ID.eq(store.id))
            .where_(RentalBike::STATUS.ne(BikeStatus::Retired))
    };
    let bikes_here = here().count(db).await? as i64;
    // [/explain:stores.handler]
    let bikes_free = here()
        .where_(RentalBike::STATUS.eq(BikeStatus::Available))
        .count(db)
        .await? as i64;
    let products_in_stock = renox::db::sql(
        "SELECT COUNT(DISTINCT variant_id) FROM stock_levels \
         WHERE location_store_id = ? AND on_hand - reserved > 0",
    )
    .bind(store.id)
    .scalar::<i64>(db)
    .await?;
    let shop = state.config.url.trim_end_matches('/').to_owned();
    Ok(view(
        "home/store.html",
        context! {
            site => Site { store, address, bikes_free, bikes_here, products_in_stock },
            // The shop's pages, absolute: this host has only this page.
            shop_url => format!("{shop}/"),
            rent_url => format!("{shop}{}", state.url("rentals.create", &[])?),
            service_url => format!("{shop}{}", state.url("workshop.book", &[])?),
            catalog_url => format!("{shop}{}", state.url("catalog.index", &[])?),
            stores_url => format!("{shop}/#stores-title"),
        },
    ))
}

//! The JSON API (#241): store kiosks and the shop's mobile app.
//!
//! | Who | What | File |
//! |---|---|---|
//! | A store's kiosk | `/api/v1/kiosk/…`: free bikes, a reservation, check out, take back (with damage photos) | [`kiosk`] |
//! | The customer's app | `/api/v1/…`: catalogue and search, product stock per store, my rentals (reserve, cancel), orders, bikes, work orders, plans | [`customer`] |
//! | Customers | `/account/api-tokens`: personal tokens (abilities `read`, `rent`, `order`) | [`tokens`] |
//! | Managers | `/staff/api-tokens`: the store's kiosk tokens (`rentals:read`, `rentals:checkout`, `rentals:return`), revoked when a kiosk is lost | [`tokens`] |
//! | Readers | `/about/api`: every endpoint, its ability, request, answer and a `curl` example | [`endpoints`] |
//!
//! How a request goes: CORS for the app's origin ([`APP_ORIGIN`],
//! `Routes::cors`); the per-token rate limit ([`limit`], registered in
//! `src/lib.rs` as `bikeshop-api`, `Routes::throttle_by`: 429 over it);
//! `require_auth` for customers or `require_device` for kiosks (no valid
//! `Authorization: Bearer` token → 401, JSON); `require_ability` /
//! `require_device_ability` (a token without the endpoint's ability → 403); then
//! the handler, which for a kiosk checks its store. Bearer requests need no
//! CSRF token (Renox skips it for them). Validation errors are Renox's
//! `422 {"message", "errors"}`; lists are paginated with `links`; rentals
//! are found by their `Ulid` code, never by a counting id.
//!
//! Made with `rnx make:module api`, then the files by hand; the migration
//! with `rnx make:migration create_kiosks_table`.

pub mod customer;
pub mod endpoints;
pub mod explain;
pub mod kiosk;
pub mod tokens;

use renox::prelude::*;
use renox::rate_limit::{Limit, LimitRequest};

use crate::app::access::{self, catalogue};

/// The origin of the shop's mobile app (its web build), allowed by CORS.
pub const APP_ORIGIN: &str = "https://app.bikeshop.example";

/// The name of the API's rate limiter (`App::rate_limiter` in `src/lib.rs`).
pub const LIMITER: &str = "bikeshop-api";

/// Requests a minute per token.
pub const PER_MINUTE: u32 = 120;

/// What a kiosk token may be given, with its translation key.
pub const KIOSK_ABILITIES: [(&str, &str); 3] = [
    ("rentals:read", "api.abilities.rentals_read"),
    ("rentals:checkout", "api.abilities.rentals_checkout"),
    ("rentals:return", "api.abilities.rentals_return"),
];

/// What a personal token may be given, with its translation key.
pub const CUSTOMER_ABILITIES: [(&str, &str); 3] = [
    ("read", "api.abilities.read"),
    ("rent", "api.abilities.rent"),
    ("order", "api.abilities.order"),
];

// [explain:api.about.limit]
/// The rate limit of an API request: [`PER_MINUTE`] per token (the
/// token's id, the part before `|`), else per user, else 30 a minute per
/// IP address.
pub fn limit(req: &LimitRequest) -> Limit {
    if let Some(device) = req.device {
        return Limit::per_minute(PER_MINUTE).by(format!("device:{}", device.key()));
    }
    let token = req
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|t| t.split_once('|'))
        .map(|(id, _)| id.to_owned());
    match (token, req.user) {
        (Some(id), Some(_)) => Limit::per_minute(PER_MINUTE).by(format!("token:{id}")),
        (_, Some(user)) => Limit::per_minute(PER_MINUTE).by(format!("user:{}", user.id)),
        _ => Limit::per_minute(30),
    }
}
// [/explain:api.about.limit]

/// The base URL shown in the pages' `curl` examples.
pub fn base_url() -> String {
    renox::context::app()
        .map(|state| state.config.url.trim_end_matches('/').to_owned())
        .unwrap_or_default()
}

/// The api area, registered in `src/lib.rs`.
pub struct Api;

impl Module for Api {
    fn name(&self) -> &'static str {
        "api"
    }

    fn routes(&self) -> Routes {
        // [explain:api.about.routes]
        // Kiosks: each ability guards the routes added before it.
        let kiosk = Routes::new()
            .get("/api/v1/kiosk/bikes", kiosk::bikes)
            .name("api.kiosk.bikes")
            .get("/api/v1/kiosk/rentals/{code}", kiosk::rental)
            .name("api.kiosk.rental")
            .require_device_ability("rentals:read")
            // [/explain:api.about.routes]
            .merge(
                Routes::new()
                    .post("/api/v1/kiosk/rentals/{code}/checkout", kiosk::checkout)
                    .name("api.kiosk.checkout")
                    .require_device_ability("rentals:checkout"),
            )
            .merge(
                Routes::new()
                    .post("/api/v1/kiosk/rentals/{code}/return", kiosk::give_back)
                    .name("api.kiosk.return")
                    .require_device_ability("rentals:return"),
            );
        let customers = Routes::new()
            .get("/api/v1/me", customer::me)
            .name("api.me")
            .get("/api/v1/products", customer::products)
            .name("api.products")
            .get("/api/v1/products/{slug}", customer::product)
            .name("api.products.show")
            .get("/api/v1/me/bikes", customer::bikes)
            .name("api.me.bikes")
            .get("/api/v1/me/work-orders", customer::work_orders)
            .name("api.me.work_orders")
            .get("/api/v1/me/plan", customer::plans)
            .name("api.me.plan")
            .require_ability("read")
            .merge(
                Routes::new()
                    .get("/api/v1/me/rentals", customer::rentals)
                    .name("api.me.rentals")
                    .post("/api/v1/me/rentals", customer::reserve)
                    .name("api.me.rentals.store")
                    .delete("/api/v1/me/rentals/{code}", customer::cancel)
                    .name("api.me.rentals.destroy")
                    .require_ability("rent"),
            )
            .merge(
                Routes::new()
                    .get("/api/v1/me/orders", customer::orders)
                    .name("api.me.orders")
                    .require_ability("order"),
            );
        // [explain:api.about.routes]
        // Added last, so they run first: CORS answers preflights, the
        // limit counts every call, a missing or wrong token is a 401.
        let api = kiosk
            .merge(customers.require_auth())
            .throttle_by(LIMITER)
            .cors(&[APP_ORIGIN]);
        // [/explain:api.about.routes]

        let pages = Routes::new().get("/about/api", about).name("api.about");
        let personal = Routes::new()
            .get("/account/api-tokens", tokens::index)
            .name("api.tokens")
            .post("/account/api-tokens", tokens::store)
            .name("api.tokens.store")
            .post("/account/api-tokens/{token}/revoke", tokens::destroy)
            .name("api.tokens.destroy")
            .require_auth();
        let kiosks = access::staff_routes(
            Routes::new()
                .get("/staff/api-tokens", tokens::kiosks)
                .name("api.kiosks")
                .post("/staff/api-tokens", tokens::kiosk_store)
                .name("api.kiosks.store")
                .post("/staff/api-tokens/{kiosk}/revoke", tokens::kiosk_destroy)
                .name("api.kiosks.destroy")
                .require_permission(catalogue::FLEET_MANAGE),
        );
        api.merge(pages).merge(personal).merge(kiosks)
    }
}

/// `GET /about/api` (`api.about`): every endpoint, grouped by client.
pub async fn about() -> Result<View> {
    let kiosk: Vec<_> = endpoints::ENDPOINTS
        .iter()
        .filter(|e| e.client == "kiosk")
        .collect();
    let customer: Vec<_> = endpoints::ENDPOINTS
        .iter()
        .filter(|e| e.client == "customer")
        .collect();
    Ok(view(
        "api/about.html",
        context! {
            kiosk,
            customer,
            origin => APP_ORIGIN,
            per_minute => PER_MINUTE,
            base => base_url(),
        },
    ))
}

//! "About this page" entries for the api area's pages (see `crate::explain`).
//! The JSON endpoints aren't pages: each is listed in [`not_pages`], and
//! explained on `/about/api` ([`super::endpoints`]).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const MOD: &str = "examples/bikeshop/src/app/api/mod.rs";
const KIOSK: &str = "examples/bikeshop/src/app/api/kiosk.rs";
const CUSTOMER: &str = "examples/bikeshop/src/app/api/customer.rs";
const TOKENS: &str = "examples/bikeshop/src/app/api/tokens.rs";
const ENDPOINTS: &str = "examples/bikeshop/src/app/api/endpoints.rs";
const TESTS: &str = "examples/bikeshop/tests/api.rs";

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "api.about",
            path: "/about/api",
            title: "The JSON API",
            purpose: "Every endpoint of the shop's JSON API, for the kiosks next to the bike \
                      racks and for the shop's mobile app: what it is for, the token ability \
                      it needs, what to send, what comes back, and a `curl` example to try \
                      it. A test checks the list against the app's routes, as the walker \
                      checks pages.",
            who: "Developers of the kiosk and of the mobile app; anyone reading the example.",
            audience: &[Audience::Developer],
            flow: Flow::Learn,
            features: &[
                Feature {
                    api: "Authorization: Bearer (API tokens)",
                    why: "Programs don't log in with a password: they send a token made on a \
                          tokens page. Renox's auth middleware finds the user from it (only a \
                          SHA-256 hash is stored, so a leaked database gives no usable \
                          tokens), and Bearer requests skip CSRF, since a browser never sends \
                          that header on its own. No valid token on an `/api/v1` route \
                          (`require_auth`) is a JSON 401, not a redirect to a login page.",
                },
                Feature {
                    api: "Routes::require_ability",
                    why: "Each endpoint needs one ability (`rentals:checkout`, `rent`…), \
                          declared once on its group of routes: a token without it gets 403, \
                          so a kiosk's token can't read customers' orders and a lost one can \
                          only do what it was given. Kiosk calls then also check the token is \
                          a kiosk's and the rental is at its store.",
                },
                Feature {
                    api: "Valid<T> (JSON)",
                    why: "The API reads the website's own forms (`ReserveForm`, the counter's \
                          `PickupForm` and `ReturnForm`) from JSON, or multipart for damage \
                          photos, and a refusal is Renox's `422 {\"message\", \"errors\"}`: \
                          the same rules as the pages, written once, so the API can't accept \
                          what the website refuses.",
                },
                Feature {
                    api: "Routes::throttle_by",
                    why: "The `bikeshop-api` limiter (`App::rate_limiter` in `src/lib.rs`) \
                          counts per token (its id, before the `|`), not per IP address: \
                          several kiosks behind one store router don't share a limit. 120 a \
                          minute, then 429 with `Retry-After`; a call without a token gets 30 \
                          a minute per IP.",
                },
                Feature {
                    api: "Routes::cors",
                    why: "The app's web build on its own origin \
                          (`https://app.bikeshop.example`) may call the API from a browser: \
                          Renox answers the preflight and adds the `Access-Control-Allow-*` \
                          headers for that origin only, so no other site's scripts can use \
                          it.",
                },
                Feature {
                    api: "Ulid",
                    why: "Rentals are found by their reservation code, a `Ulid`: it can't be \
                          guessed or counted the way 1, 2, 3 can.",
                },
                Feature {
                    api: "Query::paginate",
                    why: "Lists come a page at a time (`?page=`), with `meta` (page, per \
                          page, total, last page) and `links` (first, last, prev, next) made \
                          from the paginator: a phone never downloads a customer's whole \
                          history, and the app needs no paging logic of its own.",
                },
            ],
            under_hood: "No query: the page renders the endpoint list in \
                         `src/app/api/endpoints.rs` (`ENDPOINTS`), which a test compares with the \
                         app's `/api/v1` routes. The API itself shares the website's \
                         functions: `booking::book` and `reserve::cancel_rental` for \
                         customers, `counter::hand_over` and `counter::take_back` for \
                         kiosks.",
            docs: &[
                "docs/authorization.md#api-tokens-and-abilities",
                "docs/routing.md#rate-limits",
                "docs/routing.md#cors",
                "docs/validation.md#answer-1-htmx-and-json-requests-get-a-422",
            ],
            sources: &[
                MOD,
                ENDPOINTS,
                KIOSK,
                CUSTOMER,
                "examples/bikeshop/resources/views/api/about.html",
                "examples/bikeshop/resources/views/components/api_endpoint.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Routes: an ability per group; then auth, the limit and CORS over all",
                    region: "api.about.routes",
                },
                Code {
                    title: "Limiter: a budget per token, else per user, else per address",
                    region: "api.about.limit",
                },
                Code {
                    title: "Template: one card per endpoint, its `curl` ready to paste",
                    region: "api.about.template",
                },
            ],
        },
        Explanation {
            route: "api.tokens",
            path: "/account/api-tokens",
            title: "My API tokens",
            purpose: "A customer's personal tokens for the shop's mobile app (or their own \
                      scripts): make one with the abilities it needs (read, rent, order), \
                      see it once to copy it, see when each was last used, revoke one.",
            who: "Logged-in customers.",
            audience: &[Audience::Customer],
            flow: Flow::Account,
            features: &[
                Feature {
                    api: "User::create_token_with",
                    why: "A token limited to the abilities ticked (`read`, `rent`, `order`) \
                          and expiring in a year, so a script that only reads can't book; \
                          `tokens()` lists them with their last use and `revoke_token` ends \
                          one at once (the app's next call gets 401).",
                },
                Feature {
                    api: "Session flash",
                    why: "Only the token's hash is stored, so the secret is shown once: \
                          `session.flash` carries it to the page after the redirect, and a \
                          reload no longer shows it. Nothing secret stays in the database or \
                          the URL.",
                },
                Feature {
                    api: "UI kit: checkbox_list + input (copyable) + table",
                    why: "The abilities as a checkbox list, the new token in a read-only \
                          field with the kit's copy button (with a `curl` line to try it), the \
                          tokens in a table with a confirm before revoking.",
                },
                Feature {
                    api: "Redirect::route",
                    why: "Creating and revoking a token go back with \
                          `Redirect::route(\"api.tokens\", &[])`, by the route's name: the same \
                          name the templates link with, so the address is written once, in the \
                          module's routes.",
                },
            ],
            under_hood: "One query for the tokens. Making one: `Valid<TokenForm>` (each \
                         ability `one_of` the three), then a `personal_access_tokens` row (its \
                         SHA-256 hash, the abilities as JSON, the expiry). Revoking deletes the \
                         row, only among the user's own tokens (404 otherwise).",
            docs: &[
                "docs/authorization.md#api-tokens-and-abilities",
                "docs/routing.md#sessions",
            ],
            sources: &[
                TOKENS,
                MOD,
                "examples/bikeshop/resources/views/api/tokens.html",
                "examples/bikeshop/resources/views/components/api_token_form.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: `create_token_with` and its abilities, shown once",
                    region: "api.tokens.store",
                },
                Code {
                    title: "Form: each ability one of those a customer may have",
                    region: "api.tokens.rules",
                },
                Code {
                    title: "Template: the form, one tick per ability a customer may have",
                    region: "api.tokens.template",
                },
            ],
        },
        Explanation {
            route: "api.kiosks",
            path: "/staff/api-tokens",
            title: "Kiosk tokens",
            purpose: "The active store's self-service kiosks: a manager makes a kiosk's \
                      token with the rental abilities it needs (see bikes, check out, take \
                      back), copies it into the kiosk once, and revokes it when a kiosk is \
                      lost or replaced.",
            who: "Store managers (the permission `fleet.manage` in the store).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "access::staff_routes + require_permission",
                    why: "The page works in the active store and needs `fleet.manage` there; \
                          a kiosk is made for that store, and revoking one checks \
                          `access::can_in` on the kiosk's own store (404 elsewhere), so a \
                          manager of one store can't switch off another store's kiosks.",
                },
                Feature {
                    api: "User::create_token_with",
                    why: "Each kiosk acts as its own user (a random password nobody is \
                          told), not as the manager who made it, with one token limited to \
                          `rentals:read`, `rentals:checkout` and `rentals:return` as ticked and \
                          no expiry (the manager revokes it); the API then only shows that \
                          store's reservations.",
                },
                Feature {
                    api: "Session flash",
                    why: "The token is shown once, right after it is made (only its hash is \
                          stored), to be typed or pasted into the kiosk.",
                },
            ],
            under_hood: "Three queries: the store, its kiosks and their tokens' last use. \
                         Making a kiosk: `Valid<KioskForm>`, then its user is registered, the \
                         token made and the `kiosks` row written. Revoking deletes the kiosk \
                         user's tokens (`revoke_tokens`) and marks the row revoked, kept for \
                         the history.",
            docs: &[
                "docs/authorization.md#api-tokens-and-abilities",
                "docs/authorization.md#checking-one-record-has_permission_in",
            ],
            sources: &[
                TOKENS,
                KIOSK,
                MOD,
                "examples/bikeshop/src/app/access/active_store.rs",
                "examples/bikeshop/resources/views/api/kiosks.html",
                "examples/bikeshop/resources/views/components/api_token_form.html",
                "examples/bikeshop/migrations/20260103001100_create_kiosks_table.up.sql",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: a kiosk is a user of its own with one token",
                    region: "api.kiosks.store",
                },
                Code {
                    title: "Test: a revoked token is a 401 at once; a cashier gets a 403",
                    region: "api.kiosks.test",
                },
                Code {
                    title: "Template: the same form as the customers' token page",
                    region: "api.tokens.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    const JSON: &str = "a JSON API endpoint, explained on /about/api";
    [
        "api.kiosk.bikes",
        "api.kiosk.rental",
        "api.me",
        "api.products",
        "api.products.show",
        "api.me.bikes",
        "api.me.work_orders",
        "api.me.plan",
        "api.me.rentals",
        "api.me.orders",
    ]
    .into_iter()
    .map(|route| NotAPage {
        route,
        reason: JSON,
    })
    .collect()
}

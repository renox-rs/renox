use std::collections::HashMap;
use std::fmt::{Display, Write};

use std::convert::Infallible;

use anyhow::{anyhow, bail};
use axum::Router;
use axum::extract::Request;
use axum::handler::Handler;
use axum::middleware::from_fn;
use axum::response::IntoResponse;
use axum::routing::{self, MethodRouter, Route};
use tower::{Layer, Service};

use crate::AppState;

/// A module's routes, with optional names for URL generation.
///
/// ```
/// # use renox::prelude::*;
/// # async fn index() {}
/// # async fn show() {}
/// # async fn store() {}
/// # let _ =
/// Routes::new()
///     .get("/products", index).name("products.index")
///     .get("/products/{id}", show).name("products.show")
///     .post("/products", store).name("products.store")
/// # ;
/// ```
#[derive(Default)]
pub struct Routes {
    router: Router<AppState>,
    names: Vec<(String, String)>,
    last_path: Option<String>,
    listing: Vec<RouteInfo>,
    fallback: Option<MethodRouter<AppState>>,
    domains: Vec<(String, Routes)>,
}

/// What a module's `Routes` hold, for `App::boot`.
pub(crate) struct RouteParts {
    pub router: Router<AppState>,
    pub names: Vec<(String, String)>,
    pub listing: Vec<RouteInfo>,
    pub fallback: Option<MethodRouter<AppState>>,
    pub domains: Vec<(String, Routes)>,
}

/// One route as `route:list` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RouteInfo {
    /// `GET`, `POST`, … or `*` for a `route()` whose methods Renox can't see.
    pub method: String,
    /// The path pattern, e.g. `/posts/{id}`.
    pub path: String,
    /// The route's name, if it has one.
    pub name: Option<String>,
    /// The module that defined it, or `renox` for the framework's own.
    pub module: String,
    /// Guards and limits, e.g. `auth`, `throttle:60/60s`.
    pub middleware: Vec<String>,
    /// The host pattern of a `Routes::domain` route, e.g. `admin.example.com`.
    pub domain: Option<String>,
}

macro_rules! method {
    ($($method:ident),*) => {$(
        /// Adds a route for this HTTP method (the function's name) at `path`.
        pub fn $method<H, T>(self, path: &str, handler: H) -> Self
        where
            H: Handler<T, AppState>,
            T: 'static,
        {
            self.add(path, routing::$method(handler), &stringify!($method).to_uppercase())
        }
    )*};
}

impl Routes {
    /// An empty set of routes.
    pub fn new() -> Self {
        Self::default()
    }

    method!(get, post, put, patch, delete);

    /// The routes of a resource, like Laravel's `Route::resource`: only the
    /// actions given, under `path`, named `{name}.{action}`.
    ///
    /// | Action | Method and path | Name |
    /// |---|---|---|
    /// | `index` | `GET /products` | `products.index` |
    /// | `create` | `GET /products/new` | `products.create` |
    /// | `store` | `POST /products` | `products.store` |
    /// | `show` | `GET /products/{id}` | `products.show` |
    /// | `edit` | `GET /products/{id}/edit` | `products.edit` |
    /// | `update` | `PUT` and `PATCH /products/{id}` | `products.update` |
    /// | `destroy` | `DELETE /products/{id}` | `products.destroy` |
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::Resource;
    /// # async fn index() -> &'static str { "" }
    /// # async fn show(Path(id): Path<i64>) -> String { id.to_string() }
    /// # async fn destroy(Path(id): Path<i64>) -> String { id.to_string() }
    /// # let _: Routes =
    /// Routes::new()
    ///     .resource("/products", "products", Resource::new().index(index).show(show).destroy(destroy))
    ///     .require_auth()
    /// # ;
    /// ```
    pub fn resource(mut self, path: &str, name: &str, resource: Resource) -> Self {
        assert!(
            path.starts_with('/') && !path.ends_with('/'),
            "Routes::resource(\"{path}\", …): the path must start with `/` and not end with one"
        );
        let member = format!("{path}/{{id}}");
        let actions = [
            ("index", "GET", path.to_owned(), resource.index),
            ("create", "GET", format!("{path}/new"), resource.create),
            ("store", "POST", path.to_owned(), resource.store),
            ("show", "GET", member.clone(), resource.show),
            ("edit", "GET", format!("{member}/edit"), resource.edit),
            ("update", "PUT", member.clone(), resource.update),
            ("destroy", "DELETE", member, resource.destroy),
        ];
        for (action, method, at, router) in actions {
            if let Some(router) = router {
                let method = if action == "update" {
                    "PUT|PATCH"
                } else {
                    method
                };
                self = self
                    .add(&at, router, method)
                    .name(&format!("{name}.{action}"));
            }
        }
        self
    }

    /// Adds a route with any axum method router, e.g. `get(show).post(update)`.
    pub fn route(self, path: &str, method_router: MethodRouter<AppState>) -> Self {
        self.add(path, method_router, "*")
    }

    /// A page that needs no handler: `GET path` renders `template` with the
    /// usual globals (Laravel's `Route::view`), e.g. an "About" page.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _: Routes =
    /// Routes::new().view("/about", "pages/about.html").name("about")
    /// # ;
    /// ```
    pub fn view(self, path: &str, template: &str) -> Self {
        let template = template.to_owned();
        self.add(
            path,
            axum::routing::get(move || {
                let template = template.clone();
                async move { crate::view(&template, minijinja::context! {}) }
            }),
            "GET",
        )
    }

    /// Sends `path` to `to` with a 302 (Laravel's `Route::redirect`), e.g.
    /// an old address.
    pub fn redirect(self, path: &str, to: &str) -> Self {
        let to = to.to_owned();
        self.add(
            path,
            axum::routing::any(move || {
                let to = to.clone();
                async move { redirect_with(axum::http::StatusCode::FOUND, &to) }
            }),
            "*",
        )
    }

    /// Like [`redirect`](Self::redirect), with a 301: the move is permanent.
    pub fn permanent_redirect(self, path: &str, to: &str) -> Self {
        let to = to.to_owned();
        self.add(
            path,
            axum::routing::any(move || {
                let to = to.clone();
                async move { redirect_with(axum::http::StatusCode::MOVED_PERMANENTLY, &to) }
            }),
            "*",
        )
    }

    fn add(mut self, path: &str, method_router: MethodRouter<AppState>, method: &str) -> Self {
        self.router = self.router.route(path, method_router);
        self.last_path = Some(path.to_owned());
        self.listing.push(RouteInfo {
            method: method.to_owned(),
            path: path.to_owned(),
            name: None,
            module: String::new(),
            middleware: Vec::new(),
            domain: None,
        });
        self
    }

    /// Notes a layer on every route added so far, for `route:list`.
    fn mark(mut self, middleware: &str) -> Self {
        for route in &mut self.listing {
            route.middleware.push(middleware.to_owned());
        }
        self
    }

    /// Names the route added just before, for use with `url()` and `route()` in templates.
    ///
    /// # Panics
    ///
    /// If no route has been added yet.
    pub fn name(mut self, name: &str) -> Self {
        let path = self
            .last_path
            .clone()
            .expect("Routes::name() must follow a route");
        self.names.push((name.to_owned(), path));
        // A name belongs to a path, so it also covers the methods added just
        // before for the same path (`.get(p, a).post(p, b).name(n)`) that
        // don't have a name of their own.
        let path = self.last_path.clone();
        for route in self.listing.iter_mut().rev() {
            if Some(&route.path) != path.as_ref() {
                break;
            }
            if route.name.is_none() {
                route.name = Some(name.to_owned());
            }
        }
        self
    }

    /// Only logged-in users may use the routes added so far; guests are sent
    /// to the `login` route. Call it after adding the routes it should cover.
    pub fn require_auth(self) -> Self {
        self.route_layer(from_fn(crate::auth::require_auth))
            .mark("auth")
    }

    /// Like `require_auth`, and the user must have verified their email;
    /// others are sent to the `verification.notice` route.
    pub fn require_verified(self) -> Self {
        self.route_layer(from_fn(crate::auth::require_verified))
            .mark("verified")
    }

    /// Only users the gate `name` lets through (`App::gate`, `gate_async` or
    /// a permission of that name, after `gate_before`) may use the routes
    /// added so far; guests are sent to log in, others get 403.
    pub fn require_gate(self, name: &str) -> Self {
        self.requirement(
            crate::auth::Requirement::Gate(name.to_owned()),
            "gate",
            name,
        )
    }

    /// Only users with `role` (the `Permissions` module) may use the routes
    /// added so far; guests are sent to log in, others get 403.
    pub fn require_role(self, role: &str) -> Self {
        self.requirement(
            crate::auth::Requirement::Role(role.to_owned()),
            "role",
            role,
        )
    }

    /// Only users granted `permission` (the `Permissions` module, after
    /// `gate_before`) may use the routes added so far.
    pub fn require_permission(self, permission: &str) -> Self {
        self.requirement(
            crate::auth::Requirement::Permission(permission.to_owned()),
            "permission",
            permission,
        )
    }

    /// Requests with an API token must have `ability`
    /// (`User::create_token_with`); sessions and unrestricted tokens pass.
    pub fn require_ability(self, ability: &str) -> Self {
        self.requirement(
            crate::auth::Requirement::Ability(ability.to_owned()),
            "ability",
            ability,
        )
    }

    /// Only requests that authenticated as a device (`Authorization: Bearer`
    /// with a `DeviceToken`) may use the routes added so far; others get 401
    /// as JSON. Users and guests are not devices.
    pub fn require_device(self) -> Self {
        self.device_guard(None, "device")
    }

    /// Like `require_device`, and the device's token must have `ability`
    /// (403 otherwise).
    pub fn require_device_ability(self, ability: &str) -> Self {
        self.device_guard(Some(ability), &format!("device:{ability}"))
    }

    fn device_guard(self, ability: Option<&str>, mark: &str) -> Self {
        let ability = ability.map(|a| std::sync::Arc::new(a.to_owned()));
        self.route_layer(from_fn(
            move |req: Request, next: axum::middleware::Next| {
                crate::auth::device_guard(ability.clone(), req, next)
            },
        ))
        .mark(mark)
    }

    fn requirement(self, requirement: crate::auth::Requirement, kind: &str, name: &str) -> Self {
        let requirement = std::sync::Arc::new(requirement);
        self.route_layer(from_fn(
            move |req: Request, next: axum::middleware::Next| {
                crate::auth::require(requirement.clone(), req, next)
            },
        ))
        .mark(&format!("{kind}:{name}"))
    }

    /// Users who haven't typed their password in the last three hours are
    /// asked for it (`/confirm-password`, from the `Auth` module) before the
    /// routes added so far, e.g. billing settings. Also marks `auth`.
    pub fn require_password_confirmed(self) -> Self {
        self.route_layer(from_fn(crate::auth::require_password_confirmed))
            .route_layer(from_fn(crate::auth::require_auth))
            .mark("password.confirm")
    }

    /// Only guests may use the routes added so far; logged-in users are sent
    /// to the `home` route (e.g. for login and registration pages).
    pub fn guest_only(self) -> Self {
        self.route_layer(from_fn(crate::auth::guest_only))
            .mark("guest")
    }

    /// Limits the routes added so far to `max` requests per `per`, counted per
    /// logged-in user or per IP address. Over the limit: 429 with `Retry-After`.
    pub fn throttle(self, max: u32, per: std::time::Duration) -> Self {
        let covered: Vec<String> = self
            .listing
            .iter()
            .map(|route| format!("{} {}", route.method, route.path))
            .collect();
        let id =
            crate::webhook::sha256_hex(format!("{}|{max}|{}", covered.join(","), per.as_secs()));
        let limiter = std::sync::Arc::new(crate::rate_limit::Limiter::new(
            id[..16].to_owned(),
            max,
            per,
        ));
        self.route_layer(from_fn(
            move |req: Request, next: axum::middleware::Next| {
                let limiter = limiter.clone();
                async move { crate::rate_limit::check(&limiter, req, next).await }
            },
        ))
        .mark(&format!("throttle:{max}/{}s", per.as_secs()))
    }

    /// Limits the routes added so far with the named limiter `name`
    /// (`App::rate_limiter`), whose rule picks the limit per request.
    pub fn throttle_by(self, name: &str) -> Self {
        let owned = name.to_owned();
        self.route_layer(from_fn(
            move |req: Request, next: axum::middleware::Next| {
                let name = owned.clone();
                async move { crate::rate_limit::check_named(&name, req, next).await }
            },
        ))
        .mark(&format!("throttle:{name}"))
    }

    /// Receives `W`'s webhooks at `path` (POST): verifies, stores once per
    /// event and processes them in the queue. See `renox::webhook`. The
    /// route is named `webhooks.<provider>`, skips CSRF and keeps working in
    /// maintenance mode.
    pub fn webhook<W: crate::webhook::Webhook>(self, path: &str) -> Self {
        let mut routes = self
            .post(path, crate::webhook::receive::<W>)
            .name(&format!("webhooks.{}", W::PROVIDER));
        if let Some(route) = routes.listing.last_mut() {
            route.middleware.push("no-csrf".into());
            route.middleware.push(format!("webhook:{}", W::PROVIDER));
        }
        routes
    }

    /// Gives the routes added so far an `ETag` (a hash of the page as sent),
    /// and answers `304 Not Modified` without the body when the browser
    /// already has that version (`If-None-Match`). For pages fetched again
    /// and again that rarely change, e.g. a catalogue or an API list.
    /// Only `GET`/`HEAD` answers with status 200 and a body of at most
    /// 2 MB get one; streamed bodies don't.
    pub fn etag(self) -> Self {
        self.route_layer(from_fn(
            |req: Request, next: axum::middleware::Next| async move {
                let mut res = next.run(req).await;
                res.extensions_mut().insert(crate::security::WantsEtag);
                res
            },
        ))
        .mark("etag")
    }

    /// Lets the routes added so far be posted to without a CSRF token, for
    /// callers that have no session, such as a payment gateway's webhook.
    /// Such a handler must check the request itself (e.g. its signature).
    pub fn without_csrf(self) -> Self {
        self.mark("no-csrf")
    }

    /// Lets browsers on `origins` (e.g. `https://app.example.com`, or `*`
    /// for any) call the routes added so far with `fetch`: answers CORS
    /// preflights and adds the `Access-Control-Allow-*` headers. Allows the
    /// usual methods and the `Content-Type`, `Authorization`, `Accept` and
    /// `X-CSRF-Token` headers. Use `cors_layer` for anything else.
    pub fn cors(self, origins: &[&str]) -> Self {
        use axum::http::{HeaderName, HeaderValue, Method, header};
        use tower_http::cors::{AllowOrigin, CorsLayer};

        let origin = if origins.contains(&"*") {
            AllowOrigin::any()
        } else {
            AllowOrigin::list(
                origins
                    .iter()
                    .filter_map(|o| HeaderValue::from_str(o.trim_end_matches('/')).ok()),
            )
        };
        let layer = CorsLayer::new()
            .allow_origin(origin)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                header::ACCEPT,
                HeaderName::from_static(crate::CSRF_HEADER),
            ])
            .max_age(std::time::Duration::from_secs(3600));
        self.cors_layer(layer)
    }

    /// Like `cors`, with a `tower_http::cors::CorsLayer` built by hand
    /// (`renox::cors::CorsLayer`), e.g. to allow credentials.
    pub fn cors_layer(mut self, layer: tower_http::cors::CorsLayer) -> Self {
        // `layer`, not `route_layer`: preflights use OPTIONS, which the
        // routes themselves don't handle.
        self.router = self.router.layer(layer);
        self.mark("cors")
    }

    /// Wraps the routes added so far in a tower layer (axum's `route_layer`).
    pub fn route_layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.router = self.router.route_layer(layer);
        self
    }

    /// Adds the routes of `other`, with their names and layers.
    pub fn merge(mut self, other: impl Into<Routes>) -> Self {
        let other = other.into();
        self.router = self.router.merge(other.router);
        self.names.extend(other.names);
        self.listing.extend(other.listing);
        self.domains.extend(other.domains);
        assert!(
            self.fallback.is_none() || other.fallback.is_none(),
            "Routes::merge: both sides have a fallback"
        );
        self.fallback = self.fallback.or(other.fallback);
        self.last_path = None;
        self
    }

    /// What answers a request no route and no public file matches: a page
    /// of your own instead of the 404 page (Laravel's `Route::fallback`).
    /// One per app, or per domain inside [`Routes::domain`].
    ///
    /// ```
    /// # use renox::prelude::*;
    /// async fn missing(uri: axum::http::Uri) -> (StatusCode, String) {
    ///     (StatusCode::NOT_FOUND, format!("Nothing at {}. Try the search.", uri.path()))
    /// }
    /// # let _: Routes =
    /// Routes::new().fallback(missing)
    /// # ;
    /// ```
    pub fn fallback<H, T>(mut self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        assert!(
            self.fallback.is_none(),
            "Routes::fallback: these routes have a fallback already"
        );
        self.fallback = Some(routing::any(handler));
        self
    }

    /// Routes served only on hosts matching `pattern`, e.g.
    /// `admin.example.com`, or `{account}.example.com` with the account in
    /// [`DomainParams`](crate::DomainParams) (Laravel's `Route::domain`).
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # async fn dashboard() {}
    /// # async fn home() {}
    /// # let _: Routes =
    /// Routes::new()
    ///     .get("/", home) // other hosts
    ///     .domain(
    ///         "admin.example.com",
    ///         Routes::new().get("/", dashboard).name("admin.dashboard").require_auth(),
    ///     )
    /// # ;
    /// ```
    ///
    /// A host that matches a domain gets that domain's routes only (all
    /// modules' routes for that pattern), plus Renox's own (`/health`, the
    /// scripts, public files); every other host gets the routes without a
    /// domain. So the same path can mean different pages on different hosts.
    /// `route()` gives the path, as for other routes. Matching uses the
    /// `Host` header: behind a proxy, keep it (Caddy and nginx's
    /// `proxy_set_header Host $host` do).
    pub fn domain(mut self, pattern: &str, routes: impl Into<Routes>) -> Self {
        self.domains.push((pattern.to_owned(), routes.into()));
        self.last_path = None;
        self
    }

    /// Adds `routes` under a path prefix and a name prefix, like Laravel's
    /// `Route::prefix('admin')->name('admin.')->group(…)`. Guards added to
    /// `routes` cover only them; guards added after the group cover
    /// everything added so far, as usual.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # async fn dashboard() {}
    /// # async fn users() {}
    /// # let _ =
    /// Routes::new().group(
    ///     "/admin",
    ///     "admin.",
    ///     Routes::new()
    ///         .get("/", dashboard).name("dashboard") // GET /admin, `admin.dashboard`
    ///         .get("/users", users).name("users")    // GET /admin/users, `admin.users`
    ///         .require_auth(),
    /// )
    /// # ;
    /// ```
    ///
    /// # Panics
    ///
    /// If `path` doesn't start with `/` or ends with `/`.
    pub fn group(mut self, path: &str, name: &str, routes: impl Into<Routes>) -> Self {
        assert!(
            path.starts_with('/') && !path.ends_with('/'),
            "Routes::group(\"{path}\", …): the prefix must start with `/` and not end with one"
        );
        let routes = routes.into();
        assert!(
            routes.domains.is_empty() && routes.fallback.is_none(),
            "Routes::group(\"{path}\", …): put `domain` and `fallback` outside path groups"
        );
        let join = |inner: &str| {
            if inner == "/" {
                path.to_owned()
            } else {
                format!("{path}{inner}")
            }
        };
        self.router = self.router.nest(path, routes.router);
        self.names.extend(
            routes
                .names
                .into_iter()
                .map(|(route_name, route_path)| (format!("{name}{route_name}"), join(&route_path))),
        );
        self.listing
            .extend(routes.listing.into_iter().map(|info| RouteInfo {
                path: join(&info.path),
                name: info.name.map(|route_name| format!("{name}{route_name}")),
                ..info
            }));
        self.last_path = None;
        self
    }

    pub(crate) fn into_parts(self) -> RouteParts {
        RouteParts {
            router: self.router,
            names: self.names,
            listing: self.listing,
            fallback: self.fallback,
            domains: self.domains,
        }
    }
}

impl From<Router<AppState>> for Routes {
    fn from(router: Router<AppState>) -> Self {
        Self {
            router,
            ..Self::default()
        }
    }
}

/// Every named route in the application.
#[derive(Debug, Default)]
pub struct RouteTable {
    paths: HashMap<String, String>,
    /// The `Routes::domain` pattern of names defined inside one.
    domains: HashMap<String, String>,
    /// The methods of each named route, as listed: `GET`, `PUT|PATCH`, `*`.
    methods: HashMap<String, String>,
}

impl RouteTable {
    pub(crate) fn insert(&mut self, name: String, path: String) -> anyhow::Result<()> {
        if let Some(existing) = self.paths.get(&name) {
            bail!("route name `{name}` is used for both `{existing}` and `{path}`");
        }
        self.paths.insert(name, path);
        Ok(())
    }

    /// The path pattern of a named route, e.g. `/products/{id}`.
    pub fn path(&self, name: &str) -> Option<&str> {
        self.paths.get(name).map(String::as_str)
    }

    /// Every route name, in no particular order.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.paths.keys().map(String::as_str)
    }

    /// Records the methods `name` answers (`GET`, `PUT|PATCH`, `*` for any).
    pub(crate) fn set_method(&mut self, name: &str, method: &str) {
        self.methods.insert(name.to_owned(), method.to_owned());
    }

    /// Records that `name` belongs to the routes of `domain`.
    pub(crate) fn set_domain(&mut self, name: &str, domain: &str) {
        self.domains.insert(name.to_owned(), domain.to_owned());
    }

    /// The name of the route `method` reached at the path pattern `path`
    /// (axum's `MatchedPath`, e.g. `/products/{id}`) on `domain` (`None` for
    /// routes without one): several routes can share a path, one per method, such as
    /// `GET /account` (`account.show`) and `DELETE /account`
    /// (`account.destroy`). `HEAD` counts as `GET`. Without `method`, or when
    /// no route names that method, the first name in order.
    pub(crate) fn name_of(
        &self,
        path: &str,
        domain: Option<&str>,
        method: Option<&str>,
    ) -> Option<&str> {
        let on_path = || {
            self.paths
                .iter()
                .filter(move |(name, p)| {
                    p.as_str() == path && self.domains.get(*name).map(String::as_str) == domain
                })
                .map(|(name, _)| name.as_str())
        };
        let wanted = method.map(|m| if m == "HEAD" { "GET" } else { m });
        let by_method = wanted.and_then(|wanted| {
            on_path()
                .filter(|name| {
                    self.methods.get(*name).is_some_and(|listed| {
                        listed == "*" || listed.split('|').any(|m| m == wanted)
                    })
                })
                .min()
        });
        by_method.or_else(|| on_path().min())
    }

    /// Builds the URL path of a named route, filling its parameters in order.
    pub fn url(&self, name: &str, params: &[&dyn Display]) -> anyhow::Result<String> {
        let pattern = self
            .path(name)
            .ok_or_else(|| anyhow!("route `{name}` is not defined"))?;
        fill(pattern, params).map_err(|err| anyhow!("route `{name}`: {err}"))
    }
}

fn fill(pattern: &str, params: &[&dyn Display]) -> anyhow::Result<String> {
    let mut out = String::with_capacity(pattern.len());
    let mut params = params.iter();
    let mut rest = pattern;

    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..]
            .find('}')
            .ok_or_else(|| anyhow!("unclosed `{{` in `{pattern}`"))?
            + start;
        let placeholder = &rest[start + 1..end];
        let value = params
            .next()
            .ok_or_else(|| anyhow!("missing value for `{{{placeholder}}}`"))?
            .to_string();
        encode(&mut out, &value, placeholder.starts_with('*'));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);

    if params.next().is_some() {
        bail!("too many parameters for `{pattern}`");
    }
    Ok(out)
}

pub(crate) fn encode(out: &mut String, value: &str, keep_slashes: bool) {
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b'/' if keep_slashes => out.push('/'),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> RouteTable {
        let mut t = RouteTable::default();
        t.insert("home".into(), "/".into()).unwrap();
        t.insert("products.show".into(), "/products/{id}".into())
            .unwrap();
        t.insert("docs".into(), "/docs/{*path}".into()).unwrap();
        t
    }

    #[test]
    fn builds_urls() {
        let t = table();
        assert_eq!(t.url("home", &[]).unwrap(), "/");
        assert_eq!(t.url("products.show", &[&42]).unwrap(), "/products/42");
        assert_eq!(
            t.url("products.show", &[&"a b/c"]).unwrap(),
            "/products/a%20b%2Fc"
        );
        assert_eq!(
            t.url("docs", &[&"guide/intro"]).unwrap(),
            "/docs/guide/intro"
        );
    }

    #[test]
    fn rejects_bad_calls() {
        let t = table();
        assert!(t.url("missing", &[]).is_err());
        assert!(t.url("products.show", &[]).is_err());
        assert!(t.url("home", &[&1]).is_err());
    }

    #[test]
    fn rejects_duplicate_names() {
        let mut t = table();
        assert!(t.insert("home".into(), "/home".into()).is_err());
    }
}

/// The handlers of a resource, for [`Routes::resource`]; leave out the
/// actions it doesn't have.
#[derive(Default)]
#[must_use = "a resource does nothing until given to Routes::resource"]
pub struct Resource {
    index: Option<MethodRouter<AppState>>,
    create: Option<MethodRouter<AppState>>,
    store: Option<MethodRouter<AppState>>,
    show: Option<MethodRouter<AppState>>,
    edit: Option<MethodRouter<AppState>>,
    update: Option<MethodRouter<AppState>>,
    destroy: Option<MethodRouter<AppState>>,
}

macro_rules! resource_action {
    ($($action:ident => $router:expr),* $(,)?) => {$(
        /// Sets the handler of this action (the function's name).
        pub fn $action<H, T>(mut self, handler: H) -> Self
        where
            H: Handler<T, AppState>,
            T: 'static,
        {
            let make: fn(H) -> MethodRouter<AppState> = $router;
            self.$action = Some(make(handler));
            self
        }
    )*};
}

impl Resource {
    /// A resource with no actions; add them with `index`, `store`, …
    pub fn new() -> Self {
        Self::default()
    }

    resource_action!(
        index => routing::get,
        create => routing::get,
        store => routing::post,
        show => routing::get,
        edit => routing::get,
        update => |h| routing::put(h.clone()).patch(h),
        destroy => routing::delete,
    );
}

/// Whether a route name matches a pattern where `*` stands for anything,
/// e.g. `admin.*` or `products.*`, like Laravel's `routeIs`.
pub(crate) fn route_name_matches(name: &str, pattern: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty(); // no `*`: the whole name
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// The route that answers this request, from a handler or middleware:
/// its name (`products.show`) and path pattern (`/products/{id}`).
///
/// ```
/// # use renox::prelude::*;
/// use renox::CurrentRoute;
///
/// async fn menu(route: CurrentRoute) -> String {
///     if route.is("admin.*") { "admin".into() } else { route.name().unwrap_or("?").into() }
/// }
/// ```
///
/// In templates: `{% if route_is('products.*') %}` and `request.route`.
#[derive(Debug, Clone)]
pub struct CurrentRoute {
    name: Option<String>,
    path: Option<String>,
}

impl CurrentRoute {
    /// The route's name, if it has one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The path pattern the request matched, e.g. `/products/{id}`.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Whether the route's name matches any of `patterns` (`*` stands for
    /// anything): `route.is("admin.*")`.
    pub fn is(&self, pattern: &str) -> bool {
        self.name
            .as_deref()
            .is_some_and(|name| route_name_matches(name, pattern))
    }

    pub(crate) fn of(
        method: &axum::http::Method,
        extensions: &axum::http::Extensions,
        state: &AppState,
    ) -> Self {
        let path = extensions
            .get::<axum::extract::MatchedPath>()
            .map(|p| p.as_str().to_owned());
        let domain = extensions
            .get::<crate::domain::MatchedDomain>()
            .map(|d| &*d.0);
        let name = path
            .as_deref()
            .and_then(|p| state.routes.name_of(p, domain, Some(method.as_str())))
            .map(str::to_owned);
        Self { name, path }
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for CurrentRoute {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _: &S,
    ) -> Result<Self, Infallible> {
        Ok(match parts.extensions.get::<AppState>() {
            Some(state) => Self::of(&parts.method, &parts.extensions, state),
            None => Self {
                name: None,
                path: None,
            },
        })
    }
}

#[cfg(test)]
mod route_name_tests {
    use super::route_name_matches;

    #[test]
    fn patterns_match_like_laravel() {
        assert!(route_name_matches("admin.users.index", "admin.*"));
        assert!(route_name_matches("admin.users.index", "*.index"));
        assert!(route_name_matches("admin.users.index", "admin.*.index"));
        assert!(route_name_matches("products.show", "products.show"));
        assert!(!route_name_matches("products.show", "products"));
        assert!(!route_name_matches("shop.products.show", "products.*"));
        assert!(route_name_matches("anything", "*"));
        assert!(!route_name_matches("admin", "admin.*"));
        // A middle part that isn't there (#252).
        assert!(!route_name_matches("admin.users.index", "admin.*.posts.*"));
        assert!(route_name_matches(
            "admin.users.posts.edit",
            "admin.*.posts.*"
        ));
    }

    /// Outside the app's middleware (no `AppState` on the request), no
    /// route is current.
    #[tokio::test]
    async fn current_route_without_the_app_is_none() {
        use axum::extract::FromRequestParts;
        let (mut parts, ()) = axum::http::Request::builder()
            .uri("/x")
            .body(())
            .unwrap()
            .into_parts();
        let current = super::CurrentRoute::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(current.name(), None);
    }
}

/// A redirect with this status and `Location`.
fn redirect_with(status: axum::http::StatusCode, to: &str) -> axum::response::Response {
    match axum::http::HeaderValue::from_str(to) {
        Ok(location) => (status, [(axum::http::header::LOCATION, location)]).into_response(),
        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

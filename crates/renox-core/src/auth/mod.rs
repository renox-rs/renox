//! Authentication and authorization: the `users` table, password hashing,
//! session login with "remember me", route guards, policies and gates.
//!
//! ```
//! # #[derive(Model, serde::Serialize, Default)]
//! # #[model(table = "products")]
//! # struct Product { id: i64, name: String, price: i64, category: Option<String>, user_id: i64 }
//! # use renox::prelude::*;
//! # impl Policy for Product { fn allows(&self, user: &User, _: &str) -> bool { self.user_id == user.id } }
//! # let _ =
//! App::new()
//!     .module(Auth::new())                       // /login, /register, /logout
//!     .gate("admin", |user| user.email.ends_with("@shop.example"))
//!     .module(Shop)
//! # ;
//!
//! struct Shop;
//!
//! impl Module for Shop {
//! #   fn name(&self) -> &'static str { "shop" }
//!     fn routes(&self) -> Routes {
//!         Routes::new()
//!             .get("/products/{id}/edit", edit)
//!             .require_auth()                    // everything above needs a login
//!     }
//! }
//!
//! async fn edit(auth: AuthUser, State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
//!     let product = Product::find_or_404(&db, id).await?;
//!     auth.authorize("update", &product)?;       // 403 unless the policy allows it
//!     Ok(view("products/edit.html", context! { product }))
//! }
//! ```

pub(crate) mod account;
mod device;
pub mod events;
mod external;
mod inbox;
pub(crate) mod module;
pub mod notifications;
mod passwords;
pub mod permissions;
pub mod second_factor;
mod throttle;
mod tokens;
mod user;
mod verification;

use std::collections::HashMap;
use std::convert::Infallible;
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::{FromRequestParts, OptionalFromRequestParts, Request};
use axum::http::header::{ACCEPT, AUTHORIZATION};
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

pub(crate) use account::require_password_confirmed;
pub(crate) use device::require as device_guard;
pub use device::{Device, DeviceToken, NewDeviceToken};
pub use external::{confirm_identity, register_verified, registration_open, sign_in};
pub use module::{Auth, Registration};
pub use notifications::{
    Channel, DatabaseMessage, DatabaseNotification, Notification, Recipient,
    prune_read_notifications,
};
pub(crate) use permissions::Grants;
pub use permissions::Permissions;
pub use second_factor::{PendingLogin, complete_login, pending_login};
pub(crate) use throttle::LoginThrottle;
pub use tokens::{AccessToken, NewToken, prune_expired_tokens};
pub use user::{User, hash_password, needs_rehash, normalize_email, verify_password};
pub use verification::send_verification;

use crate::crypto::constant_time_eq;
use crate::db::Db;
use crate::{AppState, Error, Htmx, HxRedirect, Result, Session};

pub(crate) const AUTH_ID: &str = "_auth_user_id";
const AUTH_HASH: &str = "_auth_password_hash";
/// Unix milliseconds of the login, compared with `users.sessions_revoked_at`.
const AUTH_AT: &str = "_auth_at";
/// A random id per login, so one device can be logged out (`revoked_sessions`).
const AUTH_SID: &str = "_auth_session_id";
const INTENDED: &str = "_intended";

/// Decides whether a user may perform an ability on a model.
///
/// ```
/// # #[derive(Model, serde::Serialize, Default)]
/// # #[model(table = "products")]
/// # struct Product { id: i64, name: String, price: i64, category: Option<String>, user_id: i64 }
/// # use renox::prelude::*;
/// impl Policy for Product {
///     fn allows(&self, user: &User, ability: &str) -> bool {
///         match ability {
///             "update" | "delete" => self.user_id == user.id,
///             _ => true,
///         }
///     }
/// }
/// ```
pub trait Policy {
    /// Whether `user` may perform `ability` (e.g. `"update"`) on this model.
    fn allows(&self, user: &User, ability: &str) -> bool;
}

/// A model together with what the current user may do with it, so templates
/// can ask the policy: `{% if can('update', product) %}`.
///
/// ```
/// # use renox::prelude::*;
/// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, user_id: i64 }
/// # impl Policy for Product { fn allows(&self, user: &User, _: &str) -> bool { self.user_id == user.id } }
/// async fn index(State(db): State<Db>, user: Option<AuthUser>, Page(page): Page) -> Result<View> {
///     let products = Product::query().paginate(&db, page, 20).await?
///         .map(|p| Can::new(p, user.as_deref(), &["update", "delete"]));
///     Ok(view("products/index.html", context! { products }))
/// }
/// ```
///
/// It serializes as the model's own fields plus `_can` (`{"update": true, …}`);
/// guests get `false` for every ability.
#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub struct Can<T> {
    /// The model, serialized as its own fields.
    #[serde(flatten)]
    pub item: T,
    /// Each asked ability and whether it is allowed, serialized as `_can`.
    #[serde(rename = "_can")]
    pub abilities: std::collections::BTreeMap<String, bool>,
}

/// Who asks a policy, for [`Can::new`]: a [`User`], or an [`AuthUser`],
/// which also applies `App::gate_before` (pass `user.as_ref()` for that).
/// Sealed: only those two.
pub trait Viewer: viewer::Sealed {
    /// The user the policy is asked about.
    fn as_user(&self) -> &User;

    /// `App::gate_before`'s answer, if any.
    fn before(&self, _ability: &str) -> Option<bool> {
        None
    }
}

mod viewer {
    pub trait Sealed {}
    impl Sealed for super::User {}
    impl Sealed for super::AuthUser {}
}

impl Viewer for User {
    fn as_user(&self) -> &User {
        self
    }
}

impl Viewer for AuthUser {
    fn as_user(&self) -> &User {
        &self.user
    }

    fn before(&self, ability: &str) -> Option<bool> {
        AuthUser::before(self, ability)
    }
}

impl<T: Policy> Can<T> {
    /// Asks `item`'s policy about each of `abilities`; a guest (`None`) gets `false` for all.
    pub fn new<V: Viewer + ?Sized>(item: T, user: Option<&V>, abilities: &[&str]) -> Self {
        let abilities = abilities
            .iter()
            .map(|ability| {
                let allowed = user.is_some_and(|user| {
                    user.before(ability)
                        .unwrap_or_else(|| item.allows(user.as_user(), ability))
                });
                ((*ability).to_owned(), allowed)
            })
            .collect();
        Self { item, abilities }
    }
}

pub(crate) type Gate = Arc<dyn Fn(&User) -> bool + Send + Sync>;
pub(crate) type GateBefore = Arc<dyn Fn(&User, &str) -> Option<bool> + Send + Sync>;
pub(crate) type Gates = Arc<Access>;

/// The app's gates (`App::gate`), its `App::gate_before` hook, and whether
/// the `Permissions` module is on.
#[derive(Default)]
pub(crate) struct Access {
    pub gates: HashMap<String, Gate>,
    pub before: Option<GateBefore>,
    pub permissions: bool,
}

impl Access {
    /// A gate or permission named `name`: `gate_before` first, then the gate,
    /// then the user's permissions. Unknown names deny.
    pub(crate) fn check(&self, user: &User, grants: &Grants, name: &str) -> bool {
        if let Some(allowed) = self.before.as_ref().and_then(|before| before(user, name)) {
            return allowed;
        }
        match self.gates.get(name) {
            Some(check) => check(user),
            None => grants.has_permission(name),
        }
    }
}

/// The logged-in user's grants, in [`crate::context`] for the request.
#[derive(Clone)]
pub(crate) struct CurrentGrants {
    user_id: i64,
    grants: Arc<Grants>,
}

impl User {
    /// Whether this user has `role` (the `Permissions` module), answered
    /// from the roles loaded for the current request, so it works in
    /// `Policy::allows` and `App::gate_before` ("admins may do anything").
    /// It is `false` for any other user, and outside a request (a job, a
    /// command): use the async `user.roles(&db)` there.
    ///
    /// Global roles count, plus those given in the request's scope
    /// (`permissions::set_scope`), within their dates.
    pub fn has_role(&self, role: &str) -> bool {
        current_grants(self.id).is_some_and(|g| g.has_role(role))
    }

    /// Like [`User::has_role`], for a permission granted by one of the
    /// user's roles.
    pub fn has_permission(&self, permission: &str) -> bool {
        current_grants(self.id).is_some_and(|g| g.has_permission(permission))
    }
}

/// The logged-in user's id in the current request, if any.
pub(crate) fn current_user_id() -> Option<i64> {
    crate::context::get::<CurrentGrants>().map(|current| current.user_id)
}

fn current_grants(user_id: i64) -> Option<Arc<Grants>> {
    crate::context::get::<CurrentGrants>()
        .filter(|current| current.user_id == user_id)
        .map(|current| current.grants)
}

pub(crate) type AsyncGate = Arc<
    dyn Fn(
            User,
            AppState,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send>>
        + Send
        + Sync,
>;

/// The logged-in user of the current request, if any; set by Renox's auth
/// middleware for every request.
#[derive(Clone)]
pub(crate) struct CurrentUser {
    pub user: Option<Arc<User>>,
    pub gates: Gates,
    /// The API token of `Authorization: Bearer`, when that authenticated
    /// the request (so CSRF doesn't apply).
    pub token_id: Option<i64>,
    /// That token's abilities; `None` for every ability (sessions, and
    /// tokens made without a list).
    pub abilities: Option<Arc<Vec<String>>>,
    /// The device a device token authenticated (no user then).
    pub device: Option<Arc<Device>>,
    pub grants: Arc<Grants>,
}

/// The logged-in user. Requests without one are sent to the `login` route
/// (HTMX requests via `HX-Redirect`) or get 401 when they want JSON. Use
/// `Option<AuthUser>` where logging in is optional.
#[derive(Clone)]
pub struct AuthUser {
    user: Arc<User>,
    gates: Gates,
    state: Option<AppState>,
    token_id: Option<i64>,
    abilities: Option<Arc<Vec<String>>>,
    grants: Arc<Grants>,
    /// `role_names`, worked out the first time it's asked.
    role_names: std::sync::OnceLock<Vec<String>>,
}

impl Deref for AuthUser {
    type Target = User;

    fn deref(&self) -> &User {
        &self.user
    }
}

impl AuthUser {
    /// The id of the API token this request logged in with
    /// (`Authorization: Bearer`), or `None` for a session login. Revoke just
    /// that token on "log out" from an app: `user.revoke_token(&db, id)`.
    pub fn token_id(&self) -> Option<i64> {
        self.token_id
    }

    /// Whether the API token this request logged in with may do `ability`
    /// (`create_token_with(.., &["orders:read"], ..)`). Sessions, and tokens
    /// made without a list, may do everything.
    pub fn token_can(&self, ability: &str) -> bool {
        self.abilities
            .as_ref()
            .is_none_or(|list| list.iter().any(|a| a == ability || a == "*"))
    }

    /// Whether the user has `role` (the `Permissions` module): a global
    /// role, or one given in the request's scope (`permissions::set_scope`),
    /// within its dates.
    pub fn has_role(&self, role: &str) -> bool {
        self.grants.has_role(role)
    }

    /// Whether one of the user's roles grants `permission` (the
    /// `Permissions` module; the same roles as `has_role`).
    /// `allows(permission)` also asks `App::gate_before`.
    pub fn has_permission(&self, permission: &str) -> bool {
        self.grants.has_permission(permission)
    }

    /// Whether the user has `role` globally or in `scope` (the record's,
    /// not the request's), within its dates.
    pub fn has_role_in(&self, role: &str, scope: &permissions::Scope) -> bool {
        self.grants.has_role_in(role, Some(scope))
    }

    /// Whether a global role of the user, or one given in `scope` (the
    /// record's, not the request's), grants `permission` now; for
    /// policies and handlers that work on one record.
    pub fn has_permission_in(&self, permission: &str, scope: &permissions::Scope) -> bool {
        self.grants.has_permission_in(permission, Some(scope))
    }

    /// The records of model `M` in which the user holds `permission`:
    /// `Scopes::All` when a global role grants it, else their keys. See
    /// [`User::scopes_with`].
    pub fn scopes_with<M: crate::db::Model>(
        &self,
        permission: &str,
    ) -> permissions::Scopes<M::Key> {
        self.grants.scopes_with::<M>(permission)
    }

    /// The user's roles (the `Permissions` module) in effect: the global
    /// ones plus those in the request's scope, sorted, as they were the
    /// first time this was asked.
    pub fn role_names(&self) -> &[String] {
        self.role_names.get_or_init(|| self.grants.roles())
    }

    /// Whether the policy of `target` allows `ability` (after
    /// `App::gate_before`).
    pub fn can(&self, ability: &str, target: &impl Policy) -> bool {
        self.before(ability)
            .unwrap_or_else(|| target.allows(&self.user, ability))
    }

    fn before(&self, ability: &str) -> Option<bool> {
        self.gates
            .before
            .as_ref()
            .and_then(|before| before(&self.user, ability))
    }

    /// Like `can`, but a refusal becomes a 403 response.
    pub fn authorize(&self, ability: &str, target: &impl Policy) -> Result {
        if self.can(ability, target) {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }

    /// Whether the gate named `gate` lets this user through: `gate_before`,
    /// then the gate, then the user's permissions of that name. Unknown
    /// names deny.
    pub fn allows(&self, gate: &str) -> bool {
        self.gates.check(&self.user, &self.grants, gate)
    }

    /// Like `allows`, but a refusal becomes a 403 response.
    pub fn gate(&self, gate: &str) -> Result {
        if self.allows(gate) {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }

    /// Whether the gate named `gate` lets this user through, for gates made
    /// with `App::gate_async` (which may query the database) as well as
    /// plain ones. Unknown gates deny.
    pub async fn allows_async(&self, gate: &str) -> Result<bool> {
        if let Some(allowed) = self.before(gate) {
            return Ok(allowed);
        }
        if self.gates.gates.contains_key(gate) || self.grants.has_permission(gate) {
            return Ok(self.allows(gate));
        }
        let Some(state) = &self.state else {
            return Ok(false);
        };
        match state.async_gates.get(gate) {
            Some(check) => check(self.user.as_ref().clone(), state.clone()).await,
            None => Ok(false),
        }
    }

    /// Like `allows_async`, but a refusal becomes a 403 response.
    pub async fn gate_async(&self, gate: &str) -> Result {
        if self.allows_async(gate).await? {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }

    /// The logged-in user.
    pub fn user(&self) -> &User {
        &self.user
    }
}

impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> std::result::Result<Self, Response> {
        match current(&parts.extensions) {
            Some(user) => Ok(user),
            None => Err(unauthenticated(parts)),
        }
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for AuthUser {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> std::result::Result<Option<Self>, Infallible> {
        Ok(current(&parts.extensions))
    }
}

fn current(extensions: &axum::http::Extensions) -> Option<AuthUser> {
    let current = extensions.get::<CurrentUser>()?;
    Some(AuthUser {
        user: current.user.clone()?,
        gates: current.gates.clone(),
        state: extensions.get::<AppState>().cloned(),
        token_id: current.token_id,
        abilities: current.abilities.clone(),
        grants: current.grants.clone(),
        role_names: std::sync::OnceLock::new(),
    })
}

/// Logs `user` in for this session. With `remember` (usually
/// `Some(state.config.remember_lifetime)`, "remember me"), the session lasts
/// that long instead of `SESSION_LIFETIME`.
pub fn login(session: &Session, user: &User, remember: Option<std::time::Duration>) -> Result {
    session.regenerate_token();
    session.put(AUTH_ID, user.id)?;
    session.put(AUTH_HASH, fingerprint(&user.password))?;
    session.put(AUTH_AT, unix_millis())?;
    session.put(AUTH_SID, crate::crypto::random_token())?;
    if let Some(lifetime) = remember {
        session.set_lifetime(lifetime);
    }
    Ok(())
}

/// Logs this device out: ends the session (user, data and CSRF token), and
/// remembers its id so a copy of the cookie stops working too. The user's
/// other devices stay logged in; see [`logout_other_devices`] and
/// [`User::revoke_sessions`].
pub async fn logout(db: &Db, session: &Session) -> Result {
    match (session.get::<i64>(AUTH_ID), session.get::<String>(AUTH_SID)) {
        (Some(_), Some(sid)) => revoke_session(db, session, &sid).await?,
        // Logged in before sessions had ids: end them all to be safe.
        (Some(id), None) => {
            user::revoke_sessions(db, id).await?;
        }
        _ => {}
    }
    session.flush();
    Ok(())
}

/// Logs the user out everywhere except this device (e.g. "log out other
/// devices", or after a password change).
pub async fn logout_other_devices(db: &Db, session: &Session, user: &User) -> Result {
    let cut_off = user::revoke_sessions(db, user.id).await?;
    // Logged in again after the cut-off (even within the same millisecond),
    // so this session survives it.
    let lifetime = session
        .lifetime()
        .map(|minutes| std::time::Duration::from_secs(minutes * 60));
    login(session, user, lifetime)?;
    session.put(AUTH_AT, cut_off + 1)?;
    Ok(())
}

/// Changes the user's password and keeps this session logged in; every
/// other session ends.
pub async fn change_password(
    db: &Db,
    session: &Session,
    user: &mut User,
    password: &str,
) -> Result {
    user.set_password(db, password).await?;
    logout_other_devices(db, session, user).await
}

/// Denylists one session id until a copy of its cookie would have expired.
async fn revoke_session(db: &Db, session: &Session, sid: &str) -> Result {
    let minutes = session.lifetime().unwrap_or(60 * 24 * 30);
    let expires = crate::db::now() + chrono::Duration::minutes(minutes as i64);
    crate::db::sql("DELETE FROM revoked_sessions WHERE expires_at < ?")
        .bind(crate::db::now())
        .execute(db)
        .await?;
    crate::db::sql(
        "INSERT INTO revoked_sessions (id, expires_at) SELECT ?, ? \
         WHERE NOT EXISTS (SELECT 1 FROM revoked_sessions WHERE id = ?)",
    )
    .bind(sid)
    .bind(expires)
    .bind(sid)
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) fn unix_millis() -> i64 {
    crate::clock::unix_millis()
}

/// Ties a session to the password it was logged in with, so changing the
/// password logs out every other session.
fn fingerprint(password_hash: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(password_hash.as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// Loads the session's user once per request for extractors, guards and templates.
pub(crate) async fn middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let bearer = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    let session = req.extensions().get::<Session>().cloned();
    let mut device = None;
    let (user, token) = match (&bearer, &session) {
        (Some(bearer), _) if bearer.trim().starts_with('d') => {
            match device::authenticate(&state.db, bearer.trim()).await {
                Ok(found) => device = found.map(Arc::new),
                Err(err) => tracing::error!(error = ?err, "could not check the device token"),
            }
            (None, None)
        }
        // Only a token that authenticates turns CSRF off: a wrong or unknown
        // one leaves the request a guest's, with CSRF checked as usual.
        (Some(bearer), _) => match tokens::authenticate(&state.db, bearer.trim()).await {
            Ok(Some((user, token))) => (Some(user), Some(token)),
            Ok(None) => (None, None),
            Err(err) => {
                tracing::error!(error = ?err, "could not check the API token");
                (None, None)
            }
        },
        (None, Some(session)) => (resolve(&state, session).await, None),
        (None, None) => (None, None),
    };
    let grants = match &user {
        Some(user) if state.gates.permissions => {
            match permissions::grants(&state.db, user.id).await {
                Ok(grants) => grants,
                Err(err) => {
                    tracing::error!(error = ?err, "could not load the user's roles");
                    Grants::default()
                }
            }
        }
        _ => Grants::default(),
    };
    let grants = Arc::new(grants);
    if let Some(user) = &user {
        // For `User::has_role` in policies and `gate_before`, which get a
        // plain `User`.
        crate::context::set(CurrentGrants {
            user_id: user.id,
            grants: grants.clone(),
        });
    }
    req.extensions_mut().insert(CurrentUser {
        user: user.map(Arc::new),
        gates: state.gates.clone(),
        token_id: token.as_ref().map(|t| t.0),
        abilities: token.and_then(|t| t.1).map(Arc::new),
        device,
        grants,
    });
    req.extensions_mut().insert(state);
    next.run(req).await
}

async fn resolve(state: &AppState, session: &Session) -> Option<User> {
    let id: i64 = session.get(AUTH_ID)?;
    let hash: String = session.get(AUTH_HASH).unwrap_or_default();
    let logged_in_at: i64 = session.get(AUTH_AT).unwrap_or(0);
    let sid: String = session.get(AUTH_SID).unwrap_or_default();
    match User::find_with_revocation(&state.db, id, &sid).await {
        Ok(Some((user, revoked_at, session_revoked)))
            if constant_time_eq(&fingerprint(&user.password), &hash)
                && (revoked_at == 0 || logged_in_at > revoked_at)
                && !session_revoked =>
        {
            Some(user)
        }
        Ok(_) => {
            // Deleted user, changed password or logged out elsewhere: this
            // session is no longer theirs.
            session.remove(AUTH_ID);
            session.remove(AUTH_HASH);
            session.remove(AUTH_AT);
            session.remove(AUTH_SID);
            None
        }
        Err(err) => {
            tracing::error!(error = ?err, "could not load the logged-in user");
            None
        }
    }
}

fn wants_json(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/json"))
}

fn path_or(state: Option<&AppState>, route: &str, fallback: &str) -> String {
    state
        .and_then(|s| s.url(route, &[]).ok())
        .unwrap_or_else(|| fallback.to_owned())
}

/// Sends a guest to the login page, remembering where they were going.
fn unauthenticated(parts: &Parts) -> Response {
    if wants_json(&parts.headers) || parts.headers.contains_key(AUTHORIZATION) {
        let body = serde_json::json!({ "message": "Unauthenticated." });
        return (StatusCode::UNAUTHORIZED, axum::Json(body)).into_response();
    }
    let login = path_or(parts.extensions.get::<AppState>(), "login", "/login");
    if let (Some(session), &Method::GET) = (parts.extensions.get::<Session>(), &parts.method) {
        let intended = parts
            .uri
            .path_and_query()
            .map(|p| p.as_str().to_owned())
            .unwrap_or_else(|| "/".into());
        let _ = session.put(INTENDED, intended);
    }
    if Htmx::from_headers(&parts.headers).request {
        return HxRedirect(login).into_response();
    }
    Redirect::to(&login).into_response()
}

/// Route guard: only logged-in users. See `Routes::require_auth`.
pub(crate) async fn require_auth(req: Request, next: Next) -> Response {
    if current(req.extensions()).is_some() {
        return next.run(req).await;
    }
    let (parts, _) = req.into_parts();
    unauthenticated(&parts)
}

/// What a route guard asks of the logged-in user (`Routes::require_gate`,
/// `require_role`, `require_permission`, `require_ability`).
#[derive(Clone)]
pub(crate) enum Requirement {
    Gate(String),
    Role(String),
    Permission(String),
    Ability(String),
}

/// Route guard: a guest is sent to log in; a user who doesn't meet
/// `requirement` gets 403.
pub(crate) async fn require(requirement: Arc<Requirement>, req: Request, next: Next) -> Response {
    let Some(user) = current(req.extensions()) else {
        let (parts, _) = req.into_parts();
        return unauthenticated(&parts);
    };
    let allowed = match requirement.as_ref() {
        Requirement::Gate(gate) => match user.allows_async(gate).await {
            Ok(allowed) => allowed,
            Err(err) => return err.into_response(),
        },
        Requirement::Role(role) => user.has_role(role),
        Requirement::Permission(permission) => user.allows(permission),
        Requirement::Ability(ability) => user.token_can(ability),
    };
    if allowed {
        next.run(req).await
    } else {
        Error::Forbidden.into_response()
    }
}

/// Route guard: only users who verified their email. See `Routes::require_verified`.
pub(crate) async fn require_verified(req: Request, next: Next) -> Response {
    let Some(user) = current(req.extensions()) else {
        let (parts, _) = req.into_parts();
        return unauthenticated(&parts);
    };
    if user.email_verified_at.is_some() {
        return next.run(req).await;
    }
    if wants_json(req.headers()) || user_via_token(req.extensions()) {
        let body = serde_json::json!({ "message": "Your email address is not verified." });
        return (StatusCode::FORBIDDEN, axum::Json(body)).into_response();
    }
    let notice = path_or(
        req.extensions().get::<AppState>(),
        "verification.notice",
        "/verify-email",
    );
    if Htmx::from_headers(req.headers()).request {
        return HxRedirect(notice).into_response();
    }
    Redirect::to(&notice).into_response()
}

pub(crate) fn user_via_token(extensions: &axum::http::Extensions) -> bool {
    extensions
        .get::<CurrentUser>()
        .is_some_and(|c| c.token_id.is_some() || c.device.is_some())
}

/// Route guard: only guests; logged-in users go to the `home` route.
pub(crate) async fn guest_only(req: Request, next: Next) -> Response {
    if current(req.extensions()).is_none() {
        return next.run(req).await;
    }
    Redirect::to(&path_or(req.extensions().get::<AppState>(), "home", "/")).into_response()
}

/// Where to go after logging in: the page that asked for a login, else `fallback`.
pub(crate) fn intended(session: &Session, fallback: String) -> String {
    session
        .pull::<String>(INTENDED)
        .filter(|path| crate::htmx::is_local_path(path))
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_short_and_stable() {
        let a = fingerprint("$argon2id$v=19$abc");
        assert_eq!(a.len(), 32);
        assert_eq!(a, fingerprint("$argon2id$v=19$abc"));
        assert_ne!(a, fingerprint("$argon2id$v=19$abd"));
    }
}

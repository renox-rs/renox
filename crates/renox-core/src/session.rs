use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use cookie::{Cookie, CookieJar, Key, SameSite};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::crypto::random_token;
use crate::{AppState, Error, Result};

const OLD_INPUT: &str = "_old_input";
const ERRORS: &str = "_errors";
/// The named error bag the flashed errors belong to, when not the default one.
const ERROR_BAG: &str = "_error_bag";

/// The current visitor's session, stored in an encrypted cookie.
///
/// Values put in the session last until it expires. Flashed values are
/// readable during the next request only, which is how "saved!" messages and
/// old form input survive a redirect.
#[derive(Clone)]
pub struct Session(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    data: Map<String, Value>,
    /// Flashed by the previous request, readable now.
    flashed: Map<String, Value>,
    /// Flashed by this request, readable during the next one.
    flash_next: Map<String, Value>,
    token: String,
    /// Minutes this session lasts instead of `SESSION_LIFETIME` ("remember me").
    lifetime: Option<u64>,
    /// The session needs a new id (login, logout): with `SESSION_DRIVER=database`
    /// the old row is deleted, so a copy of the old cookie stops working.
    rotate: bool,
}

/// What the encrypted cookie holds: the whole session (`SESSION_DRIVER=cookie`)
/// or its id (`database`). A database app still reads a whole-session cookie,
/// so switching drivers logs nobody out (and `TestApp` can write one).
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    Handle { sid: String },
    Full(Payload),
}

#[derive(Serialize, Deserialize)]
struct Payload {
    #[serde(default)]
    data: Map<String, Value>,
    #[serde(default)]
    flash: Map<String, Value>,
    token: String,
    expires: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lifetime: Option<u64>,
}

/// Most bytes of old input kept, leaving room in the cookie for the rest
/// of the session.
const OLD_INPUT_LIMIT: usize = 2048;

fn drop_passwords(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !key.to_ascii_lowercase().contains("password"));
            map.values_mut().for_each(drop_passwords);
        }
        Value::Array(items) => items.iter_mut().for_each(drop_passwords),
        _ => {}
    }
}

fn old_input(input: Value) -> Value {
    let Value::Object(mut map) = input else {
        return Value::Object(Map::new());
    };
    map.retain(|key, _| !key.starts_with('_') && !key.to_ascii_lowercase().contains("password"));
    // Nested forms (`users[0][password]`) keep no password either.
    for value in map.values_mut() {
        drop_passwords(value);
    }
    let size = |map: &Map<String, Value>| serde_json::to_string(map).map_or(0, |s| s.len());
    while size(&map) > OLD_INPUT_LIMIT {
        let Some(largest) = map
            .iter()
            .max_by_key(|(_, v)| v.to_string().len())
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        map.remove(&largest);
    }
    Value::Object(map)
}

impl Session {
    fn new(payload: Option<Payload>) -> Self {
        let inner = match payload {
            Some(p) => Inner {
                data: p.data,
                flashed: p.flash,
                flash_next: Map::new(),
                token: p.token,
                lifetime: p.lifetime,
                rotate: false,
            },
            None => Inner {
                token: random_token(),
                ..Inner::default()
            },
        };
        Self(Arc::new(Mutex::new(inner)))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reads a value, including values flashed by the previous request.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let inner = self.lock();
        let value = inner
            .data
            .get(key)
            .or_else(|| inner.flash_next.get(key))
            .or_else(|| inner.flashed.get(key))?;
        serde_json::from_value(value.clone()).ok()
    }

    /// Whether `key` is set, including values flashed by the previous request.
    pub fn has(&self, key: &str) -> bool {
        let inner = self.lock();
        inner.data.contains_key(key)
            || inner.flash_next.contains_key(key)
            || inner.flashed.contains_key(key)
    }

    /// Stores a value under `key`.
    pub fn put(&self, key: &str, value: impl Serialize) -> Result {
        let value = serde_json::to_value(value)?;
        self.lock().data.insert(key.to_owned(), value);
        Ok(())
    }

    /// Removes `key` and returns its value, if it was set.
    pub fn remove(&self, key: &str) -> Option<Value> {
        self.lock().data.remove(key)
    }

    /// Appends `value` to the list stored under `key` (a new list if there
    /// is none; a value that isn't a list becomes its first item), e.g. the
    /// recently viewed products. Returns the list's new length.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # fn demo(session: Session) -> Result {
    /// session.push("recent", 42)?;
    /// let views = session.increment("views", 1)?; // 1, then 2, …
    /// # let _ = views; Ok(()) }
    /// ```
    pub fn push(&self, key: &str, value: impl Serialize) -> Result<usize> {
        let value = serde_json::to_value(value)?;
        let mut inner = self.lock();
        let entry = inner
            .data
            .entry(key.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        if !entry.is_array() {
            *entry = Value::Array(vec![entry.take()]);
        }
        let list = entry.as_array_mut().expect("made an array above");
        list.push(value);
        Ok(list.len())
    }

    /// Adds `by` (which may be negative) to the number under `key`, taking a
    /// missing or non-numeric value as 0, and returns the new number.
    pub fn increment(&self, key: &str, by: i64) -> Result<i64> {
        let mut inner = self.lock();
        let current = inner.data.get(key).and_then(Value::as_i64).unwrap_or(0);
        let next = current.saturating_add(by);
        inner.data.insert(key.to_owned(), Value::from(next));
        Ok(next)
    }

    /// Reads a value and removes it.
    pub fn pull<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let value = self.lock().data.remove(key)?;
        serde_json::from_value(value).ok()
    }

    /// Keeps this session for `lifetime` of inactivity instead of
    /// `SESSION_LIFETIME`, e.g. for "remember me" (whole minutes, at least one).
    pub fn set_lifetime(&self, lifetime: std::time::Duration) {
        self.lock().lifetime = Some(lifetime.as_secs().div_ceil(60).max(1));
    }

    /// Stores a value for the next request only.
    pub fn flash(&self, key: &str, value: impl Serialize) -> Result {
        let value = serde_json::to_value(value)?;
        self.lock().flash_next.insert(key.to_owned(), value);
        Ok(())
    }

    /// Keeps the values flashed by the previous request for one more request.
    pub fn reflash(&self) {
        let mut inner = self.lock();
        let flashed = inner.flashed.clone();
        for (key, value) in flashed {
            inner.flash_next.entry(key).or_insert(value);
        }
    }

    /// Keeps only these of the values flashed by the previous request for
    /// one more request (Laravel's `keep`), e.g. a status message across a
    /// second redirect.
    pub fn keep(&self, keys: &[&str]) {
        let mut inner = self.lock();
        for key in keys {
            if let Some(value) = inner.flashed.get(*key).cloned() {
                inner.flash_next.entry((*key).to_owned()).or_insert(value);
            }
        }
    }

    /// A flash value for this request only (Laravel's `flash()->now()`):
    /// the page rendered now sees it in `flash`, the next request doesn't.
    /// For a handler that renders a page instead of redirecting.
    pub fn flash_now(&self, key: &str, value: impl Serialize) -> Result {
        let value = serde_json::to_value(value)?;
        self.lock().flashed.insert(key.to_owned(), value);
        Ok(())
    }

    /// Flashes the submitted form so the next page can refill it with `old()`.
    /// Passwords and Renox's own fields (`_token`, `_method`) are never kept,
    /// and the largest values are dropped when the rest wouldn't fit in the
    /// cookie (browsers drop cookies over 4 KB).
    pub fn flash_input(&self, input: &impl Serialize) -> Result {
        self.flash(OLD_INPUT, old_input(serde_json::to_value(input)?))
    }

    /// A field from the input flashed by the previous request.
    /// `field` may be a nested name (`items[0][name]` or `items.0.name`)
    /// of a form that used them.
    pub fn old(&self, field: &str) -> Option<Value> {
        let inner = self.lock();
        let input = inner.flashed.get(OLD_INPUT)?;
        input
            .get(field)
            .or_else(|| crate::validation::nested::lookup(input, field))
            .cloned()
    }

    /// Whether the previous request flashed its input (a failed submit), even
    /// with no field in it: an unticked checkbox sends nothing, so `old()`
    /// alone can't tell "unticked" from "not submitted yet".
    pub fn has_old_input(&self) -> bool {
        self.lock().flashed.contains_key(OLD_INPUT)
    }

    /// Flashes validation errors, keyed by field, for the next request.
    pub fn flash_errors(&self, errors: &impl Serialize) -> Result {
        self.flash(ERRORS, errors)
    }

    /// Flashes validation errors for the next request in a named bag (Laravel's
    /// error bags), e.g. `login` when a page has two forms; templates read
    /// them with `error('email', bag='login')`.
    pub fn flash_errors_in(&self, bag: &str, errors: &impl Serialize) -> Result {
        self.flash(ERRORS, errors)?;
        self.flash(ERROR_BAG, bag)
    }

    /// Validation errors flashed by the previous request, in the default bag
    /// (errors flashed in a named bag aren't here; see [`errors_in`](Self::errors_in)).
    pub fn errors(&self) -> Map<String, Value> {
        let inner = self.lock();
        match (inner.flashed.get(ERRORS), inner.flashed.get(ERROR_BAG)) {
            (Some(Value::Object(errors)), None) => errors.clone(),
            _ => Map::new(),
        }
    }

    /// Validation errors flashed by the previous request in the bag `bag`.
    pub fn errors_in(&self, bag: &str) -> Map<String, Value> {
        let inner = self.lock();
        match (inner.flashed.get(ERRORS), inner.flashed.get(ERROR_BAG)) {
            (Some(Value::Object(errors)), Some(Value::String(name))) if name == bag => {
                errors.clone()
            }
            _ => Map::new(),
        }
    }

    /// The bag the previous request's errors were flashed in, if not the default.
    pub fn error_bag(&self) -> Option<String> {
        match self.lock().flashed.get(ERROR_BAG) {
            Some(Value::String(name)) => Some(name.clone()),
            _ => None,
        }
    }

    /// Values flashed by the previous request, except old input and errors.
    pub fn flashed(&self) -> Map<String, Value> {
        let mut flashed = self.lock().flashed.clone();
        flashed.remove(OLD_INPUT);
        flashed.remove(ERRORS);
        flashed.remove(ERROR_BAG);
        flashed
    }

    /// The CSRF token forms and HTMX requests must send back.
    pub fn token(&self) -> String {
        self.lock().token.clone()
    }

    /// Issues a new CSRF token, e.g. after logging in.
    pub fn regenerate_token(&self) {
        let mut inner = self.lock();
        inner.token = random_token();
        inner.rotate = true;
    }

    /// Removes all data and flashed values and issues a new CSRF token.
    pub fn flush(&self) {
        let mut inner = self.lock();
        *inner = Inner {
            token: random_token(),
            rotate: true,
            ..Inner::default()
        };
    }

    fn take_rotate(&self) -> bool {
        std::mem::take(&mut self.lock().rotate)
    }

    pub(crate) fn lifetime(&self) -> Option<u64> {
        self.lock().lifetime
    }

    fn to_payload(&self, expires: u64) -> Payload {
        let inner = self.lock();
        Payload {
            data: inner.data.clone(),
            flash: inner.flash_next.clone(),
            token: inner.token.clone(),
            expires,
            lifetime: inner.lifetime,
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Session {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self> {
        parts
            .extensions
            .get::<Session>()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("the session middleware is not installed").into())
    }
}

pub(crate) async fn middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let config = &state.config;
    let now = unix_now();
    let database = config.session_driver == crate::SessionDriver::Database;
    let (sid, payload) = match read_cookie(req.headers(), &config.session_cookie, &state.key) {
        Some(Stored::Handle { sid }) if database => {
            let payload = store::load(&state, &sid, now).await;
            (payload.is_some().then_some(sid), payload)
        }
        Some(Stored::Full(payload)) if payload.expires > now => (None, Some(payload)),
        _ => (None, None),
    };
    let loaded = payload.as_ref().map(store::fingerprint);
    let session = Session::new(payload);
    req.extensions_mut().insert(session.clone());

    let mut res = next.run(req).await;

    let lifetime = session
        .lifetime()
        .map_or(config.session_lifetime.as_secs(), |minutes| minutes * 60);
    let payload = session.to_payload(now + lifetime);
    let stored = if database {
        let rotate = session.take_rotate();
        let (sid, fresh) = match sid {
            Some(old) if rotate => {
                store::destroy(&state, &old).await;
                (random_token(), true)
            }
            Some(sid) => (sid, false),
            None => (random_token(), true),
        };
        // A new id has no row yet, whatever the session holds.
        let changed = fresh || loaded.as_deref() != Some(store::fingerprint(&payload).as_str());
        if let Err(err) = store::save(&state, &sid, &payload, now, changed).await {
            tracing::error!(error = ?err, "could not save the session");
            return res;
        }
        store::maybe_prune(&state, now);
        Stored::Handle { sid }
    } else {
        Stored::Full(payload)
    };
    let value = match serde_json::to_string(&stored) {
        Ok(value) => value,
        Err(err) => {
            tracing::error!(error = %err, "could not serialize the session");
            return res;
        }
    };
    let cookie = Cookie::build((config.session_cookie.clone(), value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(config.url.starts_with("https://"))
        .max_age(cookie::time::Duration::seconds(lifetime as i64))
        .build();
    let mut jar = CookieJar::new();
    jar.private_mut(&state.key).add(cookie);
    for cookie in jar.delta() {
        let encoded = cookie.encoded().to_string();
        if encoded.len() > 4000 {
            tracing::warn!(
                bytes = encoded.len(),
                "the session cookie is larger than browsers reliably store; \
                 SESSION_DRIVER=database keeps sessions on the server"
            );
        }
        if let Ok(value) = HeaderValue::from_str(&encoded) {
            res.headers_mut().append(SET_COOKIE, value);
        }
    }
    res
}

/// The `sessions` table (`SESSION_DRIVER=database`).
mod store {
    use super::Payload;
    use crate::{AppState, Result};

    /// Rows are keyed by the id's hash, so the table alone takes over nothing.
    fn key(sid: &str) -> String {
        crate::webhook::sha256_hex(sid)
    }

    /// What decides whether a session changed (not its expiry).
    pub(super) fn fingerprint(payload: &Payload) -> String {
        serde_json::to_string(&(
            &payload.data,
            &payload.flash,
            &payload.token,
            payload.lifetime,
        ))
        .unwrap_or_default()
    }

    pub(super) async fn load(state: &AppState, sid: &str, now: u64) -> Option<Payload> {
        let key = key(sid);
        if let Some(mirror) = &state.session_mirror {
            return mirror
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&key)
                .and_then(|json| serde_json::from_str::<Payload>(json).ok())
                .filter(|p| p.expires > now);
        }
        let row: Option<String> =
            crate::db::sql("SELECT payload FROM sessions WHERE id = ? AND expires_at > ?")
                .bind(key)
                .bind(now as i64)
                .scalar_optional(&state.db)
                .await
                .map_err(|err| tracing::error!(error = ?err, "could not read a session"))
                .ok()
                .flatten();
        row.and_then(|json| serde_json::from_str(&json).ok())
    }

    /// Writes the session when it changed, and otherwise at most once a
    /// minute to move its expiry along.
    pub(super) async fn save(
        state: &AppState,
        sid: &str,
        payload: &Payload,
        now: u64,
        changed: bool,
    ) -> Result {
        let key = key(sid);
        let json = serde_json::to_string(payload).map_err(anyhow::Error::from)?;
        if let Some(mirror) = &state.session_mirror {
            mirror
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key, json);
            return Ok(());
        }
        let user_id = payload
            .data
            .get(crate::auth::AUTH_ID)
            .and_then(serde_json::Value::as_i64);
        if changed {
            crate::db::sql(
                "INSERT INTO sessions (id, user_id, payload, expires_at, last_activity) \
                 VALUES (?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET user_id = excluded.user_id, \
                 payload = excluded.payload, expires_at = excluded.expires_at, \
                 last_activity = excluded.last_activity",
            )
            .bind(key)
            .bind(user_id)
            .bind(json)
            .bind(payload.expires as i64)
            .bind(now as i64)
            .execute(&state.db)
            .await?;
        } else {
            crate::db::sql(
                "UPDATE sessions SET expires_at = ?, last_activity = ? \
                 WHERE id = ? AND last_activity < ?",
            )
            .bind(payload.expires as i64)
            .bind(now as i64)
            .bind(key)
            .bind(now as i64 - 60)
            .execute(&state.db)
            .await?;
        }
        Ok(())
    }

    pub(super) async fn destroy(state: &AppState, sid: &str) {
        let key = key(sid);
        if let Some(mirror) = &state.session_mirror {
            mirror
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
            return;
        }
        if let Err(err) = crate::db::sql("DELETE FROM sessions WHERE id = ?")
            .bind(key)
            .execute(&state.db)
            .await
        {
            tracing::error!(error = ?err, "could not delete a session");
        }
    }

    /// Now and then (1 request in 50), deletes expired sessions in the background.
    pub(super) fn maybe_prune(state: &AppState, now: u64) {
        if state.session_mirror.is_some() || rand::random_range(0..50) != 0 {
            return;
        }
        let db = state.db.clone();
        tokio::spawn(async move {
            if let Err(err) = prune(&db, now).await {
                tracing::warn!(error = ?err, "could not prune sessions");
            }
        });
    }

    pub(crate) async fn prune(db: &crate::db::Db, now: u64) -> Result<u64> {
        Ok(crate::db::sql("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(now as i64)
            .execute(db)
            .await?)
    }
}

impl Session {
    /// Deletes expired rows of the `sessions` table (`SESSION_DRIVER=database`)
    /// and returns how many; `my-app session:prune` runs it. Requests also
    /// prune now and then, so this is for a quiet app or a schedule.
    pub async fn prune_expired(db: &crate::db::Db) -> Result<u64> {
        store::prune(db, unix_now()).await
    }
}

/// The sessions table's migration, always installed (it's empty with the
/// cookie driver).
pub(crate) const MIGRATION: crate::db::Migration =
    crate::db::framework_migration!("session", "00010101000210_create_sessions_table");

fn read_cookie(headers: &HeaderMap, name: &str, key: &Key) -> Option<Stored> {
    let mut jar = CookieJar::new();
    for header in headers.get_all(COOKIE) {
        let Ok(header) = header.to_str() else {
            continue;
        };
        for cookie in Cookie::split_parse_encoded(header.to_owned()).flatten() {
            if cookie.name() == name {
                jar.add_original(cookie);
            }
        }
    }
    let cookie = jar.private(key).get(name)?;
    serde_json::from_str(cookie.value()).ok()
}

fn unix_now() -> u64 {
    crate::clock::unix_secs().max(0) as u64
}

/// The session carried by a `Cookie` header value (a new one if absent or
/// invalid). Used by `renox::testing`.
pub(crate) fn from_cookie(state: &AppState, cookie_header: Option<&str>) -> Session {
    let mut headers = HeaderMap::new();
    if let Some(value) = cookie_header.and_then(|v| HeaderValue::from_str(v).ok()) {
        headers.insert(COOKIE, value);
    }
    let now = unix_now();
    let payload = match read_cookie(&headers, &state.config.session_cookie, &state.key) {
        Some(Stored::Full(payload)) => Some(payload),
        // A database session, read back through the test mirror.
        Some(Stored::Handle { sid }) => state.session_mirror.as_ref().and_then(|mirror| {
            mirror
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&crate::webhook::sha256_hex(&sid))
                .and_then(|json| serde_json::from_str::<Payload>(json).ok())
        }),
        None => None,
    };
    Session::new(payload.filter(|payload| payload.expires > now))
}

/// `session` encrypted as a `name=value` cookie pair. Used by `renox::testing`.
pub(crate) fn cookie_pair(state: &AppState, session: &Session) -> String {
    let lifetime = session
        .lifetime()
        .map_or(state.config.session_lifetime.as_secs(), |minutes| {
            minutes * 60
        });
    // A whole-session cookie, which the database driver reads too.
    let value = serde_json::to_string(&Stored::Full(session.to_payload(unix_now() + lifetime)))
        .expect("session payloads serialize");
    let mut jar = CookieJar::new();
    jar.private_mut(&state.key)
        .add(Cookie::new(state.config.session_cookie.clone(), value));
    let cookie = jar
        .delta()
        .next()
        .expect("the cookie was just added")
        .encoded()
        .to_string();
    cookie.split(';').next().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn old_input_keeps_objects_only_and_drops_passwords_and_the_largest_values() {
        // Old input is always an object: a JSON list or text keeps nothing.
        assert_eq!(old_input(json!(["a", "b"])), json!({}));
        assert_eq!(old_input(json!("text")), json!({}));
        let big = "x".repeat(OLD_INPUT_LIMIT);
        let kept = old_input(json!({
            "_token": "t",
            "name": "Ann",
            "users": [{ "email": "a@b.c", "Password": "secret" }],
            "essay": big,
        }));
        assert_eq!(
            kept,
            json!({ "name": "Ann", "users": [{ "email": "a@b.c" }] })
        );
    }
}

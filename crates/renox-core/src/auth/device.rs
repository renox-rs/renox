//! API tokens owned by a device (a store's kiosk, a till, a sensor) instead
//! of a user, so it needs no placeholder `users` row.
//!
//! The app names its devices with a key of its own (`"kiosk:3"`). A token
//! made for that key is sent like any API token, as
//! `Authorization: Bearer <plain>`; the request then carries a [`Device`]
//! (and no user). Device tokens are stored in `device_tokens` (the `Auth`
//! module's migration) and have the same abilities and expiry as a user's.

use std::ops::Deref;
use std::sync::Arc;

use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use super::tokens::sha256_hex;
use crate::Result;
use crate::crypto::{constant_time_eq, random_token};
use crate::db::{DateTime, Db, now};

/// A stored API token of a device. Only a SHA-256 hash of the secret is stored.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct DeviceToken {
    /// The `device_tokens` row id, also what follows the `d` in the plain token.
    pub id: i64,
    /// The app's key for the device that owns the token, e.g. `kiosk:3`.
    pub device: String,
    /// A label chosen at creation.
    pub name: String,
    /// What the token may do ([`Device::can`]); `None` for everything.
    pub abilities: Option<Vec<String>>,
    /// When a request last authenticated with it; `None` if never used.
    pub last_used_at: Option<DateTime>,
    /// When it stops working; `None` for never.
    pub expires_at: Option<DateTime>,
    /// When it was created.
    pub created_at: Option<DateTime>,
}

/// A freshly created device token. `plain` is shown once; the device sends
/// it as `Authorization: Bearer <plain>`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct NewDeviceToken {
    /// The stored token.
    pub token: DeviceToken,
    /// The secret as `d<id>|<random>`; not stored, so it can't be shown again.
    pub plain: String,
}

const COLUMNS: &str = "id, device, name, abilities, last_used_at, expires_at, created_at";

fn from_row(row: &crate::db::Row) -> std::result::Result<DeviceToken, crate::db::DbError> {
    let abilities: Option<String> = row.try_get("abilities")?;
    Ok(DeviceToken {
        id: row.try_get("id")?,
        device: row.try_get("device")?,
        name: row.try_get("name")?,
        abilities: abilities.and_then(|json| serde_json::from_str(&json).ok()),
        last_used_at: row.try_get("last_used_at")?,
        expires_at: row.try_get("expires_at")?,
        created_at: row.try_get("created_at")?,
    })
}

impl DeviceToken {
    /// Creates a token for `device` (the app's key, e.g. `"kiosk:3"`).
    /// `abilities` limits it (`Some(&["sales:create"])`, `"*"` allows
    /// everything); `None` allows everything. It stops working at `expires_at`.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # async fn demo(db: Db) -> Result {
    /// use renox::auth::DeviceToken;
    /// let made = DeviceToken::create(&db, "kiosk:3", "front desk", Some(&["sales:create"]), None).await?;
    /// println!("give this to the kiosk once: {}", made.plain);
    /// # Ok(()) }
    /// ```
    pub async fn create(
        db: &Db,
        device: &str,
        name: &str,
        abilities: Option<&[&str]>,
        expires_at: Option<DateTime>,
    ) -> Result<NewDeviceToken> {
        let secret = random_token();
        let created = now();
        let abilities = abilities.map(|list| serde_json::json!(list).to_string());
        let row = crate::db::sql(format!(
            "INSERT INTO device_tokens (device, name, abilities, token, expires_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING {COLUMNS}"
        ))
        .bind(device)
        .bind(name)
        .bind(abilities)
        .bind(sha256_hex(&secret))
        .bind(expires_at)
        .bind(created)
        .bind(created)
        .fetch_one(db)
        .await?;
        let token = from_row(&row)?;
        Ok(NewDeviceToken {
            plain: format!("d{}|{secret}", token.id),
            token,
        })
    }

    /// The tokens of `device`, newest first.
    pub async fn for_device(db: &Db, device: &str) -> Result<Vec<DeviceToken>> {
        let rows = crate::db::sql(format!(
            "SELECT {COLUMNS} FROM device_tokens WHERE device = ? ORDER BY id DESC"
        ))
        .bind(device)
        .fetch_all(db)
        .await?;
        Ok(rows
            .iter()
            .map(from_row)
            .collect::<std::result::Result<_, _>>()?)
    }

    /// Revokes one token of `device`; returns whether it existed.
    pub async fn revoke(db: &Db, device: &str, token_id: i64) -> Result<bool> {
        let done = crate::db::sql("DELETE FROM device_tokens WHERE id = ? AND device = ?")
            .bind(token_id)
            .bind(device)
            .execute(db)
            .await?;
        Ok(done > 0)
    }

    /// Revokes every token of `device` (when it is retired or lost);
    /// returns how many.
    pub async fn revoke_all(db: &Db, device: &str) -> Result<u64> {
        Ok(crate::db::sql("DELETE FROM device_tokens WHERE device = ?")
            .bind(device)
            .execute(db)
            .await?)
    }

    /// Deletes device tokens that expired more than `grace` ago; returns
    /// how many. `tokens:prune` (from the `Auth` module) runs it too.
    pub async fn prune_expired(db: &Db, grace: std::time::Duration) -> Result<u64> {
        let before = now() - chrono::Duration::from_std(grace).unwrap_or_default();
        Ok(
            crate::db::sql("DELETE FROM device_tokens WHERE expires_at < ?")
                .bind(before)
                .execute(db)
                .await?,
        )
    }
}

/// The device a request authenticated as, with `Authorization: Bearer` and a
/// [`DeviceToken`]. Requests without one get 401; use `Option<Device>`
/// where a device is optional. A device request has no user, so
/// [`AuthUser`](crate::AuthUser) answers 401 on it.
///
/// ```
/// # use renox::prelude::*;
/// use renox::auth::Device;
/// async fn sale(device: Device) -> Result<String> {
///     if !device.can("sales:create") {
///         return Err(Error::Forbidden);
///     }
///     Ok(format!("sale at {}", device.key()))
/// }
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Device {
    key: String,
    token_id: i64,
    name: String,
    abilities: Option<Arc<Vec<String>>>,
}

impl Device {
    /// The app's key of the device that owns the token, e.g. `kiosk:3`.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The id of the token this request used (`DeviceToken::revoke`).
    pub fn token_id(&self) -> i64 {
        self.token_id
    }

    /// The token's label.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the token may do `ability`; tokens made without a list, and
    /// `"*"`, may do everything.
    pub fn can(&self, ability: &str) -> bool {
        self.abilities
            .as_ref()
            .is_none_or(|list| list.iter().any(|a| a == ability || a == "*"))
    }

    /// The `id` part of a key like `kiosk:3` (`device.id_of("kiosk")` is
    /// `Some("3")`); `None` for another kind.
    pub fn id_of(&self, kind: &str) -> Option<&str> {
        self.key.strip_prefix(kind)?.strip_prefix(':')
    }
}

impl Deref for Device {
    type Target = str;

    fn deref(&self) -> &str {
        &self.key
    }
}

fn unauthenticated() -> Response {
    let body = serde_json::json!({ "message": "Unauthenticated." });
    (StatusCode::UNAUTHORIZED, axum::Json(body)).into_response()
}

pub(super) fn current(extensions: &axum::http::Extensions) -> Option<Device> {
    extensions
        .get::<super::CurrentUser>()?
        .device
        .as_deref()
        .cloned()
}

impl<S: Send + Sync> FromRequestParts<S> for Device {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> std::result::Result<Self, Response> {
        current(&parts.extensions).ok_or_else(unauthenticated)
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for Device {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> std::result::Result<Option<Self>, std::convert::Infallible> {
        Ok(current(&parts.extensions))
    }
}

/// The device behind `Authorization: Bearer d<id>|<secret>`, if the token is
/// valid.
pub(super) async fn authenticate(db: &Db, bearer: &str) -> Result<Option<Device>> {
    let Some((id, secret)) = bearer.strip_prefix('d').and_then(|b| b.split_once('|')) else {
        return Ok(None);
    };
    let Ok(id) = id.parse::<i64>() else {
        return Ok(None);
    };
    let Some(row) = crate::db::sql(
        "SELECT device, name, token, abilities, expires_at FROM device_tokens WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };
    let hash: String = row.try_get("token")?;
    let expires_at: Option<DateTime> = row.try_get("expires_at")?;
    if !constant_time_eq(&hash, &sha256_hex(secret)) || expires_at.is_some_and(|at| at <= now()) {
        return Ok(None);
    }
    crate::db::sql("UPDATE device_tokens SET last_used_at = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(db)
        .await?;
    let abilities: Option<String> = row.try_get("abilities")?;
    Ok(Some(Device {
        key: row.try_get("device")?,
        token_id: id,
        name: row.try_get("name")?,
        abilities: abilities
            .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
            .map(Arc::new),
    }))
}

/// Route guard: only requests that authenticated as a device, and, with
/// `ability`, whose token may do it. See `Routes::require_device`.
pub(crate) async fn require(
    ability: Option<Arc<String>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(device) = current(req.extensions()) else {
        return unauthenticated();
    };
    if ability.is_some_and(|a| !device.can(&a)) {
        return crate::Error::Forbidden.into_response();
    }
    next.run(req).await
}

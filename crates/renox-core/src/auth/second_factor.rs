//! A second login step that a module provides, such as two-factor
//! authentication (`renox-2fa`).
//!
//! A module registers it with [`Registry::second_factor`](crate::Registry::second_factor):
//! the name of its challenge route, and whether a user must pass it. When a
//! user who must logs in with the right password, the `Auth` module doesn't
//! log them in: the login waits in the session ([`PendingLogin`], for ten
//! minutes) and the browser goes to the challenge. The challenge's handler
//! checks the code and calls [`complete_login`], which logs in exactly as the
//! login page would have (and sends the user where they were going).
//! Registration asks the same question about the new user, so a step every
//! user must pass isn't skipped by signing up.
//!
//! ```
//! use renox::auth::{complete_login, pending_login};
//! use renox::prelude::*;
//! use serde::Deserialize;
//!
//! struct Pin;
//!
//! impl Module for Pin {
//!     fn name(&self) -> &'static str {
//!         "pin"
//!     }
//!
//!     fn register(&self, app: &mut Registry) {
//!         // Whether `user` must pass the challenge (here: everyone with a PIN set).
//!         app.second_factor("pin.challenge", |user, _state| async move {
//!             Ok(user.extra.contains_key("pin"))
//!         });
//!     }
//!
//!     fn routes(&self) -> Routes {
//!         Routes::new()
//!             .get("/pin", show)
//!             .post("/pin", check)
//!             .name("pin.challenge")
//!             .guest_only()
//!     }
//! }
//!
//! async fn show(session: Session) -> Result<Response> {
//!     match pending_login(&session) {
//!         Some(_) => Ok(view("pin.html", context! {}).into_response()),
//!         None => Ok(Redirect::to("/login").into_response()), // expired, or never started
//!     }
//! }
//!
//! #[derive(Deserialize, Validate)]
//! struct PinForm {
//!     #[validate(required)]
//!     pin: String,
//! }
//!
//! async fn check(
//!     State(state): State<AppState>,
//!     session: Session,
//!     htmx: Htmx,
//!     ClientIp(ip): ClientIp,
//!     Valid(form): Valid<PinForm>,
//! ) -> Result<Response> {
//!     let Some(pending) = pending_login(&session) else {
//!         return Ok(Redirect::to("/login").into_response());
//!     };
//!     if pending.locked_out(&state, ip).await.is_some() {
//!         return Err(Error::TooManyRequests);
//!     }
//!     let user = User::find_or_404(&state.db, pending.user_id).await?;
//!     if user.extra.get("pin").and_then(|pin| pin.as_str()) != Some(form.pin.as_str()) {
//!         pending.failed(&state, ip).await;
//!         let mut errors = Errors::new();
//!         errors.add("pin", "That PIN is wrong.");
//!         return Err(ValidationError::new(errors).into());
//!     }
//!     let to = complete_login(&state, &session, &pending, ip).await?;
//!     Ok(match to {
//!         Some(to) if htmx.request => HxRedirect(to).into_response(),
//!         Some(to) => Redirect::to(&to).into_response(),
//!         None => Redirect::to("/login").into_response(), // the password changed meanwhile
//!     })
//! }
//! ```

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::events::{LoggedIn, LoginFailed, announce};
use super::{User, fingerprint, login};
use crate::db::Model;
use crate::{AppState, Result, Session};

/// How long a login waits for its second step.
const PENDING_FOR: i64 = 10 * 60;
/// Where the waiting login lives in the session.
const PENDING: &str = "_auth_pending";

pub(crate) type RequiredFn =
    Arc<dyn Fn(User, AppState) -> Pin<Box<dyn Future<Output = Result<bool>> + Send>> + Send + Sync>;

/// What a module registered with `Registry::second_factor`.
#[derive(Clone)]
pub(crate) struct SecondFactor {
    /// The challenge's route name.
    pub(crate) challenge: String,
    pub(crate) required: RequiredFn,
}

pub(crate) fn second_factor<F, Fut>(challenge: &str, required: F) -> SecondFactor
where
    F: Fn(User, AppState) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<bool>> + Send + 'static,
{
    SecondFactor {
        challenge: challenge.to_owned(),
        required: Arc::new(move |user, state| Box::pin(required(user, state))),
    }
}

/// The waiting login, as kept in the session.
#[derive(Serialize, Deserialize)]
struct Stored {
    user_id: i64,
    email: String,
    remember: bool,
    /// The password's fingerprint: a new password ends the wait.
    hash: String,
    /// When the password was right (unix seconds).
    at: i64,
    /// Where the user goes once logged in.
    to: String,
}

/// A login that had the right password and waits for its second step; from
/// [`pending_login`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PendingLogin {
    /// The user logging in.
    pub user_id: i64,
    /// The email address they typed, for the login throttle.
    pub email: String,
    /// Whether they ticked "remember me".
    pub remember: bool,
    hash: String,
    to: String,
}

impl PendingLogin {
    /// Seconds this login must wait after too many wrong codes (the login
    /// throttle, as for wrong passwords); `None` when it may try.
    pub async fn locked_out(&self, state: &AppState, ip: Option<IpAddr>) -> Option<u64> {
        state.throttle.blocked_for(&self.email, ip).await
    }

    /// Counts a wrong code: towards the login throttle, and as a
    /// `LoginFailed` event (which the `Audit` module records).
    pub async fn failed(&self, state: &AppState, ip: Option<IpAddr>) {
        state.throttle.fail(&self.email, ip).await;
        let event = LoginFailed {
            email: self.email.clone(),
            ip: ip.map(|ip| ip.to_string()),
        };
        announce(state, event).await;
    }
}

/// Starts waiting for the second step (the `Auth` module's login does it).
pub(crate) fn begin(
    session: &Session,
    user: &User,
    email: &str,
    remember: bool,
    to: String,
) -> Result {
    let stored = Stored {
        user_id: user.id,
        email: email.to_owned(),
        remember,
        hash: fingerprint(&user.password),
        at: crate::clock::unix_secs(),
        to,
    };
    session.put(PENDING, stored)
}

/// Drops a login waiting for its second step (another login replaced it).
pub(crate) fn clear(session: &Session) {
    session.remove(PENDING);
}

/// The login waiting for its second step in this session, if there is one
/// and it is less than ten minutes old.
pub fn pending_login(session: &Session) -> Option<PendingLogin> {
    let stored: Stored = session.get(PENDING)?;
    if crate::clock::unix_secs() - stored.at > PENDING_FOR {
        session.remove(PENDING);
        return None;
    }
    Some(PendingLogin {
        user_id: stored.user_id,
        email: stored.email,
        remember: stored.remember,
        hash: stored.hash,
        to: stored.to,
    })
}

/// Logs the pending user in, once the second step is passed: the session
/// gets a new id, the password counts as just confirmed, the throttle is
/// cleared and `LoggedIn` fires, as after a plain login. Returns where to
/// send the user (the page that asked them to log in, else the `Auth`
/// module's `redirect_to`, else `home`), or `None` when the login can't
/// finish: the user is gone or changed their password meanwhile. Either way
/// the wait is over.
pub async fn complete_login(
    state: &AppState,
    session: &Session,
    pending: &PendingLogin,
    ip: Option<IpAddr>,
) -> Result<Option<String>> {
    session.remove(PENDING);
    let Some(user) = User::find(&state.db, pending.user_id).await? else {
        return Ok(None);
    };
    if fingerprint(&user.password) != pending.hash {
        return Ok(None);
    }
    state.throttle.clear(&pending.email, ip).await;
    let remember = pending.remember.then_some(state.config.remember_lifetime);
    login(session, &user, remember)?;
    super::account::mark_confirmed(session)?;
    let event = LoggedIn {
        user_id: user.id,
        ip: ip.map(|ip| ip.to_string()),
    };
    announce(state, event).await;
    Ok(Some(pending.to.clone()))
}

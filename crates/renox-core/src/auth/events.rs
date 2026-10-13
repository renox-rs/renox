//! Events the built-in auth pages emit, e.g. for a welcome mail or an audit
//! log (`renox::audit` records them all). Listen like to any event:
//!
//! ```
//! # use renox::prelude::*;
//! use renox::auth::events::{LoggedIn, Registered};
//!
//! # let _ =
//! App::new()
//!     .module(Auth::new())
//!     .listen(|e: Registered, state| async move {
//!         tracing::info!(user = e.user_id, "welcome!"); // e.g. queue a welcome mail
//!         let _ = state;
//!         Ok(())
//!     })
//!     .listen(|e: LoggedIn, _state| async move {
//!         tracing::info!(user = e.user_id, ip = ?e.ip, "logged in");
//!         Ok(())
//!     })
//! # ;
//! ```
//!
//! A listener's error is logged; it never fails the login itself. Your own
//! login code (`auth::login`) emits nothing: emit these yourself if you need
//! them.

use crate::AppState;
use crate::events::Event;

/// A new account, after `on_registered` succeeded.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Registered {
    /// The new user's id.
    pub user_id: i64,
    /// The new user's email address (normalized).
    pub email: String,
}

/// A successful login on the login page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoggedIn {
    /// The user who logged in.
    pub user_id: i64,
    /// The client's IP address (`ClientIp`), if known.
    pub ip: Option<String>,
}

/// A wrong email or password on the login page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoginFailed {
    /// The email address that was entered.
    pub email: String,
    /// The client's IP address (`ClientIp`), if known.
    pub ip: Option<String>,
}

/// Too many failed logins: this email (or address) must wait.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LockedOut {
    /// The email address that was entered.
    pub email: String,
    /// The client's IP address (`ClientIp`), if known.
    pub ip: Option<String>,
    /// Seconds left until another attempt is allowed.
    pub seconds: u64,
}

/// A logout of one device.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoggedOut {
    /// The user who logged out.
    pub user_id: i64,
}

/// A password chosen through a reset link.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PasswordReset {
    /// The user whose password was reset.
    pub user_id: i64,
}

/// A password changed on the account page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PasswordChanged {
    /// The user whose password changed.
    pub user_id: i64,
}

/// An email address confirmed through its link.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmailVerified {
    /// The user whose email address was confirmed.
    pub user_id: i64,
}

/// Name or email changed on the account page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProfileUpdated {
    /// The user whose profile changed.
    pub user_id: i64,
    /// Whether the email address changed.
    pub email_changed: bool,
}

/// "Log out other devices" on the account page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OtherDevicesLoggedOut {
    /// The user whose other sessions were ended.
    pub user_id: i64,
}

/// An account deleted on the account page (the row is already gone).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AccountDeleted {
    /// The deleted user's id (no longer in `users`).
    pub user_id: i64,
    /// The deleted user's email address.
    pub email: String,
}

impl Event for Registered {}
impl Event for LoggedIn {}
impl Event for LoginFailed {}
impl Event for LockedOut {}
impl Event for LoggedOut {}
impl Event for PasswordReset {}
impl Event for PasswordChanged {}
impl Event for EmailVerified {}
impl Event for ProfileUpdated {}
impl Event for OtherDevicesLoggedOut {}
impl Event for AccountDeleted {}

/// Emits `event`; a listener's failure is logged, not returned.
pub(crate) async fn announce<E: Event>(state: &AppState, event: E) {
    if let Err(err) = state.emit(event).await {
        tracing::error!(event = std::any::type_name::<E>(), error = ?err, "auth listener failed");
    }
}

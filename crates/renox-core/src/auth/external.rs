//! Logging in some other way than with the password on `/login`, such as a
//! social login (the `renox-oauth` crate): once a module has proved who the
//! visitor is, [`sign_in`] logs them in as the login page would (the second
//! login step included), [`register_verified`] makes an account for an
//! email address another service vouched for, and [`confirm_identity`]
//! counts as typing the password at `/confirm-password`.
//!
//! ```
//! use renox::prelude::*;
//! use std::net::IpAddr;
//!
//! /// Called once the visitor proved they own `email` (say, with a link
//! /// sent to it); returns where to send their browser.
//! async fn proved(
//!     state: &AppState,
//!     session: &Session,
//!     ip: Option<IpAddr>,
//!     email: &str,
//! ) -> Result<String> {
//!     let user = match User::find_by_email(&state.db, email).await? {
//!         Some(user) => user,
//!         None if renox::auth::registration_open(state) => {
//!             renox::auth::register_verified(state, "", email, &[("via", "link")]).await?
//!         }
//!         None => return Err(Error::Forbidden),
//!     };
//!     // The page they were going to, or the second login step's challenge.
//!     renox::auth::sign_in(state, session, &user, false, ip).await
//! }
//! ```
//!
//! A user made by [`register_verified`] has no password: `password` is
//! empty, which no typed password matches ([`User::has_password`] is
//! `false`). They set one with "Forgot your password?", whose link goes to
//! their (verified) address.

use std::net::IpAddr;

use serde_json::Value;

use super::events::{LoggedIn, Registered, announce};
use super::module::{Registration, after_login};
use super::{User, intended, login};
use crate::db::Model;
use crate::{AppState, Error, Result, Session};

/// Whether visitors may make an account: the app has the `Auth` module,
/// without `without_registration`.
pub fn registration_open(state: &AppState) -> bool {
    state
        .auth
        .as_ref()
        .is_some_and(|settings| settings.registration)
}

/// Makes an account for `email`, which another service verified (a social
/// login): no password (see [`User::has_password`]), the address counted as
/// verified, then the `Auth` module's `on_registered` hook, which reads
/// `name`, `email` and `fields` from its [`Registration`] (the registration
/// form's own rules don't run: there was no form). `name` falls back to the
/// part of the address before `@`. `Registered` fires as after `/register`.
///
/// Fails with `Error::Forbidden` when registration is closed
/// ([`registration_open`]), and with a unique violation when the address
/// already has an account. If the hook fails, the user is deleted again.
pub async fn register_verified(
    state: &AppState,
    name: &str,
    email: &str,
    fields: &[(&str, &str)],
) -> Result<User> {
    let Some(settings) = state.auth.clone().filter(|s| s.registration) else {
        return Err(Error::Forbidden);
    };
    let email = super::user::normalize_email(email);
    let name = match name.trim() {
        "" => email.split('@').next().unwrap_or_default().to_owned(),
        name => name.to_owned(),
    };
    let user = User {
        name: name.clone(),
        email: email.clone(),
        password: String::new(),
        email_verified_at: Some(crate::db::now()),
        ..User::default()
    };
    let id = User::create(&state.db, user).await?.id;
    if let Some(hook) = &settings.on_registered {
        let mut map = serde_json::Map::new();
        map.insert("name".into(), Value::String(name));
        map.insert("email".into(), Value::String(email.clone()));
        for (key, value) in fields {
            map.insert((*key).to_owned(), Value::String((*value).to_owned()));
        }
        let user = User::find_or_404(&state.db, id).await?;
        if let Err(err) = hook(state.clone(), user, Registration { fields: map }).await {
            // Undo the sign-up, so the visitor can try again.
            crate::db::sql("DELETE FROM users WHERE id = ?")
                .bind(id)
                .execute(&state.db)
                .await?;
            return Err(err);
        }
    }
    let user = User::find_or_404(&state.db, id).await?;
    let event = Registered {
        user_id: user.id,
        email: user.email.clone(),
    };
    announce(state, event).await;
    Ok(user)
}

/// Logs `user` in as the login page does after the right password, and
/// returns where to send the browser. When a module's second login step
/// (`Registry::second_factor`) applies to the user, the login waits for it
/// and this is its challenge's URL ([`pending_login`](super::pending_login)
/// is then `Some`); otherwise the session is logged in
/// (for the "remember me" lifetime when `remember`), the identity counts as
/// just confirmed, `LoggedIn` fires, and this is the page that asked for a
/// login, else the `Auth` module's `redirect_to`, else `home` or `/`.
pub async fn sign_in(
    state: &AppState,
    session: &Session,
    user: &User,
    remember: bool,
    ip: Option<IpAddr>,
) -> Result<String> {
    let to = match &state.auth {
        Some(settings) => after_login(state, settings, session),
        None => intended(
            session,
            state.url("home", &[]).unwrap_or_else(|_| "/".into()),
        ),
    };
    if let Some(second) = &state.second_factor
        && (second.required)(user.clone(), state.clone()).await?
    {
        super::second_factor::begin(session, user, &user.email, remember, to)?;
        return state.url(&second.challenge, &[]);
    }
    // A login that waited for its second step is over: this one replaces it.
    super::second_factor::clear(session);
    let lifetime = remember.then_some(state.config.remember_lifetime);
    login(session, user, lifetime)?;
    super::account::mark_confirmed(session)?;
    let event = LoggedIn {
        user_id: user.id,
        ip: ip.map(|ip| ip.to_string()),
    };
    announce(state, event).await;
    Ok(to)
}

/// Counts as typing the password at `/confirm-password` (for routes behind
/// `require_password_confirmed`), once the logged-in user proved who they
/// are another way, e.g. with a provider their account is linked to.
/// Returns where to go: the page that asked for the confirmation, else `/`.
pub fn confirm_identity(session: &Session) -> Result<String> {
    super::account::mark_confirmed(session)?;
    Ok(super::account::confirmed_destination(session))
}

//! `Redirect::route` and `Redirect::intended`, like Laravel's
//! `redirect()->route(…)` and `redirect()->intended(…)`.

use std::fmt::Display;

use axum::response::Redirect;

use crate::{Error, Result, Session};

/// Named-route and intended-page redirects on axum's `Redirect` (in the
/// prelude):
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(session: Session, id: i64) -> Result<Redirect> {
/// // To a named route, with its parameters:
/// let to_product = Redirect::route("products.show", &[&id])?;
/// // Where a guard sent the user from (e.g. before logging in), else /dashboard:
/// let back = Redirect::intended(&session, "/dashboard");
/// # let _ = back; Ok(to_product) }
/// ```
///
/// The trait is sealed: it's implemented for `Redirect` only, so it can grow.
pub trait RedirectExt: sealed::Sealed + Sized {
    /// A 303 redirect to the named route `name` with `params` filling its
    /// `{…}` placeholders in order. Needs the app, so it works in handlers,
    /// middleware, jobs and commands (not in code outside any of them).
    fn route(name: &str, params: &[&dyn Display]) -> Result<Self>;

    /// A 303 redirect to the page a guard stored before sending the user
    /// away (`require_auth` keeps the page that asked for a login), or to
    /// `fallback`. The stored page is used once and only if it's on this
    /// site.
    fn intended(session: &Session, fallback: &str) -> Self;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for axum::response::Redirect {}
}

impl RedirectExt for Redirect {
    fn route(name: &str, params: &[&dyn Display]) -> Result<Self> {
        let state = crate::context::app().ok_or_else(|| {
            Error::Internal(anyhow::anyhow!(
                "Redirect::route(\"{name}\") needs the app: call it in a handler, job or command"
            ))
        })?;
        Ok(Redirect::to(&state.url(name, params)?))
    }

    fn intended(session: &Session, fallback: &str) -> Self {
        Redirect::to(&crate::auth::intended(session, fallback.to_owned()))
    }
}

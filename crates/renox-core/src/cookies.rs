//! The app's own cookies: `Cookies` and `SetCookie`.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::FromRequestParts;
use axum::http::HeaderValue;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::response::{IntoResponseParts, ResponseParts};
use cookie::{Cookie, CookieJar, Key, SameSite};

use crate::AppState;

/// The app's own cookies (the session has its own). Read them with the
/// `Cookies` extractor; set them by returning a `SetCookie` with the
/// response.
///
/// ```
/// # use renox::prelude::*;
/// use std::time::Duration;
/// use renox::{Cookies, SetCookie};
///
/// async fn page(cookies: Cookies) -> View {
///     let theme = cookies.get("theme").unwrap_or_else(|| "light".into());
///     let referral = cookies.get_encrypted("ref"); // set with SetCookie::encrypted
///     view("page.html", context! { theme, referral })
/// }
///
/// async fn pick_theme(State(state): State<AppState>, Path(theme): Path<String>) -> (SetCookie, Redirect) {
///     let cookie = SetCookie::new(&state, "theme", theme).max_age(Duration::from_secs(365 * 86_400));
///     (cookie, Redirect::to("/"))
/// }
///
/// async fn forget(State(state): State<AppState>) -> (SetCookie, Redirect) {
///     (SetCookie::remove(&state, "theme"), Redirect::to("/"))
/// }
/// ```
///
/// `Cookies` holds the cookies a request came with.
#[derive(Clone)]
pub struct Cookies {
    jar: CookieJar,
    key: Key,
}

impl Cookies {
    /// A cookie's value as the browser sent it.
    pub fn get(&self, name: &str) -> Option<String> {
        self.jar.get(name).map(|c| c.value().to_owned())
    }

    /// A cookie set with `SetCookie::encrypted`; `None` if it's missing or
    /// was changed or made by anyone without `APP_KEY`.
    pub fn get_encrypted(&self, name: &str) -> Option<String> {
        self.jar
            .private(&self.key)
            .get(name)
            .map(|c| c.value().to_owned())
    }
}

impl FromRequestParts<AppState> for Cookies {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Infallible> {
        let mut jar = CookieJar::new();
        for header in parts.headers.get_all(COOKIE) {
            let Ok(header) = header.to_str() else {
                continue;
            };
            for cookie in Cookie::split_parse_encoded(header.to_owned()).flatten() {
                jar.add_original(cookie);
            }
        }
        Ok(Self {
            jar,
            key: state.key.clone(),
        })
    }
}

/// A cookie to set, returned with the response: `(SetCookie::new(…), view)`.
/// By default it's for the whole site (`Path=/`), not readable by
/// JavaScript (`HttpOnly`), `SameSite=Lax`, `Secure` when `APP_URL` is
/// https, and lasts until the browser closes (see `max_age`).
#[derive(Debug, Clone)]
pub struct SetCookie(Cookie<'static>);

impl SetCookie {
    /// A cookie `name` with `value` and the defaults above.
    pub fn new(state: &AppState, name: impl Into<String>, value: impl Into<String>) -> Self {
        let mut cookie = Cookie::new(name.into(), value.into());
        cookie.set_path("/");
        cookie.set_http_only(true);
        cookie.set_same_site(SameSite::Lax);
        cookie.set_secure(state.config.url.starts_with("https://"));
        Self(cookie)
    }

    /// Encrypted and signed with `APP_KEY`: the browser can't read or change
    /// it. Read it back with `Cookies::get_encrypted`.
    pub fn encrypted(state: &AppState, name: impl Into<String>, value: impl Into<String>) -> Self {
        let plain = Self::new(state, name, value).0;
        let mut jar = CookieJar::new();
        jar.private_mut(&state.key).add(plain.clone());
        let sealed = jar.get(plain.name()).cloned().unwrap_or(plain);
        Self(sealed)
    }

    /// Deletes the cookie `name` in the browser.
    pub fn remove(state: &AppState, name: impl Into<String>) -> Self {
        let mut cookie = Self::new(state, name, "").0;
        cookie.make_removal();
        Self(cookie)
    }

    /// How long the cookie lasts.
    pub fn max_age(mut self, age: Duration) -> Self {
        let seconds = i64::try_from(age.as_secs()).unwrap_or(i64::MAX);
        self.0.set_max_age(cookie::time::Duration::seconds(seconds));
        self
    }

    /// Limits the cookie to URLs under `path` (default `/`).
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.0.set_path(path.into());
        self
    }

    /// Lets JavaScript read it (`HttpOnly` off), e.g. a UI preference that
    /// a script applies.
    pub fn readable_by_scripts(mut self) -> Self {
        self.0.set_http_only(false);
        self
    }

    /// `SameSite=Strict`: not sent when arriving from another site.
    pub fn strict(mut self) -> Self {
        self.0.set_same_site(SameSite::Strict);
        self
    }
}

impl IntoResponseParts for SetCookie {
    type Error = Infallible;

    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Infallible> {
        if let Ok(value) = HeaderValue::from_str(&self.0.encoded().to_string()) {
            res.headers_mut().append(SET_COOKIE, value);
        }
        Ok(res)
    }
}

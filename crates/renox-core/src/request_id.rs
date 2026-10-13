//! Request ids: one per request, in the logs, the response and error reports.

use std::convert::Infallible;

use axum::extract::{FromRequestParts, Request};
use axum::http::HeaderValue;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;

pub(crate) const HEADER: &str = "x-request-id";

/// The request's id: the `X-Request-Id` a proxy sent (when it looks safe:
/// 8–64 letters, digits, `.`, `_` or `-`), otherwise a new random one. It's
/// in every log line of the request, in the response's `X-Request-Id`, and
/// in error reports, so a visitor's "request id" leads to the logs.
///
/// ```
/// # use renox::prelude::*;
/// use renox::RequestId;
///
/// async fn show(id: RequestId) -> String {
///     format!("Quote this if something went wrong: {id}")
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestId {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(parts
            .extensions
            .get::<RequestId>()
            .cloned()
            .unwrap_or_else(|| RequestId(String::new())))
    }
}

fn acceptable(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Outermost: picks the id and puts it on the request (for the log span)
/// and on the response.
pub(crate) async fn middleware(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get(HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|id| acceptable(id))
        .map_or_else(|| crate::random_token()[..20].to_owned(), str::to_owned);
    if let Ok(value) = HeaderValue::from_str(&id) {
        req.headers_mut().insert(HEADER, value.clone());
        req.extensions_mut().insert(RequestId(id));
        let mut res = next.run(req).await;
        res.headers_mut().insert(HEADER, value);
        return res;
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn accepts_only_safe_ids() {
        assert!(super::acceptable("abcd-1234_ef.gh"));
        assert!(!super::acceptable("short"));
        assert!(!super::acceptable("has space in it"));
        assert!(!super::acceptable("new\nline-injected"));
        assert!(!super::acceptable(&"a".repeat(65)));
    }
}

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::{Method, header::CONTENT_TYPE};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::crypto::constant_time_eq;
use crate::{Error, Session};

/// The request header that carries the CSRF token (htmx and `fetch` requests).
pub const CSRF_HEADER: &str = "x-csrf-token";
/// The cookie that carries the CSRF token to JavaScript clients, with
/// [`App::xsrf_cookie`](crate::App::xsrf_cookie).
pub const XSRF_COOKIE: &str = "XSRF-TOKEN";
/// The header such clients send it back in (axios does it by itself).
pub const XSRF_HEADER: &str = "x-xsrf-token";
/// The form field that carries the CSRF token.
pub const CSRF_FIELD: &str = "_token";

/// Finds the text field `name` in a buffered multipart body. The body was
/// already read within the app's `UPLOAD_MAX_SIZE`, so no other limit
/// applies here (axum's `Multipart` would add its own 2 MB one).
pub(crate) async fn multipart_field(
    headers: &axum::http::HeaderMap,
    bytes: axum::body::Bytes,
    name: &str,
) -> Option<String> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)?
        .to_str()
        .ok()?;
    let boundary = multer::parse_boundary(content_type).ok()?;
    let body = futures_util::stream::once(async move { Ok::<_, std::io::Error>(bytes) });
    let mut multipart = multer::Multipart::new(body, boundary);
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some(name) {
            return field.text().await.ok();
        }
    }
    None
}

/// Largest urlencoded body the CSRF check buffers to look for `_token`.
pub(crate) const FORM_LIMIT: usize = 2 * 1024 * 1024;

/// Rejects state-changing requests that don't carry the session's CSRF token,
/// either in the `X-CSRF-Token` header (sent automatically for HTMX requests)
/// or in a `_token` field of a urlencoded or multipart form (`{{ csrf_field() }}`).
pub(crate) async fn middleware(req: Request, next: Next) -> Response {
    let xsrf = req
        .extensions()
        .get::<crate::AppState>()
        .filter(|state| state.security.xsrf_cookie)
        .and_then(|state| {
            let session = req.extensions().get::<Session>()?.clone();
            Some((session, state.config.url.starts_with("https://")))
        });
    let mut res = check(req, next).await;
    // The token as it is after the handler (a login issues a new one).
    if let Some((session, secure)) = xsrf {
        let cookie = cookie::Cookie::build((XSRF_COOKIE, session.token()))
            .path("/")
            .same_site(cookie::SameSite::Lax)
            .secure(secure)
            .build();
        if let Ok(value) = axum::http::HeaderValue::from_str(&cookie.to_string()) {
            res.headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    res
}

async fn check(req: Request, next: Next) -> Response {
    if matches!(
        *req.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    ) {
        return next.run(req).await;
    }

    // API tokens are sent explicitly, not by the browser, so they can't be forged cross-site.
    if crate::auth::user_via_token(req.extensions()) {
        return next.run(req).await;
    }
    // A Bearer token that didn't authenticate: the caller is an API client
    // with bad credentials, so say that (401) instead of asking for a CSRF
    // token. Either way the request goes no further.
    if req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer "))
    {
        return Error::Unauthorized.into_response();
    }

    // Routes marked `without_csrf()`, e.g. webhooks, which verify signatures instead.
    let exempt = req
        .extensions()
        .get::<crate::AppState>()
        .is_some_and(|state| {
            state.security.skips_csrf(
                req.method(),
                req.extensions().get::<axum::extract::MatchedPath>(),
            )
        });
    if exempt {
        return next.run(req).await;
    }

    let Some(session) = req.extensions().get::<Session>().cloned() else {
        return Error::from(anyhow::anyhow!(
            "CSRF protection requires the session middleware"
        ))
        .into_response();
    };
    let expected = session.token();

    if let Some(token) = req
        .headers()
        .get(CSRF_HEADER)
        .or_else(|| req.headers().get(XSRF_HEADER))
    {
        return match token.to_str() {
            Ok(token) if constant_time_eq(token, &expected) => next.run(req).await,
            _ => Error::PageExpired.into_response(),
        };
    }

    let content_type = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let multipart = content_type.starts_with("multipart/form-data");
    if !multipart && !content_type.starts_with("application/x-www-form-urlencoded") {
        return Error::PageExpired.into_response();
    }
    let limit = if multipart {
        req.extensions()
            .get::<crate::AppState>()
            .map_or(FORM_LIMIT, |state| state.config.upload_max_size)
    } else {
        FORM_LIMIT
    };

    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, limit).await {
        Ok(bytes) => bytes,
        Err(_) => return Error::BadRequest("The form is too large.".into()).into_response(),
    };
    let token = if multipart {
        multipart_field(&parts.headers, bytes.clone(), CSRF_FIELD).await
    } else {
        form_urlencoded::parse(&bytes)
            .find(|(name, _)| name == CSRF_FIELD)
            .map(|(_, token)| token.into_owned())
    };
    let valid = token.is_some_and(|token| constant_time_eq(&token, &expected));
    if !valid {
        return Error::PageExpired.into_response();
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    /// The CSRF check placed outside the session layer (a layer added in
    /// the wrong place) says so, instead of letting the request through.
    #[tokio::test]
    async fn without_the_session_layer_a_post_is_a_500() {
        let app = axum::Router::new()
            .route("/", axum::routing::post(|| async { "posted" }))
            .layer(axum::middleware::from_fn(super::middleware));
        let req = Request::builder()
            .method("POST")
            .uri("/")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("_token=x"))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}

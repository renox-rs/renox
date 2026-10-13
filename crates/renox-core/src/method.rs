//! Method spoofing, as in Laravel: HTML forms can only send GET and POST, so a
//! POST carrying `_method=PUT|PATCH|DELETE` (a form field, or the
//! `X-HTTP-Method-Override` header) is routed as that method.
//!
//! ```html
//! <form method="post" action="{{ route('products.update', product.id) }}">
//!   {{ csrf_field() }}{{ method_field('PUT') }}
//! ```
//!
//! This runs in front of the router (route middleware runs after the method
//! has been matched). HTMX can also send the real method (`hx-put`).

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The form field that overrides a POST's method (`PUT`, `PATCH` or `DELETE`).
pub const METHOD_FIELD: &str = "_method";
const METHOD_HEADER: &str = "x-http-method-override";

fn spoofable(value: &str) -> Option<Method> {
    match value.trim().to_ascii_uppercase().as_str() {
        "PUT" => Some(Method::PUT),
        "PATCH" => Some(Method::PATCH),
        "DELETE" => Some(Method::DELETE),
        _ => None,
    }
}

pub(crate) async fn middleware(mut req: Request, next: Next, upload_limit: usize) -> Response {
    if req.method() != Method::POST {
        return next.run(req).await;
    }
    if let Some(method) = header(req.headers()).and_then(|v| spoofable(&v)) {
        *req.method_mut() = method;
        return next.run(req).await;
    }
    let content_type = header_value(req.headers(), CONTENT_TYPE.as_str()).unwrap_or_default();
    let multipart = content_type.starts_with("multipart/form-data");
    if !multipart && !content_type.starts_with("application/x-www-form-urlencoded") {
        return next.run(req).await;
    }

    let limit = if multipart {
        upload_limit
    } else {
        crate::csrf::FORM_LIMIT
    };
    let (mut parts, body) = req.into_parts();
    let Ok(bytes) = to_bytes(body, limit).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let value = if multipart {
        crate::csrf::multipart_field(&parts.headers, bytes.clone(), METHOD_FIELD).await
    } else {
        form_urlencoded::parse(&bytes)
            .find(|(name, _)| name == METHOD_FIELD)
            .map(|(_, value)| value.into_owned())
    };
    if let Some(method) = value.as_deref().and_then(spoofable) {
        parts.method = method;
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

fn header(headers: &HeaderMap) -> Option<String> {
    header_value(headers, METHOD_HEADER)
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

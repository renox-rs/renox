//! Security headers on every response: a Content-Security-Policy (see
//! [`CspMode`], `CSP` in `.env`),
//! `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options`, a
//! `Permissions-Policy` (see [`PermissionsPolicy`], `PERMISSIONS_POLICY` in
//! `.env`), and HSTS in production over https. A header the handler already
//! set is kept.
//!
//! Allow another site's scripts, images or frames with [`App::csp`](crate::App::csp):
//!
//! ```
//! # use renox::prelude::*;
//! # let _ =
//! App::new().csp(|csp| {
//!     csp.allow("script-src", "https://www.googletagmanager.com")
//!        .allow("frame-src", "https://www.youtube.com");
//! })
//! # ;
//! ```
//!
//! With `CSP=strict`, inline scripts need the request's nonce:
//! `<script nonce="{{ csp_nonce() }}">…</script>`.
//!
//! The `Permissions-Policy` switches off the browser features a typical app
//! never uses (camera, microphone, geolocation, payment, USB…). Allow the one
//! your app needs with [`App::permissions_policy`](crate::App::permissions_policy).

use std::collections::{BTreeMap, HashSet};
use std::convert::Infallible;

use axum::extract::{FromRequestParts, MatchedPath, Request, State};
use axum::http::header::{
    CONTENT_SECURITY_POLICY, REFERRER_POLICY, STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS,
    X_FRAME_OPTIONS,
};
use axum::http::request::Parts;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::config::{Config, CspMode, Environment, PermissionsMode};
use crate::routing::RouteInfo;

/// Sources an app adds to the Content-Security-Policy, per directive.
#[derive(Debug, Clone, Default)]
pub struct Csp {
    extra: BTreeMap<String, Vec<String>>,
}

impl Csp {
    /// Allows `source` (e.g. `https://www.googletagmanager.com`) for
    /// `directive` (e.g. `script-src`, `img-src`, `connect-src`, `frame-src`).
    pub fn allow(&mut self, directive: &str, source: &str) -> &mut Self {
        self.extra
            .entry(directive.trim().to_ascii_lowercase())
            .or_default()
            .push(source.trim().to_owned());
        self
    }
}

/// The features the default `Permissions-Policy` switches off. The framework's
/// own scripts use none of them: `navigator.clipboard.writeText` (the copy
/// buttons) is allowed for the page's own origin by the browsers' default,
/// which this list leaves alone, and neither fullscreen, the date picker, file
/// inputs nor the editors need a gated feature.
const DEFAULT_OFF: &[&str] = &[
    "accelerometer",
    "autoplay",
    "bluetooth",
    "camera",
    "display-capture",
    "geolocation",
    "gyroscope",
    "hid",
    "magnetometer",
    "microphone",
    "midi",
    "payment",
    "serial",
    "usb",
    "xr-spatial-tracking",
];

/// Features an app allows on top of the `Permissions-Policy`, from
/// [`App::permissions_policy`](crate::App::permissions_policy).
///
/// The default policy switches off the browser features a typical app never
/// uses (`camera`, `microphone`, `geolocation`, `payment`, `usb`…); each
/// [`allow`](PermissionsPolicy::allow) turns one back on, for the site itself,
/// for another origin or for everyone.
///
/// ```
/// # use renox::prelude::*;
/// # let _ =
/// App::new().permissions_policy(|p| {
///     p.allow("geolocation", "self")
///         .allow("payment", "https://pay.example.com");
/// })
/// # ;
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PermissionsPolicy {
    extra: Vec<(String, String)>,
}

impl PermissionsPolicy {
    /// Allows `feature` (e.g. `geolocation`, `camera`) for `origin`: `self`
    /// (this site), `*` (everyone) or an origin such as
    /// `https://pay.example.com`. Calling it again adds more origins. A name
    /// or origin the header cannot carry makes the app fail at boot.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let mut p = renox::security::PermissionsPolicy::default();
    /// p.allow("camera", "self").allow("camera", "https://scan.example.com");
    /// ```
    pub fn allow(&mut self, feature: &str, origin: &str) -> &mut Self {
        self.extra.push((
            feature.trim().to_ascii_lowercase(),
            origin.trim().to_owned(),
        ));
        self
    }
}

/// One feature of the policy with its allowlist, tokens as they are written
/// in the header (`self`, `*`, `"https://x"`).
type Feature = (String, Vec<String>);

fn check_feature(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        anyhow::bail!("`{name}` is not a feature name (lowercase letters, digits and dashes)");
    }
    Ok(())
}

/// An origin as the header writes it: `self` and `*` bare, the rest quoted.
fn origin_token(origin: &str) -> anyhow::Result<String> {
    let origin = origin.trim().trim_matches('"');
    match origin {
        "self" | "*" | "src" => Ok(origin.to_owned()),
        _ if origin.is_empty()
            || origin
                .bytes()
                .any(|b| b.is_ascii_control() || b" \"(),;".contains(&b)) =>
        {
            anyhow::bail!("`{origin}` is not an origin (`self`, `*` or like `https://x.example`)")
        }
        _ => Ok(format!("\"{origin}\"")),
    }
}

/// Reads a full header value (`geolocation=(self), camera=()`).
pub(crate) fn parse_permissions_policy(value: &str) -> anyhow::Result<Vec<Feature>> {
    let mut features: Vec<Feature> = Vec::new();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, list) = part
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("`{part}` should look like `feature=(self)`"))?;
        let name = name.trim();
        check_feature(name)?;
        let list = list.trim();
        let inner = match list.strip_prefix('(') {
            Some(rest) => rest
                .strip_suffix(')')
                .ok_or_else(|| anyhow::anyhow!("`{part}` is missing a `)`"))?,
            None => list,
        };
        let tokens = inner
            .split_whitespace()
            .map(|token| {
                if !matches!(token, "self" | "*" | "src") && !token.starts_with('"') {
                    anyhow::bail!("`{token}` needs quotes: `\"{token}\"`");
                }
                origin_token(token)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        features.push((name.to_owned(), tokens));
    }
    if features.is_empty() {
        anyhow::bail!("it names no feature");
    }
    Ok(features)
}

/// The header value, or `None` when the policy is off.
fn permissions_header(
    mode: &PermissionsMode,
    app: &PermissionsPolicy,
) -> anyhow::Result<Option<String>> {
    let mut features: Vec<Feature> = match mode {
        PermissionsMode::Off => return Ok(None),
        PermissionsMode::Default => DEFAULT_OFF
            .iter()
            .map(|name| ((*name).to_owned(), Vec::new()))
            .collect(),
        PermissionsMode::Custom(value) => parse_permissions_policy(value)?,
    };
    for (name, origin) in &app.extra {
        check_feature(name).map_err(|e| anyhow::anyhow!("App::permissions_policy: {e}"))?;
        let token = origin_token(origin)
            .map_err(|e| anyhow::anyhow!("App::permissions_policy({name}): {e}"))?;
        let at = match features.iter().position(|(n, _)| n == name) {
            Some(at) => at,
            None => {
                features.push((name.clone(), Vec::new()));
                features.len() - 1
            }
        };
        if !features[at].1.contains(&token) {
            features[at].1.push(token);
        }
    }
    Ok(Some(
        features
            .iter()
            .map(|(name, tokens)| {
                if tokens.iter().any(|t| t == "*") {
                    format!("{name}=*")
                } else {
                    format!("{name}=({})", tokens.join(" "))
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
    ))
}

/// The request's CSP nonce, also `csp_nonce()` in templates.
#[derive(Debug, Clone)]
pub struct CspNonce(pub String);

impl<S: Send + Sync> FromRequestParts<S> for CspNonce {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(parts
            .extensions
            .get::<CspNonce>()
            .cloned()
            .unwrap_or(CspNonce(String::new())))
    }
}

const NONCE: &str = "{nonce}";

/// What the middleware adds, worked out once at boot.
#[derive(Debug)]
pub(crate) struct Security {
    pub mode: CspMode,
    /// The policy, with `{nonce}` where each request's nonce goes.
    policy: Option<String>,
    /// The `Permissions-Policy` value; `None` when `PERMISSIONS_POLICY=off`.
    permissions: Option<String>,
    hsts: bool,
    /// (method, path pattern) of routes marked `without_csrf()`; method `*` is any.
    csrf_exempt: HashSet<(String, String)>,
    /// Path patterns of webhook routes, which work in maintenance mode.
    webhook_paths: HashSet<String>,
    /// `App::xsrf_cookie`: the CSRF token also goes out as `XSRF-TOKEN`.
    pub xsrf_cookie: bool,
    /// `TRUSTED_HOSTS` plus `APP_URL`'s host; empty answers any host.
    trusted_hosts: Vec<String>,
}

impl Security {
    pub fn new(
        config: &Config,
        csp: &Csp,
        permissions: &PermissionsPolicy,
        routes: &[RouteInfo],
    ) -> anyhow::Result<Self> {
        let permissions = permissions_header(&config.permissions_policy, permissions)?;
        let script = match config.csp {
            CspMode::Strict => format!("'self' 'nonce-{NONCE}'"),
            _ => "'self' 'unsafe-inline' 'unsafe-eval'".to_owned(),
        };
        let mut directives: Vec<(String, String)> = [
            ("default-src", "'self'".to_owned()),
            ("script-src", script),
            ("style-src", "'self' 'unsafe-inline'".to_owned()),
            ("img-src", "'self' data: blob: https:".to_owned()),
            ("font-src", "'self' data: https:".to_owned()),
            ("connect-src", "'self'".to_owned()),
            ("frame-ancestors", "'self'".to_owned()),
            ("base-uri", "'self'".to_owned()),
            ("object-src", "'none'".to_owned()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        let mut extra = csp.extra.clone();
        for (directive, source) in crate::seo::csp_sources(config) {
            extra
                .entry(directive.to_owned())
                .or_default()
                .push(source.to_owned());
        }
        for (name, sources) in &extra {
            let sources = sources.join(" ");
            match directives.iter_mut().find(|(n, _)| n == name) {
                Some((_, value)) => {
                    value.push(' ');
                    value.push_str(&sources);
                }
                // A new directive would otherwise stop falling back to
                // default-src, so it keeps this site too.
                None => directives.push((name.clone(), format!("'self' {sources}"))),
            }
        }
        let policy = (config.csp != CspMode::Off).then(|| {
            directives
                .iter()
                .map(|(name, value)| format!("{name} {value}"))
                .collect::<Vec<_>>()
                .join("; ")
        });
        let csrf_exempt = routes
            .iter()
            .filter(|route| route.middleware.iter().any(|m| m == "no-csrf"))
            // A resource's update route is listed as `PUT|PATCH`.
            .flat_map(|route| {
                route
                    .method
                    .split('|')
                    .map(|method| (method.to_owned(), route.path.clone()))
            })
            .collect();
        let webhook_paths = routes
            .iter()
            .filter(|route| route.middleware.iter().any(|m| m.starts_with("webhook:")))
            .map(|route| route.path.clone())
            .collect();
        Ok(Self {
            webhook_paths,
            mode: config.csp,
            policy,
            permissions,
            hsts: config.env == Environment::Production && config.url.starts_with("https://"),
            csrf_exempt,
            xsrf_cookie: false,
            trusted_hosts: trusted_hosts(config),
        })
    }

    /// Whether the app answers requests for `host` (`TRUSTED_HOSTS`).
    pub fn allows_host(&self, host: Option<&str>) -> bool {
        if self.trusted_hosts.is_empty() {
            return true;
        }
        let Some(host) = host.map(str::to_ascii_lowercase) else {
            return false;
        };
        self.trusted_hosts
            .iter()
            .any(|allowed| match allowed.strip_prefix("*.") {
                Some(parent) => host
                    .strip_suffix(parent)
                    .is_some_and(|sub| sub.len() > 1 && sub.ends_with('.')),
                None => *allowed == host,
            })
    }

    /// Whether the matched route receives webhooks.
    pub fn is_webhook(&self, path: Option<&MatchedPath>) -> bool {
        path.is_some_and(|path| self.webhook_paths.contains(path.as_str()))
    }

    /// Whether the route `req` matched was marked `without_csrf()`.
    pub fn skips_csrf(&self, method: &Method, path: Option<&MatchedPath>) -> bool {
        let Some(path) = path else {
            return false;
        };
        let path = path.as_str().to_owned();
        self.csrf_exempt
            .contains(&(method.as_str().to_owned(), path.clone()))
            || self.csrf_exempt.contains(&("*".to_owned(), path))
    }
}

/// Marks a response from a `Routes::etag()` route.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WantsEtag;

/// The largest body `Routes::etag()` hashes.
const ETAG_LIMIT: u64 = 2 * 1024 * 1024;

/// Adds an `ETag` to a rendered page, or turns it into a 304 when the
/// browser's `If-None-Match` names it.
///
/// The request's CSP nonce is masked before hashing: it changes on every
/// request, so a page with scripts would otherwise never match (#306).
async fn etag(
    res: Response,
    method: &Method,
    if_none_match: Option<HeaderValue>,
    nonce: &str,
) -> Response {
    use axum::body::HttpBody as _;
    use axum::http::StatusCode;
    let wanted = res.extensions().get::<WantsEtag>().is_some()
        && matches!(*method, Method::GET | Method::HEAD)
        && res.status() == StatusCode::OK
        && !res.headers().contains_key(axum::http::header::ETAG)
        && res
            .body()
            .size_hint()
            .exact()
            .is_some_and(|size| size <= ETAG_LIMIT);
    if !wanted {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, ETAG_LIMIT as usize).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "could not read the page").into_response();
    };
    let hash = crate::webhook::sha256_hex(without(&bytes, nonce.as_bytes()));
    let tag = format!("\"{}\"", &hash[..32]);
    let matches = if_none_match
        .as_ref()
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(|t| t.trim().trim_start_matches("W/"))
                .any(|t| t == tag || t == "*")
        });
    if let Ok(value) = HeaderValue::from_str(&tag) {
        parts.headers.insert(axum::http::header::ETAG, value);
    }
    if matches {
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(axum::http::header::CONTENT_LENGTH);
        parts.headers.remove(axum::http::header::CONTENT_TYPE);
        return Response::from_parts(parts, axum::body::Body::empty());
    }
    Response::from_parts(parts, axum::body::Body::from(bytes))
}

/// `bytes` with every `needle` left out (all of `bytes` when `needle` is
/// empty).
fn without<'a>(bytes: &'a [u8], needle: &[u8]) -> std::borrow::Cow<'a, [u8]> {
    if needle.is_empty() || !bytes.windows(needle.len()).any(|w| w == needle) {
        return std::borrow::Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(at) = rest.windows(needle.len()).position(|w| w == needle) {
        out.extend_from_slice(&rest[..at]);
        rest = &rest[at + needle.len()..];
    }
    out.extend_from_slice(rest);
    std::borrow::Cow::Owned(out)
}

fn trusted_hosts(config: &Config) -> Vec<String> {
    let mut hosts = config.trusted_hosts.clone();
    if !hosts.is_empty()
        && let Ok(url) = config.url.parse::<axum::http::Uri>()
        && let Some(host) = url.host()
    {
        hosts.push(host.to_ascii_lowercase());
    }
    hosts
}

pub(crate) async fn middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    // A request for a host the app doesn't serve (`TRUSTED_HOSTS`): links
    // built from it, such as password reset mails, would point elsewhere.
    // Load balancers' health checks often use an IP, so `/health` answers.
    if req.uri().path() != "/health"
        && !state
            .security
            .allows_host(crate::domain::host(&req).as_deref())
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "This host is not served here.",
        )
            .into_response();
    }
    let client = crate::client_ip::resolve(&req, &state.config.trusted_proxies);
    req.extensions_mut().insert(client);
    let security = &state.security;
    let nonce = crate::crypto::random_token();
    req.extensions_mut().insert(CspNonce(nonce.clone()));
    let wants_json = crate::error::wants_json(req.headers());
    let method = req.method().clone();
    let if_none_match = req
        .headers()
        .get(axum::http::header::IF_NONE_MATCH)
        .cloned();
    let mut res = next.run(req).await;
    res = etag(res, &method, if_none_match, &nonce).await;
    // Errors from outside the view layer (e.g. CSRF's 419) for API clients.
    if wants_json && let Some(page) = res.extensions_mut().remove::<crate::error::ErrorPage>() {
        res = page.json(state.config.debug);
    }

    let not_modified = res.status() == axum::http::StatusCode::NOT_MODIFIED;
    let headers = res.headers_mut();
    let mut set = |name: HeaderName, value: &str| {
        if !headers.contains_key(&name)
            && let Ok(value) = HeaderValue::from_str(value)
        {
            headers.insert(name, value);
        }
    };
    set(X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(REFERRER_POLICY, "strict-origin-when-cross-origin");
    set(X_FRAME_OPTIONS, "SAMEORIGIN");
    if security.hsts {
        set(STRICT_TRANSPORT_SECURITY, "max-age=31536000");
    }
    if let Some(policy) = &security.permissions {
        set(HeaderName::from_static("permissions-policy"), policy);
    }
    // Not on a 304: the browser keeps the policy it stored with the page,
    // whose nonce is the one in that page.
    if let Some(policy) = &security.policy
        && !not_modified
    {
        set(CONTENT_SECURITY_POLICY, &policy.replace(NONCE, &nonce));
    }
    res
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use axum::body::{Body, Bytes, HttpBody};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    use super::*;

    fn header(mode: PermissionsMode, build: impl FnOnce(&mut PermissionsPolicy)) -> String {
        let mut app = PermissionsPolicy::default();
        build(&mut app);
        permissions_header(&mode, &app).unwrap().unwrap()
    }

    #[test]
    fn the_default_policy_switches_features_off() {
        let h = header(PermissionsMode::Default, |_| {});
        assert!(
            h.starts_with("accelerometer=(), autoplay=(), bluetooth=()"),
            "{h}"
        );
        assert!(
            h.contains("camera=()") && h.contains("xr-spatial-tracking=()"),
            "{h}"
        );
        assert!(!h.contains("clipboard") && !h.contains("fullscreen"), "{h}");
    }

    #[test]
    fn an_app_allows_features_back() {
        let h = header(PermissionsMode::Default, |p| {
            p.allow("geolocation", "self")
                .allow("payment", "https://pay.example.com")
                .allow("payment", "self")
                .allow("camera", "*")
                .allow("fullscreen", "self");
        });
        assert!(h.contains("geolocation=(self)"), "{h}");
        assert!(
            h.contains(r#"payment=("https://pay.example.com" self)"#),
            "{h}"
        );
        assert!(h.contains("camera=*"), "{h}");
        assert!(h.ends_with("fullscreen=(self)"), "{h}");
    }

    #[test]
    fn a_custom_value_is_the_base_and_off_is_none() {
        let mode = PermissionsMode::Custom("geolocation=(self), camera=()".into());
        assert_eq!(
            header(mode.clone(), |_| {}),
            "geolocation=(self), camera=()"
        );
        assert_eq!(
            header(mode, |p| {
                p.allow("camera", "self");
            }),
            "geolocation=(self), camera=(self)"
        );
        assert!(
            permissions_header(&PermissionsMode::Off, &PermissionsPolicy::default())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_bad_feature_or_origin_is_an_error() {
        for (feature, origin) in [
            ("Bad Name", "self"),
            ("camera", "two words"),
            ("camera", ""),
        ] {
            let mut app = PermissionsPolicy::default();
            app.allow(feature, origin);
            assert!(
                permissions_header(&PermissionsMode::Default, &app).is_err(),
                "{feature} {origin}"
            );
        }
    }

    /// What the ETag hashes: the page without the request's nonce.
    #[test]
    fn the_nonce_is_left_out_of_the_hash() {
        assert_eq!(&*without(b"a-NONCE-b-NONCE", b"NONCE"), b"a--b-");
        assert_eq!(&*without(b"plain", b"NONCE"), b"plain");
        assert_eq!(&*without(b"plain", b""), b"plain");
    }

    /// A page body that claims a size, then fails while being read.
    struct Broken;

    impl HttpBody for Broken {
        type Data = Bytes;
        type Error = std::io::Error;

        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<http_body::Frame<Bytes>, std::io::Error>>> {
            Poll::Ready(Some(Err(std::io::Error::other("disk went away"))))
        }

        fn size_hint(&self) -> http_body::SizeHint {
            http_body::SizeHint::with_exact(10)
        }
    }

    #[tokio::test]
    async fn an_etag_page_that_cant_be_read_is_a_500() {
        let mut res = (StatusCode::OK, Body::new(Broken)).into_response();
        res.extensions_mut().insert(WantsEtag);
        let res = etag(res, &Method::GET, None, "").await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}

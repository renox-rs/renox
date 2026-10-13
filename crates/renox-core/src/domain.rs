//! Routes for other hosts: `Routes::domain("admin.example.com", …)` and
//! `Routes::domain("{account}.example.com", …)`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::Router;
use axum::extract::{FromRequestParts, Request};
use axum::http::request::Parts;

/// A host pattern: labels separated by dots, each literal or `{name}`
/// (one label, e.g. `{account}` in `{account}.example.com`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DomainPattern {
    text: String,
    labels: Vec<Label>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Label {
    Literal(String),
    Param(String),
}

impl DomainPattern {
    pub(crate) fn parse(text: &str) -> anyhow::Result<Self> {
        let text = text.trim().to_ascii_lowercase();
        anyhow::ensure!(
            !text.is_empty() && !text.contains(['/', ':']),
            "Routes::domain(\"{text}\"): give a host like `admin.example.com` or \
             `{{account}}.example.com`, without a scheme, port or path"
        );
        let labels = text
            .split('.')
            .map(|label| {
                if let Some(name) = label.strip_prefix('{').and_then(|l| l.strip_suffix('}')) {
                    anyhow::ensure!(
                        !name.is_empty()
                            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                        "Routes::domain(\"{text}\"): `{label}` isn't a valid parameter"
                    );
                    Ok(Label::Param(name.to_owned()))
                } else {
                    anyhow::ensure!(
                        !label.is_empty() && !label.contains(['{', '}']),
                        "Routes::domain(\"{text}\"): `{label}` isn't a valid part of a host"
                    );
                    Ok(Label::Literal(label.to_owned()))
                }
            })
            .collect::<anyhow::Result<_>>()?;
        Ok(Self { text, labels })
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    /// The parameters when `host` (without its port) matches.
    fn matches(&self, host: &str) -> Option<HashMap<String, String>> {
        let host = host.to_ascii_lowercase();
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() != self.labels.len() {
            return None;
        }
        let mut params = HashMap::new();
        for (label, part) in self.labels.iter().zip(parts) {
            match label {
                Label::Literal(literal) if literal == part => {}
                Label::Literal(_) => return None,
                Label::Param(name) => {
                    if part.is_empty() {
                        return None;
                    }
                    params.insert(name.clone(), part.to_owned());
                }
            }
        }
        Some(params)
    }
}

/// The request's host from `Host` (HTTP/1) or the URI's authority (HTTP/2),
/// without a port.
pub(crate) fn host(req: &Request) -> Option<String> {
    let raw = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| req.uri().authority().map(|a| a.as_str().to_owned()))?;
    // `[::1]:3000` and `example.com:3000`: drop the port.
    let host = if raw.starts_with('[') {
        raw.split(']').next().map(|h| format!("{h}]"))?
    } else {
        raw.split(':').next()?.to_owned()
    };
    Some(host)
}

/// Sends each request to the router of the first domain its host matches,
/// else to `default`.
pub(crate) fn dispatch(domains: Vec<(DomainPattern, Router)>, default: Router) -> Router {
    let domains = Arc::new(domains);
    Router::new().fallback_service(tower::service_fn(move |mut req: Request| {
        let (domains, default) = (domains.clone(), default.clone());
        async move {
            if let Some(host) = host(&req) {
                for (pattern, router) in domains.iter() {
                    if let Some(params) = pattern.matches(&host) {
                        req.extensions_mut().insert(DomainParams(Arc::new(params)));
                        req.extensions_mut()
                            .insert(MatchedDomain(Arc::from(pattern.as_str())));
                        return tower::ServiceExt::oneshot(router.clone(), req).await;
                    }
                }
            }
            tower::ServiceExt::oneshot(default, req).await
        }
    }))
}

/// The `Routes::domain` pattern a request matched (for route names).
#[derive(Debug, Clone)]
pub(crate) struct MatchedDomain(pub Arc<str>);

/// The parameters of the domain a request came to, for routes added with
/// `Routes::domain("{account}.example.com", …)`:
///
/// ```
/// # use renox::prelude::*;
/// use renox::DomainParams;
///
/// async fn home(domain: DomainParams) -> String {
///     format!("Welcome to {}", domain.get("account").unwrap_or("?"))
/// }
///
/// # let _: Routes =
/// Routes::new().domain("{account}.example.com", Routes::new().get("/", home))
/// # ;
/// ```
///
/// Empty for other routes.
#[derive(Debug, Clone, Default)]
pub struct DomainParams(Arc<HashMap<String, String>>);

impl DomainParams {
    /// A parameter of the domain pattern, e.g. `account`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for DomainParams {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(parts
            .extensions
            .get::<DomainParams>()
            .cloned()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_hosts() {
        let admin = DomainPattern::parse("Admin.Example.com").unwrap();
        assert!(admin.matches("admin.example.com").is_some());
        assert!(admin.matches("ADMIN.example.com").is_some());
        assert!(admin.matches("example.com").is_none());
        assert!(admin.matches("x.admin.example.com").is_none());

        let tenant = DomainParams(Arc::new(
            DomainPattern::parse("{account}.example.com")
                .unwrap()
                .matches("acme.example.com")
                .unwrap(),
        ));
        assert_eq!(tenant.get("account"), Some("acme"));
        assert!(
            DomainPattern::parse("{account}.example.com")
                .unwrap()
                .matches("example.com")
                .is_none()
        );

        for bad in [
            "",
            "https://x.com",
            "x.com:80",
            "x.com/a",
            "{}.x.com",
            "a..b",
            "{a b}.x.com",
        ] {
            assert!(DomainPattern::parse(bad).is_err(), "{bad}");
        }
        // An empty label never fills a parameter.
        let tenant = DomainPattern::parse("{account}.example.com").unwrap();
        assert!(tenant.matches(".example.com").is_none());
    }

    #[test]
    fn hosts_drop_their_port_ipv6_included() {
        let host_of = |value: &str| {
            let req = Request::builder()
                .header(axum::http::header::HOST, value)
                .body(axum::body::Body::empty())
                .unwrap();
            host(&req)
        };
        assert_eq!(host_of("[::1]:3000").as_deref(), Some("[::1]"));
        assert_eq!(host_of("shop.test:8080").as_deref(), Some("shop.test"));
    }
}

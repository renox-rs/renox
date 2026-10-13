//! Being found and shared: `seo()` in templates (title, description,
//! canonical URL, OpenGraph and Twitter cards), `robots.txt`, sitemaps, and
//! the head tags for Search Console, Google Analytics 4 and Tag Manager.
//!
//! ```html
//! <head>
//!   {{ seo(title=product.name ~ " · " ~ app.name, description=product.summary,
//!          image=storage_url(product.photo)) }}
//!   {{ renox_head() }}
//! </head>
//! ```
//!
//! Outside production every page says `noindex` and `robots.txt` disallows
//! everything, so a staging server doesn't end up in search results.

use std::sync::Arc;

use axum::Router;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use minijinja::value::{Kwargs, Value};

use crate::config::{Config, Environment};
use crate::db::DateTime;
use crate::routing::RouteTable;
use crate::{AppState, Result};

/// Escapes text for an HTML attribute or element.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// `path` as an absolute URL on `APP_URL`; URLs with a scheme are kept.
pub(crate) fn absolute(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") || path.starts_with("//") {
        return path.to_owned();
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// The `seo(title=…, description=…, image=…, type=…, canonical=…)` template
/// function: `<title>`, the description, the canonical link, OpenGraph and
/// Twitter card tags. `canonical` defaults to this page's URL on `APP_URL`
/// (without the query string); relative `image`s are made absolute.
pub(crate) fn tags(
    config: &Config,
    path: &str,
    locale: &str,
    kwargs: &Kwargs,
) -> Result<String, minijinja::Error> {
    let get = |name: &str| -> Result<Option<String>, minijinja::Error> {
        Ok(kwargs
            .get::<Option<Value>>(name)?
            .filter(|v| !v.is_none() && !v.is_undefined())
            .map(|v| v.to_string())
            .filter(|v| !v.is_empty()))
    };
    let title = get("title")?.unwrap_or_else(|| config.name.clone());
    let description = get("description")?;
    let image = get("image")?.map(|image| absolute(&config.url, &image));
    let kind = get("type")?.unwrap_or_else(|| "website".into());
    let canonical = get("canonical")?
        .map(|url| absolute(&config.url, &url))
        .unwrap_or_else(|| absolute(&config.url, path));
    kwargs.assert_all_used()?;

    let mut html = format!("<title>{}</title>\n", escape(&title));
    let mut meta = |attr: &str, name: &str, content: &str| {
        html.push_str(&format!(
            "<meta {attr}=\"{name}\" content=\"{}\">\n",
            escape(content)
        ));
    };
    if let Some(description) = &description {
        meta("name", "description", description);
    }
    meta("property", "og:title", &title);
    if let Some(description) = &description {
        meta("property", "og:description", description);
    }
    meta("property", "og:type", &kind);
    meta("property", "og:url", &canonical);
    meta("property", "og:site_name", &config.name);
    meta("property", "og:locale", &locale.replace('-', "_"));
    if let Some(image) = &image {
        meta("property", "og:image", image);
    }
    let card = if image.is_some() {
        "summary_large_image"
    } else {
        "summary"
    };
    meta("name", "twitter:card", card);
    meta("name", "twitter:title", &title);
    if let Some(description) = &description {
        meta("name", "twitter:description", description);
    }
    if let Some(image) = &image {
        meta("name", "twitter:image", image);
    }
    html.push_str(&format!(
        "<link rel=\"canonical\" href=\"{}\">",
        escape(&canonical)
    ));
    Ok(html)
}

/// Head tags `renox_head()` adds for search engines and analytics.
pub(crate) fn head_tags(
    config: &Config,
    nonce: &str,
    events: &[crate::analytics::Event],
) -> String {
    let mut html = String::new();
    if config.env != Environment::Production {
        html.push_str("<meta name=\"robots\" content=\"noindex, nofollow\">\n");
        // Events are still delivered to renox.js, e.g. for a stubbed gtag in tests.
        html.push_str(&crate::analytics::events_meta(events));
        return html;
    }
    let analytics = &config.analytics;
    if let Some(code) = &analytics.google_site_verification {
        html.push_str(&format!(
            "<meta name=\"google-site-verification\" content=\"{}\">\n",
            escape(code)
        ));
    }
    let nonce = escape(nonce);
    if let Some(id) = &analytics.ga4_measurement_id {
        let id = escape(id);
        html.push_str(&format!(
            "<script async src=\"https://www.googletagmanager.com/gtag/js?id={id}\" nonce=\"{nonce}\"></script>\n\
             <script nonce=\"{nonce}\">window.dataLayer=window.dataLayer||[];\
             function gtag(){{dataLayer.push(arguments);}}gtag('js',new Date());gtag('config','{id}');</script>\n"
        ));
    }
    if let Some(id) = &analytics.gtm_container_id {
        let id = escape(id);
        // Google's snippet, passing the nonce on to the script it loads.
        html.push_str(&format!(
            "<script nonce=\"{nonce}\">(function(w,d,s,l,i){{w[l]=w[l]||[];w[l].push({{'gtm.start':\
             new Date().getTime(),event:'gtm.js'}});var f=d.getElementsByTagName(s)[0],\
             j=d.createElement(s),dl=l!='dataLayer'?'&l='+l:'';j.async=true;j.src=\
             'https://www.googletagmanager.com/gtm.js?id='+i+dl;var n=d.querySelector('[nonce]');\
             n&&j.setAttribute('nonce',n.nonce||n.getAttribute('nonce'));f.parentNode.insertBefore(j,f);\
             }})(window,document,'script','dataLayer','{id}');</script>\n"
        ));
    }
    html.push_str(&crate::analytics::events_meta(events));
    html
}

/// CSP sources the analytics tags need, added to the policy at boot.
pub(crate) fn csp_sources(config: &Config) -> Vec<(&'static str, &'static str)> {
    let analytics = &config.analytics;
    if config.env != Environment::Production
        || (analytics.ga4_measurement_id.is_none() && analytics.gtm_container_id.is_none())
    {
        return Vec::new();
    }
    vec![
        ("script-src", "https://www.googletagmanager.com"),
        ("connect-src", "https://*.google-analytics.com"),
        ("connect-src", "https://*.analytics.google.com"),
        ("connect-src", "https://*.googletagmanager.com"),
    ]
}

/// `GET /robots.txt`, unless the app has its own in `public/`. Production
/// allows everything and points at the sitemap when a route is named
/// `sitemap`; other environments disallow everything.
pub(crate) fn robots_router(state: &AppState) -> Router<AppState> {
    let body = if state.config.env == Environment::Production {
        let mut body = String::from("User-agent: *\nAllow: /\n");
        if let Some(path) = state.routes.path("sitemap") {
            body.push_str(&format!(
                "\nSitemap: {}\n",
                absolute(&state.config.url, path)
            ));
        }
        body
    } else {
        String::from("User-agent: *\nDisallow: /\n")
    };
    Router::new().route(
        "/robots.txt",
        get(move || {
            let body = body.clone();
            async move { ([(CONTENT_TYPE, "text/plain; charset=utf-8")], body) }
        }),
    )
}

/// A `sitemap.xml` response. Name its route `sitemap` and `robots.txt`
/// will point search engines at it.
///
/// ```
/// # use renox::prelude::*;
/// # use renox::seo::Sitemap;
/// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, updated_at: Option<DateTime> }
/// # let _: Routes =
/// Routes::new().get("/sitemap.xml", sitemap).name("sitemap")
/// # ;
///
/// async fn sitemap(State(state): State<AppState>) -> Result<Sitemap> {
///     let mut map = Sitemap::new(&state).route("home", &[], None)?;
///     for p in Product::all(&state.db).await? {
///         map = map.route("products.show", &[&p.id], p.updated_at)?;
///     }
///     Ok(map)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Sitemap {
    base: String,
    routes: Arc<RouteTable>,
    urls: Vec<(String, Option<DateTime>)>,
}

impl Sitemap {
    /// An empty sitemap; URLs are made absolute with `APP_URL`.
    pub fn new(state: &AppState) -> Self {
        Self {
            base: state.config.url.clone(),
            routes: state.routes.clone(),
            urls: Vec::new(),
        }
    }

    /// A path (`/about`) or full URL, and when its content last changed.
    pub fn add(mut self, path: &str, last_modified: Option<DateTime>) -> Self {
        self.urls.push((absolute(&self.base, path), last_modified));
        self
    }

    /// A named route with its parameters.
    pub fn route(
        self,
        name: &str,
        params: &[&dyn std::fmt::Display],
        last_modified: Option<DateTime>,
    ) -> Result<Self> {
        let path = self.routes.url(name, params)?;
        Ok(self.add(&path, last_modified))
    }

    /// The sitemap as XML.
    pub fn to_xml(&self) -> String {
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
        );
        for (url, modified) in &self.urls {
            xml.push_str(&format!("  <url><loc>{}</loc>", escape(url)));
            if let Some(modified) = modified {
                xml.push_str(&format!(
                    "<lastmod>{}</lastmod>",
                    modified.format("%Y-%m-%dT%H:%M:%SZ")
                ));
            }
            xml.push_str("</url>\n");
        }
        xml.push_str("</urlset>\n");
        xml
    }
}

impl IntoResponse for Sitemap {
    fn into_response(self) -> Response {
        (
            [(CONTENT_TYPE, "application/xml; charset=utf-8")],
            self.to_xml(),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_urls_absolute_and_escapes() {
        assert_eq!(
            absolute("https://shop.example/", "/a"),
            "https://shop.example/a"
        );
        assert_eq!(
            absolute("https://shop.example", "a"),
            "https://shop.example/a"
        );
        assert_eq!(
            absolute("https://shop.example", "https://cdn.x/y.png"),
            "https://cdn.x/y.png"
        );
        assert_eq!(
            escape(r#"Coffee & "Tea" <b>"#),
            "Coffee &amp; &quot;Tea&quot; &lt;b&gt;"
        );
    }
}

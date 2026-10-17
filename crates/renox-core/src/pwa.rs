//! Installable apps: the [`Pwa`] builder and the web app manifest it
//! generates.
//!
//! ```
//! use renox::pwa::{Pwa, PwaRequest};
//!
//! let pwa = Pwa::new("Bikeshop").theme_color("#123456");
//! let manifest = pwa.manifest(&PwaRequest::new("shop.test", "ar"), None);
//! assert_eq!(manifest["name"], "Bikeshop");
//! assert_eq!(manifest["dir"], "rtl");
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use axum::Router;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HOST, HeaderName, VARY};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};

use crate::webhook::sha256_hex;

use crate::i18n::{Direction, direction};
use crate::state::AppState;

/// The kit's light accent colour, the default `theme_color`.
pub const DEFAULT_THEME_COLOR: &str = "#0b6e66";

/// The default `background_color`.
pub const DEFAULT_BACKGROUND_COLOR: &str = "#ffffff";

/// URL prefix the icon files are served under.
pub const ICON_ROUTE: &str = "/_renox/pwa/icons";

/// The icon files a manifest lists: file name, size and purpose.
const ICON_FILES: [(&str, &str, &str); 3] = [
    ("icon-192.png", "192x192", "any"),
    ("icon-512.png", "512x512", "any"),
    ("icon-maskable-512.png", "512x512", "maskable"),
];

/// Where the offline page is served.
pub const OFFLINE_ROUTE: &str = "/_renox/offline";

/// The service worker source; the placeholders are filled in per app.
const WORKER: &str = include_str!("../assets/renox-sw.js");

/// Shown when the app names no offline view.
const DEFAULT_OFFLINE: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Offline</title></head><body><h1>You are offline</h1><p>Check your connection and try again.</p></body></html>\n";

/// How an installed app is displayed (the manifest `display` member).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PwaDisplay {
    /// A normal browser tab.
    Browser,
    /// Like an app, with a minimal browser bar.
    MinimalUi,
    /// Like an app, without browser chrome. The default.
    #[default]
    Standalone,
    /// Takes the whole screen.
    Fullscreen,
}

impl PwaDisplay {
    /// The manifest value, such as `"standalone"`.
    ///
    /// ```
    /// use renox::pwa::PwaDisplay;
    /// assert_eq!(PwaDisplay::MinimalUi.as_str(), "minimal-ui");
    /// ```
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::MinimalUi => "minimal-ui",
            Self::Standalone => "standalone",
            Self::Fullscreen => "fullscreen",
        }
    }
}

/// What a manifest is generated for: the request's host and locale.
///
/// The locale decides `lang` and `dir`. The localized names are the values
/// found under the lang keys `pwa.name` and `pwa.short_name`, which the
/// route fills in when the app defines them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PwaRequest {
    host: String,
    locale: String,
    dir: Direction,
    name: Option<String>,
    short_name: Option<String>,
}

impl PwaRequest {
    /// A request for `host` in `locale`; the direction comes from
    /// [`direction`].
    ///
    /// ```
    /// use renox::pwa::PwaRequest;
    /// let req = PwaRequest::new("shop.test", "he");
    /// assert_eq!(req.host(), "shop.test");
    /// assert_eq!(req.locale(), "he");
    /// ```
    pub fn new(host: impl Into<String>, locale: impl Into<String>) -> Self {
        let locale = locale.into();
        Self {
            host: host.into(),
            dir: direction(&locale),
            locale,
            name: None,
            short_name: None,
        }
    }

    /// Sets the writing direction, for apps that changed
    /// `App::rtl_locales`.
    #[must_use]
    pub fn direction(mut self, dir: Direction) -> Self {
        self.dir = dir;
        self
    }

    /// Sets the localized name (lang key `pwa.name`).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the localized short name (lang key `pwa.short_name`).
    #[must_use]
    pub fn short_name(mut self, name: impl Into<String>) -> Self {
        self.short_name = Some(name.into());
        self
    }

    /// The request host, without a port when the caller removed it.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The request locale, such as `en` or `ar-AE`.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// The writing direction of the locale.
    pub fn dir(&self) -> Direction {
        self.dir
    }
}

/// Per-request branding returned by a `pwa_for` resolver; it replaces the
/// static [`Pwa`] values for that request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PwaBrand {
    name: String,
    short_name: Option<String>,
    theme_color: Option<String>,
    icon_dir: Option<PathBuf>,
    icon_bytes: Vec<(String, Vec<u8>)>,
}

impl PwaBrand {
    /// A brand with this app name.
    ///
    /// ```
    /// use renox::pwa::PwaBrand;
    /// let brand = PwaBrand::new("Acme").short_name("AC").theme_color("#ff0000");
    /// assert_eq!(brand.app_name(), "Acme");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Sets the short name shown under the home-screen icon.
    #[must_use]
    pub fn short_name(mut self, name: impl Into<String>) -> Self {
        self.short_name = Some(name.into());
        self
    }

    /// Sets the theme colour.
    #[must_use]
    pub fn theme_color(mut self, color: impl Into<String>) -> Self {
        self.theme_color = Some(color.into());
        self
    }

    /// Reads the icon files from a directory (same file names as
    /// [`Pwa::icons`]).
    #[must_use]
    pub fn icons(mut self, dir: impl Into<PathBuf>) -> Self {
        self.icon_dir = Some(dir.into());
        self
    }

    /// Supplies one icon file as bytes, such as `icon-192.png`.
    #[must_use]
    pub fn icon_bytes(mut self, file: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.icon_bytes.push((file.into(), bytes.into()));
        self
    }

    /// The brand's app name.
    pub fn app_name(&self) -> &str {
        &self.name
    }
}

/// The installable-app configuration: names, colours, icons and start page.
///
/// ```
/// use renox::pwa::{Pwa, PwaDisplay};
///
/// let pwa = Pwa::new("Bikeshop")
///     .short_name("Bikes")
///     .start_url("/shop")
///     .display(PwaDisplay::MinimalUi);
/// assert_eq!(pwa.app_name(), "Bikeshop");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pwa {
    name: String,
    short_name: Option<String>,
    theme_color: String,
    background_color: String,
    icons: Option<PathBuf>,
    start_url: String,
    display: PwaDisplay,
    offline_page: Option<String>,
}

impl Pwa {
    /// A configuration for an app called `name`, with the default colours,
    /// start URL `/` and `standalone` display.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            short_name: None,
            theme_color: DEFAULT_THEME_COLOR.to_string(),
            background_color: DEFAULT_BACKGROUND_COLOR.to_string(),
            icons: None,
            start_url: "/".to_string(),
            display: PwaDisplay::default(),
            offline_page: None,
        }
    }

    /// Sets the short name; it defaults to the name.
    #[must_use]
    pub fn short_name(mut self, name: impl Into<String>) -> Self {
        self.short_name = Some(name.into());
        self
    }

    /// Sets the theme colour (default `#0b6e66`).
    #[must_use]
    pub fn theme_color(mut self, color: impl Into<String>) -> Self {
        self.theme_color = color.into();
        self
    }

    /// Sets the background colour (default `#ffffff`).
    #[must_use]
    pub fn background_color(mut self, color: impl Into<String>) -> Self {
        self.background_color = color.into();
        self
    }

    /// Sets the directory holding `icon-192.png`, `icon-512.png` and
    /// `icon-maskable-512.png`.
    #[must_use]
    pub fn icons(mut self, dir: impl Into<PathBuf>) -> Self {
        self.icons = Some(dir.into());
        self
    }

    /// Sets the page the installed app opens (default `/`).
    #[must_use]
    pub fn start_url(mut self, url: impl Into<String>) -> Self {
        self.start_url = url.into();
        self
    }

    /// Sets how the installed app is displayed (default standalone).
    #[must_use]
    pub fn display(mut self, display: PwaDisplay) -> Self {
        self.display = display;
        self
    }

    /// Sets the view shown when the network is down.
    #[must_use]
    pub fn offline_page(mut self, view: impl Into<String>) -> Self {
        self.offline_page = Some(view.into());
        self
    }

    /// The configured app name.
    pub fn app_name(&self) -> &str {
        &self.name
    }

    /// The icon directory, when set.
    pub fn icon_dir(&self) -> Option<&Path> {
        self.icons.as_deref()
    }

    /// The offline view name, when set.
    pub fn offline_view(&self) -> Option<&str> {
        self.offline_page.as_deref()
    }

    /// The web app manifest for `req`.
    ///
    /// The name comes from the brand, else the request's localized name,
    /// else the builder value; `lang` and `dir` come from the request.
    ///
    /// ```
    /// use renox::pwa::{Pwa, PwaBrand, PwaRequest};
    ///
    /// let pwa = Pwa::new("Bikeshop");
    /// let req = PwaRequest::new("a.test", "en");
    /// let brand = PwaBrand::new("Acme").theme_color("#ff0000");
    /// let m = pwa.manifest(&req, Some(&brand));
    /// assert_eq!(m["name"], "Acme");
    /// assert_eq!(m["short_name"], "Acme");
    /// assert_eq!(m["theme_color"], "#ff0000");
    /// ```
    pub fn manifest(&self, req: &PwaRequest, brand: Option<&PwaBrand>) -> Value {
        let name = brand
            .map(|b| b.name.clone())
            .or_else(|| req.name.clone())
            .unwrap_or_else(|| self.name.clone());
        let short_name = brand
            .map(|b| b.short_name.clone().unwrap_or_else(|| b.name.clone()))
            .or_else(|| req.short_name.clone())
            .or_else(|| self.short_name.clone())
            .unwrap_or_else(|| name.clone());
        let theme = brand
            .and_then(|b| b.theme_color.clone())
            .unwrap_or_else(|| self.theme_color.clone());
        let icons: Vec<Value> = ICON_FILES
            .iter()
            .map(|(file, sizes, purpose)| {
                json!({
                    "src": format!("{ICON_ROUTE}/{file}"),
                    "sizes": sizes,
                    "type": "image/png",
                    "purpose": purpose,
                })
            })
            .collect();
        json!({
            "name": name,
            "short_name": short_name,
            "start_url": self.start_url,
            "scope": "/",
            "display": self.display.as_str(),
            "theme_color": theme,
            "background_color": self.background_color,
            "lang": req.locale,
            "dir": req.dir.as_str(),
            "icons": icons,
        })
    }
}

/// What the worker routes serve: the icons, the offline page and the
/// versioned worker source, all built once at boot.
#[derive(Debug, Clone)]
pub(crate) struct PwaAssets {
    icons: BTreeMap<&'static str, Vec<u8>>,
    offline_html: String,
    worker: String,
    #[cfg_attr(not(test), allow(dead_code, reason = "read by the tests"))]
    version: String,
}

#[cfg(test)]
impl PwaAssets {
    /// The worker version: a prefix of the hash of the worker source, the
    /// asset paths, the offline page and the brand hash.
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// The worker source with version and precache list filled in.
    pub(crate) fn worker(&self) -> &str {
        &self.worker
    }

    /// The rendered offline page.
    pub(crate) fn offline_html(&self) -> &str {
        &self.offline_html
    }
}

impl PwaAssets {
    /// Routes `/sw.js`, `/_renox/offline` and the icon files. They need no
    /// state, so they merge into any router.
    pub(crate) fn router<S: Clone + Send + Sync + 'static>(
        self: Arc<Self>,
        serve_worker: bool,
    ) -> Router<S> {
        let (worker, offline, icons) = (self.clone(), self.clone(), self);
        let mut router = Router::new();
        if serve_worker {
            router = router.route(
                "/sw.js",
                get(move || {
                    let body = worker.worker.clone();
                    async move {
                        (
                            [
                                (CONTENT_TYPE, "text/javascript; charset=utf-8"),
                                (CACHE_CONTROL, "no-cache"),
                                (HeaderName::from_static("service-worker-allowed"), "/"),
                            ],
                            body,
                        )
                    }
                }),
            );
        }
        router
            .route(
                OFFLINE_ROUTE,
                get(move || {
                    let body = offline.offline_html.clone();
                    async move { offline_response(body, false) }
                }),
            )
            .merge(icons.icons_router())
    }

    /// Only the icon files, for the resolver routes, which answer the worker
    /// and the offline page themselves.
    pub(crate) fn icons_router<S: Clone + Send + Sync + 'static>(self: Arc<Self>) -> Router<S> {
        Router::new().route(
            &format!("{ICON_ROUTE}/{{file}}"),
            get(move |UrlPath(file): UrlPath<String>| {
                let icon = self.icons.get(file.as_str()).cloned();
                async move { icon_response(icon) }
            }),
        )
    }
}

fn offline_response(body: String, vary_host: bool) -> Response {
    let mut res = (
        [
            (CONTENT_TYPE, "text/html; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response();
    if vary_host {
        res.headers_mut()
            .insert(VARY, HeaderValue::from_static("Host"));
    }
    res
}

impl Pwa {
    /// The head tags `<rx-pwa/>` emits: the manifest link, `theme-color`, the
    /// apple-touch-icon and the worker registration script. The inline script
    /// carries the request's CSP `nonce`, so it runs under `CSP=strict`.
    ///
    /// ```
    /// use renox::pwa::Pwa;
    /// let html = Pwa::new("Bikeshop").theme_color("#123456").head_tags("abc");
    /// assert!(html.contains(r#"<link rel="manifest" href="/manifest.webmanifest">"#));
    /// assert!(html.contains(r##"<meta name="theme-color" content="#123456">"##));
    /// assert!(html.contains(r#"<script nonce="abc">"#));
    /// ```
    pub fn head_tags(&self, nonce: &str) -> String {
        format!(
            "<link rel=\"manifest\" href=\"/manifest.webmanifest\">\n\
             <meta name=\"theme-color\" content=\"{color}\">\n\
             <link rel=\"apple-touch-icon\" href=\"{ICON_ROUTE}/icon-192.png\">\n\
             <script nonce=\"{nonce}\">if('serviceWorker' in navigator){{addEventListener('load',function(){{\
             navigator.serviceWorker.register('/sw.js',{{scope:'/'}}).catch(function(){{}})}})}}</script>",
            color = crate::seo::escape(&self.theme_color),
            nonce = crate::seo::escape(nonce),
        )
    }

    /// `GET /manifest.webmanifest`, outside sessions: the locale is the
    /// `Accept-Language` pick (when the app detects it) or `APP_LOCALE`, and
    /// the names come from the lang keys `pwa.name` and `pwa.short_name`.
    pub(crate) fn manifest_router(self: Arc<Self>) -> Router<AppState> {
        Router::new().route(
            "/manifest.webmanifest",
            get(move |State(state): State<AppState>, headers: HeaderMap| {
                let pwa = self.clone();
                async move {
                    let req = request_for(&state, &headers);
                    let mut res = (
                        [
                            (CONTENT_TYPE, "application/manifest+json"),
                            (CACHE_CONTROL, "no-cache"),
                        ],
                        pwa.manifest(&req, None).to_string(),
                    )
                        .into_response();
                    if state.detect_locale {
                        res.headers_mut()
                            .insert(VARY, HeaderValue::from_static("Accept-Language"));
                    }
                    res
                }
            }),
        )
    }
}

/// The [`PwaRequest`] for a request: host, locale, direction and the
/// localized names from the lang files.
fn request_for(state: &AppState, headers: &HeaderMap) -> PwaRequest {
    let locale = crate::i18n::header_locale(state, headers);
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let text = |key: &str| {
        let text = state.translator.get(&locale, &state.config.locale, key);
        (text != key).then_some(text)
    };
    let mut req = PwaRequest::new(host, locale.clone()).direction(state.direction(&locale));
    if let Some(name) = text("pwa.name") {
        req = req.name(name);
    }
    if let Some(name) = text("pwa.short_name") {
        req = req.short_name(name);
    }
    req
}

fn icon_response(icon: Option<Vec<u8>>) -> Response {
    match icon {
        Some(bytes) => (
            [
                (CONTENT_TYPE, "image/png"),
                (CACHE_CONTROL, "public, max-age=86400"),
            ],
            bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The worker source and its version for an offline page, the precache list
/// and a brand hash (empty for the static `Pwa`).
fn worker_source(
    offline_html: &str,
    precache: &[String],
    brand_hash: &str,
) -> anyhow::Result<(String, String)> {
    let mut paths = vec![OFFLINE_ROUTE.to_string()];
    paths.extend(precache.iter().cloned());
    let list = serde_json::to_string(&paths)?;
    let version =
        sha256_hex(format!("{WORKER}\n{list}\n{offline_html}\n{brand_hash}"))[..16].to_string();
    let worker = WORKER
        .replace("__RENOX_SW_VERSION__", &version)
        .replace("__RENOX_SW_PRECACHE__", &list)
        .replace("__RENOX_SW_OFFLINE__", OFFLINE_ROUTE);
    Ok((worker, version))
}

impl Pwa {
    /// Loads the icons, renders the offline page with `render` (view name to
    /// HTML, no request) and builds the worker. A missing icon or a failed
    /// render is an error naming the file or view, for the boot to report.
    /// `precache` lists the hashed asset paths; `brand_hash` is mixed into
    /// the version (empty without a resolver).
    pub(crate) fn assets(
        &self,
        render: impl Fn(&str) -> anyhow::Result<String>,
        precache: &[String],
        brand_hash: &str,
    ) -> anyhow::Result<PwaAssets> {
        let dir = self.icons.as_deref().context(
            "PWA icons are not set: call Pwa::icons(dir) with icon-192.png, icon-512.png and icon-maskable-512.png",
        )?;
        let mut icons = BTreeMap::new();
        for (file, _, _) in ICON_FILES {
            let path = dir.join(file);
            let bytes = std::fs::read(&path)
                .with_context(|| format!("PWA icon {} is missing or unreadable", path.display()))?;
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                bail!("PWA icon {} is not a PNG file", path.display());
            }
            icons.insert(file, bytes);
        }
        let offline_html = match self.offline_view() {
            Some(view) => render(view)
                .with_context(|| format!("PWA offline view `{view}` failed to render"))?,
            None => DEFAULT_OFFLINE.to_string(),
        };
        let (worker, version) = worker_source(&offline_html, precache, brand_hash)?;
        Ok(PwaAssets {
            icons,
            offline_html,
            worker,
            version,
        })
    }
}

/// How a per-request resolver is kept.
pub(crate) type PwaFn = Arc<
    dyn Fn(
            PwaRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = crate::Result<Option<PwaBrand>>> + Send>,
        > + Send
        + Sync,
>;

pub(crate) fn pwa_fn<F, Fut>(resolve: F) -> PwaFn
where
    F: Fn(PwaRequest) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = crate::Result<Option<PwaBrand>>> + Send + 'static,
{
    Arc::new(move |req| Box::pin(resolve(req)))
}

/// The static `Pwa`, its boot-time assets and the optional resolver.
pub(crate) type PwaParts = (Arc<Pwa>, Arc<PwaAssets>, Option<Arc<PwaResolver>>);

/// One brand's assets, built on its first request and kept by brand hash.
#[derive(Debug)]
struct BrandAssets {
    brand: PwaBrand,
    hash: String,
    /// `None` when the brand has no icons or they were invalid: the static
    /// icons are used.
    icons: Option<BTreeMap<&'static str, Vec<u8>>>,
    offline_html: String,
    worker: String,
}

/// The per-request resolver (`App::pwa_for`) and the brands it produced.
pub(crate) struct PwaResolver {
    choose: PwaFn,
    precache: Vec<String>,
    brands: std::sync::RwLock<std::collections::HashMap<String, Arc<BrandAssets>>>,
}

impl std::fmt::Debug for PwaResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PwaResolver").finish_non_exhaustive()
    }
}

impl PwaBrand {
    /// A hash of everything the brand shows, without reading icon files:
    /// the icon directory's contents are read once per brand, so changing
    /// them takes a restart or a different directory.
    fn hash(&self) -> String {
        let mut text = format!(
            "{}\0{:?}\0{:?}\0{:?}",
            self.name, self.short_name, self.theme_color, self.icon_dir
        );
        for (file, bytes) in &self.icon_bytes {
            text.push_str(&format!("\0{file}\0{}", sha256_hex(bytes)));
        }
        sha256_hex(text)[..16].to_string()
    }

    /// Reads and checks the three icon files; `None` when the brand names
    /// no icon source at all.
    async fn load_icons(&self) -> anyhow::Result<Option<BTreeMap<&'static str, Vec<u8>>>> {
        if self.icon_dir.is_none() && self.icon_bytes.is_empty() {
            return Ok(None);
        }
        let mut icons = BTreeMap::new();
        for (file, _, _) in ICON_FILES {
            let bytes = match self.icon_bytes.iter().rev().find(|(f, _)| f == file) {
                Some((_, bytes)) => bytes.clone(),
                None => match &self.icon_dir {
                    Some(dir) => {
                        let path = dir.join(file);
                        tokio::fs::read(&path).await.with_context(|| {
                            format!("PWA icon {} is missing or unreadable", path.display())
                        })?
                    }
                    None => bail!("PWA brand `{}` has no icon {file}", self.name),
                },
            };
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                bail!("PWA brand `{}`: icon {file} is not a PNG file", self.name);
            }
            icons.insert(file, bytes);
        }
        Ok(Some(icons))
    }
}

impl PwaResolver {
    pub(crate) fn new(choose: PwaFn, precache: Vec<String>) -> Self {
        Self {
            choose,
            precache,
            brands: std::sync::RwLock::default(),
        }
    }

    /// The brand for a request. A resolver that fails is logged and falls
    /// back to the static `Pwa`, like one that answers `None`.
    async fn resolve(
        &self,
        state: &AppState,
        pwa: &Pwa,
        req: PwaRequest,
    ) -> Option<Arc<BrandAssets>> {
        let brand = match (self.choose)(req).await {
            Ok(Some(brand)) => brand,
            Ok(None) => return None,
            Err(err) => {
                tracing::warn!(error = ?err, "the PWA resolver failed");
                return None;
            }
        };
        let hash = brand.hash();
        let known = self
            .brands
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&hash)
            .cloned();
        if known.is_some() {
            return known;
        }
        let icons = brand.load_icons().await.unwrap_or_else(|err| {
            tracing::error!(error = ?err, "invalid PWA brand icons; using the static icons");
            None
        });
        let offline_html = match pwa.offline_view() {
            Some(view) => {
                let context = json!({"pwa": {
                    "name": brand.name,
                    "short_name": brand.short_name,
                    "theme_color": brand.theme_color,
                }});
                state.views.render(view, context).unwrap_or_else(|err| {
                    tracing::error!(error = ?err, view, "the PWA offline view failed for a brand");
                    DEFAULT_OFFLINE.to_string()
                })
            }
            None => DEFAULT_OFFLINE.to_string(),
        };
        let (worker, _) = worker_source(&offline_html, &self.precache, &hash).ok()?;
        let built = Arc::new(BrandAssets {
            brand,
            hash: hash.clone(),
            icons,
            offline_html,
            worker,
        });
        Some(
            self.brands
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(hash)
                .or_insert(built)
                .clone(),
        )
    }

    /// The routes that depend on the host: the manifest, `/sw.js`, the
    /// offline page and the brand icons; they send `Vary: Host`. The static
    /// icons are included.
    pub(crate) fn router(
        self: Arc<Self>,
        pwa: Arc<Pwa>,
        assets: Arc<PwaAssets>,
        serve_manifest: bool,
        serve_worker: bool,
    ) -> Router<AppState> {
        let mut router = Router::new().merge(assets.clone().icons_router());
        if serve_manifest {
            let (me, pwa) = (self.clone(), pwa.clone());
            router = router.route(
                "/manifest.webmanifest",
                get(move |State(state): State<AppState>, headers: HeaderMap| {
                    let (me, pwa) = (me.clone(), pwa.clone());
                    async move {
                        let req = request_for(&state, &headers);
                        let brand = me.resolve(&state, &pwa, req.clone()).await;
                        let mut manifest = pwa.manifest(&req, brand.as_deref().map(|b| &b.brand));
                        if let Some(brand) = brand.as_deref().filter(|b| b.icons.is_some()) {
                            rewrite_icons(&mut manifest, &brand.hash);
                        }
                        let mut res = (
                            [
                                (CONTENT_TYPE, "application/manifest+json"),
                                (CACHE_CONTROL, "no-cache"),
                            ],
                            manifest.to_string(),
                        )
                            .into_response();
                        let vary = if state.detect_locale {
                            "Host, Accept-Language"
                        } else {
                            "Host"
                        };
                        res.headers_mut()
                            .insert(VARY, HeaderValue::from_static(vary));
                        res
                    }
                }),
            );
        }
        if serve_worker {
            let (me, pwa, assets) = (self.clone(), pwa.clone(), assets.clone());
            router = router.route(
                "/sw.js",
                get(move |State(state): State<AppState>, headers: HeaderMap| {
                    let (me, pwa, assets) = (me.clone(), pwa.clone(), assets.clone());
                    async move {
                        let req = request_for(&state, &headers);
                        let body = match me.resolve(&state, &pwa, req).await {
                            Some(brand) => brand.worker.clone(),
                            None => assets.worker.clone(),
                        };
                        (
                            [
                                (CONTENT_TYPE, "text/javascript; charset=utf-8"),
                                (CACHE_CONTROL, "no-cache"),
                                (HeaderName::from_static("service-worker-allowed"), "/"),
                                (VARY, "Host"),
                            ],
                            body,
                        )
                    }
                }),
            );
        }
        let (me, offline_pwa, offline_assets) = (self.clone(), pwa, assets);
        router = router.route(
            OFFLINE_ROUTE,
            get(move |State(state): State<AppState>, headers: HeaderMap| {
                let (me, pwa, assets) = (me.clone(), offline_pwa.clone(), offline_assets.clone());
                async move {
                    let req = request_for(&state, &headers);
                    let html = match me.resolve(&state, &pwa, req).await {
                        Some(brand) => brand.offline_html.clone(),
                        None => assets.offline_html.clone(),
                    };
                    offline_response(html, true)
                }
            }),
        );
        router.route(
            &format!("{ICON_ROUTE}/{{hash}}/{{file}}"),
            get(move |UrlPath((hash, file)): UrlPath<(String, String)>| {
                let icon = self
                    .brands
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&hash)
                    .and_then(|b| b.icons.as_ref()?.get(file.as_str()).cloned());
                async move { icon_response(icon) }
            }),
        )
    }
}

/// Points the manifest's icons at the brand's own files.
fn rewrite_icons(manifest: &mut Value, hash: &str) {
    let Some(icons) = manifest["icons"].as_array_mut() else {
        return;
    };
    for icon in icons {
        if let Some(src) = icon["src"].as_str() {
            let file = src.rsplit('/').next().unwrap_or(src);
            icon["src"] = Value::from(format!("{ICON_ROUTE}/{hash}/{file}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(locale: &str) -> PwaRequest {
        PwaRequest::new("shop.test", locale)
    }

    #[test]
    fn defaults() {
        let m = Pwa::new("Shop").manifest(&req("en"), None);
        assert_eq!(m["name"], "Shop");
        assert_eq!(m["short_name"], "Shop");
        assert_eq!(m["start_url"], "/");
        assert_eq!(m["display"], "standalone");
        assert_eq!(m["theme_color"], "#0b6e66");
        assert_eq!(m["background_color"], "#ffffff");
        assert_eq!(m["lang"], "en");
        assert_eq!(m["dir"], "ltr");
    }

    #[test]
    fn icons_list_192_512_and_maskable() {
        let m = Pwa::new("Shop").manifest(&req("en"), None);
        let icons = m["icons"].as_array().unwrap();
        assert_eq!(icons.len(), 3);
        assert_eq!(icons[0]["src"], "/_renox/pwa/icons/icon-192.png");
        assert_eq!(icons[0]["sizes"], "192x192");
        assert_eq!(icons[1]["sizes"], "512x512");
        assert_eq!(icons[2]["purpose"], "maskable");
    }

    #[test]
    fn locale_sets_lang_and_dir() {
        let m = Pwa::new("Shop").manifest(&req("ar-AE"), None);
        assert_eq!(m["lang"], "ar-AE");
        assert_eq!(m["dir"], "rtl");
        let m = Pwa::new("Shop").manifest(&req("ar").direction(Direction::Ltr), None);
        assert_eq!(m["dir"], "ltr");
    }

    #[test]
    fn builder_values_are_used() {
        let m = Pwa::new("Shop")
            .short_name("S")
            .theme_color("#111111")
            .background_color("#222222")
            .start_url("/home")
            .display(PwaDisplay::Fullscreen)
            .manifest(&req("en"), None);
        assert_eq!(m["short_name"], "S");
        assert_eq!(m["theme_color"], "#111111");
        assert_eq!(m["background_color"], "#222222");
        assert_eq!(m["start_url"], "/home");
        assert_eq!(m["display"], "fullscreen");
    }

    #[test]
    fn localized_names_beat_builder_and_brand_beats_both() {
        let pwa = Pwa::new("Shop").short_name("S");
        let r = req("es").name("Tienda").short_name("Tda");
        let m = pwa.manifest(&r, None);
        assert_eq!(m["name"], "Tienda");
        assert_eq!(m["short_name"], "Tda");
        let brand = PwaBrand::new("Acme").short_name("AC");
        let m = pwa.manifest(&r, Some(&brand));
        assert_eq!(m["name"], "Acme");
        assert_eq!(m["short_name"], "AC");
        assert_eq!(m["theme_color"], "#0b6e66");
    }

    fn icon_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renox-pwa-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (file, _, _) in ICON_FILES {
            std::fs::write(dir.join(file), b"\x89PNG\r\n\x1a\nx").unwrap();
        }
        dir
    }

    fn paths() -> Vec<String> {
        vec!["/_renox/a-1.js".to_string()]
    }

    #[test]
    fn worker_version_tracks_inputs() {
        let dir = icon_dir("version");
        let pwa = Pwa::new("Shop").icons(&dir).offline_page("offline.html");
        let build = |html: &'static str, p: &[String], brand: &str| {
            pwa.assets(|_| Ok(html.to_string()), p, brand).unwrap()
        };
        let base = build("<p>off</p>", &paths(), "");
        assert_eq!(base.version(), build("<p>off</p>", &paths(), "").version());
        assert_ne!(
            base.version(),
            build("<p>other</p>", &paths(), "").version()
        );
        assert_ne!(base.version(), build("<p>off</p>", &[], "").version());
        assert_ne!(base.version(), build("<p>off</p>", &paths(), "b").version());
        assert!(base.worker().contains(base.version()));
        assert!(base.worker().contains("/_renox/a-1.js"));
        assert!(base.worker().contains(OFFLINE_ROUTE));
        assert!(!base.worker().contains("__RENOX_SW_"));
        assert_eq!(base.offline_html(), "<p>off</p>");
    }

    #[test]
    fn missing_icon_names_the_file() {
        let dir = icon_dir("missing");
        std::fs::remove_file(dir.join("icon-512.png")).unwrap();
        let err = Pwa::new("Shop")
            .icons(&dir)
            .assets(|_| Ok(String::new()), &[], "")
            .unwrap_err();
        assert!(format!("{err:#}").contains("icon-512.png"), "{err:#}");
        let err = Pwa::new("Shop")
            .assets(|_| Ok(String::new()), &[], "")
            .unwrap_err();
        assert!(format!("{err:#}").contains("Pwa::icons"));
    }

    #[test]
    fn failed_offline_render_names_the_view() {
        let dir = icon_dir("render");
        let err = Pwa::new("Shop")
            .icons(&dir)
            .offline_page("offline.html")
            .assets(|_| anyhow::bail!("boom"), &[], "")
            .unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.contains("offline.html") && text.contains("boom"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn routes_serve_worker_offline_and_icons() {
        use tower::ServiceExt;
        let dir = icon_dir("routes");
        let assets = Pwa::new("Shop")
            .icons(&dir)
            .assets(|_| Ok(String::new()), &paths(), "")
            .unwrap();
        let app: Router = Arc::new(assets).router(true);
        let get = |uri: &str| {
            axum::http::Request::get(uri.to_string())
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let res = app.clone().oneshot(get("/sw.js")).await.unwrap();
        assert_eq!(res.headers()["service-worker-allowed"], "/");
        assert_eq!(res.headers()["cache-control"], "no-cache");
        assert!(res.headers().get("set-cookie").is_none());
        let res = app.clone().oneshot(get(OFFLINE_ROUTE)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let icon = app
            .clone()
            .oneshot(get("/_renox/pwa/icons/icon-192.png"))
            .await
            .unwrap();
        assert_eq!(icon.headers()["content-type"], "image/png");
        let res = app
            .oneshot(get("/_renox/pwa/icons/nope.png"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn manifest_is_valid_json_text() {
        let m = Pwa::new("Q\"uote").manifest(&req("en"), None);
        let text = serde_json::to_string(&m).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["name"], "Q\"uote");
    }
}

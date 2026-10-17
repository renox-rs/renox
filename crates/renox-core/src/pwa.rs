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

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::i18n::{Direction, direction};

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

    #[test]
    fn manifest_is_valid_json_text() {
        let m = Pwa::new("Q\"uote").manifest(&req("en"), None);
        let text = serde_json::to_string(&m).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["name"], "Q\"uote");
    }
}

//! Brand themes: colour math and the contrast core.
//!
//! This part holds the building blocks the theme derivation uses: sRGB and
//! OKLCH conversion, hex parsing, the WCAG 2.x contrast ratio, mixing, and a
//! lightness search that finds the nearest colour passing a check. On top of
//! it, [`Brand`] and [`Theme`] derive the kit's accent tokens (light and dark)
//! from one or two brand colours and refuse colours that cannot reach readable
//! contrast. No I/O.

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest, Sha256};

use self::color::{Oklch, Rgb, nearest_passing};

/// The kit's light page background (`--rx-bg`), one of the colours text is checked on.
const LIGHT_BG: Rgb = Rgb::new(0xf6, 0xf1, 0xe8);
/// The kit's light card surface (`--rx-surface`).
const LIGHT_SURFACE: Rgb = Rgb::new(0xff, 0xfd, 0xf9);
/// The kit's dark page background.
const DARK_BG: Rgb = Rgb::new(0x15, 0x14, 0x12);
/// The kit's dark card surface.
const DARK_SURFACE: Rgb = Rgb::new(0x1d, 0x1b, 0x18);
const WHITE: Rgb = Rgb::new(255, 255, 255);
const NEAR_BLACK: Rgb = Rgb::new(0x1f, 0x24, 0x21);
/// Contrast for text (WCAG AA).
const TEXT_RATIO: f64 = 4.5;
/// Contrast for button labels and chart marks (WCAG AA for large text and graphics).
const LARGE_RATIO: f64 = 3.0;
/// How far (OKLCH lightness) a brand colour may move to become a readable
/// text colour before the brand is refused. Dark mode may lighten further:
/// a very dark brand colour (black, navy) is a normal choice there.
const MAX_SHIFT: f64 = 0.3;
const MAX_SHIFT_DARK: f64 = 0.7;

/// A brand: one colour, an optional second one and an optional display font.
///
/// ```
/// use renox::theme::{Brand, Theme};
///
/// let brand = Brand::new("#0F766E").second("#B8532E").display_font("Poppins");
/// let theme = Theme::from_brand(&brand).unwrap();
/// assert!(theme.css().contains("--rx-accent"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brand {
    color: String,
    second: Option<String>,
    display_font: Option<String>,
}

impl Brand {
    /// A brand with this colour, `#rgb` or `#rrggbb`.
    pub fn new(color: impl Into<String>) -> Self {
        Self {
            color: color.into(),
            second: None,
            display_font: None,
        }
    }

    /// Sets the second brand colour (`--rx-accent-2`).
    #[must_use]
    pub fn second(mut self, color: impl Into<String>) -> Self {
        self.second = Some(color.into());
        self
    }

    /// Sets the display font family (`--rx-font-display`). The name is reduced
    /// to letters, digits, spaces, `-` and `_`; the font itself is not loaded.
    #[must_use]
    pub fn display_font(mut self, family: impl Into<String>) -> Self {
        self.display_font = Some(family.into());
        self
    }
}

/// What went wrong in [`Theme::from_brand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContrastErrorKind {
    /// The text is not a `#rgb` / `#rrggbb` colour.
    InvalidColor,
    /// The colour cannot reach readable contrast.
    LowContrast,
}

/// A brand colour that is malformed or cannot reach readable contrast.
///
/// For [`ContrastErrorKind::LowContrast`] the `suggestion` is the nearest
/// colour that [`Theme::from_brand`] accepts, so a settings form can show it.
///
/// ```
/// use renox::theme::{Brand, Theme};
///
/// let err = Theme::from_brand(&Brand::new("#FFF59D")).unwrap_err();
/// assert!(Theme::from_brand(&Brand::new(err.suggestion.as_str())).is_ok());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContrastError {
    /// What went wrong.
    pub kind: ContrastErrorKind,
    /// The colour as given.
    pub color: String,
    /// The nearest passing colour as `#rrggbb`; empty for an invalid colour.
    pub suggestion: String,
}

impl fmt::Display for ContrastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ContrastErrorKind::InvalidColor => {
                write!(f, "`{}` is not a colour like #0f766e", self.color)
            }
            ContrastErrorKind::LowContrast => write!(
                f,
                "`{}` cannot reach readable contrast; try {}",
                self.color, self.suggestion
            ),
        }
    }
}

impl std::error::Error for ContrastError {}

/// The accent tokens of a brand for one colour scheme.
#[derive(Debug, Clone)]
struct Scheme {
    accent: Rgb,
    hover: Rgb,
    pressed: Rgb,
    text: Rgb,
    on_accent: Rgb,
    soft: Rgb,
    focus: String,
    chart: Rgb,
    second: Option<(Rgb, Rgb)>,
}

impl Scheme {
    /// Derives the tokens for `brand` on `bg`/`surface`; `None` when no text
    /// colour within [`MAX_SHIFT`] (or [`MAX_SHIFT_DARK`]) reaches [`TEXT_RATIO`].
    fn derive(brand: Rgb, second: Option<Rgb>, bg: Rgb, surface: Rgb, dark: bool) -> Option<Self> {
        let shift = if dark { MAX_SHIFT_DARK } else { MAX_SHIFT };
        let readable = |c: Rgb| c.contrast(bg) >= TEXT_RATIO && c.contrast(surface) >= TEXT_RATIO;
        let text = nearest_passing(brand, shift, readable)?;
        let second = match second {
            Some(c) => Some((c, nearest_passing(c, shift, readable)?)),
            None => None,
        };
        let (white, black) = (brand.contrast(WHITE), brand.contrast(NEAR_BLACK));
        let on_accent = if white >= TEXT_RATIO || white >= black {
            WHITE
        } else {
            NEAR_BLACK
        };
        if brand.contrast(on_accent) < LARGE_RATIO {
            return None;
        }
        // Hover and pressed move so the label keeps (or gains) contrast.
        let step = |by: f64| {
            let o = brand.to_oklch();
            let l = if on_accent == WHITE {
                o.l - by
            } else {
                o.l + by
            };
            Oklch { l, ..o }.to_rgb()
        };
        let chart =
            nearest_passing(brand, shift, |c| c.contrast(surface) >= LARGE_RATIO).unwrap_or(brand);
        let soft = surface.mix(brand, if dark { 0.22 } else { 0.14 });
        let (focus_base, alpha) = if dark { (text, 0.45) } else { (brand, 0.35) };
        Some(Self {
            accent: brand,
            hover: step(0.03),
            pressed: step(0.06),
            text,
            on_accent,
            soft,
            focus: format!(
                "rgba({}, {}, {}, {alpha})",
                focus_base.r, focus_base.g, focus_base.b
            ),
            chart,
            second,
        })
    }

    fn tokens(&self) -> BTreeMap<&'static str, String> {
        let mut t = BTreeMap::new();
        t.insert("--rx-accent", self.accent.to_hex());
        t.insert("--rx-accent-hover", self.hover.to_hex());
        t.insert("--rx-accent-pressed", self.pressed.to_hex());
        t.insert("--rx-accent-text", self.text.to_hex());
        t.insert("--rx-on-accent", self.on_accent.to_hex());
        t.insert("--rx-accent-soft", self.soft.to_hex());
        t.insert("--rx-focus", self.focus.clone());
        t.insert("--rx-chart-1", self.chart.to_hex());
        if let Some((c, text)) = self.second {
            t.insert("--rx-accent-2", c.to_hex());
            t.insert("--rx-accent-2-text", text.to_hex());
        }
        t
    }
}

/// The accent tokens derived from a [`Brand`], for light and dark mode.
///
/// Build it with [`Theme::from_brand`]; [`Theme::css`] is the stylesheet,
/// [`Theme::tokens`] and [`Theme::dark_tokens`] the values as data.
#[derive(Debug, Clone)]
pub struct Theme {
    light: BTreeMap<&'static str, String>,
    dark: BTreeMap<&'static str, String>,
    css: String,
    hash: String,
}

impl Theme {
    /// Derives the kit's accent tokens from `brand`.
    ///
    /// Text colours must reach 4.5:1 on the kit's background and surface in
    /// both colour schemes, the label on the accent 3:1. The brand colour
    /// itself is kept; only the text variants move in lightness. When that is
    /// not enough, the error carries the nearest colour that works.
    ///
    /// ```
    /// use renox::theme::{Brand, Theme};
    ///
    /// assert!(Theme::from_brand(&Brand::new("#1E3A5F")).is_ok());
    /// assert!(Theme::from_brand(&Brand::new("#FFF59D")).is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// [`ContrastError`] for a malformed colour or one that cannot reach
    /// readable contrast.
    pub fn from_brand(brand: &Brand) -> Result<Self, ContrastError> {
        let invalid = |c: &str| ContrastError {
            kind: ContrastErrorKind::InvalidColor,
            color: c.to_string(),
            suggestion: String::new(),
        };
        let low = |color: &str, suggestion: String| ContrastError {
            kind: ContrastErrorKind::LowContrast,
            color: color.to_string(),
            suggestion,
        };
        let main = Rgb::parse_hex(&brand.color).ok_or_else(|| invalid(&brand.color))?;
        let second = match &brand.second {
            Some(c) => Some(Rgb::parse_hex(c).ok_or_else(|| invalid(c))?),
            None => None,
        };
        let derive = |main: Rgb, second: Option<Rgb>| {
            let light = Scheme::derive(main, second, LIGHT_BG, LIGHT_SURFACE, false)?;
            let dark = Scheme::derive(main, second, DARK_BG, DARK_SURFACE, true)?;
            Some((light, dark))
        };
        // The suggestion is searched over the whole lightness range, and is
        // checked with the same derivation, so it always passes.
        if derive(main, None).is_none() {
            let nearest = nearest_passing(main, 1.0, |c| derive(c, None).is_some());
            return Err(low(&brand.color, nearest.unwrap_or(main).to_hex()));
        }
        if let (Some(c), Some(raw)) = (second, &brand.second)
            && derive(main, Some(c)).is_none()
        {
            let nearest = nearest_passing(c, 1.0, |x| derive(main, Some(x)).is_some());
            return Err(low(raw, nearest.unwrap_or(c).to_hex()));
        }
        let (light, dark) = derive(main, second).expect("checked above");
        let mut light_tokens = light.tokens();
        if let Some(font) = brand.display_font.as_deref().and_then(sanitize_family) {
            light_tokens.insert("--rx-font-display", format!("\"{font}\", var(--rx-font)"));
        }
        let dark_tokens = dark.tokens();
        let css = render_css(&light_tokens, &dark_tokens);
        let hash = hash_css(&css);
        Ok(Self {
            light: light_tokens,
            dark: dark_tokens,
            css,
            hash,
        })
    }

    /// The stylesheet: a `:root` block for light mode and the kit's two dark
    /// selectors (`prefers-color-scheme: dark` and `[data-theme="dark"]`).
    /// Load it after the kit's CSS.
    pub fn css(&self) -> &str {
        &self.css
    }

    /// A short content hash of [`Theme::css`] (16 hex digits), the key of the
    /// theme's cacheable stylesheet URL.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The derived tokens for light mode, such as `--rx-accent` => `#0f766e`.
    pub fn tokens(&self) -> BTreeMap<&'static str, String> {
        self.light.clone()
    }

    /// The derived tokens for dark mode.
    pub fn dark_tokens(&self) -> BTreeMap<&'static str, String> {
        self.dark.clone()
    }
}

fn sanitize_family(name: &str) -> Option<String> {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .collect();
    let clean = clean.trim().to_string();
    (!clean.is_empty()).then_some(clean)
}

fn declarations(tokens: &BTreeMap<&'static str, String>) -> String {
    tokens.iter().map(|(k, v)| format!("{k}:{v};")).collect()
}

fn render_css(
    light: &BTreeMap<&'static str, String>,
    dark: &BTreeMap<&'static str, String>,
) -> String {
    let dark = declarations(dark);
    format!(
        ":root{{{}}}\n@media (prefers-color-scheme:dark){{:root:not([data-theme=\"light\"]){{{dark}}}}}\n:root[data-theme=\"dark\"]{{{dark}}}\n",
        declarations(light)
    )
}

fn hash_css(css: &str) -> String {
    Sha256::digest(css.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    fn rgb(t: &BTreeMap<&'static str, String>, k: &str) -> Rgb {
        Rgb::parse_hex(&t[k]).unwrap_or_else(|| panic!("{k} is not a hex colour: {}", t[k]))
    }

    fn assert_ratios(brand: &str) {
        let theme = Theme::from_brand(&Brand::new(brand).second("#b8532e")).expect(brand);
        for (t, bg, surface) in [
            (theme.tokens(), LIGHT_BG, LIGHT_SURFACE),
            (theme.dark_tokens(), DARK_BG, DARK_SURFACE),
        ] {
            for key in ["--rx-accent-text", "--rx-accent-2-text"] {
                let c = rgb(&t, key);
                assert!(c.contrast(bg) >= 4.5, "{brand} {key} on bg");
                assert!(c.contrast(surface) >= 4.5, "{brand} {key} on surface");
            }
            let accent = rgb(&t, "--rx-accent");
            assert!(
                rgb(&t, "--rx-on-accent").contrast(accent) >= 3.0,
                "{brand} label"
            );
            for key in [
                "--rx-accent-hover",
                "--rx-accent-pressed",
                "--rx-accent-soft",
                "--rx-chart-1",
            ] {
                assert!(t.contains_key(key), "{key}");
            }
            assert!(t["--rx-focus"].starts_with("rgba("));
        }
    }

    #[test]
    fn accepted_colours_meet_the_ratios_in_both_modes() {
        for c in [
            "#0b6e66", "#1E3A5F", "#0F766E", "#b8532e", "#7c3aed", "#000", "#333",
        ] {
            assert_ratios(c);
        }
    }

    #[test]
    fn the_kit_default_accent_is_kept() {
        let t = Theme::from_brand(&Brand::new("#0b6e66")).unwrap().tokens();
        assert_eq!(t["--rx-accent"], "#0b6e66");
        assert_eq!(t["--rx-on-accent"], "#ffffff");
        assert_eq!(t["--rx-accent-text"], "#0b6e66");
    }

    #[test]
    fn pale_yellow_is_refused_with_a_passing_suggestion() {
        let err = Theme::from_brand(&Brand::new("#FFF59D")).unwrap_err();
        assert_eq!(err.kind, ContrastErrorKind::LowContrast);
        assert!(err.to_string().contains(&err.suggestion));
        assert_ne!(err.suggestion, "#fff59d");
        assert_ratios(&err.suggestion);
    }

    #[test]
    fn a_failing_second_colour_is_refused_and_named() {
        let err = Theme::from_brand(&Brand::new("#0f766e").second("#fff59d")).unwrap_err();
        assert_eq!(err.color, "#fff59d");
        Theme::from_brand(&Brand::new("#0f766e").second(err.suggestion.as_str())).unwrap();
    }

    #[test]
    fn malformed_hex_is_a_distinct_error() {
        for bad in ["", "red", "#12", "#ggg"] {
            let err = Theme::from_brand(&Brand::new(bad)).unwrap_err();
            assert_eq!(err.kind, ContrastErrorKind::InvalidColor, "{bad}");
            assert!(err.suggestion.is_empty());
        }
        let err = Theme::from_brand(&Brand::new("#0f766e").second("nope")).unwrap_err();
        assert_eq!(err.kind, ContrastErrorKind::InvalidColor);
    }

    #[test]
    fn second_colour_and_display_font_are_optional_tokens() {
        let plain = Theme::from_brand(&Brand::new("#0f766e")).unwrap().tokens();
        assert!(!plain.contains_key("--rx-accent-2"));
        assert!(!plain.contains_key("--rx-font-display"));
        let theme = Theme::from_brand(
            &Brand::new("#0f766e")
                .second("#b8532e")
                .display_font("Poppins\";}body{x:y"),
        )
        .unwrap();
        let t = theme.tokens();
        assert_eq!(t["--rx-accent-2"], "#b8532e");
        assert_eq!(t["--rx-font-display"], "\"Poppinsbodyxy\", var(--rx-font)");
        // The font only goes into light mode (dark inherits it).
        assert!(!theme.dark_tokens().contains_key("--rx-font-display"));
    }

    #[test]
    fn css_has_the_kits_three_selector_blocks() {
        let css = Theme::from_brand(&Brand::new("#1e3a5f"))
            .unwrap()
            .css()
            .to_string();
        assert!(css.starts_with(":root{--rx-accent:"));
        assert!(
            css.contains("@media (prefers-color-scheme:dark){:root:not([data-theme=\"light\"]){")
        );
        assert!(css.contains(":root[data-theme=\"dark\"]{"));
        // No `:where` on the generated selectors, so the brand is at least as
        // specific as the kit's `data-rx-theme` blocks and, loaded later, wins.
        assert!(!css.contains(":where"));
    }

    #[test]
    fn hash_follows_the_content() {
        let a = Theme::from_brand(&Brand::new("#1e3a5f")).unwrap();
        let b = Theme::from_brand(&Brand::new("#1E3A5F")).unwrap();
        let c = Theme::from_brand(&Brand::new("#0f766e")).unwrap();
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), c.hash());
        assert_eq!(a.hash().len(), 16);
    }
}

pub(crate) mod color {
    //! sRGB, OKLCH and WCAG contrast math.

    /// An 8-bit sRGB colour.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Rgb {
        pub(crate) r: u8,
        pub(crate) g: u8,
        pub(crate) b: u8,
    }

    /// A colour in OKLCH: lightness `l` in 0..=1, chroma `c`, hue `h` in degrees.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct Oklch {
        pub(crate) l: f64,
        pub(crate) c: f64,
        pub(crate) h: f64,
    }

    impl Rgb {
        /// Builds a colour from its three channels.
        pub(crate) const fn new(r: u8, g: u8, b: u8) -> Self {
            Self { r, g, b }
        }

        /// Parses `#rgb` or `#rrggbb` (the `#` is optional, case is ignored).
        pub(crate) fn parse_hex(s: &str) -> Option<Self> {
            let s = s.trim();
            let s = s.strip_prefix('#').unwrap_or(s);
            if !s.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let digit = |i: usize| u8::from_str_radix(&s[i..=i], 16).ok();
            let pair = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
            match s.len() {
                3 => Some(Self::new(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
                6 => Some(Self::new(pair(0)?, pair(2)?, pair(4)?)),
                _ => None,
            }
        }

        /// The colour as lowercase `#rrggbb`.
        pub(crate) fn to_hex(self) -> String {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        }

        /// WCAG relative luminance, 0 (black) to 1 (white).
        pub(crate) fn luminance(self) -> f64 {
            let lin = |v: u8| srgb_to_linear(f64::from(v) / 255.0);
            0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
        }

        /// The WCAG contrast ratio against `other`, 1.0 to 21.0.
        pub(crate) fn contrast(self, other: Rgb) -> f64 {
            let (a, b) = (self.luminance(), other.luminance());
            let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
            (hi + 0.05) / (lo + 0.05)
        }

        /// This colour in OKLCH.
        pub(crate) fn to_oklch(self) -> Oklch {
            let (l, a, b) = self.to_oklab();
            let c = a.hypot(b);
            let h = if c < 1e-9 {
                0.0
            } else {
                b.atan2(a).to_degrees().rem_euclid(360.0)
            };
            Oklch { l, c, h }
        }

        fn to_oklab(self) -> (f64, f64, f64) {
            let r = srgb_to_linear(f64::from(self.r) / 255.0);
            let g = srgb_to_linear(f64::from(self.g) / 255.0);
            let b = srgb_to_linear(f64::from(self.b) / 255.0);
            let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
            let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
            let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
            (
                0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
                1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
                0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
            )
        }

        fn from_oklab(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
            let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
            let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
            let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
            let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
            (
                4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
                -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
                -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s,
            )
        }

        /// Mixes `self` with `other` in OKLab; `t` is the share of `other`, 0..=1.
        pub(crate) fn mix(self, other: Rgb, t: f64) -> Rgb {
            let t = t.clamp(0.0, 1.0);
            let (a, b) = (self.to_oklab(), other.to_oklab());
            let lerp = |x: f64, y: f64| x + (y - x) * t;
            let (r, g, bl) = Self::from_oklab(lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2));
            from_linear(r, g, bl)
        }
    }

    impl Oklch {
        /// The colour as sRGB. When it is outside the sRGB gamut the chroma is
        /// reduced (hue and lightness kept) until it fits.
        pub(crate) fn to_rgb(self) -> Rgb {
            let l = self.l.clamp(0.0, 1.0);
            let lin = |c: f64| {
                let (a, b) = (c * self.h.to_radians().cos(), c * self.h.to_radians().sin());
                Rgb::from_oklab(l, a, b)
            };
            let in_gamut = |(r, g, b): (f64, f64, f64)| {
                let ok = |v: f64| (-1e-4..=1.0 + 1e-4).contains(&v);
                ok(r) && ok(g) && ok(b)
            };
            let mut c = self.c.max(0.0);
            if !in_gamut(lin(c)) {
                let (mut lo, mut hi) = (0.0, c);
                for _ in 0..24 {
                    let mid = (lo + hi) / 2.0;
                    if in_gamut(lin(mid)) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                c = lo;
            }
            let (r, g, b) = lin(c);
            from_linear(r, g, b)
        }
    }

    fn srgb_to_linear(v: f64) -> f64 {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }

    fn linear_to_srgb(v: f64) -> f64 {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    }

    fn from_linear(r: f64, g: f64, b: f64) -> Rgb {
        let ch = |v: f64| (linear_to_srgb(v) * 255.0).round() as u8;
        Rgb::new(ch(r), ch(g), ch(b))
    }

    /// The colour nearest to `from` in lightness (hue and chroma kept) for which
    /// `passes` holds, searching at most `max_shift` of OKLCH lightness up or down.
    /// `from` itself is returned when it already passes. `None` when nothing in
    /// range passes.
    pub(crate) fn nearest_passing(
        from: Rgb,
        max_shift: f64,
        passes: impl Fn(Rgb) -> bool,
    ) -> Option<Rgb> {
        if passes(from) {
            return Some(from);
        }
        let base = from.to_oklch();
        let step = 0.002;
        let mut i = 1.0;
        while i * step <= max_shift + 1e-12 {
            let shift = i * step;
            let at = |l: f64| {
                (0.0..=1.0)
                    .contains(&l)
                    .then(|| Oklch { l, ..base }.to_rgb())
                    .filter(|c| passes(*c))
            };
            // When both directions pass, keep the one closer to the original.
            match (at(base.l + shift), at(base.l - shift)) {
                (Some(u), Some(d)) => {
                    return Some(if dist(from, u) <= dist(from, d) { u } else { d });
                }
                (Some(c), None) | (None, Some(c)) => return Some(c),
                (None, None) => {}
            }
            i += 1.0;
        }
        None
    }

    fn dist(a: Rgb, b: Rgb) -> f64 {
        let d = |x: u8, y: u8| (f64::from(x) - f64::from(y)).powi(2);
        d(a.r, b.r) + d(a.g, b.g) + d(a.b, b.b)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn hex_parses_short_long_and_rejects_junk() {
            assert_eq!(Rgb::parse_hex("#0F766E"), Some(Rgb::new(15, 118, 110)));
            assert_eq!(Rgb::parse_hex("fa0"), Some(Rgb::new(255, 170, 0)));
            assert_eq!(Rgb::parse_hex(" #abc "), Some(Rgb::new(170, 187, 204)));
            for bad in ["", "#", "#12", "#12345", "#1234567", "#ggg", "red", "#é12"] {
                assert_eq!(Rgb::parse_hex(bad), None, "{bad}");
            }
            assert_eq!(Rgb::new(11, 110, 102).to_hex(), "#0b6e66");
        }

        #[test]
        fn contrast_matches_wcag_reference_values() {
            let (black, white) = (Rgb::new(0, 0, 0), Rgb::new(255, 255, 255));
            assert!((black.contrast(white) - 21.0).abs() < 1e-9);
            assert!((white.contrast(black) - 21.0).abs() < 1e-9);
            assert!((white.contrast(white) - 1.0).abs() < 1e-9);
            // #777 on white is the well-known 4.48:1.
            let grey = Rgb::parse_hex("#777777").unwrap();
            assert!((grey.contrast(white) - 4.48).abs() < 0.01);
        }

        #[test]
        fn oklch_round_trips_and_matches_reference() {
            let white = Rgb::new(255, 255, 255).to_oklch();
            assert!((white.l - 1.0).abs() < 1e-3 && white.c < 1e-3);
            let red = Rgb::new(255, 0, 0).to_oklch();
            assert!((red.l - 0.628).abs() < 0.002 && (red.c - 0.258).abs() < 0.002);
            assert!((red.h - 29.2).abs() < 0.5);
            for hex in [
                "#0f766e", "#b8532e", "#1e3a5f", "#fff59d", "#808080", "#000000",
            ] {
                let c = Rgb::parse_hex(hex).unwrap();
                assert_eq!(c.to_oklch().to_rgb(), c, "{hex}");
            }
        }

        #[test]
        fn out_of_gamut_chroma_is_reduced_keeping_lightness() {
            let wild = Oklch {
                l: 0.7,
                c: 0.5,
                h: 150.0,
            }
            .to_rgb();
            let back = wild.to_oklch();
            assert!((back.l - 0.7).abs() < 0.02);
            assert!((back.h - 150.0).abs() < 3.0);
        }

        #[test]
        fn mix_endpoints_and_midpoint() {
            let (a, b) = (Rgb::new(0, 0, 0), Rgb::new(255, 255, 255));
            assert_eq!(a.mix(b, 0.0), a);
            assert_eq!(a.mix(b, 1.0), b);
            let mid = a.mix(b, 0.5);
            assert!(mid.r > 90 && mid.r < 130 && mid.r == mid.g && mid.g == mid.b);
        }

        #[test]
        fn nearest_passing_keeps_a_passing_colour_and_moves_a_failing_one() {
            let white = Rgb::new(255, 255, 255);
            let teal = Rgb::parse_hex("#0b6e66").unwrap();
            assert_eq!(
                nearest_passing(teal, 0.3, |c| c.contrast(white) >= 4.5),
                Some(teal)
            );

            let yellow = Rgb::parse_hex("#fff59d").unwrap();
            let fixed = nearest_passing(yellow, 0.7, |c| c.contrast(white) >= 4.5).unwrap();
            assert!(fixed.contrast(white) >= 4.5);
            // Hue is kept.
            assert!((fixed.to_oklch().h - yellow.to_oklch().h).abs() < 8.0);
            // Too small a range finds nothing.
            assert_eq!(
                nearest_passing(yellow, 0.1, |c| c.contrast(white) >= 4.5),
                None
            );
        }

        #[test]
        fn nearest_passing_is_minimal_in_lightness() {
            let white = Rgb::new(255, 255, 255);
            let mid = Rgb::parse_hex("#8a8a8a").unwrap();
            let fixed = nearest_passing(mid, 0.5, |c| c.contrast(white) >= 4.5).unwrap();
            assert!(fixed.contrast(white) >= 4.5);
            // A hair closer to the original lightness no longer passes.
            let (from_l, o) = (mid.to_oklch().l, fixed.to_oklch());
            let closer = if o.l < from_l { o.l + 0.01 } else { o.l - 0.01 };
            assert!(Oklch { l: closer, ..o }.to_rgb().contrast(white) < 4.5);
        }
    }
}

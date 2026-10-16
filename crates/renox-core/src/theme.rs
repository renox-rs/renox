//! Brand themes: colour math and the contrast core.
//!
//! This part holds the building blocks the theme derivation uses: sRGB and
//! OKLCH conversion, hex parsing, the WCAG 2.x contrast ratio, mixing, and a
//! lightness search that finds the nearest colour passing a check. It has no
//! dependency and no I/O.

#[allow(dead_code)] // used by the theme derivation added on top of this core
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

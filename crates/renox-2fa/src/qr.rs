//! The QR code of the `otpauth://` URI, as an inline SVG.

use qrcode::QrCode;
use qrcode::render::svg;

/// `text` as an SVG QR code, `size` pixels wide at least, black on white
/// (authenticator apps need the contrast, also in dark mode).
pub fn svg(text: &str, size: u32) -> Option<String> {
    let code = QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color>()
            .min_dimensions(size, size)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .quiet_zone(true)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn draws_an_svg() {
        let svg = super::svg(
            "otpauth://totp/Acme:ana@example.com?secret=JBSWY3DPEHPK3PXP",
            200,
        )
        .unwrap();
        assert!(
            svg.starts_with("<?xml") || svg.starts_with("<svg"),
            "{}",
            &svg[..60]
        );
        assert!(svg.contains("<path"));
    }
}

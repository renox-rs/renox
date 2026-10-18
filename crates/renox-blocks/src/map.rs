//! The map blocks' settings: where the tiles come from.

use renox::minijinja::Value;

/// The tile URL template, when it is usable: an `http(s)` URL.
fn usable(tiles: &str) -> bool {
    let tiles = tiles.trim();
    tiles.starts_with("https://") || tiles.starts_with("http://")
}

/// `{{ renox_blocks_map() }}`: `{tiles, attribution}` from the module's
/// settings, else `BLOCKS_MAP_TILES` / `BLOCKS_MAP_ATTRIBUTION` in the
/// environment (`.env` included); nothing without usable tiles. Empty
/// values count as unset, and a tile URL that is not `http(s)` is ignored.
pub(crate) fn settings(tiles: Option<&str>, attribution: Option<&str>) -> Value {
    let from_env = |name: &str| std::env::var(name).ok();
    let pick = |own: Option<&str>, name: &str| {
        own.map(str::to_owned)
            .or_else(|| from_env(name))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let Some(tiles) = pick(tiles, "BLOCKS_MAP_TILES").filter(|t| usable(t)) else {
        return Value::from(());
    };
    let attribution = pick(attribution, "BLOCKS_MAP_ATTRIBUTION").unwrap_or_default();
    renox::minijinja::context! { tiles, attribution }
}

#[cfg(test)]
mod tests {
    use super::settings;

    #[test]
    fn tiles_must_be_an_http_url() {
        let tiles = "https://tiles.example.com/{z}/{x}/{y}.png";
        let got = settings(Some(tiles), Some("(c) Example"));
        assert_eq!(got.get_attr("tiles").unwrap().to_string(), tiles);
        assert_eq!(
            got.get_attr("attribution").unwrap().to_string(),
            "(c) Example"
        );
        assert!(settings(Some("javascript:alert(1)"), None).is_none());
        assert!(settings(Some("  "), None).is_none());
        assert!(settings(Some("/tiles/{z}/{x}/{y}.png"), None).is_none());
    }
}

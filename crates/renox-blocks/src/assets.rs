//! The blocks' script and stylesheet, compiled into the crate and served by
//! the app (`Registry::asset`) with a year-long cache.
//!
//! A page with a block loads two files: the stylesheet and `blocks.js`, a
//! JavaScript module. The module imports a block's own code (`gallery.js`,
//! `kanban.js`, …) only when it finds that block on the page, and a browser
//! runs each module once per page, even when htmx brings a block in later.
//! Every file's name carries a hash of its content.

use std::sync::LazyLock;

use renox::Registry;

/// Where the files are served.
const BASE: &str = "/_renox/blocks";

const CSS: &str = include_str!("../assets/blocks.css");
const LOADER: &str = include_str!("../assets/blocks.js");

/// The blocks that have code: `(name, source)`. The loader finds each one's
/// file in a `data-{name}` attribute of its `<script>` tag.
const PARTS: [(&str, &str); 14] = [
    ("gallery", include_str!("../assets/parts/gallery.js")),
    ("range", include_str!("../assets/parts/range.js")),
    ("quantity", include_str!("../assets/parts/quantity.js")),
    ("keypad", include_str!("../assets/parts/keypad.js")),
    ("kanban", include_str!("../assets/parts/kanban.js")),
    ("datetime", include_str!("../assets/parts/datetime.js")),
    ("history", include_str!("../assets/parts/history.js")),
    ("receipt", include_str!("../assets/parts/receipt.js")),
    ("week", include_str!("../assets/parts/week.js")),
    ("signature", include_str!("../assets/parts/signature.js")),
    ("camera", include_str!("../assets/parts/camera.js")),
    ("map", include_str!("../assets/parts/map.js")),
    ("geolocate", include_str!("../assets/parts/geolocate.js")),
    ("barcode", include_str!("../assets/parts/barcode.js")),
];

/// Leaflet, vendored (assets/vendor/leaflet/NOTICE). `map.js` adds these two
/// files to a page that has a map with tiles, and no other page fetches them.
const LEAFLET_JS: &str = include_str!("../assets/vendor/leaflet/leaflet.js");
const LEAFLET_CSS: &str = include_str!("../assets/vendor/leaflet/leaflet.css");

const JS: &str = "text/javascript; charset=utf-8";

/// A served file.
struct File {
    path: &'static str,
    content_type: &'static str,
    body: &'static str,
}

fn file(name: &str, extension: &str, content_type: &'static str, body: &'static str) -> File {
    let path = format!("{BASE}/{name}-{:016x}.{extension}", fnv1a(body));
    File {
        path: Box::leak(path.into_boxed_str()),
        content_type,
        body,
    }
}

/// The stylesheet, the loader, each block's code (in `PARTS` order), then
/// Leaflet's script and stylesheet.
static FILES: LazyLock<Vec<File>> = LazyLock::new(|| {
    let mut files = vec![
        file("blocks", "css", "text/css; charset=utf-8", CSS),
        file("blocks", "js", JS, LOADER),
    ];
    files.extend(PARTS.iter().map(|(name, body)| file(name, "js", JS, body)));
    files.push(file("leaflet", "js", JS, LEAFLET_JS));
    files.push(file(
        "leaflet",
        "css",
        "text/css; charset=utf-8",
        LEAFLET_CSS,
    ));
    files
});

/// Serves every file.
pub(crate) fn register(app: &mut Registry) {
    for file in FILES.iter() {
        app.asset(file.path, file.content_type, file.body.as_bytes());
    }
}

/// `{{ renox_blocks() }}`: the stylesheet and the loader, which every block
/// prints once per page. The loader is told where each block's code is.
pub(crate) fn tags() -> String {
    let parts: String = PARTS
        .iter()
        .zip(&FILES[2..])
        .map(|((name, _), file)| format!(" data-{name}=\"{}\"", file.path))
        .collect();
    let leaflet = &FILES[2 + PARTS.len()..];
    format!(
        "<link rel=\"stylesheet\" href=\"{}\">\n<script type=\"module\" src=\"{}\" data-renox-blocks{parts} data-leaflet-js=\"{}\" data-leaflet-css=\"{}\"></script>",
        FILES[0].path, FILES[1].path, leaflet[0].path, leaflet[1].path
    )
}

/// FNV-1a, as Renox hashes its own files' names.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::{FILES, PARTS, tags};

    // Every block's file is named in the loader's tag, and the loader
    // knows each block (its PARTS list names the same blocks).
    #[test]
    fn the_tag_names_every_part_the_loader_knows() {
        let tag = tags();
        assert_eq!(FILES.len(), PARTS.len() + 4);
        for (name, _) in PARTS {
            assert!(
                tag.contains(&format!(" data-{name}=\"/_renox/blocks/{name}-")),
                "{tag}"
            );
            assert!(super::LOADER.contains(&format!("[\"{name}\", ")), "{name}");
        }
    }

    // Leaflet is named for map.js to add, never loaded by the loader, and its
    // licence and NOTICE ship with it.
    #[test]
    fn leaflet_is_served_but_only_named() {
        let tag = tags();
        assert!(
            tag.contains(" data-leaflet-js=\"/_renox/blocks/leaflet-"),
            "{tag}"
        );
        assert!(
            tag.contains(" data-leaflet-css=\"/_renox/blocks/leaflet-"),
            "{tag}"
        );
        assert!(super::LEAFLET_JS.contains("Leaflet 1.9.4"));
        assert!(!super::LOADER.contains("leaflet"));
        assert!(super::PARTS.iter().all(|(name, _)| *name != "leaflet"));
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/vendor/leaflet");
        let licence = std::fs::read_to_string(format!("{dir}/LICENSE")).unwrap();
        assert!(licence.contains("BSD 2-Clause"));
        let notice = std::fs::read_to_string(format!("{dir}/NOTICE")).unwrap();
        assert!(notice.contains("Leaflet 1.9.4"));
    }

    // map.js reads the paths the tag carries, and the loader starts it for
    // both blocks.
    #[test]
    fn map_js_adds_leaflet_from_the_tag() {
        let map = super::PARTS.iter().find(|(n, _)| *n == "map").unwrap().1;
        assert!(map.contains("data-leaflet-js") && map.contains("data-leaflet-css"));
        assert!(super::LOADER.contains("[data-rx-map-picker]"));
    }
}

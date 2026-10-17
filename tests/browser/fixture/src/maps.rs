//! Map block pages (maps.test.mjs, #47): `/maps` has the picker in its three
//! modes and a read-only map, `/maps/plain` has a block that is not a map
//! (so the test can check that no Leaflet file is requested), and `/echo`
//! (in `main.rs`) shows what the form posted. The tiles come from
//! `BLOCKS_MAP_TILES`, which the test points at a local stub; the stub's
//! origin is allowed in `img-src` here, as an app with an `http://` tile host
//! must do (the default policy allows `https:` hosts only).

use renox::prelude::*;

pub struct Maps;

impl Module for Maps {
    fn name(&self) -> &'static str {
        "maps"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/maps", || async { view("maps.html", context! {}) })
            .get("/maps/plain", || async {
                view("maps_plain.html", context! {})
            })
    }
}

/// The origin (`http://127.0.0.1:4000`) of the tile URL template in
/// `BLOCKS_MAP_TILES`, if there is one.
pub fn tiles_origin() -> Option<String> {
    let tiles = std::env::var("BLOCKS_MAP_TILES").ok()?;
    let (scheme, rest) = tiles.split_once("://")?;
    let host = rest.split('/').next()?;
    Some(format!("{scheme}://{host}"))
}

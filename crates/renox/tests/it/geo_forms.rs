//! #47: what the map picker posts, read by `Valid<T>`: a point as the nested
//! fields `spot[lat]` and `spot[lng]`, a circle or polygon as GeoJSON text.

use renox::geo::{Circle, Point, Polygon};
use renox::prelude::*;
use renox::testing::TestApp;

#[derive(serde::Deserialize, Validate)]
struct Visit {
    spot: Option<Point>,
    #[validate(point)]
    #[serde(default)]
    centre: String,
    #[serde(default)]
    zone: String,
}

struct Visits;

impl Module for Visits {
    fn name(&self) -> &'static str {
        "visits"
    }

    fn routes(&self) -> Routes {
        Routes::new().post("/visit", |Valid(visit): Valid<Visit>| async move {
            let spot = visit.spot.expect("a spot");
            let zone: Polygon = renox::serde_json::from_str(&visit.zone).expect("a polygon");
            let circle: Circle = renox::serde_json::from_str(&visit.centre).expect("a circle");
            format!(
                "{} {} in-zone={} in-circle={}",
                spot.lat(),
                spot.lng(),
                zone.contains(spot),
                circle.contains(spot)
            )
        })
    }
}

const ZONE: &str = r#"{"type":"Polygon","coordinates":[[[55.0,25.0],[56.0,25.0],[56.0,26.0],[55.0,26.0],[55.0,25.0]]]}"#;
const CIRCLE: &str = r#"{"type":"Point","coordinates":[55.2708,25.2048],"radius":150}"#;

/// The picker's fields arrive as a `Point` (lat and lng the right way round),
/// and the GeoJSON texts read as a polygon and a circle.
#[renox::test]
async fn the_pickers_fields_arrive_as_geo_values() {
    let app = TestApp::new(App::new().module(Visits)).await;
    app.post(
        "/visit",
        &[
            ("spot[lat]", "25.2048"),
            ("spot[lng]", "55.2708"),
            ("centre", CIRCLE),
            ("zone", ZONE),
        ],
    )
    .await
    .assert_ok()
    .assert_see("25.2048 55.2708 in-zone=true in-circle=true");
}

/// Coordinates out of range are validation errors, not a crash.
#[renox::test]
async fn invalid_coordinates_are_validation_errors() {
    let app = TestApp::new(App::new().module(Visits)).await;
    app.htmx()
        .post(
            "/visit",
            &[
                ("spot[lat]", "95"),
                ("spot[lng]", "55.2708"),
                ("centre", CIRCLE),
                ("zone", ZONE),
            ],
        )
        .await
        .assert_invalid("spot");

    // The `point` rule on the circle's GeoJSON text.
    app.htmx()
        .post(
            "/visit",
            &[
                ("spot[lat]", "25"),
                ("spot[lng]", "55"),
                ("centre", r#"{"type":"Point","coordinates":[255,25]}"#),
                ("zone", ZONE),
            ],
        )
        .await
        .assert_invalid("centre");
}

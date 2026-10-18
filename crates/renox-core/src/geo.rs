//! Geographic values: a validated [`Point`], a [`Circle`] and a [`Polygon`],
//! with distances in metres and containment tests, so a check-in, a delivery
//! stop or a service area can be verified on the server without a GIS stack.
//!
//! The API order is `(lat, lng)` everywhere (`Point::new`, the form fields
//! `lat` and `lng`). GeoJSON is the other way round: its coordinates are
//! `[lng, lat]`. The (de)serialization here converts between the two, and the
//! tests fail if they are swapped.
//!
//! ```
//! use renox::geo::{Circle, Point, Polygon};
//!
//! let here = Point::new(25.2048, 55.2708).unwrap();
//! let site = Circle::new(Point::new(25.2050, 55.2710).unwrap(), 150.0).unwrap();
//! assert!(site.contains(here));
//!
//! let area: Polygon = renox::serde_json::from_str(
//!     r#"{"type":"Polygon","coordinates":[[[55.0,25.0],[56.0,25.0],[56.0,26.0],[55.0,26.0],[55.0,25.0]]]}"#,
//! ).unwrap();
//! assert!(area.contains(here));
//! assert!(here.distance_to(Point::new(25.2048, 55.2708).unwrap()) < 1e-9);
//! ```
//!
//! Edge behaviour, decided once: the boundary counts as inside. A point
//! exactly on a circle's radius, on a polygon's edge or on one of its
//! vertices is contained (with a tolerance of a micrometre, or `1e-9` degrees,
//! for floating point error). The boundary of a hole also belongs to the
//! polygon: the hole is open. A polygon is tested on a flat plane of
//! longitude and latitude, with longitudes shifted relative to its first
//! vertex, so one crossing the antimeridian works; one that spans more than
//! 180 degrees of longitude is not supported.

use std::fmt;

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};

/// The mean radius of the earth in metres, used by the haversine distance.
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Tolerance in metres when a point is compared with a circle's radius.
const RADIUS_EPSILON_M: f64 = 1e-6;
/// Tolerance in degrees when a point is compared with a polygon edge.
const EDGE_EPSILON_DEG: f64 = 1e-9;

/// Why a geographic value was rejected.
///
/// ```
/// use renox::geo::{GeoError, Point};
///
/// assert_eq!(Point::new(91.0, 0.0).unwrap_err(), GeoError::Latitude);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GeoError {
    /// The latitude is not a finite number between -90 and 90.
    Latitude,
    /// The longitude is not a finite number between -180 and 180.
    Longitude,
    /// A circle radius that is not a positive, finite number of metres.
    Radius,
    /// A ring with fewer than three distinct vertices.
    Ring,
    /// Text or JSON that is not the expected GeoJSON or lat/lng shape.
    Invalid(String),
}

impl fmt::Display for GeoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Latitude => f.write_str("latitude must be a number between -90 and 90"),
            Self::Longitude => f.write_str("longitude must be a number between -180 and 180"),
            Self::Radius => f.write_str("radius must be a positive number of metres"),
            Self::Ring => f.write_str("a polygon ring needs at least three distinct points"),
            Self::Invalid(why) => write!(f, "invalid geographic value: {why}"),
        }
    }
}

impl std::error::Error for GeoError {}

/// A position on earth: latitude and longitude in degrees, validated.
///
/// ```
/// use renox::geo::Point;
///
/// let dubai = Point::new(25.2048, 55.2708).unwrap();
/// let abu_dhabi = Point::new(24.4539, 54.3773).unwrap();
/// let km = dubai.distance_to(abu_dhabi) / 1000.0;
/// assert!((km - 123.0).abs() < 1.0);
/// assert!(Point::new(f64::NAN, 0.0).is_err());
/// ```
///
/// It deserializes from `{"lat": .., "lng": ..}`, from a GeoJSON `Point`
/// (`[lng, lat]`) and from the form fields `lat` and `lng` (as text), and
/// serializes as `{"lat": .., "lng": ..}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    lat: f64,
    lng: f64,
}

impl Point {
    /// A point at `lat` (-90 to 90) and `lng` (-180 to 180) degrees.
    pub fn new(lat: f64, lng: f64) -> Result<Self, GeoError> {
        if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
            return Err(GeoError::Latitude);
        }
        if !lng.is_finite() || !(-180.0..=180.0).contains(&lng) {
            return Err(GeoError::Longitude);
        }
        Ok(Self { lat, lng })
    }

    /// The latitude in degrees.
    pub fn lat(&self) -> f64 {
        self.lat
    }

    /// The longitude in degrees.
    pub fn lng(&self) -> f64 {
        self.lng
    }

    /// The great-circle distance to `other` in metres (haversine, mean earth
    /// radius), the short way round the globe, also across the antimeridian.
    ///
    /// ```
    /// use renox::geo::Point;
    ///
    /// let a = Point::new(0.0, 179.9).unwrap();
    /// let b = Point::new(0.0, -179.9).unwrap();
    /// assert!(a.distance_to(b) < 25_000.0);
    /// ```
    pub fn distance_to(&self, other: Point) -> f64 {
        let (lat1, lat2) = (self.lat.to_radians(), other.lat.to_radians());
        let dlat = lat2 - lat1;
        let dlng = (other.lng - self.lng).to_radians();
        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlng / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_M * a.sqrt().min(1.0).asin()
    }

    /// The point as a GeoJSON `Point` geometry (`[lng, lat]`).
    pub fn to_geojson(&self) -> Value {
        json!({"type": "Point", "coordinates": [self.lng, self.lat]})
    }

    fn from_lng_lat(coordinates: &[f64]) -> Result<Self, GeoError> {
        match coordinates {
            [lng, lat, ..] => Self::new(*lat, *lng),
            _ => Err(GeoError::Invalid("a position needs [lng, lat]".into())),
        }
    }
}

impl Serialize for Point {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        json!({"lat": self.lat, "lng": self.lng}).serialize(s)
    }
}

/// A number that arrives as a JSON number or as form text.
struct Num(f64);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Num;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Num, E> {
                Ok(Num(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Num, E> {
                v.trim()
                    .parse::<f64>()
                    .map(Num)
                    .map_err(|_| E::custom("not a number"))
            }
        }
        d.deserialize_any(V)
    }
}

impl<'de> Deserialize<'de> for Point {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Point;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a point: {lat, lng} or a GeoJSON Point")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Point, A::Error> {
                let (mut lat, mut lng, mut coordinates) = (None, None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "lat" | "latitude" => lat = Some(map.next_value::<Num>()?.0),
                        "lng" | "longitude" => lng = Some(map.next_value::<Num>()?.0),
                        "coordinates" => {
                            let list = map.next_value::<Vec<Num>>()?;
                            coordinates = Some(list.into_iter().map(|n| n.0).collect::<Vec<_>>());
                        }
                        "type" => {
                            let kind = map.next_value::<String>()?;
                            if kind != "Point" {
                                return Err(de::Error::custom("GeoJSON type must be Point"));
                            }
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                let point = match (coordinates, lat, lng) {
                    (Some(c), _, _) => Point::from_lng_lat(&c),
                    (None, Some(lat), Some(lng)) => Point::new(lat, lng),
                    _ => Err(GeoError::Invalid("both lat and lng are required".into())),
                };
                point.map_err(de::Error::custom)
            }
        }
        d.deserialize_map(V)
    }
}

/// A circle on earth: a centre and a radius in metres.
///
/// ```
/// use renox::geo::{Circle, Point};
///
/// let site = Circle::new(Point::new(25.2050, 55.2710).unwrap(), 150.0).unwrap();
/// assert!(site.contains(Point::new(25.2051, 55.2711).unwrap()));
/// assert!(!site.contains(Point::new(25.3, 55.3).unwrap()));
/// assert!(Circle::new(site.center(), 0.0).is_err());
/// ```
///
/// Serialized as `{"center": {"lat", "lng"}, "radius": m}`; it also reads a
/// GeoJSON `Point` with a `radius` (top level or in `properties`), which is
/// how the map picker posts a circle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    center: Point,
    radius: f64,
}

impl Circle {
    /// A circle around `center` with `radius` metres (positive and finite).
    pub fn new(center: Point, radius: f64) -> Result<Self, GeoError> {
        if !radius.is_finite() || radius <= 0.0 {
            return Err(GeoError::Radius);
        }
        Ok(Self { center, radius })
    }

    /// The centre.
    pub fn center(&self) -> Point {
        self.center
    }

    /// The radius in metres.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// Whether `point` is inside the circle; exactly on the radius counts as
    /// inside.
    pub fn contains(&self, point: Point) -> bool {
        self.center.distance_to(point) <= self.radius + RADIUS_EPSILON_M
    }
}

impl Serialize for Circle {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        json!({"center": self.center, "radius": self.radius}).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Circle {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        circle_from_value(&value).map_err(de::Error::custom)
    }
}

fn circle_from_value(value: &Value) -> Result<Circle, GeoError> {
    let radius = |v: &Value| v.get("radius").and_then(Value::as_f64);
    // A Feature wraps the geometry and keeps the radius in its properties.
    if value.get("type").and_then(Value::as_str) == Some("Feature") {
        let geometry = value
            .get("geometry")
            .ok_or_else(|| GeoError::Invalid("a Feature needs a geometry".into()))?;
        let center = point_from_value(geometry)?;
        let r = value
            .get("properties")
            .and_then(radius)
            .or_else(|| radius(geometry));
        return Circle::new(center, r.ok_or(GeoError::Radius)?);
    }
    let r = radius(value).ok_or(GeoError::Radius)?;
    let center = match value.get("center") {
        Some(c) => point_from_value(c)?,
        None => point_from_value(value)?,
    };
    Circle::new(center, r)
}

fn point_from_value(value: &Value) -> Result<Point, GeoError> {
    Point::deserialize(value).map_err(|e| {
        // Deserializing only carries text; recover the range errors from it.
        let why = e.to_string();
        if why.contains(&GeoError::Latitude.to_string()) {
            GeoError::Latitude
        } else if why.contains(&GeoError::Longitude.to_string()) {
            GeoError::Longitude
        } else {
            GeoError::Invalid(why)
        }
    })
}

impl GeoError {
    /// The validation message key for this error (see `messages.rs`).
    pub(crate) fn rule_key(&self) -> &'static str {
        match self {
            Self::Latitude => "latitude",
            Self::Longitude => "longitude",
            _ => "point",
        }
    }
}

/// Reads a point from text: JSON `{"lat", "lng"}` or a GeoJSON `Point`.
pub(crate) fn parse_point(text: &str) -> Result<Point, GeoError> {
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|_| GeoError::Invalid("not JSON".to_owned()))?;
    point_from_value(&value)
}

/// A polygon: an outer ring and optional holes.
///
/// ```
/// use renox::geo::{Point, Polygon};
///
/// let p = |lat, lng| Point::new(lat, lng).unwrap();
/// let square = vec![p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0)];
/// let hole = vec![p(4.0, 4.0), p(4.0, 6.0), p(6.0, 6.0), p(6.0, 4.0)];
/// let area = Polygon::new(square, vec![hole]).unwrap();
/// assert!(area.contains(p(1.0, 1.0)));
/// assert!(!area.contains(p(5.0, 5.0)));
/// ```
///
/// Serialized as a GeoJSON `Polygon` geometry (`[lng, lat]`, rings closed);
/// it reads that, a `Feature` wrapping one, and rings that are not closed.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    outer: Vec<Point>,
    holes: Vec<Vec<Point>>,
}

/// Where a point lies relative to a ring.
#[derive(PartialEq)]
enum Location {
    Inside,
    Boundary,
    Outside,
}

/// Drops the closing vertex if the ring repeats its first point, checks that
/// at least three distinct vertices remain, and closes it again.
fn close_ring(mut ring: Vec<Point>) -> Result<Vec<Point>, GeoError> {
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    let mut distinct: Vec<Point> = Vec::new();
    for p in &ring {
        if !distinct.contains(p) {
            distinct.push(*p);
        }
    }
    if distinct.len() < 3 {
        return Err(GeoError::Ring);
    }
    ring.push(ring[0]);
    Ok(ring)
}

/// The longitude difference `lng - reference` folded into (-180, 180].
fn shifted(lng: f64, reference: f64) -> f64 {
    let mut d = (lng - reference) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d <= -180.0 {
        d += 360.0;
    }
    d
}

fn locate(ring: &[Point], point: Point, reference: f64) -> Location {
    let (px, py) = (shifted(point.lng, reference), point.lat);
    let mut inside = false;
    for pair in ring.windows(2) {
        let (ax, ay) = (shifted(pair[0].lng, reference), pair[0].lat);
        let (bx, by) = (shifted(pair[1].lng, reference), pair[1].lat);
        // On the segment: within the bounding box and (nearly) collinear.
        let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
        let length = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        if length > 0.0
            && cross.abs() <= EDGE_EPSILON_DEG * length
            && px >= ax.min(bx) - EDGE_EPSILON_DEG
            && px <= ax.max(bx) + EDGE_EPSILON_DEG
            && py >= ay.min(by) - EDGE_EPSILON_DEG
            && py <= ay.max(by) + EDGE_EPSILON_DEG
        {
            return Location::Boundary;
        }
        if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax {
            inside = !inside;
        }
    }
    if inside {
        Location::Inside
    } else {
        Location::Outside
    }
}

impl Polygon {
    /// A polygon from an outer ring and holes. Each ring needs at least three
    /// distinct vertices; it may repeat its first point at the end or not.
    pub fn new(outer: Vec<Point>, holes: Vec<Vec<Point>>) -> Result<Self, GeoError> {
        let outer = close_ring(outer)?;
        let holes = holes
            .into_iter()
            .map(close_ring)
            .collect::<Result<_, _>>()?;
        Ok(Self { outer, holes })
    }

    /// The outer ring, closed (the first point is repeated at the end).
    pub fn outer(&self) -> &[Point] {
        &self.outer
    }

    /// The holes, each closed.
    pub fn holes(&self) -> &[Vec<Point>] {
        &self.holes
    }

    /// Whether `point` is inside the polygon (ray casting). A point on an
    /// edge or a vertex of the outer ring is inside; one strictly inside a
    /// hole is outside, and the boundary of a hole is inside.
    pub fn contains(&self, point: Point) -> bool {
        let reference = self.outer[0].lng;
        match locate(&self.outer, point, reference) {
            Location::Outside => false,
            Location::Boundary => true,
            Location::Inside => !self
                .holes
                .iter()
                .any(|hole| locate(hole, point, reference) == Location::Inside),
        }
    }

    /// The polygon as a GeoJSON `Polygon` geometry (`[lng, lat]`).
    pub fn to_geojson(&self) -> Value {
        let ring = |r: &[Point]| r.iter().map(|p| json!([p.lng, p.lat])).collect::<Vec<_>>();
        let rings: Vec<Vec<Value>> = std::iter::once(&self.outer)
            .chain(&self.holes)
            .map(|r| ring(r))
            .collect();
        json!({"type": "Polygon", "coordinates": rings})
    }
}

impl Serialize for Polygon {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_geojson().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Polygon {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        polygon_from_value(&value).map_err(de::Error::custom)
    }
}

fn polygon_from_value(value: &Value) -> Result<Polygon, GeoError> {
    let bad = |why: &str| GeoError::Invalid(why.into());
    let geometry = match value.get("type").and_then(Value::as_str) {
        Some("Feature") => value
            .get("geometry")
            .ok_or_else(|| bad("a Feature needs a geometry"))?,
        _ => value,
    };
    if geometry.get("type").and_then(Value::as_str) != Some("Polygon") {
        return Err(bad("GeoJSON type must be Polygon"));
    }
    let rings = geometry
        .get("coordinates")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("a Polygon needs coordinates"))?;
    let mut parsed = Vec::with_capacity(rings.len());
    for ring in rings {
        let positions = ring
            .as_array()
            .ok_or_else(|| bad("a ring is a list of positions"))?;
        let mut points = Vec::with_capacity(positions.len());
        for position in positions {
            let numbers: Option<Vec<f64>> = position
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_f64).collect());
            points.push(Point::from_lng_lat(&numbers.unwrap_or_default())?);
        }
        parsed.push(points);
    }
    let mut parsed = parsed.into_iter();
    let outer = parsed.next().ok_or(GeoError::Ring)?;
    Polygon::new(outer, parsed.collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(lat: f64, lng: f64) -> Point {
        Point::new(lat, lng).unwrap()
    }

    #[test]
    fn point_validates_range_and_finiteness() {
        assert!(Point::new(90.0, 180.0).is_ok());
        assert!(Point::new(-90.0, -180.0).is_ok());
        assert_eq!(Point::new(90.1, 0.0), Err(GeoError::Latitude));
        assert_eq!(Point::new(0.0, 180.1), Err(GeoError::Longitude));
        assert_eq!(Point::new(f64::NAN, 0.0), Err(GeoError::Latitude));
        assert_eq!(Point::new(0.0, f64::INFINITY), Err(GeoError::Longitude));
    }

    #[test]
    fn distance_matches_known_city_pairs() {
        let london = p(51.5074, -0.1278);
        let paris = p(48.8566, 2.3522);
        let new_york = p(40.7128, -74.0060);
        let near = |got: f64, want: f64| (got - want).abs() / want < 0.005;
        assert!(near(london.distance_to(paris), 343_500.0));
        assert!(near(london.distance_to(new_york), 5_570_000.0));
        assert_eq!(london.distance_to(paris), paris.distance_to(london));
        assert_eq!(london.distance_to(london), 0.0);
    }

    #[test]
    fn distance_across_the_antimeridian_is_the_short_way() {
        let d = p(0.0, 179.5).distance_to(p(0.0, -179.5));
        assert!((d - 111_195.0).abs() < 100.0, "{d}");
    }

    #[test]
    fn circle_edges() {
        let center = p(25.2050, 55.2710);
        let circle = Circle::new(center, 150.0).unwrap();
        assert!(circle.contains(center));
        assert!(circle.contains(p(25.2051, 55.2711)));
        assert!(!circle.contains(p(25.2070, 55.2710)));
        // Exactly on the radius counts as inside.
        let edge = Circle::new(center, center.distance_to(p(25.2060, 55.2710))).unwrap();
        assert!(edge.contains(p(25.2060, 55.2710)));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(Circle::new(center, bad), Err(GeoError::Radius));
        }
    }

    fn square() -> Vec<Point> {
        vec![p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0)]
    }

    #[test]
    fn polygon_inside_outside_edge_vertex() {
        let area = Polygon::new(square(), vec![]).unwrap();
        assert!(area.contains(p(5.0, 5.0)));
        assert!(!area.contains(p(11.0, 5.0)));
        assert!(!area.contains(p(5.0, -0.5)));
        assert!(area.contains(p(0.0, 5.0)), "on an edge");
        assert!(area.contains(p(5.0, 10.0)), "on an edge");
        assert!(area.contains(p(10.0, 10.0)), "on a vertex");
    }

    #[test]
    fn polygon_holes_are_open() {
        let hole = vec![p(4.0, 4.0), p(4.0, 6.0), p(6.0, 6.0), p(6.0, 4.0)];
        let area = Polygon::new(square(), vec![hole]).unwrap();
        assert!(area.contains(p(2.0, 2.0)));
        assert!(!area.contains(p(5.0, 5.0)), "inside the hole");
        assert!(area.contains(p(4.0, 5.0)), "on the hole's edge");
        assert!(area.contains(p(6.0, 6.0)), "on the hole's vertex");
    }

    #[test]
    fn polygon_concave() {
        // An L shape: the notch at the top right is outside.
        let l = vec![
            p(0.0, 0.0),
            p(0.0, 10.0),
            p(5.0, 10.0),
            p(5.0, 5.0),
            p(10.0, 5.0),
            p(10.0, 0.0),
        ];
        let area = Polygon::new(l, vec![]).unwrap();
        assert!(area.contains(p(2.0, 8.0)));
        assert!(area.contains(p(8.0, 2.0)));
        assert!(!area.contains(p(8.0, 8.0)));
    }

    #[test]
    fn polygon_across_the_antimeridian() {
        let ring = vec![
            p(-5.0, 175.0),
            p(-5.0, -175.0),
            p(5.0, -175.0),
            p(5.0, 175.0),
        ];
        let area = Polygon::new(ring, vec![]).unwrap();
        assert!(area.contains(p(0.0, 180.0)));
        assert!(area.contains(p(0.0, -178.0)));
        assert!(area.contains(p(0.0, 178.0)));
        assert!(!area.contains(p(0.0, 170.0)));
        assert!(!area.contains(p(0.0, -170.0)));
        assert!(!area.contains(p(0.0, 0.0)));
    }

    #[test]
    fn polygon_needs_three_distinct_vertices() {
        assert_eq!(
            Polygon::new(vec![p(0.0, 0.0), p(1.0, 1.0)], vec![]),
            Err(GeoError::Ring)
        );
        let twice = vec![p(0.0, 0.0), p(0.0, 0.0), p(1.0, 1.0)];
        assert_eq!(Polygon::new(twice, vec![]), Err(GeoError::Ring));
        let bad_hole = vec![p(4.0, 4.0), p(5.0, 5.0)];
        assert_eq!(Polygon::new(square(), vec![bad_hole]), Err(GeoError::Ring));
    }

    #[test]
    fn geojson_order_is_lng_lat_both_ways() {
        // Dubai: lat 25.2, lng 55.3. A swap would fail Point::new (lat 55.3 is
        // valid, so check the values themselves).
        let point: Point =
            serde_json::from_str(r#"{"type":"Point","coordinates":[55.3,25.2]}"#).unwrap();
        assert_eq!((point.lat(), point.lng()), (25.2, 55.3));
        assert_eq!(point.to_geojson()["coordinates"], json!([55.3, 25.2]));
        assert_eq!(
            serde_json::to_value(point).unwrap(),
            json!({"lat": 25.2, "lng": 55.3})
        );

        let area = Polygon::new(vec![p(25.0, 55.0), p(25.0, 56.0), p(26.0, 56.0)], vec![]).unwrap();
        let geojson = serde_json::to_value(&area).unwrap();
        assert_eq!(geojson["type"], "Polygon");
        assert_eq!(geojson["coordinates"][0][0], json!([55.0, 25.0]));
        assert_eq!(
            geojson["coordinates"][0][3],
            json!([55.0, 25.0]),
            "stored closed"
        );
        let back: Polygon = serde_json::from_value(geojson).unwrap();
        assert_eq!(back, area);
    }

    #[test]
    fn point_from_json_geojson_and_form_fields() {
        let json: Point = serde_json::from_str(r#"{"lat":25.2,"lng":55.3}"#).unwrap();
        let text: Point = serde_json::from_str(r#"{"lat":"25.2","lng":" 55.3 "}"#).unwrap();
        let form: Point = serde_urlencoded::from_str("lat=25.2&lng=55.3").unwrap();
        assert_eq!(json, p(25.2, 55.3));
        assert_eq!(text, json);
        assert_eq!(form, json);
    }

    #[test]
    fn invalid_values_are_errors_not_panics() {
        for bad in [
            r#"{"lat":91,"lng":0}"#,
            r#"{"lat":0,"lng":181}"#,
            r#"{"lat":"north","lng":0}"#,
            r#"{"lat":1}"#,
            r#"{"type":"Point","coordinates":[1]}"#,
            r#"{"type":"LineString","coordinates":[1,2]}"#,
        ] {
            assert!(serde_json::from_str::<Point>(bad).is_err(), "{bad}");
        }
        assert!(serde_urlencoded::from_str::<Point>("lat=x&lng=1").is_err());
        let err = serde_json::from_str::<Point>(r#"{"lat":91,"lng":0}"#).unwrap_err();
        assert!(err.to_string().contains("latitude"));
    }

    #[test]
    fn polygon_reads_features_and_open_rings() {
        let feature = r#"{"type":"Feature","properties":{},"geometry":{"type":"Polygon",
            "coordinates":[[[0,0],[10,0],[10,10],[0,10]]]}}"#;
        let area: Polygon = serde_json::from_str(feature).unwrap();
        assert_eq!(area.outer().len(), 5);
        assert!(area.contains(p(5.0, 5.0)));
        for bad in [
            r#"{"type":"Point","coordinates":[1,2]}"#,
            r#"{"type":"Polygon","coordinates":[]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,1]]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,1],[200,3]]]}"#,
        ] {
            assert!(serde_json::from_str::<Polygon>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn circle_json_forms() {
        let want = Circle::new(p(25.2, 55.3), 150.0).unwrap();
        let plain: Circle = serde_json::from_value(serde_json::to_value(want).unwrap()).unwrap();
        assert_eq!(plain, want);
        let feature = r#"{"type":"Feature","properties":{"radius":150},
            "geometry":{"type":"Point","coordinates":[55.3,25.2]}}"#;
        assert_eq!(serde_json::from_str::<Circle>(feature).unwrap(), want);
        let point = r#"{"type":"Point","coordinates":[55.3,25.2],"radius":150}"#;
        assert_eq!(serde_json::from_str::<Circle>(point).unwrap(), want);
        assert!(
            serde_json::from_str::<Circle>(r#"{"center":{"lat":1,"lng":2},"radius":0}"#).is_err()
        );
        assert!(serde_json::from_str::<Circle>(r#"{"center":{"lat":1,"lng":2}}"#).is_err());
    }
}

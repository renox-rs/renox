//! The catalogue's filters and sort orders, read from the query string.
//!
//! Every filter lives in the address (`/shop/road-bikes?brand=trek&size=54+cm&sort=price_asc`),
//! so a filtered page can be shared, bookmarked and reloaded, and works
//! without JavaScript: the filter form is a plain `GET` form. With
//! JavaScript, htmx sends the same form and swaps only the results.
//!
//! [`Filters::from_pairs`] reads the query string as a list of pairs
//! (repeated names like `brand=trek&brand=giant` are several brands), and
//! [`Filters::apply`] turns it into conditions on a `Query<Product>`: plain
//! `where_in` / `where_in_query` where the query builder has one, and
//! `where_raw` with bound values for the `EXISTS` sub-queries (the
//! variants' prices and sizes, the stock at a store, what fits a bike). The
//! specifications (wheel size, frame material, motor) are JSON, read with
//! each database's own JSON operator ([`spec_sql`]).

use renox::db::{Dialect, Query};
use renox::prelude::*;
use serde::Serialize;

use super::model::{Brand, Product, ProductVariant};

/// How a list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// Best search matches first (search results only; elsewhere it is
    /// [`Sort::Popular`]).
    Relevance,
    /// Sold most, over all time.
    #[default]
    Popular,
    /// Newest in the catalogue first.
    Newest,
    /// Cheapest variant first.
    PriceAsc,
    /// Dearest variant first.
    PriceDesc,
}

impl Sort {
    /// Every order, as the sort menu lists them.
    pub const ALL: [Sort; 5] = [
        Sort::Relevance,
        Sort::Popular,
        Sort::Newest,
        Sort::PriceAsc,
        Sort::PriceDesc,
    ];

    /// The value in the query string (`sort=price_asc`).
    pub fn key(self) -> &'static str {
        match self {
            Sort::Relevance => "relevance",
            Sort::Popular => "popular",
            Sort::Newest => "newest",
            Sort::PriceAsc => "price_asc",
            Sort::PriceDesc => "price_desc",
        }
    }

    /// The order with this key; unknown keys are `None`.
    pub fn from_key(key: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// The wheel sizes and frame materials are specifications (JSON) with
/// these names in `products.specs`.
pub const SPEC_WHEELS: &str = "Wheels";
/// The frame material's specification.
pub const SPEC_FRAME: &str = "Frame";
/// Present on e-bikes only.
pub const SPEC_MOTOR: &str = "Motor";

/// What a visitor asked for, read from the query string.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Filters {
    /// Words to search for (`q`); empty outside search.
    pub q: String,
    /// Brand slugs (`brand=trek&brand=giant`): any of them.
    pub brands: Vec<String>,
    /// The lowest variant price wanted (`price_min`), in the smallest unit.
    pub price_min: Option<i64>,
    /// The highest (`price_max`).
    pub price_max: Option<i64>,
    /// Sizes (`size=M&size=L`): a variant in any of them.
    pub sizes: Vec<String>,
    /// A wheel size (`wheels=29"`).
    pub wheels: Option<String>,
    /// A frame material (`frame=Carbon`).
    pub frame: Option<String>,
    /// E-bikes only (`ebike=1`).
    pub ebike: bool,
    /// In stock (on hand, not reserved) at this store (`store=2`).
    pub store: Option<i64>,
    /// Parts that fit the customer's bikes: `mine` (any of them) or one
    /// registered bike's id (`fits=12`).
    pub fits: Option<String>,
    /// The order (`sort`).
    pub sort: Sort,
    /// Whether `sort` was given (the search page defaults to relevance).
    pub sort_given: bool,
    /// The page (`page`), from 1.
    pub page: u32,
}

/// A filter that is on, shown as a chip with a link that turns it off.
#[derive(Debug, Clone, Serialize)]
pub struct Chip {
    /// What it says: "Brand: Trek".
    pub label: String,
    /// The same page without this filter.
    pub href: String,
}

impl Filters {
    /// Reads the query string's pairs. Values that don't parse are left out
    /// (a filter page never answers an error for a hand-edited address).
    pub fn from_pairs(pairs: &[(String, String)]) -> Filters {
        let mut f = Filters {
            page: 1,
            ..Default::default()
        };
        for (key, value) in pairs {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.as_str() {
                "q" => f.q = value.chars().take(100).collect(),
                "brand" | "brand[]" if f.brands.len() < 20 => f.brands.push(value.to_owned()),
                "size" | "size[]" if f.sizes.len() < 20 => f.sizes.push(value.to_owned()),
                "price_min" => f.price_min = value.parse().ok().filter(|v: &i64| *v >= 0),
                "price_max" => f.price_max = value.parse().ok().filter(|v: &i64| *v >= 0),
                "wheels" => f.wheels = Some(value.to_owned()),
                "frame" => f.frame = Some(value.to_owned()),
                "ebike" => f.ebike = matches!(value, "1" | "on" | "true"),
                "store" => f.store = value.parse().ok(),
                "fits" => f.fits = Some(value.to_owned()),
                "sort" => {
                    if let Some(sort) = Sort::from_key(value) {
                        f.sort = sort;
                        f.sort_given = true;
                    }
                }
                "page" => f.page = value.parse().unwrap_or(1).max(1),
                _ => {}
            }
        }
        if let (Some(low), Some(high)) = (f.price_min, f.price_max)
            && low > high
        {
            // Two handles crossed by hand in the address: swap them.
            f.price_min = Some(high);
            f.price_max = Some(low);
        }
        if !f.sort_given {
            f.sort = if f.q.is_empty() {
                Sort::Popular
            } else {
                Sort::Relevance
            };
        }
        if f.sort == Sort::Relevance && f.q.is_empty() {
            f.sort = Sort::Popular;
        }
        f
    }

    /// Whether any filter (not the words, the sort or the page) is on.
    pub fn any(&self) -> bool {
        !self.brands.is_empty()
            || self.price_min.is_some()
            || self.price_max.is_some()
            || !self.sizes.is_empty()
            || self.wheels.is_some()
            || self.frame.is_some()
            || self.ebike
            || self.store.is_some()
            || self.fits.is_some()
    }

    /// The query string for these filters (without the page), e.g. for the
    /// pagination links and the chips. `skip` leaves one value out:
    /// `("brand", "trek")`, or `("price", "")` for both prices.
    pub fn query_string(&self, skip: Option<(&str, &str)>) -> String {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        let skipped = |key: &str, value: &str| {
            skip.is_some_and(|(k, v)| k == key && (v.is_empty() || v == value))
        };
        if !self.q.is_empty() && !skipped("q", &self.q) {
            pairs.push(("q", self.q.clone()));
        }
        for brand in &self.brands {
            if !skipped("brand", brand) {
                pairs.push(("brand", brand.clone()));
            }
        }
        for size in &self.sizes {
            if !skipped("size", size) {
                pairs.push(("size", size.clone()));
            }
        }
        if !skipped("price", "") {
            if let Some(v) = self.price_min {
                pairs.push(("price_min", v.to_string()));
            }
            if let Some(v) = self.price_max {
                pairs.push(("price_max", v.to_string()));
            }
        }
        for (key, value) in [
            ("wheels", &self.wheels),
            ("frame", &self.frame),
            ("fits", &self.fits),
        ] {
            if let Some(v) = value
                && !skipped(key, v)
            {
                pairs.push((key, v.clone()));
            }
        }
        if self.ebike && !skipped("ebike", "1") {
            pairs.push(("ebike", "1".into()));
        }
        if let Some(store) = self.store
            && !skipped("store", &store.to_string())
        {
            pairs.push(("store", store.to_string()));
        }
        if self.sort_given {
            pairs.push(("sort", self.sort.key().into()));
        }
        pairs
            .into_iter()
            .map(|(k, v)| format!("{k}={}", encode(&v)))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// `path` with these filters, minus `skip` (see [`Filters::query_string`]).
    pub fn url(&self, path: &str, skip: Option<(&str, &str)>) -> String {
        let query = self.query_string(skip);
        if query.is_empty() {
            path.to_owned()
        } else {
            format!("{path}?{query}")
        }
    }

    /// Adds the filters' conditions and the order to `query`. `dialect`
    /// picks the JSON operator for the specifications; `fits_bikes` are
    /// the bike models (product ids) the `fits` filter stands for (empty:
    /// nothing fits). `query` already holds the scope and the search
    /// (`where_search(q)`); this adds the filters and ranks by it.
    pub fn apply(
        &self,
        mut query: Query<Product>,
        dialect: Dialect,
        fits_bikes: &[i64],
    ) -> Query<Product> {
        if !self.brands.is_empty() {
            query = query.where_in_query(
                "brand_id",
                Brand::query().where_(Brand::SLUG.is_in(self.brands.clone())),
                "id",
            );
        }
        // Both ends in one EXISTS: one variant must be in the range (a bike
        // with variants at $1,000 and $10,000 is not "between $4,000 and
        // $5,000").
        if self.price_min.is_some() || self.price_max.is_some() {
            query = query.where_raw(
                "EXISTS (SELECT 1 FROM product_variants pv WHERE pv.product_id = products.id \
                 AND pv.price >= ? AND pv.price <= ?)",
                [
                    self.price_min.unwrap_or(0),
                    self.price_max.unwrap_or(i64::MAX),
                ],
            );
        }
        if !self.sizes.is_empty() {
            query = query.where_in_query(
                "id",
                ProductVariant::query().where_(ProductVariant::SIZE.is_in(self.sizes.clone())),
                "product_id",
            );
        }
        if let Some(wheels) = &self.wheels {
            query = query.where_raw(
                &format!("{} = ?", spec_sql(dialect, SPEC_WHEELS)),
                [wheels.clone()],
            );
        }
        if let Some(frame) = &self.frame {
            query = query.where_raw(
                &format!("{} = ?", spec_sql(dialect, SPEC_FRAME)),
                [frame.clone()],
            );
        }
        if self.ebike {
            query = query.where_raw(
                &format!("{} IS NOT NULL", spec_sql(dialect, SPEC_MOTOR)),
                std::iter::empty::<i64>(),
            );
        }
        if let Some(store) = self.store {
            query = query.where_raw(
                "EXISTS (SELECT 1 FROM product_variants pv JOIN stock_levels sl \
                 ON sl.variant_id = pv.id WHERE pv.product_id = products.id \
                 AND sl.location_store_id = ? AND sl.on_hand - sl.reserved > 0)",
                [store],
            );
        }
        if self.fits.is_some() {
            if fits_bikes.is_empty() {
                query = query.where_raw("1 = 0", std::iter::empty::<i64>());
            } else {
                let marks = vec!["?"; fits_bikes.len()].join(", ");
                query = query.where_raw(
                    &format!(
                        "EXISTS (SELECT 1 FROM part_fits pf WHERE pf.part_id = products.id \
                         AND pf.bike_id IN ({marks}))"
                    ),
                    fits_bikes.iter().copied(),
                );
            }
        }
        self.order(query)
    }

    /// The order, after the filters. Ties are broken by id so pages never
    /// overlap. The search itself (`where_search(q)`) is already on the
    /// query the caller passes, where the facets read it too: here it only
    /// ranks.
    fn order(&self, query: Query<Product>) -> Query<Product> {
        let query = match self.sort {
            Sort::Relevance => query.order_by_relevance(&self.q),
            Sort::Popular => query.order_by_raw(
                "(SELECT COALESCE(SUM(oi.quantity), 0) FROM order_items oi \
                 JOIN product_variants pv ON pv.id = oi.variant_id \
                 WHERE pv.product_id = products.id) DESC",
            ),
            Sort::Newest => query.order_by_desc(Product::CREATED_AT),
            Sort::PriceAsc => query.order_by_raw(
                "(SELECT MIN(pv.price) FROM product_variants pv WHERE pv.product_id = products.id) ASC",
            ),
            Sort::PriceDesc => query.order_by_raw(
                "(SELECT MIN(pv.price) FROM product_variants pv WHERE pv.product_id = products.id) DESC",
            ),
        };
        query.order_by(Product::ID)
    }
}

/// The SQL reading the specification `key` of `products.specs` as text:
/// `json_extract` on SQLite, `->>` on PostgreSQL (a JSONB column). `key`
/// is one of this module's constants, never user input.
pub fn spec_sql(dialect: Dialect, key: &str) -> String {
    debug_assert!(key.chars().all(|c| c.is_ascii_alphanumeric() || c == ' '));
    if dialect == Dialect::Postgres {
        format!("(products.specs ->> '{key}')")
    } else {
        format!("json_extract(products.specs, '$.\"{key}\"')")
    }
}

/// Percent-encodes a query-string value (letters, digits and `-._~` stay).
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn reads_repeated_values_and_ignores_rubbish() {
        let f = Filters::from_pairs(&pairs(&[
            ("brand", "trek"),
            ("brand", "giant"),
            ("price_min", "abc"),
            ("price_max", "500000"),
            ("sort", "nope"),
            ("page", "0"),
        ]));
        assert_eq!(f.brands, ["trek", "giant"]);
        assert_eq!(f.price_min, None);
        assert_eq!(f.price_max, Some(500_000));
        assert_eq!(f.sort, Sort::Popular);
        assert_eq!(f.page, 1);
    }

    #[test]
    fn search_defaults_to_relevance_and_crossed_prices_swap() {
        let f = Filters::from_pairs(&pairs(&[
            ("q", "helmet"),
            ("price_min", "9"),
            ("price_max", "3"),
        ]));
        assert_eq!(f.sort, Sort::Relevance);
        assert_eq!((f.price_min, f.price_max), (Some(3), Some(9)));
    }

    #[test]
    fn query_strings_drop_one_value() {
        let f = Filters::from_pairs(&pairs(&[
            ("brand", "trek"),
            ("brand", "giant"),
            ("wheels", "29\""),
        ]));
        assert_eq!(f.query_string(None), "brand=trek&brand=giant&wheels=29%22");
        assert_eq!(
            f.url("/shop", Some(("brand", "trek"))),
            "/shop?brand=giant&wheels=29%22"
        );
    }
}

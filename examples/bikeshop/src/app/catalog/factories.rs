//! Factories for the catalogue: `products().discontinued()`,
//! `variants_of(&product).sized(&["S", "M", "L"])`…

use renox::db::{FactoryBuilder, Json};
use renox::fake::Fake;
use renox::fake::faker::lorem::en::{Paragraph, Word};
use renox::prelude::*;
use std::collections::BTreeMap;

use super::model::{Brand, Category, CategoryKind, Product, ProductPhoto, ProductVariant};
use crate::seed::unique;

impl Factory for Category {
    fn definition() -> Self {
        let n = unique();
        Category {
            name: format!("Category {n}"),
            slug: format!("category-{n}"),
            kind: CategoryKind::Bike,
            ..Default::default()
        }
    }
}

/// States of a category.
pub trait CategoryStates {
    /// Of this kind.
    fn kind(self, kind: CategoryKind) -> Self;
}

impl CategoryStates for FactoryBuilder<Category> {
    fn kind(self, kind: CategoryKind) -> Self {
        self.state(move |c| c.kind = kind)
    }
}

impl Factory for Brand {
    fn definition() -> Self {
        let n = unique();
        Brand {
            name: format!("Brand {n}"),
            slug: format!("brand-{n}"),
            ..Default::default()
        }
    }
}

impl Factory for Product {
    fn definition() -> Self {
        let n = unique();
        let word: String = Word().fake();
        Product {
            name: format!("{} {n}", capitalize(&word)),
            slug: format!("{word}-{n}"),
            description: Paragraph(2..4).fake(),
            specs: Json(BTreeMap::from([(
                "Frame".to_owned(),
                "Aluminium".to_owned(),
            )])),
            ..Default::default()
        }
    }
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// `Product::factory()`.
pub fn products() -> FactoryBuilder<Product> {
    Product::factory()
}

/// States of a product.
pub trait ProductStates {
    /// In this category, of this brand.
    fn of(self, category_id: i64, brand_id: i64) -> Self;
    /// No longer sold (soft deleted).
    fn discontinued(self) -> Self;
}

impl ProductStates for FactoryBuilder<Product> {
    fn of(self, category_id: i64, brand_id: i64) -> Self {
        self.state(move |p| {
            p.category_id = category_id;
            p.brand_id = brand_id;
        })
    }

    fn discontinued(self) -> Self {
        self.state(|p| p.deleted_at = Some(renox::db::now()))
    }
}

impl Factory for ProductVariant {
    fn definition() -> Self {
        let price = (50..5_000).fake::<i64>() * 100;
        ProductVariant {
            sku: format!("SKU-{:06}", unique()),
            price,
            cost: price * 6 / 10,
            reorder_level: (2..6).fake(),
            ..Default::default()
        }
    }
}

/// Variants of `product_id`.
pub fn variants_of(product_id: i64) -> FactoryBuilder<ProductVariant> {
    ProductVariant::factory().state(move |v| v.product_id = product_id)
}

/// States of a variant.
pub trait VariantStates {
    /// One variant per size, in order (a sequence: use `count(sizes.len())`).
    fn sized(self, sizes: &'static [&'static str]) -> Self;
    /// At this price (cost 60 % of it).
    fn priced(self, price: i64) -> Self;
}

impl VariantStates for FactoryBuilder<ProductVariant> {
    fn sized(self, sizes: &'static [&'static str]) -> Self {
        self.sequence(move |i, v| v.size = Some(sizes[i % sizes.len()].to_owned()))
    }

    fn priced(self, price: i64) -> Self {
        self.state(move |v| {
            v.price = price;
            v.cost = price * 6 / 10;
        })
    }
}

impl Factory for ProductPhoto {
    fn definition() -> Self {
        ProductPhoto {
            path: "images/products/placeholder.svg".into(),
            alt: "A photo of the product".into(),
            ..Default::default()
        }
    }
}

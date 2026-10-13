//! Builds the demo shop: places, stores, people and their roles, the
//! catalogue and the fleet here; 18 months of rentals, orders, stock and
//! workshop history in `super::history` (private). Big tables are written with
//! `Model::insert_many` (a few statements per thousand rows) inside
//! transactions, so the large seed takes seconds, not minutes.

use renox::auth::User;
use renox::chrono::Duration;
use renox::db::{Db, Json, Transaction, sql};
use renox::prelude::*;
use std::collections::{BTreeMap, HashMap};

use super::content::{self, CategorySpec};
use super::history;
use super::rng::Rng;
use super::{DEMO_PASSWORD, today};
use crate::app::access::catalogue::{self as perms, CASHIER, MANAGER, MECHANIC, OWNER, STAFF};
use crate::app::access::policy::store_scope;
use crate::app::accounts::factories::CustomerStates;
use crate::app::accounts::model::{Address, City, Country, Customer};
use crate::app::catalog::factories::{ProductStates, VariantStates, products, variants_of};
use crate::app::catalog::model::{
    Brand, Category, CategoryKind, PART_FITS, ProductPhoto, ProductVariant, keywords,
};
use crate::app::plans::model::{Frequency, PLAN_TASKS, ServicePlan};
use crate::app::rentals::factories::{BikeStates, rental_bikes};
use crate::app::rentals::model::{BikePlacement, PlacementStatus, RentalBike};
use crate::app::staff::factories::{HelpStates, midnight, stores_at, this_week, usual_hours};
use crate::app::staff::model::{HelpStatus, Staff, StaffHelpHour, StaffHelpRequest, Store};
use crate::app::stock::model::Supplier;
use crate::app::workshop::model::ServiceTask;

/// How much the seed makes.
#[derive(Debug, Clone, Copy)]
pub struct Volume {
    /// Products in the catalogue (each with one to five variants).
    pub products: usize,
    /// Customers (some with a login).
    pub customers: usize,
    /// Rentals over the history, roughly.
    pub rentals: usize,
    /// Orders over the history.
    pub orders: usize,
    /// Work orders over the history and the next two weeks.
    pub work_orders: usize,
    /// How far back the history goes, in days.
    pub history_days: i64,
    /// Rental bikes owned by each store.
    pub bikes_per_store: usize,
}

impl Volume {
    /// `db:seed`: enough to click through every page, in a second or two.
    pub fn small() -> Self {
        Volume {
            products: 150,
            customers: 50,
            rentals: 400,
            orders: 200,
            work_orders: 120,
            history_days: 90,
            bikes_per_store: 12,
        }
    }

    /// `demo:seed --size large`: Pagila's volume over 18 months (about
    /// 1,000 variants, 600 customers, 16,000 rentals, 5,000 orders, 3,000
    /// work orders).
    pub fn large() -> Self {
        Volume {
            products: 370,
            customers: 600,
            rentals: 16_000,
            orders: 5_000,
            work_orders: 3_000,
            history_days: 548,
            bikes_per_store: 60,
        }
    }
}

/// A demo user printed at the end.
#[derive(Debug, Clone)]
pub struct DemoUser {
    /// Their login.
    pub email: String,
    /// Who they are: "Manager of North".
    pub what: String,
}

/// What the seed made.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    /// Rows per table, for the printout.
    pub counts: Vec<(String, i64)>,
    /// The demo users.
    pub users: Vec<DemoUser>,
}

/// Whether the shop is there already (it has stores).
pub async fn seeded(db: &Db) -> Result<bool> {
    Ok(Store::query().count(db).await? > 0)
}

/// A staff member as the seed keeps them.
#[derive(Debug, Clone)]
pub struct Person {
    pub user_id: i64,
    pub staff_id: i64,
    pub store_id: i64,
    pub role: &'static str,
}

/// A variant as the seed keeps it.
#[derive(Debug, Clone)]
pub struct Item {
    pub variant_id: i64,
    pub product_id: i64,
    pub kind: CategoryKind,
    pub category: &'static str,
    pub price: i64,
    pub reorder_level: i64,
}

/// A customer as the seed keeps them.
#[derive(Debug, Clone)]
pub struct Client {
    pub id: i64,
    pub user_id: Option<i64>,
    pub address_id: Option<i64>,
    /// Their ID is on file and checked: they may rent.
    pub may_rent: bool,
}

/// Everything later steps need to point at.
pub struct World {
    pub rng: Rng,
    pub volume: Volume,
    pub stores: Vec<Store>,
    pub staff: Vec<Person>,
    pub items: Vec<Item>,
    pub bike_products: Vec<(i64, String)>,
    pub clients: Vec<Client>,
    pub tasks: Vec<ServiceTask>,
    pub plans: Vec<(ServicePlan, Vec<i64>)>,
    pub bikes: Vec<RentalBike>,
    /// Bike id → (the store it moved to, when): placed bikes.
    pub placements: HashMap<i64, (i64, DateTime)>,
    pub suppliers: Vec<Supplier>,
    pub users: Vec<DemoUser>,
    /// The fleet bike kept for the demo customer's rentals (no history).
    pub demo_bike: Option<i64>,
}

impl World {
    /// The staff of `store_id` with `role`.
    pub fn staff_of(&self, store_id: i64, role: &str) -> Vec<&Person> {
        self.staff
            .iter()
            .filter(|p| p.store_id == store_id && p.role == role)
            .collect()
    }

    /// Someone at the counter of `store_id` (a cashier, staff or the manager).
    pub fn counter_person(&mut self, store_id: i64) -> Option<i64> {
        let people: Vec<i64> = self
            .staff
            .iter()
            .filter(|p| p.store_id == store_id && [CASHIER, STAFF, MANAGER].contains(&p.role))
            .map(|p| p.staff_id)
            .collect();
        (!people.is_empty()).then(|| *self.rng.pick(&people))
    }

    /// A mechanic of `store_id`.
    pub fn mechanic(&mut self, store_id: i64) -> Option<i64> {
        let people: Vec<i64> = self
            .staff_of(store_id, MECHANIC)
            .iter()
            .map(|p| p.staff_id)
            .collect();
        (!people.is_empty()).then(|| *self.rng.pick(&people))
    }

    /// The store with this id.
    pub fn store(&self, id: i64) -> &Store {
        self.stores
            .iter()
            .find(|s| s.id == id)
            .expect("a seeded store")
    }

    /// A random store id.
    pub fn any_store(&mut self) -> i64 {
        let ids: Vec<i64> = self.stores.iter().map(|s| s.id).collect();
        *self.rng.pick(&ids)
    }

    /// A random store id other than `but`.
    pub fn other_store(&mut self, but: i64) -> i64 {
        let ids: Vec<i64> = self
            .stores
            .iter()
            .map(|s| s.id)
            .filter(|id| *id != but)
            .collect();
        *self.rng.pick(&ids)
    }
}

/// Makes the whole shop and returns what it made.
pub async fn build(db: &Db, volume: Volume) -> Result<Summary> {
    perms::define_roles(db).await?;
    let mut world = World {
        rng: Rng::new(231),
        volume,
        stores: Vec::new(),
        staff: Vec::new(),
        items: Vec::new(),
        bike_products: Vec::new(),
        clients: Vec::new(),
        tasks: Vec::new(),
        plans: Vec::new(),
        bikes: Vec::new(),
        placements: HashMap::new(),
        suppliers: Vec::new(),
        users: Vec::new(),
        demo_bike: None,
    };

    let mut tx = db.begin().await?;
    let cities = places(&mut tx).await?;
    stores(&mut tx, &mut world, &cities).await?;
    catalogue(&mut tx, &mut world).await?;
    services(&mut tx, &mut world).await?;
    customers(&mut tx, &mut world, &cities).await?;
    tx.commit().await?;

    // Roles are given through the Permissions module's API (its own transactions).
    staff(db, &mut world).await?;

    let mut tx = db.begin().await?;
    fleet(&mut tx, &mut world).await?;
    tx.commit().await?;

    history::build(db, &mut world).await?;
    demo_customer(db, &mut world).await?;

    let mut summary = Summary {
        users: world.users.clone(),
        ..Default::default()
    };
    for table in [
        "stores",
        "staff",
        "customers",
        "products",
        "product_variants",
        "rental_bikes",
        "rentals",
        "orders",
        "order_items",
        "payments",
        "work_orders",
        "plan_subscriptions",
        "stock_levels",
        "stock_movements",
        "consignment_shipments",
        "intercompany_entries",
        "settlements",
    ] {
        let count: i64 = sql(format!("SELECT COUNT(*) FROM {table}"))
            .scalar(db)
            .await?;
        summary.counts.push((table.to_owned(), count));
    }
    Ok(summary)
}

/// Countries and cities; returns the city ids by name.
async fn places(tx: &mut Transaction) -> Result<HashMap<&'static str, i64>> {
    let mut cities = HashMap::new();
    for (name, code, names) in content::PLACES {
        let country = Country::create(
            &mut *tx,
            Country {
                name: (*name).into(),
                code: (*code).into(),
                ..Default::default()
            },
        )
        .await?;
        for city in *names {
            let row = City::create(
                &mut *tx,
                City {
                    country_id: country.id,
                    name: (*city).into(),
                    ..Default::default()
                },
            )
            .await?;
            cities.insert(*city, row.id);
        }
    }
    Ok(cities)
}

/// The three stores, in Jakarta.
async fn stores(
    tx: &mut Transaction,
    world: &mut World,
    cities: &HashMap<&'static str, i64>,
) -> Result {
    let jakarta = cities["Jakarta"];
    for (i, (name, slug, street, district, phone)) in content::STORES.iter().enumerate() {
        let address = Address::create(
            &mut *tx,
            Address {
                city_id: jakarta,
                line1: (*street).into(),
                district: Some((*district).into()),
                postal_code: Some(format!("1{}1{}0", i + 1, i + 3)),
                ..Default::default()
            },
        )
        .await?;
        let mut store = stores_at(address.id).make_one();
        store.name = (*name).into();
        store.slug = (*slug).into();
        store.phone = (*phone).into();
        store.email = format!("{slug}@bikeshop.test");
        store.opening_hours = Json(usual_hours());
        // South's workshop is the big one.
        store.workshop_minutes_per_day = if *slug == "south" { 1_440 } else { 960 };
        // West negotiated a higher fee for the work it does for the others.
        store.fee_rate_bp = if *slug == "west" { 2_500 } else { 2_000 };
        store.insert(&mut *tx).await?;
        world.stores.push(store);
    }
    Ok(())
}

/// Categories, brands, products with their variants and photos, and what
/// fits what.
async fn catalogue(tx: &mut Transaction, world: &mut World) -> Result {
    // Three top-level categories, then the specific ones under them.
    let mut parents = HashMap::new();
    for (position, (name, slug, kind)) in [
        ("Bikes", "bikes", CategoryKind::Bike),
        ("Gear", "gear", CategoryKind::Gear),
        ("Parts", "parts", CategoryKind::Part),
    ]
    .into_iter()
    .enumerate()
    {
        let row = Category::create(
            &mut *tx,
            Category {
                name: name.into(),
                slug: slug.into(),
                kind,
                position: position as i64,
                ..Default::default()
            },
        )
        .await?;
        parents.insert(kind.as_str(), row.id);
    }
    let mut categories: Vec<(&'static CategorySpec, i64)> = Vec::new();
    for (position, spec) in content::CATEGORIES.iter().enumerate() {
        let row = Category::create(
            &mut *tx,
            Category {
                parent_id: Some(parents[spec.kind.as_str()]),
                name: spec.name.into(),
                slug: spec.slug.into(),
                kind: spec.kind,
                position: position as i64,
                ..Default::default()
            },
        )
        .await?;
        categories.push((spec, row.id));
    }
    let mut brands: HashMap<&str, i64> = HashMap::new();
    for (name, website) in content::BRANDS {
        let row = Brand::create(
            &mut *tx,
            Brand {
                name: (*name).into(),
                slug: name.to_lowercase(),
                website: Some((*website).into()),
                ..Default::default()
            },
        )
        .await?;
        brands.insert(*name, row.id);
    }

    // Products, spread over the categories (bikes weigh most).
    let trims = [
        "", "2", "3", "4", "5", "SL", "Pro", "Comp", "Sport", "Elite",
    ];
    let weights: Vec<u64> = categories
        .iter()
        .map(|(spec, _)| match spec.kind {
            CategoryKind::Bike => 9,
            CategoryKind::Gear => 6,
            CategoryKind::Part => 5,
        })
        .collect();
    let mut taken = std::collections::HashSet::new();
    let mut photos = Vec::new();
    let mut fits_parts = Vec::new();
    let mut made = 0;
    let rng = &mut world.rng;
    while made < world.volume.products {
        let (spec, category_id) = categories[rng.weighted(&weights)];
        let brand = *rng.pick(spec.brands);
        let model = *rng.pick(spec.models);
        let trim = *rng.pick(&trims);
        let name = format!("{brand} {model} {trim}").trim().to_owned();
        if !taken.insert(name.clone()) {
            continue;
        }
        made += 1;
        let slug = slugify(&name);
        let base_price = rng.retail(spec.price.0, spec.price.1);

        // The variants first: their SKUs go into the product's keywords.
        let sizes: Vec<&str> = match spec.kind {
            CategoryKind::Bike => spec.sizes.iter().take(4).copied().collect(),
            _ => spec.sizes.to_vec(),
        };
        let colours: Vec<&str> = spec.colours.to_vec();
        let mut variants: Vec<(Option<String>, Option<String>)> = Vec::new();
        if sizes.is_empty() {
            if colours.is_empty() {
                variants.push((None, None));
            } else {
                for colour in &colours {
                    variants.push((None, Some((*colour).to_owned())));
                }
            }
        } else {
            for (i, size) in sizes.iter().enumerate() {
                let colour = (!colours.is_empty()).then(|| colours[i % colours.len()].to_owned());
                variants.push((Some((*size).to_owned()), colour));
            }
        }
        let code = sku_code(brand, model, made);
        let skus: Vec<String> = (0..variants.len())
            .map(|i| format!("{code}-{}", i + 1))
            .collect();
        let specs: BTreeMap<String, String> = spec
            .specs
            .iter()
            .map(|(key, values)| ((*key).to_owned(), (*rng.pick(values)).to_owned()))
            .collect();
        let description = describe(spec, brand, model, &specs);
        let discontinued = rng.chance(3);

        let mut factory = products().of(category_id, brands[brand]);
        if discontinued {
            factory = factory.discontinued();
        }
        let mut product = factory.make_one();
        product.name = name.clone();
        product.slug = slug;
        product.description = description;
        product.specs = Json(specs);
        product.keywords = keywords(brand, &skus);
        product.created_at = Some(renox::db::now() - Duration::days(world_days(rng, 700)));
        product.insert(&mut *tx).await?;

        let sized: Vec<ProductVariant> = variants_of(product.id)
            .count(variants.len())
            .priced(base_price)
            .make()
            .into_iter()
            .zip(variants.into_iter().zip(skus))
            .map(|(mut v, ((size, colour), sku))| {
                v.size = size;
                v.colour = colour;
                v.sku = sku;
                // Bigger frames cost a little more.
                v.price = base_price;
                v.cost = base_price * 62 / 100;
                v.reorder_level = match spec.kind {
                    CategoryKind::Bike => 1,
                    CategoryKind::Gear => 3,
                    CategoryKind::Part => 5,
                };
                v
            })
            .collect();
        for mut variant in sized {
            variant.insert(&mut *tx).await?;
            if !discontinued {
                world.items.push(Item {
                    variant_id: variant.id,
                    product_id: product.id,
                    kind: spec.kind,
                    category: spec.slug,
                    price: variant.price,
                    reorder_level: variant.reorder_level,
                });
            }
        }
        photos.push(ProductPhoto {
            product_id: product.id,
            path: content::product_photo(spec.slug, product.id),
            alt: product.name.clone(),
            ..Default::default()
        });
        match spec.kind {
            CategoryKind::Bike if !discontinued => {
                world.bike_products.push((product.id, product.name.clone()))
            }
            CategoryKind::Part => fits_parts.push(product.id),
            _ => {}
        }
    }
    ProductPhoto::insert_many(&mut *tx, photos).await?;

    // What fits what: each part fits a handful of bike models.
    let bike_ids: Vec<i64> = world.bike_products.iter().map(|(id, _)| *id).collect();
    if !bike_ids.is_empty() {
        for part in fits_parts {
            let n = world.rng.range(2, 7);
            for _ in 0..n {
                let bike = *world.rng.pick(&bike_ids);
                let note = world
                    .rng
                    .chance(30)
                    .then_some("Check the axle standard before fitting.");
                PART_FITS
                    .attach_with(&mut *tx, part, bike, &[("note", &note)])
                    .await?;
            }
        }
    }
    Ok(())
}

fn world_days(rng: &mut Rng, max: i64) -> i64 {
    rng.range(1, max)
}

/// `Trek Domane SL` → `trek-domane-sl`.
pub fn slugify(text: &str) -> String {
    let mut slug = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

/// `TRK-DOM-0042`: three letters of the brand and of the model, a number.
fn sku_code(brand: &str, model: &str, n: usize) -> String {
    let three = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .take(3)
            .collect::<String>()
            .to_uppercase()
    };
    format!("{}-{}-{n:04}", three(brand), three(model))
}

/// A product description in Markdown.
fn describe(
    spec: &CategorySpec,
    brand: &str,
    model: &str,
    specs: &BTreeMap<String, String>,
) -> String {
    let what = match spec.kind {
        CategoryKind::Bike => format!(
            "The {brand} {model} is a {} built for riders who want one bike that does it all \
             well: quick on the road, calm on rough streets and easy to keep running.",
            spec.name.trim_end_matches('s').to_lowercase()
        ),
        CategoryKind::Gear => format!(
            "{brand} {model}: {} chosen by our staff for daily riding, tested in our stores.",
            spec.name.to_lowercase()
        ),
        CategoryKind::Part => format!(
            "A {brand} {model} spare part. Check *What it fits* before you buy, or ask the \
             workshop to fit it for you."
        ),
    };
    let lines: Vec<String> = specs
        .iter()
        .map(|(k, v)| format!("- **{k}:** {v}"))
        .collect();
    format!("{what}\n\n{}", lines.join("\n"))
}

/// Service tasks, plans and suppliers.
async fn services(tx: &mut Transaction, world: &mut World) -> Result {
    for (name, slug, minutes, price) in content::SERVICE_TASKS {
        let task = ServiceTask::create(
            &mut *tx,
            ServiceTask {
                name: (*name).into(),
                slug: (*slug).into(),
                minutes: *minutes,
                price: *price,
                ..Default::default()
            },
        )
        .await?;
        world.tasks.push(task);
    }
    for (name, slug, frequency, price, description, task_slugs) in content::SERVICE_PLANS {
        let plan = ServicePlan::create(
            &mut *tx,
            ServicePlan {
                name: (*name).into(),
                slug: (*slug).into(),
                frequency: frequency.parse::<Frequency>().unwrap_or_default(),
                price: *price,
                description: (*description).into(),
                active: true,
                parts_discount_bp: crate::app::plans::billing::default_parts_discount_bp(
                    frequency.parse::<Frequency>().unwrap_or_default(),
                ),
                ..Default::default()
            },
        )
        .await?;
        let ids: Vec<i64> = world
            .tasks
            .iter()
            .filter(|t| task_slugs.contains(&t.slug.as_str()))
            .map(|t| t.id)
            .collect();
        PLAN_TASKS.attach(&mut *tx, plan.id, ids.clone()).await?;
        world.plans.push((plan, ids));
    }
    for (name, email, lead_days) in content::SUPPLIERS {
        let supplier = Supplier::create(
            &mut *tx,
            Supplier {
                name: (*name).into(),
                email: Some((*email).into()),
                lead_days: *lead_days,
                ..Default::default()
            },
        )
        .await?;
        world.suppliers.push(supplier);
    }
    Ok(())
}

/// Inserts users who all have the demo password (hashed once), verified;
/// returns their ids by email.
pub async fn insert_users(
    tx: &mut Transaction,
    people: &[(String, String)],
) -> Result<HashMap<String, i64>> {
    let hash = renox::auth::hash_password(DEMO_PASSWORD).await?;
    let at = renox::db::now();
    for chunk in people.chunks(200) {
        let marks = vec!["(?, ?, ?, ?, ?, ?)"; chunk.len()].join(", ");
        let mut statement = sql(format!(
            "INSERT INTO users (name, email, password, email_verified_at, created_at, updated_at) \
             VALUES {marks}"
        ));
        for (name, email) in chunk {
            statement = statement
                .bind(name.clone())
                .bind(email.clone())
                .bind(hash.clone())
                .bind(at)
                .bind(at)
                .bind(at);
        }
        statement.execute(&mut *tx).await?;
    }
    let rows: Vec<(i64, String)> = sql("SELECT id, email FROM users")
        .fetch_as(&mut *tx)
        .await?;
    Ok(rows.into_iter().map(|(id, email)| (email, id)).collect())
}

/// Customers: addresses, some with a login, most with an ID on file.
async fn customers(
    tx: &mut Transaction,
    world: &mut World,
    cities: &HashMap<&'static str, i64>,
) -> Result {
    let city_ids: Vec<(&str, i64)> = cities.iter().map(|(n, id)| (*n, *id)).collect();
    let jakarta = cities["Jakarta"];
    let n = world.volume.customers;
    let rng = &mut world.rng;

    // Who they are.
    let mut people = Vec::with_capacity(n);
    for i in 0..n {
        let name = format!(
            "{} {}",
            rng.pick(content::FIRST_NAMES),
            rng.pick(content::LAST_NAMES)
        );
        let email = format!("customer{}@example.com", i + 1);
        // Most live in Jakarta; tourists come from everywhere.
        let city = if rng.chance(65) {
            jakarta
        } else {
            rng.pick(&city_ids).1
        };
        people.push((name, email, city));
    }

    // Their addresses.
    let addresses: Vec<Address> = people
        .iter()
        .map(|(_, _, city)| Address {
            city_id: *city,
            line1: format!("{} {}", rng.pick(content::STREETS), rng.range(1, 250)),
            postal_code: Some(format!("{:05}", rng.range(10_000, 99_999))),
            ..Default::default()
        })
        .collect();
    Address::insert_many(&mut *tx, addresses).await?;
    let address_ids: Vec<i64> = sql("SELECT id FROM addresses ORDER BY id")
        .scalars(&mut *tx)
        .await?;
    let address_ids = &address_ids[address_ids.len() - n..];

    // Logins for six in ten.
    let with_login: Vec<bool> = (0..n).map(|_| rng.chance(60)).collect();
    let logins: Vec<(String, String)> = people
        .iter()
        .zip(&with_login)
        .filter(|(_, login)| **login)
        .map(|((name, email, _), _)| (name.clone(), email.clone()))
        .collect();
    let user_ids = insert_users(tx, &logins).await?;

    let rng = &mut world.rng;
    let mut rows = Vec::with_capacity(n);
    let mut may_rent = Vec::with_capacity(n);
    for (i, (name, email, _)) in people.iter().enumerate() {
        let id_state = rng.weighted(&[70, 10, 20]); // checked, waiting, none
        let mut factory = Customer::factory().living_at(address_ids[i]);
        factory = match id_state {
            0 => factory.verified(),
            1 => factory.unverified_id(),
            _ => factory,
        };
        let mut customer = factory.make_one();
        customer.name = name.clone();
        customer.email = Some(email.clone());
        customer.user_id = with_login[i].then(|| user_ids[email]);
        customer.created_at =
            Some(renox::db::now() - Duration::days(rng.range(1, world.volume.history_days + 30)));
        if rng.chance(2) {
            customer.active = false;
            customer.deleted_at = Some(renox::db::now() - Duration::days(rng.range(1, 60)));
        }
        may_rent.push(id_state == 0 && customer.deleted_at.is_none());
        rows.push(customer);
    }
    let users: Vec<Option<i64>> = rows.iter().map(|c| c.user_id).collect();
    Customer::insert_many(&mut *tx, rows).await?;
    let ids: Vec<i64> = sql("SELECT id FROM customers ORDER BY id")
        .scalars(&mut *tx)
        .await?;
    for (i, id) in ids.into_iter().enumerate() {
        world.clients.push(Client {
            id,
            user_id: users[i],
            address_id: Some(address_ids[i]),
            may_rent: may_rent[i],
        });
    }
    Ok(())
}

/// The staff: users, `staff` rows and their roles per store, a person
/// with roles in two stores, and someone helping another store this week.
async fn staff(db: &Db, world: &mut World) -> Result {
    let stores = world.stores.clone();
    let mut people: Vec<(String, String, &'static str, Option<i64>)> = vec![(
        "Olivia Hartono".into(),
        "owner@bikeshop.test".into(),
        OWNER,
        None,
    )];
    let names = [
        ("Budi", "Santoso"),
        ("Citra", "Lestari"),
        ("Dewi", "Kusuma"),
        ("Eko", "Pratama"),
        ("Fajar", "Nugroho"),
        ("Gita", "Halim"),
        ("Hana", "Wijaya"),
        ("Indra", "Gunawan"),
        ("Joko", "Saputra"),
        ("Kartika", "Tan"),
        ("Lina", "Lim"),
        ("Made", "Wirawan"),
        ("Nadia", "Chen"),
        ("Oscar", "Ng"),
        ("Putri", "Halim"),
    ];
    let mut n = 0;
    for store in &stores {
        // Logins are named after the role: manager.north@…, mechanic2.north@….
        for (role, second) in [
            (MANAGER, false),
            (CASHIER, false),
            (MECHANIC, false),
            (MECHANIC, true),
            (STAFF, false),
        ] {
            let (first, last) = names[n % names.len()];
            n += 1;
            let prefix = if second {
                format!("{role}2")
            } else {
                role.to_owned()
            };
            people.push((
                format!("{first} {last}"),
                format!("{prefix}.{}@bikeshop.test", store.slug),
                role,
                Some(store.id),
            ));
        }
    }
    people.push((
        "Rizky Saputra".into(),
        "floater@bikeshop.test".into(),
        CASHIER,
        Some(stores[0].id),
    ));

    let mut tx = db.begin().await?;
    let logins: Vec<(String, String)> = people
        .iter()
        .map(|(n, e, _, _)| (n.clone(), e.clone()))
        .collect();
    let ids = insert_users(&mut tx, &logins).await?;
    let mut rows = Vec::new();
    for (_, email, _, store) in &people {
        rows.push(Staff {
            user_id: ids[email],
            home_store_id: store.unwrap_or(stores[0].id),
            hired_on: Some(today() - Duration::days(world.rng.range(60, 2_000))),
            active: true,
            ..Default::default()
        });
    }
    Staff::insert_many(&mut tx, rows).await?;
    let staff_ids: HashMap<i64, i64> = sql("SELECT user_id, id FROM staff")
        .fetch_as::<(i64, i64)>(&mut tx)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let users: HashMap<i64, User> = User::find_many(db, ids.values().copied())
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();
    let store_name = |id: i64| {
        stores
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };
    for (_, email, role, store) in &people {
        let user = &users[&ids[email]];
        match store {
            None => user.assign_role(db, role).await?,
            Some(store) => user.assign_role_in(db, role, &store_scope(*store)).await?,
        }
        let person = Person {
            user_id: user.id,
            staff_id: staff_ids[&user.id],
            store_id: store.unwrap_or(stores[0].id),
            role,
        };
        if store.is_some() {
            world.staff.push(person);
        }
        let what = match store {
            None => "Owner: every permission in every store (a global role)".to_owned(),
            Some(id) => format!("{} of {}", label(role), store_name(*id)),
        };
        world.users.push(DemoUser {
            email: email.clone(),
            what,
        });
    }

    // One person with roles in two stores: the floater is also a cashier at South.
    let floater = &users[&ids["floater@bikeshop.test"]];
    floater
        .assign_role_in(db, CASHIER, &store_scope(stores[1].id))
        .await?;
    if let Some(user) = world
        .users
        .iter_mut()
        .find(|u| u.email == "floater@bikeshop.test")
    {
        user.what = format!(
            "Cashier of {} and of {} (roles in two stores)",
            stores[0].name, stores[1].name
        );
    }

    // West's mechanic helps South this week: an approved help request
    // becomes a mechanic role in South from Monday to next Monday.
    let helper = &users[&ids["mechanic.west@bikeshop.test"]];
    let west_manager = &users[&ids["manager.west@bikeshop.test"]];
    let south_manager = &users[&ids["manager.south@bikeshop.test"]];
    let (west, south) = (stores[2].id, stores[1].id);
    let mut request = StaffHelpRequest::factory()
        .lending(staff_ids[&helper.id], west, south)
        .approved_by(west_manager.id)
        .this_week()
        .make_one();
    request.role = MECHANIC.into();
    request.requested_by = Some(south_manager.id);
    request.reason = "Two mechanics on leave; the workshop is booked full this week.".into();
    request.insert(db).await?;
    helper
        .assign_role_in(db, MECHANIC, &store_scope(south))
        .from(request.starts_at)
        .until(request.ends_at)
        .await?;
    let (monday, _) = this_week();
    let mut day = monday.date_naive();
    while day < today() {
        StaffHelpHour::create(
            db,
            StaffHelpHour {
                help_request_id: request.id,
                staff_id: request.staff_id,
                store_id: south,
                worked_on: day,
                minutes: world.rng.range(6, 9) * 60,
                ..Default::default()
            },
        )
        .await?;
        day += Duration::days(1);
    }
    // An earlier help that has ended, and a request still waiting.
    let mut ended = StaffHelpRequest::factory()
        .lending(
            staff_ids[&users[&ids["staff.north@bikeshop.test"]].id],
            stores[0].id,
            west,
        )
        .approved_by(users[&ids["manager.north@bikeshop.test"]].id)
        .make_one();
    ended.starts_at = midnight(today() - Duration::days(40));
    ended.ends_at = midnight(today() - Duration::days(33));
    ended.requested_by = Some(west_manager.id);
    ended.insert(db).await?;
    users[&ids["staff.north@bikeshop.test"]]
        .assign_role_in(db, STAFF, &store_scope(west))
        .from(ended.starts_at)
        .until(ended.ends_at)
        .await?;
    let mut waiting = StaffHelpRequest::factory()
        .lending(
            staff_ids[&users[&ids["cashier.south@bikeshop.test"]].id],
            south,
            stores[0].id,
        )
        .make_one();
    waiting.status = HelpStatus::Requested;
    waiting.requested_by = Some(users[&ids["manager.north@bikeshop.test"]].id);
    waiting.reason = "Weekend market: we expect twice the usual rentals.".into();
    waiting.insert(db).await?;

    if let Some(user) = world
        .users
        .iter_mut()
        .find(|u| u.email == "mechanic.west@bikeshop.test")
    {
        user.what = format!(
            "Mechanic of {}, helping {} this week (a role with dates)",
            stores[2].name, stores[1].name
        );
    }
    Ok(())
}

fn label(role: &str) -> &'static str {
    perms::roles()
        .into_iter()
        .find(|r| r.name == role)
        .map(|r| r.label)
        .unwrap_or("Staff")
}

/// The rental fleet: bikes of rentable models per store, some placed at
/// another store.
async fn fleet(tx: &mut Transaction, world: &mut World) -> Result {
    let rentable: Vec<Item> = world
        .items
        .iter()
        .filter(|i| {
            [
                "city-bikes",
                "mountain-bikes",
                "e-bikes",
                "road-bikes",
                "folding-bikes",
            ]
            .contains(&i.category)
        })
        .cloned()
        .collect();
    if rentable.is_empty() {
        return Ok(());
    }
    let stores: Vec<i64> = world.stores.iter().map(|s| s.id).collect();
    let mut bikes = Vec::new();
    for store in &stores {
        for _ in 0..world.volume.bikes_per_store {
            let item = world.rng.pick(&rentable).clone();
            let daily = match item.category {
                "e-bikes" => world.rng.price(7_500, 9_000),
                "road-bikes" => world.rng.price(6_000, 7_500),
                "mountain-bikes" => world.rng.price(5_000, 6_500),
                _ => world.rng.price(4_000, 5_000),
            };
            let mut bike = rental_bikes()
                .model(item.variant_id)
                .owned_by(*store)
                .make_one();
            bike.daily_rate = daily;
            bike.hourly_rate = daily / 5 / 100 * 100;
            bike.deposit = daily * 5;
            bike.asset_value = item.price * 7 / 10;
            bike.ridden_hours = world.rng.range(20, 1_500);
            // Serviced recently for most bikes (up to 180 of the 200 hours
            // between services), so only a few are due when the demo opens.
            bike.serviced_at_hours = (bike.ridden_hours - world.rng.range(0, 180)).max(0);
            bike.purchased_on = Some(
                today() - Duration::days(world.rng.range(60, world.volume.history_days + 300)),
            );
            bikes.push(bike);
        }
    }
    RentalBike::insert_many(&mut *tx, bikes).await?;
    world.bikes = RentalBike::query()
        .order_by(RentalBike::ID)
        .get(&mut *tx)
        .await?;

    // About one in ten is placed at another store, some time in the last
    // eight months; the owner store keeps it in its books.
    let mut placements = Vec::new();
    for i in 0..world.bikes.len() {
        // (The fourth bike of each store always is, so even the small seed has some.)
        let fourth = i % world.volume.bikes_per_store.max(1) == 3;
        if !(fourth || world.rng.chance(10)) {
            continue;
        }
        let bike = world.bikes[i].clone();
        let to = world.other_store(bike.owner_store_id);
        let moved = renox::db::now()
            - Duration::days(world.rng.range(5, world.volume.history_days.min(240)));
        let approver = world
            .staff_of(bike.owner_store_id, MANAGER)
            .first()
            .map(|p| p.staff_id);
        let asker = world.staff_of(to, MANAGER).first().map(|p| p.staff_id);
        placements.push(BikePlacement {
            rental_bike_id: bike.id,
            from_store_id: bike.owner_store_id,
            to_store_id: to,
            status: PlacementStatus::Moved,
            requested_at: moved - Duration::days(3),
            approved_at: Some(moved - Duration::days(2)),
            moved_at: Some(moved),
            requested_by: asker,
            approved_by: approver,
            note: Some("Busy season at the receiving store.".into()),
            ..Default::default()
        });
        world.placements.insert(bike.id, (to, moved));
        world.bikes[i].location_store_id = to;
        sql("UPDATE rental_bikes SET location_store_id = ? WHERE id = ?")
            .bind(to)
            .bind(bike.id)
            .execute(&mut *tx)
            .await?;
    }
    BikePlacement::insert_many(&mut *tx, placements).await?;
    let north = world.stores[0].id;
    world.demo_bike = world
        .bikes
        .iter()
        .find(|b| b.owner_store_id == north && b.location_store_id == north)
        .map(|b| b.id);
    Ok(())
}

/// The demo customer: two bikes, a plan, rentals past and present, built
/// with the factories' states.
async fn demo_customer(db: &Db, world: &mut World) -> Result {
    use crate::app::plans::factories::{SubscriptionStates, plan_subscriptions};
    use crate::app::rentals::factories::{RentalStates, rentals};
    use crate::app::sales::factories::{PaymentStates, payments};
    use crate::app::workshop::factories::{WorkOrderStates, customer_bikes_of, work_orders};

    let mut tx = db.begin().await?;
    let ids = insert_users(
        &mut tx,
        &[("Sofia Wijaya".into(), "customer@bikeshop.test".into())],
    )
    .await?;
    tx.commit().await?;
    let user_id = ids["customer@bikeshop.test"];
    let address = Address::query()
        .order_by(Address::ID)
        .first(db)
        .await?
        .map(|a| a.id);
    let mut customer = Customer::factory().verified().make_one();
    customer.name = "Sofia Wijaya".into();
    customer.email = Some("customer@bikeshop.test".into());
    customer.user_id = Some(user_id);
    customer.address_id = address;
    customer.insert(db).await?;

    let north = world.stores[0].id;
    let (product_id, product_name) = world
        .bike_products
        .first()
        .cloned()
        .unwrap_or((0, "City bike".into()));
    let mut bikes = Vec::new();
    for (name, product) in [
        (
            format!("{product_name}, blue"),
            Some(product_id).filter(|id| *id > 0),
        ),
        ("Folding bike for the train".to_owned(), None),
    ] {
        let mut bike = customer_bikes_of(customer.id).make_one();
        bike.name = name;
        bike.product_id = product;
        bike.bought_on = Some(today() - Duration::days(200));
        bike.insert(db).await?;
        bikes.push(bike);
    }
    if let Some((plan, _)) = world
        .plans
        .iter()
        .find(|(p, _)| p.frequency == Frequency::Monthly)
    {
        plan_subscriptions()
            .of(bikes[0].id, plan.id, north)
            .due_soon()
            .create_one(db)
            .await?;
    }
    let demo_bike = world
        .bikes
        .iter()
        .find(|b| Some(b.id) == world.demo_bike)
        .cloned();
    if let Some(fleet_bike) = &demo_bike {
        let past = rentals()
            .count(3)
            .of_bike(fleet_bike)
            .for_customer(customer.id)
            .returned()
            .sequence(|i, r| {
                let back = Duration::days(10 * (i as i64 + 1));
                r.starts_at -= back;
                r.due_at -= back;
                r.picked_up_at = Some(r.starts_at);
                r.returned_at = r.returned_at.map(|t| t - back);
            })
            .create(db)
            .await?;
        for rental in &past {
            payments().for_rental(rental).paid().create_one(db).await?;
        }
        let mut current = rentals()
            .of_bike(fleet_bike)
            .for_customer(customer.id)
            .due_today()
            .make_one();
        current.served_by = world.counter_person(north);
        current.insert(db).await?;
        sql("UPDATE rental_bikes SET status = ? WHERE id = ?")
            .bind(crate::app::rentals::model::BikeStatus::Rented)
            .bind(fleet_bike.id)
            .execute(db)
            .await?;
        payments()
            .for_rental(&current)
            .paid()
            .create_one(db)
            .await?;
    }
    work_orders()
        .at(north)
        .on_bike(bikes[0].id)
        .completed(9_000)
        .create_one(db)
        .await?;
    work_orders()
        .at(north)
        .on_bike(bikes[1].id)
        .create_one(db)
        .await?;

    world.users.push(DemoUser {
        email: "customer@bikeshop.test".into(),
        what: "Customer with two bikes, a monthly plan, a rental due today and past rentals".into(),
    });
    Ok(())
}

//! The demo data: `db:seed` (a small shop, in seconds) and
//! `demo:seed --size large` (Pagila's volume: 18 months of a busy shop).
//!
//! Both build the same shop with [`shop::build`], only the [`Volume`]
//! differs, from the area factories (`src/app/<area>/factories.rs`: each
//! model's `Factory` with states such as `rentals().overdue()`) and a
//! random generator with a fixed seed ([`rng::Rng`]), so every run makes
//! the same people, bikes and stories. Dates are counted back from today
//! (`renox::db::now()`), so the data is never stale: a rental due today is
//! due today whenever you seed, and the history ends now.
//!
//! Running a seed twice doesn't duplicate anything: when the stores exist
//! already, `db:seed` says so and does nothing, and `demo:seed` refuses
//! with how to start again (`migrate:fresh`).
//!
//! The demo users (password `password`) are printed at the end: the
//! owner; each store's manager, cashier and mechanic; one person with roles
//! in two stores; one helping another store this week; a customer with
//! bikes, rentals and a plan.

pub mod content;
pub mod fixtures;
mod history;
pub mod rng;
pub mod shop;

use renox::clap;
use renox::command::AppCommand;
use renox::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub use shop::{DemoUser, Summary, Volume};

/// A number never handed out before in this process, for unique test
/// values (slugs, SKUs, emails) in factories.
pub fn unique() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Today's date (UTC), from Renox's clock (so `TestApp::travel` moves it).
pub fn today() -> renox::chrono::NaiveDate {
    renox::db::now().date_naive()
}

/// The demo users' password.
pub const DEMO_PASSWORD: &str = "password";

/// `db:seed`: the small shop, unless the database already has stores.
pub async fn run(state: AppState) -> Result {
    if shop::seeded(&state.db).await? {
        println!("The shop is seeded already (it has stores); nothing to do.");
        return Ok(());
    }
    let summary = shop::build(&state.db, Volume::small()).await?;
    print_summary(&summary);
    Ok(())
}

/// How much demo data `demo:seed` makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Size {
    /// The same as `db:seed`: about 150 products, 50 customers, a few
    /// hundred rentals.
    Small,
    /// Pagila's volume: ~1,000 variants, ~600 customers, ~16,000 rentals,
    /// ~5,000 orders, ~3,000 work orders over 18 months.
    Large,
}

/// `demo:seed [--size small|large]`: fills an empty database with the demo
/// shop and prints the demo users. Refuses when there are stores already.
#[derive(clap::Parser, Debug)]
#[command(
    name = "demo:seed",
    about = "Fill an empty database with the demo bike shop"
)]
pub struct DemoSeed {
    /// `small` (seconds) or `large` (Pagila's volume).
    #[arg(long, value_enum, default_value = "small")]
    pub size: Size,
}

impl AppCommand for DemoSeed {
    async fn run(self, state: AppState) -> Result {
        if shop::seeded(&state.db).await? {
            return Err(renox::anyhow::anyhow!(
                "the database already has the shop's data; to start again run \
                 `cargo run -- migrate:fresh` and then `cargo run -- demo:seed --size {}`",
                match self.size {
                    Size::Small => "small",
                    Size::Large => "large",
                }
            )
            .into());
        }
        let volume = match self.size {
            Size::Small => Volume::small(),
            Size::Large => Volume::large(),
        };
        let started = std::time::Instant::now();
        let summary = shop::build(&state.db, volume).await?;
        print_summary(&summary);
        println!("Done in {:.1} s.", started.elapsed().as_secs_f64());
        Ok(())
    }
}

/// Prints the counts and the demo users.
pub fn print_summary(summary: &Summary) {
    println!("Seeded the bike shop:");
    for (what, count) in &summary.counts {
        println!("  {count:>7} {what}");
    }
    println!();
    println!("Demo users (password `{DEMO_PASSWORD}`):");
    let width = summary
        .users
        .iter()
        .map(|u| u.email.len())
        .max()
        .unwrap_or(0);
    for user in &summary.users {
        println!("  {:width$}  {}", user.email, user.what);
    }
}

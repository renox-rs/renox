//! # Renox
//!
//! A batteries-included web framework for Rust, inspired by Laravel.
//! Axum + HTMX + Alpine.js + SQLite or PostgreSQL.
//!
//! ```no_run
//! use renox::prelude::*;
//!
//! struct Hello;
//!
//! impl Module for Hello {
//!     fn name(&self) -> &'static str { "hello" }
//!
//!     fn routes(&self) -> Routes {
//!         Routes::new().get("/", home).name("home")
//!     }
//! }
//!
//! async fn home() -> View {
//!     view("home.html", context! { title => "Hello from Renox" })
//! }
//!
//! fn main() -> renox::Result {
//!     App::new().module(Hello).run()
//! }
//! ```
#![warn(missing_docs)]

pub use renox_core::*;
pub use renox_macros::{
    DbEnum, FromRow, Model, Validate, ViewData, embedded, live_component, migrations, test,
};

pub use axum;
pub use tokio;

/// What most files of an app import: `use renox::prelude::*;` brings the
/// app builder, modules and routes, models and queries, validation, auth,
/// views, jobs and events, and axum's extractors and responses.
pub mod prelude {
    pub use renox_core::auth::{Auth, Can, User};
    pub use renox_core::db::{DateTime, Db, Factory, FromRow, Model, Page, Paginated};
    pub use renox_core::events::Event;
    pub use renox_core::live_component::LiveComponent;
    pub use renox_core::queue::{Job, JobContext};
    pub use renox_core::serde_json::json;
    pub use renox_core::webhook::{SignedRequest, Webhook, WebhookCall, WebhookRequest};
    pub use renox_core::{
        App, AppState, Back, Config, Environment, Error, Errors, Htmx, HxRedirect, HxRefresh,
        HxTrigger, Module, Resource, Result, Routes, Session, Toast, Valid, Validate,
        ValidationError, Validator, View, ViewData, context, view,
    };
    pub use renox_core::{AuthUser, ClientIp, KeyValues, Lang, Policy, Registry, Upload};
    pub use renox_core::{RedirectExt, abort, abort_if, abort_unless};
    pub use renox_macros::{DbEnum, FromRow, Model, Validate, ViewData};

    pub use axum::extract::{Form, Json, Query, State};
    pub use axum::http::StatusCode;
    pub use axum::response::{Html, IntoResponse, Redirect, Response};
    pub use renox_core::{Found, Path};
}

/// Compiles the Rust in docs/types.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/types.md")]
pub struct TypesGuide;

/// Compiles the Rust in docs/relations.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/relations.md")]
pub struct RelationsGuide;

/// Compiles the Rust in docs/search.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/search.md")]
pub struct SearchGuide;

/// Compiles the Rust in docs/authorization.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/authorization.md")]
pub struct AuthorizationGuide;

/// Compiles the Rust in docs/queue.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/queue.md")]
pub struct QueueGuide;

/// Compiles the Rust in docs/ui.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/ui.md")]
pub struct UiGuide;

/// Compiles the Rust in docs/grid.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/grid.md")]
pub struct GridGuide;

/// Compiles the Rust in docs/validation.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/validation.md")]
pub struct ValidationGuide;

/// Compiles the Rust in docs/routing.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/routing.md")]
pub struct RoutingGuide;

/// Compiles the Rust in docs/mail.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/mail.md")]
pub struct MailGuide;

/// Compiles the Rust in docs/scheduling.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/scheduling.md")]
pub struct SchedulingGuide;

/// Compiles the Rust in docs/live.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/live.md")]
pub struct LiveGuide;

/// Compiles the Rust in docs/state-machines.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/state-machines.md")]
pub struct StateMachinesGuide;

/// Compiles the Rust in docs/testing.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/testing.md")]
pub struct TestingGuide;

/// Compiles the Rust in docs/tutorial.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/tutorial.md")]
pub struct TutorialGuide;

/// Compiles the Rust in docs/laravel.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/laravel.md")]
pub struct LaravelGuide;

/// Compiles the Rust in docs/operations.md as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/operations.md")]
pub struct OperationsGuide;

/// Compiles every Rust example in the README as a doctest, so the front page
/// can't drift from the API.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
pub struct ReadMe;

/// Compiles every Rust example in `CHEATSHEET.md` as a doctest, so the
/// cheat-sheet can't drift from the API.
#[cfg(doctest)]
#[doc = include_str!("../../../CHEATSHEET.md")]
pub struct CheatSheet;

/// Mistakes the derives turn into compile errors (checked as doctests).
///
/// A model needs an `id`:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// struct NoId { name: String }
/// ```
///
/// The `id` must be a key type (`i64`, `Ulid`, `Uuid` or `String`):
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// struct FloatId { id: f64, name: String }
/// ```
/// …which these are:
/// ```
/// # use renox::prelude::*;
/// #[derive(Model)]
/// struct Numbered { id: i64, name: String }
/// #[derive(Model)]
/// struct Coded { id: String, name: String }
/// #[derive(Model)]
/// struct Sortable { id: renox::db::Ulid, name: String }
/// ```
///
/// A `#[validate(…)]` rule must exist (a typo is a compile error):
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// struct Typo { #[validate(requred)] name: String }
/// ```
/// …while real ones compile, with arguments that may use `self`:
/// ```
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// struct Ok { #[validate(required, max = 10, same("again", &self.again))] name: String, again: String }
/// ```
///
/// A `ViewData` page names its template:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(serde::Serialize, ViewData)]
/// struct NoTemplate { title: String }
/// ```
/// …which this one does:
/// ```
/// # use renox::prelude::*;
/// #[derive(serde::Serialize, ViewData)]
/// #[view(template = "page.html")]
/// struct WithTemplate { title: String }
/// ```
///
/// `Validate` is derived for structs with named fields only:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// enum NotAForm { A }
/// ```
///
/// `soft_deletes` needs `deleted_at`:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(soft_deletes)]
/// struct NoDeletedAt { id: i64 }
/// ```
///
/// `search` names the model's own columns:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(search = "title, bodyy")]
/// struct Post { id: i64, title: String, body: String }
/// ```
///
/// `search_language` is a plain lowercase name:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(search = "title", search_language = "english'; --")]
/// struct Post { id: i64, title: String }
/// ```
///
/// Unknown attributes are refused:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(tabel = "typo")]
/// struct Typo { id: i64 }
/// ```
///
/// Generic structs can't be models:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// struct Generic<T> { id: i64, value: T }
/// ```
///
/// `DbEnum` is for fieldless enums:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(DbEnum)]
/// enum WithData { A(i64) }
/// ```
///
/// `FromRow` needs named fields:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(FromRow)]
/// struct Tuple(i64);
/// ```
///
/// A field type that can't come from a column doesn't compile either:
/// ```compile_fail
/// # use renox::prelude::*;
/// struct NotAColumn;
/// #[derive(FromRow)]
/// struct Row { value: NotAColumn }
/// ```
///
/// A database's own `.down.sql` without its own `.up.sql` (here a
/// `.postgres.down.sql` next to a plain `.up.sql`) would be ignored, so
/// `migrations!` refuses it:
/// ```compile_fail
/// let _ = renox::migrations!("tests/migrations_bad_down");
/// ```
/// …while a directory without that mistake compiles:
/// ```
/// let _ = renox::migrations!("tests/migrations_types");
/// ```
///
/// A migration needs a plain `.up.sql`, or one for each database (here only
/// `.sqlite.up.sql`):
/// ```compile_fail
/// let _ = renox::migrations!("tests/migrations_sqlite_only");
/// ```
///
/// `embedded!()` takes no arguments:
/// ```compile_fail
/// let _ = renox::embedded!("resources");
/// ```
///
/// `Model` is derived for structs with named fields:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// enum NotAModel { A }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// struct Tuple(i64, String);
/// ```
///
/// A field takes only `#[model(skip)]`:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// struct Post { id: i64, #[model(hidden)] title: String }
/// ```
/// …and a skipped field anywhere is fine:
/// ```
/// # use renox::prelude::*;
/// #[derive(Model)]
/// struct Post { id: i64, #[model(skip)] cached: String, title: String }
/// assert_eq!(Post::COLUMNS, &["id", "title"]);
/// ```
///
/// `search` lists each column once, at least one, and never `id`:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(search = "title, title")]
/// struct Post { id: i64, title: String }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(search = " , ")]
/// struct Post { id: i64, title: String }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(search = "id")]
/// struct Post { id: i64, title: String }
/// ```
///
/// `search_language` needs `search`:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model)]
/// #[model(search_language = "simple")]
/// struct Post { id: i64, title: String }
/// ```
///
/// `Validate` takes `hooks` and `bag` on the struct, nothing else:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// #[validate(strict)]
/// struct Form { name: String }
/// ```
/// …and needs named fields:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// struct Form(String);
/// ```
/// `rename` is `rename = "field"`, and `each` a list of rules:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// struct Form { #[validate(rename("other"))] name: String }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// struct Form { #[validate(each = "required")] tags: Vec<String> }
/// ```
/// A raw identifier compiles (it's checked as `type`; see `it/derive_validate.rs`):
/// ```
/// # use renox::prelude::*;
/// #[derive(serde::Deserialize, Validate)]
/// struct Form { #[validate(required)] r#type: String }
/// ```
///
/// `DbEnum` is for enums, each text used once, with `rename` only:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(DbEnum)]
/// struct NotAnEnum { a: i64 }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(DbEnum)]
/// enum Status { Open, #[db(rename = "open")] Opened }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(DbEnum)]
/// enum Status { #[db(label = "Open")] Open }
/// ```
///
/// `FromRow` is for plain structs, with `skip` or `rename` only:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(FromRow)]
/// struct Row<T> { value: T }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(FromRow)]
/// enum Row { A }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(FromRow)]
/// struct Row { #[row(default)] value: i64 }
/// ```
///
/// Typed columns are checked by the compiler:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// struct Product { id: i64, price: i64 }
/// let _ = Product::query().where_(Product::PRICE.lt("cheap"));
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// struct Product { id: i64, name: String }
/// #[derive(Model, Default)]
/// struct Order { id: i64, total: i64 }
/// let _ = Order::query().order_by(Product::NAME);
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// struct Product { id: i64, price: i64 }
/// let _ = Product::PRCE;
/// ```
///
/// Index columns must exist, and a default is a string:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// #[model(index(nope))]
/// struct Post { id: i64, title: String }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// struct Post { id: i64, #[model(default = 0)] views: i64 }
/// ```
///
/// Form misuse:
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// struct Task { id: i64, #[form(skip)] owner_id: i64 }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// #[model(form)]
/// struct Task { id: i64, #[form(upload)] size: i64 }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// #[derive(Model, Default)]
/// #[model(form)]
/// struct Task { id: i64, #[form(bogus)] title: String }
/// ```
///
/// `#[live_component]` misuse:
/// ```compile_fail
/// # use renox::prelude::*;
/// # use renox::live_component::LiveContext;
/// # #[derive(serde::Serialize, serde::Deserialize)]
/// # struct Todo {}
/// #[renox::live_component(name = "todo")]
/// impl Todo {
///     #[live(action)]
///     async fn toggle(&mut self, _ctx: &mut LiveContext) -> Result { Ok(()) }
/// }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// # use renox::live_component::LiveContext;
/// # #[derive(serde::Serialize, serde::Deserialize)]
/// # struct Todo {}
/// #[renox::live_component(view = "todo.html")]
/// impl Todo {
///     #[live(action)]
///     async fn toggle(&self, _ctx: &mut LiveContext) -> Result { Ok(()) }
/// }
/// ```
/// ```compile_fail
/// # use renox::prelude::*;
/// # use renox::live_component::LiveContext;
/// # #[derive(serde::Serialize, serde::Deserialize)]
/// # struct Todo {}
/// #[renox::live_component(view = "todo.html")]
/// impl Todo {
///     #[live(action)]
///     async fn _toggle(&mut self, _ctx: &mut LiveContext) -> Result { Ok(()) }
/// }
/// ```
#[cfg(doctest)]
pub struct MacroCompileErrors;

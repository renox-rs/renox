use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use axum::Router;
use axum::handler::HandlerWithoutStateExt;
use axum::middleware::{from_fn, from_fn_with_state};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::auth::{Gate, LoginThrottle, User};
use crate::crypto::parse_key;
use crate::db::{Db, Migration, MigrationStatus, Migrator};
use crate::events::Event;
use crate::mail::Mailer;
use crate::queue::{Handlers, Job, Queue, Worker};
use crate::routing::RouteInfo;
use crate::schedule::Schedule;
use crate::timezone::Zone;
use crate::{
    AppState, Config, Environment, Error, Module, Registry, Result, RouteTable, Views, assets,
    auth, csrf, session, view,
};

/// How long `serve` waits for running jobs after a shutdown signal.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

const USAGE: &str = "\
Usage: <app> [command]

Commands:
  serve                     Start the web server, queue workers and scheduler (default)
  migrate                   Run pending migrations
  migrate:rollback [--step N]
                            Undo the last N batches of migrations (default 1)
  migrate:fresh [--seed]    Drop all tables, run every migration, optionally seed
  migrate:status            List migrations and whether they have run
  db:check                  Compare the registered models with the tables the migrations build
  db:diff [name] [--yes]    Write a migration for what changed in the registered models
  db:seed                   Run the seeders
  queue:work [--queue a,b] [--workers N] [--once]
                            Run queued jobs (until stopped, or --once for what's there)
  queue:failed              List failed jobs
  queue:retry <id|all>      Put failed jobs back on the queue
  queue:flush               Delete failed jobs
  queue:forget <id>         Delete one failed job
  queue:prune-failed [--hours N]
                            Delete failed jobs older than N hours (default 168)
  queue:prune-batches [--hours N]
                            Delete batches finished more than N hours ago (default 24)
  webhook:failed            List webhook calls whose processing failed
  webhook:retry <id>        Process a stored webhook call again
  ui:publish [--force]      Copy the UI kit (renox/ui.html and its CSS) into the app to change it
  sequence:list             List the numbering series (`Sequence`) and their last numbers
  cache:prune               Delete expired rows of the database cache store
  session:prune             Delete expired sessions (SESSION_DRIVER=database)
  import:prune [--days N]   Delete the records of imports whose undo window ended N days ago (default 0)
  schedule:list             List scheduled tasks and when they run next
  schedule:run <task>       Run one scheduled task now
  schedule:work             Run scheduled tasks (when SCHEDULER=false for serve)
  route:list                List every route with its name, module and guards
  view:check                Compile every template; check route names and typed views
  view:data [--out P]       Write .vscode/renox-components.json for editor autocomplete
  db:shell                  Run SQL against the database (`.tables`, `.quit`)
  down [--secret S] [--retry N]
                            Maintenance mode: answer 503 (visit /S to bypass it)
  up                        Leave maintenance mode
  help                      Show this message";

/// The application builder.
///
/// ```no_run
/// # use renox::prelude::*;
/// # use serde::{Deserialize, Serialize};
/// # struct Products;
/// # impl Module for Products { fn name(&self) -> &'static str { "products" } }
/// # #[derive(Serialize, Deserialize)] struct SendReceipt;
/// # impl Job for SendReceipt { const NAME: &'static str = "send-receipt"; async fn handle(self, _: JobContext) -> Result { Ok(()) } }
/// # async fn cleanup(_: AppState) -> Result { Ok(()) }
/// # async fn seed(_: AppState) -> Result { Ok(()) }
/// fn main() -> renox::Result {
///     App::new()
///         .migrations(renox::migrations!())
///         .module(Products)
///         .job::<SendReceipt>()
///         .schedule(|s| { s.daily_at("02:00", "cleanup", cleanup); })
///         .seeder(seed)
///         .run()
/// }
/// ```
///
/// The built binary is also the app's command line, like Laravel's artisan:
/// `my-app migrate`, `my-app queue:work`, `my-app help`.
pub struct App {
    config: Option<Config>,
    modules: Vec<Box<dyn Module>>,
    migrations: Vec<Migration>,
    gates: HashMap<String, Gate>,
    async_gates: HashMap<String, crate::auth::AsyncGate>,
    gate_before: Option<crate::auth::GateBefore>,
    registry: Registry,
    embedded: Option<crate::Embedded>,
    csp: crate::security::Csp,
    permissions_policy: crate::security::PermissionsPolicy,
    provided: HashMap<std::any::TypeId, Arc<dyn std::any::Any + Send + Sync>>,
    layers: Vec<AppLayer>,
    limiters: HashMap<String, crate::rate_limit::LimitRule>,
    detect_locale: bool,
    rtl_locales: Option<Vec<String>>,
    disks: Vec<(String, DiskSettings)>,
    uploads: crate::upload::Pipeline,
    mailers: Vec<(String, MailerSettings)>,
    mailer_limits: HashMap<String, crate::queue::Limit>,
    xsrf_cookie: bool,
    settings_seed: Option<std::path::PathBuf>,
}

/// How `App::mailer` gets a mailer's settings once the configuration is loaded.
type MailerSettings = Box<dyn Fn(&Config) -> Result<crate::mail::MailConfig> + Send + Sync>;

/// How `App::disk` gets a disk's settings once the configuration is loaded.
type DiskSettings = Box<dyn Fn(&Config) -> Result<crate::storage::StorageConfig> + Send + Sync>;

/// A layer from `App::layer`, applied to the app's routes at boot.
/// Applied to the app's router and to each `Routes::domain` router.
type AppLayer = Box<dyn Fn(Router<AppState>) -> Router<AppState> + Send + Sync>;

impl App {
    /// Creates an application that loads its configuration from `.env` on start.
    pub fn new() -> Self {
        Self {
            config: None,
            modules: Vec::new(),
            migrations: Vec::new(),
            gates: HashMap::new(),
            async_gates: HashMap::new(),
            gate_before: None,
            registry: Registry::default(),
            embedded: None,
            csp: crate::security::Csp::default(),
            permissions_policy: crate::security::PermissionsPolicy::default(),
            provided: HashMap::new(),
            layers: Vec::new(),
            limiters: HashMap::new(),
            detect_locale: false,
            rtl_locales: None,
            disks: Vec::new(),
            uploads: crate::upload::Pipeline::new(),
            mailers: Vec::new(),
            mailer_limits: HashMap::new(),
            xsrf_cookie: false,
            settings_seed: None,
        }
    }

    /// Picks a visitor's language from their browser (`Accept-Language`)
    /// when they haven't chosen one: the first of their languages this app
    /// has texts for (the built-in `en` and `id`, or a
    /// `resources/lang/<locale>.json`), else `APP_LOCALE`. A language set
    /// with `i18n::remember_locale` still wins. Responses then carry
    /// `Vary: Accept-Language`, so caches keep the languages apart.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().detect_locale()
    /// # ;
    /// ```
    pub fn detect_locale(mut self) -> Self {
        self.detect_locale = true;
        self
    }

    /// Replaces the languages written right to left (by default Arabic,
    /// Hebrew, Persian, Urdu, Pashto, Yiddish, Divehi and Central Kurdish)
    /// with `locales`, matched by base language (`ar` covers `ar-AE`).
    /// `AppState::direction` then answers from this list.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().rtl_locales(&["ar", "he"])
    /// # ;
    /// ```
    pub fn rtl_locales(mut self, locales: &[&str]) -> Self {
        self.rtl_locales = Some(locales.iter().map(|l| (*l).to_owned()).collect());
        self
    }

    /// Wraps every route of the app's modules in a tower layer, e.g. a
    /// middleware function (framework routes such as `/health` and
    /// `public/` files aren't wrapped). It runs after Renox has loaded the
    /// session and the user, so it can use `AuthUser` or `Session`:
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::axum::extract::Request;
    /// use renox::axum::middleware::{Next, from_fn};
    ///
    /// async fn stamp(user: Option<AuthUser>, req: Request, next: Next) -> Response {
    ///     let mut res = next.run(req).await;
    ///     let who = if user.is_some() { "member" } else { "guest" };
    ///     res.headers_mut().insert("x-visitor", who.parse().unwrap());
    ///     res
    /// }
    ///
    /// # let _ =
    /// App::new().layer(from_fn(stamp))
    /// # ;
    /// ```
    ///
    /// Layers run in the order added: the first one sees the request first.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: tower::Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
        L::Service: tower::Service<axum::extract::Request> + Clone + Send + Sync + 'static,
        <L::Service as tower::Service<axum::extract::Request>>::Response:
            axum::response::IntoResponse + 'static,
        <L::Service as tower::Service<axum::extract::Request>>::Error:
            Into<std::convert::Infallible> + 'static,
        <L::Service as tower::Service<axum::extract::Request>>::Future: Send + 'static,
    {
        self.layers
            .push(Box::new(move |router| router.layer(layer.clone())));
        self
    }

    /// Allows other sites in the Content-Security-Policy, e.g.
    /// `.csp(|csp| { csp.allow("script-src", "https://www.googletagmanager.com"); })`.
    pub fn csp(mut self, allow: impl FnOnce(&mut crate::security::Csp)) -> Self {
        allow(&mut self.csp);
        self
    }

    /// Allows browser features in the `Permissions-Policy`, which switches off
    /// the ones a typical app never uses (camera, microphone, geolocation,
    /// payment, USB…): `.permissions_policy(|p| { p.allow("geolocation", "self"); })`.
    /// A feature can be allowed for `self`, for an origin
    /// (`"https://pay.example.com"`) or for everyone (`"*"`). `PERMISSIONS_POLICY`
    /// in `.env` sets the base: `default`, `off` or a full header value.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().permissions_policy(|p| {
    ///     p.allow("camera", "self");
    /// })
    /// # ;
    /// ```
    pub fn permissions_policy(
        mut self,
        allow: impl FnOnce(&mut crate::security::PermissionsPolicy),
    ) -> Self {
        allow(&mut self.permissions_policy);
        self
    }

    /// Views, translations and public files compiled into the binary:
    /// `.embed(renox::embedded!())`. They're used when `APP_DEBUG` is off;
    /// while debugging, files are read from disk so edits show up at once.
    pub fn embed(mut self, embedded: crate::Embedded) -> Self {
        self.embedded = Some(embedded);
        self
    }

    /// Uses the given configuration instead of loading it from the environment.
    pub fn with_config(config: Config) -> Self {
        Self {
            config: Some(config),
            ..Self::new()
        }
    }

    /// Replaces the configuration, e.g. in tests.
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Adds a module: its routes, migrations and registrations.
    pub fn module(mut self, module: impl Module) -> Self {
        self.modules.push(Box::new(module));
        self
    }

    /// Registers app-level migrations, usually `renox::migrations!()`.
    pub fn migrations(mut self, migrations: &[Migration]) -> Self {
        self.migrations.extend_from_slice(migrations);
        self
    }

    /// Registers a seeder for `db:seed`. Seeders run in registration order
    /// (the app's, then each module's from `Registry::seeder`, in the order
    /// the modules were added), in the app's context (model hooks and `renox::context::app()` see
    /// it), and get the app's state: `state.db`, its config, storage…
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64 }
    /// # impl Factory for Product { fn definition() -> Self { Product::default() } }
    /// # let _ =
    /// App::new().seeder(|state| async move {
    ///     Product::factory().count(50).create(&state.db).await?;
    ///     Ok(())
    /// })
    /// # ;
    /// ```
    pub fn seeder<F, Fut>(mut self, seeder: F) -> Self
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.registry.seeder(seeder);
        self
    }

    /// Defines a gate: an ability that depends only on the user.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().gate("admin", |user| user.email.ends_with("@shop.example"))
    /// // in a handler: auth.gate("admin")?;   in a template: {% if can('admin') %}
    /// # ;
    /// ```
    pub fn gate(
        mut self,
        name: &str,
        check: impl Fn(&User) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.gates.insert(name.to_owned(), Arc::new(check));
        self
    }

    /// A gate that may query the database, e.g. whether the user belongs to
    /// a team. Check it in handlers with `auth.gate_async(name).await?`;
    /// templates can't wait for it (`can()` denies it), so pass its answer in
    /// the view's context.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().gate_async("billing", |user, state| async move {
    ///     let n: i64 = renox::db::sql("SELECT COUNT(*) FROM team_admins WHERE user_id = ?")
    ///         .bind(user.id)
    ///         .scalar(&state.db)
    ///         .await?;
    ///     Ok(n > 0)
    /// })
    /// // in a handler: auth.gate_async("billing").await?;
    /// # ;
    /// ```
    /// Asked before every gate, permission and policy check: `Some(true)`
    /// allows, `Some(false)` denies, `None` goes on to the check itself.
    /// Typically lets super-admins do everything.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().gate_before(|user, _ability| {
    ///     (user.get::<String>("role").as_deref() == Some("owner")).then_some(true)
    /// })
    /// # ;
    /// ```
    pub fn gate_before(
        mut self,
        check: impl Fn(&User, &str) -> Option<bool> + Send + Sync + 'static,
    ) -> Self {
        self.gate_before = Some(Arc::new(check));
        self
    }

    /// Defines the gate `name` with a check that can await, e.g. to query
    /// the database. `can(name)` and `require_gate` ask it like any gate.
    pub fn gate_async<F, Fut>(mut self, name: &str, check: F) -> Self
    where
        F: Fn(User, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        self.async_gates.insert(
            name.to_owned(),
            Arc::new(move |user, state| Box::pin(check(user, state))),
        );
        self
    }

    /// Receives `W`'s webhooks: see `renox::webhook`. Also add the route
    /// with `Routes::webhook::<W>(path)`.
    pub fn webhook<W: crate::webhook::Webhook>(mut self) -> Self {
        self.registry.webhook::<W>();
        self
    }

    /// Registers a model for `db:check`, which compares its fields with the
    /// table. Registering it twice keeps one.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64 }
    /// let app = App::new().model::<Product>();
    /// ```
    pub fn model<M: crate::db::Model>(mut self) -> Self {
        self.registry.model::<M>();
        self
    }

    /// Registers a page struct for `view:check`, which then reports the variables its
    /// template uses that the struct lacks; see [`crate::ViewData`]. Registering it twice
    /// keeps one.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// #[derive(serde::Serialize, ViewData)]
    /// #[view(template = "suppliers/index.html")]
    /// struct SupplierList { suppliers: Vec<String> }
    ///
    /// let app = App::new().view_data::<SupplierList>();
    /// ```
    pub fn view_data<T: crate::ViewData>(mut self) -> Self {
        self.registry.view_data::<T>();
        self
    }

    /// Registers the live component `C` (see `renox::live_component`).
    pub fn live_component<C: crate::live_component::LiveComponent>(mut self) -> Self {
        self.registry.live_component::<C>();
        self
    }

    /// Lets queue workers run jobs of type `J`.
    pub fn job<J: Job>(mut self) -> Self {
        self.registry.job::<J>();
        self
    }

    /// Runs `listener` whenever an `E` is emitted with `state.emit(..)`.
    pub fn listen<E, F, Fut>(mut self, listener: F) -> Self
    where
        E: Event,
        F: Fn(E, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.registry.listen(listener);
        self
    }

    /// Declares a group of settings kept in the database; see
    /// [`crate::settings`] and [`Registry::settings`]. Needs
    /// `App::module(Settings)`.
    pub fn settings<T: crate::settings::SettingsGroup>(mut self) -> Self {
        self.registry.settings::<T>();
        self
    }

    /// Declares a group of settings like [`App::settings`] and gives every
    /// view its values as `settings.<key>` (`settings.company.name`); see
    /// [`Registry::settings_in_views`].
    pub fn settings_in_views<T: crate::settings::SettingsGroup>(mut self) -> Self {
        self.registry.settings_in_views::<T>();
        self
    }

    /// Seeds declared settings groups from a JSON file of groups
    /// (`{"company": {"name": "Acme"}}`) the first time each is read: a
    /// deploy tool drops the file, and the app starts with those values.
    /// Values already in the table win, and a missing file seeds nothing.
    /// The seeded values are validated like any save.
    pub fn settings_from(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.settings_seed = Some(path.into());
        self
    }

    /// Adds a command the app binary runs: `my-app <name> [args]`, e.g. to
    /// create the first admin or run an import. See [`crate::command`].
    pub fn command<F, Fut>(mut self, name: &str, about: &str, run: F) -> Self
    where
        F: Fn(crate::command::Args, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.registry.command(name, about, run);
        self
    }

    /// A command whose arguments are declared with clap; see
    /// [`AppCommand`](crate::command::AppCommand).
    pub fn typed_command<T: crate::command::AppCommand>(mut self) -> Self {
        self.registry.typed_command::<T>();
        self
    }

    /// A named rate limit for `Routes::throttle_by(name)`, whose `rule`
    /// picks the limit for each request (by user, role, IP, API key…); see
    /// [`crate::rate_limit::Limit`].
    pub fn rate_limiter(
        mut self,
        name: &str,
        rule: impl Fn(&crate::rate_limit::LimitRequest) -> crate::rate_limit::Limit
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.limiters
            .insert(name.to_owned(), std::sync::Arc::new(rule));
        self
    }

    /// Sends every error that needs a person (a 500, a job that failed for
    /// good, a failed scheduled task) to `reporter`, e.g. an error tracker;
    /// see [`crate::report`].
    pub fn report<F, Fut>(mut self, reporter: F) -> Self
    where
        F: Fn(crate::report::ErrorReport, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.registry.report(reporter);
        self
    }

    /// Adds a notification channel (WhatsApp, SMS, Slack…); see
    /// [`Registry::channel`].
    pub fn channel<F, Fut>(mut self, name: &str, send: F) -> Self
    where
        F: Fn(crate::auth::Recipient, serde_json::Value, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.registry.channel(name, send);
        self
    }

    /// Caps how fast the app's own queued deliveries to the channel `name`
    /// go out; see [`Registry::channel_rate_limit`]. It is a separate call
    /// (not chained on `channel`), so it works in any order.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use std::time::Duration;
    /// # let _ =
    /// App::new()
    ///     .channel("sms", |_to, _message, _state| async { Ok(()) })
    ///     .channel_rate_limit("sms", 20, Duration::from_secs(1))
    /// # ;
    /// ```
    pub fn channel_rate_limit(mut self, name: &str, max: u32, per: std::time::Duration) -> Self {
        self.registry.channel_rate_limit(name, max, per);
        self
    }

    /// Caps how fast queued mail (`queue_mail`, `queue_mail_via`, queued
    /// notifications) goes out through the mailer `name`: at most `max` per
    /// `per`, shared by every worker. `"default"` is the app's default
    /// mailer. A mail over the limit waits for the next window without
    /// using up an attempt. An unknown mailer is an error when the app boots.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::mail::MailConfig;
    /// # use std::time::Duration;
    /// # let _ =
    /// App::new()
    ///     .mailer("bulk", |config| MailConfig::from_env(config, "BULK"))
    ///     .mailer_rate_limit("bulk", 100, Duration::from_secs(60))
    /// # ;
    /// ```
    pub fn mailer_rate_limit(mut self, name: &str, max: u32, per: std::time::Duration) -> Self {
        self.mailer_limits
            .insert(name.to_owned(), crate::queue::Limit::new(max, per));
        self
    }

    /// Also sends the CSRF token as an `XSRF-TOKEN` cookie that scripts can
    /// read, and accepts it back in an `X-XSRF-TOKEN` header (Laravel's
    /// behaviour), for a JavaScript client on the same site: axios sends
    /// it by itself. Forms and htmx don't need it; they send `_token` or
    /// `X-CSRF-Token`.
    pub fn xsrf_cookie(mut self) -> Self {
        self.xsrf_cookie = true;
        self
    }

    /// Adds a mailer named `name`, e.g. a provider for newsletters or a
    /// second SMTP account, used with `state.mailer_named(name)` or, by
    /// listing it in `MAIL_FAILOVER`, when the default mailer fails.
    /// `settings` runs once the configuration is loaded;
    /// [`MailConfig::from_env`] reads `<PREFIX>_MAILER`, `_HOST`, …
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::mail::MailConfig;
    ///
    /// # let _ =
    /// // BACKUP_HOST=smtp.other-provider.com, BACKUP_USERNAME=…; MAIL_FAILOVER=backup
    /// App::new().mailer("backup", |config| MailConfig::from_env(config, "BACKUP"))
    /// # ;
    /// ```
    ///
    /// [`MailConfig::from_env`]: crate::mail::MailConfig::from_env
    pub fn mailer(
        mut self,
        name: &str,
        settings: impl Fn(&Config) -> Result<crate::mail::MailConfig> + Send + Sync + 'static,
    ) -> Self {
        self.mailers.push((name.to_owned(), Box::new(settings)));
        self
    }

    /// Adds a storage disk named `name` (letters, digits, `-`, `_`), e.g.
    /// backups on another bucket, read with `state.disk_named(name)`. `settings`
    /// runs once the configuration is loaded; [`StorageConfig::from_env`]
    /// reads `<PREFIX>_DISK`, `<PREFIX>_BUCKET`, … A local disk keeps its
    /// files in `STORAGE_PATH/<name>` unless its settings say otherwise.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::storage::StorageConfig;
    ///
    /// # let _ =
    /// App::new()
    ///     .disk("backups", |config| StorageConfig::from_env(config, "BACKUPS"))
    ///     .disk("exports", |_| Ok(StorageConfig::default())) // local: storage/exports
    /// # ;
    /// ```
    ///
    /// [`StorageConfig::from_env`]: crate::storage::StorageConfig::from_env
    pub fn disk(
        mut self,
        name: &str,
        settings: impl Fn(&Config) -> Result<crate::storage::StorageConfig> + Send + Sync + 'static,
    ) -> Self {
        self.disks.push((name.to_owned(), Box::new(settings)));
        self
    }

    /// The steps every uploaded file passes after the form's rules and
    /// before the handler sees it: a malware scan, image processing, your own
    /// [`UploadStep`](crate::upload::UploadStep)s. See [`crate::upload`].
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::upload::{ClamAv, Pipeline};
    ///
    /// # let _ =
    /// App::new().uploads(Pipeline::new().step(ClamAv::tcp("127.0.0.1:3310")))
    /// # ;
    /// ```
    pub fn uploads(mut self, pipeline: crate::upload::Pipeline) -> Self {
        self.uploads = pipeline;
        self
    }

    /// Makes `value` available everywhere the app runs: `Provided<T>` in
    /// handlers, `state.provided::<T>()` in jobs, listeners, commands and
    /// scheduled tasks. One value per type; a second one replaces the first,
    /// and it wins over a module's ([`Registry::provide`]). See
    /// [`crate::Provided`].
    pub fn provide<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.provided
            .insert(std::any::TypeId::of::<T>(), Arc::new(value));
        self
    }

    /// Gives every view a value computed per request; see [`Registry::share`].
    pub fn share<F, Fut, T>(mut self, key: &str, compute: F) -> Self
    where
        F: Fn(crate::view::ViewContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
        T: serde::Serialize,
    {
        self.registry.share(key, compute);
        self
    }

    /// Adds `link` to the navigation menu `menu`; see [`Registry::nav`].
    pub fn nav(mut self, menu: &str, link: crate::nav::NavLink) -> Self {
        self.registry.nav(menu, link);
        self
    }

    /// Hides the link with `key` from the menu `menu`; see [`Registry::nav_hide`].
    pub fn nav_hide(mut self, menu: &str, key: &str) -> Self {
        self.registry.nav_hide(menu, key);
        self
    }

    /// Adds template functions, filters or globals; see [`Registry::templates`].
    pub fn templates(
        mut self,
        hook: impl Fn(&mut minijinja::Environment<'static>) + Send + Sync + 'static,
    ) -> Self {
        self.registry.templates(hook);
        self
    }

    /// Defines scheduled tasks.
    pub fn schedule(mut self, define: impl FnOnce(&mut Schedule)) -> Self {
        define(self.registry.schedule());
        self
    }

    /// Connects to the database and builds the router, without serving.
    pub async fn boot(mut self) -> Result<Kernel> {
        let config = match self.config {
            Some(config) => config,
            None => Config::load()?,
        };

        self.registry.job::<crate::mail::SendMail>();
        self.registry.job::<crate::mail::SendMailVia>();
        self.registry
            .job::<crate::auth::notifications::SendToChannel>();
        self.registry
            .job::<crate::auth::passwordless::SendLoginSecret>();
        self.registry.job::<crate::webhook::ProcessWebhook>();
        #[cfg(feature = "server-events")]
        self.registry.job::<crate::analytics::ServerEvent>();
        let mut migrations = crate::queue::MIGRATIONS.to_vec();
        migrations.push(crate::cache::MIGRATION);
        migrations.push(crate::session::MIGRATION);
        migrations.push(crate::grid::MIGRATION);
        migrations.push(crate::import::MIGRATION);
        migrations.extend(crate::webhook::MIGRATIONS);
        migrations.push(crate::db::SEQUENCE_MIGRATION);
        migrations.push(crate::state_machine::MIGRATION);
        migrations.extend(self.migrations);
        for module in &self.modules {
            migrations.extend_from_slice(module.migrations());
            module.register(&mut self.registry);
        }
        let Registry {
            jobs,
            listeners,
            schedule,
            duplicate_job,
            webhooks,
            live_components,
            duplicate_live,
            commands,
            templates,
            components,
            mut shares,
            channels,
            channel_limits,
            reporters,
            permissions,
            second_factor,
            duplicate_second_factor,
            mut account_sections,
            auth,
            assets: static_assets,
            provided: mut module_provided,
            seeders,
            models,
            view_data,
            settings_groups,
            settings_module,
            nav_links,
            nav_hidden,
        } = self.registry;
        if !settings_groups.is_empty() && !settings_module {
            return Err(anyhow!(
                "settings groups are declared (`App::settings`), but the `Settings` module isn't \
                 on: add `.module(renox::settings::Settings)`"
            )
            .into());
        }
        let mut seen = std::collections::HashSet::new();
        for group in &settings_groups {
            if !seen.insert(group.key) {
                return Err(anyhow!("two settings groups have the key `{}`", group.key).into());
            }
        }
        let settings_seed = match &self.settings_seed {
            Some(path) => crate::settings::read_seed(path)?,
            None => None,
        };
        if settings_groups.iter().any(|group| group.shared) {
            shares.push((
                "settings".to_owned(),
                crate::view::share_fn(|ctx: crate::view::ViewContext| async move {
                    ctx.state.settings_for_views().await
                }),
            ));
        }
        // The app's own `App::provide` values win over the modules'.
        module_provided.extend(self.provided);
        check_assets(&static_assets)?;
        let static_assets: Arc<[crate::registry::StaticAsset]> = static_assets.into();
        if let Some(name) = duplicate_job {
            return Err(anyhow!("job `{name}` is registered twice").into());
        }
        if let Some(name) = duplicate_live {
            return Err(anyhow!("live component `{name}` is registered twice").into());
        }
        if duplicate_second_factor {
            return Err(
                anyhow!("two modules set a second login step (`Registry::second_factor`)").into(),
            );
        }
        let nav = crate::nav::Nav::build(nav_links, nav_hidden)?;
        check_commands(&commands)?;
        schedule.check()?;
        let zone: Zone = config.timezone;

        let migrator = Migrator::new(migrations)?;
        let key = match &config.key {
            Some(key) => parse_key(key)?,
            None => parse_key(&crate::generate_key())?,
        };
        // `Encrypted` model fields are sealed and opened with APP_KEY.
        let db = crate::db::connect(&config).await?.with_key(key.clone());

        let mut router = Router::new();
        let mut fallback: Option<axum::routing::MethodRouter<AppState>> = None;
        // One router per `Routes::domain` pattern, with its own fallback.
        let mut domains: Vec<(
            crate::domain::DomainPattern,
            Router<AppState>,
            Option<axum::routing::MethodRouter<AppState>>,
        )> = Vec::new();
        let mut routes = RouteTable::default();
        let mut listing: Vec<RouteInfo> = Vec::new();
        for module in &self.modules {
            tracing::debug!(module = module.name(), "registering module");
            let parts = module.routes().into_parts();
            check_clashes(&listing, &parts.listing, None, module.name())?;
            router = merge_routes(router, parts.router, module.name())?;
            for (name, path) in parts.names {
                routes.insert(name, path)?;
            }
            listing.extend(parts.listing.into_iter().map(|info| RouteInfo {
                module: module.name().to_owned(),
                ..info
            }));
            if let Some(handler) = parts.fallback {
                if fallback.is_some() {
                    return Err(anyhow!(
                        "two modules set a fallback route (`{}` is the second)",
                        module.name()
                    )
                    .into());
                }
                fallback = Some(handler);
            }
            for (text, domain_routes) in parts.domains {
                let pattern = crate::domain::DomainPattern::parse(&text)?;
                let inner = domain_routes.into_parts();
                if !inner.domains.is_empty() {
                    return Err(anyhow!(
                        "Routes::domain(\"{text}\", …) inside another domain (module `{}`)",
                        module.name()
                    )
                    .into());
                }
                check_clashes(
                    &listing,
                    &inner.listing,
                    Some(pattern.as_str()),
                    module.name(),
                )?;
                let at = match domains.iter().position(|(p, _, _)| *p == pattern) {
                    Some(at) => at,
                    None => {
                        domains.push((pattern.clone(), Router::new(), None));
                        domains.len() - 1
                    }
                };
                let (_, domain_router, domain_fallback) = &mut domains[at];
                *domain_router =
                    merge_routes(std::mem::take(domain_router), inner.router, module.name())?;
                if let Some(handler) = inner.fallback {
                    if domain_fallback.is_some() {
                        return Err(anyhow!(
                            "two fallbacks for the domain `{text}` (module `{}`)",
                            module.name()
                        )
                        .into());
                    }
                    *domain_fallback = Some(handler);
                }
                for (name, path) in inner.names {
                    routes.set_domain(&name, pattern.as_str());
                    routes.insert(name, path)?;
                }
                listing.extend(inner.listing.into_iter().map(|info| RouteInfo {
                    module: module.name().to_owned(),
                    domain: Some(pattern.as_str().to_owned()),
                    ..info
                }));
            }
        }
        // The app's own layers; the first one added ends up outermost.
        for layer in self.layers.iter().rev() {
            router = layer(router);
            for (_, domain_router, _) in &mut domains {
                *domain_router = layer(std::mem::take(domain_router));
            }
        }
        listing.extend(framework_routes(&config));
        listing.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
        // The current route's name follows the request's method (#299).
        for info in &listing {
            if let Some(name) = &info.name {
                routes.set_method(name, &info.method);
            }
        }
        // `throttle_by("name")` needs `App::rate_limiter("name", …)`.
        for route in &listing {
            for mark in &route.middleware {
                if let Some(name) = mark.strip_prefix("throttle:")
                    && !name.contains('/')
                    && !self.limiters.contains_key(name)
                {
                    return Err(anyhow!(
                        "{} {} uses throttle_by(\"{name}\"), but there's no App::rate_limiter(\"{name}\", …)",
                        route.method,
                        route.path
                    )
                    .into());
                }
            }
        }
        for route in &listing {
            for provider in route
                .middleware
                .iter()
                .filter_map(|m| m.strip_prefix("webhook:"))
            {
                if !webhooks.contains_key(provider) {
                    return Err(anyhow!(
                        "the route {} receives `{provider}` webhooks, but they're not registered: \
                         add `app.webhook::<…>()` in the module's `register`",
                        route.path
                    )
                    .into());
                }
            }
        }
        let routes = Arc::new(routes);

        let storage = crate::storage::Storage::from_config(&config)?.with_key(key.clone());
        let mut disks = HashMap::new();
        for (name, settings) in &self.disks {
            let valid = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !valid {
                return Err(anyhow!(
                    "disk name `{name}`: use letters, digits, `-` and `_` (App::disk)"
                )
                .into());
            }
            let disk = crate::storage::Storage::named(&config, name, &settings(&config)?)?
                .with_key(key.clone());
            if disks.insert(name.clone(), disk).is_some() {
                return Err(anyhow!("two disks are named `{name}` (App::disk)").into());
            }
        }
        // Release builds serve what was compiled in; debug builds read the disk.
        let embedded = self.embedded.filter(|_| !config.debug);
        // Rate limits and login locks shared by every server (CACHE_STORE=database).
        let shared_counters =
            (config.cache_store == crate::CacheStore::Database).then(|| db.clone());
        let versions = Arc::new(crate::embedded::AssetVersions::new(
            &config.public_path,
            embedded.map(|e| e.public),
        ));
        let views = Views::new(
            &config,
            routes.clone(),
            storage.clone(),
            embedded.map(|e| e.views),
            Arc::new(templates),
            Arc::new(components.iter().map(|c| c.contract()).collect()),
            zone,
            versions,
        )
        .with_typed(
            view_data,
            shares.iter().map(|(key, _)| key.clone()).collect(),
        );
        let mut security =
            crate::security::Security::new(&config, &self.csp, &self.permissions_policy, &listing)?;
        security.xsrf_cookie = self.xsrf_cookie;
        let security = Arc::new(security);
        let mut mailers = HashMap::new();
        for (name, settings) in &self.mailers {
            let mailer = Mailer::open(&settings(&config)?, &config)?;
            if mailers.insert(name.clone(), mailer).is_some() {
                return Err(anyhow!("two mailers are named `{name}` (App::mailer)").into());
            }
        }
        for name in self.mailer_limits.keys() {
            if name != "default" && !mailers.contains_key(name) {
                return Err(anyhow!(
                    "App::mailer_rate_limit names `{name}`, but no mailer has that name: add it with App::mailer"
                )
                .into());
            }
        }
        for name in channel_limits.keys() {
            if !channels.contains_key(name) {
                return Err(anyhow!(
                    "App::channel_rate_limit names `{name}`, but no channel has that name: add it with App::channel"
                )
                .into());
            }
        }
        let mut failover = Vec::new();
        for name in &config.mail.failover {
            let mailer = mailers.get(name).ok_or_else(|| {
                anyhow!("MAIL_FAILOVER names `{name}`, but no mailer has that name: add it with App::mailer")
            })?;
            failover.push((name.clone(), mailer.clone()));
        }
        let mailer = Mailer::from_config(&config)?.with_failover(failover);
        let state = AppState::new(crate::state::AppStateInner {
            security,
            webhooks: Arc::new(webhooks),
            live_components: Arc::new(live_components),
            mailer,
            mailers: Arc::new(mailers),
            themes: crate::theme::ThemeRegistry::default(),
            channel_limits: Arc::new(channel_limits),
            mailer_limits: Arc::new(self.mailer_limits),
            queue: Queue::new(db.clone(), key.clone()),
            cache: crate::cache::Cache::new(config.cache_store, db.clone())?,
            storage,
            disks: Arc::new(disks),
            uploads: self.uploads.clone(),
            http: crate::http::Http::default(),
            fakes: Arc::default(),
            settings_store: Arc::new(crate::settings::Store::new(settings_groups, settings_seed)),
            translator: Arc::new(match embedded {
                Some(files) => crate::i18n::Translator::embedded(files.lang)?,
                None => crate::i18n::Translator::load(&config.lang_path, config.debug)?,
            }),
            // Tests read database sessions synchronously (`TestApp::session_get`).
            session_mirror: (config.session_driver == crate::SessionDriver::Database
                && config.env == Environment::Testing)
                .then(Default::default),
            live: (config.debug && config.env == Environment::Local).then(|| {
                crate::live::Live::start(vec![
                    config.views_path.clone(),
                    config.public_path.clone(),
                    config.lang_path.clone(),
                ])
            }),
            notification_hub: Arc::new(crate::auth::notifications::Hub::new()),
            listeners: Arc::new(listeners),
            inspector: (config.debug && config.env == Environment::Local)
                .then(|| Arc::new(crate::inspector::Inspector::default())),
            config: Arc::new(config),
            routes,
            views,
            db,
            key,
            gates: Arc::new(crate::auth::Access {
                gates: self.gates,
                before: self.gate_before,
                permissions,
            }),
            async_gates: Arc::new(self.async_gates),
            shares: Arc::new(shares),
            channels: Arc::new(channels),
            reporters: Arc::new(reporters),
            second_factor: second_factor.map(Arc::new),
            nav: Arc::new(nav),
            account_sections: {
                account_sections.sort_by_key(|section| section.order);
                Arc::new(account_sections)
            },
            auth,
            limiters: Arc::new(
                self.limiters
                    .into_iter()
                    .map(|(name, rule)| (name, crate::rate_limit::NamedLimiter::new(rule)))
                    .collect(),
            ),
            provided: Arc::new(module_provided),
            throttle: Arc::new(LoginThrottle::new(shared_counters.clone())),
            detect_locale: self.detect_locale,
            rtl_locales: self.rtl_locales.map(Arc::new),
        });
        if let Some(second) = &state.second_factor
            && state.url(&second.challenge, &[]).is_err()
        {
            return Err(anyhow!(
                "the second login step's challenge route `{}` doesn't exist",
                second.challenge
            )
            .into());
        }

        let public = embedded.map(|e| e.public);
        let default = build_router(router, state.clone(), public, fallback, &static_assets);
        let router = if domains.is_empty() {
            default
        } else {
            let hosts = domains
                .into_iter()
                .map(|(pattern, router, fallback)| {
                    (
                        pattern,
                        build_router(router, state.clone(), public, fallback, &static_assets),
                    )
                })
                .collect();
            crate::domain::dispatch(hosts, default)
        };
        Ok(Kernel {
            listing,
            router,
            state,
            migrator,
            seeders,
            models,
            handlers: Arc::new(jobs),
            schedule,
            zone,
            commands,
        })
    }

    /// Builds the router without starting a server, e.g. for tests.
    pub async fn into_router(self) -> Result<Router> {
        Ok(self.boot().await?.router)
    }

    /// Runs the command given on the command line (`serve` by default) on a
    /// new Tokio runtime.
    pub fn run(self) -> Result {
        let args: Vec<String> = std::env::args().skip(1).collect();
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(self.run_command(&args))
    }

    /// Runs one command as the app binary would, e.g. from a test or a
    /// program that drives the app: `["migrate:status"]`, `["down",
    /// "--secret", "abc"]`, or `[]` for `serve`. Output goes to stdout.
    ///
    /// ```no_run
    /// # use renox::prelude::*;
    /// # async fn demo() -> Result {
    /// App::new().run_args(["migrate"]).await?;
    /// # Ok(()) }
    /// ```
    pub async fn run_args(self, args: impl IntoIterator<Item = impl Into<String>>) -> Result {
        let args: Vec<String> = args.into_iter().map(Into::into).collect();
        self.run_command(&args).await
    }

    /// Starts the server on the current Tokio runtime.
    pub async fn serve(self) -> Result {
        self.run_command(&[]).await
    }

    async fn run_command(self, args: &[String]) -> Result {
        let command = args.first().map(String::as_str).unwrap_or("serve");
        if matches!(command, "help" | "--help" | "-h") {
            // Modules add their commands in `register`, which boot runs.
            let mut modules = Registry::default();
            for module in &self.modules {
                module.register(&mut modules);
            }
            let mut commands = self.registry.commands.clone();
            commands.extend(modules.commands);
            println!("{USAGE}{}", app_commands_help(&commands));
            return Ok(());
        }

        let config = match self.config.clone() {
            Some(config) => config,
            None => Config::load()?,
        };
        let long_running = matches!(command, "serve" | "queue:work" | "schedule:work");
        init_tracing(&config, long_running);
        if command == "serve" && config.key.is_none() && config.env != Environment::Testing {
            tracing::warn!(
                "APP_KEY is not set; using a temporary key, so sessions end on restart. \
                 Run `rnx key:generate`."
            );
        }
        let kernel = App {
            config: Some(config),
            ..self
        }
        .boot()
        .await?;

        match command {
            "serve" => kernel.serve().await?,
            "migrate" => print_done("Migrated", &kernel.migrate().await?),
            "migrate:rollback" => {
                let steps = flag_value(args, "--step")?.unwrap_or(1);
                print_done("Rolled back", &kernel.rollback(steps).await?);
            }
            "migrate:fresh" => {
                println!("Dropped all tables.");
                print_done("Migrated", &kernel.fresh().await?);
                if args.iter().any(|a| a == "--seed") {
                    kernel.seed().await?;
                    println!("Seeded.");
                }
            }
            "db:diff" if !kernel.commands.iter().any(|c| c.name == "db:diff") => {
                crate::db::auto_migration::run(&kernel, &args[1..]).await?
            }
            "db:check" if !kernel.commands.iter().any(|c| c.name == "db:check") => {
                kernel.db_check().await?
            }
            "view:check" if !kernel.commands.iter().any(|c| c.name == "view:check") => {
                let problems = kernel.state.views.check(&kernel.state.routes);
                for problem in &problems {
                    println!("{problem}");
                }
                let checked = kernel.state.views.names().len();
                println!("Checked {checked} templates: {} problems.", problems.len());
                if !problems.is_empty() {
                    return Err(anyhow!("{} template problem(s)", problems.len()).into());
                }
            }
            "view:data" if !kernel.commands.iter().any(|c| c.name == "view:data") => {
                let path = std::path::Path::new(
                    flag_text(args, "--out").unwrap_or(".vscode/renox-components.json"),
                );
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)?;
                }
                let mut text = serde_json::to_string_pretty(&kernel.state.views.custom_data())
                    .map_err(|e| anyhow!(e))?;
                text.push('\n');
                std::fs::write(path, text)?;
                println!("Wrote {}.", path.display());
            }
            "migrate:status" => {
                for m in kernel.migration_status().await? {
                    let note = if m.missing {
                        "  (applied, but its file is gone)"
                    } else if m.changed {
                        "  (edited after it ran; the edit won't run)"
                    } else {
                        ""
                    };
                    match m.batch {
                        Some(batch) => println!("  ran (batch {batch})  {}{note}", m.name),
                        None => println!("  pending          {}", m.name),
                    }
                }
            }
            "db:seed" => {
                kernel.seed().await?;
                println!("Seeded.");
            }
            "queue:work" => {
                let queues: Vec<String> = flag_text(args, "--queue")
                    .map(|q| q.split(',').map(|s| s.trim().to_owned()).collect())
                    .unwrap_or_default();
                let worker = kernel.worker(queues);
                if args.iter().any(|a| a == "--once") {
                    println!("Ran {} job(s).", worker.drain().await?);
                } else {
                    let workers = flag_value(args, "--workers")?.unwrap_or(1) as usize;
                    let (stop, stopped) = watch::channel(false);
                    let running = tokio::spawn(worker.run(workers, stopped));
                    shutdown_signal().await;
                    let _ = stop.send(true);
                    let _ = running.await;
                }
            }
            "webhook:failed" => {
                let failed = crate::webhook::WebhookCall::failed(&kernel.state.db).await?;
                if failed.is_empty() {
                    println!("No failed webhook calls.");
                }
                for call in failed {
                    println!(
                        "  #{} {} {}: {}",
                        call.id,
                        call.provider,
                        call.event_id,
                        call.error
                            .unwrap_or_default()
                            .lines()
                            .next()
                            .unwrap_or_default()
                    );
                }
            }
            "webhook:retry" => {
                let id: i64 = args
                    .get(1)
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(|| anyhow!("usage: webhook:retry <id>"))?;
                if crate::webhook::retry(&kernel.state, id).await? {
                    println!("Webhook call #{id} queued again.");
                } else {
                    return Err(anyhow!("there is no webhook call #{id}").into());
                }
            }
            "queue:failed" => {
                let failed = kernel.state.queue.failed().await?;
                if failed.is_empty() {
                    println!("No failed jobs.");
                }
                for job in failed {
                    println!("  #{} {} ({}): {}", job.id, job.job, job.queue, job.error);
                }
            }
            "queue:retry" => {
                let id = match args.get(1).map(String::as_str) {
                    Some("all") => None,
                    Some(id) => Some(
                        id.parse()
                            .map_err(|_| anyhow!("expected a job id or `all`"))?,
                    ),
                    None => return Err(anyhow!("usage: queue:retry <id|all>").into()),
                };
                let queued = match id {
                    Some(id) => u64::from(kernel.state.queue.retry(id).await?),
                    None => kernel.state.queue.retry_all().await?,
                };
                println!("Queued {queued} job(s) again.");
            }
            "queue:forget" => {
                let id: i64 = args
                    .get(1)
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(|| anyhow!("usage: queue:forget <id>"))?;
                if kernel.state.queue.forget_failed(id).await? {
                    println!("Deleted failed job {id}.");
                } else {
                    return Err(anyhow!("no failed job {id}").into());
                }
            }
            "queue:prune-failed" => {
                let hours = flag_value(args, "--hours")?.unwrap_or(168);
                let age = Duration::from_secs(u64::from(hours) * 3600);
                println!(
                    "Deleted {} failed job(s) older than {hours} hour(s).",
                    kernel.state.queue.prune_failed(age).await?
                );
            }
            "queue:prune-batches" => {
                let hours = flag_value(args, "--hours")?.unwrap_or(24);
                let age = Duration::from_secs(u64::from(hours) * 3600);
                println!(
                    "Deleted {} batch(es) finished more than {hours} hour(s) ago.",
                    kernel.state.queue.prune_batches(age).await?
                );
            }
            "queue:flush" => println!(
                "Deleted {} failed job(s).",
                kernel.state.queue.flush_failed().await?
            ),
            "ui:publish" => {
                let force = args.iter().any(|a| a == "--force");
                crate::assets::publish_ui(&kernel.state.config, force)?;
            }
            "session:prune" => println!(
                "Deleted {} expired session(s).",
                crate::Session::prune_expired(&kernel.state.db).await?
            ),
            "sequence:list" => {
                let rows = crate::db::Sequence::list(&kernel.state.db).await?;
                if rows.is_empty() {
                    println!("No numbering series.");
                }
                for row in rows {
                    println!(
                        "  {}  scope={:?} period={:?}  last={}",
                        row.series, row.scope, row.period, row.last
                    );
                }
            }
            "cache:prune" => println!(
                "Deleted {} expired cache row(s).",
                kernel.state.cache.prune().await?
            ),
            "import:prune" => {
                let days = flag_value(args, "--days")?.unwrap_or(0);
                let age = Duration::from_secs(u64::from(days) * 86_400);
                println!(
                    "Deleted {} import record(s) whose undo window ended more than {days} day(s) ago.",
                    crate::import::prune(&kernel.state, age).await?
                );
            }
            "schedule:list" => {
                if kernel.schedule.is_empty() {
                    println!("No scheduled tasks.");
                }
                for run in kernel.schedule.upcoming(kernel.zone) {
                    let when = match run.at {
                        Some(at) => run
                            .zone
                            .local(at.timestamp())
                            .format("%Y-%m-%d %H:%M")
                            .to_string(),
                        None => "never".to_owned(),
                    };
                    println!("  {when:<16}  {:<18}  {}", run.zone, run.name);
                }
            }
            "schedule:run" => {
                let Some(name) = args.get(1).filter(|a| !a.starts_with('-')) else {
                    return Err(anyhow!("usage: schedule:run <task>").into());
                };
                kernel.run_scheduled(name).await?;
                println!("Ran `{name}`.");
            }
            "schedule:work" => {
                let (stop, stopped) = watch::channel(false);
                let running = tokio::spawn(kernel.schedule.clone().run(
                    kernel.state.clone(),
                    kernel.zone,
                    stopped,
                ));
                shutdown_signal().await;
                let _ = stop.send(true);
                let _ = running.await;
            }
            "route:list" => print_routes(kernel.routes()),
            "db:shell" => crate::shell::run(kernel.db()).await?,
            "down" => {
                let secret = flag_text(args, "--secret").map(str::to_owned);
                let retry = flag_value(args, "--retry")?.map(u64::from);
                let mut options = crate::maintenance::DownOptions::new();
                if let Some(secret) = &secret {
                    options = options.secret(secret.clone());
                }
                if let Some(retry) = retry {
                    options = options.retry(retry);
                }
                crate::maintenance::down(&kernel.state.config.storage_path, options)?;
                match secret {
                    Some(secret) => println!("The app is down. Visit /{secret} to bypass it."),
                    None => println!("The app is down."),
                }
            }
            "up" => match crate::maintenance::up(&kernel.state.config.storage_path)? {
                true => println!("The app is up."),
                false => println!("The app was not down."),
            },
            other if kernel.commands.iter().any(|c| c.name == other) => {
                kernel.call(other, args[1..].iter().cloned()).await?;
            }
            other => {
                return Err(anyhow!(
                    "unknown command `{other}`\n\n{USAGE}{}",
                    app_commands_help(&kernel.commands)
                )
                .into());
            }
        }
        Ok(())
    }
}

/// The listening socket systemd passes with socket activation
/// (`LISTEN_FDS`, a `.socket` unit): it stays open while the service
/// restarts, so connections wait in its queue instead of being refused.
fn inherited_listener() -> Result<Option<TcpListener>> {
    let mut fds = listenfd::ListenFd::from_env();
    let Some(listener) = fds
        .take_tcp_listener(0)
        .map_err(|err| anyhow!("the socket systemd passed isn't a TCP listener: {err}"))?
    else {
        return Ok(None);
    };
    listener
        .set_nonblocking(true)
        .map_err(anyhow::Error::from)?;
    Ok(Some(
        TcpListener::from_std(listener).map_err(anyhow::Error::from)?,
    ))
}

/// Built-in commands; an app command can't take one of these names.
///
/// The list is the 1.0 set and stays as it is through 1.x (docs/stability.md):
/// refusing a name an app already uses would stop that app from booting. A
/// command added in 1.x must let an app command of the same name run in its
/// place instead of joining this list.
const BUILT_IN_COMMANDS: &[&str] = &[
    "serve",
    "migrate",
    "migrate:rollback",
    "migrate:fresh",
    "migrate:status",
    "db:seed",
    "queue:work",
    "queue:failed",
    "queue:retry",
    "queue:flush",
    "queue:forget",
    "queue:prune-failed",
    "queue:prune-batches",
    "webhook:failed",
    "webhook:retry",
    "sequence:list",
    "cache:prune",
    "session:prune",
    "import:prune",
    "ui:publish",
    "schedule:list",
    "schedule:run",
    "schedule:work",
    "route:list",
    "db:shell",
    "down",
    "up",
    "help",
];

fn check_commands(commands: &[crate::command::Command]) -> Result {
    let mut seen = std::collections::HashSet::new();
    for command in commands {
        let name = command.name.as_str();
        if name.is_empty() || name.starts_with('-') || name.contains(char::is_whitespace) {
            return Err(anyhow!("`{name}` is not a valid command name").into());
        }
        if BUILT_IN_COMMANDS.contains(&name) {
            return Err(anyhow!("the command `{name}` is built in; choose another name").into());
        }
        if !seen.insert(name) {
            return Err(anyhow!("the command `{name}` is registered twice").into());
        }
    }
    Ok(())
}

fn app_commands_help(commands: &[crate::command::Command]) -> String {
    if commands.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nApp commands:\n");
    for command in commands {
        out.push_str(&format!("  {:<26}{}\n", command.name, command.about));
    }
    out.trim_end().to_owned()
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// A booted application: its router, database, queue and maintenance commands.
pub struct Kernel {
    router: Router,
    listing: Vec<RouteInfo>,
    state: AppState,
    migrator: Migrator,
    seeders: Vec<crate::registry::Seeder>,
    models: Vec<crate::db::schema::ModelInfo>,
    handlers: Handlers,
    schedule: Schedule,
    zone: Zone,
    commands: Vec<crate::command::Command>,
}

impl Kernel {
    /// A scratch database with every migration run (reused by #371).
    pub(crate) async fn scratch_db(&self) -> Result<Db> {
        let db = crate::db::connect_scratch(&self.state.config).await?;
        self.migrator.run(&db).await?;
        Ok(db)
    }

    /// `db:check`: compares the models with what the migrations build.
    async fn db_check(&self) -> Result {
        let db = self.scratch_db().await?;
        let reports = crate::db::schema_check::check(&db, &self.models).await;
        crate::db::drop_scratch(&db).await;
        let (text, problems) = crate::db::schema_check::render(&reports?);
        print!("{text}");
        if problems > 0 {
            return Err(anyhow::anyhow!("{problems} problem(s) in the models").into());
        }
        Ok(())
    }

    /// The models registered with `App::model`, once each.
    pub(crate) fn models(&self) -> &[crate::db::schema::ModelInfo] {
        &self.models
    }

    /// Runs the app command `name` (see [`App::command`]), e.g. from a test.
    pub async fn call(
        &self,
        name: &str,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result {
        let command = self
            .commands
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| anyhow!("unknown command `{name}`"))?;
        let run = (command.run)(self.state.clone(), crate::command::Args::new(args));
        crate::context::scope_app(self.state.clone(), run).await
    }

    /// Runs the scheduled task `name` now, with its hooks (what
    /// `schedule:run` does). In a test, `TestApp::run_scheduled` does the same
    /// at the time `TestApp::travel` moved the clock to.
    pub async fn run_scheduled(&self, name: &str) -> Result {
        self.schedule.run_now(self.state.clone(), name).await
    }

    /// The assembled router, e.g. to drive with `tower::ServiceExt::oneshot`.
    pub fn router(&self) -> Router {
        self.router.clone()
    }

    /// Every route, sorted by path, as `route:list` prints them.
    pub fn routes(&self) -> &[RouteInfo] {
        &self.listing
    }

    /// What handlers get as `State<AppState>`.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// The database connection pool.
    pub fn db(&self) -> &Db {
        &self.state.db
    }

    /// The mailer; with `MAIL_MAILER=memory`, `mailer().sent()` lists what was sent.
    pub fn mailer(&self) -> &Mailer {
        &self.state.mailer
    }

    /// A worker for the given queues (all queues when empty).
    pub fn worker(&self, queues: Vec<String>) -> Worker {
        Worker::new(self.state.clone(), self.handlers.clone(), queues)
    }

    /// Runs every job that is available now, e.g. in tests; returns how many ran.
    pub async fn run_jobs(&self) -> Result<usize> {
        self.worker(Vec::new()).drain().await
    }

    /// Serves HTTP with queue workers and the scheduler in the same process,
    /// until Ctrl-C or SIGTERM; then lets running jobs finish.
    pub async fn serve(self) -> Result {
        let config = self.state.config.clone();
        let listener = match inherited_listener()? {
            Some(listener) => {
                tracing::info!("using the socket systemd passed (socket activation)");
                listener
            }
            None => TcpListener::bind(config.addr()).await?,
        };
        tracing::info!(
            "{} listening on http://{}",
            config.name,
            listener.local_addr()?
        );

        let (stop, stopped) = watch::channel(false);
        let mut background = Vec::new();
        if config.queue_workers > 0 {
            tracing::info!(workers = config.queue_workers, "queue workers started");
            let worker = self.worker(Vec::new());
            background.push(tokio::spawn(
                worker.run(config.queue_workers, stopped.clone()),
            ));
        }
        if config.scheduler && !self.schedule.is_empty() {
            tracing::info!("scheduler started");
            background.push(tokio::spawn(self.schedule.clone().run(
                self.state.clone(),
                self.zone,
                stopped,
            )));
        }

        let service = self
            .router
            .into_make_service_with_connect_info::<SocketAddr>();
        let live = self.state.live.clone();
        let notification_hub = self.state.notification_hub.clone();
        axum::serve(listener, service)
            .with_graceful_shutdown(async move {
                shutdown_signal().await;
                // Open live-reload and notification streams would otherwise
                // hold the shutdown.
                if let Some(live) = live {
                    live.stop();
                }
                notification_hub.stop();
            })
            .await?;

        let _ = stop.send(true);
        let finished = async {
            for task in background {
                let _ = task.await;
            }
        };
        if tokio::time::timeout(SHUTDOWN_GRACE, finished)
            .await
            .is_err()
        {
            tracing::warn!("background work was still running after {SHUTDOWN_GRACE:?}");
        }
        tracing::info!("{} stopped", config.name);
        Ok(())
    }

    /// Runs pending migrations; returns their names.
    pub async fn migrate(&self) -> Result<Vec<String>> {
        let done = self.migrator.run(self.db()).await?;
        Ok(done.into_iter().map(str::to_owned).collect())
    }

    /// Undoes the last `batches` batches of migrations; returns their names.
    pub async fn rollback(&self, batches: u32) -> Result<Vec<String>> {
        Ok(self.migrator.rollback(self.db(), batches).await?)
    }

    /// Drops every table and runs all migrations.
    pub async fn fresh(&self) -> Result<Vec<String>> {
        let done = self.migrator.fresh(self.db()).await?;
        Ok(done.into_iter().map(str::to_owned).collect())
    }

    /// Every migration and whether it has run, as `migrate:status` prints them.
    pub async fn migration_status(&self) -> Result<Vec<MigrationStatus>> {
        Ok(self.migrator.status(self.db()).await?)
    }

    /// Runs every seeder in registration order.
    pub async fn seed(&self) -> Result {
        for seeder in &self.seeders {
            // In the app's context, so a seeder can reach `renox::context::app()`
            // (config, `encrypt`, the cache) and model hooks see it too.
            crate::context::scope_app(self.state.clone(), seeder(self.state.clone())).await?;
        }
        Ok(())
    }
}

/// Routes Renox adds itself.
fn framework_routes(config: &Config) -> Vec<RouteInfo> {
    let route = |method: &str, path: &str| RouteInfo {
        method: method.to_owned(),
        path: path.to_owned(),
        name: None,
        module: "renox".to_owned(),
        middleware: Vec::new(),
        domain: None,
    };
    let mut routes = vec![
        route("GET", "/health"),
        route("GET", "/robots.txt"),
        route("GET", "/favicon.ico"),
        route("GET", "/_renox/{asset}"),
        route("GET", "/_renox/theme/{hash}.css"),
        route("GET", "/_renox/files/{*key}"),
        route("POST", "/_renox/grid/{grid}/prefs"),
        route("DELETE", "/_renox/grid/{grid}/prefs"),
        route("POST", "/_renox/live/{component}/{action}"),
        route("GET", "/storage/{*path}"),
    ];
    if config.debug {
        routes.push(route("GET", "/_renox/mail"));
        routes.push(route("GET", "/_renox/mail/{id}"));
    }
    if config.debug && config.env == Environment::Local {
        routes.push(route("GET", "/_renox/live"));
    }
    routes
}

fn print_routes(routes: &[RouteInfo]) {
    print!("{}", route_table(routes));
}

/// `route:list`'s table.
fn route_table(routes: &[RouteInfo]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let with_domains = routes.iter().any(|r| r.domain.is_some());
    let rows: Vec<Vec<String>> = routes
        .iter()
        .map(|r| {
            let mut row = vec![
                r.method.clone(),
                r.path.clone(),
                r.name.clone().unwrap_or_default(),
                r.module.clone(),
                r.middleware
                    .iter()
                    .map(|guard| {
                        if is_role_guard(guard) {
                            format!("{guard}*")
                        } else {
                            guard.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ];
            if with_domains {
                row.insert(0, r.domain.clone().unwrap_or_default());
            }
            row
        })
        .collect();
    let mut header: Vec<String> = ["METHOD", "PATH", "NAME", "MODULE", "MIDDLEWARE"]
        .map(str::to_owned)
        .to_vec();
    if with_domains {
        header.insert(0, "DOMAIN".into());
    }
    let mut widths: Vec<usize> = header.iter().map(String::len).collect();
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    for row in std::iter::once(&header).chain(&rows) {
        let line: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}", width = *width))
            .collect();
        let _ = writeln!(out, "{}", line.join("  ").trim_end());
    }
    if routes
        .iter()
        .flat_map(|r| &r.middleware)
        .any(|g| is_role_guard(g))
    {
        out.push_str(
            "\n* role and permission guards count the user's global roles plus the roles \
             given in the request's active scope (permissions::set_scope).\n",
        );
    }
    out
}

/// A `role:` or `permission:` guard, which counts scoped roles too.
fn is_role_guard(guard: &str) -> bool {
    guard.starts_with("role:") || guard.starts_with("permission:")
}

fn print_done(verb: &str, names: &[String]) {
    if names.is_empty() {
        println!("Nothing to do.");
    }
    for name in names {
        println!("{verb}: {name}");
    }
}

fn flag_text<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

fn flag_value(args: &[String], flag: &str) -> Result<Option<u32>> {
    if !args.iter().any(|a| a == flag) {
        return Ok(None);
    }
    match flag_text(args, flag).and_then(|v| v.parse().ok()) {
        Some(value) => Ok(Some(value)),
        None => Err(anyhow!("{flag} needs a number").into()),
    }
}

/// Turns a handler that panics or runs past `REQUEST_TIMEOUT` into a 500
/// with the error page, instead of a dropped connection or a hung client.
async fn guard(
    limit: Option<Duration>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let run = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(next.run(req)));
    let outcome = match limit {
        Some(limit) => match tokio::time::timeout(limit, run).await {
            Ok(outcome) => outcome,
            Err(_) => {
                return Error::Internal(anyhow!(
                    "the request took longer than REQUEST_TIMEOUT ({}s)",
                    limit.as_secs()
                ))
                .into_response();
            }
        },
        None => run.await,
    };
    outcome.unwrap_or_else(|panic| {
        let message = crate::error::panic_message(&*panic);
        Error::Internal(anyhow!("the handler panicked: {message}")).into_response()
    })
}

/// How much longer than `REQUEST_TIMEOUT` the outer limit (`backstop`) waits,
/// so a slow handler still gets the inner one's error page.
const BACKSTOP_GRACE: Duration = Duration::from_secs(1);

/// `REQUEST_TIMEOUT` around the session, i18n, auth, CSRF and view layers:
/// `guard` only covers the handler, so a hang before it (database sessions
/// on a database that stopped answering) would otherwise go unbounded.
/// Answers a plain 500 a little after the handler's own limit.
async fn backstop(
    limit: Option<Duration>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(limit) = limit else {
        return next.run(req).await;
    };
    match tokio::time::timeout(limit + BACKSTOP_GRACE, next.run(req)).await {
        Ok(res) => res,
        Err(_) => Error::Internal(anyhow!(
            "the request took longer than REQUEST_TIMEOUT ({}s) before reaching its handler",
            limit.as_secs()
        ))
        .into_response(),
    }
}

/// Adds a module's routes; what axum refuses to merge is a boot error.
fn merge_routes(
    router: Router<AppState>,
    other: Router<AppState>,
    module: &str,
) -> Result<Router<AppState>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| router.merge(other))).map_err(
        |panic| {
            let message = crate::error::panic_message(&*panic);
            anyhow!("the routes of the `{module}` module clash: {message}").into()
        },
    )
}

/// A route of `module` that another module defines already (on the same
/// domain, or both without one).
fn check_clashes(
    listing: &[RouteInfo],
    new: &[RouteInfo],
    domain: Option<&str>,
    module: &str,
) -> Result {
    for info in new {
        let clash = listing.iter().find(|other| {
            other.domain.as_deref() == domain
                && other.path == info.path
                && (other.method == info.method || other.method == "*" || info.method == "*")
        });
        if let Some(other) = clash {
            let on = domain.map(|d| format!(" on {d}")).unwrap_or_default();
            return Err(anyhow!(
                "{} {}{on} is defined by both the `{}` and the `{module}` module",
                info.method,
                info.path,
                other.module,
            )
            .into());
        }
    }
    Ok(())
}

/// `Registry::asset` paths: each starts with `/` and is used once.
fn check_assets(assets: &[crate::registry::StaticAsset]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for asset in assets {
        if !asset.path.starts_with('/') || asset.path.contains(['{', '}']) {
            return Err(anyhow!(
                "Registry::asset(\"{}\", …): the path must start with `/` and have no `{{…}}` parameters",
                asset.path
            )
            .into());
        }
        if !seen.insert(asset.path) {
            return Err(anyhow!(
                "two modules serve a file at `{}` (Registry::asset)",
                asset.path
            )
            .into());
        }
    }
    Ok(())
}

fn build_router(
    router: Router<AppState>,
    state: AppState,
    embedded_public: Option<&'static [(&'static str, &'static [u8])]>,
    fallback: Option<axum::routing::MethodRouter<AppState>>,
    static_assets: &[crate::registry::StaticAsset],
) -> Router {
    // No route and no public file: the app's fallback (`Routes::fallback`), else a 404.
    let fallback = fallback.map(|handler| handler.with_state(state.clone()));
    let not_found = move |req: axum::extract::Request| {
        let fallback = fallback.clone();
        async move {
            match fallback {
                Some(handler) => match tower::ServiceExt::oneshot(handler, req).await {
                    Ok(res) => res,
                    Err(never) => match never {},
                },
                None => axum::response::IntoResponse::into_response(Error::NotFound),
            }
        }
    };
    let router = router
        .merge(crate::storage::router())
        .merge(crate::grid::router())
        .merge(crate::live_component::route::router());
    let router = if state.config.debug {
        router
            .merge(crate::mail::preview_router())
            .merge(crate::inspector::router())
    } else {
        router
    };
    let public = state.config.public_path.clone();
    let router = if let Some(files) = embedded_public {
        let files = crate::embedded::public_map(files);
        let not_found = not_found.clone();
        router.fallback(move |req: axum::extract::Request| {
            let (files, not_found) = (files.clone(), not_found.clone());
            async move {
                let res = crate::embedded::serve(&files, req.uri());
                if res.status() == axum::http::StatusCode::NOT_FOUND {
                    not_found(req).await
                } else {
                    res
                }
            }
        })
    } else if public.is_dir() {
        // `fallback`, not `not_found_service`: the latter makes every answer
        // a 404, and an app's fallback may redirect or answer 200.
        let files = ServeDir::new(public).fallback(not_found.clone().into_service());
        router.fallback(move |req: axum::extract::Request| {
            let files = files.clone();
            async move {
                // A name no file system accepts (over 255 bytes) can't be a
                // file here; asking the OS would be a "name too long" 500.
                if req
                    .uri()
                    .path()
                    .split('/')
                    .any(|segment| segment.len() > 255)
                {
                    return axum::response::IntoResponse::into_response(Error::NotFound);
                }
                let versioned = crate::embedded::is_versioned(req.uri());
                match tower::ServiceExt::oneshot(files, req).await {
                    Ok(res) => {
                        let mut res = axum::response::IntoResponse::into_response(res);
                        if versioned && res.status().is_success() {
                            res.headers_mut().insert(
                                axum::http::header::CACHE_CONTROL,
                                axum::http::HeaderValue::from_static(
                                    "public, max-age=31536000, immutable",
                                ),
                            );
                        }
                        res
                    }
                    Err(never) => match never {},
                }
            }
        })
    } else {
        router.fallback(not_found)
    };

    let request_timeout = state.config.request_timeout;
    let router: Router = router
        .layer(from_fn(
            move |req: axum::extract::Request, next: axum::middleware::Next| {
                guard(request_timeout, req, next)
            },
        ))
        .layer(from_fn_with_state(
            state.clone(),
            crate::maintenance::middleware,
        ))
        .layer(from_fn_with_state(state.clone(), view::middleware))
        .layer(from_fn(csrf::middleware))
        .layer(from_fn_with_state(state.clone(), auth::middleware))
        .layer(from_fn_with_state(state.clone(), crate::i18n::middleware))
        .layer(from_fn_with_state(state.clone(), session::middleware))
        // Each request's own `renox::context`, around everything the app runs.
        .layer(from_fn_with_state(
            state.clone(),
            crate::context::middleware,
        ))
        .layer(from_fn(
            move |req: axum::extract::Request, next: axum::middleware::Next| {
                backstop(request_timeout, req, next)
            },
        ))
        // `/_renox/debug`: around it all, so the session's and user's SQL count.
        .layer(from_fn_with_state(
            state.clone(),
            crate::inspector::middleware,
        ))
        .merge(assets::router())
        .merge(assets::module_router(static_assets))
        .merge(crate::health::router())
        .merge(robots(&state, embedded_public))
        .merge(favicon(&state, embedded_public))
        .merge(crate::live::router())
        .merge(public_files(&state))
        .layer(axum::extract::DefaultBodyLimit::max(
            state.config.upload_max_size,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &axum::extract::Request| {
                let ip = crate::ClientIp::of(req).map(|ip| ip.to_string());
                let id = req
                    .headers()
                    .get(crate::request_id::HEADER)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default();
                tracing::info_span!(
                    "request",
                    id = %id,
                    method = %req.method(),
                    uri = %req.uri(),
                    ip = ip.as_deref().unwrap_or("unknown"),
                )
            }),
        )
        .layer(from_fn(crate::request_id::middleware))
        .with_state(state.clone());
    // Method spoofing must change the method before the router matches it.
    let limit = state.config.upload_max_size;
    let spoofing = from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            crate::method::middleware(req, next, limit)
        },
    );
    // Security headers outermost, so every response gets them, including
    // the method-spoofing layer's own 413.
    Router::new()
        .fallback_service(tower::Layer::layer(&spoofing, router))
        .layer(from_fn_with_state(state, crate::security::middleware))
}

/// Whether the app ships `name` in `public/` (on disk, or embedded).
fn has_public_file(
    state: &AppState,
    embedded_public: Option<&'static [(&'static str, &'static [u8])]>,
    name: &str,
) -> bool {
    match embedded_public {
        Some(files) => files.iter().any(|(path, _)| *path == name),
        None => state.config.public_path.join(name).is_file(),
    }
}

/// The generated `/robots.txt`, unless the app ships its own in `public/`.
fn robots(
    state: &AppState,
    embedded_public: Option<&'static [(&'static str, &'static [u8])]>,
) -> Router<AppState> {
    if has_public_file(state, embedded_public, "robots.txt") {
        Router::new()
    } else {
        crate::seo::robots_router(state)
    }
}

/// `/favicon.ico` answers 204 (no icon, cached for a day) unless the app
/// ships one in `public/`: browsers ask for it on every site, and a 404
/// would run the whole stack, render the error page and log it each time.
fn favicon(
    state: &AppState,
    embedded_public: Option<&'static [(&'static str, &'static [u8])]>,
) -> Router<AppState> {
    if has_public_file(state, embedded_public, "favicon.ico") {
        return Router::new();
    }
    Router::new().route(
        "/favicon.ico",
        axum::routing::get(|| async {
            (
                axum::http::StatusCode::NO_CONTENT,
                [(axum::http::header::CACHE_CONTROL, "public, max-age=86400")],
            )
        }),
    )
}

/// Public files of the local disk at `/storage/...`, outside sessions.
fn public_files(state: &AppState) -> Router<AppState> {
    match state.storage.public_root() {
        Some(root) => Router::new()
            .nest_service("/storage", ServeDir::new(root))
            .layer(axum::middleware::map_response(user_file_headers)),
        None => Router::new(),
    }
}

/// Uploaded files are other people's content served from the app's origin:
/// a sandbox CSP keeps any HTML or SVG among them from running scripts,
/// `nosniff` stops browsers guessing a type, and documents are downloaded
/// instead of displayed.
pub(crate) async fn user_file_headers(
    mut res: axum::response::Response,
) -> axum::response::Response {
    use axum::http::HeaderValue;
    use axum::http::header::{
        CONTENT_DISPOSITION, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
    };

    let content_type = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let headers = res.headers_mut();
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'; sandbox",
        ),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    let document = ["html", "xml", "javascript", "ecmascript"]
        .iter()
        .any(|kind| content_type.contains(kind))
        && !content_type.contains("svg");
    if document {
        headers.insert(CONTENT_DISPOSITION, HeaderValue::from_static("attachment"));
    }
    res
}

/// Long-running commands log at info; others only log warnings and errors.
fn init_tracing(config: &Config, long_running: bool) {
    let default = match (long_running, config.debug) {
        (true, true) => "info,renox=debug",
        (true, false) => "info",
        (false, _) => "warn",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    // LOG_FILE appends to a file (rotate it with logrotate, or leave logs to
    // journald/Docker on stdout); LOG_FORMAT=json writes one JSON object per
    // line, with the request span's fields (id, method, uri, ip).
    let file = config.log_file.as_ref().and_then(|path| {
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(file) => Some(std::sync::Mutex::new(file)),
            Err(err) => {
                eprintln!("LOG_FILE {}: {err}; logging to stdout", path.display());
                None
            }
        }
    });
    let json = config.log_format == crate::LogFormat::Json;
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let _ = match (json, file) {
        (true, Some(file)) => builder
            .json()
            .with_current_span(true)
            .with_span_list(false)
            .with_writer(file)
            .try_init(),
        (true, None) => builder
            .json()
            .with_current_span(true)
            .with_span_list(false)
            .try_init(),
        (false, Some(file)) => builder.with_ansi(false).with_writer(file).try_init(),
        (false, None) => builder.try_init(),
    };
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(path: &str, middleware: &[&str]) -> RouteInfo {
        RouteInfo {
            method: "GET".into(),
            path: path.into(),
            name: None,
            module: "app".into(),
            middleware: middleware.iter().map(|m| (*m).to_owned()).collect(),
            domain: None,
        }
    }

    #[test]
    fn route_list_says_role_guards_count_the_active_scope() {
        let plain = route_table(&[route("/", &["auth"])]);
        assert!(!plain.contains('*'), "{plain}");
        let table = route_table(&[
            route("/orders", &["auth", "permission:orders.view"]),
            route("/drafts", &["role:editor"]),
            route("/admin", &["gate:admin"]),
        ]);
        assert!(table.contains("auth, permission:orders.view*"), "{table}");
        assert!(table.contains("role:editor*"), "{table}");
        assert!(table.contains("gate:admin\n"), "{table}");
        assert!(
            table.contains("active scope (permissions::set_scope)"),
            "{table}"
        );
    }

    struct Note;
    impl crate::db::FromRow for Note {
        fn from_row(_: &crate::db::Row) -> Result<Self, crate::db::DbError> {
            Ok(Note)
        }
    }
    impl crate::db::Model for Note {
        const TABLE: &'static str = "notes";
        const COLUMNS: &'static [&'static str] = &["id"];
        type Key = i64;
        fn id(&self) -> i64 {
            0
        }
        fn set_id(&mut self, _: i64) {}
        fn values(&self) -> Vec<crate::db::DbValue> {
            Vec::new()
        }
    }

    struct Registers;
    impl Module for Registers {
        fn name(&self) -> &'static str {
            "registers"
        }
        fn register(&self, app: &mut Registry) {
            app.model::<crate::auth::User>();
        }
    }

    #[tokio::test]
    async fn models_are_kept_once_from_the_app_and_modules() {
        let boot = |app: App| async move { app.config(Config::default()).boot().await.unwrap() };
        let one = boot(
            App::new()
                .model::<crate::auth::User>()
                .model::<crate::auth::User>()
                .module(Registers),
        )
        .await;
        assert_eq!(one.models().len(), 1);
        assert_eq!(one.models()[0].table, "users");
        let two = boot(
            App::new()
                .model::<crate::auth::User>()
                .model::<Note>()
                .module(Registers),
        )
        .await;
        assert_eq!(two.models().len(), 2);
    }
}

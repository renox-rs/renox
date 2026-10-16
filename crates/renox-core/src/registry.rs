use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;

use crate::events::{Event, listener};
use crate::queue::{Job, JobHandler, handler};
use crate::schedule::Schedule;
use crate::{AppState, Result};

/// Where the app and its modules register jobs, listeners, scheduled tasks
/// and commands. Modules get it in `Module::register`.
#[derive(Default)]
pub struct Registry {
    pub(crate) jobs: HashMap<&'static str, JobHandler>,
    pub(crate) listeners: HashMap<TypeId, Vec<crate::events::ListenerFn>>,
    pub(crate) schedule: Schedule,
    pub(crate) duplicate_job: Option<&'static str>,
    pub(crate) webhooks: HashMap<&'static str, crate::webhook::HandleFn>,
    pub(crate) live_components: HashMap<&'static str, crate::live_component::LiveFn>,
    pub(crate) duplicate_live: Option<&'static str>,
    pub(crate) commands: Vec<crate::command::Command>,
    pub(crate) templates: Vec<crate::view::TemplateHook>,
    pub(crate) components: Vec<crate::components::Component>,
    pub(crate) shares: Vec<(String, crate::view::ShareFn)>,
    pub(crate) channels: HashMap<String, crate::auth::notifications::ChannelFn>,
    pub(crate) reporters: Vec<crate::report::ReportFn>,
    /// The `Permissions` module is on: load each user's roles.
    pub(crate) permissions: bool,
    /// A second login step (`second_factor`), and whether two modules set one.
    pub(crate) second_factor: Option<crate::auth::second_factor::SecondFactor>,
    pub(crate) duplicate_second_factor: bool,
    /// Sections other modules add to the `/account` page.
    pub(crate) account_sections: Vec<crate::auth::account::AccountSection>,
    /// The `Auth` module's settings, for other ways of logging in
    /// (`auth::sign_in`, `auth::register_verified`).
    pub(crate) auth: Option<std::sync::Arc<crate::auth::module::Settings>>,
    /// Files modules serve as they are (`asset`): path, content type, body.
    pub(crate) assets: Vec<StaticAsset>,
    /// Values modules provide (`provide`), under the app's own `App::provide`.
    pub(crate) provided: HashMap<TypeId, std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    /// Seeders for `db:seed`: the app's (`App::seeder`), then the modules'.
    pub(crate) seeders: Vec<Seeder>,
    pub(crate) models: Vec<crate::db::schema::ModelInfo>,
    pub(crate) view_data: Vec<crate::view::TypedView>,
    /// Groups of settings (`settings`), and whether the `Settings` module
    /// that owns their table is on.
    pub(crate) settings_groups: Vec<crate::settings::Group>,
    pub(crate) settings_module: bool,
}

/// A seeder registered with `App::seeder` or `Registry::seeder`.
pub(crate) type Seeder =
    Box<dyn Fn(AppState) -> std::pin::Pin<Box<dyn Future<Output = Result> + Send>> + Send + Sync>;

/// A file a module serves as it is (`Registry::asset`).
#[derive(Clone, Copy)]
pub(crate) struct StaticAsset {
    pub(crate) path: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) body: &'static [u8],
}

impl Registry {
    /// Tells `db:check` about a model, so its fields are compared with the
    /// table. Registering the same model twice keeps one.
    pub fn model<M: crate::db::Model>(&mut self) -> &mut Self {
        let info = crate::db::schema::model_info::<M>();
        if !self.models.iter().any(|m| m.type_id == info.type_id) {
            self.models.push(info);
        }
        self
    }

    /// Tells `view:check` about a page struct ([`crate::ViewData`]), so the variables
    /// its template uses that the struct lacks are reported. Registering it twice keeps
    /// one.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// #[derive(serde::Serialize, ViewData)]
    /// #[view(template = "reports/index.html")]
    /// struct Reports { rows: Vec<String> }
    ///
    /// fn register(registry: &mut Registry) {
    ///     registry.view_data::<Reports>();
    /// }
    /// ```
    pub fn view_data<T: crate::ViewData>(&mut self) -> &mut Self {
        let typed = crate::view::TypedView::of::<T>();
        if !self.view_data.contains(&typed) {
            self.view_data.push(typed);
        }
        self
    }

    /// Lets workers run jobs of type `J`.
    pub fn job<J: Job>(&mut self) -> &mut Self {
        if self.jobs.insert(J::NAME, handler::<J>()).is_some() {
            self.duplicate_job.get_or_insert(J::NAME);
        }
        self
    }

    /// Registers the live component `C` (see `renox::live_component`).
    pub fn live_component<C: crate::live_component::LiveComponent>(&mut self) -> &mut Self {
        if self
            .live_components
            .insert(C::NAME, crate::live_component::handler::<C>())
            .is_some()
        {
            self.duplicate_live.get_or_insert(C::NAME);
        }
        self
    }

    /// Lets queue workers process `W`'s webhooks; pair it with
    /// `Routes::webhook::<W>(path)`.
    pub fn webhook<W: crate::webhook::Webhook>(&mut self) -> &mut Self {
        if self
            .webhooks
            .insert(W::PROVIDER, crate::webhook::handler::<W>())
            .is_some()
        {
            self.duplicate_job.get_or_insert(W::PROVIDER);
        }
        self
    }

    /// Runs `listener` whenever an `E` is emitted.
    pub fn listen<E, F, Fut>(&mut self, listener_fn: F) -> &mut Self
    where
        E: Event,
        F: Fn(E, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        let (type_id, run) = listener(listener_fn);
        self.listeners.entry(type_id).or_default().push(run);
        self
    }

    /// Adds a command the app binary runs: `my-app <name> [args]`. See
    /// [`crate::command`].
    pub fn command<F, Fut>(&mut self, name: &str, about: &str, run: F) -> &mut Self
    where
        F: Fn(crate::command::Args, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.commands
            .push(crate::command::command(name, about, run));
        self
    }

    /// Registers a seeder for `db:seed`, like `App::seeder`, so a module (an
    /// area of a big app, or a plugin) seeds its own tables. The app's seeders
    /// run first, then each module's in the order the modules were added.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64 }
    /// # impl Factory for Product { fn definition() -> Self { Product::default() } }
    /// struct Catalog;
    ///
    /// impl Module for Catalog {
    ///     fn name(&self) -> &'static str { "catalog" }
    ///
    ///     fn register(&self, registry: &mut Registry) {
    ///         registry.seeder(|state| async move {
    ///             Product::factory().count(50).create(&state.db).await?;
    ///             Ok(())
    ///         });
    ///     }
    /// }
    /// ```
    pub fn seeder<F, Fut>(&mut self, seeder: F) -> &mut Self
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.seeders
            .push(Box::new(move |state| Box::pin(seeder(state))));
        self
    }

    /// A command whose arguments are declared with clap; see
    /// [`AppCommand`](crate::command::AppCommand).
    pub fn typed_command<T: crate::command::AppCommand>(&mut self) -> &mut Self {
        self.commands.push(crate::command::typed::<T>());
        self
    }

    /// Adds template functions, filters or globals, e.g. a `euros` filter
    /// that always writes cents the German way:
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().templates(|env| {
    ///     env.add_filter("euros", |cents: i64| format!("{} €", renox::format_number(cents as f64 / 100.0, 2, "de")));
    /// })
    /// # ;
    /// ```
    ///
    /// Built in: `number` (`{{ price | number }}` → `75.000` in German,
    /// `number(2)` for decimals) and `date` (`{{ created_at | date("%d/%m/%Y") }}`,
    /// in `APP_TIMEZONE`).
    pub fn templates(
        &mut self,
        hook: impl Fn(&mut minijinja::Environment<'static>) + Send + Sync + 'static,
    ) -> &mut Self {
        self.templates.push(std::sync::Arc::new(hook));
        self
    }

    /// Registers a template component, so pages can write `<rx-hello …>` for a
    /// macro of the module's own templates (added with
    /// [`add_template`](crate::view::add_template) in [`Registry::templates`]).
    /// An unknown attribute or a missing required one fails when the page
    /// loads, like the built-in components.
    ///
    /// # Panics
    ///
    /// When the tag doesn't start with `rx-` or is a built-in component, or
    /// when two modules register the same tag.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::view::{Component, Prop, add_template};
    ///
    /// struct Hello;
    ///
    /// impl Module for Hello {
    ///     fn name(&self) -> &'static str {
    ///         "hello"
    ///     }
    ///
    ///     fn register(&self, app: &mut Registry) {
    ///         app.templates(|env| {
    ///             add_template(
    ///                 env,
    ///                 "hello/ui.html",
    ///                 "{% macro hello(name) %}<b>Hello {{ name }}</b>{% endmacro %}",
    ///             )
    ///             .unwrap();
    ///         });
    ///         app.component(
    ///             Component::macro_call("rx-hello", "hello/ui.html", "hello")
    ///                 .prop(Prop::text("name").required())
    ///                 .content(false),
    ///         );
    ///     }
    /// }
    /// ```
    pub fn component(&mut self, component: crate::components::Component) -> &mut Self {
        if let Some(problem) = component.problem() {
            panic!("Registry::component: {problem}");
        }
        assert!(
            !self.components.iter().any(|c| c.tag() == component.tag()),
            "Registry::component: <{}> is registered twice",
            component.tag()
        );
        self.components.push(component);
        self
    }

    /// Serves `body` at `path` the way Renox serves its own scripts and
    /// styles: in front of sessions, CSRF and maintenance mode (no cookie is
    /// set), with a year-long `immutable` cache. For a module's JavaScript,
    /// CSS or fonts, compiled into its crate; put a version or a hash of the
    /// content in `path`, so a new release gets a new address.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// struct Charts;
    ///
    /// impl Module for Charts {
    ///     fn name(&self) -> &'static str { "charts" }
    ///
    ///     fn register(&self, app: &mut Registry) {
    ///         app.asset(
    ///             "/_charts/charts-1.2.0.js",
    ///             "text/javascript; charset=utf-8",
    ///             b"console.log('charts')",
    ///         );
    ///     }
    /// }
    /// ```
    ///
    /// The path must start with `/`; two files at the same path stop the
    /// app at boot.
    pub fn asset(
        &mut self,
        path: &'static str,
        content_type: &'static str,
        body: &'static [u8],
    ) -> &mut Self {
        self.assets.push(StaticAsset {
            path,
            content_type,
            body,
        });
        self
    }

    /// Gives every view `key`, computed per request, e.g. the categories in
    /// a menu or the number of items in a cart. It runs for every rendered
    /// view, so keep it quick (cache what doesn't change per request).
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use std::time::Duration;
    /// # let _ =
    /// App::new().share("cart_count", |ctx: renox::view::ViewContext| async move {
    ///     let Some(user) = ctx.user else { return Ok(0) };
    ///     let n: i64 = renox::db::sql("SELECT COUNT(*) FROM cart_items WHERE user_id = ?")
    ///         .bind(user.id)
    ///         .scalar(&ctx.state.db)
    ///         .await?;
    ///     Ok(n)
    /// })
    /// # ;
    /// ```
    ///
    /// `ctx.session` is the request's session, for values kept there (a
    /// guest's cart). Values a handler passes in `context!` win over shared
    /// ones.
    pub fn share<F, Fut, T>(&mut self, key: &str, compute: F) -> &mut Self
    where
        F: Fn(crate::view::ViewContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
        T: serde::Serialize,
    {
        self.shares
            .push((key.to_owned(), crate::view::share_fn(compute)));
        self
    }

    /// Adds a notification channel, used by notifications that list
    /// `Channel::Custom(name)`: `send` gets the recipient and the message
    /// `Notification::to_channel` built. See [`crate::auth::notifications`].
    pub fn channel<F, Fut>(&mut self, name: &str, send: F) -> &mut Self
    where
        F: Fn(crate::auth::Recipient, serde_json::Value, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.channels.insert(
            name.to_owned(),
            crate::auth::notifications::channel_fn(send),
        );
        self
    }

    /// Adds a second step to logging in, such as two-factor authentication;
    /// see [`crate::auth::second_factor`]. After the right password, a user
    /// for whom `required` answers `true` isn't logged in yet: the browser
    /// goes to the route named `challenge`, whose handler checks the code and
    /// calls [`crate::auth::complete_login`]. One module may set it; a second
    /// one is an error at boot.
    pub fn second_factor<F, Fut>(&mut self, challenge: &str, required: F) -> &mut Self
    where
        F: Fn(crate::auth::User, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        if self.second_factor.is_some() {
            self.duplicate_second_factor = true;
        }
        self.second_factor = Some(crate::auth::second_factor::second_factor(
            challenge, required,
        ));
        self
    }

    /// Adds a section to the `Auth` module's `/account` page (with
    /// `Auth::new().account()`), such as two-factor authentication or linked
    /// logins. `template` is rendered with the page's context; `data` runs
    /// for the logged-in user on every visit, and the template reads what it
    /// returns as `section.data`. Sections show in `order` (then in the order
    /// they were added), after the built-in cards and before "Delete account".
    ///
    /// ```
    /// # use renox::prelude::*;
    /// struct Pin;
    ///
    /// impl Module for Pin {
    ///     fn name(&self) -> &'static str {
    ///         "pin"
    ///     }
    ///
    ///     fn register(&self, app: &mut Registry) {
    ///         app.templates(|env| {
    ///             env.add_template(
    ///                 "pin/account.html",
    ///                 r#"<section id="pin">PIN {{ "on" if section.data.on else "off" }}</section>"#,
    ///             )
    ///             .unwrap();
    ///         });
    ///         app.account_section("pin/account.html", 10, |user, _state| async move {
    ///             Ok(json!({ "on": user.extra.contains_key("pin") }))
    ///         });
    ///     }
    /// }
    /// ```
    pub fn account_section<F, Fut>(&mut self, template: &str, order: i32, data: F) -> &mut Self
    where
        F: Fn(crate::auth::User, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value>> + Send + 'static,
    {
        self.account_sections
            .push(crate::auth::account::section(template, order, data));
        self
    }

    /// Sends every error that needs a person to `reporter`; see
    /// [`crate::report`].
    pub fn report<F, Fut>(&mut self, reporter: F) -> &mut Self
    where
        F: Fn(crate::report::ErrorReport, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.reporters.push(crate::report::report_fn(reporter));
        self
    }

    /// Makes `value` available everywhere the app runs, as
    /// [`App::provide`](crate::App::provide) does: `Provided<T>` in handlers,
    /// `state.provided::<T>()` in jobs, listeners, webhooks and commands. For
    /// a module's settings that code without a request needs (a payment
    /// module's plans in its webhook handler). One value per type; a value
    /// the app gives with `App::provide` wins.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// struct Plans(Vec<&'static str>);
    ///
    /// struct Billing;
    ///
    /// impl Module for Billing {
    ///     fn name(&self) -> &'static str {
    ///         "billing"
    ///     }
    ///
    ///     fn register(&self, app: &mut Registry) {
    ///         app.provide(Plans(vec!["basic", "pro"]));
    ///     }
    /// }
    ///
    /// async fn in_a_job(state: AppState) {
    ///     let plans = state.provided::<Plans>().expect("the Billing module is on");
    /// #   let _ = plans;
    /// }
    /// ```
    pub fn provide<T: Send + Sync + 'static>(&mut self, value: T) -> &mut Self {
        self.provided
            .insert(TypeId::of::<T>(), std::sync::Arc::new(value));
        self
    }

    /// The schedule, to add tasks to.
    pub fn schedule(&mut self) -> &mut Schedule {
        &mut self.schedule
    }
}

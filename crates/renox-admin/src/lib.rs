//! An admin panel for Renox apps, generated from their models (Laravel's
//! Filament resources): each model is declared once, as an
//! [`AdminResource`] (its grid columns, its form fields, its policy), and
//! the panel gives it a list with search, filters, sorting, bulk actions
//! and exports, and create, edit, view and delete pages, on the UI kit and
//! `renox::grid`.
//!
//! ```
//! use renox::prelude::*;
//! use renox_admin::Admin;
//! # use renox::grid::Column;
//! # use renox_admin::{AdminResource, Field};
//! # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String }
//! # impl Policy for Product { fn allows(&self, _: &User, _: &str) -> bool { true } }
//! # #[derive(serde::Deserialize, serde::Serialize, Validate)] struct ProductForm { #[validate(required)] name: String }
//! # struct Products;
//! # impl AdminResource for Products {
//! #     type Model = Product;
//! #     type Form = ProductForm;
//! #     fn label(&self) -> &str { "Product" }
//! #     fn plural_label(&self) -> &str { "Products" }
//! #     fn columns(&self) -> Vec<Column> { vec![Column::text("name", "Name")] }
//! #     fn fields(&self) -> Vec<Field> { vec![Field::text("name", "Name")] }
//! #     fn fill(&self, p: &mut Product, f: ProductForm) { p.name = f.name; }
//! # }
//!
//! # let _ =
//! App::new()
//!     .module(Auth::new())
//!     .module(
//!         Admin::new()
//!             .title("Back office")
//!             .authorize(|user| user.has_role("staff")) // who may open the panel
//!             .resource(Products),
//!     )
//! # ;
//! ```
//!
//! The panel lives at `/admin` (`path`), behind a login. Nobody gets in
//! until [`Admin::authorize`] (or [`Admin::gate`]) says who may; then each
//! page asks the resource's [`AdminResource::allows`], which by default
//! asks the model's [`Policy`] (`viewAny`, `view`, `create`, `update`,
//! `delete`, …). Pages are built-in templates (`renox-admin/*.html`) that
//! an app replaces with files of the same name. The guide is
//! docs/admin.md in the Renox repository.

#![warn(missing_docs)]

use std::fmt;
use std::sync::Arc;

use renox::prelude::*;

mod custom_fields;
pub mod entry;
pub mod field;
mod panel;
pub mod relation;
pub mod resource;
pub mod settings;
mod texts;

pub use entry::Entry;
pub use field::{Field, FieldKind};
pub use relation::RelationManager;
pub use resource::{
    ActionContext, ActionInput, AdminAction, AdminResource, BoxFuture, Filter, SaveContext,
};
pub use settings::AdminSettings;

use panel::{Access, Holder, Listed, Panel};
use settings::{Page, SettingsPage};

/// The templates, compiled in. An app replaces one with a file of the same
/// name under its views directory (`resources/views/renox-admin/layout.html`).
const VIEWS: &[(&str, &str)] = &[
    (
        "renox-admin/layout.html",
        include_str!("../views/layout.html"),
    ),
    (
        "renox-admin/dashboard.html",
        include_str!("../views/dashboard.html"),
    ),
    (
        "renox-admin/index.html",
        include_str!("../views/index.html"),
    ),
    ("renox-admin/form.html", include_str!("../views/form.html")),
    (
        "renox-admin/relation.html",
        include_str!("../views/relation.html"),
    ),
    ("renox-admin/show.html", include_str!("../views/show.html")),
    (
        "renox-admin/fields.html",
        include_str!("../views/fields.html"),
    ),
    (
        "renox-admin/settings.html",
        include_str!("../views/settings.html"),
    ),
];

/// A place in the panel's layout where an app (or a module) adds content
/// without replacing the layout: [`Admin::slot`], or a file of the slot's
/// name under the app's views (`renox-admin/slots/after_content.html`).
/// A slot's template sees the page's values (`admin`, `resource`, `record`,
/// `title`, …) and the request's globals (`request.route`, `auth`, …).
///
/// ```
/// use renox_admin::Slot;
///
/// assert_eq!(Slot::AfterContent.template(), "renox-admin/slots/after_content.html");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Slot {
    /// In the page's `<head>`, after the app's `head` stack.
    Head,
    /// At the top of the sidebar, before the Dashboard link.
    SidebarStart,
    /// At the bottom of the sidebar, after the resources.
    SidebarEnd,
    /// In the top bar, before the account menu.
    NavbarEnd,
    /// Above the page's content.
    BeforeContent,
    /// Below the page's content ("About this page").
    AfterContent,
}

impl Slot {
    /// The template the layout includes for the slot, if there is one.
    pub fn template(self) -> &'static str {
        match self {
            Self::Head => "renox-admin/slots/head.html",
            Self::SidebarStart => "renox-admin/slots/sidebar_start.html",
            Self::SidebarEnd => "renox-admin/slots/sidebar_end.html",
            Self::NavbarEnd => "renox-admin/slots/navbar_end.html",
            Self::BeforeContent => "renox-admin/slots/before_content.html",
            Self::AfterContent => "renox-admin/slots/after_content.html",
        }
    }
}

/// The admin panel module: its address, its title, who may use it and its
/// resources.
#[derive(Clone)]
pub struct Admin {
    path: String,
    title: String,
    access: Option<Access>,
    resources: Vec<Arc<dyn Listed>>,
    settings: Vec<Arc<dyn SettingsPage>>,
    slots: Vec<(Slot, String)>,
    name: Option<String>,
    own_only: bool,
}

/// A resource a module adds to an admin panel from its `register`, see
/// [`contribute`]. By default it joins the panel without a name; with
/// [`panel`](Self::panel) it joins the [`Admin::named`] panel of that name.
///
/// ```
/// # use renox::prelude::*;
/// # use renox_admin::{AdminEntry, AdminResource};
/// # fn demo<R: AdminResource>(resource: R, app: &mut Registry) {
/// app.contribute(AdminEntry::new(resource).panel("staff"));
/// # }
/// ```
#[derive(Clone)]
pub struct AdminEntry {
    resource: Arc<dyn Listed>,
    panel: Option<String>,
}

impl AdminEntry {
    /// An entry for `resource`, for the panel without a name.
    ///
    /// # Panics
    ///
    /// If the slug isn't letters, digits, `_` and `-`.
    pub fn new<R: AdminResource>(resource: R) -> Self {
        check_slug("AdminEntry::new", resource.slug());
        Self {
            resource: Arc::new(Holder(Arc::new(resource))),
            panel: None,
        }
    }

    /// Sends the resource to the panel named `name` ([`Admin::named`])
    /// instead of the unnamed one. A name no `Admin` has fails at boot.
    pub fn panel(mut self, name: &str) -> Self {
        self.panel = Some(name.to_owned());
        self
    }
}

/// Adds `resource` to the app's admin panel from a module's `register`
/// (shorthand for `app.contribute(AdminEntry::new(resource))`). The panel
/// shows it after its own resources, with the same gate and navigation.
///
/// ```
/// # use renox::prelude::*;
/// # use renox_admin::AdminResource;
/// # fn demo<R: AdminResource>(resource: R, app: &mut Registry) {
/// renox_admin::contribute(app, resource);
/// # }
/// ```
pub fn contribute<R: AdminResource>(app: &mut Registry, resource: R) {
    app.contribute(AdminEntry::new(resource));
}

/// The name of a panel, so entries aimed at an unknown one fail at boot.
struct PanelName(String);

fn check_slug(what: &str, slug: &str) {
    assert!(
        !slug.is_empty()
            && slug.len() <= 64
            && slug
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "{what}: the slug `{slug}` may only have letters, digits, `_` and `-`"
    );
}

impl Default for Admin {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Admin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Admin")
            .field("path", &self.path)
            .field("title", &self.title)
            .field(
                "resources",
                &self.resources.iter().map(|r| r.slug()).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Admin {
    /// A panel at `/admin`, titled "Admin", with no resources yet, that
    /// nobody may open until [`authorize`](Self::authorize) says who may.
    pub fn new() -> Self {
        Self {
            path: "/admin".into(),
            title: "Admin".into(),
            access: None,
            resources: Vec::new(),
            settings: Vec::new(),
            slots: Vec::new(),
            name: None,
            own_only: false,
        }
    }

    /// Names the panel, so it takes the [`AdminEntry`]s aimed at that name
    /// (`AdminEntry::panel`) instead of the unnamed ones. A named panel has
    /// its own route names, `admin.<name>.*` (`admin.staff.dashboard`), and
    /// module name `admin.<name>`, so several panels can run in one app.
    pub fn named(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    /// Ignores what modules contribute: only the resources added here.
    pub fn only_own_resources(mut self) -> Self {
        self.own_only = true;
        self
    }

    /// Where the panel lives (`/admin` by default). The route names stay
    /// `admin.*` (`admin.<name>.*` for a [named](Self::named) panel).
    ///
    /// # Panics
    ///
    /// If `path` doesn't start with `/` or ends with one.
    pub fn path(mut self, path: &str) -> Self {
        assert!(
            path.starts_with('/') && !path.ends_with('/'),
            "Admin::path(\"{path}\"): it must start with `/` and not end with one"
        );
        self.path = path.to_owned();
        self
    }

    /// The panel's name, at the top of its navigation and in page titles.
    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_owned();
        self
    }

    /// Who may open the panel at all: logged-in users for whom `allows`
    /// says yes (others get 403, guests go to log in). Without it, nobody
    /// may.
    pub fn authorize(mut self, allows: impl Fn(&AuthUser) -> bool + Send + Sync + 'static) -> Self {
        self.access = Some(Arc::new(allows));
        self
    }

    /// Lets in the users the gate `name` lets through (`App::gate`, or a
    /// permission of that name with the `Permissions` module, after
    /// `App::gate_before`).
    pub fn gate(self, name: &str) -> Self {
        let name = name.to_owned();
        self.authorize(move |user| user.allows(&name))
    }

    /// Adds `template` (MiniJinja source) to a place in every page's
    /// layout, without replacing the layout. Several for one slot are drawn
    /// in the order added. A file of the slot's name under the app's views
    /// ([`Slot::template`]) replaces them.
    ///
    /// ```
    /// use renox_admin::{Admin, Slot};
    ///
    /// let admin = Admin::new().slot(
    ///     Slot::AfterContent,
    ///     r#"<aside class="about"><h2>About this page</h2><p>{{ request.route }}</p></aside>"#,
    /// );
    /// # let _ = admin;
    /// ```
    pub fn slot(mut self, slot: Slot, template: &str) -> Self {
        self.slots.push((slot, template.to_owned()));
        self
    }

    /// Adds a resource, after the ones before it in the navigation.
    ///
    /// # Panics
    ///
    /// If its slug isn't letters, digits, `_` and `-`, or another resource
    /// has it.
    pub fn resource<R: AdminResource>(mut self, resource: R) -> Self {
        let slug = resource.slug().to_owned();
        check_slug("Admin::resource", &slug);
        assert!(
            self.resources.iter().all(|r| r.slug() != slug),
            "Admin::resource: two resources have the slug `{slug}`"
        );
        self.resources.push(Arc::new(Holder(Arc::new(resource))));
        self
    }

    /// Adds a page that edits a group of settings kept in the database
    /// (`renox::settings`), at `/admin/settings/<key>` under a "Settings"
    /// heading of the navigation. The form comes from
    /// [`AdminSettings::fields`]; saving runs the group's rules and
    /// `AppState::save_settings`, so it is audited and emits
    /// `SettingsChanged`. The app needs the `renox::settings::Settings`
    /// module (the group is declared for you).
    ///
    /// A group marked `ENCRYPTED` never prints its values: its fields start
    /// empty with a note saying whether they are set and when they changed,
    /// and a field left empty keeps its stored value.
    ///
    /// # Panics
    ///
    /// If another page has the same group.
    pub fn settings<S: AdminSettings>(mut self, label: &str) -> Self {
        assert!(
            self.settings.iter().all(|page| page.key() != S::KEY),
            "Admin::settings: two pages edit the group `{}`",
            S::KEY
        );
        self.settings.push(Arc::new(Page::<S>::new(label)));
        self
    }
}

impl Admin {
    fn panel_routes(&self, mut resources: Vec<Arc<dyn Listed>>) -> Routes {
        // Models with custom fields get the screen that manages them.
        let models: Vec<(String, String)> = resources
            .iter()
            .filter_map(|r| r.custom_fields_model())
            .collect();
        if !models.is_empty() && resources.iter().all(|r| r.slug() != custom_fields::SLUG) {
            resources.push(Arc::new(Holder(Arc::new(custom_fields::CustomFields {
                models,
            }))));
        }
        panel::routes(Arc::new(Panel {
            path: self.path.clone(),
            title: self.title.clone(),
            name: self.name.clone(),
            access: self.access.clone(),
            resources,
            settings: self.settings.clone(),
        }))
    }
}

impl Module for Admin {
    fn name(&self) -> &'static str {
        match &self.name {
            None => "admin",
            // Once per panel at boot; the name must outlive the module list.
            Some(name) => Box::leak(format!("admin.{name}").into_boxed_str()),
        }
    }

    fn routes(&self) -> Routes {
        self.panel_routes(self.resources.clone())
    }

    fn routes_with(&self, contributions: &Contributions) -> Routes {
        let mut resources = self.resources.clone();
        let mut from: Vec<String> = vec!["the app's Admin".into(); resources.len()];
        // An entry for a panel no Admin has is a mistake worth failing on.
        let names: Vec<&str> = contributions
            .all::<PanelName>()
            .iter()
            .map(|n| n.0.as_str())
            .collect();
        for (module, entry) in contributions.all_from::<AdminEntry>() {
            if let Some(panel) = &entry.panel {
                assert!(
                    names.contains(&panel.as_str()),
                    "{module} contributes an admin resource to the panel \"{panel}\", \
                     but no Admin is named \"{panel}\" (Admin::named)"
                );
            }
        }
        if !self.own_only {
            for (module, entry) in contributions.all_from::<AdminEntry>() {
                if entry.panel != self.name {
                    continue;
                }
                let slug = entry.resource.slug();
                let who = if module == "app" {
                    "the app".to_owned()
                } else {
                    module.to_owned()
                };
                if let Some(at) = resources.iter().position(|r| r.slug() == slug) {
                    panic!("\"{slug}\" is contributed by {} and by {who}", from[at]);
                }
                resources.push(entry.resource.clone());
                from.push(who);
            }
        }
        self.panel_routes(resources)
    }

    fn register(&self, app: &mut Registry) {
        if let Some(name) = &self.name {
            app.contribute(PanelName(name.clone()));
        }
        for page in &self.settings {
            page.declare(app);
        }
        // One template per slot: what was added to it, in order.
        let mut slots: Vec<(&'static str, String)> = Vec::new();
        for (slot, source) in &self.slots {
            match slots.iter_mut().find(|(name, _)| *name == slot.template()) {
                Some((_, all)) => {
                    all.push('\n');
                    all.push_str(source);
                }
                None => slots.push((slot.template(), source.clone())),
            }
        }
        app.templates(move |env| {
            for (name, source) in VIEWS {
                // The app's own file of that name wins.
                if env.get_template(name).is_err() {
                    renox::view::add_template(env, *name, *source)
                        .expect("a built-in template compiles");
                }
            }
            for (name, source) in &slots {
                if env.get_template(name).is_err() {
                    renox::view::add_template(env, *name, source.clone())
                        .expect("a built-in template compiles");
                }
            }
        });
    }
}

/// Compiles the Rust in docs/admin.md (the guide) as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/admin.md")]
pub struct Guide;

/// Compiles the Rust in docs/settings.md (the guide) as doctests; it lives
/// here because its admin page example needs this crate.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/settings.md")]
pub struct SettingsGuide;

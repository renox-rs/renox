//! The pages: the repository's Markdown, compiled into the binary, so the
//! site always shows the docs of the commit it was built from.

/// Where a page sits in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Section {
    Start,
    Guides,
    Project,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Start, Section::Guides, Section::Project];

    /// The section's icon (a name from `icons.rs`).
    pub fn icon(self) -> &'static str {
        match self {
            Section::Start => "rocket",
            Section::Guides => "book-open",
            Section::Project => "package",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Start => "Start here",
            Section::Guides => "Guides",
            Section::Project => "Project",
        }
    }
}

/// One page of the site.
#[derive(Debug)]
pub struct Page {
    /// Its address: `/docs/{slug}`.
    pub slug: &'static str,
    /// The name in the sidebar (the page's own title is its first heading).
    pub nav: &'static str,
    pub section: Section,
    /// Its icon (a name from `icons.rs`).
    pub icon: &'static str,
    /// What the page is for, in one plain sentence: under its title, and in
    /// search results and the home page.
    pub blurb: &'static str,
    /// The file in the repository, for "Edit on GitHub" and link rewriting.
    pub path: &'static str,
    pub markdown: &'static str,
}

macro_rules! page {
    ($slug:literal, $nav:literal, $section:ident, $icon:literal, $path:literal, $blurb:literal) => {
        Page {
            slug: $slug,
            nav: $nav,
            section: Section::$section,
            icon: $icon,
            blurb: $blurb,
            path: $path,
            markdown: include_str!(concat!("../../", $path)),
        }
    };
}

/// Every page, in sidebar order.
pub static PAGES: &[Page] = &[
    page!(
        "overview",
        "Overview",
        Start,
        "compass",
        "README.md",
        "What Renox is, what comes in the box, and a first taste of the code."
    ),
    page!(
        "tutorial",
        "Tutorial",
        Start,
        "graduation-cap",
        "docs/tutorial.md",
        "Build a small app step by step, from an empty folder to a running server."
    ),
    page!(
        "laravel",
        "Coming from Laravel",
        Start,
        "arrow-right-left",
        "docs/laravel.md",
        "Know Laravel? Each idea you know, and its name in Renox."
    ),
    page!(
        "cheatsheet",
        "Cheat sheet",
        Start,
        "list-checks",
        "CHEATSHEET.md",
        "The most common things you'll write, a few lines each."
    ),
    page!(
        "routing",
        "Routing and middleware",
        Guides,
        "route",
        "docs/routing.md",
        "Which code answers which web address, and what runs before it."
    ),
    page!(
        "validation",
        "Forms and validation",
        Guides,
        "square-check",
        "docs/validation.md",
        "Read what people type into forms, and check it before you use it."
    ),
    page!(
        "ui",
        "Views and the UI kit",
        Guides,
        "layout-dashboard",
        "docs/ui.md",
        "Build pages from templates and ready-made parts: buttons, forms, tables."
    ),
    page!(
        "live",
        "Live components",
        Guides,
        "zap",
        "docs/live.md",
        "A part of a page with state in a Rust struct: click, and the server draws it again."
    ),
    page!(
        "grid",
        "The data grid",
        Guides,
        "table",
        "docs/grid.md",
        "A table of records people can sort, filter, search and export."
    ),
    page!(
        "relations",
        "Models and relations",
        Guides,
        "database",
        "docs/relations.md",
        "Save data in tables and load the rows that belong together."
    ),
    page!(
        "search",
        "Full-text search",
        Guides,
        "search",
        "docs/search.md",
        "Find records by the words in them, best matches first."
    ),
    page!(
        "types",
        "Field types",
        Guides,
        "type",
        "docs/types.md",
        "Which Rust type to use for each kind of form field and table column."
    ),
    page!(
        "authorization",
        "Authorization and tenants",
        Guides,
        "shield-check",
        "docs/authorization.md",
        "Decide who may see or change what."
    ),
    page!(
        "two-factor",
        "Two-factor authentication",
        Guides,
        "lock",
        "docs/two-factor.md",
        "A code from a phone app after the password, with the renox-2fa crate."
    ),
    page!(
        "oauth",
        "Social login",
        Guides,
        "log-in",
        "docs/oauth.md",
        "Sign in with Google or GitHub, linked to your users, with the renox-oauth crate."
    ),
    page!(
        "editors",
        "Rich text, Markdown and code editors",
        Guides,
        "pencil",
        "docs/editors.md",
        "Editor fields for formatted text, Markdown and code, with the renox-editors crate."
    ),
    page!(
        "blocks",
        "Blocks beyond the kit",
        Guides,
        "layout-dashboard",
        "docs/blocks.md",
        "A stepper, a range slider, a gallery, calendars and a kanban board, with the renox-blocks crate."
    ),
    page!(
        "admin",
        "Admin panel",
        Guides,
        "panel-left",
        "docs/admin.md",
        "A back office generated from your models, with the renox-admin crate."
    ),
    page!(
        "billing",
        "Billing",
        Guides,
        "credit-card",
        "docs/billing.md",
        "Charge for plans with trials, upgrades and cancellations, with the renox-billing crate."
    ),
    page!(
        "queue",
        "The queue",
        Guides,
        "layers",
        "docs/queue.md",
        "Do slow work in the background, so pages stay fast."
    ),
    page!(
        "mail",
        "Mail and notifications",
        Guides,
        "mail",
        "docs/mail.md",
        "Send emails and in-app messages to your users."
    ),
    page!(
        "scheduling",
        "Scheduler, events, cache",
        Guides,
        "clock",
        "docs/scheduling.md",
        "Run tasks on a timetable, react to things that happen, remember results."
    ),
    page!(
        "testing",
        "Testing",
        Guides,
        "flask",
        "docs/testing.md",
        "Check that your app works, automatically, every time you change it."
    ),
    page!(
        "postgresql",
        "PostgreSQL",
        Guides,
        "hard-drive",
        "docs/postgresql.md",
        "Use PostgreSQL instead of SQLite when one server isn't enough."
    ),
    page!(
        "operations",
        "Running in production",
        Guides,
        "server",
        "docs/operations.md",
        "Put your app on a real server and keep it healthy."
    ),
    page!(
        "development",
        "Faster builds",
        Guides,
        "zap",
        "docs/development.md",
        "Make compiling quicker while you work."
    ),
    page!(
        "stability",
        "Stability and versions",
        Project,
        "anchor",
        "docs/stability.md",
        "What may change between versions, and what won't."
    ),
    page!(
        "changelog",
        "Changelog",
        Project,
        "history",
        "CHANGELOG.md",
        "Everything that changed, version by version."
    ),
    page!(
        "contributing",
        "Contributing",
        Project,
        "git-pull-request",
        "CONTRIBUTING.md",
        "How to help: report a problem, or send a change."
    ),
    page!(
        "security",
        "Security",
        Project,
        "lock",
        "SECURITY.md",
        "How to tell us about a security problem, privately."
    ),
    page!(
        "releasing",
        "Releasing",
        Project,
        "package",
        "RELEASING.md",
        "How a new version is published."
    ),
];

/// The page with this slug.
pub fn page(slug: &str) -> Option<&'static Page> {
    PAGES.iter().find(|page| page.slug == slug)
}

/// The page made from this repository file (`docs/routing.md`), for links.
pub fn page_for_path(path: &str) -> Option<&'static Page> {
    PAGES.iter().find(|page| page.path == path)
}

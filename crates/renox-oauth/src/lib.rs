//! Social login for Renox apps: "Continue with Google" and "Continue with
//! GitHub" on the login and register pages, linked to the app's `users`.
//!
//! ```
//! use renox::prelude::*;
//! use renox_oauth::OAuth;
//!
//! # let _ =
//! App::new()
//!     .module(Auth::new().account()) // the account page lists linked logins
//!     .module(OAuth::new().google().github()) // GOOGLE_CLIENT_ID, GITHUB_CLIENT_ID, … in .env
//! # ;
//! ```
//!
//! The authorization code flow with PKCE (S256) and a single-use `state`
//! bound to the session. A provider account logs in the user it is linked
//! to; at a first login, it is linked to the account with the same
//! **verified** address, or makes a new account (without a password) when
//! registration is open. Users link and unlink providers from their account
//! page. The guide is docs/oauth.md in the Renox repository.

#![warn(missing_docs)]

use std::fmt;
use std::sync::Arc;

use renox::db::Migration;
use renox::prelude::*;
use serde::Serialize;

pub mod events;
pub mod github;
pub mod google;
mod handlers;
mod model;
pub mod provider;

pub use events::{AccountLinked, AccountUnlinked, LoggedInWith};
pub use github::GitHub;
pub use google::Google;
pub use model::OAuthAccount;
pub use provider::{BoxFuture, Credentials, Profile, Provider, Token, TokenRequest, exchange_code};

const MIGRATIONS: &[Migration] = &[Migration::new(
    "00010101000800_create_oauth_accounts_table",
    "",
    Some(include_str!(
        "../migrations/00010101000800_create_oauth_accounts_table.down.sql"
    )),
)
.sqlite(
    include_str!("../migrations/00010101000800_create_oauth_accounts_table.up.sql"),
    None,
)
.postgres(
    include_str!("../migrations/00010101000800_create_oauth_accounts_table.postgres.up.sql"),
    None,
)];

/// The templates, compiled in. An app replaces one with a file of the same
/// name under its views directory (`resources/views/oauth/section.html`).
const VIEWS: &[(&str, &str)] = &[
    (
        "renox/auth/login_options.html",
        include_str!("../views/login_options.html"),
    ),
    ("oauth/section.html", include_str!("../views/section.html")),
];

/// The social login module: add it next to `Auth` (with `.account()` for
/// the page where users link and unlink providers).
#[derive(Clone, Default)]
pub struct OAuth {
    providers: Vec<Arc<dyn Provider>>,
}

impl OAuth {
    /// The module, with no provider yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds Google, with `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` from
    /// the configuration (off while they're missing).
    pub fn google(self) -> Self {
        self.provider(Google::from_config())
    }

    /// Adds GitHub, with `GITHUB_CLIENT_ID` and `GITHUB_CLIENT_SECRET` from
    /// the configuration (off while they're missing).
    pub fn github(self) -> Self {
        self.provider(GitHub::from_config())
    }

    /// Adds a provider: [`Google::new`] with credentials given in code, or
    /// one of your own ([`Provider`]). One of the same name is replaced.
    /// Buttons show in the order providers are added.
    pub fn provider(mut self, provider: impl Provider) -> Self {
        self.providers.retain(|p| p.name() != provider.name());
        self.providers.push(Arc::new(provider));
        self
    }
}

impl fmt::Debug for OAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.providers.iter().map(|p| p.name().to_owned()))
            .finish()
    }
}

/// The providers, shared by the routes, the account section and the views.
#[derive(Clone)]
pub(crate) struct Providers(Arc<Vec<Arc<dyn Provider>>>);

impl Providers {
    /// The provider named `name`, when its credentials are set.
    pub(crate) fn enabled(&self, state: &AppState, name: &str) -> Option<Arc<dyn Provider>> {
        self.0
            .iter()
            .find(|p| p.name() == name)
            .filter(|p| p.credentials().resolve(&state.config).is_some())
            .cloned()
    }

    /// Every provider whose credentials are set, in order.
    pub(crate) fn all_enabled(&self, state: &AppState) -> Vec<Arc<dyn Provider>> {
        self.0
            .iter()
            .filter(|p| p.credentials().resolve(&state.config).is_some())
            .cloned()
            .collect()
    }
}

/// A provider as the login pages' buttons see it (`oauth_providers`).
#[derive(Serialize)]
struct Button {
    name: String,
    label: String,
    url: String,
}

/// A provider on the account page (`section.data.providers`).
#[derive(Serialize)]
struct Row {
    name: String,
    label: String,
    /// Where linking starts; `None` for a provider no longer offered.
    link_url: Option<String>,
    linked: bool,
    email: Option<String>,
    since: Option<DateTime>,
}

/// The account page's section (`section.data`).
#[derive(Serialize)]
struct Section {
    providers: Vec<Row>,
    has_password: bool,
    /// Whether unlinking leaves a way to log in.
    can_unlink: bool,
    /// "Forgot your password?", where a user without a password sets one.
    set_password_url: Option<String>,
}

impl Module for OAuth {
    fn name(&self) -> &'static str {
        "oauth"
    }

    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS
    }

    fn routes(&self) -> Routes {
        handlers::routes(Providers(Arc::new(self.providers.clone())))
    }

    fn register(&self, app: &mut Registry) {
        let providers = Providers(Arc::new(self.providers.clone()));
        // The buttons on the login, register and confirm-password pages.
        let shared = providers.clone();
        app.share("oauth_providers", move |ctx: renox::view::ViewContext| {
            let providers = shared.clone();
            async move { buttons(&ctx, &providers).await }
        });
        let listed = providers.clone();
        app.account_section("oauth/section.html", 50, move |user, state| {
            let providers = listed.clone();
            async move {
                let section = section(&state, &providers, &user).await?;
                Ok(renox::serde_json::to_value(section)?)
            }
        });
        app.templates(|env| {
            for (name, source) in VIEWS {
                // The app's own file of that name wins.
                if env.get_template(name).is_err() {
                    renox::view::add_template(env, *name, *source)
                        .expect("a built-in template compiles");
                }
            }
        });
        // Recorded in the activity log when the app has the `Audit` module.
        app.listen(|e: AccountLinked, state| async move {
            let entry = renox::audit::Entry::new("oauth.linked")
                .user(e.user_id)
                .data(renox::serde_json::json!({ "provider": e.provider }));
            audit(&state, entry).await
        })
        .listen(|e: AccountUnlinked, state| async move {
            let entry = renox::audit::Entry::new("oauth.unlinked")
                .user(e.user_id)
                .data(renox::serde_json::json!({ "provider": e.provider }));
            audit(&state, entry).await
        })
        .listen(|e: LoggedInWith, state| async move {
            let entry = renox::audit::Entry::new("oauth.login")
                .user(e.user_id)
                .data(renox::serde_json::json!({
                    "provider": e.provider,
                    "second_step": e.second_step,
                }));
            audit(&state, entry).await
        });
    }
}

/// The providers to offer on this page. On `/confirm-password`, only the
/// ones the user linked (another one wouldn't prove who they are).
async fn buttons(ctx: &renox::view::ViewContext, providers: &Providers) -> Result<Vec<Button>> {
    let state = &ctx.state;
    let mut enabled = providers.all_enabled(state);
    if enabled.is_empty() {
        return Ok(Vec::new());
    }
    let confirming = state
        .url("password.confirm", &[])
        .is_ok_and(|path| path == ctx.path);
    let mut confirm = false;
    if let Some(user) = ctx.user.as_ref().filter(|_| confirming) {
        let linked = OAuthAccount::of_user(&state.db, user.id).await?;
        enabled.retain(|p| linked.iter().any(|a| a.provider == p.name()));
        confirm = true;
    }
    enabled
        .iter()
        .map(|p| {
            let mut url = state.url("oauth.redirect", &[&p.name()])?;
            if confirm {
                url.push_str("?intent=confirm");
            }
            Ok(Button {
                name: p.name().to_owned(),
                label: p.label().to_owned(),
                url,
            })
        })
        .collect()
}

/// What the account page shows: each provider, linked or not.
async fn section(state: &AppState, providers: &Providers, user: &User) -> Result<Section> {
    let linked = OAuthAccount::of_user(&state.db, user.id).await?;
    let mut rows = Vec::new();
    for provider in providers.all_enabled(state) {
        let account = linked.iter().find(|a| a.provider == provider.name());
        rows.push(Row {
            name: provider.name().to_owned(),
            label: provider.label().to_owned(),
            link_url: Some(state.url("oauth.redirect", &[&provider.name()])?),
            linked: account.is_some(),
            email: account.and_then(|a| a.email.clone()),
            since: account.and_then(|a| a.created_at),
        });
    }
    // Accounts at a provider the app no longer offers can still be unlinked.
    for account in &linked {
        if !rows.iter().any(|row| row.name == account.provider) {
            let label = providers
                .0
                .iter()
                .find(|p| p.name() == account.provider)
                .map_or_else(|| account.provider.clone(), |p| p.label().to_owned());
            rows.push(Row {
                name: account.provider.clone(),
                label,
                link_url: None,
                linked: true,
                email: account.email.clone(),
                since: account.created_at,
            });
        }
    }
    Ok(Section {
        can_unlink: handlers::can_unlink(user, linked.len()),
        has_password: user.has_password(),
        set_password_url: state.url("password.request", &[]).ok(),
        providers: rows,
    })
}

/// Records `entry` when the app has the `Audit` module (its table exists).
async fn audit(state: &AppState, entry: renox::audit::Entry) -> Result {
    let has_log = renox::db::sql("SELECT COUNT(*) FROM audit_logs WHERE 1 = 0")
        .scalar::<i64>(&state.db)
        .await
        .is_ok();
    if has_log {
        renox::audit::record(&state.db, entry).await?;
    }
    Ok(())
}

/// Compiles the Rust in docs/oauth.md (the guide) as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/oauth.md")]
pub struct Guide;

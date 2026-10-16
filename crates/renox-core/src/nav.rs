//! Navigation links that modules and the app contribute to named menus
//! ([`NavLink`], [`Registry::nav`](crate::Registry::nav),
//! [`App::nav`](crate::App::nav)).

// The menu builder that reads the links comes with the next change.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::anyhow;

use crate::{AppState, AuthUser, Result};

/// Where a [`NavLink`] points.
#[derive(Clone)]
pub(crate) enum Target {
    /// A named route.
    Route(String),
    /// A fixed URL.
    Url(String),
}

pub(crate) type WhenFn = Arc<dyn Fn(&AuthUser) -> bool + Send + Sync>;

pub(crate) type BadgeFn = Arc<
    dyn Fn(AppState, AuthUser) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>>
        + Send
        + Sync,
>;

/// One link of a navigation menu. Build it with [`NavLink::route`] or
/// [`NavLink::url`], refine it with the other methods and add it to a menu
/// with [`App::nav`](crate::App::nav) or [`Registry::nav`](crate::Registry::nav).
///
/// ```
/// use renox::prelude::*;
/// use renox::nav::NavLink;
///
/// struct Billing;
///
/// impl Module for Billing {
///     fn name(&self) -> &'static str { "billing" }
///
///     fn register(&self, app: &mut Registry) {
///         app.nav(
///             "staff",
///             NavLink::route("invoices.index", "Invoices")
///                 .group("Billing")
///                 .order(10)
///                 .permission("invoices.view")
///                 .badge(|_state: AppState, _user: AuthUser| async move {
///                     Ok(Some("3".to_owned()))
///                 }),
///         );
///     }
/// }
/// ```
#[derive(Clone)]
pub struct NavLink {
    pub(crate) target: Target,
    pub(crate) label: String,
    pub(crate) icon: Option<String>,
    pub(crate) group: Option<String>,
    pub(crate) order: i32,
    pub(crate) key: String,
    pub(crate) active: Option<String>,
    pub(crate) permission: Option<String>,
    pub(crate) gate: Option<String>,
    pub(crate) when: Option<WhenFn>,
    pub(crate) badge: Option<BadgeFn>,
}

impl NavLink {
    fn new(target: Target, key: &str, label: &str) -> Self {
        Self {
            target,
            label: label.to_owned(),
            icon: None,
            group: None,
            order: 0,
            key: key.to_owned(),
            active: None,
            permission: None,
            gate: None,
            when: None,
            badge: None,
        }
    }

    /// A link to the named route; its key is the route name. `label` is a
    /// translation key or plain text.
    pub fn route(name: &str, label: &str) -> Self {
        Self::new(Target::Route(name.to_owned()), name, label)
    }

    /// A link to a fixed URL; its key is the URL unless [`NavLink::key`]
    /// gives another.
    pub fn url(url: &str, label: &str) -> Self {
        Self::new(Target::Url(url.to_owned()), url, label)
    }

    /// The icon shown next to the label.
    pub fn icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.to_owned());
        self
    }

    /// The heading the link is listed under; links without one come first.
    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_owned());
        self
    }

    /// Where the link sits among the others (lower first; default 0).
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Names the link for [`App::nav_hide`](crate::App::nav_hide) and for
    /// the duplicate check, instead of its route name or URL.
    pub fn key(mut self, key: &str) -> Self {
        self.key = key.to_owned();
        self
    }

    /// A route name pattern (`invoices.*`) that marks the link active,
    /// instead of its own route and the routes under its prefix.
    pub fn active(mut self, pattern: &str) -> Self {
        self.active = Some(pattern.to_owned());
        self
    }

    /// Shows the link only to users who hold `permission` globally or in at
    /// least one scope.
    pub fn permission(mut self, permission: &str) -> Self {
        self.permission = Some(permission.to_owned());
        self
    }

    /// Shows the link only to users the gate `gate` allows.
    pub fn gate(mut self, gate: &str) -> Self {
        self.gate = Some(gate.to_owned());
        self
    }

    /// Shows the link only to users for whom `check` returns true.
    pub fn when<F>(mut self, check: F) -> Self
    where
        F: Fn(&AuthUser) -> bool + Send + Sync + 'static,
    {
        self.when = Some(Arc::new(check));
        self
    }

    /// Computes a small text (a count) shown beside the label, per request
    /// and only when the menu is drawn and the link is visible. `None` shows
    /// no badge.
    pub fn badge<F, Fut>(mut self, badge: F) -> Self
    where
        F: Fn(AppState, AuthUser) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        self.badge = Some(Arc::new(move |state, user| Box::pin(badge(state, user))));
        self
    }
}

/// Every menu of the app, as collected at boot.
#[derive(Default)]
pub(crate) struct Nav {
    pub(crate) menus: HashMap<String, Vec<NavLink>>,
    pub(crate) hidden: HashSet<(String, String)>,
}

impl Nav {
    /// Groups the contributed links by menu, in the order they were added.
    /// Two links with one key in a menu are an error.
    pub(crate) fn build(
        links: Vec<(String, NavLink)>,
        hidden: Vec<(String, String)>,
    ) -> Result<Self> {
        let mut menus: HashMap<String, Vec<NavLink>> = HashMap::new();
        for (menu, link) in links {
            let entries = menus.entry(menu.clone()).or_default();
            if entries.iter().any(|other| other.key == link.key) {
                return Err(anyhow!(
                    "navigation menu `{menu}` has two links with the key `{}` \
                     (`NavLink::key` renames one)",
                    link.key
                )
                .into());
            }
            entries.push(link);
        }
        Ok(Self {
            menus,
            hidden: hidden.into_iter().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_default_to_the_route_or_url_and_can_be_renamed() {
        assert_eq!(NavLink::route("a.index", "A").key, "a.index");
        assert_eq!(NavLink::url("/x", "X").key, "/x");
        assert_eq!(NavLink::url("/x", "X").key("docs").key, "docs");
    }

    #[test]
    fn links_group_by_menu_and_duplicates_fail() {
        let nav = Nav::build(
            vec![
                ("staff".into(), NavLink::route("a", "A")),
                ("staff".into(), NavLink::route("b", "B")),
                ("portal".into(), NavLink::route("a", "A")),
            ],
            vec![("staff".into(), "b".into())],
        )
        .unwrap();
        assert_eq!(nav.menus["staff"].len(), 2);
        assert_eq!(nav.menus["portal"].len(), 1);
        assert!(nav.hidden.contains(&("staff".to_owned(), "b".to_owned())));

        let err = Nav::build(
            vec![
                ("staff".into(), NavLink::route("a", "A")),
                ("staff".into(), NavLink::route("a", "Again")),
            ],
            vec![],
        )
        .err()
        .unwrap();
        assert!(format!("{err:?}").contains("two links with the key `a`"));
    }
}

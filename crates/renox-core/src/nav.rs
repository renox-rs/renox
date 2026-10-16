//! Navigation links that modules and the app contribute to named menus
//! ([`NavLink`], [`Registry::nav`](crate::Registry::nav),
//! [`App::nav`](crate::App::nav)).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::anyhow;

use serde::Serialize;

use crate::i18n::Lang;
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

/// One link of a built menu, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MenuItem {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) url: String,
    pub(crate) icon: Option<String>,
    pub(crate) badge: Option<String>,
    pub(crate) active: bool,
}

/// A heading and its links; the heading is `None` for links without a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MenuGroup {
    pub(crate) label: Option<String>,
    pub(crate) items: Vec<MenuItem>,
}

/// What a menu is built for: the request it is drawn in.
pub(crate) struct MenuRequest<'a> {
    pub(crate) state: &'a AppState,
    /// `None` for an anonymous visitor.
    pub(crate) user: Option<&'a AuthUser>,
    pub(crate) locale: &'a str,
    /// The current route name, if the request matched a named route.
    pub(crate) route: Option<&'a str>,
    pub(crate) path: &'a str,
}

impl Nav {
    /// Builds the menu `menu` for one request: drops hidden links and the
    /// ones the user may not see (permission, gate, `when`, in that order),
    /// runs the badges of the rest, translates the labels, marks the active
    /// link and arranges groups by their lowest `order`. An unknown menu is
    /// empty. A badge that fails is logged and the link drawn without it.
    pub(crate) async fn menu(&self, menu: &str, request: &MenuRequest<'_>) -> Vec<MenuGroup> {
        let Some(links) = self.menus.get(menu) else {
            return Vec::new();
        };
        let lang = Lang::of(request.state, request.locale);
        let mut entries = Vec::new();
        for link in links {
            if self.hidden.contains(&(menu.to_owned(), link.key.clone())) {
                continue;
            }
            if !visible(link, request).await {
                continue;
            }
            let url = match &link.target {
                Target::Url(url) => url.clone(),
                Target::Route(name) => match request.state.routes.url(name, &[]) {
                    Ok(url) => url,
                    Err(err) => {
                        tracing::warn!(menu, key = %link.key, "navigation link skipped: {err}");
                        continue;
                    }
                },
            };
            let badge = match (&link.badge, request.user) {
                (Some(badge), Some(user)) => {
                    match badge(request.state.clone(), user.clone()).await {
                        Ok(badge) => badge.filter(|text| !text.is_empty()),
                        Err(err) => {
                            tracing::warn!(menu, key = %link.key, "navigation badge failed: {err}");
                            None
                        }
                    }
                }
                _ => None,
            };
            let item = MenuItem {
                key: link.key.clone(),
                label: lang.t(&link.label, &[]),
                url,
                icon: link.icon.clone(),
                badge,
                active: is_active(link, request.route, request.path),
            };
            entries.push((link.group.clone(), link.order, item));
        }
        arrange(entries, |group| lang.t(group, &[]))
    }
}

/// The menus a set of templates asks for.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MenusUsed {
    /// Menu names written as text (`nav("staff")`, `menu="staff"`).
    pub(crate) names: Vec<String>,
    /// A menu name that is not text: any menu may be needed.
    pub(crate) dynamic: bool,
}

impl MenusUsed {
    /// Adds what `source` (one template) asks for: `nav("…")` calls and
    /// `menu="…"` props.
    pub(crate) fn scan(&mut self, source: &str) {
        let bytes = source.as_bytes();
        for (word, call) in [("nav", true), ("menu", false)] {
            let mut from = 0;
            while let Some(at) = source[from..].find(word) {
                let start = from + at;
                let end = start + word.len();
                from = end;
                let before = source[..start].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                let mut i = end;
                let skip = |i: &mut usize| {
                    while bytes.get(*i).is_some_and(u8::is_ascii_whitespace) {
                        *i += 1;
                    }
                };
                skip(&mut i);
                let punct = if call { b'(' } else { b'=' };
                if bytes.get(i) != Some(&punct) {
                    continue;
                }
                i += 1;
                skip(&mut i);
                match bytes.get(i) {
                    Some(&quote @ (b'"' | b'\'')) => {
                        let rest = &source[i + 1..];
                        match rest.find(quote as char) {
                            Some(len) => {
                                let name = rest[..len].to_owned();
                                if !self.names.contains(&name) {
                                    self.names.push(name);
                                }
                            }
                            None => self.dynamic = true,
                        }
                    }
                    _ => self.dynamic = true,
                }
            }
        }
    }
}

/// Builds the menus a request's templates ask for (all of them when
/// `used.dynamic`), as the map `nav(name)` reads.
pub(crate) async fn build_for_request(
    state: &AppState,
    user: Option<&AuthUser>,
    locale: &str,
    route: Option<&str>,
    path: &str,
    used: &MenusUsed,
) -> HashMap<String, Vec<MenuGroup>> {
    let mut built = HashMap::new();
    let request = MenuRequest {
        state,
        user,
        locale,
        route,
        path,
    };
    for name in state.nav.menus.keys() {
        if used.dynamic || used.names.iter().any(|n| n == name) {
            built.insert(name.clone(), state.nav.menu(name, &request).await);
        }
    }
    built
}

/// Whether the request's user may see `link`.
async fn visible(link: &NavLink, request: &MenuRequest<'_>) -> bool {
    let restricted = link.permission.is_some() || link.gate.is_some() || link.when.is_some();
    let Some(user) = request.user else {
        return !restricted;
    };
    if let Some(permission) = &link.permission {
        // Without the Permissions module a permission name falls back to the
        // gates, where an unknown name denies.
        if !user.has_permission_anywhere(permission) && !gate_allows(user, permission).await {
            return false;
        }
    }
    if let Some(gate) = &link.gate
        && !gate_allows(user, gate).await
    {
        return false;
    }
    link.when.as_ref().is_none_or(|check| check(user))
}

async fn gate_allows(user: &AuthUser, gate: &str) -> bool {
    match user.allows_async(gate).await {
        Ok(allowed) => allowed,
        Err(err) => {
            tracing::warn!(gate, "navigation gate failed: {err}");
            false
        }
    }
}

/// Whether `link` is the current page: a route link matches its own route
/// and the routes under its prefix (`invoices.index` also matches
/// `invoices.show`), `NavLink::active` replaces that pattern, and a URL link
/// matches the exact path.
fn is_active(link: &NavLink, route: Option<&str>, path: &str) -> bool {
    if let Some(pattern) = &link.active {
        return route.is_some_and(|name| crate::routing::route_name_matches(name, pattern));
    }
    match &link.target {
        Target::Url(url) => url == path,
        Target::Route(own) => route.is_some_and(|name| {
            name == own
                || own
                    .rsplit_once('.')
                    .is_some_and(|(prefix, _)| name.starts_with(&format!("{prefix}.")))
        }),
    }
}

/// Splits entries into groups: ungrouped links first, then the groups by
/// their lowest `order` (ties keep the order the groups first appeared in);
/// links inside a group by `order`, ties in insertion order.
fn arrange(
    entries: Vec<(Option<String>, i32, MenuItem)>,
    label: impl Fn(&str) -> String,
) -> Vec<MenuGroup> {
    type Bucket = (Option<String>, i32, Vec<(i32, MenuItem)>);
    let mut groups: Vec<Bucket> = Vec::new();
    for (group, order, item) in entries {
        match groups.iter_mut().find(|(name, ..)| *name == group) {
            Some((_, lowest, items)) => {
                *lowest = (*lowest).min(order);
                items.push((order, item));
            }
            None => groups.push((group, order, vec![(order, item)])),
        }
    }
    // Stable sorts keep insertion order on ties; `false < true` puts the
    // ungrouped links first.
    groups.sort_by_key(|(name, lowest, _)| (name.is_some(), *lowest));
    groups
        .into_iter()
        .map(|(name, _, mut items)| {
            items.sort_by_key(|(order, _)| *order);
            MenuGroup {
                label: name.map(|name| label(&name)),
                items: items.into_iter().map(|(_, item)| item).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str) -> MenuItem {
        MenuItem {
            key: key.into(),
            label: key.into(),
            url: format!("/{key}"),
            icon: None,
            badge: None,
            active: false,
        }
    }

    #[test]
    fn groups_follow_their_lowest_order_and_ungrouped_links_come_first() {
        let entries = vec![
            (Some("Sales".to_owned()), 30, item("orders")),
            (Some("Finance".to_owned()), 20, item("invoices")),
            (None, 90, item("home")),
            (Some("Sales".to_owned()), 5, item("quotes")),
            (Some("Finance".to_owned()), 10, item("payments")),
        ];
        let groups = arrange(entries, |name| name.to_uppercase());
        let shape: Vec<(Option<&str>, Vec<&str>)> = groups
            .iter()
            .map(|g| {
                (
                    g.label.as_deref(),
                    g.items.iter().map(|i| i.key.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (None, vec!["home"]),
                (Some("SALES"), vec!["quotes", "orders"]),
                (Some("FINANCE"), vec!["payments", "invoices"]),
            ]
        );
    }

    #[test]
    fn scan_finds_the_menus_a_template_names() {
        let mut used = MenusUsed::default();
        used.scan(
            r#"{% for g in nav("staff") %}{% endfor %}{{ nav( 'portal' ) }} <x menu="side"/>"#,
        );
        used.scan("{{ subnav('no') }} {{ x.nav('no') }} {{ submenu='no' }} {{ menu }}");
        assert_eq!(used.names, vec!["staff", "portal", "side"]);
        assert!(!used.dynamic);
        used.scan("{{ nav(which) }}");
        assert!(used.dynamic);
    }

    #[test]
    fn active_follows_the_route_prefix_pattern_or_exact_path() {
        let index = NavLink::route("invoices.index", "Invoices");
        assert!(is_active(&index, Some("invoices.index"), "/invoices"));
        assert!(is_active(&index, Some("invoices.show"), "/invoices/1"));
        assert!(!is_active(&index, Some("orders.index"), "/orders"));
        assert!(!is_active(&index, None, "/"));
        let flat = NavLink::route("home", "Home");
        assert!(is_active(&flat, Some("home"), "/"));
        assert!(!is_active(&flat, Some("home.other"), "/"));
        let custom = NavLink::route("a.index", "A").active("b.*");
        assert!(is_active(&custom, Some("b.show"), "/b/1"));
        assert!(!is_active(&custom, Some("a.index"), "/a"));
        let url = NavLink::url("/docs", "Docs");
        assert!(is_active(&url, None, "/docs"));
        assert!(!is_active(&url, None, "/docs/x"));
    }

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

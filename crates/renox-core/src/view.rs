use std::path::{Component as PathPart, Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use minijinja::value::{Rest, merge_maps};
use minijinja::{Environment, ErrorKind, Value, context};
use minijinja_autoreload::AutoReloader;
use serde::Serialize;

use crate::auth::CurrentUser;
use crate::error::{ErrorPage, reason};
use crate::storage::Storage;
use crate::validation::ValidationError;
use crate::{AppState, Config, Error, Htmx, RouteTable, Session, assets};

/// Templates that ship with Renox. An app overrides one by creating a file
/// with the same name in its views directory.
pub(crate) const BUILTIN: &[(&str, &str)] = &[
    ("renox/error.html", include_str!("../views/error.html")),
    (
        "renox/pagination.html",
        include_str!("../views/pagination.html"),
    ),
    (
        "renox/auth/layout.html",
        include_str!("../views/auth/layout.html"),
    ),
    (
        "renox/auth/login.html",
        include_str!("../views/auth/login.html"),
    ),
    (
        "renox/auth/register.html",
        include_str!("../views/auth/register.html"),
    ),
    (
        "renox/auth/forgot-password.html",
        include_str!("../views/auth/forgot-password.html"),
    ),
    (
        "renox/auth/reset-password.html",
        include_str!("../views/auth/reset-password.html"),
    ),
    (
        "renox/auth/verify-email.html",
        include_str!("../views/auth/verify-email.html"),
    ),
    (
        "renox/mail/layout.html",
        include_str!("../views/mail/layout.html"),
    ),
    (
        "renox/mail/button.html",
        include_str!("../views/mail/button.html"),
    ),
    ("renox/live.html", include_str!("../views/live.html")),
    ("renox/ui.html", include_str!("../views/ui.html")),
    (
        "renox/import_report.html",
        include_str!("../views/import_report.html"),
    ),
    ("renox/grid.html", include_str!("../views/grid.html")),
    (
        "renox/grid_print.html",
        include_str!("../views/grid_print.html"),
    ),
    ("renox/debug.html", include_str!("../views/debug.html")),
    (
        "renox/transition_actions.html",
        include_str!("../views/transition_actions.html"),
    ),
    (
        "renox/notifications.html",
        include_str!("../views/notifications.html"),
    ),
    (
        "renox/queue/dashboard.html",
        include_str!("../views/queue/dashboard.html"),
    ),
    (
        "renox/mail/components.html",
        include_str!("../views/mail/components.html"),
    ),
    (
        "renox/auth/account.html",
        include_str!("../views/auth/account.html"),
    ),
    (
        "renox/auth/account_sections.html",
        include_str!("../views/auth/account_sections.html"),
    ),
    (
        "renox/auth/confirm-password.html",
        include_str!("../views/auth/confirm-password.html"),
    ),
    (
        "renox/mail/auth/reset-password.html",
        include_str!("../views/mail/auth/reset-password.html"),
    ),
    (
        "renox/mail/auth/reset-password.txt",
        include_str!("../views/mail/auth/reset-password.txt"),
    ),
    (
        "renox/mail/auth/verify-email.html",
        include_str!("../views/mail/auth/verify-email.html"),
    ),
    (
        "renox/mail/auth/verify-email.txt",
        include_str!("../views/mail/auth/verify-email.txt"),
    ),
    (
        "renox/auth/login-link.html",
        include_str!("../views/auth/login-link.html"),
    ),
    (
        "renox/auth/login-link-confirm.html",
        include_str!("../views/auth/login-link-confirm.html"),
    ),
    (
        "renox/auth/login-code.html",
        include_str!("../views/auth/login-code.html"),
    ),
    (
        "renox/auth/login-code-verify.html",
        include_str!("../views/auth/login-code-verify.html"),
    ),
    (
        "renox/mail/auth/login-link.html",
        include_str!("../views/mail/auth/login-link.html"),
    ),
    (
        "renox/mail/auth/login-link.txt",
        include_str!("../views/mail/auth/login-link.txt"),
    ),
    (
        "renox/mail/auth/login-code.html",
        include_str!("../views/mail/auth/login-code.html"),
    ),
    (
        "renox/mail/auth/login-code.txt",
        include_str!("../views/mail/auth/login-code.txt"),
    ),
];

/// The template engine (MiniJinja), reading from `VIEWS_PATH`.
///
/// Templates are reloaded when they change while `APP_DEBUG` is on.
#[derive(Clone)]
pub struct Views {
    reloader: Arc<AutoReloader>,
    /// `APP_DEBUG`: also warn about values hidden by imported macros.
    debug: bool,
    /// `VIEWS_PATH`, where the app's templates live.
    dir: PathBuf,
    /// Templates compiled into the binary, used instead of `dir` when given.
    embedded: Option<&'static [(&'static str, &'static str)]>,
    /// The plugins' components, next to the built-in ones.
    plugin_components: Arc<Vec<crate::components::Contract>>,
    /// The page structs registered with `App::view_data`, checked by `view:check`.
    typed: Arc<Vec<TypedView>>,
    /// The keys of `App::share` / `Registry::share` values, which every view gets.
    shared: Arc<Vec<String>>,
}

/// A page struct registered with `App::view_data`: its template and fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedView {
    /// The struct's name, without its module path.
    pub name: String,
    /// `ViewData::TEMPLATE`.
    pub template: &'static str,
    /// `ViewData::FIELDS`.
    pub fields: &'static [&'static str],
}

impl TypedView {
    pub(crate) fn of<T: ViewData>() -> Self {
        Self {
            name: short_type_name(std::any::type_name::<T>()),
            template: T::TEMPLATE,
            fields: T::FIELDS,
        }
    }
}

/// `app::pages::Suppliers<'a>` → `Suppliers<'a>`: the last path segment, generics kept.
fn short_type_name(full: &str) -> String {
    let base = full.split('<').next().unwrap_or(full);
    let start = base.rfind("::").map_or(0, |i| i + 2);
    full[start..].to_owned()
}

/// The data of one page: a struct whose fields are its template's variables, so the
/// compiler checks the handler's side and `view:check` the template's (the variables it
/// uses that the struct lacks). Derive it with `#[derive(ViewData)]` and
/// `#[view(template = "…")]`, render it with [`View::typed`], and register it with
/// `App::view_data::<T>()` (`Registry::view_data` in a module) so `view:check` knows it.
///
/// ```
/// # use renox::prelude::*;
/// #[derive(serde::Serialize, ViewData)]
/// #[view(template = "suppliers/index.html")]
/// struct SupplierList {
///     suppliers: Vec<String>,
///     #[serde(rename = "total")]
///     count: usize,
/// }
///
/// assert_eq!(SupplierList::TEMPLATE, "suppliers/index.html");
/// assert_eq!(SupplierList::FIELDS, ["suppliers", "total"]);
///
/// async fn index() -> View {
///     View::typed(SupplierList { suppliers: vec!["Velo".into()], count: 1 })
/// }
/// let app = App::new().view_data::<SupplierList>();
/// ```
///
/// The check reads what MiniJinja loads (after the `rx-*` components are compiled) and
/// follows neither `extends` nor `include` nor imported macros: a variable used only in
/// the layout or a partial isn't checked. Besides the struct's fields, a template may
/// use the request globals (`auth`, `request`, `t`, `old`…), the environment's
/// functions and globals, the `App::share` keys and the names it defines itself.
pub trait ViewData: Serialize {
    /// The template the page renders, relative to `VIEWS_PATH`.
    const TEMPLATE: &'static str;
    /// The names the template sees: the fields as serialized (`#[serde(rename)]` applied,
    /// skipped fields left out).
    const FIELDS: &'static [&'static str];
}

/// One template that does not compile, found by [`Views::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewProblem {
    /// The template's name, e.g. `products/index.html`.
    pub template: String,
    /// The line the error is on, when the compiler knows it.
    pub line: Option<usize>,
    /// What is wrong.
    pub message: String,
}

impl std::fmt::Display for ViewProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.template, self.message),
            None => write!(f, "{}: {}", self.template, self.message),
        }
    }
}

/// Every `*.html` and `*.txt` file under `dir`, as names relative to it with
/// `/` separators. Dotfiles and dot directories are skipped.
fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') {
            continue;
        }
        let name = if prefix.is_empty() {
            file_name
        } else {
            format!("{prefix}/{file_name}")
        };
        let path = entry.path();
        if path.is_dir() {
            walk(&path, &name, out);
        } else if name.ends_with(".html") || name.ends_with(".txt") {
            out.push(name);
        }
    }
}

/// What an `App::share` function knows about the request being rendered.
#[non_exhaustive]
#[derive(Clone)]
pub struct ViewContext {
    /// The application state.
    pub state: AppState,
    /// The logged-in user, if any.
    pub user: Option<Arc<crate::auth::User>>,
    /// The request's language, e.g. `en`.
    pub locale: String,
    /// The request's path, e.g. `/products`.
    pub path: String,
    /// The request's session (absent only where no session middleware ran),
    /// e.g. a guest's cart kept there:
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().share("cart_count", |ctx: renox::view::ViewContext| async move {
    ///     let cart: Vec<i64> = ctx.session.and_then(|s| s.get("cart")).unwrap_or_default();
    ///     Ok(cart.len())
    /// })
    /// # ;
    /// ```
    pub session: Option<crate::Session>,
}

pub(crate) type ShareFn = Arc<
    dyn Fn(
            ViewContext,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<Value>> + Send>>
        + Send
        + Sync,
>;

pub(crate) fn share_fn<F, Fut, T>(compute: F) -> ShareFn
where
    F: Fn(ViewContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = crate::Result<T>> + Send + 'static,
    T: Serialize,
{
    let compute = Arc::new(compute);
    Arc::new(move |ctx| {
        let compute = compute.clone();
        Box::pin(async move { Ok(Value::from_serialize(compute(ctx).await?)) })
    })
}

/// The names `{% from … import … %}` and `{% import … as … %}` bind in
/// `source` that are also keys of one of `values` (the handler's context, the
/// shared values): inside the template those names are the macros.
fn hidden_by_imports(source: &str, values: &[&Value]) -> Vec<String> {
    static FROM: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r#"(?s)\{%[-+]?\s*from\s+(?:"[^"]*"|'[^']*'|\S+)\s+import\s+(.*?)\s*[-+]?%\}"#,
        )
        .expect("valid regex")
    });
    static IMPORT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"\{%[-+]?\s*import\s+(?:"[^"]*"|'[^']*'|\S+)\s+as\s+(\w+)"#)
            .expect("valid regex")
    });
    let mut names: Vec<String> = Vec::new();
    for found in FROM.captures_iter(source) {
        let list = found[1]
            .trim_end_matches("without context")
            .trim_end_matches("with context");
        for item in list.split(',') {
            let name = item.rsplit(" as ").next().unwrap_or(item).trim();
            if !name.is_empty() {
                names.push(name.to_owned());
            }
        }
    }
    names.extend(
        IMPORT
            .captures_iter(source)
            .map(|found| found[1].to_owned()),
    );
    names.retain(|name| {
        values
            .iter()
            .any(|value| value.get_attr(name).is_ok_and(|v| !v.is_undefined()))
    });
    names.sort();
    names.dedup();
    names
}

/// Adds functions, filters or globals to the template environment; see
/// `App::templates`.
pub(crate) type TemplateHook = Arc<dyn Fn(&mut Environment<'static>) + Send + Sync>;

thread_local! {
    /// The components known while template hooks run, for [`add_template`].
    static HOOK_COMPONENTS: std::cell::RefCell<Option<Arc<Vec<crate::components::Contract>>>> =
        const { std::cell::RefCell::new(None) };
}

/// Makes the registered components visible to [`add_template`] until dropped.
struct HookScope(Option<Arc<Vec<crate::components::Contract>>>);

impl HookScope {
    fn enter(contracts: Arc<Vec<crate::components::Contract>>) -> Self {
        Self(HOOK_COMPONENTS.with(|c| c.borrow_mut().replace(contracts)))
    }
}

impl Drop for HookScope {
    fn drop(&mut self) {
        let previous = self.0.take();
        HOOK_COMPONENTS.with(|c| *c.borrow_mut() = previous);
    }
}

pub use crate::components::{Component, Prop};

/// Adds the template `name` to `env` after compiling its component tags
/// (`<rx-…>`, `rx-if`, `rx-for`), as the app's own views are. For modules and
/// plugins, inside [`Registry::templates`](crate::Registry::templates), where
/// the components other modules registered are known too. A mistake in the
/// source is an error naming the template and line.
///
/// ```
/// use renox::view::add_template;
///
/// let mut env = minijinja::Environment::new();
/// add_template(&mut env, "hi.html", "<p>{{ name }}</p>").unwrap();
/// ```
pub fn add_template(
    env: &mut Environment<'static>,
    name: impl Into<String>,
    source: impl Into<String>,
) -> Result<(), minijinja::Error> {
    let name = name.into();
    let source = source.into();
    let contracts = HOOK_COMPONENTS
        .with(|c| c.borrow().clone())
        .unwrap_or_else(|| Arc::new(crate::components::BUILTIN.to_vec()));
    let catalog = crate::components::Catalog {
        contracts: &contracts,
        lookup: &|_| None,
    };
    let compiled = crate::components::compile(&name, &source, &catalog).map_err(|e| {
        minijinja::Error::new(
            ErrorKind::SyntaxError,
            format!("{name}:{}: {}", e.line, e.message),
        )
    })?;
    env.add_template_owned(name, compiled)
}

impl Views {
    /// `embedded`: templates compiled into the binary, used instead of
    /// `VIEWS_PATH` when given.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        config: &Config,
        routes: Arc<RouteTable>,
        storage: Storage,
        embedded: Option<&'static [(&'static str, &'static str)]>,
        hooks: Arc<Vec<TemplateHook>>,
        components: Arc<Vec<crate::components::Contract>>,
        zone: crate::timezone::Zone,
        versions: Arc<crate::embedded::AssetVersions>,
    ) -> Self {
        let plugin_components = components.clone();
        let dir = config.views_path.clone();
        let watch = config.debug && embedded.is_none() && dir.is_dir();
        let debug = config.debug;
        let currency = config.currency.clone();
        let reloader = AutoReloader::new(move |notifier| {
            let mut env = Environment::new();
            env.set_formatter(format_value);
            // While developing, printing a misspelled variable is an error
            // instead of an empty string (`{% if x %}` on a missing one is fine).
            if debug {
                env.set_undefined_behavior(minijinja::UndefinedBehavior::SemiStrict);
            }
            let loader_dir = dir.clone();
            let contracts: Arc<Vec<crate::components::Contract>> = Arc::new(
                crate::components::BUILTIN
                    .iter()
                    .chain(components.iter())
                    .copied()
                    .collect(),
            );
            let loader_contracts = contracts.clone();
            env.set_loader(move |name| {
                let Some(src) = load(&loader_dir, embedded, name)? else {
                    return Ok(None);
                };
                let lookup = |n: &str| load(&loader_dir, embedded, n).ok().flatten();
                let catalog = crate::components::Catalog {
                    contracts: &loader_contracts,
                    lookup: &lookup,
                };
                crate::components::compile(name, &src, &catalog)
                    .map(Some)
                    .map_err(|e| {
                        minijinja::Error::new(
                            ErrorKind::SyntaxError,
                            format!("{name}:{}: {}", e.line, e.message),
                        )
                    })
            });

            let routes = routes.clone();
            env.add_function(
                "route",
                move |name: String, params: Rest<Value>| -> Result<Value, minijinja::Error> {
                    // `route('products.index', page=2)`: named arguments are
                    // the query string (`?page=2`).
                    let (query, params): (Vec<&Value>, Vec<&Value>) =
                        params.iter().partition(|p| p.is_kwargs());
                    let params: Vec<&dyn std::fmt::Display> = params
                        .iter()
                        .map(|p| *p as &dyn std::fmt::Display)
                        .collect();
                    // Percent-encoded, so safe to use in HTML without escaping `/`.
                    let mut url = routes.url(&name, &params).map_err(|err| {
                        minijinja::Error::new(ErrorKind::InvalidOperation, err.to_string())
                    })?;
                    if let Some(kwargs) = query.first() {
                        let mut pairs = form_urlencoded::Serializer::new(String::new());
                        for key in kwargs.try_iter()? {
                            let value = kwargs.get_item(&key)?;
                            if value.is_none() || value.is_undefined() {
                                continue;
                            }
                            pairs.append_pair(&key.to_string(), &value.to_string());
                        }
                        let pairs = pairs.finish();
                        if !pairs.is_empty() {
                            url.push(if url.contains('?') { '&' } else { '?' });
                            url.push_str(&pairs.replace('&', "&amp;"));
                        }
                    }
                    Ok(Value::from_safe_string(url))
                },
            );
            // The current URL's query string with `page` set to `page`, for
            // pagination links that keep filters such as `?q=coffee`.
            env.add_function("page_url", |state: &minijinja::State, page: u32| -> Value {
                let query = state
                    .lookup("request")
                    .and_then(|request| request.get_attr("query").ok())
                    .and_then(|query| query.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let mut url = form_urlencoded::Serializer::new(String::new());
                for (key, value) in form_urlencoded::parse(query.as_bytes()) {
                    if key != "page" {
                        url.append_pair(&key, &value);
                    }
                }
                url.append_pair("page", &page.to_string());
                Value::from(format!("?{}", url.finish()))
            });
            // The current URL's query string with some keys set (or removed
            // with `none`), and `page` dropped: `query_with(period="7d")`
            // for filters that keep the others.
            env.add_function(
                "query_with",
                |state: &minijinja::State,
                 kwargs: minijinja::value::Kwargs|
                 -> Result<Value, minijinja::Error> {
                    let query = state
                        .lookup("request")
                        .and_then(|request| request.get_attr("query").ok())
                        .and_then(|query| query.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    let keys: Vec<String> = kwargs.args().map(str::to_owned).collect();
                    let mut url = form_urlencoded::Serializer::new(String::new());
                    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
                        if key != "page" && !keys.iter().any(|k| *k == key) {
                            url.append_pair(&key, &value);
                        }
                    }
                    for key in &keys {
                        let value: Value = kwargs.get(key)?;
                        if !value.is_none() && !value.is_undefined() {
                            url.append_pair(key, &value.to_string());
                        }
                    }
                    kwargs.assert_all_used()?;
                    Ok(Value::from(format!("?{}", url.finish())))
                },
            );
            // The current URL's query string as hidden inputs, without `page`
            // and the keys named: `query_fields("period", "from", "to")`, so
            // a GET form keeps the page's other filters.
            env.add_function(
                "query_fields",
                |state: &minijinja::State, except: minijinja::value::Rest<String>| -> Value {
                    let query = state
                        .lookup("request")
                        .and_then(|request| request.get_attr("query").ok())
                        .and_then(|query| query.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    let mut html = String::new();
                    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
                        if key != "page" && !except.iter().any(|k| *k == key) {
                            html.push_str(&format!(
                                r#"<input type="hidden" name="{}" value="{}">"#,
                                crate::toast::escape(&key),
                                crate::toast::escape(&value)
                            ));
                        }
                    }
                    Value::from_safe_string(html)
                },
            );
            env.add_function("method_field", |method: String| {
                let method: String = method.chars().filter(char::is_ascii_alphabetic).collect();
                Value::from_safe_string(format!(
                    "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
                    crate::method::METHOD_FIELD,
                    method.to_ascii_uppercase()
                ))
            });
            // `/app.css?v=1a2b3c4d`: a new URL whenever the file changes.
            let versions = versions.clone();
            env.add_function("asset", move |path: String| {
                let mut url = String::from("/");
                crate::routing::encode(&mut url, path.trim_start_matches('/'), true);
                if let Some(version) = versions.version(&path) {
                    url.push_str("?v=");
                    url.push_str(&version);
                }
                Value::from_safe_string(url)
            });
            let storage = storage.clone();
            env.add_function("storage_url", move |key: String| {
                Value::from_safe_string(storage.url(&key))
            });
            // The request's values for components: `old`, `error`, `t`, `can`,
            // `csrf_field`, `auth`, `request`, `flash`… work inside imported
            // macros too, not only in the rendered template.
            for name in REQUEST_GLOBALS {
                env.add_global(*name, Value::from_object(RequestGlobal(name)));
            }
            crate::view_stack::register(&mut env);
            env.add_function("renox_ui", |kwargs: minijinja::value::Kwargs| {
                let styles: Option<bool> = kwargs.get("styles")?;
                kwargs.assert_all_used()?;
                Ok::<_, minijinja::Error>(Value::from_safe_string(crate::assets::ui_tags(
                    styles.unwrap_or(true),
                )))
            });
            // The request's brand stylesheet (`App::theme`, `App::theme_for`):
            // a `<link>` to the hashed route (`/_renox/theme/{hash}.css`, so
            // nothing to escape), and nothing without a theme.
            env.add_function("renox_theme", |state: &minijinja::State| {
                let url = state
                    .lookup(THEME_KEY)
                    .and_then(|v| v.as_str().map(str::to_owned));
                Value::from_safe_string(match url {
                    Some(url) => format!("<link rel=\"stylesheet\" href=\"{url}\">"),
                    None => String::new(),
                })
            });
            env.add_function("renox_calendar", || {
                Value::from_safe_string(crate::assets::calendar_tags())
            });
            // Until the live component script ships (#435): nothing.
            env.add_function("renox_live", || {
                Value::from_safe_string(crate::assets::live_tags())
            });
            // The kit's `icon(…)` macro: a Lucide icon as inline SVG.
            env.add_function(
                "renox_icon",
                |name: String, kwargs: minijinja::value::Kwargs| {
                    let size: Option<u32> = kwargs.get("size")?;
                    let label: Option<String> = kwargs.get("label")?;
                    let class: Option<String> = kwargs.get("class")?;
                    kwargs.assert_all_used()?;
                    Ok::<_, minijinja::Error>(Value::from_safe_string(crate::icons::svg(
                        &name,
                        size.unwrap_or(20),
                        label.as_deref(),
                        class.as_deref(),
                    )))
                },
            );
            env.add_function("renox_grid", || {
                Value::from_safe_string(crate::assets::grid_tags())
            });
            env.add_function("sparkline", crate::view_filters::sparkline);
            env.add_filter("number", crate::view_filters::number);
            env.add_filter("date", crate::view_filters::date(zone));
            env.add_filter("since", crate::view_filters::since(zone));
            env.add_filter("money", crate::view_filters::money(currency.clone()));
            env.add_filter("words", crate::view_filters::words);
            env.add_filter("markdown", crate::view_filters::markdown);
            env.add_function("chart", crate::chart::chart(currency.clone()));
            env.add_function("class_names", crate::view_filters::class_names);
            env.add_function("money_input", {
                let app_currency = currency.clone();
                move |code: Option<String>| {
                    crate::view_filters::money_input(code.as_deref().unwrap_or(&app_currency))
                }
            });
            env.add_function("rx_attr", crate::view_filters::rx_attr);
            env.add_function("rx_attr_value", crate::view_filters::attr_value);
            // The app's own functions and filters (`App::templates`).
            let _scope = HookScope::enter(contracts.clone());
            for hook in hooks.iter() {
                hook(&mut env);
            }

            if watch {
                notifier.watch_path(&dir, true);
            }
            Ok(env)
        });
        Self {
            reloader: Arc::new(reloader),
            debug,
            dir: config.views_path.clone(),
            embedded,
            plugin_components,
            typed: Arc::new(Vec::new()),
            shared: Arc::new(Vec::new()),
        }
    }

    /// The name of every template: the app's files (or the embedded ones),
    /// the built-in ones and those added to the environment, sorted.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut names = Vec::new();
        match self.embedded {
            Some(files) => names.extend(files.iter().map(|(name, _)| (*name).to_owned())),
            None => walk(&self.dir, "", &mut names),
        }
        names.extend(BUILTIN.iter().map(|(name, _)| (*name).to_owned()));
        if let Ok(env) = self.reloader.acquire_env() {
            names.extend(env.templates().map(|(name, _)| name.to_owned()));
        }
        names.sort();
        names.dedup();
        names
    }

    /// The editor data file (VS Code's `html.customData`): the built-in and plugin components
    /// and the app's own (`components/*.html` files that declare `<rx-props>`).
    pub(crate) fn custom_data(&self) -> serde_json::Value {
        let contracts: Vec<crate::components::Contract> = crate::components::BUILTIN
            .iter()
            .chain(self.plugin_components.iter())
            .copied()
            .collect();
        let sources: Vec<(String, String)> = self
            .names()
            .into_iter()
            .filter(|n| n.starts_with("components/") && n.ends_with(".html"))
            .filter_map(|n| {
                let src = load(&self.dir, self.embedded, &n).ok().flatten()?;
                src.contains("<rx-props").then_some((n, src))
            })
            .collect();
        crate::components::custom_data_for(&contracts, &sources)
    }

    /// Compiles every template and returns those that fail, in name order.
    pub(crate) fn check(&self, routes: &RouteTable) -> Vec<ViewProblem> {
        let names = self.names();
        let env = match self.reloader.acquire_env() {
            Ok(env) => env,
            Err(err) => {
                return vec![ViewProblem {
                    template: String::new(),
                    line: None,
                    message: err.to_string(),
                }];
            }
        };
        let mut problems = Vec::new();
        let known: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
        for name in &names {
            match env.get_template(name) {
                Err(err) => problems.push(problem(name, &err)),
                // The framework's own templates show example names in their docs.
                Ok(template) if !name.starts_with("renox/") => {
                    check_route_names(name, template.source(), routes, &mut problems);
                    check_includes(name, template.source(), &known, &mut problems);
                }
                Ok(_) => {}
            }
        }
        for typed in self.typed.iter() {
            self.check_typed(&env, typed, &mut problems);
        }
        problems
    }

    /// Records the page structs (`App::view_data`) and the shared keys, for [`Views::check`].
    pub(crate) fn with_typed(mut self, typed: Vec<TypedView>, shared: Vec<String>) -> Self {
        self.typed = Arc::new(typed);
        self.shared = Arc::new(shared);
        self
    }

    /// Reports the variables `typed`'s template uses that its struct doesn't have. The
    /// template is the one MiniJinja loads, after the `rx-*` components are compiled.
    fn check_typed(
        &self,
        env: &minijinja::Environment<'_>,
        typed: &TypedView,
        problems: &mut Vec<ViewProblem>,
    ) {
        let template = match env.get_template(typed.template) {
            Ok(template) => template,
            // A template that doesn't compile is already reported.
            Err(err) if err.kind() != ErrorKind::TemplateNotFound => return,
            Err(_) => {
                problems.push(ViewProblem {
                    template: typed.template.to_owned(),
                    line: None,
                    message: format!("not found, but {} renders it", typed.name),
                });
                return;
            }
        };
        let globals: std::collections::HashSet<&str> =
            env.globals().map(|(name, _)| name).collect();
        let mut unknown: Vec<String> = template
            .undeclared_variables(false)
            .into_iter()
            .filter(|var| {
                !typed.fields.contains(&var.as_str())
                    && !REQUEST_GLOBALS.contains(&var.as_str())
                    && !globals.contains(var.as_str())
                    && !self.shared.iter().any(|key| key == var)
                    // Names the component compiler makes up (`__rx_slot_1`).
                    && !var.starts_with("__rx")
            })
            .collect();
        unknown.sort();
        for var in unknown {
            problems.push(ViewProblem {
                template: typed.template.to_owned(),
                line: first_use(template.source(), &var),
                message: format!("\"{var}\" isn't a field of {}", typed.name),
            });
        }
    }

    /// The navigation menus the template `name` and the templates it reaches
    /// (layouts, includes, imports) ask for with `nav("…")` or `menu="…"`.
    /// Renox's own templates are not read: they get the menu name from the
    /// caller.
    pub(crate) fn menus_used(&self, name: &str) -> crate::nav::MenusUsed {
        let mut used = crate::nav::MenusUsed::default();
        let Ok(env) = self.reloader.acquire_env() else {
            return used;
        };
        let mut seen = std::collections::HashSet::new();
        let mut queue = vec![name.to_owned()];
        while let Some(next) = queue.pop() {
            if !seen.insert(next.clone()) || BUILTIN.iter().any(|(n, _)| *n == next) {
                continue;
            }
            let Ok(template) = env.get_template(&next) else {
                continue;
            };
            let source = template.source();
            used.scan(source);
            // Every quoted `….html` name is a layout, an include or an
            // import (or text that happens to match). Found from the end of
            // the name, so an apostrophe in a comment cannot pair the quotes
            // wrongly.
            let mut from = 0;
            while let Some(at) = source[from..].find(".html") {
                let end = from + at + ".html".len();
                from = end;
                let Some(quote) = source[end..]
                    .chars()
                    .next()
                    .filter(|c| matches!(c, '"' | '\''))
                else {
                    continue;
                };
                let line_start = source[..end].rfind('\n').map_or(0, |i| i + 1);
                if let Some(open) = source[line_start..end].rfind(quote) {
                    queue.push(source[line_start + open + 1..end].to_owned());
                }
            }
        }
        used
    }

    /// Whether a template of this name exists (the app's or a built-in).
    pub fn exists(&self, name: &str) -> bool {
        self.reloader
            .acquire_env()
            .is_ok_and(|env| env.get_template(name).is_ok())
    }

    /// Renders a template with the given context and no request globals.
    pub fn render(&self, name: &str, ctx: impl Serialize) -> anyhow::Result<String> {
        let env = self.reloader.acquire_env()?;
        Ok(env.get_template(name)?.render(ctx)?)
    }

    fn render_view(
        &self,
        view: &View,
        shared: Value,
        globals: Value,
        htmx: &Htmx,
    ) -> anyhow::Result<String> {
        let env = self.reloader.acquire_env()?;
        let template = env.get_template(&view.name)?;
        // Components (macros imported from other templates) don't see this
        // context; they reach the same values through `RequestGlobal`s.
        let _current = CurrentGlobals::set(globals.clone());
        // The last map wins: shared values, then the handler's, then Renox's.
        // A value named like a macro the template imports is hidden by the
        // macro, which fails far from the cause (#312): say so while debugging.
        let hidden = if self.debug {
            hidden_by_imports(template.source(), &[&shared, &view.ctx])
        } else {
            Vec::new()
        };
        for name in &hidden {
            tracing::warn!(
                template = %view.name,
                name = %name,
                "the view gets a value named like a macro the template imports; the macro hides it (rename one)"
            );
        }
        // `nav('menu')` is a request global, but a page that already passes its
        // own `nav` value keeps it (the global would otherwise hide it).
        let own_nav = [&view.ctx, &shared]
            .iter()
            .find_map(|layer| layer.get_attr("nav").ok().filter(|n| !n.is_undefined()))
            .map_or_else(
                || Value::from_serialize(serde_json::Map::new()),
                |nav| context! { nav },
            );
        let ctx = merge_maps([shared, view.ctx.clone(), globals, own_nav]);
        let stacks = crate::view_stack::Scope::begin();
        let rendered = match &view.fragment {
            Some(block) if htmx.wants_fragment() => (|| {
                let mut captured = template.render_captured_to(ctx, std::io::sink())?;
                let mut out = captured.with_state_mut(|state| state.render_block(block))?;
                for extra in &view.also {
                    out.push_str(&captured.with_state_mut(|state| state.render_block(extra))?);
                }
                Ok::<_, minijinja::Error>(out)
            })(),
            _ => template.render(ctx),
        };
        let html = rendered.map_err(|err| {
            let err = anyhow::Error::from(err);
            match hidden.as_slice() {
                [] => err,
                names => err.context(format!(
                    "{} imports a macro named like a value the view gets ({}): \
                     inside the template the name is the macro; rename the value or import the macro `as` another name",
                    view.name,
                    names.join(", ")
                )),
            }
        })?;
        Ok(stacks.finish(html))
    }

    /// The error page: the app's `errors/{status}.html`, else its
    /// `errors/default.html`, else Renox's. With `globals` (a request's), an
    /// app's page can extend its layout; a page that fails falls back to
    /// Renox's, so an error in the layout doesn't hide the first error.
    fn render_error(
        &self,
        page: &ErrorPage,
        debug: bool,
        request: &str,
        globals: Option<Value>,
    ) -> anyhow::Result<String> {
        let env = self.reloader.acquire_env()?;
        let ctx = context! {
            status => page.status.as_u16(),
            reason => reason(page.status),
            detail => page.shown_detail(debug),
            // Only while developing: what was asked, and where a template failed.
            debug => debug,
            request_line => debug.then_some(request),
            template => page.template.as_deref().filter(|_| debug),
        };
        for name in [
            format!("errors/{}.html", page.status.as_u16()),
            "errors/default.html".to_owned(),
        ] {
            let template = match env.get_template(&name) {
                Ok(template) => template,
                Err(err) if err.kind() == ErrorKind::TemplateNotFound => continue,
                // One that doesn't parse fails like one that doesn't render.
                Err(err) => {
                    tracing::error!(error = ?err, template = %name, "the error page failed; showing Renox's");
                    break;
                }
            };
            let rendered = match &globals {
                Some(globals) => {
                    let _current = CurrentGlobals::set(globals.clone());
                    let stacks = crate::view_stack::Scope::begin();
                    template
                        .render(merge_maps([globals.clone(), ctx.clone()]))
                        .map(|html| stacks.finish(html))
                }
                None => template.render(ctx.clone()),
            };
            match rendered {
                Ok(html) => return Ok(html),
                Err(err) => {
                    tracing::error!(error = ?err, template = %name, "the error page failed; showing Renox's");
                    break;
                }
            }
        }
        Ok(env.get_template("renox/error.html")?.render(ctx)?)
    }
}

/// The template a response was rendered from.
#[derive(Debug, Clone)]
pub(crate) struct RenderedView(pub String);

/// The globals Renox gives each rendered page (see `globals`), which
/// components reach through the environment.
pub(crate) const REQUEST_GLOBALS: &[&str] = &[
    "app",
    "auth",
    "t",
    "can",
    "request",
    "route_is",
    "nav",
    "csrf_token",
    "csrf_field",
    "flash",
    "errors",
    "errors_in",
    "error",
    "old",
    "has_old",
    "renox_head",
    "seo",
    "csp_nonce",
    "toasts",
    "once",
];

thread_local! {
    /// The globals of the page being rendered on this thread.
    static CURRENT: std::cell::RefCell<Option<Value>> = const { std::cell::RefCell::new(None) };
}

/// Makes `globals` the current page's while alive (rendering is synchronous).
struct CurrentGlobals(Option<Value>);

impl CurrentGlobals {
    fn set(globals: Value) -> Self {
        Self(CURRENT.with(|c| c.borrow_mut().replace(globals)))
    }
}

impl Drop for CurrentGlobals {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT.with(|c| *c.borrow_mut() = previous);
    }
}

/// A request global as seen from the environment: forwards to the value of
/// the page being rendered, so an imported macro gets the same `old()`,
/// `auth`, `t()`… as the template that called it.
#[derive(Debug)]
struct RequestGlobal(&'static str);

impl RequestGlobal {
    fn current(&self) -> Option<Value> {
        CURRENT
            .with(|c| c.borrow().clone())
            .and_then(|globals| globals.get_attr(self.0).ok())
            .filter(|v| !v.is_undefined())
    }
}

impl minijinja::value::Object for RequestGlobal {
    fn repr(self: &Arc<Self>) -> minijinja::value::ObjectRepr {
        minijinja::value::ObjectRepr::Map
    }

    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        self.current()?
            .get_item(key)
            .ok()
            .filter(|v| !v.is_undefined())
    }

    fn enumerate(self: &Arc<Self>) -> minijinja::value::Enumerator {
        match self
            .current()
            .and_then(|v| v.try_iter().ok().map(|keys| keys.collect::<Vec<_>>()))
        {
            Some(keys)
                if self
                    .current()
                    .is_some_and(|v| v.kind() == minijinja::value::ValueKind::Map) =>
            {
                minijinja::value::Enumerator::Values(keys)
            }
            _ => minijinja::value::Enumerator::Empty,
        }
    }

    fn is_true(self: &Arc<Self>) -> bool {
        self.current().is_some_and(|v| v.is_true())
    }

    fn call(
        self: &Arc<Self>,
        state: &minijinja::State<'_, '_>,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        match self.current() {
            Some(value) => value.call(state, args),
            None => Err(minijinja::Error::new(
                ErrorKind::InvalidOperation,
                format!("`{}` is only available while rendering a page", self.0),
            )),
        }
    }

    fn render(self: &Arc<Self>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.current() {
            Some(value) => std::fmt::Display::fmt(&value, f),
            None => Ok(()),
        }
    }
}

/// Like MiniJinja's default formatter, but HTML escaping leaves `/` alone:
/// escaping `& < > " '` is enough for text and quoted attributes, and URLs
/// (links in pages and mail) stay readable.
fn format_value(
    out: &mut minijinja::Output,
    state: &minijinja::State,
    value: &Value,
) -> Result<(), minijinja::Error> {
    use std::fmt::Write;
    if let (minijinja::AutoEscape::Html, false, Some(text)) =
        (state.auto_escape(), value.is_safe(), value.as_str())
    {
        for c in text.chars() {
            let written = match c {
                '&' => out.write_str("&amp;"),
                '<' => out.write_str("&lt;"),
                '>' => out.write_str("&gt;"),
                '"' => out.write_str("&quot;"),
                '\'' => out.write_str("&#39;"),
                c => out.write_char(c),
            };
            written.map_err(|_| {
                minijinja::Error::new(ErrorKind::WriteFailure, "could not write output")
            })?;
        }
        return Ok(());
    }
    minijinja::escape_formatter(out, state, value)
}

fn load(
    dir: &Path,
    embedded: Option<&'static [(&'static str, &'static str)]>,
    name: &str,
) -> Result<Option<String>, minijinja::Error> {
    if let Some(files) = embedded {
        if let Some((_, source)) = files.iter().find(|(file, _)| *file == name) {
            return Ok(Some((*source).to_owned()));
        }
    } else if let Some(path) = safe_join(dir, name) {
        match std::fs::read_to_string(&path) {
            Ok(source) => return Ok(Some(source)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(minijinja::Error::new(
                    ErrorKind::InvalidOperation,
                    format!("could not read template {}", path.display()),
                )
                .with_source(err));
            }
        }
    }
    Ok(BUILTIN
        .iter()
        .find(|(builtin, _)| *builtin == name)
        .map(|(_, source)| (*source).to_owned()))
}

/// Reports every literal `route('name')` in the compiled `source` that no route has.
fn check_route_names(
    template: &str,
    source: &str,
    routes: &RouteTable,
    problems: &mut Vec<ViewProblem>,
) {
    static ROUTE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"\broute\(\s*["']([^"']+)["']"#).expect("valid regex")
    });
    for caps in ROUTE.captures_iter(source) {
        let name = &caps[1];
        if routes.path(name).is_some() {
            continue;
        }
        let hint = crate::components::suggest::did_you_mean(name, routes.names())
            .map(|x| format!("; did you mean \"{x}\"?"))
            .unwrap_or_default();
        let offset = caps.get(0).map_or(0, |m| m.start());
        problems.push(ViewProblem {
            template: template.to_owned(),
            line: Some(source[..offset].matches('\n').count() + 1),
            message: format!("unknown route \"{name}\"{hint}"),
        });
    }
}

/// Reports `{% include "name" %}` (what `<rx-include template="name"/>` compiles to) of a template
/// that does not exist. An include with `ignore missing`, or whose name is worked out at run
/// time, is not checked.
fn check_includes(
    template: &str,
    source: &str,
    known: &std::collections::HashSet<&str>,
    problems: &mut Vec<ViewProblem>,
) {
    static INCLUDE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"\{%-?\s*include\s+["']([^"']+)["']\s*-?%\}"#).expect("valid regex")
    });
    for caps in INCLUDE.captures_iter(source) {
        let name = &caps[1];
        if known.contains(name) {
            continue;
        }
        let hint = crate::components::suggest::did_you_mean(name, known.iter().copied())
            .map(|x| format!("; did you mean \"{x}\"?"))
            .unwrap_or_default();
        let offset = caps.get(0).map_or(0, |m| m.start());
        problems.push(ViewProblem {
            template: template.to_owned(),
            line: Some(source[..offset].matches('\n').count() + 1),
            message: format!(
                "<rx-include> (or include) names a template that does not exist: \"{name}\"{hint}"
            ),
        });
    }
}

/// A [`ViewProblem`] from a compile error. The component compiler's details
/// already start with `name:line: `, which is taken out of the message.
fn problem(name: &str, err: &minijinja::Error) -> ViewProblem {
    let detail = err.detail().unwrap_or("could not be compiled");
    if let Some(rest) = detail.strip_prefix(name).and_then(|r| r.strip_prefix(':'))
        && let Some((line, message)) = rest.split_once(": ")
        && let Ok(line) = line.parse::<usize>()
    {
        return ViewProblem {
            template: name.to_owned(),
            line: Some(line),
            message: message.to_owned(),
        };
    }
    ViewProblem {
        template: name.to_owned(),
        line: err.line(),
        message: detail.to_owned(),
    }
}

fn safe_join(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut path = dir.to_path_buf();
    for component in Path::new(name).components() {
        match component {
            PathPart::Normal(part) => path.push(part),
            _ => return None,
        }
    }
    Some(path)
}

/// A template response, rendered by Renox with the request's globals:
/// `app` (with the request's `app.locale`), `request`, `auth` (`auth.check`,
/// `auth.user`), `t()`, `can()`, `flash`, `errors`, `error()`, `old()`,
/// `csrf_token`, `csrf_field()` and `renox_head()`.
///
/// ```
/// # use renox::prelude::*;
/// # let list: Vec<String> = Vec::new();
/// # let _ = move || {
/// async fn index() -> View {
/// #   let list: Vec<String> = Vec::new();
///     view("products/index.html", context! { products => list }).fragment("list")
/// }
/// # };
/// ```
#[derive(Clone)]
pub struct View {
    name: String,
    ctx: Value,
    fragment: Option<String>,
    /// More blocks sent with the fragment, for out-of-band swaps.
    also: Vec<String>,
    status: StatusCode,
}

/// The line of the first `{{ … }}` or `{% … %}` tag that names `var`, if any.
fn first_use(source: &str, var: &str) -> Option<usize> {
    let word = regex::Regex::new(&format!(r"\b{}\b", regex::escape(var))).ok()?;
    let mut rest = source;
    let mut offset = 0;
    while let Some(open) = rest.find('{') {
        let after = &rest[open..];
        let close = match after.get(..2) {
            Some("{{") => "}}",
            Some("{%") => "%}",
            _ => {
                offset += open + 1;
                rest = &rest[open + 1..];
                continue;
            }
        };
        let end = after.find(close).map_or(after.len(), |i| i + 2);
        if word.is_match(&after[..end]) {
            return Some(source[..offset + open].matches('\n').count() + 1);
        }
        offset += open + end;
        rest = &rest[open + end..];
    }
    None
}

/// Renders `name` from the views directory with `ctx` (anything serializable,
/// usually `context! { ... }`).
pub fn view(name: impl Into<String>, ctx: impl Serialize) -> View {
    View {
        name: name.into(),
        ctx: Value::from_serialize(ctx),
        fragment: None,
        also: Vec::new(),
        status: StatusCode::OK,
    }
}

impl View {
    /// Renders a page struct in its own template ([`ViewData::TEMPLATE`]): the typed
    /// form of [`view`]. Register the struct with `App::view_data::<T>()` so
    /// `view:check` reports the template's variables the struct lacks.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// #[derive(serde::Serialize, ViewData)]
    /// #[view(template = "orders/show.html")]
    /// struct OrderPage {
    ///     number: String,
    /// }
    ///
    /// async fn show() -> View {
    ///     View::typed(OrderPage { number: "A-1".into() }).fragment("summary")
    /// }
    /// ```
    pub fn typed<T: ViewData>(page: T) -> View {
        view(T::TEMPLATE, page)
    }

    /// For HTMX requests (except `hx-boost`), render only this `{% block %}`.
    pub fn fragment(mut self, block: impl Into<String>) -> Self {
        self.fragment = Some(block.into());
        self
    }

    /// For the same HTMX requests, also render `block` after the fragment,
    /// to update other parts of the page in one response: give the block's
    /// root element an `id` and `hx-swap-oob="true"` and htmx swaps it into
    /// the element with that id.
    ///
    /// ```html
    /// {% block row %}<tr id="order-{{ order.id }}">…</tr>{% endblock %}
    /// {% block count %}<span id="order-count" hx-swap-oob="true">{{ count }}</span>{% endblock %}
    /// ```
    ///
    /// `view("orders/index.html", ctx).fragment("row").also("count")`
    pub fn also(mut self, block: impl Into<String>) -> Self {
        self.also.push(block.into());
        self
    }

    /// The response status (200 by default).
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
}

impl IntoResponse for View {
    fn into_response(self) -> Response {
        let mut res = self.status.into_response();
        res.extensions_mut().insert(self);
        res
    }
}

/// Renders `View` and error responses once the handler has returned, and
/// turns validation errors into a redirect back for regular form posts.
pub(crate) async fn middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let session = req.extensions().get::<Session>().cloned();
    let current_user = req.extensions().get::<CurrentUser>().cloned();
    let htmx = Htmx::from_headers(req.headers());
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let query = req.uri().query().unwrap_or_default().to_owned();
    let route = crate::routing::CurrentRoute::of(req.method(), req.extensions(), &state)
        .name()
        .map(str::to_owned);
    let host = crate::domain::host(&req).unwrap_or_default();
    let nonce = req
        .extensions()
        .get::<crate::security::CspNonce>()
        .map(|n| n.0.clone())
        .unwrap_or_default();
    let (wants_json, referer) = {
        let header = |name| req.headers().get(name).and_then(|v| v.to_str().ok());
        let wants_json = header(ACCEPT).is_some_and(|v| v.contains("application/json"))
            || header(CONTENT_TYPE).is_some_and(|v| v.starts_with("application/json"));
        // Back to the form's page, but never to another site.
        (wants_json, crate::htmx::same_site_referer(req.headers()))
    };

    let request_line = if query.is_empty() {
        format!("{method} {path}")
    } else {
        format!("{method} {path}?{query}")
    };
    let mut res = next.run(req).await;
    // A layer may have switched the language (`RequestLocale::switch`).
    let locale = crate::i18n::current_locale(&state);

    if let Some(crate::toast::PendingToasts(toasts)) =
        res.extensions_mut().remove::<crate::toast::PendingToasts>()
    {
        // An htmx swap shows them now; a page, an htmx redirect or refresh
        // on the next page, from the session (the reload would lose them).
        if htmx.request
            && !res.headers().contains_key("hx-redirect")
            && !res.headers().contains_key("hx-refresh")
        {
            crate::htmx::add_trigger(
                &mut res,
                crate::toast::EVENT,
                serde_json::json!({ "toasts": toasts }),
            );
        } else if let Some(session) = &session {
            let mut waiting: Vec<crate::Toast> =
                session.get(crate::toast::SESSION_KEY).unwrap_or_default();
            waiting.extend(toasts);
            if let Err(err) = session.put(crate::toast::SESSION_KEY, &waiting) {
                return err.into_response();
            }
        }
    }

    if let Some(failed) = res.extensions_mut().remove::<ValidationError>() {
        if htmx.request || wants_json {
            return res;
        }
        if let Some(session) = &session {
            let bag = failed.bag().map(str::to_owned);
            // A `ValidationError` made after `Valid` (a model hook, the
            // handler) carries no input: refill with what `Valid` read.
            let input = if failed.input.is_empty() {
                crate::context::get::<crate::validation::extract::SubmittedInput>()
                    .map(|submitted| submitted.0)
                    .unwrap_or_default()
            } else {
                failed.input
            };
            let flashed = match &bag {
                Some(bag) => session.flash_errors_in(bag, &failed.errors),
                None => session.flash_errors(&failed.errors),
            }
            .and_then(|()| session.flash_input(&input));
            if let Err(err) = flashed {
                return err.into_response();
            }
        }
        return Redirect::to(referer.as_deref().unwrap_or("/")).into_response();
    }

    // Analytics events: with an htmx swap, in its HX-Trigger; with a page,
    // in its head (below); otherwise they wait for the next page.
    if let Some(session) = &session
        && crate::analytics::has_pending(session)
        && htmx.request
        && crate::analytics::deliverable_by_htmx(&res)
    {
        crate::analytics::add_trigger(&mut res, crate::analytics::take(session));
    }

    if let Some(view) = res.extensions_mut().remove::<View>() {
        let mut shared = std::collections::BTreeMap::new();
        for (key, compute) in state.shares.iter() {
            let ctx = ViewContext {
                state: state.clone(),
                user: current_user.as_ref().and_then(|c| c.user.clone()),
                locale: locale.clone(),
                path: path.clone(),
                session: session.clone(),
            };
            match compute(ctx).await {
                Ok(value) => {
                    shared.insert(key.clone(), value);
                }
                Err(err) => {
                    let err = match err {
                        Error::Internal(err) => err,
                        other => anyhow::anyhow!("{other:?}"),
                    };
                    // Shown like any error: the error page, with the detail
                    // while debugging.
                    let mut failed =
                        Error::Internal(err.context(format!("sharing `{key}` with views")))
                            .into_response();
                    return match failed.extensions_mut().remove::<ErrorPage>() {
                        Some(page) => error_response(
                            &state,
                            page,
                            failed,
                            wants_json,
                            &htmx,
                            &request_line,
                            None,
                        ),
                        None => failed,
                    };
                }
            }
        }
        add_theme(
            &mut shared,
            &state,
            &host,
            &path,
            &locale,
            current_user.as_ref(),
        )
        .await;
        let events = match &session {
            Some(session) if !htmx.request => crate::analytics::take(session),
            _ => Vec::new(),
        };
        let menus = request_menus(
            &state,
            current_user.as_ref(),
            &locale,
            route.as_deref(),
            &path,
            state.views.menus_used(&view.name),
        )
        .await;
        let globals = globals(
            &state,
            session.as_ref(),
            current_user.clone(),
            &htmx,
            &Requested {
                path: &path,
                query: &query,
                route: route.as_deref(),
                nonce: &nonce,
                events: &events,
            },
            &locale,
            menus,
        );
        return match state
            .views
            .render_view(&view, Value::from_serialize(&shared), globals, &htmx)
        {
            Ok(html) => {
                let mut page = with_html(res, html);
                // For `TestResponse::assert_view`.
                page.extensions_mut()
                    .insert(RenderedView(view.name.clone()));
                page
            }
            Err(err) => {
                let mut failed = Error::Internal(err.context(format!("rendering {}", view.name)))
                    .into_response();
                match failed.extensions_mut().remove::<ErrorPage>() {
                    Some(page) => {
                        error_response(&state, page, failed, wants_json, &htmx, &request_line, None)
                    }
                    None => failed,
                }
            }
        };
    }

    if let Some(page) = res.extensions_mut().remove::<ErrorPage>() {
        // The app's error pages may extend its layout: give them what pages
        // get, the values `App::share`s included (a layout's cart count).
        let globals = if !wants_json || htmx.request {
            let mut shared = std::collections::BTreeMap::new();
            for (key, compute) in state.shares.iter() {
                let ctx = ViewContext {
                    state: state.clone(),
                    user: current_user.as_ref().and_then(|c| c.user.clone()),
                    locale: locale.clone(),
                    path: path.clone(),
                    session: session.clone(),
                };
                // A share that fails here only leaves its value out: the page
                // is about another error, which it mustn't hide.
                match compute(ctx).await {
                    Ok(value) => {
                        shared.insert(key.clone(), value);
                    }
                    Err(err) => {
                        tracing::warn!(error = ?err, key = %key, "a shared view value failed on an error page")
                    }
                }
            }
            add_theme(
                &mut shared,
                &state,
                &host,
                &path,
                &locale,
                current_user.as_ref(),
            )
            .await;
            // The app's error page may extend a layout with a menu: build
            // every menu, as the page that extends is not known here.
            let menus = request_menus(
                &state,
                current_user.as_ref(),
                &locale,
                route.as_deref(),
                &path,
                crate::nav::MenusUsed {
                    names: Vec::new(),
                    dynamic: true,
                },
            )
            .await;
            let page_globals = globals(
                &state,
                session.as_ref(),
                current_user,
                &htmx,
                &Requested {
                    path: &path,
                    query: &query,
                    route: route.as_deref(),
                    nonce: &nonce,
                    events: &[],
                },
                &locale,
                menus,
            );
            Some(merge_maps([page_globals, Value::from_serialize(&shared)]))
        } else {
            None
        };
        return error_response(&state, page, res, wants_json, &htmx, &request_line, globals);
    }
    res
}

/// The key a request's brand stylesheet URL is shared under, for `renox_theme()` (not named like the function: a value would hide it).
pub(crate) const THEME_KEY: &str = "renox_theme_url";

/// Resolves the request's brand (`App::theme`, `App::theme_for`) and shares
/// the URL of its stylesheet with the views; nothing is shared when the app
/// has no theme for this request.
async fn add_theme(
    shared: &mut std::collections::BTreeMap<String, Value>,
    state: &AppState,
    host: &str,
    path: &str,
    locale: &str,
    user: Option<&CurrentUser>,
) {
    if !state.themes.is_set() {
        return;
    }
    let url = state
        .themes
        .resolve(crate::theme::ThemeRequest {
            host: host.to_owned(),
            path: path.to_owned(),
            locale: locale.to_owned(),
            user: user.and_then(|c| c.user.clone()),
            state: state.clone(),
        })
        .await;
    if let Some(url) = url {
        shared.insert(THEME_KEY.to_owned(), Value::from(url));
    }
}

/// The error page (or JSON for API clients) for `page`.
fn error_response(
    state: &AppState,
    page: ErrorPage,
    res: Response,
    wants_json: bool,
    htmx: &Htmx,
    request_line: &str,
    globals: Option<Value>,
) -> Response {
    let debug = state.config.debug;
    if wants_json && !htmx.request {
        // Keep what the error response carried (`Retry-After` on a 429, …).
        let mut json = page.json(debug);
        for (name, value) in res.headers() {
            if name != CONTENT_TYPE && name != CONTENT_LENGTH {
                json.headers_mut().insert(name.clone(), value.clone());
            }
        }
        return json;
    }
    let html = state
        .views
        .render_error(&page, debug, request_line, globals)
        .unwrap_or_else(|err| {
            tracing::error!(error = ?err, "could not render the error page");
            crate::error::error_page(page.status, page.shown_detail(debug))
        });
    with_html(res, html)
}

fn with_html(mut res: Response, html: String) -> Response {
    let headers = res.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.remove(CONTENT_LENGTH);
    *res.body_mut() = Body::from(html);
    res
}

/// Flashed values; a key that wasn't flashed reads as `""`, so
/// `{{ flash.status }}` needs no `if`, even with strict templates.
#[derive(Debug)]
struct Flashed(serde_json::Map<String, serde_json::Value>);

impl minijinja::value::Object for Flashed {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let key = key.as_str()?;
        Some(
            self.0
                .get(key)
                .map(Value::from_serialize)
                .unwrap_or_else(|| Value::from("")),
        )
    }

    fn enumerate(self: &Arc<Self>) -> minijinja::value::Enumerator {
        minijinja::value::Enumerator::Values(
            self.0.keys().map(|k| Value::from(k.as_str())).collect(),
        )
    }

    fn repr(self: &Arc<Self>) -> minijinja::value::ObjectRepr {
        minijinja::value::ObjectRepr::Map
    }
}

/// Builds the menus `used` names for the request's user.
async fn request_menus(
    state: &AppState,
    current_user: Option<&CurrentUser>,
    locale: &str,
    route: Option<&str>,
    path: &str,
    used: crate::nav::MenusUsed,
) -> std::collections::HashMap<String, Vec<crate::nav::MenuGroup>> {
    if state.nav.menus.is_empty() || (used.names.is_empty() && !used.dynamic) {
        return Default::default();
    }
    let user = current_user.and_then(|c| crate::auth::auth_user_of(c, state));
    crate::nav::build_for_request(state, user.as_ref(), locale, route, path, &used).await
}

/// What templates see of the request, besides the session and user.
struct Requested<'a> {
    path: &'a str,
    query: &'a str,
    /// The matched route's name, if it has one.
    route: Option<&'a str>,
    nonce: &'a str,
    /// Analytics events for this page's head.
    events: &'a [crate::analytics::Event],
}

/// The templates' `t(key, name=…, count=…)` in `locale`.
pub(crate) fn translate_function(state: &AppState, locale: &str) -> Value {
    let translator = state.translator.clone();
    let (locale, fallback) = (locale.to_owned(), state.config.fallback_locale.clone());
    Value::from_function(
        move |key: String, kwargs: minijinja::value::Kwargs| -> Result<Value, minijinja::Error> {
            let mut params = Vec::new();
            let mut count = None;
            for name in kwargs.args() {
                let value: Value = kwargs.get(name)?;
                if name == "count" {
                    match i64::try_from(value.clone()) {
                        Ok(whole) => count = Some(whole),
                        // Not a whole number (a fraction, or text such as
                        // "1,204"): `:count` shows it as given, and a plural
                        // form is still picked by its value when it has one.
                        Err(_) => {
                            let shown = value.to_string();
                            let number = f64::try_from(value)
                                .ok()
                                .or_else(|| shown.trim().parse::<f64>().ok());
                            count = number.map(plural_count);
                            params.push((name.to_owned(), shown));
                        }
                    }
                } else {
                    params.push((name.to_owned(), value.to_string()));
                }
            }
            kwargs.assert_all_used()?;
            let params: Vec<(&str, String)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            let text = translator.get(&locale, &fallback, &key);
            Ok(Value::from(crate::i18n::format(&text, &params, count)))
        },
    )
}

/// The count a number stands for when picking a plural form: one only for
/// exactly 1 (so 1.5 or 0.5 kilos are plural), else the nearest whole one
/// (for ranges such as `[2,5]`).
fn plural_count(number: f64) -> i64 {
    if number == 1.0 {
        1
    } else {
        match number.round() as i64 {
            1 => 2,
            other => other,
        }
    }
}

fn globals(
    state: &AppState,
    session: Option<&Session>,
    current_user: Option<CurrentUser>,
    htmx: &Htmx,
    requested: &Requested,
    locale: &str,
    menus: std::collections::HashMap<String, Vec<crate::nav::MenuGroup>>,
) -> Value {
    let config = &state.config;
    let token = session.map(Session::token).unwrap_or_default();

    let strict = state.security.mode == crate::CspMode::Strict;
    let head = Value::from_safe_string(format!(
        "{}\n{}",
        assets::head_tags(&token, state.live.is_some(), strict),
        crate::seo::head_tags(&state.config, requested.nonce, requested.events)
    ));
    let seo = {
        let (config, path, locale) = (
            state.config.clone(),
            requested.path.to_owned(),
            locale.to_owned(),
        );
        Value::from_function(
            move |kwargs: minijinja::value::Kwargs| -> Result<Value, minijinja::Error> {
                crate::seo::tags(&config, &path, &locale, &kwargs).map(Value::from_safe_string)
            },
        )
    };
    let nonce = requested.nonce.to_owned();
    let field = Value::from_safe_string(format!(
        "<input type=\"hidden\" name=\"{}\" value=\"{token}\">",
        crate::csrf::CSRF_FIELD
    ));
    let old_input = session.cloned();
    let has_old = session.is_some_and(Session::has_old_input);
    let toast_session = session.cloned();
    let toast_texts = crate::toast::RegionTexts {
        dismiss: state
            .translator
            .get(locale, &state.config.fallback_locale, "ui.dismiss"),
        failed: state
            .translator
            .get(locale, &state.config.fallback_locale, "ui.request_failed"),
    };
    let seen = std::sync::Mutex::new(std::collections::HashSet::<String>::new());
    let user = current_user.as_ref().and_then(|c| c.user.clone());
    let roles = current_user
        .as_ref()
        .map(|c| c.grants.roles())
        .unwrap_or_default();
    let gate_user = current_user.and_then(|c| Some((c.user?, c.gates, c.grants)));
    // Errors flashed in a named bag (`Validate::ERROR_BAG`) are shown only
    // by `error(field, bag=…)` and `errors_in(bag)`.
    let error_bag = session.and_then(Session::error_bag);
    let flashed_errors = match (session, &error_bag) {
        (Some(session), Some(bag)) => session.errors_in(bag),
        (Some(session), None) => session.errors(),
        (None, _) => Default::default(),
    };
    let errors = if error_bag.is_none() {
        flashed_errors.clone()
    } else {
        Default::default()
    };
    let first_errors: std::collections::BTreeMap<String, String> = flashed_errors
        .iter()
        .filter_map(|(field, messages)| {
            messages
                .get(0)
                .and_then(|m| m.as_str())
                .map(|m| (field.clone(), m.to_owned()))
        })
        .collect();

    context! {
        app => context! {
            name => config.name,
            env => format!("{:?}", config.env).to_lowercase(),
            debug => config.debug,
            url => config.url,
            locale => locale,
            dir => state.direction(locale).as_str(),
            currency => config.currency,
        },
        auth => context! {
            check => user.is_some(),
            user => user.as_deref(),
            roles => roles,
        },
        t => translate_function(state, locale),
        // `can('admin')` asks a gate; `can('update', product)` reads the
        // abilities `auth::Can` attached to the model in the handler.
        can => Value::from_function(move |ability: String, target: Option<Value>| {
            match target {
                Some(target) => target
                    .get_attr("_can")
                    .and_then(|can| can.get_attr(&ability))
                    .is_ok_and(|allowed| allowed.is_true()),
                None => gate_user
                    .as_ref()
                    .is_some_and(|(user, gates, grants)| gates.check(user, grants, &ability)),
            }
        }),
        request => context! {
            path => requested.path,
            query => requested.query,
            route => requested.route,
            htmx => htmx.request,
            boosted => htmx.boosted,
        },
        // `route_is('admin.*')`, `route_is('products.index', 'products.show')`.
        route_is => {
            let route = requested.route.map(str::to_owned);
            Value::from_function(move |patterns: minijinja::value::Rest<String>| {
                route.as_deref().is_some_and(|name| {
                    patterns
                        .iter()
                        .any(|pattern| crate::routing::route_name_matches(name, pattern))
                })
            })
        },
        // `nav('staff')`: the groups of a menu the app's modules contributed
        // to, for this request's user (built before rendering, only for the
        // menus the page's templates name).
        nav => Value::from_function(move |menu: String| {
            Value::from_serialize(menus.get(&menu).map(Vec::as_slice).unwrap_or_default())
        }),
        csrf_token => token,
        flash => Value::from_object(Flashed(session.map(Session::flashed).unwrap_or_default())),
        errors => errors,
        // `errors_in('login')`: every error of a named bag.
        errors_in => {
            let bag = error_bag.clone();
            Value::from_function(move |name: String| {
                if bag.as_deref() == Some(name.as_str()) {
                    Value::from_serialize(&flashed_errors)
                } else {
                    Value::from_serialize(serde_json::Map::new())
                }
            })
        },
        // `error('photos')` also shows the first error of an item (`photos.1`);
        // `error('email', bag='login')` reads a named bag.
        error => Value::from_function(move |field: String, kwargs: minijinja::value::Kwargs| {
            let bag: Option<String> = kwargs.get("bag")?;
            kwargs.assert_all_used()?;
            if bag != error_bag {
                return Ok::<_, minijinja::Error>(String::new());
            }
            // `items[0][name]`'s errors are keyed `items.0.name`.
            let field = crate::validation::nested::normalize(&field);
            Ok(first_errors
                .get(&field)
                .or_else(|| {
                    let prefix = format!("{field}.");
                    first_errors
                        .iter()
                        .find(|(key, _)| key.starts_with(&prefix))
                        .map(|(_, message)| message)
                })
                .cloned()
                .unwrap_or_default())
        }),
        renox_head => Value::from_function(move || head.clone()),
        seo => seo,
        csp_nonce => Value::from_function(move || nonce.clone()),
        csrf_field => Value::from_function(move || field.clone()),
        // `{{ toasts() }}`: the toast region, with the toasts waiting for
        // this page (taken from the session: shown once).
        // `toasts(position="bottom-end")` moves them (see toast::POSITIONS).
        toasts => Value::from_function(move |kwargs: minijinja::value::Kwargs| {
            let position: Option<String> = kwargs.get("position")?;
            kwargs.assert_all_used()?;
            let waiting: Vec<crate::Toast> = toast_session
                .as_ref()
                .and_then(|s| s.pull(crate::toast::SESSION_KEY))
                .unwrap_or_default();
            Ok::<_, minijinja::Error>(Value::from_safe_string(crate::toast::region(
                &waiting,
                &toast_texts,
                position.as_deref().unwrap_or("top"),
            )))
        }),
        // `{% if once('date-picker') %}…{% endif %}`: true the first time a key
        // is asked for on a page, e.g. for a component's script.
        once => Value::from_function(move |key: String| {
            seen.lock().unwrap_or_else(|e| e.into_inner()).insert(key)
        }),
        // `has_old()`: the previous request was a failed submit, so a field
        // missing from `old()` was sent empty (an unticked checkbox).
        has_old => Value::from_function(move || has_old),
        old => Value::from_function(move |field: String, default: Option<Value>| {
            old_input
                .as_ref()
                .and_then(|s| s.old(&field))
                .map(Value::from_serialize)
                .or(default)
                .unwrap_or_else(|| Value::from(""))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views_in(dir: &Path) -> Views {
        let config = Config {
            views_path: dir.to_path_buf(),
            ..Config::default()
        };
        Views::new(
            &config,
            Arc::new(RouteTable::default()),
            Storage::from_config(&config).unwrap(),
            None,
            Arc::new(Vec::new()),
            Arc::new(Vec::new()),
            config.timezone,
            Arc::new(crate::embedded::AssetVersions::new(dir, None)),
        )
    }

    #[test]
    fn check_finds_every_broken_template_with_its_line() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, body: &str| {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        };
        write("good.html", "<h1>{{ 1 + 1 }}</h1>");
        write("sub/typo.html", "<p>x</p>\n<rx-tabel>y</rx-tabel>\n");
        write("bad.html", "a\nb\n{% if x %}\nc\n");
        write(".hidden.html", "{% if %}");
        write("notes.md", "{% if %}");
        let views = views_in(dir.path());

        let names = views.names();
        assert!(names.contains(&"good.html".to_owned()));
        assert!(names.contains(&"sub/typo.html".to_owned()));
        assert!(names.contains(&"renox/error.html".to_owned()));
        assert!(
            !names
                .iter()
                .any(|n| n.contains("hidden") || n.ends_with(".md"))
        );
        assert!(names.windows(2).all(|w| w[0] < w[1]));

        let problems = views.check(&RouteTable::default());
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(problems[0].template, "bad.html");
        assert!(problems[0].line.is_some_and(|l| l >= 3), "{problems:?}");
        assert_eq!(problems[1].template, "sub/typo.html");
        assert_eq!(problems[1].line, Some(2));
        assert!(problems[1].message.contains("rx-tabel"), "{problems:?}");
        assert!(
            problems[1]
                .to_string()
                .starts_with("sub/typo.html:2: unknown component"),
        );
    }

    #[test]
    fn check_reports_unknown_route_names_with_a_suggestion() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("p.html"),
            "<a href=\"{{ route('products.index') }}\">ok</a>\n\
             <a href=\"{{ route('products.indx') }}\">typo</a>\n\
             <a href=\"{{ route('shop.home') }}\">domain</a>\n\
             <a href=\"{{ route('nothing.like.it') }}\">far</a>\n",
        )
        .unwrap();
        let mut routes = RouteTable::default();
        routes
            .insert("products.index".into(), "/products".into())
            .unwrap();
        // A route defined inside `Routes::domain` is in the table as well.
        routes.insert("shop.home".into(), "/".into()).unwrap();
        routes.set_domain("shop.home", "shop.example.com");
        let problems = views_in(dir.path()).check(&routes);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(problems[0].line, Some(2));
        assert_eq!(
            problems[0].to_string(),
            "p.html:2: unknown route \"products.indx\"; did you mean \"products.index\"?"
        );
        assert_eq!(problems[1].line, Some(4));
        assert!(!problems[1].message.contains("did you mean"));
    }

    fn env() -> Environment<'static> {
        let mut env = Environment::new();
        for name in REQUEST_GLOBALS {
            env.add_global(*name, Value::from_object(RequestGlobal(name)));
        }
        env
    }

    /// A template rendered while a page renders (a component a Rust
    /// template function renders, say) reads the page's request values
    /// through the `RequestGlobal`s; outside a page they are empty.
    #[test]
    fn request_globals_read_the_page_being_rendered() {
        let env = env();
        let page = "{% if auth %}[{{ auth.name }}]{% endif %}\
                    {% for key in flash %}{{ key }}={{ flash[key] }};{% endfor %}\
                    {{ t('hi') }}|{{ request }}|{% for e in errors %}{{ e }}{% endfor %}.";

        let outside = env
            .render_str(
                "{% if auth %}in{% endif %}[{{ request }}]{% for k in flash %}{{ k }}{% endfor %}",
                (),
            )
            .unwrap();
        assert_eq!(outside, "[]");
        let err = env.render_str("{{ t('hi') }}", ()).unwrap_err();
        assert!(
            err.to_string()
                .contains("`t` is only available while rendering a page"),
            "{err}"
        );

        let globals = minijinja::context! {
            auth => minijinja::context! { name => "Ann" },
            flash => minijinja::context! { status => "Saved" },
            t => Value::from_function(|key: String| format!("({key})")),
            request => "GET /",
            // A list: not a map, so it lists no keys.
            errors => vec!["a"],
        };
        let current = CurrentGlobals::set(globals);
        assert_eq!(
            env.render_str(page, ()).unwrap(),
            "[Ann]status=Saved;(hi)|GET /|."
        );
        drop(current);
        assert_eq!(env.render_str("{{ request }}", ()).unwrap(), "");
    }
}

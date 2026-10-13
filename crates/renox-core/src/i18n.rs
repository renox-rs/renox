//! Translations for app texts, and the language of each request.
//!
//! Renox ships English texts; an app adds its own languages as
//! `resources/lang/{locale}.json` files (`LANG_PATH`, e.g. `es.json`), nested or flat:
//!
//! ```json
//! { "products": { "saved": ":name guardado", "count": "Un producto|:count productos" } }
//! ```
//!
//! ```
//! # use renox::prelude::*;
//! // templates: {{ t('products.saved', name='Coffee') }}  {{ t('products.count', count=3) }}
//! async fn store(lang: Lang, session: Session, back: Back) -> Result<Back> {
//!     session.flash("status", lang.t("products.saved", &[("name", &"Coffee")]))?;
//!     Ok(back)
//! }
//! # fn demo(session: &Session) -> Result {
//! renox::i18n::remember_locale(&session, "en")?;   // this visitor's language from now on
//! # Ok(()) }
//! ```
//!
//! A request's language is the one stored with `remember_locale`, else `APP_LOCALE`.
//! Missing keys fall back to `APP_FALLBACK_LOCALE`, then to the key itself.
//! Renox's own texts can be replaced or translated from the same files:
//! `renox.validation.required`, `renox.validation.attributes.email` and
//! `renox.auth.login_title` (see the built-in pages for the other keys).

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use serde_json::Value;

use crate::{AppState, Result, Session};

const SESSION_KEY: &str = "_locale";
/// How often debug mode looks for changed translation files.
const RELOAD_CHECK: Duration = Duration::from_secs(1);

pub(crate) type Texts = Arc<HashMap<String, String>>;

struct Loaded {
    locales: HashMap<String, Texts>,
    fingerprint: Vec<(PathBuf, SystemTime)>,
    checked: Instant,
}

/// Every locale's texts, keyed by dotted path.
pub struct Translator {
    dir: PathBuf,
    reload: bool,
    loaded: RwLock<Loaded>,
}

fn fingerprint(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let modified = p
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (p, modified)
        })
        .collect();
    files.sort();
    files
}

fn flatten(prefix: &str, value: &Value, out: &mut HashMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&key, value, out);
            }
        }
        Value::String(text) => {
            out.insert(prefix.to_owned(), text.clone());
        }
        Value::Null => {}
        other => {
            out.insert(prefix.to_owned(), other.to_string());
        }
    }
}

fn parse(locales: &mut HashMap<String, Texts>, file: &str, text: &str) -> anyhow::Result<()> {
    let locale = file.strip_suffix(".json").unwrap_or(file).to_owned();
    let json: Value =
        serde_json::from_str(text).with_context(|| format!("{file} is not valid JSON"))?;
    let mut flat = HashMap::new();
    flatten("", &json, &mut flat);
    locales.insert(locale, Arc::new(flat));
    Ok(())
}

fn read(dir: &Path) -> anyhow::Result<HashMap<String, Texts>> {
    let mut locales = HashMap::new();
    for (path, _) in fingerprint(dir) {
        let locale = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let json: Value = serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON", path.display()))?;
        let mut flat = HashMap::new();
        flatten("", &json, &mut flat);
        locales.insert(locale, Arc::new(flat));
    }
    Ok(locales)
}

impl Translator {
    /// Uses translation files compiled into the binary.
    pub(crate) fn embedded(files: &[(&str, &str)]) -> anyhow::Result<Self> {
        let mut locales = HashMap::new();
        for (file, text) in files
            .iter()
            .filter(|(f, _)| f.ends_with(".json") && !f.contains('/'))
        {
            parse(&mut locales, file, text)?;
        }
        Ok(Self {
            dir: PathBuf::new(),
            reload: false,
            loaded: RwLock::new(Loaded {
                locales,
                fingerprint: Vec::new(),
                checked: Instant::now(),
            }),
        })
    }

    /// Loads `dir`; a file that isn't valid JSON is an error.
    pub(crate) fn load(dir: &Path, reload: bool) -> anyhow::Result<Self> {
        Ok(Self {
            dir: dir.to_path_buf(),
            reload,
            loaded: RwLock::new(Loaded {
                locales: read(dir)?,
                fingerprint: fingerprint(dir),
                checked: Instant::now(),
            }),
        })
    }

    fn refresh(&self) {
        if !self.reload {
            return;
        }
        {
            let loaded = self.loaded.read().unwrap_or_else(|e| e.into_inner());
            if loaded.checked.elapsed() < RELOAD_CHECK {
                return;
            }
        }
        let mut loaded = self.loaded.write().unwrap_or_else(|e| e.into_inner());
        loaded.checked = Instant::now();
        let now = fingerprint(&self.dir);
        if now != loaded.fingerprint {
            match read(&self.dir) {
                Ok(locales) => loaded.locales = locales,
                Err(err) => tracing::error!(error = ?err, "keeping the previous translations"),
            }
            loaded.fingerprint = now;
        }
    }

    /// The texts of one locale (empty if there is no file for it).
    pub(crate) fn texts(&self, locale: &str) -> Texts {
        self.refresh();
        let loaded = self.loaded.read().unwrap_or_else(|e| e.into_inner());
        loaded.locales.get(locale).cloned().unwrap_or_default()
    }

    /// Locales with a translation file.
    pub fn locales(&self) -> Vec<String> {
        self.refresh();
        let loaded = self.loaded.read().unwrap_or_else(|e| e.into_inner());
        let mut locales: Vec<_> = loaded.locales.keys().cloned().collect();
        locales.sort();
        locales
    }

    /// The text for `key` in `locale`, else in `fallback`, else the key itself.
    pub fn get(&self, locale: &str, fallback: &str, key: &str) -> String {
        if let Some(text) = self.texts(locale).get(key) {
            return text.clone();
        }
        self.texts(fallback)
            .get(key)
            .cloned()
            .or_else(|| {
                builtin(locale)
                    .or_else(|| builtin(fallback))?
                    .get(key)
                    .map(|t| (*t).to_owned())
            })
            .unwrap_or_else(|| key.to_owned())
    }
}

/// A built-in text of `locale` (English when it has none), else the key.
pub(crate) fn builtin_text(locale: &str, key: &str) -> String {
    builtin(locale)
        .and_then(|texts| texts.get(key))
        .or_else(|| builtin("en")?.get(key))
        .map_or_else(|| key.to_owned(), |t| (*t).to_owned())
}

/// Texts Renox's own templates use (the UI kit), in English; an app's
/// `lang/*.json` can change them or add other languages.
fn builtin(locale: &str) -> Option<&'static HashMap<&'static str, &'static str>> {
    static EN: std::sync::LazyLock<HashMap<&str, &str>> = std::sync::LazyLock::new(|| {
        HashMap::from([
            ("ui.optional", "optional"),
            ("ui.cancel", "Cancel"),
            ("ui.close", "Close"),
            ("ui.dismiss", "Dismiss"),
            ("ui.request_failed", "That didn't work. Try again."),
            ("ui.more", "More"),
            ("ui.skip", "Skip to content"),
            ("ui.main_navigation", "Main"),
            ("ui.sections", "Sections"),
            ("ui.errors_title", "Please check the highlighted fields."),
            ("ui.show_password", "Show password"),
            ("ui.hide_password", "Hide password"),
            ("ui.copy", "Copy"),
            ("ui.copied", "Copied"),
            ("ui.choose_file", "Choose a file or drop it here"),
            ("ui.choose_files", "Choose files or drop them here"),
            ("ui.current_file", "Current file"),
            ("ui.choose_date", "Choose a date"),
            (
                "ui.date_unavailable",
                "That day can't be chosen: pick another one.",
            ),
            ("ui.previous_month", "Previous month"),
            ("ui.next_month", "Next month"),
            ("ui.search", "Search"),
            ("ui.no_results", "No matches"),
            ("ui.remove", "Remove"),
            ("ui.add_row", "Add row"),
            ("ui.move_up", "Move up"),
            ("ui.move_down", "Move down"),
            ("ui.key", "Key"),
            ("ui.value", "Value"),
            ("ui.back", "Back"),
            ("ui.actions", "Actions"),
            ("ui.import.file", "CSV file"),
            ("ui.import.columns", "Columns:"),
            ("ui.import.template", "Download a template"),
            ("ui.import.row", "Row"),
            ("ui.import.problem", "Problem"),
            ("ui.import.more", "…and :count more rows."),
            ("ui.import.done", "One row imported.|:count rows imported."),
            (
                "ui.import.partial",
                ":imported imported, one row left out.|:imported imported, :count rows left out.",
            ),
            (
                "ui.import.nothing",
                "Nothing was imported: one row has errors.|Nothing was imported: :count rows have errors.",
            ),
            ("ui.import.not_utf8", "The file must be CSV text (UTF-8)."),
            ("ui.import.empty", "The file has no rows to import."),
            ("ui.import.too_many", "The file has more than :count rows."),
            (
                "ui.import.duplicate",
                "A value of this row must be unique, and is already taken.",
            ),
            ("ui.import.not_saved", "This row could not be saved."),
            ("ui.next", "Next"),
            ("ui.since.now", "just now"),
            ("ui.since.past", ":time ago"),
            ("ui.since.future", "in :time"),
            ("ui.since.minutes", "a minute|:count minutes"),
            ("ui.since.hours", "an hour|:count hours"),
            ("ui.since.days", "a day|:count days"),
            ("ui.since.months", "a month|:count months"),
            ("ui.since.years", "a year|:count years"),
            ("ui.show_more", "Show :count more"),
            ("ui.yes", "Yes"),
            ("ui.no", "No"),
            ("ui.loading", "Loading…"),
            ("ui.chart.show_data", "Show the data"),
            ("ui.chart.other", "Other"),
            ("ui.chart.series", "Series"),
            ("ui.chart.size", "Size"),
            ("ui.chart.axes", ":y by :x"),
            ("ui.chart.less", "Less"),
            ("ui.chart.more", "More"),
            ("ui.stat.vs_previous", "vs previous period"),
            ("ui.period.label", "Period"),
            ("ui.period.7d", "7 days"),
            ("ui.period.30d", "30 days"),
            ("ui.period.90d", "90 days"),
            ("ui.period.12w", "12 weeks"),
            ("ui.period.12m", "12 months"),
            ("ui.period.mtd", "This month"),
            ("ui.period.ytd", "This year"),
            ("ui.period.custom", "Custom"),
            ("ui.period.custom_label", "Custom range"),
            ("ui.period.from", "From"),
            ("ui.period.to", "To"),
            ("ui.period.apply", "Apply"),
            (
                "ui.period.invalid",
                "Choose a start date on or before the end date, at most three years apart.",
            ),
            ("ui.notifications.title", "Notifications"),
            ("ui.notifications.bell", "Notifications"),
            ("ui.notifications.unread", ":count unread"),
            ("ui.notifications.unread_marker", "unread"),
            ("ui.notifications.mark_all_read", "Mark all as read"),
            ("ui.notifications.clear", "Clear all"),
            ("ui.notifications.mark_read", "Mark as read"),
            ("ui.notifications.mark_unread", "Mark as unread"),
            ("ui.notifications.delete", "Delete"),
            ("ui.notifications.empty", "No notifications"),
            ("ui.notifications.empty_hint", "You're all caught up."),
            ("ui.notifications.see_all", "See all"),
            ("ui.notifications.older", "Older"),
            ("ui.notifications.newest", "Newest"),
            ("ui.notifications.open", "Open"),
            ("ui.notifications.loading", "Loading…"),
            ("ui.searching", "Searching…"),
            ("ui.load_failed", "Couldn't load the options."),
            ("ui.add_option", "Add “:value”"),
            ("ui.edit", "Edit"),
            ("ui.editing", "Editing “:value”: Enter saves, Esc cancels."),
            ("ui.save_failed", "Couldn't save it."),
            ("ui.grid.columns", "Columns"),
            ("ui.grid.filter", "Filter"),
            ("ui.grid.apply", "Apply"),
            ("ui.grid.clear", "Clear"),
            ("ui.grid.clear_all", "Clear filters"),
            ("ui.grid.contains", "Contains"),
            ("ui.grid.starts", "Starts with"),
            ("ui.grid.ends", "Ends with"),
            ("ui.grid.equals", "Equals"),
            (
                "ui.grid.pattern_hint",
                "% matches anything: cof% starts with “cof”.",
            ),
            ("ui.grid.min", "From"),
            ("ui.grid.max", "To"),
            ("ui.grid.from", "From"),
            ("ui.grid.to", "To"),
            ("ui.grid.yes", "Yes"),
            ("ui.grid.no", "No"),
            ("ui.grid.rows", "Rows"),
            ("ui.grid.row_word", "row|rows"),
            ("ui.grid.of", "of"),
            ("ui.grid.previous", "Previous page"),
            ("ui.grid.next", "Next page"),
            ("ui.grid.page", "Page"),
            ("ui.grid.empty", "Nothing matches these filters."),
            ("ui.grid.empty_all", "No rows yet."),
            ("ui.grid.sort", "Sort"),
            ("ui.grid.show", "Show on this screen"),
            ("ui.grid.freeze", "Freeze"),
            ("ui.grid.freeze_left", "Left"),
            ("ui.grid.freeze_none", "None"),
            ("ui.grid.freeze_right", "Right"),
            ("ui.grid.move_up", "Move up"),
            ("ui.grid.move_down", "Move down"),
            ("ui.grid.reset", "Reset columns"),
            ("ui.grid.filtered", "filtered"),
            ("ui.grid.loading", "Loading…"),
            ("ui.grid.advanced", "Advanced filter"),
            ("ui.grid.match", "Show rows matching"),
            ("ui.grid.match_all", "all of the rules"),
            ("ui.grid.match_any", "any of the rules"),
            ("ui.grid.add_rule", "Add a rule"),
            ("ui.grid.remove_rule", "Remove the rule"),
            ("ui.grid.rule_column", "Column"),
            ("ui.grid.rule_op", "Condition"),
            ("ui.grid.rule_value", "Value"),
            ("ui.grid.rule_word", "rule|rules"),
            ("ui.grid.op.contains", "contains"),
            ("ui.grid.op.not_contains", "doesn't contain"),
            ("ui.grid.op.equals", "is"),
            ("ui.grid.op.not_equals", "isn't"),
            ("ui.grid.op.starts", "starts with"),
            ("ui.grid.op.ends", "ends with"),
            ("ui.grid.op.empty", "is empty"),
            ("ui.grid.op.not_empty", "isn't empty"),
            ("ui.grid.op.eq", "="),
            ("ui.grid.op.ne", "≠"),
            ("ui.grid.op.gt", ">"),
            ("ui.grid.op.gte", "≥"),
            ("ui.grid.op.lt", "<"),
            ("ui.grid.op.lte", "≤"),
            ("ui.grid.op.on", "is on"),
            ("ui.grid.op.before", "is before"),
            ("ui.grid.op.after", "is after"),
            ("ui.grid.op.is_true", "is yes"),
            ("ui.grid.op.is_false", "is no"),
            ("ui.grid.op.is", "is"),
            ("ui.grid.op.is_not", "isn't"),
            ("ui.grid.copy", "Copy"),
            ("ui.grid.copied", "Copied"),
            ("ui.grid.sum", "Total"),
            ("ui.grid.average", "Average"),
            ("ui.grid.range", "Range"),
            ("ui.grid.count", "Count"),
            ("ui.grid.group_by", "Group"),
            ("ui.grid.no_groups", "No groups"),
            ("ui.grid.all_rows", "All rows"),
            ("ui.grid.group_rows", "This group"),
            ("ui.grid.select_page", "Select every row on this page"),
            ("ui.grid.select_row", "Select row"),
            ("ui.grid.selected", "selected"),
            ("ui.grid.select_matching", "Select all :total matching"),
            ("ui.grid.select_none", "Clear selection"),
            ("ui.grid.actions", "Actions"),
            ("ui.grid.confirm", "Continue"),
            ("ui.grid.all_matching", "all matching"),
            ("ui.grid.search", "Search"),
            ("ui.grid.active_filters", "Active filters"),
            ("ui.grid.open", "Open"),
            ("ui.grid.resize", "Column width"),
            (
                "ui.grid.resize_hint",
                "Drag to resize, double-click for the automatic width; drag the heading to move the column",
            ),
            ("ui.grid.export", "Export"),
            (
                "ui.grid.export_hint",
                "Every row the filters match, in your columns.",
            ),
            ("ui.grid.print", "Print or save as PDF"),
            ("ui.grid.printed", "printed"),
            ("ui.grid.back", "Back"),
            ("ui.grid.created", "Created"),
            ("ui.grid.updated", "Last updated"),
            ("ui.grid.details", "Details"),
            ("ui.grid.edit", "Edit row"),
            ("ui.grid.save", "Save"),
            ("ui.grid.move", "Drag to reorder (or use the arrow keys)"),
            ("ui.grid.sort_to_move", "Sort by order to move rows"),
            ("ui.grid.row_tools", "Row"),
        ])
    });
    match locale {
        "en" => Some(&EN),
        _ => None,
    }
}

/// Fills `:name` (and `:Name`, capitalised) placeholders, and picks the
/// singular or plural side of `one|many` texts when `count` is given, or
/// the matching range of `{0} none|[1,5] a few|[6,*] many` texts (Laravel's
/// `trans_choice` ranges: `{n}` exactly, `[a,b]` from a to b, `*` open).
pub fn format(text: &str, params: &[(&str, String)], count: Option<i64>) -> String {
    let text = match (count, text.split_once('|')) {
        (Some(count), Some(_)) if has_ranges(text) => choose_range(text, count),
        (Some(1), Some((one, _))) => one.to_owned(),
        (Some(_), Some((_, many))) => many.to_owned(),
        _ => text.to_owned(),
    };
    let mut params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    if let Some(count) = count {
        params.push(("count".into(), count.to_string()));
    }
    // Longest names first, so `:name` doesn't eat the start of `:names`.
    params.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    let mut out = text;
    for (name, value) in params {
        let mut capitalised = value.clone();
        if let Some(first) = capitalised.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        let mut upper_name = name.clone();
        if let Some(first) = upper_name.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        out = out
            .replace(&format!(":{upper_name}"), &capitalised)
            .replace(&format!(":{name}"), &value);
    }
    out
}

/// Whether a `|` text uses ranges (`{0} …|[1,*] …`).
fn has_ranges(text: &str) -> bool {
    text.split('|')
        .any(|part| part.trim_start().starts_with(['{', '[']))
}

/// The part of a ranged text whose range holds `count`; the last part when
/// none does. The range itself is left out.
fn choose_range(text: &str, count: i64) -> String {
    let parts: Vec<&str> = text.split('|').collect();
    for part in &parts {
        let part = part.trim_start();
        let (range, rest) = if let Some(inner) = part.strip_prefix('{') {
            match inner.split_once('}') {
                Some((exact, rest)) => ((exact, exact), rest),
                None => continue,
            }
        } else if let Some(inner) = part.strip_prefix('[') {
            match inner.split_once(']').and_then(|(range, rest)| {
                range.split_once(',').map(|(from, to)| ((from, to), rest))
            }) {
                Some(found) => found,
                None => continue,
            }
        } else {
            continue;
        };
        let bound = |text: &str, open: i64| match text.trim() {
            "*" => Some(open),
            number => number.parse::<i64>().ok(),
        };
        let (Some(from), Some(to)) = (bound(range.0, i64::MIN), bound(range.1, i64::MAX)) else {
            continue;
        };
        if (from..=to).contains(&count) {
            return rest.trim_start().to_owned();
        }
    }
    let last = parts.last().copied().unwrap_or_default().trim_start();
    // Without a matching range, the last text, minus its range.
    match last.find([']', '}']) {
        Some(end) if last.starts_with(['{', '[']) => last[end + 1..].trim_start().to_owned(),
        _ => last.to_owned(),
    }
}

/// The language chosen for the current request, kept in the request's
/// extensions by Renox's language middleware (session choice, then
/// `Accept-Language` with `App::detect_locale`, then `APP_LOCALE`).
///
/// A layer added with `App::layer` runs after it and may change the language
/// of the rest of the request, e.g. to the signed-in user's own:
///
/// ```
/// # use renox::prelude::*;
/// use renox::i18n::RequestLocale;
/// use renox::axum::{extract::Request, middleware::Next};
///
/// async fn account_language(mut req: Request, next: Next) -> Response {
///     if let Some(chosen) = req.extensions().get::<RequestLocale>() {
///         println!("chosen so far: {}", chosen.as_str());
///     }
///     RequestLocale::switch(&mut req, "es");
///     next.run(req).await
/// }
/// # let _ = App::new().layer(renox::axum::middleware::from_fn(account_language));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RequestLocale(String);

impl RequestLocale {
    /// A locale value, e.g. `es`.
    pub fn new(locale: impl Into<String>) -> Self {
        Self(locale.into())
    }

    /// The locale, e.g. `en`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Makes `locale` the language of the rest of this request: the `Lang`
    /// extractor, validation messages, `t()` in views, mails and
    /// [`current_locale`] all follow. Call it from a layer; it does not
    /// check that the locale has a translation file (texts fall back to
    /// `APP_FALLBACK_LOCALE`, then to the key).
    pub fn switch(req: &mut Request, locale: &str) {
        set_current_locale(locale);
        req.extensions_mut().insert(RequestLocale::new(locale));
    }
}

impl std::ops::Deref for RequestLocale {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

/// The request's language in [`crate::context`], for code without the
/// request (mail views, notifications).
#[derive(Debug, Clone)]
struct ContextLocale(String);

thread_local! {
    /// Set while a notification builds its messages for a recipient.
    static OVERRIDE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The language to use now: the recipient's while a notification is built,
/// else the current request's (or the one a job set with
/// [`with_locale`]), else `APP_LOCALE`.
pub fn current_locale(state: &AppState) -> String {
    OVERRIDE
        .with(|o| o.borrow().clone())
        .or_else(|| crate::context::get::<ContextLocale>().map(|l| l.0))
        .unwrap_or_else(|| state.config.locale.clone())
}

/// Runs `f` (synchronous code, e.g. building a mail) in `locale`; `None`
/// leaves the language as it is.
pub fn with_locale<T>(locale: Option<&str>, f: impl FnOnce() -> T) -> T {
    let Some(locale) = locale else { return f() };
    let previous = OVERRIDE.with(|o| o.borrow_mut().replace(locale.to_owned()));
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            OVERRIDE.with(|o| *o.borrow_mut() = previous);
        }
    }
    let _restore = Restore(previous);
    f()
}

/// Makes `locale` the language of the rest of this request, job or task
/// (e.g. a job that mails a user in their language).
pub fn set_current_locale(locale: &str) {
    crate::context::set(ContextLocale(locale.to_owned()));
}

/// Makes `locale` this visitor's language from the next request on.
pub fn remember_locale(session: &Session, locale: &str) -> Result {
    session.put(SESSION_KEY, locale)
}

/// Picks the request's language: the session's choice if Renox knows the
/// locale (a translation file or a built-in language), else `APP_LOCALE`.
pub(crate) async fn middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    // In a block: nothing borrowing `req` may live across `next.run(req)` (§4.2).
    let locale = {
        let available =
            |locale: &str| locale == "en" || state.translator.locales().iter().any(|l| l == locale);
        let chosen = req
            .extensions()
            .get::<Session>()
            .and_then(|s| s.get::<String>(SESSION_KEY))
            .filter(|locale| available(locale));
        let chosen = chosen.or_else(|| {
            if !state.detect_locale {
                return None;
            }
            req.headers()
                .get(axum::http::header::ACCEPT_LANGUAGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|header| from_accept_language(header, available))
        });
        chosen.unwrap_or_else(|| state.config.locale.clone())
    };
    set_current_locale(&locale);
    req.extensions_mut().insert(RequestLocale::new(locale));
    let mut res = next.run(req).await;
    if state.detect_locale {
        res.headers_mut().append(
            axum::http::header::VARY,
            axum::http::HeaderValue::from_static("Accept-Language"),
        );
    }
    res
}

/// The first language of an `Accept-Language` header (`es-MX,es;q=0.9,
/// en;q=0.8`) that `available` accepts, by quality: the whole tag
/// (`pt-br`), then its language (`pt`).
fn from_accept_language(header: &str, available: impl Fn(&str) -> bool) -> Option<String> {
    let mut wanted: Vec<(f32, usize, String)> = header
        .split(',')
        .enumerate()
        .filter_map(|(order, item)| {
            let mut parts = item.split(';');
            let tag = parts.next()?.trim().to_ascii_lowercase();
            let quality = parts
                .find_map(|p| p.trim().strip_prefix("q="))
                .map_or(Some(1.0), |q| q.trim().parse::<f32>().ok())?;
            (!tag.is_empty() && tag != "*" && quality > 0.0).then_some((quality, order, tag))
        })
        .collect();
    // Highest quality first; the header's order among equals.
    wanted.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    wanted.into_iter().find_map(|(_, _, tag)| {
        if available(&tag) {
            return Some(tag);
        }
        let language = tag.split(['-', '_']).next()?;
        available(language).then(|| language.to_owned())
    })
}

pub(crate) fn request_locale(extensions: &axum::http::Extensions, state: &AppState) -> String {
    extensions
        .get::<RequestLocale>()
        .map(|l| l.0.clone())
        .unwrap_or_else(|| state.config.locale.clone())
}

/// The current request's language and its texts.
///
/// ```
/// # use renox::prelude::*;
/// async fn index(lang: Lang) -> String { lang.t("welcome", &[("name", &"Anna")]) }
/// ```
#[derive(Clone)]
pub struct Lang {
    /// The request's locale, e.g. `en`.
    pub locale: String,
    pub(crate) state: AppState,
}

impl Lang {
    /// The request's language, for code that has the state but no request.
    pub(crate) fn of(state: &AppState, locale: &str) -> Self {
        Self {
            locale: locale.to_owned(),
            state: state.clone(),
        }
    }

    pub(crate) fn texts(&self) -> Texts {
        self.state.translator.texts(&self.locale)
    }

    /// Translates `key`, replacing `:name` placeholders with `params`; falls
    /// back to `APP_FALLBACK_LOCALE`, then to the key itself.
    pub fn t(&self, key: &str, params: &[(&str, &dyn Display)]) -> String {
        let params: Vec<(&str, String)> = params.iter().map(|(k, v)| (*k, v.to_string())).collect();
        format(&self.raw(key), &params, None)
    }

    /// Like `t`, choosing the `one|many` side by `count` and filling `:count`.
    pub fn choice(&self, key: &str, count: i64, params: &[(&str, &dyn Display)]) -> String {
        let params: Vec<(&str, String)> = params.iter().map(|(k, v)| (*k, v.to_string())).collect();
        format(&self.raw(key), &params, Some(count))
    }

    fn raw(&self, key: &str) -> String {
        self.state
            .translator
            .get(&self.locale, &self.state.config.fallback_locale, key)
    }
}

impl AppState {
    /// The texts of `locale`, for code without a request (a job, a mail
    /// to someone in another language).
    pub fn lang(&self, locale: &str) -> Lang {
        Lang::of(self, locale)
    }

    /// The texts of the [`current_locale`].
    pub fn current_lang(&self) -> Lang {
        Lang::of(self, &current_locale(self))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Lang {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> std::result::Result<Self, Infallible> {
        let state = parts
            .extensions
            .get::<AppState>()
            .cloned()
            .expect("the auth middleware puts AppState in every request");
        Ok(Self {
            locale: request_locale(&parts.extensions, &state),
            state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renox's own texts: English, English for a language without its own,
    /// and the key itself for a text there isn't.
    #[test]
    fn builtin_texts_fall_back_to_english_then_the_key() {
        assert_eq!(super::builtin_text("en", "ui.cancel"), "Cancel");
        assert_eq!(super::builtin_text("xx", "ui.cancel"), "Cancel");
        assert_eq!(
            super::builtin_text("en", "ui.no-such-text"),
            "ui.no-such-text"
        );
    }

    #[test]
    fn accept_language_picks_the_first_available() {
        let ours = |l: &str| matches!(l, "en" | "es" | "pt-br");
        assert_eq!(
            from_accept_language("es-MX,es;q=0.9,en;q=0.8", ours).as_deref(),
            Some("es")
        );
        assert_eq!(
            from_accept_language("fr, en;q=0.5", ours).as_deref(),
            Some("en")
        );
        assert_eq!(
            from_accept_language("en;q=0.2, es;q=0.9", ours).as_deref(),
            Some("es")
        );
        assert_eq!(
            from_accept_language("PT-BR", ours).as_deref(),
            Some("pt-br")
        );
        assert_eq!(from_accept_language("de, fr;q=0.8", ours), None);
        assert_eq!(from_accept_language("es;q=0, *", ours), None);
        assert_eq!(from_accept_language("", ours), None);
        assert_eq!(from_accept_language("en;q=abc", ours), None);
    }

    #[test]
    fn formats_placeholders_and_plurals() {
        let p = [("name", "coffee".to_owned())];
        assert_eq!(format(":Name, :name!", &p, None), "Coffee, coffee!");
        assert_eq!(
            format("One product|:count products", &[], Some(1)),
            "One product"
        );
        assert_eq!(
            format("One product|:count products", &[], Some(4)),
            "4 products"
        );
        // Like Laravel: 0 and negative counts take the plural form.
        assert_eq!(
            format("One product|:count products", &[], Some(0)),
            "0 products"
        );
        assert_eq!(format("one|:count many", &[], Some(-1)), "-1 many");
        // Without a plural form the text is used as it is.
        assert_eq!(format(":count item", &[], Some(3)), "3 item");
        // Laravel's ranges.
        let ranged = "{0} No messages yet|[1,5] :count messages|[6,*] Many messages (:count)";
        assert_eq!(format(ranged, &[], Some(0)), "No messages yet");
        assert_eq!(format(ranged, &[], Some(1)), "1 messages");
        assert_eq!(format(ranged, &[], Some(5)), "5 messages");
        assert_eq!(format(ranged, &[], Some(40)), "Many messages (40)");
        assert_eq!(
            format("[*,-1] minus|{0} zero|[1,*] :count", &[], Some(-3)),
            "minus"
        );
        // No range matches: the last text.
        assert_eq!(format("{1} one|{2} two", &[], Some(7)), "two");
        // Without a count, ranged texts are left alone.
        assert_eq!(format("{0} a|[1,*] b", &[], None), "{0} a|[1,*] b");
        assert_eq!(
            format(
                ":name :names",
                &[("name", "a".into()), ("names", "b".into())],
                None
            ),
            "a b"
        );
    }

    #[test]
    fn flattens_nested_files() {
        let json = serde_json::json!({ "a": { "b": "x", "n": 3 }, "c": "y" });
        let mut out = HashMap::new();
        flatten("", &json, &mut out);
        assert_eq!(out["a.b"], "x");
        assert_eq!(out["a.n"], "3");
        assert_eq!(out["c"], "y");
    }

    /// Parts that aren't ranges, or whose ranges don't read, are skipped;
    /// with no match the last part is used as it is.
    #[test]
    fn ranges_that_dont_read_are_skipped() {
        let text = "{0 unclosed|[1 no end|plain|[a,b] letters|[1,2] few|the rest";
        assert_eq!(choose_range(text, 1), "few");
        assert_eq!(choose_range(text, 9), "the rest");
    }

    /// A lang file broken while the app runs (a half-saved edit): the
    /// previous translations stay until the file reads again.
    #[test]
    fn a_lang_file_broken_while_reloading_keeps_the_previous_texts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("es.json");
        std::fs::write(&file, r#"{"hi": "Hola"}"#).unwrap();
        let translator = Translator::load(dir.path(), true).unwrap();
        assert_eq!(translator.get("es", "en", "hi"), "Hola");
        std::fs::write(&file, r#"{"hi": "Hola de nue"#).unwrap();
        std::thread::sleep(RELOAD_CHECK + Duration::from_millis(100));
        assert_eq!(translator.get("es", "en", "hi"), "Hola");
        std::fs::write(&file, r#"{"hi": "Hola de nuevo"}"#).unwrap();
        std::thread::sleep(RELOAD_CHECK + Duration::from_millis(100));
        assert_eq!(translator.get("es", "en", "hi"), "Hola de nuevo");
    }
}

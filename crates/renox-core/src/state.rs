use std::fmt::Display;
use std::sync::Arc;

use cookie::Key;

use crate::auth::{Gates, LoginThrottle};
use crate::cache::Cache;
use crate::db::Db;
use crate::events::Listeners;
use crate::i18n::Translator;
use crate::mail::Mailer;
use crate::queue::Queue;
use crate::storage::Storage;
use crate::{Config, Result, RouteTable, Views};

/// Shared state available to every handler through `State<AppState>`.
///
/// One `Arc` around [`AppStateInner`]: every middleware layer, the request's
/// extensions and each handler's `State` clone it, so a clone is one atomic
/// increment (it was a struct of about 35 fields, cloned tens of times per
/// request: #334). Read its parts as fields, through `Deref`:
/// `state.db`, `state.config`, `state.queue`…
#[derive(Clone)]
pub struct AppState(Arc<AppStateInner>);

impl std::ops::Deref for AppState {
    type Target = AppStateInner;

    fn deref(&self) -> &AppStateInner {
        &self.0
    }
}

impl AppState {
    /// Wraps the parts made at boot (`App::boot`).
    pub(crate) fn new(inner: AppStateInner) -> Self {
        AppState(Arc::new(inner))
    }
}

/// What an [`AppState`] holds. Apps read these as the state's fields
/// (`state.db`); only Renox builds one, at boot.
#[non_exhaustive]
pub struct AppStateInner {
    /// The configuration.
    pub config: Arc<Config>,
    /// Named routes, to build URLs (`state.url(name, params)`).
    pub(crate) routes: Arc<RouteTable>,
    /// The template engine (`view()` renders, `mail_view` for mail).
    pub(crate) views: Views,
    /// The database connection pool.
    pub db: Db,
    /// Sends mail (`MAIL_MAILER`).
    pub mailer: Mailer,
    /// The app's other mailers (`App::mailer`), by name.
    pub(crate) mailers: Arc<std::collections::HashMap<String, Mailer>>,
    /// Dispatches jobs.
    pub queue: Queue,
    /// The cache (`CACHE_STORE`).
    pub cache: Cache,
    /// The file storage disk (`STORAGE_DISK`).
    pub storage: Storage,
    /// The app's other disks (`App::disk`), by name; see [`AppState::disk`].
    pub(crate) disks: Arc<std::collections::HashMap<String, Storage>>,
    /// Calls other services; faked in tests (`TestApp::fake_http`).
    pub http: crate::http::Http,
    /// Translations from `LANG_PATH` and the built-in ones (`Lang`, `t()`).
    pub(crate) translator: Arc<Translator>,
    /// Live reload, only while developing locally.
    pub(crate) live: Option<Arc<crate::live::Live>>,
    /// Wakes the users' open notification streams.
    pub(crate) notification_hub: Arc<crate::auth::notifications::Hub>,
    pub(crate) listeners: Listeners,
    pub(crate) key: Key,
    /// With `SESSION_DRIVER=database` in tests, sessions are kept here
    /// instead of the table (keyed by the id's hash).
    pub(crate) session_mirror:
        Option<Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>>,
    pub(crate) gates: Gates,
    pub(crate) async_gates: Arc<std::collections::HashMap<String, crate::auth::AsyncGate>>,
    pub(crate) throttle: Arc<LoginThrottle>,
    /// A module's second login step (`Registry::second_factor`).
    pub(crate) second_factor: Option<Arc<crate::auth::second_factor::SecondFactor>>,
    /// Sections other modules add to `/account` (`Registry::account_section`).
    pub(crate) account_sections: Arc<Vec<crate::auth::account::AccountSection>>,
    /// The `Auth` module's settings, when the app has it.
    pub(crate) auth: Option<Arc<crate::auth::module::Settings>>,
    /// `App::detect_locale`: the browser's `Accept-Language` picks the locale.
    pub(crate) detect_locale: bool,
    pub(crate) security: Arc<crate::security::Security>,
    pub(crate) webhooks: crate::webhook::Handlers,
    /// Read by the live component route (#433).
    #[allow(dead_code)]
    pub(crate) live_components: crate::live_component::LiveMap,
    /// Values every view gets (`App::share`).
    pub(crate) shares: Arc<Vec<(String, crate::view::ShareFn)>>,
    /// The app's notification channels (`App::channel`).
    pub(crate) channels:
        Arc<std::collections::HashMap<String, crate::auth::notifications::ChannelFn>>,
    /// The app's own values (`App::provide`).
    pub(crate) provided: crate::provided::ProvidedMap,
    /// `/_renox/debug`'s recent requests, while developing locally.
    pub(crate) inspector: Option<Arc<crate::inspector::Inspector>>,
    /// Named rate limiters (`App::rate_limiter`).
    pub(crate) limiters: Arc<std::collections::HashMap<String, crate::rate_limit::NamedLimiter>>,
    /// The app's error reporters (`App::report`).
    pub(crate) reporters: Arc<Vec<crate::report::ReportFn>>,
    /// What tests asked to record instead of doing (`TestApp::fake_events`, …).
    pub(crate) fakes: Arc<Fakes>,
}

/// A notification a test recorded instead of sending.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SentNotification {
    /// `Notification::kind`.
    pub kind: &'static str,
    /// Who it was sent to.
    pub to: crate::auth::Recipient,
}

/// An event a test recorded instead of sending it to the open pages
/// (`TestApp::fake_broadcasts`, `AppState::broadcast`).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SentBroadcast {
    /// The user whose pages it was for; `None` for every page.
    pub user_id: Option<i64>,
    /// The DOM event's name.
    pub event: String,
    /// Its data (the DOM event's `detail`).
    pub data: serde_json::Value,
}

/// Recorders for `TestApp::fake_events`, `fake_notifications` and
/// `fake_broadcasts`.
#[derive(Default)]
pub(crate) struct Fakes {
    pub events: std::sync::Mutex<Option<Vec<Box<dyn std::any::Any + Send>>>>,
    pub notifications: std::sync::Mutex<Option<Vec<SentNotification>>>,
    pub broadcasts: std::sync::Mutex<Option<Vec<SentBroadcast>>>,
}

impl Fakes {
    /// Records `event` if events are faked; returns whether it did.
    pub(crate) fn record_event<E: Send + 'static>(&self, event: E) -> bool {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        match events.as_mut() {
            Some(list) => {
                list.push(Box::new(event));
                true
            }
            None => false,
        }
    }

    /// Records a broadcast if broadcasts are faked; returns whether it did.
    pub(crate) fn record_broadcast(&self, broadcast: SentBroadcast) -> bool {
        let mut sent = self.broadcasts.lock().unwrap_or_else(|e| e.into_inner());
        match sent.as_mut() {
            Some(list) => {
                list.push(broadcast);
                true
            }
            None => false,
        }
    }

    /// Records a notification if notifications are faked.
    pub(crate) fn record_notification(
        &self,
        kind: &'static str,
        to: &crate::auth::Recipient,
    ) -> bool {
        let mut sent = self.notifications.lock().unwrap_or_else(|e| e.into_inner());
        match sent.as_mut() {
            Some(list) => {
                list.push(SentNotification {
                    kind,
                    to: to.clone(),
                });
                true
            }
            None => false,
        }
    }
}

impl AppState {
    /// The URL path of a named route, e.g. `state.url("products.show", &[&id])`.
    pub fn url(&self, name: &str, params: &[&dyn Display]) -> Result<String> {
        Ok(self.routes.url(name, params)?)
    }

    /// Encrypts `plain` with `APP_KEY` (AES-256-GCM), e.g. an API secret a
    /// user saves, before storing it. The result is base64 text; only
    /// [`AppState::decrypt`] with the same key reads it back, and any change
    /// to it is detected. Rotating `APP_KEY` makes old values unreadable.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # fn demo(state: AppState) -> Result {
    /// let sealed = state.encrypt("sk_live_123");
    /// assert_eq!(state.decrypt(&sealed)?, "sk_live_123");
    /// # Ok(()) }
    /// ```
    pub fn encrypt(&self, plain: &str) -> String {
        crate::crypto::seal(&self.key, plain)
    }

    /// Reads a value from [`AppState::encrypt`]. Fails if it was changed or
    /// sealed with another key.
    pub fn decrypt(&self, sealed: &str) -> Result<String> {
        Ok(crate::crypto::open(&self.key, sealed)?)
    }
}

impl AppState {
    /// A disk the app added with [`App::disk`](crate::App::disk), e.g.
    /// `state.disk_named("backups")?`; an unknown name is an error (500).
    pub fn disk_named(&self, name: &str) -> Result<&Storage> {
        self.disks.get(name).ok_or_else(|| {
            anyhow::anyhow!("no disk named `{name}`: add it with `App::disk(\"{name}\", …)`").into()
        })
    }
}

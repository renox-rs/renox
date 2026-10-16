//! Typed settings kept in the database: a company's name and tax number, a
//! document number format, which features are on. They are read quickly from
//! any handler, job or command, edited by an administrator, and changed
//! without a new `.env` or a deploy. Opt in with `App::module(Settings)`,
//! which adds the `settings` table and the `settings:get`, `settings:set` and
//! `settings:list` commands.
//!
//! A group of settings is a plain struct with defaults:
//!
//! ```
//! use renox::prelude::*;
//! use renox::settings::{Settings, SettingsGroup};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, Validate, Default, Clone)]
//! pub struct Company {
//!     #[validate(required, max(120))]
//!     pub name: String,
//!     pub tax_number: Option<String>,
//!     #[validate(max(40))]
//!     pub invoice_format: String, // "INV-{yyyy}-{0000}"
//! }
//!
//! impl SettingsGroup for Company {
//!     const KEY: &'static str = "company"; // one row per group
//! }
//!
//! # fn app() -> App {
//! App::new()
//!     .module(Settings)          // the table and the commands
//!     .settings::<Company>()     // declares the group
//! # }
//!
//! # async fn demo(state: AppState, user: User) -> Result {
//! // Read (cached; the first read of a group is one query):
//! let company = state.settings::<Company>().await?; // Arc<Company>, the defaults until saved
//! # let _ = company;
//!
//! // Write (validated with the struct's rules, audited when `Audit` is on):
//! let mut company = Company { name: "Acme".into(), ..Company::default() };
//! company.tax_number = Some("12.345.678".into());
//! state.save_settings(&company, Some(&user)).await?;
//! # Ok(()) }
//! ```
//!
//! In a handler, the [`Setting`] extractor reads a group; in templates,
//! `settings.company.name` works for the groups declared with
//! `App::settings_in_views`. The guide is docs/settings.md.
//!
//! # How it is cached
//!
//! Each process keeps the groups it has read. A save replaces the group in
//! the process that wrote it at once and stamps a version row in the `cache`
//! table (`renox:settings:<key>`), so with several servers on one database
//! the others notice: a process asks for that stamp (one small query) at
//! most once a second per group and reloads the group when it changed.
//! Between those checks a read costs no query.
//!
//! # Rules
//!
//! - A group's [`Validate`] rules run on every save
//!   (`rules` only; the `authorize` and `after` hooks need a request).
//!   Invalid values are rejected with the rules' messages.
//! - `ENCRYPTED = true` stores the group sealed with `APP_KEY`, as
//!   [`Encrypted`] columns are, for gateway keys and
//!   other secrets. `settings:get` and `settings:list` print `<sealed>` for
//!   it unless `--reveal` is given.
//! - Fields a stored group lacks (added to the struct later) take the
//!   struct's `Default`, so adding a setting needs no migration.
//! - A *scope* names a variant of a group, such as one per company or
//!   branch: `state.settings_in::<T>("branch-7")`. The default scope is `""`.
//!   Scopes are for your code and the commands (`--scope`); the groups of
//!   `App::settings_in_views` and the admin pages use the default scope.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::auth::User;
use crate::clock::Stamp;
use crate::db::{DateTime, Encrypted, Migration, now, sql};
use crate::events::Event;
use crate::validation::Errors;
use crate::{AppState, Error, Module, Registry, Result, Routes, Validate, Validator};

const MIGRATIONS: &[Migration] = &[crate::db::framework_migration!(
    "settings",
    "00010101000400_create_settings_table"
)];

/// How long a process trusts the group it holds before asking the `cache`
/// table whether another server changed it.
const RECHECK: std::time::Duration = std::time::Duration::from_secs(1);

/// A group of settings: a plain struct that serializes, validates and has
/// defaults. Declare it with `App::settings::<T>()`, read it with
/// [`AppState::settings`], change it with [`AppState::save_settings`].
///
/// ```
/// use renox::prelude::*;
/// use renox::settings::SettingsGroup;
///
/// #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
/// struct Payments {
///     #[validate(required)]
///     secret_key: String,
///     test_mode: bool,
/// }
///
/// impl SettingsGroup for Payments {
///     const KEY: &'static str = "payments";
///     const ENCRYPTED: bool = true; // sealed with APP_KEY
/// }
/// ```
pub trait SettingsGroup:
    Serialize + DeserializeOwned + Validate + Default + Clone + Send + Sync + 'static
{
    /// The group's name: its row in the `settings` table, and the first part
    /// of `settings:set company.name …`. Letters, digits, `_` and `-`.
    const KEY: &'static str;

    /// Seals the group with `APP_KEY` before it is stored (gateway keys,
    /// tokens). The admin page and the commands then never show its values.
    const ENCRYPTED: bool = false;
}

/// The opt-in module: the `settings` table and the `settings:get`,
/// `settings:set` and `settings:list` commands. Groups are declared with
/// `App::settings::<T>()`.
///
/// ```
/// use renox::prelude::*;
/// use renox::settings::Settings;
///
/// # let _ =
/// App::new().module(Settings)
/// # ;
/// ```
pub struct Settings;

impl Module for Settings {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS
    }

    fn routes(&self) -> Routes {
        Routes::new()
    }

    fn register(&self, app: &mut Registry) {
        app.settings_module = true;
        app.command(
            "settings:get",
            "Print a group or one setting: settings:get company[.name] [--scope S] [--reveal]",
            |args, state| async move {
                let path = args.positional().first().copied().unwrap_or_default();
                let scope = args.value("--scope").unwrap_or_default();
                if path.is_empty() {
                    return Err(Error::BadRequest(
                        "usage: settings:get <group>[.<field>] [--scope S] [--reveal]".into(),
                    ));
                }
                println!(
                    "{}",
                    describe(&state, path, scope, args.has("--reveal")).await?
                );
                Ok(())
            },
        );
        app.command(
            "settings:set",
            "Change one setting, validated: settings:set company.name \"Acme\" [--scope S]",
            |args, state| async move {
                let words = args.positional();
                let (Some(path), Some(text)) = (words.first(), words.get(1)) else {
                    return Err(Error::BadRequest(
                        "usage: settings:set <group>.<field> <value> [--scope S]".into(),
                    ));
                };
                let scope = args.value("--scope").unwrap_or_default();
                set_from_text(&state, path, text, scope).await?;
                println!("Saved {path}.");
                Ok(())
            },
        );
        app.command(
            "settings:list",
            "Print every declared group: settings:list [--scope S] [--reveal]",
            |args, state| async move {
                let scope = args.value("--scope").unwrap_or_default();
                println!(
                    "{}",
                    describe_all(&state, scope, args.has("--reveal")).await?
                );
                Ok(())
            },
        );
    }
}

/// A group of settings was saved. Emitted by [`AppState::save_settings`],
/// the commands and the admin page; listen for it to warm a cache or notify
/// someone, without needing `Audit`.
///
/// ```
/// use renox::prelude::*;
/// use renox::settings::SettingsChanged;
///
/// # let _ =
/// App::new().listen(|changed: SettingsChanged, _state| async move {
///     println!("{} changed: {}", changed.key, changed.changed.join(", "));
///     Ok(())
/// })
/// # ;
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SettingsChanged {
    /// The group's [`SettingsGroup::KEY`].
    pub key: String,
    /// The scope, `""` for the default one.
    pub scope: String,
    /// The user who saved it, if one did (not for a seed or a command).
    pub actor: Option<i64>,
    /// The names of the fields whose values changed.
    pub changed: Vec<String>,
    /// The group before, as JSON (the defaults when it was never saved).
    /// `null` for an encrypted group: only `changed` tells what moved.
    pub previous: Value,
    /// The group after, as JSON; `null` for an encrypted group.
    pub current: Value,
}

impl Event for SettingsChanged {}

/// When a group was last saved, and by whom.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SettingInfo {
    /// When the row was last written.
    pub updated_at: DateTime,
    /// The user who wrote it, when one did.
    pub updated_by: Option<i64>,
}

/// A group of settings as a handler's argument: the same as
/// `state.settings::<T>()`.
///
/// ```
/// use renox::prelude::*;
/// use renox::settings::{Setting, SettingsGroup};
///
/// #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
/// struct Company { name: String }
/// impl SettingsGroup for Company { const KEY: &'static str = "company"; }
///
/// async fn footer(company: Setting<Company>) -> String {
///     format!("© {}", company.name) // `Setting` derefs to `Company`
/// }
/// ```
pub struct Setting<T>(pub Arc<T>);

impl<T> Deref for Setting<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: SettingsGroup> FromRequestParts<AppState> for Setting<T> {
    type Rejection = Error;

    async fn from_request_parts(
        _: &mut Parts,
        state: &AppState,
    ) -> std::result::Result<Self, Error> {
        state.settings::<T>().await.map(Setting)
    }
}

// ---------------------------------------------------------------------
// What the app declares
// ---------------------------------------------------------------------

/// What checking a value against a group found.
pub(crate) enum Checked {
    /// Valid: the group as it will be stored.
    Valid(Value),
    /// The value fits the struct but its rules failed.
    Invalid(Errors),
    /// The value doesn't fit the struct's types.
    Mismatch(String),
}

type CheckFn =
    fn(AppState, Value, Option<User>) -> Pin<Box<dyn Future<Output = Result<Checked>> + Send>>;

/// A group an app or module declared (`App::settings`).
#[derive(Clone)]
pub(crate) struct Group {
    pub(crate) key: &'static str,
    pub(crate) type_id: TypeId,
    pub(crate) encrypted: bool,
    pub(crate) shared: bool,
    pub(crate) default: Value,
    check: CheckFn,
}

impl Group {
    pub(crate) fn of<T: SettingsGroup>() -> Self {
        assert!(
            !T::KEY.is_empty()
                && T::KEY.len() <= 64
                && T::KEY
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "SettingsGroup::KEY `{}` may only have letters, digits, `_` and `-`",
            T::KEY
        );
        Self {
            key: T::KEY,
            type_id: TypeId::of::<T>(),
            encrypted: T::ENCRYPTED,
            shared: false,
            default: serde_json::to_value(T::default()).unwrap_or(Value::Null),
            check: check_erased::<T>,
        }
    }
}

fn check_erased<T: SettingsGroup>(
    state: AppState,
    json: Value,
    actor: Option<User>,
) -> Pin<Box<dyn Future<Output = Result<Checked>> + Send>> {
    Box::pin(check::<T>(state, json, actor))
}

async fn check<T: SettingsGroup>(
    state: AppState,
    json: Value,
    actor: Option<User>,
) -> Result<Checked> {
    let merged = overlay(serde_json::to_value(T::default())?, json);
    let mut value: T = match serde_json::from_value(merged) {
        Ok(value) => value,
        Err(err) => return Ok(Checked::Mismatch(err.to_string())),
    };
    value.prepare();
    let errors = Validator::rules_of(&value)
        .finish_for(&state, actor.as_ref())
        .await?;
    if !errors.is_empty() {
        return Ok(Checked::Invalid(errors));
    }
    Ok(Checked::Valid(serde_json::to_value(&value)?))
}

/// `over`'s fields on top of `base`'s (one level), so a stored group that
/// lacks a field added later takes its default.
fn overlay(base: Value, over: Value) -> Value {
    match (base, over) {
        (Value::Object(mut base), Value::Object(over)) => {
            for (name, value) in over {
                base.insert(name, value);
            }
            Value::Object(base)
        }
        (_, over) => over,
    }
}

impl Registry {
    /// Declares a group of settings, as `App::settings` does: the commands
    /// and the admin pages find it by [`SettingsGroup::KEY`]. Needs the
    /// [`Settings`] module (boot says so if it is missing). Declaring the
    /// same type twice keeps one.
    ///
    /// # Panics
    ///
    /// If the group's key has characters other than letters, digits, `_`
    /// and `-`.
    pub fn settings<T: SettingsGroup>(&mut self) -> &mut Self {
        if !self
            .settings_groups
            .iter()
            .any(|g| g.type_id == TypeId::of::<T>())
        {
            self.settings_groups.push(Group::of::<T>());
        }
        self
    }

    /// Declares a group like [`Registry::settings`] and gives every view its
    /// default-scope values as `settings.<key>` (`settings.company.name`).
    ///
    /// # Panics
    ///
    /// For an `ENCRYPTED` group, which must not reach templates.
    pub fn settings_in_views<T: SettingsGroup>(&mut self) -> &mut Self {
        assert!(
            !T::ENCRYPTED,
            "settings_in_views::<{}>: an encrypted group can't be shared with views",
            T::KEY
        );
        self.settings::<T>();
        if let Some(group) = self
            .settings_groups
            .iter_mut()
            .find(|g| g.type_id == TypeId::of::<T>())
        {
            group.shared = true;
        }
        self
    }
}

// ---------------------------------------------------------------------
// The per-process cache
// ---------------------------------------------------------------------

struct Entry {
    json: Arc<Value>,
    /// The `cache` row's value when this was loaded (`None`: never saved).
    version: Option<String>,
    checked: Stamp,
    /// The group as its struct, once someone asked for it.
    typed: Option<Arc<dyn Any + Send + Sync>>,
}

/// The groups the app declared, the seed file's values and the loaded groups.
pub(crate) struct Store {
    groups: Vec<Group>,
    seed: Option<Map<String, Value>>,
    cache: Mutex<HashMap<(String, String), Entry>>,
}

impl Store {
    pub(crate) fn new(groups: Vec<Group>, seed: Option<Map<String, Value>>) -> Self {
        Self {
            groups,
            seed,
            cache: Mutex::default(),
        }
    }

    pub(crate) fn group(&self, key: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.key == key)
    }

    pub(crate) fn groups(&self) -> &[Group] {
        &self.groups
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), Entry>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Reads the seed file `App::settings_from` names: a JSON object of groups.
/// A missing file is no seed (a first run may not have one yet).
pub(crate) fn read_seed(path: &std::path::Path) -> Result<Option<Map<String, Value>>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow::anyhow!("can't read {}: {err}", path.display()).into());
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(anyhow::anyhow!(
            "{}: the settings file must be a JSON object of groups, e.g. {{\"company\": {{\"name\": \"Acme\"}}}}",
            path.display()
        )
        .into()),
        Err(err) => Err(anyhow::anyhow!("{}: {err}", path.display()).into()),
    }
}

fn version_key(key: &str, scope: &str) -> String {
    if scope.is_empty() {
        format!("renox:settings:{key}")
    } else {
        format!("renox:settings:{key}@{scope}")
    }
}

fn new_version() -> String {
    format!("{}-{}", crate::clock::unix_millis(), crate::random_token())
}

/// Top-level field names whose values differ.
fn changed_fields(previous: &Value, current: &Value) -> Vec<String> {
    let empty = Map::new();
    let (before, after) = (
        previous.as_object().unwrap_or(&empty),
        current.as_object().unwrap_or(&empty),
    );
    let mut names: Vec<String> = after
        .keys()
        .chain(before.keys())
        .filter(|name| before.get(*name) != after.get(*name))
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    names
}

// ---------------------------------------------------------------------
// Reading and writing
// ---------------------------------------------------------------------

type DefaultFn<'a> = &'a (dyn Fn() -> Result<Value> + Sync);

impl AppState {
    /// The default-scope group `T`: what was saved, with the struct's
    /// defaults for anything missing; the defaults before anything was saved.
    /// Cached in this process, so a read is free until someone saves (see
    /// the [module](crate::settings) for how other servers notice).
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::settings::SettingsGroup;
    /// # #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
    /// # struct Company { name: String }
    /// # impl SettingsGroup for Company { const KEY: &'static str = "company"; }
    /// # async fn demo(state: AppState) -> Result {
    /// let company = state.settings::<Company>().await?;
    /// println!("{}", company.name);
    /// # Ok(()) }
    /// ```
    pub async fn settings<T: SettingsGroup>(&self) -> Result<Arc<T>> {
        self.settings_in::<T>("").await
    }

    /// Like [`AppState::settings`] for a scope: one variant of the group per
    /// company, branch or account (`""` is the default scope).
    pub async fn settings_in<T: SettingsGroup>(&self, scope: &str) -> Result<Arc<T>> {
        let default = || Ok(serde_json::to_value(T::default())?);
        let (json, typed) = self
            .settings_entry(T::KEY, scope, T::ENCRYPTED, &default)
            .await?;
        if let Some(typed) = typed
            && let Ok(typed) = typed.downcast::<T>()
        {
            return Ok(typed);
        }
        let value: T = serde_json::from_value((*json).clone()).map_err(|err| {
            anyhow::anyhow!(
                "the stored `{}` settings don't fit {}: {err}",
                T::KEY,
                std::any::type_name::<T>()
            )
        })?;
        let value = Arc::new(value);
        let mut cache = self.settings_store.lock();
        if let Some(entry) = cache.get_mut(&(T::KEY.to_owned(), scope.to_owned()))
            && Arc::ptr_eq(&entry.json, &json)
        {
            entry.typed = Some(value.clone());
        }
        Ok(value)
    }

    /// Saves the default-scope group `T`, after its rules pass (invalid
    /// values are a `ValidationError` with the rules' messages and nothing
    /// is written). The change is seen at once in this process and within a
    /// second by the other servers; it emits [`SettingsChanged`], and the
    /// `Audit` module (when on) records it. `by` is who changed it.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::settings::SettingsGroup;
    /// # #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
    /// # struct Company { name: String }
    /// # impl SettingsGroup for Company { const KEY: &'static str = "company"; }
    /// # async fn demo(state: AppState, user: User) -> Result {
    /// let mut company = (*state.settings::<Company>().await?).clone();
    /// company.name = "Acme".into();
    /// state.save_settings(&company, Some(&user)).await?;
    /// # Ok(()) }
    /// ```
    pub async fn save_settings<T: SettingsGroup>(&self, value: &T, by: Option<&User>) -> Result {
        self.save_settings_in(value, "", by).await
    }

    /// Like [`AppState::save_settings`] for a scope.
    pub async fn save_settings_in<T: SettingsGroup>(
        &self,
        value: &T,
        scope: &str,
        by: Option<&User>,
    ) -> Result {
        let mut value = value.clone();
        value.prepare();
        let errors = Validator::rules_of(&value).finish_for(self, by).await?;
        if !errors.is_empty() {
            return Err(errors.into());
        }
        let json = serde_json::to_value(&value)?;
        let typed: Arc<dyn Any + Send + Sync> = Arc::new(value);
        self.commit_settings(
            T::KEY,
            scope,
            T::ENCRYPTED,
            json,
            by.map(|u| u.id),
            Some(typed),
        )
        .await
    }

    /// When the default-scope group `T` was last saved and by whom; `None`
    /// while it never was (the defaults are in use).
    pub async fn settings_info<T: SettingsGroup>(&self) -> Result<Option<SettingInfo>> {
        self.settings_info_in::<T>("").await
    }

    /// Like [`AppState::settings_info`] for a scope.
    pub async fn settings_info_in<T: SettingsGroup>(
        &self,
        scope: &str,
    ) -> Result<Option<SettingInfo>> {
        let row = sql("SELECT updated_at, updated_by FROM settings WHERE key = ? AND scope = ?")
            .bind(T::KEY)
            .bind(scope)
            .fetch_optional(&self.db)
            .await?;
        row.map(|row| {
            Ok(SettingInfo {
                updated_at: row.try_get("updated_at")?,
                updated_by: row.try_get("updated_by")?,
            })
        })
        .transpose()
    }

    /// The group `key` as JSON (the declared defaults under what was saved),
    /// for the commands, the admin panel and views.
    pub(crate) async fn settings_json(&self, key: &str, scope: &str) -> Result<Arc<Value>> {
        let group = self.declared_settings(key)?;
        let default = group.default.clone();
        let default = move || Ok(default.clone());
        let (json, _) = self
            .settings_entry(group.key, scope, group.encrypted, &default)
            .await?;
        Ok(json)
    }

    pub(crate) fn declared_settings(&self, key: &str) -> Result<&Group> {
        self.settings_store.group(key).ok_or_else(|| {
            let known: Vec<&str> = self.settings_store.groups().iter().map(|g| g.key).collect();
            Error::BadRequest(format!(
                "no settings group `{key}` (declared: {})",
                if known.is_empty() {
                    "none".into()
                } else {
                    known.join(", ")
                }
            ))
        })
    }

    /// `settings` for the views: every shared group, by key.
    pub(crate) async fn settings_for_views(&self) -> Result<Value> {
        let mut all = Map::new();
        for group in self.settings_store.groups().iter().filter(|g| g.shared) {
            let json = self.settings_json(group.key, "").await?;
            all.insert(group.key.to_owned(), (*json).clone());
        }
        Ok(Value::Object(all))
    }

    /// Checks `json` against the group `key`'s types and rules, and saves
    /// it. The path the commands take.
    pub(crate) async fn save_settings_json(
        &self,
        key: &str,
        scope: &str,
        json: Value,
        by: Option<&User>,
    ) -> Result {
        let group = self.declared_settings(key)?;
        match (group.check)(self.clone(), json, by.cloned()).await? {
            Checked::Valid(json) => {
                self.commit_settings(
                    group.key,
                    scope,
                    group.encrypted,
                    json,
                    by.map(|u| u.id),
                    None,
                )
                .await
            }
            Checked::Invalid(errors) => Err(errors.into()),
            Checked::Mismatch(message) => Err(Error::BadRequest(message)),
        }
    }

    /// The group's cached JSON and typed value, loading it when this
    /// process has none or another server may have changed it.
    async fn settings_entry(
        &self,
        key: &str,
        scope: &str,
        encrypted: bool,
        default: DefaultFn<'_>,
    ) -> Result<(Arc<Value>, Option<Arc<dyn Any + Send + Sync>>)> {
        let id = (key.to_owned(), scope.to_owned());
        let known = {
            let cache = self.settings_store.lock();
            match cache.get(&id) {
                Some(entry) if entry.checked.elapsed() < RECHECK => {
                    return Ok((entry.json.clone(), entry.typed.clone()));
                }
                Some(entry) => Some(entry.version.clone()),
                None => None,
            }
        };
        if let Some(known) = known {
            let current: Option<String> = sql("SELECT value FROM cache WHERE key = ?")
                .bind(version_key(key, scope))
                .scalar_optional(&self.db)
                .await?;
            let mut cache = self.settings_store.lock();
            if let Some(entry) = cache.get_mut(&id)
                && entry.version == known
                && current == known
            {
                entry.checked = Stamp::now();
                return Ok((entry.json.clone(), entry.typed.clone()));
            }
        }
        let (json, version) = self
            .load_settings(key, scope, encrypted, default, true)
            .await?;
        let json = Arc::new(json);
        self.settings_store.lock().insert(
            id,
            Entry {
                json: json.clone(),
                version,
                checked: Stamp::now(),
                typed: None,
            },
        );
        Ok((json, None))
    }

    /// One query: the group's row and the version stamp. A group never
    /// saved is the defaults, or what the seed file says.
    async fn load_settings(
        &self,
        key: &str,
        scope: &str,
        encrypted: bool,
        default: DefaultFn<'_>,
        seeding: bool,
    ) -> Result<(Value, Option<String>)> {
        let row = sql(
            "SELECT (SELECT value FROM cache WHERE key = ?) AS version, \
             (SELECT value FROM settings WHERE key = ? AND scope = ?) AS value",
        )
        .bind(version_key(key, scope))
        .bind(key)
        .bind(scope)
        .fetch_one(&self.db)
        .await?;
        let version: Option<String> = row.try_get("version")?;
        let stored: Option<Value> = if encrypted {
            row.try_get::<Option<Encrypted<Value>>>("value")?
                .map(Encrypted::into_inner)
        } else {
            row.try_get::<Option<String>>("value")?
                .map(|text| serde_json::from_str(&text))
                .transpose()?
        };
        match stored {
            Some(stored) => Ok((overlay(default()?, stored), version)),
            None if seeding && scope.is_empty() && self.seed_for(key).is_some() => {
                self.apply_seed(key, encrypted).await?;
                Box::pin(self.load_settings(key, scope, encrypted, default, false)).await
            }
            None => Ok((default()?, version)),
        }
    }

    fn seed_for(&self, key: &str) -> Option<&Value> {
        self.settings_store
            .seed
            .as_ref()
            .and_then(|seed| seed.get(key))
            .filter(|_| self.settings_store.group(key).is_some())
    }

    /// Stores the seed file's values for `key` unless a row exists.
    async fn apply_seed(&self, key: &str, encrypted: bool) -> Result {
        let (Some(seed), Some(group)) = (self.seed_for(key), self.settings_store.group(key)) else {
            return Ok(());
        };
        let json = match (group.check)(self.clone(), seed.clone(), None).await? {
            Checked::Valid(json) => json,
            Checked::Invalid(errors) => {
                return Err(anyhow::anyhow!(
                    "the settings file's `{key}` is invalid: {}",
                    errors
                        .iter()
                        .flat_map(|(field, m)| m.iter().map(move |m| format!("{field}: {m}")))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
                .into());
            }
            Checked::Mismatch(message) => {
                return Err(
                    anyhow::anyhow!("the settings file's `{key}` doesn't fit: {message}").into(),
                );
            }
        };
        let mut tx = self.db.begin().await?;
        let inserted = write_row(&mut tx, key, "", encrypted, &json, None, true).await?;
        if inserted > 0 {
            stamp_version(&mut tx, key, "").await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Writes the group, stamps the version, refreshes this process's copy
    /// and emits the event.
    async fn commit_settings(
        &self,
        key: &str,
        scope: &str,
        encrypted: bool,
        json: Value,
        actor: Option<i64>,
        typed: Option<Arc<dyn Any + Send + Sync>>,
    ) -> Result {
        let group_default = self.settings_store.group(key).map(|g| g.default.clone());
        let mut tx = self.db.begin().await?;
        let row = sql("SELECT value FROM settings WHERE key = ? AND scope = ?")
            .bind(key)
            .bind(scope)
            .fetch_optional(&mut tx)
            .await?;
        let before: Option<Value> = match row {
            None => None,
            Some(row) if encrypted => Some(row.try_get::<Encrypted<Value>>("value")?.into_inner()),
            Some(row) => Some(serde_json::from_str(&row.try_get::<String>("value")?)?),
        };
        let previous = before.clone().or(group_default).unwrap_or(Value::Null);
        let changed = changed_fields(&previous, &json);
        if before.is_some() && changed.is_empty() {
            return Ok(());
        }
        write_row(&mut tx, key, scope, encrypted, &json, actor, false).await?;
        let version = stamp_version(&mut tx, key, scope).await?;
        tx.commit().await?;
        self.settings_store.lock().insert(
            (key.to_owned(), scope.to_owned()),
            Entry {
                json: Arc::new(json.clone()),
                version: Some(version),
                checked: Stamp::now(),
                typed,
            },
        );
        let shown = |value: Value| if encrypted { Value::Null } else { value };
        self.emit(SettingsChanged {
            key: key.to_owned(),
            scope: scope.to_owned(),
            actor,
            changed,
            previous: shown(previous),
            current: shown(json),
        })
        .await
    }
}

/// Inserts or replaces the row; `insert_only` leaves an existing one alone.
async fn write_row(
    tx: &mut crate::db::Transaction,
    key: &str,
    scope: &str,
    encrypted: bool,
    json: &Value,
    actor: Option<i64>,
    insert_only: bool,
) -> Result<u64> {
    let conflict = if insert_only {
        "DO NOTHING"
    } else {
        "DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, \
         updated_by = excluded.updated_by"
    };
    let statement = sql(format!(
        "INSERT INTO settings (key, scope, value, updated_at, updated_by) \
         VALUES (?, ?, ?, ?, ?) ON CONFLICT (key, scope) {conflict}"
    ))
    .bind(key)
    .bind(scope);
    let statement = if encrypted {
        statement.bind(Encrypted::new(json.clone()))
    } else {
        statement.bind(json.to_string())
    };
    Ok(statement.bind(now()).bind(actor).execute(&mut *tx).await?)
}

/// A new version stamp in the `cache` table, which other servers compare.
async fn stamp_version(tx: &mut crate::db::Transaction, key: &str, scope: &str) -> Result<String> {
    let version = new_version();
    sql(
        "INSERT INTO cache (key, value, expires_at) VALUES (?, ?, NULL) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, expires_at = NULL",
    )
    .bind(version_key(key, scope))
    .bind(version.clone())
    .execute(&mut *tx)
    .await?;
    Ok(version)
}

// ---------------------------------------------------------------------
// What the commands run
// ---------------------------------------------------------------------

/// A path like `company` or `company.name` split into the group and the
/// field path under it.
fn split_path(path: &str) -> (&str, Vec<&str>) {
    let mut parts = path.split('.');
    let key = parts.next().unwrap_or_default();
    (key, parts.filter(|p| !p.is_empty()).collect())
}

fn show(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// What `settings:get <group>[.<field>]` prints: the group as JSON, or one
/// field's value. An encrypted group is `<sealed>` unless `reveal`.
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(state: AppState) -> Result {
/// let text = renox::settings::describe(&state, "company.name", "", false).await?;
/// # let _ = text; Ok(()) }
/// ```
pub async fn describe(state: &AppState, path: &str, scope: &str, reveal: bool) -> Result<String> {
    let (key, fields) = split_path(path);
    let group = state.declared_settings(key)?;
    if group.encrypted && !reveal {
        return Ok("<sealed>".into());
    }
    let json = state.settings_json(key, scope).await?;
    let mut at = &*json;
    for field in &fields {
        at = at.get(*field).ok_or_else(|| {
            Error::BadRequest(format!(
                "the `{key}` settings have no field `{}`",
                fields.join(".")
            ))
        })?;
    }
    if fields.is_empty() {
        Ok(serde_json::to_string_pretty(at)?)
    } else {
        Ok(show(at))
    }
}

/// What `settings:list` prints: a line per declared group.
pub async fn describe_all(state: &AppState, scope: &str, reveal: bool) -> Result<String> {
    let mut lines = Vec::new();
    for group in state.settings_store.groups() {
        let value = if group.encrypted && !reveal {
            "<sealed>".to_owned()
        } else {
            state.settings_json(group.key, scope).await?.to_string()
        };
        lines.push(format!("{} = {value}", group.key));
    }
    if lines.is_empty() {
        lines.push("No settings groups are declared (App::settings::<T>()).".into());
    }
    Ok(lines.join("\n"))
}

/// What `settings:set <group>.<field> <value>` does: sets one field of a
/// group and saves it, after the group's types and rules accept it. The text
/// is read as JSON when it is (`true`, `42`, `["a"]`), else as a string.
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(state: AppState) -> Result {
/// renox::settings::set_from_text(&state, "company.name", "Acme", "").await?;
/// # Ok(()) }
/// ```
pub async fn set_from_text(state: &AppState, path: &str, text: &str, scope: &str) -> Result {
    let (key, fields) = split_path(path);
    if fields.is_empty() {
        return Err(Error::BadRequest(
            "name a field: settings:set <group>.<field> <value>".into(),
        ));
    }
    let group = state.declared_settings(key)?;
    let current = state.settings_json(key, scope).await?;
    if current.get(fields[0]).is_none() {
        return Err(Error::BadRequest(format!(
            "the `{key}` settings have no field `{}`",
            fields[0]
        )));
    }
    let mut candidates = Vec::new();
    if let Ok(parsed) = serde_json::from_str::<Value>(text)
        && !parsed.is_string()
    {
        candidates.push(parsed);
    }
    candidates.push(Value::String(text.to_owned()));
    if text.is_empty() {
        candidates.push(Value::Null);
    }
    let (mut invalid, mut mismatch) = (None, None);
    for candidate in candidates {
        let mut json = (*current).clone();
        let mut at = &mut json;
        for field in &fields[..fields.len() - 1] {
            if !at.get(*field).is_some_and(Value::is_object) {
                at[*field] = Value::Object(Map::new());
            }
            at = &mut at[*field];
        }
        at[fields[fields.len() - 1]] = candidate;
        match (group.check)(state.clone(), json, None).await? {
            Checked::Valid(json) => {
                return state.save_settings_json(key, scope, json, None).await;
            }
            Checked::Invalid(errors) => {
                invalid.get_or_insert(errors);
            }
            Checked::Mismatch(message) => {
                mismatch.get_or_insert(message);
            }
        }
    }
    if let Some(errors) = invalid {
        let lines: Vec<String> = errors
            .iter()
            .flat_map(|(field, messages)| {
                messages.iter().map(move |m| format!("{key}.{field}: {m}"))
            })
            .collect();
        return Err(Error::BadRequest(lines.join("\n")));
    }
    Err(Error::BadRequest(format!(
        "`{text}` doesn't fit {path}: {}",
        mismatch.unwrap_or_default()
    )))
}

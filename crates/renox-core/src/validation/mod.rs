//! Form validation with Laravel-style rules, database-backed `unique` and
//! `exists`, and English messages (an app translates them in its lang files).
//!
//! ```
//! # use renox::prelude::*;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Deserialize, Serialize)]
//! struct ProductForm {
//!     name: String,
//!     price: i64,
//!     email: Option<String>,
//! }
//!
//! impl Validate for ProductForm {
//!     fn rules(&self, v: &mut Validator) {
//!         v.field("name", &self.name).required().max(100).unique("products", "name");
//!         v.field("price", &self.price).label("sale price").min(1_000);
//!         v.field("email", &self.email).email();
//!     }
//! }
//!
//! async fn store(State(db): State<Db>, back: Back, Valid(form): Valid<ProductForm>) -> Result<Back> {
//!     // `form` passed every rule; invalid input never gets here.
//! #   let _ = (db, form);
//!     Ok(back)
//! }
//! ```

pub(crate) mod extract;
mod key_values;
mod messages;
pub(crate) mod nested;
mod value;

use std::collections::BTreeMap;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use extract::Valid;
pub use key_values::KeyValues;
pub(crate) use messages::{render, template_for};
pub use value::{FieldValue, Inspected};

use crate::Result;
use crate::db::{Db, DbValue, ToDbValue, quote};
use chrono::NaiveDateTime;

/// Validation errors: messages keyed by field name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Errors(BTreeMap<String, Vec<String>>);

impl Errors {
    /// No errors.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `message` to the messages of `field`.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.0.entry(field.into()).or_default().push(message.into());
    }

    /// Whether there are no errors.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `field` has at least one error.
    pub fn has(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }

    /// The first message for a field.
    pub fn first(&self, field: &str) -> Option<&str> {
        self.0
            .get(field)
            .and_then(|m| m.first())
            .map(String::as_str)
    }

    /// Every field with its messages, fields in alphabetical order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.0.iter().map(|(f, m)| (f.as_str(), m.as_slice()))
    }
}

/// Declares the rules for a form. Used by the `Valid<T>` extractor, which
/// calls, in order: [`prepare`](Validate::prepare),
/// [`authorize`](Validate::authorize), [`rules`](Validate::rules) and, when
/// they pass, [`after`](Validate::after). Only `rules` is required.
///
/// ```
/// # use renox::prelude::*;
/// use renox::validation::FormContext;
///
/// #[derive(serde::Deserialize)]
/// struct Invite { email: String, team_id: i64 }
///
/// impl Validate for Invite {
///     // Laravel's prepareForValidation: tidy the input first.
///     fn prepare(&mut self) {
///         self.email = self.email.trim().to_lowercase();
///     }
///
///     // Laravel's authorize: `false` answers 403 before any rule runs.
///     async fn authorize(&self, form: &FormContext<'_>) -> Result<bool> {
///         Ok(form.user.is_some_and(|user| user.has_role("owner")))
///     }
///
///     fn rules(&self, v: &mut Validator) {
///         v.field("email", &self.email).required().email();
///     }
///
///     // Laravel's `after`: checks that need the database or several fields,
///     // once the rules pass. Errors added here are shown like any other.
///     async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
///         let members: i64 = renox::db::sql("SELECT COUNT(*) FROM team_user WHERE team_id = ?")
///             .bind(self.team_id)
///             .scalar(&form.state.db)
///             .await?;
///         if members >= 10 {
///             errors.add("email", "This team is full (10 members).");
///         }
///         Ok(())
///     }
/// }
/// ```
pub trait Validate {
    /// The named error bag this form's errors are flashed in (Laravel's
    /// error bags), for a page with two forms that share field names, e.g.
    /// `Some("login")` next to a sign-up form. Templates read them with
    /// `error('email', bag='login')`. `#[validate(bag = "login")]` on a
    /// derived struct sets it.
    const ERROR_BAG: Option<&'static str> = None;

    /// Declares the fields' rules on `v`; they run after `prepare` and `authorize`.
    fn rules(&self, v: &mut Validator);

    /// Tidies the input before anything checks it (Laravel's
    /// `prepareForValidation`): trim, lowercase an email, fill a slug. The
    /// form refilled after an error still shows what was typed.
    fn prepare(&mut self) {}

    /// Whether this request may send the form (Laravel's `authorize`), e.g.
    /// only the team's owner invites. `false` answers 403 before the rules
    /// run. It sees the input, so it runs once the input is read.
    fn authorize(
        &self,
        form: &FormContext<'_>,
    ) -> impl std::future::Future<Output = Result<bool>> + Send {
        let _ = form;
        std::future::ready(Ok(true))
    }

    /// Checks that need the database, another service or several fields,
    /// once the rules pass (Laravel's `after`, and async rules). Add
    /// errors with `errors.add(field, message)`.
    fn after(
        &self,
        form: &FormContext<'_>,
        errors: &mut Errors,
    ) -> impl std::future::Future<Output = Result> + Send {
        let _ = (form, errors);
        std::future::ready(Ok(()))
    }
}

/// `prepare`, `authorize` and `after` for a struct with
/// `#[derive(Validate)]`, which writes `rules` from its `#[validate(…)]`
/// attributes: add `#[validate(hooks)]` on the struct and implement the
/// ones you need.
///
/// ```
/// # use renox::prelude::*;
/// use renox::validation::{FormContext, ValidateHooks};
///
/// #[derive(serde::Deserialize, Validate)]
/// #[validate(hooks)]
/// struct Invite {
///     #[validate(required, email)]
///     email: String,
/// }
///
/// impl ValidateHooks for Invite {
///     fn prepare(&mut self) {
///         self.email = self.email.trim().to_lowercase();
///     }
///
///     async fn authorize(&self, form: &FormContext<'_>) -> Result<bool> {
///         Ok(form.user.is_some())
///     }
/// }
/// ```
pub trait ValidateHooks {
    /// Tidies the input before anything checks it; see [`Validate::prepare`].
    fn prepare(&mut self) {}

    /// Whether this request may send the form; `false` answers 403. See
    /// [`Validate::authorize`].
    fn authorize(
        &self,
        form: &FormContext<'_>,
    ) -> impl std::future::Future<Output = Result<bool>> + Send {
        let _ = form;
        std::future::ready(Ok(true))
    }

    /// Checks run once the rules pass; see [`Validate::after`].
    fn after(
        &self,
        form: &FormContext<'_>,
        errors: &mut Errors,
    ) -> impl std::future::Future<Output = Result> + Send {
        let _ = (form, errors);
        std::future::ready(Ok(()))
    }
}

/// What [`Validate::authorize`] and [`Validate::after`] see of the request.
#[non_exhaustive]
pub struct FormContext<'a> {
    /// The app's state, e.g. for its database.
    pub state: &'a crate::AppState,
    /// The logged-in user, if any.
    pub user: Option<&'a crate::auth::User>,
    /// The request's HTTP method.
    pub method: &'a axum::http::Method,
    /// The request's path, without the query string.
    pub path: &'a str,
}

struct Pending {
    field: String,
    label: String,
    table: String,
    column: String,
    value: DbValue,
    ignore_id: Option<crate::db::DbValue>,
    /// Extra conditions (`where_eq`, `where_null`, `where_not_null`).
    scope: Vec<ScopeCondition>,
    unique: bool,
    message: Option<String>,
}

/// A check that needs the request's user or a service, run by `finish`.
struct AsyncCheck {
    field: String,
    label: String,
    kind: AsyncKind,
    message: Option<String>,
}

enum AsyncKind {
    /// The logged-in user's password (Laravel's `current_password`).
    CurrentPassword(String),
    /// Not in a known data breach (Have I Been Pwned's range API).
    Uncompromised(String),
}

/// A condition added to a `unique`/`exists` check.
enum ScopeCondition {
    Eq(String, DbValue),
    Null(String),
    NotNull(String),
}

impl ScopeCondition {
    fn column(&self) -> &str {
        match self {
            Self::Eq(column, _) | Self::Null(column) | Self::NotNull(column) => column,
        }
    }
}

/// SQLite reads an unknown double-quoted column as a string literal, which
/// would make `unique("t", "typo")` always pass; refuse unknown columns.
async fn ensure_sqlite_column(db: &Db, table: &str, column: &str) -> Result {
    let found: i64 = crate::db::sql("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
        .bind(table)
        .bind(column)
        .scalar(db)
        .await?;
    if found == 0 {
        return Err(anyhow::anyhow!(
            "unique/exists rule: table `{table}` has no column `{column}`"
        )
        .into());
    }
    Ok(())
}

/// Collects rule failures. Rules run in order and stop at a field's first
/// failure; rules other than `required` and `accepted` skip empty values.
pub struct Validator {
    /// The request language's lang file, for overridden messages and labels.
    texts: Option<crate::i18n::Texts>,
    errors: Errors,
    pending: Vec<Pending>,
    checks: Vec<AsyncCheck>,
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

impl Validator {
    /// A validator with Renox's English messages.
    pub fn new() -> Self {
        Self {
            texts: None,
            errors: Errors::new(),
            pending: Vec::new(),
            checks: Vec::new(),
        }
    }

    /// Messages and field names in `lang`, from the app's lang file
    /// (`renox.validation.*`, `renox.validation.attributes.*`), e.g. for
    /// rules run in a handler or a command: `Validator::new().in_lang(&lang)`.
    pub fn in_lang(self, lang: &crate::Lang) -> Self {
        self.with_texts(lang.texts())
    }

    /// Uses the app's translations of messages (`renox.validation.*`) and
    /// field names (`renox.validation.attributes.*`).
    pub(crate) fn with_texts(mut self, texts: crate::i18n::Texts) -> Self {
        self.texts = Some(texts);
        self
    }

    fn template(&self, key: &str) -> std::borrow::Cow<'static, str> {
        messages::template_for(self.texts.as_ref(), key)
    }

    /// A field's name for messages: the app's translation
    /// (`renox.validation.attributes.{name}`) or the name with spaces.
    fn translated_label(&self, name: &str) -> Option<String> {
        self.texts.as_ref().and_then(|t| {
            t.get(&format!("renox.validation.attributes.{name}"))
                .cloned()
        })
    }

    fn label_for(&self, name: &str) -> String {
        nested::label(name, |key| self.translated_label(key))
    }

    /// Rules for each item of a list, e.g. every tag or every uploaded
    /// photo; errors are keyed `name.0`, `name.1`, … and labelled
    /// "`name` #1", "#2", …
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # struct Form { tags: Vec<String>, photos: Vec<Upload> }
    /// # impl Validate for Form {
    /// fn rules(&self, v: &mut Validator) {
    ///     v.field("tags", &self.tags).max(5);
    ///     v.each("tags", &self.tags, |tag| tag.required().max(20));
    ///     v.each("photos", &self.photos, |photo| photo.image().max(2048));
    /// }
    /// # }
    /// ```
    pub fn each<T: FieldValue>(
        &mut self,
        name: &str,
        items: &[T],
        rules: impl for<'a> Fn(Field<'a>) -> Field<'a>,
    ) {
        let base = self.label_for(name);
        for (i, item) in items.iter().enumerate() {
            let key = format!("{name}.{i}");
            let label = format!("{base} #{}", i + 1);
            rules(self.field(&key, item).label(&label));
        }
    }

    /// Each item's own `Validate` rules, for a list of structs (e.g. the
    /// lines of an order sent as JSON); errors are keyed `name.0.field`.
    pub fn nested<T: Validate>(&mut self, name: &str, items: &[T]) {
        for (i, item) in items.iter().enumerate() {
            let mut inner = Validator {
                texts: self.texts.clone(),
                errors: Errors::new(),
                pending: Vec::new(),
                checks: Vec::new(),
            };
            item.rules(&mut inner);
            for (field, messages) in inner.errors.iter() {
                for message in messages {
                    self.errors
                        .add(format!("{name}.{i}.{field}"), message.clone());
                }
            }
            for mut pending in inner.pending {
                pending.field = format!("{name}.{i}.{}", pending.field);
                self.pending.push(pending);
            }
            for mut check in inner.checks {
                check.field = format!("{name}.{i}.{}", check.field);
                self.checks.push(check);
            }
        }
    }

    /// No two items of a list are the same (Laravel's `distinct`), e.g. the
    /// emails invited at once; each repeat gets the error, keyed `name.i`.
    pub fn distinct<T: FieldValue>(&mut self, name: &str, items: &[T]) {
        let base = self.label_for(name);
        let template = self.template("distinct");
        let mut seen: Vec<Inspected> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let value = match item.inspect() {
                Inspected::Text(text) => Inspected::Text(text.trim().to_lowercase()),
                other => other,
            };
            if value == Inspected::Missing {
                continue;
            }
            if seen.contains(&value) {
                let label = format!("{base} #{}", i + 1);
                self.errors
                    .add(format!("{name}.{i}"), render(&template, &label, &[]));
            } else {
                seen.push(value);
            }
        }
    }

    /// Starts the rules for one field. The label in messages defaults to the
    /// name with `_` replaced by spaces.
    pub fn field<'v>(&'v mut self, name: &str, value: &impl FieldValue) -> Field<'v> {
        let translated = self.translated_label(name).is_some();
        let mut field = Field {
            translated,
            // `items.0.name` reads "name" (or the app's `items.*.name`).
            label: self.label_for(name),
            name: name.to_owned(),
            value: value.inspect(),
            db_value: value.db_value(),
            failed: false,
            last_pending: None,
            last_check: None,
            v: self,
        };
        // `NaN`, `inf` and `1e999` parse as floats, but aren't numbers anyone entered.
        if let Inspected::Number(n) = field.value
            && !n.is_finite()
        {
            field.fail("numeric", &[]);
        }
        field
    }

    /// Adds an error that no rule covers.
    pub fn error(&mut self, field: &str, message: impl Into<String>) {
        self.errors.add(field, message);
    }

    /// Runs the database checks and returns every error (empty when valid).
    /// `current_password` fails here, as there's no user to check against,
    /// and the breach check of `Password::uncompromised` is skipped; use
    /// [`finish_for`](Self::finish_for) for those (`Valid<T>` does).
    pub async fn finish(self, db: &Db) -> Result<Errors> {
        self.finish_with(db, None, None).await
    }

    /// Like [`finish`](Self::finish), with the logged-in user for
    /// `current_password` and the app's HTTP client for
    /// `Password::uncompromised`.
    pub async fn finish_for(
        self,
        state: &crate::AppState,
        user: Option<&crate::auth::User>,
    ) -> Result<Errors> {
        self.finish_with(&state.db, Some(state), user).await
    }

    async fn finish_with(
        self,
        db: &Db,
        state: Option<&crate::AppState>,
        user: Option<&crate::auth::User>,
    ) -> Result<Errors> {
        let mut errors = self.errors;
        for check in self.pending {
            if errors.has(&check.field) {
                continue;
            }
            let dialect = db.dialect();
            if dialect == crate::db::Dialect::Sqlite {
                ensure_sqlite_column(db, &check.table, &check.column).await?;
                for condition in &check.scope {
                    ensure_sqlite_column(db, &check.table, condition.column()).await?;
                }
            }
            // Form input is text; PostgreSQL won't compare text with a number
            // column, so compare as text (a no-op for text columns).
            let column = match (&check.value, dialect) {
                (DbValue::Text(_), crate::db::Dialect::Postgres) => {
                    format!("CAST({} AS TEXT)", quote(&check.column))
                }
                _ => quote(&check.column),
            };
            let mut sql = format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE {column} = ?",
                quote(&check.table),
            );
            if check.ignore_id.is_some() {
                sql.push_str(" AND \"id\" != ?");
            }
            let mut scope_values = Vec::new();
            for condition in check.scope {
                match condition {
                    ScopeCondition::Eq(column, value) => {
                        sql.push_str(&format!(" AND {} = ?", quote(&column)));
                        scope_values.push(value);
                    }
                    ScopeCondition::Null(column) => {
                        sql.push_str(&format!(" AND {} IS NULL", quote(&column)));
                    }
                    ScopeCondition::NotNull(column) => {
                        sql.push_str(&format!(" AND {} IS NOT NULL", quote(&column)));
                    }
                }
            }
            sql.push(')');
            let mut query = crate::db::sql(sql).bind(check.value);
            if let Some(id) = check.ignore_id {
                query = query.bind(id);
            }
            let query = query.bind_all(scope_values);
            let found: bool = query.scalar(db).await?;
            if found == check.unique {
                let key = if check.unique { "unique" } else { "exists" };
                let message = check.message.unwrap_or_else(|| {
                    render(
                        &messages::template_for(self.texts.as_ref(), key),
                        &check.label,
                        &[],
                    )
                });
                errors.add(check.field, message);
            }
        }
        for check in self.checks {
            if errors.has(&check.field) {
                continue;
            }
            let key = match &check.kind {
                AsyncKind::CurrentPassword(password) => match user {
                    Some(user) if user.check_password(password).await => continue,
                    _ => "current_password",
                },
                AsyncKind::Uncompromised(password) => match state {
                    Some(state) if breached(state, password).await => "password.uncompromised",
                    _ => continue,
                },
            };
            let message = check.message.unwrap_or_else(|| {
                render(
                    &messages::template_for(self.texts.as_ref(), key),
                    &check.label,
                    &[],
                )
            });
            errors.add(check.field, message);
        }
        Ok(errors)
    }

    /// Like `rules_of`, with the app's translations of messages and labels.
    pub(crate) fn rules_with_texts(data: &impl Validate, texts: crate::i18n::Texts) -> Self {
        let mut validator = Self::new().with_texts(texts);
        data.rules(&mut validator);
        validator
    }

    /// Applies `data`'s rules; call `finish` to run the database checks.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(serde::Deserialize)] struct ProductForm { name: String }
    /// # impl Validate for ProductForm { fn rules(&self, v: &mut Validator) { v.field("name", &self.name).required(); } }
    /// # async fn demo(form: ProductForm, db: Db) -> Result {
    /// let errors = Validator::rules_of(&form).finish(&db).await?;
    /// # let _ = errors; Ok(()) }
    /// ```
    pub fn rules_of(data: &impl Validate) -> Self {
        let mut validator = Self::new();
        data.rules(&mut validator);
        validator
    }
}

/// The rules for one field, chained: `v.field("name", &self.name).required().max(100)`.
pub struct Field<'v> {
    v: &'v mut Validator,
    name: String,
    label: String,
    /// The label came from the app's lang file.
    translated: bool,
    value: Inspected,
    db_value: DbValue,
    failed: bool,
    last_pending: Option<usize>,
    /// The `current_password`/`uncompromised` check just added, for `message`.
    last_check: Option<usize>,
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

impl Field<'_> {
    /// The name used in messages, e.g. `.label("sale price")`.
    pub fn label(mut self, label: &str) -> Self {
        self.label = label.to_owned();
        self
    }

    /// Uses `label` unless the app's lang file names this field.
    pub(crate) fn fallback_label(mut self, label: &str) -> Self {
        if !self.translated {
            self.label = label.to_owned();
        }
        self
    }

    fn fail(&mut self, key: &str, params: &[(&str, String)]) {
        if !self.failed {
            let message = render(&self.v.template(key), &self.label, params);
            self.v.errors.add(&self.name, message);
            self.failed = true;
            self.last_pending = None;
            self.last_check = None;
        }
    }

    fn check(&mut self, kind: AsyncKind) {
        self.v.checks.push(AsyncCheck {
            field: self.name.clone(),
            label: self.label.clone(),
            kind,
            message: None,
        });
        self.last_pending = None;
        self.last_check = Some(self.v.checks.len() - 1);
    }

    fn present(&self) -> bool {
        !self.failed && self.value != Inspected::Missing
    }

    /// Replaces this field's error so far with `message`: the error of the
    /// rule just before it, or of any earlier rule that failed (only one error
    /// is kept per field). A database or async check just before it gets the
    /// message when it fails.
    pub fn message(self, message: impl Into<String>) -> Self {
        let message = message.into();
        if let Some(i) = self.last_pending {
            self.v.pending[i].message = Some(message);
        } else if let Some(i) = self.last_check {
            self.v.checks[i].message = Some(message);
        } else if self.failed
            && let Some(last) = self
                .v
                .errors
                .0
                .get_mut(&self.name)
                .and_then(|messages| messages.last_mut())
        {
            *last = message;
        }
        self
    }

    /// Fails when the value is missing: `None`, or text that is blank after trimming.
    pub fn required(mut self) -> Self {
        if !self.failed && self.value == Inspected::Missing {
            self.fail("required", &[]);
        }
        self
    }

    fn size_rule(
        mut self,
        kind: &str,
        ok: impl Fn(f64) -> bool,
        params: &[(&str, String)],
    ) -> Self {
        if !self.present() {
            return self;
        }
        let (size, suffix) = match &self.value {
            Inspected::Text(text) => (text.chars().count() as f64, "string"),
            Inspected::Number(n) => (*n, "numeric"),
            Inspected::Items(n) => (*n as f64, "array"),
            Inspected::File { kilobytes, .. } => (*kilobytes, "file"),
            _ => return self,
        };
        if !ok(size) {
            self.fail(&format!("{kind}.{suffix}"), params);
        }
        self
    }

    /// At least `min` characters, items, kilobytes (files), or as a number.
    pub fn min(self, min: impl Into<f64>) -> Self {
        let min = min.into();
        self.size_rule("min", |s| s >= min, &[("min", number(min))])
    }

    /// At most `max` characters, items, kilobytes (files), or as a number.
    pub fn max(self, max: impl Into<f64>) -> Self {
        let max = max.into();
        self.size_rule("max", |s| s <= max, &[("max", number(max))])
    }

    /// Between `min` and `max` inclusive, measured as `min`/`max` do.
    pub fn between(self, min: impl Into<f64>, max: impl Into<f64>) -> Self {
        let (min, max) = (min.into(), max.into());
        self.size_rule(
            "between",
            |s| s >= min && s <= max,
            &[("min", number(min)), ("max", number(max))],
        )
    }

    /// An email address: something before a single `@`, a dotted domain, no spaces.
    pub fn email(mut self) -> Self {
        if let (true, Inspected::Text(text)) = (self.present(), &self.value)
            && !is_email(text)
        {
            self.fail("email", &[]);
        }
        self
    }

    /// An `http://` or `https://` URL with a host and no spaces.
    pub fn url(mut self) -> Self {
        if let (true, Inspected::Text(text)) = (self.present(), &self.value)
            && !is_url(text)
        {
            self.fail("url", &[]);
        }
        self
    }

    /// An uploaded image: PNG, JPEG, GIF or WebP, checked from the file's
    /// content rather than its name.
    pub fn image(mut self) -> Self {
        if let (true, Inspected::File { image, .. }) = (self.present(), &self.value)
            && !*image
        {
            self.fail("image", &[]);
        }
        self
    }

    /// An uploaded file of one of these types, e.g. `&["jpg", "png", "pdf"]`
    /// (`jpeg` counts as `jpg`). The content decides for the formats Renox can
    /// recognise, the file name for the rest.
    pub fn mimes(mut self, extensions: &[&str]) -> Self {
        if let (true, Inspected::File { extension, .. }) = (self.present(), &self.value) {
            let normalise = |e: &str| match e.to_ascii_lowercase().as_str() {
                "jpeg" => "jpg".to_owned(),
                other => other.to_owned(),
            };
            let ok = extensions
                .iter()
                .any(|e| normalise(e) == normalise(extension));
            if !ok {
                self.fail("mimes", &[("values", extensions.join(", "))]);
            }
        }
        self
    }

    /// One of the given values (Laravel's `in`).
    pub fn one_of<V: FieldValue>(mut self, allowed: &[V]) -> Self {
        if self.present() && !allowed.iter().any(|a| a.inspect() == self.value) {
            self.fail("in", &[]);
        }
        self
    }

    /// Equal to its confirmation field, e.g. `password` and `password_confirmation`.
    pub fn confirmed(mut self, confirmation: &impl FieldValue) -> Self {
        if self.present() && confirmation.inspect() != self.value {
            self.fail("confirmed", &[]);
        }
        self
    }

    /// A checkbox that must be ticked.
    pub fn accepted(mut self) -> Self {
        if !self.failed && self.value != Inspected::Bool(true) {
            self.fail("accepted", &[]);
        }
        self
    }

    /// A custom check: fails with `message` when `valid` is false.
    pub fn rule(mut self, valid: bool, message: impl Into<String>) -> Self {
        if !self.failed && !valid {
            self.v.errors.add(&self.name, message);
            self.failed = true;
            self.last_pending = None;
            self.last_check = None;
        }
        self
    }

    /// The whole text matches `pattern` (a regular expression; anchor it
    /// with `^…$` to match all of it), e.g. `.matches(r"^[A-Z]{2}\d{4}$")`.
    pub fn matches(mut self, pattern: &str) -> Self {
        if let (true, Inspected::Text(text)) = (self.present(), &self.value) {
            let ok = match cached_regex(pattern) {
                Ok(regex) => regex.is_match(text),
                Err(err) => {
                    tracing::error!(pattern, error = %err, "invalid pattern in a `matches` rule");
                    false
                }
            };
            if !ok {
                self.fail("regex", &[]);
            }
        }
        self
    }

    /// Exactly `n` digits (and nothing else), e.g. a PIN.
    pub fn digits(mut self, n: usize) -> Self {
        if self.present() && digit_count(&self.value) != Some(n) {
            self.fail("digits", &[("digits", n.to_string())]);
        }
        self
    }

    /// Between `min` and `max` digits (and nothing else), e.g. a phone number.
    pub fn digits_between(mut self, min: usize, max: usize) -> Self {
        if self.present() && !digit_count(&self.value).is_some_and(|n| n >= min && n <= max) {
            self.fail(
                "digits_between",
                &[("min", min.to_string()), ("max", max.to_string())],
            );
        }
        self
    }

    /// A date (`2026-10-01`) or a date and time (`2026-10-01T10:30`).
    pub fn date(mut self) -> Self {
        if self.present() && self.as_date().is_none() {
            self.fail("date", &[]);
        }
        self
    }

    fn date_rule(
        mut self,
        key: &str,
        limit: NaiveDateTime,
        ok: impl Fn(NaiveDateTime, NaiveDateTime) -> bool,
    ) -> Self {
        if !self.present() {
            return self;
        }
        match self.as_date() {
            None => self.fail("date", &[]),
            Some(date) if !ok(date, limit) => {
                let shown = if limit.time() == chrono::NaiveTime::MIN {
                    limit.date().to_string()
                } else {
                    limit.format("%Y-%m-%d %H:%M").to_string()
                };
                self.fail(key, &[("date", shown)]);
            }
            Some(_) => {}
        }
        self
    }

    /// A date before `limit` (a `NaiveDate`, `NaiveDateTime` or `DateTime`,
    /// or text such as `"2026-01-01"`), e.g. `.before(today)` for a birth date.
    pub fn before(self, limit: impl FieldValue) -> Self {
        match limit_date(&limit) {
            Some(limit) => self.date_rule("before", limit, |d, l| d < l),
            None => self,
        }
    }

    /// A date on or before `limit`.
    pub fn before_or_equal(self, limit: impl FieldValue) -> Self {
        match limit_date(&limit) {
            Some(limit) => self.date_rule("before_or_equal", limit, |d, l| d <= l),
            None => self,
        }
    }

    /// A date after `limit`, e.g. `.after(self.start)` for an end date.
    pub fn after(self, limit: impl FieldValue) -> Self {
        match limit_date(&limit) {
            Some(limit) => self.date_rule("after", limit, |d, l| d > l),
            None => self,
        }
    }

    /// A date on or after `limit`.
    pub fn after_or_equal(self, limit: impl FieldValue) -> Self {
        match limit_date(&limit) {
            Some(limit) => self.date_rule("after_or_equal", limit, |d, l| d >= l),
            None => self,
        }
    }

    fn as_date(&self) -> Option<NaiveDateTime> {
        match &self.value {
            Inspected::Date(date) => Some(*date),
            Inspected::Text(text) => parse_date(text.trim()),
            _ => None,
        }
    }

    /// None of the given values (Laravel's `not_in`).
    pub fn none_of<V: FieldValue>(mut self, refused: &[V]) -> Self {
        if self.present() && refused.iter().any(|r| r.inspect() == self.value) {
            self.fail("not_in", &[]);
        }
        self
    }

    fn text_rule(
        mut self,
        key: &str,
        ok: impl Fn(&str) -> bool,
        params: &[(&str, String)],
    ) -> Self {
        if let (true, Inspected::Text(text)) = (self.present(), &self.value)
            && !ok(text)
        {
            self.fail(key, params);
        }
        self
    }

    /// Letters only (any language's: `é`, `ü`, `ß` count).
    pub fn alpha(self) -> Self {
        self.text_rule("alpha", |t| t.chars().all(char::is_alphabetic), &[])
    }

    /// Letters and digits only.
    pub fn alpha_num(self) -> Self {
        self.text_rule("alpha_num", |t| t.chars().all(char::is_alphanumeric), &[])
    }

    /// Letters, digits, `-` and `_`, e.g. a username or a slug.
    pub fn alpha_dash(self) -> Self {
        self.text_rule(
            "alpha_dash",
            |t| {
                t.chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            },
            &[],
        )
    }

    /// No uppercase letters.
    pub fn lowercase(self) -> Self {
        self.text_rule("lowercase", |t| !t.chars().any(char::is_uppercase), &[])
    }

    /// No lowercase letters.
    pub fn uppercase(self) -> Self {
        self.text_rule("uppercase", |t| !t.chars().any(char::is_lowercase), &[])
    }

    /// Starts with one of `prefixes`, e.g. `.starts_with(&["08", "+62"])`.
    pub fn starts_with(self, prefixes: &[&str]) -> Self {
        let values = prefixes.join(", ");
        self.text_rule(
            "starts_with",
            |t| prefixes.iter().any(|p| t.starts_with(p)),
            &[("values", values)],
        )
    }

    /// Ends with one of `suffixes`, e.g. `.ends_with(&["@company.com"])`.
    pub fn ends_with(self, suffixes: &[&str]) -> Self {
        let values = suffixes.join(", ");
        self.text_rule(
            "ends_with",
            |t| suffixes.iter().any(|s| t.ends_with(s)),
            &[("values", values)],
        )
    }

    /// A UUID (`8-4-4-4-12` hexadecimal digits).
    pub fn uuid(self) -> Self {
        self.text_rule("uuid", is_uuid, &[])
    }

    /// An IPv4 or IPv6 address.
    pub fn ip(self) -> Self {
        self.text_rule("ip", |t| t.trim().parse::<std::net::IpAddr>().is_ok(), &[])
    }

    /// Exactly `size` characters, items, kilobytes (files), or equal to it as
    /// a number (Laravel's `size`).
    pub fn size(self, size: impl Into<f64>) -> Self {
        let size = size.into();
        self.size_rule("size", |s| s == size, &[("size", number(size))])
    }

    /// Required when `other` is empty, e.g. an email when there's no phone.
    pub fn required_without(self, other: &impl FieldValue) -> Self {
        let missing = other.inspect() == Inspected::Missing;
        self.required_if(missing)
    }

    /// Must be empty when `condition` holds (Laravel's `prohibited_if`), e.g.
    /// no discount code on a gift card order.
    pub fn prohibited_if(mut self, condition: bool) -> Self {
        if condition && !self.failed && self.value != Inspected::Missing {
            self.fail("prohibited", &[]);
        }
        self
    }

    /// Required when `condition` holds, e.g.
    /// `.required_if(self.kind == "company")` for a company name.
    pub fn required_if(self, condition: bool) -> Self {
        if condition { self.required() } else { self }
    }

    /// Required unless `condition` holds.
    pub fn required_unless(self, condition: bool) -> Self {
        self.required_if(!condition)
    }

    /// Required when `other` has a value, e.g. a phone number's country
    /// code when a phone number is given.
    pub fn required_with(self, other: &impl FieldValue) -> Self {
        let given = other.inspect() != Inspected::Missing;
        self.required_if(given)
    }

    /// Equal to another field, named `other` in the message.
    pub fn same(mut self, other: &str, value: &impl FieldValue) -> Self {
        if self.present() && value.inspect() != self.value {
            let other = self.v.label_for(other);
            self.fail("same", &[("other", other)]);
        }
        self
    }

    /// Different from another field, named `other` in the message.
    pub fn different(mut self, other: &str, value: &impl FieldValue) -> Self {
        if self.present() && value.inspect() == self.value {
            let other = self.v.label_for(other);
            self.fail("different", &[("other", other)]);
        }
        self
    }

    fn compare_rule(
        mut self,
        key: &str,
        other: &str,
        value: &impl FieldValue,
        ok: fn(std::cmp::Ordering) -> bool,
    ) -> Self {
        if !self.present() {
            return self;
        }
        let other_value = value.inspect();
        if other_value == Inspected::Missing {
            return self;
        }
        // Two texts that both read as numbers compare as numbers (a price
        // in a `String`); other text compares by length.
        let both_numeric = matches!(
            (&self.value, &other_value),
            (Inspected::Text(_), Inspected::Text(_))
        ) && numeric_value(&self.value).is_some()
            && numeric_value(&other_value).is_some();
        let measured = |value: &Inspected| -> Option<(f64, &'static str)> {
            match value {
                Inspected::Text(_) if both_numeric => Some((numeric_value(value)?, "numeric")),
                Inspected::Number(n) => Some((*n, "numeric")),
                Inspected::Text(text) => Some((text.chars().count() as f64, "string")),
                Inspected::Items(n) => Some((*n as f64, "array")),
                Inspected::File { kilobytes, .. } => Some((*kilobytes, "file")),
                _ => None,
            }
        };
        let label = self.v.label_for(other);
        // Dates compare as dates, whatever form they came in.
        let as_date = |value: &Inspected| match value {
            Inspected::Date(date) => Some(*date),
            Inspected::Text(text) => parse_date(text.trim()),
            _ => None,
        };
        let dates = as_date(&self.value).zip(as_date(&other_value));
        if let Some((a, b)) = dates {
            if !ok(a.cmp(&b)) {
                self.fail(&format!("{key}.date"), &[("other", label)]);
            }
            return self;
        }
        match (measured(&self.value), measured(&other_value)) {
            (Some((a, kind)), Some((b, other_kind))) if kind == other_kind => {
                if !a.partial_cmp(&b).is_some_and(ok) {
                    self.fail(&format!("{key}.{kind}"), &[("other", label)]);
                }
            }
            _ => self.fail(&format!("{key}.numeric"), &[("other", label)]),
        }
        self
    }

    /// Greater than another field (Laravel's `gt`), named `other` in the
    /// message: numbers by value (also two texts that both read as
    /// numbers), dates by date, other text by length, lists by items,
    /// files by size, e.g. a maximum price above the minimum. Skipped when
    /// the other field is empty.
    pub fn gt(self, other: &str, value: &impl FieldValue) -> Self {
        self.compare_rule("gt", other, value, |o| o.is_gt())
    }

    /// Greater than or equal to another field; see [`gt`](Self::gt).
    pub fn gte(self, other: &str, value: &impl FieldValue) -> Self {
        self.compare_rule("gte", other, value, |o| o.is_ge())
    }

    /// Less than another field; see [`gt`](Self::gt).
    pub fn lt(self, other: &str, value: &impl FieldValue) -> Self {
        self.compare_rule("lt", other, value, |o| o.is_lt())
    }

    /// Less than or equal to another field; see [`gt`](Self::gt).
    pub fn lte(self, other: &str, value: &impl FieldValue) -> Self {
        self.compare_rule("lte", other, value, |o| o.is_le())
    }

    /// A number with `min` to `max` decimal places (Laravel's `decimal`),
    /// e.g. `.decimal(2, 2)` for a price typed as `12.50`. A text field
    /// keeps what was typed; an `f64` field has lost trailing zeros
    /// (`12.50` is `12.5`), so check prices as text.
    pub fn decimal(mut self, min: usize, max: usize) -> Self {
        if !self.present() {
            return self;
        }
        let text = match &self.value {
            Inspected::Text(text) => text.trim().to_owned(),
            Inspected::Number(n) => n.to_string(),
            _ => String::new(),
        };
        let places = decimal_places(&text);
        if !places.is_some_and(|p| p >= min && p <= max) {
            let places = if min == max {
                min.to_string()
            } else {
                format!("{min}-{max}")
            };
            self.fail("decimal", &[("decimal", places)]);
        }
        self
    }

    /// An uploaded image within `limits`, e.g.
    /// `.dimensions(&Dimensions::new().min_width(400).ratio(16, 9))`; see
    /// [`Dimensions`]. A file that isn't an image fails.
    pub fn dimensions(mut self, limits: &Dimensions) -> Self {
        if let (true, Inspected::File { dimensions, .. }) = (self.present(), &self.value) {
            let ok = dimensions.is_some_and(|(w, h)| limits.allows(w, h));
            if !ok {
                self.fail("dimensions", &[]);
            }
        }
        self
    }

    /// Must be empty (Laravel's `prohibited`), e.g. a field only an admin
    /// may send, checked for everyone else.
    pub fn prohibited(self) -> Self {
        self.prohibited_if(true)
    }

    /// Must be empty unless `condition` holds.
    pub fn prohibited_unless(self, condition: bool) -> Self {
        self.prohibited_if(!condition)
    }

    /// When this field has a value, `other` must be empty (Laravel's
    /// `prohibits`), e.g. a coupon code and a gift card can't both be used.
    pub fn prohibits(mut self, other: &str, value: &impl FieldValue) -> Self {
        if self.present() && value.inspect() != Inspected::Missing {
            let other = self.v.label_for(other);
            self.fail("prohibits", &[("other", other)]);
        }
        self
    }

    /// Required when every one of `others` has a value.
    pub fn required_with_all(self, others: &[&dyn FieldValue]) -> Self {
        let all = others.iter().all(|o| o.inspect() != Inspected::Missing);
        self.required_if(all)
    }

    /// Required when none of `others` has a value, e.g. one way to reach
    /// the customer at least.
    pub fn required_without_all(self, others: &[&dyn FieldValue]) -> Self {
        let none = others.iter().all(|o| o.inspect() == Inspected::Missing);
        self.required_if(none)
    }

    /// An integer of at least `min` digits.
    pub fn min_digits(mut self, min: usize) -> Self {
        if self.present() && !integer_digits(&self.value).is_some_and(|n| n >= min) {
            self.fail("min_digits", &[("min", min.to_string())]);
        }
        self
    }

    /// An integer of at most `max` digits.
    pub fn max_digits(mut self, max: usize) -> Self {
        if self.present() && !integer_digits(&self.value).is_some_and(|n| n <= max) {
            self.fail("max_digits", &[("max", max.to_string())]);
        }
        self
    }

    /// A multiple of `step`, e.g. `.multiple_of(500)` for amounts in
    /// steps of 500, or `.multiple_of(0.25)`.
    pub fn multiple_of(mut self, step: impl Into<f64>) -> Self {
        let step = step.into();
        if !self.present() {
            return self;
        }
        let ok = numeric_value(&self.value).is_some_and(|n| {
            let ratio = n / step;
            step != 0.0 && (ratio - ratio.round()).abs() < 1e-9
        });
        if !ok {
            self.fail("multiple_of", &[("value", number(step))]);
        }
        self
    }

    /// A number: a number field, or text that reads as one (`12`, `-3.5`).
    pub fn numeric(mut self) -> Self {
        if self.present() && numeric_value(&self.value).is_none() {
            self.fail("numeric", &[]);
        }
        self
    }

    /// A whole number: a number field without a fraction, or text that
    /// reads as one.
    pub fn integer(mut self) -> Self {
        if self.present() && !numeric_value(&self.value).is_some_and(|n| n.fract() == 0.0) {
            self.fail("integer", &[]);
        }
        self
    }

    /// Valid JSON text, e.g. a settings field edited by hand.
    pub fn json(self) -> Self {
        self.text_rule(
            "json",
            |t| serde_json::from_str::<serde_json::Value>(t).is_ok(),
            &[],
        )
    }

    /// A ULID (26 characters of Crockford's base 32).
    pub fn ulid(self) -> Self {
        self.text_rule("ulid", |t| t.trim().parse::<crate::db::Ulid>().is_ok(), &[])
    }

    /// An IANA time zone name, e.g. `Asia/Jakarta`, or `UTC`.
    pub fn timezone(self) -> Self {
        self.text_rule(
            "timezone",
            |t| t.trim().parse::<chrono_tz::Tz>().is_ok(),
            &[],
        )
    }

    /// A MAC address: `00:1A:2B:3C:4D:5E`, with `-` as well, or
    /// `001A.2B3C.4D5E`.
    pub fn mac_address(self) -> Self {
        self.text_rule("mac_address", is_mac_address, &[])
    }

    /// ASCII characters only.
    pub fn ascii(self) -> Self {
        self.text_rule("ascii", |t| t.is_ascii(), &[])
    }

    /// A hex colour: `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.
    pub fn hex_color(self) -> Self {
        self.text_rule(
            "hex_color",
            |t| {
                t.strip_prefix('#').is_some_and(|hex| {
                    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
                })
            },
            &[],
        )
    }

    /// Doesn't start with any of `prefixes`.
    pub fn doesnt_start_with(self, prefixes: &[&str]) -> Self {
        let values = prefixes.join(", ");
        self.text_rule(
            "doesnt_start_with",
            |t| !prefixes.iter().any(|p| t.starts_with(p)),
            &[("values", values)],
        )
    }

    /// Doesn't end with any of `suffixes`.
    pub fn doesnt_end_with(self, suffixes: &[&str]) -> Self {
        let values = suffixes.join(", ");
        self.text_rule(
            "doesnt_end_with",
            |t| !suffixes.iter().any(|s| t.ends_with(s)),
            &[("values", values)],
        )
    }

    /// The text does not match `pattern` (Laravel's `not_regex`).
    pub fn not_matches(mut self, pattern: &str) -> Self {
        if let (true, Inspected::Text(text)) = (self.present(), &self.value) {
            let matched = match cached_regex(pattern) {
                Ok(regex) => regex.is_match(text),
                Err(err) => {
                    tracing::error!(pattern, error = %err, "invalid pattern in a `not_matches` rule");
                    true
                }
            };
            if matched {
                self.fail("not_regex", &[]);
            }
        }
        self
    }

    /// A checkbox that must be ticked when `condition` holds.
    pub fn accepted_if(self, condition: bool) -> Self {
        if condition { self.accepted() } else { self }
    }

    /// Must be declined: an unticked checkbox (`false`), or `no`, `off`,
    /// `0` or `false`, e.g. "Don't share my data" answered no.
    pub fn declined(mut self) -> Self {
        let declined = match &self.value {
            Inspected::Bool(b) => !b,
            Inspected::Number(n) => *n == 0.0,
            Inspected::Text(text) => {
                matches!(
                    text.trim().to_ascii_lowercase().as_str(),
                    "no" | "off" | "0" | "false"
                )
            }
            _ => false,
        };
        if !self.failed && !declined {
            self.fail("declined", &[]);
        }
        self
    }

    /// Must be declined when `condition` holds; see [`declined`](Self::declined).
    pub fn declined_if(self, condition: bool) -> Self {
        if condition { self.declined() } else { self }
    }

    /// The value meets `policy` (length, letters, mixed case, numbers,
    /// symbols); see [`Password`].
    pub fn password(mut self, policy: &Password) -> Self {
        if let (true, Inspected::Text(text)) = (self.present(), &self.value) {
            match policy.broken(text) {
                Some((key, params)) => self.fail(key, &params),
                None if policy.uncompromised => {
                    let text = text.clone();
                    self.check(AsyncKind::Uncompromised(text));
                }
                None => {}
            }
        }
        self
    }

    /// The logged-in user's password (Laravel's `current_password`), e.g.
    /// before changing an email address. Checked last, once the other
    /// rules pass; it fails when no one is logged in. Needs
    /// [`Validator::finish_for`], which `Valid<T>` uses.
    pub fn current_password(mut self) -> Self {
        if let (true, Inspected::Text(text)) = (self.present(), &self.value) {
            let text = text.clone();
            self.check(AsyncKind::CurrentPassword(text));
        }
        self
    }

    /// A reusable rule; see [`Rule`].
    pub fn apply(mut self, rule: &impl Rule) -> Self {
        if !self.present() {
            return self;
        }
        if let Err(message) = rule.check(&self.value) {
            let message = render(&message, &self.label, &[]);
            self.v.errors.add(&self.name, message);
            self.failed = true;
            self.last_pending = None;
            self.last_check = None;
        }
        self
    }

    fn database(mut self, table: &str, column: &str, unique: bool) -> Self {
        if self.present() {
            self.v.pending.push(Pending {
                field: self.name.clone(),
                label: self.label.clone(),
                table: table.to_owned(),
                column: column.to_owned(),
                value: self.db_value.clone(),
                ignore_id: None,
                scope: Vec::new(),
                unique,
                message: None,
            });
            self.last_pending = Some(self.v.pending.len() - 1);
            self.last_check = None;
        }
        self
    }

    /// No row in `table` has this value in `column`.
    pub fn unique(self, table: &str, column: &str) -> Self {
        self.database(table, column, true)
    }

    /// Skips the row with this id in the preceding `unique`, for updates:
    /// `.unique("products", "sku").ignore(product.id)`, whatever the key's
    /// type (`i64`, `Ulid`, `Uuid`, `String`).
    pub fn ignore(self, id: impl crate::db::ToDbValue) -> Self {
        if let Some(i) = self.last_pending {
            self.v.pending[i].ignore_id = Some(id.to_db_value());
        }
        self
    }

    /// Some row in `table` has this value in `column`.
    pub fn exists(self, table: &str, column: &str) -> Self {
        self.database(table, column, false)
    }

    /// Only rows where `column = value` count for the preceding `unique` or
    /// `exists`, e.g. the current team's: SKUs are unique per team.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # struct ProductForm { id: i64, sku: String, category_id: i64, team_id: i64 }
    /// # impl Validate for ProductForm {
    /// fn rules(&self, v: &mut Validator) {
    ///     v.field("sku", &self.sku)
    ///         .unique("products", "sku")
    ///         .ignore(self.id)
    ///         .where_eq("team_id", self.team_id)
    ///         .where_null("deleted_at"); // soft-deleted rows don't count
    ///     v.field("category_id", &self.category_id)
    ///         .exists("categories", "id")
    ///         .where_eq("team_id", self.team_id); // not another team's category
    /// }
    /// # }
    /// ```
    pub fn where_eq(self, column: &str, value: impl ToDbValue) -> Self {
        self.scope(ScopeCondition::Eq(column.to_owned(), value.to_db_value()))
    }

    /// Only rows where `column` is null count (e.g. `deleted_at`).
    pub fn where_null(self, column: &str) -> Self {
        self.scope(ScopeCondition::Null(column.to_owned()))
    }

    /// Only rows where `column` is not null count.
    pub fn where_not_null(self, column: &str) -> Self {
        self.scope(ScopeCondition::NotNull(column.to_owned()))
    }

    fn scope(self, condition: ScopeCondition) -> Self {
        if let Some(i) = self.last_pending {
            self.v.pending[i].scope.push(condition);
        }
        self
    }
}

/// What a password must contain; `Field::password(&policy)` checks it.
/// The built-in register, reset and account forms use the app's policy
/// (`Auth::password_rules`), `Password::min(8)` by default.
///
/// ```
/// # use renox::prelude::*;
/// use renox::validation::Password;
/// # struct Form { password: String }
/// # impl Validate for Form {
/// fn rules(&self, v: &mut Validator) {
///     let policy = Password::min(12).mixed_case().numbers().symbols();
///     v.field("password", &self.password).required().password(&policy);
/// }
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Password {
    min: usize,
    letters: bool,
    mixed_case: bool,
    numbers: bool,
    symbols: bool,
    uncompromised: bool,
}

impl Password {
    /// At least `min` characters.
    pub fn min(min: usize) -> Self {
        Self {
            min,
            letters: false,
            mixed_case: false,
            numbers: false,
            symbols: false,
            uncompromised: false,
        }
    }

    /// Not a password found in a known data breach (Laravel's
    /// `uncompromised`), checked with Have I Been Pwned: only the first
    /// five characters of the password's SHA-1 leave the server
    /// (k-anonymity), through `state.http` (faked in tests). When the
    /// service can't be reached the password is allowed and a warning
    /// logged. Runs once the other rules pass.
    pub fn uncompromised(mut self) -> Self {
        self.uncompromised = true;
        self
    }

    /// At least one letter.
    pub fn letters(mut self) -> Self {
        self.letters = true;
        self
    }

    /// At least one uppercase and one lowercase letter.
    pub fn mixed_case(mut self) -> Self {
        self.mixed_case = true;
        self
    }

    /// At least one digit.
    pub fn numbers(mut self) -> Self {
        self.numbers = true;
        self
    }

    /// At least one character that isn't a letter, digit or space.
    pub fn symbols(mut self) -> Self {
        self.symbols = true;
        self
    }

    /// The first rule `password` breaks: a message key and its parameters.
    fn broken(&self, password: &str) -> Option<(&'static str, Vec<(&'static str, String)>)> {
        if password.chars().count() < self.min {
            return Some(("min.string", vec![("min", self.min.to_string())]));
        }
        let has = |test: fn(&char) -> bool| password.chars().any(|c| test(&c));
        if self.letters && !has(|c| c.is_alphabetic()) {
            return Some(("password.letters", Vec::new()));
        }
        if self.mixed_case && !(has(|c| c.is_uppercase()) && has(|c| c.is_lowercase())) {
            return Some(("password.mixed", Vec::new()));
        }
        if self.numbers && !has(|c| c.is_numeric()) {
            return Some(("password.numbers", Vec::new()));
        }
        if self.symbols && !has(|c| !c.is_alphanumeric() && !c.is_whitespace()) {
            return Some(("password.symbols", Vec::new()));
        }
        None
    }
}

impl Default for Password {
    fn default() -> Self {
        Self::min(8)
    }
}

/// A rule to reuse across forms, e.g. a tax number:
///
/// ```
/// # use renox::prelude::*;
/// use renox::validation::{Inspected, Rule};
///
/// struct TaxId;
///
/// impl Rule for TaxId {
///     fn check(&self, value: &Inspected) -> std::result::Result<(), String> {
///         let Inspected::Text(text) = value else { return Ok(()) };
///         let digits = text.chars().filter(char::is_ascii_digit).count();
///         if digits == 15 || digits == 16 {
///             Ok(())
///         } else {
///             Err("The :attribute must be a valid tax ID.".into()) // :attribute is the field's label
///         }
///     }
/// }
///
/// # struct Form { tax_id: String }
/// # impl Validate for Form {
/// fn rules(&self, v: &mut Validator) {
///     v.field("tax_id", &self.tax_id).required().apply(&TaxId);
/// }
/// # }
/// ```
///
/// Missing values skip the rule (combine it with `required`).
pub trait Rule {
    /// `Err(message)` when `value` breaks the rule.
    fn check(&self, value: &Inspected) -> std::result::Result<(), String>;
}

/// Limits for an uploaded image's size in pixels, for
/// [`Field::dimensions`] (Laravel's `dimensions`).
///
/// ```
/// # use renox::prelude::*;
/// use renox::validation::Dimensions;
/// # struct Form { banner: Option<Upload> }
/// # impl Validate for Form {
/// fn rules(&self, v: &mut Validator) {
///     let banner = Dimensions::new().min_width(1200).ratio(3, 1);
///     v.field("banner", &self.banner).image().dimensions(&banner);
/// }
/// # }
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dimensions {
    min_width: Option<u32>,
    max_width: Option<u32>,
    min_height: Option<u32>,
    max_height: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    ratio: Option<(u32, u32)>,
}

impl Dimensions {
    /// No limits yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// At least `px` wide.
    pub fn min_width(mut self, px: u32) -> Self {
        self.min_width = Some(px);
        self
    }

    /// At most `px` wide.
    pub fn max_width(mut self, px: u32) -> Self {
        self.max_width = Some(px);
        self
    }

    /// At least `px` high.
    pub fn min_height(mut self, px: u32) -> Self {
        self.min_height = Some(px);
        self
    }

    /// At most `px` high.
    pub fn max_height(mut self, px: u32) -> Self {
        self.max_height = Some(px);
        self
    }

    /// Exactly `px` wide.
    pub fn width(mut self, px: u32) -> Self {
        self.width = Some(px);
        self
    }

    /// Exactly `px` high.
    pub fn height(mut self, px: u32) -> Self {
        self.height = Some(px);
        self
    }

    /// Width to height in this ratio, e.g. `ratio(16, 9)` or `ratio(1, 1)`
    /// for a square (to within a pixel of rounding).
    pub fn ratio(mut self, width: u32, height: u32) -> Self {
        self.ratio = Some((width, height));
        self
    }

    fn allows(&self, w: u32, h: u32) -> bool {
        let at_least = |limit: Option<u32>, v: u32| limit.is_none_or(|l| v >= l);
        let at_most = |limit: Option<u32>, v: u32| limit.is_none_or(|l| v <= l);
        let exactly = |limit: Option<u32>, v: u32| limit.is_none_or(|l| v == l);
        let ratio = self.ratio.is_none_or(|(rw, rh)| {
            // Within one pixel of the exact height for this width.
            rw > 0 && (h as f64 - w as f64 * rh as f64 / rw as f64).abs() <= 1.0
        });
        at_least(self.min_width, w)
            && at_most(self.max_width, w)
            && at_least(self.min_height, h)
            && at_most(self.max_height, h)
            && exactly(self.width, w)
            && exactly(self.height, h)
            && ratio
    }
}

/// The Have I Been Pwned range API (`app.fake_http()` answers it in tests).
const PWNED_RANGE: &str = "https://api.pwnedpasswords.com/range/";

/// Whether `password` appears in a known breach. Errors (no network, a
/// slow service) count as "no", with a warning, so sign-ups keep working.
async fn breached(state: &crate::AppState, password: &str) -> bool {
    use sha1::{Digest, Sha1};
    let hash: String = Sha1::digest(password.as_bytes())
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let (prefix, suffix) = hash.split_at(5);
    let response = state
        .http
        .get(format!("{PWNED_RANGE}{prefix}"))
        .header("Add-Padding", "true")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => response.text().lines().any(|line| {
            line.split_once(':').is_some_and(|(candidate, count)| {
                candidate.trim().eq_ignore_ascii_case(suffix)
                    && count.trim().parse::<u64>().is_ok_and(|n| n > 0)
            })
        }),
        Ok(response) => {
            tracing::warn!(status = %response.status(), "the password breach check answered with an error; allowing the password");
            false
        }
        Err(err) => {
            tracing::warn!(error = ?err, "the password breach check failed; allowing the password");
            false
        }
    }
}

/// Decimal places of a plain decimal number (`12`, `-3.50`), else `None`.
fn decimal_places(text: &str) -> Option<usize> {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    (!whole.is_empty()
        && all_digits(whole)
        && all_digits(fraction)
        && !(digits.contains('.') && fraction.is_empty()))
    .then_some(fraction.len())
}

/// The date a date rule compares with: a date value, or text that reads as
/// one (`"2026-01-01"`). Anything else leaves the rule out.
fn limit_date(limit: &impl FieldValue) -> Option<NaiveDateTime> {
    match limit.inspect() {
        Inspected::Date(date) => Some(date),
        Inspected::Text(text) => parse_date(text.trim()),
        _ => None,
    }
}

/// The value as a number: a number, or text that reads as a finite one.
fn numeric_value(value: &Inspected) -> Option<f64> {
    match value {
        Inspected::Number(n) => Some(*n),
        Inspected::Text(text) => {
            let text = text.trim();
            decimal_places(text)?;
            text.parse::<f64>().ok().filter(|n| n.is_finite())
        }
        _ => None,
    }
}

/// How many digits an integer value has (the sign doesn't count).
fn integer_digits(value: &Inspected) -> Option<usize> {
    let n = numeric_value(value)?;
    (n.fract() == 0.0).then(|| format!("{}", n.abs() as u64).len())
}

fn is_mac_address(text: &str) -> bool {
    let text = text.trim();
    let hex = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_hexdigit());
    for separator in [':', '-'] {
        let parts: Vec<&str> = text.split(separator).collect();
        if parts.len() == 6 && parts.iter().all(|p| hex(p, 2)) {
            return true;
        }
    }
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| hex(p, 4))
}

fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.trim().split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Compiled patterns, so a rule in a hot form doesn't recompile each time.
fn cached_regex(pattern: &str) -> std::result::Result<regex::Regex, regex::Error> {
    static CACHE: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, regex::Regex>>,
    > = std::sync::LazyLock::new(Default::default);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(regex) = cache.get(pattern) {
        return Ok(regex.clone());
    }
    let regex = regex::Regex::new(pattern)?;
    if cache.len() < 1000 {
        cache.insert(pattern.to_owned(), regex.clone());
    }
    Ok(regex)
}

/// How many digits the value is made of, if it's only digits.
fn digit_count(value: &Inspected) -> Option<usize> {
    let text = match value {
        Inspected::Text(text) => text.trim().to_owned(),
        Inspected::Number(n) if n.fract() == 0.0 && *n >= 0.0 => format!("{}", *n as u64),
        _ => return None,
    };
    (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit())).then_some(text.len())
}

/// `2026-10-01`, `2026-10-01T10:30`, `2026-10-01 10:30:00` or RFC 3339.
fn parse_date(text: &str) -> Option<NaiveDateTime> {
    if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(date.and_time(chrono::NaiveTime::MIN));
    }
    for format in [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(date) = NaiveDateTime::parse_from_str(text, format) {
            return Some(date);
        }
    }
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|d| d.naive_utc())
}

fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !text.chars().any(char::is_whitespace)
        && !domain.contains('@')
        && domain.contains('.')
        && domain.split('.').all(|part| !part.is_empty())
}

fn is_url(text: &str) -> bool {
    let rest = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"));
    matches!(rest, Some(rest) if !rest.is_empty()
        && !rest.starts_with('/')
        && !text.chars().any(char::is_whitespace))
}

/// Keys never flashed back into forms.
const DONT_FLASH: &[&str] = &[
    "password",
    "password_confirmation",
    "current_password",
    "_token",
];

/// A failed validation. As a response it is `422` with
/// `{"message": ..., "errors": {...}}`; for regular (non-HTMX, non-JSON)
/// requests Renox turns it into a redirect back with the errors and old
/// input flashed to the session.
///
/// Return it from a handler for errors found after validation:
///
/// ```
/// # use renox::prelude::*;
/// # #[derive(serde::Serialize)] struct StockForm { quantity: i64 }
/// # fn demo(form: StockForm) -> Result {
/// let mut errors = Errors::new();
/// errors.add("quantity", "Not enough stock.");
/// return Err(ValidationError::new(errors).with_input(&form).into());
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// The messages, keyed by field name.
    pub errors: Errors,
    /// The submitted values flashed back into the form (never passwords).
    pub input: Map<String, Value>,
    /// The named error bag (`in_bag`), for a page with several forms.
    bag: Option<String>,
}

impl ValidationError {
    /// An error with these messages and no input to refill.
    pub fn new(errors: Errors) -> Self {
        Self {
            errors,
            input: Map::new(),
            bag: None,
        }
    }

    /// Flashes the errors in the named bag `bag` (Laravel's error bags) when
    /// the form is sent back: the page shows them with
    /// `error('email', bag='login')`, and the default `error('email')` of
    /// another form on the page stays empty. `Valid<T>` does it for
    /// [`Validate::ERROR_BAG`].
    pub fn in_bag(mut self, bag: impl Into<String>) -> Self {
        self.bag = Some(bag.into());
        self
    }

    /// The named error bag, if any.
    pub fn bag(&self) -> Option<&str> {
        self.bag.as_deref()
    }

    /// The submitted values to refill the form with (passwords are dropped).
    pub fn with_input(mut self, input: &impl Serialize) -> Self {
        if let Ok(Value::Object(map)) = serde_json::to_value(input) {
            self.input = map;
        }
        self.input
            .retain(|key, _| !DONT_FLASH.contains(&key.as_str()));
        self
    }

    pub(crate) fn with_input_map(mut self, input: Map<String, Value>) -> Self {
        self.input = input;
        self.input
            .retain(|key, _| !DONT_FLASH.contains(&key.as_str()));
        self
    }
}

impl From<Errors> for ValidationError {
    fn from(errors: Errors) -> Self {
        Self::new(errors)
    }
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> Response {
        let message = self
            .errors
            .iter()
            .next()
            .and_then(|(_, m)| m.first().cloned())
            .unwrap_or_default();
        let body = json!({ "message": message, "errors": self.errors });
        let mut res = (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(body)).into_response();
        res.extensions_mut().insert(self);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_emails_and_urls() {
        assert!(is_email("alex@example.com"));
        assert!(!is_email("alex@localhost"));
        assert!(!is_email("alex example@x.com"));
        assert!(!is_email("@x.com"));
        assert!(is_url("https://renox.dev/docs"));
        assert!(!is_url("ftp://renox.dev"));
        assert!(!is_url("https://"));
    }

    #[test]
    fn formats_numbers_without_trailing_zeros() {
        assert_eq!(number(3.0), "3");
        assert_eq!(number(2.5), "2.5");
    }
    /// A value that inspects as whatever a test needs (a file, a bool).
    struct As(Inspected);

    impl FieldValue for As {
        fn inspect(&self) -> Inspected {
            self.0.clone()
        }
        fn db_value(&self) -> DbValue {
            DbValue::Null
        }
    }

    /// A rule that fails every value it sees.
    struct Never;

    impl Rule for Never {
        fn check(&self, _: &Inspected) -> std::result::Result<(), String> {
            Err("never".into())
        }
    }

    fn file(kilobytes: f64, dimensions: Option<(u32, u32)>) -> As {
        As(Inspected::File {
            kilobytes,
            extension: "png".into(),
            image: true,
            dimensions,
        })
    }

    // #250: the branches rules take for values they don't measure, and the
    // passing side of rules only failures had reached.
    #[test]
    fn rules_pass_or_step_aside_for_values_they_dont_judge() {
        let mut v = Validator::default();
        let photo = file(120.0, Some((800, 600)));
        v.field("photo", &photo)
            .mimes(&["PNG", "jpeg"])
            .dimensions(&Dimensions::new().min_width(400).max_height(600));
        v.field("code", &"AB1234")
            .matches(r"^[A-Z]{2}\d{4}$")
            .not_matches(r"^\d+$");
        v.field("secret", &"long enough password")
            .password(&Password::min(8));
        // Rules that can't read the value leave it alone.
        let missing: Option<String> = None;
        v.field("note", &missing)
            .decimal(2, 2)
            .apply(&Never)
            .gt("other", &"5");
        v.field("price", &"12").gt("budget", &missing);
        // A limit that isn't a date leaves the date rules out.
        v.field("day", &"2026-01-01")
            .before_or_equal(5)
            .after(true)
            .after_or_equal(2.5);
        assert!(v.errors.is_empty(), "{:?}", v.errors);
    }

    #[test]
    fn rules_fail_values_of_the_wrong_kind() {
        let mut v = Validator::default();
        // A number isn't a date; a bool has no digits, no decimals, no number.
        v.field("day", &7).before("2026-01-01");
        v.field("agree", &true).digits(3);
        v.field("flag", &true).decimal(2, 2);
        v.field("on", &true).numeric();
        // Two files compare by size; two booleans can't be compared.
        v.field("photo", &file(10.0, None))
            .gt("thumb", &file(20.0, None));
        v.field("yes", &true).lt("no", &false);
        assert_eq!(v.errors.first("day"), Some("The day is not a valid date."));
        assert_eq!(v.errors.first("agree"), Some("The agree must be 3 digits."));
        assert_eq!(
            v.errors.first("flag"),
            Some("The flag must have 2 decimal places.")
        );
        assert_eq!(v.errors.first("on"), Some("The on must be a number."));
        assert_eq!(
            v.errors.first("photo"),
            Some("The photo must be larger than thumb.")
        );
        assert!(v.errors.has("yes"), "{:?}", v.errors);
        // An image too wide fails `dimensions`.
        let mut v = Validator::new();
        v.field("photo", &file(1.0, Some((2000, 10))))
            .dimensions(&Dimensions::new().max_width(100));
        assert!(v.errors.has("photo"));
    }

    #[test]
    fn nested_rows_carry_their_async_checks_under_the_row_name() {
        struct Row {
            password: String,
        }
        impl Validate for Row {
            fn rules(&self, v: &mut Validator) {
                v.field("password", &self.password).current_password();
            }
        }
        let mut v = Validator::new();
        v.nested(
            "rows",
            &[
                Row {
                    password: "a".into(),
                },
                Row {
                    password: "b".into(),
                },
            ],
        );
        let fields: Vec<_> = v.checks.iter().map(|c| c.field.as_str()).collect();
        assert_eq!(fields, ["rows.0.password", "rows.1.password"]);
    }

    #[tokio::test]
    async fn checks_on_a_field_that_already_failed_are_skipped() {
        let db = crate::db::connect(&crate::Config::default()).await.unwrap();
        let mut v = Validator::new();
        // The field failed a rule before its database and async checks were
        // added: neither runs (no `users` table is asked), one error stays.
        v.field("email", &"not an email").email();
        v.field("email", &"not an email")
            .unique("no_such_table", "email")
            .current_password();
        // `.message` after an async check is that check's message.
        v.field("password", &"guess")
            .current_password()
            .message("That isn't your password.");
        let errors = v.finish(&db).await.unwrap();
        assert_eq!(
            errors.first("email"),
            Some("The email must be a valid email address.")
        );
        assert_eq!(
            errors.iter().find(|(f, _)| *f == "email").unwrap().1.len(),
            1
        );
        assert_eq!(errors.first("password"), Some("That isn't your password."));
    }
}

//! A resource's form fields: what [`AdminResource::fields`] returns, drawn
//! on the create and edit pages with the UI kit's field macros.
//!
//! [`AdminResource::fields`]: crate::AdminResource::fields

use renox::serde_json::Value;
use serde::Serialize;

/// What a [`Field`] asks for, which decides the kit macro that draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FieldKind {
    /// One line of text.
    Text,
    /// An email address (`type="email"`).
    Email,
    /// A password: never filled in from the record.
    Password,
    /// A web address (`type="url"`).
    Url,
    /// A phone number (`type="tel"`).
    Tel,
    /// Several lines of text.
    Textarea,
    /// A number (`type="number"`).
    Number,
    /// An amount kept in the currency's smallest unit, shown and typed in
    /// whole units, with the currency's code before it.
    Money,
    /// A date with a calendar (`NaiveDate`, sent as `YYYY-MM-DD`).
    Date,
    /// A date and a time (`type="datetime-local"`, sent as
    /// `YYYY-MM-DDTHH:MM`).
    DateTime,
    /// One of a few values.
    Select,
    /// A yes/no checkbox (`bool`).
    Checkbox,
    /// A yes/no switch (`bool`).
    Toggle,
    /// The id of a row of another table, chosen by one of its columns.
    BelongsTo,
}

/// One field of a resource's form: its name (the form's and the model's
/// field), its label, its kind and how it looks. The rules are the form
/// type's own ([`Validate`](renox::Validate)); `required` here only marks the
/// field for people and the browser.
///
/// ```
/// use renox_admin::Field;
///
/// let fields = vec![
///     Field::text("name", "Name").required(),
///     Field::select("status", "Status", [("draft", "Draft"), ("live", "Live")]).required(),
///     Field::money("price", "Price").required(),
///     Field::belongs_to("category_id", "Category", "categories", "name"),
///     Field::textarea("notes", "Notes").span_full(),
///     Field::toggle("active", "Active").default_value(true),
/// ];
/// # assert_eq!(fields.len(), 6);
/// ```
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct Field {
    name: String,
    label: String,
    kind: FieldKind,
    options: Vec<(String, String)>,
    required: bool,
    hint: Option<String>,
    placeholder: Option<String>,
    prefix: Option<String>,
    suffix: Option<String>,
    span: Option<String>,
    rows: u32,
    step: Option<String>,
    min: Option<String>,
    max: Option<String>,
    autocomplete: Option<String>,
    searchable: bool,
    default: Option<Value>,
    on_create: bool,
    on_edit: bool,
    readonly_on_edit: bool,
    #[serde(skip)]
    relation: Option<(String, String)>,
}

impl Field {
    fn new(name: &str, label: &str, kind: FieldKind) -> Self {
        Self {
            name: name.to_owned(),
            label: label.to_owned(),
            kind,
            options: Vec::new(),
            required: false,
            hint: None,
            placeholder: None,
            prefix: None,
            suffix: None,
            span: None,
            rows: 4,
            step: None,
            min: None,
            max: None,
            autocomplete: None,
            searchable: false,
            default: None,
            on_create: true,
            on_edit: true,
            readonly_on_edit: false,
            relation: None,
        }
    }

    /// One line of text.
    pub fn text(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Text)
    }

    /// An email address.
    pub fn email(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Email)
    }

    /// A password. The edit page leaves it empty: make the form's field an
    /// `Option<String>` and change the password only when one was typed.
    pub fn password(name: &str, label: &str) -> Self {
        let mut field = Self::new(name, label, FieldKind::Password);
        field.autocomplete = Some("new-password".into());
        field
    }

    /// A web address.
    pub fn url(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Url)
    }

    /// A phone number.
    pub fn tel(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Tel)
    }

    /// Several lines of text (`rows(n)`, 4 by default).
    pub fn textarea(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Textarea)
    }

    /// A number (`step`, `min` and `max` limit it).
    pub fn number(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Number)
    }

    /// An amount the model keeps in the smallest unit (cents, like the
    /// data grid's `money` columns), with `APP_CURRENCY`'s code before it.
    /// The form shows and takes whole units: with `USD`, `1299` shows as
    /// `12.99` (a `0.01` step), and `12.99` is read back as `1299` before
    /// the form's rules and [`fill`](crate::AdminResource::fill) see it
    /// (rounded to the cent). A currency without decimals (IDR, JPY) is
    /// as typed. Rules on the form (`min`, `max`) are in the smallest unit.
    pub fn money(name: &str, label: &str) -> Self {
        let mut field = Self::new(name, label, FieldKind::Money);
        field.min = Some("0".into());
        field
    }

    /// A date with the kit's calendar, sent as `YYYY-MM-DD` (a `NaiveDate`).
    pub fn date(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Date)
    }

    /// A date and a time, sent as `YYYY-MM-DDTHH:MM` (a `NaiveDateTime` in
    /// the form; Renox adds the seconds). The edit page shows the stored
    /// moment's first 16 characters, so store UTC or a naive time.
    pub fn datetime(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::DateTime)
    }

    /// One of `options`, `(value, label)` pairs. `searchable()` adds a box
    /// that filters them.
    pub fn select<V: Into<String>, L: Into<String>>(
        name: &str,
        label: &str,
        options: impl IntoIterator<Item = (V, L)>,
    ) -> Self {
        let mut field = Self::new(name, label, FieldKind::Select);
        field.options = options
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect();
        field
    }

    /// A checkbox: a `bool` in the form (ticked or not).
    pub fn checkbox(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Checkbox)
    }

    /// A switch: a `bool` in the form, for a setting that is on or off.
    pub fn toggle(name: &str, label: &str) -> Self {
        Self::new(name, label, FieldKind::Toggle)
    }

    /// The id of a row of `table`, chosen by its `title` column: a
    /// searchable select of `table`'s rows (the first 1,000, by `title`).
    /// Pair it with `#[validate(exists("table", "id"))]` on the form.
    /// `table` and `title` may only have letters, digits and `_`.
    pub fn belongs_to(name: &str, label: &str, table: &str, title: &str) -> Self {
        let mut field = Self::new(name, label, FieldKind::BelongsTo);
        field.relation = Some((table.to_owned(), title.to_owned()));
        field.searchable = true;
        field
    }

    /// Marks the field required for people and the browser (the form's
    /// rules decide).
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// A line of help under the field.
    pub fn hint(mut self, hint: &str) -> Self {
        self.hint = Some(hint.to_owned());
        self
    }

    /// Text shown in the empty field (for a select: its empty choice).
    pub fn placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = Some(placeholder.to_owned());
        self
    }

    /// Text joined to the start of the field ("https://", "$").
    pub fn prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_owned());
        self
    }

    /// Text joined to the end of the field ("kg", "%").
    pub fn suffix(mut self, suffix: &str) -> Self {
        self.suffix = Some(suffix.to_owned());
        self
    }

    /// Takes the form's whole width (it has two columns on wider screens).
    pub fn span_full(mut self) -> Self {
        self.span = Some("full".into());
        self
    }

    /// The lines a textarea shows.
    pub fn rows(mut self, rows: u32) -> Self {
        self.rows = rows;
        self
    }

    /// A number's step (`"0.01"`, `"any"`).
    pub fn step(mut self, step: &str) -> Self {
        self.step = Some(step.to_owned());
        self
    }

    /// The smallest number, or the earliest date.
    pub fn min(mut self, min: impl ToString) -> Self {
        self.min = Some(min.to_string());
        self
    }

    /// The largest number, or the latest date.
    pub fn max(mut self, max: impl ToString) -> Self {
        self.max = Some(max.to_string());
        self
    }

    /// The browser's `autocomplete` hint (`"off"`, `"email"`).
    pub fn autocomplete(mut self, autocomplete: &str) -> Self {
        self.autocomplete = Some(autocomplete.to_owned());
        self
    }

    /// For a select: a box to type in that filters the options.
    pub fn searchable(mut self) -> Self {
        self.searchable = true;
        self
    }

    /// The value the create page starts with (the edit page shows the
    /// record's).
    pub fn default_value(mut self, value: impl Serialize) -> Self {
        self.default = renox::serde_json::to_value(value).ok();
        self
    }

    /// Only on the create page.
    pub fn only_on_create(mut self) -> Self {
        self.on_edit = false;
        self
    }

    /// Only on the edit page.
    pub fn only_on_edit(mut self) -> Self {
        self.on_create = false;
        self
    }

    /// Shown but not changeable on the edit page (it is still sent, so the
    /// form's rules see it).
    pub fn readonly_on_edit(mut self) -> Self {
        self.readonly_on_edit = true;
        self
    }

    /// The field's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The field's kind.
    pub fn kind(&self) -> FieldKind {
        self.kind
    }

    pub(crate) fn shown(&self, creating: bool) -> bool {
        if creating {
            self.on_create
        } else {
            self.on_edit
        }
    }

    pub(crate) fn relation(&self) -> Option<(&str, &str)> {
        self.relation
            .as_ref()
            .map(|(table, title)| (table.as_str(), title.as_str()))
    }

    pub(crate) fn set_options(&mut self, options: Vec<(String, String)>) {
        self.options = options;
    }

    pub(crate) fn label_text(&self) -> &str {
        &self.label
    }

    pub(crate) fn has_placeholder(&self) -> bool {
        self.placeholder.is_some()
    }

    pub(crate) fn is_required(&self) -> bool {
        self.required
    }

    pub(crate) fn min_text(&self) -> Option<&str> {
        self.min.as_deref()
    }

    pub(crate) fn max_text(&self) -> Option<&str> {
        self.max.as_deref()
    }

    pub(crate) fn choices(&self) -> &[(String, String)] {
        &self.options
    }

    /// Runs the field's words (label, hint, placeholder, affixes and the
    /// choices' labels) through `translate`.
    pub(crate) fn translate(&mut self, translate: &dyn Fn(&str) -> String) {
        self.label = translate(&self.label);
        for text in [
            &mut self.hint,
            &mut self.placeholder,
            &mut self.prefix,
            &mut self.suffix,
        ]
        .into_iter()
        .flatten()
        {
            *text = translate(text);
        }
        for (_, label) in &mut self.options {
            *label = translate(label);
        }
    }
}

/// Whether `name` is a plain SQL name: letters, digits and `_`.
pub(crate) fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

//! Data grids: a table that fills the screen, with server-side filters,
//! sorting and pagination, columns each user picks per screen size, frozen
//! columns, grouped headers and cells of any kind.
//!
//! A grid is defined once in Rust and rendered with the `grid` macro of
//! `renox/grid.html`:
//!
//! ```
//! use renox::prelude::*;
//! use renox::grid::{Column, Grid, GridRequest};
//!
//! #[derive(Model, serde::Serialize, Default)]
//! struct Order { id: i64, number: String, customer: String, status: String, total: i64 }
//!
//! fn orders() -> Grid {
//!     Grid::new("orders")
//!         .title("Orders")
//!         .column(Column::text("number", "Order").frozen().mobile())
//!         .column(Column::text("customer", "Customer").mobile())
//!         .column(Column::select("status", "Status", [("new", "New"), ("paid", "Paid")]).mobile())
//!         .column(Column::money("total", "Total").under(["Amounts"]))
//!         .sort_by("-id")
//! }
//!
//! async fn index(request: GridRequest) -> Result<View> {
//!     let page = orders().page(Order::query(), &request).await?;
//!     Ok(view("orders/index.html", context! { orders => page }))
//! }
//! ```
//!
//! ```jinja
//! {% from "renox/grid.html" import grid %}
//! <main class="rx-grid-fill">{{ grid(orders) }}</main>
//! ```
//!
//! Filters, sorting and pages travel in the query string (`q.number=A%`,
//! `min.total=1000`, `from.ordered_on=2026-01-01`, `in.status=paid`,
//! `sort=-total`, `page=2`, `per_page=50`), so a filtered page has a link of
//! its own. Only columns the grid defines are filtered or sorted, and the
//! query builder checks the names again. Columns of kind `custom` are drawn
//! by the template: `{% call(row, column) grid(orders) %}…{% endcall %}`.
//!
//! What a user picks in the column menu (visible columns on small and wide
//! screens, order, frozen columns) is kept per user in `grid_preferences`,
//! or in the session for guests.

use std::collections::BTreeMap;

use axum::Router;
use axum::extract::{FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

mod export;

pub use export::{ExportFormat, MAX_EXPORT_ROWS, Workbook};

use crate::auth::AuthUser;
use crate::db::{Db, DbValue, Model, Paginated, Query, ToDbValue};
use crate::{AppState, Error, Result, Session};

/// The framework migration creating `grid_preferences` (every app gets it).
pub(crate) const MIGRATION: crate::db::Migration =
    crate::db::framework_migration!("grid", "00010101000220_create_grid_preferences_table");

/// Rows per page a user may pick when the grid doesn't say.
const PER_PAGE_OPTIONS: [u32; 4] = [10, 25, 50, 100];

/// What a column holds, which decides how its cells look and how it filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Kind {
    /// Text; filtered with "contains", "starts with", "ends with", "equals",
    /// or a pattern with `%`.
    Text,
    /// A number, right-aligned; filtered with a minimum and a maximum.
    Number,
    /// An amount in the smallest unit (cents, fils), with thousands
    /// separators; filtered like a number.
    Money,
    /// A date (`NaiveDate`); filtered with a date range.
    Date,
    /// A moment (`DateTime`); filtered with a date range.
    DateTime,
    /// Yes or no; filtered with a choice.
    Bool,
    /// One of a few values; filtered by picking some of them.
    Select,
    /// Several of a few values, stored as a JSON array of strings
    /// (`Json<Vec<String>>`); filtered by rows having any of the picked ones.
    Tags,
    /// Drawn by the page's template (`caller(row, column)`): charts, buttons,
    /// links. Not sorted or filtered.
    Custom,
    /// An image whose URL is the value (an avatar, a product photo).
    Image,
    /// A CSS color (`#0a7d5a`) shown as a swatch with its code.
    Color,
}

/// Which edge a frozen column sticks to while the grid scrolls sideways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pin {
    /// The left edge.
    Left,
    /// The right edge.
    Right,
}

/// A figure a column shows in the footer and under each group
/// ([`Column::summary`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Summary {
    /// The total.
    Sum,
    /// The average.
    Average,
    /// The smallest and the largest value.
    Range,
    /// How many rows have a value.
    Count,
}

/// One column of a [`Grid`]; build it with a constructor per [`Kind`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Column {
    key: String,
    label: String,
    kind: Kind,
    under: Vec<String>,
    sortable: bool,
    filterable: bool,
    mobile: bool,
    hidden: bool,
    pin: Option<Pin>,
    decimals: Option<u8>,
    options: Vec<(String, String)>,
    width: Option<String>,
    editable: bool,
    merge: bool,
    searchable: bool,
    summaries: Vec<Summary>,
    badges: Option<Vec<(String, String)>>,
    icons: bool,
    description: Option<String>,
    tooltip: Option<String>,
    wrap: bool,
    limit: Option<usize>,
    link: Option<String>,
    copyable: bool,
    round: bool,
    /// A value from another table: SQL with `{T}` for this model's table.
    expr: Option<String>,
}

impl Column {
    fn new(key: &str, label: &str, kind: Kind) -> Self {
        let plain = kind != Kind::Custom;
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            kind,
            under: Vec::new(),
            sortable: plain && kind != Kind::Tags,
            filterable: plain,
            mobile: false,
            hidden: false,
            pin: None,
            decimals: None,
            options: Vec::new(),
            width: None,
            editable: false,
            merge: false,
            searchable: false,
            summaries: Vec::new(),
            badges: None,
            icons: false,
            description: None,
            tooltip: None,
            wrap: false,
            limit: None,
            link: None,
            copyable: false,
            round: false,
            expr: None,
        }
    }

    /// A value of the row this one belongs to: `table`'s `column` where its
    /// `id` is this row's `foreign_key`
    /// (`Column::related("customer", "Customer", "customers", "customer_id", "name")`).
    /// Shown, sorted, filtered and searched like a text column (`.numeric()`
    /// for a number), in one query per page.
    pub fn related(key: &str, label: &str, table: &str, foreign_key: &str, column: &str) -> Self {
        let mut c = Self::new(key, label, Kind::Text);
        if [table, foreign_key, column].iter().all(|n| plain_name(n)) {
            c.expr = Some(format!(
                "(SELECT \"{table}\".\"{column}\" FROM \"{table}\" WHERE \"{table}\".\"id\" = {{T}}.\"{foreign_key}\")"
            ));
        }
        c
    }

    /// How many rows of `table` point at this row through `foreign_key`
    /// (`Column::count_of("notes", "Notes", "order_notes", "order_id")`).
    pub fn count_of(key: &str, label: &str, table: &str, foreign_key: &str) -> Self {
        let mut c = Self::new(key, label, Kind::Number);
        if [table, foreign_key].iter().all(|n| plain_name(n)) {
            c.expr = Some(format!(
                "(SELECT COUNT(*) FROM \"{table}\" WHERE \"{table}\".\"{foreign_key}\" = {{T}}.\"id\")"
            ));
        }
        c
    }

    /// The total of `column` over the rows of `table` that point at this
    /// row through `foreign_key`.
    pub fn sum_of(key: &str, label: &str, table: &str, foreign_key: &str, column: &str) -> Self {
        let mut c = Self::new(key, label, Kind::Number);
        if [table, foreign_key, column].iter().all(|n| plain_name(n)) {
            c.expr = Some(format!(
                "(SELECT COALESCE(SUM(\"{table}\".\"{column}\"), 0) FROM \"{table}\" WHERE \"{table}\".\"{foreign_key}\" = {{T}}.\"id\")"
            ));
        }
        c
    }

    /// The column's value is `sql`, a scalar SQL expression over the row,
    /// where `{T}` stands for the model's table: stock value as
    /// `Column::money("value", "Value").computed("{T}.\"on_hand\" * (SELECT \"cost\" FROM \"variants\" WHERE \"id\" = {T}.\"variant_id\")")`.
    /// It is shown, sorted, filtered, searched, summed ([`Column::summary`])
    /// and grouped by ([`Grid::groups`]) like a related column, with no
    /// database view. `sql` is written by the app, never taken from a
    /// request: values go in the model's columns and subqueries, not in
    /// the string.
    pub fn computed(mut self, sql: &str) -> Self {
        self.expr = Some(sql.to_owned());
        self
    }

    /// Shows a related value as a number (filtered with a range).
    pub fn numeric(mut self) -> Self {
        if self.expr.is_some() {
            self.kind = Kind::Number;
        }
        self
    }

    /// The SQL this column reads, for `M`'s table: its own column or the
    /// related value.
    fn target<M: Model>(&self) -> Target {
        match &self.expr {
            Some(expr) => Target::Expr(expr.replace("{T}", &format!("\"{}\"", M::TABLE))),
            None => Target::Column(self.key.clone()),
        }
    }

    /// An image column: the value is the image's URL (`.round()` for
    /// avatars).
    pub fn image(key: &str, label: &str) -> Self {
        let mut column = Self::new(key, label, Kind::Image);
        column.sortable = false;
        column.filterable = false;
        column
    }

    /// A color column: a swatch of the CSS color in the value, with its code.
    pub fn color(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::Color)
    }

    /// Values as colored badges: `(value, tone)` pairs, the tone one of
    /// `success`, `warning`, `danger`, `info` or `neutral`. Values not
    /// listed get a neutral badge; `.badges(&[])` makes every value one.
    pub fn badges(mut self, tones: &[(&str, &str)]) -> Self {
        self.badges = Some(
            tones
                .iter()
                .map(|(v, t)| ((*v).to_owned(), (*t).to_owned()))
                .collect(),
        );
        self
    }

    /// Yes/no columns as a check and a cross instead of words.
    pub fn icons(mut self) -> Self {
        self.icons = true;
        self
    }

    /// Another of the row's values, small under this one (an email under a
    /// name).
    pub fn description(mut self, key: &str) -> Self {
        self.description = Some(key.to_owned());
        self
    }

    /// Another of the row's values, shown when the cell is hovered.
    pub fn tooltip(mut self, key: &str) -> Self {
        self.tooltip = Some(key.to_owned());
        self
    }

    /// Long text wraps instead of widening the column.
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    /// Text past `chars` characters is cut with `…` (the whole text shows on
    /// hover).
    pub fn limit(mut self, chars: usize) -> Self {
        self.limit = Some(chars);
        self
    }

    /// The value links to this URL (`{id}` is the row's id).
    pub fn link(mut self, url: &str) -> Self {
        self.link = Some(url.to_owned());
        self
    }

    /// A button next to the value copies it.
    pub fn copyable(mut self) -> Self {
        self.copyable = true;
        self
    }

    /// Images round (avatars).
    pub fn round(mut self) -> Self {
        self.round = true;
        self
    }

    /// A text column; `key` is the model's column.
    pub fn text(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::Text)
    }

    /// A number column (`.decimals(n)` for a fixed number of decimals).
    pub fn number(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::Number)
    }

    /// An amount in the smallest unit (cents, fils), shown in whole units
    /// of `APP_CURRENCY` with its usual decimals (`129900` cents is
    /// `1,299.00` in USD, `400,000` in IDR), as the `money` template filter
    /// takes it. Summaries and exports show whole units too,
    /// and range filters take them. An inline edit still sends the stored
    /// value (the smallest unit).
    pub fn money(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::Money)
    }

    /// A date column.
    pub fn date(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::Date)
    }

    /// A date and time column (shown in `APP_TIMEZONE`).
    pub fn datetime(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::DateTime)
    }

    /// A yes/no column.
    pub fn bool(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::Bool)
    }

    /// One of `options` (stored value, label shown).
    pub fn select<V: Into<String>, L: Into<String>>(
        key: &str,
        label: &str,
        options: impl IntoIterator<Item = (V, L)>,
    ) -> Self {
        let mut column = Self::new(key, label, Kind::Select);
        column.options = options
            .into_iter()
            .map(|(v, l)| (v.into(), l.into()))
            .collect();
        column
    }

    /// Several of `options`, stored as a JSON array of the values.
    pub fn tags<V: Into<String>, L: Into<String>>(
        key: &str,
        label: &str,
        options: impl IntoIterator<Item = (V, L)>,
    ) -> Self {
        let mut column = Self::select(key, label, options);
        column.kind = Kind::Tags;
        column
    }

    /// A column the page's template draws (`{% call(row, column) grid(…) %}`).
    /// `key` needn't be a database column.
    pub fn custom(key: &str, label: &str) -> Self {
        Self::new(key, label, Kind::Custom)
    }

    /// Puts the column under grouped headings, outermost first:
    /// `.under(["Amounts"])`, `.under(["Sales", "Q1"])`. Neighbouring
    /// columns under the same headings share them.
    pub fn under<S: Into<String>>(mut self, groups: impl IntoIterator<Item = S>) -> Self {
        self.under = groups.into_iter().map(Into::into).collect();
        self
    }

    /// Whether a click on the heading sorts by this column (default: yes,
    /// except for tags and custom columns).
    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable && self.kind != Kind::Custom;
        self
    }

    /// Whether the heading has a filter (default: yes, except for custom
    /// columns).
    pub fn filterable(mut self, filterable: bool) -> Self {
        self.filterable = filterable && self.kind != Kind::Custom;
        self
    }

    /// Shown on small screens until the user picks otherwise. When no column
    /// says so, the first three are.
    pub fn mobile(mut self) -> Self {
        self.mobile = true;
        self
    }

    /// Hidden on wide screens too until the user turns it on.
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// Frozen at the left edge until the user picks otherwise.
    pub fn frozen(mut self) -> Self {
        self.pin = Some(Pin::Left);
        self
    }

    /// Frozen at the right edge (e.g. a column of actions).
    pub fn frozen_right(mut self) -> Self {
        self.pin = Some(Pin::Right);
        self
    }

    /// Shows a number with exactly `decimals` decimals.
    pub fn decimals(mut self, decimals: u8) -> Self {
        self.decimals = Some(decimals);
        self
    }

    /// A minimum width in CSS units, e.g. `"14rem"`.
    pub fn width(mut self, width: &str) -> Self {
        self.width = Some(width.to_owned());
        self
    }

    /// Cells can be edited in place (double-click, Enter or F2; Enter saves,
    /// Escape cancels) and in the row's edit mode. Saving sends the value
    /// to the grid's [`Grid::edit_url`]. Not for custom columns, nor for
    /// merged ones.
    pub fn editable(mut self) -> Self {
        self.editable = self.kind != Kind::Custom;
        self
    }

    /// Neighbouring rows with the same value share one cell. Merged columns
    /// nest from left to right: a column merges only within the merged
    /// groups of the merged columns before it (Region, then City). Merging
    /// is off while rows are dragged into order.
    pub fn merge(mut self) -> Self {
        self.merge = true;
        self.editable = false;
        self
    }

    /// The toolbar's search box looks in this column (as text, ignoring
    /// case). The box shows when a column is searchable. Not for custom
    /// columns.
    ///
    /// When the grid's model has a full-text index (`#[model(search = …)]`,
    /// see [`renox::db::search`](crate::db::search)), the box searches
    /// through it instead: every column of the index counts (also those the
    /// grid doesn't show), words match their other forms and prefixes, and
    /// rows come best match first until the user sorts. Searchable columns
    /// outside the index are still matched as text.
    pub fn searchable(mut self) -> Self {
        self.searchable = self.kind != Kind::Custom;
        self
    }

    /// A figure over every row the filters match, in the grid's footer and
    /// under each group: `.summary(Summary::Sum)`, several in a row
    /// (`.summary(Summary::Sum).summary(Summary::Average)`). Sum, average
    /// and range are for number and money columns; count for any.
    pub fn summary(mut self, summary: Summary) -> Self {
        let numeric = matches!(self.kind, Kind::Number | Kind::Money);
        if (numeric || summary == Summary::Count) && !self.summaries.contains(&summary) {
            self.summaries.push(summary);
        }
        self
    }

    /// The column's key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The column's kind.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The column's heading.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The same column under another heading, e.g. a translation.
    pub fn titled(mut self, label: &str) -> Self {
        label.clone_into(&mut self.label);
        self
    }

    /// The `(value, label)` choices of a `select` or `tags` column (empty
    /// for the other kinds).
    pub fn options(&self) -> &[(String, String)] {
        &self.options
    }
}

/// A data grid's definition: its columns and defaults. Cheap to build, so
/// a function returning it is the usual place for one.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Grid {
    id: String,
    title: Option<String>,
    columns: Vec<Column>,
    per_page: u32,
    per_page_options: Vec<u32>,
    sort: Option<String>,
    audit: bool,
    details: bool,
    edit_url: Option<String>,
    reorder: Option<(String, String)>,
    exports: bool,
    prefix: Option<String>,
    row_url: Option<String>,
    empty: Option<(String, Option<String>)>,
    bulk: Vec<Action>,
    row_actions: Vec<Action>,
    groups: Vec<String>,
    group: Option<String>,
    cards: bool,
    advanced: bool,
    poll: Option<u32>,
    remember: bool,
}

impl Grid {
    /// A grid named `id` (letters, digits, `_` and `-`), which keys each
    /// user's preferences and the element's id.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            title: None,
            columns: Vec::new(),
            per_page: 25,
            per_page_options: PER_PAGE_OPTIONS.to_vec(),
            sort: None,
            audit: false,
            details: false,
            edit_url: None,
            reorder: None,
            exports: false,
            prefix: None,
            row_url: None,
            empty: None,
            bulk: Vec::new(),
            row_actions: Vec::new(),
            groups: Vec::new(),
            group: None,
            cards: false,
            advanced: false,
            poll: None,
            remember: false,
        }
    }

    /// Reloads the grid's page every `seconds` (at least 5) while the tab is
    /// visible and nobody is editing, selecting or filtering in it.
    pub fn poll(mut self, seconds: u32) -> Self {
        self.poll = Some(seconds.max(5));
        self
    }

    /// Keeps the user's filters, search, sort, group and page size in the
    /// session: coming back to the page shows the grid as they left it.
    pub fn remember(mut self) -> Self {
        self.remember = true;
        self
    }

    /// The request's values for this grid (without the prefix); with
    /// [`Grid::remember`], the session's when the request has none of its
    /// own (a fresh visit), and saved when it does.
    fn effective_params(&self, request: &GridRequest) -> Vec<(String, String)> {
        let own = self.own_params(&request.params);
        if !self.remember {
            return own;
        }
        let key = format!("_grid_q.{}", self.id);
        let Some(session) = &request.session else {
            return own;
        };
        if own.iter().any(|(k, _)| k == "state") {
            let kept: Vec<(String, String)> = own
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "state" | "export" | "page"))
                .cloned()
                .collect();
            let _ = session.put(&key, &kept);
            own
        } else {
            session.get::<Vec<(String, String)>>(&key).unwrap_or(own)
        }
    }

    /// An advanced filter in the toolbar: rules on any filterable column
    /// ("Total is greater than 1,000,000", "Name doesn't contain coffee",
    /// "Ordered before 2026-03-01", "Email is empty"), all or any of them
    /// holding. They travel in the query string (`match=any&r.0.c=total&
    /// r.0.o=gt&r.0.v=1000000`), next to the headings' filters.
    pub fn advanced_filter(mut self) -> Self {
        self.advanced = true;
        self
    }

    /// On phones (under 768 px) each row is a card: the columns picked for
    /// small screens stacked as label and value, with sorting and filters
    /// in the toolbar. Wider screens keep the table.
    pub fn cards_on_mobile(mut self) -> Self {
        self.cards = true;
        self
    }

    /// Columns the user may group rows by (a "Group" choice in the
    /// toolbar): each group starts with a heading (its value and how many
    /// rows it has, folding its rows away on a click) and ends with its own
    /// summaries. Rows are sorted by the group first.
    pub fn groups(mut self, keys: &[&str]) -> Self {
        self.groups = keys.iter().map(|k| (*k).to_owned()).collect();
        self
    }

    /// Groups rows by `key` until the user picks otherwise (it joins
    /// [`Grid::groups`]).
    pub fn group_by(mut self, key: &str) -> Self {
        if !self.groups.iter().any(|g| g == key) {
            self.groups.insert(0, key.to_owned());
        }
        self.group = Some(key.to_owned());
        self
    }

    /// An action on the selected rows: a checkbox starts each row, and
    /// selecting some shows the actions over the grid. The action is sent
    /// to its URL (with the grid's query string, for "all matching") as
    /// [`Selection`]; the grid reloads after a 2xx.
    pub fn bulk_action(mut self, action: Action) -> Self {
        self.bulk.push(action);
        self
    }

    /// An action in each row's menu (`⋯`); `{id}` in its URL is the row's id.
    pub fn row_action(mut self, action: Action) -> Self {
        self.row_actions.push(action);
        self
    }

    /// `query` narrowed to what a bulk action was sent for: the selected
    /// ids, or every row the grid's filters match when the user chose
    /// "all matching".
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::grid::{Action, Column, Grid, GridRequest, Selection};
    /// # #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, status: String }
    /// fn orders() -> Grid {
    ///     Grid::new("orders")
    ///         .column(Column::text("status", "Status"))
    ///         .bulk_action(Action::new("Mark paid", "/orders/paid"))
    /// }
    ///
    /// async fn mark_paid(State(db): State<Db>, request: GridRequest, Form(selection): Form<Selection>) -> Result<String> {
    ///     let changed = orders()
    ///         .selected(Order::query(), &request, &selection)?
    ///         .update(&db, &[("status", &"paid")])
    ///         .await?;
    ///     Ok(format!("{changed} orders paid"))
    /// }
    /// ```
    pub fn selected<M: Model>(
        &self,
        query: Query<M>,
        request: &GridRequest,
        selection: &Selection,
    ) -> Result<Query<M>> {
        if selection.all {
            return Ok(self.filter(query, request));
        }
        let mut keys = Vec::with_capacity(selection.ids.len());
        for id in selection.ids.iter().take(10_000) {
            let key: M::Key = id
                .parse()
                .map_err(|_| Error::BadRequest(format!("`{id}` is not an id")))?;
            keys.push(key);
        }
        Ok(query.where_in("id", keys))
    }

    /// Prefixes the grid's query string names (`orders.page=2`,
    /// `orders.q.number=…`), for pages with more than one grid. Without
    /// it the names are plain (`page=2`). Either way a grid keeps the query
    /// string values that aren't its own.
    pub fn prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_owned());
        self
    }

    /// A click on a row (outside its buttons and links) opens this URL,
    /// `{id}` replaced by the row's id; Ctrl/Cmd-click opens it in a new tab.
    /// With row details ([`Grid::audit`], [`Grid::details`]) a click opens
    /// those instead and the row tools get a link.
    pub fn row_url(mut self, url: &str) -> Self {
        self.row_url = Some(url.to_owned());
        self
    }

    /// What an empty grid (before any filter) says; the page's call block
    /// can add buttons for `column.key == "_empty"`.
    pub fn empty_state(mut self, heading: &str, description: Option<&str>) -> Self {
        self.empty = Some((heading.to_owned(), description.map(str::to_owned)));
        self
    }

    /// Whether a query string name belongs to this grid, and its name
    /// without the prefix.
    fn own<'a>(&self, name: &'a str) -> Option<&'a str> {
        let name = match &self.prefix {
            Some(prefix) => name.strip_prefix(prefix.as_str())?.strip_prefix('.')?,
            None => name,
        };
        let known = matches!(
            name,
            "page" | "per_page" | "sort" | "search" | "export" | "group" | "match" | "state"
        ) || name.split_once('.').is_some_and(|(part, _)| {
            matches!(part, "q" | "m" | "min" | "max" | "from" | "to" | "in" | "r")
        });
        known.then_some(name)
    }

    /// The request's values for this grid, without the prefix.
    fn own_params(&self, params: &[(String, String)]) -> Vec<(String, String)> {
        params
            .iter()
            .filter_map(|(k, v)| self.own(k).map(|k| (k.to_owned(), v.clone())))
            .collect()
    }

    /// A name of this grid's, with the prefix.
    fn name(&self, name: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix}.{name}"),
            None => name.to_owned(),
        }
    }

    /// An export menu in the toolbar: CSV, Excel (with the `xlsx` feature)
    /// and a page to print or save as PDF, each of every row the filters
    /// match (not just the page), in the user's columns. The handler answers
    /// them with [`Grid::export`].
    pub fn exports(mut self) -> Self {
        self.exports = true;
        self
    }

    /// A click on a row (or its chevron) opens details under it: who
    /// created and last changed the row, and when, from its `created_by`,
    /// `created_at`, `updated_by` and `updated_at` values (those it has).
    pub fn audit(mut self) -> Self {
        self.audit = true;
        self
    }

    /// A click on a row opens details under it, drawn by the page's
    /// template: `caller(row, column)` with `column.key == "_details"`.
    /// With [`Grid::audit`], the audit fields come first.
    pub fn details(mut self) -> Self {
        self.details = true;
        self
    }

    /// Where edits are sent: `PATCH` to this URL with `{id}` replaced by the
    /// row's id, the changed fields as a form (`customer=…`, `tags=a&tags=b`,
    /// `paid=true`). Answer with a 2xx (a `Toast` shows), or with
    /// `Valid<T>`'s 422 and the errors show next to the fields; the grid
    /// then reloads its page.
    pub fn edit_url(mut self, url: &str) -> Self {
        self.edit_url = Some(url.to_owned());
        self
    }

    /// Rows can be dragged into order (or moved with the arrow keys on
    /// their handle) while the grid is sorted by `column` ascending; the
    /// new order of the page is `POST`ed to `url` as `ids` and `offset`
    /// (see [`RowOrder`]).
    pub fn reorder(mut self, column: &str, url: &str) -> Self {
        self.reorder = Some((column.to_owned(), url.to_owned()));
        self
    }

    /// The heading shown above the grid.
    pub fn title(mut self, title: &str) -> Self {
        self.title = Some(title.to_owned());
        self
    }

    /// Adds a column.
    pub fn column(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }

    /// Rows per page until the user picks another number (default 25).
    pub fn per_page(mut self, per_page: u32) -> Self {
        self.per_page = per_page.clamp(1, 500);
        if !self.per_page_options.contains(&self.per_page) {
            self.per_page_options.push(self.per_page);
            self.per_page_options.sort_unstable();
        }
        self
    }

    /// The order before the user sorts: a column key, `-` first for
    /// descending (`"-created_at"`), or several separated by commas
    /// (`"region,city,-total"`, what merged columns want). Ties are broken
    /// by `id`.
    pub fn sort_by(mut self, sort: &str) -> Self {
        self.sort = Some(sort.to_owned());
        self
    }

    /// The grid's id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The grid's columns, in their defined order.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    fn find(&self, key: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.key == key)
    }

    /// One page of `query` with the request's filters, sort and page
    /// applied, ready for the `grid` macro.
    pub async fn page<M: Model + Serialize>(
        &self,
        query: Query<M>,
        request: &GridRequest,
    ) -> Result<GridPage<M>> {
        if !valid_key(&self.id) {
            return Err(Error::Internal(anyhow::anyhow!(
                "grid id `{}` may only have letters, digits, `_` and `-`",
                self.id
            )));
        }
        let prefs = load_prefs(request, &self.id).await;
        let state = State_::parse(self, &self.effective_params(request));
        let query = self.filtered(query, &state, request);
        let unsorted = query.clone();
        let query = self.sorted(query, &state);
        let rows = query
            .paginate(&request.db, state.page, state.per_page)
            .await?;
        let related = self.related_values(&request.db, &rows.items).await?;
        let summaries = self.summarize(&unsorted, &request.db, None).await?;
        let group_summaries = match &state.group {
            Some(group) if self.find(group).is_some() => {
                self.summarize(&unsorted, &request.db, Some(group)).await?
            }
            _ => BTreeMap::new(),
        };
        let keep = request
            .params
            .iter()
            .filter(|(k, _)| self.own(k).is_none())
            .cloned()
            .collect();
        Ok(GridPage {
            grid: self.clone(),
            prefs,
            state,
            path: request.path.clone(),
            keep,
            rows,
            extra: Vec::new(),
            related,
            summaries,
            group_summaries,
            money_decimals: request.money_decimals,
        })
    }

    /// The summaries of every row `query` matches: per group value (as
    /// text) when `group` is set, else under the key `""`. Each value maps
    /// column keys to `{sum, average, min, max, count}`; groups also get
    /// `_rows`.
    async fn summarize<M: Model>(
        &self,
        query: &Query<M>,
        db: &Db,
        group: Option<&String>,
    ) -> Result<BTreeMap<String, Map<String, Value>>> {
        let mut out: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
        // The SQL a column reads: its expression or the model's column.
        let sql_of = |key: &str| -> Option<String> {
            match self.find(key)?.target::<M>() {
                Target::Expr(expr) => Some(expr),
                Target::Column(key) if M::COLUMNS.contains(&key.as_str()) => {
                    Some(format!("\"{key}\""))
                }
                Target::Column(_) => None,
            }
        };
        let group_expr = match group {
            Some(g) => match sql_of(g) {
                Some(expr) => Some(expr),
                None => return Ok(out),
            },
            None => None,
        };
        let group_sql = group_expr.as_ref().map(|e| format!("CAST({e} AS TEXT)"));
        if let (Some(group_sql), Some(expr)) = (&group_sql, &group_expr) {
            let counts: Vec<(Option<String>, i64)> = query
                .clone()
                .group_by_raw(expr)
                .select_as(db, &format!("{group_sql}, COUNT(*)"))
                .await?;
            for (value, count) in counts {
                out.entry(value.unwrap_or_default())
                    .or_default()
                    .insert("_rows".into(), count.into());
            }
        }
        for column in self.columns.iter().filter(|c| !c.summaries.is_empty()) {
            let Some(sql) = sql_of(&column.key) else {
                continue;
            };
            let key = &column.key;
            let numeric = matches!(column.kind, Kind::Number | Kind::Money);
            let figures = if numeric {
                format!(
                    "CAST(SUM({sql}) AS DOUBLE PRECISION), CAST(AVG({sql}) AS DOUBLE PRECISION), \
                     CAST(MIN({sql}) AS DOUBLE PRECISION), CAST(MAX({sql}) AS DOUBLE PRECISION), COUNT({sql})"
                )
            } else {
                format!("NULL, NULL, NULL, NULL, COUNT({sql})")
            };
            type Figures = (Option<f64>, Option<f64>, Option<f64>, Option<f64>, i64);
            let rows: Vec<(String, Figures)> = match (&group_sql, &group_expr) {
                (Some(group_sql), Some(group)) => query
                    .clone()
                    .group_by_raw(group)
                    .select_as::<(
                        Option<String>,
                        Option<f64>,
                        Option<f64>,
                        Option<f64>,
                        Option<f64>,
                        i64,
                    ), _>(db, &format!("{group_sql}, {figures}"))
                    .await?
                    .into_iter()
                    .map(|(g, a, b, c, d, e)| (g.unwrap_or_default(), (a, b, c, d, e)))
                    .collect(),
                _ => query
                    .clone()
                    .select_as::<Figures, _>(db, &figures)
                    .await?
                    .into_iter()
                    .map(|f| (String::new(), f))
                    .collect(),
            };
            for (value, (sum, average, min, max, count)) in rows {
                out.entry(value).or_default().insert(
                    key.clone(),
                    json!({ "sum": sum, "average": average, "min": min, "max": max, "count": count }),
                );
            }
        }
        Ok(out)
    }

    /// The related columns' values for `items`, one query per column.
    async fn related_values<M: Model + Serialize>(
        &self,
        db: &Db,
        items: &[M],
    ) -> Result<Vec<Map<String, Value>>> {
        let mut out = vec![Map::new(); items.len()];
        let columns: Vec<&Column> = self.columns.iter().filter(|c| c.expr.is_some()).collect();
        if columns.is_empty() || items.is_empty() {
            return Ok(out);
        }
        let ids: Vec<String> = items
            .iter()
            .map(|item| {
                match serde_json::to_value(item)
                    .ok()
                    .and_then(|v| v.get("id").cloned())
                {
                    Some(Value::String(id)) => id,
                    Some(other) => other.to_string(),
                    None => String::new(),
                }
            })
            .collect();
        let keys: Vec<M::Key> = ids.iter().filter_map(|id| id.parse().ok()).collect();
        // Unscoped: the ids come from the rows the caller's own query returned,
        // so a default scope (none outside a tenant context) mustn't drop them.
        for column in columns {
            let Target::Expr(expr) = column.target::<M>() else {
                continue;
            };
            let numeric = matches!(column.kind, Kind::Number | Kind::Money);
            let values: BTreeMap<String, Value> = if numeric {
                M::unscoped()
                    .where_in("id", keys.clone())
                    .select_as::<(String, Option<f64>), _>(
                        db,
                        &format!("CAST(\"id\" AS TEXT), CAST({expr} AS DOUBLE PRECISION)"),
                    )
                    .await?
                    .into_iter()
                    .map(|(id, v)| {
                        let v = match v {
                            Some(f) if f.fract() == 0.0 && f.abs() < 9e15 => Value::from(f as i64),
                            Some(f) => Value::from(f),
                            None => Value::Null,
                        };
                        (id, v)
                    })
                    .collect()
            } else {
                M::unscoped()
                    .where_in("id", keys.clone())
                    .select_as::<(String, Option<String>), _>(
                        db,
                        &format!("CAST(\"id\" AS TEXT), CAST({expr} AS TEXT)"),
                    )
                    .await?
                    .into_iter()
                    .map(|(id, v)| (id, v.map_or(Value::Null, Value::from)))
                    .collect()
            };
            for (i, id) in ids.iter().enumerate() {
                out[i].insert(
                    column.key.clone(),
                    values.get(id).cloned().unwrap_or(Value::Null),
                );
            }
        }
        Ok(out)
    }

    /// `query` with the request's filters applied (also used by `page`):
    /// for exports or totals over every filtered row.
    pub fn filter<M: Model>(&self, query: Query<M>, request: &GridRequest) -> Query<M> {
        let state = State_::parse(self, &self.effective_params(request));
        let query = self.filtered(query, &state, request);
        self.sorted(query, &state)
    }

    fn filtered<M: Model>(
        &self,
        mut query: Query<M>,
        state: &State_,
        request: &GridRequest,
    ) -> Query<M> {
        let zone = &request.zone;
        let money = 10f64.powi(request.money_decimals as i32);
        for (key, filter) in &state.filters {
            let Some(column) = self.find(key) else {
                continue;
            };
            query = filter.apply(query, column, zone, money);
        }
        if !state.rules.is_empty() {
            let mut parts = Vec::new();
            let mut binds: Vec<DbValue> = Vec::new();
            for rule in &state.rules {
                let Some(column) = self.find(&rule.column) else {
                    continue;
                };
                let target = match column.target::<M>() {
                    Target::Expr(expr) => expr,
                    Target::Column(key) if M::COLUMNS.contains(&key.as_str()) => {
                        format!("\"{key}\"")
                    }
                    Target::Column(_) => continue,
                };
                if let Some((sql, values)) = rule.sql(&target, column.kind, zone, money) {
                    parts.push(format!("({sql})"));
                    binds.extend(values);
                }
            }
            if !parts.is_empty() {
                let joined = parts.join(if state.any { " OR " } else { " AND " });
                query = query.where_raw(&joined, binds);
            }
        }
        if let Some(search) = &state.search {
            // A model with a full-text index (`#[model(search = …)]`) is
            // searched through it; the searchable columns it doesn't cover
            // (related values, other columns) with LIKE.
            let indexed = !M::SEARCHABLE.is_empty();
            let columns: Vec<String> = self
                .columns
                .iter()
                .filter(|c| c.searchable)
                .filter_map(|c| match c.target::<M>() {
                    Target::Expr(expr) => Some(expr),
                    Target::Column(key) if indexed && M::SEARCHABLE.contains(&key.as_str()) => None,
                    Target::Column(key) if M::COLUMNS.contains(&key.as_str()) => {
                        Some(format!("\"{key}\""))
                    }
                    Target::Column(_) => None,
                })
                .collect();
            if indexed && columns.is_empty() {
                query = query.where_search(search);
            } else if indexed || !columns.is_empty() {
                // Every word somewhere: in the index or a searchable column.
                for word in search.split_whitespace().take(8) {
                    let pattern = format!("%{}%", word.to_lowercase());
                    let sql = columns
                        .iter()
                        .map(|target| format!("LOWER(CAST({target} AS TEXT)) LIKE ?"))
                        .collect::<Vec<_>>()
                        .join(" OR ");
                    query = if indexed {
                        query.where_any(|q| {
                            q.where_search(word)
                                .where_raw(&sql, vec![pattern.clone(); columns.len()])
                        })
                    } else {
                        query.where_raw(&sql, vec![pattern; columns.len()])
                    };
                }
            }
        }
        query
    }

    fn sorted<M: Model>(&self, mut query: Query<M>, state: &State_) -> Query<M> {
        let mut keys: Vec<(String, bool)> = if state.defaulted {
            self.default_sort()
        } else {
            state.sort.iter().cloned().collect()
        };
        // Grouped rows come group by group.
        if let Some(group) = &state.group
            && keys.first().is_none_or(|(k, _)| k != group)
        {
            let desc = keys
                .iter()
                .find(|(k, _)| k == group)
                .is_some_and(|(_, d)| *d);
            keys.retain(|(k, _)| k != group);
            keys.insert(0, (group.clone(), desc));
        }
        // Searching a model with a full-text index, without a sort of the
        // user's: best matches first (inside the groups, when grouped).
        let mut relevance = match &state.search {
            Some(search) if state.defaulted && !M::SEARCHABLE.is_empty() => Some(search),
            _ => None,
        };
        if state.group.is_none()
            && let Some(search) = relevance.take()
        {
            query = query.order_by_relevance(search);
        }
        for (key, desc) in &keys {
            let dir = if *desc { "DESC" } else { "ASC" };
            // Empty values last either way, on both databases (PostgreSQL
            // puts NULLs first when descending, SQLite last).
            query = match self.find(key).map(Column::target::<M>) {
                Some(Target::Expr(expr)) => {
                    query.order_by_raw(&format!("({expr} IS NULL), {expr} {dir}"))
                }
                _ if M::COLUMNS.contains(&key.as_str()) => {
                    query.order_by_raw(&format!("(\"{key}\" IS NULL), \"{key}\" {dir}"))
                }
                _ if *desc => query.order_by_desc(key),
                _ => query.order_by(key),
            };
            // After the group's key.
            if let Some(search) = relevance.take() {
                query = query.order_by_relevance(search);
            }
        }
        if keys.iter().any(|(key, _)| key == "id") {
            query
        } else {
            query.order_by("id")
        }
    }

    /// `sort_by`'s keys: `"region,city,-total"`.
    fn default_sort(&self) -> Vec<(String, bool)> {
        self.sort
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(|k| match k.strip_prefix('-') {
                Some(key) => (key.to_owned(), true),
                None => (k.to_owned(), false),
            })
            .collect()
    }

    /// The columns in the user's order: frozen-left ones first, then the
    /// others, then frozen-right ones, each with where it's pinned.
    fn ordered(&self, prefs: &GridPrefs) -> Vec<(&Column, Option<Pin>)> {
        let mut columns: Vec<&Column> = self.columns.iter().collect();
        if let Some(order) = &prefs.order {
            columns.sort_by_key(|c| order.iter().position(|k| *k == c.key).unwrap_or(usize::MAX));
        }
        let pin = |c: &Column| match (&prefs.left, &prefs.right) {
            (None, None) => c.pin,
            (left, right) => {
                if left.as_ref().is_some_and(|l| l.contains(&c.key)) {
                    Some(Pin::Left)
                } else if right.as_ref().is_some_and(|r| r.contains(&c.key)) {
                    Some(Pin::Right)
                } else {
                    None
                }
            }
        };
        let mut out: Vec<(&Column, Option<Pin>)> = Vec::new();
        for side in [Some(Pin::Left), None, Some(Pin::Right)] {
            out.extend(
                columns
                    .iter()
                    .filter(|c| pin(c) == side)
                    .map(|c| (*c, side)),
            );
        }
        out
    }

    fn default_visible(&self, compact: bool) -> Vec<String> {
        if compact {
            let flagged: Vec<String> = self
                .columns
                .iter()
                .filter(|c| c.mobile)
                .map(|c| c.key.clone())
                .collect();
            if !flagged.is_empty() {
                return flagged;
            }
            return self
                .columns
                .iter()
                .filter(|c| !c.hidden)
                .take(3)
                .map(|c| c.key.clone())
                .collect();
        }
        self.columns
            .iter()
            .filter(|c| !c.hidden)
            .map(|c| c.key.clone())
            .collect()
    }
}

/// A table or column name: letters, digits and `_`.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Where a filter or sort looks: the model's column, or a related value.
enum Target {
    Column(String),
    Expr(String),
}

impl Target {
    fn compare<M: Model>(&self, query: Query<M>, op: &str, value: impl ToDbValue) -> Query<M> {
        match self {
            Target::Column(key) => query.where_op(key, op, value),
            Target::Expr(expr) => query.where_raw(&format!("{expr} {op} ?"), [value]),
        }
    }

    fn like<M: Model>(&self, query: Query<M>, pattern: String) -> Query<M> {
        match self {
            Target::Column(key) => query.where_like(key, pattern),
            Target::Expr(expr) => query.where_raw(
                &format!("LOWER(CAST({expr} AS TEXT)) LIKE ?"),
                [pattern.to_lowercase()],
            ),
        }
    }

    fn among<M: Model, V: ToDbValue>(&self, query: Query<M>, values: Vec<V>) -> Query<M> {
        match self {
            Target::Column(key) => query.where_in(key, values),
            Target::Expr(expr) => {
                let marks = vec!["?"; values.len()].join(", ");
                query.where_raw(&format!("{expr} IN ({marks})"), values)
            }
        }
    }
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What one column is filtered on, from the query string.
#[derive(Debug, Clone, Default, PartialEq)]
struct Filter {
    text: Option<String>,
    mode: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    any_of: Vec<String>,
}

impl Filter {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The `LIKE` pattern for a text filter.
    fn pattern(&self) -> Option<String> {
        let text = self.text.as_deref()?.trim();
        if text.is_empty() {
            return None;
        }
        if text.contains('%') {
            return Some(text.to_owned());
        }
        Some(match self.mode.as_deref() {
            Some("starts") => format!("{text}%"),
            Some("ends") => format!("%{text}"),
            Some("equals") => text.to_owned(),
            _ => format!("%{text}%"),
        })
    }

    fn apply<M: Model>(
        &self,
        mut query: Query<M>,
        column: &Column,
        zone: &crate::timezone::Zone,
        money: f64,
    ) -> Query<M> {
        let key = column.key.as_str();
        let target = column.target::<M>();
        match column.kind {
            Kind::Text | Kind::Color => {
                if let Some(pattern) = self.pattern() {
                    query = target.like(query, pattern);
                }
            }
            Kind::Number | Kind::Money => {
                for (value, op) in [(self.min, ">="), (self.max, "<=")] {
                    let Some(value) = value else { continue };
                    // Money is typed in whole units and stored in the smallest.
                    let value = if column.kind == Kind::Money {
                        smallest_unit(value, money)
                    } else {
                        value
                    };
                    query = if value.fract() == 0.0 && value.abs() < 9e15 {
                        target.compare(query, op, value as i64)
                    } else {
                        target.compare(query, op, value)
                    };
                }
            }
            Kind::Date => {
                if let Some(from) = self.from {
                    query = target.compare(query, ">=", from);
                }
                if let Some(to) = self
                    .to
                    .and_then(|d| d.checked_add_signed(Duration::days(1)))
                {
                    query = target.compare(query, "<", to);
                }
            }
            Kind::DateTime => {
                let at = |d: NaiveDate| day_start(zone, d);
                if let Some(from) = self.from.and_then(at) {
                    query = target.compare(query, ">=", from);
                }
                if let Some(to) = self
                    .to
                    .and_then(|d| d.checked_add_signed(Duration::days(1)))
                    .and_then(at)
                {
                    query = target.compare(query, "<", to);
                }
            }
            Kind::Bool => {
                let picked: Vec<bool> = self
                    .any_of
                    .iter()
                    .filter_map(|v| match v.as_str() {
                        "1" | "true" => Some(true),
                        "0" | "false" => Some(false),
                        _ => None,
                    })
                    .collect();
                if !picked.is_empty() {
                    query = target.among(query, picked);
                }
            }
            Kind::Select => {
                let picked: Vec<&String> = self
                    .any_of
                    .iter()
                    .filter(|v| column.options.iter().any(|(o, _)| o == *v))
                    .collect();
                if !picked.is_empty() {
                    let values: Vec<String> = picked.into_iter().cloned().collect();
                    // Options that are numbers belong to an integer column,
                    // which PostgreSQL won't compare with text: compare the
                    // column's text instead.
                    let numeric = column.options.iter().all(|(o, _)| o.parse::<i64>().is_ok());
                    query = match (&target, numeric) {
                        (Target::Column(key), true) if M::COLUMNS.contains(&key.as_str()) => {
                            let marks = vec!["?"; values.len()].join(", ");
                            query
                                .where_raw(&format!("CAST(\"{key}\" AS TEXT) IN ({marks})"), values)
                        }
                        (Target::Expr(expr), true) => {
                            let marks = vec!["?"; values.len()].join(", ");
                            query.where_raw(&format!("CAST({expr} AS TEXT) IN ({marks})"), values)
                        }
                        _ => target.among(query, values),
                    };
                }
            }
            Kind::Tags => {
                let picked: Vec<String> = self
                    .any_of
                    .iter()
                    .filter(|v| column.options.iter().any(|(o, _)| o == *v))
                    .map(|v| format!("%{}%", serde_json::to_string(v).unwrap_or_default()))
                    .collect();
                if !picked.is_empty() && M::COLUMNS.contains(&key) {
                    // The JSON text holds `"value"` for each picked one.
                    let sql =
                        vec![format!("CAST(\"{key}\" AS TEXT) LIKE ?"); picked.len()].join(" OR ");
                    query = query.where_raw(&sql, picked);
                }
            }
            Kind::Custom | Kind::Image => {}
        }
        query
    }

    fn to_value(&self) -> Value {
        json!({
            "text": self.text,
            "mode": self.mode.clone().unwrap_or_else(|| "contains".into()),
            "min": self.min,
            "max": self.max,
            "from": self.from.map(|d| d.to_string()),
            "to": self.to.map(|d| d.to_string()),
            "any_of": self.any_of,
        })
    }
}

/// The request's filters, sort and page, checked against the grid.
#[derive(Debug, Clone)]
struct State_ {
    filters: BTreeMap<String, Filter>,
    /// The toolbar's search.
    search: Option<String>,
    /// The advanced filter's rules, and whether any (not all) must hold.
    rules: Vec<Rule>,
    any: bool,
    /// The column rows are grouped by.
    group: Option<String>,
    /// The grid's own default group (the query string says when it differs).
    grid_group: Option<String>,
    sort: Option<(String, bool)>,
    /// `sort` is the grid's default, not in the query string.
    defaulted: bool,
    page: u32,
    per_page: u32,
}

impl State_ {
    fn parse(grid: &Grid, params: &[(String, String)]) -> Self {
        let mut filters: BTreeMap<String, Filter> = BTreeMap::new();
        let mut sort = None;
        let mut search = None;
        let mut group = grid.group.clone();
        let mut rules: BTreeMap<u32, Rule> = BTreeMap::new();
        let mut any = false;
        let mut page = 1;
        let mut per_page = grid.per_page;
        for (name, value) in params {
            let value = value.trim();
            match name.as_str() {
                "page" => page = value.parse().unwrap_or(1).max(1),
                "group" => {
                    group = grid.groups.iter().find(|g| *g == value).cloned();
                }
                "match" => any = value == "any",
                _ if name.starts_with("r.") => {
                    // r.<n>.c (column), r.<n>.o (operator), r.<n>.v (value).
                    let mut parts = name.splitn(3, '.').skip(1);
                    let (Some(n), Some(field)) = (parts.next(), parts.next()) else {
                        continue;
                    };
                    let Ok(n) = n.parse::<u32>() else { continue };
                    if n >= 20 {
                        continue;
                    }
                    let rule = rules.entry(n).or_default();
                    match field {
                        "c" => rule.column = value.to_owned(),
                        "o" => rule.op = value.to_owned(),
                        "v" => rule.value = value.chars().take(200).collect(),
                        _ => {}
                    }
                }
                "search" if !value.is_empty() && grid.columns.iter().any(|c| c.searchable) => {
                    search = Some(value.chars().take(200).collect());
                }
                "per_page" => {
                    if let Ok(n) = value.parse::<u32>()
                        && grid.per_page_options.contains(&n)
                    {
                        per_page = n;
                    }
                }
                "sort" => {
                    let (key, desc) = match value.strip_prefix('-') {
                        Some(key) => (key, true),
                        None => (value, false),
                    };
                    if grid.find(key).is_some_and(|c| c.sortable) || key == "id" {
                        sort = Some((key.to_owned(), desc));
                    }
                }
                _ => {
                    let Some((part, key)) = name.split_once('.') else {
                        continue;
                    };
                    let Some(column) = grid.find(key).filter(|c| c.filterable) else {
                        continue;
                    };
                    if value.is_empty() {
                        continue;
                    }
                    let filter = filters.entry(column.key.clone()).or_default();
                    match part {
                        "q" => filter.text = Some(value.chars().take(200).collect()),
                        "m" if ["contains", "starts", "ends", "equals"].contains(&value) => {
                            filter.mode = Some(value.to_owned())
                        }
                        "min" => filter.min = value.parse().ok().filter(|v: &f64| v.is_finite()),
                        "max" => filter.max = value.parse().ok().filter(|v: &f64| v.is_finite()),
                        "from" => filter.from = value.parse().ok(),
                        "to" => filter.to = value.parse().ok(),
                        "in" if filter.any_of.len() < 100 => filter.any_of.push(value.to_owned()),
                        _ => {}
                    }
                }
            }
        }
        // A mode alone filters nothing.
        filters.retain(|_, f| {
            !Filter {
                mode: None,
                ..f.clone()
            }
            .is_empty()
        });
        // Without a sort of the user's, the grid's (its first key marks the
        // heading).
        let defaulted = sort.is_none();
        if defaulted {
            sort = grid.default_sort().into_iter().next();
        }
        let rules: Vec<Rule> = rules
            .into_values()
            .filter(|r| {
                grid.find(&r.column)
                    .is_some_and(|c| c.filterable && ops_for(c.kind).contains(&r.op.as_str()))
                    && (!op_takes_value(&r.op) || !r.value.is_empty())
            })
            .collect();
        Self {
            filters,
            search,
            rules,
            any,
            grid_group: grid.group.clone(),
            group,
            sort,
            defaulted,
            page,
            per_page,
        }
    }

    /// The query string for this state, without `page` (links set it).
    fn query_string(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Some(search) = &self.search {
            out.push(("search".into(), search.clone()));
        }
        if !self.rules.is_empty() {
            out.push(("match".into(), if self.any { "any" } else { "all" }.into()));
            for (n, rule) in self.rules.iter().enumerate() {
                out.push((format!("r.{n}.c"), rule.column.clone()));
                out.push((format!("r.{n}.o"), rule.op.clone()));
                if op_takes_value(&rule.op) {
                    out.push((format!("r.{n}.v"), rule.value.clone()));
                }
            }
        }
        if self.group != self.grid_group {
            out.push(("group".into(), self.group.clone().unwrap_or_default()));
        }
        for (key, f) in &self.filters {
            if let Some(text) = &f.text {
                out.push((format!("q.{key}"), text.clone()));
            }
            if let Some(mode) = &f.mode {
                out.push((format!("m.{key}"), mode.clone()));
            }
            for (name, value) in [("min", f.min), ("max", f.max)] {
                if let Some(v) = value {
                    out.push((format!("{name}.{key}"), v.to_string()));
                }
            }
            for (name, value) in [("from", f.from), ("to", f.to)] {
                if let Some(v) = value {
                    out.push((format!("{name}.{key}"), v.to_string()));
                }
            }
            for v in &f.any_of {
                out.push((format!("in.{key}"), v.clone()));
            }
        }
        if let Some((key, desc)) = self.sort.as_ref().filter(|_| !self.defaulted) {
            out.push((
                "sort".into(),
                format!("{}{key}", if *desc { "-" } else { "" }),
            ));
        }
        out.push(("per_page".into(), self.per_page.to_string()));
        out
    }
}

/// An amount typed in whole units (`40.5`) in the smallest unit (`4050`
/// for a currency with 2 decimals), rounded so `40.1 * 100` stays `4010`.
fn smallest_unit(units: f64, scale: f64) -> f64 {
    (units * scale * 1e6).round() / 1e6
}

/// The moment the day `day` starts in `zone`: date-time filters take whole
/// days of `APP_TIMEZONE`, the zone their cells are shown in. When a clock
/// change skips midnight, the day starts at its first wall-clock time.
fn day_start(
    zone: &crate::timezone::Zone,
    day: NaiveDate,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let midnight = day.and_hms_opt(0, 0, 0)?;
    let at = (0..=2).find_map(|h| zone.resolve(midnight + Duration::hours(h)))?;
    chrono::DateTime::from_timestamp(at, 0)
}

/// One rule of the advanced filter.
#[derive(Debug, Clone, Default, PartialEq)]
struct Rule {
    column: String,
    op: String,
    value: String,
}

/// The advanced filter's operators for a kind of column.
fn ops_for(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Text | Kind::Color | Kind::Tags => &[
            "contains",
            "not_contains",
            "equals",
            "not_equals",
            "starts",
            "ends",
            "empty",
            "not_empty",
        ],
        Kind::Number | Kind::Money => &["eq", "ne", "gt", "gte", "lt", "lte", "empty", "not_empty"],
        Kind::Date | Kind::DateTime => &["on", "before", "after", "empty", "not_empty"],
        Kind::Bool => &["is_true", "is_false"],
        Kind::Select => &["is", "is_not", "empty", "not_empty"],
        Kind::Custom | Kind::Image => &[],
    }
}

fn op_takes_value(op: &str) -> bool {
    !matches!(op, "empty" | "not_empty" | "is_true" | "is_false")
}

impl Rule {
    /// The rule as SQL on `target` (a quoted column or a related value),
    /// with its values; `None` when the value doesn't fit the column.
    fn sql(
        &self,
        target: &str,
        kind: Kind,
        zone: &crate::timezone::Zone,
        money: f64,
    ) -> Option<(String, Vec<DbValue>)> {
        let text = format!("LOWER(CAST({target} AS TEXT))");
        let lower = self.value.to_lowercase();
        let like = |pattern: String| (format!("{text} LIKE ?"), vec![pattern.to_db_value()]);
        Some(match self.op.as_str() {
            "empty" => (
                format!("{target} IS NULL OR CAST({target} AS TEXT) = ''"),
                vec![],
            ),
            "not_empty" => (
                format!("{target} IS NOT NULL AND CAST({target} AS TEXT) <> ''"),
                vec![],
            ),
            "is_true" => (format!("{target} = ?"), vec![true.to_db_value()]),
            "is_false" => (
                format!("{target} = ? OR {target} IS NULL"),
                vec![false.to_db_value()],
            ),
            "contains" => like(format!("%{lower}%")),
            "not_contains" => (
                format!("{target} IS NULL OR {text} NOT LIKE ?"),
                vec![format!("%{lower}%").to_db_value()],
            ),
            "equals" => like(lower),
            "not_equals" => (
                format!("{target} IS NULL OR {text} <> ?"),
                vec![lower.to_db_value()],
            ),
            "starts" => like(format!("{lower}%")),
            "ends" => like(format!("%{lower}")),
            "is" => (format!("{target} = ?"), vec![self.value.to_db_value()]),
            "is_not" => (
                format!("{target} IS NULL OR {target} <> ?"),
                vec![self.value.to_db_value()],
            ),
            "eq" | "ne" | "gt" | "gte" | "lt" | "lte" => {
                let n: f64 = self
                    .value
                    .trim()
                    .parse()
                    .ok()
                    .filter(|n: &f64| n.is_finite())?;
                let n = if kind == Kind::Money {
                    smallest_unit(n, money)
                } else {
                    n
                };
                let op = match self.op.as_str() {
                    "eq" => "=",
                    "ne" => "<>",
                    "gt" => ">",
                    "gte" => ">=",
                    "lt" => "<",
                    _ => "<=",
                };
                let value = if n.fract() == 0.0 && n.abs() < 9e15 {
                    (n as i64).to_db_value()
                } else {
                    n.to_db_value()
                };
                (format!("{target} {op} ?"), vec![value])
            }
            "on" | "before" | "after" => {
                let day: NaiveDate = self.value.trim().parse().ok()?;
                let next = day.checked_add_signed(Duration::days(1))?;
                let bound = |d: NaiveDate| -> DbValue {
                    if kind == Kind::DateTime {
                        day_start(zone, d).to_db_value()
                    } else {
                        d.to_db_value()
                    }
                };
                match self.op.as_str() {
                    "on" => (
                        format!("{target} >= ? AND {target} < ?"),
                        vec![bound(day), bound(next)],
                    ),
                    "before" => (format!("{target} < ?"), vec![bound(day)]),
                    _ => (format!("{target} >= ?"), vec![bound(next)]),
                }
            }
            _ => return None,
        })
    }
}

/// What a user chose in a grid's column menu. `None` means "the grid's
/// default".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GridPrefs {
    /// Column keys in the order they're shown.
    #[serde(default)]
    pub order: Option<Vec<String>>,
    /// Columns frozen at the left edge.
    #[serde(default)]
    pub left: Option<Vec<String>>,
    /// Columns frozen at the right edge.
    #[serde(default)]
    pub right: Option<Vec<String>>,
    /// Columns shown on small screens (under 768 px).
    #[serde(default)]
    pub compact: Option<Vec<String>>,
    /// Columns shown on wide screens.
    #[serde(default)]
    pub wide: Option<Vec<String>>,
    /// Widths the user gave columns, in CSS pixels (40 to 2000).
    #[serde(default)]
    pub widths: Option<BTreeMap<String, u32>>,
}

impl GridPrefs {
    fn is_valid(&self) -> bool {
        [
            &self.order,
            &self.left,
            &self.right,
            &self.compact,
            &self.wide,
        ]
        .into_iter()
        .flatten()
        .all(|keys| keys.len() <= 200 && keys.iter().all(|k| valid_key(k)))
            && self.widths.as_ref().is_none_or(|widths| {
                widths.len() <= 200
                    && widths
                        .iter()
                        .all(|(k, w)| valid_key(k) && (40..=2000).contains(w))
            })
    }
}

fn session_key(grid: &str) -> String {
    format!("_grid.{grid}")
}

async fn load_prefs(request: &GridRequest, grid: &str) -> GridPrefs {
    match request.user_id {
        Some(user_id) => {
            let data: Option<String> =
                crate::db::sql("SELECT data FROM grid_preferences WHERE user_id = ? AND grid = ?")
                    .bind(user_id)
                    .bind(grid)
                    .scalar_optional(&request.db)
                    .await
                    .ok()
                    .flatten();
            data.and_then(|d| serde_json::from_str(&d).ok())
                .unwrap_or_default()
        }
        None => request
            .session
            .as_ref()
            .and_then(|s| s.get(&session_key(grid)))
            .unwrap_or_default(),
    }
}

/// Saves a user's choices for `grid` (what the column menu does).
pub async fn save_prefs(db: &Db, user_id: i64, grid: &str, prefs: &GridPrefs) -> Result {
    let data = serde_json::to_string(prefs)?;
    crate::db::sql(
        "INSERT INTO grid_preferences (user_id, grid, data, updated_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (user_id, grid) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
    )
    .bind(user_id)
    .bind(grid)
    .bind(data)
    .bind(crate::db::now())
    .execute(db)
    .await?;
    Ok(())
}

/// The query string, the session and the user a grid needs, as an
/// extractor: `async fn index(request: GridRequest) -> Result<View>`.
#[derive(Clone)]
pub struct GridRequest {
    params: Vec<(String, String)>,
    path: String,
    db: Db,
    session: Option<Session>,
    user_id: Option<i64>,
    lang: Option<crate::Lang>,
    zone: crate::timezone::Zone,
    /// `APP_CURRENCY`'s usual decimals: money is stored in its smallest unit.
    money_decimals: u32,
}

impl GridRequest {
    /// A request for tests and commands: `params` as a query string would
    /// give them, for a guest.
    pub fn new(db: &Db, path: &str, params: &[(&str, &str)]) -> Self {
        Self {
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            path: path.to_owned(),
            db: db.clone(),
            session: None,
            user_id: None,
            lang: None,
            zone: crate::timezone::Zone::UTC,
            money_decimals: crate::view_filters::currency_decimals(
                &crate::Config::default().currency,
            ),
        }
    }

    /// The query string's value for `name`, if any.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

impl<S: Send + Sync> FromRequestParts<S> for GridRequest {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Response> {
        let Some(app) = parts.extensions.get::<AppState>().cloned() else {
            return Err(
                Error::Internal(anyhow::anyhow!("GridRequest needs Renox's middleware"))
                    .into_response(),
            );
        };
        let params: Vec<(String, String)> = parts
            .uri
            .query()
            .and_then(|q| serde_html_form::from_str(q).ok())
            .unwrap_or_default();
        let session = Session::from_request_parts(parts, state).await.ok();
        let user = <AuthUser as axum::extract::OptionalFromRequestParts<S>>::from_request_parts(
            parts, state,
        )
        .await
        .ok()
        .flatten();
        let lang = crate::Lang::from_request_parts(parts, state).await.ok();
        // The whole path: inside a `Routes::group` the router sees it
        // without the group's prefix, and the grid's links need it all.
        let path = parts
            .extensions
            .get::<axum::extract::OriginalUri>()
            .map_or_else(|| parts.uri.path(), |original| original.0.path())
            .to_owned();
        Ok(Self {
            params,
            path,
            db: app.db.clone(),
            session,
            user_id: user.map(|u| u.id),
            lang,
            zone: app.config.timezone,
            money_decimals: crate::view_filters::currency_decimals(&app.config.currency),
        })
    }
}

/// One page of a grid, as the `grid` macro of `renox/grid.html` draws it.
/// Pass it to the view as is; [`GridPage::extend`] adds values that aren't
/// columns of the model (a chart's points, a link).
pub struct GridPage<M> {
    grid: Grid,
    prefs: GridPrefs,
    state: State_,
    path: String,
    /// Query string values that aren't this grid's, kept in its links.
    keep: Vec<(String, String)>,
    rows: Paginated<M>,
    extra: Vec<Map<String, Value>>,
    /// The related columns' values, per row.
    related: Vec<Map<String, Value>>,
    summaries: BTreeMap<String, Map<String, Value>>,
    group_summaries: BTreeMap<String, Map<String, Value>>,
    /// `APP_CURRENCY`'s usual decimals, for money columns.
    money_decimals: u32,
}

impl<M: Serialize> GridPage<M> {
    /// Adds values to each row, from the row's model: `page.extend(|order|
    /// json!({ "trend": trends.get(&order.id) }))`. The values must be an
    /// object; its keys join the row's (the `custom` columns read them).
    pub fn extend(mut self, mut values: impl FnMut(&M) -> Value) -> Self {
        self.extra = self
            .rows
            .items
            .iter()
            .map(|item| match values(item) {
                Value::Object(map) => map,
                _ => Map::new(),
            })
            .collect();
        self
    }

    /// The models on this page.
    pub fn items(&self) -> &[M] {
        &self.rows.items
    }

    /// Rows matching the filters, on all pages.
    pub fn total(&self) -> u64 {
        self.rows.total
    }

    fn to_value(&self) -> Value {
        let ordered = self.grid.ordered(&self.prefs);
        let compact = self
            .prefs
            .compact
            .clone()
            .unwrap_or_else(|| self.grid.default_visible(true));
        let wide = self
            .prefs
            .wide
            .clone()
            .unwrap_or_else(|| self.grid.default_visible(false));
        let sort = self.state.sort.clone();
        // Rows can be dragged while sorted by the order column, ascending.
        let grouped = self
            .state
            .group
            .clone()
            .filter(|g| self.grid.find(g).is_some());
        let dragging = grouped.is_none()
            && matches!((&self.grid.reorder, &sort), (Some((column, _)), Some((key, false))) if column == key);
        let columns: Vec<Value> = ordered
            .iter()
            .map(|(c, pin)| {
                let filter = self.state.filters.get(&c.key);
                json!({
                    "key": c.key,
                    "label": c.label,
                    "kind": c.kind,
                    "pin": pin,
                    "sortable": c.sortable,
                    "filterable": c.filterable,
                    "decimals": match c.kind {
                        Kind::Money => Some(c.decimals.map_or(self.money_decimals, u32::from)),
                        _ => c.decimals.map(u32::from),
                    },
                    // Money cells are divided by this: stored in the smallest unit.
                    "scale": match c.kind {
                        Kind::Money => 10u64.pow(self.money_decimals),
                        _ => 1,
                    },
                    "width": c.width,
                    "numeric": matches!(c.kind, Kind::Number | Kind::Money),
                    "options": c.options.iter().map(|(v, l)| json!({"value": v, "label": l})).collect::<Vec<_>>(),
                    "labels": c.options.iter().map(|(v, l)| (v.clone(), Value::from(l.clone()))).collect::<Map<_, _>>(),
                    "filter": filter.map(Filter::to_value).unwrap_or_else(|| Filter::default().to_value()),
                    "filtered": filter.is_some(),
                    "sort": match &sort {
                        Some((key, desc)) if *key == c.key => Value::from(if *desc { "desc" } else { "asc" }),
                        _ => Value::Null,
                    },
                    "compact": compact.contains(&c.key),
                    "wide": wide.contains(&c.key),
                    "editable": c.editable && self.grid.edit_url.is_some(),
                    "merge": c.merge && !dragging && grouped.is_none(),
                    "summaries": c.summaries,
                    "badges": c.badges.as_ref().map(|b| b.iter().map(|(v, t)| (v.clone(), Value::from(t.clone()))).collect::<Map<_, _>>()),
                    "icons": c.icons,
                    "description": c.description,
                    "tooltip": c.tooltip,
                    "wrap": c.wrap,
                    "limit": c.limit,
                    "link": c.link,
                    "copyable": c.copyable,
                    "round": c.round,
                })
            })
            .collect();
        let rows: Vec<Value> = self
            .rows
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let mut row = match serde_json::to_value(item) {
                    Ok(Value::Object(map)) => map,
                    _ => Map::new(),
                };
                if let Some(related) = self.related.get(i) {
                    row.extend(related.clone());
                }
                if let Some(extra) = self.extra.get(i) {
                    row.extend(extra.clone());
                }
                Value::Object(row)
            })
            .collect();
        let merged: Vec<&str> = if dragging || grouped.is_some() {
            Vec::new()
        } else {
            ordered
                .iter()
                .filter(|(c, _)| c.merge)
                .map(|(c, _)| c.key.as_str())
                .collect()
        };
        let spans = merge_spans(&rows, &merged);
        // Where groups start and end on this page.
        let group_text = |row: &Value| -> Option<String> {
            let key = grouped.as_ref()?;
            Some(match row.get(key) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => String::new(),
                Some(Value::Bool(b)) => {
                    if *b {
                        "1".into()
                    } else {
                        "0".into()
                    }
                }
                Some(other) => other.to_string(),
            })
        };
        let texts: Vec<Option<String>> = rows.iter().map(group_text).collect();
        // The row each group's run on this page starts at.
        let mut run_start = vec![0; texts.len()];
        for i in 1..texts.len() {
            run_start[i] = if texts[i] == texts[i - 1] {
                run_start[i - 1]
            } else {
                i
            };
        }
        let group_figures = |text: &str| -> Option<&Map<String, Value>> {
            self.group_summaries.get(text).or_else(|| {
                // Booleans read back as `true`/`false` on PostgreSQL.
                let alt = match text {
                    "1" => "true",
                    "0" => "false",
                    _ => return None,
                };
                self.group_summaries.get(alt)
            })
        };
        let rows: Vec<Value> = rows
            .into_iter()
            .zip(spans)
            .enumerate()
            .map(|(i, (mut row, merge))| {
                let text = texts[i].clone();
                let starts = text.is_some() && (i == 0 || texts[i - 1] != text);
                let ends = text.is_some() && texts.get(i + 1).is_none_or(|next| *next != text);
                let group = text.as_ref().map(|t| {
                    json!({
                        "id": format!("g{}", run_start[i]),
                        "value": row.get(grouped.as_deref().unwrap_or_default()).cloned(),
                        "rows": group_figures(t).and_then(|f| f.get("_rows").cloned()),
                        "figures": group_figures(t),
                        "starts": starts,
                        "ends": ends,
                    })
                });
                let id = row.get("id").map(|id| match id {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
                if let Value::Object(map) = &mut row {
                    map.insert(
                        "_rx".into(),
                        json!({
                            "id": id,
                            "spans": merge.spans,
                            "covered": merge.covered,
                            "groups": merge.groups,
                            "edit": self.grid.edit_url.as_ref().zip(id.as_ref()).map(|(url, id)| url.replace("{id}", id)),
                            "href": self.grid.row_url.as_ref().zip(id.as_ref()).map(|(url, id)| url.replace("{id}", id)),
                            "actions": self.grid.row_actions.iter().map(|a| a.to_value(id.as_deref())).collect::<Vec<_>>(),
                            "group": group,
                        }),
                    );
                }
                row
            })
            .collect();
        let editable = self.grid.edit_url.is_some() && self.grid.columns.iter().any(|c| c.editable);
        // This grid's values (prefixed) after the others the page had.
        let own: Vec<(String, String)> = self
            .state
            .query_string()
            .into_iter()
            .map(|(k, v)| (self.grid.name(&k), v))
            .collect();
        let query: Vec<(String, String)> = self.keep.iter().cloned().chain(own).collect();
        let query = &query;
        let config = json!({
            "id": self.grid.id,
            "prefix": self.grid.prefix.as_ref().map(|p| format!("{p}.")).unwrap_or_default(),
            "poll": self.grid.poll,
            "prefs": format!("/_renox/grid/{}/prefs", self.grid.id),
            "order": ordered.iter().map(|(c, _)| c.key.clone()).collect::<Vec<_>>(),
            "left": ordered.iter().filter(|(_, p)| *p == Some(Pin::Left)).map(|(c, _)| c.key.clone()).collect::<Vec<_>>(),
            "right": ordered.iter().filter(|(_, p)| *p == Some(Pin::Right)).map(|(c, _)| c.key.clone()).collect::<Vec<_>>(),
            "compact": compact,
            "wide": wide,
            "widths": self.prefs.widths.clone().unwrap_or_default(),
            "defaults": {
                "compact": self.grid.default_visible(true),
                "wide": self.grid.default_visible(false),
            },
            "columns": ordered.iter().map(|(c, _)| (c.key.clone(), json!({
                "kind": c.kind,
                "label": c.label,
                "options": c.options,
                "editable": c.editable && self.grid.edit_url.is_some(),
            }))).collect::<Map<_, _>>(),
            "reorder": self.grid.reorder.as_ref().filter(|_| dragging).map(|(_, url)| url),
            "offset": u64::from(self.rows.page.saturating_sub(1)) * u64::from(self.rows.per_page),
        });
        json!({
            "id": self.grid.id,
            "title": self.grid.title,
            "path": self.path,
            "columns": columns,
            "header": header_rows(&ordered),
            "rows": rows,
            "default_per_page": self.grid.per_page,
            "page": {
                "page": self.rows.page,
                "per_page": self.rows.per_page,
                "total": self.rows.total,
                "last_page": self.rows.last_page,
                "from": self.rows.from,
                "to": self.rows.to,
                "has_prev": self.rows.has_prev,
                "has_next": self.rows.has_next,
                "pages": self.rows.pages,
            },
            "per_page_options": self.grid.per_page_options,
            "sort": sort.filter(|_| !self.state.defaulted).map(|(key, desc)| format!("{}{key}", if desc { "-" } else { "" })),
            "query": query,
            "filtered": self.state.filters.len() + usize::from(!self.state.rules.is_empty()),
            "config": config.to_string(),
            "audit": self.grid.audit,
            "details": self.grid.audit || self.grid.details,
            "custom_details": self.grid.details,
            "editable": editable,
            "reorder": self.grid.reorder.as_ref().map(|(column, _)| json!({
                "column": column,
                "active": dragging,
            })),
            "tools": self.grid.audit || self.grid.details || editable || self.grid.reorder.is_some()
                || !self.grid.bulk.is_empty() || !self.grid.row_actions.is_empty(),
            "bulk": self.grid.bulk.iter().map(|a| a.to_value(None)).collect::<Vec<_>>(),
            "row_actions": !self.grid.row_actions.is_empty(),
            "cards": self.grid.cards,
            "remember": self.grid.remember,
            "advanced": {
                "on": self.grid.advanced,
                "any": self.state.any,
                "rules": self.state.rules.iter().map(|r| json!({"column": r.column, "op": r.op, "value": r.value})).collect::<Vec<_>>(),
                "columns": ordered.iter().filter(|(c, _)| c.filterable && !ops_for(c.kind).is_empty()).map(|(c, _)| json!({
                    "key": c.key,
                    "label": c.label,
                    "kind": c.kind,
                    "ops": ops_for(c.kind),
                    "options": c.options.iter().map(|(v, l)| json!({"value": v, "label": l})).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            },
            "summary": self.summaries.get("").filter(|_| self.grid.columns.iter().any(|c| !c.summaries.is_empty())),
            "grouping": (!self.grid.groups.is_empty()).then(|| json!({
                "options": self.grid.groups.iter().filter_map(|g| self.grid.find(g)).map(|c| json!({"key": c.key, "label": c.label})).collect::<Vec<_>>(),
                "current": grouped,
                "column": grouped.as_ref().and_then(|g| ordered.iter().position(|(c, _)| c.key == *g)),
            })),
            "exports": self.grid.exports.then(|| export::urls(&self.path, query, &self.grid.name("per_page"), &self.grid.name("export"))),
            // Field names: `p` before each (`orders.` with a prefix).
            "p": self.grid.prefix.as_ref().map(|p| format!("{p}.")).unwrap_or_default(),
            "keep": self.keep,
            "search": {
                "on": self.grid.columns.iter().any(|c| c.searchable),
                "value": self.state.search,
            },
            "empty": self.grid.empty.as_ref().map(|(heading, description)| json!({
                "heading": heading,
                "description": description,
            })),
            "row_links": self.grid.row_url.is_some(),
        })
    }
}

impl<M: Serialize> Serialize for GridPage<M> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

/// A button that sends a request: in each row's menu ([`Grid::row_action`])
/// or over the selected rows ([`Grid::bulk_action`]).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Action {
    label: String,
    url: String,
    method: String,
    confirm: Option<String>,
    danger: bool,
    link: bool,
    sheet: Option<String>,
}

impl Action {
    /// An action that `POST`s to `url` (`{id}` is the row's id in row
    /// actions).
    pub fn new(label: &str, url: &str) -> Self {
        Self {
            label: label.to_owned(),
            url: url.to_owned(),
            method: "POST".into(),
            confirm: None,
            danger: false,
            link: false,
            sheet: None,
        }
    }

    /// A link to `url` instead of a request (row actions: `Edit`, `Open`).
    pub fn link(label: &str, url: &str) -> Self {
        Self {
            link: true,
            ..Self::new(label, url)
        }
    }

    /// The request's method: `POST` (the default), `PATCH`, `PUT` or
    /// `DELETE`.
    pub fn method(mut self, method: &str) -> Self {
        self.method = method.to_ascii_uppercase();
        self
    }

    /// Asks first, in a dialog with this question.
    pub fn confirm(mut self, question: &str) -> Self {
        self.confirm = Some(question.to_owned());
        self
    }

    /// Shown in red, for actions that delete or can't be undone.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Opens the dialog `id` (a kit `action_sheet`, elsewhere on the page)
    /// instead of sending the request at once: the grid points the sheet's
    /// form at this action's `url`, adds the selection (`ids`, `all`) and
    /// shows it, so the action can ask for input first. Use it with
    /// `action_sheet(id, …, button=false)`.
    pub fn sheet(mut self, id: &str) -> Self {
        self.sheet = Some(id.to_owned());
        self
    }

    fn to_value(&self, id: Option<&str>) -> Value {
        json!({
            "sheet": self.sheet,
            "label": self.label,
            "url": id.map_or_else(|| self.url.clone(), |id| self.url.replace("{id}", id)),
            "method": self.method,
            "confirm": self.confirm,
            "danger": self.danger,
            "link": self.link,
        })
    }
}

/// What a bulk action was sent for (read with axum's `Form`): the selected
/// rows' `ids` (separated by commas), or `all=true` for every row the
/// grid's filters match. See [`Grid::selected`].
#[derive(Debug, Clone, Default, Deserialize)]
#[non_exhaustive]
pub struct Selection {
    /// The selected rows' ids.
    #[serde(default, deserialize_with = "comma_list")]
    pub ids: Vec<String>,
    /// Every row the filters match, not just the selected ones.
    #[serde(default)]
    pub all: bool,
}

/// Where a row sits in the merged columns' groups.
#[derive(Debug, Default, Clone, PartialEq)]
struct MergeInfo {
    /// For the rows starting a group: how many rows the cell spans.
    spans: Map<String, Value>,
    /// The merged columns whose cell an earlier row draws.
    covered: Vec<String>,
    /// The group of each merged column, e.g. `city-3` (its first row).
    groups: Map<String, Value>,
}

/// Groups neighbouring rows with equal values in the `merged` columns,
/// each nested in the groups of the columns before it.
fn merge_spans(rows: &[Value], merged: &[&str]) -> Vec<MergeInfo> {
    let mut info = vec![MergeInfo::default(); rows.len()];
    // Where each row's group starts, per column.
    let mut starts: Vec<Vec<usize>> = vec![vec![0; merged.len()]; rows.len()];
    for i in 0..rows.len() {
        let mut outer_break = i == 0;
        for (k, key) in merged.iter().enumerate() {
            let same = !outer_break && rows[i].get(*key) == rows[i - 1].get(*key);
            if same {
                starts[i][k] = starts[i - 1][k];
            } else {
                starts[i][k] = i;
                outer_break = true;
            }
        }
    }
    for (k, key) in merged.iter().enumerate() {
        for i in 0..rows.len() {
            let start = starts[i][k];
            info[i]
                .groups
                .insert((*key).to_owned(), format!("{key}-{start}").into());
            if start == i {
                let len = (i..rows.len()).take_while(|&j| starts[j][k] == i).count();
                info[i].spans.insert((*key).to_owned(), len.into());
            } else {
                info[i].covered.push((*key).to_owned());
            }
        }
    }
    info
}

/// The new order of a page of rows, as a reorderable grid sends it
/// (`ids=4,2,9`: the ids in their new order, `offset` the position of the
/// first), read with axum's `Form`:
///
/// ```
/// # use renox::prelude::*;
/// use renox::grid::RowOrder;
/// # #[derive(Model, serde::Serialize, Default)] struct Task { id: i64, position: i64 }
/// async fn reorder(State(db): State<Db>, Form(order): Form<RowOrder>) -> Result<StatusCode> {
///     order.save::<Task>(&db, "position").await?;
///     Ok(StatusCode::NO_CONTENT)
/// }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[non_exhaustive]
pub struct RowOrder {
    /// The rows' ids, in their new order (sent separated by commas).
    #[serde(default, deserialize_with = "comma_list")]
    pub ids: Vec<String>,
    /// The position of the first row (the rows before this page).
    #[serde(default)]
    pub offset: u64,
}

fn comma_list<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<String>, D::Error> {
    let text = String::deserialize(d)?;
    Ok(text
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect())
}

impl RowOrder {
    /// Writes `offset + n` into `column` of the n-th row, in one
    /// transaction. Ids that don't parse as the model's key are an error;
    /// `updated_at` is left alone.
    pub async fn save<M: Model>(&self, db: &Db, column: &str) -> Result<u64> {
        if !M::COLUMNS.contains(&column) || column == "id" {
            return Err(Error::Internal(anyhow::anyhow!(
                "`{}` has no column `{column}` to order by",
                M::TABLE
            )));
        }
        if self.ids.len() > 1000 {
            return Err(Error::BadRequest("too many rows".into()));
        }
        let mut keys = Vec::with_capacity(self.ids.len());
        for id in &self.ids {
            let key: M::Key = id
                .parse()
                .map_err(|_| Error::BadRequest(format!("`{id}` is not an id")))?;
            keys.push(key);
        }
        let sql = format!(
            "UPDATE \"{}\" SET \"{column}\" = ? WHERE \"id\" = ?",
            M::TABLE
        );
        let mut tx = db.begin().await?;
        let mut changed = 0;
        for (n, key) in keys.into_iter().enumerate() {
            let position = i64::try_from(self.offset + n as u64).unwrap_or(i64::MAX);
            changed += crate::db::sql(&sql)
                .bind(position)
                .bind(key)
                .execute(&mut tx)
                .await?;
        }
        tx.commit().await?;
        Ok(changed)
    }
}

/// The heading rows: grouped headings (`Column::under`) span the columns
/// under them, leaf headings span down to the last row. Groups don't cross
/// a frozen edge.
fn header_rows(columns: &[(&Column, Option<Pin>)]) -> Vec<Vec<Value>> {
    let depth = columns
        .iter()
        .map(|(c, _)| c.under.len() + 1)
        .max()
        .unwrap_or(1);
    let mut rows: Vec<Vec<Value>> = vec![Vec::new(); depth];
    for (level, row) in rows.iter_mut().enumerate() {
        let mut previous: Option<(Vec<String>, Option<Pin>)> = None;
        for (index, (column, pin)) in columns.iter().enumerate() {
            if level < column.under.len() {
                let path = column.under[..=level].to_vec();
                let same = previous
                    .as_ref()
                    .is_some_and(|(p, side)| *p == path && side == pin);
                if same && let Some(Value::Object(cell)) = row.last_mut() {
                    let span = cell["colspan"].as_u64().unwrap_or(1) + 1;
                    cell.insert("colspan".into(), span.into());
                    if let Some(Value::Array(keys)) = cell.get_mut("keys") {
                        keys.push(column.key.clone().into());
                    }
                } else {
                    row.push(json!({
                        "group": true,
                        "label": column.under[level],
                        "colspan": 1,
                        "rowspan": 1,
                        "keys": [column.key],
                        "pin": pin,
                    }));
                }
                previous = Some((path, *pin));
            } else {
                if level == column.under.len() {
                    row.push(json!({
                        "group": false,
                        "key": column.key,
                        "index": index,
                        "colspan": 1,
                        "rowspan": depth - level,
                        "keys": [column.key],
                        "pin": pin,
                    }));
                }
                previous = None;
            }
        }
    }
    rows
}

/// `POST /_renox/grid/{id}/prefs` (JSON [`GridPrefs`]) saves a user's
/// choices; `DELETE` forgets them.
pub(crate) fn router() -> Router<AppState> {
    Router::new().route(
        "/_renox/grid/{grid}/prefs",
        post(save_route).delete(reset_route),
    )
}

async fn save_route(
    State(state): State<AppState>,
    axum::extract::Path(grid): axum::extract::Path<String>,
    session: Session,
    user: Option<AuthUser>,
    axum::Json(prefs): axum::Json<GridPrefs>,
) -> Result<Response> {
    if !valid_key(&grid) || !prefs.is_valid() {
        return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
    }
    match user {
        Some(user) => save_prefs(&state.db, user.id, &grid, &prefs).await?,
        None => session.put(&session_key(&grid), &prefs)?,
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn reset_route(
    State(state): State<AppState>,
    axum::extract::Path(grid): axum::extract::Path<String>,
    session: Session,
    user: Option<AuthUser>,
) -> Result<Response> {
    match user {
        Some(user) => {
            crate::db::sql("DELETE FROM grid_preferences WHERE user_id = ? AND grid = ?")
                .bind(user.id)
                .bind(&grid)
                .execute(&state.db)
                .await?;
        }
        None => {
            session.remove(&session_key(&grid));
        }
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid::new("orders")
            .column(Column::text("number", "Order").frozen())
            .column(Column::text("name", "Name").under(["Customer"]))
            .column(Column::text("email", "Email").under(["Customer"]).hidden())
            .column(Column::money("total", "Total").under(["Amounts", "Sum"]))
            .column(Column::number("tax", "Tax").under(["Amounts", "Sum"]))
            .column(Column::select("status", "Status", [("new", "New"), ("paid", "Paid")]).mobile())
            .column(Column::custom("trend", "Trend"))
            .column(Column::custom("actions", "").frozen_right())
    }

    fn params(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn the_query_string_is_checked_against_the_grid() {
        let grid = grid().sort_by("-total");
        let state = State_::parse(
            &grid,
            &params(&[
                ("q.name", "coffee"),
                ("m.name", "starts"),
                ("min.total", "1000"),
                ("max.total", "x"),
                ("in.status", "paid"),
                ("q.trend", "custom columns don't filter"),
                ("q.missing", "nor unknown ones"),
                ("m.email", "starts"),
                ("page", "3"),
                ("per_page", "1000"),
                ("sort", "trend"),
            ]),
        );
        assert_eq!(state.filters.len(), 3, "{:?}", state.filters);
        assert_eq!(state.filters["name"].pattern().as_deref(), Some("coffee%"));
        assert_eq!(state.filters["total"].min, Some(1000.0));
        assert_eq!(state.filters["total"].max, None);
        assert_eq!(state.page, 3);
        assert_eq!(state.per_page, 25, "only the offered sizes");
        assert_eq!(state.sort, Some(("total".into(), true)), "the default");

        let sorted = State_::parse(&grid, &params(&[("sort", "name"), ("per_page", "50")]));
        assert_eq!(sorted.sort, Some(("name".into(), false)));
        assert_eq!(sorted.per_page, 50);
        assert!(
            sorted
                .query_string()
                .contains(&("sort".into(), "name".into()))
        );
    }

    #[test]
    fn text_filters_make_like_patterns() {
        let pattern = |text: &str, mode: Option<&str>| {
            Filter {
                text: Some(text.into()),
                mode: mode.map(Into::into),
                ..Default::default()
            }
            .pattern()
        };
        assert_eq!(pattern("coffee", None).as_deref(), Some("%coffee%"));
        assert_eq!(pattern("coffee", Some("ends")).as_deref(), Some("%coffee"));
        assert_eq!(pattern("coffee", Some("equals")).as_deref(), Some("coffee"));
        assert_eq!(
            pattern("ko%pi", Some("equals")).as_deref(),
            Some("ko%pi"),
            "% wins"
        );
        assert_eq!(pattern("  ", None), None);
    }

    #[test]
    fn headings_group_and_span() {
        let grid = grid();
        let ordered = grid.ordered(&GridPrefs::default());
        let keys: Vec<&str> = ordered.iter().map(|(c, _)| c.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "number", "name", "email", "total", "tax", "status", "trend", "actions"
            ]
        );
        let rows = header_rows(&ordered);
        assert_eq!(rows.len(), 3);
        let labels: Vec<(String, u64, u64)> = rows[0]
            .iter()
            .map(|c| {
                (
                    c["label"]
                        .as_str()
                        .or(c["key"].as_str())
                        .unwrap()
                        .to_owned(),
                    c["colspan"].as_u64().unwrap(),
                    c["rowspan"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            labels,
            [
                ("number".into(), 1, 3),
                ("Customer".into(), 2, 1),
                ("Amounts".into(), 2, 1),
                ("status".into(), 1, 3),
                ("trend".into(), 1, 3),
                ("actions".into(), 1, 3),
            ]
        );
        // Row 2: Customer's leaves span down; Amounts → Sum.
        let second: Vec<&str> = rows[1]
            .iter()
            .map(|c| c["label"].as_str().or(c["key"].as_str()).unwrap())
            .collect();
        assert_eq!(second, ["name", "email", "Sum"]);
        assert_eq!(rows[1][0]["rowspan"], 2);
        let third: Vec<&str> = rows[2].iter().map(|c| c["key"].as_str().unwrap()).collect();
        assert_eq!(third, ["total", "tax"]);
    }

    #[test]
    fn preferences_reorder_and_freeze() {
        let grid = grid();
        let prefs = GridPrefs {
            order: Some(vec!["status".into(), "total".into(), "number".into()]),
            left: Some(vec!["total".into()]),
            right: Some(vec![]),
            ..Default::default()
        };
        let ordered = grid.ordered(&prefs);
        let keys: Vec<(&str, Option<Pin>)> =
            ordered.iter().map(|(c, p)| (c.key.as_str(), *p)).collect();
        assert_eq!(keys[0], ("total", Some(Pin::Left)));
        assert_eq!(keys[1], ("status", None));
        assert_eq!(keys[2], ("number", None), "unfrozen by the user");
        assert!(keys.iter().all(|(_, p)| *p != Some(Pin::Right)));
        // A group split by the frozen edge is two groups.
        let rows = header_rows(&ordered);
        let amounts = rows[0].iter().filter(|c| c["label"] == "Amounts").count();
        assert_eq!(amounts, 2);
    }

    #[test]
    fn small_screens_start_with_a_few_columns() {
        let grid = grid();
        assert_eq!(grid.default_visible(true), ["status"]);
        assert!(!grid.default_visible(false).contains(&"email".to_string()));
        let plain = Grid::new("x")
            .column(Column::text("a", "A"))
            .column(Column::text("b", "B").hidden())
            .column(Column::text("c", "C"))
            .column(Column::text("d", "D"))
            .column(Column::text("e", "E"));
        assert_eq!(plain.default_visible(true), ["a", "c", "d"]);
    }

    #[test]
    fn merged_columns_nest() {
        let rows: Vec<Value> = [
            ("java", "Bandung"),
            ("java", "Bandung"),
            ("java", "Jakarta"),
            ("bali", "Jakarta"),
            ("bali", "Denpasar"),
            ("java", "Denpasar"),
        ]
        .iter()
        .map(|(r, c)| json!({ "region": r, "city": c }))
        .collect();
        let info = merge_spans(&rows, &["region", "city"]);
        let spans = |i: usize| {
            (
                info[i].spans.get("region").cloned(),
                info[i].spans.get("city").cloned(),
            )
        };
        assert_eq!(spans(0), (Some(json!(3)), Some(json!(2))));
        assert_eq!(info[1].covered, ["region", "city"]);
        assert_eq!(spans(2), (None, Some(json!(1))));
        assert_eq!(info[2].covered, ["region"]);
        // Jakarta again, but under another region: a new group.
        assert_eq!(spans(3), (Some(json!(2)), Some(json!(1))));
        assert_eq!(spans(4), (None, Some(json!(1))));
        assert_eq!(spans(5), (Some(json!(1)), Some(json!(1))));
        assert_eq!(info[1].groups["city"], "city-0");
        assert!(merge_spans(&rows, &[]).iter().all(|m| m.spans.is_empty()));
    }

    #[test]
    fn preferences_are_checked() {
        assert!(GridPrefs::default().is_valid());
        let widths = |w: u32| GridPrefs {
            widths: Some(BTreeMap::from([("total".to_owned(), w)])),
            ..Default::default()
        };
        assert!(widths(180).is_valid());
        assert!(!widths(10).is_valid() && !widths(5000).is_valid());
        let bad = GridPrefs {
            order: Some(vec!["a; DROP".into()]),
            ..Default::default()
        };
        assert!(!bad.is_valid());
        assert!(valid_key("orders-2026"));
        assert!(!valid_key(""));
    }
}

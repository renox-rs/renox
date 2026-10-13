//! What an app declares for each model in the panel: [`AdminResource`],
//! with its [`AdminAction`]s and [`Filter`]s.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use renox::db::{Model, Query};
use renox::grid::{Column, Grid};
use renox::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Entry, Field, RelationManager};

/// A boxed future that can be sent between threads, as an action returns.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A model in the admin panel (Filament's resources), declared once: the
/// grid's columns, the form's fields, its filters and actions, and who may
/// do what (its [`Policy`]).
///
/// ```
/// use renox::grid::Column;
/// use renox::prelude::*;
/// use renox_admin::{AdminResource, Field};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Model, Serialize, Default, Clone)]
/// struct Product { id: i64, name: String, price: i64, active: bool }
///
/// impl Policy for Product {
///     fn allows(&self, user: &User, ability: &str) -> bool {
///         // Everyone in the panel looks; only managers change things.
///         matches!(ability, "viewAny" | "view") || user.has_role("manager")
///     }
/// }
///
/// #[derive(Deserialize, Serialize, Validate)]
/// struct ProductForm {
///     #[validate(required, max = 100)]
///     name: String,
///     #[validate(required, min = 0)]
///     price: i64,
///     active: bool,
/// }
///
/// struct Products;
///
/// impl AdminResource for Products {
///     type Model = Product;
///     type Form = ProductForm;
///
///     fn label(&self) -> &str { "Product" }
///     fn plural_label(&self) -> &str { "Products" }
///
///     fn columns(&self) -> Vec<Column> {
///         vec![
///             Column::text("name", "Name").searchable(),
///             Column::money("price", "Price"),
///             Column::bool("active", "Active"),
///         ]
///     }
///
///     fn fields(&self) -> Vec<Field> {
///         vec![
///             Field::text("name", "Name").required(),
///             Field::money("price", "Price").required(),
///             Field::toggle("active", "Active").default_value(true),
///         ]
///     }
///
///     fn fill(&self, product: &mut Product, form: ProductForm) {
///         product.name = form.name;
///         product.price = form.price;
///         product.active = form.active;
///     }
/// }
/// ```
pub trait AdminResource: Send + Sync + 'static {
    /// The model the resource manages. `Default` gives the record a create
    /// page starts from, and the one list-level abilities are asked about.
    type Model: Model + Policy + Serialize + Default + Send + Sync;

    /// The form of the create and edit pages, checked with its own rules
    /// (`Valid<T>`). `Serialize` refills it after [`rules`](Self::rules)
    /// fails (passwords aren't refilled).
    type Form: DeserializeOwned + Validate + Serialize + Send + 'static;

    /// One record's name: "Product".
    fn label(&self) -> &str;

    /// Several records' name, in the navigation and the list's title:
    /// "Products".
    fn plural_label(&self) -> &str;

    /// What a record's view and edit pages call it: "Product #7" by
    /// default; its name reads better (`product.name.clone()`).
    fn record_title(&self, record: &Self::Model) -> String {
        format!("{} #{}", self.label(), record.id())
    }

    /// The resource's part of the address and of the route names
    /// (`/admin/products`, `admin.products.index`): letters, digits, `_`
    /// and `-`. The model's table by default.
    fn slug(&self) -> &str {
        Self::Model::TABLE
    }

    /// A heading the navigation puts the resource under ("Shop").
    fn navigation_group(&self) -> Option<&str> {
        None
    }

    /// The list's columns, left to right (`renox::grid`). Make some
    /// `.searchable()` for the list's search box.
    fn columns(&self) -> Vec<Column>;

    /// The create and edit pages' fields, in order.
    fn fields(&self) -> Vec<Field>;

    /// Copies a valid form into the record, before it is saved (a new
    /// record starts from `Default`).
    fn fill(&self, record: &mut Self::Model, form: Self::Form);

    /// Rules that need the record being edited (`None` while creating),
    /// checked after the form's own, e.g. a unique value that may stay the
    /// record's own: `v.field("sku", &form.sku).unique("products", "sku")`
    /// followed by `.ignore(record.id)` when there is one.
    fn rules(&self, form: &Self::Form, record: Option<&Self::Model>, v: &mut Validator) {
        let _ = (form, record, v);
    }

    /// The details of a record's view page. Without any, the resource has
    /// no view page and its rows open the edit page. By default, an entry
    /// per column ([`Entry::from_column`]).
    fn entries(&self) -> Vec<Entry> {
        self.columns()
            .iter()
            .filter_map(Entry::from_column)
            .collect()
    }

    /// Named filters over the list, shown as tabs above it ("Active",
    /// "Low stock"); the grid's columns filter on their own too.
    fn filters(&self) -> Vec<Filter<Self::Model>> {
        Vec::new()
    }

    /// The resource's own actions on selected rows (and, with
    /// [`AdminAction::row`], on one row).
    fn actions(&self) -> Vec<AdminAction<Self::Model>> {
        Vec::new()
    }

    /// Whether the panel offers a create page and a "New" button. Return
    /// `false` for a resource that is only edited (a settings row, a
    /// record other code creates): the create address answers 404.
    fn creatable(&self) -> bool {
        true
    }

    /// Whether records can be deleted from the panel (the delete buttons,
    /// the bulk delete, the trash and its restore). Return `false` for an
    /// edit-only resource: those addresses answer 404.
    fn deletable(&self) -> bool {
        true
    }

    /// Child rows managed from a record's own pages (Filament's relation
    /// managers): its variants, photos or tags, each a tab of the record
    /// with its own list. See [`RelationManager`].
    fn relations(&self) -> Vec<RelationManager> {
        Vec::new()
    }

    /// Runs after the create or edit page saved a record (Filament's
    /// `afterSave`), with who saved it and, on an edit, the record as it was
    /// ([`SaveContext::previous`]): audit an exact price change, refresh
    /// search keywords. An error answers the request with it, though the
    /// record is saved by then; do the work in a transaction of your own
    /// when it must go in with the record.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # use renox::grid::Column;
    /// # use renox_admin::{AdminResource, Field, SaveContext};
    /// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String, price: i64 }
    /// # impl Policy for Product { fn allows(&self, _: &User, _: &str) -> bool { true } }
    /// # #[derive(serde::Deserialize, serde::Serialize, Validate)] struct ProductForm { #[validate(required)] name: String }
    /// struct Products;
    ///
    /// impl AdminResource for Products {
    /// #   type Model = Product;
    /// #   type Form = ProductForm;
    /// #   fn label(&self) -> &str { "Product" }
    /// #   fn plural_label(&self) -> &str { "Products" }
    /// #   fn columns(&self) -> Vec<Column> { vec![] }
    /// #   fn fields(&self) -> Vec<Field> { vec![] }
    /// #   fn fill(&self, p: &mut Product, f: ProductForm) { p.name = f.name; }
    ///     // ...
    ///     async fn saved(&self, product: &Product, cx: &SaveContext) -> Result {
    ///         let before = cx.previous.as_ref().and_then(|old| old["price"].as_i64());
    ///         if before.is_some_and(|before| before != product.price) {
    ///             // Write the audit entry: who (`cx.user`), which product, from, to.
    ///             let _ = (&cx.state.db, &cx.user);
    ///         }
    ///         Ok(())
    ///     }
    /// }
    /// ```
    fn saved(&self, record: &Self::Model, cx: &SaveContext) -> impl Future<Output = Result> + Send {
        let _ = (record, cx);
        async { Ok(()) }
    }

    /// Every query the panel runs for this resource starts here: the
    /// model's query (with its default scope) by default. Narrow it to
    /// what the panel may ever show.
    fn query(&self) -> Query<Self::Model> {
        Self::Model::query()
    }

    /// Changes the list's grid after the panel built it from the columns,
    /// e.g. `grid.sort_by("name").per_page(50).groups(&["status"])`.
    fn grid(&self, grid: Grid) -> Grid {
        grid
    }

    /// Whether `user` may do `ability` (`viewAny`, `view`, `create`,
    /// `update`, `delete`, `deleteAny`, `restore`, `restoreAny`,
    /// `forceDelete`, `forceDeleteAny`, or an action's own) on `record`.
    /// By default, the model's [`Policy`] after `App::gate_before`; when
    /// there's no record (the list, the create page, what the list
    /// offers), the policy is asked about a blank record
    /// (`Self::Model::default()`).
    fn allows(&self, user: &AuthUser, ability: &str, record: Option<&Self::Model>) -> bool {
        match record {
            Some(record) => user.can(ability, record),
            None => user.can(ability, &Self::Model::default()),
        }
    }
}

/// What an action's code gets besides the records.
#[derive(Clone)]
#[non_exhaustive]
pub struct ActionContext {
    /// The app's state, e.g. its database (`state.db`).
    pub state: AppState,
    /// Who chose the action.
    pub user: AuthUser,
    /// What the user typed in the action's form ([`AdminAction::form`]);
    /// empty for an action without one.
    pub input: ActionInput,
}

/// What the user typed into an action's form, already checked against the
/// form's fields ([`AdminAction::form`]). Money fields are in the currency's
/// smallest unit, toggles are `true` or `false`, and a field left empty is
/// the empty text.
///
/// ```
/// # use renox_admin::ActionInput;
/// # fn demo(input: &ActionInput) {
/// let percent: f64 = input.parse("percent").unwrap_or(0.0);
/// let reason = input.text("reason");
/// let notify = input.bool("notify");
/// # let _ = (percent, reason, notify); }
/// ```
#[derive(Debug, Clone, Default)]
pub struct ActionInput {
    values: Vec<(String, String)>,
}

impl ActionInput {
    pub(crate) fn new(values: Vec<(String, String)>) -> Self {
        Self { values }
    }

    /// The value of field `name`, if the form had one.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The value of field `name`, or `""`.
    pub fn text(&self, name: &str) -> &str {
        self.get(name).unwrap_or("")
    }

    /// The value of field `name` read as a `T` (a number, a date…), or
    /// `None` when it is empty or doesn't read as one.
    pub fn parse<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.get(name)?.trim().parse().ok()
    }

    /// Whether the checkbox or toggle `name` was on.
    pub fn bool(&self, name: &str) -> bool {
        matches!(self.get(name), Some("true" | "on" | "1"))
    }

    /// Every field's name and value, in the form's order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// What a resource's [`saved`](AdminResource::saved) hook gets besides the
/// record.
#[derive(Clone)]
#[non_exhaustive]
pub struct SaveContext {
    /// The app's state, e.g. its database (`state.db`).
    pub state: AppState,
    /// Who saved the record.
    pub user: AuthUser,
    /// The record was just created (the create page), not edited.
    pub created: bool,
    /// On an edit, the record as it was before the form changed it (its
    /// fields as JSON); `None` for a new record.
    pub previous: Option<renox::serde_json::Value>,
}

type Run<M> = Arc<dyn Fn(Vec<M>, ActionContext) -> BoxFuture<'static, Result<Toast>> + Send + Sync>;

/// An action of a resource's list (Filament's bulk actions): a button over
/// the selected rows that runs `run` with their records and shows the
/// toast it returns. With [`row`](Self::row), each row's menu has it too.
///
/// ```
/// # use renox::prelude::*;
/// use renox_admin::AdminAction;
/// # #[derive(Model, serde::Serialize, Default, Clone)] struct Product { id: i64, active: bool }
///
/// let deactivate = AdminAction::new("deactivate", "Deactivate", |products: Vec<Product>, cx| async move {
///     let ids: Vec<i64> = products.iter().map(|p| p.id).collect();
///     let n = Product::query().where_in("id", ids).update(&cx.state.db, &[("active", &false)]).await?;
///     Ok(Toast::success(format!("{n} products deactivated.")))
/// })
/// .confirm("Deactivate the selected products?")
/// .row();
/// # let _ = deactivate;
/// ```
///
/// Every record must pass [`AdminResource::allows`] for the action's
/// ability (`update` by default), or nothing runs (403).
pub struct AdminAction<M> {
    key: String,
    label: String,
    ability: String,
    confirm: Option<String>,
    danger: bool,
    bulk: bool,
    row: bool,
    form: Vec<Field>,
    description: Option<String>,
    submit: Option<String>,
    run: Run<M>,
}

impl<M> Clone for AdminAction<M> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            label: self.label.clone(),
            ability: self.ability.clone(),
            confirm: self.confirm.clone(),
            danger: self.danger,
            bulk: self.bulk,
            row: self.row,
            form: self.form.clone(),
            description: self.description.clone(),
            submit: self.submit.clone(),
            run: self.run.clone(),
        }
    }
}

impl<M> fmt::Debug for AdminAction<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdminAction")
            .field("key", &self.key)
            .field("label", &self.label)
            .field("ability", &self.ability)
            .finish_non_exhaustive()
    }
}

impl<M: Send + 'static> AdminAction<M> {
    /// An action named `key` (letters, digits, `_` and `-`; part of its
    /// address), shown as `label`, that runs `run`.
    pub fn new<F, Fut>(key: &str, label: &str, run: F) -> Self
    where
        F: Fn(Vec<M>, ActionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Toast>> + Send + 'static,
    {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            ability: "update".into(),
            confirm: None,
            danger: false,
            bulk: true,
            row: false,
            form: Vec::new(),
            description: None,
            submit: None,
            run: Arc::new(move |records, cx| Box::pin(run(records, cx))),
        }
    }

    /// The ability each record must allow (`update` by default).
    pub fn ability(mut self, ability: &str) -> Self {
        self.ability = ability.to_owned();
        self
    }

    /// Asks first, in a dialog with this question.
    pub fn confirm(mut self, question: &str) -> Self {
        self.confirm = Some(question.to_owned());
        self
    }

    /// Shown in red, for actions that can't be undone.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Also in each row's menu, for that row alone.
    pub fn row(mut self) -> Self {
        self.row = true;
        self
    }

    /// Only in each row's menu, not over the selection.
    pub fn row_only(mut self) -> Self {
        self.row = true;
        self.bulk = false;
        self
    }

    /// Asks for input first: the button opens a sheet with these fields
    /// (any [`Field`] except `belongs_to`), and the action runs once they
    /// are valid, with what was typed in [`ActionContext::input`]. A field
    /// is checked as far as its declaration goes: `required`, numbers and
    /// dates that read as such, `min` and `max`, a select's choices.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox_admin::{AdminAction, Field};
    /// # #[derive(Model, serde::Serialize, Default, Clone)] struct Product { id: i64, price: i64 }
    ///
    /// let reprice = AdminAction::new("reprice", "Change price by %", |products: Vec<Product>, cx| async move {
    ///     let percent: f64 = cx.input.parse("percent").unwrap_or(0.0);
    ///     for product in &products {
    ///         let price = (product.price as f64 * (1.0 + percent / 100.0)).round() as i64;
    ///         Product::query()
    ///             .where_eq("id", product.id)
    ///             .update(&cx.state.db, &[("price", &price)])
    ///             .await?;
    ///     }
    ///     Ok(Toast::success(format!("{} prices changed by {percent} %.", products.len())))
    /// })
    /// .form(vec![Field::number("percent", "Percent").required().min(-90).max(500).suffix("%")])
    /// .description("Every selected product's price changes by this much.")
    /// .submit_label("Change prices")
    /// .row();
    /// # let _ = reprice;
    /// ```
    pub fn form(mut self, fields: Vec<Field>) -> Self {
        self.form = fields;
        self
    }

    /// A line under the title of the input sheet ([`form`](Self::form)).
    pub fn description(mut self, text: &str) -> Self {
        self.description = Some(text.to_owned());
        self
    }

    /// The input sheet's button, instead of the action's label.
    pub fn submit_label(mut self, label: &str) -> Self {
        self.submit = Some(label.to_owned());
        self
    }

    pub(crate) fn fields(&self) -> &[Field] {
        &self.form
    }

    pub(crate) fn describe(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub(crate) fn submit(&self) -> Option<&str> {
        self.submit.as_deref()
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn ability_name(&self) -> &str {
        &self.ability
    }

    pub(crate) fn question(&self) -> Option<&str> {
        self.confirm.as_deref()
    }

    pub(crate) fn is_danger(&self) -> bool {
        self.danger
    }

    pub(crate) fn on_bulk(&self) -> bool {
        self.bulk
    }

    pub(crate) fn on_row(&self) -> bool {
        self.row
    }

    pub(crate) fn run(
        &self,
        records: Vec<M>,
        cx: ActionContext,
    ) -> BoxFuture<'static, Result<Toast>> {
        (self.run)(records, cx)
    }
}

type Scope<M> = Arc<dyn Fn(Query<M>) -> Query<M> + Send + Sync>;

/// A named filter over a resource's list (Filament's tabs and preset
/// filters), chosen with `?filter=key`.
///
/// ```
/// # use renox::prelude::*;
/// use renox_admin::Filter;
/// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, active: bool, stock: i64 }
///
/// let filters = vec![
///     Filter::new("active", "Active", |q| q.where_eq("active", true)),
///     Filter::new("low", "Low stock", |q| q.where_op("stock", "<", 5)),
/// ];
/// # let _: Vec<Filter<Product>> = filters;
/// ```
pub struct Filter<M> {
    key: String,
    label: String,
    scope: Scope<M>,
}

impl<M> Clone for Filter<M> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            label: self.label.clone(),
            scope: self.scope.clone(),
        }
    }
}

impl<M> fmt::Debug for Filter<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filter")
            .field("key", &self.key)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl<M> Filter<M> {
    /// A filter named `key` (its value in `?filter=`), shown as `label`,
    /// that narrows the list's query with `scope`.
    pub fn new(
        key: &str,
        label: &str,
        scope: impl Fn(Query<M>) -> Query<M> + Send + Sync + 'static,
    ) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            scope: Arc::new(scope),
        }
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn apply(&self, query: Query<M>) -> Query<M> {
        (self.scope)(query)
    }
}

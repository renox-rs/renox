//! The panel's routes and pages: the dashboard, and per resource the list
//! (with its exports and actions), create, edit, view and delete.

use std::collections::BTreeMap;
use std::sync::Arc;

use renox::axum::Extension;
use renox::axum::extract::{FromRequest, Request};
use renox::db::{Model, Query};
use renox::grid::{Action, Grid, GridRequest, Selection};
use renox::prelude::*;
use renox::serde_json::{Map, Value};
use renox::validation::Errors;
use serde::Serialize;

use crate::field::{FieldKind, plain_name};
use crate::resource::{ActionInput, BoxFuture, SaveContext};
use crate::texts::{Texts, lower};
use crate::{AdminAction, AdminResource, Field};

/// The most records a bulk action loads.
const MAX_SELECTED: u64 = 10_000;

/// The most rows a `belongs_to` field offers.
const MAX_OPTIONS: u64 = 1_000;

/// The `?filter=` of soft-deleted rows.
const TRASHED: &str = "trashed";

/// Who may use the panel at all.
pub(crate) type Access = Arc<dyn Fn(&AuthUser) -> bool + Send + Sync>;

/// The panel as its handlers share it.
pub(crate) struct Panel {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) access: Option<Access>,
    pub(crate) resources: Vec<Arc<dyn Listed>>,
}

impl Panel {
    /// Lets `user` in, or answers 403. Nobody gets in until the app says
    /// who may (`Admin::authorize`).
    pub(crate) fn check(&self, user: &AuthUser) -> Result {
        match &self.access {
            Some(allows) if allows(user) => Ok(()),
            _ => Err(Error::Forbidden),
        }
    }

    /// What the layout needs: the title, the navigation and the account
    /// links.
    pub(crate) fn frame(
        &self,
        state: &AppState,
        user: &AuthUser,
        current: Option<&str>,
        texts: &Texts,
    ) -> Frame {
        let mut nav: Vec<NavGroup> = Vec::new();
        for resource in self.resources.iter().filter(|r| r.visible(user)) {
            let item = NavItem {
                label: texts.label(resource.slug(), resource.plural()),
                url: format!("{}/{}", self.path, resource.slug()),
                active: current == Some(resource.slug()),
            };
            let title = resource.group().map(|group| texts.label("nav", group));
            match nav.iter_mut().find(|group| group.title == title) {
                Some(group) => group.items.push(item),
                None => nav.push(NavGroup {
                    title,
                    items: vec![item],
                }),
            }
        }
        // Ungrouped resources first, then the groups in the order they appear.
        nav.sort_by_key(|group| group.title.is_some());
        let currency = state.config.currency.clone();
        Frame {
            title: self.title.clone(),
            home: self.path.clone(),
            dashboard: current.is_none(),
            nav,
            account_url: state.url("account.show", &[]).ok(),
            logout_url: state.url("logout", &[]).ok(),
            currency,
            text: texts.all(),
        }
    }
}

/// The layout's `admin` value.
#[derive(Serialize)]
pub(crate) struct Frame {
    title: String,
    home: String,
    /// The dashboard is the page shown.
    dashboard: bool,
    nav: Vec<NavGroup>,
    account_url: Option<String>,
    logout_url: Option<String>,
    /// `APP_CURRENCY`, before money fields.
    currency: String,
    /// The panel's own words in the request's language (`admin.text`).
    text: Value,
}

#[derive(Serialize)]
pub(crate) struct NavGroup {
    title: Option<String>,
    items: Vec<NavItem>,
}

#[derive(Serialize)]
pub(crate) struct NavItem {
    label: String,
    url: String,
    active: bool,
}

/// A resource as the panel lists it, whatever its model.
pub(crate) trait Listed: Send + Sync {
    fn slug(&self) -> &str;
    fn plural(&self) -> &str;
    fn group(&self) -> Option<&str>;
    /// The user may see the list (`viewAny`).
    fn visible(&self, user: &AuthUser) -> bool;
    /// How many records the list starts with.
    fn count(&self, db: Db) -> BoxFuture<'static, Result<u64>>;
    fn routes(&self, panel: Arc<Panel>) -> Routes;
}

/// A resource behind [`Listed`].
pub(crate) struct Holder<R>(pub(crate) Arc<R>);

impl<R: AdminResource> Listed for Holder<R> {
    fn slug(&self) -> &str {
        self.0.slug()
    }

    fn plural(&self) -> &str {
        self.0.plural_label()
    }

    fn group(&self) -> Option<&str> {
        self.0.navigation_group()
    }

    fn visible(&self, user: &AuthUser) -> bool {
        self.0.allows(user, "viewAny", None)
    }

    fn count(&self, db: Db) -> BoxFuture<'static, Result<u64>> {
        let query = self.0.query();
        Box::pin(async move { query.count(&db).await })
    }

    fn routes(&self, panel: Arc<Panel>) -> Routes {
        resource_routes(Ctx {
            panel,
            resource: self.0.clone(),
        })
    }
}

/// The panel's routes, under its path, named `admin.*`.
pub(crate) fn routes(panel: Arc<Panel>) -> Routes {
    let mut inner = Routes::new()
        .get("/", dashboard)
        .name("dashboard")
        .route_layer(Extension(panel.clone()))
        .require_auth();
    for resource in &panel.resources {
        inner = inner.merge(resource.routes(panel.clone()));
    }
    Routes::new().group(&panel.path, "admin.", inner)
}

/// What a resource's handlers share.
pub(crate) struct Ctx<R> {
    panel: Arc<Panel>,
    resource: Arc<R>,
}

impl<R> Clone for Ctx<R> {
    fn clone(&self) -> Self {
        Self {
            panel: self.panel.clone(),
            resource: self.resource.clone(),
        }
    }
}

fn resource_routes<R: AdminResource>(cx: Ctx<R>) -> Routes {
    let slug = cx.resource.slug().to_owned();
    let one = format!("/{slug}/{{id}}");
    let mut routes = Routes::new()
        .get(&format!("/{slug}"), index::<R>)
        .name(&format!("{slug}.index"))
        .post(&format!("/{slug}"), store::<R>)
        .name(&format!("{slug}.store"))
        .get(&format!("/{slug}/create"), create::<R>)
        .name(&format!("{slug}.create"))
        .post(&format!("/{slug}/actions/{{action}}"), bulk::<R>)
        .name(&format!("{slug}.bulk"))
        .get(&one, show::<R>)
        .name(&format!("{slug}.show"))
        .put(&one, update::<R>)
        .name(&format!("{slug}.update"))
        .delete(&one, destroy::<R>)
        .name(&format!("{slug}.destroy"))
        .get(&format!("{one}/edit"), edit::<R>)
        .name(&format!("{slug}.edit"))
        .post(&format!("{one}/actions/{{action}}"), row_action::<R>)
        .name(&format!("{slug}.action"))
        .route_layer(Extension(cx.clone()))
        .require_auth();
    for manager in cx.resource.relations() {
        routes = routes.merge(manager.routes(Arc::new(cx.clone())));
    }
    routes
}

/// The resource as its pages show it (`resource`).
#[derive(Serialize)]
struct Info {
    slug: String,
    label: String,
    plural_label: String,
    index_url: String,
    create_url: String,
    has_view: bool,
}

/// What the user may do, for the pages' buttons (`can`).
#[derive(Serialize, Default)]
struct Abilities {
    create: bool,
    update: bool,
    delete: bool,
    restore: bool,
    force_delete: bool,
}

impl<R: AdminResource> Ctx<R> {
    fn base(&self) -> String {
        format!("{}/{}", self.panel.path, self.resource.slug())
    }

    fn info(&self, texts: &Texts) -> Info {
        let base = self.base();
        Info {
            slug: self.resource.slug().to_owned(),
            label: self.label(texts),
            plural_label: self.plural(texts),
            create_url: format!("{base}/create"),
            has_view: !self.resource.entries().is_empty(),
            index_url: base,
        }
    }

    /// The resource's name for one record, in the request's language.
    fn label(&self, texts: &Texts) -> String {
        texts.label(self.resource.slug(), self.resource.label())
    }

    /// The resource's name for several records, in the request's language.
    fn plural(&self, texts: &Texts) -> String {
        texts.label(self.resource.slug(), self.resource.plural_label())
    }

    /// A record's title: the resource's own, or "Product #7" in the
    /// request's language.
    fn title_of(&self, record: &R::Model, texts: &Texts) -> String {
        record_title(&*self.resource, record, texts)
    }

    /// Whether the resource offers `ability` at all: an edit-only resource
    /// has no create and no delete.
    fn offers(&self, ability: &str) -> bool {
        offers(&*self.resource, ability)
    }

    /// `ability` is offered and the user may (the policy's answer).
    fn allows(&self, user: &AuthUser, ability: &str, record: Option<&R::Model>) -> bool {
        self.offers(ability) && self.resource.allows(user, ability, record)
    }

    /// The panel lets `user` in and `ability` is allowed, or 403 (404 for
    /// what the resource doesn't offer).
    fn authorize(&self, user: &AuthUser, ability: &str, record: Option<&R::Model>) -> Result {
        self.panel.check(user)?;
        if !self.offers(ability) {
            return Err(Error::NotFound);
        }
        if self.resource.allows(user, ability, record) {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }

    /// The list's query for `?filter=`: the resource's query, narrowed by
    /// the named filter, or only the deleted rows for `trashed`.
    fn scoped(&self, filter: Option<&str>) -> Query<R::Model> {
        let query = self.resource.query();
        match filter {
            Some(TRASHED) if R::Model::SOFT_DELETES && self.offers("restoreAny") => {
                query.only_trashed()
            }
            Some(key) => match self.resource.filters().iter().find(|f| f.key() == key) {
                Some(found) => found.apply(query),
                None => query,
            },
            None => query,
        }
    }

    /// The record `id`, among the deleted ones too when `trashed`; 404
    /// when there's none.
    async fn find(&self, db: &Db, id: &str, trashed: bool) -> Result<R::Model> {
        let key: <R::Model as Model>::Key = id.parse().map_err(|_| Error::NotFound)?;
        let query = self.resource.query();
        let query = if trashed { query.with_trashed() } else { query };
        query
            .where_eq("id", key)
            .first(db)
            .await?
            .ok_or(Error::NotFound)
    }

    /// The id of the sheet that asks for `action`'s input.
    fn sheet_id(action: &AdminAction<R::Model>) -> String {
        format!("rx-admin-action-{}", action.key())
    }

    /// The list's grid, with the actions `user` may take.
    fn grid(&self, user: &AuthUser, trashed: bool, texts: &Texts) -> Grid {
        let base = self.base();
        let resource = &self.resource;
        let slug = resource.slug();
        let label = lower(&self.label(texts));
        let plural = lower(&self.plural(texts));
        let mut grid = Grid::new(slug)
            .title(&self.plural(texts))
            .sort_by("-id")
            .exports()
            .advanced_filter()
            .empty_state(&texts.get("empty", &[("plural", &plural)]), None);
        for column in resource.columns() {
            let heading = texts.label(slug, column.label());
            grid = grid.column(column.titled(&heading));
        }
        let has_view = !resource.entries().is_empty();
        let may = |ability: &str| self.allows(user, ability, None);
        if trashed {
            let restore = texts.get("restore", &[]);
            let for_good = texts.get("delete_for_good", &[]);
            if may("restoreAny") {
                grid = grid.bulk_action(Action::new(&restore, &format!("{base}/actions/restore")));
            }
            if may("forceDeleteAny") {
                grid = grid.bulk_action(
                    Action::new(&for_good, &format!("{base}/actions/force-delete"))
                        .confirm(&texts.get("bulk_delete_for_good", &[("plural", &plural)]))
                        .danger(),
                );
            }
            if may("restore") {
                grid = grid.row_action(Action::new(
                    &restore,
                    &format!("{base}/{{id}}/actions/restore"),
                ));
            }
            if may("forceDelete") {
                grid = grid.row_action(
                    Action::new(&for_good, &format!("{base}/{{id}}/actions/force-delete"))
                        .confirm(&texts.get("row_delete_for_good", &[("label", &label)]))
                        .danger(),
                );
            }
            if has_view {
                grid = grid.row_url(&format!("{base}/{{id}}"));
            }
            return resource.grid(grid);
        }
        grid = grid.row_url(&if has_view {
            format!("{base}/{{id}}")
        } else {
            format!("{base}/{{id}}/edit")
        });
        for action in resource.actions() {
            if !may(action.ability_name()) {
                continue;
            }
            let name = texts.label(slug, action.label());
            let mut bulk = Action::new(&name, &format!("{base}/actions/{}", action.key()));
            let mut row = Action::new(&name, &format!("{base}/{{id}}/actions/{}", action.key()));
            if !action.fields().is_empty() {
                let sheet = Self::sheet_id(&action);
                bulk = bulk.sheet(&sheet);
                row = row.sheet(&sheet);
            } else if let Some(question) = action.question() {
                let question = texts.label(slug, question);
                bulk = bulk.confirm(&question);
                row = row.confirm(&question);
            }
            if action.is_danger() {
                bulk = bulk.danger();
                row = row.danger();
            }
            if action.on_bulk() {
                grid = grid.bulk_action(bulk);
            }
            if action.on_row() {
                grid = grid.row_action(row);
            }
        }
        if may("deleteAny") {
            let key = if R::Model::SOFT_DELETES {
                "bulk_delete_soft"
            } else {
                "bulk_delete_hard"
            };
            grid = grid.bulk_action(
                Action::new(&texts.get("delete", &[]), &format!("{base}/actions/delete"))
                    .confirm(&texts.get(key, &[("plural", &plural)]))
                    .danger(),
            );
        }
        if has_view {
            grid = grid.row_action(Action::link(
                &texts.get("view", &[]),
                &format!("{base}/{{id}}"),
            ));
        }
        if may("update") {
            grid = grid.row_action(Action::link(
                &texts.get("edit", &[]),
                &format!("{base}/{{id}}/edit"),
            ));
        }
        if may("delete") {
            grid = grid.row_action(
                Action::new(&texts.get("delete", &[]), &format!("{base}/{{id}}"))
                    .method("DELETE")
                    .confirm(&texts.get("delete_question", &[("label", &label)]))
                    .danger(),
            );
        }
        resource.grid(grid)
    }

    /// The sheets that ask for input before an action (the list draws
    /// them with the kit's `action_sheet`, opened from the grid).
    fn sheets(&self, user: &AuthUser, texts: &Texts, decimals: u32) -> Vec<Value> {
        let slug = self.resource.slug();
        let base = self.base();
        let mut sheets = Vec::new();
        for action in self.resource.actions() {
            if action.fields().is_empty() || !self.allows(user, action.ability_name(), None) {
                continue;
            }
            let fields: Vec<FieldView> = action
                .fields()
                .iter()
                .cloned()
                .map(|mut field| {
                    field.translate(&|text| texts.label(slug, text));
                    FieldView::new(field, None, true, decimals)
                })
                .collect();
            let name = texts.label(slug, action.label());
            sheets.push(json!({
                "id": Self::sheet_id(&action),
                "title": name,
                "submit_label": action.submit().map(|text| texts.label(slug, text)),
                "description": action.describe().map(|text| texts.label(slug, text)),
                "danger": action.is_danger(),
                "url": format!("{base}/actions/{}", action.key()),
                "fields": fields,
            }));
        }
        sheets
    }

    /// The form's fields for the create page (`record` is `None`) or the
    /// edit page, each with its value and a `belongs_to`'s choices.
    async fn fields(
        &self,
        state: &AppState,
        record: Option<&Value>,
        texts: &Texts,
    ) -> Result<Vec<FieldView>> {
        form_fields(&*self.resource, state, record, texts, &[]).await
    }

    /// The record's tabs: "Details", then each relation manager the user
    /// may see. Empty when the resource has none.
    fn relation_tabs(&self, user: &AuthUser, texts: &Texts, id: &str) -> Vec<(String, String)> {
        let managers: Vec<_> = self
            .resource
            .relations()
            .into_iter()
            .filter(|manager| manager.visible(user))
            .collect();
        if managers.is_empty() {
            return Vec::new();
        }
        let base = self.base();
        let has_view = !self.resource.entries().is_empty();
        let details = if has_view {
            format!("{base}/{id}")
        } else {
            format!("{base}/{id}/edit")
        };
        let mut tabs = vec![(details, texts.get("details", &[]))];
        for manager in managers {
            tabs.push((
                format!("{base}/{id}/relations/{}", manager.key()),
                texts.label(self.resource.slug(), manager.label()),
            ));
        }
        tabs
    }

    /// The names of the form's money fields.
    fn money_fields(&self) -> Vec<String> {
        money_fields(&*self.resource)
    }

    /// The resource's record-aware rules for `form`, with what refills the
    /// form if they fail. Synchronous, so the form isn't held across an
    /// `.await` (it needn't be `Sync`).
    fn rules(&self, form: &R::Form, record: Option<&R::Model>) -> (Validator, Value) {
        rules_of(&*self.resource, form, record)
    }
}

/// A record whose relation manager is open, as the manager sees it.
pub(crate) struct ParentRecord {
    pub(crate) id: String,
    pub(crate) key: renox::db::DbValue,
    pub(crate) title: String,
    pub(crate) can_update: bool,
}

/// The resource that owns a relation manager, whatever its model.
pub(crate) trait Parent: Send + Sync {
    fn panel(&self) -> &Arc<Panel>;
    fn slug(&self) -> &str;
    /// The resource's list address.
    fn base(&self) -> String;
    fn names(&self, texts: &Texts) -> (String, String);
    /// The record `id`, once the user may open it (to view or to edit).
    fn open(
        &self,
        state: &AppState,
        user: &AuthUser,
        id: &str,
        texts: &Texts,
    ) -> BoxFuture<'static, Result<ParentRecord>>;
    fn tabs(&self, user: &AuthUser, texts: &Texts, id: &str) -> Vec<(String, String)>;
}

impl<R: AdminResource> Parent for Ctx<R> {
    fn panel(&self) -> &Arc<Panel> {
        &self.panel
    }

    fn slug(&self) -> &str {
        self.resource.slug()
    }

    fn base(&self) -> String {
        Ctx::base(self)
    }

    fn names(&self, texts: &Texts) -> (String, String) {
        (self.label(texts), self.plural(texts))
    }

    fn open(
        &self,
        state: &AppState,
        user: &AuthUser,
        id: &str,
        texts: &Texts,
    ) -> BoxFuture<'static, Result<ParentRecord>> {
        let (cx, state, user, id, texts) = (
            self.clone(),
            state.clone(),
            user.clone(),
            id.to_owned(),
            texts.clone(),
        );
        Box::pin(async move {
            cx.panel.check(&user)?;
            let record = cx.find(&state.db, &id, false).await?;
            let can_update = cx.allows(&user, "update", Some(&record));
            if !can_update && !cx.allows(&user, "view", Some(&record)) {
                return Err(Error::Forbidden);
            }
            Ok(ParentRecord {
                id: record.id().to_string(),
                key: renox::db::ToDbValue::to_db_value(&record.id()),
                title: cx.title_of(&record, &texts),
                can_update,
            })
        })
    }

    fn tabs(&self, user: &AuthUser, texts: &Texts, id: &str) -> Vec<(String, String)> {
        self.relation_tabs(user, texts, id)
    }
}

/// Whether `resource` offers `ability` at all (an edit-only resource has
/// no create and no delete).
pub(crate) fn offers<R: AdminResource>(resource: &R, ability: &str) -> bool {
    match ability {
        "create" => resource.creatable(),
        "delete" | "deleteAny" | "restore" | "restoreAny" | "forceDelete" | "forceDeleteAny" => {
            resource.deletable()
        }
        _ => true,
    }
}

/// A record's title: the resource's own, or "Product #7" (in the
/// request's language) when it keeps the default.
pub(crate) fn record_title<R: AdminResource>(
    resource: &R,
    record: &R::Model,
    texts: &Texts,
) -> String {
    let title = resource.record_title(record);
    let id = record.id().to_string();
    if title == format!("{} #{}", resource.label(), id) {
        let label = texts.label(resource.slug(), resource.label());
        texts.get("record_title", &[("label", &label), ("id", &id)])
    } else {
        title
    }
}

/// The names of `resource`'s money fields.
pub(crate) fn money_fields<R: AdminResource>(resource: &R) -> Vec<String> {
    resource
        .fields()
        .iter()
        .filter(|field| field.kind() == FieldKind::Money)
        .map(|field| field.name().to_owned())
        .collect()
}

/// `resource`'s form fields for a create page (`record` is `None`) or an
/// edit page, translated, each with its value and a `belongs_to`'s
/// choices. `hidden` names fields left out (a relation's foreign key).
pub(crate) async fn form_fields<R: AdminResource>(
    resource: &R,
    state: &AppState,
    record: Option<&Value>,
    texts: &Texts,
    hidden: &[&str],
) -> Result<Vec<FieldView>> {
    let (db, decimals) = (&state.db, money_decimals(state));
    let creating = record.is_none();
    let slug = resource.slug();
    let mut views = Vec::new();
    for mut field in resource.fields() {
        if !field.shown(creating) || hidden.contains(&field.name()) {
            continue;
        }
        if let Some((table, title)) = field.relation() {
            let options = relation_options(db, table, title).await?;
            field.set_options(options);
        }
        if matches!(field.kind(), FieldKind::Select | FieldKind::BelongsTo)
            && !field.has_placeholder()
        {
            field = field.placeholder(&texts.get("select_placeholder", &[]));
        }
        field.translate(&|text| texts.label(slug, text));
        views.push(FieldView::new(field, record, creating, decimals));
    }
    Ok(views)
}

/// `resource`'s record-aware rules for `form`, with what refills the form
/// if they fail.
pub(crate) fn rules_of<R: AdminResource>(
    resource: &R,
    form: &R::Form,
    record: Option<&R::Model>,
) -> (Validator, Value) {
    let mut validator = Validator::new();
    resource.rules(form, record, &mut validator);
    let input = renox::serde_json::to_value(form).unwrap_or(Value::Null);
    (validator, input)
}

/// Runs the rules' database checks: a failure answers like `Valid<T>`
/// does (422 for htmx, back to the form otherwise).
pub(crate) async fn check(db: &Db, (validator, input): (Validator, Value)) -> Result {
    let errors = validator.finish(db).await?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationError::new(errors).with_input(&input).into())
    }
}

/// A field as `renox-admin/fields.html` draws it.
#[derive(Serialize)]
pub(crate) struct FieldView {
    #[serde(flatten)]
    field: Field,
    value: Value,
    /// Extra attributes of the input (`step`, `min`, `max`).
    attrs: BTreeMap<String, String>,
    /// Shown but not changeable (edit pages, `readonly_on_edit`).
    locked: bool,
}

impl FieldView {
    /// `decimals`: `APP_CURRENCY`'s, for money fields, which show whole
    /// units (`12.99`) of an amount kept in the smallest unit (`1299`).
    pub(crate) fn new(field: Field, record: Option<&Value>, creating: bool, decimals: u32) -> Self {
        let json = renox::serde_json::to_value(&field).unwrap_or(Value::Null);
        let read = |key: &str| json.get(key).cloned().unwrap_or(Value::Null);
        let mut value = match record {
            Some(record) => record.get(field.name()).cloned().unwrap_or(Value::Null),
            None => read("default"),
        };
        match field.kind() {
            FieldKind::Password => value = Value::Null,
            FieldKind::DateTime => {
                // `2026-10-05T09:30:00Z` → `2026-10-05T09:30`, as the input takes it.
                if let Some(text) = value.as_str() {
                    let text: String = text.replace(' ', "T").chars().take(16).collect();
                    value = Value::String(text);
                }
            }
            FieldKind::Date => {
                if let Some(text) = value.as_str() {
                    value = Value::String(text.chars().take(10).collect());
                }
            }
            FieldKind::Money if decimals > 0 => {
                if let Some(amount) = value.as_i64() {
                    value = Value::String(whole_units(amount, decimals));
                }
            }
            _ => {}
        }
        let mut attrs = BTreeMap::new();
        if field.kind() == FieldKind::Money && decimals > 0 {
            // `0.01` for cents, so the browser takes `12.99`.
            attrs.insert(
                "step".to_owned(),
                format!("{:.*}", decimals as usize, 0.1f64.powi(decimals as i32)),
            );
        }
        for key in ["step", "min", "max"] {
            if let Some(text) = read(key).as_str() {
                attrs.insert(key.to_owned(), text.to_owned());
            }
        }
        let locked = !creating && read("readonly_on_edit").as_bool() == Some(true);
        Self {
            field,
            value,
            attrs,
            locked,
        }
    }
}

/// `table`'s rows as `(id, title)` choices, by title.
async fn relation_options(db: &Db, table: &str, title: &str) -> Result<Vec<(String, String)>> {
    if !plain_name(table) || !plain_name(title) {
        return Err(Error::Internal(renox::anyhow::anyhow!(
            "Field::belongs_to: `{table}` and `{title}` may only have letters, digits and `_`"
        )));
    }
    let rows = renox::db::sql(format!(
        "SELECT CAST(\"id\" AS TEXT) AS id, CAST(\"{title}\" AS TEXT) AS title \
         FROM \"{table}\" ORDER BY \"{title}\" LIMIT {MAX_OPTIONS}"
    ))
    .fetch_all(db)
    .await?;
    let mut options = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id")?;
        let title: Option<String> = row.try_get("title")?;
        options.push((id, title.unwrap_or_default()));
    }
    Ok(options)
}

/// The record as the pages read it: its fields as JSON.
pub(crate) fn to_json(record: &impl Serialize) -> Result<Value> {
    Ok(renox::serde_json::to_value(record)?)
}

/// A page of the panel, with the layout's values.
fn page<R: AdminResource>(
    cx: &Ctx<R>,
    state: &AppState,
    user: &AuthUser,
    name: &str,
    mut values: Map<String, Value>,
    texts: &Texts,
) -> Result<View> {
    values.insert(
        "admin".into(),
        to_json(&cx.panel.frame(state, user, Some(cx.resource.slug()), texts))?,
    );
    values.insert("resource".into(), to_json(&cx.info(texts))?);
    Ok(view(name, Value::Object(values)))
}

/// After a save: the list, with a toast (`HX-Redirect` for htmx forms).
pub(crate) fn done(htmx: &Htmx, toast: Toast, to: &str) -> Response {
    (toast, htmx.redirect(to)).into_response()
}

/// What an input sheet's form is checked against: each field's own
/// declaration (`required`, numbers, dates, `min`/`max`, the choices).
/// The values come back as the action reads them: money in the smallest
/// unit, toggles as `true`/`false`.
pub(crate) fn check_input(
    fields: &[Field],
    posted: &[(String, String)],
    decimals: u32,
    texts: &Texts,
    scope: &str,
) -> std::result::Result<ActionInput, Errors> {
    let mut errors = Errors::new();
    let mut values = Vec::new();
    for field in fields {
        let name = field.name();
        let label = texts.label(scope, field.label_text());
        let mut value = posted
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.trim().to_owned())
            .unwrap_or_default();
        let mut fail = |key: &str, extra: &[(&str, &str)]| {
            let mut all = vec![("field", label.as_str())];
            all.extend_from_slice(extra);
            errors.add(name, texts.get(key, &all));
        };
        match field.kind() {
            FieldKind::Checkbox | FieldKind::Toggle => {
                let on = matches!(value.as_str(), "on" | "true" | "1");
                values.push((name.to_owned(), on.to_string()));
                continue;
            }
            _ if value.is_empty() => {
                if field.is_required() {
                    fail("required", &[]);
                }
                values.push((name.to_owned(), value));
                continue;
            }
            FieldKind::Number | FieldKind::Money => match value.parse::<f64>() {
                Ok(n) if n.is_finite() => {
                    for (limit, key, below) in [
                        (field.min_text(), "min", true),
                        (field.max_text(), "max", false),
                    ] {
                        if let Some(limit) = limit
                            && let Ok(bound) = limit.parse::<f64>()
                            && (if below { n < bound } else { n > bound })
                        {
                            fail(key, &[(key, limit)]);
                        }
                    }
                    if field.kind() == FieldKind::Money
                        && decimals > 0
                        && let Some(amount) = smallest_unit(&value, decimals)
                    {
                        value = amount;
                    }
                }
                _ => fail("number", &[]),
            },
            FieldKind::Date
                if renox::chrono::NaiveDate::parse_from_str(&value, "%Y-%m-%d").is_err() =>
            {
                fail("date", &[]);
            }
            FieldKind::Email if !value.contains('@') => fail("email", &[]),
            FieldKind::Select if !field.choices().iter().any(|(v, _)| *v == value) => {
                fail("choice", &[]);
            }
            _ => {}
        }
        values.push((name.to_owned(), value));
    }
    if errors.is_empty() {
        Ok(ActionInput::new(values))
    } else {
        Err(errors)
    }
}

/// Reads and checks the form, after the user was let in: `Valid<T>`'s
/// answer (errors, or a live-validation reply) when it isn't valid. The
/// `money` fields' whole units (`12.99`) become the smallest unit
/// (`1299`) first, as the model keeps them.
pub(crate) async fn read_form<F>(
    state: &AppState,
    req: Request,
    money: &[String],
    fixed: &[(String, String)],
) -> std::result::Result<F, Box<Response>>
where
    F: serde::de::DeserializeOwned + Validate + Send,
{
    let req = rewrite_body(state, req, money, fixed).await?;
    Valid::<F>::from_request(req, state)
        .await
        .map(|Valid(form)| form)
        .map_err(Box::new)
}

/// `APP_CURRENCY`'s usual decimals (2 for `USD`, 0 for `IDR`).
pub(crate) fn money_decimals(state: &AppState) -> u32 {
    renox::currency_decimals(&state.config.currency).min(6)
}

/// `1299` with 2 decimals → `12.99`.
fn whole_units(amount: i64, decimals: u32) -> String {
    let scale = 10i64.pow(decimals);
    let sign = if amount < 0 { "-" } else { "" };
    let (whole, part) = (
        amount.unsigned_abs() / scale as u64,
        amount.unsigned_abs() % scale as u64,
    );
    format!("{sign}{whole}.{part:0width$}", width = decimals as usize)
}

/// `12.99` with 2 decimals → `1299` (rounded to the smallest unit). Text
/// that isn't an amount is left for the form's rules to report.
/// Worked out on the digits, so `0.305` is `31`, not a float's `30`.
pub(crate) fn smallest_unit(text: &str, decimals: u32) -> Option<String> {
    let text = text.trim();
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
        return None;
    }
    let places = decimals as usize;
    let kept: String = fraction
        .chars()
        .chain(std::iter::repeat('0'))
        .take(places)
        .collect();
    let mut amount: i64 = format!("{whole}{kept}")
        .trim_start_matches('0')
        .parse()
        .unwrap_or(0);
    if whole.trim_start_matches('0').len() + places > 17 {
        return None;
    }
    if fraction.as_bytes().get(places).is_some_and(|b| *b >= b'5') {
        amount += 1;
    }
    Some(format!(
        "{}{amount}",
        if negative && amount != 0 { "-" } else { "" }
    ))
}

/// The request with its money fields in the smallest unit, and the
/// `fixed` values set whatever the browser sent (a relation's foreign
/// key): a form (`application/x-www-form-urlencoded`) or a JSON object.
/// Other bodies, and currencies without decimals, pass as they are.
async fn rewrite_body(
    state: &AppState,
    req: Request,
    money: &[String],
    fixed: &[(String, String)],
) -> std::result::Result<Request, Box<Response>> {
    use renox::axum::body::{Body, Bytes};
    use renox::axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};

    let decimals = money_decimals(state);
    let money: &[String] = if decimals == 0 { &[] } else { money };
    if money.is_empty() && fixed.is_empty() {
        return Ok(req);
    }
    let kind = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let form = kind.starts_with("application/x-www-form-urlencoded");
    let json = kind.starts_with("application/json");
    if !form && !json {
        return Ok(req);
    }
    let (mut parts, body) = req.into_parts();
    let bytes = Bytes::from_request(Request::from_parts(parts.clone(), body), state)
        .await
        .map_err(|rejection| Box::new(rejection.into_response()))?;
    let changed = if form {
        let text = String::from_utf8_lossy(&bytes);
        let mut pairs: Vec<String> = text
            .split('&')
            .filter(|pair| {
                let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
                !fixed.iter().any(|(name, _)| *name == url_decode(key))
            })
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                if money.iter().any(|name| *name == url_decode(key))
                    && let Some(amount) = smallest_unit(&url_decode(value), decimals)
                {
                    format!("{key}={amount}")
                } else {
                    pair.to_owned()
                }
            })
            .collect();
        for (name, value) in fixed {
            pairs.push(format!("{}={}", url_encode(name), url_encode(value)));
        }
        Bytes::from(pairs.join("&"))
    } else {
        match renox::serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(mut object)) => {
                for name in money {
                    let amount = match object.get(name) {
                        Some(Value::String(text)) => smallest_unit(text, decimals),
                        Some(Value::Number(n)) => smallest_unit(&n.to_string(), decimals),
                        _ => None,
                    };
                    if let Some(amount) = amount.and_then(|a| a.parse::<i64>().ok()) {
                        object.insert(name.clone(), Value::from(amount));
                    }
                }
                for (name, value) in fixed {
                    let value = value
                        .parse::<i64>()
                        .map_or_else(|_| Value::String(value.clone()), Value::from);
                    object.insert(name.clone(), value);
                }
                Bytes::from(renox::serde_json::to_vec(&object).unwrap_or_default())
            }
            _ => bytes,
        }
    };
    parts.headers.remove(CONTENT_LENGTH);
    Ok(Request::from_parts(parts, Body::from(changed)))
}

/// `a b` → `a%20b` (a form's key or value).
fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `a=1&b=x%20y` → `[("a", "1"), ("b", "x y")]`.
pub(crate) fn parse_pairs(body: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(body)
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (url_decode(key), url_decode(value))
        })
        .collect()
}

/// `a%20b+c` → `a b c` (a form's key or value).
fn url_decode(text: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| Some(hex(bytes[i + 1])? * 16 + hex(bytes[i + 2])?))
            .flatten();
        match (bytes[i], escaped) {
            (_, Some(byte)) => {
                out.push(byte as u8);
                i += 2;
            }
            (b'+', None) => out.push(b' '),
            (byte, None) => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn dashboard(
    Extension(panel): Extension<Arc<Panel>>,
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<View> {
    panel.check(&user)?;
    let texts = Texts::new(&state);
    let mut cards = Vec::new();
    for resource in &panel.resources {
        if !resource.visible(&user) {
            continue;
        }
        let count = resource.count(state.db.clone()).await?;
        cards.push(json!({
            "label": texts.label(resource.slug(), resource.plural()),
            "count": count,
            "url": format!("{}/{}", panel.path, resource.slug()),
        }));
    }
    Ok(view(
        "renox-admin/dashboard.html",
        context! { admin => panel.frame(&state, &user, None, &texts), cards },
    ))
}

async fn index<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    request: GridRequest,
) -> Result<Response> {
    cx.authorize(&user, "viewAny", None)?;
    let texts = Texts::new(&state);
    let slug = cx.resource.slug();
    let filter = request.param("filter").filter(|f| !f.is_empty());
    let trashed = filter == Some(TRASHED) && R::Model::SOFT_DELETES && cx.offers("restoreAny");
    let grid = cx.grid(&user, trashed, &texts);
    let query = cx.scoped(filter);
    if let Some(file) = grid.export(query.clone(), &request).await? {
        return Ok(file);
    }
    let rows = grid.page(query, &request).await?;
    // The named filters as tabs: "All", the resource's own, then the trash.
    let base = cx.base();
    let mut tabs = Vec::new();
    let filters = cx.resource.filters();
    let has_trash = R::Model::SOFT_DELETES && cx.offers("restoreAny");
    if !filters.is_empty() || has_trash {
        tabs.push((base.clone(), texts.get("all", &[])));
        for found in &filters {
            tabs.push((
                format!("{base}?filter={}", found.key()),
                texts.label(slug, found.label()),
            ));
        }
        if has_trash && cx.resource.allows(&user, "restoreAny", None) {
            tabs.push((format!("{base}?filter={TRASHED}"), texts.get("trash", &[])));
        }
    }
    let current = match filter {
        Some(key)
            if tabs
                .iter()
                .any(|(url, _)| url.ends_with(&format!("?filter={key}"))) =>
        {
            format!("{base}?filter={key}")
        }
        _ => base.clone(),
    };
    let can = Abilities {
        create: !trashed && cx.allows(&user, "create", None),
        ..Abilities::default()
    };
    let plural = cx.plural(&texts);
    let mut values = Map::new();
    values.insert("rows".into(), to_json(&rows)?);
    values.insert("tabs".into(), to_json(&tabs)?);
    values.insert("current_tab".into(), Value::String(current));
    values.insert("allowed".into(), to_json(&can)?);
    values.insert(
        "tabs_label".into(),
        Value::String(texts.get("filters_label", &[("plural", &plural)])),
    );
    values.insert(
        "new_label".into(),
        Value::String(texts.get("new", &[("label", &lower(&cx.label(&texts)))])),
    );
    let sheets = if trashed {
        Vec::new()
    } else {
        cx.sheets(&user, &texts, money_decimals(&state))
    };
    values.insert("sheets".into(), to_json(&sheets)?);
    Ok(page(&cx, &state, &user, "renox-admin/index.html", values, &texts)?.into_response())
}

async fn create<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<View> {
    cx.authorize(&user, "create", None)?;
    let texts = Texts::new(&state);
    let fields = cx.fields(&state, None, &texts).await?;
    let info = cx.info(&texts);
    let label = lower(&info.label);
    let mut values = Map::new();
    values.insert(
        "title".into(),
        Value::String(texts.get("new", &[("label", &label)])),
    );
    values.insert("fields".into(), to_json(&fields)?);
    values.insert("action".into(), Value::String(info.index_url.clone()));
    values.insert("method".into(), Value::String("POST".into()));
    values.insert(
        "submit_label".into(),
        Value::String(texts.get("create", &[("label", &label)])),
    );
    values.insert("record".into(), Value::Null);
    page(&cx, &state, &user, "renox-admin/form.html", values, &texts)
}

async fn store<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    req: Request,
) -> Result<Response> {
    cx.authorize(&user, "create", None)?;
    let texts = Texts::new(&state);
    let form: R::Form = match read_form(&state, req, &cx.money_fields(), &[]).await {
        Ok(form) => form,
        Err(answer) => return Ok(*answer),
    };
    let rules = cx.rules(&form, None);
    check(&state.db, rules).await?;
    let mut record = R::Model::default();
    cx.resource.fill(&mut record, form);
    record.save(&state.db).await?;
    let saved = SaveContext {
        state: state.clone(),
        user: user.clone(),
        created: true,
        previous: None,
    };
    cx.resource.saved(&record, &saved).await?;
    let toast = Toast::success(texts.get("created", &[("label", &cx.label(&texts))]));
    Ok(done(&htmx, toast, &cx.base()))
}

async fn show<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<View> {
    cx.panel.check(&user)?;
    let texts = Texts::new(&state);
    let mut entries = cx.resource.entries();
    if entries.is_empty() {
        return Err(Error::NotFound);
    }
    let slug = cx.resource.slug();
    for entry in &mut entries {
        entry.translate(&|text| texts.label(slug, text));
    }
    let record = cx.find(&state.db, &id, true).await?;
    cx.authorize(&user, "view", Some(&record))?;
    let deleted = R::Model::SOFT_DELETES
        && to_json(&record)?
            .get("deleted_at")
            .is_some_and(|at| !at.is_null());
    let may = |ability: &str| cx.allows(&user, ability, Some(&record));
    let can = Abilities {
        create: false,
        update: !deleted && may("update"),
        delete: !deleted && may("delete"),
        restore: deleted && may("restore"),
        force_delete: deleted && may("forceDelete"),
    };
    let base = cx.base();
    let id = record.id().to_string();
    let label = cx.label(&texts);
    let mut values = Map::new();
    values.insert("record".into(), to_json(&record)?);
    values.insert("entries".into(), to_json(&entries)?);
    values.insert("allowed".into(), to_json(&can)?);
    values.insert("deleted".into(), Value::Bool(deleted));
    values.insert("soft".into(), Value::Bool(R::Model::SOFT_DELETES));
    values.insert("url".into(), Value::String(format!("{base}/{id}")));
    values.insert(
        "edit_url".into(),
        Value::String(format!("{base}/{id}/edit")),
    );
    values.insert(
        "restore_url".into(),
        Value::String(format!("{base}/{id}/actions/restore")),
    );
    values.insert(
        "force_delete_url".into(),
        Value::String(format!("{base}/{id}/actions/force-delete")),
    );
    values.insert("title".into(), Value::String(cx.title_of(&record, &texts)));
    values.insert("label_lower".into(), Value::String(lower(&label)));
    let tabs = cx.relation_tabs(&user, &texts, &id);
    values.insert("current_tab".into(), Value::String(format!("{base}/{id}")));
    values.insert("relations".into(), to_json(&tabs)?);
    page(&cx, &state, &user, "renox-admin/show.html", values, &texts)
}

async fn edit<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<View> {
    cx.panel.check(&user)?;
    let texts = Texts::new(&state);
    let record = cx.find(&state.db, &id, false).await?;
    cx.authorize(&user, "update", Some(&record))?;
    let json = to_json(&record)?;
    let fields = cx.fields(&state, Some(&json), &texts).await?;
    let base = cx.base();
    let id = record.id().to_string();
    let label = cx.label(&texts);
    let title = cx.title_of(&record, &texts);
    let mut values = Map::new();
    values.insert(
        "title".into(),
        Value::String(texts.get("edit_title", &[("title", &title)])),
    );
    values.insert("fields".into(), to_json(&fields)?);
    values.insert("action".into(), Value::String(format!("{base}/{id}")));
    values.insert("method".into(), Value::String("PUT".into()));
    values.insert(
        "submit_label".into(),
        Value::String(texts.get("save_changes", &[])),
    );
    values.insert("record".into(), json);
    if cx.allows(&user, "delete", Some(&record)) {
        values.insert(
            "delete".into(),
            json!({
                "url": format!("{base}/{id}"),
                "label": label,
                "soft": R::Model::SOFT_DELETES,
            }),
        );
    }
    let has_view = !cx.resource.entries().is_empty();
    let tabs = cx.relation_tabs(&user, &texts, &id);
    let details = if has_view {
        format!("{base}/{id}")
    } else {
        format!("{base}/{id}/edit")
    };
    values.insert("current_tab".into(), Value::String(details));
    values.insert("relations".into(), to_json(&tabs)?);
    page(&cx, &state, &user, "renox-admin/form.html", values, &texts)
}

async fn update<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<String>,
    req: Request,
) -> Result<Response> {
    cx.panel.check(&user)?;
    let texts = Texts::new(&state);
    let mut record = cx.find(&state.db, &id, false).await?;
    cx.authorize(&user, "update", Some(&record))?;
    let form: R::Form = match read_form(&state, req, &cx.money_fields(), &[]).await {
        Ok(form) => form,
        Err(answer) => return Ok(*answer),
    };
    let rules = cx.rules(&form, Some(&record));
    check(&state.db, rules).await?;
    let previous = to_json(&record)?;
    cx.resource.fill(&mut record, form);
    record.save(&state.db).await?;
    let saved = SaveContext {
        state: state.clone(),
        user: user.clone(),
        created: false,
        previous: Some(previous),
    };
    cx.resource.saved(&record, &saved).await?;
    let toast = Toast::success(texts.get("saved", &[("label", &cx.label(&texts))]));
    Ok(done(&htmx, toast, &cx.base()))
}

async fn destroy<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<String>,
) -> Result<Response> {
    cx.panel.check(&user)?;
    let texts = Texts::new(&state);
    let mut record = cx.find(&state.db, &id, false).await?;
    cx.authorize(&user, "delete", Some(&record))?;
    record.delete(&state.db).await?;
    let toast = Toast::success(texts.get("deleted", &[("label", &cx.label(&texts))]));
    // The grid's row action reloads the grid; the edit page's form goes
    // back to the list.
    if htmx.request {
        Ok(toast.into_response())
    } else {
        Ok(done(&htmx, toast, &cx.base()))
    }
}

/// What the built-in actions do to each record.
#[derive(Clone, Copy)]
enum Builtin {
    Delete,
    Restore,
    ForceDelete,
}

impl Builtin {
    fn of(key: &str, soft: bool) -> Option<Self> {
        match key {
            "delete" => Some(Self::Delete),
            "restore" if soft => Some(Self::Restore),
            "force-delete" if soft => Some(Self::ForceDelete),
            _ => None,
        }
    }

    /// The ability each record must allow.
    fn ability(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Restore => "restore",
            Self::ForceDelete => "forceDelete",
        }
    }

    /// The ability the list asks for before offering it over a selection.
    fn bulk_ability(self) -> &'static str {
        match self {
            Self::Delete => "deleteAny",
            Self::Restore => "restoreAny",
            Self::ForceDelete => "forceDeleteAny",
        }
    }

    /// Whether it works on deleted records.
    fn on_trashed(self) -> bool {
        !matches!(self, Self::Delete)
    }

    fn done(self, count: usize, label: &str, plural: &str, texts: &Texts) -> Toast {
        let what = if count == 1 {
            format!("1 {}", label.to_lowercase())
        } else {
            format!("{count} {}", plural.to_lowercase())
        };
        let key = match self {
            Self::Delete => "n_deleted",
            Self::Restore => "n_restored",
            Self::ForceDelete => "n_deleted_for_good",
        };
        Toast::success(texts.get(key, &[("what", &what)]))
    }
}

/// Runs action `key` on `records`, once each is allowed. `posted` is the
/// request's form, for an action that asks for input.
async fn run<R: AdminResource>(
    cx: &Ctx<R>,
    state: &AppState,
    user: &AuthUser,
    key: &str,
    mut records: Vec<R::Model>,
    posted: &[(String, String)],
    texts: &Texts,
) -> Result<Toast> {
    if let Some(builtin) = Builtin::of(key, R::Model::SOFT_DELETES) {
        if !cx.offers(builtin.ability()) {
            return Err(Error::NotFound);
        }
        if records
            .iter()
            .any(|record| !cx.resource.allows(user, builtin.ability(), Some(record)))
        {
            return Err(Error::Forbidden);
        }
        let count = records.len();
        let mut tx = state.db.begin().await?;
        for record in &mut records {
            match builtin {
                Builtin::Delete => record.delete(&mut tx).await?,
                Builtin::Restore => record.restore(&mut tx).await?,
                Builtin::ForceDelete => record.force_delete(&mut tx).await?,
            }
        }
        tx.commit().await?;
        return Ok(builtin.done(count, &cx.label(texts), &cx.plural(texts), texts));
    }
    let action = cx
        .resource
        .actions()
        .into_iter()
        .find(|action| action.key() == key)
        .ok_or(Error::NotFound)?;
    if records.iter().any(|record| {
        !cx.resource
            .allows(user, action.ability_name(), Some(record))
    }) {
        return Err(Error::Forbidden);
    }
    let input = if action.fields().is_empty() {
        ActionInput::default()
    } else {
        check_input(
            action.fields(),
            posted,
            money_decimals(state),
            texts,
            cx.resource.slug(),
        )
        .map_err(ValidationError::new)?
    };
    let context = crate::ActionContext {
        state: state.clone(),
        user: user.clone(),
        input,
    };
    action.run(records, context).await
}

/// Whether the custom action `key` asks for input.
fn asks_input<R: AdminResource>(cx: &Ctx<R>, key: &str) -> bool {
    cx.resource
        .actions()
        .iter()
        .any(|action| action.key() == key && !action.fields().is_empty())
}

async fn bulk<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    Path(key): Path<String>,
    htmx: Htmx,
    request: GridRequest,
    body: renox::axum::body::Bytes,
) -> Result<Response> {
    cx.panel.check(&user)?;
    let texts = Texts::new(&state);
    let builtin = Builtin::of(&key, R::Model::SOFT_DELETES);
    match builtin {
        Some(builtin) if !cx.offers(builtin.bulk_ability()) => return Err(Error::NotFound),
        Some(builtin) if !cx.resource.allows(&user, builtin.bulk_ability(), None) => {
            return Err(Error::Forbidden);
        }
        None if !cx
            .resource
            .actions()
            .iter()
            .any(|action| action.key() == key && action.on_bulk()) =>
        {
            return Err(Error::NotFound);
        }
        _ => {}
    }
    let filter = request.param("filter").filter(|f| !f.is_empty());
    let trashed = filter == Some(TRASHED) && R::Model::SOFT_DELETES;
    // A built-in that works on deleted rows only sees the trash; the
    // others never do.
    if builtin.is_some_and(Builtin::on_trashed) != trashed {
        return Err(Error::NotFound);
    }
    let posted = parse_pairs(&body);
    let mut selection = Selection::default();
    for (name, value) in &posted {
        match name.as_str() {
            "ids" => {
                selection.ids = value
                    .split(',')
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            "all" => selection.all = value == "true",
            _ => {}
        }
    }
    let grid = cx.grid(&user, trashed, &texts);
    let records = grid
        .selected(cx.scoped(filter), &request, &selection)?
        .limit(MAX_SELECTED)
        .get(&state.db)
        .await?;
    if records.is_empty() {
        return Ok(Toast::info(texts.get("nothing_selected", &[])).into_response());
    }
    let input = asks_input(&cx, &key);
    let toast = run(&cx, &state, &user, &key, records, &posted, &texts).await?;
    // An action with a sheet sits outside the grid: reload the page.
    if input && htmx.request {
        Ok((toast, HxRefresh).into_response())
    } else {
        Ok(toast.into_response())
    }
}

async fn row_action<R: AdminResource>(
    Extension(cx): Extension<Ctx<R>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path((id, key)): Path<(String, String)>,
    body: renox::axum::body::Bytes,
) -> Result<Response> {
    cx.panel.check(&user)?;
    let texts = Texts::new(&state);
    let builtin = Builtin::of(&key, R::Model::SOFT_DELETES);
    let trashed = builtin.is_some_and(Builtin::on_trashed);
    if builtin.is_none()
        && !cx
            .resource
            .actions()
            .iter()
            .any(|action| action.key() == key && action.on_row())
    {
        return Err(Error::NotFound);
    }
    let record = cx.find(&state.db, &id, trashed).await?;
    let posted = parse_pairs(&body);
    let input = asks_input(&cx, &key);
    let toast = run(&cx, &state, &user, &key, vec![record], &posted, &texts).await?;
    // From the grid: the toast, and the grid reloads (a sheet is outside
    // it: the page does). From the view page's form: back to the list
    // (the record may be gone).
    if htmx.request {
        if input {
            Ok((toast, HxRefresh).into_response())
        } else {
            Ok(toast.into_response())
        }
    } else {
        Ok(done(&htmx, toast, &cx.base()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_moves_between_whole_and_smallest_units() {
        assert_eq!(whole_units(1299, 2), "12.99");
        assert_eq!(whole_units(5, 2), "0.05");
        assert_eq!(whole_units(-1250, 2), "-12.50");
        assert_eq!(whole_units(7, 3), "0.007");
        let unit = |text: &str| smallest_unit(text, 2);
        assert_eq!(unit("12.99").as_deref(), Some("1299"));
        assert_eq!(unit(" 12 ").as_deref(), Some("1200"));
        assert_eq!(unit("12.5").as_deref(), Some("1250"));
        assert_eq!(unit(".5").as_deref(), Some("50"));
        assert_eq!(unit("0.305").as_deref(), Some("31"));
        assert_eq!(unit("0.304").as_deref(), Some("30"));
        assert_eq!(unit("-1.00").as_deref(), Some("-100"));
        assert_eq!(unit("0.00").as_deref(), Some("0"));
        assert_eq!(unit("-0.001").as_deref(), Some("0"));
        for bad in [
            "",
            ".",
            "abc",
            "1,5",
            "1e3",
            "12.3.4",
            "99999999999999999999",
        ] {
            assert_eq!(unit(bad), None, "{bad}");
        }
        assert_eq!(url_decode("a%20b+c%2"), "a b c%2");
        assert_eq!(url_decode("price%5B0%5D"), "price[0]");
    }
}

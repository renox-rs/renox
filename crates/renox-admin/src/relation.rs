//! Child rows managed from a record's own pages (Filament's relation
//! managers): [`RelationManager`].

use std::fmt;
use std::sync::Arc;

use renox::axum::Extension;
use renox::axum::extract::Request;
use renox::db::{DbValue, Model, Query, ToDbValue};
use renox::grid::{Action, Grid, GridRequest, Selection};
use renox::prelude::*;
use renox::serde_json::{Map, Value};
use renox::validation::Errors;

use crate::AdminResource;
use crate::field::plain_name;
use crate::panel::{
    Parent, ParentRecord, check, done, form_fields, money_fields, offers, parse_pairs, read_form,
    record_title, rules_of, to_json,
};
use crate::resource::SaveContext;
use crate::texts::{Texts, lower};

/// The most rows an attach sheet offers.
const MAX_OPTIONS: u64 = 1_000;

/// How the managed rows belong to the record.
#[derive(Clone)]
enum Kind {
    /// The rows hold the record's id in `foreign_key`.
    HasMany { foreign_key: String },
    /// The rows are joined to the record by a pivot table.
    Pivot {
        table: String,
        parent_key: String,
        related_key: String,
    },
}

/// What a manager is, apart from the resource it shows.
struct Spec {
    key: String,
    label: String,
    kind: Kind,
    title: String,
}

/// A manager's handlers, for one resource type.
trait Def: Send + Sync {
    fn visible(&self, user: &AuthUser) -> bool;
    fn routes(&self, spec: Arc<Spec>, parent: Arc<dyn Parent>) -> Routes;
}

struct Typed<C>(Arc<C>);

impl<C: AdminResource> Def for Typed<C> {
    fn visible(&self, user: &AuthUser) -> bool {
        offers(&*self.0, "viewAny") && self.0.allows(user, "viewAny", None)
    }

    fn routes(&self, spec: Arc<Spec>, parent: Arc<dyn Parent>) -> Routes {
        routes(RelCtx {
            parent,
            child: self.0.clone(),
            spec,
        })
    }
}

/// The rows of another resource, managed from a record's own pages
/// (Filament's relation managers): each is a tab of the record (view and
/// edit pages), with a list of the rows that belong to it.
///
/// - [`has_many`](Self::has_many): rows holding the record's id (a product's
///   variants or photos). They get create, edit and delete pages under the
///   record, and the list's bulk delete. The child's form must have the
///   foreign key as a field (`product_id`): the panel fills it with the
///   record's id, whatever the browser sends, and leaves it out of the form.
/// - [`belongs_to_many`](Self::belongs_to_many): rows joined to the record
///   by a pivot table (the bike models a part fits). The tab lists the
///   attached rows, with Attach (a select of the rest) and Detach.
///
/// What the user may do is asked of the managed resource
/// ([`AdminResource::allows`]: `viewAny`, `create`, `update`, `delete`), and
/// to attach or detach, of the record's `update` too. The managed resource
/// needn't be registered with [`Admin::resource`](crate::Admin::resource).
///
/// ```
/// # use renox::prelude::*;
/// # use renox::grid::Column;
/// use renox_admin::{AdminResource, Field, RelationManager};
/// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String }
/// # impl Policy for Product { fn allows(&self, _: &User, _: &str) -> bool { true } }
/// # #[derive(Model, serde::Serialize, Default)] struct Variant { id: i64, product_id: i64, name: String }
/// # impl Policy for Variant { fn allows(&self, _: &User, _: &str) -> bool { true } }
/// # #[derive(serde::Deserialize, serde::Serialize, Validate)] struct ProductForm { #[validate(required)] name: String }
/// # #[derive(serde::Deserialize, serde::Serialize, Validate)] struct VariantForm { product_id: i64, #[validate(required)] name: String }
/// # struct Variants;
/// # impl AdminResource for Variants {
/// #     type Model = Variant;
/// #     type Form = VariantForm;
/// #     fn label(&self) -> &str { "Variant" }
/// #     fn plural_label(&self) -> &str { "Variants" }
/// #     fn columns(&self) -> Vec<Column> { vec![Column::text("name", "Name")] }
/// #     fn fields(&self) -> Vec<Field> { vec![Field::text("name", "Name").required()] }
/// #     fn fill(&self, v: &mut Variant, f: VariantForm) { v.product_id = f.product_id; v.name = f.name; }
/// # }
/// # struct Products;
/// # impl AdminResource for Products {
/// #     type Model = Product;
/// #     type Form = ProductForm;
/// #     fn label(&self) -> &str { "Product" }
/// #     fn plural_label(&self) -> &str { "Products" }
/// #     fn columns(&self) -> Vec<Column> { vec![Column::text("name", "Name")] }
/// #     fn fields(&self) -> Vec<Field> { vec![Field::text("name", "Name")] }
/// #     fn fill(&self, p: &mut Product, f: ProductForm) { p.name = f.name; }
/// // In the product's `impl AdminResource`:
/// fn relations(&self) -> Vec<RelationManager> {
///     vec![RelationManager::has_many("variants", "Variants", Variants, "product_id")]
/// }
/// # }
/// ```
#[derive(Clone)]
pub struct RelationManager {
    spec: Arc<Spec>,
    def: Arc<dyn Def>,
}

impl fmt::Debug for RelationManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelationManager")
            .field("key", &self.spec.key)
            .field("label", &self.spec.label)
            .finish_non_exhaustive()
    }
}

impl RelationManager {
    /// Rows of `child` whose `foreign_key` column holds the record's id,
    /// under the address segment `key` (letters, digits, `_` and `-`) and
    /// the tab `label`.
    ///
    /// # Panics
    ///
    /// If `key` or `foreign_key` isn't a plain name.
    pub fn has_many<C: AdminResource>(key: &str, label: &str, child: C, foreign_key: &str) -> Self {
        assert!(
            plain_name(foreign_key),
            "RelationManager::has_many: `{foreign_key}` may only have letters, digits and `_`"
        );
        Self::build(
            key,
            label,
            child,
            Kind::HasMany {
                foreign_key: foreign_key.to_owned(),
            },
        )
    }

    /// Rows of `related` joined to the record by `pivot_table`, whose
    /// `parent_key` column holds the record's id and `related_key` the
    /// row's.
    ///
    /// # Panics
    ///
    /// If `key` isn't plain, or a table or column name has anything but
    /// letters, digits and `_`.
    pub fn belongs_to_many<C: AdminResource>(
        key: &str,
        label: &str,
        related: C,
        pivot_table: &str,
        parent_key: &str,
        related_key: &str,
    ) -> Self {
        for name in [pivot_table, parent_key, related_key] {
            assert!(
                plain_name(name),
                "RelationManager::belongs_to_many: `{name}` may only have letters, digits and `_`"
            );
        }
        Self::build(
            key,
            label,
            related,
            Kind::Pivot {
                table: pivot_table.to_owned(),
                parent_key: parent_key.to_owned(),
                related_key: related_key.to_owned(),
            },
        )
    }

    fn build<C: AdminResource>(key: &str, label: &str, resource: C, kind: Kind) -> Self {
        assert!(
            !key.is_empty()
                && key.len() <= 64
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "RelationManager: the key `{key}` may only have letters, digits, `_` and `-`"
        );
        Self {
            spec: Arc::new(Spec {
                key: key.to_owned(),
                label: label.to_owned(),
                kind,
                title: "name".into(),
            }),
            def: Arc::new(Typed(Arc::new(resource))),
        }
    }

    /// The column that names a row in the select of
    /// [`belongs_to_many`](Self::belongs_to_many)'s Attach (`name` by
    /// default).
    ///
    /// # Panics
    ///
    /// If `column` isn't a plain name.
    pub fn title_column(mut self, column: &str) -> Self {
        assert!(
            plain_name(column),
            "RelationManager::title_column: `{column}` may only have letters, digits and `_`"
        );
        let spec = Spec {
            key: self.spec.key.clone(),
            label: self.spec.label.clone(),
            kind: self.spec.kind.clone(),
            title: column.to_owned(),
        };
        self.spec = Arc::new(spec);
        self
    }

    pub(crate) fn key(&self) -> &str {
        &self.spec.key
    }

    pub(crate) fn label(&self) -> &str {
        &self.spec.label
    }

    pub(crate) fn visible(&self, user: &AuthUser) -> bool {
        self.def.visible(user)
    }

    pub(crate) fn routes(&self, parent: Arc<dyn Parent>) -> Routes {
        self.def.routes(self.spec.clone(), parent)
    }
}

/// What a manager's handlers share.
struct RelCtx<C> {
    parent: Arc<dyn Parent>,
    child: Arc<C>,
    spec: Arc<Spec>,
}

impl<C> Clone for RelCtx<C> {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent.clone(),
            child: self.child.clone(),
            spec: self.spec.clone(),
        }
    }
}

fn routes<C: AdminResource>(cx: RelCtx<C>) -> Routes {
    let name = format!("{}.{}", cx.parent.slug(), cx.spec.key);
    let at = format!("/{}/{{id}}/relations/{}", cx.parent.slug(), cx.spec.key);
    let one = format!("{at}/{{rid}}");
    let mut routes = Routes::new()
        .get(&at, index::<C>)
        .name(&format!("{name}.index"));
    match cx.spec.kind {
        Kind::HasMany { .. } => {
            routes = routes
                .post(&at, store::<C>)
                .name(&format!("{name}.store"))
                .get(&format!("{at}/create"), create::<C>)
                .name(&format!("{name}.create"))
                .get(&format!("{one}/edit"), edit::<C>)
                .name(&format!("{name}.edit"))
                .put(&one, update::<C>)
                .name(&format!("{name}.update"))
                .delete(&one, remove::<C>)
                .name(&format!("{name}.destroy"));
        }
        Kind::Pivot { .. } => {
            routes = routes
                .post(&at, attach::<C>)
                .name(&format!("{name}.attach"))
                .delete(&one, remove::<C>)
                .name(&format!("{name}.detach"));
        }
    }
    routes
        .post(&format!("{at}/actions/remove"), bulk_remove::<C>)
        .name(&format!("{name}.bulk"))
        .route_layer(Extension(cx))
        .require_auth()
}

impl<C: AdminResource> RelCtx<C> {
    /// The manager's list address under record `id`.
    fn url(&self, id: &str) -> String {
        format!("{}/{id}/relations/{}", self.parent.base(), self.spec.key)
    }

    fn allows(&self, user: &AuthUser, ability: &str, record: Option<&C::Model>) -> bool {
        offers(&*self.child, ability) && self.child.allows(user, ability, record)
    }

    /// `ability` is offered and allowed, or 403 (404 when not offered).
    fn authorize(&self, user: &AuthUser, ability: &str, record: Option<&C::Model>) -> Result {
        if !offers(&*self.child, ability) {
            return Err(Error::NotFound);
        }
        if self.child.allows(user, ability, record) {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }

    /// Changing which rows are attached needs the record's `update`.
    fn pivot_write(&self, parent: &ParentRecord) -> Result {
        if parent.can_update {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }

    fn is_pivot(&self) -> bool {
        matches!(self.spec.kind, Kind::Pivot { .. })
    }

    /// The foreign key a has-many form is filled with.
    fn fixed(&self, parent: &ParentRecord) -> Vec<(String, String)> {
        match &self.spec.kind {
            Kind::HasMany { foreign_key } => vec![(foreign_key.clone(), parent.id.clone())],
            Kind::Pivot { .. } => Vec::new(),
        }
    }

    /// The rows that belong to the record.
    fn scoped(&self, parent: &ParentRecord) -> Query<C::Model> {
        let query = self.child.query();
        match &self.spec.kind {
            Kind::HasMany { foreign_key } => query.where_eq(foreign_key, parent.key.clone()),
            Kind::Pivot {
                table,
                parent_key,
                related_key,
            } => query.where_raw(
                &format!(
                    "\"{}\".\"id\" IN (SELECT \"{related_key}\" FROM \"{table}\" WHERE \"{parent_key}\" = ?)",
                    C::Model::TABLE
                ),
                [parent.key.clone()],
            ),
        }
    }

    /// The row `rid` of the record, 404 when it isn't one of its rows.
    async fn find(&self, db: &Db, parent: &ParentRecord, rid: &str) -> Result<C::Model> {
        let key: <C::Model as Model>::Key = rid.parse().map_err(|_| Error::NotFound)?;
        self.scoped(parent)
            .where_eq("id", key)
            .first(db)
            .await?
            .ok_or(Error::NotFound)
    }

    fn grid(&self, parent: &ParentRecord, user: &AuthUser, texts: &Texts) -> Grid {
        let slug = self.child.slug();
        let url = self.url(&parent.id);
        let label = lower(&texts.label(slug, self.child.label()));
        let plural = lower(&texts.label(slug, self.child.plural_label()));
        let mut grid = Grid::new(&format!("{}-{}", self.parent.slug(), self.spec.key))
            .title(&texts.label(self.parent.slug(), &self.spec.label))
            .sort_by("-id")
            .empty_state(&texts.get("relation_empty", &[("plural", &plural)]), None);
        for column in self.child.columns() {
            let heading = texts.label(slug, column.label());
            grid = grid.column(column.titled(&heading));
        }
        if self.is_pivot() {
            if parent.can_update {
                grid = grid
                    .bulk_action(
                        Action::new(&texts.get("detach", &[]), &format!("{url}/actions/remove"))
                            .confirm(&texts.get("detach_question", &[("label", &label)]))
                            .danger(),
                    )
                    .row_action(
                        Action::new(&texts.get("detach", &[]), &format!("{url}/{{id}}"))
                            .method("DELETE")
                            .confirm(&texts.get("detach_question", &[("label", &label)]))
                            .danger(),
                    );
            }
            return grid;
        }
        if self.allows(user, "deleteAny", None) {
            grid = grid.bulk_action(
                Action::new(&texts.get("delete", &[]), &format!("{url}/actions/remove"))
                    .confirm(&texts.get("bulk_delete_hard", &[("plural", &plural)]))
                    .danger(),
            );
        }
        if self.allows(user, "update", None) {
            grid = grid
                .row_url(&format!("{url}/{{id}}/edit"))
                .row_action(Action::link(
                    &texts.get("edit", &[]),
                    &format!("{url}/{{id}}/edit"),
                ));
        }
        if self.allows(user, "delete", None) {
            grid = grid.row_action(
                Action::new(&texts.get("delete", &[]), &format!("{url}/{{id}}"))
                    .method("DELETE")
                    .confirm(&texts.get("delete_question", &[("label", &label)]))
                    .danger(),
            );
        }
        grid
    }

    /// The layout's values and the child's `resource`, for a page under
    /// the record.
    fn page(
        &self,
        state: &AppState,
        user: &AuthUser,
        parent: &ParentRecord,
        texts: &Texts,
        name: &str,
        mut values: Map<String, Value>,
    ) -> Result<View> {
        let slug = self.child.slug();
        let url = self.url(&parent.id);
        values.insert(
            "admin".into(),
            to_json(
                &self
                    .parent
                    .panel()
                    .frame(state, user, Some(self.parent.slug()), texts),
            )?,
        );
        values.insert(
            "resource".into(),
            json!({
                "slug": slug,
                "label": texts.label(slug, self.child.label()),
                "plural_label": texts.label(slug, self.child.plural_label()),
                "index_url": url,
                "create_url": format!("{url}/create"),
                "has_view": false,
            }),
        );
        Ok(view(name, Value::Object(values)))
    }
}

async fn open<C: AdminResource>(
    cx: &RelCtx<C>,
    state: &AppState,
    user: &AuthUser,
    id: &str,
    texts: &Texts,
) -> Result<ParentRecord> {
    cx.parent.open(state, user, id, texts).await
}

async fn index<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    request: GridRequest,
) -> Result<View> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    cx.authorize(&user, "viewAny", None)?;
    let grid = cx.grid(&parent, &user, &texts);
    let rows = grid.page(cx.scoped(&parent), &request).await?;
    let slug = cx.child.slug();
    let url = cx.url(&parent.id);
    let label = texts.label(slug, cx.child.label());
    let tabs = cx.parent.tabs(&user, &texts, &parent.id);
    let mut values = Map::new();
    values.insert("rows".into(), to_json(&rows)?);
    values.insert("tabs".into(), to_json(&tabs)?);
    values.insert("current_tab".into(), Value::String(url.clone()));
    let (parent_label, parent_plural) = cx.parent.names(&texts);
    values.insert(
        "parent".into(),
        json!({
            "title": parent.title,
            "index_url": cx.parent.base(),
            "plural_label": parent_plural,
            "label": parent_label,
        }),
    );
    values.insert(
        "relation".into(),
        json!({
            "label": texts.label(cx.parent.slug(), &cx.spec.label),
            "url": url,
            "child_slug": slug,
        }),
    );
    if cx.is_pivot() {
        if parent.can_update && cx.allows(&user, "viewAny", None) {
            let options = attach_options(&cx, &state, &parent).await?;
            values.insert(
                "attach".into(),
                json!({
                    "label": texts.get("attach", &[("label", &lower(&label))]),
                    "none": texts.get("attach_none", &[]),
                    "field": label,
                    "options": options,
                }),
            );
        }
    } else if cx.allows(&user, "create", None) {
        values.insert(
            "new".into(),
            json!({
                "label": texts.get("new", &[("label", &lower(&label))]),
                "url": format!("{url}/create"),
            }),
        );
    }
    cx.page(
        &state,
        &user,
        &parent,
        &texts,
        "renox-admin/relation.html",
        values,
    )
}

/// The rows not attached yet, as `(id, title)` choices.
async fn attach_options<C: AdminResource>(
    cx: &RelCtx<C>,
    state: &AppState,
    parent: &ParentRecord,
) -> Result<Vec<(String, String)>> {
    let Kind::Pivot {
        table,
        parent_key,
        related_key,
    } = &cx.spec.kind
    else {
        return Ok(Vec::new());
    };
    let rows = cx
        .child
        .query()
        .where_raw(
            &format!(
                "\"{}\".\"id\" NOT IN (SELECT \"{related_key}\" FROM \"{table}\" WHERE \"{parent_key}\" = ?)",
                C::Model::TABLE
            ),
            [parent.key.clone()],
        )
        .limit(MAX_OPTIONS)
        .get(&state.db)
        .await?;
    let mut options = Vec::with_capacity(rows.len());
    for row in &rows {
        let json = to_json(row)?;
        let title = match json.get(&cx.spec.title) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Null) | None => row.id().to_string(),
            Some(other) => other.to_string(),
        };
        options.push((row.id().to_string(), title));
    }
    options.sort_by_key(|option| option.1.to_lowercase());
    Ok(options)
}

async fn create<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<View> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    cx.authorize(&user, "create", None)?;
    let hidden = hidden_names(&cx);
    let fields = form_fields(&*cx.child, &state, None, &texts, &hidden).await?;
    let label = lower(&texts.label(cx.child.slug(), cx.child.label()));
    let mut values = Map::new();
    values.insert(
        "title".into(),
        Value::String(texts.get("new", &[("label", &label)])),
    );
    values.insert("fields".into(), to_json(&fields)?);
    values.insert("action".into(), Value::String(cx.url(&parent.id)));
    values.insert("method".into(), Value::String("POST".into()));
    values.insert(
        "submit_label".into(),
        Value::String(texts.get("create", &[("label", &label)])),
    );
    values.insert("record".into(), Value::Null);
    cx.page(
        &state,
        &user,
        &parent,
        &texts,
        "renox-admin/form.html",
        values,
    )
}

/// The fields a form leaves out: the foreign key the panel fills in.
fn hidden_names<C>(cx: &RelCtx<C>) -> Vec<&str> {
    match &cx.spec.kind {
        Kind::HasMany { foreign_key } => vec![foreign_key.as_str()],
        Kind::Pivot { .. } => Vec::new(),
    }
}

async fn store<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<String>,
    req: Request,
) -> Result<Response> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    cx.authorize(&user, "create", None)?;
    let form: C::Form =
        match read_form(&state, req, &money_fields(&*cx.child), &cx.fixed(&parent)).await {
            Ok(form) => form,
            Err(answer) => return Ok(*answer),
        };
    check(&state.db, rules_of(&*cx.child, &form, None)).await?;
    let mut record = C::Model::default();
    cx.child.fill(&mut record, form);
    record.save(&state.db).await?;
    let saved = SaveContext {
        state: state.clone(),
        user: user.clone(),
        created: true,
        previous: None,
    };
    cx.child.saved(&record, &saved).await?;
    let label = texts.label(cx.child.slug(), cx.child.label());
    let toast = Toast::success(texts.get("created", &[("label", &label)]));
    Ok(done(&htmx, toast, &cx.url(&parent.id)))
}

async fn edit<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, rid)): Path<(String, String)>,
) -> Result<View> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    let record = cx.find(&state.db, &parent, &rid).await?;
    cx.authorize(&user, "update", Some(&record))?;
    let json = to_json(&record)?;
    let hidden = hidden_names(&cx);
    let fields = form_fields(&*cx.child, &state, Some(&json), &texts, &hidden).await?;
    let url = cx.url(&parent.id);
    let rid = record.id().to_string();
    let label = texts.label(cx.child.slug(), cx.child.label());
    let title = record_title(&*cx.child, &record, &texts);
    let mut values = Map::new();
    values.insert(
        "title".into(),
        Value::String(texts.get("edit_title", &[("title", &title)])),
    );
    values.insert("fields".into(), to_json(&fields)?);
    values.insert("action".into(), Value::String(format!("{url}/{rid}")));
    values.insert("method".into(), Value::String("PUT".into()));
    values.insert(
        "submit_label".into(),
        Value::String(texts.get("save_changes", &[])),
    );
    values.insert("record".into(), json);
    if cx.allows(&user, "delete", Some(&record)) {
        values.insert(
            "delete".into(),
            json!({ "url": format!("{url}/{rid}"), "label": label, "soft": false }),
        );
    }
    cx.page(
        &state,
        &user,
        &parent,
        &texts,
        "renox-admin/form.html",
        values,
    )
}

async fn update<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path((id, rid)): Path<(String, String)>,
    req: Request,
) -> Result<Response> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    let mut record = cx.find(&state.db, &parent, &rid).await?;
    cx.authorize(&user, "update", Some(&record))?;
    let form: C::Form =
        match read_form(&state, req, &money_fields(&*cx.child), &cx.fixed(&parent)).await {
            Ok(form) => form,
            Err(answer) => return Ok(*answer),
        };
    check(&state.db, rules_of(&*cx.child, &form, Some(&record))).await?;
    let previous = to_json(&record)?;
    cx.child.fill(&mut record, form);
    record.save(&state.db).await?;
    let saved = SaveContext {
        state: state.clone(),
        user: user.clone(),
        created: false,
        previous: Some(previous),
    };
    cx.child.saved(&record, &saved).await?;
    let label = texts.label(cx.child.slug(), cx.child.label());
    let toast = Toast::success(texts.get("saved", &[("label", &label)]));
    Ok(done(&htmx, toast, &cx.url(&parent.id)))
}

/// Deletes a has-many row, or detaches a pivot's.
async fn remove<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path((id, rid)): Path<(String, String)>,
) -> Result<Response> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    let mut record = cx.find(&state.db, &parent, &rid).await?;
    let label = texts.label(cx.child.slug(), cx.child.label());
    let toast = if cx.is_pivot() {
        cx.pivot_write(&parent)?;
        detach(&cx, &state, &parent, &[record.id().to_db_value()]).await?;
        Toast::success(texts.get("detached", &[("label", &label)]))
    } else {
        cx.authorize(&user, "delete", Some(&record))?;
        record.delete(&state.db).await?;
        Toast::success(texts.get("deleted", &[("label", &label)]))
    };
    if htmx.request {
        Ok(toast.into_response())
    } else {
        Ok(done(&htmx, toast, &cx.url(&parent.id)))
    }
}

/// Removes the pivot rows joining the record to `related`.
async fn detach<C: AdminResource>(
    cx: &RelCtx<C>,
    state: &AppState,
    parent: &ParentRecord,
    related: &[DbValue],
) -> Result {
    let Kind::Pivot {
        table,
        parent_key,
        related_key,
    } = &cx.spec.kind
    else {
        return Ok(());
    };
    let mut tx = state.db.begin().await?;
    for value in related {
        renox::db::sql(format!(
            "DELETE FROM \"{table}\" WHERE \"{parent_key}\" = ? AND \"{related_key}\" = ?"
        ))
        .bind(parent.key.clone())
        .bind(value.clone())
        .execute(&mut tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn attach<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<String>,
    body: renox::axum::body::Bytes,
) -> Result<Response> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    cx.pivot_write(&parent)?;
    cx.authorize(&user, "viewAny", None)?;
    let Kind::Pivot {
        table,
        parent_key,
        related_key,
    } = &cx.spec.kind
    else {
        return Err(Error::NotFound);
    };
    let label = texts.label(cx.child.slug(), cx.child.label());
    let chosen = parse_pairs(&body)
        .into_iter()
        .rev()
        .find(|(name, _)| name == "attach")
        .map(|(_, value)| value)
        .unwrap_or_default();
    let mut errors = Errors::new();
    // Read the key before awaiting: a parse error needn't be `Send`.
    let key = chosen.parse::<<C::Model as Model>::Key>().ok();
    let row = match key {
        Some(key) if !chosen.is_empty() => {
            cx.child
                .query()
                .where_eq("id", key)
                .first(&state.db)
                .await?
        }
        _ => None,
    };
    let Some(row) = row else {
        errors.add("attach", texts.get("choice", &[("field", &label)]));
        return Err(ValidationError::new(errors).into());
    };
    let related = row.id().to_db_value();
    let already = renox::db::sql(format!(
        "SELECT COUNT(*) FROM \"{table}\" WHERE \"{parent_key}\" = ? AND \"{related_key}\" = ?"
    ))
    .bind(parent.key.clone())
    .bind(related.clone())
    .scalar::<i64>(&state.db)
    .await?;
    if already == 0 {
        renox::db::sql(format!(
            "INSERT INTO \"{table}\" (\"{parent_key}\", \"{related_key}\") VALUES (?, ?)"
        ))
        .bind(parent.key.clone())
        .bind(related)
        .execute(&state.db)
        .await?;
    }
    let toast = Toast::success(texts.get("attached", &[("label", &label)]));
    if htmx.request {
        Ok((toast, HxRefresh).into_response())
    } else {
        Ok(done(&htmx, toast, &cx.url(&parent.id)))
    }
}

/// The grid's bulk action: deletes has-many rows, detaches a pivot's.
async fn bulk_remove<C: AdminResource>(
    Extension(cx): Extension<RelCtx<C>>,
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    request: GridRequest,
    body: renox::axum::body::Bytes,
) -> Result<Toast> {
    let texts = Texts::new(&state);
    let parent = open(&cx, &state, &user, &id, &texts).await?;
    if cx.is_pivot() {
        cx.pivot_write(&parent)?;
    } else {
        cx.authorize(&user, "deleteAny", None)?;
    }
    let mut selection = Selection::default();
    for (name, value) in parse_pairs(&body) {
        match name.as_str() {
            "ids" => {
                selection.ids = value
                    .split(',')
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "all" => selection.all = value == "true",
            _ => {}
        }
    }
    let grid = cx.grid(&parent, &user, &texts);
    let mut records = grid
        .selected(cx.scoped(&parent), &request, &selection)?
        .limit(10_000)
        .get(&state.db)
        .await?;
    if records.is_empty() {
        return Ok(Toast::info(texts.get("nothing_selected", &[])));
    }
    let count = records.len();
    let slug = cx.child.slug();
    let what = if count == 1 {
        format!("1 {}", lower(&texts.label(slug, cx.child.label())))
    } else {
        format!(
            "{count} {}",
            lower(&texts.label(slug, cx.child.plural_label()))
        )
    };
    if cx.is_pivot() {
        let related: Vec<DbValue> = records.iter().map(|r| r.id().to_db_value()).collect();
        detach(&cx, &state, &parent, &related).await?;
        return Ok(Toast::success(texts.get("n_detached", &[("what", &what)])));
    }
    if records
        .iter()
        .any(|record| !cx.child.allows(&user, "delete", Some(record)))
    {
        return Err(Error::Forbidden);
    }
    let mut tx = state.db.begin().await?;
    for record in &mut records {
        record.delete(&mut tx).await?;
    }
    tx.commit().await?;
    Ok(Toast::success(texts.get("n_deleted", &[("what", &what)])))
}

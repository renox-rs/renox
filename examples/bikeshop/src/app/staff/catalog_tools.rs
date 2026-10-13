//! Two catalogue pages next to the admin panel (#239):
//!
//! - **Move to a category** (`staff.catalog.move`): the products panel's
//!   bulk action "Move to a category…" keeps the selection in the cache for
//!   half an hour and answers with a toast linking here, where the category
//!   is picked. (Written before `renox-admin` actions could take input.)
//! - **What fits** (`staff.catalog.fits`): which bike models a part fits
//!   (or, from a bike, which parts fit it), with a note: the `part_fits`
//!   many-to-many (`Pivot` and its `inverse()`, Pagila's `film_actor`),
//!   which a panel form can't edit. Linked from the panel's products list.
//!
//! Both need `catalog.manage`; changes are in the audit log.

use renox::prelude::*;
use renox_admin::ActionContext;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::audit;
use crate::app::access::active_store;
use crate::app::access::catalogue::CATALOG_MANAGE;
use crate::app::catalog::model::{Category, CategoryKind, FITTING_PARTS, Fit, PART_FITS, Product};
use renox::db::relations::Pivot;

/// How long a "move" selection waits for its category.
const MOVE_FOR: Duration = Duration::from_secs(30 * 60);

/// The routes (`catalog.manage`).
pub fn routes() -> Routes {
    active_store::staff_routes(
        Routes::new()
            .get("/staff/catalog/move/{token}", move_page)
            .name("staff.catalog.move")
            .post("/staff/catalog/move/{token}", move_products)
            .name("staff.catalog.move.store")
            .get("/staff/catalog/fits/{product}", fits)
            .name("staff.catalog.fits")
            .post("/staff/catalog/fits/{product}", add_fit)
            .name("staff.catalog.fits.store")
            .delete("/staff/catalog/fits/{product}/{other}", remove_fit)
            .name("staff.catalog.fits.destroy")
            .require_permission(CATALOG_MANAGE),
    )
}

/// What waits in the cache.
#[derive(Serialize, Deserialize, Debug)]
struct Selection {
    user_id: i64,
    products: Vec<i64>,
}

fn key(token: &str) -> String {
    format!("bikeshop:move-category:{token}")
}

// [explain:staff.catalog.move.action]
/// The panel's "Move to a category…" action: keeps the selection, links here.
pub async fn start_move(products: Vec<Product>, cx: ActionContext) -> Result<Toast> {
    let token = renox::random_token();
    let selection = Selection {
        user_id: cx.user.id,
        products: products.iter().map(|p| p.id).collect(),
    };
    cx.state
        .cache
        .put(&key(&token), &selection, Some(MOVE_FOR))
        .await?;
    let url = cx.state.url("staff.catalog.move", &[&token])?;
    let lang = cx.state.current_lang();
    let count = products.len().to_string();
    Ok(
        Toast::info(lang.t("staff.catalog.selected", &[("count", &count)]))
            .link(lang.t("staff.catalog.move_link", &[]), url)
            .persistent(),
    )
}
// [/explain:staff.catalog.move.action]

async fn selection(state: &AppState, user: &User, token: &str) -> Result<Selection> {
    match state.cache.get::<Selection>(&key(token)).await? {
        Some(s) if s.user_id == user.id => Ok(s),
        _ => Err(Error::NotFound),
    }
}

/// `GET /staff/catalog/move/{token}`: the selected products and a category select.
pub async fn move_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(token): Path<String>,
) -> Result<View> {
    let chosen = selection(&state, &user, &token).await?;
    let products = Product::query()
        .where_(Product::ID.is_in(chosen.products))
        .order_by(Product::NAME)
        .get(&state.db)
        .await?;
    let categories: Vec<(String, String)> = Category::query()
        .order_by(Category::NAME)
        .get(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id.to_string(), c.name))
        .collect();
    Ok(view(
        "staff/catalog/move.html",
        context! { products, categories, token },
    ))
}

/// The category to move to.
#[derive(Deserialize, Validate)]
pub struct MoveForm {
    #[validate(required, exists("categories", "id"))]
    pub category_id: i64,
}

// [explain:staff.catalog.move.handler]
/// `POST /staff/catalog/move/{token}`: moves them and goes back to the panel.
pub async fn move_products(
    State(state): State<AppState>,
    user: AuthUser,
    Path(token): Path<String>,
    Valid(form): Valid<MoveForm>,
) -> Result<(Toast, Redirect)> {
    let chosen = selection(&state, &user, &token).await?;
    let moved = Product::query()
        .where_(Product::ID.is_in(chosen.products.clone()))
        .update(&state.db, &[("category_id", &form.category_id)])
        .await?;
    state.cache.forget(&key(&token)).await?;
    audit::record(&state.db, &user, CATALOG_MANAGE, "catalog.category_moved")
        .subject("categories", form.category_id)
        .data(json!({ "products": chosen.products }))
        .save()
        .await?;
    // [/explain:staff.catalog.move.handler]
    Ok((
        Toast::success(
            state
                .current_lang()
                .t("staff.catalog.moved", &[("count", &moved)]),
        ),
        Redirect::to(&state.url("admin.products.index", &[])?),
    ))
}

/// The product with its category.
async fn product(db: &Db, id: i64) -> Result<(Product, Category)> {
    let product = Product::find_or_404(db, id).await?;
    let category = Category::find_or_404(db, product.category_id).await?;
    Ok((product, category))
}

// [explain:staff.catalog.fits.pivot]
/// The pivot seen from this product (a part → bikes, a bike → parts), and
/// the kind of product on the other side; `None` for gear.
fn side(kind: CategoryKind) -> Option<(Pivot, CategoryKind)> {
    match kind {
        CategoryKind::Part => Some((PART_FITS, CategoryKind::Bike)),
        CategoryKind::Bike => Some((FITTING_PARTS, CategoryKind::Part)),
        CategoryKind::Gear => None,
    }
}
// [/explain:staff.catalog.fits.pivot]

// [explain:staff.catalog.fits.pivot]
/// `GET /staff/catalog/fits/{product}`: for a part, the bike models it
/// fits; for a bike, the parts that fit it; each with its note, and a form
/// to add one. Three queries.
pub async fn fits(State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
    let (product, category) = product(&db, id).await?;
    let Some((pivot, other)) = side(category.kind) else {
        return Ok(view(
            "staff/catalog/fits.html",
            context! { product, category, gear => true, fitted => Vec::<()>::new(), choices => Vec::<()>::new() },
        ));
    };
    let fitted: Vec<(Product, Fit)> = pivot
        .load_with_pivot::<Product, Fit>(&db, [product.id])
        .await?
        .remove(&product.id)
        .unwrap_or_default();
    let fitted_ids: Vec<i64> = fitted.iter().map(|(p, _)| p.id).collect();
    // [/explain:staff.catalog.fits.pivot]
    let choices: Vec<(String, String)> = Product::query()
        .where_raw(
            "category_id IN (SELECT id FROM categories WHERE kind = ?)",
            vec![renox::db::ToDbValue::to_db_value(&other)],
        )
        .order_by(Product::NAME)
        .get(&db)
        .await?
        .into_iter()
        .filter(|p| !fitted_ids.contains(&p.id))
        .map(|p| (p.id.to_string(), p.name))
        .collect();
    let fitted: Vec<_> = fitted
        .into_iter()
        .map(|(other, fit)| json!({ "product": other, "note": fit.note }))
        .collect();
    Ok(view(
        "staff/catalog/fits.html",
        context! { product, category, gear => false, part => category.kind == CategoryKind::Part, fitted, choices },
    ))
}

/// A product on the other side, with a note.
#[derive(Deserialize, Validate)]
pub struct FitForm {
    #[validate(required, exists("products", "id"))]
    pub other_id: i64,
    #[validate(max = 200)]
    pub note: Option<String>,
}

// [explain:staff.catalog.fits.attach]
/// `POST /staff/catalog/fits/{product}`.
pub async fn add_fit(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<FitForm>,
) -> Result<(Toast, Redirect)> {
    let (product, category) = product(&state.db, id).await?;
    let (pivot, _) = side(category.kind).ok_or(Error::NotFound)?;
    let note = form.note.filter(|n| !n.trim().is_empty());
    pivot
        .attach_with(&state.db, product.id, form.other_id, &[("note", &note)])
        .await?;
    // [/explain:staff.catalog.fits.attach]
    audit::record(&state.db, &user, CATALOG_MANAGE, "catalog.fit_added")
        .subject("products", product.id)
        .data(json!({ "with": form.other_id }))
        .save()
        .await?;
    Ok((
        Toast::success(state.current_lang().t("staff.catalog.fit_added", &[])),
        Redirect::to(&state.url("staff.catalog.fits", &[&product.id])?),
    ))
}

/// `DELETE /staff/catalog/fits/{product}/{other}`.
pub async fn remove_fit(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, other)): Path<(i64, i64)>,
) -> Result<(Toast, Redirect)> {
    let (product, category) = product(&state.db, id).await?;
    let (pivot, _) = side(category.kind).ok_or(Error::NotFound)?;
    pivot.detach(&state.db, product.id, [other]).await?;
    audit::record(&state.db, &user, CATALOG_MANAGE, "catalog.fit_removed")
        .subject("products", product.id)
        .data(json!({ "with": other }))
        .save()
        .await?;
    Ok((
        Toast::success(state.current_lang().t("staff.catalog.fit_removed", &[])),
        Redirect::to(&state.url("staff.catalog.fits", &[&product.id])?),
    ))
}

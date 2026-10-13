//! The cart: in the session for guests, in the `carts` table for customers
//! who are logged in, merged at their first visit after logging in.
//!
//! A cart belongs to one store: the store whose stock it is checked
//! against, where the order is picked up (or sent from). Each line's
//! quantity is held to what that store has **available**: on hand minus
//! reserved, summed over every owner of the goods there (consigned goods
//! count, #245). When a line no longer fits (someone else bought them), the
//! cart page lowers it and says so.
//!
//! | Route | Name | What |
//! |---|---|---|
//! | `GET /cart` | `cart.show` | the cart page |
//! | `GET /cart/mini` | `cart.mini` | the navbar's cart link with its count (htmx) |
//! | `POST /cart` | `cart.add` | add a variant (the product page) |
//! | `PATCH /cart/{variant}` | `cart.update` | change a quantity |
//! | `DELETE /cart/{variant}` | `cart.remove` | take a line out |
//! | `POST /cart/store` | `cart.store` | pick the store |
//!
//! With htmx, a change answers the cart's `lines` block plus the navbar's
//! `mini` block, which htmx swaps out of band (`View::also`), and a toast.

use std::collections::HashMap;

use renox::db::Json;
use renox::db::relations::has_many;
use renox::prelude::*;
use renox::validation::{FormContext, ValidateHooks};
use renox::{ToastAction, validation::Errors};
use serde::{Deserialize, Serialize};

use super::model::SavedCart;
use crate::app::catalog::browse::photo_url;
use crate::app::catalog::model::{Category, CategoryKind, Product, ProductPhoto, ProductVariant};
use crate::app::staff::model::Store;
use crate::app::stock::model::StockLevel;

/// The session key of a guest's cart.
pub const SESSION_KEY: &str = "cart";
/// The most of one variant a cart takes.
pub const MAX_QUANTITY: i64 = 20;
/// The most lines a cart takes.
pub const MAX_LINES: usize = 30;

/// One line: a variant and how many.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CartLine {
    pub variant_id: i64,
    pub quantity: i64,
}

/// A cart: its store and its lines.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Cart {
    /// The store it is checked against and picked up from; `None` until
    /// chosen (the first store is used).
    pub store_id: Option<i64>,
    pub lines: Vec<CartLine>,
}

impl Cart {
    /// The number of items (the navbar's count).
    pub fn count(&self) -> i64 {
        self.lines.iter().map(|l| l.quantity).sum()
    }

    /// Adds `quantity` of `variant_id` (to its line, if there is one).
    pub fn add(&mut self, variant_id: i64, quantity: i64) {
        if let Some(line) = self.lines.iter_mut().find(|l| l.variant_id == variant_id) {
            line.quantity = (line.quantity + quantity).min(MAX_QUANTITY);
        } else if self.lines.len() < MAX_LINES {
            self.lines.push(CartLine {
                variant_id,
                quantity: quantity.min(MAX_QUANTITY),
            });
        }
    }

    /// Sets a line's quantity; 0 or less takes it out.
    pub fn set(&mut self, variant_id: i64, quantity: i64) {
        if quantity <= 0 {
            self.lines.retain(|l| l.variant_id != variant_id);
        } else if let Some(line) = self.lines.iter_mut().find(|l| l.variant_id == variant_id) {
            line.quantity = quantity.min(MAX_QUANTITY);
        }
    }

    /// Another cart's lines added to this one (guest → account at login).
    pub fn merge(&mut self, other: Cart) {
        if self.store_id.is_none() {
            self.store_id = other.store_id;
        }
        for line in other.lines {
            self.add(line.variant_id, line.quantity);
        }
    }

    // [explain:cart.show.load]
    /// The cart of this visitor: the account's saved cart for a logged-in
    /// customer (with the session's guest cart merged into it, once), else
    /// the session's.
    pub async fn load(db: &Db, session: &Session, user: Option<&User>) -> Result<Cart> {
        let guest: Option<Cart> = session.get(SESSION_KEY);
        let Some(user) = user else {
            return Ok(guest.unwrap_or_default());
        };
        let saved = SavedCart::query()
            .where_(SavedCart::USER_ID.eq(user.id))
            .first(db)
            .await?;
        let mut cart = saved
            .map(|s| Cart {
                store_id: s.store_id,
                lines: s.lines.0,
            })
            .unwrap_or_default();
        if let Some(guest) = guest {
            // The cart filled before logging in joins the saved one.
            cart.merge(guest);
            session.remove(SESSION_KEY);
            cart.save(db, session, Some(user)).await?;
        }
        Ok(cart)
    }
    // [/explain:cart.show.load]

    /// Stores the cart where [`Cart::load`] finds it.
    pub async fn save(&self, db: &Db, session: &Session, user: Option<&User>) -> Result {
        let Some(user) = user else {
            return session.put(SESSION_KEY, self);
        };
        let now = renox::db::now();
        renox::db::sql(
            "INSERT INTO carts (user_id, store_id, lines, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?) ON CONFLICT (user_id) DO UPDATE SET \
             store_id = excluded.store_id, lines = excluded.lines, updated_at = excluded.updated_at",
        )
        .bind(user.id)
        .bind(self.store_id)
        .bind(Json(self.lines.clone()))
        .bind(now)
        .bind(now)
        .execute(db)
        .await?;
        Ok(())
    }

    /// Empties it (after the order is placed).
    pub async fn clear(db: &Db, session: &Session, user: Option<&User>) -> Result {
        session.remove(SESSION_KEY);
        if let Some(user) = user {
            SavedCart::query()
                .where_(SavedCart::USER_ID.eq(user.id))
                .delete(db)
                .await?;
        }
        Ok(())
    }
}

/// What a store has available of each of these variants: on hand minus
/// reserved, over every owner of the goods there. One query.
pub async fn available_at(
    db: &Db,
    store_id: i64,
    variant_ids: &[i64],
) -> Result<HashMap<i64, i64>> {
    let mut available = HashMap::new();
    if variant_ids.is_empty() {
        return Ok(available);
    }
    for level in StockLevel::query()
        .where_(StockLevel::LOCATION_STORE_ID.eq(store_id))
        .where_(StockLevel::VARIANT_ID.is_in(variant_ids.to_vec()))
        .get(db)
        .await?
    {
        *available.entry(level.variant_id).or_insert(0) += level.available().max(0);
    }
    Ok(available)
}

/// A line as the cart page shows it.
#[derive(Serialize, Debug, Clone)]
pub struct LineView {
    pub variant: ProductVariant,
    pub product: Product,
    pub photo: Option<String>,
    pub quantity: i64,
    /// What the store has available now.
    pub available: i64,
    pub total: i64,
    /// Lowered to what's available on this visit.
    pub lowered: bool,
    /// A spare part (the plan discount applies to these).
    pub part: bool,
}

/// The cart with everything its page shows.
#[derive(Serialize, Debug, Clone)]
pub struct CartView {
    pub lines: Vec<LineView>,
    pub count: i64,
    pub subtotal: i64,
    pub store: Option<Store>,
    pub stores: Vec<Store>,
    /// The stores as a select's options: `[id as text, name]`.
    pub store_options: Vec<[String; 2]>,
    /// Some lines were lowered (or taken out) because the store has fewer.
    pub changed: bool,
}

impl CartView {
    /// Loads the cart's variants, products, photos, categories and the
    /// store's stock: six queries however long the cart. Lines the store
    /// can't fill any more are lowered in `cart` (the caller saves it when
    /// `changed`); lines whose product was discontinued are taken out.
    pub async fn load(db: &Db, cart: &mut Cart) -> Result<CartView> {
        let stores = Store::query().order_by(Store::ID).get(db).await?;
        if cart
            .store_id
            .is_none_or(|id| !stores.iter().any(|s| s.id == id))
        {
            cart.store_id = stores.first().map(|s| s.id);
        }
        let store = stores.iter().find(|s| Some(s.id) == cart.store_id).cloned();
        let ids: Vec<i64> = cart.lines.iter().map(|l| l.variant_id).collect();
        let variants: HashMap<i64, ProductVariant> = ProductVariant::find_many(db, ids.clone())
            .await?
            .into_iter()
            .map(|v| (v.id, v))
            .collect();
        let products: Vec<Product> = Product::find_many(
            db,
            variants.values().map(|v| v.product_id).collect::<Vec<_>>(),
        )
        .await?;
        let kinds: HashMap<i64, CategoryKind> = Category::find_many(
            db,
            products.iter().map(|p| p.category_id).collect::<Vec<_>>(),
        )
        .await?
        .into_iter()
        .map(|c| (c.id, c.kind))
        .collect();
        let mut photos: HashMap<i64, Vec<ProductPhoto>> = has_many(
            db,
            &products,
            ProductPhoto::query()
                .order_by(ProductPhoto::POSITION)
                .order_by(ProductPhoto::ID),
            "product_id",
            |p| p.product_id,
        )
        .await?;
        let available = match store.as_ref() {
            Some(s) => available_at(db, s.id, &ids).await?,
            None => HashMap::new(),
        };
        let mut changed = false;
        let mut lines = Vec::new();
        for line in cart.lines.iter_mut() {
            // A discontinued product (soft deleted) isn't found: the line goes.
            let Some(variant) = variants.get(&line.variant_id) else {
                line.quantity = 0;
                changed = true;
                continue;
            };
            let Some(product) = products.iter().find(|p| p.id == variant.product_id) else {
                line.quantity = 0;
                changed = true;
                continue;
            };
            let left = available.get(&variant.id).copied().unwrap_or(0);
            let lowered = line.quantity > left;
            if lowered {
                line.quantity = left;
                changed = true;
            }
            if line.quantity == 0 {
                continue;
            }
            lines.push(LineView {
                photo: photos
                    .get_mut(&product.id)
                    .and_then(|p| p.first())
                    .map(|p| photo_url(&p.path)),
                part: kinds.get(&product.category_id) == Some(&CategoryKind::Part),
                total: variant.price * line.quantity,
                quantity: line.quantity,
                available: left,
                lowered,
                variant: variant.clone(),
                product: product.clone(),
            });
        }
        cart.lines.retain(|l| l.quantity > 0);
        Ok(CartView {
            count: cart.count(),
            subtotal: lines.iter().map(|l| l.total).sum(),
            lines,
            store,
            store_options: stores
                .iter()
                .map(|s| [s.id.to_string(), s.name.clone()])
                .collect(),
            stores,
            changed,
        })
    }
}

/// `GET /cart` (`cart.show`).
pub async fn show(State(db): State<Db>, session: Session, user: Option<AuthUser>) -> Result<View> {
    let user = user.as_deref();
    let mut cart = Cart::load(&db, &session, user).await?;
    let view_data = CartView::load(&db, &mut cart).await?;
    if view_data.changed {
        cart.save(&db, &session, user).await?;
    }
    Ok(page(view_data))
}

fn page(cart: CartView) -> View {
    view("sales/cart/show.html", context! { cart, oob => true }).fragment("lines")
}

/// `GET /cart/mini` (`cart.mini`): the navbar's cart link with its count,
/// asked for by the navbar when a page loads (pages stay the same for
/// everyone, so their ETags work).
pub async fn mini(State(db): State<Db>, session: Session, user: Option<AuthUser>) -> Result<View> {
    let cart = Cart::load(&db, &session, user.as_deref()).await?;
    Ok(view(
        "sales/cart/_mini.html",
        context! { count => cart.count(), oob => false },
    ))
}

/// The add-to-cart form (the product page).
#[derive(Deserialize, Validate, Debug)]
#[validate(hooks)]
pub struct AddForm {
    #[validate(required)]
    pub variant_id: i64,
    #[validate(between(1, 20))]
    pub quantity: i64,
}

impl ValidateHooks for AddForm {
    /// The variant must be one of a product still sold.
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let sold = match ProductVariant::find(&form.state.db, self.variant_id).await? {
            Some(v) => Product::find(&form.state.db, v.product_id).await?.is_some(),
            None => false,
        };
        if !sold {
            errors.add(
                "variant_id",
                form.state.current_lang().t("sales.cart.not_sold", &[]),
            );
        }
        Ok(())
    }
}

/// `POST /cart` (`cart.add`): adds a variant, up to what the cart's store
/// has available. With htmx: a toast (with "View cart") and the navbar's
/// count, swapped out of band; without: back to the page, with the toast.
pub async fn add(
    State(db): State<Db>,
    session: Session,
    user: Option<AuthUser>,
    lang: Lang,
    htmx: Htmx,
    back: Back,
    Valid(form): Valid<AddForm>,
) -> Result<Response> {
    let user = user.as_deref();
    let mut cart = Cart::load(&db, &session, user).await?;
    let stores = Store::query().order_by(Store::ID).get(&db).await?;
    // A cart's first line picks its store: the first that has the variant.
    let store = match cart
        .store_id
        .and_then(|id| stores.iter().find(|s| s.id == id))
    {
        Some(store) => store.clone(),
        None => {
            let mut chosen = None;
            for store in &stores {
                if available_at(&db, store.id, &[form.variant_id])
                    .await?
                    .get(&form.variant_id)
                    .is_some_and(|n| *n > 0)
                {
                    chosen = Some(store.clone());
                    break;
                }
            }
            chosen.or_else(|| stores.first().cloned()).ok_or_else(|| {
                abort(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The shop has no store yet.",
                )
            })?
        }
    };
    cart.store_id = Some(store.id);
    let available = available_at(&db, store.id, &[form.variant_id])
        .await?
        .get(&form.variant_id)
        .copied()
        .unwrap_or(0);
    let in_cart = cart
        .lines
        .iter()
        .find(|l| l.variant_id == form.variant_id)
        .map_or(0, |l| l.quantity);
    let wanted = form.quantity;
    let added = wanted.min(available - in_cart).max(0);
    let variant = ProductVariant::find_or_404(&db, form.variant_id).await?;
    let product = Product::find_or_404(&db, variant.product_id).await?;
    let toast = if added == 0 {
        Toast::warning(lang.t(
            "sales.cart.none_left",
            &[("name", &product.name), ("store", &store.name)],
        ))
    } else {
        cart.add(form.variant_id, added);
        cart.save(&db, &session, user).await?;
        let toast = if added < wanted {
            Toast::warning(lang.t(
                "sales.cart.only_left",
                &[
                    ("count", &added.to_string()),
                    ("name", &product.name),
                    ("store", &store.name),
                ],
            ))
        } else {
            Toast::success(lang.t("sales.cart.added", &[("name", &product.name)]))
        };
        toast.action(ToastAction::link(lang.t("sales.cart.view", &[]), "/cart"))
    };
    if htmx.request {
        return Ok((
            toast,
            view(
                "sales/cart/_mini.html",
                context! { count => cart.count(), oob => true },
            ),
        )
            .into_response());
    }
    Ok((toast, back).into_response())
}

/// A new quantity for a line.
#[derive(Deserialize, Validate, Debug)]
pub struct QuantityForm {
    #[validate(between(0, 20))]
    pub quantity: i64,
}

// [explain:cart.show.htmx]
/// `PATCH /cart/{variant}` (`cart.update`): a new quantity, held to what
/// the store has.
pub async fn update(
    State(db): State<Db>,
    session: Session,
    user: Option<AuthUser>,
    lang: Lang,
    htmx: Htmx,
    Path(variant): Path<i64>,
    Valid(form): Valid<QuantityForm>,
) -> Result<Response> {
    let user = user.as_deref();
    let mut cart = Cart::load(&db, &session, user).await?;
    cart.set(variant, form.quantity);
    let data = CartView::load(&db, &mut cart).await?;
    cart.save(&db, &session, user).await?;
    let toast = if data.changed {
        Toast::warning(lang.t("sales.cart.lowered", &[]))
    } else {
        Toast::success(lang.t("sales.cart.updated", &[]))
    };
    answer(htmx, toast, data)
}
// [/explain:cart.show.htmx]

/// `DELETE /cart/{variant}` (`cart.remove`).
pub async fn remove(
    State(db): State<Db>,
    session: Session,
    user: Option<AuthUser>,
    lang: Lang,
    htmx: Htmx,
    Path(variant): Path<i64>,
) -> Result<Response> {
    let user = user.as_deref();
    let mut cart = Cart::load(&db, &session, user).await?;
    cart.set(variant, 0);
    let data = CartView::load(&db, &mut cart).await?;
    cart.save(&db, &session, user).await?;
    answer(htmx, Toast::info(lang.t("sales.cart.removed", &[])), data)
}

/// The store a cart is checked against and picked up from.
#[derive(Deserialize, Validate, Debug)]
pub struct StoreForm {
    #[validate(required, exists("stores", "id"))]
    pub store_id: i64,
}

/// `POST /cart/store` (`cart.store`): another store; the lines are checked
/// against its stock at once.
pub async fn store(
    State(db): State<Db>,
    session: Session,
    user: Option<AuthUser>,
    lang: Lang,
    htmx: Htmx,
    Valid(form): Valid<StoreForm>,
) -> Result<Response> {
    let user = user.as_deref();
    let mut cart = Cart::load(&db, &session, user).await?;
    cart.store_id = Some(form.store_id);
    let data = CartView::load(&db, &mut cart).await?;
    cart.save(&db, &session, user).await?;
    let name = data
        .store
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default();
    let toast = if data.changed {
        Toast::warning(lang.t("sales.cart.store_lowered", &[("store", &name)]))
    } else {
        Toast::success(lang.t("sales.cart.store_set", &[("store", &name)]))
    };
    answer(htmx, toast, data)
}

/// htmx: the `lines` block plus the navbar's count out of band, and the
/// toast; a plain form: back to the cart page with the toast.
// [explain:cart.show.htmx]
fn answer(htmx: Htmx, toast: Toast, data: CartView) -> Result<Response> {
    if htmx.wants_fragment() {
        return Ok((
            toast,
            view(
                "sales/cart/show.html",
                context! { cart => data, oob => true },
            )
            .fragment("lines")
            .also("mini"),
        )
            .into_response());
    }
    Ok((toast, Redirect::to("/cart")).into_response())
}
// [/explain:cart.show.htmx]

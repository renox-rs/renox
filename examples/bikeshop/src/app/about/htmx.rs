//! `/about/htmx`: htmx and Alpine recipes, live, on bike shop data (#351).
//!
//! A pre-ride checklist and the catalogue's bikes show the interactions
//! people usually reach for a JavaScript framework for, each a few htmx
//! attributes on the kit's components and a handler that answers with the
//! smallest piece of HTML that changes:
//!
//! - a modal form (the kit's `action_sheet`) that adds a row at the top;
//! - two places changing at once: the row and, out of band, the open count
//!   and the empty note (`view(…).fragment("row").also("count")`);
//! - the server picking where its answer goes (`HxRetarget` + `HxReswap`)
//!   when an item is already on the list, with a custom event (`HxTrigger`);
//! - inline edit (double-click, Escape cancels), a checkbox that toggles in
//!   place, a row menu whose Delete asks first (`hx-confirm`);
//! - tabs that filter the rows in the browser (Alpine, no request);
//! - `HxRefresh` after a change to many rows, `htmx.redirect` (HX-Redirect,
//!   or a 303 for a plain form) to another page, toasts that ride along or
//!   wait for the next page;
//! - infinite scroll over the catalogue (`hx-trigger="revealed"`).
//!
//! Every action also works as a plain form post (a redirect back), which
//! the tests check too. The checklist lives in the visitor's session (a few
//! short items, so the cookie stays small), so visitors never see each
//! other's and nothing needs cleaning up.

use renox::prelude::*;
use renox::{HxReswap, HxRetarget};
use serde::{Deserialize, Serialize};

use crate::app::catalog::model::Product;
use crate::explain::NotAPage;

/// The session key of the checklist.
const SESSION_KEY: &str = "htmx_checklist";

/// At most this many items: the session is a cookie.
pub const MAX_ITEMS: usize = 12;

/// Bikes per infinite-scroll load.
pub const PER_LOAD: u64 = 8;

/// One item of the checklist.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

/// The visitor's checklist, newest first.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Checklist {
    pub(super) next_id: u32,
    pub items: Vec<Item>,
}

impl Default for Checklist {
    /// What a first visit starts with.
    fn default() -> Self {
        let titles = [
            "Pump the tyres",
            "Check the brakes",
            "Oil the chain",
            "Charge the lights",
            "Pack a spare tube",
        ];
        Checklist {
            next_id: titles.len() as u32 + 1,
            items: titles
                .iter()
                .enumerate()
                .map(|(i, title)| Item {
                    id: i as u32 + 1,
                    title: (*title).to_owned(),
                    done: i == 0,
                })
                .collect(),
        }
    }
}

impl Checklist {
    /// The session's checklist, or a new one.
    pub fn of(session: &Session) -> Checklist {
        session.get(SESSION_KEY).unwrap_or_default()
    }

    pub fn save(&self, session: &Session) -> Result {
        session.put(SESSION_KEY, self)
    }

    /// How many items are still to do.
    pub fn open(&self) -> usize {
        self.items.iter().filter(|i| !i.done).count()
    }

    pub fn find(&mut self, id: u32) -> Result<&mut Item> {
        self.items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or(Error::NotFound)
    }
}

/// The routes of the page and its demos (added to `About::routes`).
pub fn routes() -> Routes {
    Routes::new()
        .get("/about/htmx", page)
        .name("about.htmx")
        .get("/about/htmx/bikes", bikes)
        .name("about.htmx.bikes")
        .post("/about/htmx/items", store)
        .name("about.htmx.store")
        .get("/about/htmx/items/{item}", row)
        .name("about.htmx.row")
        .get("/about/htmx/items/{item}/edit", edit)
        .name("about.htmx.edit")
        .patch("/about/htmx/items/{item}", update)
        .name("about.htmx.update")
        .patch("/about/htmx/items/{item}/toggle", toggle)
        .name("about.htmx.toggle")
        .delete("/about/htmx/items/{item}", destroy)
        .name("about.htmx.destroy")
        .post("/about/htmx/clear-done", clear_done)
        .name("about.htmx.clear_done")
        .post("/about/htmx/ready", ready)
        .name("about.htmx.ready")
}

/// The GET routes here that aren't pages.
pub fn not_pages() -> Vec<NotAPage> {
    vec![
        NotAPage {
            route: "about.htmx.bikes",
            reason: "an htmx fragment: the next bikes of the infinite scroll on /about/htmx",
        },
        NotAPage {
            route: "about.htmx.row",
            reason: "an htmx fragment: one checklist row, as the inline edit's Escape asks for it",
        },
        NotAPage {
            route: "about.htmx.edit",
            reason: "an htmx fragment: one checklist row as a small form, swapped in on double-click",
        },
    ]
}

/// A row of the infinite scroll: a bike of the catalogue.
#[derive(Serialize)]
struct Bike {
    id: i64,
    name: String,
    slug: String,
}

/// The bikes older than `before` (by id), one load's worth, and the cursor
/// of the next load when there are more. By id, not page number: a bike
/// added meanwhile would shift pages and repeat a row.
async fn load_bikes(db: &Db, before: Option<i64>) -> Result<(Vec<Bike>, Option<i64>)> {
    let mut rows = Product::query()
        .when(before.is_some(), |q| {
            q.where_(Product::ID.lt(before.unwrap_or_default()))
        })
        .order_by_desc(Product::ID)
        .limit(PER_LOAD + 1)
        .get(db)
        .await?;
    let more = rows.len() as u64 > PER_LOAD;
    rows.truncate(PER_LOAD as usize);
    let next = more.then(|| rows.last().map(|p| p.id)).flatten();
    let bikes = rows
        .into_iter()
        .map(|p| Bike {
            id: p.id,
            name: p.name,
            slug: p.slug,
        })
        .collect();
    Ok((bikes, next))
}

/// `GET /about/htmx`: the checklist and the first bikes.
async fn page(State(db): State<Db>, session: Session) -> Result<View> {
    let list = Checklist::of(&session);
    let (bikes, next) = load_bikes(&db, None).await?;
    Ok(view(
        "about/htmx.html",
        context! {
            open => list.open(),
            total => list.items.len(),
            items => list.items,
            bikes,
            next,
            max => MAX_ITEMS,
        },
    ))
}

#[derive(Deserialize)]
struct After {
    before: Option<i64>,
}

/// `GET /about/htmx/bikes?before=…`: the next bikes and the next loader.
async fn bikes(State(db): State<Db>, Query(after): Query<After>) -> Result<View> {
    let (bikes, next) = load_bikes(&db, after.before).await?;
    Ok(view("about/htmx/_bikes.html", context! { bikes, next }))
}

/// The modal's form, and the inline edit's.
#[derive(Deserialize, Serialize, Validate)]
struct ItemForm {
    #[validate(required, max = 60)]
    title: String,
}

/// The row (or nothing, after a delete) and, out of band, the open count
/// and the empty note (about/htmx/_answer.html).
fn answer(list: &Checklist, item: Option<&Item>) -> View {
    view(
        "about/htmx/_answer.html",
        context! { item, open => list.open(), total => list.items.len() },
    )
    .fragment("row")
    .also("count")
    .also("empty")
}

/// `POST /about/htmx/items`, from the modal: the new row at the top (and
/// the count, out of band), and an event the page can listen for. Invalid
/// input or a full list gets a 422 whose errors appear in the modal.
///
/// An item already on the list isn't added twice: the server changes where
/// the answer goes (`HX-Retarget` to that row, `HX-Reswap: outerHTML`) and
/// says so in a toast.
async fn store(
    session: Session,
    htmx: Htmx,
    lang: Lang,
    Valid(form): Valid<ItemForm>,
) -> Result<Response> {
    let mut list = Checklist::of(&session);
    let title = form.title.trim().to_owned();
    let existing = list
        .items
        .iter()
        .find(|i| i.title.eq_ignore_ascii_case(&title))
        .cloned();
    // [explain:about.htmx.store]
    if let Some(item) = existing {
        let toast = Toast::info(lang.t("htmx.already", &[]));
        if !htmx.request {
            return Ok((toast, Redirect::to("/about/htmx")).into_response());
        }
        return Ok((
            HxRetarget(format!("#item-{}", item.id)),
            HxReswap("outerHTML".into()),
            HxTrigger("item-added".into()),
            toast,
            answer(&list, Some(&item)),
        )
            .into_response());
    }
    // [/explain:about.htmx.store]
    if list.items.len() >= MAX_ITEMS {
        let mut errors = Errors::new();
        errors.add("title", lang.t("htmx.full", &[("max", &MAX_ITEMS)]));
        return Err(ValidationError::new(errors).with_input(&form).into());
    }
    let item = Item {
        id: list.next_id,
        title,
        done: false,
    };
    list.next_id += 1;
    list.items.insert(0, item.clone());
    list.save(&session)?;
    if htmx.request {
        return Ok((HxTrigger("item-added".into()), answer(&list, Some(&item))).into_response());
    }
    Ok(Redirect::to("/about/htmx").into_response())
}

/// `GET /about/htmx/items/{item}`: one row as shown in the list (the inline
/// edit's Escape asks for it).
async fn row(session: Session, Path(id): Path<u32>) -> Result<View> {
    let mut list = Checklist::of(&session);
    let item = list.find(id)?.clone();
    Ok(view("about/htmx/_row.html", context! { item }))
}

/// `GET /about/htmx/items/{item}/edit`: the row as a small form.
async fn edit(session: Session, Path(id): Path<u32>) -> Result<View> {
    let mut list = Checklist::of(&session);
    let item = list.find(id)?.clone();
    Ok(view("about/htmx/_edit.html", context! { item }))
}

/// The row for htmx, a redirect back for a plain form.
fn row_or_back(list: &Checklist, htmx: &Htmx, item: &Item) -> Response {
    if htmx.request {
        return answer(list, Some(item)).into_response();
    }
    Redirect::to("/about/htmx").into_response()
}

/// `PATCH /about/htmx/items/{item}`: the inline edit's Save.
async fn update(
    session: Session,
    htmx: Htmx,
    Path(id): Path<u32>,
    Valid(form): Valid<ItemForm>,
) -> Result<Response> {
    let mut list = Checklist::of(&session);
    let item = list.find(id)?;
    item.title = form.title.trim().to_owned();
    let item = item.clone();
    list.save(&session)?;
    Ok(row_or_back(&list, &htmx, &item))
}

/// `PATCH /about/htmx/items/{item}/toggle`: the checkbox; the answer is the
/// row, re-rendered, and the count.
async fn toggle(session: Session, htmx: Htmx, Path(id): Path<u32>) -> Result<Response> {
    let mut list = Checklist::of(&session);
    let item = list.find(id)?;
    item.done = !item.done;
    let item = item.clone();
    list.save(&session)?;
    Ok(row_or_back(&list, &htmx, &item))
}

/// `DELETE /about/htmx/items/{item}`: htmx swaps the row with an empty
/// answer, which removes it; the count comes along out of band, and a
/// toast confirms it.
async fn destroy(
    session: Session,
    htmx: Htmx,
    lang: Lang,
    Path(id): Path<u32>,
) -> Result<Response> {
    let mut list = Checklist::of(&session);
    let title = list.find(id)?.title.clone();
    list.items.retain(|i| i.id != id);
    list.save(&session)?;
    let toast = Toast::success(lang.t("htmx.deleted", &[("title", &title)]));
    if htmx.request {
        return Ok((toast, answer(&list, None)).into_response());
    }
    Ok((toast, Redirect::to("/about/htmx")).into_response())
}

/// `POST /about/htmx/clear-done`: many rows change at once, so the page is
/// simply reloaded (`HX-Refresh`). The toast waits in the session for it.
async fn clear_done(session: Session, htmx: Htmx, lang: Lang) -> Result<Response> {
    let mut list = Checklist::of(&session);
    let before = list.items.len();
    list.items.retain(|i| !i.done);
    let gone = before - list.items.len();
    list.save(&session)?;
    let toast = Toast::success(lang.t("htmx.cleared", &[("count", &gone)]));
    if htmx.request {
        return Ok((toast, HxRefresh).into_response());
    }
    Ok((toast, Redirect::to("/about/htmx")).into_response())
}

/// `POST /about/htmx/ready`: done checking, off to the shop: `HX-Redirect`
/// for htmx, a 303 for a plain form (both from `htmx.redirect`). The list
/// starts afresh; the toast waits for the shop's page.
async fn ready(
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    lang: Lang,
) -> Result<Response> {
    let list = Checklist::of(&session);
    let checked = list.items.iter().filter(|i| i.done).count();
    session.remove(SESSION_KEY);
    let toast = Toast::success(lang.t("htmx.ready", &[("count", &checked)]));
    Ok((toast, htmx.redirect(&state.url("catalog.index", &[])?)).into_response())
}

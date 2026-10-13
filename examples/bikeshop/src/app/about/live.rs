//! `/about/htmx/live`: the checklist of `/about/htmx` again, as a live
//! component (#440). The state is a struct, the actions are methods, and
//! `rx-click` / `rx-model` in the template call them; no routes, fragments or
//! out-of-band swaps. It reads and writes the same session checklist as the
//! htmx page, so the two pages show the same items.
//!
//! Left out on purpose: the bikes' infinite scroll, the duplicate
//! (`HxRetarget`/`HxReswap`), "ready" (`HX-Redirect`, `HxRefresh`) and
//! `hx-confirm` on delete. The README counts the lines of both versions.

use renox::live_component::LiveContext;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::htmx::{Checklist, Item, MAX_ITEMS};

/// The new item's field.
#[derive(Serialize, Validate)]
struct NewItem {
    #[validate(required, max = 60)]
    title: String,
}

/// The inline edit's field.
#[derive(Serialize, Validate)]
struct Rename {
    #[validate(required, max = 60)]
    edit_title: String,
}

/// The routes of the page (added to `About::routes`).
pub fn routes() -> Routes {
    Routes::new()
        .get("/about/htmx/live", page)
        .name("about.htmx_live")
}

/// `GET /about/htmx/live`: the component, mounted.
async fn page(ctx: LiveContext) -> Result<View> {
    let checklist = ctx.mount(LiveChecklist::default()).await?;
    Ok(view("about/live.html", context! { checklist }))
}

// [explain:about.htmx_live.component]
/// The state the browser carries (signed); the items stay in the session.
#[derive(Serialize, Deserialize, Default)]
pub struct LiveChecklist {
    pub title: String,
    pub editing: Option<u32>,
    pub edit_title: String,
    pub filter: String,
}

#[renox::live_component(view = "about/live/_checklist.html", name = "checklist")]
impl LiveChecklist {
    /// What the template shows besides the state: the items, filtered.
    async fn data(&self, ctx: &LiveContext) -> Result<renox::serde_json::Value> {
        let list = Checklist::of(session(ctx)?);
        let items: Vec<&Item> = list
            .items
            .iter()
            .filter(|i| match self.filter.as_str() {
                "open" => !i.done,
                "done" => i.done,
                _ => true,
            })
            .collect();
        Ok(json!({ "items": items, "open": list.open(), "total": list.items.len() }))
    }

    #[live(action)]
    async fn toggle(&mut self, ctx: &mut LiveContext, id: u32) -> Result {
        let session = session(ctx)?;
        let mut list = Checklist::of(session);
        let item = list.find(id)?;
        item.done = !item.done;
        list.save(session)
    }

    // [/explain:about.htmx_live.component]
    #[live(action)]
    async fn add(&mut self, ctx: &mut LiveContext) -> Result {
        let new = NewItem {
            title: self.title.trim().to_owned(),
        };
        ctx.validate(&new).await?;
        let session = session(ctx)?;
        let mut list = Checklist::of(session);
        if list.items.len() >= MAX_ITEMS {
            let mut errors = Errors::new();
            let max = MAX_ITEMS;
            errors.add(
                "title",
                ctx.state().current_lang().t("htmx.full", &[("max", &max)]),
            );
            return Err(ValidationError::new(errors).into());
        }
        let id = list.next_id;
        list.next_id += 1;
        list.items.insert(
            0,
            Item {
                id,
                title: new.title,
                done: false,
            },
        );
        list.save(session)?;
        self.title.clear();
        Ok(())
    }

    #[live(action)]
    async fn edit(&mut self, ctx: &mut LiveContext, id: u32) -> Result {
        let mut list = Checklist::of(session(ctx)?);
        self.edit_title = list.find(id)?.title.clone();
        self.editing = Some(id);
        Ok(())
    }

    #[live(action)]
    async fn save_edit(&mut self, ctx: &mut LiveContext) -> Result {
        let rename = Rename {
            edit_title: self.edit_title.trim().to_owned(),
        };
        ctx.validate(&rename).await?;
        let id = self.editing.ok_or(Error::NotFound)?;
        let session = session(ctx)?;
        let mut list = Checklist::of(session);
        list.find(id)?.title = rename.edit_title;
        list.save(session)?;
        self.editing = None;
        Ok(())
    }

    #[live(action)]
    async fn cancel_edit(&mut self, _ctx: &mut LiveContext) -> Result {
        self.editing = None;
        Ok(())
    }

    #[live(action)]
    async fn delete(&mut self, ctx: &mut LiveContext, id: u32) -> Result {
        let session = session(ctx)?;
        let mut list = Checklist::of(session);
        list.find(id)?;
        list.items.retain(|i| i.id != id);
        list.save(session)
    }

    #[live(action)]
    async fn clear_done(&mut self, ctx: &mut LiveContext) -> Result {
        let session = session(ctx)?;
        let mut list = Checklist::of(session);
        list.items.retain(|i| !i.done);
        list.save(session)
    }

    #[live(action)]
    async fn show(&mut self, _ctx: &mut LiveContext, filter: String) -> Result {
        self.filter = filter;
        Ok(())
    }
}

/// The visitor's session; the checklist lives there.
fn session(ctx: &LiveContext) -> Result<&Session> {
    ctx.session()
        .ok_or_else(|| Error::BadRequest("the checklist needs a session".into()))
}

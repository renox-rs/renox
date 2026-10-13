//! Every data API with a closure, a generic executor or a borrowed slice,
//! called from a handler that is really routed. axum needs a handler's
//! future to be `Send`; an API whose future holds a closure over `&T` across
//! an `.await` fails that check (rustc issue #100013) only where it's
//! routed, so doctests and plain `async fn` tests don't catch it. The
//! relation loaders did, until an example app (the shop) routed them.

use std::time::Duration;

use renox::db::relations::{Pivot, belongs_to, has_many, has_many_through};
use renox::prelude::*;
use renox::testing::TestApp;

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "notes", no_typed_columns)]
struct Note {
    id: i64,
    body: String,
    stars: i64,
}

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "tags")]
struct Tag {
    id: i64,
    note_id: Option<i64>,
    name: String,
}

impl Note {
    const BODY: renox::db::Col<Note, String> = renox::db::Col::new("body");
}

const NOTE_TAGS: Pivot = Pivot::new("note_tags", "note_id", "tag_id");

async fn relations(State(db): State<Db>) -> Result<String> {
    let notes = Note::query().order_by("id").get(&db).await?;
    let tags = Tag::query().get(&db).await?;
    let parents = belongs_to::<Note, _, _>(&db, &tags, |t| t.note_id).await?;
    let children = has_many(&db, &notes, Tag::query(), "note_id", |t| t.note_id).await?;
    // Notes → tags → tags again (by id): only the future's `Send` matters here.
    let _through = has_many_through(
        &db,
        &notes,
        Tag::query(),
        "note_id",
        |t| t.note_id,
        Tag::query(),
        "id",
        |t| t.id,
    )
    .await?;
    let linked = NOTE_TAGS.load_for::<Tag, _>(&db, &notes).await?;
    let loaded = NOTE_TAGS
        .load::<Tag>(&db, notes.iter().map(|n| n.id))
        .await?;
    NOTE_TAGS.attach(&db, 1, [1]).await?;
    NOTE_TAGS.detach(&db, 1, vec![2]).await?;
    NOTE_TAGS.sync(&db, 1, [1]).await?;
    let ids = NOTE_TAGS.ids(&db, 1).await?;
    Ok(format!(
        "{} {} {} {} {:?}",
        parents.len(),
        children.len(),
        linked.len(),
        loaded.len(),
        ids
    ))
}

/// A helper taking `&Db`, the way apps split handlers up.
async fn tagged(db: &Db, notes: &[Note]) -> Result<usize> {
    Ok(
        belongs_to::<Note, _, _>(db, &Tag::query().get(db).await?, |t| t.note_id)
            .await?
            .len()
            + notes.len(),
    )
}

async fn queries(State(state): State<AppState>) -> Result<String> {
    let db = &state.db;
    let first = Note::where_eq("body", "a")
        .first_or_create(db, || Note {
            body: "a".into(),
            ..Default::default()
        })
        .await?;
    let mut seen = 0;
    Note::query()
        .chunk(db, 10, |rows| {
            seen += rows.len();
            async { Ok(()) }
        })
        .await?;
    let cached: i64 = state
        .cache
        .remember("notes", Duration::from_secs(5), || async {
            Note::query().count(db).await.map(|n| n as i64)
        })
        .await?;
    let notes = Note::find_many(db, vec![first.id]).await?;
    let helped = tagged(db, &notes).await?;
    let names: Vec<String> = Note::query().pluck(db, "body").await?;
    let mut tx = db.begin().await?;
    Note::create(
        &mut tx,
        Note {
            body: "b".into(),
            ..Default::default()
        },
    )
    .await?;
    Note::where_eq("body", "b")
        .update(&mut tx, &[("body", &"c")])
        .await?;
    tx.commit().await?;
    Ok(format!("{seen} {cached} {helped} {}", names.len()))
}

/// Typed columns in a routed handler.
async fn typed(State(db): State<Db>) -> Result<String> {
    let notes = Note::query()
        .where_(Note::BODY.like("%"))
        .order_by(Note::BODY)
        .order_by_desc(Note::BODY)
        .get(&db)
        .await?;
    let bodies: Vec<String> = Note::query().pluck(&db, Note::BODY).await?;
    Ok(format!("{} {}", notes.len(), bodies.len()))
}

/// The rest of the query builder and model API, each awaited in a handler.
async fn more(State(state): State<AppState>, user: Option<AuthUser>) -> Result<String> {
    let db = &state.db;
    let page = Note::query().latest().paginate(db, 1, 10).await?;
    let total: i64 = Note::query().sum(db, "id").await?;
    Note::where_eq("id", 1).increment(db, "id", 0).await?;
    let exists = Note::where_eq("id", 1).exists(db).await?;
    let found = Note::find(db, 1).await?;
    let one = Note::find_or_404(db, 1).await?;
    Note::insert_many(
        db,
        vec![Note {
            body: "d".into(),
            ..Default::default()
        }],
    )
    .await?;
    Note::upsert(db, vec![one.clone()], &["body"], &["stars"]).await?;
    let gone = Note::where_eq("body", "d").delete(db).await?;
    let rows = renox::db::sql("SELECT id, body FROM notes WHERE id = ?")
        .bind(1)
        .fetch_optional(db)
        .await?;
    let pairs: Vec<(i64, String)> = renox::db::sql("SELECT id, body FROM notes")
        .fetch_as(db)
        .await?;
    if let Some(user) = user {
        user.notifications(db, 5).await?;
        user.notifications_before(db, 10, 5).await?;
        user.notification(db, 1).await?;
        user.mark_notification_unread(db, 1).await?;
        user.delete_notification(db, 1).await?;
        user.delete_notifications(db).await?;
    }
    state.emit(Ping).await?;
    Ok(format!(
        "{} {total} {exists} {} {gone} {} {}",
        page.total,
        found.is_some(),
        rows.is_some(),
        pairs.len()
    ))
}

#[derive(Clone)]
struct Ping;

impl Event for Ping {}

/// Dashboards' time series (routed for `Send`; not called: notes have
/// no dates).
async fn trends(State(state): State<AppState>, period: renox::chart::Period) -> Result<String> {
    use renox::chart::Trend;
    let count = Trend::of(Note::query(), "created_at")
        .over(period)
        .count(&state)
        .await?;
    let sum = Trend::of(Note::query(), "created_at")
        .sum(&state, "stars")
        .await?;
    let average = Trend::of(Note::query(), "created_at")
        .over(renox::chart::Period::days(90).per(renox::chart::Bucket::Week))
        .average(&state, "stars")
        .await?;
    Ok(format!(
        "{} {} {}",
        count.total(),
        sum.total(),
        average.total()
    ))
}

/// Roles, permissions, token abilities and scoped rules (routed so the
/// futures are checked for `Send`; the tables aren't created here).
async fn access(State(db): State<Db>, user: AuthUser, session: Session) -> Result<String> {
    use renox::auth::permissions;
    permissions::define_role(&db, "editor", &["posts.publish"]).await?;
    permissions::grant(&db, "editor", &["posts.edit"]).await?;
    permissions::revoke(&db, "editor", &["posts.edit"]).await?;
    user.assign_role(&db, "editor").await?;
    user.sync_roles(&db, &["editor"]).await?;
    user.remove_role(&db, "editor").await?;
    let roles = user.roles(&db).await?;
    let all = permissions::roles(&db).await?;
    // Roles in one record (#244).
    let store = permissions::Scope::of_id::<Note>(1);
    permissions::set_scope(store.clone());
    user.assign_role_in(&db, "editor", &store)
        .from(renox::db::now())
        .until(renox::db::now() + renox::chrono::Duration::days(1))
        .await?;
    user.assign_role_in(&db, "editor", &permissions::Scope::of(&Note::default()))
        .await?;
    user.remove_role_in(&db, "editor", &store).await?;
    user.sync_roles_in(&db, &["editor"], &store).await?;
    let assignments = user.assignments(&db).await?;
    let managers = permissions::users_with_role_in(&db, "editor", &store).await?;
    let editors = permissions::users_with_permission_in(&db, "posts.edit", &store).await?;
    let publishers = permissions::users_with_permission(&db, "posts.publish").await?;
    // Several statements on one generic executor (#303).
    {
        let mut tx = db.begin().await?;
        let mut conn = renox::db::Executor::into_conn(&mut tx);
        Note::query().count(conn.reborrow()).await?;
        renox::db::sql("SELECT 1").execute(conn.reborrow()).await?;
        tx.commit().await?;
    }
    permissions::prune_ended_assignments(&db, Duration::from_secs(60)).await?;
    let mine = permissions::scopes_with::<Note>("posts.publish");
    let scoped = Note::query().where_eq("stars", 1);
    let _narrowed = user
        .scopes_with::<Note>("posts.publish")
        .apply(scoped, &["id"])
        .count(&db)
        .await?;
    let _checks = (
        user.has_permission_in("posts.publish", &store),
        user.has_role_in("editor", &store),
        mine.is_empty(),
        assignments.len() + managers.len() + editors.len() + publishers.len(),
    );
    permissions::clear_scope();
    permissions::delete_role(&db, "editor").await?;
    user.create_token_with(&db, "t", &["a"], None).await?;
    renox::auth::prune_expired_tokens(&db, Duration::from_secs(60)).await?;
    let kiosk = renox::auth::DeviceToken::create(&db, "kiosk:1", "k", Some(&["a"]), None).await?;
    renox::auth::DeviceToken::for_device(&db, "kiosk:1").await?;
    renox::auth::DeviceToken::revoke(&db, "kiosk:1", kiosk.token.id).await?;
    renox::auth::DeviceToken::revoke_all(&db, "kiosk:1").await?;
    renox::auth::DeviceToken::prune_expired(&db, Duration::from_secs(60)).await?;
    renox::auth::prune_read_notifications(&db, Duration::from_secs(60)).await?;
    let scoped = Note::unscoped().none().count(&db).await?;
    let mut me = user.user().clone();
    renox::auth::change_password(&db, &session, &mut me, "a new password").await?;
    renox::auth::logout_other_devices(&db, &session, &me).await?;
    renox::audit::record(&db, renox::audit::Entry::new("x").user(me.id)).await?;
    renox::audit::latest(&db, 5).await?;
    renox::audit::for_user(&db, me.id, 5).await?;
    renox::audit::prune(&db, Duration::from_secs(60)).await?;
    renox::auth::logout(&db, &session).await?;
    me.delete_account(&db).await?;
    Ok(format!("{} {} {scoped}", roles.len(), all.len()))
}

/// M19a's data APIs, routed (not called: a compile-time `Send` check).
async fn builder(State(db): State<Db>) -> Result<String> {
    let notes = Note::query().get(&db).await?;
    let counts = renox::db::relations::count_many(&db, &notes, Tag::query(), "note_id").await?;
    let sums =
        renox::db::relations::sum_many::<i64, _, _>(&db, &notes, Tag::query(), "note_id", "id")
            .await?;
    let groups: Vec<(i64, i64)> = Note::query()
        .group_by("stars")
        .select_as(&db, "stars, COUNT(*)")
        .await?;
    let simple = Note::query().simple_paginate(&db, 1, 10).await?;
    let cursor = Note::query().cursor_paginate(&db, None, 10).await?;
    let mut note = Note::query()
        .update_or_create(&db, Note::default, |n| n.body = "u".into())
        .await?;
    note.refresh(&db).await?;
    let fresh = Note::query().first_or_new(&db, Note::default).await?;
    let has = Note::query()
        .where_has(Tag::query(), "note_id")
        .count(&db)
        .await?;
    let moved: i64 = db
        .transaction_retrying(2, |tx| {
            Box::pin(
                async move { Ok(Note::query().lock_for_update().count(&mut *tx).await? as i64) },
            )
        })
        .await?;
    Ok(format!(
        "{} {} {} {} {} {} {} {moved}",
        counts.len(),
        sums.len(),
        groups.len(),
        simple.items.len(),
        cursor.items.len(),
        fresh.id,
        has
    ))
}

/// M19b's model APIs, routed (not called: a compile-time `Send` check).
async fn models(State(state): State<AppState>) -> Result<String> {
    let db = &state.db;
    let original = Note::find_or_404(db, 1).await?;
    let mut note = original.clone();
    note.stars += 1;
    note.save_only(db, &["stars"]).await?;
    let changed = note.save_changes(db, &original).await?;
    NOTE_TAGS.attach_with(db, 1, 2, &[("tag_id", &2)]).await?;
    NOTE_TAGS.update_pivot(db, 1, 2, &[]).await?;
    let (on, off) = NOTE_TAGS.toggle(db, 1, [1, 2]).await?;
    let pivots = NOTE_TAGS.load_with_pivot::<Tag, (i64,)>(db, [1]).await?;
    const TAGGABLE: renox::db::relations::Morph =
        renox::db::relations::Morph::new("name", "note_id");
    let notes = Note::query().get(db).await?;
    let tags = TAGGABLE
        .load_many(db, &notes, Tag::query(), |t| t.note_id.unwrap_or(0))
        .await?;
    let all_tags = TAGGABLE.of(&note, Tag::query()).get(db).await?;
    let parents = TAGGABLE
        .parents::<Note, _>(db, &all_tags, |t| (t.name.clone(), t.note_id.unwrap_or(0)))
        .await?;
    let secret = state.decrypt(&state.encrypt("s"))?;
    Ok(format!(
        "{changed} {} {} {} {} {} {secret}",
        on.len() + off.len(),
        pivots.len(),
        tags.len(),
        parents.len(),
        renox::context::app().is_some()
    ))
}

/// M20a's cache APIs, routed and called.
async fn cache(State(state): State<AppState>) -> Result<String> {
    let cache = &state.cache;
    let first = cache.add("send:add", &vec![1, 2], None).await?;
    let again = cache
        .add("send:add", &"x", Some(Duration::from_secs(60)))
        .await?;
    let pulled: Option<Vec<i64>> = cache.pull("send:add").await?;
    let count = cache.increment("send:count", 2).await? + cache.decrement("send:count", 1).await?;
    let lock = cache.lock("send:lock", Duration::from_secs(5));
    let guard = lock.try_acquire().await?;
    let held = lock.is_held().await?;
    drop(guard);
    let guard = cache
        .lock("send:block", Duration::from_secs(5))
        .block(Duration::from_secs(1))
        .await?;
    let released = guard.release().await?;
    Ok(format!(
        "{first} {again} {} {count} {held} {released}",
        pulled.map_or(0, |v| v.len())
    ))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Nudge(i64);

impl Job for Nudge {
    const NAME: &'static str = "nudge";

    async fn handle(self, _: JobContext) -> Result {
        Ok(())
    }
}

/// M20's queue and cache APIs, routed (not called: a compile-time `Send` check).
async fn background(State(state): State<AppState>) -> Result<String> {
    let queue = &state.queue;
    let chain = queue
        .chain()
        .then(Nudge(1))
        .then(Nudge(2))
        .dispatch()
        .await?;
    let batch = queue
        .batch("b")
        .push(Nudge(3))
        .then(Nudge(4))
        .catch(Nudge(5))
        .finally(Nudge(6))
        .allow_failures()
        .dispatch()
        .await?;
    let status = queue.batch_status(batch).await?;
    queue.cancel_batch(batch).await?;
    queue.dispatch_on("high", Nudge(7)).await?;
    state.dispatch_sync(Nudge(8)).await?;
    queue.forget_failed(1).await?;
    queue.prune_failed(Duration::from_secs(1)).await?;
    queue.prune_batches(Duration::from_secs(1)).await?;
    let cache = &state.cache;
    let n = cache.increment("n", 1).await?;
    cache.add("a", &1, None).await?;
    let pulled: Option<i64> = cache.pull("a").await?;
    let lock = cache.lock("l", Duration::from_secs(5));
    if let Some(guard) = lock.try_acquire().await? {
        guard.release().await?;
    }
    let guard = lock.block(Duration::from_secs(1)).await?;
    drop(guard);
    cache.prune().await?;
    Ok(format!(
        "{chain} {} {n} {pulled:?}",
        status.map_or(0, |s| s.progress())
    ))
}

/// M21a's APIs, routed (not called: a compile-time `Send` check).
async fn polish(State(state): State<AppState>, user: AuthUser) -> Result<String> {
    let db = &state.db;
    let label = String::from("borrowed");
    let n = db
        .retrying(2, || async {
            let mut tx = db.begin().await?;
            let n = Note::query().count(&mut tx).await?;
            tx.commit().await?;
            Ok(format!("{label} {n}"))
        })
        .await?;
    let admins = renox::auth::permissions::users_with_role(db, "admin").await?;
    let likes = renox::db::relations::Morph::new("name", "note_id")
        .count_many(db, &Note::query().get(db).await?, Tag::query())
        .await?;
    Ok(format!(
        "{n} {} {} {} {}",
        admins.len(),
        likes.len(),
        user.has_role("admin"),
        renox::random_token().len()
    ))
}

/// M22: a ULID-keyed model through the same loaders.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "docs")]
struct Doc {
    id: renox::db::Ulid,
    title: String,
}

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "pages")]
struct Page {
    id: i64,
    doc_id: renox::db::Ulid,
}

const DOC_TAGS: Pivot<renox::db::Ulid, i64> = Pivot::new("doc_tags", "doc_id", "tag_id");

async fn keyed(State(db): State<Db>) -> Result<String> {
    let mut doc = Doc {
        title: "Handbook".into(),
        ..Default::default()
    };
    doc.insert(&db).await?;
    let doc = Doc::find_or_404(&db, doc.id.clone()).await?;
    Page::create(
        &db,
        Page {
            doc_id: doc.id.clone(),
            ..Default::default()
        },
    )
    .await?;
    let docs = Doc::find_many(&db, [doc.id.clone()]).await?;
    let pages = Page::query().get(&db).await?;
    let parents = belongs_to::<Doc, _, _>(&db, &pages, |p| p.doc_id.clone()).await?;
    let children = has_many(&db, &docs, Page::query(), "doc_id", |p| p.doc_id.clone()).await?;
    let counts = renox::db::relations::count_many(&db, &docs, Page::query(), "doc_id").await?;
    DOC_TAGS.attach(&db, doc.id.clone(), [1]).await?;
    DOC_TAGS.sync(&db, doc.id.clone(), [1, 2]).await?;
    DOC_TAGS.toggle(&db, doc.id.clone(), [2]).await?;
    let tags = DOC_TAGS.load_for::<Tag, _>(&db, &docs).await?;
    let morph = renox::db::relations::Morph::new("title", "id")
        .parents::<Doc, _>(&db, &pages, |p| ("docs".into(), p.doc_id.clone()))
        .await?;
    let mut chunked = 0;
    Doc::query()
        .chunk(&db, 10, |rows| {
            chunked += rows.len();
            async { Ok(()) }
        })
        .await?;
    let page = Doc::query().cursor_paginate(&db, None, 10).await?;
    Ok(format!(
        "{} {} {} {} {} {} {chunked} {}",
        parents.len(),
        children.len(),
        counts[&doc.id],
        tags[&doc.id].len(),
        morph.len(),
        docs.len(),
        page.items.len()
    ))
}

/// M23: a savepoint (a closure) and an `Encrypted` field in a routed handler.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "vaults")]
struct Vault {
    id: i64,
    secret: renox::db::Encrypted<String>,
}

async fn sealed(State(db): State<Db>) -> Result<String> {
    let label = String::from("borrowed");
    let mut tx = db.begin().await?;
    let inner = tx
        .savepoint(|tx| {
            Box::pin(async move {
                let vault = Vault::create(
                    &mut *tx,
                    Vault {
                        secret: renox::db::Encrypted::new("s3cret".into()),
                        ..Default::default()
                    },
                )
                .await?;
                Ok(vault.id)
            })
        })
        .await?;
    tx.commit().await?;
    let vault = Vault::find_or_404(&db, inner).await?;
    Ok(format!("{label} {}", *vault.secret))
}

/// M24: factory states (closures) and session helpers in a routed handler.
impl Factory for Note {
    fn definition() -> Self {
        Note {
            body: format!("note {}", renox::random_token()),
            ..Default::default()
        }
    }
}

async fn factories(State(db): State<Db>, session: Session) -> Result<String> {
    let label = String::from("borrowed");
    let notes = Note::factory()
        .count(2)
        .state(|n: &mut Note| n.stars = 5)
        .sequence(move |i, n: &mut Note| n.body = format!("{label} {i}"))
        .create(&db)
        .await?;
    let one = Note::factory().create_one(&db).await?;
    let pushed = session.push("seen", one.id)?;
    Ok(format!("{} {pushed}", notes.len()))
}

/// #153: an import (a closure per row, after the rows' rules), an export
/// of a query outside the grid's page and a replicated model.
#[derive(serde::Deserialize, Validate)]
struct NoteRow {
    #[validate(required, max = 50)]
    body: String,
    #[validate(min = 0)]
    stars: Option<i64>,
}

async fn imports(
    State(state): State<AppState>,
    request: renox::grid::GridRequest,
    lang: Lang,
) -> Result<String> {
    use renox::grid::{Column, ExportFormat, Grid};
    let suffix = String::from("!");
    let report = renox::import::Import::csv("body,stars\nimported,2\n,1\nimported,3\n")
        .lang(&lang)
        .run(&state, move |tx, row: NoteRow| {
            let body = format!("{}{suffix}", row.body);
            Box::pin(async move {
                Note::create(
                    tx,
                    Note {
                        body,
                        stars: row.stars.unwrap_or(0),
                        ..Default::default()
                    },
                )
                .await?;
                Ok(())
            })
        })
        .await?;
    let copy = Note::find_or_404(&state.db, 1).await?.replicate();
    let file = Grid::new("notes")
        .column(Column::text("body", "Body"))
        .export_as(Note::query(), ExportFormat::Csv, &request)
        .await?;
    // A workbook of two sheets (answers 400 without the `xlsx` feature).
    let book = renox::grid::Workbook::new("notes")
        .sheet(
            "Notes",
            &Grid::new("notes").column(Column::text("body", "Body")),
            Note::query(),
            &request,
        )
        .await;
    let _ = book.map(|b| b.into_response());
    Ok(format!(
        "{} {} {} {}",
        report.imported,
        report.failed.len(),
        copy.id,
        file.status().as_u16()
    ))
}

/// #154: full-text search on a model with an index.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "articles", search = "title, body")]
struct Article {
    id: i64,
    title: String,
    body: String,
}

const ARTICLES: renox::db::Migration =
    renox::db::Migration::new("20300101000000_articles", "", Some("DROP TABLE articles"))
        .sqlite(
            "CREATE TABLE articles (id INTEGER PRIMARY KEY, title TEXT NOT NULL, body TEXT NOT NULL)",
            None,
        )
        .postgres(
            "CREATE TABLE articles (id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, title TEXT NOT NULL, body TEXT NOT NULL)",
            None,
        );

async fn search(State(db): State<Db>) -> Result<String> {
    Article::create(
        &db,
        Article {
            title: "Coffee".into(),
            body: "Roasting beans".into(),
            ..Default::default()
        },
    )
    .await?;
    let found = Article::search("coffee").get(&db).await?;
    let page = Article::query()
        .where_search("roast")
        .order_by_relevance("roast")
        .paginate(&db, 1, 10)
        .await?;
    let mut tx = db.begin().await?;
    let in_tx = Article::query().search("bean").count(&mut tx).await?;
    tx.commit().await?;
    renox::db::search::rebuild::<Article>(&db).await?;
    Ok(format!("{} {} {in_tx}", found.len(), page.total))
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Counter {
    count: i64,
}

impl LiveComponent for Counter {
    const NAME: &'static str = "counter";
    const VIEW: &'static str = "live/counter.html";

    async fn call(
        &mut self,
        _action: &str,
        _args: Vec<serde_json::Value>,
        _ctx: &mut renox::live_component::LiveContext,
    ) -> Result {
        Ok(())
    }
}

async fn live(ctx: renox::live_component::LiveContext) -> Result<String> {
    Ok(ctx.mount(Counter::default()).await?.snapshot)
}

struct Handlers;

/// M27a: a data grid's page and its filtered query.
async fn grid(request: renox::grid::GridRequest, State(db): State<Db>) -> Result<String> {
    use renox::grid::{Column, Grid};
    let grid = Grid::new("notes")
        .column(Column::text("body", "Body"))
        .column(Column::number("stars", "Stars"));
    let page = grid
        .page(Note::query(), &request)
        .await?
        .extend(|note| json!({ "double": note.stars * 2 }));
    let all = grid.filter(Note::query(), &request).count(&db).await?;
    Ok(format!("{} {all}", page.total()))
}

impl Module for Handlers {
    fn name(&self) -> &'static str {
        "handlers"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/relations", relations)
            .get("/queries", queries)
            .get("/typed", typed)
            .get("/more", more)
            .get("/access", access)
            .get("/trends", trends)
            .get("/builder", builder)
            .get("/models", models)
            .get("/cache", cache)
            .get("/background", background)
            .get("/polish", polish)
            .get("/keyed", keyed)
            .get("/sealed", sealed)
            .get("/factories", factories)
            .get("/grid", grid)
            .get("/imports", imports)
            .get("/search", search)
            .get("/live", live)
    }
}

#[renox::test]
async fn data_apis_work_in_routed_handlers() {
    let app = TestApp::new(App::new().module(Handlers).migrations(&[
        ARTICLES,
        renox::db::search::migration::<Article>("20300101000001_search_articles"),
    ]))
    .await;
    let id = match app.db().dialect() {
        renox::db::Dialect::Postgres => "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY",
        _ => "INTEGER PRIMARY KEY",
    };
    for statement in [
        format!(
            "CREATE TABLE notes (id {id}, body TEXT NOT NULL UNIQUE, stars BIGINT NOT NULL DEFAULT 0)"
        ),
        format!("CREATE TABLE tags (id {id}, note_id BIGINT, name TEXT NOT NULL)"),
        "CREATE TABLE note_tags (note_id BIGINT NOT NULL, tag_id BIGINT NOT NULL)".into(),
        "CREATE TABLE docs (id TEXT PRIMARY KEY, title TEXT NOT NULL)".into(),
        format!("CREATE TABLE vaults (id {id}, secret TEXT NOT NULL)"),
        format!("CREATE TABLE pages (id {id}, doc_id TEXT NOT NULL)"),
        "CREATE TABLE doc_tags (doc_id TEXT NOT NULL, tag_id BIGINT NOT NULL)".into(),
        "INSERT INTO notes (body) VALUES ('x')".into(),
        "INSERT INTO tags (note_id, name) VALUES (1, 'coffee'), (NULL, 'tea')".into(),
    ] {
        renox::db::sql(statement).execute(app.db()).await.unwrap();
    }
    app.get("/live").await.assert_ok();
    app.get("/relations")
        .await
        .assert_ok()
        .assert_see("1 1 0 0 [1]");
    app.get("/queries").await.assert_ok().assert_see("2 2 2 2");
    app.get("/typed").await.assert_ok();
    app.get("/more")
        .await
        .assert_ok()
        .assert_see("3 6 true true 1 true 3");
    app.get("/cache")
        .await
        .assert_ok()
        .assert_see("true false 2 3 true true");
    app.get("/keyed")
        .await
        .assert_ok()
        .assert_see("1 1 1 1 1 1 1 1");
    app.get("/sealed")
        .await
        .assert_ok()
        .assert_see("borrowed s3cret");
    app.get("/factories").await.assert_ok().assert_see("2 1");
    app.get("/grid?min.stars=0").await.assert_ok();
    // One row imported, a blank body and a repeat refused.
    app.get("/imports")
        .await
        .assert_ok()
        .assert_see("1 2 0 200");
    app.get("/search").await.assert_ok().assert_see("1 1 1");
}

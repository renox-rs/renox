//! A guestbook showing Renox's features so far: named routes, views with a
//! layout, sessions and flash messages, CSRF, HTMX fragments, validation with
//! old input, SQLite with a model, migrations, a seeder and pagination,
//! login/registration and the account page from the `Auth` module, and an event whose listener
//! queues a job, plus a scheduled task and an app command
//! (`cargo run -- entries:prune --days 7`, a clap `AppCommand`).
//!
//! Written by hand in one file to show everything at a glance; in an app made
//! with `rnx new`, the same pieces come from `rnx make:module guestbook`,
//! `rnx make:model Entry --module guestbook -m`,
//! `rnx make:event EntryPosted --module guestbook`,
//! `rnx make:job ThankGuest --module guestbook` and
//! `rnx make:command entries:prune --module guestbook`.
//!
//! The app lives in this library (`app()`), so `tests/` can boot it;
//! `main.rs` only runs it. Run it from this directory:
//!
//! ```text
//! cargo run -- migrate
//! cargo run -- db:seed     # optional: 30 fake entries
//! cargo run
//! ```

use renox::clap;
use renox::command::AppCommand;
use renox::fake::Fake;
use renox::fake::faker::lorem::en::Sentence;
use renox::fake::faker::name::en::FirstName;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Model, Serialize, Default)]
#[model(table = "entries")]
struct Entry {
    id: i64,
    name: String,
    message: String,
    /// Storage key of an optional photo, e.g. `public/entries/abc.jpg`.
    photo: Option<String>,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
}

impl Factory for Entry {
    fn definition() -> Self {
        Entry {
            name: FirstName().fake(),
            message: Sentence(3..8).fake(),
            ..Default::default()
        }
    }
}

/// The form and its rules. Field names in messages come from
/// resources/lang/*.json (`renox.validation.attributes.*`), in the
/// visitor's language.
#[derive(Deserialize, Validate)]
struct EntryForm {
    #[validate(required, max = 50)]
    name: String,
    #[validate(required, between(3, 280))]
    message: String,
    #[validate(image, max = 2048)] // KB; the content is sniffed, not the name
    photo: Option<Upload>,
}

/// Emitted when someone signs the guestbook.
#[derive(Clone)]
struct EntryPosted {
    entry_id: i64,
}

impl Event for EntryPosted {}

/// Thanks the guest in the background (to the log, with `MAIL_MAILER=log`).
#[derive(Serialize, Deserialize)]
struct ThankGuest {
    entry_id: i64,
}

impl Job for ThankGuest {
    const NAME: &'static str = "thank-guest";

    async fn handle(self, ctx: JobContext) -> Result {
        let entry = Entry::find_or_404(&ctx.state.db, self.entry_id).await?;
        let mail = renox::mail::Mail::new(
            "owner@example.com",
            format!("{} signed the guestbook", entry.name),
            entry.message,
        );
        ctx.state.mailer.send(mail).await
    }
}

struct Guestbook;

impl Module for Guestbook {
    fn name(&self) -> &'static str {
        "guestbook"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", index)
            .name("guestbook.index")
            .post("/entries", store)
            .name("guestbook.store")
            .get("/hello/{name}", greet)
            .name("greet")
            .get("/language/{locale}", switch_language)
            .name("language")
    }

    fn register(&self, app: &mut Registry) {
        // `db:check` and `view:check` read what the app registers.
        app.model::<Entry>().view_data::<GuestbookPage>();
        app.job::<ThankGuest>()
            .listen(|event: EntryPosted, state| async move {
                state
                    .dispatch(ThankGuest {
                        entry_id: event.entry_id,
                    })
                    .await?;
                Ok(())
            });
        // `cargo run -- entries:prune --days 7`
        app.typed_command::<PruneEntries>();
        app.schedule()
            .every_minute("count-entries", |state| async move {
                let total = Entry::query().count(&state.db).await?;
                tracing::info!(total, "guestbook entries");
                Ok(())
            });
    }
}

/// Delete entries older than --days (default 30).
///
/// The doc comment above is the line in `cargo run -- help`; clap parses the
/// arguments (`--days x` is an error) and writes `entries:prune --help`.
#[derive(clap::Parser)]
#[command(name = "entries:prune")]
struct PruneEntries {
    /// Keep entries younger than this many days.
    #[arg(long, default_value_t = 30)]
    days: i64,
    /// Don't ask first (asked only in a terminal; cron never waits).
    #[arg(long)]
    force: bool,
}

impl AppCommand for PruneEntries {
    async fn run(self, state: AppState) -> Result {
        let cutoff = renox::db::now() - renox::chrono::TimeDelta::days(self.days);
        let old = || Entry::query().where_(Entry::CREATED_AT.lt(cutoff));
        let count = old().count(&state.db).await?;
        if count == 0 {
            println!("No entries older than {} days.", self.days);
            return Ok(());
        }
        let question = format!("Delete {count} entries older than {} days?", self.days);
        if !self.force && !renox::prompt::confirm(&question, true).await? {
            return Ok(());
        }
        let deleted = old().delete(&state.db).await?;
        println!("Deleted {deleted} entries older than {} days.", self.days);
        Ok(())
    }
}

/// What `guestbook/index.html` gets: a typed context, so `view:check` knows the page's
/// variables.
#[derive(Serialize, ViewData)]
#[view(template = "guestbook/index.html")]
struct GuestbookPage {
    entries: Paginated<Entry>,
}

async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
    let entries = Entry::query().latest().paginate(&db, page, 10).await?;
    Ok(View::typed(GuestbookPage { entries }).fragment("entries"))
}

/// Invalid input never gets here: `Valid` sends regular posts back with the
/// errors and old input, and HTMX posts get a 422 the bundled script shows
/// next to the fields.
async fn store(
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    back: Back,
    lang: Lang,
    Valid(form): Valid<EntryForm>,
) -> Result<Response> {
    let photo = match &form.photo {
        Some(photo) => Some(photo.store_public(&state.storage, "entries").await?),
        None => None,
    };
    let entry = Entry::create(
        &state.db,
        Entry {
            name: form.name,
            message: form.message,
            photo,
            ..Default::default()
        },
    )
    .await?;
    state.emit(EntryPosted { entry_id: entry.id }).await?;

    if htmx.request {
        let entries = Entry::query().latest().paginate(&state.db, 1, 10).await?;
        return Ok((
            HxTrigger("entry-added".into()),
            View::typed(GuestbookPage { entries }).fragment("entries"),
        )
            .into_response());
    }
    session.flash(
        "status",
        lang.t("guestbook.thanks", &[("name", &entry.name)]),
    )?;
    Ok(back.into_response())
}

/// Remembers the visitor's language and goes back to where they were.
async fn switch_language(session: Session, back: Back, Path(locale): Path<String>) -> Result<Back> {
    renox::i18n::remember_locale(&session, &locale)?;
    Ok(back)
}

async fn greet(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}

/// The guestbook app; `main.rs` runs it and the tests boot it.
pub fn app() -> App {
    App::new()
        .embed(renox::embedded!())
        .migrations(renox::migrations!())
        .module(Auth::new().account().redirect_to("/")) // login, register, /account
        .module(Guestbook)
        // A visitor who hasn't picked a language gets their browser's (en or
        // es), else APP_LOCALE; /language/{locale} still wins.
        .detect_locale()
        .seeder(|state| async move {
            let db = state.db.clone();
            Entry::factory().count(30).create(&db).await?;
            Ok(())
        })
}

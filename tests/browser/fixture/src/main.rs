//! The pages `tests/browser` drives in headless Chrome: forms that fail
//! validation, the UI kit's widgets and overlays. Each page uses the kit
//! the way an app would; the browser tests check what renox.js and
//! renox-ui.js then do with it. The data grid's pages are in `grid.rs`.

mod grid;
mod parties;
mod tickets;

use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// A form that fails in every way the error placement handles.
#[derive(Deserialize, Serialize, Validate)]
struct Signup {
    #[validate(required, max = 10)]
    name: String,
    #[validate(required, email)]
    email: String,
    #[serde(default)]
    #[validate(each(required, max = 5))]
    tags: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Line {
    name: String,
    #[serde(default)]
    qty: Option<i64>,
}

/// Rows of a repeater (nested names: `lines[0][name]`).
#[derive(Deserialize, Serialize)]
struct Order {
    #[serde(default)]
    lines: Vec<Line>,
}

impl Validate for Order {
    fn rules(&self, v: &mut Validator) {
        for (i, line) in self.lines.iter().enumerate() {
            v.field(&format!("lines.{i}.name"), &line.name).required();
            v.field(&format!("lines.{i}.qty"), &line.qty)
                .required()
                .min(1);
        }
    }
}

/// A wizard's two steps.
#[derive(Deserialize, Serialize, Validate)]
struct Trip {
    #[validate(required)]
    from: String,
    #[validate(required)]
    to: String,
}

/// Fields with no error slot: renox.js inserts its own error paragraphs,
/// and finds a list item's input among several of one name.
#[derive(Deserialize, Serialize, Validate)]
struct Notes {
    #[validate(required, max = 3)]
    note: String,
    #[serde(default)]
    #[validate(each(max = 3))]
    tags: Vec<String>,
}

/// A notification for the bell (database only).
struct Hello;

impl renox::auth::Notification for Hello {
    fn kind(&self) -> &'static str {
        "hello"
    }

    fn channels(&self, _to: &renox::auth::Recipient) -> Vec<renox::auth::Channel> {
        vec![renox::auth::Channel::Database]
    }

    fn to_database(
        &self,
        _to: &renox::auth::Recipient,
        _state: &AppState,
    ) -> Result<renox::serde_json::Value> {
        Ok(renox::auth::DatabaseMessage::info("Hello there").into())
    }
}

/// The searchable select's server-side options (`options_url`), kept in
/// memory: searched, added and renamed.
static CATEGORIES: std::sync::Mutex<Vec<(i64, String)>> = std::sync::Mutex::new(Vec::new());

fn categories() -> std::sync::MutexGuard<'static, Vec<(i64, String)>> {
    let mut list = CATEGORIES.lock().unwrap();
    if list.is_empty() {
        list.extend([
            (1, "Coffee".to_owned()),
            (2, "Tea".to_owned()),
            (3, "Cocoa".to_owned()),
        ]);
    }
    list
}

#[derive(Deserialize)]
struct CategoryForm {
    #[serde(default)]
    value: Option<i64>,
    label: String,
}

async fn category_options(
    query: renox::select::OptionQuery,
) -> Json<Vec<renox::select::SelectOption>> {
    let needle = query.q.to_lowercase();
    Json(
        categories()
            .iter()
            .filter(|(_, label)| label.to_lowercase().contains(&needle))
            .map(|(id, label)| renox::select::SelectOption::new(id, label.clone()))
            .collect(),
    )
}

async fn add_category(
    renox::axum::Form(form): renox::axum::Form<CategoryForm>,
) -> Json<renox::select::SelectOption> {
    let mut list = categories();
    let id = list.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    list.push((id, form.label.clone()));
    Json(renox::select::SelectOption::new(id, form.label))
}

async fn rename_category(
    renox::axum::Form(form): renox::axum::Form<CategoryForm>,
) -> Json<renox::select::SelectOption> {
    let id = form.value.unwrap_or_default();
    if let Some(entry) = categories().iter_mut().find(|(i, _)| *i == id) {
        entry.1 = form.label.clone();
    }
    Json(renox::select::SelectOption::new(id, form.label))
}

/// A live component (`rx-click`, `rx-submit`, morphing): tests/browser/live.test.mjs.
#[derive(Default, Serialize, Deserialize)]
struct Counter {
    count: i64,
    name: String,
    search: String,
    nick: String,
    lazy: String,
    title: String,
}

impl Validate for Counter {
    fn rules(&self, v: &mut Validator) {
        v.field("title", &self.title).required();
    }
}

impl LiveComponent for Counter {
    const NAME: &'static str = "counter";
    const VIEW: &'static str = "live/counter.html";

    async fn data(
        &self,
        _ctx: &renox::live_component::LiveContext,
    ) -> Result<renox::serde_json::Value> {
        Ok(json!({ "label": "clicks" }))
    }

    async fn call(
        &mut self,
        action: &str,
        args: Vec<renox::serde_json::Value>,
        ctx: &mut renox::live_component::LiveContext,
    ) -> Result {
        match action {
            "increment" => self.count += 1,
            "add" => {
                self.count += args.first().and_then(|n| n.as_i64()).unwrap_or(0);
            }
            "rename" => {}
            "save" => ctx.validate(&*self).await?,
            "notify" => ctx.toast(Toast::success("Saved from live.")),
            "go" => ctx.redirect("/stock"),
            "ping" => ctx.dispatch("pinged", json!({ "n": 1 })),
            _ => return Err(Error::NotFound),
        }
        Ok(())
    }
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { view("home.html", context! {}) })
            .name("home")
            // A route on its own host, so `route:list` shows the DOMAIN
            // column (tests/process).
            .domain(
                "{team}.fixture.test",
                Routes::new().get("/team", || async { "team" }),
            )
            .get("/form", || async { view("form.html", context! {}) })
            .post("/signup", |Valid(form): Valid<Signup>| async move {
                Toast::success(format!("Welcome, {}.", form.name))
            })
            .get("/plain", || async { view("plain.html", context! {}) })
            .get("/tags", || async { view("tags.html", context! { n => 2 }) })
            .post("/plain", |Valid(form): Valid<Signup>| async move {
                (
                    Toast::success(format!("Saved {}.", form.name)),
                    Redirect::to("/plain"),
                )
            })
            .get("/widgets", || async { view("widgets.html", context! {}) })
            .post("/order", |Valid(order): Valid<Order>| async move {
                format!("{} lines", order.lines.len())
            })
            .post("/trip", |Valid(trip): Valid<Trip>| async move {
                format!("{} to {}", trip.from, trip.to)
            })
            .post("/slow", || async {
                renox::tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                "done"
            })
            .post("/slow-plain", || async {
                renox::tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                Redirect::to("/widgets")
            })
            .get("/overlays", || async { view("overlays.html", context! {}) })
            .get(
                "/live",
                |ctx: renox::live_component::LiveContext| async move {
                    let counter = ctx.mount(Counter::default()).await?;
                    Ok::<_, Error>(view("live.html", context! { counter }))
                },
            )
            .get("/events", || async { view("events.html", context! {}) })
            .get("/events-tags", || async {
                view("events_tags.html", context! {})
            })
            .get("/events-tags/stamp", || async {
                static LOADS: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let n = LOADS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                format!("loads: {n}")
            })
            .post("/confirm-done", || async { "done" })
            // The navbar with a phone tab bar and a search behind a button.
            .get("/tabs", || async { view("tabs.html", context! {}) })
            .name("tabs.home")
            .get("/tabs/orders", || async { view("tabs.html", context! {}) })
            .name("tabs.orders")
            // A sidebar drawn from a menu (`<rx-sidebar menu="staff"/>`).
            .get("/menu", || async { view("menu.html", context! {}) })
            .name("overview.index")
            .get("/menu/reports", || async { view("menu.html", context! {}) })
            .name("reports.index")
            .get("/menu/reports/{id}", |Path(_id): Path<i64>| async {
                view("menu.html", context! {})
            })
            .name("reports.show")
            .post("/toast", || async {
                Toast::warning("Stock is low.")
                    .link("Unsafe", "javascript:alert(1)")
                    .link("Open stock", "/stock")
                    .seconds(1)
            })
            .post("/sheet", |Valid(form): Valid<Trip>| async move {
                (Toast::success(format!("{} saved.", form.from)), "ok")
            })
            .get("/stock", || async { "stock page" })
            // renox.js: errors without slots; analytics events.
            .get("/errors", || async { view("errors.html", context! {}) })
            .post("/notes", |Valid(notes): Valid<Notes>| async move {
                format!("{} saved", notes.note)
            })
            .post("/track", |session: Session| async move {
                renox::analytics::event(&session, "signup", json!({ "plan": "pro" }))?;
                Ok::<_, Error>("tracked")
            })
            .get("/tracked", |session: Session| async move {
                renox::analytics::event(&session, "page_seen", json!({ "page": "tracked" }))?;
                Ok::<_, Error>(view("home.html", context! {}))
            })
            .get("/track-then-go", |session: Session| async move {
                renox::analytics::event(&session, "went", json!({}))?;
                Ok::<_, Error>(Redirect::to("/"))
            })
            // renox-ui.js: what a form sent, toasts that wait for the next
            // page, a toast action's answers.
            .post(
                "/echo",
                |renox::axum::Form(fields): renox::axum::Form<Vec<(String, String)>>| async move {
                    fields
                        .into_iter()
                        .filter(|(k, _)| k != "_token")
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join("&")
                },
            )
            .post("/toast-redirect", || async {
                (Toast::success("Moved along."), HxRedirect("/".into()))
            })
            .post("/toast-refresh", || async {
                (Toast::info("Fresh again."), HxRefresh)
            })
            .post("/undo", || async { Toast::success("Undone.") })
            .post("/fails", || async {
                Err::<String, _>(Error::BadRequest("no".into()))
            })
            .get("/categories", category_options)
            .post("/categories", add_category)
            .put("/categories", rename_category)
            .post("/picked", || async { "picked" })
            .get("/charts", || async { view("charts.html", context! {}) })
            .get("/parts", || async { view("parts.html", context! {}) })
            .get("/parts/more", || async {
                view("parts_more.html", context! {})
            })
            .get("/nav", || async { view("nav.html", context! {}) })
            .get("/shell", || async { view("shell.html", context! {}) })
            // Right-to-left (#28): a Latin page with a chevron, and one with
            // Arabic text, for the font network check.
            .get("/rtl", || async { view("rtl.html", context! {}) })
            .get("/arabic", || async { view("arabic.html", context! {}) })
            // The dashboard standard, `<rx-shell fill>` (#24).
            .get("/shell-fill", || async {
                view("shell_fill.html", context! { tall => false })
            })
            .get("/shell-fill/tall", || async {
                view("shell_fill.html", context! { tall => true })
            })
            .get("/shell-fill/more", || async {
                Html(r#"<li id="loaded">More orders</li>"#)
            })
            // The bell: a page with it, and a notification for the user.
            .get("/inbox", || async { view("inbox.html", context! {}) })
            .post(
                "/notify-me",
                |State(state): State<AppState>, user: AuthUser| async move {
                    state.notify(&*user, &Hello).await?;
                    Ok::<_, Error>("sent")
                },
            )
            // A request still running when the server is told to stop.
            .get("/pause", || async {
                renox::tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                "finished"
            })
    }
}

/// Appends `line` to the file `FIXTURE_LOG` names, for tests/process to read.
fn note(line: &str) {
    use std::io::Write;
    if let Ok(path) = std::env::var("FIXTURE_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// A job that notes it ran.
#[derive(Serialize, Deserialize)]
struct Touch {
    n: u32,
}

impl Job for Touch {
    const NAME: &'static str = "fixture-touch";
    async fn handle(self, _ctx: JobContext) -> Result {
        note(&format!("job {}", self.n));
        Ok(())
    }
}

/// A job that takes a while, so tests/process can stop the app while it runs.
#[derive(Serialize, Deserialize)]
struct Nap;

impl Job for Nap {
    const NAME: &'static str = "fixture-nap";
    async fn handle(self, _ctx: JobContext) -> Result {
        note("nap started");
        renox::tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        note("nap done");
        Ok(())
    }
}

fn main() -> Result {
    let mut app = App::new()
        // The grid pages' tables (orders, customers, notes).
        .migrations(renox::migrations!())
        // Accounts (a login rotates the CSRF token) and the bell.
        // Passwordless login (passwordless.test.mjs): any address that proves
        // it owns its mailbox gets in, by link or by code.
        .module(
            Auth::new()
                .notifications()
                .magic_link()
                .register_on_first_use()
                .login_codes(renox::auth::LoginCodes::new()),
        )
        // The data grid's pages (src/grid.rs), and their `db:seed`.
        .module(grid::Orders)
        .seeder(grid::seed)
        .module(Pages)
        // A ticket with a state machine (transition-actions.test.mjs).
        .module(tickets::Tickets)
        // Parties with custom fields (custom-fields.test.mjs).
        .module(parties::Parties)
        .state_machine::<tickets::Ticket>()
        .nav(
            "staff",
            renox::nav::NavLink::route("overview.index", "Overview").order(1),
        )
        .nav(
            "staff",
            renox::nav::NavLink::route("reports.index", "Reports")
                .group("Billing")
                .order(10),
        )
        .nav(
            "staff",
            renox::nav::NavLink::url("/widgets", "Widgets")
                .group("Billing")
                .order(20),
        )
        .nav(
            "staff",
            renox::nav::NavLink::url("/secret", "Secret")
                .group("Billing")
                .order(30)
                .gate("nobody"),
        )
        .live_component::<Counter>()
        .job::<Touch>()
        .job::<Nap>()
        // `jobs:nap`: queues one Nap job.
        .command("jobs:nap", "Queues a slow job", |_args, state| async move {
            state.dispatch(Nap).await?;
            Ok(())
        })
        // `jobs:push 3`: queues three Touch jobs.
        .command("jobs:push", "Queues Touch jobs", |args, state| async move {
            let count: u32 = args
                .positional()
                .first()
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            for n in 1..=count {
                state.dispatch(Touch { n }).await?;
            }
            Ok(())
        })
        // `ask:me`: asks every kind of question and prints the answers.
        .command("ask:me", "Asks questions", |_args, _state| async move {
            let name = renox::prompt::ask("Name?").await?;
            let sure = renox::prompt::confirm("Sure?", false).await?;
            let size = renox::prompt::choice("Size?", &["small", "large"], None).await?;
            let secret = renox::prompt::secret("Secret?").await?;
            println!(
                "answers: {name} | {sure} | {size} | {} characters",
                secret.len()
            );
            Ok(())
        });
    // A task that can never run (hourly, but only between 10:30 and 10:40),
    // only when asked: `schedule:list` shows "never" (tests/process).
    if std::env::var("FIXTURE_NEVER").is_ok() {
        app = app.schedule(|s| {
            s.hourly("never-on-the-hour", |_state| async { Ok(()) })
                .between("10:30", "10:40");
        });
    }
    // A task every second, only when asked (tests/process).
    if std::env::var("FIXTURE_TICK").is_ok() {
        app = app.schedule(|s| {
            s.every(std::time::Duration::from_secs(1), "tick", |_state| async {
                note("tick");
                Ok(())
            });
        });
    }
    app.run()
}

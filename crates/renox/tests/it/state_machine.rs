//! #39: the machine definition API: states, transitions, access, guards,
//! effects, errors and the Mermaid diagram.

use renox::auth::permissions::Scope;
use renox::db::{Col, Dialect};
use renox::prelude::*;
use renox::state_machine::{
    Decision, Machine, Outcome, Preview, StateMachine, Transition, TransitionCtx, TransitionError,
    TransitionInterceptor, TransitionRecord, TransitionRequest, Transitioned, clear_interceptor,
    history, preview, set_interceptor,
};
use renox::testing::TestApp;

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum OrderStatus {
    Draft,
    Placed,
    Paid,
    Cancelled,
}

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "orders")]
struct Order {
    id: i64,
    status: String,
    lines: i64,
}

impl StateMachine for Order {
    type State = OrderStatus;
    const COLUMN: Col<Self, OrderStatus> = Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new()
            .initial([OrderStatus::Draft])
            .state(OrderStatus::Draft)
            .tone("neutral")
            .state(OrderStatus::Cancelled)
            .tone("danger")
            .terminal()
            .transition(
                Transition::new("place")
                    .from([OrderStatus::Draft])
                    .to(OrderStatus::Placed)
                    .permission("orders.place")
                    .guard("has_lines", |order: &Order, _ctx| {
                        Box::pin(async move {
                            if order.lines > 0 {
                                Ok(())
                            } else {
                                Err("The order has no lines.".into())
                            }
                        })
                    })
                    .guard("input_ok", |_order, ctx| {
                        Box::pin(async move { ctx.input_as::<Option<i64>>().map(|_| ()) })
                    })
                    .effect(|order: &Order, ctx, tx| {
                        Box::pin(async move {
                            TransitionRecord::new("orders", order.id, "placed", "place")
                                .from("draft")
                                .reason(ctx.reason().unwrap_or("none"))
                                .record(tx)
                                .await?;
                            Ok(())
                        })
                    }),
            )
            .transition(
                Transition::new("pay")
                    .from([OrderStatus::Placed])
                    .to(OrderStatus::Paid)
                    .permission_in("orders.pay", |order: &Order| Scope::new("stores", order.id)),
            )
            .transition(
                Transition::new("auto_cancel")
                    .label("Cancel unpaid")
                    .tone("danger")
                    .from([OrderStatus::Draft, OrderStatus::Placed])
                    .to(OrderStatus::Cancelled)
                    .system()
                    .reason_required(),
            )
    }
}

fn order(lines: i64) -> Order {
    Order {
        id: 1,
        status: "draft".into(),
        lines,
    }
}

#[test]
fn machine_describes_states_and_transitions() {
    let m = Order::machine();
    let names: Vec<_> = m.states().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["draft", "cancelled", "placed", "paid"]);
    assert_eq!(m.states()[0].tone.as_deref(), Some("neutral"));
    assert!(m.states()[1].terminal && !m.states()[0].terminal);
    assert_eq!(m.initial_states(), ["draft"]);

    let place = m.find("place").unwrap();
    assert_eq!(place.label_text(), "Place");
    assert_eq!(place.from_states(), ["draft"]);
    assert_eq!(place.to_state(), Some("placed"));
    assert_eq!(place.guard_names(), ["has_lines", "input_ok"]);
    let auto = m.find("auto_cancel").unwrap();
    assert_eq!(auto.label_text(), "Cancel unpaid");
    assert_eq!(auto.tone_name(), Some("danger"));
    assert!(auto.is_system() && auto.needs_reason());
    assert!(matches!(m.find("nope"), Err(TransitionError::Unknown(n)) if n == "nope"));

    let from_placed: Vec<_> = m.from_state("placed").map(|t| t.name()).collect();
    assert_eq!(from_placed, ["pay", "auto_cancel"]);
}

#[test]
fn from_and_access_are_checked() {
    let m = Order::machine();
    let place = m.find("place").unwrap();
    assert!(place.check_from(OrderStatus::Draft).is_ok());
    assert!(matches!(
        place.check_from(OrderStatus::Paid),
        Err(TransitionError::NotFrom(s)) if s == "paid"
    ));
    // Nobody logged in: only a system transition accepts that.
    assert!(matches!(
        place.check_access(&order(1), None),
        Err(TransitionError::Unauthenticated)
    ));
    assert!(
        m.find("auto_cancel")
            .unwrap()
            .check_access(&order(1), None)
            .is_ok()
    );
    assert!(matches!(
        m.find("pay").unwrap().check_access(&order(1), None),
        Err(TransitionError::Unauthenticated)
    ));
}

#[tokio::test]
async fn guards_run_in_order_and_stop_at_the_first_no() {
    let m = Order::machine();
    let place = m.find("place").unwrap();
    let ctx = TransitionCtx::new(None);
    assert!(place.check_guards(&order(2), &ctx).await.is_ok());
    let err = place.check_guards(&order(0), &ctx).await.unwrap_err();
    assert!(matches!(&err, TransitionError::Guard(m) if m == "The order has no lines."));
    assert_eq!(err.to_string(), "The order has no lines.");

    // The second guard sees the input.
    let bad = TransitionCtx::new(None).with_input(serde_json::json!("not a number"));
    assert!(matches!(
        place.check_guards(&order(2), &bad).await,
        Err(TransitionError::Guard(_))
    ));
    let good = TransitionCtx::new(None).with_input(serde_json::json!(5));
    assert!(place.check_guards(&order(2), &good).await.is_ok());
}

#[tokio::test]
async fn effects_run_in_the_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.env = Environment::Testing;
    config.key = Some(renox::generate_key());
    config.views_path = dir.path().to_path_buf();
    let kernel = App::with_config(config).boot().await.unwrap();
    kernel.migrate().await.unwrap();
    let db = kernel.db();

    let m = Order::machine();
    let place = m.find("place").unwrap();
    let ctx = TransitionCtx::new(None).with_reason("phone call");

    let mut tx = db.begin().await.unwrap();
    place.run_effects(&order(1), &ctx, &mut tx).await.unwrap();
    drop(tx); // rolled back
    assert!(history(db, "orders", 1).await.unwrap().is_empty());

    let mut tx = db.begin().await.unwrap();
    place.run_effects(&order(1), &ctx, &mut tx).await.unwrap();
    tx.commit().await.unwrap();
    let log = history(db, "orders", 1).await.unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].reason.as_deref(), Some("phone call"));
}

#[test]
fn mermaid_matches_snapshot() {
    assert_eq!(
        Order::machine().mermaid(),
        "stateDiagram-v2\n\
         \x20   [*] --> draft\n\
         \x20   draft --> placed: place\n\
         \x20   placed --> paid: pay\n\
         \x20   draft --> cancelled: auto_cancel\n\
         \x20   placed --> cancelled: auto_cancel\n\
         \x20   cancelled --> [*]\n"
    );
}

#[test]
#[should_panic(expected = "leaves the terminal state `cancelled`")]
fn leaving_a_terminal_state_is_refused() {
    let _ = Machine::<Order>::new()
        .state(OrderStatus::Cancelled)
        .terminal()
        .transition(
            Transition::new("revive")
                .from([OrderStatus::Cancelled])
                .to(OrderStatus::Draft),
        );
}

#[test]
#[should_panic(expected = "defined twice")]
fn duplicate_transition_names_are_refused() {
    let t = || {
        Transition::<Order>::new("go")
            .from([OrderStatus::Draft])
            .to(OrderStatus::Placed)
    };
    let _ = Machine::new().transition(t()).transition(t());
}

// The engine.

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum TicketStatus {
    Open,
    Closed,
}

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "tickets")]
struct Ticket {
    id: i64,
    status: String,
}

impl StateMachine for Ticket {
    type State = TicketStatus;
    const COLUMN: Col<Self, TicketStatus> = Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new()
            .transition(
                Transition::new("close")
                    .from([TicketStatus::Open])
                    .to(TicketStatus::Closed)
                    .system()
                    .reason_required()
                    .guard("not_locked", |_t: &Ticket, ctx| {
                        Box::pin(async move {
                            if ctx.input() == "locked" {
                                Err("The ticket is locked.".into())
                            } else {
                                Ok(())
                            }
                        })
                    })
                    .effect(|_t: &Ticket, ctx, _tx| {
                        Box::pin(async move {
                            if ctx.input() == "boom" {
                                Err(renox::Error::Internal(anyhow::anyhow!("boom")))
                            } else {
                                Ok(())
                            }
                        })
                    }),
            )
            .transition(
                Transition::new("staff_only")
                    .from([TicketStatus::Open])
                    .to(TicketStatus::Closed)
                    .permission("tickets.close"),
            )
    }
}

pub(crate) async fn ticket_app() -> TestApp {
    let app = TestApp::new(App::new()).await;
    renox::db::sql(format!(
        "CREATE TABLE tickets ({}, status TEXT NOT NULL)",
        pk(app.db())
    ))
    .execute(app.db())
    .await
    .unwrap();
    renox::db::sql("INSERT INTO tickets (id, status) VALUES (1, 'open')")
        .execute(app.db())
        .await
        .unwrap();
    app
}

fn ticket() -> Ticket {
    Ticket {
        id: 1,
        status: "open".into(),
    }
}

async fn ticket_status(app: &TestApp) -> String {
    renox::db::sql("SELECT status FROM tickets WHERE id = 1")
        .scalar(app.db())
        .await
        .unwrap()
}

#[tokio::test]
async fn run_moves_the_status_writes_history_and_emits_after_commit() {
    let app = ticket_app().await;
    let done = ticket()
        .transition("close")
        .by_system()
        .reason("resolved")
        .meta(serde_json::json!({"via": "test"}))
        .run(app.state())
        .await
        .unwrap()
        .into_done()
        .unwrap();
    assert_eq!((done.from.as_str(), done.to.as_str()), ("open", "closed"));
    assert_eq!(done.meta["via"], "test");
    assert_eq!(ticket_status(&app).await, "closed");
    let log = history(app.db(), "tickets", 1).await.unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].reason.as_deref(), Some("resolved"));
    assert_eq!(log[0].actor_id, None);
    assert_eq!(log[0].metadata["via"], "test");
}

#[tokio::test]
async fn checks_run_in_order_and_refuse_with_typed_errors() {
    let app = ticket_app().await;
    let state = app.state();
    assert!(matches!(
        ticket().transition("close").reason("r").run(state).await,
        Err(TransitionError::Unauthenticated)
    ));
    assert!(matches!(
        ticket().transition("nope").by_system().run(state).await,
        Err(TransitionError::Unknown(_))
    ));
    assert!(matches!(
        ticket()
            .transition("staff_only")
            .by_system()
            .run(state)
            .await,
        Err(TransitionError::Unauthenticated)
    ));
    assert!(matches!(
        ticket().transition("close").by_system().input(&"locked").run(state).await,
        Err(TransitionError::Guard(m)) if m == "The ticket is locked."
    ));
    assert!(matches!(
        ticket().transition("close").by_system().run(state).await,
        Err(TransitionError::ReasonRequired)
    ));
    let closed = Ticket {
        id: 1,
        status: "closed".into(),
    };
    assert!(matches!(
        closed.transition("close").by_system().reason("r").run(state).await,
        Err(TransitionError::NotFrom(s)) if s == "closed"
    ));
    assert_eq!(ticket_status(&app).await, "open");
}

#[tokio::test]
async fn a_stale_record_gets_a_conflict() {
    let app = ticket_app().await;
    let state = app.state();
    let first = ticket();
    let second = ticket();
    let _ = first
        .transition("close")
        .by_system()
        .reason("a")
        .run(state)
        .await
        .unwrap();
    assert!(matches!(
        second
            .transition("close")
            .by_system()
            .reason("b")
            .run(state)
            .await,
        Err(TransitionError::Conflict)
    ));
    assert_eq!(history(app.db(), "tickets", 1).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_failing_effect_rolls_everything_back() {
    let app = ticket_app().await;
    let mut tx = app.db().begin().await.unwrap();
    let err = ticket()
        .transition("close")
        .by_system()
        .reason("r")
        .input(&"boom")
        .run_in(&mut tx)
        .await;
    assert!(matches!(err, Err(TransitionError::Failed(_))));
    drop(tx);
    assert_eq!(ticket_status(&app).await, "open");
    assert!(history(app.db(), "tickets", 1).await.unwrap().is_empty());
}

#[tokio::test]
async fn run_in_does_not_commit_or_emit() {
    let app = ticket_app().await;
    app.fake_events();
    let mut tx = app.db().begin().await.unwrap();
    let done = ticket()
        .transition("close")
        .by_system()
        .reason("r")
        .run_in(&mut tx)
        .await
        .unwrap();
    assert!(app.emitted::<Transitioned<Ticket>>().is_empty());
    tx.commit().await.unwrap();
    done.emit(app.state()).await.unwrap();
    assert_eq!(app.emitted::<Transitioned<Ticket>>().len(), 1);
    assert_eq!(ticket_status(&app).await, "closed");
}

/// Starts `contenders` transitions on the same ticket at once and checks that
/// exactly one wins, the others get `Conflict`, and one history row remains.
/// Shared with the PostgreSQL run in `postgres.rs`.
pub(crate) async fn race_for_one_winner(app: &TestApp, contenders: usize) {
    let mut tasks = Vec::new();
    for i in 0..contenders {
        let state = app.state().clone();
        tasks.push(tokio::spawn(async move {
            let reason = format!("racer {i}");
            ticket()
                .transition("close")
                .by_system()
                .reason(&reason)
                .run(&state)
                .await
                .map(|_| ())
        }));
    }
    let mut won = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(()) => won += 1,
            Err(TransitionError::Conflict) => {}
            Err(other) => panic!("expected success or a conflict, got {other:?}"),
        }
    }
    assert_eq!(won, 1, "exactly one concurrent transition succeeds");
    assert_eq!(ticket_status(app).await, "closed");
    assert_eq!(history(app.db(), "tickets", 1).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_transitions_on_one_record_allow_exactly_one() {
    let app = ticket_app().await;
    race_for_one_winner(&app, 8).await;
}

#[tokio::test]
async fn a_failing_effect_through_run_emits_nothing_and_keeps_the_old_status() {
    let app = ticket_app().await;
    app.fake_events();
    let err = ticket()
        .transition("close")
        .by_system()
        .reason("r")
        .input(&"boom")
        .run(app.state())
        .await;
    assert!(matches!(err, Err(TransitionError::Failed(_))));
    assert_eq!(ticket_status(&app).await, "open");
    assert!(history(app.db(), "tickets", 1).await.unwrap().is_empty());
    assert!(app.emitted::<Transitioned<Ticket>>().is_empty());
}

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum DocStatus {
    Open,
    Signed,
}

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "sm_docs")]
struct Doc {
    id: i64,
    status: String,
}

impl StateMachine for Doc {
    type State = DocStatus;
    const COLUMN: Col<Self, DocStatus> = Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new().transition(
            Transition::new("sign")
                .from([DocStatus::Open])
                .to(DocStatus::Signed)
                .system()
                .effect(|_d: &Doc, _ctx, tx| {
                    Box::pin(async move {
                        renox::db::sql("INSERT INTO sm_log (note) VALUES ('effect')")
                            .execute(&mut *tx)
                            .await?;
                        Ok(())
                    })
                }),
        )
    }
}

/// Defers `sign` runs of `sm_docs` whose input is `"defer"` and stores a
/// request row; lets every other run (and model) through.
struct Approvals;

impl TransitionInterceptor for Approvals {
    fn intercept<'a>(
        &'a self,
        request: &'a TransitionRequest,
        tx: &'a mut renox::db::Transaction,
    ) -> futures_util::future::BoxFuture<'a, renox::Result<Decision>> {
        Box::pin(async move {
            if request.model != "sm_docs" || request.input != "defer" {
                return Ok(Decision::Proceed);
            }
            renox::db::sql("INSERT INTO sm_log (note) VALUES (?)")
                .bind(format!("request {} {}", request.key, request.transition))
                .execute(&mut *tx)
                .await?;
            Ok(Decision::Defer(serde_json::json!({"request": 1})))
        })
    }

    fn preview<'a>(
        &'a self,
        request: &'a TransitionRequest,
        _db: &'a renox::db::Db,
    ) -> futures_util::future::BoxFuture<'a, renox::Result<Preview>> {
        Box::pin(async move {
            Ok(if request.model == "sm_docs" && request.input == "defer" {
                Preview::RequiresApproval
            } else {
                Preview::Proceed
            })
        })
    }
}

/// An auto-numbered `id` primary key column definition for the connected engine.
fn pk(db: &Db) -> &'static str {
    match db.dialect() {
        Dialect::Sqlite => "id INTEGER PRIMARY KEY",
        Dialect::Postgres => "id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY",
    }
}

/// The interceptor is process-global: tests that register one hold this lock.
static INTERCEPTOR_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn an_interceptor_defers_or_lets_a_run_proceed() {
    let _one_interceptor = INTERCEPTOR_LOCK.lock().await;
    let app = TestApp::new(App::new()).await;
    app.fake_events();
    let db = app.db();
    for q in [
        format!("CREATE TABLE sm_docs ({}, status TEXT NOT NULL)", pk(db)),
        format!("CREATE TABLE sm_log ({}, note TEXT NOT NULL)", pk(db)),
        "INSERT INTO sm_docs (id, status) VALUES (1, 'open')".to_string(),
    ] {
        renox::db::sql(q).execute(db).await.unwrap();
    }
    set_interceptor(Approvals);
    let doc = Doc {
        id: 1,
        status: "open".into(),
    };
    let status = || async {
        renox::db::sql("SELECT status FROM sm_docs WHERE id = 1")
            .scalar::<String>(db)
            .await
            .unwrap()
    };
    let notes = || async {
        let rows = renox::db::sql("SELECT note FROM sm_log ORDER BY id")
            .fetch_all(db)
            .await
            .unwrap();
        rows.iter()
            .map(|r| r.try_get::<String>("note").unwrap())
            .collect::<Vec<_>>()
    };

    // Preview marks the action and writes nothing.
    let req = TransitionRequest::new("sm_docs", 1, "sign", "open", "signed")
        .with_input(serde_json::json!("defer"));
    assert_eq!(preview(&req, db).await.unwrap(), Preview::RequiresApproval);
    assert!(notes().await.is_empty());

    // Defer: Ok(Pending), the interceptor's write is kept, nothing moved,
    // no effect, no history, nothing emitted.
    let outcome = doc
        .transition("sign")
        .by_system()
        .input(&"defer")
        .run(app.state())
        .await
        .unwrap();
    match &outcome {
        Outcome::Pending(p) => {
            assert_eq!((p.from.as_str(), p.to.as_str()), ("open", "signed"));
            assert_eq!(p.data["request"], 1);
        }
        Outcome::Done(_) => panic!("expected a pending outcome"),
    }
    assert_eq!(status().await, "open");
    assert_eq!(notes().await, ["request 1 sign"]);
    assert!(history(db, "sm_docs", 1).await.unwrap().is_empty());
    assert!(app.emitted::<Transitioned<Doc>>().is_empty());

    // Proceed: behaves normally.
    let outcome = doc
        .transition("sign")
        .by_system()
        .run(app.state())
        .await
        .unwrap();
    assert!(!outcome.is_pending());
    assert_eq!(status().await, "signed");
    assert_eq!(notes().await, ["request 1 sign", "effect"]);
    assert_eq!(app.emitted::<Transitioned<Doc>>().len(), 1);
    clear_interceptor();
}

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum MemoStatus {
    Draft,
    Published,
}

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "sm_memos")]
struct Memo {
    id: i64,
    status: String,
}

impl StateMachine for Memo {
    type State = MemoStatus;
    const COLUMN: Col<Self, MemoStatus> = Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new().transition(
            Transition::new("publish")
                .from([MemoStatus::Draft])
                .to(MemoStatus::Published),
        )
    }
}

/// Asks for approval of every `sm_memos` run and counts what it is asked.
struct CountingApprovals {
    intercepts: &'static std::sync::atomic::AtomicUsize,
    previews: &'static std::sync::atomic::AtomicUsize,
}

impl TransitionInterceptor for CountingApprovals {
    fn intercept<'a>(
        &'a self,
        _request: &'a TransitionRequest,
        _tx: &'a mut renox::db::Transaction,
    ) -> futures_util::future::BoxFuture<'a, renox::Result<Decision>> {
        self.intercepts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok(Decision::Proceed) })
    }

    fn preview<'a>(
        &'a self,
        request: &'a TransitionRequest,
        _db: &'a renox::db::Db,
    ) -> futures_util::future::BoxFuture<'a, renox::Result<Preview>> {
        self.previews
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            Ok(if request.model == "sm_memos" {
                Preview::RequiresApproval
            } else {
                Preview::Proceed
            })
        })
    }
}

struct MemoRoutes;

impl Module for MemoRoutes {
    fn name(&self) -> &'static str {
        "memo-routes"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get(
                "/memo-available",
                |State(state): State<AppState>, user: AuthUser| async move {
                    let memo = Memo {
                        id: 1,
                        status: "draft".into(),
                    };
                    let list = memo.available(&user, &state.db).await?;
                    Ok::<_, renox::Error>(Json(list))
                },
            )
            .require_auth()
    }
}

static MEMO_INTERCEPTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MEMO_PREVIEWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[tokio::test]
async fn available_marks_approval_from_the_preview_without_side_effects() {
    use std::sync::atomic::Ordering;
    let _one_interceptor = INTERCEPTOR_LOCK.lock().await;
    let app = TestApp::new(App::new().module(Auth::new()).module(MemoRoutes)).await;
    let db = app.db();
    for q in [
        format!("CREATE TABLE sm_memos ({}, status TEXT NOT NULL)", pk(db)),
        "INSERT INTO sm_memos (id, status) VALUES (1, 'draft')".to_string(),
    ] {
        renox::db::sql(q).execute(db).await.unwrap();
    }
    let ana = User::register(db, "Ana", "ana@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&ana);

    // No interceptor: nothing needs approval.
    clear_interceptor();
    let body: serde_json::Value = app.get("/memo-available").await.assert_ok().json();
    assert_eq!(body[0]["name"], "publish");
    assert_eq!(body[0]["requires_approval"], false);

    set_interceptor(CountingApprovals {
        intercepts: &MEMO_INTERCEPTS,
        previews: &MEMO_PREVIEWS,
    });
    let body: serde_json::Value = app.get("/memo-available").await.assert_ok().json();
    clear_interceptor();
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["requires_approval"], true);
    // `available` only previews: it never intercepts, moves or logs anything.
    assert_eq!(MEMO_PREVIEWS.load(Ordering::SeqCst), 1);
    assert_eq!(MEMO_INTERCEPTS.load(Ordering::SeqCst), 0);
    let status: String = renox::db::sql("SELECT status FROM sm_memos WHERE id = 1")
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(status, "draft");
    assert!(history(db, "sm_memos", 1).await.unwrap().is_empty());
}

struct OrderRoutes;

impl Module for OrderRoutes {
    fn name(&self) -> &'static str {
        "order-routes"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get(
                "/available/{status}/{lines}",
                |State(state): State<AppState>,
                 user: AuthUser,
                 Path((status, lines)): Path<(String, i64)>| async move {
                    let order = Order {
                        id: 1,
                        status,
                        lines,
                    };
                    let list = order.available(&user, &state.db).await?;
                    Ok::<_, renox::Error>(Json(list))
                },
            )
            .get("/refuse/{kind}", |Path(kind): Path<String>| async move {
                Err::<(), _>(match kind.as_str() {
                    "login" => TransitionError::Unauthenticated,
                    "forbidden" => TransitionError::Forbidden,
                    "guard" => TransitionError::Guard("The order has no lines.".into()),
                    "from" => TransitionError::NotFrom("paid".into()),
                    "conflict" => TransitionError::Conflict,
                    "reason" => TransitionError::ReasonRequired,
                    _ => TransitionError::Unknown("nope".into()),
                })
            })
            .require_auth()
    }
}

#[tokio::test]
async fn available_lists_what_the_user_may_run_now() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(renox::auth::Permissions)
            .module(OrderRoutes),
    )
    .await;
    let db = app.db();
    let ana = User::register(db, "Ana", "ana@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&ana);

    // No permission yet: nothing, and the system transition never shows.
    app.get("/available/draft/1")
        .await
        .assert_ok()
        .assert_json(serde_json::json!([]));

    renox::auth::permissions::define_role(db, "clerk", &["orders.place"])
        .await
        .unwrap();
    ana.assign_role(db, "clerk").await.unwrap();
    app.get("/available/draft/1")
        .await
        .assert_ok()
        .assert_json(serde_json::json!([
            {"name": "place", "label": "Place", "tone": null, "needs_reason": false, "requires_approval": false}
        ]));
    // The guard hides it when it fails.
    app.get("/available/draft/0")
        .await
        .assert_ok()
        .assert_json(serde_json::json!([]));

    // `pay` needs `orders.pay` in the order's scope.
    app.get("/available/placed/1")
        .await
        .assert_ok()
        .assert_json(serde_json::json!([]));
    renox::auth::permissions::define_role(db, "cashier", &["orders.pay"])
        .await
        .unwrap();
    ana.assign_role_in(db, "cashier", &Scope::new("stores", 1))
        .await
        .unwrap();
    app.get("/available/placed/1")
        .await
        .assert_ok()
        .assert_json(serde_json::json!([
            {"name": "pay", "label": "Pay", "tone": null, "needs_reason": false, "requires_approval": false}
        ]));
}

#[tokio::test]
async fn transition_errors_answer_403_and_422() {
    let app = TestApp::new(App::new().module(Auth::new()).module(OrderRoutes)).await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&ana);
    app.get("/refuse/forbidden").await.assert_status(403);
    app.get("/refuse/login").await.assert_status(401);
    for kind in ["guard", "from", "conflict", "reason"] {
        app.get(&format!("/refuse/{kind}")).await.assert_status(422);
    }
    app.get("/refuse/guard")
        .await
        .assert_see("The order has no lines.");
    app.get("/refuse/unknown").await.assert_status(404);
}

#[test]
fn machines_are_found_by_name() {
    renox::state_machine::register::<Order>();
    let info = renox::state_machine::machine_info("Order").unwrap();
    assert_eq!(info.table, "orders");
    assert_eq!(info.mermaid, Order::machine().mermaid());
    let place = info.transitions.iter().find(|t| t.name == "place").unwrap();
    assert_eq!(place.guards, ["has_lines", "input_ok"]);
    assert!(
        info.transitions
            .iter()
            .any(|t| t.name == "auto_cancel" && t.system)
    );
    assert_eq!(renox::state_machine::machine_info("ORDERS"), Some(info));
    assert!(renox::state_machine::machine_info("Nope").is_none());
    assert!(renox::state_machine::machine_names().contains(&"Order".to_owned()));
}

#[renox::test]
async fn state_diagram_prints_a_registered_machine_and_refuses_unknown_ones() {
    let dir = tempfile::tempdir().unwrap();
    let make = || {
        let mut config = Config::default();
        config.database_url = format!("sqlite://{}", dir.path().join("app.db").display());
        config.storage_path = dir.path().join("storage");
        App::with_config(config).state_machine::<Order>()
    };
    make().run_args(["state:diagram"]).await.unwrap();
    make().run_args(["state:diagram", "Order"]).await.unwrap();
    make().run_args(["state:diagram", "orders"]).await.unwrap();
    assert!(make().run_args(["state:diagram", "Nope"]).await.is_err());
}

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum JobStatus {
    New,
    Running,
    Done,
}

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "guarded_jobs", state_machine)]
struct Job {
    id: i64,
    status: String,
    title: String,
}

impl StateMachine for Job {
    type State = JobStatus;
    const COLUMN: Col<Self, JobStatus> = Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new()
            .initial([JobStatus::New])
            .transition(
                Transition::new("start")
                    .from([JobStatus::New])
                    .to(JobStatus::Running)
                    .system(),
            )
            .transition(
                Transition::new("finish")
                    .from([JobStatus::Running])
                    .to(JobStatus::Done)
                    .system(),
            )
    }
}

async fn job_app() -> TestApp {
    let app = TestApp::new(App::new()).await;
    renox::db::sql(format!(
        "CREATE TABLE guarded_jobs ({}, status TEXT NOT NULL, title TEXT NOT NULL)",
        pk(app.db())
    ))
    .execute(app.db())
    .await
    .unwrap();
    app
}

fn is_refusal(error: &renox::Error) -> bool {
    matches!(error, renox::Error::Status(s, _) if s.as_u16() == 422)
}

#[tokio::test]
async fn a_guarded_status_is_refused_by_save_and_save_changes() {
    let app = job_app().await;
    let db = app.db();
    let mut job = Job {
        status: "new".into(),
        title: "a".into(),
        ..Default::default()
    };
    job.save(db).await.unwrap();

    // Other columns save; the status column is left alone.
    job.title = "b".into();
    job.save(db).await.unwrap();
    assert_eq!(Job::find_or_404(db, job.id).await.unwrap().title, "b");

    let original = Job::find_or_404(db, job.id).await.unwrap();
    let mut moved = original.clone();
    moved.status = "done".into();
    assert!(is_refusal(&moved.save(db).await.unwrap_err()));
    assert!(is_refusal(
        &moved.save_changes(db, &original).await.unwrap_err()
    ));
    assert!(is_refusal(
        &moved.save_only(db, &["status"]).await.unwrap_err()
    ));
    let mut renamed = original.clone();
    renamed.title = "c".into();
    assert!(renamed.save_changes(db, &original).await.unwrap());
    assert_eq!(Job::find_or_404(db, job.id).await.unwrap().status, "new");

    // A transition moves it; the stale record is then refused by save.
    let mut stale = Job::find_or_404(db, job.id).await.unwrap();
    let _ = stale
        .transition("start")
        .by_system()
        .run(app.state())
        .await
        .unwrap();
    assert!(is_refusal(&stale.save(db).await.unwrap_err()));
    let mut fresh = Job::find_or_404(db, job.id).await.unwrap();
    assert_eq!(fresh.status, "running");
    fresh.title = "d".into();
    fresh.save(db).await.unwrap();

    // A missing row is still a 404.
    let mut gone = Job {
        id: 99,
        status: "new".into(),
        ..Default::default()
    };
    assert!(matches!(
        gone.save(db).await.unwrap_err(),
        renox::Error::NotFound
    ));
}

#[tokio::test]
async fn inserts_honour_initial_and_insert_in_state_records_no_from() {
    let app = job_app().await;
    let db = app.db();
    let mut late = Job {
        status: "running".into(),
        title: "late".into(),
        ..Default::default()
    };
    assert!(is_refusal(&late.insert(db).await.unwrap_err()));
    assert!(is_refusal(
        &Job::insert_many(db, vec![late.clone()]).await.unwrap_err()
    ));
    assert!(Job::query().get(db).await.unwrap().is_empty());

    let mut tx = db.begin().await.unwrap();
    assert!(
        late.insert_in_state(&mut tx, JobStatus::Done, "x")
            .await
            .is_err()
    );
    late.insert_in_state(&mut tx, JobStatus::Running, "imported")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let log = history(db, "guarded_jobs", late.id).await.unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].from_state, None);
    assert_eq!(log[0].to_state, "running");
    assert_eq!(log[0].reason.as_deref(), Some("imported"));

    let mut fresh = Job {
        status: "new".into(),
        ..Default::default()
    };
    fresh.insert(db).await.unwrap();
}

// The UI: `<rx-transition-actions>`, the generic route and the history items.

struct TicketPages;

impl Module for TicketPages {
    fn name(&self) -> &'static str {
        "ticket-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get(
                "/tickets/{id}",
                |State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>| async move {
                    let ticket = Ticket::find(&state.db, id).await?.unwrap();
                    let actions = ticket.available(&user, &state.db).await?;
                    Ok::<_, renox::Error>(view("ticket.html", context! { actions }))
                },
            )
            .group(
                "/tickets",
                "tickets.",
                renox::state_machine::routes::<Ticket>(),
            )
            .require_auth()
    }
}

#[tokio::test]
async fn transition_actions_render_post_and_log() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ticket.html"),
        r#"<rx-transition-actions :actions="actions" action="/tickets/1/transitions" label="Ticket actions"/>"#,
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(renox::auth::Permissions)
            .module(TicketPages),
        move |c| c.views_path = path,
    )
    .await;
    let db = app.db();
    renox::db::sql(format!(
        "CREATE TABLE tickets ({}, status TEXT NOT NULL)",
        pk(db)
    ))
    .execute(db)
    .await
    .unwrap();
    renox::db::sql("INSERT INTO tickets (id, status) VALUES (1, 'open')")
        .execute(db)
        .await
        .unwrap();
    let ana = User::register(db, "Ana", "ana@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&ana);

    // No permission: no button, and the route refuses.
    app.get("/tickets/1")
        .await
        .assert_ok()
        .assert_dont_see("data-transition");
    app.post("/tickets/1/transitions/staff_only", &[])
        .await
        .assert_status(403);

    renox::auth::permissions::define_role(db, "support", &["tickets.close"])
        .await
        .unwrap();
    ana.assign_role(db, "support").await.unwrap();
    app.get("/tickets/1")
        .await
        .assert_ok()
        .assert_see(r#"data-transition="staff_only""#)
        .assert_see(r#"action="/tickets/1/transitions/staff_only""#)
        .assert_dont_see(r#"data-transition="close""#);

    app.post("/tickets/1/transitions/nope", &[])
        .await
        .assert_status(404);
    app.post("/tickets/9/transitions/staff_only", &[])
        .await
        .assert_status(404);
    // The system transition can't be run from the page.
    app.post("/tickets/1/transitions/close", &[("reason", "x")])
        .await
        .assert_status(403);
    assert_eq!(ticket_status(&app).await, "open");

    app.post("/tickets/1/transitions/staff_only", &[("reason", " done ")])
        .await
        .assert_redirect("/");
    assert_eq!(ticket_status(&app).await, "closed");
    let log = history(db, "tickets", 1).await.unwrap();
    assert_eq!(log[0].reason.as_deref(), Some("done"));
    assert_eq!(log[0].actor_id, Some(ana.id));

    let info = renox::state_machine::info_of::<Ticket>();
    let items = renox::state_machine::timeline(&log, Some(&info));
    assert_eq!(items[0]["title"], "Staff only: open \u{2192} closed");
    assert_eq!(items[0]["body"], "done");
    assert_eq!(items[0]["kind"], "info");
}

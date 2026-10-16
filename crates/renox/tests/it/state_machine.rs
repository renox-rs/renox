//! #39: the machine definition API: states, transitions, access, guards,
//! effects, errors and the Mermaid diagram.

use renox::auth::permissions::Scope;
use renox::db::Col;
use renox::prelude::*;
use renox::state_machine::{
    Machine, StateMachine, Transition, TransitionCtx, TransitionError, TransitionRecord, history,
};

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

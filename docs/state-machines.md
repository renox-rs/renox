# State machines

Many models have a **status** that moves along a fixed path: an order is drafted, placed, paid or
cancelled; a rental is reserved, active, overdue or returned. A **state machine** declares that
path once (the states, the transitions between them, who may run each, what must hold) and then
every change of the status goes through it. You get one place that holds the rules, a history of
who moved what and why, safe concurrent changes, buttons that appear only when they apply, and a
diagram for free.

The bike shop's rentals use one: [`machine.rs`](../examples/bikeshop/src/app/rentals/machine.rs)
declares the machine, and the counter, the kiosk API, the customer's cancel button and the
scheduled tasks all call `rental.transition(..)`.

- [Declaring a machine](#declaring-a-machine)
- [Running a transition](#running-a-transition)
- [What is checked, in order](#what-is-checked-in-order)
- [Errors and HTTP responses](#errors-and-http-responses)
- [Effects and events](#effects-and-events)
- [Available actions and buttons](#available-actions-and-buttons)
- [The guarded status column](#the-guarded-status-column)
- [History](#history)
- [Approvals: the interceptor](#approvals-the-interceptor)
- [Diagrams and the admin panel](#diagrams-and-the-admin-panel)
- [Moving an existing model onto a machine](#moving-an-existing-model-onto-a-machine)
- [Testing](#testing)

## Declaring a machine

A model implements `StateMachine`: the status type (usually a `#[derive(DbEnum)]` enum, whose
text is the state's name in the machine, the history and the diagram), the status column, and
`machine()`, which returns the `Machine`.

```rust
use renox::prelude::*;
use renox::state_machine::{Machine, StateMachine, Transition};

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum OrderStatus { Draft, Placed, Cancelled }

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "orders", state_machine)]
struct Order { id: i64, status: String, lines: i64 }

impl StateMachine for Order {
    type State = OrderStatus;
    const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new()
            .initial([OrderStatus::Draft])
            .state(OrderStatus::Draft).tone("neutral")
            .state(OrderStatus::Cancelled).tone("danger").terminal()
            .transition(
                Transition::new("place")
                    .label("Place order")
                    .from([OrderStatus::Draft])
                    .to(OrderStatus::Placed)
                    .permission("orders.place")
                    .guard("has_lines", |order: &Order, _ctx| {
                        Box::pin(async move {
                            if order.lines > 0 { Ok(()) } else { Err("The order has no lines.".into()) }
                        })
                    }),
            )
            .transition(
                Transition::new("cancel")
                    .from([OrderStatus::Draft, OrderStatus::Placed])
                    .to(OrderStatus::Cancelled)
                    .reason_required(),
            )
    }
}
```

A transition has a name (what you call it by), a label and tone (for buttons), the states it
leaves (`from`) and the state it enters (`to`), and who may run it:

| Call | Who runs it |
|---|---|
| (nothing) | Any logged-in user |
| `.permission("orders.place")` | A user holding the permission (`has_permission`) |
| `.permission_in("orders.place", \|order\| Scope::of_id::<Store>(order.store_id))` | A user holding it in the record's scope, or globally (`has_permission_in`) |
| `.system()` | Only the system (`by_system()`): a scheduled task, a listener. No user may run it |

`guard(name, closure)` adds a condition: the closure returns `Ok(())` or `Err(message)`, and
the message is shown to the user. `reason_required()` makes a non-blank reason part of the run.
`initial([..])` lists the states a new record may be inserted in, `terminal()` marks a state
nothing leaves (declaring a transition out of one panics when the machine is built, so a typo
shows at once), and `tone` names a state's colour for the diagram and the history. The machine is
plain data, built on each call to `machine()`.

## Running a transition

`record.transition(name)` starts a run; chain what it needs and end with `run` or `run_in`:

```rust
# use renox::prelude::*;
# use renox::state_machine::{Machine, StateMachine, Transition, TransitionError};
# #[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
# enum OrderStatus { Draft, Placed }
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "orders", state_machine)]
# struct Order { id: i64, status: String }
# impl StateMachine for Order {
#     type State = OrderStatus;
#     const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");
#     fn machine() -> Machine<Self> { Machine::new() }
# }
async fn place(state: AppState, user: AuthUser, order: Order) -> Result {
    order
        .transition("place")
        .by(&user)                                  // who (or `.by_system()`)
        .reason("customer paid by phone")           // stored in the history
        .input(&serde_json::json!({ "channel": "phone" })) // guards and effects read it
        .meta(serde_json::json!({ "ip": "10.0.0.1" }))     // stored in the history row
        .run(&state)
        .await
        .map_err(TransitionError::into_error)?;
    Ok(())
}
```

- `run(&state)` opens a transaction, runs the transition, commits, then emits `Transitioned<M>`
  to the app's listeners.
- `run_in(&mut tx)` runs inside a transaction you already have and **does not commit**. It returns
  an `Outcome` (`#[must_use]`): after your commit, call `outcome.emit(&state).await?`. Use it when
  the status change belongs with other writes (the bike shop moves a rental's status in the
  same transaction as its fees, the deposit and the bike's status).
- The result is `Outcome::Done(Transitioned)`, or `Outcome::Pending` when an interceptor deferred
  it (see [Approvals](#approvals-the-interceptor)).
- The record you hold is not changed: set its status field yourself afterwards if you keep using
  it (`rental.status = RentalStatus::Active;`), or load it again.

A run that cannot be started because neither `by` nor `by_system` was called is refused as
unauthenticated.

## What is checked, in order

1. **Authentication**: a user, or the system for a `.system()` transition. A user can never run a
   `.system()` transition.
2. **Permission**: `permission` with `has_permission`, `permission_in` with `has_permission_in`
   in the scope the closure gives for this record.
3. **From**: the record's status must be one of the transition's `from` states.
4. **Guards**, in the order added. They also run when you ask which actions are
   [available](#available-actions-and-buttons), so a guard must not write anything.
5. **Reason**, when the transition requires one.
6. **The interceptor**, if one is registered.
7. The **optimistic update**: `UPDATE .. SET status = ? WHERE id = ? AND status = <loaded>`. If no
   row changed, someone else moved the record first and the run fails with `Conflict`. Of two
   concurrent runs on one record, exactly one succeeds.
8. The **history row**, then the **effects**, in the same transaction.

## Errors and HTTP responses

`TransitionError` says why a run did not happen; `into_error()` turns it into the framework's
`Error`, so a handler can use it with `?` after a `map_err`:

| Variant | Meaning | HTTP |
|---|---|---|
| `Unauthenticated` | Nobody is logged in (or `by`/`by_system` was not called) | 401 |
| `Forbidden` | The user may not run it | 403 |
| `Unknown(name)` | The machine has no such transition | 404 |
| `NotFrom(state)` | The record is not in a state the transition leaves | 422 |
| `Guard(message)` | A guard said no; the message is safe to show | 422 |
| `ReasonRequired` | The transition needs a reason | 422 |
| `Conflict` | The record changed under the run | 422 |
| `Failed(error)` | The database or an effect failed | the error's own |

## Effects and events

An **effect** is work that must happen with the change or not at all. It runs inside the
transition's transaction after the history row, and if it fails the status change and the
history row roll back with it:

```rust
# use renox::prelude::*;
# use renox::state_machine::{Machine, StateMachine, Transition};
# #[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
# enum OrderStatus { Draft, Placed }
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "orders", state_machine)]
# struct Order { id: i64, status: String }
# impl StateMachine for Order {
#     type State = OrderStatus;
#     const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");
#     fn machine() -> Machine<Self> {
Transition::<Order>::new("place")
    .from([OrderStatus::Draft])
    .to(OrderStatus::Placed)
    .effect(|order, _ctx, tx| {
        let id = order.id;
        Box::pin(async move {
            renox::db::sql("UPDATE stock SET reserved = reserved + 1 WHERE order_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            Ok(())
        })
    })
#     ;
#     Machine::new() }
# }
```

Work that may fail without undoing the change (a mail, a notification) belongs in a listener
instead: `Transitioned<M>` is emitted **after the commit**, never on a rollback, and with
`run_in` only when you emit it.

```rust
# use renox::prelude::*;
# use renox::state_machine::{Machine, StateMachine, Transitioned};
# #[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
# enum OrderStatus { Draft }
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "orders", state_machine)]
# struct Order { id: i64, status: String }
# impl StateMachine for Order {
#     type State = OrderStatus;
#     const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");
#     fn machine() -> Machine<Self> { Machine::new() }
# }
# fn register(app: &mut Registry) {
app.listen(|event: Transitioned<Order>, state: AppState| async move {
    if event.transition == "place" {
        // event.key, event.from, event.to, event.actor_id, event.reason, event.meta
        let _ = &state;
    }
    Ok(())
});
# }
```

## Available actions and buttons

`record.available(&user, &db)` returns the transitions this user may run on this record now:
their permission (in the record's scope), the record's state and the guards all hold. System
transitions are left out. Each `Available` has `name`, `label`, `tone`, `needs_reason` and
`requires_approval`. Nothing is written.

`<rx-transition-actions>` draws a button for each, with a reason field for the ones that need
one, posting to `{action}/{name}`; `state_machine::routes::<M>()` is the route that receives
the post, runs the transition as the logged-in user and goes back to the page:

```rust
# use renox::prelude::*;
# use renox::state_machine::{self, Machine, StateMachine};
# #[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
# enum OrderStatus { Draft }
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "orders", state_machine)]
# struct Order { id: i64, status: String }
# impl StateMachine for Order {
#     type State = OrderStatus;
#     const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");
#     fn machine() -> Machine<Self> { Machine::new() }
# }
fn routes() -> Routes {
    // POST /orders/{id}/transitions/{name}
    Routes::new().group("/orders", "orders.", state_machine::routes::<Order>())
}

async fn show(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> Result<View> {
    let order = Order::find_or_404(&state.db, id).await?;
    let actions = order.available(&user, &state.db).await?;
    Ok(view("orders/show.html", context! { order, actions }))
}
```

```html
<rx-transition-actions :actions="actions" action="/orders/{{ order.id }}/transitions"/>
```

A refusal answers as in the table above. Props: `actions` and `action` are required; `label`
names the group, `reason-label` and `approval-label` set the texts, `id` the element's id.

## The guarded status column

`#[model(state_machine)]` makes the model's status column read-only for everything but a
transition:

- `save`, `save_only` and `save_changes` **refuse a changed status**. `save` matches the row on
  the status the record was loaded with and does not write it, so a stale or edited status is an
  error and the record's other columns are still saved.
- Bulk `update` queries (`Order::query().update(..)`) **bypass the guard**, like model hooks.
  Don't use them for the status of a machine model.
- Inserts honour `Machine::initial`: if the machine declares initial states, a record inserted in
  another state is refused. With no `initial` declared, inserts are unrestricted.
- `insert_in_state(&mut tx, state, reason)` is the explicit exception (an import, a fixture, a
  record born mid-way): it inserts in `state` and writes a history row with no `from` state.
  Set the record's status field first; the argument states the exception.

Code that must write the status column some other way (an undo, a data fix) uses the same
bypass and says so in a comment.

## History

Every change writes a row to the `state_transitions` table (a framework migration every app
gets): the model's table and key, the transition, `from_state`, `to_state`, the actor, the reason
and the metadata. `state_machine::history(&db, "orders", id)` reads it back, oldest first, and
`state_machine::timeline(&rows, Some(&info_of::<Order>()))` turns it into the items
`<rx-history>` shows, titled with the transition's label and coloured by the tone of the state
entered:

```rust
# use renox::prelude::*;
# use renox::state_machine::{self, Machine, StateMachine};
# #[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
# enum OrderStatus { Draft }
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "orders", state_machine)]
# struct Order { id: i64, status: String }
# impl StateMachine for Order {
#     type State = OrderStatus;
#     const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");
#     fn machine() -> Machine<Self> { Machine::new() }
# }
async fn timeline(db: &Db, id: i64) -> Result<Vec<serde_json::Value>> {
    let rows = state_machine::history(db, "orders", id).await?;
    Ok(state_machine::timeline(&rows, Some(&state_machine::info_of::<Order>())))
}
```

```html
<rx-history :items="timeline"/>
```

## Approvals: the interceptor

Some apps want certain transitions to wait for someone's approval. The engine does not build an
approval workflow, but it has the hook for one: a **transition interceptor**, registered once
for all machines with `set_interceptor`. It runs after permission, `from` and the guards and
returns `Decision::Proceed` (carry on) or `Decision::Defer(data)`. On `Defer` the status does not
move and no effect runs; the interceptor's own writes in the transaction (the approval request
it stored) are kept, and the run returns `Outcome::Pending`. Its `preview` is the side-effect
free version `available` uses to set `requires_approval` on the action.

```rust
use futures_util::future::BoxFuture;
use renox::state_machine::{Decision, Preview, TransitionInterceptor, TransitionRequest};

struct Approvals;

impl TransitionInterceptor for Approvals {
    fn intercept<'a>(
        &'a self,
        request: &'a TransitionRequest,
        tx: &'a mut renox::db::Transaction,
    ) -> BoxFuture<'a, renox::Result<Decision>> {
        Box::pin(async move {
            if request.model != "orders" || request.transition != "refund" {
                return Ok(Decision::Proceed);
            }
            renox::db::sql("INSERT INTO approval_requests (model, key) VALUES (?, ?)")
                .bind(request.model)
                .bind(request.key.clone())
                .execute(&mut *tx)
                .await?;
            Ok(Decision::Defer(serde_json::json!({ "waiting_for": "a manager" })))
        })
    }

    fn preview<'a>(
        &'a self,
        request: &'a TransitionRequest,
        _db: &'a renox::db::Db,
    ) -> BoxFuture<'a, renox::Result<Preview>> {
        Box::pin(async move {
            Ok(if request.transition == "refund" { Preview::RequiresApproval } else { Preview::Proceed })
        })
    }
}

fn boot() {
    renox::state_machine::set_interceptor(Approvals);
}
```

Replaying an approved request, run-time targets and a by-name `run(model, key, transition)` are
not part of the engine.

## Diagrams and the admin panel

`Machine::mermaid()` returns the machine as a Mermaid `stateDiagram-v2`, ready to paste into a
README or a pull request. Register the machine and the command prints it:

```rust
# use renox::prelude::*;
# use renox::state_machine::{Machine, StateMachine};
# #[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
# enum OrderStatus { Draft }
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "orders", state_machine)]
# struct Order { id: i64, status: String }
# impl StateMachine for Order {
#     type State = OrderStatus;
#     const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");
#     fn machine() -> Machine<Self> { Machine::new() }
# }
let app = App::new().state_machine::<Order>();
```

```text
rnx state:diagram Order     # the diagram (the model or its table name, any case)
rnx state:diagram           # lists the registered machines
```

`App::state_machine::<M>()` also registers the model for `rnx db:check`, and makes
`state_machine::machine_info("Order")` (name, states, transitions, guards, the diagram) work for
tools that only know the name.

The admin panel opts in per resource. Override `state_machine()` to show a read-only diagram on
the view page, `available_transitions()` for the buttons, and add `AdminAction::transitions()`
to the resource's `actions()`: one row action per non-system transition, run as the panel's user
with the engine's checks, asking for a reason in a sheet when the transition needs one. See
[admin.md](admin.md).

## Moving an existing model onto a machine

The bike shop's rentals had a `status` column changed in a dozen places. Moving them over was
mostly mechanical, and the order of things matters:

1. List the statuses and who moves between them (`machine.rs`). Transitions whose caller is
   already authorised by the area's own checks (a store-scoped counter permission, a kiosk token,
   the customer's own reservation, a scheduled task) can be `.system()` and run with
   `by_system()`; put the staff member in `.meta(..)` so the history keeps it.
2. Add `state_machine` to `#[model(..)]` and register the machine with `App::state_machine::<M>()`.
   Inserts are unrestricted until you declare `initial`, so factories and seeders keep working.
3. In each place that did `record.status = X; record.save(..)`: set the **other** columns,
   `save` (the status still matches the loaded one), then `record.transition(..).run_in(&mut tx)`
   in the same transaction, and set the field to the new status for the code that follows.
   Emit the outcome after `commit`.
4. Replace the checks that were hand-written before the change (is it still reserved? may it
   still be cancelled?) with `from` states and guards, so `available` and the run agree.
5. Tests that changed a status with `save` now call the transition (or `insert_in_state`).

## Testing

A machine is plain data, so test it as such: `Order::machine().mermaid()` against a snapshot,
`machine().transitions()` for the names and `from` states, and `record.transition("place")
.by(&user).run(app.state())` against a `TestApp` for the rules (`app.acting_as(..)` gives the
user). Assert the error variant for the refusals (`TransitionError::Forbidden`, `NotFrom`,
`Guard`), the history with `state_machine::history`, and, for two runs started together, that
exactly one succeeds and the other gets `Conflict`. `TestApp::assert_models_match_schema()`
covers the status column like any other.

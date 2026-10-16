//! State machines for models: declare a model's states and transitions
//! once, and keep the history of every status change.
//!
//! A model implements [`StateMachine`] and describes its [`Machine`]: the
//! states (with a tone and a terminal flag), and the [`Transition`]s between
//! them with who may run each (a permission, a permission in the record's
//! scope, or the system), the guards that must hold and the effects that run
//! with it. The machine is plain data: [`Machine::mermaid`] draws it, and
//! the engine and `available` read it.
//!
//! ```
//! use renox::prelude::*;
//! use renox::state_machine::{Machine, StateMachine, Transition};
//!
//! #[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
//! enum OrderStatus { Draft, Placed, Cancelled }
//!
//! #[derive(Model, serde::Serialize, Default, Clone)]
//! struct Order { id: i64, status: String, lines: i64 }
//!
//! impl StateMachine for Order {
//!     type State = OrderStatus;
//!     const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");
//!
//!     fn machine() -> Machine<Self> {
//!         Machine::new()
//!             .initial([OrderStatus::Draft])
//!             .state(OrderStatus::Draft).tone("neutral")
//!             .state(OrderStatus::Cancelled).tone("danger").terminal()
//!             .transition(
//!                 Transition::new("place")
//!                     .from([OrderStatus::Draft])
//!                     .to(OrderStatus::Placed)
//!                     .permission("orders.place")
//!                     .guard("has_lines", |order: &Order, _ctx| {
//!                         Box::pin(async move {
//!                             if order.lines > 0 { Ok(()) } else { Err("The order has no lines.".into()) }
//!                         })
//!                     }),
//!             )
//!             .transition(
//!                 Transition::new("cancel")
//!                     .from([OrderStatus::Draft, OrderStatus::Placed])
//!                     .to(OrderStatus::Cancelled)
//!                     .reason_required(),
//!             )
//!     }
//! }
//!
//! assert!(Order::machine().mermaid().starts_with("stateDiagram-v2"));
//! ```
//!
//! The storage every machine writes to is the `state_transitions` table (an
//! every-app framework migration): one [`StateTransition`] row per change,
//! written with [`TransitionRecord`] and read back with [`history`], oldest
//! first, for an `<rx-history>` block.
//!
//! ```
//! use renox::state_machine::{TransitionRecord, history};
//! # async fn demo(db: renox::db::Db) -> renox::Result {
//! TransitionRecord::new("orders", 42, "placed", "place")
//!     .from("draft")
//!     .actor(7)
//!     .reason("customer paid by phone")
//!     .record(&db)
//!     .await?;
//! let log = history(&db, "orders", 42).await?;
//! assert_eq!(log[0].to_state, "placed");
//! # Ok(()) }
//! ```

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::auth::AuthUser;
use crate::auth::permissions::Scope;
use crate::db::{
    Col, DateTime, DbError, Executor, Json, Migration, Model, Row, Transaction, now, sql,
};

pub(crate) const MIGRATION: Migration = crate::db::framework_migration!(
    "state_machine",
    "00010101000410_create_state_transitions_table"
);

/// One row of the `state_transitions` table: a status change of one record.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct StateTransition {
    /// The row id.
    pub id: i64,
    /// The model's table, e.g. `orders`.
    pub model: String,
    /// The record's key as text.
    pub record_key: String,
    /// The state it left; `None` for an insert (`insert_in_state`).
    pub from_state: Option<String>,
    /// The state it entered.
    pub to_state: String,
    /// The transition's name, e.g. `place`.
    pub transition: String,
    /// The user who ran it; `None` for a system transition.
    pub actor_id: Option<i64>,
    /// The reason given, if any.
    pub reason: Option<String>,
    /// Extra data kept with the change (`null` when none).
    pub metadata: Value,
    /// When it happened.
    pub at: DateTime,
}

impl StateTransition {
    fn from_row(row: &Row) -> std::result::Result<Self, DbError> {
        let metadata: Json<Value> = row.try_get("metadata")?;
        Ok(StateTransition {
            id: row.try_get("id")?,
            model: row.try_get("model")?,
            record_key: row.try_get("record_key")?,
            from_state: row.try_get("from_state")?,
            to_state: row.try_get("to_state")?,
            transition: row.try_get("transition")?,
            actor_id: row.try_get("actor_id")?,
            reason: row.try_get("reason")?,
            metadata: metadata.0,
            at: row.try_get("at")?,
        })
    }
}

/// A history row to write: build it, then [`record`](TransitionRecord::record)
/// it on a `&Db` or inside the transition's transaction.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "a transition record is only stored when you call `record`"]
pub struct TransitionRecord {
    model: String,
    record_key: String,
    from_state: Option<String>,
    to_state: String,
    transition: String,
    actor_id: Option<i64>,
    reason: Option<String>,
    metadata: Value,
}

impl TransitionRecord {
    /// A change of record `key` of `model` (its table) into `to_state`,
    /// by the transition named `transition`.
    pub fn new(
        model: impl Into<String>,
        key: impl ToString,
        to_state: impl Into<String>,
        transition: impl Into<String>,
    ) -> Self {
        TransitionRecord {
            model: model.into(),
            record_key: key.to_string(),
            from_state: None,
            to_state: to_state.into(),
            transition: transition.into(),
            actor_id: None,
            reason: None,
            metadata: Value::Null,
        }
    }

    /// The state the record left (leave it out for an insert).
    pub fn from(mut self, state: impl Into<String>) -> Self {
        self.from_state = Some(state.into());
        self
    }

    /// The user who ran it (leave it out for a system transition).
    pub fn actor(mut self, user_id: i64) -> Self {
        self.actor_id = Some(user_id);
        self
    }

    /// Why it was done.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Extra data to keep with the change.
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Writes the row and returns it as stored.
    pub async fn record<'c>(self, db: impl Executor<'c>) -> Result<StateTransition> {
        let at = now();
        let id: i64 = sql("INSERT INTO state_transitions \
             (model, record_key, from_state, to_state, transition, actor_id, reason, metadata, at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id")
        .bind(self.model.as_str())
        .bind(self.record_key.as_str())
        .bind(self.from_state.as_deref())
        .bind(self.to_state.as_str())
        .bind(self.transition.as_str())
        .bind(self.actor_id)
        .bind(self.reason.as_deref())
        .bind(Json(self.metadata.clone()))
        .bind(at)
        .scalar(db)
        .await?;
        Ok(StateTransition {
            id,
            model: self.model,
            record_key: self.record_key,
            from_state: self.from_state,
            to_state: self.to_state,
            transition: self.transition,
            actor_id: self.actor_id,
            reason: self.reason,
            metadata: self.metadata,
            at,
        })
    }
}

/// Every change of record `key` of `model` (its table), oldest first.
pub async fn history<'c>(
    db: impl Executor<'c>,
    model: &str,
    key: impl ToString,
) -> Result<Vec<StateTransition>> {
    let rows = sql(
        "SELECT id, model, record_key, from_state, to_state, transition, actor_id, reason, \
         metadata, at FROM state_transitions WHERE model = ? AND record_key = ? ORDER BY id",
    )
    .bind(model)
    .bind(key.to_string())
    .fetch_all(db)
    .await?;
    rows.iter()
        .map(StateTransition::from_row)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

/// A model whose status column moves through a [`Machine`].
///
/// `State` is usually a `#[derive(DbEnum)]` enum: its text (`Display`) is the
/// name of a state in the machine, in the history and in diagrams.
pub trait StateMachine: Model {
    /// The status type, e.g. an enum deriving `DbEnum`.
    type State: Copy + PartialEq + fmt::Display + Send + Sync + 'static;

    /// The status column.
    const COLUMN: Col<Self, Self::State>;

    /// The states and transitions. Built on each call; cheap, so call it
    /// where you need it or keep the result.
    fn machine() -> Machine<Self>;
}

/// Why a transition did not run.
#[derive(Debug)]
#[non_exhaustive]
pub enum TransitionError {
    /// Nobody is logged in and the transition is not a system one.
    Unauthenticated,
    /// The user may not run this transition (or it is a system transition
    /// and a user tried).
    Forbidden,
    /// The record is not in a state the transition leaves; holds the state
    /// it is in.
    NotFrom(String),
    /// A guard said no; holds its message, safe to show.
    Guard(String),
    /// The record changed under the transition (another one ran first).
    Conflict,
    /// The machine has no transition of this name.
    Unknown(String),
    /// A `reason_required` transition ran without a reason.
    ReasonRequired,
    /// Anything else (the database, an effect).
    Failed(crate::Error),
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionError::Unauthenticated => f.write_str("log in to do this"),
            TransitionError::Forbidden => f.write_str("you may not do this"),
            TransitionError::NotFrom(state) => {
                write!(f, "this can't be done from the state `{state}`")
            }
            TransitionError::Guard(message) => f.write_str(message),
            TransitionError::Conflict => {
                f.write_str("the record was changed by someone else; reload and try again")
            }
            TransitionError::Unknown(name) => write!(f, "there is no transition `{name}`"),
            TransitionError::ReasonRequired => f.write_str("a reason is required"),
            TransitionError::Failed(error) => write!(f, "the transition failed: {error:?}"),
        }
    }
}

impl std::error::Error for TransitionError {}

impl From<crate::Error> for TransitionError {
    fn from(error: crate::Error) -> Self {
        TransitionError::Failed(error)
    }
}

impl From<DbError> for TransitionError {
    fn from(error: DbError) -> Self {
        TransitionError::Failed(error.into())
    }
}

/// What a guard or an effect sees of the run: who, why, and the input and
/// metadata given with it.
#[derive(Clone, Default)]
pub struct TransitionCtx {
    actor: Option<AuthUser>,
    reason: Option<String>,
    input: Value,
    meta: Value,
}

impl TransitionCtx {
    /// A context for `actor` (`None` for a system run), with no reason, no
    /// input and no metadata.
    pub fn new(actor: Option<AuthUser>) -> Self {
        TransitionCtx {
            actor,
            ..Default::default()
        }
    }

    /// Sets the reason.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Sets the input guards and effects read with [`TransitionCtx::input`].
    pub fn with_input(mut self, input: Value) -> Self {
        self.input = input;
        self
    }

    /// Sets the metadata stored with the history row.
    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = meta;
        self
    }

    /// The user running the transition; `None` for a system run.
    pub fn actor(&self) -> Option<&AuthUser> {
        self.actor.as_ref()
    }

    /// The reason given, if any.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// The input given with the run, as JSON (`null` when none).
    pub fn input(&self) -> &Value {
        &self.input
    }

    /// The input, read as `T`.
    pub fn input_as<T: serde::de::DeserializeOwned>(&self) -> std::result::Result<T, String> {
        serde_json::from_value(self.input.clone()).map_err(|e| e.to_string())
    }

    /// The metadata stored with the history row (`null` when none).
    pub fn meta(&self) -> &Value {
        &self.meta
    }
}

type GuardFn<M> = Arc<
    dyn for<'a> Fn(&'a M, &'a TransitionCtx) -> BoxFuture<'a, std::result::Result<(), String>>
        + Send
        + Sync,
>;
type EffectFn<M> = Arc<
    dyn for<'a> Fn(&'a M, &'a TransitionCtx, &'a mut Transaction) -> BoxFuture<'a, Result>
        + Send
        + Sync,
>;
type ScopeFn<M> = Arc<dyn Fn(&M) -> Scope + Send + Sync>;

/// Who may run a transition.
enum Access<M> {
    /// Any logged-in user.
    Authenticated,
    /// A user holding the permission.
    Permission(String),
    /// A user holding the permission in the scope the function gives.
    PermissionIn(String, ScopeFn<M>),
    /// Only the system (`by_system`): scheduled tasks, listeners.
    System,
}

/// One state of a [`Machine`]: its name, tone and whether it is terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct StateDef {
    /// The state's text, as stored.
    pub name: String,
    /// A tone for badges and steppers (`neutral`, `danger`, ...), if set.
    pub tone: Option<String>,
    /// No transition leaves a terminal state.
    pub terminal: bool,
}

/// A change between states, built with [`Transition::new`].
pub struct Transition<M: StateMachine> {
    name: String,
    label: Option<String>,
    tone: Option<String>,
    from: Vec<String>,
    to: Option<String>,
    reason_required: bool,
    access: Access<M>,
    guards: Vec<(String, GuardFn<M>)>,
    effects: Vec<EffectFn<M>>,
}

impl<M: StateMachine> Transition<M> {
    /// A transition called `name` (e.g. `place`), open to any logged-in user
    /// until you restrict it with `permission`, `permission_in` or `system`.
    pub fn new(name: impl Into<String>) -> Self {
        Transition {
            name: name.into(),
            label: None,
            tone: None,
            from: Vec::new(),
            to: None,
            reason_required: false,
            access: Access::Authenticated,
            guards: Vec::new(),
            effects: Vec::new(),
        }
    }

    /// The states it may start from.
    pub fn from<S: fmt::Display>(mut self, states: impl IntoIterator<Item = S>) -> Self {
        self.from = states.into_iter().map(|s| s.to_string()).collect();
        self
    }

    /// The state it ends in.
    pub fn to(mut self, state: impl fmt::Display) -> Self {
        self.to = Some(state.to_string());
        self
    }

    /// The button text (default: the name with spaces, capitalised: `Mark paid`).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A tone for its button (`primary`, `danger`, ...).
    pub fn tone(mut self, tone: impl Into<String>) -> Self {
        self.tone = Some(tone.into());
        self
    }

    /// The run must give a reason (the button asks for one).
    pub fn reason_required(mut self) -> Self {
        self.reason_required = true;
        self
    }

    /// Only users holding `permission` (the `Permissions` module).
    pub fn permission(mut self, permission: impl Into<String>) -> Self {
        self.access = Access::Permission(permission.into());
        self
    }

    /// Only users holding `permission` in the scope `scope` gives for the
    /// record (e.g. its store), or globally.
    pub fn permission_in(
        mut self,
        permission: impl Into<String>,
        scope: impl Fn(&M) -> Scope + Send + Sync + 'static,
    ) -> Self {
        self.access = Access::PermissionIn(permission.into(), Arc::new(scope));
        self
    }

    /// Only the system may run it (`by_system`), e.g. a scheduled task.
    pub fn system(mut self) -> Self {
        self.access = Access::System;
        self
    }

    /// A condition that must hold: the closure returns `Ok(())` or
    /// `Err(message)`. Guards run in the order added, also when asking which
    /// actions are available, so they must not write anything. Name the
    /// record's type on the closure's first parameter
    /// (`|order: &Order, ctx| Box::pin(async move { .. })`), as the compiler
    /// can't tell it from the chain.
    pub fn guard<F>(mut self, name: impl Into<String>, guard: F) -> Self
    where
        F: for<'a> Fn(&'a M, &'a TransitionCtx) -> BoxFuture<'a, std::result::Result<(), String>>
            + Send
            + Sync
            + 'static,
    {
        self.guards.push((name.into(), Arc::new(guard)));
        self
    }

    /// Work that runs with the transition, in its transaction: if it fails,
    /// the status change and the history row are rolled back. Effects run in
    /// the order added.
    pub fn effect<F>(mut self, effect: F) -> Self
    where
        F: for<'a> Fn(&'a M, &'a TransitionCtx, &'a mut Transaction) -> BoxFuture<'a, Result>
            + Send
            + Sync
            + 'static,
    {
        self.effects.push(Arc::new(effect));
        self
    }

    /// The transition's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The button text.
    pub fn label_text(&self) -> String {
        self.label.clone().unwrap_or_else(|| humanize(&self.name))
    }

    /// The button's tone, if set.
    pub fn tone_name(&self) -> Option<&str> {
        self.tone.as_deref()
    }

    /// The states it may start from.
    pub fn from_states(&self) -> &[String] {
        &self.from
    }

    /// The state it ends in.
    pub fn to_state(&self) -> Option<&str> {
        self.to.as_deref()
    }

    /// Whether a reason must be given.
    pub fn needs_reason(&self) -> bool {
        self.reason_required
    }

    /// Whether only the system may run it.
    pub fn is_system(&self) -> bool {
        matches!(self.access, Access::System)
    }

    /// The names of its guards, in order.
    pub fn guard_names(&self) -> Vec<&str> {
        self.guards.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Checks who is running it: authentication, then permission (in the
    /// record's scope for `permission_in`). `actor` is `None` for a system
    /// run, which only `system` transitions accept; a user may not run one.
    pub fn check_access(
        &self,
        record: &M,
        actor: Option<&AuthUser>,
    ) -> std::result::Result<(), TransitionError> {
        let Some(user) = actor else {
            return if self.is_system() {
                Ok(())
            } else {
                Err(TransitionError::Unauthenticated)
            };
        };
        let allowed = match &self.access {
            Access::Authenticated => true,
            Access::Permission(p) => user.has_permission(p),
            Access::PermissionIn(p, scope) => user.has_permission_in(p, &scope(record)),
            Access::System => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(TransitionError::Forbidden)
        }
    }

    /// Checks the record's current state is one the transition leaves.
    pub fn check_from(
        &self,
        current: impl fmt::Display,
    ) -> std::result::Result<(), TransitionError> {
        let current = current.to_string();
        if self.from.contains(&current) {
            Ok(())
        } else {
            Err(TransitionError::NotFrom(current))
        }
    }

    /// Runs the guards in order and stops at the first that says no.
    pub async fn check_guards(
        &self,
        record: &M,
        ctx: &TransitionCtx,
    ) -> std::result::Result<(), TransitionError> {
        for (_, guard) in &self.guards {
            guard(record, ctx).await.map_err(TransitionError::Guard)?;
        }
        Ok(())
    }

    /// Runs the effects in order inside `tx`, stopping at the first failure.
    pub async fn run_effects(
        &self,
        record: &M,
        ctx: &TransitionCtx,
        tx: &mut Transaction,
    ) -> Result {
        for effect in &self.effects {
            effect(record, ctx, tx).await?;
        }
        Ok(())
    }
}

/// `mark_paid` becomes `Mark paid`.
fn humanize(name: &str) -> String {
    let text = name.replace(['_', '-'], " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The states and transitions of a [`StateMachine`] model.
pub struct Machine<M: StateMachine> {
    states: Vec<StateDef>,
    initial: Vec<String>,
    transitions: Vec<Transition<M>>,
    /// The state `tone` and `terminal` apply to.
    last: Option<usize>,
}

impl<M: StateMachine> Default for Machine<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: StateMachine> Machine<M> {
    /// An empty machine.
    pub fn new() -> Self {
        Machine {
            states: Vec::new(),
            initial: Vec::new(),
            transitions: Vec::new(),
            last: None,
        }
    }

    fn index_of(&mut self, name: &str) -> usize {
        if let Some(i) = self.states.iter().position(|s| s.name == name) {
            return i;
        }
        self.states.push(StateDef {
            name: name.to_owned(),
            tone: None,
            terminal: false,
        });
        self.states.len() - 1
    }

    /// Declares a state, to describe with `tone` and `terminal`. States that
    /// only transitions mention are declared implicitly.
    pub fn state(mut self, state: impl fmt::Display) -> Self {
        let i = self.index_of(&state.to_string());
        self.last = Some(i);
        self
    }

    /// Sets the tone (`neutral`, `danger`, ...) of the state just declared.
    ///
    /// # Panics
    ///
    /// When no `state` came before it.
    pub fn tone(mut self, tone: impl Into<String>) -> Self {
        let i = self.last.expect("`tone` follows `state`");
        self.states[i].tone = Some(tone.into());
        self
    }

    /// Marks the state just declared as terminal: no transition may leave it.
    ///
    /// # Panics
    ///
    /// When no `state` came before it.
    pub fn terminal(mut self) -> Self {
        let i = self.last.expect("`terminal` follows `state`");
        self.states[i].terminal = true;
        self
    }

    /// The states a new record may be inserted in. With none declared,
    /// inserts are unrestricted.
    pub fn initial<S: fmt::Display>(mut self, states: impl IntoIterator<Item = S>) -> Self {
        let names: Vec<String> = states.into_iter().map(|s| s.to_string()).collect();
        for name in &names {
            self.index_of(name);
        }
        self.initial = names;
        self
    }

    /// Adds a transition. Its states are declared if they were not.
    ///
    /// # Panics
    ///
    /// When it has no `to` state, no `from` state, leaves a terminal state,
    /// or its name is already taken: a machine is code, so these are
    /// mistakes found the first time it is built.
    pub fn transition(mut self, transition: Transition<M>) -> Self {
        let to = transition
            .to
            .clone()
            .unwrap_or_else(|| panic!("transition `{}` has no `to` state", transition.name));
        assert!(
            !transition.from.is_empty(),
            "transition `{}` has no `from` state",
            transition.name
        );
        assert!(
            self.transitions.iter().all(|t| t.name != transition.name),
            "transition `{}` is defined twice",
            transition.name
        );
        for name in transition.from.clone().iter().chain([&to]) {
            self.index_of(name);
        }
        for from in &transition.from {
            let terminal = self.states.iter().any(|s| &s.name == from && s.terminal);
            assert!(
                !terminal,
                "transition `{}` leaves the terminal state `{from}`",
                transition.name
            );
        }
        self.last = None;
        self.transitions.push(transition);
        self
    }

    /// Every state, in the order declared.
    pub fn states(&self) -> &[StateDef] {
        &self.states
    }

    /// The states inserts are limited to; empty when unrestricted.
    pub fn initial_states(&self) -> &[String] {
        &self.initial
    }

    /// Every transition, in the order added.
    pub fn transitions(&self) -> &[Transition<M>] {
        &self.transitions
    }

    /// The transition called `name`.
    pub fn find(&self, name: &str) -> std::result::Result<&Transition<M>, TransitionError> {
        self.transitions
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| TransitionError::Unknown(name.to_owned()))
    }

    /// The transitions that leave `state`, in the order added.
    pub fn from_state<'a>(
        &'a self,
        state: &'a str,
    ) -> impl Iterator<Item = &'a Transition<M>> + 'a {
        self.transitions
            .iter()
            .filter(move |t| t.from.iter().any(|f| f == state))
    }

    /// A Mermaid `stateDiagram-v2`: initial states are entered from `[*]`,
    /// terminal states lead to `[*]`, and each arrow carries the
    /// transition's name. Paste it in a Markdown page or the Mermaid editor.
    pub fn mermaid(&self) -> String {
        let mut out = String::from("stateDiagram-v2\n");
        for name in &self.initial {
            out.push_str(&format!("    [*] --> {}\n", mermaid_id(name)));
        }
        for t in &self.transitions {
            let to = t.to.as_deref().unwrap_or_default();
            for from in &t.from {
                out.push_str(&format!(
                    "    {} --> {}: {}\n",
                    mermaid_id(from),
                    mermaid_id(to),
                    t.name
                ));
            }
        }
        for state in self.states.iter().filter(|s| s.terminal) {
            out.push_str(&format!("    {} --> [*]\n", mermaid_id(&state.name)));
        }
        out
    }
}

/// A state name as a Mermaid id (letters, digits and `_`).
fn mermaid_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

//! State machines for models: the history of every status change.
//!
//! This part holds the storage every machine writes to: the
//! `state_transitions` table (an every-app framework migration), one
//! [`StateTransition`] row per change, written with [`TransitionRecord`] and
//! read back with [`history`], newest last, for an `<rx-history>` block.
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

use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::db::{DateTime, DbError, Executor, Json, Migration, Row, now, sql};

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

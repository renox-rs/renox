//! An audit trail: who did what, to which record, from where. Opt in with
//! `App::module(Audit)`, which also records every auth event (logins,
//! failed logins, lockouts, password and profile changes, deleted
//! accounts). Record the app's own actions with [`record`]:
//!
//! ```
//! use renox::prelude::*;
//! use renox::audit::{self, Audit, Entry};
//!
//! # fn app() -> App {
//! App::new().module(Auth::new()).module(Audit)
//! # }
//! async fn refund(State(db): State<Db>, user: AuthUser, ClientIp(ip): ClientIp) -> Result<&'static str> {
//!     // … refund order 42 …
//!     audit::record(
//!         &db,
//!         Entry::new("order.refunded")
//!             .user(user.id)
//!             .subject("orders", 42)
//!             .data(json!({ "amount": 75_000 }))
//!             .ip(ip),
//!     )
//!     .await?;
//!     let recent = audit::for_subject(&db, "orders", 42, 20).await?; // newest first
//!     # let _ = recent;
//!     Ok("refunded")
//! }
//! # let _ = (app, refund);
//! ```
//!
//! `rnx audit:prune --days 365` deletes older entries.

use std::net::IpAddr;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::auth::events::{
    AccountDeleted, EmailVerified, LockedOut, LoggedIn, LoggedOut, LoginFailed,
    OtherDevicesLoggedOut, PasswordChanged, PasswordReset, ProfileUpdated, Registered,
};
use crate::db::{DateTime, Db, DbValue, Migration, Row, now, sql};
use crate::{AppState, Module, Registry, Result, Routes};

const MIGRATIONS: &[Migration] = &[crate::db::framework_migration!(
    "audit",
    "00010101000600_create_audit_logs_table"
)];

/// Adds the `audit_logs` table, records auth events, and the
/// `audit:prune` command.
pub struct Audit;

impl Module for Audit {
    fn name(&self) -> &'static str {
        "audit"
    }

    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS
    }

    fn routes(&self) -> Routes {
        Routes::new()
    }

    fn register(&self, app: &mut Registry) {
        app.listen(|e: Registered, s| log(s, Entry::new("auth.registered").user(e.user_id)))
            .listen(|e: LoggedIn, s| log(s, Entry::new("auth.login").user(e.user_id).ip_text(e.ip)))
            .listen(|e: LoginFailed, s| {
                let data = json!({ "email": e.email });
                log(s, Entry::new("auth.login_failed").data(data).ip_text(e.ip))
            })
            .listen(|e: LockedOut, s| {
                let data = json!({ "email": e.email, "seconds": e.seconds });
                log(s, Entry::new("auth.locked_out").data(data).ip_text(e.ip))
            })
            .listen(|e: LoggedOut, s| log(s, Entry::new("auth.logout").user(e.user_id)))
            .listen(|e: PasswordReset, s| log(s, Entry::new("auth.password_reset").user(e.user_id)))
            .listen(|e: PasswordChanged, s| {
                log(s, Entry::new("auth.password_changed").user(e.user_id))
            })
            .listen(|e: EmailVerified, s| log(s, Entry::new("auth.email_verified").user(e.user_id)))
            .listen(|e: ProfileUpdated, s| {
                let data = json!({ "email_changed": e.email_changed });
                log(
                    s,
                    Entry::new("auth.profile_updated")
                        .user(e.user_id)
                        .data(data),
                )
            })
            .listen(|e: OtherDevicesLoggedOut, s| {
                log(
                    s,
                    Entry::new("auth.other_devices_logged_out").user(e.user_id),
                )
            })
            .listen(|e: AccountDeleted, s| {
                let data = json!({ "email": e.email });
                log(
                    s,
                    Entry::new("auth.account_deleted")
                        .user(e.user_id)
                        .data(data),
                )
            });
        app.command(
            "audit:prune",
            "Delete audit entries older than --days (default 365)",
            |args, state| async move {
                let days: u64 = args
                    .value("--days")
                    .unwrap_or("365")
                    .parse()
                    .map_err(|_| crate::Error::BadRequest("--days must be a number".into()))?;
                let pruned = prune(&state.db, Duration::from_secs(days * 24 * 60 * 60)).await?;
                println!("Deleted {pruned} audit entries older than {days} days.");
                Ok(())
            },
        );
    }
}

async fn log(state: AppState, entry: Entry) -> Result {
    record(&state.db, entry).await
}

/// One thing that happened, to be recorded with [`record`].
#[derive(Debug, Clone)]
pub struct Entry {
    action: String,
    user_id: Option<i64>,
    subject: Option<(String, i64)>,
    data: Value,
    ip: Option<String>,
}

impl Entry {
    /// What happened, e.g. `order.refunded`.
    pub fn new(action: &str) -> Self {
        Self {
            action: action.to_owned(),
            user_id: None,
            subject: None,
            data: json!({}),
            ip: None,
        }
    }

    /// Who did it.
    pub fn user(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// The record it happened to, e.g. `("orders", 42)`.
    pub fn subject(mut self, kind: &str, id: i64) -> Self {
        self.subject = Some((kind.to_owned(), id));
        self
    }

    /// Details, e.g. the old and new values.
    pub fn data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    /// Where the request came from (`ClientIp`).
    pub fn ip(mut self, ip: Option<IpAddr>) -> Self {
        self.ip = ip.map(|ip| ip.to_string());
        self
    }

    fn ip_text(mut self, ip: Option<String>) -> Self {
        self.ip = ip;
        self
    }
}

/// A recorded entry.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct AuditLog {
    /// The `audit_logs` row id.
    pub id: i64,
    /// Who did it; `None` when unknown (e.g. a failed login).
    pub user_id: Option<i64>,
    /// What happened, e.g. `auth.login` or `order.refunded`.
    pub action: String,
    /// The kind of record it happened to, e.g. `orders`.
    pub subject_type: Option<String>,
    /// That record's id.
    pub subject_id: Option<i64>,
    /// Details, e.g. the old and new values; `{}` when none were given.
    pub data: Value,
    /// Where the request came from, if recorded.
    pub ip: Option<String>,
    /// When it was recorded.
    pub created_at: DateTime,
}

fn from_row(row: &Row) -> std::result::Result<AuditLog, crate::db::DbError> {
    let data: String = row.try_get("data")?;
    Ok(AuditLog {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        action: row.try_get("action")?,
        subject_type: row.try_get("subject_type")?,
        subject_id: row.try_get("subject_id")?,
        data: serde_json::from_str(&data).unwrap_or(Value::Null),
        ip: row.try_get("ip")?,
        created_at: row.try_get("created_at")?,
    })
}

/// Records `entry` (needs the `Audit` module's table).
pub async fn record(db: &Db, entry: Entry) -> Result {
    let (subject_type, subject_id) = match entry.subject {
        Some((kind, id)) => (Some(kind), Some(id)),
        None => (None, None),
    };
    sql(
        "INSERT INTO audit_logs (user_id, action, subject_type, subject_id, data, ip, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(entry.user_id)
    .bind(entry.action)
    .bind(subject_type)
    .bind(subject_id)
    .bind(entry.data.to_string())
    .bind(entry.ip)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

const COLUMNS: &str = "id, user_id, action, subject_type, subject_id, data, ip, created_at";

async fn fetch(db: &Db, condition: &str, binds: Vec<DbValue>, limit: u32) -> Result<Vec<AuditLog>> {
    let rows = sql(format!(
        "SELECT {COLUMNS} FROM audit_logs {condition} ORDER BY id DESC LIMIT ?"
    ))
    .bind_all(binds)
    .bind(i64::from(limit))
    .fetch_all(db)
    .await?;
    Ok(rows
        .iter()
        .map(from_row)
        .collect::<std::result::Result<_, _>>()?)
}

/// The newest `limit` entries.
pub async fn latest(db: &Db, limit: u32) -> Result<Vec<AuditLog>> {
    fetch(db, "", Vec::new(), limit).await
}

/// What `user_id` did, newest first.
pub async fn for_user(db: &Db, user_id: i64, limit: u32) -> Result<Vec<AuditLog>> {
    fetch(
        db,
        "WHERE user_id = ?",
        vec![DbValue::Integer(user_id)],
        limit,
    )
    .await
}

/// What happened to one record, newest first.
pub async fn for_subject(db: &Db, kind: &str, id: i64, limit: u32) -> Result<Vec<AuditLog>> {
    let binds = vec![DbValue::Text(kind.to_owned()), DbValue::Integer(id)];
    fetch(
        db,
        "WHERE subject_type = ? AND subject_id = ?",
        binds,
        limit,
    )
    .await
}

/// Deletes entries older than `age`; returns how many.
pub async fn prune(db: &Db, age: Duration) -> Result<u64> {
    let before = now() - chrono::Duration::from_std(age).unwrap_or_default();
    Ok(sql("DELETE FROM audit_logs WHERE created_at < ?")
        .bind(before)
        .execute(db)
        .await?)
}

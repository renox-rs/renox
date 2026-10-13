//! What the module announces. Listen with `App::listen`; the `Audit` module,
//! when the app has it, records each one in `audit_logs`.

use renox::prelude::*;

/// A user turned two-factor authentication on (they confirmed a code).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TwoFactorEnabled {
    /// Who.
    pub user_id: i64,
}

impl Event for TwoFactorEnabled {}

/// A user turned two-factor authentication off.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TwoFactorDisabled {
    /// Who.
    pub user_id: i64,
}

impl Event for TwoFactorDisabled {}

/// A user logged in with a recovery code instead of a code from their app.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecoveryCodeUsed {
    /// Who.
    pub user_id: i64,
    /// The codes they have left.
    pub remaining: usize,
}

impl Event for RecoveryCodeUsed {}

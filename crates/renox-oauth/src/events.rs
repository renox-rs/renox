//! What the module announces. Listen with `App::listen`; when the app has
//! the `Audit` module, each one is recorded in `audit_logs`.

use renox::prelude::*;

/// A provider account was linked to a user: from the account page, at a
/// first social login with a verified address that already had an
/// account, or with a new account made by a social login.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AccountLinked {
    /// Who.
    pub user_id: i64,
    /// The provider's name (`google`).
    pub provider: String,
}

impl Event for AccountLinked {}

/// A user unlinked a provider account from the account page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AccountUnlinked {
    /// Who.
    pub user_id: i64,
    /// The provider's name.
    pub provider: String,
}

impl Event for AccountUnlinked {}

/// A user logged in with a provider. When a second login step applies
/// (two-factor authentication), the login waits for it: `second_step` is
/// true, and the `Auth` module's `LoggedIn` fires once it's passed (without
/// it, `LoggedIn` fires right away too).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoggedInWith {
    /// Who.
    pub user_id: i64,
    /// The provider's name.
    pub provider: String,
    /// Whether the login waits for the second step.
    pub second_step: bool,
}

impl Event for LoggedInWith {}

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

/// A sign-in was turned away: the provider vouched for the account, but it
/// isn't one this app accepts (another Microsoft tenant, a personal
/// account, a Google Workspace domain or an email domain outside
/// `OAuth::allow_domains`). Recorded in the activity log as `oauth.refused`
/// when the app has the `Audit` module.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OAuthRefused {
    /// The provider's name.
    pub provider: String,
    /// A short code: `domain`, `tenant`, `personal_account`, `hosted_domain`.
    pub reason: String,
    /// The account's email address, when the provider shared one.
    pub email: Option<String>,
}

impl Event for OAuthRefused {}

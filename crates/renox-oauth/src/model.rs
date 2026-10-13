use renox::prelude::*;
use serde::Serialize;

/// A provider account (a Google or GitHub account) linked to a user: they
/// log in with it. One per provider and user, and a provider account links
/// to one user only. Deleted with its user.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "oauth_accounts")]
pub struct OAuthAccount {
    /// The row's id.
    pub id: i64,
    /// The user it logs in.
    pub user_id: i64,
    /// The provider's name (`google`).
    pub provider: String,
    /// The user's id at the provider.
    pub provider_user_id: String,
    /// The email address the provider gave last time.
    pub email: Option<String>,
    /// The name the provider gave last time.
    pub name: Option<String>,
    /// The picture's URL the provider gave last time.
    pub avatar: Option<String>,
    /// When it was linked.
    pub created_at: Option<DateTime>,
    /// When it was last used to log in.
    pub updated_at: Option<DateTime>,
}

impl OAuthAccount {
    /// The account `provider_user_id` at `provider`, if a user linked it.
    pub async fn find_linked(
        db: &Db,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<Self>> {
        Self::where_eq("provider", provider)
            .where_eq("provider_user_id", provider_user_id)
            .first(db)
            .await
    }

    /// `user_id`'s account at `provider`, if they linked one.
    pub async fn of_user_at(db: &Db, user_id: i64, provider: &str) -> Result<Option<Self>> {
        Self::where_eq("user_id", user_id)
            .where_eq("provider", provider)
            .first(db)
            .await
    }

    /// Every account `user_id` linked, oldest first.
    pub async fn of_user(db: &Db, user_id: i64) -> Result<Vec<Self>> {
        Self::where_eq("user_id", user_id)
            .order_by("id")
            .get(db)
            .await
    }
}

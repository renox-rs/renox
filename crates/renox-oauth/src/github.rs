//! Sign in with GitHub: the `read:user user:email` scopes, the profile
//! from `/user` and the verified primary address from `/user/emails`.

use renox::http::Http;
use renox::prelude::*;
use serde::Deserialize;

use crate::provider::get_json;
use crate::{BoxFuture, Credentials, Profile, Provider, Token};

/// GitHub's API for the signed-in user.
pub const USER: &str = "https://api.github.com/user";
/// Their email addresses, with whether each is verified.
pub const EMAILS: &str = "https://api.github.com/user/emails";
const ACCEPT: &str = "application/vnd.github+json";

/// GitHub. Make an OAuth app in GitHub's settings (Developer settings →
/// OAuth Apps) with the callback URL `APP_URL/auth/github/callback`.
#[derive(Debug, Clone)]
pub struct GitHub {
    credentials: Credentials,
}

impl GitHub {
    /// With `GITHUB_CLIENT_ID` and `GITHUB_CLIENT_SECRET` from the configuration.
    pub fn from_config() -> Self {
        Self {
            credentials: Credentials::from_config("GITHUB"),
        }
    }

    /// With these credentials.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            credentials: Credentials::new(client_id, client_secret),
        }
    }
}

#[derive(Deserialize)]
struct User {
    id: i64,
    login: String,
    name: Option<String>,
    email: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct Email {
    email: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    verified: bool,
}

impl Provider for GitHub {
    fn name(&self) -> &str {
        "github"
    }

    fn label(&self) -> &str {
        "GitHub"
    }

    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    fn authorize_endpoint(&self) -> &str {
        "https://github.com/login/oauth/authorize"
    }

    fn token_endpoint(&self) -> &str {
        "https://github.com/login/oauth/access_token"
    }

    fn scopes(&self) -> &[&str] {
        &["read:user", "user:email"]
    }

    fn profile<'a>(&'a self, http: &'a Http, token: &'a Token) -> BoxFuture<'a, Result<Profile>> {
        Box::pin(async move {
            let user: User = get_json(http, USER, token, ACCEPT).await?;
            // The public email on `/user` isn't said to be verified: ask for
            // the list, and take the primary address if it's verified, else
            // any verified one.
            let emails: Vec<Email> = get_json(http, EMAILS, token, ACCEPT)
                .await
                .unwrap_or_default();
            let verified = emails
                .iter()
                .find(|e| e.primary && e.verified)
                .or_else(|| emails.iter().find(|e| e.verified));
            let mut profile = Profile::new(user.id.to_string());
            if let Some(email) = verified {
                profile = profile.email(email.email.clone(), true);
            } else if let Some(email) = user.email {
                profile = profile.email(email, false);
            }
            let name = user.name.or(Some(user.login));
            Ok(profile.name(name).avatar(user.avatar_url))
        })
    }
}

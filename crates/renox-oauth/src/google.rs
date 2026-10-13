//! Sign in with Google (OpenID Connect): the `openid email profile` scopes,
//! the profile from Google's userinfo endpoint.

use renox::http::Http;
use renox::prelude::*;
use serde::Deserialize;

use crate::provider::get_json;
use crate::{BoxFuture, Credentials, Profile, Provider, Token};

/// Google's userinfo endpoint (OpenID Connect).
pub const USERINFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// Google. Make the client in Google Cloud's console (APIs & Services →
/// Credentials → OAuth client ID, a web application) with the redirect URI
/// `APP_URL/auth/google/callback`.
#[derive(Debug, Clone)]
pub struct Google {
    credentials: Credentials,
}

impl Google {
    /// With `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` from the configuration.
    pub fn from_config() -> Self {
        Self {
            credentials: Credentials::from_config("GOOGLE"),
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
struct UserInfo {
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
    name: Option<String>,
    picture: Option<String>,
}

impl Provider for Google {
    fn name(&self) -> &str {
        "google"
    }

    fn label(&self) -> &str {
        "Google"
    }

    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    fn authorize_endpoint(&self) -> &str {
        "https://accounts.google.com/o/oauth2/v2/auth"
    }

    fn token_endpoint(&self) -> &str {
        "https://oauth2.googleapis.com/token"
    }

    fn scopes(&self) -> &[&str] {
        &["openid", "email", "profile"]
    }

    fn authorize_params(&self) -> Vec<(&str, &str)> {
        // Lets people with several Google accounts pick one.
        vec![("prompt", "select_account")]
    }

    fn profile<'a>(&'a self, http: &'a Http, token: &'a Token) -> BoxFuture<'a, Result<Profile>> {
        Box::pin(async move {
            let info: UserInfo = get_json(http, USERINFO, token, "application/json").await?;
            let mut profile = Profile::new(info.sub);
            if let Some(email) = info.email {
                profile = profile.email(email, info.email_verified);
            }
            Ok(profile.name(info.name).avatar(info.picture))
        })
    }
}

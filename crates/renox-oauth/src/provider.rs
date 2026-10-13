//! What a provider is: where users approve, how a code becomes a token, and
//! how the token becomes a [`Profile`]. Google and GitHub come with the
//! crate; another provider is one small impl of [`Provider`]:
//!
//! ```
//! use renox::http::Http;
//! use renox::prelude::*;
//! use renox_oauth::{BoxFuture, Credentials, Profile, Provider, Token};
//!
//! /// GitLab: users approve on gitlab.com, the profile comes from its API.
//! struct GitLab {
//!     credentials: Credentials,
//! }
//!
//! impl Provider for GitLab {
//!     fn name(&self) -> &str {
//!         "gitlab"
//!     }
//!     fn label(&self) -> &str {
//!         "GitLab"
//!     }
//!     fn credentials(&self) -> &Credentials {
//!         &self.credentials
//!     }
//!     fn authorize_endpoint(&self) -> &str {
//!         "https://gitlab.com/oauth/authorize"
//!     }
//!     fn token_endpoint(&self) -> &str {
//!         "https://gitlab.com/oauth/token"
//!     }
//!     fn scopes(&self) -> &[&str] {
//!         &["read_user"]
//!     }
//!     fn profile<'a>(&'a self, http: &'a Http, token: &'a Token) -> BoxFuture<'a, Result<Profile>> {
//!         Box::pin(async move {
//!             #[derive(serde::Deserialize)]
//!             struct Me {
//!                 id: i64,
//!                 name: Option<String>,
//!                 email: Option<String>,
//!                 confirmed_at: Option<String>,
//!                 avatar_url: Option<String>,
//!             }
//!             let me: Me = http
//!                 .get("https://gitlab.com/api/v4/user")
//!                 .bearer(&token.access_token)
//!                 .send()
//!                 .await?
//!                 .error_for_status()?
//!                 .json()?;
//!             let mut profile = Profile::new(me.id.to_string());
//!             if let Some(email) = me.email {
//!                 profile = profile.email(email, me.confirmed_at.is_some());
//!             }
//!             Ok(profile.name(me.name).avatar(me.avatar_url))
//!         })
//!     }
//! }
//!
//! # let _ =
//! renox_oauth::OAuth::new().provider(GitLab {
//!     credentials: Credentials::from_config("GITLAB"), // GITLAB_CLIENT_ID, GITLAB_CLIENT_SECRET
//! })
//! # ;
//! ```

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use renox::Config;
use renox::http::Http;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// A boxed future that is `Send`, what [`Provider`]'s methods return (so the
/// trait works behind `dyn`, and handlers stay `Send`).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How long a call to a provider may take.
const TIMEOUT: Duration = Duration::from_secs(10);

/// A sign-in provider (Google, GitHub, …): the OAuth 2.0 authorization code
/// flow with PKCE. The module builds the authorization URL (with `state`
/// and an S256 code challenge) from [`Provider::authorize_endpoint`],
/// [`Provider::scopes`] and [`Provider::authorize_params`]; [`Provider::exchange`]
/// turns the code into a [`Token`] (the standard request by default) and
/// [`Provider::profile`] reads who the user is.
pub trait Provider: Send + Sync + 'static {
    /// The provider's name in URLs (`/auth/google/redirect`) and in the
    /// `oauth_accounts` table: short, lower case, never changed once users
    /// linked accounts.
    fn name(&self) -> &str;

    /// Its name for people, on buttons: `Google`.
    fn label(&self) -> &str;

    /// The app's client id and secret, from the provider's console.
    fn credentials(&self) -> &Credentials;

    /// Where the browser goes to approve the sign-in.
    fn authorize_endpoint(&self) -> &str;

    /// Where the code is exchanged for a token.
    fn token_endpoint(&self) -> &str;

    /// The scopes asked for, e.g. `openid email profile`.
    fn scopes(&self) -> &[&str];

    /// Other parameters for the authorization URL, e.g. Google's `prompt`.
    fn authorize_params(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    /// Exchanges the authorization code for a token. The default sends the
    /// standard request ([`exchange_code`]); override it for a provider that
    /// does something else.
    fn exchange<'a>(
        &'a self,
        http: &'a Http,
        request: TokenRequest,
    ) -> BoxFuture<'a, Result<Token>> {
        Box::pin(exchange_code(http, self.token_endpoint(), request))
    }

    /// Who the token belongs to.
    fn profile<'a>(&'a self, http: &'a Http, token: &'a Token) -> BoxFuture<'a, Result<Profile>>;
}

/// The app's client id and secret at a provider: given, or read from the
/// configuration when a request needs them (`config.var`, i.e. `.env` or the
/// environment). A provider without them is off: no button, and its routes
/// answer 404.
#[derive(Clone)]
pub struct Credentials(Source);

#[derive(Clone)]
enum Source {
    Given { id: String, secret: String },
    Config(String),
}

impl Credentials {
    /// These credentials.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self(Source::Given {
            id: client_id.into(),
            secret: client_secret.into(),
        })
    }

    /// `<PREFIX>_CLIENT_ID` and `<PREFIX>_CLIENT_SECRET` from the
    /// configuration, e.g. `Credentials::from_config("GOOGLE")`.
    pub fn from_config(prefix: impl Into<String>) -> Self {
        Self(Source::Config(prefix.into()))
    }

    /// The client id and secret, or `None` when one is missing.
    pub fn resolve(&self, config: &Config) -> Option<(String, String)> {
        match &self.0 {
            Source::Given { id, secret } if !id.is_empty() && !secret.is_empty() => {
                Some((id.clone(), secret.clone()))
            }
            Source::Given { .. } => None,
            Source::Config(prefix) => Some((
                config.var(&format!("{prefix}_CLIENT_ID"))?,
                config.var(&format!("{prefix}_CLIENT_SECRET"))?,
            )),
        }
    }
}

impl fmt::Debug for Credentials {
    /// Never prints the secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Source::Given { id, .. } => write!(f, "Credentials({id}, secret hidden)"),
            Source::Config(prefix) => write!(f, "Credentials::from_config({prefix:?})"),
        }
    }
}

/// What [`Provider::exchange`] sends: the code from the callback, and what
/// proves this app asked for it.
#[non_exhaustive]
pub struct TokenRequest {
    /// The authorization code from the callback.
    pub code: String,
    /// The callback URL, the same as in the authorization URL.
    pub redirect_uri: String,
    /// The PKCE code verifier, whose challenge went in the authorization URL.
    pub code_verifier: String,
    /// The app's client id.
    pub client_id: String,
    /// The app's client secret.
    pub client_secret: String,
}

impl fmt::Debug for TokenRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenRequest")
            .field("redirect_uri", &self.redirect_uri)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

/// The token a provider answered with. Renox uses it once, to read the
/// profile, and doesn't store it.
#[derive(Clone, Deserialize)]
#[non_exhaustive]
pub struct Token {
    /// What API calls send as `Authorization: Bearer …`.
    pub access_token: String,
    /// Usually `Bearer`.
    #[serde(default)]
    pub token_type: Option<String>,
    /// For new access tokens later, when the provider gives one.
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Seconds the access token lasts.
    #[serde(default)]
    pub expires_in: Option<u64>,
    /// The scopes granted (some providers grant fewer than asked).
    #[serde(default)]
    pub scope: Option<String>,
    /// OpenID Connect's ID token, when the provider sends one.
    #[serde(default)]
    pub id_token: Option<String>,
}

impl Token {
    /// A token with only its access token, e.g. from an overridden
    /// [`Provider::exchange`].
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            token_type: None,
            refresh_token: None,
            expires_in: None,
            scope: None,
            id_token: None,
        }
    }
}

impl fmt::Debug for Token {
    /// Never prints the tokens themselves.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Token")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

/// Who signed in, as every provider describes them.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct Profile {
    /// Their id at the provider, which never changes (Google's `sub`).
    pub id: String,
    /// Their email address, if the provider shared one.
    pub email: Option<String>,
    /// Whether the provider checked that they own `email`. Accounts are
    /// only ever linked or made by a verified address.
    pub email_verified: bool,
    /// Their name.
    pub name: Option<String>,
    /// A URL of their picture.
    pub avatar: Option<String>,
}

impl Profile {
    /// A profile with this id and nothing else.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }

    /// With an email address, `verified` when the provider checked it.
    pub fn email(mut self, email: impl Into<String>, verified: bool) -> Self {
        self.email = Some(email.into());
        self.email_verified = verified;
        self
    }

    /// With a name (an empty one counts as none).
    pub fn name(mut self, name: Option<String>) -> Self {
        self.name = name.filter(|name| !name.trim().is_empty());
        self
    }

    /// With a picture's URL.
    pub fn avatar(mut self, avatar: Option<String>) -> Self {
        self.avatar = avatar.filter(|url| !url.is_empty());
        self
    }

    /// The email address, when the provider verified it.
    pub fn verified_email(&self) -> Option<&str> {
        self.email
            .as_deref()
            .filter(|email| self.email_verified && !email.is_empty())
    }
}

/// The standard token request (RFC 6749 §4.1.3, with RFC 7636's
/// `code_verifier`): a form POST to `endpoint` asking for JSON. A 2xx
/// answer without an `access_token` (GitHub answers errors with 200) is an
/// error too.
pub async fn exchange_code(http: &Http, endpoint: &str, request: TokenRequest) -> Result<Token> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", request.code.as_str()),
        ("redirect_uri", request.redirect_uri.as_str()),
        ("client_id", request.client_id.as_str()),
        ("client_secret", request.client_secret.as_str()),
        ("code_verifier", request.code_verifier.as_str()),
    ];
    let response = http
        .post(endpoint)
        .header("accept", "application/json")
        .form(&form)
        .timeout(TIMEOUT)
        .send()
        .await?;
    let body: renox::serde_json::Value = response.json().unwrap_or_default();
    if !response.ok() || body.get("access_token").is_none() {
        let error = body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("no access token");
        return Err(renox::anyhow::anyhow!(
            "{endpoint}: the code wasn't exchanged ({}: {error})",
            response.status()
        )
        .into());
    }
    Ok(renox::serde_json::from_value(body)?)
}

/// A GET to a provider's API with the token, read as JSON.
pub(crate) async fn get_json<T: serde::de::DeserializeOwned>(
    http: &Http,
    url: &str,
    token: &Token,
    accept: &str,
) -> Result<T> {
    http.get(url)
        .bearer(&token.access_token)
        .header("accept", accept)
        .timeout(TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .json()
}

#[cfg(test)]
mod debug_tests {
    use super::*;

    /// Secrets, codes and tokens never show in `Debug` (logs); credentials
    /// given in code need both halves.
    #[test]
    fn debug_hides_secrets_codes_and_tokens() {
        let given = Credentials::new("client-1", "very-secret");
        assert_eq!(format!("{given:?}"), "Credentials(client-1, secret hidden)");
        assert_eq!(
            given.resolve(&Config::default()),
            Some(("client-1".to_owned(), "very-secret".to_owned()))
        );
        assert_eq!(
            Credentials::new("client-1", "").resolve(&Config::default()),
            None
        );
        assert_eq!(
            format!("{:?}", Credentials::from_config("GOOGLE")),
            "Credentials::from_config(\"GOOGLE\")"
        );

        let request = TokenRequest {
            code: "the-code".into(),
            redirect_uri: "https://app.test/auth/google/callback".into(),
            code_verifier: "the-verifier".into(),
            client_id: "client-1".into(),
            client_secret: "very-secret".into(),
        };
        let shown = format!("{request:?}");
        assert!(
            shown.contains("client-1") && shown.contains("/callback"),
            "{shown}"
        );
        for hidden in ["the-code", "the-verifier", "very-secret"] {
            assert!(!shown.contains(hidden), "{shown}");
        }

        let mut token = Token::new("access-123");
        assert_eq!(token.access_token, "access-123");
        assert!(token.refresh_token.is_none() && token.id_token.is_none());
        token.refresh_token = Some("refresh-456".into());
        token.expires_in = Some(3600);
        let shown = format!("{token:?}");
        assert!(shown.contains("3600"), "{shown}");
        assert!(
            !shown.contains("access-123") && !shown.contains("refresh-456"),
            "{shown}"
        );
    }
}

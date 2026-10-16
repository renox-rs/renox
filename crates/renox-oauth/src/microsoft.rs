//! Sign in with a Microsoft work or school account (Microsoft Entra ID,
//! OpenID Connect against `login.microsoftonline.com/{tenant}/v2.0`): the
//! `openid email profile` scopes, the profile from the ID token. Personal
//! Microsoft accounts are never accepted.
//!
//! ```
//! use renox_oauth::{Microsoft, OAuth};
//!
//! // MICROSOFT_CLIENT_ID, MICROSOFT_CLIENT_SECRET; only Contoso's accounts.
//! let _ = OAuth::new().provider(Microsoft::from_config().tenant("contoso.onmicrosoft.com"));
//! ```

use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use renox::Config;
use renox::http::Http;
use renox::prelude::*;
use serde::Deserialize;

use crate::provider::{Refused, TIMEOUT};
use crate::{BoxFuture, Credentials, Profile, Provider, Token};

/// The tenant id of personal Microsoft accounts (outlook.com, live.com, …).
pub const CONSUMER_TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// What `MICROSOFT_TENANT` is when unset: any work or school account.
pub const ANY_WORK_ACCOUNT: &str = "organizations";

const LOGIN: &str = "https://login.microsoftonline.com";

/// Microsoft. Register the app in the Entra admin center (App registrations
/// → New registration) with the redirect URI
/// `APP_URL/auth/microsoft/callback` (platform "Web"), and make a client
/// secret. `MICROSOFT_TENANT` (or [`Microsoft::tenant`]) is the tenant to
/// accept: its id, its domain (`contoso.onmicrosoft.com`), or
/// `organizations` (the default) for any work account.
///
/// The ID token comes from the token endpoint, a direct TLS call, so its
/// signature isn't verified (OpenID Connect Core §3.1.3.7 allows it); `iss`,
/// `aud`, `exp` and `tid` are checked. The tenant is checked on the
/// callback, not only in the URL. The email counts as verified only when a
/// specific tenant is configured (Entra doesn't always verify `email`, and
/// any tenant can claim any address): with `organizations`, accounts can be
/// logged in once linked, but none is linked or made by an address.
#[derive(Debug, Clone)]
pub struct Microsoft {
    credentials: Credentials,
    /// Given in code; else `MICROSOFT_TENANT`, else any work account.
    tenant: Option<String>,
    /// The tenant in use (set when the configuration was read).
    active: String,
    authorize: String,
    token: String,
    /// The client id, to check the ID token's audience.
    client_id: Option<String>,
    /// A domain tenant's id, from the discovery document: (tenant, id).
    resolved: Arc<Mutex<Option<(String, String)>>>,
}

impl Microsoft {
    /// With `MICROSOFT_CLIENT_ID` and `MICROSOFT_CLIENT_SECRET` from the
    /// configuration, and the tenant from `MICROSOFT_TENANT` (any work
    /// account while unset).
    pub fn from_config() -> Self {
        Self::with(Credentials::from_config("MICROSOFT"))
    }

    /// With these credentials (and any work account, until [`tenant`](Self::tenant)).
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self::with(Credentials::new(client_id, client_secret))
    }

    fn with(credentials: Credentials) -> Self {
        let mut microsoft = Self {
            client_id: credentials.resolve(&Config::default()).map(|(id, _)| id),
            credentials,
            tenant: None,
            active: String::new(),
            authorize: String::new(),
            token: String::new(),
            resolved: Arc::default(),
        };
        microsoft.activate(ANY_WORK_ACCOUNT);
        microsoft
    }

    /// Only accounts of this tenant sign in: its id (a GUID), its domain
    /// (`contoso.onmicrosoft.com`), or `organizations` for any work account.
    /// Overrides `MICROSOFT_TENANT`.
    pub fn tenant(mut self, tenant: impl Into<String>) -> Self {
        let tenant: String = tenant.into();
        self.activate(tenant.trim());
        self.tenant = Some(tenant.trim().to_owned());
        self
    }

    fn activate(&mut self, tenant: &str) {
        self.active = tenant.to_owned();
        self.authorize = format!("{LOGIN}/{tenant}/oauth2/v2.0/authorize");
        self.token = format!("{LOGIN}/{tenant}/oauth2/v2.0/token");
    }

    /// The tenant's id when it is one (a GUID) already.
    fn tenant_id(&self) -> Option<&str> {
        is_guid(&self.active).then_some(self.active.as_str())
    }

    /// Whether any work or school tenant is accepted.
    fn any_work_account(&self) -> bool {
        ["organizations", "common"]
            .iter()
            .any(|any| self.active.eq_ignore_ascii_case(any))
    }

    /// The id of the configured domain tenant: from the discovery document
    /// (`/{tenant}/v2.0/.well-known/openid-configuration`, its `issuer`),
    /// fetched once and kept.
    async fn resolve_tenant(&self, http: &Http) -> Result<String> {
        let cached = self.resolved.lock().ok().and_then(|cached| cached.clone());
        if let Some((tenant, id)) = cached
            && tenant == self.active
        {
            return Ok(id);
        }
        #[derive(Deserialize)]
        struct Discovery {
            issuer: String,
        }
        let url = format!(
            "{LOGIN}/{}/v2.0/.well-known/openid-configuration",
            self.active
        );
        let discovery: Discovery = http
            .get(&url)
            .header("accept", "application/json")
            .timeout(TIMEOUT)
            .send()
            .await?
            .error_for_status()?
            .json()?;
        // `https://login.microsoftonline.com/{tid}/v2.0`
        let id = tid_of_issuer(&discovery.issuer)
            .filter(|id| is_guid(id))
            .ok_or_else(|| renox::anyhow::anyhow!("{url}: no tenant id in the issuer"))?
            .to_owned();
        if let Ok(mut cached) = self.resolved.lock() {
            *cached = Some((self.active.clone(), id.clone()));
        }
        Ok(id)
    }
}

/// `https://login.microsoftonline.com/{tid}/v2.0` → `tid`.
fn tid_of_issuer(issuer: &str) -> Option<&str> {
    issuer.trim_end_matches('/').rsplit('/').nth(1)
}

fn is_guid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// What the ID token says. The payload of a JWT is base64url JSON.
#[derive(Deserialize)]
struct Claims {
    iss: Option<String>,
    aud: Option<String>,
    exp: Option<i64>,
    tid: Option<String>,
    oid: Option<String>,
    sub: Option<String>,
    email: Option<String>,
    preferred_username: Option<String>,
    name: Option<String>,
}

/// The claims of an ID token, **without verifying its signature**.
///
/// OpenID Connect Core §3.1.3.7 (ID Token Validation, step 6): when the ID
/// token is received directly from the token endpoint over TLS, the TLS
/// server validation may stand in for the signature check. Renox only ever
/// gets this token from that back-channel call, never from the browser; the
/// claims that matter are checked in [`Microsoft::profile`].
fn claims(id_token: &str) -> Result<Claims> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| renox::anyhow::anyhow!("the ID token isn't a JWT"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| renox::anyhow::anyhow!("the ID token's payload isn't base64url"))?;
    Ok(renox::serde_json::from_slice(&bytes)?)
}

impl Provider for Microsoft {
    fn name(&self) -> &str {
        "microsoft"
    }

    fn label(&self) -> &str {
        "Microsoft"
    }

    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    fn authorize_endpoint(&self) -> &str {
        &self.authorize
    }

    fn token_endpoint(&self) -> &str {
        &self.token
    }

    fn scopes(&self) -> &[&str] {
        &["openid", "email", "profile"]
    }

    fn authorize_params(&self) -> Vec<(&str, &str)> {
        vec![("prompt", "select_account")]
    }

    fn with_config(&self, config: &Config) -> Option<Arc<dyn Provider>> {
        let mut resolved = self.clone();
        let tenant = self
            .tenant
            .clone()
            .or_else(|| config.var("MICROSOFT_TENANT"))
            .unwrap_or_else(|| ANY_WORK_ACCOUNT.to_owned());
        resolved.activate(tenant.trim());
        resolved.client_id = self.credentials.resolve(config).map(|(id, _)| id);
        Some(Arc::new(resolved))
    }

    fn profile<'a>(&'a self, http: &'a Http, token: &'a Token) -> BoxFuture<'a, Result<Profile>> {
        Box::pin(async move {
            let id_token = token
                .id_token
                .as_deref()
                .ok_or_else(|| renox::anyhow::anyhow!("Microsoft sent no ID token"))?;
            let claims = claims(id_token)?;
            let tid = claims
                .tid
                .as_deref()
                .ok_or_else(|| renox::anyhow::anyhow!("the ID token has no tid"))?;
            let ours = self
                .client_id
                .as_deref()
                .ok_or_else(|| renox::anyhow::anyhow!("no client id to check the audience"))?;
            if claims.aud.as_deref() != Some(ours) {
                return Err(renox::anyhow::anyhow!("the ID token isn't for this app").into());
            }
            if claims
                .exp
                .is_none_or(|exp| exp <= renox::db::now().timestamp())
            {
                return Err(renox::anyhow::anyhow!("the ID token expired").into());
            }
            if claims.iss.as_deref() != Some(&format!("{LOGIN}/{tid}/v2.0")) {
                return Err(renox::anyhow::anyhow!("the ID token's issuer doesn't match").into());
            }
            if tid.eq_ignore_ascii_case(CONSUMER_TENANT) {
                return Err(Refused::new(
                    "personal_account",
                    "Personal Microsoft accounts can't sign in here. Use your work or school account.",
                )
                .into());
            }
            // The tenant in the URL is only where the browser went: this
            // is the check. A specific tenant also makes the email verified.
            let verified = if self.any_work_account() {
                false
            } else {
                let wanted = match self.tenant_id() {
                    Some(id) => id.to_owned(),
                    None => self.resolve_tenant(http).await?,
                };
                if !tid.eq_ignore_ascii_case(&wanted) {
                    return Err(Refused::new(
                        "tenant",
                        "That Microsoft account belongs to another organisation.",
                    )
                    .into());
                }
                true
            };
            let oid = claims
                .oid
                .as_deref()
                .or(claims.sub.as_deref())
                .filter(|id| !id.is_empty())
                .ok_or_else(|| renox::anyhow::anyhow!("the ID token has no oid"))?;
            // An `oid` is unique within its tenant only.
            let id = if self.any_work_account() {
                format!("{tid}.{oid}")
            } else {
                oid.to_owned()
            };
            let mut profile = Profile::new(id);
            let email = claims
                .email
                .or_else(|| claims.preferred_username.filter(|name| name.contains('@')));
            if let Some(email) = email {
                profile = profile.email(email, verified);
            }
            Ok(profile.name(claims.name))
        })
    }
}

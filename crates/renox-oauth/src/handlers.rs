//! The routes: to the provider, back from it (log in, link, or confirm who
//! the user is), and unlinking.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use renox::auth::{confirm_identity, pending_login, register_verified, registration_open, sign_in};
use renox::axum::extract::Extension;
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::events::{AccountLinked, AccountUnlinked, LoggedInWith};
use crate::{OAuthAccount, Profile, Provider, Providers, TokenRequest};

/// Where the started sign-in waits in the session.
const STARTED: &str = "_oauth";
/// How long the provider may take to send the browser back.
const VALID_FOR: i64 = 10 * 60;

/// Every route, named `oauth.*`.
pub(crate) fn routes(providers: Providers) -> Routes {
    let flow = Routes::new()
        .get("/auth/{provider}/redirect", redirect)
        .name("oauth.redirect")
        .get("/auth/{provider}/callback", callback)
        .name("oauth.callback");
    let manage = Routes::new()
        .delete("/auth/{provider}", unlink)
        .name("oauth.unlink")
        .require_auth();
    flow.merge(manage).route_layer(Extension(providers))
}

/// What the sign-in is for.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum Intent {
    /// A guest logs in (or signs up).
    Login,
    /// A logged-in user links the provider to their account.
    Link,
    /// A logged-in user proves who they are, for `/confirm-password`.
    Confirm,
}

/// A sign-in sent to the provider, kept in the session until it comes back.
#[derive(Serialize, Deserialize)]
struct Started {
    provider: String,
    /// The `state` sent along: the callback must bring it back.
    state: String,
    /// The PKCE code verifier; only its SHA-256 left the server.
    verifier: String,
    /// When it started (unix seconds).
    at: i64,
    /// Who was logged in then, if anyone.
    user_id: Option<i64>,
    intent: Intent,
    remember: bool,
}

/// `a == b`, taking as long whatever the bytes are.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

/// The PKCE S256 code challenge of `verifier` (RFC 7636 §4.2).
pub(crate) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Whether `user` may unlink one of the `linked` provider accounts they
/// have: not their only way to log in.
pub(crate) fn can_unlink(user: &User, linked: usize) -> bool {
    user.has_password() || linked > 1
}

/// The account page when the app has it, else `/`.
fn account(state: &AppState) -> String {
    state
        .url("account.show", &[])
        .unwrap_or_else(|_| "/".into())
}

/// The login page, else `/`.
fn login_page(state: &AppState) -> String {
    state.url("login", &[]).unwrap_or_else(|_| "/".into())
}

/// Seconds since 1970 on Renox's clock (which tests can move).
fn now() -> i64 {
    renox::db::now().timestamp()
}

/// Back to `to`, with `message` as an error toast.
fn fail(to: String, message: impl Into<String>) -> Response {
    (Toast::error(message), Redirect::to(&to)).into_response()
}

#[derive(Deserialize)]
struct RedirectQuery {
    intent: Option<String>,
    remember: Option<String>,
}

/// Sends the browser to the provider, with a new `state` and PKCE challenge.
async fn redirect(
    State(state): State<AppState>,
    Extension(providers): Extension<Providers>,
    Path(name): Path<String>,
    session: Session,
    user: Option<AuthUser>,
    Query(query): Query<RedirectQuery>,
) -> Result<Response> {
    let provider = providers.enabled(&state, &name).ok_or(Error::NotFound)?;
    let (client_id, _) = provider
        .credentials()
        .resolve(&state.config)
        .ok_or(Error::NotFound)?;
    let intent = match (&user, query.intent.as_deref()) {
        (None, _) => Intent::Login,
        (Some(_), Some("confirm")) => Intent::Confirm,
        (Some(_), _) => Intent::Link,
    };
    let started = Started {
        provider: name.clone(),
        state: renox::random_token(),
        verifier: renox::random_token(),
        at: now(),
        user_id: user.as_ref().map(|u| u.id),
        intent,
        remember: query.remember.is_some_and(|r| !r.is_empty() && r != "0"),
    };
    let redirect_uri = state.absolute_url("oauth.callback", &[&name])?;
    let url = authorize_url(
        provider.as_ref(),
        &client_id,
        &redirect_uri,
        &started.state,
        &challenge(&started.verifier),
    );
    // A new sign-in replaces one that didn't come back.
    session.put(STARTED, &started)?;
    Ok(Redirect::to(&url).into_response())
}

/// The provider's authorization URL for this sign-in.
fn authorize_url(
    provider: &dyn Provider,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    let scope = provider.scopes().join(" ");
    let mut query = form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", &scope)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    for (key, value) in provider.authorize_params() {
        query.append_pair(key, value);
    }
    let endpoint = provider.authorize_endpoint();
    let join = if endpoint.contains('?') { '&' } else { '?' };
    format!("{endpoint}{join}{}", query.finish())
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Back from the provider: checks `state`, exchanges the code (with the
/// PKCE verifier), reads the profile, then logs in, links or confirms.
async fn callback(
    State(state): State<AppState>,
    Extension(providers): Extension<Providers>,
    Path(name): Path<String>,
    session: Session,
    user: Option<AuthUser>,
    ClientIp(ip): ClientIp,
    Query(query): Query<CallbackQuery>,
) -> Result<Response> {
    // Single use: whatever happens next, this sign-in is over.
    let started: Option<Started> = session.pull(STARTED);
    let back = match &user {
        Some(_) => account(&state),
        None => login_page(&state),
    };
    let provider = providers.enabled(&state, &name).ok_or(Error::NotFound)?;
    let label = provider.label().to_owned();
    let Some(started) = started.filter(|started| {
        started.provider == name
            && query
                .state
                .as_deref()
                .is_some_and(|sent| same(sent, &started.state))
            && now() - started.at <= VALID_FOR
            && started.user_id == user.as_ref().map(|u| u.id)
    }) else {
        return Ok(fail(
            back,
            format!("That sign-in with {label} expired or wasn't started here. Try again."),
        ));
    };
    if query.error.is_some() {
        return Ok(fail(
            back,
            format!("Signing in with {label} was cancelled."),
        ));
    }
    let Some(code) = query.code.filter(|code| !code.is_empty()) else {
        return Ok(fail(
            back,
            format!("{label} didn't send a sign-in code. Try again."),
        ));
    };
    let Some((client_id, client_secret)) = provider.credentials().resolve(&state.config) else {
        return Err(Error::NotFound);
    };
    let request = TokenRequest {
        code,
        redirect_uri: state.absolute_url("oauth.callback", &[&name])?,
        code_verifier: started.verifier.clone(),
        client_id,
        client_secret,
    };
    let profile = match fetch_profile(&state, provider.clone(), request).await {
        Ok(profile) => profile,
        Err(err) => {
            // For the app's operators; the visitor only sees "try again".
            tracing::warn!(provider = %name, error = ?err, "a social login failed");
            return Ok(fail(
                back,
                format!("{label} didn't confirm the sign-in. Try again."),
            ));
        }
    };
    match (started.intent, user) {
        (Intent::Login, None) => {
            log_in(
                &state,
                &session,
                ip,
                &name,
                &label,
                profile,
                started.remember,
            )
            .await
        }
        (Intent::Link, Some(user)) => link(&state, &user, &name, &label, profile).await,
        (Intent::Confirm, Some(user)) => {
            confirm(&state, &session, &user, &name, &label, profile).await
        }
        // `user_id` matched above, so the intent fits who is logged in.
        _ => Ok(fail(back, "Try again.")),
    }
}

/// Exchanges the code and reads the profile.
async fn fetch_profile(
    state: &AppState,
    provider: Arc<dyn Provider>,
    request: TokenRequest,
) -> Result<Profile> {
    let token = provider.exchange(&state.http, request).await?;
    let profile = provider.profile(&state.http, &token).await?;
    if profile.id.is_empty() {
        return Err(renox::anyhow::anyhow!("the profile has no id").into());
    }
    Ok(profile)
}

/// The provider account's row, with what the provider said this time.
async fn refresh(state: &AppState, mut account: OAuthAccount, profile: &Profile) -> Result {
    account.email = profile.email.clone();
    account.name = profile.name.clone();
    account.avatar = profile.avatar.clone();
    account.save(&state.db).await
}

/// A guest came back: the linked user, else the user with that verified
/// address, else a new user.
async fn log_in(
    state: &AppState,
    session: &Session,
    ip: Option<std::net::IpAddr>,
    name: &str,
    label: &str,
    profile: Profile,
    remember: bool,
) -> Result<Response> {
    let back = login_page(state);
    let user = match OAuthAccount::find_linked(&state.db, name, &profile.id).await? {
        Some(account) => {
            let user_id = account.user_id;
            refresh(state, account, &profile).await?;
            match User::find(&state.db, user_id).await? {
                Some(user) => user,
                None => return Ok(fail(back, "Try again.")),
            }
        }
        None => {
            // Never by an address the provider didn't verify: whoever
            // typed it there could take over the account here.
            let Some(email) = profile.verified_email().map(str::to_owned) else {
                return Ok(fail(
                    back,
                    format!(
                        "{label} didn't share a verified email address, so you can't sign in with it here."
                    ),
                ));
            };
            match User::find_by_email(&state.db, &email).await? {
                Some(user) => {
                    // An account whose own address nobody verified may have
                    // been made by someone else in this person's name, who
                    // knows its password: linking would let them in too.
                    if user.email_verified_at.is_none() {
                        return Ok(fail(
                            back,
                            format!(
                                "An account with this email already exists. Log in with your password, then link {label} from your account page."
                            ),
                        ));
                    }
                    if OAuthAccount::of_user_at(&state.db, user.id, name)
                        .await?
                        .is_some()
                    {
                        return Ok(fail(
                            back,
                            format!("This account is linked to another {label} account."),
                        ));
                    }
                    create_link(state, &user, name, &profile).await?;
                    user
                }
                None if registration_open(state) => {
                    let display = profile.name.clone().unwrap_or_default();
                    let user =
                        register_verified(state, &display, &email, &[("provider", name)]).await?;
                    create_link(state, &user, name, &profile).await?;
                    user
                }
                None => {
                    return Ok(fail(
                        back,
                        format!("No account uses the email address of this {label} account."),
                    ));
                }
            }
        }
    };
    let to = sign_in(state, session, &user, remember, ip).await?;
    let event = LoggedInWith {
        user_id: user.id,
        provider: name.to_owned(),
        second_step: pending_login(session).is_some(),
    };
    state.emit(event).await?;
    Ok(Redirect::to(&to).into_response())
}

/// Saves the link and announces it.
async fn create_link(state: &AppState, user: &User, name: &str, profile: &Profile) -> Result {
    OAuthAccount::create(
        &state.db,
        OAuthAccount {
            user_id: user.id,
            provider: name.to_owned(),
            provider_user_id: profile.id.clone(),
            email: profile.email.clone(),
            name: profile.name.clone(),
            avatar: profile.avatar.clone(),
            ..Default::default()
        },
    )
    .await?;
    let event = AccountLinked {
        user_id: user.id,
        provider: name.to_owned(),
    };
    state.emit(event).await
}

/// A logged-in user came back: link the provider account to them.
async fn link(
    state: &AppState,
    user: &User,
    name: &str,
    label: &str,
    profile: Profile,
) -> Result<Response> {
    let back = account(state);
    if let Some(account) = OAuthAccount::find_linked(&state.db, name, &profile.id).await? {
        if account.user_id != user.id {
            return Ok(fail(
                back,
                format!("This {label} account is linked to another user."),
            ));
        }
        refresh(state, account, &profile).await?;
        let toast = Toast::info(format!("This {label} account is already linked."));
        return Ok((toast, Redirect::to(&back)).into_response());
    }
    if OAuthAccount::of_user_at(&state.db, user.id, name)
        .await?
        .is_some()
    {
        return Ok(fail(
            back,
            format!("Another {label} account is linked to yours. Unlink it first."),
        ));
    }
    create_link(state, user, name, &profile).await?;
    let toast = Toast::success(format!("{label} is linked: you can log in with it."));
    Ok((toast, Redirect::to(&back)).into_response())
}

/// A logged-in user proved who they are with a provider linked to them.
async fn confirm(
    state: &AppState,
    session: &Session,
    user: &User,
    name: &str,
    label: &str,
    profile: Profile,
) -> Result<Response> {
    match OAuthAccount::find_linked(&state.db, name, &profile.id).await? {
        Some(account) if account.user_id == user.id => {
            refresh(state, account, &profile).await?;
            let to = confirm_identity(session)?;
            Ok(Redirect::to(&to).into_response())
        }
        _ => {
            let back = state
                .url("password.confirm", &[])
                .unwrap_or_else(|_| account(state));
            Ok(fail(
                back,
                format!("This {label} account isn't linked to yours."),
            ))
        }
    }
}

/// Unlinks a provider, unless it's the user's only way to log in.
async fn unlink(
    State(state): State<AppState>,
    Path(name): Path<String>,
    user: AuthUser,
    htmx: Htmx,
) -> Result<Response> {
    let back = account(&state);
    let go = |to: String| {
        if htmx.request {
            HxRedirect(to).into_response()
        } else {
            Redirect::to(&to).into_response()
        }
    };
    let linked = OAuthAccount::of_user(&state.db, user.id).await?;
    let Some(mut account) = linked.iter().find(|a| a.provider == name).cloned() else {
        return Ok(go(back));
    };
    if !can_unlink(user.user(), linked.len()) {
        let toast = Toast::error(
            "It's your only way to log in: set a password first (\"Forgot your password?\" on the login page).",
        );
        return Ok((toast, go(back)).into_response());
    }
    account.delete(&state.db).await?;
    let event = AccountUnlinked {
        user_id: user.id,
        provider: name,
    };
    state.emit(event).await?;
    Ok((Toast::success("Unlinked."), go(back)).into_response())
}

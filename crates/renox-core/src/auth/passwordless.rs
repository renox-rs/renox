//! Logging in without a password: a link sent by email (magic link) or a
//! short code sent through any notification channel (mail, SMS, WhatsApp…).
//! Both are single-use, expire, are throttled, and log in exactly as the
//! password page does: the second login step (`renox-2fa`) still applies,
//! `LoggedIn` fires (with `via` set to `"magic_link"` or `"code"`) and the
//! `Audit` module records it.
//!
//! ```
//! # use renox::prelude::*;
//! use std::time::Duration;
//! use renox::auth::LoginCodes;
//!
//! # let _ =
//! App::new()
//!     // An SMS channel; see `App::channel`.
//!     .channel("sms", |to, message, _state| async move {
//!         let _ = (to.address("sms"), message["text"].as_str()); // call the provider here
//!         Ok(())
//!     })
//!     .module(
//!         Auth::new()
//!             .magic_link() // /login/link: email in, link out (15 minutes, once)
//!             .login_codes(
//!                 LoginCodes::new()
//!                     .channel("sms") // any `App::channel`; "mail" works too
//!                     .digits(6)
//!                     .ttl(Duration::from_secs(300))
//!                     .max_attempts(5)
//!                     // Who a phone number (or any id) belongs to; `phone` is a
//!                     // column the app added to `users` with its own migration.
//!                     .identify(|state, input: String| async move {
//!                         let row = renox::db::sql("SELECT * FROM users WHERE phone = ?")
//!                             .bind(input.trim())
//!                             .fetch_optional_as::<User>(&state.db)
//!                             .await?;
//!                         Ok(row)
//!                     }),
//!             ),
//!     )
//! # ;
//! ```
//!
//! # Magic links
//!
//! `/login/link` (route `login.link.request`) asks for an email address and
//! sends a link to `/login/link/{token}` (`login.link.show`). The link only
//! shows a "Log in as …?" page: mail scanners open links, so *opening* it
//! must not use it up. Its button posts to the same address
//! (`login.link.consume`, CSRF-protected), and only that logs in and burns
//! the token. A token works once, for 15 minutes ([`Auth::magic_link_ttl`](super::Auth::magic_link_ttl)).
//! By default it works in any browser (people read mail on another device);
//! [`Auth::same_browser`](super::Auth::same_browser) makes it work only in the browser that asked for
//! it.
//!
//! # Login codes
//!
//! `/login/code` (`login.code.request`) asks for an identifier (an email
//! address by default; a phone number once [`LoginCodes::identify`] says how
//! to find the account) and sends a code; `/login/code/verify`
//! (`login.code.verify`) takes it. The code is valid only in the browser
//! session that asked for it, so a forwarded SMS alone is not enough. After
//! [`LoginCodes::max_attempts`] wrong codes the code is burned: a new one
//! must be requested (a correct code after that is refused). Account-level
//! lockout is the login throttle's, as for passwords.
//! With several [`LoginCodes::channels`] the form lets the user choose and
//! the verify page offers "send it another way".
//!
//! # What an attacker sees
//!
//! Requests answer the same whether the account exists or not, and do the
//! same work (a code is stored for unknown identifiers too; only the
//! delivery, queued as a job, is skipped). Sending is limited per
//! identifier and per IP address, wrong codes count in the login throttle
//! (`LoginFailed`, `LockedOut` events), codes are stored as keyed hashes and
//! the queued delivery job is encrypted. Delivery runs on the queue
//! (`queue:work`, which `serve` runs by default).
//!
//! [`Auth::register_on_first_use`](super::Auth::register_on_first_use) makes an account (without a password,
//! with a verified address) for an unknown email address that proves it owns
//! it, through [`register_verified`](super::register_verified).

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Extension, Path, State};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::events::{LockedOut, LoginFailed, announce};
use super::module::{Settings, go, message, texts};
use super::one_time::{self, CodeCheck};
use super::tokens::sha256_hex;
use super::{Recipient, User};
use crate::db::Model;
use crate::i18n::Lang;
use crate::queue::{Job, JobContext};
use crate::validation::{Errors, Valid, Validate, ValidationError, Validator};
use crate::{AppState, ClientIp, Error, Htmx, Result, Session, View, context, view};

/// The `purpose` of magic link tokens in `one_time_codes`.
const LINK: &str = "login-link";
/// The `purpose` of login codes.
const CODE: &str = "login-code";
/// The session key of the nonce codes (and, optionally, links) are bound to.
const NONCE: &str = "_login_nonce";
/// The session key of a code request waiting for its code.
const PENDING: &str = "_login_code";

/// Sends allowed per identifier and per IP address in a window.
const PER_IDENTIFIER: u32 = 5;
const PER_IP: u32 = 20;
const SEND_WINDOW: Duration = Duration::from_secs(15 * 60);

type IdentifyFn = Arc<
    dyn Fn(AppState, String) -> Pin<Box<dyn Future<Output = Result<Option<User>>> + Send>>
        + Send
        + Sync,
>;
type DeliverFn = Arc<
    dyn Fn(AppState, LoginCodeDelivery) -> Pin<Box<dyn Future<Output = Result> + Send>>
        + Send
        + Sync,
>;

/// Options for logging in with a one-time code; see
/// [`Auth::login_codes`](super::Auth::login_codes) and the
/// [module](self).
#[derive(Clone)]
pub struct LoginCodes {
    pub(super) channels: Vec<String>,
    pub(super) digits: u32,
    pub(super) ttl: Duration,
    pub(super) max_attempts: u32,
    identify: Option<IdentifyFn>,
    deliver: Option<DeliverFn>,
}

impl LoginCodes {
    /// Six digits, valid for five minutes, five tries, sent by email to the
    /// account with that email address.
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            digits: 6,
            ttl: Duration::from_secs(5 * 60),
            max_attempts: 5,
            identify: None,
            deliver: None,
        }
    }

    /// Adds a way to send the code: `"mail"`, or the name of a channel
    /// registered with `App::channel` (`"sms"`, `"whatsapp"`). The first
    /// one is the default.
    pub fn channel(mut self, name: &str) -> Self {
        if !self.channels.iter().any(|c| c == name) {
            self.channels.push(name.to_owned());
        }
        self
    }

    /// Sets the ways to send the code. With more than one, the request form
    /// asks which to use and the verify page offers "send it another way".
    pub fn channels<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.channels.clear();
        for name in names {
            self = self.channel(name.as_ref());
        }
        self
    }

    /// How many digits the code has (4 to 12); six by default.
    pub fn digits(mut self, digits: u32) -> Self {
        self.digits = digits.clamp(4, 12);
        self
    }

    /// How long a code works; five minutes by default.
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// How many wrong codes burn a code (a new one must be requested);
    /// five by default.
    pub fn max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts.max(1);
        self
    }

    /// Finds the account an identifier (what was typed on the request form:
    /// a phone number, a customer number…) belongs to; `None` for nobody. By
    /// default the identifier is an email address. Do the normalising here
    /// (`+62 812…` and `0812…` are one number); Renox has no opinion on it.
    /// Its answer must not depend on who is asking, and the form answers the
    /// same for `None`.
    pub fn identify<F, Fut>(mut self, find: F) -> Self
    where
        F: Fn(AppState, String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<User>>> + Send + 'static,
    {
        self.identify = Some(Arc::new(move |state, input| Box::pin(find(state, input))));
        self
    }

    /// Delivers codes yourself, for apps with a messaging layer of their own
    /// (templates, language, delivery logs): it replaces the built-in
    /// message on every channel but is called from the queue like it.
    /// `delivery.text()` is the built-in message.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::auth::LoginCodes;
    ///
    /// # let _ =
    /// LoginCodes::new().channel("whatsapp").deliver(|state, delivery| async move {
    ///     // An approved authentication template, in the user's language.
    ///     let _ = (&state, &delivery.channel, &delivery.address, &delivery.code, &delivery.locale);
    ///     Ok(())
    /// })
    /// # ;
    /// ```
    pub fn deliver<F, Fut>(mut self, deliver: F) -> Self
    where
        F: Fn(AppState, LoginCodeDelivery) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result> + Send + 'static,
    {
        self.deliver = Some(Arc::new(move |state, delivery| {
            Box::pin(deliver(state, delivery))
        }));
        self
    }
}

impl Default for LoginCodes {
    fn default() -> Self {
        Self::new()
    }
}

/// A login code to send, handed to [`LoginCodes::deliver`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoginCodeDelivery {
    /// The account the code is for; `None` for an address that gets an
    /// account on first use ([`Auth::register_on_first_use`](super::Auth::register_on_first_use)(super::Auth::register_on_first_use)).
    pub user: Option<User>,
    /// The channel to send on (`"mail"`, `"sms"`…).
    pub channel: String,
    /// What the user typed to ask for the code (a phone number); for
    /// `"mail"` the account's email address.
    pub address: String,
    /// The code.
    pub code: String,
    /// How long the code works from now.
    pub expires_in: Duration,
    /// The language of the request that asked for the code (`"en"`).
    pub locale: String,
    /// The built-in message, in that language: "Your App login code is
    /// 123456. It expires in 5 minutes."
    pub text: String,
}

/// What a magic link needs.
#[derive(Clone)]
pub(crate) struct MagicLink {
    pub(crate) ttl: Duration,
}

/// A login secret the framework was asked to deliver, kept in memory in
/// debug mode for `TestApp::last_login_link` / `last_login_code`.
#[derive(Debug, Clone)]
pub(crate) struct Sent {
    pub(crate) channel: String,
    pub(crate) code: Option<String>,
    pub(crate) link: Option<String>,
}

/// The passwordless part of the `Auth` module's settings.
#[derive(Clone)]
pub(crate) struct Passwordless {
    pub(crate) magic_link: Option<MagicLink>,
    pub(crate) same_browser: bool,
    pub(crate) codes: Option<LoginCodes>,
    pub(crate) register_on_first_use: bool,
    pub(crate) sent: Arc<Mutex<Vec<Sent>>>,
}

/// The delivery of a login link or code, queued so the response doesn't
/// wait for the provider (and doesn't differ for unknown accounts). The
/// payload holds the secret, so it is stored encrypted.
#[derive(Serialize, Deserialize)]
pub(crate) struct SendLoginSecret {
    link: Option<String>,
    code: Option<String>,
    user_id: Option<i64>,
    channel: String,
    address: String,
    ttl_secs: u64,
    locale: String,
}

impl Job for SendLoginSecret {
    const NAME: &'static str = "renox.send-login-secret";
    const MAX_ATTEMPTS: u32 = 3;
    const ENCRYPTED: bool = true;

    async fn handle(self, ctx: JobContext) -> Result {
        let state = ctx.state;
        let user = match self.user_id {
            Some(id) => User::find(&state.db, id).await?,
            None => None,
        };
        let lang = Lang::of(&state, &self.locale);
        let text = texts(&lang);
        let minutes = (self.ttl_secs / 60).max(1).to_string();
        let say = |key: &str, params: &[(&str, String)]| {
            crate::i18n::format(text[key].as_str().unwrap_or_default(), params, None)
        };
        if state.config.debug
            && let Some(settings) = &state.auth
        {
            let mut sent = settings
                .passwordless
                .sent
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            sent.push(Sent {
                channel: self.channel.clone(),
                code: self.code.clone(),
                link: self.link.clone(),
            });
            let extra = sent.len().saturating_sub(50);
            sent.drain(..extra);
        }

        if let Some(link) = &self.link {
            let to = user
                .as_ref()
                .map_or_else(|| self.address.clone(), |u| u.email.clone());
            let outro = say("mail_link_outro", &[("minutes", minutes)]);
            let mail = state.mail_view_in(
                &self.locale,
                to,
                say("mail_link_subject", &[]),
                "renox/mail/auth/login-link",
                context! { link, outro, text => text.clone() },
            )?;
            return state.mailer.send(mail).await;
        }

        let Some(code) = self.code else {
            return Ok(());
        };
        let message = say(
            "login_code_message",
            &[
                ("app", state.config.name.clone()),
                ("code", code.clone()),
                ("minutes", minutes.clone()),
            ],
        );
        let delivery = LoginCodeDelivery {
            user: user.clone(),
            channel: self.channel.clone(),
            address: self.address.clone(),
            code: code.clone(),
            expires_in: Duration::from_secs(self.ttl_secs),
            locale: self.locale.clone(),
            text: message.clone(),
        };
        let hook = state
            .auth
            .as_ref()
            .and_then(|settings| settings.passwordless.codes.as_ref())
            .and_then(|codes| codes.deliver.clone());
        if let Some(hook) = hook {
            return hook(state.clone(), delivery).await;
        }
        if self.channel == "mail" {
            let to = user
                .as_ref()
                .map_or_else(|| self.address.clone(), |u| u.email.clone());
            let outro = say("mail_code_outro", &[("minutes", minutes)]);
            let mail = state.mail_view_in(
                &self.locale,
                to,
                say("mail_code_subject", &[]),
                "renox/mail/auth/login-code",
                context! { code, outro, text => text.clone() },
            )?;
            return state.mailer.send(mail).await;
        }
        let recipient = match &user {
            Some(user) => Recipient::for_user(user).and(&self.channel, &self.address),
            None => Recipient::to(&self.channel, &self.address),
        }
        .in_locale(&self.locale);
        let send = state.channel(&self.channel)?;
        let payload = json!({
            "kind": "login_code",
            "code": code,
            "text": message,
            "expires_in": self.ttl_secs,
        });
        send(state.clone(), recipient, payload).await
    }
}

// --- Shared pieces -------------------------------------------------------

/// The nonce that ties codes to this browser session, made on first use.
fn nonce(session: &Session) -> Result<String> {
    if let Some(nonce) = session.get::<String>(NONCE) {
        return Ok(nonce);
    }
    let nonce = crate::crypto::random_token();
    session.put(NONCE, &nonce)?;
    Ok(nonce)
}

/// Counts a send and says whether it is within the limits. An address over
/// its limit is silently not sent to (the page answers as always, so the
/// limit can't be used to find accounts); an IP over its limit gets a 429.
async fn within_send_limits(
    state: &AppState,
    kind: &str,
    identifier: &str,
    ip: Option<IpAddr>,
) -> Result<bool> {
    if let Some(ip) = ip {
        let (count, _) =
            crate::counters::increment(&state.db, &format!("passwordless:ip:{ip}"), SEND_WINDOW)
                .await?;
        if count > PER_IP {
            return Err(Error::TooManyRequests);
        }
    }
    let key = format!("passwordless:{kind}:{identifier}");
    let (count, _) = crate::counters::increment(&state.db, &key, SEND_WINDOW).await?;
    Ok(count <= PER_IDENTIFIER)
}

/// Fails with the login throttle's message while `identifier` is locked out.
async fn ensure_not_locked(
    state: &AppState,
    lang: &Lang,
    identifier: &str,
    ip: Option<IpAddr>,
    field: &str,
) -> Result<()> {
    let Some(seconds) = state.throttle.blocked_for(identifier, ip).await else {
        return Ok(());
    };
    announce(
        state,
        LockedOut {
            email: identifier.to_owned(),
            ip: ip.map(|ip| ip.to_string()),
            seconds,
        },
    )
    .await;
    let mut errors = Errors::new();
    errors.add(
        field,
        message(lang, "auth.throttle", &[("seconds", seconds.to_string())]),
    );
    Err(ValidationError::new(errors).into())
}

/// A wrong code or link: counted in the login throttle and announced.
async fn failed(state: &AppState, identifier: &str, ip: Option<IpAddr>) {
    state.throttle.fail(identifier, ip).await;
    let event = LoginFailed {
        email: identifier.to_owned(),
        ip: ip.map(|ip| ip.to_string()),
    };
    announce(state, event).await;
}

fn field_error(field: &str, text: impl Into<String>) -> Error {
    let mut errors = Errors::new();
    errors.add(field, text.into());
    ValidationError::new(errors).into()
}

/// Marks the address as verified: the link or code went to it.
async fn mark_verified(state: &AppState, user: &User) -> Result {
    if user.email_verified_at.is_none() {
        crate::db::sql(
            "UPDATE users SET email_verified_at = ? WHERE id = ? AND email_verified_at IS NULL",
        )
        .bind(crate::db::now())
        .bind(user.id)
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

fn looks_like_email(input: &str) -> bool {
    input.contains('@') && !input.contains(char::is_whitespace)
}

/// An account for an unknown address, when registration on first use is on.
async fn first_use_account(
    state: &AppState,
    settings: &Settings,
    email: &str,
    via: &str,
) -> Result<Option<User>> {
    if !settings.passwordless.register_on_first_use || !settings.registration {
        return Ok(None);
    }
    match super::register_verified(state, "", email, &[("via", via)]).await {
        Ok(user) => Ok(Some(user)),
        // Someone else registered it at the same moment.
        Err(err) if err.is_unique_violation() => User::find_by_email(&state.db, email).await,
        Err(err) => Err(err),
    }
}

// --- Magic links ----------------------------------------------------------

pub(super) async fn show_link_request(lang: Lang) -> View {
    view(
        "renox/auth/login-link.html",
        context! { text => texts(&lang) },
    )
}

#[derive(Deserialize)]
pub(super) struct LinkForm {
    email: String,
}

impl Validate for LinkForm {
    fn rules(&self, v: &mut Validator) {
        v.field("email", &super::user::normalize_email(&self.email))
            .required()
            .email()
            .max(255);
    }
}

/// Mails a login link if the address has an account (or may get one). The
/// reply is the same either way.
pub(super) async fn send_link(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    lang: Lang,
    Valid(form): Valid<LinkForm>,
) -> Result<Response> {
    let Some(link) = settings.passwordless.magic_link.clone() else {
        return Err(Error::NotFound);
    };
    let email = super::user::normalize_email(&form.email);
    if within_send_limits(&state, "link", &email, ip).await? {
        let user = User::find_by_email(&state.db, &email).await?;
        let first_use =
            user.is_none() && settings.passwordless.register_on_first_use && settings.registration;
        let binding = if settings.passwordless.same_browser {
            Some(nonce(&session)?)
        } else {
            None
        };
        // Made for unknown addresses too, so both do the same work.
        let token =
            one_time::issue_token(&state, LINK, &email, binding.as_deref(), link.ttl).await?;
        if user.is_some() || first_use {
            let job = SendLoginSecret {
                link: Some(state.absolute_url("login.link.show", &[&token])?),
                code: None,
                user_id: user.as_ref().map(|u| u.id),
                channel: "mail".into(),
                address: email,
                ttl_secs: link.ttl.as_secs(),
                locale: crate::i18n::current_locale(&state),
            };
            state.dispatch(job).await?;
        }
    }
    session.flash("status", texts(&lang)["login_link_sent"].clone())?;
    Ok(go(&htmx, state.url("login.link.request", &[])?))
}

#[derive(Serialize)]
struct LinkPage {
    token: String,
    email: String,
    valid: bool,
}

/// "Log in as …?": opening the link doesn't use it up (mail scanners open
/// links); the button does.
pub(super) async fn show_link(
    State(state): State<AppState>,
    session: Session,
    lang: Lang,
    Path(token): Path<String>,
) -> Result<View> {
    let seen = one_time::peek_token(
        &state,
        LINK,
        &token,
        session.get::<String>(NONCE).as_deref(),
    )
    .await?;
    let page = LinkPage {
        valid: seen.is_some(),
        email: seen.map(|(_, subject, _)| subject).unwrap_or_default(),
        token,
    };
    Ok(view(
        "renox/auth/login-link-confirm.html",
        context! { text => texts(&lang), page },
    ))
}

pub(super) async fn consume_link(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    lang: Lang,
    Path(token): Path<String>,
) -> Result<Response> {
    let text = texts(&lang);
    let back = |session: &Session| -> Result<Response> {
        session.flash("error", text["login_link_invalid"].clone())?;
        Ok(go(&htmx, state.url("login.link.request", &[])?))
    };
    let nonce_now = session.get::<String>(NONCE);
    // Look first: a locked-out address shouldn't burn its link.
    let Some((_, subject, _)) =
        one_time::peek_token(&state, LINK, &token, nonce_now.as_deref()).await?
    else {
        return back(&session);
    };
    ensure_not_locked(&state, &lang, &subject, ip, "email").await?;
    let Some(email) = one_time::consume_token(&state, LINK, &token, nonce_now.as_deref()).await?
    else {
        return back(&session);
    };
    let user = match User::find_by_email(&state.db, &email).await? {
        Some(user) => user,
        None => match first_use_account(&state, &settings, &email, "magic_link").await? {
            Some(user) => user,
            None => return back(&session),
        },
    };
    mark_verified(&state, &user).await?;
    let to = super::external::sign_in_via(&state, &session, &user, false, ip, "magic_link").await?;
    session.remove(NONCE);
    Ok(go(&htmx, to))
}

// --- Login codes ------------------------------------------------------------

/// A code request waiting for its code, in the session.
#[derive(Serialize, Deserialize)]
struct PendingCode {
    identifier: String,
    channel: String,
}

#[derive(Serialize)]
struct ChannelChoice {
    name: String,
    label: String,
}

fn choices(codes: &LoginCodes, text: &Value) -> Vec<ChannelChoice> {
    codes
        .channels
        .iter()
        .map(|name| ChannelChoice {
            label: channel_label(name, text),
            name: name.clone(),
        })
        .collect()
}

/// `channel_sms` from the texts, else the name with a capital.
fn channel_label(name: &str, text: &Value) -> String {
    match text.get(format!("channel_{name}")).and_then(Value::as_str) {
        Some(label) => label.to_owned(),
        None => {
            let mut chars = name.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

fn codes_of(settings: &Settings) -> Result<&LoginCodes> {
    settings.passwordless.codes.as_ref().ok_or(Error::NotFound)
}

pub(super) async fn show_code_request(
    Extension(settings): Extension<Arc<Settings>>,
    lang: Lang,
) -> Result<View> {
    let codes = codes_of(&settings)?;
    let text = texts(&lang);
    let channels = choices(codes, &text);
    Ok(view(
        "renox/auth/login-code.html",
        context! { multiple => channels.len() > 1, channels, text },
    ))
}

#[derive(Deserialize)]
pub(super) struct CodeRequestForm {
    identifier: String,
    channel: Option<String>,
}

impl Validate for CodeRequestForm {
    fn rules(&self, v: &mut Validator) {
        v.field("identifier", &self.identifier).required().max(255);
    }
}

/// The account for an identifier and the key its code is stored under:
/// the user's id, or a hash of what was typed, so unknown identifiers have
/// codes too.
async fn who(
    state: &AppState,
    codes: &LoginCodes,
    identifier: &str,
) -> Result<(Option<User>, String)> {
    let user = match &codes.identify {
        Some(find) => find(state.clone(), identifier.to_owned()).await?,
        None => User::find_by_email(&state.db, identifier).await?,
    };
    let subject = match &user {
        Some(user) => format!("user:{}", user.id),
        None => format!("unknown:{}", sha256_hex(&identifier.to_lowercase())),
    };
    Ok((user, subject))
}

/// Stores a code for the identifier and queues its delivery (not for an
/// identifier nobody owns), then remembers in the session what to expect.
async fn issue_code(
    settings: &Settings,
    state: &AppState,
    session: &Session,
    ip: Option<IpAddr>,
    identifier: &str,
    channel: &str,
) -> Result<()> {
    let codes = codes_of(settings)?;
    let key = identifier.to_lowercase();
    let allowed = within_send_limits(state, "code", &key, ip).await?;
    let (user, subject) = who(state, codes, identifier).await?;
    let nonce = nonce(session)?;
    session.put(
        PENDING,
        PendingCode {
            identifier: identifier.to_owned(),
            channel: channel.to_owned(),
        },
    )?;
    if !allowed {
        return Ok(());
    }
    let first_use = user.is_none()
        && settings.passwordless.register_on_first_use
        && settings.registration
        && channel == "mail"
        && looks_like_email(identifier);
    let code = one_time::random_digits(codes.digits);
    one_time::store(
        state,
        CODE,
        &subject,
        &code,
        Some(&nonce),
        codes.ttl,
        codes.max_attempts,
    )
    .await?;
    if user.is_some() || first_use {
        let job = SendLoginSecret {
            link: None,
            code: Some(code),
            user_id: user.as_ref().map(|u| u.id),
            channel: channel.to_owned(),
            address: if channel == "mail" {
                super::user::normalize_email(identifier)
            } else {
                identifier.to_owned()
            },
            ttl_secs: codes.ttl.as_secs(),
            locale: crate::i18n::current_locale(state),
        };
        state.dispatch(job).await?;
    }
    Ok(())
}

/// The configured channel named in a form, else the default.
fn chosen(codes: &LoginCodes, wanted: Option<&str>) -> String {
    wanted
        .filter(|name| codes.channels.iter().any(|c| c == name))
        .or(codes.channels.first().map(String::as_str))
        .unwrap_or("mail")
        .to_owned()
}

pub(super) async fn send_code(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    lang: Lang,
    Valid(form): Valid<CodeRequestForm>,
) -> Result<Response> {
    let codes = codes_of(&settings)?;
    let identifier = form.identifier.trim();
    ensure_not_locked(&state, &lang, &identifier.to_lowercase(), ip, "identifier").await?;
    let channel = chosen(codes, form.channel.as_deref());
    issue_code(&settings, &state, &session, ip, identifier, &channel).await?;
    session.flash("status", texts(&lang)["login_code_sent"].clone())?;
    Ok(go(&htmx, state.url("login.code.verify", &[])?))
}

#[derive(Deserialize)]
pub(super) struct ResendForm {
    channel: Option<String>,
}

impl Validate for ResendForm {
    fn rules(&self, _: &mut Validator) {}
}

/// A new code for the same identifier, perhaps on another channel.
pub(super) async fn resend_code(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    lang: Lang,
    Valid(form): Valid<ResendForm>,
) -> Result<Response> {
    let codes = codes_of(&settings)?;
    let Some(pending) = session.get::<PendingCode>(PENDING) else {
        return Ok(go(&htmx, state.url("login.code.request", &[])?));
    };
    ensure_not_locked(
        &state,
        &lang,
        &pending.identifier.to_lowercase(),
        ip,
        "code",
    )
    .await?;
    let channel = chosen(codes, form.channel.as_deref().or(Some(&pending.channel)));
    issue_code(
        &settings,
        &state,
        &session,
        ip,
        &pending.identifier,
        &channel,
    )
    .await?;
    session.flash("status", texts(&lang)["login_code_sent"].clone())?;
    Ok(go(&htmx, state.url("login.code.verify", &[])?))
}

#[derive(Serialize)]
struct VerifyPage {
    channel: String,
    minutes: u64,
    others: Vec<ChannelChoice>,
}

pub(super) async fn show_code_verify(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    lang: Lang,
) -> Result<Response> {
    use axum::response::IntoResponse;
    let codes = codes_of(&settings)?;
    let Some(pending) = session.get::<PendingCode>(PENDING) else {
        return Ok(go(&htmx, state.url("login.code.request", &[])?));
    };
    let text = texts(&lang);
    let page = VerifyPage {
        channel: channel_label(&pending.channel, &text),
        minutes: (codes.ttl.as_secs() / 60).max(1),
        others: choices(codes, &text)
            .into_iter()
            .filter(|choice| choice.name != pending.channel)
            .collect(),
    };
    let intro = crate::i18n::format(
        text["login_code_verify_intro"].as_str().unwrap_or_default(),
        &[
            ("channel", page.channel.clone()),
            ("minutes", page.minutes.to_string()),
        ],
        None,
    );
    Ok(view(
        "renox/auth/login-code-verify.html",
        context! { text, page, intro },
    )
    .into_response())
}

#[derive(Deserialize)]
pub(super) struct CodeForm {
    code: String,
}

impl Validate for CodeForm {
    fn rules(&self, v: &mut Validator) {
        v.field("code", &self.code).required().max(32);
    }
}

pub(super) async fn check_code(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    lang: Lang,
    Valid(form): Valid<CodeForm>,
) -> Result<Response> {
    let codes = codes_of(&settings)?;
    let text = texts(&lang);
    let to_request = |session: &Session, key: &str| -> Result<Response> {
        session.remove(PENDING);
        session.flash("error", text[key].clone())?;
        Ok(go(&htmx, state.url("login.code.request", &[])?))
    };
    let (Some(pending), Some(nonce)) = (
        session.get::<PendingCode>(PENDING),
        session.get::<String>(NONCE),
    ) else {
        return Ok(go(&htmx, state.url("login.code.request", &[])?));
    };
    let key = pending.identifier.to_lowercase();
    ensure_not_locked(&state, &lang, &key, ip, "code").await?;
    let (user, subject) = who(&state, codes, &pending.identifier).await?;
    let result = one_time::check(&state, CODE, &subject, &form.code, Some(&nonce)).await?;
    match result {
        CodeCheck::Valid => {}
        CodeCheck::Wrong { tries_left } => {
            failed(&state, &key, ip).await;
            if tries_left == 0 {
                return to_request(&session, "login_code_burned");
            }
            return Err(field_error(
                "code",
                text["login_code_wrong"].as_str().unwrap_or_default(),
            ));
        }
        CodeCheck::Missing => return to_request(&session, "login_code_expired"),
    }
    let user = match user {
        Some(user) => user,
        None if pending.channel == "mail" && looks_like_email(&pending.identifier) => {
            let email = super::user::normalize_email(&pending.identifier);
            match first_use_account(&state, &settings, &email, "code").await? {
                Some(user) => user,
                None => return to_request(&session, "login_code_expired"),
            }
        }
        None => return to_request(&session, "login_code_expired"),
    };
    if pending.channel == "mail" {
        mark_verified(&state, &user).await?;
    }
    let to = super::external::sign_in_via(&state, &session, &user, false, ip, "code").await?;
    session.remove(PENDING);
    session.remove(NONCE);
    Ok(go(&htmx, to))
}

/// Every secret delivered so far, oldest first.
pub(crate) fn all_sent(state: &AppState) -> Vec<Sent> {
    state.auth.as_ref().map_or_else(Vec::new, |settings| {
        settings
            .passwordless
            .sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    })
}

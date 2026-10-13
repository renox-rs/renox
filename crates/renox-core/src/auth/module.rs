use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::events::{LockedOut, LoggedIn, LoggedOut, LoginFailed, Registered, announce};
use super::user::dummy_hash;
use super::{User, intended, login, logout, passwords, verification, verify_password};
use crate::db::Migration;
use crate::i18n::Lang;
use crate::validation::{Errors, Valid, Validate, ValidationError, Validator};
use crate::{
    AppState, AuthUser, ClientIp, Htmx, HxRedirect, Module, Result, Routes, Session, View, context,
    view,
};

const MIGRATIONS: &[Migration] = &[
    crate::db::framework_migration!("auth", "00010101000000_create_users_table"),
    crate::db::framework_migration!("auth", "00010101000001_create_password_reset_tokens_table"),
    crate::db::framework_migration!("auth", "00010101000002_create_personal_access_tokens_table"),
    crate::db::framework_migration!("auth", "00010101000003_create_notifications_table"),
    crate::db::framework_migration!("auth", "00010101000004_add_sessions_revoked_at_to_users"),
    crate::db::framework_migration!(
        "auth",
        "00010101000005_add_abilities_to_personal_access_tokens"
    ),
    crate::db::framework_migration!("auth", "00010101000006_create_revoked_sessions_table"),
    crate::db::framework_migration!("auth", "00010101000007_create_device_tokens_table"),
];

pub(crate) struct Settings {
    pub(super) password: crate::validation::Password,
    pub(super) registration: bool,
    redirect_to: Option<String>,
    pub(super) verify_email: bool,
    rules: Option<RulesFn>,
    pub(super) on_registered: Option<RegisteredFn>,
}

type RulesFn = Arc<dyn Fn(&Registration, &mut Validator) + Send + Sync>;
pub(super) type RegisteredFn = Arc<
    dyn Fn(
            AppState,
            User,
            Registration,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result> + Send>>
        + Send
        + Sync,
>;

/// What was submitted to `/register`, for [`Auth::registration_rules`] and
/// [`Auth::on_registered`]. Add the fields to your own
/// `renox/auth/register.html` (copy the built-in one).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Registration {
    pub(super) fields: serde_json::Map<String, Value>,
}

impl Registration {
    /// A submitted field, trimmed; `""` when it's missing.
    pub fn get(&self, field: &str) -> String {
        match self.fields.get(field) {
            Some(Value::String(value)) => value.trim().to_owned(),
            Some(Value::Array(values)) => values
                .first()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned(),
            _ => String::new(),
        }
    }

    /// Every value of a field sent several times (a group of checkboxes).
    pub fn all(&self, field: &str) -> Vec<String> {
        match self.fields.get(field) {
            Some(Value::String(value)) => vec![value.clone()],
            Some(Value::Array(values)) => values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// Login, registration, password reset and email verification pages, and
/// the `users`, `password_reset_tokens` and `personal_access_tokens` tables.
///
/// Routes: `login`, `register` (unless `without_registration`), `logout`,
/// `password.request`, `password.email`, `password.reset`,
/// `password.update`, `password.confirm`, `verification.notice`,
/// `verification.verify` and `verification.send`. [`Auth::account`] adds
/// `account.show`, `account.profile`, `account.password`,
/// `account.logout_others` and `account.destroy`;
/// [`Auth::notifications`] adds the `notifications.*` routes.
/// docs/authorization.md lists each one's method and address.
/// The pages live in `renox/auth/*.html`, inside `renox/auth/layout.html`;
/// create a file with the same name under your views to replace one.
///
/// Failed logins lock out 5 tries per email and IP a minute, 20 per email in
/// 15 minutes from any IP, and 50 per IP in 15 minutes for any email (set
/// `TRUSTED_PROXIES` behind a proxy). Logging out ends this device's
/// session only; a new password or a password reset ends the other sessions.
#[derive(Clone)]
pub struct Auth {
    password: crate::validation::Password,
    account: bool,
    registration: bool,
    redirect_to: Option<String>,
    verify_email: bool,
    notifications: bool,
    rules: Option<RulesFn>,
    on_registered: Option<RegisteredFn>,
}

impl Auth {
    /// Registration on, account pages and email verification off, the default `Password` policy.
    pub fn new() -> Self {
        Self {
            password: crate::validation::Password::default(),
            account: false,
            registration: true,
            redirect_to: None,
            verify_email: false,
            notifications: false,
            rules: None,
            on_registered: None,
        }
    }

    /// Turns on the in-app notification list for the UI kit's
    /// `notification_bell`: the `notifications.*` routes (a page that is
    /// also the bell's panel, mark read or unread, delete, and a stream of
    /// Server-Sent Events for new ones), and `unread_notifications` (the
    /// logged-in user's unread count) in every view.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// App::new().module(Auth::new().notifications())
    /// # ;
    /// ```
    pub fn notifications(mut self) -> Self {
        self.notifications = true;
        self
    }

    /// Validates fields the app adds to the registration form, with the
    /// built-in ones. Their errors show next to the inputs like any other.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # let _ =
    /// Auth::new()
    ///     .registration_rules(|form, v| {
    ///         v.field("phone", &form.get("phone")).required().max(20);
    ///     })
    ///     .on_registered(|mut user, form, state| async move {
    ///         // `phone` and `role` are columns the app added to `users`.
    ///         user.set(&state.db, "phone", form.get("phone")).await?;
    ///         let first = User::query().count(&state.db).await? == 1;
    ///         user.set(&state.db, "role", if first { "admin" } else { "member" }).await
    ///     })
    /// # ;
    /// ```
    pub fn registration_rules(
        mut self,
        rules: impl Fn(&Registration, &mut Validator) + Send + Sync + 'static,
    ) -> Self {
        self.rules = Some(Arc::new(rules));
        self
    }

    /// Runs after a new user is saved and before they're logged in, e.g. to
    /// save the app's own fields or give a role. If it fails, the user is
    /// deleted again and the visitor gets the error, so they can try again.
    pub fn on_registered<F, Fut>(mut self, hook: F) -> Self
    where
        F: Fn(User, Registration, AppState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result> + Send + 'static,
    {
        self.on_registered = Some(Arc::new(move |state, user, form| {
            Box::pin(hook(user, form, state))
        }));
        self
    }

    /// Emails new users a verification link. Guard routes that need a
    /// verified address with `Routes::require_verified()`.
    pub fn verify_email(mut self) -> Self {
        self.verify_email = true;
        self
    }

    /// What passwords must contain on the register, reset and account
    /// forms; `Password::min(8)` by default.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::validation::Password;
    /// # let _ =
    /// Auth::new().password_rules(Password::min(12).mixed_case().numbers())
    /// # ;
    /// ```
    pub fn password_rules(mut self, policy: crate::validation::Password) -> Self {
        self.password = policy;
        self
    }

    /// Adds the account page (`/account`, route `account.show`): change
    /// name and email (a new email must be verified again with
    /// `verify_email`), change the password, log out other devices, delete
    /// the account. The last two ask for the password. Override the page at
    /// `resources/views/renox/auth/account.html`.
    pub fn account(mut self) -> Self {
        self.account = true;
        self
    }

    /// Hides `/register`, e.g. for back-office apps where an admin adds users.
    pub fn without_registration(mut self) -> Self {
        self.registration = false;
        self
    }

    /// Where to go after logging in or registering when no page asked for
    /// the login. Defaults to the `home` route, or `/`.
    pub fn redirect_to(mut self, path: &str) -> Self {
        self.redirect_to = Some(path.to_owned());
        self
    }

    fn settings(&self) -> Settings {
        Settings {
            password: self.password.clone(),
            registration: self.registration,
            redirect_to: self.redirect_to.clone(),
            verify_email: self.verify_email,
            rules: self.rules.clone(),
            on_registered: self.on_registered.clone(),
        }
    }
}

impl Default for Auth {
    fn default() -> Self {
        Self::new()
    }
}

impl Module for Auth {
    fn name(&self) -> &'static str {
        "auth"
    }

    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS
    }

    fn register(&self, app: &mut crate::Registry) {
        // For other ways of logging in (a social login): `auth::sign_in`.
        app.auth = Some(Arc::new(self.settings()));
        if self.notifications {
            app.share(
                "unread_notifications",
                |ctx: crate::view::ViewContext| async move {
                    match ctx.user {
                        Some(user) => user.unread_notification_count(&ctx.state.db).await,
                        None => Ok(0),
                    }
                },
            );
        }
        // Schedule it, e.g. daily: `app.schedule().daily_at("03:00", …)`.
        app.command(
            "tokens:prune",
            "Delete API tokens that expired more than a day ago",
            |_args, state| async move {
                let day = std::time::Duration::from_secs(24 * 60 * 60);
                let pruned = super::prune_expired_tokens(&state.db, day).await?
                    + super::DeviceToken::prune_expired(&state.db, day).await?;
                println!("Deleted {pruned} expired API tokens.");
                Ok(())
            },
        );
        app.command(
            "notifications:prune",
            "Delete notifications read more than --days ago (default 30)",
            |args, state| async move {
                let days: u64 = args
                    .value("--days")
                    .unwrap_or("30")
                    .parse()
                    .map_err(|_| crate::Error::BadRequest("--days must be a number".into()))?;
                let age = std::time::Duration::from_secs(days * 24 * 60 * 60);
                let pruned = super::prune_read_notifications(&state.db, age).await?;
                println!("Deleted {pruned} notifications read more than {days} days ago.");
                Ok(())
            },
        );
    }

    fn routes(&self) -> Routes {
        let settings = Arc::new(self.settings());

        let mut guest = Routes::new()
            .get("/login", show_login)
            .post("/login", store_login)
            .name("login");
        if self.registration {
            guest = guest
                .get("/register", show_register)
                .post("/register", store_register)
                .name("register");
        }
        let guest = guest
            .get("/forgot-password", passwords::show_forgot)
            .name("password.request")
            .post("/forgot-password", passwords::send_link)
            .name("password.email")
            .get("/reset-password/{token}", passwords::show_reset)
            .name("password.reset")
            .post("/reset-password", passwords::reset)
            .name("password.update")
            .guest_only();
        let verification = Routes::new()
            .get("/verify-email", verification::notice)
            .name("verification.notice")
            .get("/verify-email/{id}/{hash}", verification::verify)
            .name("verification.verify")
            .post("/email/verification-notification", verification::resend)
            .name("verification.send")
            .require_auth();
        let confirm = Routes::new()
            .get("/confirm-password", super::account::show_confirm)
            .post("/confirm-password", super::account::confirm)
            .name("password.confirm")
            .require_auth();
        let mut routes = guest
            .merge(verification)
            .merge(confirm)
            .merge(Routes::new().post("/logout", destroy).name("logout"));
        if self.account {
            routes = routes.merge(super::account::routes());
        }
        if self.notifications {
            routes = routes.merge(super::inbox::routes());
        }
        routes.route_layer(Extension(settings))
    }
}

/// The built-in (English) texts, with the app's `renox.auth.*`
/// translations for the request's language on top.
pub(super) fn texts(lang: &Lang) -> Value {
    let mut base = text();
    if let Value::Object(map) = &mut base {
        for (key, value) in lang.texts().iter() {
            if let Some(key) = key.strip_prefix("renox.auth.") {
                map.insert(key.to_owned(), Value::String(value.clone()));
            }
        }
    }
    base
}

/// A built-in message (e.g. `auth.failed`), translated if the app's lang file has it.
fn message(lang: &Lang, key: &str, params: &[(&str, String)]) -> String {
    let template = crate::validation::template_for(Some(&lang.texts()), key);
    crate::i18n::format(&template, params, None)
}

pub(super) fn after_login(state: &AppState, settings: &Settings, session: &Session) -> String {
    let fallback = settings
        .redirect_to
        .clone()
        .unwrap_or_else(|| state.url("home", &[]).unwrap_or_else(|_| "/".into()));
    intended(session, fallback)
}

/// A full page load after logging in or out, since the layout changes.
pub(super) fn go(htmx: &Htmx, to: String) -> Response {
    if htmx.request {
        HxRedirect(to).into_response()
    } else {
        Redirect::to(&to).into_response()
    }
}

async fn show_login(Extension(settings): Extension<Arc<Settings>>, lang: Lang) -> View {
    view(
        "renox/auth/login.html",
        context! { registration => settings.registration, text => texts(&lang) },
    )
}

#[derive(Deserialize, Serialize)]
struct LoginForm {
    email: String,
    password: String,
    remember: Option<String>,
}

/// Built-in labels for Renox's own forms; an app's lang file wins.
pub(super) fn label(_v: &Validator, field: &'static str) -> &'static str {
    match field {
        "current_password" => "current password",
        _ => field,
    }
}

impl Validate for LoginForm {
    fn rules(&self, v: &mut Validator) {
        v.field("email", &super::user::normalize_email(&self.email))
            .required()
            .email();
        let password = label(v, "password");
        v.field("password", &self.password)
            .fallback_label(password)
            .required();
    }
}

async fn store_login(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    lang: Lang,
    Valid(form): Valid<LoginForm>,
) -> Result<Response> {
    let failed = |key: &str, seconds: Option<u64>| {
        let mut errors = Errors::new();
        let seconds = seconds.map(|s| s.to_string()).unwrap_or_default();
        errors.add("email", message(&lang, key, &[("seconds", seconds)]));
        ValidationError::new(errors)
            .with_input(&json!({ "email": form.email, "remember": form.remember }))
    };

    let address = ip.map(|ip| ip.to_string());
    if let Some(seconds) = state.throttle.blocked_for(&form.email, ip).await {
        let event = LockedOut {
            email: form.email.clone(),
            ip: address,
            seconds,
        };
        announce(&state, event).await;
        return Err(failed("auth.throttle", Some(seconds)).into());
    }

    let user = User::find_by_email(&state.db, &form.email).await?;
    // Hash even when the email is unknown, so timing doesn't reveal which emails exist.
    let hash = user
        .as_ref()
        .map_or_else(dummy_hash, |u| u.password.clone());
    let valid = verify_password(&form.password, &hash).await;
    let Some(mut user) = user.filter(|_| valid) else {
        state.throttle.fail(&form.email, ip).await;
        let event = LoginFailed {
            email: form.email.clone(),
            ip: address,
        };
        announce(&state, event).await;
        return Err(failed("auth.failed", None).into());
    };
    user.rehash_if_needed(&state.db, &form.password).await?;

    // A second step (two-factor authentication) first: the login waits in
    // the session. The throttle isn't cleared until it's passed, so knowing
    // the password doesn't reset the count of wrong codes.
    if let Some(second) = &state.second_factor
        && (second.required)(user.clone(), state.clone()).await?
    {
        let to = after_login(&state, &settings, &session);
        super::second_factor::begin(&session, &user, &form.email, form.remember.is_some(), to)?;
        let challenge = state.url(&second.challenge, &[])?;
        return Ok(go(&htmx, challenge));
    }

    state.throttle.clear(&form.email, ip).await;
    let remember = form
        .remember
        .is_some()
        .then_some(state.config.remember_lifetime);
    login(&session, &user, remember)?;
    super::account::mark_confirmed(&session)?;
    let event = LoggedIn {
        user_id: user.id,
        ip: address,
    };
    announce(&state, event).await;
    Ok(go(&htmx, after_login(&state, &settings, &session)))
}

async fn show_register(lang: Lang) -> View {
    view(
        "renox/auth/register.html",
        context! { text => texts(&lang) },
    )
}

#[derive(Deserialize)]
struct RegisterForm {
    name: String,
    email: String,
    password: String,
    password_confirmation: Option<String>,
}

impl Validate for RegisterForm {
    fn rules(&self, v: &mut Validator) {
        let (name, password) = (label(v, "name"), label(v, "password"));
        v.field("name", &self.name)
            .fallback_label(name)
            .required()
            .max(255);
        // Checked as it will be stored, so `unique` ignores case on every database.
        v.field("email", &super::user::normalize_email(&self.email))
            .required()
            .email()
            .max(255)
            .unique("users", "email");
        // Checked with the app's policy in `store_register`.
        let _ = password;
    }
}

async fn store_register(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    req: axum::extract::Request,
) -> Result<Response> {
    let rules = settings.rules.clone();
    let policy = settings.password.clone();
    let validated = crate::validation::extract::validate_request(
        req,
        &state,
        move |form: &RegisterForm, fields, v| {
            let password = label(v, "password");
            v.field("password", &form.password)
                .fallback_label(password)
                .required()
                .password(&policy)
                .confirmed(&form.password_confirmation);
            if let Some(rules) = &rules {
                rules(
                    &Registration {
                        fields: fields.clone(),
                    },
                    v,
                );
            }
        },
    )
    .await;
    let (form, fields) = match validated {
        Ok(validated) => validated,
        Err(rejection) => return Ok(rejection),
    };
    let user = match User::register(&state.db, &form.name, &form.email, &form.password).await {
        Ok(user) => user,
        // Two sign-ups with one email at the same moment: the database's
        // unique index stops the second, which gets the `unique` rule's answer.
        Err(err) if err.is_unique_violation() => {
            let template = crate::validation::template_for(Some(&lang.texts()), "unique");
            let mut errors = Errors::new();
            errors.add("email", crate::validation::render(&template, "email", &[]));
            return Err(ValidationError::new(errors)
                .with_input(&json!({ "name": form.name, "email": form.email }))
                .into());
        }
        Err(err) => return Err(err),
    };
    let user = match &settings.on_registered {
        None => user,
        Some(hook) => {
            let id = user.id;
            if let Err(err) = hook(state.clone(), user, Registration { fields }).await {
                // Undo the sign-up, so the visitor can try again.
                crate::db::sql("DELETE FROM users WHERE id = ?")
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                return Err(err);
            }
            // Read it back with what the hook changed.
            <User as crate::db::Model>::find_or_404(&state.db, id).await?
        }
    };
    if settings.verify_email {
        verification::send_verification(&state, &user).await?;
    }
    let event = Registered {
        user_id: user.id,
        email: user.email.clone(),
    };
    // A module whose second step every user must pass (not only the ones
    // who set it up) gets it here too, as at the login page.
    if let Some(second) = &state.second_factor
        && (second.required)(user.clone(), state.clone()).await?
    {
        let to = after_login(&state, &settings, &session);
        super::second_factor::begin(&session, &user, &user.email, false, to)?;
        announce(&state, event).await;
        let challenge = state.url(&second.challenge, &[])?;
        return Ok(go(&htmx, challenge));
    }
    login(&session, &user, None)?;
    announce(&state, event).await;
    Ok(go(&htmx, after_login(&state, &settings, &session)))
}

async fn destroy(
    State(state): State<AppState>,
    session: Session,
    user: Option<AuthUser>,
    htmx: Htmx,
) -> Result<Response> {
    logout(&state.db, &session).await?;
    if let Some(user) = user {
        announce(&state, LoggedOut { user_id: user.id }).await;
    }
    Ok(go(
        &htmx,
        state.url("home", &[]).unwrap_or_else(|_| "/".into()),
    ))
}

/// Words on the built-in pages (English; apps translate them with
/// `renox.auth.*` keys in their lang files).
pub(super) fn text() -> Value {
    json!({
        "login_title": "Log in",
        "register_title": "Create an account",
        "name": "Name",
        "email": "Email",
        "password": "Password",
        "password_confirmation": "Confirm password",
        "remember": "Remember me",
        "login_button": "Log in",
        "register_button": "Register",
        "no_account": "No account yet?",
        "have_account": "Already registered?",
        "forgot_link": "Forgot your password?",
        "forgot_title": "Forgot your password?",
        "forgot_intro": "Enter your email and we'll send you a link to choose a new password.",
        "send_link": "Email me a reset link",
        "back_to_login": "Back to log in",
        "reset_title": "Choose a new password",
        "reset_button": "Reset password",
        "reset_link_sent": "If that email has an account, a reset link is on its way.",
        "reset_invalid": "This password reset link is invalid or has expired.",
        "password_reset_done": "Your password has been reset. You can log in now.",
        "verify_title": "Verify your email",
        "verify_intro": "We've emailed you a link to verify your address. Didn't get it?",
        "resend_button": "Send another link",
        "logout": "Log out",
        "verification_sent": "A new verification link has been sent.",
        "verified": "Your email address is verified.",
        "mail_reset_subject": "Reset your password",
        "mail_reset_intro": "You asked to reset your password. Choose a new one with the button below.",
        "mail_reset_outro": "The link works for 60 minutes. If you didn't ask for this, ignore this email.",
        "mail_verify_subject": "Verify your email address",
        "mail_verify_intro": "Please confirm that this is your email address.",
        "mail_verify_outro": "The link works for 60 minutes.",
        "account_title": "Your account",
        "profile_title": "Profile",
        "profile_saved": "Your profile is saved.",
        "email_unverified": "Your new email address isn't verified yet: check your inbox.",
        "save": "Save",
        "password_title": "Change password",
        "set_password_title": "Set a password",
        "set_password_intro": "You log in with a linked account. Choose a password to log in with it too.",
        "current_password": "Current password",
        "new_password": "New password",
        "password_changed": "Your password is changed. Your other devices are logged out.",
        "other_devices_title": "Other devices",
        "other_devices_intro": "Log out everywhere else, e.g. on a phone you lost.",
        "other_devices_button": "Log out other devices",
        "other_devices_logged_out": "Your other devices are logged out.",
        "delete_title": "Delete account",
        "delete_intro": "Your account and its data are deleted for good.",
        "delete_button": "Delete my account",
        "delete_confirm": "Delete your account for good?",
        "confirm_title": "Confirm your password",
        "confirm_intro": "This is a secure area. Please confirm your password to continue.",
        "confirm_button": "Confirm",
    })
}

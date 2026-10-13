//! The account page (`Auth::account`) and password confirmation
//! (`Routes::require_password_confirmed`).

use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::events::{
    AccountDeleted, OtherDevicesLoggedOut, PasswordChanged, ProfileUpdated, announce,
};
use super::module::{Settings, go, label, texts};
use super::{User, change_password, logout_other_devices, verification};
use crate::db::Model;
use crate::i18n::Lang;
use crate::validation::{Errors, Validate, ValidationError, Validator};
use crate::{AppState, AuthUser, Htmx, Result, Routes, Session, View, context, view};

/// When the password was last typed, in unix seconds.
const CONFIRMED_AT: &str = "_password_confirmed_at";
/// How long a password confirmation lasts, as in Laravel.
pub(crate) const CONFIRM_FOR: u64 = 3 * 60 * 60;
const CONFIRM_INTENDED: &str = "_password_confirm_intended";

pub(super) fn routes() -> Routes {
    Routes::new()
        .get("/account", show)
        .name("account.show")
        .put("/account/profile", update_profile)
        .name("account.profile")
        .put("/account/password", update_password)
        .name("account.password")
        .post("/account/logout-others", logout_others)
        .name("account.logout_others")
        .delete("/account", destroy)
        .name("account.destroy")
        .require_auth()
}

fn unix_now() -> u64 {
    crate::clock::unix_secs().max(0) as u64
}

/// Records that the user just typed their password.
pub(crate) fn mark_confirmed(session: &Session) -> Result {
    session.put(CONFIRMED_AT, unix_now())
}

/// Where to go once the password is confirmed: the page that asked, else `/`.
pub(crate) fn confirmed_destination(session: &Session) -> String {
    session
        .pull::<String>(CONFIRM_INTENDED)
        .filter(|path| crate::htmx::is_local_path(path))
        .unwrap_or_else(|| "/".into())
}

/// Whether the password was typed in the last three hours.
pub(crate) fn recently_confirmed(session: &Session) -> bool {
    session
        .get::<u64>(CONFIRMED_AT)
        .is_some_and(|at| unix_now().saturating_sub(at) < CONFIRM_FOR)
}

/// Route guard: sends users who haven't typed their password lately to
/// `/confirm-password`, then back.
pub(crate) async fn require_password_confirmed(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(session) = req.extensions().get::<Session>().cloned() else {
        return next.run(req).await;
    };
    if recently_confirmed(&session) {
        return next.run(req).await;
    }
    // After confirming, go back: to this page for a GET; for a form that
    // posts, puts or deletes, to the page the form was on (it can't be
    // replayed), taken from `Referer` when it's this site's own path.
    let back = if req.method() == axum::http::Method::GET {
        req.uri().path_and_query().map(|p| p.as_str().to_owned())
    } else {
        crate::htmx::same_site_referer(req.headers())
    };
    if let Some(back) = back {
        let _ = session.put(CONFIRM_INTENDED, back);
    }
    let confirm = req
        .extensions()
        .get::<AppState>()
        .and_then(|state| state.url("password.confirm", &[]).ok())
        .unwrap_or_else(|| "/confirm-password".into());
    if crate::Htmx::from_headers(req.headers()).request {
        return crate::HxRedirect(confirm).into_response();
    }
    Redirect::to(&confirm).into_response()
}

pub(super) async fn show_confirm(lang: Lang) -> View {
    view(
        "renox/auth/confirm-password.html",
        context! { text => texts(&lang) },
    )
}

#[derive(Deserialize)]
pub(super) struct ConfirmForm {
    password: String,
}

impl Validate for ConfirmForm {
    fn rules(&self, v: &mut Validator) {
        let password = label(v, "password");
        v.field("password", &self.password)
            .fallback_label(password)
            .required();
    }
}

/// Checks `password` against the user's, or answers with a validation
/// error on `field`.
async fn check_password(
    user: &User,
    password: &str,
    field: &str,
    lang: &Lang,
) -> std::result::Result<(), crate::Error> {
    if user.check_password(password).await {
        return Ok(());
    }
    let template = crate::validation::template_for(Some(&lang.texts()), "current_password");
    // The app's name for the field (`renox.validation.attributes.<field>`)
    // when its lang file has one.
    let name = lang
        .texts()
        .get(&format!("renox.validation.attributes.{field}"))
        .cloned()
        .unwrap_or_else(|| match field {
            "current_password" => "current password".to_owned(),
            _ => "password".to_owned(),
        });
    let mut errors = Errors::new();
    errors.add(field, crate::validation::render(&template, &name, &[]));
    Err(ValidationError::new(errors).into())
}

pub(super) async fn confirm(
    user: AuthUser,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    crate::validation::Valid(form): crate::validation::Valid<ConfirmForm>,
) -> Result<Response> {
    check_password(user.user(), &form.password, "password", &lang).await?;
    mark_confirmed(&session)?;
    Ok(go(&htmx, confirmed_destination(&session)))
}

/// A section another module adds to `/account` (`Registry::account_section`).
pub(crate) struct AccountSection {
    template: String,
    pub(crate) order: i32,
    data: SectionFn,
}

type SectionFn = Arc<
    dyn Fn(
            User,
            AppState,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;

pub(crate) fn section<F, Fut>(template: &str, order: i32, data: F) -> AccountSection
where
    F: Fn(User, AppState) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value>> + Send + 'static,
{
    AccountSection {
        template: template.to_owned(),
        order,
        data: Arc::new(move |user, state| Box::pin(data(user, state))),
    }
}

async fn show(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
) -> Result<View> {
    // Read what each section needs first: the closures borrow nothing across
    // the awaits, so the handler's future stays `Send`.
    let registered: Vec<(String, SectionFn)> = state
        .account_sections
        .iter()
        .map(|section| (section.template.clone(), section.data.clone()))
        .collect();
    let mut sections = Vec::with_capacity(registered.len());
    for (template, data) in registered {
        let data = data(user.user().clone(), state.clone()).await?;
        sections.push(context! { template, data });
    }
    Ok(view(
        "renox/auth/account.html",
        context! {
            text => texts(&lang),
            user => user.user(),
            has_password => user.has_password(),
            verify_email => settings.verify_email,
            sections,
        },
    ))
}

#[derive(Deserialize)]
struct ProfileForm {
    name: String,
    email: String,
}

impl Validate for ProfileForm {
    fn rules(&self, v: &mut Validator) {
        let name = label(v, "name");
        v.field("name", &self.name)
            .fallback_label(name)
            .required()
            .max(255);
        v.field("email", &super::user::normalize_email(&self.email))
            .required()
            .email()
            .max(255);
    }
}

async fn update_profile(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    req: axum::extract::Request,
) -> Result<Response> {
    let id = user.id;
    let validated = crate::validation::extract::validate_request(
        req,
        &state,
        move |form: &ProfileForm, _, v| {
            v.field("email", &super::user::normalize_email(&form.email))
                .unique("users", "email")
                .ignore(id);
        },
    )
    .await;
    let form = match validated {
        Ok((form, _)) => form,
        Err(rejection) => return Ok(rejection),
    };
    let mut me = user.user().clone();
    let email = super::user::normalize_email(&form.email);
    let email_changed = email != me.email;
    me.name = form.name.trim().to_owned();
    if email_changed {
        me.email = email;
        if settings.verify_email {
            me.email_verified_at = None;
        }
    }
    me.save(&state.db).await?;
    if email_changed && settings.verify_email {
        verification::send_verification(&state, &me).await?;
    }
    let event = ProfileUpdated {
        user_id: me.id,
        email_changed,
    };
    announce(&state, event).await;
    session.flash("status", &texts(&lang)["profile_saved"])?;
    Ok(go(&htmx, state.url("account.show", &[])?))
}

#[derive(Deserialize)]
struct PasswordForm {
    /// Not on the form of a user without a password.
    #[serde(default)]
    current_password: String,
    password: String,
    password_confirmation: Option<String>,
}

impl Validate for PasswordForm {
    fn rules(&self, _v: &mut Validator) {}
}

/// The password typed to confirm an action on the account page (none for
/// a user without one).
#[derive(Deserialize)]
struct PasswordCheck {
    #[serde(default)]
    password: String,
}

impl Validate for PasswordCheck {
    fn rules(&self, _v: &mut Validator) {}
}

/// A user without a password (a social login) proves who they are with a
/// recent confirmation instead (logging in, or `/confirm-password` another
/// way): without one, the browser goes to `/confirm-password` and comes
/// back to the account page.
fn needs_confirmation(
    state: &AppState,
    session: &Session,
    user: &User,
    htmx: &Htmx,
) -> Option<Response> {
    if user.has_password() || recently_confirmed(session) {
        return None;
    }
    if let Ok(account) = state.url("account.show", &[]) {
        let _ = session.put(CONFIRM_INTENDED, account);
    }
    let confirm = state
        .url("password.confirm", &[])
        .unwrap_or_else(|_| "/confirm-password".into());
    Some(go(htmx, confirm))
}

/// Checks the typed password of a user who has one.
async fn check_typed(user: &User, typed: &str, field: &str, lang: &Lang) -> Result {
    if user.has_password() {
        check_password(user, typed, field, lang).await?;
    }
    Ok(())
}

/// Validates a [`PasswordCheck`]: the password is required when the user has one.
async fn password_check(
    state: &AppState,
    user: &User,
    req: axum::extract::Request,
) -> std::result::Result<PasswordCheck, Box<Response>> {
    let has_password = user.has_password();
    crate::validation::extract::validate_request(req, state, move |form: &PasswordCheck, _, v| {
        if has_password {
            let password = label(v, "password");
            v.field("password", &form.password)
                .fallback_label(password)
                .required();
        }
    })
    .await
    .map(|(form, _)| form)
    .map_err(Box::new)
}

async fn update_password(
    Extension(settings): Extension<Arc<Settings>>,
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    req: axum::extract::Request,
) -> Result<Response> {
    if let Some(confirm) = needs_confirmation(&state, &session, user.user(), &htmx) {
        return Ok(confirm);
    }
    let policy = settings.password.clone();
    let has_password = user.has_password();
    let validated = crate::validation::extract::validate_request(
        req,
        &state,
        move |form: &PasswordForm, _, v| {
            if has_password {
                let current = label(v, "current_password");
                v.field("current_password", &form.current_password)
                    .fallback_label(current)
                    .required();
            }
            let password = label(v, "password");
            v.field("password", &form.password)
                .fallback_label(password)
                .required()
                .password(&policy)
                .confirmed(&form.password_confirmation);
        },
    )
    .await;
    let form = match validated {
        Ok((form, _)) => form,
        Err(rejection) => return Ok(rejection),
    };
    check_typed(
        user.user(),
        &form.current_password,
        "current_password",
        &lang,
    )
    .await?;
    let mut me = user.user().clone();
    change_password(&state.db, &session, &mut me, &form.password).await?;
    mark_confirmed(&session)?;
    announce(&state, PasswordChanged { user_id: me.id }).await;
    session.flash("status", &texts(&lang)["password_changed"])?;
    Ok(go(&htmx, state.url("account.show", &[])?))
}

async fn logout_others(
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    req: axum::extract::Request,
) -> Result<Response> {
    if let Some(confirm) = needs_confirmation(&state, &session, user.user(), &htmx) {
        return Ok(confirm);
    }
    let form = match password_check(&state, user.user(), req).await {
        Ok(form) => form,
        Err(rejection) => return Ok(*rejection),
    };
    check_typed(user.user(), &form.password, "password", &lang).await?;
    logout_other_devices(&state.db, &session, user.user()).await?;
    announce(&state, OtherDevicesLoggedOut { user_id: user.id }).await;
    session.flash("status", &texts(&lang)["other_devices_logged_out"])?;
    Ok(go(&htmx, state.url("account.show", &[])?))
}

async fn destroy(
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    req: axum::extract::Request,
) -> Result<Response> {
    if let Some(confirm) = needs_confirmation(&state, &session, user.user(), &htmx) {
        return Ok(confirm);
    }
    let form = match password_check(&state, user.user(), req).await {
        Ok(form) => form,
        Err(rejection) => return Ok(*rejection),
    };
    check_typed(user.user(), &form.password, "password", &lang).await?;
    user.delete_account(&state.db).await?;
    session.flush();
    let event = AccountDeleted {
        user_id: user.id,
        email: user.email.clone(),
    };
    announce(&state, event).await;
    Ok(go(
        &htmx,
        state.url("home", &[]).unwrap_or_else(|_| "/".into()),
    ))
}

impl User {
    /// Deletes the user (their tokens, notifications and sessions go with
    /// the row) and their data grid preferences. The account page's "delete
    /// account" does this.
    pub async fn delete_account(&self, db: &crate::db::Db) -> Result {
        let mut tx = db.begin().await?;
        // Every app has this table, and not every app has `users`, so it has
        // no foreign key to cascade from.
        crate::db::sql("DELETE FROM grid_preferences WHERE user_id = ?")
            .bind(self.id)
            .execute(&mut tx)
            .await?;
        crate::db::sql("DELETE FROM users WHERE id = ?")
            .bind(self.id)
            .execute(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

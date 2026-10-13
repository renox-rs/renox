use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::response::Response;
use chrono::TimeDelta;
use serde::Deserialize;
use serde_json::json;

use super::User;
use super::module::{go, texts};
use super::tokens::sha256_hex;
use crate::crypto::{constant_time_eq, random_token};
use crate::db::{DateTime, now};
use crate::i18n::Lang;
use crate::validation::{Errors, Valid, Validate, ValidationError, Validator};
use crate::{AppState, Htmx, Result, Session, View, context, view};

/// How long a reset link works.
const EXPIRES: Duration = Duration::from_secs(60 * 60);
/// Minimum time between two reset emails to the same address.
const RESEND_AFTER: Duration = Duration::from_secs(60);

pub(super) async fn show_forgot(lang: Lang) -> View {
    view(
        "renox/auth/forgot-password.html",
        context! { text => texts(&lang) },
    )
}

#[derive(Deserialize)]
pub(super) struct ForgotForm {
    email: String,
}

impl Validate for ForgotForm {
    fn rules(&self, v: &mut Validator) {
        v.field("email", &super::user::normalize_email(&self.email))
            .required()
            .email();
    }
}

/// Emails a reset link if the address has an account. The reply is the same
/// either way, so the form can't be used to find out who is registered.
pub(super) async fn send_link(
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    Valid(form): Valid<ForgotForm>,
) -> Result<Response> {
    let text = texts(&lang);
    if let Some(user) = User::find_by_email(&state.db, &form.email).await? {
        let last: Option<DateTime> =
            crate::db::sql("SELECT created_at FROM password_reset_tokens WHERE email = ?")
                .bind(&user.email)
                .scalar_optional(&state.db)
                .await?;
        let recently = last
            .is_some_and(|at| now() - at < TimeDelta::from_std(RESEND_AFTER).unwrap_or_default());
        if !recently {
            let token = random_token();
            crate::db::sql(
                "INSERT INTO password_reset_tokens (email, token, created_at) VALUES (?, ?, ?) \
                 ON CONFLICT (email) DO UPDATE SET token = excluded.token, created_at = excluded.created_at",
            )
            .bind(&user.email)
            .bind(sha256_hex(&token))
            .bind(now())
            .execute(&state.db)
            .await?;

            let email: String = form_urlencoded::byte_serialize(user.email.as_bytes()).collect();
            let link = format!(
                "{}?email={email}",
                state.absolute_url("password.reset", &[&token])?
            );
            let subject = text["mail_reset_subject"].as_str().unwrap_or_default();
            let mail = state.mail_view(
                &user.email,
                subject,
                "renox/mail/auth/reset-password",
                context! { link, text },
            )?;
            state.mailer.send(mail).await?;
        }
    }
    session.flash("status", &text["reset_link_sent"])?;
    Ok(go(&htmx, state.url("password.request", &[])?))
}

#[derive(Deserialize)]
pub(super) struct EmailQuery {
    email: Option<String>,
}

pub(super) async fn show_reset(
    lang: Lang,
    Path(token): Path<String>,
    Query(query): Query<EmailQuery>,
) -> View {
    view(
        "renox/auth/reset-password.html",
        context! { token, email => query.email.unwrap_or_default(), text => texts(&lang) },
    )
}

#[derive(Deserialize)]
pub(super) struct ResetForm {
    token: String,
    email: String,
    password: String,
    password_confirmation: Option<String>,
}

impl Validate for ResetForm {
    fn rules(&self, v: &mut Validator) {
        let password = super::module::label(v, "password");
        v.field("token", &self.token).required();
        v.field("email", &super::user::normalize_email(&self.email))
            .required()
            .email();
        // The password is checked with the app's policy in `reset`.
        let _ = password;
    }
}

pub(super) async fn reset(
    axum::extract::Extension(settings): axum::extract::Extension<
        std::sync::Arc<super::module::Settings>,
    >,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    lang: Lang,
    req: axum::extract::Request,
) -> Result<Response> {
    let policy = settings.password.clone();
    let validated =
        crate::validation::extract::validate_request(req, &state, move |form: &ResetForm, _, v| {
            let password = super::module::label(v, "password");
            v.field("password", &form.password)
                .label(password)
                .required()
                .password(&policy)
                .confirmed(&form.password_confirmation);
        })
        .await;
    let form = match validated {
        Ok((form, _)) => form,
        Err(rejection) => return Ok(rejection),
    };
    let text = texts(&lang);
    let row = crate::db::sql("SELECT token, created_at FROM password_reset_tokens WHERE email = ?")
        .bind(super::user::normalize_email(&form.email))
        .fetch_optional(&state.db)
        .await?;
    let fresh =
        |created: DateTime| now() - created < TimeDelta::from_std(EXPIRES).unwrap_or_default();
    let valid = match &row {
        Some(row) => {
            let hash: String = row.try_get("token")?;
            constant_time_eq(&hash, &sha256_hex(&form.token)) && fresh(row.try_get("created_at")?)
        }
        None => false,
    };
    let user = match valid {
        true => User::find_by_email(&state.db, &form.email).await?,
        false => None,
    };
    let Some(mut user) = user else {
        let mut errors = Errors::new();
        errors.add("email", text["reset_invalid"].as_str().unwrap_or_default());
        return Err(ValidationError::new(errors)
            .with_input(&json!({ "email": form.email }))
            .into());
    };

    user.set_password(&state.db, &form.password).await?;
    // Whoever reset the password may be recovering a stolen account: API
    // tokens made before it stop working too, like other sessions do.
    user.revoke_tokens(&state.db).await?;
    crate::db::sql("DELETE FROM password_reset_tokens WHERE email = ?")
        .bind(&user.email)
        .execute(&state.db)
        .await?;
    let event = super::events::PasswordReset { user_id: user.id };
    super::events::announce(&state, event).await;
    session.flash("status", &text["password_reset_done"])?;
    Ok(go(&htmx, state.url("login", &[])?))
}

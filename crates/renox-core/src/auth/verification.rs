use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};

use super::module::{go, texts};
use super::tokens::sha256_hex;
use super::{AuthUser, User};
use crate::crypto::constant_time_eq;
use crate::db::{Model, now};
use crate::i18n::Lang;
use crate::signed::ValidSignature;
use crate::{AppState, Error, Htmx, Result, Session, context, view};

/// How long a verification link works.
const EXPIRES: Duration = Duration::from_secs(60 * 60);

/// Emails `user` a signed link that marks their address as verified.
pub async fn send_verification(state: &AppState, user: &User) -> Result {
    let text = texts(&Lang::of(state, &state.config.locale));
    let link = state.signed_url(
        "verification.verify",
        &[&user.id, &sha256_hex(&user.email)],
        EXPIRES,
    )?;
    let subject = text["mail_verify_subject"].as_str().unwrap_or_default();
    let mail = state.mail_view(
        &user.email,
        subject,
        "renox/mail/auth/verify-email",
        context! { link, text },
    )?;
    state.mailer.send(mail).await
}

fn home(state: &AppState) -> String {
    state.url("home", &[]).unwrap_or_else(|_| "/".into())
}

pub(super) async fn notice(auth: AuthUser, State(state): State<AppState>, lang: Lang) -> Response {
    if auth.email_verified_at.is_some() {
        return axum::response::Redirect::to(&home(&state)).into_response();
    }
    view(
        "renox/auth/verify-email.html",
        context! { text => texts(&lang) },
    )
    .into_response()
}

pub(super) async fn verify(
    auth: AuthUser,
    _: ValidSignature,
    State(state): State<AppState>,
    session: Session,
    lang: Lang,
    Path((id, hash)): Path<(i64, String)>,
) -> Result<Response> {
    if id != auth.id || !constant_time_eq(&hash, &sha256_hex(&auth.email)) {
        return Err(Error::Forbidden);
    }
    if auth.email_verified_at.is_none() {
        let mut user = auth.user().clone();
        user.email_verified_at = Some(now());
        user.save(&state.db).await?;
        let event = super::events::EmailVerified { user_id: user.id };
        super::events::announce(&state, event).await;
    }
    session.flash("status", &texts(&lang)["verified"])?;
    Ok(axum::response::Redirect::to(&home(&state)).into_response())
}

pub(super) async fn resend(
    auth: AuthUser,
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    lang: Lang,
) -> Result<Response> {
    if auth.email_verified_at.is_some() {
        return Ok(go(&htmx, home(&state)));
    }
    send_verification(&state, auth.user()).await?;
    session.flash("status", &texts(&lang)["verification_sent"])?;
    Ok(go(&htmx, state.url("verification.notice", &[])?))
}

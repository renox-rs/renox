//! The module's pages: turning two-factor authentication on (a QR code, a
//! code to confirm), the recovery codes, turning it off, and the login
//! challenge after the password.

use renox::auth::{complete_login, pending_login};
use renox::db::Encrypted;
use renox::prelude::*;
use serde::Deserialize;

use crate::events::{RecoveryCodeUsed, TwoFactorDisabled, TwoFactorEnabled};
use crate::{TwoFactorCredential, qr, recovery, totp};

/// Where the recovery codes wait, for the one page that shows them.
const CODES: &str = "_two_factor_codes";

/// Every route, named `two-factor.*`.
pub(crate) fn routes() -> Routes {
    // After the password, before the login: for guests only.
    let challenge = Routes::new()
        .get("/two-factor/challenge", show_challenge)
        .name("two-factor.challenge")
        .post("/two-factor/challenge", check_challenge)
        .name("two-factor.verify")
        .guest_only();
    // Changing it asks for the password first (three hours, as elsewhere).
    let manage = Routes::new()
        .post("/two-factor/enable", enable)
        .name("two-factor.enable")
        .get("/two-factor/setup", show_setup)
        .name("two-factor.setup")
        .post("/two-factor/confirm", confirm)
        .name("two-factor.confirm")
        .post("/two-factor/recovery-codes", regenerate)
        .name("two-factor.recovery-codes.regenerate")
        .delete("/two-factor", disable)
        .name("two-factor.disable")
        .require_password_confirmed()
        .require_auth();
    let codes = Routes::new()
        .get("/two-factor/recovery-codes", show_codes)
        .name("two-factor.recovery-codes")
        .require_auth();
    Routes::new().merge(challenge).merge(manage).merge(codes)
}

/// The account page when the app has it (`Auth::new().account()`), else `/`.
fn account(state: &AppState) -> String {
    state
        .url("account.show", &[])
        .unwrap_or_else(|_| "/".into())
}

/// Seconds since 1970 on Renox's clock (which tests can move).
fn now() -> i64 {
    renox::db::now().timestamp()
}

/// Sends the browser to `to`, the htmx way when htmx asked.
fn go(htmx: &Htmx, to: String) -> Response {
    if htmx.request {
        HxRedirect(to).into_response()
    } else {
        Redirect::to(&to).into_response()
    }
}

#[derive(Deserialize, Validate)]
struct CodeForm {
    #[validate(required, max = 40, label = "code")]
    code: String,
}

/// A form error on `code`.
fn wrong(message: &str) -> Error {
    let mut errors = Errors::new();
    errors.add("code", message);
    ValidationError::new(errors).into()
}

// ---------- turning it on ----------

/// Starts (or restarts) turning it on: a new secret, not confirmed yet, so
/// nothing changes at login until the user types a code from their app.
async fn enable(State(state): State<AppState>, user: AuthUser, htmx: Htmx) -> Result<Response> {
    let existing = TwoFactorCredential::of(&state.db, user.id).await?;
    if existing
        .as_ref()
        .is_some_and(TwoFactorCredential::is_confirmed)
    {
        return Ok(go(&htmx, account(&state)));
    }
    let secret = Encrypted::new(totp::new_secret());
    match existing {
        Some(mut credential) => {
            credential.secret = secret;
            credential.save(&state.db).await?;
        }
        None => {
            TwoFactorCredential::create(
                &state.db,
                TwoFactorCredential {
                    user_id: user.id,
                    secret,
                    recovery_codes: "[]".into(),
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    Ok(go(&htmx, state.url("two-factor.setup", &[])?))
}

/// The QR code, the key to type in instead, and the form for the first code.
async fn show_setup(State(state): State<AppState>, user: AuthUser) -> Result<Response> {
    let Some(credential) = TwoFactorCredential::of(&state.db, user.id).await? else {
        return Ok(Redirect::to(&account(&state)).into_response());
    };
    if credential.is_confirmed() {
        // The secret isn't shown again once it's on.
        return Ok(Redirect::to(&account(&state)).into_response());
    }
    let secret = credential.secret.as_str().to_owned();
    let uri = totp::otpauth_uri(&state.config.name, &user.email, &secret);
    let qr = qr::svg(&uri, 200).unwrap_or_default();
    // In groups of four, easier to type: `ABCD EFGH …`.
    let key = secret
        .as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(view(
        "two-factor/setup.html",
        context! { qr, key, account => account(&state) },
    )
    .into_response())
}

/// Turns it on once the code from the app is right, and hands out the
/// recovery codes.
async fn confirm(
    State(state): State<AppState>,
    session: Session,
    user: AuthUser,
    htmx: Htmx,
    Valid(form): Valid<CodeForm>,
) -> Result<Response> {
    let Some(mut credential) = TwoFactorCredential::of(&state.db, user.id).await? else {
        return Ok(go(&htmx, account(&state)));
    };
    if credential.is_confirmed() {
        return Ok(go(&htmx, account(&state)));
    }
    let Some(step) = totp::verify(credential.secret.as_str(), &form.code, now(), None) else {
        return Err(wrong(
            "That code isn't right. Type the six digits your app shows now.",
        ));
    };
    credential.confirmed_at = Some(renox::db::now());
    credential.last_used_step = Some(step);
    let codes = credential.new_recovery_codes();
    credential.save(&state.db).await?;
    session.flash(CODES, &codes)?;
    state.emit(TwoFactorEnabled { user_id: user.id }).await?;
    Ok(go(&htmx, state.url("two-factor.recovery-codes", &[])?))
}

// ---------- recovery codes ----------

/// The recovery codes, right after they were made; once only.
async fn show_codes(State(state): State<AppState>, session: Session) -> Result<Response> {
    let Some(codes) = session.get::<Vec<String>>(CODES) else {
        return Ok(Redirect::to(&account(&state)).into_response());
    };
    let text = codes.join("\n");
    Ok(view(
        "two-factor/recovery-codes.html",
        context! { codes, text, account => account(&state) },
    )
    .into_response())
}

/// New recovery codes; the old ones stop working.
async fn regenerate(
    State(state): State<AppState>,
    session: Session,
    user: AuthUser,
    htmx: Htmx,
) -> Result<Response> {
    let Some(mut credential) = TwoFactorCredential::of(&state.db, user.id).await? else {
        return Ok(go(&htmx, account(&state)));
    };
    if !credential.is_confirmed() {
        return Ok(go(&htmx, account(&state)));
    }
    let codes = credential.new_recovery_codes();
    credential.save(&state.db).await?;
    session.flash(CODES, &codes)?;
    Ok(go(&htmx, state.url("two-factor.recovery-codes", &[])?))
}

// ---------- turning it off ----------

/// Turns it off: the secret and the recovery codes are deleted.
async fn disable(State(state): State<AppState>, user: AuthUser, htmx: Htmx) -> Result<Response> {
    if let Some(mut credential) = TwoFactorCredential::of(&state.db, user.id).await? {
        let was_on = credential.is_confirmed();
        credential.delete(&state.db).await?;
        if was_on {
            state.emit(TwoFactorDisabled { user_id: user.id }).await?;
        }
    }
    let toast = Toast::success("Two-factor authentication is off.");
    Ok((toast, go(&htmx, account(&state))).into_response())
}

// ---------- the login challenge ----------

/// Asks for the code after the right password.
async fn show_challenge(session: Session) -> Response {
    match pending_login(&session) {
        Some(_) => view("two-factor/challenge.html", context! {}).into_response(),
        // Expired (ten minutes), or nobody typed a password.
        None => Redirect::to("/login").into_response(),
    }
}

/// Checks the code (or a recovery code) and finishes the login.
async fn check_challenge(
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    Valid(form): Valid<CodeForm>,
) -> Result<Response> {
    let Some(pending) = pending_login(&session) else {
        return Ok(go(&htmx, state.url("login", &[])?));
    };
    if pending.locked_out(&state, ip).await.is_some() {
        return Err(Error::TooManyRequests);
    }
    let credential = TwoFactorCredential::of(&state.db, pending.user_id).await?;
    let passed = match credential {
        // Turned off since the password was typed: nothing more to check.
        None => true,
        Some(ref credential) if !credential.is_confirmed() => true,
        Some(mut credential) => {
            if recovery::looks_like_one(&form.code) {
                if credential.use_recovery_code(&form.code) {
                    credential.save(&state.db).await?;
                    let remaining = credential.recovery_codes_left();
                    state
                        .emit(RecoveryCodeUsed {
                            user_id: pending.user_id,
                            remaining,
                        })
                        .await?;
                    if remaining <= 2 {
                        session.flash(
                            "status",
                            format!(
                                "You have {remaining} recovery codes left: make new ones from your account page."
                            ),
                        )?;
                    }
                    true
                } else {
                    false
                }
            } else if let Some(step) = totp::verify(
                credential.secret.as_str(),
                &form.code,
                now(),
                credential.last_used_step,
            ) {
                // This code, and the ones before it, can't be used again.
                credential.last_used_step = Some(step);
                credential.save(&state.db).await?;
                true
            } else {
                false
            }
        }
    };
    if !passed {
        pending.failed(&state, ip).await;
        return Err(wrong(
            "That code isn't right. Type the six digits your app shows, or a recovery code.",
        ));
    }
    Ok(
        match complete_login(&state, &session, &pending, ip).await? {
            Some(to) => go(&htmx, to),
            // The password changed or the user is gone: start again.
            None => go(&htmx, state.url("login", &[])?),
        },
    )
}

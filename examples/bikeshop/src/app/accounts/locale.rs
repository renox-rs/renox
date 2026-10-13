//! The language, remembered in the session *and* on the account.
//!
//! Renox picks a request's language from the session (`_locale`, written by
//! `renox::i18n::remember_locale`, which the language menu and the account
//! page call), else the browser's `Accept-Language` (`App::detect_locale`),
//! else `APP_LOCALE`. That's per browser. To make the choice follow the
//! customer to a new phone, and to write their mails in it, it is also kept
//! on their login (`users.locale`), and [`middleware`] keeps the two in step
//! on every request of someone logged in:
//!
//! - the session has no language yet (a new device, just logged in) → the
//!   account's is put in the session and used for this request too;
//! - the session's language differs from the account's (they used the
//!   language menu) → the account is updated (one `UPDATE`, only then).
//!
//! It is added in `src/lib.rs` with `App::layer`, so it runs on every page
//! after Renox's own session, language and auth middleware.

use renox::axum::Extension;
use renox::axum::extract::Request;
use renox::axum::middleware::Next;
use renox::i18n::RequestLocale;
use renox::prelude::*;

use crate::app::home::LOCALES;

/// Renox's session key for the visitor's language (`remember_locale`).
const SESSION_KEY: &str = "_locale";

/// The account's column.
pub const COLUMN: &str = "locale";

/// Whether the shop is written in `locale`.
pub fn supported(locale: &str) -> bool {
    LOCALES.iter().any(|(code, _)| *code == locale)
}

/// The language saved on `user`'s account, if any.
pub fn of(user: &User) -> Option<String> {
    user.get::<String>(COLUMN).filter(|l| supported(l))
}

/// Keeps the session's language and the account's in step (see the module docs).
pub async fn middleware(
    user: Option<AuthUser>,
    session: Session,
    Extension(state): Extension<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    if let Some(user) = user {
        let mut user = user.user().clone();
        let account = of(&user);
        match session.get::<String>(SESSION_KEY) {
            None => {
                if let Some(locale) = account {
                    let _ = renox::i18n::remember_locale(&session, &locale);
                    // The whole request (`Lang`, validation, views) follows.
                    RequestLocale::switch(&mut req, &locale);
                }
            }
            Some(chosen) if supported(&chosen) && account.as_deref() != Some(chosen.as_str()) => {
                // Best effort: the page still works if the save fails.
                let _ = user.set(&state.db, COLUMN, chosen).await;
            }
            Some(_) => {}
        }
    }
    next.run(req).await
}

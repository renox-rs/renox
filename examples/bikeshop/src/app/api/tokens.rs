//! The tokens' pages: a customer's personal tokens (`/account/api-tokens`)
//! and a store's kiosk tokens (`/staff/api-tokens`, managers).
//!
//! A token is Renox's API token (`User::create_token_with`, the
//! `personal_access_tokens` table): only a SHA-256 hash of its secret is
//! stored, so the page shows the token **once**, right after it is made
//! (flashed into the next page), and never again. Each token carries only
//! the abilities ticked; revoking one deletes it, and the app or kiosk gets
//! 401 from its next request.

use renox::auth::DeviceToken;
use renox::db::Json as DbJson;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::kiosk::Kiosk;
use super::{CUSTOMER_ABILITIES, KIOSK_ABILITIES};
use crate::app::access::{self, catalogue};
use crate::app::staff::model::Store;

/// The session key holding a token just made (shown once).
pub const FLASH: &str = "api_token";
/// How long a personal token works.
pub const PERSONAL_DAYS: i64 = 365;

/// A new token: its name and abilities.
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct TokenForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub abilities: Vec<String>,
}

// [explain:api.tokens.rules]
/// The abilities a form may tick, by who makes the token.
fn check(form: &TokenForm, v: &mut Validator, allowed: &[&str]) {
    v.field("name", &form.name).required().max(60);
    v.field("abilities", &form.abilities).required();
    v.each("abilities", &form.abilities, |a| a.one_of(allowed));
}

impl Validate for TokenForm {
    fn rules(&self, v: &mut Validator) {
        let allowed: Vec<&str> = CUSTOMER_ABILITIES.iter().map(|(a, _)| *a).collect();
        check(self, v, &allowed);
    }
}
// [/explain:api.tokens.rules]

/// A kiosk's form.
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct KioskForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub abilities: Vec<String>,
}

impl Validate for KioskForm {
    fn rules(&self, v: &mut Validator) {
        let allowed: Vec<&str> = KIOSK_ABILITIES.iter().map(|(a, _)| *a).collect();
        v.field("name", &self.name).required().max(60);
        v.field("abilities", &self.abilities).required();
        v.each("abilities", &self.abilities, |a| a.one_of(&allowed));
    }
}

/// The ticks of the token form for the kit's checkbox list: `[value, label,
/// hint]`, the hint being the ability's text in the page's language.
fn options(lang: &Lang, abilities: &[(&str, &str)]) -> Vec<(String, String, String)> {
    abilities
        .iter()
        .map(|(ability, key)| ((*ability).into(), (*ability).into(), lang.t(key, &[])))
        .collect()
}

/// A token as the pages list it.
#[derive(Serialize, Debug, Clone)]
pub struct TokenRow {
    pub id: i64,
    pub name: String,
    pub abilities: Vec<String>,
    pub last_used_at: Option<DateTime>,
    pub expires_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
}

/// `GET /account/api-tokens` (`api.tokens`): the customer's tokens, the
/// form for a new one, and a new token once.
pub async fn index(
    State(db): State<Db>,
    user: AuthUser,
    session: Session,
    lang: Lang,
) -> Result<View> {
    let tokens: Vec<TokenRow> = user
        .tokens(&db)
        .await?
        .into_iter()
        .map(|t| TokenRow {
            id: t.id,
            name: t.name,
            abilities: t.abilities.unwrap_or_else(|| vec!["*".into()]),
            last_used_at: t.last_used_at,
            expires_at: t.expires_at,
            created_at: t.created_at,
        })
        .collect();
    let fresh: Option<String> = session.get(FLASH);
    Ok(view(
        "api/tokens.html",
        context! {
            tokens,
            fresh,
            abilities => options(&lang, &CUSTOMER_ABILITIES),
            base => super::base_url(),
        },
    ))
}

// [explain:api.tokens.store]
/// `POST /account/api-tokens` (`api.tokens.store`): a personal token with
/// the abilities ticked, for a year; shown once on the next page.
pub async fn store(
    State(db): State<Db>,
    user: AuthUser,
    lang: Lang,
    session: Session,
    Valid(form): Valid<TokenForm>,
) -> Result<(Toast, Redirect)> {
    let abilities: Vec<&str> = form.abilities.iter().map(String::as_str).collect();
    let expires = renox::db::now() + renox::chrono::Duration::days(PERSONAL_DAYS);
    let token = user
        .create_token_with(&db, form.name.trim(), &abilities, Some(expires))
        .await?;
    session.flash(FLASH, token.plain)?;
    Ok((
        Toast::success(lang.t("api.tokens.made", &[])),
        Redirect::route("api.tokens", &[])?,
    ))
}
// [/explain:api.tokens.store]

/// `POST /account/api-tokens/{token}/revoke` (`api.tokens.destroy`).
pub async fn destroy(
    State(db): State<Db>,
    user: AuthUser,
    lang: Lang,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    if !user.revoke_token(&db, id).await? {
        return Err(Error::NotFound);
    }
    Ok((
        Toast::success(lang.t("api.tokens.revoked", &[])),
        Redirect::route("api.tokens", &[])?,
    ))
}

/// A kiosk as the staff page lists it.
#[derive(Serialize, Debug, Clone)]
pub struct KioskRow {
    pub kiosk: Kiosk,
    pub last_used_at: Option<DateTime>,
}

/// `GET /staff/api-tokens` (`api.kiosks`): the active store's kiosks and
/// their tokens, the form for a new one, and a new token once.
pub async fn kiosks(State(db): State<Db>, session: Session, lang: Lang) -> Result<View> {
    let store_id = crate::app::rentals::active_store()?;
    let store = Store::find_or_404(&db, store_id).await?;
    let kiosks = Kiosk::query()
        .where_(Kiosk::STORE_ID.eq(store_id))
        .order_by_desc(Kiosk::ID)
        .get(&db)
        .await?;
    let used: Vec<(i64, Option<DateTime>)> = renox::db::sql(
        "SELECT id, last_used_at FROM device_tokens WHERE device IN \
         (SELECT 'kiosk:' || CAST(id AS TEXT) FROM kiosks WHERE store_id = ?)",
    )
    .bind(store_id)
    .fetch_as::<(i64, Option<DateTime>)>(&db)
    .await?;
    let rows: Vec<KioskRow> = kiosks
        .into_iter()
        .map(|kiosk| KioskRow {
            last_used_at: used
                .iter()
                .find(|(id, _)| Some(*id) == kiosk.token_id)
                .and_then(|(_, at)| *at),
            kiosk,
        })
        .collect();
    let fresh: Option<String> = session.get(FLASH);
    Ok(view(
        "api/kiosks.html",
        context! {
            store,
            kiosks => rows,
            fresh,
            abilities => options(&lang, &KIOSK_ABILITIES),
            base => super::base_url(),
        },
    ))
}

// [explain:api.kiosks.store]
/// `POST /staff/api-tokens` (`api.kiosks.store`): a kiosk for the active
/// store and a device token owned by `kiosk:<id>` (no user account) with
/// the abilities ticked (no expiry: the manager revokes it), shown once.
pub async fn kiosk_store(
    State(db): State<Db>,
    user: AuthUser,
    lang: Lang,
    session: Session,
    Valid(form): Valid<KioskForm>,
) -> Result<(Toast, Redirect)> {
    let store_id = crate::app::rentals::active_store()?;
    if !access::can_in(&user, catalogue::FLEET_MANAGE, store_id) {
        return Err(Error::Forbidden);
    }
    let mut kiosk = Kiosk::create(
        &db,
        Kiosk {
            store_id,
            name: form.name.trim().to_owned(),
            abilities: DbJson(form.abilities.clone()),
            created_by: Some(user.id),
            ..Default::default()
        },
    )
    .await?;
    let abilities: Vec<&str> = form.abilities.iter().map(String::as_str).collect();
    let device = super::kiosk::device_key(kiosk.id);
    let token = DeviceToken::create(&db, &device, &kiosk.name, Some(&abilities), None).await?;
    kiosk.token_id = Some(token.token.id);
    kiosk.save_only(&db, &["token_id"]).await?;
    session.flash(FLASH, token.plain)?;
    Ok((
        Toast::success(lang.t("api.tokens.made", &[])),
        Redirect::route("api.kiosks", &[])?,
    ))
    // [/explain:api.kiosks.store]
}

/// `POST /staff/api-tokens/{kiosk}/revoke` (`api.kiosks.destroy`): the
/// kiosk's token stops working at once (a lost or stolen kiosk); the row
/// stays for the history. Only in the kiosk's own store (404 elsewhere).
pub async fn kiosk_destroy(
    State(db): State<Db>,
    user: AuthUser,
    lang: Lang,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut kiosk = Kiosk::find_or_404(&db, id).await?;
    if !access::can_in(&user, catalogue::FLEET_MANAGE, kiosk.store_id) {
        return Err(Error::NotFound);
    }
    DeviceToken::revoke_all(&db, &super::kiosk::device_key(kiosk.id)).await?;
    kiosk.token_id = None;
    kiosk.revoked_at = Some(renox::db::now());
    kiosk.save_only(&db, &["token_id", "revoked_at"]).await?;
    Ok((
        Toast::success(lang.t("api.tokens.revoked", &[])),
        Redirect::route("api.kiosks", &[])?,
    ))
}

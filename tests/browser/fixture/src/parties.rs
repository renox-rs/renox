//! Parties with custom fields, for `<rx-custom-fields>` (custom-fields.test.mjs):
//! `/parties/new` is a form with the model's fields and the custom ones,
//! `POST /parties` validates and stores them, `/parties` lists the parties with
//! their custom values and filters on the `segment` key (`?segment=retail`).
//! `tax_id` is a sensitive field: it is sealed in the row, shown masked, and
//! revealed to the viewer who holds the `custom_fields.reveal` ability (the
//! demo user, see main.rs).

use std::collections::HashMap;

use renox::custom_fields::{CustomField, CustomValues};
use renox::db::Json;
use renox::prelude::*;
use serde::Serialize;

const MODEL: &str = "fixture_parties";

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "fixture_parties", form, custom_fields = "extra")]
pub struct Party {
    pub id: i64,
    #[form(validate(required))]
    pub name: String,
    pub extra: Json<CustomValues>,
}

/// One list row: the name and the custom values as the viewer may see them.
#[derive(Serialize)]
struct Row {
    name: String,
    extra: CustomValues,
}

/// Defines the fields once (an administrator would do it in renox-admin).
async fn define_fields(db: &Db) -> Result<()> {
    if !CustomField::for_model(db, MODEL).await?.is_empty() {
        return Ok(());
    }
    CustomField::text(MODEL, "passport_no")
        .label("Passport no.")
        .required()
        .max(10)
        .save(db)
        .await?;
    CustomField::choice(MODEL, "segment")
        .label("Segment")
        .options(&["retail", "corporate"])
        .position(1)
        .save(db)
        .await?;
    CustomField::date(MODEL, "visa_expiry")
        .label("Visa expiry")
        .position(2)
        .save(db)
        .await?;
    CustomField::text(MODEL, "tax_id")
        .label("Tax id")
        .sensitive()
        .position(3)
        .save(db)
        .await?;
    Ok(())
}

pub struct Parties;

impl Module for Parties {
    fn name(&self) -> &'static str {
        "parties"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get(
                "/parties/new",
                |State(state): State<AppState>, viewer: Option<AuthUser>| async move {
                    define_fields(&state.db).await?;
                    let fields =
                        CustomField::views_for(&state.db, MODEL, "en", viewer.as_ref()).await?;
                    Ok::<_, Error>(view("parties_new.html", context! { fields }))
                },
            )
            .post(
                "/parties",
                |State(state): State<AppState>, Valid(form): Valid<PartyForm>| async move {
                    let mut party = Party::default();
                    form.fill(&mut party);
                    Party::create(&state.db, party).await?;
                    Ok::<_, Error>(Redirect::to("/parties"))
                },
            )
            .get(
                "/parties",
                |State(state): State<AppState>,
                 viewer: Option<AuthUser>,
                 Query(params): Query<HashMap<String, String>>| async move {
                    define_fields(&state.db).await?;
                    let fields =
                        CustomField::views_for(&state.db, MODEL, "en", viewer.as_ref()).await?;
                    let segment = params.get("segment").cloned().unwrap_or_default();
                    let mut query = Party::query();
                    if !segment.is_empty() {
                        query = query.where_(Party::EXTRA.json("segment").eq(segment.clone()));
                    }
                    let parties = query
                        .with_custom_fields(&state.db)
                        .await?
                        .order_by("id")
                        .get(&state.db)
                        .await?;
                    // A masked sensitive value never reaches the template.
                    let parties: Vec<_> = parties
                        .iter()
                        .map(|p| Row {
                            name: p.name.clone(),
                            extra: p.extra.shown(&state.db, &fields),
                        })
                        .collect();
                    Ok::<_, Error>(view("parties.html", context! { parties, fields, segment }))
                },
            )
    }
}

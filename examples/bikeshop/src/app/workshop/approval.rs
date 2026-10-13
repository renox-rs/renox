//! Extra work, approved or refused by the customer without logging in.
//!
//! The mechanic proposes tasks and parts with a price
//! ([`super::order::propose`]); the customer gets a mail whose button is a
//! **signed URL** (`state.signed_url`, HMAC-SHA256 with `APP_KEY`, valid
//! for [`LINK_TTL`]). The page and its two buttons check the signature
//! (`ValidSignature`): a link changed by hand, or an old one, answers 403.
//! The answer is recorded once: after it, the page shows the decision and a
//! second answer is refused (409).

use renox::axum::extract::OriginalUri;
use renox::prelude::*;
use renox::signed::ValidSignature;
use serde::Deserialize;
use std::time::Duration;

use super::model::{ExtraStatus, ExtraWork, WorkOrder, WorkOrderTask, WorkStatus};
use super::order::take_part;
use super::status;
use crate::app::rentals::notify::{self, Notice, Tone};
use crate::app::rentals::reserve::money;
use crate::app::staff::model::Store;

/// How long the customer has to answer.
pub const LINK_TTL: Duration = Duration::from_secs(72 * 60 * 60);

/// The signed link for `extra`, as mailed.
pub fn signed_link(state: &AppState, extra: &ExtraWork) -> Result<String> {
    state.signed_url("workshop.extra.show", &[&extra.id], LINK_TTL)
}

/// `GET /service/approve/{extra}` (`workshop.extra.show`): the proposal,
/// its items and price, and "Approve" / "Refuse" (or the answer already
/// given). No login: the signature is the proof.
// [explain:workshop.extra.show.handler]
pub async fn show(
    _: ValidSignature,
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<i64>,
) -> Result<View> {
    let extra = ExtraWork::find_or_404(&state.db, id).await?;
    let order = WorkOrder::find_or_404(&state.db, extra.work_order_id).await?;
    let store = Store::find(&state.db, order.store_id).await?;
    let bike = match order.customer_bike_id {
        Some(id) => super::model::CustomerBike::find(&state.db, id).await?,
        None => None,
    };
    Ok(view(
        "workshop/approve.html",
        context! {
                    // The buttons post to this same signed address.
                    action => uri.to_string(),
                    pending => extra.status == ExtraStatus::Pending,
        // [/explain:workshop.extra.show.handler]
                    status => match extra.status {
                        ExtraStatus::Pending => "pending",
                        ExtraStatus::Approved => "approved",
                        ExtraStatus::Refused => "refused",
                    },
                    store,
                    bike,
                    order,
                    extra,
                },
    ))
}

/// The customer's answer.
#[derive(Deserialize, Validate)]
pub struct Decision {
    #[validate(required, one_of(&["approve", "refuse"]))]
    pub decision: String,
}

/// `POST /service/approve/{extra}` (`workshop.extra.decide`): records the
/// answer once. Approved: the tasks join the work order and the parts are
/// taken from stock (or waited for); either way the work goes on and the
/// store's mechanics are told.
// [explain:workshop.extra.show.decide]
pub async fn decide(
    _: ValidSignature,
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<i64>,
    Valid(form): Valid<Decision>,
) -> Result<Response> {
    let db = &state.db;
    let lang = state.current_lang();
    let mut extra = ExtraWork::find_or_404(db, id).await?;
    if extra.status != ExtraStatus::Pending || extra.expires_at < renox::db::now() {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("workshop.approve.already", &[]),
        ));
    }
    let approve = form.decision == "approve";
    let mut order = WorkOrder::find_or_404(db, extra.work_order_id).await?;
    extra.status = if approve {
        ExtraStatus::Approved
    } else {
        ExtraStatus::Refused
    };
    extra.decided_at = Some(renox::db::now());
    extra.save_only(db, &["status", "decided_at"]).await?;
    // [/explain:workshop.extra.show.decide]
    if approve {
        for item in extra.items.iter() {
            if item.kind == "task" {
                WorkOrderTask::create(
                    db,
                    WorkOrderTask {
                        work_order_id: order.id,
                        service_task_id: item.id,
                        minutes: 0,
                        price: item.price,
                        note: Some(extra.description.clone()),
                        ..Default::default()
                    },
                )
                .await?;
            }
        }
        for item in extra.items.iter().filter(|i| i.kind == "part") {
            take_part(&state, &mut order, item.id, item.quantity, None).await?;
        }
        status::recompute(db, &mut order).await?;
    }
    if order.status == WorkStatus::WaitingApproval {
        status::set_status(&state, &mut order, WorkStatus::InProgress).await?;
    }
    let url = crate::app::rentals::link(&state, "workshop.order", Some(order.id))?;
    notify::staff(
        &state,
        crate::app::access::catalogue::WORKORDERS_UPDATE,
        &[order.store_id],
        &Notice::new(
            "workshop-extra-answered",
            if approve {
                "workshop.mail.extra_approved.title"
            } else {
                "workshop.mail.extra_refused.title"
            },
            "workshop.mail.extra_answered.body",
        )
        .param("number", order.id)
        .param("total", money(&state, extra.total))
        .tone(if approve {
            Tone::Success
        } else {
            Tone::Warning
        })
        .url(url),
    )
    .await?;
    Ok((
        Toast::success(lang.t(
            if approve {
                "workshop.approve.thanks_yes"
            } else {
                "workshop.approve.thanks_no"
            },
            &[],
        )),
        Redirect::to(&uri.to_string()),
    )
        .into_response())
}

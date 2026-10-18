//! A rental's receipt as a PDF (#30): `resources/views/pdf/rental-receipt.typ`
//! rendered by `renox-pdf`, then sent two ways from the same bytes: a
//! download for staff ([`download`]) and an attachment on the mail the
//! customer gets when the bike is back ([`super::counter::take_back`]).
//!
//! The texts are written in the **customer's** language (the one saved on
//! their account, else the language of whoever asks), so a Spanish customer
//! gets Spanish labels in the mail whatever the cashier's language is. The
//! template receives them as `data.labels`; amounts stay integers in cents
//! and the kit's `money()` writes them in that language.

use renox::Download;
use renox::prelude::*;
use renox::serde_json::json;
use renox_pdf::Pdf;
use serde::Serialize;

use super::booking::to_local;
use super::counter::counter_rental;
use super::model::{DepositStatus, Rental, RentalRow, RentalStatus};
use super::pricing::settle;
use crate::app::accounts::locale;
use crate::app::accounts::model::Customer;
use crate::app::sales::model::{PAYABLE_RENTAL, Payment, PaymentMethod, PaymentStatus};

/// The rendered receipt: the file name and the PDF.
#[derive(Debug, Clone)]
pub struct Receipt {
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// One line of a table: a label and an amount in cents.
#[derive(Serialize)]
struct Line {
    label: String,
    amount: i64,
}

/// One payment, as text except for its amount.
#[derive(Serialize)]
struct Paid {
    when: String,
    method: String,
    amount: i64,
}

/// What the template reads as `data`.
#[derive(Serialize)]
struct Data {
    labels: renox::serde_json::Value,
    code: String,
    store: String,
    customer: String,
    bike: String,
    returned: String,
    currency: String,
    lines: Vec<Line>,
    total: i64,
    deposit: Vec<Line>,
    payments: Vec<Paid>,
}

/// The language the receipt is written in: the customer's account language,
/// else the current one.
pub async fn locale_for(state: &AppState, customer: Option<&Customer>) -> Result<String> {
    if let Some(user_id) = customer.and_then(|c| c.user_id)
        && let Some(user) = User::find(&state.db, user_id).await?
        && let Some(own) = locale::of(&user)
    {
        return Ok(own);
    }
    Ok(state.current_lang().locale)
}

/// Renders the receipt of `rental`, which must be returned (settled). `None`
/// for a rental that is not: there is nothing to put on a receipt yet.
pub async fn render(state: &AppState, rental: &Rental) -> Result<Option<Receipt>> {
    if rental.status != RentalStatus::Returned {
        return Ok(None);
    }
    let db = &state.db;
    let payments = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(PAYABLE_RENTAL))
        .where_(Payment::PAYABLE_ID.eq(rental.id))
        .where_(Payment::STATUS.eq(PaymentStatus::Paid))
        .order_by(Payment::ID)
        .get(db)
        .await?;
    let row = RentalRow::load(db, vec![rental.clone()])
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    let locale = locale_for(state, row.customer.as_ref()).await?;
    let lang = state.lang(&locale);
    let t = |key: &str| lang.t(key, &[]);
    let local = |at| to_local(&state.config, at);

    let mut lines = vec![Line {
        label: t("rentals.receipt.pdf.rental"),
        amount: rental.price,
    }];
    if rental.late_fee > 0 {
        lines.push(Line {
            label: t("rentals.fields.late_fee"),
            amount: rental.late_fee,
        });
    }
    if rental.damage_fee > 0 {
        lines.push(Line {
            label: t("rentals.fields.damage_fee"),
            amount: rental.damage_fee,
        });
    }
    let settlement = settle(
        if matches!(rental.deposit_status, DepositStatus::Settled) {
            rental.deposit
        } else {
            0
        },
        rental.fees(),
    );
    let deposit = vec![
        Line {
            label: t("rentals.fields.deposit"),
            amount: rental.deposit,
        },
        Line {
            label: t("rentals.receipt.kept"),
            amount: settlement.kept,
        },
        Line {
            label: t("rentals.fields.refund"),
            amount: rental.deposit_refunded,
        },
    ];
    let frame = row
        .bike
        .as_ref()
        .map(|b| b.frame_number.clone())
        .unwrap_or_default();
    let data = Data {
        labels: json!({
            "title": t("rentals.receipt.title"),
            "code": t("rentals.fields.code"),
            "returned": t("rentals.receipt.pdf.returned"),
            "customer": t("rentals.fields.customer"),
            "bike": t("rentals.fields.bike"),
            "item": t("rentals.receipt.pdf.item"),
            "amount": t("rentals.receipt.amount"),
            "total": t("rentals.receipt.total"),
            "deposit_title": t("rentals.fields.deposit"),
            "payments": t("rentals.receipt.payments"),
            "when": t("rentals.receipt.when"),
            "method": t("rentals.desk.method"),
            "thanks": t("rentals.receipt.pdf.thanks"),
        }),
        code: rental.reservation_code.to_string(),
        store: row
            .operating_store
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        customer: row
            .customer
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default(),
        bike: match &row.model {
            Some(model) => format!("{model} ({frame})"),
            None => frame,
        },
        returned: local(rental.returned_at.unwrap_or(rental.due_at))
            .format("%Y-%m-%d")
            .to_string(),
        currency: state.config.currency.clone(),
        lines,
        total: rental.total(),
        deposit,
        payments: payments
            .iter()
            .map(|p| Paid {
                when: p
                    .paid_at
                    .map(|at| local(at).format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default(),
                method: t(match p.method {
                    PaymentMethod::Cash => "rentals.methods.cash",
                    PaymentMethod::Card => "rentals.methods.card",
                    PaymentMethod::Gateway => "rentals.methods.gateway",
                }),
                amount: p.amount,
            })
            .collect(),
    };
    let bytes = Pdf::document(state, "rental-receipt")?
        .locale(&locale)
        .data(&data)
        .render(state)
        .await?;
    Ok(Some(Receipt {
        filename: format!("receipt-{}.pdf", rental.reservation_code),
        bytes,
    }))
}

/// `GET /staff/rentals/{rental}/receipt.pdf` (`rentals.receipt.pdf`): the
/// receipt as a file, for whoever may see the rental in their store (the same
/// check as the receipt page). A rental that is not returned yet has none: 404.
pub async fn download(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Download> {
    let rental = counter_rental(&state.db, &user, id).await?;
    let receipt = render(&state, &rental).await?.ok_or(Error::NotFound)?;
    Ok(Download::bytes(
        receipt.filename,
        "application/pdf",
        receipt.bytes,
    ))
}

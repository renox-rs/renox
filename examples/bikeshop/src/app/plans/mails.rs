//! `GET /plans/mails` (`plans.mails`): every mail the plans send, for a
//! made-up subscriber, so a reader sees them without paying (and the
//! "About this page" index lists them with the renox-billing events that
//! send them).
//!
//! Each goes through the same view and layout as the real one
//! (`mail/plans/notice`, the rentals' notice layout on the kit's mail
//! components), in the visitor's language, shown in a sandboxed frame.

use renox::prelude::*;
use serde::Serialize;

/// One mail's preview.
#[derive(Serialize, Debug, Clone)]
pub struct Preview {
    pub key: &'static str,
    /// What sends it.
    pub event: &'static str,
    pub subject: String,
    pub html: String,
}

/// The plan mails: (key, the event that sends it, title key, body key).
pub const MAILS: [(&str, &str, &str, &str); 5] = [
    (
        "started",
        "renox_billing::PaymentSucceeded (the first)",
        "plans.mail.started.title",
        "plans.mail.started.body",
    ),
    (
        "renewed",
        "renox_billing::PaymentSucceeded",
        "plans.mail.renewed.title",
        "plans.mail.renewed.body",
    ),
    (
        "failed",
        "renox_billing::PaymentFailed",
        "plans.mail.failed.title",
        "plans.mail.failed.body",
    ),
    (
        "visit",
        "plans:visits (a visit booked)",
        "plans.mail.visit.title",
        "plans.mail.visit.body",
    ),
    (
        "missed",
        "plans:visits (a visit missed)",
        "plans.mail.missed.title",
        "plans.mail.missed.body",
    ),
];

#[derive(Serialize)]
struct Row {
    label: String,
    value: String,
}

/// `GET /plans/mails` (`plans.mails`).
pub async fn index(State(state): State<AppState>, lang: Lang) -> Result<View> {
    let amount = crate::app::rentals::reserve::money(&state, 6_000);
    let params: [(&str, &dyn std::fmt::Display); 5] = [
        ("plan", &"Monthly tune-up"),
        ("bike", &"Blue roadie"),
        ("amount", &amount),
        ("day", &"2026-10-14"),
        ("number", &1042),
    ];
    // [explain:plans.mails.handler]
    let mut previews = Vec::new();
    for (key, event, title, body) in MAILS {
        let subject = lang.t(title, &params);
        // [/explain:plans.mails.handler]
        let rows = vec![
            Row {
                label: lang.t("plans.fields.amount", &[]),
                value: amount.clone(),
            },
            Row {
                label: lang.t("plans.fields.next_visit", &[]),
                value: "2026-10-14".into(),
            },
        ];
        // [explain:plans.mails.handler]
        let mail = state.mail_view_in(
            &lang.locale,
            "customer@example.com",
            subject.clone(),
            "mail/plans/notice",
            context! {
                title => subject.clone(),
                body => lang.t(body, &params),
                rows,
                url => "https://bikeshop.example/plans/mine/1",
                action => lang.t("rentals.mail.open", &[]),
            },
        )?;
        previews.push(Preview {
            key,
            event,
            subject,
            html: mail.html.unwrap_or(mail.text),
        });
    }
    Ok(view("plans/mails.html", context! { previews }))
    // [/explain:plans.mails.handler]
}

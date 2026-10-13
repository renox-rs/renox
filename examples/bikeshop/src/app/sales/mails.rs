//! `GET /sales/mails` (`sales.mails`): every mail the sales area sends,
//! rendered for a made-up order, so a reader can see them without placing
//! an order (and the "About this page" index lists them).
//!
//! Each preview goes through the same `mail_view_in` the real mails use
//! (the kit's mail layout and components, the visitor's language); its HTML
//! is shown in a sandboxed frame.

use renox::prelude::*;
use serde::Serialize;

use super::model::{Channel, Fulfilment, Order, OrderItem, OrderStatus};
use super::notify::{LineView, Moment, OrderView};
use crate::app::accounts::model::Customer;
use crate::app::staff::model::Store;

/// One mail's preview.
#[derive(Serialize, Debug, Clone)]
pub struct Preview {
    pub key: &'static str,
    pub subject: String,
    pub html: String,
}

/// A made-up delivery order: two lines, a discount, a fee.
fn sample() -> OrderView {
    let line = |id: i64, name: &str, variant: &str, quantity: i64, price: i64| LineView {
        item: OrderItem {
            id,
            quantity,
            unit_price: price,
            total: price * quantity,
            ..Default::default()
        },
        name: name.into(),
        sku: format!("DEMO-{id}"),
        variant: variant.into(),
        slug: String::new(),
    };
    OrderView {
        order: Order {
            id: 0,
            number: "N-DEMO0001".into(),
            channel: Channel::Online,
            fulfilment: Fulfilment::Delivery,
            status: OrderStatus::Paid,
            subtotal: 113_998,
            discount: 400,
            delivery_fee: 1_000,
            total: 114_598,
            placed_at: Some(renox::db::now()),
            ..Default::default()
        },
        lines: vec![
            line(1, "Trek FX 3", "M · Grey", 1, 109_999),
            line(2, "Shimano chain", "", 1, 3_999),
        ],
        store: Some(Store {
            name: "North".into(),
            phone: "+62 21 555 0101".into(),
            ..Default::default()
        }),
        customer: Some(Customer {
            name: "Sofia Wijaya".into(),
            ..Default::default()
        }),
        address: Some("Jalan Kemang Raya 5, Jakarta, Indonesia".into()),
        delivery: true,
    }
}

// [explain:sales.mails.handler]
/// `GET /sales/mails` (`sales.mails`).
pub async fn index(State(state): State<AppState>, lang: Lang) -> Result<View> {
    let order = sample();
    let mut previews = Vec::new();
    for moment in [
        Moment::Paid,
        Moment::Ready,
        Moment::Shipped,
        Moment::Refunded,
        Moment::Expired,
    ] {
        let subject = lang.t(
            &format!("sales.mail.{}.subject", moment.key()),
            &[("number", &order.order.number)],
        );
        let mail = state.mail_view_in(
            &lang.locale,
            "customer@example.com",
            subject.clone(),
            &format!("mail/sales/{}", moment.key()),
            context! {
                table => order.mail_table(&lang, &state.config.currency),
                order => order.clone(),
                url => "https://bikeshop.example/orders/1/view?signature=…",
                refund => Some(3_999),
                name => "Sofia Wijaya",
            },
        )?;
        previews.push(Preview {
            key: moment.key(),
            subject,
            html: mail.html.unwrap_or(mail.text),
        });
    }
    Ok(view("sales/mails.html", context! { previews }))
}
// [/explain:sales.mails.handler]

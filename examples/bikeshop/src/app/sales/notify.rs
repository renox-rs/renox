//! Telling the customer: queued mails on the kit's mail layout and, for
//! customers with an account, an in-app notification.
//!
//! | Moment | Mail (`resources/views/mail/sales/…`) | Notification |
//! |---|---|---|
//! | paid | `confirmation`: the lines, the totals, pickup or delivery | "Order N-… is paid" |
//! | ready for pickup | `ready` | "Ready for pickup" |
//! | sent out | `shipped` | "On its way" |
//! | returned and refunded | `refund` | "Refunded" |
//! | not paid in 30 minutes | `expired` | "Cancelled" |
//!
//! Mails are written in the language the customer ordered in
//! (`orders.locale`, `mail_view_in`), even when the queue or a member of
//! staff sends them later. The link in each mail is a signed URL to the
//! order (`orders.signed`), so a guest can open it without an account.
//!
//! Both follow the customer's choice for orders on their account
//! (`accounts::preferences`, `Kind::Order`): "mail only" sends no
//! notification, "in the app" no mail, "none" neither.

use std::time::Duration;

use renox::auth::{Channel, DatabaseMessage, Notification, Recipient};
use renox::prelude::*;
use serde::Serialize;

use super::model::{Fulfilment, Order, OrderItem};
use crate::app::accounts::model::{Customer, FullAddress};
use crate::app::accounts::preferences::{Kind, channels_for};
use crate::app::catalog::model::{Product, ProductVariant};
use crate::app::staff::model::Store;

/// A signed link to the order works this long (the return window and a bit).
pub const LINK_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 30);

/// What a mail or a page shows of an order.
#[derive(Serialize, Debug, Clone)]
pub struct OrderView {
    #[serde(flatten)]
    pub order: Order,
    pub lines: Vec<LineView>,
    pub store: Option<Store>,
    pub customer: Option<Customer>,
    /// The delivery address in one line.
    pub address: Option<String>,
    pub delivery: bool,
}

/// A line with its product.
#[derive(Serialize, Debug, Clone)]
pub struct LineView {
    #[serde(flatten)]
    pub item: OrderItem,
    pub name: String,
    pub sku: String,
    pub variant: String,
    pub slug: String,
}

impl OrderView {
    /// The order with its lines, products, store, customer and address:
    /// seven queries however many lines.
    pub async fn load(db: &Db, order: Order) -> Result<OrderView> {
        let items = OrderItem::query()
            .where_(OrderItem::ORDER_ID.eq(order.id))
            .order_by(OrderItem::ID)
            .get(db)
            .await?;
        let variants =
            ProductVariant::find_many(db, items.iter().map(|i| i.variant_id).collect::<Vec<_>>())
                .await?;
        // Discontinued products still name the lines of old orders.
        let products = Product::query()
            .with_trashed()
            .where_(Product::ID.is_in(variants.iter().map(|v| v.product_id).collect::<Vec<_>>()))
            .get(db)
            .await?;
        let lines = items
            .into_iter()
            .map(|item| {
                let variant = variants.iter().find(|v| v.id == item.variant_id);
                let product = variant.and_then(|v| products.iter().find(|p| p.id == v.product_id));
                LineView {
                    name: product.map(|p| p.name.clone()).unwrap_or_default(),
                    slug: product.map(|p| p.slug.clone()).unwrap_or_default(),
                    sku: variant.map(|v| v.sku.clone()).unwrap_or_default(),
                    variant: variant
                        .map(|v| {
                            [v.size.clone(), v.colour.clone()]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join(" · ")
                        })
                        .unwrap_or_default(),
                    item,
                }
            })
            .collect();
        let store = Store::find(db, order.operating_store_id).await?;
        let customer = match order.customer_id {
            Some(id) => {
                Customer::query()
                    .with_trashed()
                    .where_(Customer::ID.eq(id))
                    .first(db)
                    .await?
            }
            None => None,
        };
        let address = match order.delivery_address_id {
            Some(id) => FullAddress::load(db, vec![id])
                .await?
                .remove(&id)
                .map(|a| a.line()),
            None => None,
        };
        Ok(OrderView {
            delivery: order.fulfilment == Fulfilment::Delivery,
            order,
            lines,
            store,
            customer,
            address,
        })
    }
}

impl OrderView {
    /// The order's lines and totals as the data of the mail kit's table
    /// (`rows`, `head`, `total`), every amount written in `lang`'s locale
    /// as the `money` filter does.
    pub fn mail_table(&self, lang: &Lang, currency: &str) -> renox::serde_json::Value {
        let money = |amount: i64| crate::money::format(amount, currency, &lang.locale);
        let mut rows: Vec<[String; 2]> = self
            .lines
            .iter()
            .map(|line| {
                let variant = if line.variant.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", line.variant)
                };
                [
                    format!("{} × {}{}", line.item.quantity, line.name, variant),
                    money(line.item.total),
                ]
            })
            .collect();
        if self.order.discount > 0 {
            rows.push([
                lang.t("sales.orders.discount", &[]),
                format!("− {}", money(self.order.discount)),
            ]);
        }
        if self.delivery {
            rows.push([
                lang.t("sales.checkout.delivery_fee", &[]),
                money(self.order.delivery_fee),
            ]);
        }
        renox::serde_json::json!({
            "rows": rows,
            "head": [lang.t("sales.orders.product", &[]), lang.t("sales.orders.line_total", &[])],
            "total": [lang.t("sales.orders.total", &[]), money(self.order.total)],
        })
    }
}

/// The moments a customer hears about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    Paid,
    Ready,
    Shipped,
    Refunded,
    Expired,
}

impl Moment {
    /// The mail's view and the translation keys' middle part.
    pub fn key(self) -> &'static str {
        match self {
            Moment::Paid => "confirmation",
            Moment::Ready => "ready",
            Moment::Shipped => "shipped",
            Moment::Refunded => "refund",
            Moment::Expired => "expired",
        }
    }
}

/// Queues the mail for `moment` (when the customer gave an address) and
/// notifies their account (when they have one), as they chose for orders.
/// `refund`: the amount paid back, for the refund mail.
// [explain:sales.mails.tell]
pub async fn tell(state: &AppState, order: &Order, moment: Moment, refund: Option<i64>) -> Result {
    if order.customer_id.is_none() {
        return Ok(()); // a walk-in at the counter
    }
    let view = OrderView::load(&state.db, order.clone()).await?;
    let Some(customer) = view.customer.clone() else {
        return Ok(());
    };
    let locale = order
        .locale
        .clone()
        .unwrap_or_else(|| state.config.locale.clone());
    let lang = state.lang(&locale);
    let number = order.number.clone();
    let url = state.signed_url("orders.signed", &[&order.id], LINK_TTL)?;
    // [/explain:sales.mails.tell]
    let subject = lang.t(
        &format!("sales.mail.{}.subject", moment.key()),
        &[("number", &number)],
    );
    let user = match customer.user_id {
        Some(id) => User::find(&state.db, id).await?,
        None => None,
    };
    let email = customer.email.as_deref().filter(|e| !e.is_empty());
    let to = match (&user, email) {
        (Some(user), _) => Recipient::for_user(user),
        (None, Some(email)) => Recipient::to("mail", email),
        (None, None) => return Ok(()),
    };
    // [explain:sales.mails.tell]
    let channels = channels_for(&to, Kind::Order);
    if let Some(email) = email
        && channels.contains(&Channel::Mail)
    {
        let mail = state.mail_view_in(
            &locale,
            email,
            subject.clone(),
            &format!("mail/sales/{}", moment.key()),
            context! {
                table => view.mail_table(&lang, &state.config.currency),
                order => view,
                url,
                refund,
                name => customer.name.clone(),
            },
        )?;
        state.queue_mail(mail).await?;
    }
    // [/explain:sales.mails.tell]
    if let Some(user) = &user {
        let body = lang.t(
            &format!("sales.notice.{}", moment.key()),
            &[("number", &number)],
        );
        state
            .notify(
                user,
                &OrderNotice {
                    moment,
                    order_id: order.id,
                    title: subject,
                    body,
                },
            )
            .await?;
    }
    Ok(())
}

/// The in-app notification about an order (a `DatabaseMessage`, shown by
/// the kit's bell and notification list).
pub struct OrderNotice {
    pub moment: Moment,
    pub order_id: i64,
    pub title: String,
    pub body: String,
}

impl Notification for OrderNotice {
    fn kind(&self) -> &'static str {
        match self.moment {
            Moment::Paid => "order-paid",
            Moment::Ready => "order-ready",
            Moment::Shipped => "order-shipped",
            Moment::Refunded => "order-refunded",
            Moment::Expired => "order-expired",
        }
    }

    // The bell, when the customer wants orders in the app. The mail is
    // queued by `tell` itself (in the language the order was placed in),
    // after the same question.
    fn channels(&self, to: &Recipient) -> Vec<Channel> {
        channels_for(to, Kind::Order)
            .into_iter()
            .filter(|c| *c == Channel::Database)
            .collect()
    }

    fn to_database(&self, _: &Recipient, _state: &AppState) -> Result<renox::serde_json::Value> {
        let message = match self.moment {
            Moment::Expired => DatabaseMessage::warning(&self.title),
            Moment::Refunded => DatabaseMessage::info(&self.title),
            _ => DatabaseMessage::success(&self.title),
        };
        Ok(message
            .body(&self.body)
            .url(format!("/orders/{}", self.order_id))
            .with("order_id", self.order_id)
            .into())
    }
}

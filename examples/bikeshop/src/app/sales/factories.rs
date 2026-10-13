//! Factories for orders and payments: `orders().at(store).paid()`,
//! `payments().for_rental(&rental).paid()`…

use renox::chrono::Duration;
use renox::db::FactoryBuilder;
use renox::prelude::*;

use super::model::{
    Channel, Fulfilment, Order, OrderItem, OrderStatus, Payment, PaymentMethod, PaymentStatus,
};
use crate::app::rentals::model::Rental;
use crate::seed::unique;

impl Factory for Order {
    fn definition() -> Self {
        Order {
            number: format!("N-{:06}", unique()),
            channel: Channel::Online,
            fulfilment: Fulfilment::Pickup,
            status: OrderStatus::Pending,
            placed_at: Some(renox::db::now()),
            ..Default::default()
        }
    }
}

/// `Order::factory()`.
pub fn orders() -> FactoryBuilder<Order> {
    Order::factory()
}

/// States of an order.
pub trait OrderStates {
    /// Sold by `store_id`.
    fn at(self, store_id: i64) -> Self;
    /// For `customer_id`.
    fn for_customer(self, customer_id: i64) -> Self;
    /// A counter sale.
    fn counter(self) -> Self;
    /// Totalling `amount` (no discount, no delivery).
    fn totalling(self, amount: i64) -> Self;
    /// Paid an hour ago.
    fn paid(self) -> Self;
    /// In the customer's hands.
    fn completed(self) -> Self;
    /// Paid back.
    fn refunded(self) -> Self;
}

impl OrderStates for FactoryBuilder<Order> {
    fn at(self, store_id: i64) -> Self {
        self.state(move |o| o.operating_store_id = store_id)
    }

    fn for_customer(self, customer_id: i64) -> Self {
        self.state(move |o| o.customer_id = Some(customer_id))
    }

    fn counter(self) -> Self {
        self.state(|o| o.channel = Channel::Counter)
    }

    fn totalling(self, amount: i64) -> Self {
        self.state(move |o| {
            o.subtotal = amount;
            o.total = amount;
        })
    }

    fn paid(self) -> Self {
        self.state(|o| {
            o.status = OrderStatus::Paid;
            o.paid_at = Some(renox::db::now() - Duration::hours(1));
        })
    }

    fn completed(self) -> Self {
        self.state(|o| {
            o.status = OrderStatus::Completed;
            o.paid_at
                .get_or_insert(renox::db::now() - Duration::days(1));
            o.completed_at.get_or_insert(renox::db::now());
        })
    }

    fn refunded(self) -> Self {
        self.state(|o| {
            o.status = OrderStatus::Refunded;
            o.paid_at
                .get_or_insert(renox::db::now() - Duration::days(3));
        })
    }
}

impl Factory for OrderItem {
    fn definition() -> Self {
        OrderItem {
            quantity: 1,
            ..Default::default()
        }
    }
}

impl Factory for Payment {
    fn definition() -> Self {
        Payment {
            method: PaymentMethod::Card,
            status: PaymentStatus::Pending,
            ..Default::default()
        }
    }
}

/// `Payment::factory()`.
pub fn payments() -> FactoryBuilder<Payment> {
    Payment::factory()
}

/// States of a payment.
pub trait PaymentStates {
    /// Paying `order` in full, at its store.
    fn for_order(self, order: &Order) -> Self;
    /// Paying `rental` (price and fees), at its operating store.
    fn for_rental(self, rental: &Rental) -> Self;
    /// Paid now.
    fn paid(self) -> Self;
}

impl PaymentStates for FactoryBuilder<Payment> {
    fn for_order(self, order: &Order) -> Self {
        let (id, store, customer, total) = (
            order.id,
            order.operating_store_id,
            order.customer_id,
            order.total,
        );
        self.state(move |p| {
            p.payable_type = Order::TABLE.into();
            p.payable_id = id;
            p.store_id = store;
            p.customer_id = customer;
            p.amount = total;
        })
    }

    fn for_rental(self, rental: &Rental) -> Self {
        let (id, store, customer, total) = (
            rental.id,
            rental.operating_store_id,
            rental.customer_id,
            rental.total(),
        );
        self.state(move |p| {
            p.payable_type = Rental::TABLE.into();
            p.payable_id = id;
            p.store_id = store;
            p.customer_id = Some(customer);
            p.amount = total;
        })
    }

    fn paid(self) -> Self {
        self.state(|p| {
            p.status = PaymentStatus::Paid;
            p.paid_at = Some(renox::db::now());
        })
    }
}

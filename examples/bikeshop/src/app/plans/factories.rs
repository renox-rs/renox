//! Factories for service plans and subscriptions.

use renox::chrono::{Datelike, Duration};
use renox::db::FactoryBuilder;
use renox::prelude::*;

use super::model::{Frequency, PlanSubscription, ServicePlan, SubscriptionStatus};
use crate::seed::{today, unique};

impl Factory for ServicePlan {
    fn definition() -> Self {
        let n = unique();
        ServicePlan {
            name: format!("Plan {n}"),
            slug: format!("plan-{n}"),
            frequency: Frequency::Monthly,
            price: 6_000,
            description: "Regular care for your bike.".into(),
            active: true,
            parts_discount_bp: 1_000,
            ..Default::default()
        }
    }
}

impl Factory for PlanSubscription {
    fn definition() -> Self {
        let start = today() - Duration::days(60);
        PlanSubscription {
            preferred_weekday: start.weekday().number_from_monday() as i64,
            status: SubscriptionStatus::Active,
            starts_on: start,
            next_visit_on: Some(today() + Duration::days(7)),
            ..Default::default()
        }
    }
}

/// `PlanSubscription::factory()`.
pub fn plan_subscriptions() -> FactoryBuilder<PlanSubscription> {
    PlanSubscription::factory()
}

/// States of a subscription.
pub trait SubscriptionStates {
    /// `customer_bike_id` on `plan_id`, serviced at `store_id`.
    fn of(self, customer_bike_id: i64, plan_id: i64, store_id: i64) -> Self;
    /// Its next visit is tomorrow.
    fn due_soon(self) -> Self;
    /// Cancelled last week.
    fn cancelled(self) -> Self;
    /// Paid online by `user_id` through `gateway` (`stripe`, `demo`…).
    fn online(self, user_id: i64, gateway: &str) -> Self;
    /// Paused by the customer.
    fn paused(self) -> Self;
}

impl SubscriptionStates for FactoryBuilder<PlanSubscription> {
    fn of(self, customer_bike_id: i64, plan_id: i64, store_id: i64) -> Self {
        self.state(move |s| {
            s.customer_bike_id = customer_bike_id;
            s.service_plan_id = plan_id;
            s.store_id = store_id;
        })
    }

    fn due_soon(self) -> Self {
        self.state(|s| s.next_visit_on = Some(today() + Duration::days(1)))
    }

    fn cancelled(self) -> Self {
        self.state(|s| {
            s.status = SubscriptionStatus::Cancelled;
            s.next_visit_on = None;
            s.cancelled_at = Some(renox::db::now() - Duration::days(7));
        })
    }

    fn online(self, user_id: i64, gateway: &str) -> Self {
        let gateway = gateway.to_owned();
        self.state(move |s| {
            s.user_id = Some(user_id);
            s.gateway = gateway.clone();
        })
    }

    fn paused(self) -> Self {
        self.state(|s| {
            s.status = SubscriptionStatus::Paused;
            s.paused_at = Some(renox::db::now());
        })
    }
}

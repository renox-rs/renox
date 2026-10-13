//! The shop's history, ending now: rentals bike by bike, orders, stock
//! (purchases, consignment between stores, sales, the ledger that adds up
//! to the stock levels), the workshop, and the books between stores with
//! their monthly settlements. Plus what is happening right now: bikes out,
//! rentals due today and overdue, work orders on the bench, low stock,
//! goods on consignment, unsettled balances.

use renox::chrono::{Datelike, Duration, NaiveDate};
use renox::db::{Db, Transaction, Ulid, sql};
use renox::prelude::*;
use std::collections::HashMap;

use super::shop::{Item, World};
use super::today;
use crate::app::access::catalogue::MANAGER;
use crate::app::catalog::model::CategoryKind;
use crate::app::multistore::model::{EntryKind, IntercompanyEntry, Settlement, SettlementStatus};
use crate::app::plans::model::{PlanSubscription, SubscriptionStatus};
use crate::app::rentals::model::{BikeStatus, Rental, RentalRate, RentalStatus};
use crate::app::sales::model::{
    Channel, Fulfilment, Order, OrderItem, OrderStatus, Payment, PaymentMethod, PaymentStatus,
};
use crate::app::staff::model::fee;
use crate::app::stock::model::{
    ConsignmentShipment, ConsignmentShipmentLine, MovementReason, PurchaseOrder, PurchaseOrderLine,
    PurchaseStatus, ShipmentStatus, StockLevel, StockMovement,
};
use crate::app::workshop::model::{CustomerBike, WorkOrder, WorkOrderTask, WorkSource, WorkStatus};

/// (variant, owner store, location store).
type Key = (i64, i64, i64);

/// What the history steps collect before writing.
#[derive(Default)]
struct Books {
    entries: Vec<IntercompanyEntry>,
    payments: Vec<Payment>,
    movements: Vec<StockMovement>,
    /// Units put aside for open orders, per key.
    reserved: HashMap<Key, i64>,
}

impl Books {
    /// `debtor` owes `creditor` `amount` for `kind`, booked at `at`.
    #[allow(clippy::too_many_arguments)]
    fn owe(
        &mut self,
        debtor: i64,
        creditor: i64,
        amount: i64,
        kind: EntryKind,
        rate: Option<i64>,
        source: (&str, i64),
        at: DateTime,
    ) {
        if amount <= 0 || debtor == creditor {
            return;
        }
        self.entries.push(IntercompanyEntry {
            debtor_store_id: debtor,
            creditor_store_id: creditor,
            amount,
            kind,
            fee_rate_bp: rate,
            source_type: source.0.into(),
            source_id: source.1,
            booked_at: at,
            created_at: Some(at),
            ..Default::default()
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn move_stock(
        &mut self,
        key: Key,
        quantity: i64,
        reason: MovementReason,
        reference: Option<(&str, i64)>,
        staff: Option<i64>,
        at: DateTime,
        note: Option<&str>,
    ) {
        self.movements.push(StockMovement {
            variant_id: key.0,
            owner_store_id: key.1,
            location_store_id: key.2,
            quantity,
            reason,
            reference_type: reference.map(|r| r.0.to_owned()),
            reference_id: reference.map(|r| r.1),
            staff_id: staff,
            note: note.map(str::to_owned),
            created_at: Some(at),
            ..Default::default()
        });
    }
}

/// Writes the whole history.
pub async fn build(db: &Db, world: &mut World) -> Result {
    let mut books = Books::default();
    let mut tx = db.begin().await?;
    let consigned = consignments(&mut tx, world, &mut books).await?;
    rentals(&mut tx, world, &mut books).await?;
    orders(&mut tx, world, &mut books, consigned).await?;
    purchases(&mut tx, world, &mut books).await?;
    workshop(&mut tx, world, &mut books).await?;
    stock(&mut tx, world, &mut books).await?;
    Payment::insert_many(&mut tx, std::mem::take(&mut books.payments)).await?;
    tx.commit().await?;
    let mut tx = db.begin().await?;
    settle(&mut tx, world, std::mem::take(&mut books.entries)).await?;
    tx.commit().await?;
    Ok(())
}

/// When the history starts.
fn start(world: &World) -> DateTime {
    renox::db::now() - Duration::days(world.volume.history_days)
}

/// A moment in the history, weighted towards weekends and opening hours.
fn moment(world: &mut World, from: DateTime, to: DateTime) -> DateTime {
    let span = (to - from).num_minutes().max(1);
    loop {
        let at = from + Duration::minutes(world.rng.range(0, span));
        let local_hour = at.hour_of_day();
        if !(8..19).contains(&local_hour) {
            continue;
        }
        let weekend = at.weekday().number_from_monday() >= 6;
        if weekend || world.rng.chance(70) {
            return at;
        }
    }
}

trait HourOfDay {
    fn hour_of_day(&self) -> u32;
}

impl HourOfDay for DateTime {
    /// The hour in Jakarta (UTC+7), where the stores are.
    fn hour_of_day(&self) -> u32 {
        use renox::chrono::Timelike;
        (self.hour() + 7) % 24
    }
}

/// Rentals bike by bike, one after another, from the start of the history
/// to a few days ahead: returned (some late, some damaged), out now, due
/// today, overdue, reserved.
async fn rentals(tx: &mut Transaction, world: &mut World, books: &mut Books) -> Result {
    let now = renox::db::now();
    let renters: Vec<i64> = world
        .clients
        .iter()
        .filter(|c| c.may_rent)
        .map(|c| c.id)
        .collect();
    if renters.is_empty() || world.bikes.is_empty() {
        return Ok(());
    }
    let bikes_total = world.bikes.len() as i64;
    // Hours between a bike's return and its next pick-up, to reach the
    // target count: the history's hours per rental, less a rental's
    // average length (about 40 hours).
    let cycle_hours =
        (world.volume.history_days * 24 * bikes_total / world.volume.rentals.max(1) as i64 - 40)
            .max(12);
    let stores: HashMap<i64, i64> = world.stores.iter().map(|s| (s.id, s.fee_rate_bp)).collect();

    let mut rows = Vec::new();
    let mut statuses: Vec<(i64, BikeStatus)> = Vec::new();
    let bikes = world.bikes.clone();
    // Whatever the volume and the time of day, each store has a bike
    // overdue and one due back later today.
    let mut seen_per_store: HashMap<i64, usize> = HashMap::new();
    let end_of_today = midnight_of(today() + Duration::days(1));
    for bike in &bikes {
        if Some(bike.id) == world.demo_bike {
            continue;
        }
        let placed = world.placements.get(&bike.id).copied();
        let nth = {
            let seen = seen_per_store.entry(bike.location_store_id).or_default();
            *seen += 1;
            *seen
        };
        let overdue_bike = nth == 1 || world.rng.chance(3);
        let due_today_bike = nth == 2;
        let maintenance_bike = !overdue_bike && !due_today_bike && world.rng.chance(5);
        let mut t = start(world) + Duration::hours(world.rng.range(0, cycle_hours));
        let mut status = BikeStatus::Available;
        let mut previous_end = start(world) - Duration::days(1);
        loop {
            let hourly = world.rng.chance(30);
            let (rate, length) = if hourly {
                (RentalRate::Hourly, Duration::hours(world.rng.range(1, 7)))
            } else {
                (RentalRate::Daily, Duration::days(world.rng.range(1, 5)))
            };
            let starts = align_to_opening(t);
            let mut due = starts + length;
            if starts > now + Duration::days(4) {
                break;
            }
            let operating = match placed {
                Some((to, moved)) if starts >= moved => to,
                _ => bike.owner_store_id,
            };
            let units = if hourly {
                length.num_hours()
            } else {
                length.num_days()
            };
            let price = units
                * if hourly {
                    bike.hourly_rate
                } else {
                    bike.daily_rate
                };
            let mut rental = Rental {
                reservation_code: Ulid::new(),
                customer_id: *world.rng.pick(&renters),
                rental_bike_id: bike.id,
                owner_store_id: bike.owner_store_id,
                operating_store_id: operating,
                rate,
                starts_at: starts,
                due_at: due,
                price,
                deposit: bike.deposit,
                served_by: world.counter_person(operating),
                created_at: Some(starts - Duration::hours(world.rng.range(1, 72))),
                ..Default::default()
            };
            if (overdue_bike || due_today_bike) && due >= now - Duration::days(1) {
                // The rental out now: picked up after the previous came back.
                rental.starts_at = (now - Duration::days(3)).max(previous_end + Duration::hours(1));
                rental.picked_up_at = Some(rental.starts_at);
                if overdue_bike {
                    // Not back: hours or a day past its due time.
                    rental.due_at = (now - Duration::hours(world.rng.range(4, 20)))
                        .max(rental.starts_at + Duration::hours(1))
                        .min(now - Duration::minutes(30));
                    rental.status = RentalStatus::Overdue;
                } else {
                    // Due back before the day ends.
                    rental.due_at = now + (end_of_today - now) / 2;
                    rental.status = RentalStatus::Active;
                }
                status = BikeStatus::Rented;
                rows.push(rental);
                break;
            }
            if due < now - Duration::hours(1) {
                // Back: mostly on time, sometimes late or damaged.
                let late = world.rng.chance(6);
                let back = if late {
                    due + Duration::hours(world.rng.range(1, 9))
                } else {
                    due - Duration::minutes(world.rng.range(0, 90))
                };
                rental.picked_up_at = Some(starts + Duration::minutes(world.rng.range(0, 20)));
                rental.returned_at = Some(back.min(now));
                rental.status = RentalStatus::Returned;
                if late {
                    rental.late_fee = (back - due).num_hours().max(1) * bike.hourly_rate;
                }
                if world.rng.chance(2) {
                    rental.damage_fee = world.rng.price(2_500, 20_000);
                }
                if world.rng.chance(3) {
                    rental.status = RentalStatus::Cancelled;
                    rental.picked_up_at = None;
                    rental.returned_at = None;
                    rental.late_fee = 0;
                    rental.damage_fee = 0;
                }
            } else if starts <= now {
                rental.picked_up_at = Some(starts);
                rental.status = RentalStatus::Active;
                if due < now {
                    due = now + Duration::hours(world.rng.range(1, 6));
                    rental.due_at = due;
                }
                status = BikeStatus::Rented;
            } else {
                rental.status = RentalStatus::Reserved;
                if status == BikeStatus::Available && starts < now + Duration::days(1) {
                    status = BikeStatus::Reserved;
                }
            }
            previous_end = rental.returned_at.unwrap_or(rental.due_at);
            rows.push(rental);
            t = starts
                + length
                + Duration::hours(world.rng.range(cycle_hours / 3, cycle_hours * 5 / 3));
        }
        if maintenance_bike && status == BikeStatus::Available {
            status = BikeStatus::Maintenance;
        }
        statuses.push((bike.id, status));
    }

    let codes: Vec<String> = rows
        .iter()
        .map(|r| r.reservation_code.to_string())
        .collect();
    Rental::insert_many(&mut *tx, rows.clone()).await?;
    let ids: HashMap<String, i64> = sql("SELECT reservation_code, id FROM rentals")
        .fetch_as::<(String, i64)>(&mut *tx)
        .await?
        .into_iter()
        .collect();
    for (rental, code) in rows.iter().zip(codes) {
        let id = ids[&code];
        let paid_at = match rental.status {
            RentalStatus::Returned => rental.returned_at,
            RentalStatus::Active | RentalStatus::Overdue => rental.picked_up_at,
            _ => None,
        };
        let Some(paid_at) = paid_at else { continue };
        let amount = match rental.status {
            RentalStatus::Returned => rental.total(),
            _ => rental.price,
        };
        let method = *world.rng.pick(&[
            PaymentMethod::Card,
            PaymentMethod::Cash,
            PaymentMethod::Card,
        ]);
        books.payments.push(Payment {
            customer_id: Some(rental.customer_id),
            payable_type: Rental::TABLE.into(),
            payable_id: id,
            store_id: rental.operating_store_id,
            amount,
            method,
            status: PaymentStatus::Paid,
            paid_at: Some(paid_at),
            received_by: rental.served_by,
            created_at: Some(paid_at),
            ..Default::default()
        });
        // The owner's rule (#245): revenue to the owner, a fee to the store
        // that served the customer; late and damage fees to the owner.
        let (owner, operating) = (rental.owner_store_id, rental.operating_store_id);
        if owner != operating {
            let rate = stores[&operating];
            let source = (Rental::TABLE, id);
            books.owe(
                operating,
                owner,
                rental.price,
                EntryKind::RentalRevenue,
                None,
                source,
                paid_at,
            );
            books.owe(
                owner,
                operating,
                fee(rental.price, rate),
                EntryKind::OperatingFee,
                Some(rate),
                source,
                paid_at,
            );
            books.owe(
                operating,
                owner,
                rental.late_fee,
                EntryKind::LateFee,
                None,
                source,
                paid_at,
            );
            books.owe(
                operating,
                owner,
                rental.damage_fee,
                EntryKind::DamageFee,
                None,
                source,
                paid_at,
            );
        }
    }
    for (id, status) in statuses {
        if status != BikeStatus::Available {
            sql("UPDATE rental_bikes SET status = ? WHERE id = ?")
                .bind(status)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(bike) = world.bikes.iter_mut().find(|b| b.id == id) {
            bike.status = status;
        }
    }
    Ok(())
}

/// Moves a start inside opening hours (08:00–18:00 in Jakarta).
fn align_to_opening(t: DateTime) -> DateTime {
    let hour = t.hour_of_day();
    if (8..18).contains(&hour) {
        t
    } else {
        let forward = (24 + 9 - hour) % 24;
        t + Duration::hours(forward as i64)
    }
}

/// Consignment shipments between stores: goods of one store sent to
/// another, most still there, a few recalled. Returns what each location
/// holds of whom: (variant, location) → (owner, line id, units left).
async fn consignments(
    tx: &mut Transaction,
    world: &mut World,
    books: &mut Books,
) -> Result<HashMap<(i64, i64), (i64, i64, i64, DateTime)>> {
    let goods: Vec<Item> = world
        .items
        .iter()
        .filter(|i| i.kind != CategoryKind::Bike)
        .cloned()
        .collect();
    let mut held = HashMap::new();
    if goods.is_empty() {
        return Ok(held);
    }
    let per_pair = (world.volume.history_days / 120).clamp(1, 5);
    let stores: Vec<i64> = world.stores.iter().map(|s| s.id).collect();
    for owner in &stores {
        for location in &stores {
            if owner == location {
                continue;
            }
            for n in 0..per_pair {
                let recalled = n == 0 && per_pair > 1 && world.rng.chance(50);
                let sent = start(world)
                    + Duration::days(world.rng.range(0, world.volume.history_days - 10));
                let received = sent + Duration::days(1);
                let recalled_at = recalled.then(|| {
                    received
                        + Duration::days(world.rng.range(20, 60))
                            .min(renox::db::now() - received - Duration::hours(1))
                });
                let created_by = world.staff_of(*owner, MANAGER).first().map(|p| p.staff_id);
                let shipment = ConsignmentShipment::create(
                    &mut *tx,
                    ConsignmentShipment {
                        owner_store_id: *owner,
                        location_store_id: *location,
                        status: if recalled {
                            ShipmentStatus::Recalled
                        } else {
                            ShipmentStatus::Received
                        },
                        sent_at: Some(sent),
                        received_at: Some(received),
                        recalled_at,
                        created_by,
                        note: Some("Sells better there: more tourists.".into()),
                        created_at: Some(sent),
                        ..Default::default()
                    },
                )
                .await?;
                let reference = Some((ConsignmentShipment::TABLE, shipment.id));
                for _ in 0..world.rng.range(3, 8) {
                    let item = world.rng.pick(&goods).clone();
                    let quantity = world.rng.range(2, 7);
                    let line = ConsignmentShipmentLine::create(
                        &mut *tx,
                        ConsignmentShipmentLine {
                            shipment_id: shipment.id,
                            variant_id: item.variant_id,
                            quantity,
                            received_quantity: quantity,
                            returned_quantity: if recalled { quantity } else { 0 },
                            created_at: Some(sent),
                            ..Default::default()
                        },
                    )
                    .await?;
                    let home = (item.variant_id, *owner, *owner);
                    let away = (item.variant_id, *owner, *location);
                    books.move_stock(
                        home,
                        -quantity,
                        MovementReason::ConsignOut,
                        reference,
                        created_by,
                        sent,
                        None,
                    );
                    books.move_stock(
                        away,
                        quantity,
                        MovementReason::ConsignIn,
                        reference,
                        None,
                        received,
                        None,
                    );
                    if let Some(at) = recalled_at {
                        books.move_stock(
                            away,
                            -quantity,
                            MovementReason::Recall,
                            reference,
                            None,
                            at,
                            None,
                        );
                        books.move_stock(
                            home,
                            quantity,
                            MovementReason::Recall,
                            reference,
                            created_by,
                            at + Duration::days(1),
                            None,
                        );
                    } else {
                        held.entry((item.variant_id, *location))
                            .or_insert((*owner, line.id, quantity, received));
                    }
                }
            }
        }
    }
    Ok(held)
}

/// Orders online and at the counters, with their lines, payments, stock
/// movements, and the books for consigned goods sold.
async fn orders(
    tx: &mut Transaction,
    world: &mut World,
    books: &mut Books,
    mut consigned: HashMap<(i64, i64), (i64, i64, i64, DateTime)>,
) -> Result {
    let now = renox::db::now();
    let sellable: Vec<Item> = world.items.clone();
    if sellable.is_empty() {
        return Ok(());
    }
    let goods: Vec<Item> = sellable
        .iter()
        .filter(|i| i.kind != CategoryKind::Bike)
        .cloned()
        .collect();
    let bikes: Vec<Item> = sellable
        .iter()
        .filter(|i| i.kind == CategoryKind::Bike)
        .cloned()
        .collect();
    let rates: HashMap<i64, i64> = world.stores.iter().map(|s| (s.id, s.fee_rate_bp)).collect();
    let prefixes: HashMap<i64, char> = world
        .stores
        .iter()
        .map(|s| (s.id, s.name.chars().next().unwrap_or('S')))
        .collect();

    let mut orders = Vec::new();
    let mut lines: Vec<Vec<OrderItem>> = Vec::new();
    let mut sold_from: HashMap<i64, i64> = HashMap::new(); // shipment line → units sold
    for n in 0..world.volume.orders {
        let store = world.any_store();
        let placed = moment(world, start(world), now - Duration::minutes(10));
        let counter = world.rng.chance(60);
        let customer = if counter && world.rng.chance(30) {
            None
        } else {
            Some(world.rng.pick(&world.clients).clone())
        };
        let delivery = !counter && world.rng.chance(30);
        let age = now - placed;
        let status = if counter {
            if world.rng.chance(2) {
                OrderStatus::Refunded
            } else {
                OrderStatus::Completed
            }
        } else if age > Duration::days(7) {
            [
                OrderStatus::Completed,
                OrderStatus::Refunded,
                OrderStatus::Cancelled,
            ][world.rng.weighted(&[92, 3, 5])]
        } else if age > Duration::days(2) {
            [
                OrderStatus::Completed,
                OrderStatus::Ready,
                OrderStatus::Paid,
            ][world.rng.weighted(&[70, 20, 10])]
        } else {
            [OrderStatus::Pending, OrderStatus::Paid, OrderStatus::Ready]
                [world.rng.weighted(&[20, 50, 30])]
        };
        let served_by = if counter {
            world.counter_person(store)
        } else {
            None
        };
        let mut items = Vec::new();
        for _ in 0..world.rng.range(1, 4) {
            let item = if !bikes.is_empty() && world.rng.chance(12) {
                world.rng.pick(&bikes).clone()
            } else {
                world.rng.pick(&goods).clone()
            };
            let quantity = if item.kind == CategoryKind::Bike {
                1
            } else {
                world.rng.range(1, 3)
            };
            // Goods on consignment here are sold first, for their owner.
            let mut owner = store;
            if let Some((from, line, left, since)) = consigned.get_mut(&(item.variant_id, store))
                && *left >= quantity
                && placed > *since
                && status != OrderStatus::Cancelled
                && world.rng.chance(70)
            {
                owner = *from;
                *left -= quantity;
                *sold_from.entry(*line).or_default() += quantity;
            }
            items.push(OrderItem {
                variant_id: item.variant_id,
                owner_store_id: owner,
                quantity,
                unit_price: item.price,
                total: item.price * quantity,
                created_at: Some(placed),
                ..Default::default()
            });
        }
        let subtotal: i64 = items.iter().map(|i| i.total).sum();
        let discount = if world.rng.chance(10) {
            subtotal / 10 / 100 * 100
        } else {
            0
        };
        let delivery_fee = if delivery {
            crate::app::sales::checkout::DELIVERY_LOCAL
        } else {
            0
        };
        let paid = !matches!(status, OrderStatus::Pending | OrderStatus::Cancelled);
        orders.push(Order {
            number: format!("{}-{:06}", prefixes[&store], n + 1),
            customer_id: customer.as_ref().map(|c| c.id),
            operating_store_id: store,
            channel: if counter {
                Channel::Counter
            } else {
                Channel::Online
            },
            fulfilment: if delivery {
                Fulfilment::Delivery
            } else {
                Fulfilment::Pickup
            },
            status,
            subtotal,
            discount,
            delivery_fee,
            total: subtotal - discount + delivery_fee,
            delivery_address_id: if delivery {
                customer.as_ref().and_then(|c| c.address_id)
            } else {
                None
            },
            placed_at: Some(placed),
            paid_at: paid.then_some(placed + Duration::minutes(world.rng.range(0, 30))),
            served_by,
            created_at: Some(placed),
            ..Default::default()
        });
        lines.push(items);
    }
    let numbers: Vec<String> = orders.iter().map(|o| o.number.clone()).collect();
    Order::insert_many(&mut *tx, orders.clone()).await?;
    let ids: HashMap<String, i64> = sql("SELECT number, id FROM orders")
        .fetch_as::<(String, i64)>(&mut *tx)
        .await?
        .into_iter()
        .collect();

    let mut all_items = Vec::new();
    for ((order, number), items) in orders.iter().zip(numbers).zip(lines) {
        let id = ids[&number];
        let store = order.operating_store_id;
        let at = order.paid_at.unwrap_or(order.placed_at.unwrap_or(now));
        if let Some(paid_at) = order.paid_at {
            let method = match order.channel {
                Channel::Counter => *world.rng.pick(&[PaymentMethod::Cash, PaymentMethod::Card]),
                Channel::Online => PaymentMethod::Gateway,
            };
            books.payments.push(Payment {
                customer_id: order.customer_id,
                payable_type: Order::TABLE.into(),
                payable_id: id,
                store_id: store,
                amount: order.total,
                method,
                gateway_reference: (method == PaymentMethod::Gateway)
                    .then(|| format!("pay_{}", Ulid::new())),
                status: if order.status == OrderStatus::Refunded {
                    PaymentStatus::Refunded
                } else {
                    PaymentStatus::Paid
                },
                paid_at: Some(paid_at),
                received_by: order.served_by,
                created_at: Some(paid_at),
                ..Default::default()
            });
        }
        let reference = Some((Order::TABLE, id));
        for mut item in items {
            item.order_id = id;
            let key = (item.variant_id, item.owner_store_id, store);
            match order.status {
                OrderStatus::Completed | OrderStatus::Refunded => {
                    books.move_stock(
                        key,
                        -item.quantity,
                        MovementReason::Sale,
                        reference,
                        order.served_by,
                        at,
                        None,
                    );
                    if order.status == OrderStatus::Refunded {
                        books.move_stock(
                            key,
                            item.quantity,
                            MovementReason::Return,
                            reference,
                            order.served_by,
                            at + Duration::days(2),
                            None,
                        );
                    } else if item.owner_store_id != store {
                        let rate = rates[&store];
                        let source = (Order::TABLE, id);
                        books.owe(
                            store,
                            item.owner_store_id,
                            item.total,
                            EntryKind::SaleRevenue,
                            None,
                            source,
                            at,
                        );
                        books.owe(
                            item.owner_store_id,
                            store,
                            fee(item.total, rate),
                            EntryKind::SellingFee,
                            Some(rate),
                            source,
                            at,
                        );
                    }
                }
                OrderStatus::Paid | OrderStatus::Ready => {
                    books.move_stock(
                        key,
                        item.quantity,
                        MovementReason::Reserved,
                        reference,
                        None,
                        at,
                        None,
                    );
                    *books.reserved.entry(key).or_default() += item.quantity;
                }
                _ => {}
            }
            all_items.push(item);
        }
    }
    OrderItem::insert_many(&mut *tx, all_items).await?;
    for (line, sold) in sold_from {
        sql("UPDATE consignment_shipment_lines SET sold_quantity = ? WHERE id = ?")
            .bind(sold)
            .bind(line)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

/// Purchase orders to suppliers: delivered ones in the past, some on their
/// way now.
async fn purchases(tx: &mut Transaction, world: &mut World, books: &mut Books) -> Result {
    let goods: Vec<Item> = world
        .items
        .iter()
        .filter(|i| i.kind != CategoryKind::Bike)
        .cloned()
        .collect();
    if goods.is_empty() || world.suppliers.is_empty() {
        return Ok(());
    }
    let now = renox::db::now();
    let per_store = (world.volume.history_days / 20).max(3);
    let stores: Vec<i64> = world.stores.iter().map(|s| s.id).collect();
    for store in stores {
        for n in 0..per_store {
            let supplier = world.rng.pick(&world.suppliers).clone();
            let recent = n + 2 >= per_store;
            let ordered = if recent {
                now - Duration::days(world.rng.range(1, 4))
            } else {
                start(world) + Duration::days(world.rng.range(0, world.volume.history_days - 10))
            };
            let status = if recent {
                if n + 1 == per_store {
                    PurchaseStatus::Draft
                } else {
                    PurchaseStatus::Ordered
                }
            } else {
                PurchaseStatus::Received
            };
            let received = (status == PurchaseStatus::Received)
                .then(|| ordered + Duration::days(supplier.lead_days));
            let created_by = world.staff_of(store, MANAGER).first().map(|p| p.staff_id);
            let mut order = PurchaseOrder::create(
                &mut *tx,
                PurchaseOrder {
                    supplier_id: supplier.id,
                    store_id: store,
                    status,
                    ordered_at: (status != PurchaseStatus::Draft).then_some(ordered),
                    expected_on: Some((ordered + Duration::days(supplier.lead_days)).date_naive()),
                    received_at: received,
                    created_by,
                    created_at: Some(ordered),
                    ..Default::default()
                },
            )
            .await?;
            let mut total = 0;
            for _ in 0..world.rng.range(3, 9) {
                let item = world.rng.pick(&goods).clone();
                let quantity = world.rng.range(3, 13);
                let unit_cost = item.price * 6 / 10;
                total += quantity * unit_cost;
                PurchaseOrderLine::create(
                    &mut *tx,
                    PurchaseOrderLine {
                        purchase_order_id: order.id,
                        variant_id: item.variant_id,
                        quantity,
                        received_quantity: if received.is_some() { quantity } else { 0 },
                        unit_cost,
                        created_at: Some(ordered),
                        ..Default::default()
                    },
                )
                .await?;
                if let Some(at) = received {
                    books.move_stock(
                        (item.variant_id, store, store),
                        quantity,
                        MovementReason::Purchase,
                        Some((PurchaseOrder::TABLE, order.id)),
                        created_by,
                        at,
                        None,
                    );
                }
            }
            order.total = total;
            order.save_only(&mut *tx, &["total"]).await?;
        }
    }
    Ok(())
}

/// Customers' bikes, plan subscriptions and work orders: done in the past,
/// on the bench today, booked for the next two weeks; fleet repairs billed
/// to the owner store.
async fn workshop(tx: &mut Transaction, world: &mut World, books: &mut Books) -> Result {
    let now = renox::db::now();
    if world.tasks.is_empty() {
        return Ok(());
    }
    // Customers' bikes: one or two for most customers.
    let mut bikes = Vec::new();
    let clients = world.clients.clone();
    for client in &clients {
        if world.rng.chance(25) {
            continue;
        }
        for _ in 0..(if world.rng.chance(30) { 2 } else { 1 }) {
            let (product, name) = if world.bike_products.is_empty() || world.rng.chance(20) {
                (None, "A bike bought elsewhere".to_owned())
            } else {
                let (id, name) = world.rng.pick(&world.bike_products).clone();
                (Some(id), name)
            };
            let colour = *world
                .rng
                .pick(&["black", "blue", "grey", "red", "white", "green"]);
            bikes.push(CustomerBike {
                customer_id: client.id,
                product_id: product,
                name: format!("{name}, {colour}"),
                frame_number: Some(format!("CB{:08}", world.rng.range(0, 99_999_999))),
                bought_on: Some(today() - Duration::days(world.rng.range(30, 1_200))),
                ..Default::default()
            });
        }
    }
    CustomerBike::insert_many(&mut *tx, bikes).await?;
    let owners: HashMap<i64, i64> = sql("SELECT id, customer_id FROM customer_bikes")
        .fetch_as::<(i64, i64)>(&mut *tx)
        .await?
        .into_iter()
        .collect();
    let mut bike_ids: Vec<i64> = owners.keys().copied().collect();
    bike_ids.sort_unstable();
    if bike_ids.is_empty() {
        return Ok(());
    }

    // About one bike in eight is on a plan.
    let mut subscriptions = Vec::new();
    for bike in &bike_ids {
        if !world.rng.chance(12) || world.plans.is_empty() {
            continue;
        }
        let (plan, _) = world.rng.pick(&world.plans).clone();
        let store = world.any_store();
        let starts =
            today() - Duration::days(world.rng.range(14, world.volume.history_days.max(15)));
        let cancelled = world.rng.chance(10);
        subscriptions.push(PlanSubscription {
            customer_bike_id: *bike,
            service_plan_id: plan.id,
            store_id: store,
            preferred_weekday: starts.weekday().number_from_monday() as i64,
            status: if cancelled {
                SubscriptionStatus::Cancelled
            } else {
                SubscriptionStatus::Active
            },
            starts_on: starts,
            next_visit_on: (!cancelled)
                .then(|| today() + Duration::days(world.rng.range(1, plan.frequency.days() + 1))),
            cancelled_at: cancelled.then(|| now - Duration::days(world.rng.range(1, 30))),
            created_at: Some(starts.and_hms_opt(10, 0, 0).expect("time").and_utc()),
            ..Default::default()
        });
    }
    PlanSubscription::insert_many(&mut *tx, subscriptions).await?;
    let subscriptions = PlanSubscription::query()
        .order_by(PlanSubscription::ID)
        .get(&mut *tx)
        .await?;

    // Work orders.
    let parts: Vec<Item> = world
        .items
        .iter()
        .filter(|i| i.kind == CategoryKind::Part)
        .cloned()
        .collect();
    let mut orders = Vec::new();
    let mut tasks: Vec<Vec<WorkOrderTask>> = Vec::new();
    let mut used_parts: Vec<Option<(Item, i64)>> = Vec::new();
    let fleet = world.bikes.clone();
    for _ in 0..world.volume.work_orders {
        let source = [
            WorkSource::WalkIn,
            WorkSource::Booking,
            WorkSource::Plan,
            WorkSource::Fleet,
        ][world.rng.weighted(&[40, 35, 15, 10])];
        let scheduled = moment(world, start(world), now + Duration::days(14));
        let subscription = (source == WorkSource::Plan && !subscriptions.is_empty())
            .then(|| world.rng.pick(&subscriptions).clone());
        let source = if source == WorkSource::Plan && subscription.is_none() {
            WorkSource::Booking
        } else {
            source
        };
        let mut order = WorkOrder {
            source,
            scheduled_for: scheduled,
            customer_note: world.rng.chance(30).then(|| {
                (*world.rng.pick(&[
                    "Squeaky brakes when it rains.",
                    "Gears skip on the hill home.",
                    "Back wheel wobbles.",
                    "Please check the battery, range dropped.",
                ]))
                .to_owned()
            }),
            created_at: Some(scheduled - Duration::days(world.rng.range(0, 7))),
            ..Default::default()
        };
        match (&subscription, source) {
            (Some(sub), _) => {
                order.store_id = sub.store_id;
                order.customer_bike_id = Some(sub.customer_bike_id);
                order.plan_subscription_id = Some(sub.id);
            }
            (None, WorkSource::Fleet) if !fleet.is_empty() => {
                let bike = world.rng.pick(&fleet).clone();
                order.rental_bike_id = Some(bike.id);
                order.store_id = bike.location_store_id;
                order.billed_store_id =
                    (bike.owner_store_id != bike.location_store_id).then_some(bike.owner_store_id);
            }
            _ => {
                order.source = if source == WorkSource::Fleet {
                    WorkSource::WalkIn
                } else {
                    source
                };
                order.store_id = world.any_store();
                order.customer_bike_id = Some(*world.rng.pick(&bike_ids));
            }
        }
        order.mechanic_id = world.mechanic(order.store_id);
        order.status = if scheduled > now + Duration::hours(2) {
            WorkStatus::Booked
        } else if scheduled > now - Duration::days(1) {
            [
                WorkStatus::CheckedIn,
                WorkStatus::InProgress,
                WorkStatus::WaitingParts,
                WorkStatus::Ready,
            ][world.rng.weighted(&[25, 35, 20, 20])]
        } else if scheduled > now - Duration::days(4) && world.rng.chance(8) {
            WorkStatus::WaitingParts
        } else if world.rng.chance(5) {
            WorkStatus::Cancelled
        } else {
            WorkStatus::Completed
        };
        if !matches!(
            order.status,
            WorkStatus::Booked | WorkStatus::Cancelled | WorkStatus::CheckedIn
        ) {
            order.started_at = Some(scheduled + Duration::minutes(world.rng.range(0, 120)));
        }
        if order.status == WorkStatus::Completed {
            order.completed_at =
                Some(scheduled + Duration::hours(world.rng.range(2, 30)).min(now - scheduled));
        }
        // Its tasks: the plan's, or one to three.
        let plan_tasks: Vec<i64> = subscription
            .as_ref()
            .and_then(|s| world.plans.iter().find(|(p, _)| p.id == s.service_plan_id))
            .map(|(_, ids)| ids.clone())
            .unwrap_or_default();
        let chosen: Vec<i64> = if plan_tasks.is_empty() {
            (0..world.rng.range(1, 4))
                .map(|_| world.rng.pick(&world.tasks).id)
                .collect()
        } else {
            plan_tasks
        };
        let mut rows = Vec::new();
        for task_id in chosen {
            let task = world
                .tasks
                .iter()
                .find(|t| t.id == task_id)
                .expect("a task")
                .clone();
            rows.push(WorkOrderTask {
                service_task_id: task.id,
                minutes: task.minutes,
                price: task.price,
                done: order.status == WorkStatus::Completed
                    || (order.status == WorkStatus::InProgress && world.rng.chance(50)),
                created_at: order.created_at,
                ..Default::default()
            });
        }
        order.labour = rows.iter().map(|t| t.price).sum();
        let part = (!parts.is_empty() && world.rng.chance(30))
            .then(|| (world.rng.pick(&parts).clone(), world.rng.range(1, 3)));
        order.parts = part.as_ref().map_or(0, |(item, qty)| item.price * qty);
        order.total = order.labour + order.parts;
        orders.push(order);
        tasks.push(rows);
        used_parts.push(part);
    }
    // Whatever the volume, each workshop has bikes on the bench right now.
    let on_bench = [
        WorkStatus::InProgress,
        WorkStatus::WaitingParts,
        WorkStatus::CheckedIn,
    ];
    let stores: Vec<i64> = world.stores.iter().map(|s| s.id).collect();
    let mut wanted: Vec<(i64, WorkStatus)> = stores
        .iter()
        .flat_map(|store| on_bench.iter().map(move |status| (*store, *status)))
        .collect();
    for (order, rows) in orders.iter_mut().zip(&mut tasks) {
        if wanted.is_empty() {
            break;
        }
        if order.source == WorkSource::Fleet || order.plan_subscription_id.is_some() {
            continue;
        }
        let (store, status) = wanted.remove(0);
        order.store_id = store;
        order.mechanic_id = world.mechanic(store);
        order.status = status;
        order.scheduled_for = now - Duration::hours(world.rng.range(1, 5));
        order.started_at =
            (status != WorkStatus::CheckedIn).then(|| order.scheduled_for + Duration::minutes(15));
        order.completed_at = None;
        for task in rows.iter_mut() {
            task.done = false;
        }
    }
    WorkOrder::insert_many(&mut *tx, orders.clone()).await?;
    let ids: Vec<i64> = sql("SELECT id FROM work_orders ORDER BY id")
        .scalars(&mut *tx)
        .await?;
    let ids = &ids[ids.len() - orders.len()..];
    let mut all_tasks = Vec::new();
    for (((order, id), rows), part) in orders.iter().zip(ids).zip(tasks).zip(used_parts) {
        for mut task in rows {
            task.work_order_id = *id;
            all_tasks.push(task);
        }
        let done = order.status == WorkStatus::Completed;
        let at = order.completed_at.unwrap_or(order.scheduled_for);
        if let Some((item, qty)) = part
            && (done || order.status == WorkStatus::InProgress)
        {
            books.move_stock(
                (item.variant_id, order.store_id, order.store_id),
                -qty,
                MovementReason::Service,
                Some((WorkOrder::TABLE, *id)),
                order.mechanic_id,
                at,
                None,
            );
        }
        if !done {
            continue;
        }
        match (order.rental_bike_id, order.billed_store_id) {
            // A repair of another store's bike: the owner pays the workshop.
            (Some(_), Some(owner)) => books.owe(
                owner,
                order.store_id,
                order.total,
                EntryKind::Repair,
                None,
                (WorkOrder::TABLE, *id),
                at,
            ),
            (Some(_), None) => {}
            _ => books.payments.push(Payment {
                customer_id: order.customer_bike_id.and_then(|b| owners.get(&b).copied()),
                payable_type: WorkOrder::TABLE.into(),
                payable_id: *id,
                store_id: order.store_id,
                amount: order.total,
                method: *world.rng.pick(&[PaymentMethod::Card, PaymentMethod::Cash]),
                status: PaymentStatus::Paid,
                paid_at: Some(at),
                created_at: Some(at),
                ..Default::default()
            }),
        }
    }
    WorkOrderTask::insert_many(&mut *tx, all_tasks).await?;
    Ok(())
}

/// Stock levels that add up to the ledger: an opening balance per store
/// and variant, so that after every movement the level is the one wanted
/// (a few below their reorder level).
async fn stock(tx: &mut Transaction, world: &mut World, books: &mut Books) -> Result {
    let mut delta: HashMap<Key, i64> = HashMap::new();
    for m in &books.movements {
        if !matches!(
            m.reason,
            MovementReason::Reserved | MovementReason::Released
        ) {
            *delta
                .entry((m.variant_id, m.owner_store_id, m.location_store_id))
                .or_default() += m.quantity;
        }
    }
    let stores: Vec<i64> = world.stores.iter().map(|s| s.id).collect();
    let mut keys: Vec<Key> = delta.keys().copied().collect();
    // Every store stocks every piece of gear and every part, and some bikes.
    for item in world.items.clone() {
        for store in &stores {
            let key = (item.variant_id, *store, *store);
            if item.kind == CategoryKind::Bike && !world.rng.chance(50) && !delta.contains_key(&key)
            {
                continue;
            }
            if !delta.contains_key(&key) {
                keys.push(key);
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    let reorder: HashMap<i64, i64> = world
        .items
        .iter()
        .map(|i| (i.variant_id, i.reorder_level))
        .collect();
    let opening_at = start(world) - Duration::days(1);
    let mut levels = Vec::new();
    for key in keys {
        let change = delta.get(&key).copied().unwrap_or(0);
        let (variant, owner, location) = key;
        let reorder_level = reorder.get(&variant).copied().unwrap_or(2);
        let on_hand = if owner != location {
            change // consigned: only what was sent, sold and recalled
        } else {
            let wanted = if world.rng.chance(6) {
                world.rng.range(0, reorder_level.max(1))
            } else {
                world.rng.range(reorder_level + 1, reorder_level + 25)
            };
            let opening = (wanted - change).max(0);
            if opening > 0 {
                let created_by = world.staff_of(owner, MANAGER).first().map(|p| p.staff_id);
                books.move_stock(
                    key,
                    opening,
                    MovementReason::Purchase,
                    None,
                    created_by,
                    opening_at,
                    Some("Opening stock"),
                );
            }
            opening + change
        };
        if owner != location && on_hand <= 0 && !books.reserved.contains_key(&key) {
            continue;
        }
        let reserved = books
            .reserved
            .get(&key)
            .copied()
            .unwrap_or(0)
            .min(on_hand.max(0));
        levels.push(StockLevel {
            variant_id: variant,
            owner_store_id: owner,
            location_store_id: location,
            on_hand,
            reserved,
            created_at: Some(opening_at),
            ..Default::default()
        });
    }
    StockLevel::insert_many(&mut *tx, levels).await?;
    let mut movements = std::mem::take(&mut books.movements);
    movements.sort_by_key(|m| m.created_at);
    StockMovement::insert_many(&mut *tx, movements).await?;
    Ok(())
}

/// The books between stores and their monthly settlements: every complete
/// month is netted per pair of stores; all but the last are settled.
async fn settle(
    tx: &mut Transaction,
    world: &mut World,
    mut entries: Vec<IntercompanyEntry>,
) -> Result {
    entries.sort_by_key(|e| e.booked_at);
    let this_month = first_of_month(today());
    let owner = sql("SELECT user_id FROM role_user WHERE scope_type = '' ORDER BY user_id")
        .scalar_optional::<i64>(&mut *tx)
        .await?;
    // (month, low store, high store) → net owed by low to high (negative: high owes low).
    let mut nets: HashMap<(NaiveDate, i64, i64), i64> = HashMap::new();
    for e in &entries {
        let month = first_of_month(e.booked_at.date_naive());
        if month >= this_month {
            continue;
        }
        let (low, high) = (
            e.debtor_store_id.min(e.creditor_store_id),
            e.debtor_store_id.max(e.creditor_store_id),
        );
        let signed = if e.debtor_store_id == low {
            e.amount
        } else {
            -e.amount
        };
        *nets.entry((month, low, high)).or_default() += signed;
    }
    let last_complete = previous_month(this_month);
    let mut keys: Vec<_> = nets.keys().copied().collect();
    keys.sort_unstable();
    let mut ids: HashMap<(NaiveDate, i64, i64), i64> = HashMap::new();
    for key in keys {
        let (month, low, high) = key;
        let net = nets[&key];
        let (debtor, creditor) = if net >= 0 { (low, high) } else { (high, low) };
        let settled = month < last_complete;
        let end = next_month(month) - Duration::days(1);
        let settlement = Settlement::create(
            &mut *tx,
            Settlement {
                debtor_store_id: debtor,
                creditor_store_id: creditor,
                period_start: month,
                period_end: end,
                amount: net.abs(),
                status: if settled {
                    SettlementStatus::Settled
                } else {
                    SettlementStatus::Open
                },
                settled_at: settled.then(|| midnight_of(next_month(month) + Duration::days(4))),
                settled_by: if settled { owner } else { None },
                created_at: Some(midnight_of(next_month(month))),
                ..Default::default()
            },
        )
        .await?;
        ids.insert(key, settlement.id);
    }
    for e in &mut entries {
        let month = first_of_month(e.booked_at.date_naive());
        let (low, high) = (
            e.debtor_store_id.min(e.creditor_store_id),
            e.debtor_store_id.max(e.creditor_store_id),
        );
        e.settlement_id = ids.get(&(month, low, high)).copied();
    }
    let _ = world;
    IntercompanyEntry::insert_many(&mut *tx, entries).await?;
    Ok(())
}

fn first_of_month(day: NaiveDate) -> NaiveDate {
    day.with_day(1).expect("the first of the month")
}

fn next_month(first: NaiveDate) -> NaiveDate {
    first_of_month(first + Duration::days(32))
}

fn previous_month(first: NaiveDate) -> NaiveDate {
    first_of_month(first - Duration::days(1))
}

fn midnight_of(day: NaiveDate) -> DateTime {
    day.and_hms_opt(0, 0, 0).expect("midnight").and_utc()
}

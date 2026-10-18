//! The rental state machine: where a rental can go from each status, and
//! what has to hold (docs/state-machines.md).
//!
//! ```text
//! reserved --pickup--> active --overdue--> overdue
//!    |  |                 |                   |
//!    |  +-- no_show       +-----return-------+--> returned
//!    +-- cancel / lapse --> cancelled
//! ```
//!
//! Every status change of a [`Rental`] goes through here: the model is
//! `#[model(state_machine)]`, so `save` refuses a changed status and the
//! handlers, the kiosk API and the scheduled tasks call
//! `rental.transition("pickup")` and friends instead. Each run checks the
//! `from` status and the guards, moves the status with an optimistic
//! `UPDATE`, and writes a `state_transitions` row (the history), in the same
//! transaction as the rental's other columns.
//!
//! All the transitions are `system` ones: who may hand a bike over, take it
//! back or cancel is decided before, by the area's own checks (the store
//! scoped `access::require` at the counter, the kiosk token, the customer's
//! own reservation), and the staff member goes into the history row's
//! `metadata`. `rnx state:diagram Rental` prints the diagram.

use renox::state_machine::{Machine, StateMachine, Transition};

use super::model::{Rental, RentalStatus};

impl StateMachine for Rental {
    type State = RentalStatus;
    const COLUMN: renox::db::Col<Rental, RentalStatus> = Rental::STATUS;

    fn machine() -> Machine<Self> {
        use RentalStatus::{Active, Cancelled, NoShow, Overdue, Reserved, Returned};
        Machine::new()
            .state(Reserved)
            .tone("info")
            .state(Active)
            .tone("success")
            .state(Overdue)
            .tone("danger")
            .state(Returned)
            .tone("neutral")
            .terminal()
            .state(Cancelled)
            .tone("neutral")
            .terminal()
            .state(NoShow)
            .tone("warning")
            .terminal()
            // The bike leaves with the customer.
            .transition(
                Transition::new("pickup")
                    .label("Hand over")
                    .from([Reserved])
                    .to(Active)
                    .system(),
            )
            // The overdue task: out past the due time.
            .transition(
                Transition::new("overdue")
                    .label("Mark overdue")
                    .from([Active])
                    .to(Overdue)
                    .system()
                    .guard("past_due", |rental: &Rental, _ctx| {
                        Box::pin(async move {
                            if rental.due_at < renox::db::now() {
                                Ok(())
                            } else {
                                Err("The rental is not past its due time.".into())
                            }
                        })
                    }),
            )
            // The bike is back, on time or late.
            .transition(
                Transition::new("return")
                    .label("Take back")
                    .from([Active, Overdue])
                    .to(Returned)
                    .system(),
            )
            // The customer calls it off, until an hour before the start.
            .transition(
                Transition::new("cancel")
                    .label("Cancel")
                    .from([Reserved])
                    .to(Cancelled)
                    .system()
                    .guard("in_time", |rental: &Rental, _ctx| {
                        Box::pin(async move {
                            if rental.cancellable() {
                                Ok(())
                            } else {
                                Err("It is too late to cancel this reservation.".into())
                            }
                        })
                    }),
            )
            // An unpaid reservation is called off (the task, or a failed payment).
            .transition(
                Transition::new("lapse")
                    .label("Lapse")
                    .from([Reserved])
                    .to(Cancelled)
                    .system(),
            )
            // Not picked up within half an hour of the start.
            .transition(
                Transition::new("no_show")
                    .label("No-show")
                    .from([Reserved])
                    .to(NoShow)
                    .system(),
            )
    }
}

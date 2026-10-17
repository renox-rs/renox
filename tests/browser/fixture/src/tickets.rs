//! A ticket that moves through a state machine, for `<rx-transition-actions>`
//! (transition-actions.test.mjs): `POST /tickets` makes one, `/tickets/{id}`
//! shows its buttons and its history.

use renox::db::Col;
use renox::prelude::*;
use renox::state_machine::{self, Machine, StateMachine, Transition};
use serde::Serialize;

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TicketStatus {
    Open,
    Escalated,
    Closed,
}

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "fixture_tickets")]
pub struct Ticket {
    pub id: i64,
    pub status: String,
}

impl StateMachine for Ticket {
    type State = TicketStatus;
    const COLUMN: Col<Self, TicketStatus> = Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new()
            .state(TicketStatus::Open)
            .state(TicketStatus::Escalated)
            .state(TicketStatus::Closed)
            .terminal()
            .transition(
                Transition::new("escalate")
                    .from([TicketStatus::Open])
                    .to(TicketStatus::Escalated),
            )
            .transition(
                Transition::new("close")
                    .tone("danger")
                    .from([TicketStatus::Open, TicketStatus::Escalated])
                    .to(TicketStatus::Closed)
                    .reason_required(),
            )
    }
}

pub struct Tickets;

impl Module for Tickets {
    fn name(&self) -> &'static str {
        "tickets"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .post("/tickets", |State(state): State<AppState>| async move {
                let ticket = Ticket::create(
                    &state.db,
                    Ticket {
                        status: "open".into(),
                        ..Default::default()
                    },
                )
                .await?;
                Ok::<_, Error>(ticket.id.to_string())
            })
            .get(
                "/tickets/{id}",
                |State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>| async move {
                    let ticket = Ticket::find(&state.db, id).await?.ok_or(Error::NotFound)?;
                    let actions = ticket.available(&user, &state.db).await?;
                    let log = state_machine::history(&state.db, "fixture_tickets", id).await?;
                    let items =
                        state_machine::timeline(&log, Some(&state_machine::info_of::<Ticket>()));
                    Ok::<_, Error>(view(
                        "tickets.html",
                        context! { ticket => ticket, actions => actions, items => items },
                    ))
                },
            )
            .group("/tickets", "tickets.", state_machine::routes::<Ticket>())
            .require_auth()
    }
}

//! #39: the admin shows a state machine read-only (diagram, transitions) and
//! offers the transitions available for the record as buttons.

use renox::db::Col;
use renox::grid::Column;
use renox::prelude::*;
use renox::state_machine::{Available, Machine, MachineInfo, StateMachine, Transition, info_of};
use renox::testing::TestApp;
use renox_admin::{Admin, AdminAction, AdminResource, Field};
use serde::{Deserialize, Serialize};

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum TicketStatus {
    Draft,
    Live,
    Retired,
}

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "tickets")]
struct Ticket {
    id: i64,
    title: String,
    status: String,
}

impl Policy for Ticket {
    fn allows(&self, _user: &User, _ability: &str) -> bool {
        true
    }
}

impl StateMachine for Ticket {
    type State = TicketStatus;
    const COLUMN: Col<Self, TicketStatus> = Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new()
            .state(TicketStatus::Draft)
            .state(TicketStatus::Retired)
            .terminal()
            .transition(
                Transition::new("publish")
                    .from([TicketStatus::Draft])
                    .to(TicketStatus::Live),
            )
            .transition(
                Transition::new("retire")
                    .from([TicketStatus::Live])
                    .to(TicketStatus::Retired)
                    .reason_required(),
            )
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct TicketForm {
    #[validate(required)]
    title: String,
}

struct Tickets;

impl AdminResource for Tickets {
    type Model = Ticket;
    type Form = TicketForm;

    fn label(&self) -> &str {
        "Ticket"
    }
    fn plural_label(&self) -> &str {
        "Tickets"
    }
    fn columns(&self) -> Vec<Column> {
        vec![Column::text("title", "Title")]
    }
    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title", "Title").required()]
    }
    fn fill(&self, ticket: &mut Ticket, form: TicketForm) {
        ticket.title = form.title;
    }
    fn actions(&self) -> Vec<AdminAction<Ticket>> {
        AdminAction::transitions()
    }
    fn state_machine(&self) -> Option<MachineInfo> {
        Some(info_of::<Ticket>())
    }
    async fn available_transitions(
        &self,
        record: &Ticket,
        user: &AuthUser,
        db: &Db,
    ) -> Result<Vec<Available>> {
        record.available(user, db).await
    }
}

async fn app() -> TestApp {
    TestApp::new(
        App::new()
            .module(Auth::new())
            .module(
                Admin::new()
                    .authorize(|user| user.email.ends_with("@example.com"))
                    .resource(Tickets),
            )
            .migrations(renox::migrations!("tests/migrations")),
    )
    .await
}

#[renox::test]
async fn the_view_page_draws_the_machine_and_the_available_transitions() {
    let app = app().await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "secret-password-1")
        .await
        .unwrap();
    app.acting_as(&ana);
    let ticket = Ticket::create(
        app.db(),
        Ticket {
            title: "First".into(),
            status: "draft".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let page = format!("/admin/tickets/{}", ticket.id);

    app.get(&page)
        .await
        .assert_ok()
        .assert_see("stateDiagram-v2")
        .assert_see(&format!("{page}/actions/publish"))
        .assert_dont_see(&format!("{page}/actions/retire"));

    app.post(&format!("{page}/actions/publish"), &[("x", "y")])
        .await;
    let ticket = Ticket::find(app.db(), ticket.id).await.unwrap().unwrap();
    assert_eq!(ticket.status, "live");

    app.get(&page)
        .await
        .assert_ok()
        .assert_dont_see(&format!("{page}/actions/publish"))
        .assert_see(&format!("{page}/actions/retire"));
}

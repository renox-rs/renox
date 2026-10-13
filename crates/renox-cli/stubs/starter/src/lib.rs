//! {{title}}: a Renox app made from the starter kit (`rnx new --starter`).
//!
//! - sign-up, login, password reset, email verification and the account
//!   pages (`Auth`), with the notification bell;
//! - roles and permissions (`Permissions`): `admin` manages users and reads
//!   the activity log, `member` is everyone else (`app::roles`);
//! - the activity log (`Audit`): logins and account changes, and what this
//!   app records with `renox::audit::record`;
//! - a dashboard, the users page and the activity page in the kit's sidebar
//!   layout.
//!
//! Make someone an admin with `cargo run -- users:admin you@example.com`, or
//! seed a local database: `cargo run -- db:seed` (admin@example.com,
//! password123).

mod app;

use renox::audit::Audit;
use renox::auth::{Auth, Permissions};

pub use app::roles;

/// The application: its modules, migrations, seeders and commands.
/// `main.rs` runs it, and tests boot it with `renox::testing::TestApp`.
pub fn app() -> renox::App {
    renox::App::new()
        .embed(renox::embedded!())
        .migrations(renox::migrations!())
        .module(
            Auth::new()
                .verify_email() // a link by mail before the dashboard opens
                .account() // /account: name, email, password, devices
                .notifications() // the bell and /notifications
                .redirect_to("/dashboard")
                // Everyone who signs up is a member.
                .on_registered(|user, _form, state| async move {
                    roles::define(&state.db).await?;
                    user.assign_role(&state.db, roles::MEMBER).await
                }),
        )
        .module(Permissions)
        .module(Audit)
        .module(app::home::Home)
        .module(app::dashboard::Dashboard)
        .module(app::users::Users)
        .module(app::activity::Activity)
        .command(
            "users:admin",
            "Give the user with this email the admin role",
            app::roles::make_admin,
        )
        .seeder(app::seed::run)
        // The seeded people on the login page, in local only (src/app/demo.rs).
        .share("demo_logins", app::demo::for_view)
}

mod app;

/// The application: its modules, migrations and seeders. `main.rs` runs it,
/// and tests boot it with `renox::testing::TestApp`.
pub fn app() -> renox::App {
    renox::App::new()
        .embed(renox::embedded!())
        .migrations(renox::migrations!())
        .module(renox::auth::Auth::new().account()) // login, register, /account
        .module(app::home::Home)
}

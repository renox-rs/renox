//! The `two_factor` table (#169): it migrates on SQLite and PostgreSQL, keeps the
//! secret sealed, and goes with its user.

use renox::db::Encrypted;
use renox::prelude::*;
use renox::testing::TestApp;
use renox_2fa::{TwoFactor, TwoFactorCredential, totp};

async fn app() -> (TestApp, User) {
    let app = TestApp::new(App::new().module(Auth::new()).module(TwoFactor::new())).await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    (app, ana)
}

#[renox::test]
async fn the_secret_is_stored_sealed_and_read_back() {
    let (app, ana) = app().await;
    let secret = totp::new_secret();
    TwoFactorCredential::create(
        app.db(),
        TwoFactorCredential {
            user_id: ana.id,
            secret: Encrypted::new(secret.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // In the table it's sealed with APP_KEY, not the secret itself.
    let raw: String = renox::db::sql("SELECT secret FROM two_factor WHERE user_id = ?")
        .bind(ana.id)
        .scalar(app.db())
        .await
        .unwrap();
    assert!(!raw.contains(&secret), "{raw}");

    let mut credential = TwoFactorCredential::of(app.db(), ana.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(credential.secret.as_str(), secret);
    // Not on until it's confirmed.
    assert!(
        !TwoFactorCredential::enabled(app.db(), ana.id)
            .await
            .unwrap()
    );
    credential.confirmed_at = Some(renox::db::now());
    credential.save(app.db()).await.unwrap();
    assert!(
        TwoFactorCredential::enabled(app.db(), ana.id)
            .await
            .unwrap()
    );
}

#[renox::test]
async fn one_row_per_user_and_it_goes_with_the_user() {
    let (app, mut ana) = app().await;
    let new = |user_id| TwoFactorCredential {
        user_id,
        secret: Encrypted::new(totp::new_secret()),
        ..Default::default()
    };
    TwoFactorCredential::create(app.db(), new(ana.id))
        .await
        .unwrap();
    assert!(
        TwoFactorCredential::create(app.db(), new(ana.id))
            .await
            .is_err()
    );

    ana.delete(app.db()).await.unwrap();
    app.assert_database_count("two_factor", 0).await;
}

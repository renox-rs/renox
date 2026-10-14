//! Customer accounts (#238): signing up makes a customer, a walk-in claims
//! their record once, social login on `FakeHttp`, the account page and its
//! sections, notification preferences deciding the channels, the language
//! on the account, the data export, and deleting an account leaving no
//! personal data while the books keep their totals.

use bikeshop::app::access::catalogue::{CASHIER, MECHANIC};
use bikeshop::app::accounts::factories::{CustomerStates, customers};
use bikeshop::app::accounts::model::{Address, Customer};
use bikeshop::app::accounts::preferences::{Choice, Kind, Preferences, channels_for};
use bikeshop::app::plans::factories::{SubscriptionStates, plan_subscriptions};
use bikeshop::app::plans::model::{PlanSubscription, ServicePlan, SubscriptionStatus};
use bikeshop::app::sales::factories::{OrderStates, orders};
use bikeshop::app::sales::model::Order;
use bikeshop::app::workshop::factories::customer_bikes_of;
use bikeshop::seed::fixtures;
use renox::auth::{Channel, DatabaseMessage, Notification, Recipient};
use renox::http::FakeResponse;
use renox::prelude::*;
use renox::testing::TestApp;

async fn boot() -> TestApp {
    let app = TestApp::new(bikeshop::app()).await;
    fixtures::roles(app.db()).await.unwrap();
    app
}

/// Signs up through the form, as a visitor would.
async fn sign_up(app: &TestApp, name: &str, email: &str) -> User {
    app.post(
        "/register",
        &[
            ("name", name),
            ("email", email),
            ("password", "password123"),
            ("password_confirmation", "password123"),
        ],
    )
    .await;
    User::query()
        .where_eq("email", email)
        .first(app.db())
        .await
        .unwrap()
        .expect("the user was made")
}

// [explain:register.test]
#[renox::test]
async fn signing_up_makes_a_customer() {
    let app = boot().await;
    let user = sign_up(&app, "Nia Lopez", "nia@example.com").await;
    let customer = Customer::of_user(app.db(), user.id)
        .await
        .unwrap()
        .expect("a customer row");
    assert_eq!(customer.name, "Nia Lopez");
    assert_eq!(customer.email.as_deref(), Some("nia@example.com"));
    // The language they signed up in is on the account.
    assert_eq!(user.get::<String>("locale").as_deref(), Some("en"));
    app.assert_authenticated(None);
}
// [/explain:register.test]

#[renox::test]
async fn the_account_page_shows_the_customers_sections() {
    let app = boot().await;
    let user = sign_up(&app, "Nia Lopez", "nia@example.com").await;
    app.acting_as(&user);
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("Contact details")
        .assert_see("ID check")
        .assert_see("Not on file")
        .assert_see("News and offers")
        .assert_see("value=\"in_app\"")
        .assert_see("Download my data")
        .assert_see("Two-factor authentication")
        // The about panel resolves this page.
        .assert_see("My account");
}

#[renox::test]
async fn contact_details_and_address_are_saved() {
    let app = boot().await;
    let user = sign_up(&app, "Nia Lopez", "nia@example.com").await;
    let store = fixtures::store(app.db(), "North").await.unwrap();
    let address = Address::find(app.db(), store.address_id)
        .await
        .unwrap()
        .unwrap();
    let city = bikeshop::app::accounts::model::City::find(app.db(), address.city_id)
        .await
        .unwrap()
        .unwrap();
    app.acting_as(&user);
    let country = city.country_id.to_string();
    app.put(
        "/account/contact",
        &[
            ("phone", "+34 600 000 000"),
            ("line1", "Calle Mayor 1"),
            ("line2", ""),
            ("district", ""),
            ("postal_code", "28013"),
            ("city", "Madrid"),
            ("country_id", country.as_str()),
        ],
    )
    .await
    .assert_redirect("/account");
    let customer = Customer::of_user(app.db(), user.id).await.unwrap().unwrap();
    assert_eq!(customer.phone.as_deref(), Some("+34 600 000 000"));
    let home = Address::find(app.db(), customer.address_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(home.line1, "Calle Mayor 1");
    app.get("/account").await.assert_see("Calle Mayor 1");

    // A missing street comes back with an error.
    app.htmx()
        .put(
            "/account/contact",
            &[
                ("line1", ""),
                ("city", "Madrid"),
                ("country_id", country.as_str()),
            ],
        )
        .await
        .assert_invalid("line1");
}

/// `text` HTML-escaped the way Renox's views escape it.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Mails the invitation for `customer` as a cashier and returns the link's path.
async fn invite(app: &TestApp, customer: &Customer, email: &str) -> String {
    let north = fixtures::store(app.db(), "North").await.unwrap();
    let cashier = fixtures::person(
        app.db(),
        "cashier@example.com",
        &[(CASHIER, Some(north.id))],
    )
    .await
    .unwrap();
    app.acting_as(&cashier);
    let page = format!("/staff/customers/{}/invite", customer.id);
    // As the page writes it: a fake name can be "Ethel O'Connell".
    app.get(&page)
        .await
        .assert_ok()
        .assert_see(&escaped(&customer.name));
    app.post(&page, &[("email", email)])
        .await
        .assert_redirect(&page);
    app.run_jobs().await;
    let mail = app
        .sent_mail()
        .into_iter()
        .find(|m| m.to.contains(&email.to_owned()))
        .expect("the invitation was mailed");
    let html = mail.html.unwrap_or_default();
    let start = html.find("/claim/").expect("a claim link");
    let end = html[start..]
        .find('"')
        .map(|n| start + n)
        .unwrap_or(html.len());
    html[start..end].replace("&amp;", "&")
}

#[renox::test]
async fn a_walk_in_claims_their_record_once() {
    let app = boot().await;
    let north = fixtures::store(app.db(), "North").await.unwrap();
    let walk_in = customers().walk_in().create_one(app.db()).await.unwrap();
    let past = orders()
        .at(north.id)
        .for_customer(walk_in.id)
        .paid()
        .create_one(app.db())
        .await
        .unwrap();

    // Only staff with customers.manage may invite.
    let mechanic = fixtures::person(app.db(), "mech@example.com", &[(MECHANIC, Some(north.id))])
        .await
        .unwrap();
    app.acting_as(&mechanic);
    app.get(&format!("/staff/customers/{}/invite", walk_in.id))
        .await
        .assert_forbidden();

    let link = invite(&app, &walk_in, "ana@example.com").await;
    app.logout();

    // A guest is sent to log in first.
    app.get(&link).await.assert_redirect("/login");

    // Someone else's login can't claim it.
    let other = sign_up(&app, "Other", "other@example.com").await;
    app.acting_as(&other);
    app.get(&link)
        .await
        .assert_ok()
        .assert_see("This invitation was sent to ana@example.com");
    app.post(&link, &[]).await.assert_forbidden();

    // A changed link is refused.
    let tampered = link.replace(
        &format!("/claim/{}/", walk_in.id),
        &format!("/claim/{}/", walk_in.id + 1),
    );
    assert_ne!(tampered, link);
    app.get(&tampered).await.assert_forbidden();

    // Ana signs up (an order placed online before claiming moves over too).
    app.logout();
    let ana = sign_up(&app, "Ana", "ana@example.com").await;
    let own = Customer::of_user(app.db(), ana.id).await.unwrap().unwrap();
    let online = orders()
        .at(north.id)
        .for_customer(own.id)
        .create_one(app.db())
        .await
        .unwrap();
    app.acting_as(&ana);
    app.get(&link).await.assert_ok().assert_see("This is me");
    app.post(&link, &[]).await.assert_redirect("/account");

    let claimed = Customer::of_user(app.db(), ana.id).await.unwrap().unwrap();
    assert_eq!(claimed.id, walk_in.id, "the walk-in record is hers now");
    assert_eq!(claimed.email.as_deref(), Some("ana@example.com"));
    for order in [past.id, online.id] {
        let order = Order::find(app.db(), order).await.unwrap().unwrap();
        assert_eq!(order.customer_id, Some(walk_in.id));
    }
    assert!(Customer::find(app.db(), own.id).await.unwrap().is_none());

    // Once only.
    app.post(&link, &[]).await.assert_not_found();
    app.get(&link)
        .await
        .assert_see("already linked to an account");
}

#[renox::test]
async fn social_login_makes_a_customer_on_fake_http() {
    let app = TestApp::with_config(bikeshop::app(), |config| {
        config.vars.insert("GOOGLE_CLIENT_ID".into(), "id".into());
        config
            .vars
            .insert("GOOGLE_CLIENT_SECRET".into(), "secret".into());
    })
    .await;
    // The buttons show only for providers with keys.
    app.get("/login")
        .await
        .assert_see("/auth/google/redirect")
        .assert_dont_see("/auth/github/redirect");

    let http = app.fake_http();
    http.on(
        "POST https://oauth2.googleapis.com/token",
        FakeResponse::json(200, json!({ "access_token": "token" })),
    );
    http.on(
        "https://openidconnect.googleapis.com/v1/userinfo",
        FakeResponse::json(
            200,
            json!({ "sub": "42", "email": "leo@example.com", "email_verified": true, "name": "Leo" }),
        ),
    );
    let to_google = app.get("/auth/google/redirect").await;
    let location = to_google.header("location").unwrap();
    assert!(location.contains("code_challenge="), "PKCE");
    let state = location
        .split(['?', '&'])
        .find_map(|pair| pair.strip_prefix("state="))
        .unwrap()
        .to_owned();
    app.get(&format!("/auth/google/callback?code=a-code&state={state}"))
        .await
        .assert_redirect("/");
    app.assert_authenticated(None);
    let user = User::query()
        .where_eq("email", "leo@example.com")
        .first(app.db())
        .await
        .unwrap()
        .unwrap();
    assert!(!user.has_password());
    let customer = Customer::of_user(app.db(), user.id).await.unwrap();
    assert!(
        customer.is_some(),
        "a first social login makes a customer too"
    );
}

#[renox::test]
async fn without_keys_there_are_no_social_buttons() {
    let app = boot().await;
    app.get("/login")
        .await
        .assert_ok()
        .assert_dont_see("/auth/google/redirect");
    app.get("/auth/google/redirect").await.assert_not_found();
}

/// A notification of one kind, as the other areas write them.
struct Ping(Kind);

impl Notification for Ping {
    fn kind(&self) -> &'static str {
        "ping"
    }

    fn channels(&self, to: &Recipient) -> Vec<Channel> {
        channels_for(to, self.0)
    }

    fn to_mail(&self, to: &Recipient, _state: &AppState) -> Result<renox::mail::Mail> {
        Ok(renox::mail::Mail::new(
            to.email().unwrap_or_default(),
            format!("Ping {}", self.0.key()),
            "ping",
        ))
    }

    fn to_database(&self, _to: &Recipient, _state: &AppState) -> Result<renox::serde_json::Value> {
        Ok(DatabaseMessage::info(format!("Ping {}", self.0.key())).into())
    }
}

#[renox::test]
async fn preferences_decide_the_channels() {
    let app = boot().await;
    let user = sign_up(&app, "Nia", "nia@example.com").await;

    // The defaults: both for her own business, nothing for marketing.
    let to = Recipient::for_user(&user);
    assert_eq!(
        channels_for(&to, Kind::Order),
        vec![Channel::Mail, Channel::Database]
    );
    assert!(channels_for(&to, Kind::Marketing).is_empty());
    // A walk-in mailed at an address: mail, never marketing.
    let walk_in = Recipient::to("mail", "walkin@example.com");
    assert_eq!(channels_for(&walk_in, Kind::Rental), vec![Channel::Mail]);
    assert!(channels_for(&walk_in, Kind::Marketing).is_empty());

    app.acting_as(&user);
    app.put(
        "/account/notifications",
        &[
            ("order", "mail"),
            ("rental", "in_app"),
            ("workshop", "none"),
            ("plan", "both"),
            ("marketing", "both"),
        ],
    )
    .await
    .assert_redirect("/account");
    let user = User::find(app.db(), user.id).await.unwrap().unwrap();
    let saved = Preferences::of(&user);
    assert_eq!(saved.choice(Kind::Order), Choice::Mail);
    assert_eq!(saved.choice(Kind::Rental), Choice::InApp);
    assert_eq!(saved.choice(Kind::Workshop), Choice::None);

    // [explain:notifications.test]
    // Sent for real: mail only, the bell only, nothing.
    let state = app.state();
    for kind in [Kind::Order, Kind::Rental, Kind::Workshop] {
        state.notify(&user, &Ping(kind)).await.unwrap();
    }
    app.run_jobs().await;
    let subjects: Vec<String> = app.sent_mail().into_iter().map(|m| m.subject).collect();
    assert!(subjects.contains(&"Ping order".to_owned()));
    assert!(!subjects.contains(&"Ping rental".to_owned()));
    assert!(!subjects.contains(&"Ping workshop".to_owned()));
    let stored: Vec<String> = renox::db::sql("SELECT data FROM notifications WHERE user_id = ?")
        .bind(user.id)
        .scalars(app.db())
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert!(stored[0].contains("Ping rental"));
    // [/explain:notifications.test]

    // An unknown choice is refused.
    app.htmx()
        .put(
            "/account/notifications",
            &[
                ("order", "pigeon"),
                ("rental", "mail"),
                ("workshop", "mail"),
                ("plan", "mail"),
                ("marketing", "none"),
            ],
        )
        .await
        .assert_invalid("order");
}

/// Saves `user`'s choices through the account page.
async fn choose(app: &TestApp, user: &User, choices: [(&str, &str); 5]) {
    app.acting_as(user);
    app.put("/account/notifications", &choices)
        .await
        .assert_redirect("/account");
}

/// The kinds of `user`'s in-app notifications, oldest first.
async fn in_app(app: &TestApp, user: &User) -> Vec<String> {
    renox::db::sql("SELECT kind FROM notifications WHERE user_id = ? ORDER BY id")
        .bind(user.id)
        .scalars(app.db())
        .await
        .unwrap()
}

/// How many mails went to `email` (after running the queued ones).
async fn mails_to(app: &TestApp, email: &str) -> usize {
    app.run_jobs().await;
    app.sent_mail()
        .iter()
        .filter(|m| m.to.iter().any(|t| t.contains(email)))
        .count()
}

#[renox::test]
async fn every_area_asks_the_customers_preferences() {
    use bikeshop::app::rentals::factories::{RentalStates, rentals};
    use bikeshop::app::sales::notify::{Moment, tell};
    use bikeshop::app::workshop::factories::{WorkOrderStates, work_orders};

    let app = boot().await;
    let db = app.db();
    let state = app.state();
    let north = fixtures::store(db, "North").await.unwrap();
    let email = "nia@example.com";
    let user = sign_up(&app, "Nia", email).await;
    let customer = Customer::of_user(db, user.id).await.unwrap().unwrap();
    choose(
        &app,
        &user,
        [
            ("order", "in_app"),
            ("rental", "mail"),
            ("workshop", "none"),
            ("plan", "both"),
            ("marketing", "none"),
        ],
    )
    .await;
    let mails = mails_to(&app, email).await;

    // Rentals by mail only: the reminder is mailed, the bell stays quiet.
    let bike = fixtures::bike(db, north.id, north.id).await.unwrap();
    let mut rental = rentals()
        .of_bike(&bike)
        .for_customer(customer.id)
        .active()
        .create_one(db)
        .await
        .unwrap();
    rental.due_at = renox::db::now() + renox::chrono::Duration::minutes(30);
    rental.save(db).await.unwrap();
    bikeshop::app::rentals::tasks::reminders(state)
        .await
        .unwrap();
    assert_eq!(mails_to(&app, email).await, mails + 1, "rentals: mail");
    assert!(in_app(&app, &user).await.is_empty(), "rentals: no bell");

    // The workshop not at all.
    let bike = customer_bikes_of(customer.id).create_one(db).await.unwrap();
    let order = work_orders()
        .at(north.id)
        .on_bike(bike.id)
        .waiting_parts()
        .create_one(db)
        .await
        .unwrap();
    bikeshop::app::workshop::status::tell_customer(state, &order)
        .await
        .unwrap();
    assert_eq!(mails_to(&app, email).await, mails + 1, "workshop: no mail");
    assert!(in_app(&app, &user).await.is_empty(), "workshop: no bell");

    // Orders in the app only: the bell rings, no confirmation mail.
    let order = orders()
        .at(north.id)
        .for_customer(customer.id)
        .paid()
        .create_one(db)
        .await
        .unwrap();
    tell(state, &order, Moment::Paid, None).await.unwrap();
    assert_eq!(mails_to(&app, email).await, mails + 1, "orders: no mail");
    assert_eq!(in_app(&app, &user).await, vec!["order-paid".to_owned()]);

    // Rentals switched off: the next reminder goes nowhere.
    choose(
        &app,
        &user,
        [
            ("order", "in_app"),
            ("rental", "none"),
            ("workshop", "none"),
            ("plan", "both"),
            ("marketing", "none"),
        ],
    )
    .await;
    let bike = fixtures::bike(db, north.id, north.id).await.unwrap();
    let mut rental = rentals()
        .of_bike(&bike)
        .for_customer(customer.id)
        .active()
        .create_one(db)
        .await
        .unwrap();
    rental.due_at = renox::db::now() + renox::chrono::Duration::minutes(30);
    rental.save(db).await.unwrap();
    assert_eq!(
        bikeshop::app::rentals::tasks::reminders(state)
            .await
            .unwrap(),
        1
    );
    assert_eq!(mails_to(&app, email).await, mails + 1, "rentals: none");
    assert_eq!(in_app(&app, &user).await, vec!["order-paid".to_owned()]);
}

#[renox::test]
async fn the_language_is_kept_on_the_account() {
    let app = boot().await;
    let user = sign_up(&app, "Nia", "nia@example.com").await;
    app.acting_as(&user);
    app.put("/account/language", &[("locale", "es")])
        .await
        .assert_redirect("/account");
    let user = User::find(app.db(), user.id).await.unwrap().unwrap();
    assert_eq!(user.get::<String>("locale").as_deref(), Some("es"));
    app.get("/account").await.assert_see("Datos de contacto");

    // Another device: a fresh session picks the account's language.
    app.logout();
    app.acting_as(&user);
    app.get("/account").await;
    app.get("/account").await.assert_see("Datos de contacto");

    // The language menu changes the account too.
    app.post("/locale/en", &[]).await;
    app.get("/account").await.assert_see("Contact details");
    let user = User::find(app.db(), user.id).await.unwrap().unwrap();
    assert_eq!(user.get::<String>("locale").as_deref(), Some("en"));
}

#[renox::test]
async fn download_my_data_mails_a_link_in_the_customers_language() {
    let app = boot().await;
    let north = fixtures::store(app.db(), "North").await.unwrap();
    let user = sign_up(&app, "Nia", "nia@example.com").await;
    let customer = Customer::of_user(app.db(), user.id).await.unwrap().unwrap();
    let order = orders()
        .at(north.id)
        .for_customer(customer.id)
        .paid()
        .create_one(app.db())
        .await
        .unwrap();
    app.acting_as(&user);
    app.put("/account/language", &[("locale", "es")]).await;
    app.post("/account/export", &[])
        .await
        .assert_redirect("/account");
    let exported = |mail: &renox::mail::Mail| mail.subject.contains("datos");
    assert!(!app.sent_mail().iter().any(exported), "the work is queued");
    app.run_jobs().await;
    let mail = app
        .sent_mail()
        .into_iter()
        .find(|m| m.to.contains(&"nia@example.com".to_owned()) && exported(m))
        .expect("the link was mailed, in Spanish");
    assert!(
        mail.text.contains("/_renox/files/exports/"),
        "{}",
        mail.text
    );

    let files = app.state().storage.list("exports/").await.unwrap();
    assert_eq!(files.len(), 1);
    let json = app
        .state()
        .storage
        .get(&files[0].key)
        .await
        .unwrap()
        .unwrap();
    let json = String::from_utf8(json.to_vec()).unwrap();
    assert!(json.contains(&order.number));
    assert!(json.contains("nia@example.com"));
}

#[renox::test]
async fn deleting_the_account_leaves_no_personal_data_and_keeps_the_books() {
    let app = boot().await;
    let north = fixtures::store(app.db(), "North").await.unwrap();
    let user = sign_up(&app, "Nia Lopez", "nia@example.com").await;
    let mut customer = Customer::of_user(app.db(), user.id).await.unwrap().unwrap();
    // Her address, ID number, a paid order, a bike on a plan, her ID photo.
    let store_address = Address::find(app.db(), north.address_id)
        .await
        .unwrap()
        .unwrap();
    let home = Address::create(
        app.db(),
        Address {
            city_id: store_address.city_id,
            line1: "Calle Secreta 7".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    customer.address_id = Some(home.id);
    customer.phone = Some("+34 611 111 111".into());
    customer.id_number = Some("X1234567L".to_owned().into());
    customer.id_verified_at = Some(renox::db::now());
    customer.save(app.db()).await.unwrap();
    let order = orders()
        .at(north.id)
        .for_customer(customer.id)
        .totalling(125_000)
        .paid()
        .create_one(app.db())
        .await
        .unwrap();
    let bike = customer_bikes_of(customer.id)
        .create_one(app.db())
        .await
        .unwrap();
    let plan = ServicePlan::factory().create_one(app.db()).await.unwrap();
    let subscription = plan_subscriptions()
        .of(bike.id, plan.id, north.id)
        .create_one(app.db())
        .await
        .unwrap();
    let photo = format!("customers/{}/id-front.jpg", customer.id);
    app.state()
        .storage
        .put(&photo, b"jpeg".to_vec().into())
        .await
        .unwrap();
    // A walk-in record of someone else stays as it is.
    let stranger = customers().verified().create_one(app.db()).await.unwrap();

    app.acting_as(&user);
    app.post(
        "/account",
        &[("_method", "DELETE"), ("password", "password123")],
    )
    .await
    .assert_redirect("/");
    app.assert_guest();
    assert!(User::find(app.db(), user.id).await.unwrap().is_none());

    let gone = Customer::query()
        .with_trashed()
        .where_(Customer::ID.eq(customer.id))
        .first(app.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gone.name, format!("Deleted customer #{}", customer.id));
    assert!(gone.email.is_none() && gone.phone.is_none() && gone.user_id.is_none());
    assert!(gone.id_number.is_none() && gone.id_verified_at.is_none());
    assert!(gone.address_id.is_none() && gone.deleted_at.is_some());
    assert!(Address::find(app.db(), home.id).await.unwrap().is_none());
    assert!(!app.state().storage.exists(&photo).await.unwrap());
    // Nothing of hers is left in the table, in any column. Her full name and
    // number: the stranger's fake ones may hold "Nia" or "611".
    let raw: Vec<String> = renox::db::sql(
        "SELECT COALESCE(name, '') || COALESCE(email, '') || COALESCE(phone, '') FROM customers",
    )
    .scalars(app.db())
    .await
    .unwrap();
    assert!(raw.iter().all(|r| !r.contains("Nia Lopez") && !r.contains("611 111 111")));

    // The books keep the order and its total, on the anonymous record.
    let kept = Order::find(app.db(), order.id).await.unwrap().unwrap();
    assert_eq!(kept.total, 125_000);
    assert_eq!(kept.customer_id, Some(customer.id));
    // The plan is cancelled.
    let subscription = PlanSubscription::find(app.db(), subscription.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(subscription.status, SubscriptionStatus::Cancelled);
    // Someone else's record is untouched.
    let stranger_now = Customer::find(app.db(), stranger.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stranger_now.name, stranger.name);
    assert!(stranger_now.id_number.is_some());
    // Recorded in the audit log.
    let erased: i64 =
        renox::db::sql("SELECT COUNT(*) FROM audit_logs WHERE action = 'customer.erased'")
            .scalar(app.db())
            .await
            .unwrap();
    assert_eq!(erased, 1);
}

// [explain:login.test]
#[renox::test]
async fn logins_are_throttled_and_locked() {
    let app = boot().await;
    sign_up(&app, "Nia", "nia@example.com").await;
    app.logout();
    for _ in 0..5 {
        app.post(
            "/login",
            &[("email", "nia@example.com"), ("password", "wrong-password")],
        )
        .await;
    }
    // Even the right password waits now.
    app.post(
        "/login",
        &[("email", "nia@example.com"), ("password", "password123")],
    )
    .await;
    app.assert_guest();
}
// [/explain:login.test]

/// The address of the last mail's link that starts with `path`, without
/// the app's host: `/reset-password/…`, `/verify-email/…`.
fn mailed_link(app: &TestApp, path: &str) -> String {
    let mail = app.sent_mail().pop().expect("a mail was sent");
    let at = mail.text.find(path).expect("the mail has the link");
    mail.text[at..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

// [explain:password.request.test]
#[renox::test]
async fn a_forgotten_password_is_chosen_again_from_the_mailed_link() {
    let app = boot().await;
    sign_up(&app, "Nia", "nia@example.com").await;
    app.logout();
    // The same answer whether the address has an account or not.
    app.post("/forgot-password", &[("email", "NIA@example.com")])
        .await
        .assert_redirect("/forgot-password");
    let link = mailed_link(&app, "/reset-password/");
    app.get(&link)
        .await
        .assert_ok()
        .assert_see("nia@example.com");
    // [/explain:password.request.test]
    // [explain:password.reset.test]
    let token = link["/reset-password/".len()..].split('?').next().unwrap();
    app.post(
        "/reset-password",
        &[
            ("token", token),
            ("email", "nia@example.com"),
            ("password", "a-new-password-9"),
            ("password_confirmation", "a-new-password-9"),
        ],
    )
    .await
    .assert_redirect("/login");
    app.post(
        "/login",
        &[
            ("email", "nia@example.com"),
            ("password", "a-new-password-9"),
        ],
    )
    .await;
    app.assert_authenticated(None);
}
// [/explain:password.reset.test]

// [explain:password.confirm.test]
#[renox::test]
async fn turning_two_factor_on_asks_for_the_password_again() {
    let app = boot().await;
    let user = sign_up(&app, "Nia", "nia@example.com").await;
    app.logout();
    app.acting_as(&user); // logged in, but no password typed lately
    app.get("/two-factor/setup")
        .await
        .assert_redirect("/confirm-password");
    app.get("/confirm-password")
        .await
        .assert_ok()
        .assert_see("Confirm your password");
    app.post("/confirm-password", &[("password", "password123")])
        .await
        .assert_redirect("/two-factor/setup");
}
// [/explain:password.confirm.test]

// [explain:verification.notice.test]
#[renox::test]
async fn a_new_account_verifies_its_email_from_a_signed_link() {
    let app = boot().await;
    let user = sign_up(&app, "Nia", "nia@example.com").await;
    assert!(user.email_verified_at.is_none());
    app.get("/verify-email").await.assert_ok();
    // Another link, signed and valid for an hour.
    app.post("/email/verification-notification", &[]).await;
    let link = mailed_link(&app, "/verify-email/");
    assert!(link.contains("&signature="), "{link}");
    app.get(&link).await.assert_redirect("/");
    let user = User::find(app.db(), user.id).await.unwrap().unwrap();
    assert!(user.email_verified_at.is_some());
    // Nothing left to verify.
    app.get("/verify-email").await.assert_redirect("/");
}
// [/explain:verification.notice.test]

/// Staff reach a walk-in customer's invitation from the pages that show the
/// customer: the rental desk and the work order (#243).
#[renox::test]
async fn staff_pages_link_a_walk_in_customer_to_the_invitation() {
    let app = TestApp::with_config(bikeshop::app(), |c| {
        c.vars
            .insert("BIKESHOP_STAFF_2FA".into(), "optional".into());
    })
    .await;
    bikeshop::seed::run(app.state().clone()).await.unwrap();
    let owner = User::find_by_email(app.db(), "owner@bikeshop.test")
        .await
        .unwrap()
        .unwrap();
    app.acting_as(&owner);

    // A work order on a walk-in's bike, and one on a registered customer's.
    let pick = |claimed: bool| {
        format!(
            "SELECT w.id, w.store_id, c.id FROM work_orders w \
             JOIN customer_bikes b ON b.id = w.customer_bike_id \
             JOIN customers c ON c.id = b.customer_id \
             WHERE c.user_id IS {} NULL AND c.deleted_at IS NULL ORDER BY w.id LIMIT 1",
            if claimed { "NOT" } else { "" }
        )
    };
    for claimed in [false, true] {
        let (order, store, customer): (i64, i64, i64) = renox::db::sql(pick(claimed))
            .fetch_as::<(i64, i64, i64)>(app.db())
            .await
            .unwrap()
            .pop()
            .expect("the seed has both kinds");
        app.post(&format!("/staff/store/{store}"), &[]).await;
        let page = app.get(&format!("/staff/workshop/{order}")).await;
        page.assert_ok();
        let link = format!("/staff/customers/{customer}/invite");
        if claimed {
            page.assert_dont_see(&link);
        } else {
            page.assert_see(&link);
            app.get(&link).await.assert_ok();
        }
    }
}

//! The rest of the parity review's small adds, closed in M34:
//! `current_password`, `Password::uncompromised`, session `keep`/`now`,
//! named error bags, named mailers with failover, `has_many_through`.

use renox::auth::User;
use renox::http::FakeResponse;
use renox::mail::{Mail, MailConfig};
use renox::testing::TestApp;
use renox::validation::Password;
use serde::{Deserialize, Serialize};

use renox::prelude::*;

#[derive(Deserialize, Serialize)]
struct ChangeEmail {
    email: String,
    current_password: String,
}

impl Validate for ChangeEmail {
    fn rules(&self, v: &mut Validator) {
        v.field("email", &self.email).required().email();
        v.field("current_password", &self.current_password)
            .required()
            .current_password();
    }
}

#[derive(Deserialize, Serialize)]
struct SignUp {
    password: String,
}

impl Validate for SignUp {
    fn rules(&self, v: &mut Validator) {
        let policy = Password::min(8).uncompromised();
        v.field("password", &self.password)
            .required()
            .password(&policy);
    }
}

struct Forms;

impl Module for Forms {
    fn name(&self) -> &'static str {
        "forms"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .post("/sign-up", |Valid(_): Valid<SignUp>| async { "welcome" })
            .post("/email", |Valid(form): Valid<ChangeEmail>| async move {
                format!("now {}", form.email)
            })
    }
}

#[renox::test]
async fn current_password_checks_the_logged_in_users_password() {
    let app = TestApp::new(App::new().module(Auth::new()).module(Forms)).await;
    let user = User::register(app.db(), "Alex", "alex@example.com", "password123")
        .await
        .unwrap();
    // Nobody logged in: there's no password to match.
    app.htmx()
        .post(
            "/email",
            &[
                ("email", "new@example.com"),
                ("current_password", "password123"),
            ],
        )
        .await
        .assert_invalid("current_password");
    app.acting_as(&user);
    let wrong = app
        .htmx()
        .post(
            "/email",
            &[("email", "new@example.com"), ("current_password", "nope")],
        )
        .await;
    wrong.assert_invalid("current_password");
    assert_eq!(
        wrong.json_path("errors.current_password.0"),
        "The current password is incorrect."
    );
    app.htmx()
        .post(
            "/email",
            &[
                ("email", "new@example.com"),
                ("current_password", "password123"),
            ],
        )
        .await
        .assert_ok()
        .assert_see("now new@example.com");
}

#[renox::test]
async fn uncompromised_passwords_ask_have_i_been_pwned_by_hash_prefix() {
    let app = TestApp::new(App::new().module(Forms)).await;
    let http = app.fake_http();
    // SHA-1 of "password" is 5BAA6 1E4C9B93F3F0682250B6CF8331B7EE68FD8: only
    // the prefix is in the URL; the answer lists suffixes and counts.
    http.on(
        "https://api.pwnedpasswords.com/range/5BAA6",
        FakeResponse::text(
            200,
            "0018A45C4D1DEF81644B54AB7F969B88D65:0\r\n1E4C9B93F3F0682250B6CF8331B7EE68FD8:9545824\r\n",
        ),
    );
    http.on(
        "https://api.pwnedpasswords.com/range/*",
        FakeResponse::text(200, "0018A45C4D1DEF81644B54AB7F969B88D65:2\r\n"),
    );
    let leaked = app
        .htmx()
        .post("/sign-up", &[("password", "password")])
        .await;
    leaked.assert_invalid("password");
    assert_eq!(
        leaked.json_path("errors.password.0"),
        "The password has appeared in a data leak. Please choose a different one."
    );
    app.htmx()
        .post("/sign-up", &[("password", "a long and unusual one")])
        .await
        .assert_ok();
    // Short ones fail the length rule and never reach the service.
    let short = app.htmx().post("/sign-up", &[("password", "pass")]).await;
    assert!(
        short
            .json_path("errors.password.0")
            .as_str()
            .unwrap()
            .contains("at least 8")
    );
    let asked: Vec<String> = http.sent().iter().map(|r| r.url.clone()).collect();
    assert_eq!(asked.len(), 2, "{asked:?}");
    // Only five characters of the hash leave the server.
    assert!(
        asked
            .iter()
            .all(|url| url.rsplit('/').next().unwrap().len() == 5),
        "{asked:?}"
    );

    // When the service can't be reached, the password is allowed.
    let offline = TestApp::new(App::new().module(Forms)).await;
    offline.fake_http().on(
        "https://api.pwnedpasswords.com/*",
        FakeResponse::status(503),
    );
    offline
        .htmx()
        .post("/sign-up", &[("password", "password")])
        .await
        .assert_ok();
}

struct Flash;

impl Module for Flash {
    fn name(&self) -> &'static str {
        "flash"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/save", |session: Session| async move {
                session.flash("status", "Saved.").unwrap();
                session.flash("note", "Only once.").unwrap();
                "ok"
            })
            .get("/hop", |session: Session| async move {
                session.keep(&["status"]);
                session.get::<String>("status").unwrap_or_default()
            })
            .get("/read", |session: Session| async move {
                format!(
                    "status={:?} note={:?}",
                    session.get::<String>("status"),
                    session.get::<String>("note")
                )
            })
            .get("/now", |session: Session| async move {
                session.flash_now("status", "Shown now.").unwrap();
                view("flash.html", context! {})
            })
    }
}

#[renox::test]
async fn keep_holds_some_flashes_and_now_shows_one_on_this_page() {
    let views = tempfile::tempdir().unwrap();
    std::fs::write(views.path().join("flash.html"), "[{{ flash.status }}]").unwrap();
    let path = views.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Flash), |c| c.views_path = path).await;
    app.get("/save").await.assert_ok();
    app.get("/hop").await.assert_see("Saved.");
    // `status` was kept for one more request, `note` wasn't.
    app.get("/read")
        .await
        .assert_see("status=Some(\"Saved.\") note=None");
    app.get("/read").await.assert_see("status=None");

    app.get("/now").await.assert_see("[Shown now.]");
    app.get("/read").await.assert_see("status=None");
}

#[derive(Deserialize, Serialize, renox::Validate)]
#[validate(bag = "login")]
struct Login {
    #[validate(required, email)]
    email: String,
}

#[derive(Deserialize, Serialize, renox::Validate)]
struct Newsletter {
    #[validate(required, email)]
    email: String,
}

struct Bags;

impl Module for Bags {
    fn name(&self) -> &'static str {
        "bags"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { view("two-forms.html", context! {}) })
            .post("/login", |Valid(_): Valid<Login>| async { "in" })
            .post("/newsletter", |Valid(_): Valid<Newsletter>| async {
                "subscribed"
            })
    }
}

#[renox::test]
async fn error_bags_keep_two_forms_apart() {
    let views = tempfile::tempdir().unwrap();
    std::fs::write(
        views.path().join("two-forms.html"),
        r#"{% import "renox/ui.html" as ui %}
login:[{{ error('email', bag='login') }}] news:[{{ error('email') }}]
bag:[{{ errors_in('login') | length }}] default:[{{ errors | length }}]
{{ ui.input("email", "Email", bag="login") }}"#,
    )
    .unwrap();
    let path = views.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Bags), |c| c.views_path = path).await;

    // A plain post: redirected back with the errors in the `login` bag.
    app.request()
        .header("referer", "/")
        .post("/login", &[("email", "nope")])
        .await
        .assert_status(303);
    let page = app.get("/").await;
    page.assert_see("login:[The email must be a valid email address.]")
        .assert_see("news:[]")
        .assert_see("bag:[1] default:[0]")
        .assert_see("aria-invalid=\"true\"");

    // The newsletter form's errors stay in the default bag.
    app.request()
        .header("referer", "/")
        .post("/newsletter", &[("email", "")])
        .await
        .assert_status(303);
    let page = app.get("/").await;
    page.assert_see("login:[]")
        .assert_see("news:[The email field is required.]")
        .assert_see("bag:[0] default:[1]")
        .assert_dont_see("aria-invalid=\"true\"");

    // htmx gets the usual 422 whatever the bag.
    app.htmx()
        .post("/login", &[("email", "nope")])
        .await
        .assert_invalid("email");
}

#[renox::test]
async fn named_mailers_and_failover() {
    let app = TestApp::with_config(
        App::new()
            .mailer("newsletter", |_| {
                let mut news = MailConfig::default();
                news.mailer = renox::mail::MailDriver::Memory;
                Ok(news)
            })
            .mailer("backup", |config| {
                let mut backup = MailConfig::from_env(config, "BACKUP")?;
                backup.mailer = renox::mail::MailDriver::Memory;
                Ok(backup)
            }),
        |c| {
            // The default mailer: an SMTP server that isn't there.
            c.mail.mailer = renox::mail::MailDriver::Smtp;
            c.mail.host = "127.0.0.1".into();
            c.mail.port = Some(9);
            c.mail.encryption = renox::mail::MailEncryption::None;
            c.mail.timeout = std::time::Duration::from_secs(2);
            c.mail.failover = vec!["backup".into()];
        },
    )
    .await;
    let state = app.state();

    // The default mailer fails, so `backup` sends it; the mail is the app's.
    state
        .mailer
        .send(Mail::new("ann@example.com", "Receipt", "Thanks."))
        .await
        .unwrap();
    assert_eq!(app.sent_mail().len(), 1);
    assert_eq!(app.sent_mail()[0].subject, "Receipt");

    // A mailer by name, now or through the queue.
    let newsletter = state.mailer_named("newsletter").unwrap();
    newsletter
        .send(Mail::new("ben@example.com", "October", "News."))
        .await
        .unwrap();
    state
        .queue_mail_via(
            "newsletter",
            Mail::new("cid@example.com", "November", "News."),
        )
        .await
        .unwrap();
    app.run_jobs().await;
    let subjects: Vec<String> = newsletter
        .sent()
        .iter()
        .map(|m| m.subject.clone())
        .collect();
    assert_eq!(subjects, ["October", "November"]);
    assert!(state.mailer_named("nope").is_err());
    assert!(
        state
            .queue_mail_via("nope", Mail::new("a@b.co", "x", "y"))
            .await
            .is_err()
    );
}

#[renox::test]
async fn failover_must_name_a_mailer() {
    let mut config = Config::default();
    config.database_url = "sqlite::memory:".into();
    config.mail.failover = vec!["missing".into()];
    let err = match App::with_config(config).boot().await {
        Ok(_) => panic!("booted"),
        Err(err) => format!("{err:?}"),
    };
    assert!(err.contains("MAIL_FAILOVER names `missing`"), "{err}");
}

#[test]
fn mail_config_from_env_falls_back_to_the_apps_sender() {
    let mut config = Config::default();
    config.mail.from_address = "shop@example.com".into();
    config
        .vars
        .insert("NEWS_HOST".into(), "smtp.news.test".into());
    config.vars.insert("NEWS_PORT".into(), "2525".into());
    config.mail.mailer = renox::mail::MailDriver::Log;
    let news = MailConfig::from_env(&config, "news").unwrap();
    assert_eq!(
        news.mailer,
        renox::mail::MailDriver::Log,
        "the app's driver unless NEWS_MAILER says"
    );
    assert_eq!(news.host, "smtp.news.test");
    assert_eq!(news.port, Some(2525));
    assert_eq!(news.from_address, "shop@example.com");
}

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "country")]
struct Country {
    id: i64,
    name: String,
}

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "customer")]
struct Customer {
    id: i64,
    country_id: i64,
}

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "sale")]
struct Sale {
    id: i64,
    customer_id: i64,
    total: i64,
}

#[renox::test]
async fn has_many_through_reaches_grandchildren_in_two_queries() {
    use renox::db::relations::has_many_through;
    let app = TestApp::new(App::new()).await;
    for sql in [
        "CREATE TABLE country (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE customer (id BIGINT PRIMARY KEY, country_id BIGINT NOT NULL)",
        "CREATE TABLE sale (id BIGINT PRIMARY KEY, customer_id BIGINT NOT NULL, total BIGINT NOT NULL)",
        "INSERT INTO country (id, name) VALUES (1, 'Indonesia'), (2, 'Spain'), (3, 'Chile')",
        "INSERT INTO customer (id, country_id) VALUES (10, 1), (11, 1), (20, 2)",
        "INSERT INTO sale (id, customer_id, total) VALUES (100, 10, 5), (101, 11, 7), (102, 20, 9), (103, 10, 1)",
    ] {
        renox::db::sql(sql).execute(app.db()).await.unwrap();
    }
    let countries = Country::query().order_by("id").get(app.db()).await.unwrap();
    let (sales, queries) = renox::db::capture_queries(has_many_through(
        app.db(),
        &countries,
        Customer::query(),
        "country_id",
        |c: &Customer| c.country_id,
        Sale::query().order_by("id"),
        "customer_id",
        |s: &Sale| s.customer_id,
    ))
    .await;
    let sales = sales.unwrap();
    assert_eq!(queries.len(), 2, "{queries:?}");
    let ids = |country: i64| -> Vec<i64> {
        sales
            .get(&country)
            .map(|s| s.iter().map(|s| s.id).collect())
            .unwrap_or_default()
    };
    assert_eq!(ids(1), [100, 101, 103]);
    assert_eq!(ids(2), [102]);
    assert!(ids(3).is_empty(), "no customers, no sales");
}

/// #250: a breach check that can't connect at all (not just an error
/// status) also lets the password through.
#[renox::test]
async fn uncompromised_allows_the_password_when_the_service_cant_be_reached() {
    let (logs, _logged) = crate::logs::capture();
    let app = TestApp::new(App::new().module(Forms)).await;
    app.fake_http().on(
        "https://api.pwnedpasswords.com/*",
        FakeResponse::connection_error(),
    );
    app.htmx()
        .post("/sign-up", &[("password", "password")])
        .await
        .assert_ok();
    assert!(
        logs.has(&["the password breach check failed; allowing the password"]),
        "{}",
        logs.text()
    );
}

/// #256: when every mailer in the failover list fails too, the last error
/// comes back. Also SMTP over TLS with credentials, and MAIL_PORT that isn't
/// a number (`{PREFIX}_PORT`).
#[renox::test]
async fn failover_that_runs_out_returns_the_last_error() {
    let (logs, _logged) = crate::logs::capture();
    let down = |config: &mut MailConfig, port: u16| {
        config.mailer = renox::mail::MailDriver::Smtp;
        config.host = "127.0.0.1".into();
        config.port = Some(port);
        config.encryption = renox::mail::MailEncryption::None;
        config.timeout = std::time::Duration::from_secs(2);
    };
    let app = TestApp::with_config(
        App::new().mailer("backup", move |_| {
            let mut backup = MailConfig::default();
            down(&mut backup, 10);
            // A host that can't be resolved: another error than the first
            // mailer's refused connection.
            backup.host = "no such host".into();
            backup.encryption = renox::mail::MailEncryption::Tls;
            backup.username = Some("app".into());
            backup.password = Some("secret".into());
            Ok(backup)
        }),
        move |c| {
            down(&mut c.mail, 9);
            c.mail.failover = vec!["backup".into()];
        },
    )
    .await;
    let err = app
        .state()
        .mailer
        .send(Mail::new("ann@example.com", "Receipt", "Thanks."))
        .await
        .unwrap_err();
    let shown = format!("{err:?}");
    assert!(!shown.contains("secret"), "{shown}");
    assert!(
        !shown.to_lowercase().contains("refused"),
        "the backup's error: {shown}"
    );
    let logged = logs.text();
    assert!(
        logged.contains("sending mail failed; trying the next mailer")
            && logged.contains("mailer=backup"),
        "{logged}"
    );

    let err = MailConfig::from_env(
        &{
            let mut c = Config::default();
            c.vars.insert("REPORTS_PORT".into(), "twenty-five".into());
            c
        },
        "REPORTS",
    )
    .unwrap_err();
    assert!(
        format!("{err:?}").contains("REPORTS_PORT must be a number, got `twenty-five`"),
        "{err:?}"
    );
}

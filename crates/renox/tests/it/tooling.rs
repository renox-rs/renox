//! M21e: typed app commands, prompts, and `push`/`stack` in views.

use std::sync::Mutex;

use renox::clap;
use renox::command::AppCommand;
use renox::prelude::*;
use renox::testing::TestApp;

/// Close unpaid orders.
#[derive(clap::Parser)]
#[command(name = "orders:close")]
struct CloseOrders {
    /// Orders older than this many days.
    #[arg(long, default_value_t = 3)]
    days: u32,
    #[arg(long)]
    dry_run: bool,
    /// Only these order ids.
    ids: Vec<i64>,
}

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

impl AppCommand for CloseOrders {
    async fn run(self, state: AppState) -> Result {
        let _ = state;
        SEEN.lock().unwrap().push(format!(
            "days={} dry_run={} ids={:?}",
            self.days, self.dry_run, self.ids
        ));
        Ok(())
    }
}

/// Create a user, asking for what's missing.
#[derive(clap::Parser)]
#[command(name = "user:create")]
struct CreateUser {
    #[arg(long)]
    email: Option<String>,
}

impl AppCommand for CreateUser {
    async fn run(self, state: AppState) -> Result {
        let email = match self.email {
            Some(email) => email,
            None => renox::prompt::ask("Email").await?,
        };
        let name = renox::prompt::ask_or("Name", "Admin").await?;
        let password = renox::prompt::secret("Password").await?;
        if renox::prompt::confirm(&format!("Create {email}?"), true).await? {
            User::register(&state.db, &name, &email, &password).await?;
        }
        Ok(())
    }
}

#[renox::test]
async fn typed_commands_parse_their_arguments() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .typed_command::<CloseOrders>()
            .typed_command::<CreateUser>(),
    )
    .await;
    let kernel = app.kernel();
    kernel
        .call("orders:close", ["7", "9", "--days", "10"])
        .await
        .unwrap();
    kernel.call("orders:close", ["--dry-run"]).await.unwrap();
    assert_eq!(
        *SEEN.lock().unwrap(),
        [
            "days=10 dry_run=false ids=[7, 9]",
            "days=3 dry_run=true ids=[]"
        ]
    );
    // --help prints the usage and succeeds.
    kernel.call("orders:close", ["--help"]).await.unwrap();
    // A wrong argument is an error that says what's wrong and how to call it.
    let err = kernel
        .call("orders:close", ["--days", "soon"])
        .await
        .unwrap_err();
    let text = format!("{err:?}");
    assert!(text.contains("invalid value 'soon'"), "{text}");
    assert!(text.contains("Usage: orders:close"), "{text}");
    assert!(kernel.call("orders:close", ["--nope"]).await.is_err());
    assert!(SEEN.lock().unwrap().len() == 2, "neither ran");
}

#[renox::test]
async fn commands_ask_for_what_is_missing() {
    let app = TestApp::new(App::new().module(Auth::new()).typed_command::<CreateUser>()).await;
    renox::prompt::answering(
        ["ana@example.com", "", "password123", "y"],
        app.kernel().call("user:create", [""; 0]),
    )
    .await
    .unwrap();
    let user = User::find_by_email(app.db(), "ana@example.com")
        .await
        .unwrap()
        .expect("created");
    assert_eq!(user.name, "Admin", "the default");

    // Given as an option, the email isn't asked; "no" creates nothing.
    renox::prompt::answering(
        ["Ben", "password123", "no"],
        app.kernel()
            .call("user:create", ["--email", "ben@example.com"]),
    )
    .await
    .unwrap();
    assert!(
        User::find_by_email(app.db(), "ben@example.com")
            .await
            .unwrap()
            .is_none()
    );
    // Nothing to answer with: the command fails and says what's missing.
    let err = renox::prompt::answering([""; 0], app.kernel().call("user:create", [""; 0]))
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("no answer for \"Email\""));
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/chart", || async {
                view("chart.html", context! {}).fragment("body")
            })
            .get("/missing", || async { Err::<&str, _>(Error::NotFound) })
            .get("/lookalike", || async {
                view("lookalike.html", context! {})
            })
    }
}

async fn pages() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write(
        "layout.html",
        "<head>{{ stack('head') }}</head><body>{% block body %}{% endblock %}{{ stack('scripts') }}</body>",
    );
    write(
        "components/chart.html",
        "{% macro chart(id) %}{% call push('scripts', once='chart-lib') %}<script src=\"/chart.js\" nonce=\"{{ csp_nonce() }}\"></script>{% endcall %}<canvas id=\"{{ id }}\"></canvas>{% endmacro %}",
    );
    write(
        "chart.html",
        "{% extends 'layout.html' %}{% from 'components/chart.html' import chart %}{% block body %}{% call push('head') %}<style>canvas{}</style>{% endcall %}{{ chart('a') }}{{ chart('b') }}{% endblock %}",
    );
    write(
        "errors/404.html",
        "{% extends 'layout.html' %}{% block body %}{% call push('scripts') %}<script>lost()</script>{% endcall %}Lost{% endblock %}",
    );
    // Text that looks like a marker, from a user, say.
    write(
        "lookalike.html",
        "{% extends 'layout.html' %}{% block body %}{{ '<!--renox-stack:x:scripts-->' }}{% endblock %}",
    );
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages), move |c| c.views_path = path).await;
    (app, dir)
}

#[renox::test]
async fn pushes_land_in_the_layouts_stacks() {
    let (app, _dir) = pages().await;
    let res = app.get("/chart").await;
    res.assert_ok();
    let html = res.text();
    assert!(
        html.starts_with("<head><style>canvas{}</style></head>"),
        "{html}"
    );
    assert_eq!(html.matches("chart.js").count(), 1, "once: {html}");
    assert!(html.ends_with("</script></body>"), "{html}");
    assert!(!html.contains("renox-stack"), "no markers left");

    // An htmx fragment has no layout: the pushes go nowhere.
    let res = app.htmx().get("/chart").await;
    let html = res.text();
    assert!(
        html.contains("<canvas id=\"a\">") && !html.contains("<head>"),
        "{html}"
    );
    assert!(!html.contains("renox-stack"));

    // Error pages have stacks too.
    let res = app.get("/missing").await;
    res.assert_not_found()
        .assert_see("<script>lost()</script></body>");
}

#[renox::test]
async fn text_that_looks_like_a_marker_is_left_alone() {
    let (app, _dir) = pages().await;
    let res = app.get("/lookalike").await;
    res.assert_ok()
        .assert_see("&lt;!--renox-stack:x:scripts--&gt;");
}

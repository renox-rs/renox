//! #251: views beyond the happy path: request globals in renders that have
//! no request, a view's status, template loading, error pages that fail,
//! shared values that fail, stacks, toasts and htmx headers.

use renox::prelude::*;
use renox::testing::TestApp;

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/created", || async {
                view("plain.html", context! {}).status(StatusCode::CREATED)
            })
            .get("/storage-link", || async {
                view("storage.html", context! {})
            })
            .get("/missing-view", || async { view("nope.html", context! {}) })
            .get("/broken-view", || async {
                view("broken.html", context! {})
            })
            .get("/escape", || async { view("../secret.html", context! {}) })
            .get("/directory", || async { view("folder.html", context! {}) })
            .get("/flash", |session: Session| async move {
                session.flash("status", "Saved")?;
                session.flash("note", "Twice")?;
                Ok::<_, Error>(Redirect::to("/flashed"))
            })
            .get("/flashed", || async { view("flashed.html", context! {}) })
            .get("/stacks", || async { view("stacks.html", context! {}) })
            .get("/no-stacks", || async { view("plain.html", context! {}) })
            .get("/toasts", || async {
                (
                    Toast::warning("Low stock.")
                        .link("Unsafe", "javascript:alert(1)")
                        .link("Open", "/stock"),
                    HxTrigger("refresh-list".into()),
                    "ok",
                )
            })
            .get("/toasted", || async { view("toasted.html", context! {}) })
            .get("/bad-header", || async {
                (HxRedirect("/a\nb".into()), "ok")
            })
            .get("/back", |back: Back| async move { back })
            .get("/forbidden", || async {
                Err::<String, _>(Error::Forbidden)
            })
            .get("/routes", || async { view("routes.html", context! {}) })
            .get("/no-route", || async { view("no_route.html", context! {}) })
            .get("/typed", || async {
                View::typed(TypedPage {
                    title: "Suppliers".into(),
                    n: 3,
                    hidden: "secret".into(),
                })
                .status(StatusCode::ACCEPTED)
            })
            .get("/json-trigger", || async {
                (
                    Toast::info("Saved."),
                    HxTrigger(r#"{"refresh-list": {"page": 2}}"#.into()),
                    "ok",
                )
            })
            .get(
                "/mail/{name}",
                |State(state): State<AppState>, Path(name): Path<String>| async move {
                    let mail = state.mail_view("ann@example.com", "Hi", &name, context! {})?;
                    Ok::<_, Error>(mail.html.unwrap_or_default())
                },
            )
    }
}

/// A page struct: its template comes from the type.
#[derive(serde::Serialize, ViewData)]
#[view(template = "typed.html")]
struct TypedPage {
    title: String,
    #[serde(rename = "count")]
    n: u32,
    #[serde(skip)]
    #[allow(dead_code)]
    hidden: String,
}

fn views() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("views");
    std::fs::create_dir_all(dir.join("errors")).unwrap();
    std::fs::create_dir_all(dir.join("folder.html")).unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.join(name), body).unwrap();
    // Next to the views, not in them: no view name may reach it.
    std::fs::write(root.path().join("secret.html"), "TOP SECRET").unwrap();
    write("plain.html", "plain page");
    write(
        "typed.html",
        "{{ title }}: {{ count }} [{% if hidden is defined %}leak{% endif %}]",
    );
    // `route()` with a query value left out, and a route that doesn't exist.
    write("routes.html", "{{ route('login', page=none, q='a b') }}");
    write("no_route.html", "{{ route('no.such.route') }}");
    write("toasted.html", "{{ toasts() }}");
    write("storage.html", "{{ storage_url('public/avatars/a.png') }}");
    write("broken.html", "{{ no_such_function() }}");
    write(
        "flashed.html",
        "{% for key in flash %}[{{ key }}={{ flash[key] }}]{% endfor %}",
    );
    write(
        "stacks.html",
        "<head>{{ stack('styles') }}</head>{% call push('styles') %}<style>a{}</style>{% endcall %}<body>after</body>",
    );
    // An app error page that fails (its layout is missing): Renox's page
    // is shown instead, with the same status.
    write("errors/404.html", "{% extends 'missing-layout.html' %}");
    // One with a syntax error: same fallback.
    write("errors/403.html", "{% if %}");
    // A component that uses the page's request globals, imported by a mail.
    write(
        "greeting.html",
        "{% macro hello() %}[{% if auth.check %}in{% else %}nobody{% endif %}]{% endmacro %}\
         {% macro form() %}{{ csrf_field() }}{% endmacro %}",
    );
    write(
        "mail_hello.html",
        "{% from 'greeting.html' import hello %}{{ hello() }}",
    );
    write(
        "mail_form.html",
        "{% from 'greeting.html' import form %}{{ form() }}",
    );
    root
}

async fn app(configure: impl FnOnce(&mut Config) + Send + 'static) -> (TestApp, tempfile::TempDir) {
    let root = views();
    let path = root.path().join("views");
    let app = TestApp::with_config(App::new().module(Auth::new()).module(Pages), move |c| {
        c.views_path = path;
        configure(c);
    })
    .await;
    (app, root)
}

#[renox::test]
async fn a_view_can_answer_with_another_status() {
    let (app, _dir) = app(|_| {}).await;
    app.get("/created")
        .await
        .assert_status(201)
        .assert_see("plain page");
}

#[renox::test]
async fn templates_link_to_stored_files() {
    let (app, _dir) = app(|_| {}).await;
    app.get("/storage-link")
        .await
        .assert_ok()
        .assert_see("/storage/avatars/a.png");
}

#[renox::test]
async fn views_that_fail_are_500s_with_the_template_named_while_debugging() {
    let (app, _dir) = app(|_| {}).await;
    app.get("/missing-view")
        .await
        .assert_status(500)
        .assert_see("nope.html");
    app.get("/broken-view")
        .await
        .assert_status(500)
        .assert_see("broken.html");
    // A directory where a template should be: an error, not a hang or a panic.
    app.get("/directory")
        .await
        .assert_status(500)
        .assert_see("could not read template");
}

#[renox::test]
async fn a_view_name_cant_leave_the_views_directory() {
    let (app, _dir) = app(|_| {}).await;
    let res = app.get("/escape").await;
    res.assert_status(500).assert_dont_see("TOP SECRET");
}

#[renox::test]
async fn an_app_error_page_that_fails_falls_back_to_renoxs() {
    let (logs, _logged) = crate::logs::capture();
    let (app, _dir) = app(|c| c.debug = false).await;
    // errors/404.html extends a missing layout; errors/403.html doesn't parse.
    app.get("/no-such-page")
        .await
        .assert_status(404)
        .assert_see("rx-error-page");
    app.get("/escape")
        .await
        .assert_status(500)
        .assert_see("rx-error-page");
    assert!(
        logs.has(&["the error page failed; showing Renox's"]),
        "{}",
        logs.text()
    );
}

#[renox::test]
async fn the_flash_can_be_looped_over() {
    let (app, _dir) = app(|_| {}).await;
    app.get("/flash").await.assert_redirect("/flashed");
    let page = app.get("/flashed").await;
    page.assert_see("[note=Twice]").assert_see("[status=Saved]");
}

#[renox::test]
async fn stacks_fill_markers_and_keep_the_rest_of_the_page() {
    let (app, _dir) = app(|_| {}).await;
    app.get("/stacks")
        .await
        .assert_see("<head><style>a{}</style></head>")
        .assert_see("<body>after</body>");
    app.get("/no-stacks").await.assert_see("plain page");
}

#[renox::test]
async fn warning_toasts_keep_other_triggers_and_pages_drop_unsafe_links() {
    let (app, _dir) = app(|_| {}).await;
    // Over htmx the toast rides HX-Trigger as data (renox-ui.js checks its
    // links), next to the handler's own trigger.
    let res = app.htmx().get("/toasts").await;
    let trigger = res.header("hx-trigger").unwrap().to_owned();
    let triggers: serde_json::Value = serde_json::from_str(&trigger).unwrap();
    assert!(triggers.get("refresh-list").is_some(), "{trigger}");
    let toast = &triggers["renox:toast"]["toasts"][0];
    assert_eq!(toast["kind"], "warning");
    assert_eq!(toast["message"], "Low stock.");
    // On a full page the server draws it, without the unsafe link.
    app.get("/toasts").await.assert_ok();
    let page = app.get("/toasted").await;
    page.assert_see("Low stock.")
        .assert_see("rx-toast--warning")
        .assert_see(r#"href="/stock""#)
        .assert_dont_see("javascript:");
}

#[renox::test]
async fn a_header_value_that_isnt_valid_is_dropped() {
    let (logs, _logged) = crate::logs::capture();
    let (app, _dir) = app(|_| {}).await;
    let res = app.get("/bad-header").await;
    res.assert_ok().assert_see("ok");
    assert!(res.header("hx-redirect").is_none());
    assert!(
        logs.has(&["invalid header value dropped"]),
        "{}",
        logs.text()
    );
}

#[renox::test]
async fn back_to_a_referer_with_no_path_is_the_home_page() {
    let (app, _dir) = app(|_| {}).await;
    app.request()
        .header("host", "shop.test")
        .header("referer", "https://shop.test")
        .get("/back")
        .await
        .assert_redirect("/");
}

#[renox::test]
async fn components_in_mails_see_no_request() {
    let (app, _dir) = app(|_| {}).await;
    // `auth` is there, empty: nobody is logged in to a mail.
    app.get("/mail/mail_hello")
        .await
        .assert_ok()
        .assert_see("[nobody]");
    // Calling a request helper says where it works.
    app.get("/mail/mail_form")
        .await
        .assert_status(500)
        .assert_see("is only available while rendering a page");
}

/// A shared value that fails: a page can't render without it (500, naming
/// the share), but an error page is shown without it.
#[renox::test]
async fn a_failing_shared_value_breaks_pages_but_not_error_pages() {
    let root = views();
    std::fs::write(
        root.path().join("views/errors/404.html"),
        "app 404 [{{ cart | default('no cart') }}]",
    )
    .unwrap();
    let path = root.path().join("views");
    let app = TestApp::with_config(
        App::new().module(Pages).share("cart", |_| async {
            Err::<i64, _>(Error::Internal(renox::anyhow::anyhow!("cart service down")))
        }),
        move |c| c.views_path = path,
    )
    .await;
    app.get("/created")
        .await
        .assert_status(500)
        .assert_see("sharing `cart` with views");
    app.get("/no-such-page")
        .await
        .assert_status(404)
        .assert_see("app 404 [no cart]");
}

#[renox::test]
async fn route_in_a_template_skips_empty_query_values_and_names_a_missing_route() {
    let (app, _dir) = app(|_| {}).await;
    app.get("/routes")
        .await
        .assert_ok()
        .assert_see("/login?q=a+b");
    app.get("/no-route")
        .await
        .assert_status(500)
        .assert_see("no.such.route");
}

/// An app error page that doesn't even parse: Renox's own page, with the
/// status kept (the failure is logged).
#[renox::test]
async fn an_error_page_that_doesnt_parse_falls_back_to_renoxs() {
    let (app, _dir) = app(|c| c.debug = false).await;
    app.get("/forbidden")
        .await
        .assert_status(403)
        .assert_see("rx-error-page");
}

/// A share that fails with an error other than an internal one, on an app
/// with its own 500 page: the page is drawn without the request's values.
#[renox::test]
async fn a_share_failing_with_any_error_shows_the_apps_500_page() {
    let root = views();
    std::fs::write(
        root.path().join("views/errors/500.html"),
        "app 500: {{ status }}",
    )
    .unwrap();
    let path = root.path().join("views");
    let app = TestApp::with_config(
        App::new()
            .module(Pages)
            .share("cart", |_| async { Err::<i64, _>(Error::Forbidden) }),
        move |c| c.views_path = path,
    )
    .await;
    app.get("/created")
        .await
        .assert_status(500)
        .assert_see("app 500: 500");
}

#[renox::test]
async fn a_toast_joins_a_json_hx_trigger() {
    let (app, _dir) = app(|_| {}).await;
    let res = app.htmx().get("/json-trigger").await;
    let trigger = res.header("hx-trigger").unwrap().to_owned();
    let triggers: serde_json::Value = serde_json::from_str(&trigger).unwrap();
    assert_eq!(triggers["refresh-list"]["page"], 2, "{trigger}");
    assert_eq!(triggers["renox:toast"]["toasts"][0]["message"], "Saved.");
}

#[derive(serde::Deserialize, Validate)]
struct NoteForm {
    #[validate(required, min = 3)]
    title: String,
}

/// Pages whose app components read the page's request values.
struct Components;

impl Module for Components {
    fn name(&self) -> &'static str {
        "components"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/compose", || async { view("compose.html", context! {}) })
            .post("/compose", |Valid(form): Valid<NoteForm>| async move {
                let _ = form.title;
                Redirect::to("/compose")
            })
            .get("/syntax", || async { view("syntax.html", context! {}) })
            .get("/fragment", || async {
                view("fragmented.html", context! {}).fragment("part")
            })
    }
}

async fn components_app() -> (TestApp, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(root.path().join(name), body).unwrap();
    write(
        "fields.html",
        "{% macro field(name) %}<input name=\"{{ name }}\" value=\"{{ old(name) }}\">\
         [{{ error(name) }}]{% if can('editor') %}<b>editor</b>{% endif %}{% endmacro %}",
    );
    write(
        "compose.html",
        "{% from 'fields.html' import field %}<form>{{ csrf_field() }}{{ field('title') }}</form>",
    );
    write("syntax.html", "{% if %}");
    write(
        "fragmented.html",
        "page {% block part %}{{ no_such_function() }}{% endblock %}",
    );
    let path = root.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(Components)
            .gate("editor", |user| user.email.starts_with("ed")),
        move |c| c.views_path = path,
    )
    .await;
    (app, root)
}

/// An app component imported by a page reads the page's old input, its
/// errors and the user's gates, after a plain form came back.
#[renox::test]
async fn components_read_old_input_errors_and_gates_during_a_page() {
    let (app, _dir) = components_app().await;
    let ed = User::register(app.db(), "Ed", "ed@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&ed);
    app.request()
        .header("referer", "/compose")
        .post("/compose", &[("title", "ab")])
        .await
        .assert_redirect("/compose");
    let page = app.get("/compose").await;
    page.assert_ok()
        .assert_see(r#"value="ab""#)
        .assert_see("at least 3")
        .assert_see("<b>editor</b>");
    // Another user fails the gate; a fresh page has no old input.
    app.logout();
    let bo = User::register(app.db(), "Bo", "bo@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&bo);
    app.get("/compose")
        .await
        .assert_see(r#"value="""#)
        .assert_see("[]")
        .assert_dont_see("<b>editor</b>");
}

/// A page whose template doesn't parse, and a fragment that fails, are 500s
/// naming the template while debugging.
#[renox::test]
async fn pages_and_fragments_that_fail_are_500s() {
    let (app, _dir) = components_app().await;
    app.get("/syntax")
        .await
        .assert_status(500)
        .assert_see("syntax.html");
    app.htmx()
        .get("/fragment")
        .await
        .assert_status(500)
        .assert_see("fragmented.html");
}

#[renox::test]
async fn a_typed_view_renders_its_own_template_with_its_fields() {
    assert_eq!(<TypedPage as renox::ViewData>::TEMPLATE, "typed.html");
    assert_eq!(<TypedPage as renox::ViewData>::FIELDS, ["title", "count"]);
    let (app, _dir) = app(|_| {}).await;
    app.get("/typed")
        .await
        .assert_status(202)
        .assert_view("typed.html")
        .assert_see("Suppliers: 3 []");
}

//! Registering live components.

use renox::live_component::LiveContext;
use renox::prelude::*;
use renox::testing::TestApp;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Counter {
    count: i64,
    name: String,
    search: String,
}

impl Validate for Counter {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
    }
}

impl LiveComponent for Counter {
    const NAME: &'static str = "counter";
    const VIEW: &'static str = "live/counter.html";

    async fn data(&self, _ctx: &LiveContext) -> Result<serde_json::Value> {
        Ok(json!({ "label": "clicks" }))
    }

    async fn call(
        &mut self,
        action: &str,
        args: Vec<serde_json::Value>,
        ctx: &mut LiveContext,
    ) -> Result {
        match action {
            "increment" => {
                self.count += 1;
                Ok(())
            }
            "add" => {
                let n: i64 = serde_json::from_value(args.first().cloned().unwrap_or_default())
                    .map_err(|e| Error::BadRequest(e.to_string()))?;
                self.count += n;
                Ok(())
            }
            "save" => ctx.validate(&*self).await,
            "notify" => {
                ctx.toast(Toast::success("Done"));
                Ok(())
            }
            "go" => {
                ctx.redirect("/elsewhere");
                Ok(())
            }
            "ping" => {
                ctx.dispatch("pinged", json!({ "n": 1 }));
                Ok(())
            }
            "_private" => Ok(()),
            _ => Err(Error::NotFound),
        }
    }
}

#[renox::test]
async fn a_component_registers_through_the_app() {
    let app = App::with_config(Config::default()).live_component::<Counter>();
    assert!(app.boot().await.is_ok());
}

#[renox::test]
async fn a_duplicate_component_is_a_boot_error() {
    let err = App::with_config(Config::default())
        .live_component::<Counter>()
        .live_component::<Counter>()
        .boot()
        .await
        .err()
        .unwrap();
    assert!(
        format!("{err:?}").contains("live component `counter` is registered twice"),
        "{err:?}"
    );
}

async fn counter(ctx: LiveContext) -> Result<View> {
    let list = ctx
        .mount(Counter {
            count: 2,
            ..Default::default()
        })
        .await?;
    Ok(view("page.html", context! { list }))
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/counter", counter)
    }
}

#[renox::test]
async fn a_page_mounts_a_component() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("live")).unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        r#"{% set component = list %}{% include "renox/live.html" %}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("live/counter.html"),
        "<b>{{ state.count }}</b>{{ data.label }}{{ csrf_field() }}",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(Pages)
            .live_component::<Counter>(),
        move |c| c.views_path = path,
    )
    .await;
    let page = app.get("/counter").await;
    page.assert_ok()
        .assert_see(r#"data-rx-live="counter""#)
        .assert_see("<b>2</b>clicks")
        .assert_see(r#"name="_token""#)
        .assert_see("data-rx-snapshot=");
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Other {
    count: i64,
}

impl LiveComponent for Other {
    const NAME: &'static str = "other";
    const VIEW: &'static str = "live/other.html";

    async fn call(&mut self, _: &str, _: Vec<serde_json::Value>, _: &mut LiveContext) -> Result {
        Ok(())
    }
}

async fn other(ctx: LiveContext) -> Result<View> {
    let list = ctx.mount(Other { count: 1 }).await?;
    Ok(view("page.html", context! { list }))
}

struct ActionPages;

impl Module for ActionPages {
    fn name(&self) -> &'static str {
        "action-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/counter", counter).get("/other", other)
    }
}

async fn actions_app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("live")).unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        r#"{% set component = list %}{% include "renox/live.html" %}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("live/counter.html"),
        "<b>{{ state.count }}</b>{{ state.search }}",
    )
    .unwrap();
    std::fs::write(dir.path().join("live/other.html"), "{{ state.count }}").unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(ActionPages)
            .live_component::<Counter>()
            .live_component::<Other>(),
        move |c| c.views_path = path,
    )
    .await;
    (app, dir)
}

async fn snapshot_of(app: &TestApp, page: &str) -> String {
    let html = app.get(page).await.text();
    let rest = html.split("data-rx-snapshot=\"").nth(1).unwrap();
    rest.split('"').next().unwrap().to_string()
}

#[renox::test]
async fn an_action_updates_the_state_and_the_snapshot() {
    let (app, _dir) = actions_app().await;
    let s = snapshot_of(&app, "/counter").await;
    let res = app
        .htmx()
        .post(
            "/_renox/live/counter/add",
            &[("_snapshot", &s), ("_args", "[5]")],
        )
        .await;
    res.assert_ok().assert_see("<b>7</b>");
    assert!(!res.text().contains(&s));
    assert!(res.text().contains("data-rx-snapshot="));
}

#[renox::test]
async fn posted_fields_are_merged() {
    let (app, _dir) = actions_app().await;
    let s = snapshot_of(&app, "/counter").await;
    let res = app
        .htmx()
        .post(
            "/_renox/live/counter/_refresh",
            &[("_snapshot", &s), ("search", "tea")],
        )
        .await;
    res.assert_ok().assert_see("<b>2</b>tea");
}

#[renox::test]
async fn bad_input_is_a_400() {
    let (app, _dir) = actions_app().await;
    let s = snapshot_of(&app, "/counter").await;
    app.htmx()
        .post(
            "/_renox/live/counter/increment",
            &[("_snapshot", &s), ("count", "abc")],
        )
        .await
        .assert_status(400);
    app.htmx()
        .post(
            "/_renox/live/counter/increment",
            &[("_snapshot", &format!("{s}x"))],
        )
        .await
        .assert_status(400);
    let other = snapshot_of(&app, "/other").await;
    app.htmx()
        .post("/_renox/live/counter/increment", &[("_snapshot", &other)])
        .await
        .assert_status(400);
    app.htmx()
        .post("/_renox/live/counter/increment", &[])
        .await
        .assert_status(400);
    app.htmx()
        .post(
            "/_renox/live/counter/add",
            &[("_snapshot", &s), ("_args", "{}")],
        )
        .await
        .assert_status(400);
}

#[renox::test]
async fn unknown_and_private_actions_are_404() {
    let (app, _dir) = actions_app().await;
    let s = snapshot_of(&app, "/counter").await;
    for action in ["nope", "_private"] {
        app.htmx()
            .post(
                &format!("/_renox/live/counter/{action}"),
                &[("_snapshot", &s)],
            )
            .await
            .assert_not_found();
    }
    app.htmx()
        .post("/_renox/live/missing/increment", &[("_snapshot", &s)])
        .await
        .assert_not_found();
}

#[renox::test]
async fn a_failed_validation_is_a_422() {
    let (app, _dir) = actions_app().await;
    let s = snapshot_of(&app, "/counter").await;
    let res = app
        .htmx()
        .post("/_renox/live/counter/save", &[("_snapshot", &s)])
        .await;
    res.assert_status(422)
        .assert_json_path("errors.name", json!(["The name field is required."]));
}

#[renox::test]
async fn effects_ride_in_headers() {
    let (app, _dir) = actions_app().await;
    let s = snapshot_of(&app, "/counter").await;
    let toast = app
        .htmx()
        .post("/_renox/live/counter/notify", &[("_snapshot", &s)])
        .await;
    toast.assert_ok();
    assert!(toast.header("hx-trigger").unwrap().contains("renox:toast"));
    app.htmx()
        .post("/_renox/live/counter/go", &[("_snapshot", &s)])
        .await
        .assert_hx_redirect("/elsewhere");
    let ping = app
        .htmx()
        .post("/_renox/live/counter/ping", &[("_snapshot", &s)])
        .await;
    assert!(
        ping.header("hx-trigger")
            .unwrap()
            .contains("rx:counter:pinged")
    );
}

#[renox::test]
async fn a_post_without_csrf_is_page_expired() {
    let (app, _dir) = actions_app().await;
    let s = snapshot_of(&app, "/counter").await;
    app.htmx()
        .without_csrf()
        .post("/_renox/live/counter/increment", &[("_snapshot", &s)])
        .await
        .assert_status(419);
}

#[renox::test]
async fn route_list_has_the_action_route() {
    let kernel = App::with_config(Config::default()).boot().await.unwrap();
    assert!(
        kernel
            .routes()
            .iter()
            .any(|r| r.method == "POST" && r.path == "/_renox/live/{component}/{action}")
    );
}

#[renox::test]
async fn a_page_with_two_components_loads_idiomorph_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("live")).unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        r#"{% set component = list %}{% include "renox/live.html" %}{% set component = list %}{% include "renox/live.html" %}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("live/counter.html"),
        "<b>{{ state.count }}</b>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(Pages)
            .live_component::<Counter>(),
        move |c| c.views_path = path,
    )
    .await;
    let body = app.get("/counter").await.text();
    assert_eq!(body.matches(r#"data-rx-live="counter""#).count(), 2);
    assert_eq!(body.matches("idiomorph-0.7.3.min.js").count(), 1);
    assert_eq!(body.matches("/_renox/live-").count(), 1);
}

#[renox::test]
async fn the_test_helper_keeps_state_between_calls() {
    let (app, _dir) = actions_app().await;
    let mut c = app.live(Counter::default());
    c.set("search", "tea");
    c.call("increment").await.assert_see("<b>1</b>");
    c.call("increment").await.assert_see("<b>2</b>tea");
    c.call_with("add", json!([5])).await.assert_see("<b>7</b>");
    let state = c.component();
    assert_eq!(state.count, 7);
    assert_eq!(state.search, "tea");
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Todo {
    done: Vec<i64>,
    title: String,
}

#[renox::live_component(view = "live/todo.html")]
impl Todo {
    async fn data(&self, _ctx: &LiveContext) -> Result<Vec<String>> {
        Ok(vec!["milk".to_string(), "tea".to_string()])
    }

    #[live(action)]
    async fn toggle(&mut self, _ctx: &mut LiveContext, id: i64) -> Result {
        self.done.push(id);
        Ok(())
    }

    #[live(action)]
    async fn rename(&mut self, _ctx: &mut LiveContext, id: i64, title: String) -> Result {
        self.title = format!("{id}:{title}");
        Ok(())
    }

    #[allow(dead_code)]
    async fn hidden(&mut self, _ctx: &mut LiveContext) -> Result {
        Ok(())
    }
}

async fn todo(ctx: LiveContext) -> Result<View> {
    let list = ctx.mount(Todo::default()).await?;
    Ok(view("page.html", context! { list }))
}

struct TodoPages;

impl Module for TodoPages {
    fn name(&self) -> &'static str {
        "todo-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/todo", todo)
    }
}

#[renox::test]
async fn a_macro_made_component_runs_its_marked_actions() {
    assert_eq!(<Todo as LiveComponent>::NAME, "todo");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("live")).unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        r#"{% set component = list %}{% include "renox/live.html" %}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("live/todo.html"),
        "<i>{{ state.done|join(',') }}</i><u>{{ state.title }}</u>{{ data|join('+') }}",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(TodoPages)
            .live_component::<Todo>(),
        move |c| c.views_path = path,
    )
    .await;
    let s = snapshot_of(&app, "/todo").await;
    let post = |action: &'static str, args: &'static str| {
        let s = s.clone();
        let app = &app;
        async move {
            app.htmx()
                .post(
                    &format!("/_renox/live/todo/{action}"),
                    &[("_snapshot", &s), ("_args", args)],
                )
                .await
        }
    };
    post("toggle", "[3]")
        .await
        .assert_ok()
        .assert_see("<i>3</i>")
        .assert_see("milk+tea");
    post("rename", r#"[4, "x"]"#)
        .await
        .assert_ok()
        .assert_see("<u>4:x</u>");
    post("toggle", "[]").await.assert_status(400);
    post("toggle", "[1, 2]").await.assert_status(400);
    post("toggle", r#"["a"]"#).await.assert_status(400);
    post("hidden", "[]").await.assert_status(404);
    post("data", "[]").await.assert_status(404);
}

/// The page with ids and snapshots blanked, which differ per mount.
fn blanked(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest
        .find(" id=\"")
        .or_else(|| rest.find("data-rx-snapshot=\""))
    {
        let quote = rest[i..].find('"').unwrap() + i + 1;
        let end = rest[quote..].find('"').unwrap() + quote;
        out.push_str(&rest[..quote]);
        out.push_str("...");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[renox::test]
async fn the_live_tag_renders_like_the_include() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("live")).unwrap();
    std::fs::write(
        dir.path().join("tag.html"),
        r#"<live-counter :component="list" @saved="open = false" class="box" data-x="1" />"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("include.html"),
        r#"{% with component = list, live_attrs = {"x-on:rx:counter:saved.self": "open = false", "class": "box", "data-x": "1"} %}{% include "renox/live.html" %}{% endwith %}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("live/counter.html"),
        r#"<button rx-click="increment">{{ state.count }}</button>"#,
    )
    .unwrap();
    async fn page(ctx: LiveContext, name: &'static str) -> Result<View> {
        let list = ctx.mount(Counter::default()).await?;
        Ok(view(name, context! { list }))
    }
    struct Both;
    impl Module for Both {
        fn name(&self) -> &'static str {
            "both"
        }
        fn routes(&self) -> Routes {
            Routes::new()
                .get("/tag", |ctx: LiveContext| page(ctx, "tag.html"))
                .get("/include", |ctx: LiveContext| page(ctx, "include.html"))
        }
    }
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(Both)
            .live_component::<Counter>(),
        move |c| c.views_path = path,
    )
    .await;
    let tag = app.get("/tag").await;
    let include = app.get("/include").await;
    tag.assert_ok()
        .assert_see(r#"class="box""#)
        .assert_see(r#"x-on:rx:counter:saved.self="open = false""#)
        .assert_see(r#"data-x="1""#)
        .assert_see(r#"rx-click="increment""#);
    assert_eq!(blanked(&tag.text()), blanked(&include.text()));
}

//! M18a: tenancy (default scopes, `renox::context`, scoped unique/exists),
//! gates on routes and `gate_before`, roles and permissions, API token
//! abilities.

use renox::auth::{Permissions, permissions};
use renox::axum::extract::Request;
use renox::axum::middleware::{Next, from_fn};
use renox::prelude::*;
use renox::testing::TestApp;

#[derive(Clone)]
struct CurrentTeam(i64);

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "projects", default_scope = "team_only")]
struct Project {
    id: i64,
    team_id: i64,
    name: String,
}

fn team_only(query: renox::db::Query<Project>) -> renox::db::Query<Project> {
    match renox::context::get::<CurrentTeam>() {
        Some(team) => query.where_eq("team_id", team.0),
        None => query.none(),
    }
}

impl Policy for Project {
    fn allows(&self, _user: &User, ability: &str) -> bool {
        ability == "view"
    }
}

/// Sets the current team from the logged-in user's membership.
async fn pick_team(user: Option<AuthUser>, req: Request, next: Next) -> Response {
    let db = req.extensions().get::<AppState>().unwrap().db.clone();
    if let Some(user) = user {
        let team: Option<i64> = renox::db::sql("SELECT team_id FROM memberships WHERE user_id = ?")
            .bind(user.id)
            .scalars(&db)
            .await
            .unwrap()
            .into_iter()
            .next();
        if let Some(team) = team {
            renox::context::set(CurrentTeam(team));
        }
    }
    next.run(req).await
}

async fn projects(State(db): State<Db>) -> Result<String> {
    let names: Vec<String> = Project::query().order_by("name").pluck(&db, "name").await?;
    Ok(names.join(","))
}

async fn project(State(db): State<Db>, Path(id): Path<i64>) -> Result<String> {
    Ok(Project::find_or_404(&db, id).await?.name)
}

#[derive(serde::Deserialize)]
struct ProjectForm {
    name: String,
    #[serde(default)]
    id: i64,
}

impl Validate for ProjectForm {
    fn rules(&self, v: &mut Validator) {
        let team = renox::context::get::<CurrentTeam>().map_or(0, |t| t.0);
        v.field("name", &self.name)
            .required()
            .unique("projects", "name")
            .ignore(self.id)
            .where_eq("team_id", team)
            .where_null("archived_at");
    }
}

async fn store(State(db): State<Db>, Valid(form): Valid<ProjectForm>) -> Result<StatusCode> {
    let team = renox::context::get::<CurrentTeam>().unwrap().0;
    Project::create(
        &db,
        Project {
            team_id: team,
            name: form.name,
            ..Default::default()
        },
    )
    .await?;
    Ok(StatusCode::CREATED)
}

async fn whoami(user: AuthUser) -> String {
    format!(
        "roles={} publish={} has_publish={} write={} admin={}",
        user.role_names().join("+"),
        user.allows("posts.publish"),
        user.has_permission("posts.publish"),
        user.token_can("orders:write"),
        user.has_role("admin"),
    )
}

async fn policy(user: AuthUser) -> Result<String> {
    let project = Project {
        id: 1,
        team_id: 1,
        name: "p".into(),
    };
    user.authorize("delete", &project)?;
    let can = Can::new(project, Some(&user), &["view", "delete"]);
    Ok(format!("{:?}", can.abilities))
}

async fn page() -> View {
    view("page.html", ())
}

struct Tenancy;

impl Module for Tenancy {
    fn name(&self) -> &'static str {
        "tenancy"
    }

    fn routes(&self) -> Routes {
        let open = Routes::new()
            .get("/projects", projects)
            .get("/projects/{id}", project)
            .post("/projects", store)
            .get("/whoami", whoami)
            .get("/page", page)
            .require_auth();
        let admin = Routes::new()
            .get("/admin", || async { "admin area" })
            .require_gate("admin");
        let billing = Routes::new()
            .get("/billing", || async { "billing" })
            .require_gate("billing");
        let editors = Routes::new()
            .get("/editors", || async { "editors" })
            .require_role("editor");
        let publish = Routes::new()
            .post("/publish", || async { "published" })
            .require_permission("posts.publish");
        let orders = Routes::new()
            .post("/orders", || async { "ordered" })
            .require_ability("orders:write")
            .without_csrf();
        let policy = Routes::new().get("/policy", policy);
        open.merge(admin)
            .merge(billing)
            .merge(editors)
            .merge(publish)
            .merge(orders)
            .merge(policy)
    }
}

fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(Permissions)
        .module(Tenancy)
        .layer(from_fn(pick_team))
        .gate("admin", |user| user.email.starts_with("admin@"))
        .gate_async("billing", |user, _state| async move {
            Ok(user.email.starts_with("bill"))
        })
        .gate_before(|user, _ability| (user.email == "root@example.com").then_some(true))
        .command("projects:count", "count", |_args, state| async move {
            // A command has its own, empty context: the scope fails closed.
            let scoped = Project::query().count(&state.db).await?;
            let all = Project::unscoped().count(&state.db).await?;
            assert_eq!((scoped, all), (0, 3));
            Ok(())
        })
}

async fn boot() -> (TestApp, tempfile::TempDir) {
    let views = tempfile::tempdir().unwrap();
    std::fs::write(
        views.path().join("page.html"),
        "{% if can('posts.publish') %}can-publish{% endif %} roles={{ auth.roles | join(',') }} \
         {% if can('admin') %}admin{% endif %}",
    )
    .unwrap();
    let app = TestApp::with_config(app(), |c| c.views_path = views.path().to_path_buf()).await;
    let id = match app.db().dialect() {
        renox::db::Dialect::Postgres => "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY",
        _ => "INTEGER PRIMARY KEY",
    };
    for statement in [
        format!(
            "CREATE TABLE projects (id {id}, team_id BIGINT NOT NULL, name TEXT NOT NULL, archived_at TEXT)"
        ),
        "CREATE TABLE memberships (user_id BIGINT NOT NULL, team_id BIGINT NOT NULL)".to_owned(),
        "INSERT INTO projects (team_id, name) VALUES (1, 'apollo'), (1, 'gemini'), (2, 'mercury')"
            .to_owned(),
    ] {
        renox::db::sql(statement).execute(app.db()).await.unwrap();
    }
    (app, views)
}

async fn user(app: &TestApp, email: &str, team: i64) -> User {
    let user = User::register(app.db(), "U", email, "password123")
        .await
        .unwrap();
    renox::db::sql("INSERT INTO memberships (user_id, team_id) VALUES (?, ?)")
        .bind(user.id)
        .bind(team)
        .execute(app.db())
        .await
        .unwrap();
    user
}

#[renox::test]
async fn default_scopes_keep_tenants_apart() {
    let (app, _views) = boot().await;
    let ana = user(&app, "ana@example.com", 1).await;
    let bo = user(&app, "bo@example.com", 2).await;

    app.acting_as(&ana);
    app.get("/projects").await.assert_see("apollo,gemini");
    app.get("/projects/1").await.assert_see("apollo");
    app.get("/projects/3").await.assert_not_found(); // another team's, by id
    app.acting_as(&bo);
    app.get("/projects")
        .await
        .assert_ok()
        .assert_see("mercury")
        .assert_dont_see("apollo");

    // Outside a request (a command) there is no team: nothing matches.
    app.kernel()
        .call("projects:count", Vec::<String>::new())
        .await
        .unwrap();
    assert_eq!(Project::unscoped().count(app.db()).await.unwrap(), 3);
}

#[renox::test]
async fn unique_and_exists_can_be_scoped() {
    let (app, _views) = boot().await;
    let ana = user(&app, "ana@example.com", 1).await;
    let bo = user(&app, "bo@example.com", 2).await;

    app.acting_as(&bo);
    // "apollo" is team 1's; team 2 may use the name.
    app.htmx()
        .post("/projects", &[("name", "apollo")])
        .await
        .assert_status(201);
    app.htmx()
        .post("/projects", &[("name", "apollo")])
        .await
        .assert_invalid("name");
    app.acting_as(&ana);
    app.htmx()
        .post("/projects", &[("name", "gemini")])
        .await
        .assert_invalid("name");
    // Editing gemini itself keeps its name.
    app.htmx()
        .post("/projects", &[("name", "gemini"), ("id", "2")])
        .await
        .assert_status(201);
    // Archived rows don't count.
    renox::db::sql("UPDATE projects SET archived_at = 'x' WHERE name = 'apollo' AND team_id = 1")
        .execute(app.db())
        .await
        .unwrap();
    app.htmx()
        .post("/projects", &[("name", "apollo")])
        .await
        .assert_status(201);

    // `exists` with a scope, straight through the validator.
    struct Pick {
        project: i64,
    }
    impl Validate for Pick {
        fn rules(&self, v: &mut Validator) {
            v.field("project", &self.project)
                .exists("projects", "id")
                .where_eq("team_id", 2);
        }
    }
    let errors = Validator::rules_of(&Pick { project: 1 })
        .finish(app.db())
        .await
        .unwrap();
    assert!(errors.has("project"), "project 1 is team 1's");
    let errors = Validator::rules_of(&Pick { project: 3 })
        .finish(app.db())
        .await
        .unwrap();
    assert!(!errors.has("project"));
}

#[renox::test]
async fn gates_guard_routes_and_gate_before_lets_root_through() {
    let (app, _views) = boot().await;
    app.get("/admin").await.assert_redirect("/login");
    let ana = user(&app, "ana@example.com", 1).await;
    let admin = user(&app, "admin@example.com", 1).await;
    let bill = user(&app, "bill@example.com", 1).await;
    let root = user(&app, "root@example.com", 1).await;

    app.acting_as(&ana);
    app.get("/admin").await.assert_forbidden();
    app.get("/billing").await.assert_forbidden();
    app.get("/policy").await.assert_forbidden(); // the policy denies "delete"
    app.acting_as(&admin);
    app.get("/admin").await.assert_ok().assert_see("admin area");
    app.acting_as(&bill);
    app.get("/billing").await.assert_ok(); // an async gate
    app.request().json().get("/admin").await.assert_forbidden();

    app.acting_as(&root);
    for path in ["/admin", "/billing"] {
        app.get(path).await.assert_ok();
    }
    // gate_before answers abilities, not role membership.
    app.get("/editors").await.assert_forbidden();
    app.post("/publish", &[]).await.assert_ok();
    // Policies too, and `Can::new` with the AuthUser.
    app.get("/policy")
        .await
        .assert_see(r#"{"delete": true, "view": true}"#);

    let listing = app.kernel().routes();
    let guards = |path: &str| {
        listing
            .iter()
            .find(|r| r.path == path)
            .map(|r| r.middleware.join(" "))
            .unwrap()
    };
    assert_eq!(guards("/admin"), "gate:admin");
    assert_eq!(guards("/editors"), "role:editor");
    assert_eq!(guards("/publish"), "permission:posts.publish");
    assert!(guards("/orders").contains("ability:orders:write"));
}

#[renox::test]
async fn roles_grant_permissions() {
    let (app, _views) = boot().await;
    let db = app.db();
    let ana = user(&app, "ana@example.com", 1).await;
    permissions::define_role(db, "editor", &["posts.create", "posts.publish"])
        .await
        .unwrap();
    permissions::define_role(db, "viewer", &["posts.view"])
        .await
        .unwrap();
    assert!(
        ana.assign_role(db, "ghost").await.is_err(),
        "roles must exist"
    );

    app.acting_as(&ana);
    app.get("/editors").await.assert_forbidden();
    app.post("/publish", &[]).await.assert_forbidden();
    app.get("/page").await.assert_dont_see("can-publish");

    ana.assign_role(db, "editor").await.unwrap();
    ana.assign_role(db, "editor").await.unwrap(); // twice is fine
    app.get("/editors").await.assert_ok();
    app.post("/publish", &[]).await.assert_ok();
    app.get("/whoami")
        .await
        .assert_see("roles=editor publish=true has_publish=true write=true admin=false");
    app.get("/page")
        .await
        .assert_see("can-publish roles=editor");
    assert_eq!(ana.roles(db).await.unwrap(), ["editor"]);
    assert_eq!(
        ana.permissions(db).await.unwrap(),
        ["posts.create", "posts.publish"]
    );

    permissions::revoke(db, "editor", &["posts.publish"])
        .await
        .unwrap();
    app.post("/publish", &[]).await.assert_forbidden();
    permissions::grant(db, "editor", &["posts.publish"])
        .await
        .unwrap();
    app.post("/publish", &[]).await.assert_ok();

    ana.sync_roles(db, &["viewer"]).await.unwrap();
    assert_eq!(ana.roles(db).await.unwrap(), ["viewer"]);
    app.get("/editors").await.assert_forbidden();
    ana.remove_role(db, "viewer").await.unwrap();
    assert!(ana.roles(db).await.unwrap().is_empty());

    // Redefining a role replaces its permissions; deleting it takes it away.
    permissions::define_role(db, "editor", &["posts.create"])
        .await
        .unwrap();
    let roles = permissions::roles(db).await.unwrap();
    assert_eq!(
        roles,
        [
            ("editor".to_owned(), vec!["posts.create".to_owned()]),
            ("viewer".to_owned(), vec!["posts.view".to_owned()])
        ]
    );
    ana.assign_role(db, "editor").await.unwrap();
    assert!(permissions::delete_role(db, "editor").await.unwrap());
    assert!(ana.roles(db).await.unwrap().is_empty());
}

#[renox::test]
async fn api_tokens_carry_abilities() {
    let (app, _views) = boot().await;
    let db = app.db();
    let ana = user(&app, "ana@example.com", 1).await;
    let read = ana
        .create_token_with(db, "reports", &["orders:read"], None)
        .await
        .unwrap();
    let write = ana
        .create_token_with(db, "pos", &["orders:read", "orders:write"], None)
        .await
        .unwrap();
    let full = ana.create_token(db, "cli", None).await.unwrap();
    assert_eq!(
        read.token.abilities.as_deref(),
        Some(&["orders:read".to_owned()][..])
    );
    assert_eq!(full.token.abilities, None);

    let post = |token: String| {
        let app = &app;
        async move {
            app.request()
                .json()
                .header("authorization", &format!("Bearer {token}"))
                .post("/orders", &[])
                .await
                .status
                .as_u16()
        }
    };
    assert_eq!(post(read.plain.clone()).await, 403);
    assert_eq!(post(write.plain).await, 200);
    assert_eq!(post(full.plain).await, 200);
    // A session may do everything.
    app.acting_as(&ana);
    app.request()
        .without_csrf()
        .post("/orders", &[])
        .await
        .assert_ok();

    // Expired tokens are pruned after a grace period.
    renox::db::sql("UPDATE personal_access_tokens SET expires_at = ? WHERE id = ?")
        .bind(renox::db::now() - renox::chrono::TimeDelta::days(2))
        .bind(read.token.id)
        .execute(db)
        .await
        .unwrap();
    app.kernel()
        .call("tokens:prune", Vec::<String>::new())
        .await
        .unwrap();
    let left: Vec<String> = ana
        .tokens(db)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(left, ["cli", "pos"]);
}

/// An app's own BOOLEAN column on `users` reads as a `bool` on both
/// databases: SQLite stores it as 0/1, which used to make `get::<bool>` (and
/// a `gate_before` built on it) answer `None`.
#[renox::test]
async fn user_get_reads_boolean_columns() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .gate("never", |_| false)
            .gate_before(|user, _| (user.get::<bool>("super_admin") == Some(true)).then_some(true))
            .module(BoolRoutes),
    )
    .await;
    let column = match app.db().dialect() {
        renox::db::Dialect::Postgres => "BOOLEAN NOT NULL DEFAULT FALSE",
        _ => "BOOLEAN NOT NULL DEFAULT 0",
    };
    renox::db::sql(format!("ALTER TABLE users ADD COLUMN super_admin {column}"))
        .execute(app.db())
        .await
        .unwrap();
    let mut root = User::register(app.db(), "Root", "root@example.com", "password123")
        .await
        .unwrap();
    root.set(app.db(), "super_admin", true).await.unwrap();
    let plain = User::register(app.db(), "Plain", "plain@example.com", "password123")
        .await
        .unwrap();

    let root = User::find(app.db(), root.id).await.unwrap().unwrap();
    let plain = User::find(app.db(), plain.id).await.unwrap().unwrap();
    assert_eq!(root.get::<bool>("super_admin"), Some(true));
    assert_eq!(plain.get::<bool>("super_admin"), Some(false));
    assert_eq!(root.get::<String>("super_admin"), None);

    app.acting_as(&root).get("/never").await.assert_ok();
    app.acting_as(&plain).get("/never").await.assert_forbidden();
}

struct BoolRoutes;

impl Module for BoolRoutes {
    fn name(&self) -> &'static str {
        "bool-routes"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/never", || async { "in" })
            .require_gate("never")
    }
}

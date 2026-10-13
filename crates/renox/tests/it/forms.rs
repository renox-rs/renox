//! M21f: more validation rules, and the form-request hooks (`prepare`,
//! `authorize`, `after`).

use renox::prelude::*;
use renox::testing::TestApp;
use renox::validation::FormContext;

async fn errors_of(app: &TestApp, rules: impl FnOnce(&mut Validator)) -> Errors {
    let mut v = Validator::new();
    rules(&mut v);
    v.finish(app.db()).await.unwrap()
}

#[renox::test]
async fn the_new_rules() {
    let app = TestApp::new(App::new()).await;
    let errors = errors_of(&app, |v| {
        v.field("name", &"José").alpha();
        v.field("name_bad", &"José 2").alpha();
        v.field("code", &"AB12").alpha_num();
        v.field("code_bad", &"AB-12").alpha_num();
        v.field("slug", &"coffee-milk_2").alpha_dash();
        v.field("slug_bad", &"coffee milk").alpha_dash();
        v.field("handle", &"ana").lowercase();
        v.field("handle_bad", &"Ana").lowercase();
        v.field("sku", &"COFFEE-1").uppercase();
        v.field("sku_bad", &"Coffee-1").uppercase();
        v.field("phone", &"0812").starts_with(&["08", "+62"]);
        v.field("phone_bad", &"12").starts_with(&["08", "+62"]);
        v.field("mail", &"a@company.com")
            .ends_with(&["@company.com"]);
        v.field("mail_bad", &"a@gmail.com")
            .ends_with(&["@company.com"]);
        v.field("id", &"123e4567-e89b-12d3-a456-426614174000")
            .uuid();
        v.field("id_bad", &"123e4567-e89b-12d3-a456").uuid();
        v.field("ip", &"2001:db8::1").ip();
        v.field("ip_bad", &"300.1.1.1").ip();
        v.field("pin", &"1234").size(4);
        v.field("pin_bad", &"123").size(4);
        v.field("qty", &3).size(3);
        v.field("email", &None::<String>)
            .required_without(&None::<String>);
        v.field("email_ok", &None::<String>)
            .required_without(&Some("0812"));
        v.field("coupon", &"SAVE10").prohibited_if(true);
        v.field("coupon_ok", &"SAVE10").prohibited_if(false);
        v.distinct("invites", &["a@b.c", "x@y.z", " A@B.C ", ""]);
    })
    .await;
    let failed: Vec<&str> = errors.iter().map(|(field, _)| field).collect();
    let mut expected = vec![
        "name_bad",
        "code_bad",
        "slug_bad",
        "handle_bad",
        "sku_bad",
        "phone_bad",
        "mail_bad",
        "id_bad",
        "ip_bad",
        "pin_bad",
        "email",
        "coupon",
        "invites.2",
    ];
    expected.sort();
    let mut failed = failed;
    failed.sort();
    assert_eq!(failed, expected);
    assert_eq!(
        errors.first("phone_bad"),
        Some("The phone bad must start with one of: 08, +62.")
    );
    assert_eq!(
        errors.first("pin_bad"),
        Some("The pin bad must be 4 characters.")
    );
    assert_eq!(
        errors.first("invites.2"),
        Some("The invites #3 is a duplicate.")
    );
    assert_eq!(
        errors.first("coupon"),
        Some("The coupon field must be empty here.")
    );
}

#[derive(serde::Deserialize)]
struct Invite {
    email: String,
}

impl Validate for Invite {
    fn prepare(&mut self) {
        self.email = self.email.trim().to_lowercase();
    }

    async fn authorize(&self, form: &FormContext<'_>) -> Result<bool> {
        assert_eq!((form.method.as_str(), form.path), ("POST", "/invites"));
        Ok(form
            .user
            .is_some_and(|user| user.email.ends_with("@team.test")))
    }

    fn rules(&self, v: &mut Validator) {
        v.field("email", &self.email).required().email();
    }

    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        if self.email == "boom@example.com" {
            return Err(Error::Internal(renox::anyhow::anyhow!(
                "the directory is down"
            )));
        }
        let taken = User::find_by_email(&form.state.db, &self.email)
            .await?
            .is_some();
        if taken {
            errors.add("email", "They already have an account.");
        }
        Ok(())
    }
}

struct Invites;

impl Module for Invites {
    fn name(&self) -> &'static str {
        "invites"
    }

    fn routes(&self) -> Routes {
        Routes::new().post("/invites", |Valid(invite): Valid<Invite>| async move {
            format!("invited {}", invite.email)
        })
    }
}

#[renox::test]
async fn form_requests_prepare_authorize_and_check_after() {
    let app = TestApp::new(App::new().module(Auth::new()).module(Invites)).await;
    let body = |email: &str| json!({ "email": email });

    // A guest, and a user outside the team: 403 before any rule runs.
    app.post_json("/invites", &body("not an email"))
        .await
        .assert_forbidden();
    let outsider = User::register(app.db(), "Out", "out@else.test", "password123")
        .await
        .unwrap();
    app.acting_as(&outsider);
    app.post_json("/invites", &body("ana@example.com"))
        .await
        .assert_forbidden();

    let owner = User::register(app.db(), "Own", "own@team.test", "password123")
        .await
        .unwrap();
    app.acting_as(&owner);
    // `prepare` ran before the rules: the padded, capitalised email passes.
    let res = app.post_json("/invites", &body("  Ana@Example.COM ")).await;
    res.assert_ok().assert_see("invited ana@example.com");
    // The rules still apply.
    app.post_json("/invites", &body("nope"))
        .await
        .assert_invalid("email");
    // `after` checks the database once the rules pass.
    let res = app.post_json("/invites", &body("OUT@else.test")).await;
    res.assert_invalid("email")
        .assert_see("They already have an account.");
    // A failing `after` is an error, not a validation message.
    app.post_json("/invites", &body("boom@example.com"))
        .await
        .assert_status(500);

    // Plain forms too: an `after` error sends the form back, like a rule's.
    let res = app
        .request()
        .header("referer", "/invite")
        .post("/invites", &[("email", "out@else.test")])
        .await;
    res.assert_redirect("/invite");
}

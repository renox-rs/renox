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

// #45 part 7: the currency and money rules, `#[model(form)]` for `Money`, and
// `<rx-input type="money">`.

#[renox::test]
async fn currency_and_money_rules() {
    let app = TestApp::new(App::new()).await;
    let errors = errors_of(&app, |v| {
        v.field("c_ok", &"aed").currency();
        v.field("c_bad", &"ABCD").currency();
        v.field("c_num", &"123").currency();
        v.field("c_empty", &"").currency();
        v.field("usd_ok", &"12.50").money("USD");
        v.field("usd_bad", &"1.234").money("USD");
        v.field("jpy_ok", &"1250").money("JPY");
        v.field("jpy_bad", &"10.5").money("JPY");
        v.field("kwd_ok", &"1.234").money("KWD");
        v.field("text_bad", &"abc").money("USD");
        v.field("unknown", &"abc").money("nope");
    })
    .await;
    for ok in ["c_ok", "c_empty", "usd_ok", "jpy_ok", "kwd_ok", "unknown"] {
        assert!(errors.first(ok).is_none(), "{ok}");
    }
    assert_eq!(
        errors.first("c_bad"),
        Some("The c bad must be a valid currency code.")
    );
    assert!(errors.first("c_num").is_some());
    assert_eq!(
        errors.first("jpy_bad"),
        Some("The jpy bad must be an amount with at most 0 decimal places.")
    );
    for bad in ["usd_bad", "text_bad"] {
        assert!(errors.first(bad).is_some(), "{bad}");
    }
}

#[derive(Model, serde::Serialize, Debug, Clone, PartialEq)]
#[model(table = "quotes", form)]
struct Quote {
    id: i64,
    #[model(money(currency = "currency"))]
    subtotal: renox::money::Money,
    #[model(money(currency = "currency"))]
    refund: Option<renox::money::Money>,
}

#[renox::test]
async fn model_form_reads_and_checks_money() {
    use renox::money::Money;
    let app = TestApp::new(App::new()).await;
    let form = |subtotal: &str, refund: &str, currency: &str| QuoteForm {
        subtotal: subtotal.into(),
        refund: refund.into(),
        currency: currency.into(),
    };
    let check = |form: &QuoteForm| {
        let v = Validator::rules_of(form);
        let db = app.db();
        async move { v.finish(db).await.unwrap() }
    };

    let good = form("12.50", "", "usd");
    assert!(check(&good).await.first("subtotal").is_none());
    let mut quote = Quote {
        id: 1,
        subtotal: Money::usd(1),
        refund: Some(Money::usd(5)),
    };
    good.fill(&mut quote);
    assert_eq!(quote.subtotal, Money::usd(1250));
    assert_eq!(quote.refund, None);
    form("1250", "10", "JPY").fill(&mut quote);
    assert_eq!(quote.subtotal, Money::jpy(1250));
    assert_eq!(quote.refund, Some(Money::jpy(10)));

    let errors = check(&form("10.5", "", "JPY")).await;
    assert!(errors.first("subtotal").is_some());
    assert!(errors.first("refund").is_none());
    let errors = check(&form("", "", "ABCD")).await;
    assert!(errors.first("subtotal").is_some(), "required");
    assert!(errors.first("currency").is_some());
}

struct MoneyPages;

impl Module for MoneyPages {
    fn name(&self) -> &'static str {
        "money-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/m", || async { view("m.html", context! {}) })
    }
}

#[renox::test]
async fn rx_input_money_steps_by_the_currencys_smallest_unit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        "<rx-input name=\"a\" label=\"A\" type=\"money\" currency=\"JPY\"/>\
         <rx-input name=\"b\" label=\"B\" type=\"money\" currency=\"kwd\"/>\
         <rx-input name=\"c\" label=\"C\" type=\"money\"/>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(MoneyPages), move |c| c.views_path = path).await;
    let html = app.get("/m").await.text();
    assert!(html.contains("step=\"1\""), "{html}");
    assert!(html.contains("step=\"0.001\""), "{html}");
    assert!(html.contains("step=\"0.01\""), "{html}");
    assert!(html.contains(">JPY</span>") && html.contains(">KWD</span>"));
    assert!(html.contains("inputmode=\"decimal\""));
}

// #69 part 6: the quantity and unit rules, `#[model(form)]` for `Quantity`, and
// `<rx-input type="quantity">`.

#[renox::test]
async fn quantity_and_unit_rules() {
    let units = renox::quantity::Units::new()
        .base("kg")
        .unit("g", "kg", "0.001")
        .build()
        .unwrap();
    let app = TestApp::new(App::new()).await;
    let errors = errors_of(&app, |v| {
        v.field("a", &"12.5").quantity(3);
        v.field("b", &"12.500").quantity(3);
        v.field("c", &"12.5000").quantity(3);
        v.field("d", &"12.5001").quantity(3);
        v.field("e", &"abc").quantity(3);
        v.field("f", &"").quantity(3);
        v.field("g", &"1.5").quantity(0);
        v.field("h", &"99999999999999999999").quantity(3);
        v.field("u_ok", &"g").unit(&units);
        v.field("u_bad", &"lb").unit(&units);
    })
    .await;
    for ok in ["a", "b", "c", "f", "u_ok"] {
        assert!(errors.first(ok).is_none(), "{ok}");
    }
    for bad in ["d", "e", "g", "h", "u_bad"] {
        assert!(errors.first(bad).is_some(), "{bad}");
    }
    assert_eq!(
        errors.first("d"),
        Some("The d must be a number with at most 3 decimal places.")
    );
    assert_eq!(
        errors.first("u_bad"),
        Some("The u bad must be a valid unit.")
    );
}

#[renox::test]
async fn quantity_limits_compare_the_number_not_the_text() {
    let app = TestApp::new(App::new()).await;
    let errors = errors_of(&app, |v| {
        v.field("low", &"0.5").quantity(3).quantity_min("1");
        v.field("edge", &"1.000").quantity(3).quantity_min("1");
        v.field("neg", &"-0.001").quantity(3).quantity_min("0");
        v.field("high", &"100.001").quantity(3).quantity_max("100");
        v.field("top", &"100").quantity(3).quantity_max("100.000");
        v.field("long", &"9").quantity(3).quantity_min("10");
        v.field("blank", &"").quantity(3).quantity_min("1");
        v.field("junk", &"abc").quantity(3).quantity_min("1");
    })
    .await;
    for bad in ["low", "neg", "high", "long"] {
        assert!(errors.first(bad).is_some(), "{bad}");
    }
    for ok in ["edge", "top", "blank"] {
        assert!(errors.first(ok).is_none(), "{ok}");
    }
    assert_eq!(errors.first("low"), Some("The low must be at least 1."));
    assert_eq!(errors.first("high"), Some("The high must be at most 100."));
    // Text that is not a decimal is the `quantity` rule's error, not a limit's.
    assert_eq!(
        errors.first("junk"),
        Some("The junk must be a number with at most 3 decimal places.")
    );
}

#[derive(Model, serde::Serialize, Debug, Clone, PartialEq)]
#[model(table = "bins", form)]
struct Bin {
    id: i64,
    stock: renox::quantity::Quantity<3>,
    spare: Option<renox::quantity::Quantity<3>>,
}

#[renox::test]
async fn model_form_reads_and_checks_quantity() {
    use renox::quantity::Quantity;
    let app = TestApp::new(App::new()).await;
    let form = |stock: &str, spare: &str| BinForm {
        stock: stock.into(),
        spare: spare.into(),
    };
    let check = |form: &BinForm| {
        let v = Validator::rules_of(form);
        let db = app.db();
        async move { v.finish(db).await.unwrap() }
    };
    let mut bin = Bin {
        id: 1,
        stock: Quantity::from_raw(1),
        spare: Some(Quantity::from_raw(5)),
    };
    let good = form("12.5", "");
    assert!(check(&good).await.first("stock").is_none());
    good.fill(&mut bin);
    assert_eq!(bin.stock.raw(), 12_500);
    assert_eq!(bin.spare, None);
    form("0.001", "2").fill(&mut bin);
    assert_eq!(
        (bin.stock.raw(), bin.spare.map(|q| q.raw())),
        (1, Some(2000))
    );

    let errors = check(&form("1.2345", "x")).await;
    assert!(errors.first("stock").is_some());
    assert!(errors.first("spare").is_some());
    assert!(
        check(&form("", "")).await.first("stock").is_some(),
        "required"
    );
}

#[renox::test]
async fn rx_input_quantity_steps_by_the_scale_and_shows_the_unit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        "<rx-input name=\"a\" label=\"A\" type=\"quantity\" scale=\"3\" unit=\"kg\"/>\
         <rx-input name=\"b\" label=\"B\" type=\"quantity\" scale=\"0\"/>\
         <rx-input name=\"c\" label=\"C\" type=\"quantity\" scale=\"1\" unit=\"m\"/>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(MoneyPages), move |c| c.views_path = path).await;
    let html = app.get("/m").await.text();
    assert!(html.contains("step=\"0.001\""), "{html}");
    assert!(html.contains("step=\"1\""), "{html}");
    assert!(html.contains("step=\"0.1\""), "{html}");
    assert!(
        html.contains(">kg</span>") && html.contains(">m</span>"),
        "{html}"
    );
    assert!(html.contains("inputmode=\"decimal\"") && html.contains("type=\"number\""));
}

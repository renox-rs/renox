//! What the landing page says, as data the template renders on the server:
//! every word, number and code sample is in the HTML a crawler gets, and
//! public/www.js only animates it.

use serde::Serialize;

/// The docs site.
pub const DOCS: &str = "https://docs.renox.rs";
/// The repository.
pub const REPOSITORY: &str = "https://github.com/renox-rs/renox";
/// The live demo.
pub const DEMO: &str = "https://bikeshop.renox.rs";
/// renox.rs's Google Analytics 4 property. `GA4_MEASUREMENT_ID` overrides it;
/// the tag is only sent in production (see the layout's `ga4`).
pub const GA4: &str = "G-48GJK3ZM4J";

/// A figure in the strip under the hero (counted from the repository).
#[derive(Serialize)]
pub struct Metric {
    pub value: u32,
    pub suffix: &'static str,
    pub label: &'static str,
}

pub fn metrics() -> Vec<Metric> {
    vec![
        Metric {
            value: 1608,
            suffix: "",
            label: "tests",
        },
        Metric {
            value: 90,
            suffix: "%",
            label: "line coverage, enforced",
        },
        Metric {
            value: 72,
            suffix: "",
            label: "UI kit components",
        },
        Metric {
            value: 130,
            suffix: "",
            label: "demo pages explained beside their code",
        },
        Metric {
            value: 6,
            suffix: "",
            label: "official plugins",
        },
        Metric {
            value: 27,
            suffix: "",
            label: "built-in commands",
        },
    ]
}

/// The crates a Rust web app usually wires by hand ("twenty crates").
pub const CRATES: &[&str] = &[
    "axum",
    "tower",
    "sqlx",
    "tokio",
    "minijinja",
    "serde",
    "validator",
    "argon2",
    "lettre",
    "cookie",
    "tower-sessions",
    "apalis",
    "cron",
    "tracing",
    "reqwest",
    "object_store",
    "chrono",
    "uuid",
    "csrf",
    "governor",
    "rust-embed",
    "clap",
];

/// One of the eight feature cards.
#[derive(Serialize)]
pub struct Feature {
    pub icon: &'static str,
    pub title: &'static str,
    pub text: &'static str,
    pub code: &'static str,
    /// Its guide on docs.renox.rs.
    pub doc: &'static str,
}

pub fn features() -> Vec<Feature> {
    vec![
        Feature {
            icon: "⌁",
            title: "Routing & requests",
            text: "Named routes, groups, domains, guards, rate limits, signed URLs, route model binding.",
            code: "Routes::new().get(\"/\", home)",
            doc: "routing",
        },
        Feature {
            icon: "▤",
            title: "Models & migrations",
            text: "A query builder that checks column names, relations without N+1, migrations with batches, SQLite or PostgreSQL.",
            code: "#[derive(Model)]",
            doc: "relations",
        },
        Feature {
            icon: "✓",
            title: "Validation",
            text: "Form requests as structs with Laravel's rules, hooks, live validation, errors placed next to the field.",
            code: "Valid<NewOrder>",
            doc: "validation",
        },
        Feature {
            icon: "⚿",
            title: "Auth & permissions",
            text: "Login, registration, reset, verification, API tokens, roles per tenant, policies, two-factor login, OAuth.",
            code: ".require_permission(\"orders.refund\")",
            doc: "authorization",
        },
        Feature {
            icon: "⟳",
            title: "Queues & scheduler",
            text: "Jobs with retries, chains and batches; a cron scheduler that is safe across servers. No Redis needed.",
            code: "state.queue.dispatch(SendInvoice { … })",
            doc: "queue",
        },
        Feature {
            icon: "✉",
            title: "Mail & notifications",
            text: "SMTP with failover, mail views, notifications by mail, in the database and as a live stream.",
            code: "state.notify(user, &OrderShipped { … })",
            doc: "mail",
        },
        Feature {
            icon: "▦",
            title: "UI kit, grid & HTMX",
            text: "72 accessible components, 112 icons, an editorial theme, a data grid with exports, and renox-blocks; htmx and Alpine.js built in.",
            code: "{{ grid(orders) }}",
            doc: "ui",
        },
        Feature {
            icon: "⬢",
            title: "One-binary deploys",
            text: "Views, translations, migrations and assets compiled in; systemd socket activation, so restarts refuse no connection.",
            code: "rnx build",
            doc: "operations",
        },
    ]
}

/// A layer of the request pipeline.
#[derive(Serialize)]
pub struct Layer {
    pub short: &'static str,
    pub name: &'static str,
    pub title: &'static str,
    pub text: &'static str,
    pub code: String,
}

pub fn pipeline() -> Vec<Layer> {
    let layer = |short, name, title, text, lang: &str, code: &str| Layer {
        short,
        name,
        title,
        text,
        code: renox_site::highlight::highlight(lang, code),
    };
    vec![
        layer(
            "HTTP",
            "request",
            "The request arrives",
            "Axum and Tokio underneath: async, on every core, nothing to install next to the binary.",
            "",
            "GET /orders/42 HTTP/1.1\nHost: shop.example.com\nCookie: renox_session=…",
        ),
        layer(
            "SEC",
            "security",
            "Security headers & CSP",
            "nosniff, frame options, HSTS and a Content-Security-Policy with a nonce per request, on every response. Trusted hosts and proxies too.",
            "",
            "content-security-policy: default-src 'self';\n  script-src 'self' 'nonce-r4Nd0m'\nstrict-transport-security: max-age=31536000",
        ),
        layer(
            "SES",
            "session",
            "Sessions",
            "An encrypted, signed cookie by default; a database driver when you outgrow it. Flash data, remember-me, per-device logout.",
            "rust",
            "session.flash(\"status\", \"Saved\")?;\nsession.put(\"cart\", &cart)?;",
        ),
        layer(
            "AUTH",
            "auth",
            "Authentication",
            "The user is loaded once per request, from the session or a Bearer token with its abilities, with roles and permissions per tenant.",
            "rust",
            "Routes::new()\n    .get(\"/orders\", index)\n    .require_permission(\"orders.view\")",
        ),
        layer(
            "CSRF",
            "csrf",
            "CSRF",
            "Every unsafe method is checked: a hidden field for forms, the X-XSRF-TOKEN header for htmx and JSON.",
            "html",
            "<form method=\"post\">\n  {{ csrf_field() }}\n  …\n</form>",
        ),
        layer(
            "FN",
            "handler",
            "Your handler",
            "A plain async fn. Extractors validate the input before it runs; ? turns any error into the right response.",
            "rust",
            "async fn store(State(state): State<AppState>,\n               Valid(form): Valid<NewOrder>) -> Result<Redirect> {\n    let order = form.into_order().insert(&state.db).await?;\n    Redirect::route(\"orders.show\", &[&order.id])\n}",
        ),
        layer(
            "VIEW",
            "view",
            "Views",
            "MiniJinja templates, a UI kit of 70 components, htmx fragments and out-of-band swaps, error pages in your layout.",
            "html",
            "{% call card(\"Order #\" ~ order.id) %}\n  {{ infolist(order) }}\n{% endcall %}",
        ),
        layer(
            "200",
            "response",
            "The response",
            "A full page, an htmx fragment, JSON, a download or a stream, with a request id, and an ETag when you ask for one.",
            "",
            "HTTP/1.1 200 OK\nx-request-id: W5-L3oxyBUY2tvYT\ncontent-type: text/html; charset=utf-8",
        ),
    ]
}

/// One Laravel ↔ Renox comparison.
#[derive(Serialize)]
pub struct Pair {
    pub name: &'static str,
    pub laravel_file: &'static str,
    pub laravel: String,
    pub renox_file: &'static str,
    pub renox: String,
}

pub fn pairs() -> Vec<Pair> {
    let pair = |name, laravel_file, laravel: &str, renox_file, renox: &str| Pair {
        name,
        laravel_file,
        laravel: renox_site::highlight::highlight("php", laravel),
        renox_file,
        renox: renox_site::highlight::highlight("rust", renox),
    };
    vec![
        pair(
            "Routes",
            "routes/web.php",
            "Route::get('/orders', [OrderController::class, 'index'])\n    ->name('orders.index')\n    ->middleware('auth');\n\nRoute::post('/orders', [OrderController::class, 'store'])\n    ->name('orders.store')\n    ->middleware(['auth', 'throttle:10,1']);",
            "src/app/orders/mod.rs",
            "Routes::new()\n    .get(\"/orders\", index).name(\"orders.index\")\n    .post(\"/orders\", store).name(\"orders.store\")\n    .throttle(10, Duration::from_secs(60))\n    .require_auth()",
        ),
        pair(
            "Models",
            "app/Models/Order.php",
            "class Order extends Model\n{\n    use SoftDeletes;\n\n    protected $fillable = ['customer_id', 'total'];\n\n    public function customer()\n    {\n        return $this->belongsTo(Customer::class);\n    }\n}",
            "src/app/orders/model.rs",
            "#[derive(Model, Serialize, Default)]\n#[model(soft_deletes)]\npub struct Order {\n    pub id: i64,\n    pub customer_id: i64,\n    pub total: i64,\n    pub deleted_at: Option<DateTime>,\n}\n\n// The customers of a page of orders, in one query:\nlet customers = belongs_to::<Customer, _, _>(\n    &db, &orders, |o| o.customer_id,\n).await?;",
        ),
        pair(
            "Validation",
            "app/Http/Requests/StoreOrder.php",
            "class StoreOrder extends FormRequest\n{\n    public function rules(): array\n    {\n        return [\n            'email' => 'required|email',\n            'quantity' => 'required|integer|min:1',\n        ];\n    }\n}",
            "src/app/orders/form.rs",
            "#[derive(Deserialize, Validate)]\npub struct StoreOrder {\n    #[validate(required, email)]\n    pub email: String,\n    #[validate(required, min = 1)]\n    pub quantity: i64,\n}\n\nasync fn store(Valid(form): Valid<StoreOrder>) -> Result<Redirect> { … }",
        ),
        pair(
            "Jobs",
            "app/Jobs/SendInvoice.php",
            "class SendInvoice implements ShouldQueue\n{\n    public $tries = 5;\n\n    public function __construct(public int $orderId) {}\n\n    public function handle(): void\n    {\n        Mail::to($this->order->email)->send(new Invoice($this->order));\n    }\n}\n\nSendInvoice::dispatch($order->id);",
            "src/app/orders/jobs.rs",
            "#[derive(Serialize, Deserialize)]\npub struct SendInvoice { pub order_id: i64 }\n\nimpl Job for SendInvoice {\n    const NAME: &'static str = \"send-invoice\";\n    const MAX_ATTEMPTS: u32 = 5;\n\n    async fn handle(self, ctx: JobContext) -> Result {\n        let order = Order::find_or_404(&ctx.state.db, self.order_id).await?;\n        ctx.state.mailer.send(invoice(&order)).await\n    }\n}\n\nstate.queue.dispatch(SendInvoice { order_id: order.id }).await?;",
        ),
    ]
}

/// A row of the comparison table: Renox, Laravel, Loco, Axum + crates.
#[derive(Serialize)]
pub struct Compare {
    pub what: &'static str,
    pub cells: [&'static str; 4],
}

pub fn comparison() -> Vec<Compare> {
    vec![
        Compare {
            what: "Language",
            cells: ["Rust", "PHP", "Rust", "Rust"],
        },
        Compare {
            what: "Checked at compile time",
            cells: ["yes", "no", "yes", "yes"],
        },
        Compare {
            what: "Auth, roles, two-factor, OAuth",
            cells: ["yes", "with packages", "auth only", "no"],
        },
        Compare {
            what: "Queue and scheduler without Redis",
            cells: ["yes", "yes", "yes", "no"],
        },
        Compare {
            what: "UI kit and data grid",
            cells: ["yes", "with Filament", "no", "no"],
        },
        Compare {
            what: "Admin panel, billing, OAuth plugins",
            cells: ["yes", "with packages", "no", "no"],
        },
        Compare {
            what: "Deploys as one file",
            cells: ["yes", "no", "yes", "yes"],
        },
        Compare {
            what: "Wiring you write yourself",
            cells: ["none", "none", "some", "all of it"],
        },
    ]
}

/// An example app in the showcase.
#[derive(Serialize)]
pub struct Example {
    pub name: &'static str,
    pub text: &'static str,
}

pub fn examples() -> Vec<Example> {
    vec![
        Example {
            name: "bikeshop",
            text: "Three stores that sell, rent and service bikes",
        },
        Example {
            name: "hello",
            text: "The smallest app: a guestbook",
        },
    ]
}

/// The benchmark results shown on the page: `content/benchmarks.json`,
/// written from a run of `benchmarks/run.sh` (`null` until there is one).
pub fn benchmarks() -> Option<serde_json::Value> {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../content/benchmarks.json")).ok()?;
    (!value.is_null()).then_some(value)
}

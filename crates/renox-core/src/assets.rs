use std::sync::LazyLock;

use axum::Router;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::get;

use crate::AppState;

/// The version of the bundled htmx.
pub const HTMX_VERSION: &str = "2.0.11";
/// The version of the bundled Alpine.js.
pub const ALPINE_VERSION: &str = "3.17.4";

const HTMX: &str = include_str!("../assets/htmx.min.js");
const ALPINE: &str = include_str!("../assets/alpine.min.js");
/// Alpine's build for `CSP=strict`: no `eval`, simpler expressions.
const ALPINE_CSP: &str = include_str!("../assets/alpine-csp.min.js");
/// The UI kit's styles and behavior (renox/ui.html).
pub(crate) const UI_CSS: &str = include_str!("../assets/renox-ui.css");
const UI_JS_SOURCE: &str = include_str!("../assets/renox-ui.js");

/// The kit's parts few pages have, each a JavaScript module renox-ui.js
/// loads when it finds the part's markup (`MARKERS` there): `(name, body)`.
const UI_PARTS: [(&str, &str); 4] = [
    ("chart", include_str!("../assets/renox-ui-chart.js")),
    ("wizard", include_str!("../assets/renox-ui-wizard.js")),
    ("repeater", include_str!("../assets/renox-ui-repeater.js")),
    ("tags", include_str!("../assets/renox-ui-tags.js")),
];

/// Where renox-ui.js finds its parts' URLs.
const UI_PARTS_SLOT: &str = "{/*renox:parts*/}";

/// The parts' hashed URLs, in `UI_PARTS`' order.
static UI_PART_URLS: LazyLock<Vec<String>> = LazyLock::new(|| {
    UI_PARTS
        .iter()
        .map(|(name, body)| format!("/_renox/ui-{name}-{:016x}.js", fnv1a(body)))
        .collect()
});

/// renox-ui.js with its parts' URLs filled in, so its own hash changes
/// whenever a part does.
pub(crate) static UI_JS: LazyLock<String> = LazyLock::new(|| {
    let urls = UI_PARTS
        .iter()
        .zip(UI_PART_URLS.iter())
        .map(|((name, _), url)| format!("{name}: \"{url}\""))
        .collect::<Vec<_>>()
        .join(", ");
    UI_JS_SOURCE.replace(UI_PARTS_SLOT, &format!("{{ {urls} }}"))
});

/// The data grid's styles and behavior (renox/grid.html), and the date
/// range calendar its date filters use.
pub(crate) const GRID_CSS: &str = include_str!("../assets/renox-grid.css");
pub(crate) const GRID_JS: &str = include_str!("../assets/renox-grid.js");
/// The version of the bundled Cally (calendar web components, MIT).
pub const CALLY_VERSION: &str = "0.9.2";
const CALLY: &str = include_str!("../assets/cally.js");

/// The version of the bundled Idiomorph (DOM morphing, 0BSD), used by live
/// components.
pub const IDIOMORPH_VERSION: &str = "0.7.3";
const IDIOMORPH: &str = include_str!("../assets/idiomorph-0.7.3.min.js");
/// The live components' script (renox-live.js).
pub(crate) const LIVE_JS: &str = include_str!("../assets/renox-live.js");

/// The kit's fonts (SIL Open Font License 1.1; assets/fonts has the
/// licences): Inter for text, Poppins for titles and figures, Latin subsets.
/// Their names carry the font's version, so they are cached for good.
pub(crate) const FONTS: [(&str, &[u8]); 4] = [
    (
        "/_renox/fonts/inter-latin-wght-4.1.woff2",
        include_bytes!("../assets/fonts/inter-latin-wght-4.1.woff2"),
    ),
    (
        "/_renox/fonts/poppins-latin-500-4.003.woff2",
        include_bytes!("../assets/fonts/poppins-latin-500-4.003.woff2"),
    ),
    (
        "/_renox/fonts/poppins-latin-600-4.003.woff2",
        include_bytes!("../assets/fonts/poppins-latin-600-4.003.woff2"),
    ),
    (
        "/_renox/fonts/poppins-latin-700-4.003.woff2",
        include_bytes!("../assets/fonts/poppins-latin-700-4.003.woff2"),
    ),
];

static GRID_URLS: LazyLock<[String; 3]> = LazyLock::new(|| {
    [
        format!("/_renox/grid-{:016x}.css", fnv1a(GRID_CSS)),
        format!("/_renox/grid-{:016x}.js", fnv1a(GRID_JS)),
        format!("/_renox/cally-{:016x}.js", fnv1a(CALLY)),
    ]
});

/// `{{ renox_grid() }}` (the `grid` macro calls it): the grid's stylesheet
/// and scripts. Twice on a page is harmless.
pub(crate) fn grid_tags() -> String {
    let [css, js, cally] = &*GRID_URLS;
    format!(
        "<link rel=\"stylesheet\" href=\"{css}\">\n<script src=\"{js}\" defer></script>\n<script type=\"module\" src=\"{cally}\"></script>"
    )
}

/// `{{ renox_calendar() }}` (the kit's `date_picker` calls it once per
/// page): the calendar web components, the same file the grid loads.
pub(crate) fn calendar_tags() -> String {
    let [_, _, cally] = &*GRID_URLS;
    format!("<script type=\"module\" src=\"{cally}\"></script>")
}

static LIVE_URLS: LazyLock<[String; 2]> = LazyLock::new(|| {
    [
        format!("/_renox/idiomorph-{IDIOMORPH_VERSION}.min.js"),
        format!("/_renox/live-{:016x}.js", fnv1a(LIVE_JS)),
    ]
});

/// `{{ renox_live() }}` (once per page, from renox/live.html): Idiomorph,
/// then the live components' script.
pub(crate) fn live_tags() -> String {
    let [idiomorph, live] = &*LIVE_URLS;
    format!("<script src=\"{idiomorph}\" defer></script>\n<script src=\"{live}\" defer></script>")
}

static UI_URLS: LazyLock<[String; 2]> = LazyLock::new(|| {
    [
        format!("/_renox/ui-{:016x}.css", fnv1a(UI_CSS)),
        format!("/_renox/ui-{:016x}.js", fnv1a(&UI_JS)),
    ]
});

/// `{{ renox_ui() }}`: the kit's stylesheet and script, for the `<head>`,
/// and a preload of the text font so the first paint uses it;
/// `renox_ui(styles=false)` only the script, for an app with its own copy
/// of the styles (`ui:publish`).
pub(crate) fn ui_tags(styles: bool) -> String {
    let [css, js] = &*UI_URLS;
    let script = format!("<script src=\"{js}\" defer></script>");
    if styles {
        let font = FONTS[0].0;
        format!(
            "<link rel=\"preload\" href=\"{font}\" as=\"font\" type=\"font/woff2\" crossorigin>\n<link rel=\"stylesheet\" href=\"{css}\">\n{script}"
        )
    } else {
        script
    }
}

/// `my-app ui:publish`: copies the kit into the app, to change it there.
pub(crate) fn publish_ui(config: &crate::Config, force: bool) -> anyhow::Result<()> {
    let view = config.views_path.join("components/ui.html");
    let css = config.public_path.join("css/renox-ui.css");
    for path in [&view, &css] {
        if path.exists() && !force {
            anyhow::bail!("{} exists; add --force to replace it", path.display());
        }
    }
    let template = include_str!("../views/ui.html").replace(
        "{% from \"renox/ui.html\" import",
        "{% from \"components/ui.html\" import",
    );
    for (path, body) in [(&view, template.as_str()), (&css, UI_CSS)] {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, body)?;
        println!("Wrote {}", path.display());
    }
    println!(
        "Import from \"components/ui.html\", and in the layout replace {{{{ renox_ui() }}}} with\n  \
         <link rel=\"stylesheet\" href=\"{{{{ asset('css/renox-ui.css') }}}}\">{{{{ renox_ui(styles=false) }}}}"
    );
    Ok(())
}

const RENOX: &str = r#"(function () {
  // Send the CSRF token with every HTMX request.
  document.addEventListener("htmx:configRequest", function (event) {
    var meta = document.querySelector('meta[name="csrf-token"]');
    if (meta) event.detail.headers["X-CSRF-Token"] = meta.content;
  });

  function formOf(elt) {
    return (elt && elt.closest && (elt.closest("[data-rx-live]") || elt.closest("form"))) || elt;
  }

  function clearErrors(form) {
    if (!form || !form.querySelectorAll) return;
    form.querySelectorAll("[data-renox-error]").forEach(function (el) { el.remove(); });
    form.querySelectorAll("[data-error-for]").forEach(function (el) { el.textContent = ""; });
    form.querySelectorAll("[aria-invalid]").forEach(function (el) { el.removeAttribute("aria-invalid"); });
  }

  // Show 422 validation errors next to the inputs that caused them: in an
  // element with data-error-for="field" if the form has one, otherwise in a
  // <p class="error"> inserted after the input.
  function showErrors(form, errors) {
    clearErrors(form);
    Object.keys(errors).forEach(function (field) {
      var message = (errors[field] || [])[0];
      if (!message) return;
      var name = CSS.escape(field);
      var input = form.querySelector('[name="' + name + '"]');
      // `items.0.name`: the input of a nested form is named `items[0][name]`.
      if (!input && field.indexOf(".") > 0) {
        var parts0 = field.split(".");
        var bracketed = parts0[0] + parts0.slice(1).map(function (p) { return "[" + p + "]"; }).join("");
        input = form.querySelector('[name="' + CSS.escape(bracketed) + '"]');
      }
      var slot = form.querySelector('[data-error-for="' + name + '"]');
      // `tags.1` / `photos.0`: an item of a list. Use the list's own input
      // (the item's one when there is one per item) and slot.
      var parts = field.split(".");
      if (!input && parts.length > 1) {
        var inputs = form.querySelectorAll('[name="' + CSS.escape(parts[0]) + '"]');
        input = inputs[parseInt(parts[1], 10)] || inputs[0] || null;
      }
      if (!slot && parts.length > 1) {
        slot = form.querySelector('[data-error-for="' + CSS.escape(parts[0]) + '"]');
      }
      if (input) input.setAttribute("aria-invalid", "true");
      if (slot) {
        slot.textContent = slot.textContent ? slot.textContent + " " + message : message;
        return;
      }
      var p = document.createElement("p");
      p.className = "error";
      p.setAttribute("data-renox-error", field);
      p.textContent = message;
      if (input) input.insertAdjacentElement("afterend", p);
      else form.prepend(p);
    });
    // First invalid input in page order, not in the (alphabetical) error order.
    var first = form.querySelector('[aria-invalid="true"]');
    if (first && first.focus) first.focus();
  }

  document.addEventListener("htmx:beforeRequest", function (event) {
    clearErrors(formOf(event.detail.elt));
  });

  document.addEventListener("htmx:beforeSwap", function (event) {
    var xhr = event.detail.xhr;
    var type = xhr.getResponseHeader("Content-Type") || "";
    if (xhr.status !== 422 || type.indexOf("application/json") !== 0) return;
    // Not swapped, and still an error: forms that reset themselves after a
    // successful request keep what the user typed.
    event.detail.shouldSwap = false;
    try {
      var body = JSON.parse(xhr.responseText);
      showErrors(formOf(event.detail.requestConfig.elt), body.errors || {});
    } catch (_) {}
  });

  // Live reload while developing: reload when a view, public or lang file
  // changes, or when the app restarts (a new boot id after reconnecting).
  // Analytics events from the server (renox::analytics::event): with an htmx
  // swap in its HX-Trigger, with a page in a <meta>.
  function track(events) {
    (events || []).forEach(function (e) {
      var params = e.params || {};
      if (typeof window.gtag === "function") window.gtag("event", e.name, params);
      if (window.dataLayer && document.querySelector('script[src*="googletagmanager.com/gtm.js"]')) {
        var entry = { event: e.name };
        Object.keys(params).forEach(function (key) { entry[key] = params[key]; });
        window.dataLayer.push(entry);
      }
      document.dispatchEvent(new CustomEvent("renox:tracked", { detail: e }));
    });
  }
  document.addEventListener("renox:analytics", function (event) {
    track(event.detail && event.detail.events);
  });
  var pending = document.querySelector('meta[name="renox-analytics"]');
  if (pending) {
    try { track(JSON.parse(pending.content)); } catch (_) {}
  }

  var live = document.querySelector('meta[name="renox-live"]');
  if (live && window.EventSource) {
    // Closed on `pagehide`: a page kept in the back/forward cache would hold
    // its stream open, and after a few navigations the browser's six
    // connections per host are used up and requests hang. Opened again on
    // `pageshow`; a new boot id then reloads a page restored after a restart.
    var boot = null;
    var source = null;
    var connect = function () {
      source = new EventSource("/_renox/live");
      source.addEventListener("boot", function (event) {
        if (boot !== null && boot !== event.data) location.reload();
        boot = event.data;
      });
      source.addEventListener("reload", function () { location.reload(); });
    };
    connect();
    window.addEventListener("pagehide", function () {
      if (source) source.close();
      source = null;
    });
    window.addEventListener("pageshow", function (event) {
      if (event.persisted && !source) connect();
    });
  }
})();
"#;

/// Content-hashed URLs, so browsers can cache the files forever.
static URLS: LazyLock<[String; 4]> = LazyLock::new(|| {
    [
        format!("/_renox/htmx-{HTMX_VERSION}.min.js"),
        format!("/_renox/alpine-{ALPINE_VERSION}.min.js"),
        format!("/_renox/renox-{:016x}.js", fnv1a(RENOX)),
        format!("/_renox/alpine-csp-{ALPINE_VERSION}.min.js"),
    ]
});

pub(crate) fn router() -> Router<AppState> {
    let [htmx, alpine, renox, alpine_csp] = &*URLS;
    let [ui_css, ui_js] = &*UI_URLS;
    let mut router = Router::new();
    for (path, bytes) in FONTS {
        router = router.route(path, get(move || async move { font(bytes) }));
    }
    for (&(_, body), url) in UI_PARTS.iter().zip(UI_PART_URLS.iter()) {
        router = router.route(url, get(move || async move { js(body) }));
    }
    router
        .route(
            ui_css,
            get(|| async { asset("text/css; charset=utf-8", UI_CSS) }),
        )
        .route(ui_js, get(|| async { js(UI_JS.as_str()) }))
        .route(
            &GRID_URLS[0],
            get(|| async { asset("text/css; charset=utf-8", GRID_CSS) }),
        )
        .route(&GRID_URLS[1], get(|| async { js(GRID_JS) }))
        .route(&GRID_URLS[2], get(|| async { js(CALLY) }))
        .route(&LIVE_URLS[0], get(|| async { js(IDIOMORPH) }))
        .route(&LIVE_URLS[1], get(|| async { js(LIVE_JS) }))
        .route(htmx, get(|| async { js(HTMX) }))
        .route(alpine, get(|| async { js(ALPINE) }))
        .route(renox, get(|| async { js(RENOX) }))
        .route(alpine_csp, get(|| async { js(ALPINE_CSP) }))
}

/// The files modules serve (`Registry::asset`), cached like Renox's own.
pub(crate) fn module_router(assets: &[crate::registry::StaticAsset]) -> Router<AppState> {
    assets.iter().fold(Router::new(), |router, file| {
        let file = *file;
        router.route(
            file.path,
            get(move || async move {
                (
                    [
                        (CONTENT_TYPE, file.content_type),
                        (CACHE_CONTROL, "public, max-age=31536000, immutable"),
                    ],
                    file.body,
                )
            }),
        )
    })
}

fn asset(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, content_type),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        body,
    )
}

fn font(body: &'static [u8]) -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "font/woff2"),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        body,
    )
}

fn js(body: &'static str) -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        body,
    )
}

/// The `<head>` tags every Renox page needs: the CSRF token, htmx, Alpine.js
/// and the script that sends the token with HTMX requests.
pub(crate) fn head_tags(csrf_token: &str, live: bool, strict_csp: bool) -> String {
    let [htmx, alpine, renox, alpine_csp] = &*URLS;
    // Under a strict CSP: Alpine's CSP build, and htmx without `eval`.
    let (alpine, htmx_config) = if strict_csp {
        (
            alpine_csp,
            "<meta name=\"htmx-config\" content='{\"allowEval\":false}'>\n",
        )
    } else {
        (alpine, "")
    };
    let live = if live {
        "<meta name=\"renox-live\" content=\"1\">\n"
    } else {
        ""
    };
    format!(
        "{live}{htmx_config}<meta name=\"csrf-token\" content=\"{csrf_token}\">\n\
         <script src=\"{htmx}\" defer></script>\n\
         <script src=\"{renox}\" defer></script>\n\
         <script src=\"{alpine}\" defer></script>"
    )
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod ui_tests {
    #[test]
    fn publishing_the_kit_copies_it_once() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::Config {
            views_path: dir.path().join("views"),
            public_path: dir.path().join("public"),
            ..crate::Config::default()
        };
        super::publish_ui(&config, false).unwrap();
        let view = std::fs::read_to_string(dir.path().join("views/components/ui.html")).unwrap();
        assert!(
            view.contains("{% macro input(") && !view.contains("from \"renox/ui.html\" import")
        );
        assert!(
            std::fs::read_to_string(dir.path().join("public/css/renox-ui.css"))
                .unwrap()
                .contains("--rx-accent")
        );
        assert!(
            super::publish_ui(&config, false).is_err(),
            "no overwrite without --force"
        );
        super::publish_ui(&config, true).unwrap();
        assert!(super::ui_tags(false).starts_with("<script"));
        let tags = super::ui_tags(true);
        assert!(tags.starts_with("<link rel=\"preload\""), "{tags}");
        assert!(tags.contains("<link rel=\"stylesheet\""), "{tags}");
    }

    /// renox-ui.js loads its rarely used parts as modules (#349): it has a
    /// slot for their URLs, filled in with each part's hashed one, and a
    /// marker per part to find it by.
    #[test]
    fn the_kit_script_names_its_parts() {
        assert_eq!(super::UI_JS_SOURCE.matches(super::UI_PARTS_SLOT).count(), 1);
        assert!(!super::UI_JS.contains(super::UI_PARTS_SLOT));
        for ((name, body), url) in super::UI_PARTS.iter().zip(super::UI_PART_URLS.iter()) {
            assert!(url.starts_with(&format!("/_renox/ui-{name}-")), "{url}");
            assert!(
                super::UI_JS.contains(&format!("{name}: \"{url}\"")),
                "{name}'s URL"
            );
            assert!(
                super::UI_JS_SOURCE.contains(&format!("\n    {name}: \"[data-rx-")),
                "{name}'s marker"
            );
            assert!(body.contains(&format!("kit.ready(\"{name}\"")), "{name}");
        }
        // Published kits (`renox_ui(styles=false)`) load the same script.
        let [_, js] = &*super::UI_URLS;
        assert!(super::ui_tags(false).contains(js.as_str()));
    }

    /// A card stretched to its row's height keeps its title next to its body (#128).
    #[test]
    fn a_stretched_card_keeps_its_rows_at_the_top() {
        let start = super::UI_CSS
            .find("\n.rx-card {")
            .expect("the .rx-card rule");
        let rule = &super::UI_CSS[start..];
        let rule = &rule[..rule.find('}').unwrap()];
        assert!(rule.contains("display: grid;"), "{rule}");
        assert!(rule.contains("align-content: start;"), "{rule}");
    }

    /// The kit's dialogs centre themselves, so a `* { margin: 0 }` reset such as
    /// Tailwind's preflight doesn't leave them in the top-left corner (#133).
    #[test]
    fn dialogs_set_their_own_margin() {
        for (css, selector) in [
            (super::UI_CSS, "\n.rx-sheet {"),
            (super::GRID_CSS, "\n.rx-grid__dialog {"),
        ] {
            let start = css.find(selector).expect(selector);
            let rule = &css[start..];
            let rule = &rule[..rule.find('}').unwrap()];
            assert!(rule.contains("margin: auto;"), "{rule}");
        }
    }

    /// Server-Sent Events streams close when the page is hidden and open again
    /// when it comes back from the back/forward cache, or every cached page
    /// keeps a connection and the browser's six per host run out (#226).
    #[test]
    fn event_streams_close_on_pagehide_and_reopen_on_pageshow() {
        for (script, opened) in [
            (super::RENOX, "new EventSource(\"/_renox/live\")"),
            (super::UI_JS.as_str(), "new EventSource(streamUrl)"),
        ] {
            let start = script.find(opened).expect(opened);
            let (mut from, mut to) = (start.saturating_sub(1200), (start + 1200).min(script.len()));
            while !script.is_char_boundary(from) {
                from -= 1;
            }
            while !script.is_char_boundary(to) {
                to += 1;
            }
            let around = &script[from..to];
            for needle in [
                "\"pagehide\"",
                ".close()",
                "\"pageshow\"",
                "event.persisted",
            ] {
                assert!(around.contains(needle), "{needle} near {opened}");
            }
        }
    }

    /// Idiomorph and the live script are served under their own names, cached for good.
    #[tokio::test]
    async fn the_live_scripts_are_served_immutable() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let kernel = crate::App::with_config(crate::Config::default())
            .boot()
            .await
            .unwrap();
        let tags = super::live_tags();
        assert!(tags.find("idiomorph").unwrap() < tags.find("/_renox/live-").unwrap());
        for url in super::LIVE_URLS.iter() {
            assert!(tags.contains(url.as_str()), "{url}");
            let res = kernel
                .router()
                .oneshot(Request::get(url.as_str()).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{url}");
            let cache = res.headers()[super::CACHE_CONTROL].to_str().unwrap();
            assert!(cache.contains("immutable"), "{url}: {cache}");
        }
    }
}

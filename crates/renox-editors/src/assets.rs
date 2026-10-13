//! The editors' scripts and styles, compiled into the crate and served by
//! the app (`Registry::asset`) with a year-long cache.
//!
//! A page loads two files, and only when it has an editor or a code entry:
//! the stylesheet and `editors.js`, a JavaScript module. The module loads
//! the libraries a page needs as it finds their fields (Trix for a rich
//! text editor, CodeJar and Prism for code), and a browser runs each module
//! once per page, even when htmx brings a second form in.

use std::sync::LazyLock;

use renox::Registry;

/// The version of the bundled Trix (the rich text editor, MIT).
pub const TRIX_VERSION: &str = "2.1.19";
/// The version of the bundled Prism (syntax highlighting, MIT).
pub const PRISM_VERSION: &str = "1.30.0";
/// The version of the bundled CodeJar (the code editor, MIT).
pub const CODEJAR_VERSION: &str = "4.3.0";

const TRIX_JS: &str = include_str!("../assets/vendor/trix-2.1.19.min.js");
const TRIX_CSS: &str = include_str!("../assets/vendor/trix-2.1.19.css");
const PRISM_JS: &str = include_str!("../assets/vendor/prism-1.30.0.min.js");
const CODEJAR_JS: &str = include_str!("../assets/vendor/codejar-4.3.0.js");
const EDITORS_JS: &str = include_str!("../assets/editors.js");
const EDITORS_CSS: &str = include_str!("../assets/editors.css");

/// Where the files are served.
const BASE: &str = "/_renox/editors";

/// The stylesheet: Trix's own, then the editors' (which restyle it like the
/// UI kit), as one file.
static CSS: LazyLock<String> = LazyLock::new(|| format!("{TRIX_CSS}\n{EDITORS_CSS}"));

/// `(path, content type, body)` of every file; the libraries' paths carry
/// their versions, the editors' own a hash of their content.
static FILES: LazyLock<Vec<(&'static str, &'static str, &'static [u8])>> = LazyLock::new(|| {
    const JS: &str = "text/javascript; charset=utf-8";
    let leak = |path: String| -> &'static str { Box::leak(path.into_boxed_str()) };
    vec![
        (
            leak(format!("{BASE}/editors-{:016x}.css", fnv1a(&CSS))),
            "text/css; charset=utf-8",
            CSS.as_bytes(),
        ),
        (
            leak(format!("{BASE}/editors-{:016x}.js", fnv1a(EDITORS_JS))),
            JS,
            EDITORS_JS.as_bytes(),
        ),
        (
            leak(format!("{BASE}/trix-{TRIX_VERSION}.min.js")),
            JS,
            TRIX_JS.as_bytes(),
        ),
        (
            leak(format!("{BASE}/prism-{PRISM_VERSION}.min.js")),
            JS,
            PRISM_JS.as_bytes(),
        ),
        (
            leak(format!("{BASE}/codejar-{CODEJAR_VERSION}.js")),
            JS,
            CODEJAR_JS.as_bytes(),
        ),
    ]
});

/// Serves every file.
pub(crate) fn register(app: &mut Registry) {
    for (path, content_type, body) in FILES.iter() {
        app.asset(path, content_type, body);
    }
}

/// `{{ renox_editors() }}`: the stylesheet and the module, which the
/// editors' macros print once per page. The module is told where the
/// libraries are, so it loads only those the page needs.
pub(crate) fn tags() -> String {
    let (css, js) = (FILES[0].0, FILES[1].0);
    format!(
        "<link rel=\"stylesheet\" href=\"{css}\">\n<script type=\"module\" src=\"{js}\" data-renox-editors data-trix=\"{}\" data-prism=\"{}\" data-codejar=\"{}\"></script>",
        FILES[2].0, FILES[3].0, FILES[4].0
    )
}

/// FNV-1a, as Renox hashes its own files' names.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

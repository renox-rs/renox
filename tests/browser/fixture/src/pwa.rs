//! Installable-app pages (pwa.test.mjs): a guest page and a logged-in page,
//! both with `<rx-pwa/>` in their head. The fixture's `App::pwa(..)` call is
//! in `main.rs`; the icons are in `pwa-icons/`, the offline page is
//! `views/offline.html`.

use renox::prelude::*;

pub struct Installable;

impl Module for Installable {
    fn name(&self) -> &'static str {
        "installable"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/pwa", || async {
                view(
                    "pwa.html",
                    context! { heading => "Installable page", other => "/pwa/account" },
                )
            })
            // Only for a logged-in user: the worker must never store it.
            .get("/pwa/account", |_user: AuthUser| async {
                view(
                    "pwa.html",
                    context! { heading => "Private account page", other => "/pwa" },
                )
            })
    }
}

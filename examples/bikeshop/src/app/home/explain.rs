//! "About this page" entries for the home area (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "home",

            path: "/",
            title: "Home",
            purpose: "The shop's front door: what the three stores offer (buy, rent, \
                  service), the best-selling bikes, the categories, and where to start. \
                  It is also the first page of the public layout every customer-facing \
                  page shares.",
            who: "Anyone who opens the site: visitors and customers. Developers reading the \
              example start here too, with the \"About this page\" button.",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Routes::get",
                    why: "One `GET /` route named `home`, so every link to it is \
                      `route('home')` and the name keys this explanation. Links never \
                      spell the path out, so moving the page later is a one-line change.",
                },
                Feature {
                    api: "UI kit: navbar",
                    why: "The public layout's bar (`navbar`, `nav_links`, `menu`) comes from \
                      the kit: keyboard use, the phone layout and dark mode are already \
                      done, and the shop only sets its accent colour in `public/app.css`. \
                      Each link is highlighted with \
                      `nav_link(…, active=route_is('catalog.*'))`, by route name rather \
                      than by comparing paths.",
                },
                Feature {
                    api: "Brand tokens over the kit",
                    why: "The storefront's look (cream page, teal and terracotta, large \
                      headlines, pill buttons, rounder corners; #328) is the kit's own \
                      tokens set to the shop's values in `public/theme.css` \
                      (`--rx-bg`, `--rx-accent`, `--rx-radius-l`, `--rx-type-title`…), \
                      so every kit component on every page follows, in light and dark \
                      mode, without a component of the shop's own. The hero, the photo \
                      tiles and the service tiles are CSS grids on the kit's spacing \
                      tokens; the hero is a component (`<app-hero>`, \
                      `components/hero.html`) the rent and plans pages share.",
                },
                Feature {
                    api: "App::share",
                    why: "The featured bikes (the best sellers) and the category tree come from \
                      the catalog area, which shares them with every view as `storefront` \
                      (`Registry::share` in its `register`). The value is computed only \
                      when the path is `/`, so other pages run no query for it, and the \
                      home module stays ignorant of the catalogue: the page uses \
                      `<app-catalog-home>` (`components/catalog_home.html`).",
                },
                Feature {
                    api: "UI kit: card_grid + media_card",
                    why: "The featured bikes are the same `media_card`s as the catalogue's, in a \
                      `card_grid`, so a bike looks the same everywhere; on public pages \
                      `theme.css` shows them as a photo with the text under it. The \
                      photos are free Unsplash photos (`public/images/CREDITS.md`) in \
                      WebP; the seeders give each product one of its category's photos. \
                      The categories are photo tiles linking to the catalogue.",
                },
                Feature {
                    api: "Routes::etag",
                    why: "`.etag()` hashes the page and answers `304 Not Modified` to a \
                      browser that already has it. The cart's count is fetched after the \
                      page loads (`cart.mini`), so it doesn't make the page differ per \
                      visitor. The layout's per-request CSP nonce still does, so for now \
                      the home page is always sent whole (a Renox gap, issue #306).",
                },
                Feature {
                    api: "seo()",
                    why: "`seo(title=…, description=…)` in the page's `seo` block writes the \
                      shop's name and lead as the title and description, with the \
                      canonical address and the Open Graph tags for shared links, without \
                      the page writing any `<meta>` by hand.",
                },
                Feature {
                    api: "App::detect_locale",
                    why: "A visitor whose browser asks for Spanish gets the Spanish texts \
                      straight away, without a choice to make first. The language menu \
                      then remembers a choice with `renox::i18n::remember_locale`, which \
                      wins over the browser from then on.",
                },
                Feature {
                    api: "App::embed",
                    why: "Views, translations and `public/` (motion.dev included) are compiled \
                      into the binary for production, so the shop deploys as one file and \
                      a page can't go out with a template from another version.",
                },
                Feature {
                    api: "motion.dev (vendored)",
                    why: "The page's sections (`data-bs-reveal`) slide in with `transform` and \
                      `opacity` only (`public/app.js`), from a copy of Motion served by the \
                      app itself: no CDN to allow in the Content-Security-Policy, and \
                      `prefers-reduced-motion` turns it off.",
                },
            ],
            under_hood: "The handler returns `view(\"home/index.html\")` with no data of its \
                     own. The catalog area's `storefront` share runs eight queries: one for \
                     the category tree, one for the eight best-selling bikes (by units sold \
                     in `order_items`), and four for their cards (photos, brands, \
                     categories, variants, loaded for all eight at once), and two small ones \
                     for the hero's figures (how many bikes, the stores). The view \
                     middleware renders the page inside `layouts/app.html`, with the \
                     shared `explain_panels` value and this explanation (the \
                     `about_page(…)` template function). After the page loads, the cart \
                     button asks `cart.mini` for its count. The language menu posts to \
                     `locale.update`, which stores the choice in the session and goes \
                     `Back`; for someone logged in, the accounts area's middleware also \
                     saves it on the account.",
            docs: &[
                "docs/routing.md#apps-modules-and-routes",
                "docs/routing.md#etags",
                "docs/ui.md#navigation-and-page-structure",
                "docs/ui.md#the-current-route-conditional-classes-loops",
                "docs/ui.md#components-see-the-request",
                "docs/laravel.md#blade--minijinja",
                "docs/laravel.md#cache-storage-sessions-cookies-and-translations",
                "docs/laravel.md#one-file-to-deploy",
            ],
            sources: &[
                "examples/bikeshop/src/app/home/mod.rs",
                "examples/bikeshop/resources/views/home/index.html",
                "examples/bikeshop/resources/views/components/catalog_home.html",
                "examples/bikeshop/src/app/catalog/mod.rs",
                "examples/bikeshop/resources/views/layouts/app.html",
                "examples/bikeshop/resources/views/components/nav_cart.html",
                "examples/bikeshop/src/app/accounts/locale.rs",
                "examples/bikeshop/public/app.js",
                "examples/bikeshop/public/theme.css",
                "examples/bikeshop/resources/views/components/hero.html",
                "examples/bikeshop/public/images/CREDITS.md",
                "examples/bikeshop/tests/about.rs",
                "examples/bikeshop/tests/catalog.rs",
                "tests/browser/bikeshop-about.test.mjs",
            ],
            code: &[
                Code {
                    title: "Routes: one named `GET /` with an ETag, and a handler with no data",
                    region: "home.routes",
                },
                Code {
                    title: "Template: the hero, a macro the rent and plans pages share",
                    region: "home.template",
                },
                Code {
                    title: "Share: the catalogue gives `/` its storefront, and nothing else",
                    region: "home.storefront",
                },
            ],
        },
        Explanation {
            route: "stores.site",
            path: "/",
            title: "Store page on its own host",
            purpose: "Each store has a page of its own on its own host: `north.localhost:3000` \
                  in development, `north.example.com` in production. It shows the store's \
                  address, phone and opening hours, how many rental bikes are free there \
                  right now, and links into the shop. The home page's store tiles link here.",
            who: "Visitors who look a store up, and anyone a store gives its own address to \
              (a flyer, a map listing).",
            audience: &[Audience::Visitor, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Routes::domain",
                    why: "`Routes::domain(\"{store}.localhost\", …)` serves the page only on \
                      hosts that match the pattern, and those hosts get only these routes \
                      (plus Renox's own files), so `/` there is the store's page while `/` \
                      on the shop's host stays the home page. The domain comes from \
                      `BIKESHOP_STORE_DOMAIN` (default `localhost`): browsers send every \
                      `*.localhost` name to this machine, so it works locally with no DNS.",
                },
                Feature {
                    api: "DomainParams",
                    why: "The handler reads the store's slug from the host (`domain.get(\"store\")`) \
                      the way `Path` reads a path parameter, and answers 404 for a host \
                      whose store doesn't exist.",
                },
                Feature {
                    api: "Routes::fallback",
                    why: "A `fallback` inside the domain sends any other path on a store's host \
                      (an old link, a typo) to the store's page instead of a 404, without \
                      touching the shop's own fallback.",
                },
                Feature {
                    api: "Routes::get",
                    why: "The route has a name, `stores.site`, like any other, so it is listed \
                      by `route:list` (with its DOMAIN) and keys this explanation. Renox \
                      names the current route per host, so the panel finds this page and \
                      not the home page, though both are `/`.",
                },
                Feature {
                    api: "UI kit: stats + infolist",
                    why: "The page is the kit's `stat`s for the figures, an `infolist` for the \
                      contact details (a `tel:` link and a copy button for the email) and a \
                      `table` for the hours, under the shop's own hero, so it matches the \
                      shop with no CSS of its own beyond the layout's grid.",
                },
            ],
            under_hood: "The host `north.localhost:3000` matches `{store}.localhost`, so Renox \
                     routes the request with the domain's router only. The handler finds \
                     the store by its slug (one query), loads its address with its city and \
                     country (three), counts the rental bikes standing there and the free \
                     ones (two), and the catalogue variants it has on its shelves (one). \
                     The page's layout, `layouts/store.html`, links back to the shop with \
                     absolute addresses made from `APP_URL`, since the shop's pages aren't \
                     on this host; the kit's files and `public/` are served on every host. \
                     `stores::site_url` makes the address the home page links to.",
            docs: &[
                "docs/routing.md#other-hosts-and-the-fallback",
                "docs/routing.md#route-urls-and-the-current-route",
                "docs/operations.md#app_url",
            ],
            sources: &[
                "examples/bikeshop/src/app/home/stores.rs",
                "examples/bikeshop/resources/views/home/store.html",
                "examples/bikeshop/resources/views/layouts/store.html",
                "examples/bikeshop/tests/catalog.rs",
            ],
            code: &[
                Code {
                    title: "Routes: the store's host, its page and its fallback",
                    region: "stores.routes",
                },
                Code {
                    title: "Handler: the store from the host with `DomainParams`",
                    region: "stores.handler",
                },
                Code {
                    title: "Template: the kit's stats with absolute links",
                    region: "stores.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![]
}

//! "About this page" entries for the catalog area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const SOURCES_LISTING: &[&str] = &[
    "examples/bikeshop/src/app/catalog/browse.rs",
    "examples/bikeshop/src/app/catalog/filters.rs",
    "examples/bikeshop/src/app/catalog/model.rs",
    "examples/bikeshop/resources/views/catalog/index.html",
    "examples/bikeshop/public/catalog/catalog.css",
    "examples/bikeshop/public/catalog/catalog.js",
    "examples/bikeshop/tests/catalog.rs",
    "tests/browser/bikeshop-catalog.test.mjs",
];

const FILTERS_FROM_QUERY: Feature = Feature {
    api: "Query<T>",
    why: "Every filter is a query-string value (`?brand=trek&size=M&price_max=900000`), \
          so a filtered page can be shared, bookmarked and reloaded. `Filters::apply` turns \
          them into conditions on `Product::query()`: `where_in_query` for brands and \
          sizes, `where_raw` with bound values for the `EXISTS` sub-queries (a variant \
          priced within the range, stock at a store, a part that fits), and each database's JSON \
          operator for the specifications. Nothing the visitor typed is written into the SQL.",
};

const PAGINATION: Feature = Feature {
    api: "Paginated",
    why: "`.paginate(db, filters.page, PER_PAGE)` (24 a page) runs the count and the page \
          in two queries and gives the template what Renox's `pagination` macro \
          (`renox/pagination.html`) needs; its `page_url(n)` keeps the filters in every page \
          link, so page 3 of a filtered list is still filtered. The links are boosted by \
          htmx and swap only the results (`hx-select`).",
};

const FRAGMENTS: Feature = Feature {
    api: "htmx fragments",
    why: "The filter form is a plain `GET` form. With htmx, a change sends it and the \
          handler answers only the `results` block (`view(…).fragment(\"results\")`), \
          which replaces `#results` while `hx-push-url` keeps the address in step. Without \
          JavaScript the same form reloads the page: one handler, both ways.",
};

const NO_N_PLUS_ONE: Feature = Feature {
    api: "renox::db::relations",
    why: "The cards' brands, categories, variants (for \"from …\" prices) and first photos \
          are loaded for the whole page at once with `belongs_to` and `has_many` \
          (`Card::load`): four queries for 24 products or 2, never one per product. \
          `tests/catalog.rs` counts them with `capture_queries` on the large seed.",
};

const ETAG: Feature = Feature {
    api: "Routes::etag",
    why: "`.etag()` hashes each answer and replies `304 Not Modified`, without the body, \
          when the browser or a crawler already has that version. The sitemap gets 304s \
          every time. HTML pages carry the layout's per-request CSP nonce in their script \
          tags, so today their hash changes on every request and they are always sent \
          whole (a Renox gap); the cart's count is fetched separately \
          (`cart.mini`) so it won't make pages differ once that is solved.",
};

const SEO: Feature = Feature {
    api: "seo()",
    why: "`seo(title=…, description=…, canonical=…)` writes the title, the description, \
          the canonical address (without the filters, so search engines index one page \
          per category) and the Open Graph and Twitter tags.",
};

const SOFT_DELETES: Feature = Feature {
    api: "#[model(soft_deletes)]",
    why: "Discontinued products are soft deleted: `Product::query()` leaves them out of \
          every list, search and facet, while past orders still point at them.",
};

const RANGE_SLIDER: Feature = Feature {
    api: "renox-blocks",
    why: "The kit has no price range slider, so the `range_slider` block \
          (the renox-blocks crate) sends two plain fields, `price_min` \
          and `price_max`, that the filters read like any other.",
};

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "catalog.index",
            path: "/shop",
            title: "All products",
            purpose: "The whole catalogue in one list: bikes, gear and spare parts, filtered by \
                      brand, price, size, wheel size, frame material, e-bike and stock at a \
                      store, and sorted by popularity, price or what's new. It is where the \
                      buying flow starts when the visitor doesn't know the category yet.",
            who: "Shoppers on a phone or a laptop, logged in or not.",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                FILTERS_FROM_QUERY,
                PAGINATION,
                FRAGMENTS,
                NO_N_PLUS_ONE,
                RANGE_SLIDER,
                Feature {
                    api: "UI kit tags: rx-card-grid + rx-media-card",
                    why: "Each product is the kit's `<rx-media-card>` (photo, name, \"from\" price, \
                          brand) in an `<rx-card-grid>`, a CSS grid that fits as many columns as \
                          the screen holds; the filter fields are the kit's `<rx-checkbox-list>`, \
                          `<rx-select>` and `<rx-checkbox switch>`.",
                },
                ETAG,
                SEO,
                SOFT_DELETES,
                Feature {
                    api: "motion.dev (vendored)",
                    why: "New results slide in with `transform` and `opacity` after each \
                          htmx swap (`data-bs-reveal`, `public/app.js`), and not at all \
                          under `prefers-reduced-motion`.",
                },
            ],
            under_hood: "Reads the categories (one query, for the menu), counts and loads one \
                         page of `products` with the filters (two), then the cards' brands, \
                         categories, variants and photos (four), and the filter choices for \
                         everything in scope: brands, sizes, the lowest and highest price, the \
                         specifications, the stores (six). A fixed number of queries whatever \
                         the page size or the catalogue's size. \"Most popular\" orders by the \
                         units sold (`order_items`), price by each product's cheapest variant, \
                         ties by id so pages never overlap.",
            docs: &[
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
                "docs/ui.md#fragments-and-out-of-band-swaps",
                "docs/routing.md#etags",
                "docs/types.md#soft-deletes",
                "docs/ui.md#navigation-and-page-structure",
            ],
            sources: SOURCES_LISTING,
            code: &[
                Code {
                    title: "Handler: every product, through the one `listing` the three pages share",
                    region: "catalog.index.handler",
                },
                Code {
                    title: "Query: one page of products and their cards, then the `results` fragment",
                    region: "catalog.listing",
                },
                Code {
                    title: "Template: a plain GET form htmx sends on every change",
                    region: "catalog.index.form",
                },
            ],
        },
        Explanation {
            route: "catalog.category",
            path: "/shop/{slug}",
            title: "Category",
            purpose: "One category of the catalogue (road bikes, helmets, chains…) with the \
                      categories under it: \"Bikes\" lists every kind of bike. Same filters and \
                      sort orders as the whole shop; part categories also offer \"fits my \
                      bike\" to customers who registered their bikes.",
            who: "Shoppers who know what they want; customers looking for a part for their \
                  own bike.",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Found<M>",
                    why: "`Found<Category>` reads `{slug}`, sees it is one of the model's \
                          columns and loads the category by it, or answers 404. No lookup \
                          code in the handler.",
                },
                Feature {
                    api: "Pivot",
                    why: "Parts and the bike models they fit are a many-to-many, the \
                          `Pivot` `PART_FITS` (table `part_fits`). \"Fits my bike\" filters \
                          on that table with the bike models the customer registered \
                          (`customer_bikes`): one `EXISTS` (`where_raw`, the ids bound), for \
                          any of their bikes or one of them. A filter needs a condition \
                          inside the products' query, which the pivot's loaders don't give.",
                },
                FILTERS_FROM_QUERY,
                PAGINATION,
                FRAGMENTS,
                NO_N_PLUS_ONE,
                ETAG,
                SEO,
            ],
            under_hood: "Like `/shop`, scoped to the category and its descendants (worked out \
                         from the one query that reads the category tree). For a logged-in \
                         customer on a part category, two more queries find their customer \
                         record and registered bikes; `fits=mine` (or a bike's id) adds \
                         `EXISTS (SELECT 1 FROM part_fits WHERE part_id = products.id AND \
                         bike_id IN (…))`. The back link goes to the parent category.",
            docs: &[
                "docs/routing.md#route-model-binding-foundm",
                "docs/relations.md#filtering-by-a-related-table-without-a-join",
                "docs/ui.md#fragments-and-out-of-band-swaps",
            ],
            sources: SOURCES_LISTING,
            code: &[
                Code {
                    title: "Handler: `Found<Category>` finds the category by its slug",
                    region: "catalog.category.handler",
                },
                Code {
                    title: "Query: the category and those under it, filtered and paginated",
                    region: "catalog.listing",
                },
                Code {
                    title: "Template: the `results` block, the fragment htmx swaps",
                    region: "catalog.index.results",
                },
            ],
        },
        Explanation {
            route: "catalog.search",
            path: "/search",
            title: "Search results",
            purpose: "Full-text search over the products' names, brands, SKUs and \
                      descriptions, best matches first, with the same filters as the \
                      catalogue. The navbar's box on every page sends people here; its \
                      suggestions as you type come from the same index.",
            who: "Anyone who types into the search box: shoppers, and staff looking up a SKU.",
            audience: &[
                Audience::Visitor,
                Audience::Customer,
                Audience::Staff,
                Audience::Developer,
            ],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "renox::db::search",
                    why: "`#[model(search = \"name, keywords, description\")]` on `Product` \
                          and the index migration (`migrations/20260101000410_search_products.*`: \
                          FTS5 on SQLite, a generated `tsvector` with a GIN index on \
                          PostgreSQL). `Query::search(words)` keeps the matches and orders \
                          them by BM25 / `ts_rank`, the name weighing most, so a product named \
                          \"Trek Domane\" beats one that mentions Trek in its description. \
                          `keywords` holds the brand and every SKU, so `GIR-JER-0001` finds \
                          its product.",
                },
                Feature {
                    api: "htmx fragments",
                    why: "The navbar box asks `GET /search/suggest?q=` as you type \
                          (`hx-trigger=\"input changed delay:200ms\"`, from two letters on) \
                          and shows the six best matches under it, a small template of its \
                          own (`catalog/_suggest.html`); the arrow keys and Escape move \
                          through them (`public/catalog/catalog.js`). Enter submits the \
                          plain form to this page, so it works without JavaScript.",
                },
                FILTERS_FROM_QUERY,
                PAGINATION,
                NO_N_PLUS_ONE,
                ETAG,
            ],
            under_hood: "`Product::query().where_search(q)` keeps the matches, then the \
                         filters, then the order: \"Best match\" (`Query::search`, i.e. \
                         `order_by_relevance`, the default when words are given), or \
                         popularity, price or newest among the matches. A word matches the \
                         start of longer words (`dom` finds Domane); search syntax is never \
                         interpreted, the words are bound as one value. With no words the page \
                         lists everything, like `/shop`.",
            docs: &[
                "docs/search.md#3-searching",
                "docs/search.md#ranking",
                "docs/search.md#what-matches",
                "docs/ui.md#fragments-and-out-of-band-swaps",
            ],
            sources: &[
                "examples/bikeshop/src/app/catalog/browse.rs",
                "examples/bikeshop/src/app/catalog/model.rs",
                "examples/bikeshop/migrations/20260101000410_search_products.sqlite.up.sql",
                "examples/bikeshop/migrations/20260101000410_search_products.postgres.up.sql",
                "examples/bikeshop/resources/views/components/nav_search.html",
                "examples/bikeshop/resources/views/catalog/_suggest.html",
                "examples/bikeshop/public/catalog/catalog.js",
                "examples/bikeshop/tests/catalog.rs",
                "tests/browser/bikeshop-catalog.test.mjs",
            ],
            code: &[
                Code {
                    title: "Model: `#[model(search = …)]` indexes three columns",
                    region: "catalog.search.model",
                },
                Code {
                    title: "Handler: the same listing, scoped to the words searched for",
                    region: "catalog.search.handler",
                },
                Code {
                    title: "Suggest: the navbar's best matches with `Product::search`",
                    region: "catalog.search.suggest",
                },
            ],
        },
        Explanation {
            route: "catalog.show",
            path: "/products/{slug}",
            title: "Product",
            purpose: "Everything needed to decide: photos, the description, the \
                      specifications, the price and stock of each size and colour in each \
                      store, what the part fits (or which parts fit the bike), a link to rent \
                      the model when it is in the rental fleet, and the add-to-cart form.",
            who: "Shoppers deciding to buy or rent; customers checking a part for their bike; \
                  staff answering a phone call (\"do you have it in M at South?\").",
            audience: &[
                Audience::Visitor,
                Audience::Customer,
                Audience::Staff,
                Audience::Developer,
            ],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Found<M>",
                    why: "`Found<Product>` loads the product by its `slug` column. A \
                          discontinued product is soft deleted, so the model's query doesn't \
                          see it and the page is a 404: no `if deleted` check to forget.",
                },
                Feature {
                    api: "UI kit tags: rx-infolist + rx-entry",
                    why: "The specifications (frame, wheels, gears, motor…) are an \
                          `<rx-infolist>` of `<rx-entry>`s in two columns, labels and values marked up \
                          by the kit rather than a hand-made table; the brand entry links to \
                          the brand's site.",
                },
                Feature {
                    api: "renox-blocks",
                    why: "The kit has no photo gallery, variant chips or quantity stepper: \
                          the `gallery`, `swatches` and `quantity` blocks fill the gap \
                          (keyboard-usable, Web Animations on `transform`, plain fields \
                          underneath).",
                },
                Feature {
                    api: "htmx fragments",
                    why: "The size and colour chips are a `GET` form on this page's address. \
                          A change asks for the `buybox` block only (price, SKU, stock per \
                          store, the add-to-cart form) and pushes `?size=…&colour=…` into the \
                          address bar; without JavaScript, \"Show\" reloads the page.",
                },
                Feature {
                    api: "Pivot",
                    why: "`PART_FITS.load_with_pivot::<Product, Fit>` gives a part's bikes \
                          with the pivot's `note` (\"Check the axle standard\"); \
                          `FITTING_PARTS` (`PART_FITS.inverse()`) gives a bike's parts. Two \
                          queries either way.",
                },
                Feature {
                    api: "Sessions",
                    why: "\"Recently viewed\" is the last six product ids in the session \
                          (`session.get` / `put`): no table, nothing to clean up, and it \
                          works for guests.",
                },
                Feature {
                    api: "money filter",
                    why: "Prices are integers in the smallest unit of `APP_CURRENCY` (cents); \
                          `{{ price | money }}` writes them the visitor's way \
                          (`$1,249.99` in English, `$1.249,99` in Spanish), so no \
                          template formats a number by hand.",
                },
                Feature {
                    api: "seo()",
                    why: "The title, a one-line description from the product's text, \
                          `type=\"product\"` and the first photo as the Open Graph image, so a \
                          shared link shows a card with the bike.",
                },
                ETAG,
            ],
            under_hood: "Reads the category tree, the brand, the variants, the photos, the \
                         stores and every variant's `stock_levels` rows (available = on hand − \
                         reserved, summed per location store, so consigned goods count where \
                         they are), then the related products through `part_fits` with their \
                         cards, whether a fleet bike (`rental_bikes`) is of one of these \
                         variants, and the recently viewed products. The description is \
                         Markdown, shown with the `markdown` filter (raw HTML is shown as \
                         text). The add-to-cart form posts to `cart.add`; with htmx the answer \
                         is a toast and the navbar's cart, swapped out of band.",
            docs: &[
                "docs/routing.md#route-model-binding-foundm",
                "docs/ui.md#infolists-read-only-details",
                "docs/relations.md#pivot-columns",
                "docs/routing.md#sessions",
                "docs/ui.md#formatting-values",
                "docs/ui.md#fragments-and-out-of-band-swaps",
                "docs/types.md#money",
            ],
            sources: &[
                "examples/bikeshop/src/app/catalog/product.rs",
                "examples/bikeshop/src/app/catalog/model.rs",
                "examples/bikeshop/resources/views/catalog/show.html",
                "crates/renox-blocks/views/blocks.html",
                "examples/bikeshop/tests/catalog.rs",
                "tests/browser/bikeshop-catalog.test.mjs",
            ],
            code: &[
                Code {
                    title: "Handler: `Found<Product>` by slug, answered with the `buybox` fragment",
                    region: "catalog.show.handler",
                },
                Code {
                    title: "Template: a GET form for the variant, the buy box htmx swaps",
                    region: "catalog.show.template",
                },
                Code {
                    title: "Pivot: what fits what, with the pivot's note",
                    region: "catalog.show.fits",
                },
            ],
        },
        Explanation {
            route: "sitemap",
            path: "/sitemap.xml",
            title: "sitemap.xml and robots.txt",
            purpose: "Not a page people read: the list of the shop's addresses for search \
                      engines (the home page, the catalogue, every category and every product \
                      still sold, with when each last changed). Renox's own `/robots.txt` \
                      points crawlers at it.",
            who: "Search engines; the shop owner, who wants the catalogue found.",
            audience: &[Audience::Owner, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Sitemap",
                    why: "`renox::seo::Sitemap::new(&state).route(name, params, updated)` \
                          builds the XML from route names, so a changed path can't leave a \
                          stale address in it. Discontinued (soft-deleted) products aren't \
                          listed.",
                },
                Feature {
                    api: "robots.txt",
                    why: "Renox answers `/robots.txt` itself (unless `public/robots.txt` \
                          exists): in production it allows crawling and names the route \
                          called `sitemap`; elsewhere it disallows everything and pages say \
                          `noindex`, so a staging server is never indexed.",
                },
                ETAG,
            ],
            under_hood: "Two queries: the categories, then each product's slug and \
                         `updated_at` (`select_as`, not whole rows).",
            docs: &["docs/routing.md#etags", "docs/operations.md#app_url"],
            sources: &[
                "examples/bikeshop/src/app/catalog/mod.rs",
                "examples/bikeshop/tests/catalog.rs",
            ],
            code: &[
                Code {
                    title: "Handler: `renox::seo::Sitemap` from routes and rows",
                    region: "sitemap.handler",
                },
                Code {
                    title: "Routes: the catalogue's routes, an ETag layer over them all",
                    region: "catalog.routes",
                },
                Code {
                    title: "Test: the second request answers 304",
                    region: "sitemap.test",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![NotAPage {
        route: "catalog.suggest",
        reason: "an htmx fragment: the navbar search box's suggestions (explained on the \
                 search results page)",
    }]
}

//! "About this page" entries for the about area (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "about.pages",
            path: "/about/pages",
            title: "Every page and its features",
            purpose: "The index of the whole example: every page, what it is for and the Renox \
                  features it uses. Start from a feature (\"which pages use \
                  `renox::grid`?\") or from a role (\"what does a cashier see?\") and \
                  follow the links.",
            who: "Developers learning Renox from the example, and anyone evaluating it.",
            audience: &[Audience::Developer],
            flow: Flow::Learn,
            features: &[
                Feature {
                    api: "Query<T>",
                    why: "The two filters are a plain GET form; `Query<Filters>` reads \
                      `?feature=…&audience=…` into a struct, so the filtered list has its \
                      own URL to share and works without JavaScript. A feature or a role \
                      the app doesn't know is ignored rather than refused.",
                },
                Feature {
                    api: "Lang",
                    why: "The extractor gives the request's language; each explanation is \
                      read from `resources/lang/<locale>.json` when it has the text, and \
                      falls back to English otherwise.",
                },
                Feature {
                    api: "Routes::get",
                    why: "Every page is a named route, and the explanations are keyed by \
                      that name. The list links to the pages without parameters with \
                      `state.url(name, &[])`; for one like `/products/{slug}` that call \
                      fails, so the row is shown without a link instead of a broken one.",
                },
                Feature {
                    api: "UI kit: toolbar + select",
                    why: "The filters line up in the kit's `toolbar`, which wraps on phones \
                      and needs no layout CSS. The feature list is long (every `api` used \
                      on any page), so it is a `select(…, searchable=true)`: type a few \
                      letters to find one.",
                },
                Feature {
                    api: "UI kit: list + badge",
                    why: "Each page is a row of the kit's `list`, its purpose through the \
                      `markdown` filter, its roles and features as `badge`s. Each feature \
                      badge links to this page filtered by it, so you can hop from a page \
                      to every other page that uses the same API.",
                },
                Feature {
                    api: "App::templates",
                    why: "The \"About this page\" panel on every page comes from the \
                      `about_page(request.route, request.path)` template function the app \
                      registers, so no handler passes anything for it: the layouts ask \
                      for it themselves. From 1200 px it is docked beside the page \
                      (`<app-about-dock>`: sticky, scrolling on its own, folding to a \
                      rail); below, the navbar's button opens the same content in the \
                      kit's `sheet`. The fold is a plain cookie the browser writes and an \
                      app layer (`explain::dock_layer`) reads into `renox::context`, so a \
                      folded panel is drawn folded from the first byte, with no jump.",
                },
                Feature {
                    api: "App::share",
                    why: "`explain_panels` (from `BIKESHOP_EXPLAIN`) is given to every view, \
                      so the layouts can hide the panels for a clean demo without any \
                      handler knowing about it. This page stays either way.",
                },
                Feature {
                    api: "build.rs code regions",
                    why: "The code on each panel is cut from the shop's own files when it \
                      is built: lines between `// [explain:name]` and \
                      `// [/explain:name]` (`{# … #}` in templates) become a region in a \
                      generated table (`src/code.rs`), coloured on the server by a copy \
                      of the docs site's highlighter (`src/highlight.rs`). A sample can't \
                      drift from the code that runs, and the walker test fails for a \
                      marker left open or a sample naming a region no file has.",
                },
            ],
            under_hood: "No database: the explanations are Rust values in each area's \
                     `explain.rs`, collected by `crate::explain::all()`. The handler \
                     filters them, localizes them and sorts them by title. \
                     `tests/about.rs` walks every GET route of the app and fails when a \
                     page has no explanation, or when a docs link or a source path here \
                     doesn't exist. The code samples cost nothing at run time: `build.rs` \
                     scanned `src/`, `resources/views/`, `tests/`, `migrations/` and \
                     `public/` for markers and compiled the regions in; each page colours \
                     the one to three it shows.",
            docs: &[
                "docs/routing.md#what-a-handler-can-take",
                "docs/routing.md#route-urls-and-the-current-route",
                "docs/ui.md#navigation-and-page-structure",
                "docs/laravel.md#blade--minijinja",
                "docs/laravel.md#cache-storage-sessions-cookies-and-translations",
                "docs/testing.md#a-first-test",
            ],
            sources: &[
                "examples/bikeshop/src/app/about/mod.rs",
                "examples/bikeshop/src/explain.rs",
                "examples/bikeshop/resources/views/about/pages.html",
                "examples/bikeshop/resources/views/components/about_explain.html",
                "examples/bikeshop/resources/views/components/about_dock.html",
                "examples/bikeshop/resources/views/components/about_panel.html",
                "examples/bikeshop/src/code.rs",
                "examples/bikeshop/build.rs",
                "examples/bikeshop/src/highlight.rs",
                "examples/bikeshop/public/explain.js",
                "examples/bikeshop/tests/about.rs",
                "tests/browser/bikeshop-about.test.mjs",
            ],
            code: &[
                Code {
                    title: "Handler: every explanation, filtered by feature and by role",
                    region: "about.pages.handler",
                },
                Code {
                    title: "Template: a plain GET form, so a filtered list has its own address",
                    region: "about.pages.template",
                },
                Code {
                    title: "Test: the walker fails for a GET route without an explanation",
                    region: "about.pages.test",
                },
            ],
        },
        Explanation {
            route: "about.blocks",
            path: "/about/blocks",
            title: "Bike shop blocks",
            purpose: "Every UI block the bike shop uses beyond Renox's kit, working, with its \
                  macro's signature: a photo gallery, a two-handle range, a quantity \
                  stepper, a counter keypad, a kanban board, a month calendar, an \
                  availability timeline, a date and time range, variant chips, a history \
                  timeline and plan cards. The other pages \
                  use them; this one shows them side by side.",
            who: "Developers who want one of the blocks on their own page, and anyone \
              checking how they look and behave (keyboard, phone, dark mode).",
            audience: &[Audience::Developer],
            flow: Flow::Learn,
            features: &[
                Feature {
                    api: "renox-blocks",
                    why: "The kit has no gallery, kanban or calendar. They were built in \
                      this example first, then moved to the `renox-blocks` crate, a \
                      plugin like renox-editors: the app adds `Blocks::new()` and a page \
                      uses a block like a kit component \
                      (a tag such as `<rx-gallery …/>`). The \
                      macros use `rx-` classes on the kit's `--rx-*` tokens and call the \
                      kit's own macros (`sheet`, `date_picker`); a page loads a block's \
                      script only when it has that block. The date picker with closed \
                      days became the kit's own `date_picker` (`disabled_dates`, \
                      `closed_weekdays`).",
                },
                Feature {
                    api: "UI kit: sheet",
                    why: "The gallery's enlarged photo opens in the kit's `sheet`: focus \
                      moves into it and back, Escape and the backdrop close it. The block \
                      reuses the kit's dialog rather than writing a second one.",
                },
                Feature {
                    api: "Valid<T>",
                    why: "Every form block sends plain fields (a number, the keypad's \
                      digits, one date-time per end, a radio's value), so \
                      `Valid<BlocksForm>` reads them like any form: `between(1, 5)`, \
                      `gt(\"starts_at\", …)` for the end, and `one_of` without the \
                      sold-out size. Its `after` hook refuses a rental starting in the \
                      past again on the server: anyone can send any date.",
                },
                Feature {
                    api: "Query<T>",
                    why: "The price filter is a GET form: the range's two handles become \
                      `?price_min=…&price_max=…`, the calendar's arrows `?month=…` and a \
                      free slot of the availability timeline `?slot=…`. Each state has \
                      its own URL, and a month that can't be shown falls back to today's.",
                },
                Feature {
                    api: "htmx fragments",
                    why: "A variant chip asks `GET /about/blocks/variant` for the price and \
                      stock (`hx-get` with `hx-include`) and swaps in \
                      `about/_variant.html`, not the whole page. The calendar's arrows are \
                      boosted links with `hx-select=\"#calendar\"`, so only the calendar \
                      changes. A kanban move is an htmx POST from a hidden form, which \
                      gets Renox's CSRF header like any htmx request.",
                },
                Feature {
                    api: "Web Animations",
                    why: "The gallery's slides and the kanban's cards glide with the \
                      browser's own Web Animations on `transform` (no library); under \
                      `prefers-reduced-motion` they jump instead.",
                },
            ],
            under_hood: "No database. `src/app/about/blocks.rs` builds the demo data (a \
                     month of visits on fixed days of whichever month is shown, today's \
                     bookings, a workshop board) and renders `about/blocks.html`. \
                     The crate's `blocks.js` module finds the blocks on the page and in \
                     whatever htmx swaps in (`htmx:load`) and imports each one's code \
                     (`gallery.js`, `kanban.js`…) from their `data-rx-*` attributes, with \
                     no inline handlers, so it runs under `CSP=strict`. \
                     Sending the form posts to `POST /about/blocks/form`: `Valid<T>` \
                     checks it, and a success redirects back to the form with a flash \
                     message (nothing is saved); a kanban move posts the card, the \
                     column and the position to `POST /about/blocks/kanban`, which \
                     answers `204` (or a `422` that puts the card back).",
            docs: &[
                "docs/ui.md#changing-the-kit-itself",
                "docs/ui.md#design-principles",
                "docs/ui.md#fragments-and-out-of-band-swaps",
                "docs/validation.md#hooks-prepare-authorize-after",
                "docs/validation.md#browser-values",
                "docs/testing.md#browser-tests",
            ],
            sources: &[
                "examples/bikeshop/src/app/about/blocks.rs",
                "examples/bikeshop/resources/views/about/blocks.html",
                "examples/bikeshop/resources/views/components/about_variant.html",
                "crates/renox-blocks/views/blocks.html",
                "crates/renox-blocks/assets/blocks.js",
                "crates/renox-blocks/assets/parts/gallery.js",
                "crates/renox-blocks/assets/parts/kanban.js",
                "crates/renox-blocks/assets/blocks.css",
                "examples/bikeshop/tests/blocks.rs",
                "tests/browser/bikeshop-blocks.test.mjs",
                "tests/browser/blocks.test.mjs",
            ],
            code: &[
                Code {
                    title: "Form: the blocks' fields checked by `Validate`, then by the shop's date",
                    region: "about.blocks.rules",
                },
                Code {
                    title: "Template: `quantity` and `keypad` in a plain POST form",
                    region: "about.blocks.template",
                },
                Code {
                    title: "Kanban: a move answered with 204, or Renox's 422",
                    region: "about.blocks.kanban",
                },
            ],
        },
        data_page(),
        fields_page(),
        field_sample_page(),
        field_sample_edit_page(),
        htmx_page(),
        htmx_live_page(),
    ]
}

/// `/about/htmx`: htmx and Alpine recipes, live.
fn htmx_page() -> Explanation {
    Explanation {
        route: "about.htmx",
        path: "/about/htmx",
        title: "htmx recipes",
        purpose: "The interactions people usually reach for a JavaScript framework for, \
                  live on a pre-ride checklist and the catalogue's bikes: a modal form that \
                  adds a row, two places changing at once, the server picking where its \
                  answer goes, inline edit, a checkbox that saves itself, a menu whose \
                  Delete asks first, tabs with no request, a reload and a redirect after an \
                  action, and infinite scroll. The table at the end links to where the \
                  shop's own pages use each one.",
        who: "Developers who want a page to feel like an app without writing JavaScript.",
        audience: &[Audience::Developer],
        flow: Flow::Learn,
        features: &[
            Feature {
                api: "Htmx",
                why: "The extractor tells a handler whether htmx asked: it answers htmx \
                      with the smallest fragment that changes and a plain form with a \
                      redirect back, so every action works without JavaScript too.",
            },
            Feature {
                api: "View::also",
                why: "`view(\"about/htmx/_answer.html\", …).fragment(\"row\").also(\"count\")` \
                      sends the row and, out of band (`hx-swap-oob`), the open count and \
                      the empty note: adding, ticking, renaming and deleting all keep \
                      \"N to do\" right with no second request.",
            },
            Feature {
                api: "HxRetarget",
                why: "An item already on the list isn't added twice: the server answers \
                      with `HxRetarget(\"#item-3\")` and `HxReswap(\"outerHTML\")`, so htmx \
                      refreshes that row instead of putting a copy at the top, and \
                      `HxTrigger(\"item-added\")` lets anything on the page react.",
            },
            Feature {
                api: "HxRefresh",
                why: "Clearing the done items changes many rows: simplest to reload the page \
                      (`HX-Refresh`). \"Ready\" goes to the shop with `htmx.redirect(…)`: \
                      `HX-Redirect` for htmx, a 303 for a plain form. Their toasts wait in \
                      the session for the next page.",
            },
            Feature {
                api: "Toast",
                why: "A toast returned with a fragment rides the `HX-Trigger` header and \
                      shows at once; with a reload or a redirect it waits in the session and \
                      the next page's `toasts()` shows it, once.",
            },
            Feature {
                api: "UI kit: action_sheet + menu",
                why: "The modal is the kit's `<rx-action-sheet>` with `target=\"#checklist\"` and \
                      `swap=\"afterbegin\"`: a 422 keeps it open with the error under the \
                      field, a success closes and clears it, and `key=\"n\"` opens it from the \
                      keyboard. The row's `<rx-checkbox>` carries its htmx attributes as plain \
                      attributes (`hx-patch`, `hx-target`), and the menu's `menu_button`s \
                      (no tag yet) take them in `attrs`, `hx-confirm` included.",
            },
            Feature {
                api: "Alpine.data",
                why: "The tabs filter the rows in the browser: one Alpine component \
                      (`x-data=\"checklist\"`) holds the active tab and each row's `x-show` \
                      reads it. Its logic is in `Alpine.data` in a nonce'd script, so the page \
                      works under `CSP=strict`, whose Alpine build refuses statements in \
                      attributes.",
            },
        ],
        under_hood: "The checklist lives in the visitor's session (at most twelve short \
                     items, so the cookie stays small): each action reads it, changes it and \
                     writes it back, and no visitor sees another's. The bikes are the \
                     catalogue's, eight per load, by id (`before=…`): the last row has \
                     `hx-trigger=\"revealed\"` and is replaced by the next load, so a bike \
                     added meanwhile never repeats a row. CSRF needs no wiring: \
                     `renox_head()` sends the token with every htmx request, and 422 errors \
                     land in the fields' `data-error-for` slots.",
        docs: &[
            "docs/ui.md#fragments-and-out-of-band-swaps",
            "docs/ui.md#htmx-response-headers",
            "docs/ui.md#toasts",
            "docs/ui.md#actions",
            "docs/laravel.md#blade--minijinja",
        ],
        sources: &[
            "examples/bikeshop/src/app/about/htmx.rs",
            "examples/bikeshop/resources/views/about/htmx.html",
            "examples/bikeshop/resources/views/components/about_htmx_row.html",
            "examples/bikeshop/resources/views/about/htmx/_answer.html",
            "examples/bikeshop/resources/views/components/about_htmx_bikes.html",
            "examples/bikeshop/tests/htmx.rs",
            "tests/browser/bikeshop-htmx.test.mjs",
        ],
        code: &[
            Code {
                title: "Handler: the server picks where its answer goes",
                region: "about.htmx.store",
            },
            Code {
                title: "Template: the row, then the count out of band",
                region: "about.htmx.answer",
            },
            Code {
                title: "Row: toggle, inline edit, a menu that asks",
                region: "about.htmx.row",
            },
        ],
    }
}

/// `/about/htmx/live`: the checklist of `/about/htmx` as a live component.
fn htmx_live_page() -> Explanation {
    Explanation {
        route: "about.htmx_live",
        path: "/about/htmx/live",
        title: "htmx recipes, as a live component",
        purpose: "The pre-ride checklist of the htmx page again, written as a live \
                  component: add, tick, rename inline, delete, filter by tab and clear the \
                  done items. It is the same session list, so both pages show the same \
                  items, but here there are no routes, fragments or out-of-band swaps: one \
                  struct and a method per action. The infinite scroll, the duplicate that \
                  is retargeted, Clear's reload, Ready's redirect and the delete \
                  confirmation are left out; the htmx page has them.",
        who: "Developers comparing the two ways to build the same interaction.",
        audience: &[Audience::Developer],
        flow: Flow::Learn,
        features: &[
            Feature {
                api: "LiveComponent",
                why: "`#[renox::live_component]` turns the marked methods of an `impl` into \
                      the component's actions. The struct is its state; `data` adds what \
                      the view shows (the items, filtered), and the page only mounts it \
                      with `ctx.mount(…)`.",
            },
            Feature {
                api: "rx-click / rx-model",
                why: "`rx-click=\"toggle(3)\"` calls an action with its arguments, \
                      `rx-model=\"title\"` keeps the typed text in the state, and \
                      `rx-submit` calls an action on Enter. The answer is the component's \
                      HTML again, morphed into the page, so the count, the empty note and \
                      the rows stay right with no out-of-band parts.",
            },
            Feature {
                api: "Signed snapshot",
                why: "The state travels in the page as a signed snapshot, checked on every \
                      call, so the browser can't forge it. The items themselves stay in \
                      the session, as on the htmx page; a failed `ctx.validate(…)` is a \
                      422 whose errors land under the field.",
            },
        ],
        under_hood: "Each click posts the snapshot, the arguments and the model fields to \
                     `/_renox/live/checklist/{action}`. The server checks the signature, \
                     runs the method, renders the view again and answers its HTML, which \
                     the browser morphs in. The checklist is the visitor's session list \
                     (`Checklist::of`), the same store as `/about/htmx`.",
        docs: &[
            "docs/live.md#the-attribute-macro",
            "docs/live.md#the-snapshot-and-its-security",
            "docs/ui.md#htmx-response-headers",
        ],
        sources: &[
            "examples/bikeshop/src/app/about/live.rs",
            "examples/bikeshop/resources/views/about/live.html",
            "examples/bikeshop/resources/views/about/live/_checklist.html",
            "examples/bikeshop/tests/htmx.rs",
            "tests/browser/bikeshop-htmx.test.mjs",
        ],
        code: &[
            Code {
                title: "Component: the state and its actions",
                region: "about.htmx_live.component",
            },
            Code {
                title: "Template: rx-click and rx-model",
                region: "about.htmx_live.view",
            },
        ],
    }
}

/// `/about/fields`: every input, its Rust types and its columns.
fn fields_page() -> Explanation {
    Explanation {
        route: "about.fields",
        path: "/about/fields",
        title: "Form fields and their types",
        purpose: "The reference for choosing types: every kind of form input with the Rust \
                  type `Valid<T>` reads it into, the model's type and the column on SQLite \
                  and on PostgreSQL, then a form that tries every row. `docs/types.md` \
                  points here. A logged-in visitor saves a sample bike, opens it read back \
                  from the database, edits it and deletes it.",
        who: "Developers choosing a field type or checking how a value travels from the \
              browser to the database and back.",
        audience: &[Audience::Developer],
        flow: Flow::Learn,
        features: &[
            Feature {
                api: "Valid<T>",
                why: "One form struct reads every kind of input: numbers, a checkbox (`on` \
                      or nothing becomes a `bool`), an enum from a radio group, lists from \
                      checkboxes and tags, `specs[0][key]` pairs as `KeyValues`, times and \
                      dates, and files as `Upload`s, from one multipart body. An empty \
                      input becomes `None`; every wrong value is reported at once, next to \
                      its field.",
            },
            Feature {
                api: "Validator::each",
                why: "`v.each(\"colors\", …, |c| c.one_of(COLOURS))` checks each ticked \
                      colour and `v.distinct` refuses one sent twice; the error is keyed \
                      `colors.1`, and the `colors` slot shows the first. `decimal(0, 2)`, \
                      `.json()`, `image()`, `dimensions(…)` and `mimes([\"pdf\"])` are \
                      rules too, files checked by their content, not their name.",
            },
            Feature {
                api: "#[derive(DbEnum)]",
                why: "The frame size is an enum stored as a word (`small`, `medium`, \
                      `large`): the radio group sends one, an unknown one fails \
                      validation, and the column reads back as the enum.",
            },
            Feature {
                api: "db::Json",
                why: "Colours and tags are `Json<Vec<String>>`, the specifications \
                      `Json<KeyValues>` (a list of pairs, so their order holds in \
                      PostgreSQL's `JSONB`, which reorders an object's keys): `TEXT` on \
                      SQLite, `JSONB` on PostgreSQL, the same code on both.",
            },
            Feature {
                api: "renox-editors",
                why: "The Markdown, rich text and code editors are plain fields: a `String`, \
                      a `RichText` (its HTML cleaned of scripts as the form is read, an \
                      emptied editor counted as empty) and a `String` checked with \
                      `.json()`. The page loads the editors' script only because it has \
                      them.",
            },
            Feature {
                api: "UI kit: form fields",
                why: "Every field is the kit's: `input` with a prefix, a suffix, a \
                      `datalist` of the shop's brands or a copy button, `checkbox` as a \
                      switch, `radio`, `checkbox_list`, `tags_input`, `key_value`, \
                      `date_picker`, `file` with a preview, in `fieldset`s; each shows the \
                      old input after a failed save.",
            },
            Feature {
                api: "Routes::require_auth",
                why: "The reference is public; saving a sample needs a login, and each \
                      sample belongs to the person who made it, so the demo's visitors \
                      never see each other's.",
            },
        ],
        under_hood: "No query for a guest; for someone logged in, one for their samples and \
                     one for the brands the name field suggests. The table comes from \
                     `fields::REFERENCE`, the same Rust list the form follows. Sending the \
                     form reads the multipart body (with `specs[0][key]` names, as a tree), \
                     checks every rule, stores the photo on the public part of the disk \
                     (`Upload::store_public`, served at `/storage/…`, or from the bucket on \
                     S3) and the manual privately (`Upload::store`), and inserts the row \
                     with a new UUID v7 key. Its migration has a `.postgres.up.sql` twin \
                     with `UUID`, `DOUBLE PRECISION`, `BOOLEAN`, `JSONB`, `TIME`, \
                     `TIMESTAMP` and `DATE` columns; the tests run on both databases.",
        docs: &[
            "docs/types.md#the-table",
            "docs/types.md#forms",
            "docs/types.md#nested-names-rows-inside-a-form",
            "docs/validation.md#uploads",
            "docs/ui.md#form-fields",
            "docs/editors.md#the-fields-in-a-form",
        ],
        sources: &[
            "examples/bikeshop/src/app/about/fields.rs",
            "examples/bikeshop/resources/views/about/fields.html",
            "examples/bikeshop/resources/views/components/about_fields_form.html",
            "examples/bikeshop/migrations/20260109000100_create_field_samples_table.up.sql",
            "examples/bikeshop/migrations/20260109000100_create_field_samples_table.postgres.up.sql",
            "examples/bikeshop/tests/fields.rs",
        ],
        code: &[
            Code {
                title: "Form: one rule per field, files by content",
                region: "fields.rules",
            },
            Code {
                title: "Template: tags, pairs and the editors",
                region: "fields.template",
            },
            Code {
                title: "PostgreSQL: the column for each type",
                region: "fields.postgres",
            },
        ],
    }
}

/// `/about/fields/{sample}`: a sample read back.
fn field_sample_page() -> Explanation {
    Explanation {
        route: "about.fields.show",
        path: "/about/fields/{sample}",
        title: "A sample read back",
        purpose: "Every field of a saved sample, read back from the database and shown the \
                  way its kind reads best: money, a yes or no, colour swatches, tag \
                  badges, the specifications as a table, Markdown, rich text, highlighted \
                  JSON, dates, the photo and a link to the private manual.",
        who: "Developers checking that each value came back as it was typed.",
        audience: &[Audience::Developer],
        flow: Flow::Learn,
        features: &[
            Feature {
                api: "Path<T>",
                why: "The address carries the sample's UUID; `renox::Path<Uuid>` answers \
                      404 for one that doesn't parse, and a sample of someone else's is a \
                      404 too, so the page never says it exists.",
            },
            Feature {
                api: "UI kit: infolist",
                why: "Each `entry` formats its value by kind: `money` in `APP_CURRENCY`, \
                      `bool`, `color`, `key_value`, `markdown` (HTML typed in shown as \
                      text), `date`, `datetime`, `since`; buttons sit beside the key and the \
                      stock (`suffix_actions`). `code_entry` from renox-editors highlights \
                      the JSON, and the `rich_text` filter cleans the stored HTML again on \
                      the way out.",
            },
            Feature {
                api: "Download",
                why: "The manual has no public address: its link asks the app, which checks \
                      the sample is yours and sends the file with \
                      `Download::from_storage(…).inline()`, under the name it was uploaded \
                      with, for the browser to show. The photo, stored public, is shown \
                      straight from `storage.url(key)`.",
            },
        ],
        under_hood: "One query for the sample (by id and owner). The photo's address comes \
                     from the storage driver: `/storage/samples/…` on the local disk, the \
                     bucket's address on S3. Delete is the kit's `confirm` sheet; it removes \
                     the row, then its files.",
        docs: &[
            "docs/ui.md#infolists-read-only-details",
            "docs/ui.md#formatting-values",
            "docs/routing.md#what-a-handler-can-return",
            "docs/types.md#keys",
        ],
        sources: &[
            "examples/bikeshop/src/app/about/fields.rs",
            "examples/bikeshop/resources/views/about/fields_show.html",
            "examples/bikeshop/tests/fields.rs",
        ],
        code: &[
            Code {
                title: "Template: each entry formatted by its kind",
                region: "fields.show.template",
            },
            Code {
                title: "Handler: the private manual, sent inline",
                region: "fields.manual",
            },
        ],
    }
}

/// `/about/fields/{sample}/edit`: the form with the stored values.
fn field_sample_edit_page() -> Explanation {
    Explanation {
        route: "about.fields.edit",
        path: "/about/fields/{sample}/edit",
        title: "Editing a sample",
        purpose: "The form of `/about/fields` filled with a sample's stored values, each \
                  in the format its input expects (`2026-10-01`, `08:00:00`, \
                  `2026-10-01T10:30:00`, the price back in dollars). A new photo or manual \
                  replaces the stored file.",
        who: "Developers checking the round trip from the database back into a form.",
        audience: &[Audience::Developer],
        flow: Flow::Learn,
        features: &[
            Feature {
                api: "Valid<T>",
                why: "The same form struct as for a new sample; the form says `PUT` with \
                      `method_field('PUT')`. After a failed save each field shows what was \
                      sent (`old()`), a checkbox or colour left unticked included \
                      (`has_old()`), and never a file.",
            },
            Feature {
                api: "Storage",
                why: "A new file is stored first, the row saved, and only then the old file \
                      deleted: a failure halfway leaves at worst an unused file, never a \
                      row pointing at a missing one.",
            },
        ],
        under_hood: "Two queries: the sample (by id and owner) and the brands for the name's \
                     suggestions. Saving reads the multipart form, stores any new file, \
                     saves the row (`updated_at` set by the model) and deletes the files it \
                     replaced.",
        docs: &[
            "docs/validation.md#showing-errors-and-old-input-in-templates",
            "docs/types.md#dates-and-times",
            "docs/routing.md#method-spoofing",
        ],
        sources: &[
            "examples/bikeshop/src/app/about/fields.rs",
            "examples/bikeshop/resources/views/about/fields_edit.html",
            "examples/bikeshop/resources/views/components/about_fields_form.html",
            "examples/bikeshop/tests/fields.rs",
        ],
        code: &[
            Code {
                title: "Handler: save, then delete the replaced files",
                region: "fields.update",
            },
            Code {
                title: "Template: the stored files above their fields",
                region: "fields.files",
            },
        ],
    }
}

/// `/about/data`: the data model explained.
fn data_page() -> Explanation {
    Explanation {
        route: "about.data",
        path: "/about/data",
        title: "The data model",
        purpose: "Every table of the bike shop by the area that owns it, how they relate \
                  and how each relation is loaded, Pagila's tables and what they became, the \
                  owner / location / operating store attributes, how money, secrets, \
                  statuses and deletions are stored, the factories and seeders, and who may \
                  do what. The row counts are live: after `demo:seed --size large` the page \
                  shows Pagila's volume.",
        who: "Developers reading the example to see how a real business is modelled in \
              Renox, before opening the models.",
        audience: &[Audience::Developer, Audience::Owner],
        flow: Flow::Learn,
        features: &[
            Feature {
                api: "#[derive(Model)]",
                why: "One model per table, in the area that owns it: \
                      `src/app/<area>/model.rs`. The derive writes the column list, the \
                      reads and the writes, so no SQL is repeated by hand; the table names \
                      are written out (`#[model(table = \"rentals\")]`), plural as Renox \
                      recommends.",
            },
            Feature {
                api: "renox::db::relations",
                why: "`belongs_to`, `has_many`, `Pivot` (`part_fits` with a note), `Morph` \
                      (payments, stock movements and intercompany entries point at several \
                      tables) and `has_many_through` (a customer's work orders through \
                      their bikes): each loads a whole page's relations in a fixed number \
                      of queries instead of one per row, which `tests/data.rs` checks with \
                      `capture_queries`.",
            },
            Feature {
                api: "DbEnum",
                why: "Every status and kind is an enum stored as a word, readable in the \
                      table and checked when it is read back; the page lists the values \
                      from `ALL`, so it can't drift from the code.",
            },
            Feature {
                api: "Encrypted<T>",
                why: "The customer's ID number is sealed with `APP_KEY` in the table, so \
                      a copied database file doesn't give it away, and never serialized \
                      (`#[serde(skip_serializing)]`), so it can't leak into a view or JSON.",
            },
            Feature {
                api: "#[model(soft_deletes)]",
                why: "Customers who leave and discontinued products are hidden, not \
                      removed: their rentals, orders and payments still point at them.",
            },
            Feature {
                api: "renox::db::search",
                why: "Products are searchable by name, brand, SKU and description \
                      (`#[model(search = …)]`, the brand and SKUs copied into a \
                      `keywords` column): an FTS5 table on SQLite, a generated `tsvector` \
                      on PostgreSQL, from one migration Renox writes, instead of a slow \
                      `LIKE` over every row.",
            },
            Feature {
                api: "Factory",
                why: "The main models have factories with states named after the \
                      business (`rentals().overdue()`, `rental_bikes().placed_at(store)`), \
                      used by the seeders and the tests, so a test says what it needs in \
                      the shop's words instead of filling every column.",
            },
            Feature {
                api: "AppCommand",
                why: "`demo:seed --size large` is a typed clap command next to the \
                      built-in `db:seed`, so the option is parsed and listed in `--help`; \
                      it refuses a seeded database and says how to start again.",
            },
            Feature {
                api: "Permissions module",
                why: "The roles and what they grant are read from the database \
                      (`permissions::roles`), so the page shows what the owner set, with \
                      the labels and the permission catalogue from `access::catalogue`.",
            },
            Feature {
                api: "renox::db::sql",
                why: "The live row counts are one `UNION ALL` query over every table \
                      listed in `data.rs`, read into `(String, i64)` pairs with \
                      `fetch_as`: one round trip instead of one per table.",
            },
            Feature {
                api: "UI kit: card + table",
                why: "Each part of the page is a kit `card`; tables, relations and the \
                      mappings are kit `table`s (scrolling sideways on phones), the money, \
                      encryption, enum and soft-delete notes sit in a kit `columns(2)` \
                      grid. No layout CSS of the app's own.",
            },
            Feature {
                api: "UI kit: infolist + entry",
                why: "The numbers at the top, the enum values and the permissions are \
                      kit `infolist`s; `entry(…, format=\"money\")` shows how an \
                      integer amount is displayed.",
            },
            Feature {
                api: "Model::insert_many",
                why: "The seeders write each table with `insert_many` inside one transaction: a \
                      few multi-row `INSERT`s instead of one statement per row, which is what \
                      lets `demo:seed --size large` build 16,000 rentals and 5,000 orders in \
                      seconds.",
            },
        ],
        under_hood: "Two queries: one `SELECT 'table', COUNT(*) FROM table UNION ALL …` \
                     for the row counts, and `permissions::roles` for the roles and their \
                     permissions. Everything else is Rust data in \
                     `src/app/about/data.rs` (the tables, relations and mappings) or read \
                     from the code (`DbEnum::ALL`, `access::catalogue`). The texts are \
                     Markdown in `resources/lang/<locale>.json` under `data_page`.",
        docs: &[
            "docs/types.md#money",
            "docs/types.md#enums",
            "docs/types.md#soft-deletes",
            "docs/types.md#the-table",
            "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
            "docs/relations.md#polymorphic-relations",
            "docs/relations.md#through-a-middle-model-has_many_through",
            "docs/search.md#2-the-index",
            "docs/testing.md#factories",
            "docs/scheduling.md#a-typed-command-clap",
            "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
            "docs/authorization.md#lists-scopes_with",
            "docs/ui.md#infolists-read-only-details",
        ],
        sources: &[
            "examples/bikeshop/src/app/about/data.rs",
            "examples/bikeshop/resources/views/about/data.html",
            "examples/bikeshop/migrations/20260101000600_create_fleet_and_rentals_tables.up.sql",
            "examples/bikeshop/src/app/rentals/model.rs",
            "examples/bikeshop/src/app/rentals/factories.rs",
            "examples/bikeshop/src/app/access/catalogue.rs",
            "examples/bikeshop/src/app/access/policy.rs",
            "examples/bikeshop/src/seed/mod.rs",
            "examples/bikeshop/src/seed/history.rs",
            "examples/bikeshop/tests/data.rs",
            "examples/bikeshop/tests/seed.rs",
            "tests/browser/bikeshop-data.test.mjs",
        ],
        code: &[
            Code {
                title: "Query: every table's row count in one `UNION ALL`",
                region: "about.data.counts",
            },
            Code {
                title: "Handler: the roles as the database has them now",
                region: "about.data.handler",
            },
            Code {
                title: "Template: the kit's `card` and `table`, one per area",
                region: "about.data.template",
            },
        ],
    }
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    [
        super::blocks::not_pages(),
        super::fields::not_pages(),
        super::htmx::not_pages(),
    ]
    .concat()
}

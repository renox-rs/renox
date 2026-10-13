//! The course's outline: one app (a product catalog with stock) built stage
//! by stage. The chapters themselves come with the content pipeline (#524);
//! until then the home page shows what each stage will teach.

use serde::Serialize;

/// One stage of the course.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Stage {
    /// Its number, from 0.
    pub number: u8,
    /// Its name in URLs.
    pub slug: &'static str,
    pub title: &'static str,
    /// One sentence: what the student can do afterwards.
    pub summary: &'static str,
    /// The concepts it introduces, in the words the code uses.
    pub topics: &'static [&'static str],
    /// The guide on docs.renox.rs that goes deeper (a page slug).
    pub guide: &'static str,
}

/// Every stage, in order.
pub static STAGES: &[Stage] = &[
    Stage {
        number: 0,
        slug: "setup",
        title: "Set up and run an app",
        summary: "Install rnx, create the catalog app and see it answer in the browser.",
        topics: &["rnx new", "rnx serve", "rnx doctor", ".env"],
        guide: "tutorial",
    },
    Stage {
        number: 1,
        slug: "migrations",
        title: "Migrations",
        summary: "Create the products table with plain SQL, then let Renox write the next migration from the model.",
        topics: &[
            "up.sql / down.sql",
            "rnx migrate",
            "make:migration --auto",
            "db:check",
        ],
        guide: "types",
    },
    Stage {
        number: 2,
        slug: "models",
        title: "Models",
        summary: "Turn the table into a Rust struct and query it with columns the compiler checks.",
        topics: &[
            "#[derive(Model)]",
            "Product::PRICE.lt(…)",
            "find_or_404",
            "save",
        ],
        guide: "relations",
    },
    Stage {
        number: 3,
        slug: "pages",
        title: "Routes and pages",
        summary: "Add a route, read the products in a handler and show them in a page made of rx- tags.",
        topics: &["Routes", "view(…)", "<rx-page>", "<rx-table>"],
        guide: "routing",
    },
    Stage {
        number: 4,
        slug: "components",
        title: "Your own components",
        summary: "Write an <app-price-tag> once and use it everywhere, with props the compiler checks.",
        topics: &[
            "rnx make:component",
            "<rx-props>",
            "<rx-slot>",
            ":expressions",
        ],
        guide: "ui",
    },
    Stage {
        number: 5,
        slug: "forms",
        title: "Forms and validation",
        summary: "Add products through a form whose errors appear next to the fields, without a reload.",
        topics: &[
            "#[derive(Validate)]",
            "Valid<T>",
            "<rx-form>",
            "422 and htmx",
        ],
        guide: "validation",
    },
    Stage {
        number: 6,
        slug: "live-components",
        title: "How live components work",
        summary: "A part of the page whose state lives in a Rust struct: snapshot, action, render, morph.",
        topics: &[
            "LiveComponent",
            "rx-click",
            "rx-model",
            "the signed snapshot",
        ],
        guide: "live",
    },
    Stage {
        number: 7,
        slug: "live-and-models",
        title: "A live component on a model",
        summary: "A stock editor that keeps only the product's id, reads the model and saves it after checking the user may.",
        topics: &["LiveContext::mount", "data()", "authorize", "<live-…>"],
        guide: "live",
    },
    Stage {
        number: 8,
        slug: "testing",
        title: "Testing it all",
        summary: "Test the pages, the form and the live component's actions with TestApp.",
        topics: &["#[renox::test]", "TestApp", "TestApp::live", "assertions"],
        guide: "testing",
    },
];

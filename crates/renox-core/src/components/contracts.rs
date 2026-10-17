//! The component table (Decision 6 of #372): what each `rx-*` tag promises. A static Rust table,
//! so `view:check`, the editor data file and the plugins can read it without parsing anything.

use super::attrs::Kind;

/// How a component turns into MiniJinja.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Render {
    /// A call to a macro of a template module.
    Macro {
        /// The template module the macro is imported from.
        module: Module,
        /// The macro's name.
        name: &'static str,
    },
    /// A plain element: the kit has CSS classes for it, not a macro.
    Element {
        /// The HTML tag written.
        tag: &'static str,
        /// The base class.
        class: &'static str,
    },
    /// A call to a template function, such as `chart(...)`: the required props go first as
    /// positional arguments, the rest as keyword arguments. It takes no content.
    Function(&'static str),
    /// Code generation of its own.
    Special(Special),
}

/// A built-in template module whose macros components call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Module {
    /// `renox/ui.html`.
    Ui,
    /// `renox/pagination.html`.
    #[allow(dead_code)]
    Pagination,
    /// `renox/grid.html`.
    Grid,
    /// `renox/mail/components.html`.
    Mail,
    /// A template a plugin registered a component for.
    Custom {
        /// The template name.
        path: &'static str,
        /// The name it is imported as.
        alias: &'static str,
    },
}

impl Module {
    /// The template name.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Module::Ui => "renox/ui.html",
            Module::Pagination => "renox/pagination.html",
            Module::Grid => "renox/grid.html",
            Module::Mail => "renox/mail/components.html",
            Module::Custom { path, .. } => path,
        }
    }

    /// The name the module is imported as.
    pub(crate) fn alias(self) -> &'static str {
        match self {
            Module::Ui => "__rx_ui",
            Module::Pagination => "__rx_pagination",
            Module::Grid => "__rx_grid",
            Module::Mail => "__rx_mail",
            Module::Custom { alias, .. } => alias,
        }
    }
}

/// The components with code generation of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Special {
    /// `rx-page`.
    Page,
    /// `rx-push`.
    Push,
    /// `rx-block`.
    Block,
    /// `rx-outlet`.
    Outlet,
    /// `rx-pagination`.
    Pagination,
    /// `rx-let`.
    Let,
    /// `rx-script`.
    Script,
    /// `rx-style`.
    Style,
    /// `rx-form`.
    Form,
    /// `rx-table`.
    Table,
    /// `rx-column`, made by its parent.
    Column,
    /// `rx-mail`.
    Mail,
    /// `rx-wizard-action`.
    WizardAction,
    /// `rx-wizard`.
    Wizard,
    /// `rx-wizard-step`, made by its parent.
    WizardStep,
    /// `rx-tabs`.
    Tabs,
    /// `rx-tab`, made by its parent.
    Tab,
    /// `rx-include`.
    Include,
    /// `rx-link-tabs`.
    LinkTabs,
    /// `rx-link-tab`, made by its parent.
    LinkTab,
    /// `rx-mail-text`.
    MailText,
}

/// One attribute a component takes.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct Prop {
    /// The name, in kebab case.
    pub name: &'static str,
    /// What the value may be.
    pub kind: Kind,
    /// Whether it must be given.
    pub required: bool,
    /// The words of an `Enum`.
    pub values: &'static [&'static str],
    /// One line about it.
    pub doc: &'static str,
}

/// One place a component takes content.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct Slot {
    /// The name; `""` is the default slot.
    pub name: &'static str,
    /// The prop the content fills, if it is not the macro's `caller()`.
    pub into: Option<&'static str>,
    /// The names the content receives (`{% call(row, prefix) %}`).
    pub args: &'static [&'static str],
    /// Whether the macro works without content (it checks `caller is defined`).
    pub optional: bool,
    /// One line about it.
    pub doc: &'static str,
}

/// What a component promises.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct Contract {
    /// The tag, such as `rx-stack`.
    pub tag: &'static str,
    /// One line about it.
    pub doc: &'static str,
    /// How it is generated.
    pub render: Render,
    /// The attributes it takes.
    pub props: &'static [Prop],
    /// Where it takes content.
    pub slots: &'static [Slot],
    /// The events it sends.
    pub events: &'static [&'static str],
    /// The prop that `route="name"` fills.
    pub route_prop: Option<&'static str>,
    /// Whether it passes other attributes through.
    pub attrs: bool,
    /// The component it must sit in.
    pub parent: Option<&'static str>,
}

const DEFAULT_SLOT: Slot = Slot {
    name: "",
    into: None,
    args: &[],
    optional: false,
    doc: "The content.",
};

const fn text(name: &'static str, required: bool, doc: &'static str) -> Prop {
    Prop {
        name,
        kind: Kind::Text,
        required,
        values: &[],
        doc,
    }
}

const fn kind_prop(name: &'static str, doc: &'static str) -> Prop {
    Prop {
        name,
        kind: Kind::Enum,
        required: false,
        values: &["info", "success", "warning", "error"],
        doc,
    }
}

const fn flag(name: &'static str, doc: &'static str) -> Prop {
    Prop {
        name,
        kind: Kind::Bool,
        required: false,
        values: &[],
        doc,
    }
}

const fn choice(name: &'static str, values: &'static [&'static str], doc: &'static str) -> Prop {
    Prop {
        name,
        kind: Kind::Enum,
        required: false,
        values,
        doc,
    }
}

const fn data(name: &'static str, required: bool, doc: &'static str) -> Prop {
    Prop {
        name,
        kind: Kind::Data,
        required,
        values: &[],
        doc,
    }
}

const fn number(name: &'static str, doc: &'static str) -> Prop {
    Prop {
        name,
        kind: Kind::Number,
        required: false,
        values: &[],
        doc,
    }
}

const INPUT_TYPES: &[&str] = &[
    "text",
    "email",
    "password",
    "number",
    "date",
    "datetime-local",
    "time",
    "month",
    "week",
    "url",
    "tel",
    "search",
    "color",
    "range",
    "money",
    "quantity",
];

const FORM_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];

const BUTTON_VARIANTS: &[&str] = &["primary", "secondary", "plain", "danger", "plain-danger"];
const ICON_BUTTON_VARIANTS: &[&str] = &["plain", "primary", "danger"];
const BUTTON_TYPES: &[&str] = &["submit", "button", "reset"];
const SIZES: &[&str] = &["small"];
const CHANGED: &[&str] = &["changed"];

const SHEET_EVENTS: &[&str] = &["opened", "closed", "saved", "failed"];

/// The default slot, filling the prop `into`.
const fn slot_into(into: &'static str, doc: &'static str) -> Slot {
    Slot {
        name: "",
        into: Some(into),
        args: &[],
        optional: false,
        doc,
    }
}

/// The components Renox ships.
pub(crate) static BUILTIN: &[Contract] = &[
    Contract {
        tag: "rx-stack",
        doc: "Children stacked in a column with the kit's spacing.",
        render: Render::Element {
            tag: "div",
            class: "rx-stack",
        },
        props: &[],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-row",
        doc: "Children side by side in a row with the kit's spacing.",
        render: Render::Element {
            tag: "div",
            class: "rx-row",
        },
        props: &[Prop {
            name: "end",
            kind: Kind::Bool,
            required: false,
            values: &[],
            doc: "Pushes the children to the end of the row.",
        }],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-page-header",
        doc: "The page's title, with an optional subtitle, back link, badge and actions.",
        render: Render::Macro {
            module: Module::Ui,
            name: "page_header",
        },
        props: &[
            text("title", true, "The page title."),
            text("subtitle", false, "A line under the title."),
            text("back", false, "The URL of a back link."),
            text("back-label", false, "The back link's text."),
            text("badge", false, "A badge next to the title."),
            kind_prop("badge-kind", "The badge's colour."),
        ],
        slots: &[Slot {
            optional: true,
            doc: "The page's actions.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-card",
        doc: "A titled section on the page.",
        render: Render::Macro {
            module: Module::Ui,
            name: "card",
        },
        props: &[
            text("title", false, "The card's heading."),
            text("subtitle", false, "A line under the heading."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-toolbar",
        doc: "A row of buttons and filters above a list.",
        render: Render::Macro {
            module: Module::Ui,
            name: "toolbar",
        },
        props: &[],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-badge",
        doc: "A small coloured label.",
        render: Render::Macro {
            module: Module::Ui,
            name: "badge",
        },
        props: &[
            text("text", true, "The label; or give it as content."),
            kind_prop("kind", "The colour."),
        ],
        slots: &[Slot {
            into: Some("text"),
            doc: "The label.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-page",
        doc: "The whole template: extends a layout, sets the SEO title and fills its blocks.",
        render: Render::Special(Special::Page),
        props: &[
            text("layout", true, "The layout's file name; plain text."),
            text("title", false, "The page title for the SEO tags."),
            text(
                "description",
                false,
                "The page description for the SEO tags.",
            ),
        ],
        slots: &[Slot {
            doc: "The page's content block; `<rx-slot name=\"X\">` fills the layout's block X.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-push",
        doc: "Content added to a stack of the layout.",
        render: Render::Special(Special::Push),
        props: &[
            text("stack", true, "The stack's name."),
            text("once", false, "A key: the content is added once per page."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-block",
        doc: "A named block of the layout: `{% block name %}`; self-closing it leaves one for pages to fill.",
        render: Render::Special(Special::Block),
        props: &[text(
            "name",
            true,
            "The block's name: letters, digits and underscores; plain text.",
        )],
        slots: &[Slot {
            optional: true,
            doc: "The block's default content.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-pagination",
        doc: "Page links for a page of rows (`Paginated` or `SimplePage`).",
        render: Render::Special(Special::Pagination),
        props: &[data(
            "page",
            true,
            "The page: a `Paginated` (numbered links) or a `SimplePage` (previous / next).",
        )],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-let",
        doc: "A local name for the rest of the enclosing element: `{% with name = value %}`.",
        render: Render::Special(Special::Let),
        props: &[
            text(
                "name",
                true,
                "The name: letters, digits and underscores; plain text.",
            ),
            text(
                "value",
                true,
                "The value: text, or `:value=\"expression\"` for data.",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-include",
        doc: "Another template, drawn here with the page's variables: `{% include %}`.",
        render: Render::Special(Special::Include),
        props: &[
            text(
                "template",
                true,
                "The template's name: text, or `:template=\"expression\"` for a name worked out at run time.",
            ),
            flag(
                "optional",
                "Shows nothing when the template does not exist (`ignore missing`).",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-script",
        doc: "An inline `<script>` that carries the page's CSP nonce.",
        render: Render::Special(Special::Script),
        props: &[],
        slots: &[Slot {
            optional: true,
            doc: "The script's code (left as written).",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-style",
        doc: "An inline `<style>` that carries the page's CSP nonce.",
        render: Render::Special(Special::Style),
        props: &[],
        slots: &[Slot {
            optional: true,
            doc: "The style rules (left as written).",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-head",
        doc: "The head tags Renox adds: assets, CSRF meta, analytics (`renox_head()`).",
        render: Render::Function("renox_head"),
        props: &[],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-ui",
        doc: "The UI kit's script and stylesheet tags (`renox_ui()`).",
        render: Render::Function("renox_ui"),
        props: &[flag(
            "styles",
            "Set to false to leave out the stylesheet (an app that builds its own).",
        )],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-theme",
        doc: "The request's brand stylesheet link, after `<rx-ui/>` (`renox_theme()`); nothing without a theme.",
        render: Render::Function("renox_theme"),
        props: &[],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-toasts",
        doc: "The toast region (`toasts()`).",
        render: Render::Function("toasts"),
        props: &[],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-outlet",
        doc: "A hole in a layout: `name` is `{% block name %}` (the content is its default), `stack` is `stack('name')` where `<rx-push>` content lands. Exactly one of the two.",
        render: Render::Special(Special::Outlet),
        props: &[
            text(
                "name",
                false,
                "A layout block's name; pages fill it. Not with `stack`.",
            ),
            text("stack", false, "A stack's name. Not with `name`."),
        ],
        slots: &[Slot {
            optional: true,
            doc: "With `name`: the block's default content.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-wizard",
        doc: "A form in steps; each `<rx-wizard-step>` inside is one step.",
        render: Render::Special(Special::Wizard),
        props: &[
            text(
                "id",
                true,
                "The wizard's id; its steps build theirs from it.",
            ),
            text("submit-label", true, "The last step's submit button."),
            text("back-label", false, "The back button's text."),
            text("next-label", false, "The next button's text."),
            Prop {
                name: "cancel",
                kind: Kind::Bool,
                required: false,
                values: &[],
                doc: "Shows a cancel button that closes the sheet.",
            },
            text("cancel-label", false, "The cancel button's text."),
        ],
        slots: &[Slot {
            doc: "Only `<rx-wizard-step>` elements.",
            ..DEFAULT_SLOT
        }],
        events: CHANGED,
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-wizard-step",
        doc: "One step of a wizard; its fields are the content.",
        render: Render::Special(Special::WizardStep),
        props: &[
            text("key", true, "The step's key, unique in the wizard."),
            text("title", false, "The step's name in the progress list."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: Some("rx-wizard"),
    },
    Contract {
        tag: "rx-tabs",
        doc: "Tabs with their panels; each `<rx-tab>` inside is one tab and its panel.",
        render: Render::Special(Special::Tabs),
        props: &[
            text("id", true, "The tabs' id; the panels build theirs from it."),
            text(
                "selected",
                false,
                "The key of the open tab; the first by default.",
            ),
            text("label", false, "The tab list's accessible name."),
            data(
                "items",
                false,
                "The tabs as data, a list of [key, label] pairs, instead of `<rx-tab>` children; the panels are then `<rx-tab-panel>` tags.",
            ),
        ],
        slots: &[Slot {
            optional: true,
            doc: "`<rx-tab>` elements; or, with `items`, the `<rx-tab-panel>` panels.",
            ..DEFAULT_SLOT
        }],
        events: CHANGED,
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-tab",
        doc: "One tab; its content is the panel.",
        render: Render::Special(Special::Tab),
        props: &[
            text("key", true, "The tab's key, unique in the tabs."),
            text("label", true, "The tab's text."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: Some("rx-tabs"),
    },
    Contract {
        tag: "rx-table",
        doc: "A table written as columns; the rows are looped and a page of rows gets its links.",
        render: Render::Special(Special::Table),
        props: &[
            Prop {
                name: "rows",
                kind: Kind::Data,
                required: true,
                values: &[],
                doc: "The rows: a list, or a page (`Paginated` / `SimplePage`).",
            },
            text("as", false, "The row's name in the cells; row by default."),
            Prop {
                name: "key",
                kind: Kind::Data,
                required: false,
                values: &[],
                doc: "The row's key for route and can; {as}.id by default.",
            },
            text("caption", false, "A caption for screen readers."),
            text(
                "id",
                false,
                "The id of the table's wrapper; rx-table-N by default.",
            ),
            flag("card", "Puts the table in a card."),
        ],
        slots: &[
            Slot {
                doc: "Only `<rx-column>` and `<rx-row-actions>`.",
                ..DEFAULT_SLOT
            },
            Slot {
                name: "empty",
                optional: true,
                doc: "What shows when there are no rows.",
                ..DEFAULT_SLOT
            },
            Slot {
                name: "footer",
                optional: true,
                doc: "Rows written by hand (`<tr>`), drawn in a `<tfoot>` after the body: a total.",
                ..DEFAULT_SLOT
            },
        ],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-simple-table",
        doc: "The kit's table around rows written by hand (`<tr>` under `rx-for`), with an optional footer.",
        render: Render::Macro {
            module: Module::Ui,
            name: "table",
        },
        props: &[
            data(
                "head",
                true,
                "The heading cells: text, or [text, \"num\"] to align figures to the end.",
            ),
            text("caption", false, "A caption for screen readers."),
        ],
        slots: &[
            Slot {
                doc: "The body rows: `<tr>` elements.",
                ..DEFAULT_SLOT
            },
            Slot {
                name: "footer",
                into: Some("foot"),
                args: &[],
                optional: true,
                doc: "Rows drawn in a `<tfoot>` after the body: a total.",
            },
        ],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-column",
        doc: "One column of a table; the content is its cell, with the row in scope.",
        render: Render::Special(Special::Column),
        props: &[
            text("label", true, "The column heading."),
            choice(
                "align",
                &["start", "num"],
                "num aligns the cell to the end.",
            ),
            flag("hide-narrow", "Hides the column on narrow screens."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: Some("rx-table"),
    },
    Contract {
        tag: "rx-row-actions",
        doc: "A row of small buttons; in a table it is the last column.",
        render: Render::Macro {
            module: Module::Ui,
            name: "row_actions",
        },
        props: &[],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-button",
        doc: "A button; its content is the label.",
        render: Render::Macro {
            module: Module::Ui,
            name: "button",
        },
        props: &[
            text("label", true, "The label; or give it as content."),
            choice("variant", BUTTON_VARIANTS, "The look."),
            choice("type", BUTTON_TYPES, "The button's type."),
            text("name", false, "The name sent with a form."),
            text("value", false, "The value sent with a form."),
            choice("size", SIZES, "The size."),
            flag("block", "Fills the width."),
            text("icon", false, "An icon's name."),
            text("badge", false, "A count shown on the button."),
            text("key", false, "A keyboard shortcut."),
            flag("disabled", "Turns the button off."),
            text("disabled-reason", false, "Why the button is off."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-link-button",
        doc: "A link that looks like a button.",
        render: Render::Macro {
            module: Module::Ui,
            name: "link_button",
        },
        props: &[
            text("href", true, "The address; or use route."),
            text("label", true, "The label; or give it as content."),
            choice("variant", BUTTON_VARIANTS, "The look."),
            choice("size", SIZES, "The size."),
            text("icon", false, "An icon's name."),
            text("badge", false, "A count shown on the button."),
            text("key", false, "A keyboard shortcut."),
            flag("new-tab", "Opens in a new tab."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: Some("href"),
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-icon-button",
        doc: "A button with only an icon; the label is for screen readers and the tooltip.",
        render: Render::Macro {
            module: Module::Ui,
            name: "icon_button",
        },
        props: &[
            text("icon", true, "An icon's name."),
            text("label", true, "What screen readers say."),
            text("href", false, "Makes it a link; or use route."),
            choice("variant", ICON_BUTTON_VARIANTS, "The look."),
            choice("type", BUTTON_TYPES, "The button's type."),
            choice("size", SIZES, "The size."),
            text("key", false, "A keyboard shortcut."),
            text("badge", false, "A count shown on the button."),
            flag("disabled", "Turns the button off."),
            text("disabled-reason", false, "Why the button is off."),
            flag("new-tab", "Opens in a new tab."),
        ],
        slots: &[],
        events: &[],
        route_prop: Some("href"),
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-confirm",
        doc: "A button that asks before it acts, with its dialog.",
        render: Render::Macro {
            module: Module::Ui,
            name: "confirm",
        },
        props: &[
            text("id", true, "The dialog's id."),
            text("label", true, "The button's label."),
            text(
                "action",
                true,
                "The address the form is sent to; or use route.",
            ),
            text("title", true, "The dialog's heading."),
            text("message", true, "The question; or give it as content."),
            text("confirm-label", false, "The confirm button's text."),
            choice(
                "method",
                &["DELETE", "POST", "PUT", "PATCH"],
                "The HTTP method.",
            ),
            choice("size", SIZES, "The size."),
            text("icon", false, "An icon's name."),
            choice(
                "modal-icon",
                &["warning", "error", "info", "success"],
                "The dialog's icon.",
            ),
            text("key", false, "A keyboard shortcut."),
            text("cancel-label", false, "The cancel button's text."),
            Prop {
                name: "fields",
                kind: Kind::Data,
                required: false,
                values: &[],
                doc: "Hidden fields sent with the form.",
            },
            flag("button", "Whether to draw the opening button."),
        ],
        slots: &[slot_into("message", "The question.")],
        events: &["confirmed", "cancelled"],
        route_prop: Some("action"),
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-sheet",
        doc: "A dialog; open it with an `<rx-open-button>` or any `data-rx-open=\"id\"`.",
        render: Render::Macro {
            module: Module::Ui,
            name: "sheet",
        },
        props: &[
            text("id", true, "The dialog's id."),
            text("title", true, "The dialog's heading."),
            text("message", false, "A line under the heading."),
            flag("slide-over", "Slides in from the side."),
            choice("width", &["sm", "lg", "xl"], "The width."),
            text("icon", false, "An icon's name."),
        ],
        slots: &[DEFAULT_SLOT],
        events: SHEET_EVENTS,
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-open-button",
        doc: "A button that opens the sheet with the given id.",
        render: Render::Macro {
            module: Module::Ui,
            name: "open_button",
        },
        props: &[
            text("id", true, "The id of the sheet to open."),
            text("label", true, "The label."),
            choice("variant", BUTTON_VARIANTS, "The look."),
            choice("size", SIZES, "The size."),
            text("icon", false, "An icon's name."),
            text("key", false, "A keyboard shortcut."),
            text("badge", false, "A count shown on the button."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-action-sheet",
        doc: "A button that opens a sheet with a form, sent with htmx; the content is the form's fields.",
        render: Render::Macro {
            module: Module::Ui,
            name: "action_sheet",
        },
        props: &[
            text("id", true, "The sheet's id."),
            text("label", true, "The button's label."),
            text(
                "action",
                true,
                "The address the form is sent to; or use route.",
            ),
            text("title", true, "The sheet's heading."),
            text("description", false, "A line under the heading."),
            text("submit-label", false, "The submit button's text."),
            choice(
                "method",
                &["POST", "PUT", "PATCH", "DELETE"],
                "The HTTP method.",
            ),
            choice("variant", BUTTON_VARIANTS, "The button's look."),
            choice("size", SIZES, "The button's size."),
            text("icon", false, "The button's icon."),
            text("key", false, "A keyboard shortcut."),
            flag("slide-over", "Slides in from the side."),
            choice("width", &["sm", "lg", "xl"], "The width."),
            text("modal-icon", false, "The sheet's icon."),
            flag("danger", "Makes the submit button red."),
            text("target", false, "A selector the answer is swapped into."),
            text("swap", false, "How the answer is swapped in."),
            text("enctype", false, "The form's encoding, for files."),
            flag("button", "Whether to draw the opening button."),
        ],
        slots: &[DEFAULT_SLOT],
        events: SHEET_EVENTS,
        route_prop: Some("action"),
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-notification-bell",
        doc: "The bell with the unread count and its panel; nothing for guests.",
        render: Render::Macro {
            module: Module::Ui,
            name: "notification_bell",
        },
        props: &[
            number("count", "The unread count to start with."),
            text("id", false, "The panel's id prefix."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-alert",
        doc: "A message in a coloured box.",
        render: Render::Macro {
            module: Module::Ui,
            name: "alert",
        },
        props: &[
            text("message", true, "The message; or give it as content."),
            choice(
                "kind",
                &["info", "success", "warning", "error"],
                "The colour.",
            ),
            text("title", false, "A heading."),
        ],
        slots: &[slot_into("message", "The message.")],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-empty",
        doc: "What a list shows when it has nothing.",
        render: Render::Macro {
            module: Module::Ui,
            name: "empty",
        },
        props: &[
            text("title", true, "The heading."),
            text(
                "message",
                false,
                "A line under the heading; or give it as content.",
            ),
            text(
                "action-href",
                false,
                "The address of a button; or use route.",
            ),
            text("action-label", false, "The button's text."),
            text("icon", false, "An icon's name."),
        ],
        slots: &[slot_into("message", "The message.")],
        events: &[],
        route_prop: Some("action-href"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-custom-fields",
        doc: "The custom fields of a record: inputs in a form, entries on a page.",
        render: Render::Macro {
            module: Module::Custom {
                path: "renox/custom_fields.html",
                alias: "__rx_custom_fields",
            },
            name: "custom_fields",
        },
        props: &[
            data(
                "fields",
                true,
                "The list from `CustomField::views(&db, \"parties\", locale)`.",
            ),
            text(
                "name",
                false,
                "The model's `Json<CustomValues>` column (`extra`); inputs are named `extra[key]`.",
            ),
            data(
                "values",
                false,
                "The record's custom values (`party.extra`).",
            ),
            choice(
                "mode",
                &["form", "show"],
                "`form` (default) draws inputs, `show` draws entries.",
            ),
            text("id", false, "The element's id."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-transition-actions",
        doc: "A button for each state machine transition the user may run now.",
        render: Render::Macro {
            module: Module::Custom {
                path: "renox/transition_actions.html",
                alias: "__rx_transitions",
            },
            name: "transition_actions",
        },
        props: &[
            data(
                "actions",
                true,
                "The list from `record.available(&user, &db)`.",
            ),
            text(
                "action",
                true,
                "The address transitions post to (`/orders/1/transitions`); each button adds `/name`.",
            ),
            text("label", false, "The group's accessible name."),
            text(
                "reason-label",
                false,
                "The placeholder of the reason field.",
            ),
            text(
                "approval-label",
                false,
                "The note under a button that waits for approval.",
            ),
            text("id", false, "The element's id."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-form",
        doc: "A form: the CSRF field and the method field are written for you.",
        render: Render::Special(Special::Form),
        props: &[
            text(
                "action",
                false,
                "The address; or use route. Left out, the page's own.",
            ),
            choice("method", FORM_METHODS, "The HTTP method (POST by default)."),
            flag(
                "live",
                "Checks the fields with the server as people leave them.",
            ),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: Some("action"),
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-input",
        doc: "A text field with its label, hint and errors.",
        render: Render::Macro {
            module: Module::Ui,
            name: "input",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            choice("type", INPUT_TYPES, "The input's type."),
            text(
                "value",
                false,
                "The starting value; the old input wins after a failed submit.",
            ),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            text("autocomplete", false, "The autocomplete word."),
            text("placeholder", false, "Text shown while empty."),
            text("id", false, "The element's id."),
            text("prefix", false, "Text before the input."),
            text("suffix", false, "Text after the input."),
            data("datalist", false, "Suggestions."),
            flag("disabled", "Turns the field off."),
            flag("readonly", "The value can't be edited."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            flag("revealable", "A button shows the password."),
            flag("copyable", "A button copies the value."),
            flag(
                "hide-label",
                "Hides the label from sight, not from screen readers.",
            ),
            text("bag", false, "The named error bag."),
            text("min", false, "The smallest value (number and date types)."),
            text("max", false, "The largest value (number and date types)."),
            text("step", false, "The step between values (number types)."),
            text(
                "currency",
                false,
                "For type money: the currency code (APP_CURRENCY by default); sets the step and the suffix.",
            ),
            text(
                "scale",
                false,
                "For type quantity: the decimals of the quantity (0 to 9); sets the step.",
            ),
            text(
                "unit",
                false,
                "For type quantity: the unit shown as the suffix (kg, m, ...).",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-textarea",
        doc: "A multi-line text field with its label, hint and errors.",
        render: Render::Macro {
            module: Module::Ui,
            name: "textarea",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            text(
                "value",
                false,
                "The starting value; the old input wins after a failed submit.",
            ),
            number("rows", "The visible lines."),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            text("placeholder", false, "Text shown while empty."),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            flag("readonly", "The value can't be edited."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            flag(
                "hide-label",
                "Hides the label from sight, not from screen readers.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-select",
        doc: "A choice among options, with its label, hint and errors.",
        render: Render::Macro {
            module: Module::Ui,
            name: "select",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            data("options", true, "The options: pairs of value and label."),
            data(
                "selected",
                false,
                "The chosen value, or values with multiple.",
            ),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            text("placeholder", false, "The empty choice's text."),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            flag("multiple", "Several choices."),
            flag("searchable", "Type to filter the options."),
            text(
                "options-url",
                false,
                "Where the options come from, as people type.",
            ),
            flag("editable", "People can add and rename options."),
            flag(
                "hide-label",
                "Hides the label from sight, not from screen readers.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: CHANGED,
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-checkbox",
        doc: "A checkbox or switch with its label, hint and errors.",
        render: Render::Macro {
            module: Module::Ui,
            name: "checkbox",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            flag(
                "checked",
                "Starts checked; the old input wins after a failed submit.",
            ),
            text("hint", false, "A line under the field."),
            text(
                "value",
                false,
                "The value sent when checked (on by default).",
            ),
            flag("switch", "Draws a switch."),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            flag(
                "hide-label",
                "Hides the label from sight, not from screen readers.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-radio",
        doc: "One choice out of a few options, as radio buttons.",
        render: Render::Macro {
            module: Module::Ui,
            name: "radio",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            data(
                "options",
                true,
                "The options: values, [value, label] pairs or [value, label, description].",
            ),
            data(
                "selected",
                false,
                "The chosen value; the old input wins after a failed submit.",
            ),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            flag("inline", "Puts short options on one line."),
            number(
                "columns",
                "Lays a long list out in this many columns (2 or 3).",
            ),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-checkbox-list",
        doc: "Several choices out of a list, as checkboxes.",
        render: Render::Macro {
            module: Module::Ui,
            name: "checkbox_list",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            data(
                "options",
                true,
                "The options: values, [value, label] pairs or [value, label, description].",
            ),
            data(
                "selected",
                false,
                "The values ticked at first; the old input wins after a failed submit.",
            ),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            flag("inline", "Puts short options on one line."),
            number(
                "columns",
                "Lays a long list out in this many columns (2 or 3).",
            ),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-toggle-buttons",
        doc: "A row of buttons, one or several pressed.",
        render: Render::Macro {
            module: Module::Ui,
            name: "toggle_buttons",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            data(
                "options",
                true,
                "The options: values, [value, label] pairs or [value, label, description].",
            ),
            data(
                "selected",
                false,
                "The pressed value, or values with multiple; the old input wins after a failed submit.",
            ),
            flag("multiple", "Any number pressed."),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-file",
        doc: "A file field: a drop zone with the chosen files listed under it.",
        render: Render::Macro {
            module: Module::Ui,
            name: "file",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            text("accept", false, "The file types accepted."),
            flag("multiple", "Several files."),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            text("current", false, "The URL of the file stored now."),
            text("current-name", false, "The stored file's name."),
            flag("preview", "Shows the chosen images."),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-date-picker",
        doc: "A date field with a calendar.",
        render: Render::Macro {
            module: Module::Ui,
            name: "date_picker",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            data(
                "value",
                false,
                "The starting date; the old input wins after a failed submit.",
            ),
            text("min", false, "The earliest date."),
            text("max", false, "The latest date."),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            text("placeholder", false, "Text shown while empty."),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            flag("readonly", "The value can't be edited."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            data("disabled-dates", false, "Dates that can't be picked."),
            data("closed-weekdays", false, "Weekdays that can't be picked."),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: CHANGED,
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-tags-input",
        doc: "A field of tags: type a word, press Enter.",
        render: Render::Macro {
            module: Module::Ui,
            name: "tags_input",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            data(
                "value",
                false,
                "The starting tags; the old input wins after a failed submit.",
            ),
            data("suggestions", false, "Tags offered while typing."),
            text("hint", false, "A line under the field."),
            flag("required", "Marks the field as required."),
            text("placeholder", false, "Text shown while empty."),
            text("id", false, "The element's id."),
            flag("disabled", "Turns the field off."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
            text("bag", false, "The named error bag."),
        ],
        slots: &[],
        events: CHANGED,
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-form-errors",
        doc: "Every error of the last submit, above the form.",
        render: Render::Macro {
            module: Module::Ui,
            name: "form_errors",
        },
        props: &[text("title", false, "The heading.")],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-form-grid",
        doc: "Fields side by side on wide screens, one column on phones.",
        render: Render::Macro {
            module: Module::Ui,
            name: "form_grid",
        },
        props: &[number("columns", "How many columns (2 by default).")],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-repeater",
        doc: "Rows that people add, remove and move; the content is one row's fields.",
        render: Render::Macro {
            module: Module::Ui,
            name: "repeater",
        },
        props: &[
            text(
                "name",
                true,
                "The field's name; rows are named `name[0][field]`.",
            ),
            text("label", true, "The label."),
            data(
                "rows",
                false,
                "The rows to show at first; the old input wins after a failed submit.",
            ),
            text("add-label", false, "The text of the add button."),
            text(
                "item-label",
                false,
                "The title of each row (the label by default).",
            ),
            number("min", "The fewest rows."),
            number("max", "The most rows."),
            flag(
                "reorderable",
                "Whether rows can move; `:reorderable=\"false\"` fixes their order.",
            ),
            text("hint", false, "A line under the field."),
            text("id", false, "The element's id."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
        ],
        slots: &[Slot {
            name: "",
            into: None,
            args: &["row", "prefix"],
            optional: false,
            doc: "One row's fields; `row` is its data and `prefix` its name part, like `lines[0]`.",
        }],
        events: &["added", "removed"],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-key-value",
        doc: "Pairs of text, like headers or settings: a key and a value per row.",
        render: Render::Macro {
            module: Module::Ui,
            name: "key_value",
        },
        props: &[
            text("name", true, "The field's name."),
            text("label", true, "The label."),
            data(
                "value",
                false,
                "The pairs: a map, a KeyValues or a list of pairs.",
            ),
            text("key-label", false, "The heading over the keys."),
            text("value-label", false, "The heading over the values."),
            text("add-label", false, "The text of the add button."),
            text("hint", false, "A line under the field."),
            text("id", false, "The element's id."),
            text(
                "span",
                false,
                "How many columns it takes in a form grid, or full.",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-fieldset",
        doc: "A titled group of fields inside a long form.",
        render: Render::Macro {
            module: Module::Ui,
            name: "fieldset",
        },
        props: &[
            text("legend", true, "The group's title."),
            text("hint", false, "A line under the title."),
            number("columns", "How many columns the fields use (1 by default)."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-show-when",
        doc: "Fields shown only while another field has one of the values.",
        render: Render::Macro {
            module: Module::Ui,
            name: "show_when",
        },
        props: &[
            text("field", true, "The name of the field to watch."),
            text("values", true, "One value, or a list with :values=\"[…]\"."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-hide-when",
        doc: "Fields hidden while another field has one of the values.",
        render: Render::Macro {
            module: Module::Ui,
            name: "hide_when",
        },
        props: &[
            text("field", true, "The name of the field to watch."),
            text("values", true, "One value, or a list with :values=\"[…]\"."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-infolist",
        doc: "Labelled values of a record, in columns.",
        render: Render::Macro {
            module: Module::Ui,
            name: "infolist",
        },
        props: &[
            number(
                "columns",
                "How many columns the entries use (1 by default).",
            ),
            flag("inline", "Puts each label beside its value."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-entry",
        doc: "One labelled value in an infolist.",
        render: Render::Macro {
            module: Module::Ui,
            name: "entry",
        },
        props: &[
            text("label", true, "The label."),
            data("value", false, "The value to show."),
            text(
                "format",
                false,
                "How to show it: date, datetime, money, number, since, bool, color, image, key_value, markdown...",
            ),
            data("badge", false, "A map from value to badge colour."),
            data("labels", false, "A map from value to the text shown."),
            text("url", false, "Makes the value a link."),
            flag("new-tab", "Opens the link in a new tab."),
            flag("copyable", "Adds a copy button."),
            text("tooltip", false, "A title shown on hover."),
            text("placeholder", false, "Shown when the value is empty."),
            text("hint", false, "A line under the value."),
            text("prefix", false, "Text before the value."),
            text("suffix", false, "Text after the value."),
            number("limit", "The most characters shown."),
            number("words", "The most words shown."),
            choice(
                "list",
                &["comma", "lines", "bullets"],
                "How a list value is laid out.",
            ),
            number("limit-list", "The most items shown of a list."),
            number("decimals", "Decimals of a number or money value."),
            text("currency", false, "The currency code of a money value."),
            number("divide-by", "Divides a money value by this."),
            text("date-format", false, "A strftime format for dates."),
            number("image-size", "The size of an image value, in pixels."),
            flag("circular", "Rounds an image value."),
            text("span", false, "How many columns it takes, or full."),
            flag("inline", "Puts the label beside the value."),
            flag("hide-label", "Hides the label from sight."),
            text("id", false, "The element's id."),
            data("prefix-actions", false, "Actions before the value."),
            data("suffix-actions", false, "Actions after the value."),
        ],
        slots: &[Slot {
            optional: true,
            doc: "Content shown instead of the value.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-repeatable",
        doc: "A list of records inside a record, each shown as a small infolist.",
        render: Render::Macro {
            module: Module::Ui,
            name: "repeatable",
        },
        props: &[
            text("label", true, "The label."),
            data("items", true, "The records to show."),
            number("columns", "How many columns each record uses."),
            text("placeholder", false, "Shown when there are no items."),
            text("span", false, "How many columns it takes, or full."),
            flag("hide-label", "Hides the label from sight."),
        ],
        slots: &[Slot {
            name: "",
            into: None,
            args: &["item"],
            optional: false,
            doc: "One record's entries; `item` is its data.",
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-list",
        doc: "A list of rows in a surface; write each row as an `li`.",
        render: Render::Macro {
            module: Module::Ui,
            name: "list",
        },
        props: &[
            text("id", false, "The list's id."),
            text("label", false, "The list's accessible name."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-card-grid",
        doc: "Cards in a grid that fills the row.",
        render: Render::Macro {
            module: Module::Ui,
            name: "card_grid",
        },
        props: &[],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-media-card",
        doc: "A card that is one link: a picture, a title and notes.",
        render: Render::Macro {
            module: Module::Ui,
            name: "media_card",
        },
        props: &[
            text("href", true, "Where the card links."),
            text("title", true, "The card's title."),
            text("image", false, "The picture's URL."),
            text(
                "subtitle",
                false,
                "A line under the title, such as a price.",
            ),
            text("note", false, "A note, such as Sold out."),
            flag("dimmed", "Greys the picture out."),
            text("image-alt", false, "The picture's alt text."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-progress",
        doc: "A bar showing how far along something is.",
        render: Render::Macro {
            module: Module::Ui,
            name: "progress",
        },
        props: &[
            Prop {
                name: "value",
                kind: Kind::Number,
                required: true,
                values: &[],
                doc: "How far along it is.",
            },
            number("max", "The value that means done (100 by default)."),
            text("label", false, "The bar's accessible name."),
            flag(
                "show-value",
                "Whether the percentage shows; `:show-value=\"false\"` hides it.",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-grid",
        doc: "The data grid of a `renox::grid` page; the content draws the `custom` columns.",
        render: Render::Macro {
            module: Module::Grid,
            name: "grid",
        },
        props: &[
            data("page", true, "The `GridPage` the handler built."),
            data("tools", false, "Extra content for the grid's toolbar."),
        ],
        slots: &[Slot {
            name: "",
            into: None,
            args: &["row", "column"],
            optional: true,
            doc: "Draws a `custom` column: it receives the row and the column.",
        }],
        events: &["selected", "sorted", "filtered"],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-stats",
        doc: "Figures side by side: two per row on phones, `columns` from tablet width.",
        render: Render::Macro {
            module: Module::Ui,
            name: "stats",
        },
        props: &[number("columns", "How many per row (4 by default).")],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-stat",
        doc: "One figure with an optional change, sparkline and link.",
        render: Render::Macro {
            module: Module::Ui,
            name: "stat",
        },
        props: &[
            text("label", true, "What the figure is."),
            text("value", true, "The figure, already formatted."),
            number("delta", "The change in percent, shown with an arrow."),
            text("delta-label", false, "What the change is against."),
            choice(
                "good",
                &["up", "down", "none"],
                "Which way is good (up by default).",
            ),
            data("trend", false, "Numbers for a sparkline."),
            text("url", false, "Makes the figure a link."),
            text("hint", false, "A line shown when there is no delta."),
            number("decimals", "Decimals of the delta (1 by default)."),
            text("icon", false, "One of the kit's icons."),
        ],
        slots: &[],
        events: &[],
        route_prop: Some("url"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-dashboard",
        doc: "Widgets in a grid: `columns` from tablet width up, one on phones.",
        render: Render::Macro {
            module: Module::Ui,
            name: "dashboard",
        },
        props: &[number("columns", "How many columns (2 by default).")],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-widget",
        doc: "A card on a dashboard; its content is given, or loaded from `url`.",
        render: Render::Macro {
            module: Module::Ui,
            name: "widget",
        },
        props: &[
            text("title", false, "The heading."),
            text("description", false, "A line under the heading."),
            text("span", false, "Columns taken (2, 3 or full)."),
            text(
                "url",
                false,
                "A fragment loaded into the card once the page is shown.",
            ),
            number("poll", "Reload the fragment every this many seconds."),
            text("id", false, "The element's id."),
        ],
        slots: &[Slot {
            name: "",
            into: None,
            args: &[],
            optional: true,
            doc: "The content; without it the card loads `url`.",
        }],
        events: &[],
        route_prop: Some("url"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-period-filter",
        doc: "Links that set `?period=` for the figures and charts below.",
        render: Render::Macro {
            module: Module::Ui,
            name: "period_filter",
        },
        props: &[
            data("selected", true, "The handler's `Period`."),
            data("options", false, "[key, label] pairs."),
            text("label", false, "The navigation's accessible name."),
            flag(
                "custom",
                "Whether a custom range is offered; `:custom=\"false\"` hides it.",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-chart",
        doc: "A chart drawn as SVG with a data table; it calls the `chart(...)` function.",
        render: Render::Function("chart"),
        props: &[
            Prop {
                name: "kind",
                kind: Kind::Enum,
                required: true,
                values: &[
                    "line", "area", "bar", "pie", "doughnut", "scatter", "bubble", "heatmap",
                ],
                doc: "The kind of chart.",
            },
            data("data", false, "A series, a list of numbers or a trend."),
            data("labels", false, "The labels along the x axis."),
            data("values", false, "The numbers."),
            data("series", false, "Several named series."),
            data("points", false, "The points of a scatter or bubble chart."),
            data("columns", false, "A heatmap's column labels."),
            data("rows", false, "A heatmap's row labels."),
            data("cells", false, "A heatmap's cells."),
            text("name", false, "The name of a single series."),
            text("title", false, "The chart's title."),
            number("height", "The height in pixels (240 by default)."),
            choice(
                "format",
                &["number", "money", "percent"],
                "How values are shown.",
            ),
            number("decimals", "Decimals shown."),
            text("currency", false, "A currency code for money."),
            number("divide-by", "Divides money values by this."),
            flag("stacked", "Stack the series."),
            flag("legend", "Whether the legend shows."),
            flag("table", "Whether the data table is offered."),
            text("x-format", false, "How the x labels are shown."),
            text("x-title", false, "The x axis title."),
            text("y-title", false, "The y axis title."),
            text("size-format", false, "How bubble sizes are shown."),
            text("size-title", false, "The size legend title."),
            text("id", false, "The element's id."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-menu",
        doc: "A button that opens a menu of actions; its content is the items.",
        render: Render::Macro {
            module: Module::Ui,
            name: "menu",
        },
        props: &[
            text("label", true, "The button's label."),
            text("id", false, "The menu list's id."),
            choice("variant", BUTTON_VARIANTS, "The look."),
            choice("size", SIZES, "The size."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-menu-link",
        doc: "A link in a menu.",
        render: Render::Macro {
            module: Module::Ui,
            name: "menu_link",
        },
        props: &[
            text("href", true, "The address; or use route."),
            text("label", true, "The label; or give it as content."),
            text("icon", false, "An icon's name."),
            flag("download", "A file to save; the page stays."),
            flag("new-tab", "Opens in a new tab."),
            flag("danger", "Draws the item as a dangerous action."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: Some("href"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-menu-action",
        doc: "A menu item that sends a form.",
        render: Render::Macro {
            module: Module::Ui,
            name: "menu_action",
        },
        props: &[
            text(
                "action",
                true,
                "The address the form is sent to; or use route.",
            ),
            text("label", true, "The label; or give it as content."),
            choice(
                "method",
                &["POST", "PUT", "PATCH", "DELETE"],
                "The HTTP method.",
            ),
            text("icon", false, "An icon's name."),
            flag("danger", "Draws the item as a dangerous action."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: Some("action"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-menu-section",
        doc: "A titled part of a menu.",
        render: Render::Macro {
            module: Module::Ui,
            name: "menu_section",
        },
        props: &[text("title", true, "The section's heading.")],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-menu-separator",
        doc: "A line between parts of a menu.",
        render: Render::Macro {
            module: Module::Ui,
            name: "menu_separator",
        },
        props: &[],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-menu-open",
        doc: "A menu item that opens a sheet or dialog by its id.",
        render: Render::Macro {
            module: Module::Ui,
            name: "menu_open",
        },
        props: &[
            text("id", true, "The id of the sheet to open."),
            text("label", true, "The label; or give it as content."),
            text("icon", false, "An icon's name."),
            flag("danger", "Draws the item as a dangerous action."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-link-tabs",
        doc: "Links that look like a segmented control, for switching between pages; from data (`items`) or from `<rx-link-tab>` children.",
        render: Render::Special(Special::LinkTabs),
        props: &[
            data(
                "items",
                false,
                "A list of [href, label] pairs; or write `<rx-link-tab>` children instead.",
            ),
            text("current", false, "With `items`: the href shown as chosen."),
            text("label", false, "The navigation's accessible name."),
        ],
        slots: &[Slot {
            optional: true,
            doc: "Only `<rx-link-tab>` elements (also under `rx-for` and `rx-if`).",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-link-tab",
        doc: "One link of `<rx-link-tabs>`; the label is its content or `label`.",
        render: Render::Special(Special::LinkTab),
        props: &[
            text("href", true, "Where the link goes."),
            text("label", false, "The link's text; or give it as content."),
            flag(
                "current",
                "Draws the link as the chosen one (`aria-current=\"page\"`).",
            ),
        ],
        slots: &[Slot {
            optional: true,
            doc: "The label.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: Some("rx-link-tabs"),
    },
    Contract {
        tag: "rx-navbar",
        doc: "The bar across the top: brand, links, search and the account menu.",
        render: Render::Macro {
            module: Module::Ui,
            name: "navbar",
        },
        props: &[
            text("brand", false, "The brand name."),
            text("href", false, "Where the brand links to (`/` by default)."),
            text("logo", false, "The URL of the brand's logo image."),
            flag("mark", "Shows the brand's initial in a badge."),
            text(
                "width",
                false,
                "How wide the bar's content is: narrow, wide or full.",
            ),
            text("label", false, "What screen readers call the navigation."),
            flag(
                "skip",
                "Whether the skip link shows (`:skip=\"false\"` hides it).",
            ),
            data("tabs", false, "The phone tab bar: a list of tabs."),
            text("tabs-label", false, "What screen readers call the tab bar."),
        ],
        slots: &[Slot {
            optional: true,
            doc: "The bar's links, search and menus.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-nav-links",
        doc: "The navbar's links, kept together (a row that scrolls on phones).",
        render: Render::Macro {
            module: Module::Ui,
            name: "nav_links",
        },
        props: &[text(
            "menu",
            false,
            "A menu's name: the links modules and the app added with `App::nav`, for this user.",
        )],
        slots: &[Slot {
            optional: true,
            doc: "Links written by hand, after the menu's.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-nav-link",
        doc: "One link of a navbar; `active` marks the current section.",
        render: Render::Macro {
            module: Module::Ui,
            name: "nav_link",
        },
        props: &[
            text("href", true, "The address; or use route."),
            text("label", true, "The label; or give it as content."),
            flag(
                "active",
                "Marks the current section, e.g. `:active=\"route_is('orders.*')\"`.",
            ),
            text("badge", false, "A count after the label."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: Some("href"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-nav-search",
        doc: "The navbar's search: a toggle button and the box it opens.",
        render: Render::Macro {
            module: Module::Ui,
            name: "nav_search",
        },
        props: &[
            text("label", false, "What screen readers call the toggle."),
            text("id", false, "The box's id."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-sidebar",
        doc: "Navigation down the side, for back offices.",
        render: Render::Macro {
            module: Module::Ui,
            name: "sidebar",
        },
        props: &[
            text("brand", false, "The brand name."),
            text("href", false, "Where the brand links to (`/` by default)."),
            text("logo", false, "The URL of the brand's logo image."),
            flag(
                "mark",
                "Shows the brand's initial in a badge (on by default).",
            ),
            text("label", false, "What screen readers call the navigation."),
            flag(
                "skip",
                "Whether the skip link shows (`:skip=\"false\"` hides it).",
            ),
            text(
                "menu",
                false,
                "A menu's name: the links modules and the app added with `App::nav`, grouped, for this user.",
            ),
        ],
        slots: &[Slot {
            optional: true,
            doc: "Links written by hand, after the menu's.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-sidebar-link",
        doc: "One link of a sidebar; `active` marks the current section.",
        render: Render::Macro {
            module: Module::Ui,
            name: "sidebar_link",
        },
        props: &[
            text("href", true, "The address; or use route."),
            text("label", true, "The label; or give it as content."),
            flag("active", "Marks the current section."),
            text("badge", false, "A count after the label."),
            text("icon", false, "An icon's name, shown before the label."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: Some("href"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-sidebar-section",
        doc: "A heading between groups of sidebar links.",
        render: Render::Macro {
            module: Module::Ui,
            name: "sidebar_section",
        },
        props: &[text("title", true, "The heading; or give it as content.")],
        slots: &[slot_into("title", "The heading.")],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-shell",
        doc: "The frame of a back office: a sidebar and the main column beside it.",
        render: Render::Element {
            tag: "div",
            class: "rx-shell",
        },
        props: &[flag(
            "fill",
            "The dashboard standard: on a large screen the shell fills the screen, the window never scrolls and each panel scrolls on its own; tablets and phones scroll the page.",
        )],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-icon",
        doc: "One of the kit's icons, drawn inline in the text's colour.",
        render: Render::Macro {
            module: Module::Ui,
            name: "icon",
        },
        props: &[
            text("name", true, "The icon's name (docs/ui.md lists them)."),
            number("size", "The size in pixels (20 by default)."),
            text(
                "label",
                false,
                "Makes the icon mean something to screen readers; without it the icon is decoration.",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-columns",
        doc: "Children side by side from tablet width up, one column on phones.",
        render: Render::Macro {
            module: Module::Ui,
            name: "columns",
        },
        props: &[number("count", "How many columns (2 by default).")],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-thumbnail",
        doc: "A small square picture, for example in a table row.",
        render: Render::Macro {
            module: Module::Ui,
            name: "thumbnail",
        },
        props: &[
            text("src", true, "The picture's URL."),
            text("alt", false, "The picture's alt text; empty by default."),
            text("href", false, "Makes the picture a link."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-tab-panel",
        doc: "One panel for a `tabs(...)` control written as a macro; with tags use `<rx-tabs>` and `<rx-tab>`.",
        render: Render::Macro {
            module: Module::Ui,
            name: "tab_panel",
        },
        props: &[
            text("id", true, "The tabs' id."),
            text("key", true, "The key of the tab this panel belongs to."),
            flag("selected", "Shows the panel at first."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-menu-button",
        doc: "A menu item that is a plain button, for what its attributes make it do (htmx).",
        render: Render::Macro {
            module: Module::Ui,
            name: "menu_button",
        },
        props: &[
            text("label", true, "The label; or give it as content."),
            text("icon", false, "An icon's name."),
            flag("danger", "Draws the item as a dangerous action."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: None,
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-action-group",
        doc: "Several actions behind one button; its content is the menu items.",
        render: Render::Macro {
            module: Module::Ui,
            name: "action_group",
        },
        props: &[
            text(
                "label",
                false,
                "The button's text; without it an icon button named \"Actions\".",
            ),
            text("id", false, "The menu list's id."),
            text("icon", false, "An icon's name."),
            choice("variant", BUTTON_VARIANTS, "The look."),
            choice("size", SIZES, "The size."),
        ],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-import-action",
        doc: "A button that opens a sheet to import a CSV or Excel file; the handler answers with an `ImportReport`.",
        render: Render::Macro {
            module: Module::Ui,
            name: "import_action",
        },
        props: &[
            text("id", true, "The sheet's id."),
            text("label", true, "The button's label."),
            text(
                "action",
                true,
                "The address the file is sent to; or use route.",
            ),
            text("title", true, "The sheet's heading."),
            text("description", false, "A line under the heading."),
            data(
                "columns",
                false,
                "The file's column names, shown to the reader.",
            ),
            text(
                "template-url",
                false,
                "A link to an empty file to fill in (`renox::import::template`).",
            ),
            text("submit-label", false, "The submit button's text."),
            text("accept", false, "The file types the field takes."),
            text("name", false, "The file field's name (file by default)."),
            choice("variant", BUTTON_VARIANTS, "The button's look."),
            choice("size", SIZES, "The button's size."),
            text("icon", false, "The button's icon (upload by default)."),
            text("key", false, "A keyboard shortcut."),
            flag("button", "Whether to draw the opening button."),
            flag(
                "check",
                "Adds a ticked \"check the file first\" box, sent as `dry_run` (see `Import::dry_run_if`).",
            ),
        ],
        slots: &[],
        events: &[],
        route_prop: Some("action"),
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-wizard-action",
        doc: "A button that opens a sheet holding a wizard; each `<rx-wizard-step>` inside is one step.",
        render: Render::Special(Special::WizardAction),
        props: &[
            text("id", true, "The sheet's id."),
            text("label", true, "The button's label."),
            text(
                "action",
                true,
                "The address the form is sent to; or use route.",
            ),
            text("title", true, "The sheet's heading."),
            text("submit-label", false, "The last step's submit button."),
            text("description", false, "A line under the heading."),
            choice(
                "method",
                &["POST", "PUT", "PATCH", "DELETE"],
                "The HTTP method.",
            ),
            choice("variant", BUTTON_VARIANTS, "The button's look."),
            choice("size", SIZES, "The button's size."),
            text("icon", false, "The button's icon."),
            text("key", false, "A keyboard shortcut."),
            flag("slide-over", "Slides in from the side."),
            choice("width", &["sm", "lg", "xl"], "The width."),
            flag(
                "live",
                "Live validation (on by default); `:live=\"false\"` turns it off.",
            ),
            text("target", false, "A selector the answer is swapped into."),
            text("swap", false, "How the answer is swapped in."),
            text("enctype", false, "The form's encoding, for files."),
            flag("button", "Whether to draw the opening button."),
        ],
        slots: &[Slot {
            doc: "Only `<rx-wizard-step>` elements.",
            ..DEFAULT_SLOT
        }],
        events: SHEET_EVENTS,
        route_prop: Some("action"),
        attrs: true,
        parent: None,
    },
    Contract {
        tag: "rx-event-stream",
        doc: "Opens the notifications stream on a page without the bell, so `state.broadcast(...)` events arrive as DOM events.",
        render: Render::Macro {
            module: Module::Ui,
            name: "event_stream",
        },
        props: &[],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-mail",
        doc: "A mail's layout: `{% extends %}` of the mail layout, with its content and footer.",
        render: Render::Special(Special::Mail),
        props: &[
            text(
                "subject",
                false,
                "The mail's title (the layout's `<title>`).",
            ),
            text(
                "layout",
                false,
                "The layout's file name; renox/mail/layout.html by default. Plain text.",
            ),
        ],
        slots: &[
            Slot {
                doc: "The mail's body.",
                ..DEFAULT_SLOT
            },
            Slot {
                name: "footer",
                optional: true,
                doc: "Replaces the layout's footer line.",
                ..DEFAULT_SLOT
            },
        ],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-mail-text",
        doc: "A mail's plain-text version (a `.txt` view): the text as written, with `{{ }}`, `rx-if` and `rx-for` (on a `<template>`).",
        render: Render::Special(Special::MailText),
        props: &[],
        slots: &[Slot {
            doc: "The text; the line break right after the opening tag and right before the closing one are not part of it.",
            ..DEFAULT_SLOT
        }],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-mail-button",
        doc: "A button for a mail, with its address written under it for clients that hide buttons.",
        render: Render::Macro {
            module: Module::Mail,
            name: "button",
        },
        props: &[
            text("url", true, "Where the button goes."),
            text("label", true, "The label; or give it as content."),
        ],
        slots: &[slot_into("label", "The label.")],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-mail-panel",
        doc: "A highlighted box in a mail.",
        render: Render::Macro {
            module: Module::Mail,
            name: "panel",
        },
        props: &[],
        slots: &[DEFAULT_SLOT],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-mail-table",
        doc: "A table for a mail: rows of cells, an optional heading and an optional total row.",
        render: Render::Macro {
            module: Module::Mail,
            name: "table",
        },
        props: &[
            data("rows", true, "The rows: a list of lists of cells."),
            data("head", false, "The heading cells."),
            data("total", false, "The last row's cells, drawn in bold."),
        ],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
    Contract {
        tag: "rx-mail-divider",
        doc: "A thin line between parts of a mail.",
        render: Render::Macro {
            module: Module::Mail,
            name: "divider",
        },
        props: &[],
        slots: &[],
        events: &[],
        route_prop: None,
        attrs: false,
        parent: None,
    },
];

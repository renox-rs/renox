//! Components that modules and plugins register (`Registry::component`): an owned, public
//! description turned into the compiler's [`Contract`] at boot.

use super::attrs::Kind;
use super::contracts::{Contract, Module, Prop as Compiled, Render, Slot};

/// One attribute of a [`Component`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Prop {
    name: String,
    kind: Kind,
    required: bool,
    values: Vec<String>,
    doc: String,
}

impl Prop {
    fn new(name: &str, kind: Kind) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            required: false,
            values: Vec::new(),
            doc: String::new(),
        }
    }

    /// Text, possibly with `{{ }}`: `title="Hello {{ name }}"`.
    pub fn text(name: &str) -> Self {
        Self::new(name, Kind::Text)
    }

    /// A flag: a bare attribute, `"true"`/`"false"`, or `:name="expr"`.
    pub fn bool(name: &str) -> Self {
        Self::new(name, Kind::Bool)
    }

    /// A number.
    pub fn number(name: &str) -> Self {
        Self::new(name, Kind::Number)
    }

    /// Data from the page's context: only `:name="expr"` is accepted.
    pub fn data(name: &str) -> Self {
        Self::new(name, Kind::Data)
    }

    /// One of a fixed list of words.
    pub fn choice(name: &str, values: &[&str]) -> Self {
        let mut prop = Self::new(name, Kind::Enum);
        prop.values = values.iter().map(|v| (*v).to_owned()).collect();
        prop
    }

    /// The attribute must be given.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// One line about it, for tooling.
    pub fn doc(mut self, doc: &str) -> Self {
        self.doc = doc.to_owned();
        self
    }
}

/// A component a module registers with `Registry::component`: a tag that
/// compiles to a call of one of the module's template macros.
///
/// ```
/// use renox::view::{Component, Prop};
///
/// let hello = Component::macro_call("rx-hello", "hello/ui.html", "hello")
///     .doc("Greets someone.")
///     .prop(Prop::text("name").required());
/// # let _ = hello;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Component {
    tag: String,
    template: String,
    macro_name: String,
    doc: String,
    props: Vec<Prop>,
    content: bool,
    attrs: bool,
}

impl Component {
    /// `<tag …>` becomes `macro_name(…)` of the template `template`, which the
    /// module adds with [`add_template`](crate::view::add_template). The tag
    /// starts with `rx-`.
    pub fn macro_call(tag: &str, template: &str, macro_name: &str) -> Self {
        Self {
            tag: tag.to_owned(),
            template: template.to_owned(),
            macro_name: macro_name.to_owned(),
            doc: String::new(),
            props: Vec::new(),
            content: true,
            attrs: false,
        }
    }

    /// One line about the component, for tooling.
    pub fn doc(mut self, doc: &str) -> Self {
        self.doc = doc.to_owned();
        self
    }

    /// Adds an attribute.
    pub fn prop(mut self, prop: Prop) -> Self {
        self.props.push(prop);
        self
    }

    /// Whether the tag takes content (the macro's `caller()`). On by default.
    pub fn content(mut self, takes: bool) -> Self {
        self.content = takes;
        self
    }

    /// Lets other HTML attributes (`data-*`, `@click`, `title`…) through as the
    /// macro's `attrs`.
    pub fn attrs(mut self, pass: bool) -> Self {
        self.attrs = pass;
        self
    }

    /// The tag, such as `rx-hello`.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// Why this can't be registered, if so.
    pub(crate) fn problem(&self) -> Option<String> {
        if !self.tag.starts_with("rx-") || self.tag.len() <= 3 {
            return Some(format!("the tag \"{}\" must start with \"rx-\"", self.tag));
        }
        if super::contracts::BUILTIN.iter().any(|c| c.tag == self.tag) {
            return Some(format!("<{}> is a built-in component", self.tag));
        }
        None
    }

    /// The compiler's form. The strings are leaked: components are registered once
    /// at boot and live as long as the process.
    pub(crate) fn contract(&self) -> Contract {
        fn leak(s: &str) -> &'static str {
            Box::leak(s.to_owned().into_boxed_str())
        }
        let props: Vec<Compiled> = self
            .props
            .iter()
            .map(|p| Compiled {
                name: leak(&p.name),
                kind: p.kind,
                required: p.required,
                values: Box::leak(
                    p.values
                        .iter()
                        .map(|v| leak(v))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
                doc: leak(&p.doc),
            })
            .collect();
        let alias: String = self
            .template
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let slots: &'static [Slot] = if self.content {
            Box::leak(Box::new([Slot {
                name: "",
                into: None,
                args: &[],
                optional: true,
                doc: "The content.",
            }]))
        } else {
            &[]
        };
        Contract {
            tag: leak(&self.tag),
            doc: leak(&self.doc),
            render: Render::Macro {
                module: Module::Custom {
                    path: leak(&self.template),
                    alias: leak(&format!("__rx_p_{alias}")),
                },
                name: leak(&self.macro_name),
            },
            props: Box::leak(props.into_boxed_slice()),
            slots,
            events: &[],
            route_prop: None,
            attrs: self.attrs,
            parent: None,
        }
    }
}

//! The tree: tokens turned into nodes. Only components (`rx-*`, `app-*`) and elements carrying
//! `rx-if` / `rx-elif` / `rx-else` / `rx-for` (and a conditional `?attr`) become `Element`s; every other tag stays inside `Text` spans, so
//! unbalanced HTML elsewhere is never touched.

use std::ops::Range;

use super::CompileError;
use super::scan::{Attr, Token};

/// Elements that never have an end tag.
pub(super) const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// A piece of the template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node<'a> {
    /// Source text, by span.
    Text(Range<usize>),
    /// A component or a control-flow element.
    Element {
        /// The tag name, lower-cased.
        name: String,
        /// Its attributes.
        attrs: Vec<Attr<'a>>,
        /// What is between its start and end tag.
        children: Vec<Node<'a>>,
        /// The start tag.
        open: Range<usize>,
        /// The end tag; `None` for void and self-closing elements.
        close: Option<Range<usize>>,
        /// The line of the start tag.
        line: usize,
    },
}

/// Whether the name is a component's (`rx-…`, `app-…` or `live-…`).
pub(crate) fn is_component(name: &str) -> bool {
    name.starts_with("rx-") || name.starts_with("app-") || name.starts_with("live-")
}

fn has_flow(attrs: &[Attr<'_>]) -> bool {
    attrs.iter().any(|a| {
        matches!(a.name, "rx-if" | "rx-elif" | "rx-else" | "rx-for") || a.name.starts_with('?')
    })
}

struct Frame<'a> {
    name: String,
    attrs: Vec<Attr<'a>>,
    open: Range<usize>,
    line: usize,
    children: Vec<Node<'a>>,
    /// Plain tags of the same name opened inside, whose end tags are not ours.
    depth: usize,
}

fn push_text<'a>(into: &mut Vec<Node<'a>>, span: Range<usize>) {
    if let Some(Node::Text(last)) = into.last_mut()
        && last.end == span.start
    {
        last.end = span.end;
        return;
    }
    into.push(Node::Text(span));
}

fn never_closed(f: &Frame<'_>) -> CompileError {
    CompileError {
        line: f.line,
        message: format!("<{}> opened here is never closed", f.name),
    }
}

/// Builds the tree of `tokens`.
pub(crate) fn build<'a>(tokens: Vec<Token<'a>>) -> Result<Vec<Node<'a>>, CompileError> {
    let mut root: Vec<Node<'a>> = Vec::new();
    let mut stack: Vec<Frame<'a>> = Vec::new();
    for token in tokens {
        match token {
            Token::Text(span) => match stack.last_mut() {
                Some(f) => push_text(&mut f.children, span),
                None => push_text(&mut root, span),
            },
            Token::Open {
                name,
                attrs,
                self_closing,
                span,
                line,
            } => {
                let leaf = self_closing || VOID.contains(&name.as_str()) || name == "rx-props";
                let target = match stack.last_mut() {
                    Some(f) => &mut f.children,
                    None => &mut root,
                };
                if is_component(&name) || has_flow(&attrs) {
                    if leaf {
                        target.push(Node::Element {
                            name,
                            attrs,
                            children: Vec::new(),
                            open: span,
                            close: None,
                            line,
                        });
                    } else {
                        stack.push(Frame {
                            name,
                            attrs,
                            open: span,
                            line,
                            children: Vec::new(),
                            depth: 0,
                        });
                    }
                } else {
                    if !leaf
                        && let Some(f) = stack.last_mut()
                        && f.name == name
                    {
                        f.depth += 1;
                    }
                    match stack.last_mut() {
                        Some(f) => push_text(&mut f.children, span),
                        None => push_text(&mut root, span),
                    }
                }
            }
            Token::Close { name, span, line } => {
                let top_matches = stack.last().is_some_and(|f| f.name == name);
                if top_matches && stack.last().is_some_and(|f| f.depth == 0) {
                    let f = stack.pop().expect("checked");
                    let node = Node::Element {
                        name: f.name,
                        attrs: f.attrs,
                        children: f.children,
                        open: f.open,
                        close: Some(span),
                        line: f.line,
                    };
                    match stack.last_mut() {
                        Some(p) => p.children.push(node),
                        None => root.push(node),
                    }
                } else if top_matches {
                    if let Some(f) = stack.last_mut() {
                        f.depth -= 1;
                        push_text(&mut f.children, span);
                    }
                } else if is_component(&name) {
                    if stack.iter().any(|f| f.name == name) {
                        let top = stack.last().expect("a frame matches");
                        return Err(never_closed(top));
                    }
                    return Err(CompileError {
                        line,
                        message: format!("</{name}> closes nothing"),
                    });
                } else {
                    match stack.last_mut() {
                        Some(f) => push_text(&mut f.children, span),
                        None => push_text(&mut root, span),
                    }
                }
            }
        }
    }
    if let Some(f) = stack.first() {
        // The outermost unclosed element is the one the author forgot.
        return Err(never_closed(f));
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::scan::scan;

    fn tree(src: &str) -> Result<Vec<Node<'_>>, CompileError> {
        build(scan(src).unwrap())
    }

    #[test]
    fn plain_html_is_one_text_node() {
        let src = "<div><p>hi</p></div>{% if x %}<a>{% endif %}";
        assert_eq!(tree(src).unwrap(), vec![Node::Text(0..src.len())]);
    }

    #[test]
    fn components_nest() {
        let src = "a<rx-card><rx-badge>x</rx-badge></rx-card>b";
        let nodes = tree(src).unwrap();
        assert_eq!(nodes.len(), 3);
        let Node::Element {
            name,
            children,
            close,
            ..
        } = &nodes[1]
        else {
            panic!("element")
        };
        assert_eq!(name, "rx-card");
        assert!(close.is_some());
        assert!(matches!(&children[0], Node::Element { name, .. } if name == "rx-badge"));
    }

    #[test]
    fn flow_attributes_make_an_element_and_count_same_name_tags() {
        let src = "<div rx-if=\"a\"><div>in</div></div><div>out</div>";
        let nodes = tree(src).unwrap();
        assert_eq!(nodes.len(), 2);
        let Node::Element { close, open, .. } = &nodes[0] else {
            panic!("element")
        };
        assert_eq!(*close, Some(28..34));
        assert_eq!(*open, 0..15);
    }

    #[test]
    fn void_and_self_closing_have_no_close() {
        let nodes = tree("<input rx-if=\"a\"><rx-icon name=\"x\" />").unwrap();
        assert_eq!(nodes.len(), 2);
        for n in nodes {
            assert!(matches!(n, Node::Element { close: None, .. }));
        }
    }

    #[test]
    fn unclosed_and_stray() {
        let e = tree("\n<rx-card>").unwrap_err();
        assert_eq!(e.line, 2);
        assert_eq!(e.message, "<rx-card> opened here is never closed");
        let e = tree("x\n</rx-card>").unwrap_err();
        assert_eq!(e.line, 2);
        assert_eq!(e.message, "</rx-card> closes nothing");
        // Stray plain end tags are left alone.
        assert!(tree("</div>").is_ok());
    }
}

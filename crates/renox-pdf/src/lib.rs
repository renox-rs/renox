//! PDF documents from Typst templates, with right-to-left text and embedded
//! fonts, for Renox apps. The Typst compiler runs inside the process, so
//! there is no headless browser and no separate service.
//!
//! This is the scaffold of the crate (issue #30): the dependencies are
//! chosen and reviewed, the API follows in the next steps.

#[cfg(test)]
mod tests {
    #[test]
    fn typst_is_linked() {
        let source = typst::syntax::Source::detached("= Invoice");
        assert_eq!(source.text(), "= Invoice");
        assert_eq!(source.root().kind(), typst::syntax::SyntaxKind::Markup);
    }
}

//! The code samples of "About this page", cut from the shop's own source
//! files when it is built, so they never drift from the code that runs.
//!
//! A sample is a *region*: lines between two marker comments in a file
//! under `src/`, `resources/views/`, `tests/`, `migrations/` or `public/`.
//!
//! ```text
//! // [explain:rentals.reserve]
//! async fn store(Valid(form): Valid<Booking>, …) -> Result<Response> {
//!     …
//! }
//! // [/explain:rentals.reserve]
//! ```
//!
//! In a template the markers are template comments, which render as
//! nothing: `{# [explain:rentals.form] #}` … `{# [/explain:rentals.form] #}`
//! (`{#- … #}` works too, but trims the whitespace before it). SQL uses
//! `-- [explain:…]`, CSS `/* [explain:…] */`. rustfmt and MiniJinja leave
//! these lines alone. Opening the same name again later in the file adds
//! another part, joined to the first with a `…` line, to skip what doesn't
//! matter.
//!
//! `build.rs` scans those folders and writes the table [`REGIONS`]; an
//! explanation names its samples with [`crate::explain::Code`]. Mistakes
//! (an unclosed marker, a name in two files, a sample nobody shows, a name
//! that no file has) fail `tests/about.rs`, never the build.

/// One region of a source file, as `build.rs` found it.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    /// The name in its markers: `rentals.reserve`.
    pub name: &'static str,
    /// The file, from the repository's root.
    pub path: &'static str,
    /// What it is written in: `rust`, `html`, `sql`, `css` or `js`.
    pub language: &'static str,
    /// The line of its first part's first line (from 1).
    pub first_line: usize,
    /// The line of its last part's last line.
    pub last_line: usize,
    /// The lines, without the markers and their common indentation.
    pub text: &'static str,
}

impl Region {
    /// The number of lines shown.
    pub fn lines(&self) -> usize {
        self.text.lines().count()
    }
}

/// Every region in the shop's files, by name.
pub static REGIONS: &[Region] = include!(concat!(env!("OUT_DIR"), "/regions.rs"));

/// What `build.rs` found wrong with the markers (`file:line: problem`).
pub static PROBLEMS: &[&str] = include!(concat!(env!("OUT_DIR"), "/region_problems.rs"));

/// The region named `name`.
///
/// ```
/// assert!(bikeshop::code::region("no such region").is_none());
/// ```
pub fn region(name: &str) -> Option<&'static Region> {
    REGIONS.iter().find(|r| r.name == name)
}

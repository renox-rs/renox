//! Rebuilds when the grid pages' migrations change, so
//! `renox::migrations!()` sees new files.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}

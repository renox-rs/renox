//! The views and public files are compiled in (`renox::embedded!()`):
//! rebuild when they change.

fn main() {
    for watched in ["resources", "public"] {
        println!("cargo:rerun-if-changed={watched}");
    }
}

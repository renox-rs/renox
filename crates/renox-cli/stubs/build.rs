fn main() {
    // Rebuild when migrations change, so `renox::migrations!()` sees new files.
    println!("cargo:rerun-if-changed=migrations");
    // And when views, translations or public files change, for `renox::embedded!()`.
    println!("cargo:rerun-if-changed=resources");
    println!("cargo:rerun-if-changed=public");
}

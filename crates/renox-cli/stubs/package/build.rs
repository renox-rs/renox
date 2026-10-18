fn main() {
    // Rebuild when migrations or views change, so `renox::migrations!()` and
    // `include_str!` see new files.
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=views");
}

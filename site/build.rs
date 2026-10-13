fn main() {
    // The pages are the repository's own Markdown, compiled in.
    for path in [
        "../docs",
        "../README.md",
        "../CHEATSHEET.md",
        "../CHANGELOG.md",
        "../CONTRIBUTING.md",
        "../SECURITY.md",
        "../RELEASING.md",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    // Views and public files, for `renox::embedded!()`.
    println!("cargo:rerun-if-changed=resources");
    println!("cargo:rerun-if-changed=public");
}

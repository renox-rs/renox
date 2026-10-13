//! Compiles the blog in: every `content/blog/*.md` becomes an entry of
//! `POST_FILES` (its file name and its text), so adding a post is adding a
//! file. Views, public files and the benchmark results are watched too.

use std::fmt::Write;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("content/blog");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "md"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out = String::from(
        "/// Every post's file name and Markdown (build.rs).\npub static POST_FILES: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path.file_name().unwrap().to_string_lossy();
        writeln!(
            out,
            "    ({name:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let target = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("posts.rs");
    std::fs::write(target, out).unwrap();
    for watched in ["resources", "public", "content/benchmarks.json"] {
        println!("cargo:rerun-if-changed={watched}");
    }
}

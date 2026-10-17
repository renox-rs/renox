//! `rnx make:pwa`: the offline page, placeholder icons and the `.pwa(..)`
//! call that make an app installable on phones.

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use heck::ToTitleCase;

use crate::generate;

/// The files it writes: where they go and their stubs (`{{title}}` in the
/// HTML is the app's name).
const FILES: &[(&str, &[u8])] = &[
    (
        "resources/views/offline.html",
        include_bytes!("../stubs/pwa/offline.html"),
    ),
    (
        "public/icons/icon-192.png",
        include_bytes!("../stubs/pwa/icon-192.png"),
    ),
    (
        "public/icons/icon-512.png",
        include_bytes!("../stubs/pwa/icon-512.png"),
    ),
    (
        "public/icons/icon-maskable-512.png",
        include_bytes!("../stubs/pwa/icon-maskable-512.png"),
    ),
];

/// `rnx make:pwa`. Refuses to overwrite existing files without `force`;
/// nothing is written when it refuses.
pub fn make_pwa(root: &Path, force: bool) -> Result<()> {
    let title = crate::deploy::package_name(root)?.to_title_case();
    let existing: Vec<&str> = FILES
        .iter()
        .map(|(path, _)| *path)
        .filter(|path| root.join(path).exists())
        .collect();
    if !force && !existing.is_empty() {
        bail!(
            "{} already exist(s); use --force to overwrite",
            existing.join(", ")
        );
    }
    for (path, contents) in FILES {
        let target = root.join(path);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        if path.ends_with(".html") {
            let text = String::from_utf8_lossy(contents).replace("{{title}}", &title);
            fs::write(&target, text)?;
        } else {
            fs::write(&target, contents)?;
        }
        println!("Created {path}");
    }
    let file = generate::app_file(root);
    if fs::read_to_string(&file).is_ok_and(|s| s.contains(".pwa(")) {
        println!(
            "Kept the existing .pwa(..) call in {}",
            generate::shown(&file)
        );
    } else {
        let call = format!(
            ".pwa(renox::prelude::Pwa::new({title:?}).icons(\"public/icons\").offline_page(\"offline.html\"))"
        );
        generate::register_call(&file, &call)?;
    }
    println!("Next: replace the placeholder icons in public/icons/ with your own.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(dir: &Path) {
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"coffee-shop\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "pub fn app() -> renox::App {\n    renox::App::new()\n        .module(app::home::Home)\n}\n",
        )
        .unwrap();
    }

    #[test]
    fn writes_files_and_registers_once() {
        let dir = tempfile::tempdir().unwrap();
        app(dir.path());
        make_pwa(dir.path(), false).unwrap();
        for icon in ["icon-192.png", "icon-512.png", "icon-maskable-512.png"] {
            let bytes = fs::read(dir.path().join("public/icons").join(icon)).unwrap();
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "{icon}");
        }
        let offline = fs::read_to_string(dir.path().join("resources/views/offline.html")).unwrap();
        assert!(
            offline.contains("Coffee Shop") && !offline.contains("{{"),
            "{offline}"
        );
        let lib = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(
            lib.contains("        .module(app::home::Home)\n        .pwa(renox::prelude::Pwa::new(\"Coffee Shop\").icons(\"public/icons\").offline_page(\"offline.html\"))\n"),
            "{lib}"
        );

        // A second run refuses; --force keeps the single call.
        let err = make_pwa(dir.path(), false).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        make_pwa(dir.path(), true).unwrap();
        let lib = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert_eq!(lib.matches(".pwa(").count(), 1, "{lib}");
    }

    #[test]
    fn an_existing_file_stops_everything() {
        let dir = tempfile::tempdir().unwrap();
        app(dir.path());
        fs::create_dir_all(dir.path().join("resources/views")).unwrap();
        fs::write(dir.path().join("resources/views/offline.html"), "mine").unwrap();
        assert!(make_pwa(dir.path(), false).is_err());
        assert!(!dir.path().join("public/icons").exists());
        assert!(
            !fs::read_to_string(dir.path().join("src/lib.rs"))
                .unwrap()
                .contains(".pwa(")
        );
    }
}

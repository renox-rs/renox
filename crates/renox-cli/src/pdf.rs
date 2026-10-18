//! `rnx make:pdf`: a Typst template for `renox-pdf`, the dependency and the
//! `Pdf` module that renders it.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use heck::{ToSnakeCase, ToTitleCase};

use crate::generate;

const STUB: &str = include_str!("../stubs/pdf.typ.stub");

/// `rnx make:pdf invoice`: `resources/views/pdf/invoice.typ`, plus (once) the
/// `renox-pdf` dependency and `.module(renox_pdf::Pdf::new()…)`.
pub fn make_pdf(root: &Path, name: &str) -> Result<()> {
    generate::check_name(name)?;
    let snake = name.to_snake_case();
    let text = STUB
        .replace("{{name}}", &snake)
        .replace("{{title}}", &name.to_title_case());
    generate::write_new(
        &root.join(format!("resources/views/pdf/{snake}.typ")),
        &text,
    )?;

    let manifest = root.join("Cargo.toml");
    let source = fs::read_to_string(&manifest).context("no Cargo.toml here")?;
    let updated = add_dependency(&source);
    if updated != source {
        fs::write(&manifest, updated)?;
        println!("Updated Cargo.toml (renox-pdf)");
    }

    let file = generate::app_file(root);
    if fs::read_to_string(&file).is_ok_and(|s| s.contains("renox_pdf::Pdf")) {
        println!(
            "Kept the existing renox_pdf::Pdf module in {}",
            generate::shown(&file)
        );
    } else {
        generate::register_call(
            &file,
            ".module(renox_pdf::Pdf::new().templates(renox::embedded!()))",
        )?;
    }
    println!(
        "Next: render it with Pdf::document(&state, \"{snake}\")?.data(&data).render(&state).await?"
    );
    Ok(())
}

/// Adds `renox-pdf = "<this version>"` to `[dependencies]` unless it is there.
fn add_dependency(manifest: &str) -> String {
    let line = format!("renox-pdf = \"{}\"", env!("CARGO_PKG_VERSION"));
    let mut lines: Vec<String> = manifest.lines().map(str::to_owned).collect();
    let mut section = String::new();
    let mut last = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.starts_with('[') {
            section = t.to_owned();
            if t == "[dependencies]" {
                last = Some(i);
            }
        } else if section == "[dependencies]" {
            if t.starts_with("renox-pdf ")
                || t.starts_with("renox-pdf=")
                || t.starts_with("renox-pdf.")
            {
                return manifest.to_owned();
            }
            if !t.is_empty() {
                last = Some(i);
            }
        }
    }
    match last {
        Some(i) => lines.insert(i + 1, line),
        None => {
            lines.push(String::new());
            lines.push("[dependencies]".to_owned());
            lines.push(line);
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(dir: &Path) {
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n\n[dependencies]\nrenox = \"1\"\n\n[profile.dev]\ndebug = 0\n",
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
    fn writes_the_template_and_wires_the_module_once() {
        let dir = tempfile::tempdir().unwrap();
        app(dir.path());
        make_pdf(dir.path(), "SalesReceipt").unwrap();
        let typ =
            fs::read_to_string(dir.path().join("resources/views/pdf/sales_receipt.typ")).unwrap();
        assert!(typ.contains("@renox/kit:0.1.0") && typ.contains("Sales Receipt"));
        assert!(!typ.contains("{{"), "{typ}");
        let manifest = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains(&format!(
                "renox = \"1\"\nrenox-pdf = \"{}\"\n",
                env!("CARGO_PKG_VERSION")
            )),
            "{manifest}"
        );
        let lib = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(
            lib.contains("        .module(app::home::Home)\n        .module(renox_pdf::Pdf::new().templates(renox::embedded!()))\n"),
            "{lib}"
        );

        // A second template adds nothing again; an existing one is refused.
        make_pdf(dir.path(), "invoice").unwrap();
        let lib = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert_eq!(lib.matches("renox_pdf::Pdf").count(), 1, "{lib}");
        let manifest = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert_eq!(manifest.matches("renox-pdf").count(), 1, "{manifest}");
        assert!(make_pdf(dir.path(), "invoice").is_err());
    }

    #[test]
    fn a_bad_name_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        app(dir.path());
        assert!(make_pdf(dir.path(), "../x").is_err());
        assert!(!dir.path().join("resources").exists());
    }
}

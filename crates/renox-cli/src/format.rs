//! The Rust files `rnx new` and `rnx make:*` write are put through rustfmt,
//! so an app passes `cargo fmt --check` right after a generator ran (#124):
//! a module's name decides where its `mod` line and imports sort, and no
//! template can know that in advance. Only the files a command wrote or
//! edited are formatted, never the rest of the app; rustfmt reads the app's
//! own `rustfmt.toml` if it has one. Without rustfmt, nothing happens.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

thread_local! {
    /// The `.rs` files this command wrote or edited.
    static TOUCHED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// Notes `path` for `format_touched`, if it's a Rust file.
pub fn touched(path: &Path) {
    if path.extension().is_some_and(|ext| ext == "rs") {
        TOUCHED.with(|files| {
            let mut files = files.borrow_mut();
            if !files.iter().any(|file| file == path) {
                files.push(path.to_path_buf());
            }
        });
    }
}

/// Runs rustfmt on the files noted so far (and forgets them). Returns how
/// many it formatted; a missing rustfmt or a file it can't parse is not an
/// error for the generator, which already did its job.
pub fn format_touched() -> usize {
    let files: Vec<PathBuf> = TOUCHED.with(|files| std::mem::take(&mut *files.borrow_mut()));
    files.iter().filter(|file| format_one(file)).count()
}

/// One file, through stdin: given a path, rustfmt would follow its `mod`
/// lines and format (or fail on) the rest of the crate. Run from the file's
/// folder, so it finds the app's `rustfmt.toml`.
fn format_one(file: &Path) -> bool {
    format_one_with("rustfmt", file)
}

/// `format_one` with another formatter program (tests use one that fails).
fn format_one_with(program: &str, file: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(file) else {
        return false;
    };
    let Ok(mut child) = Command::new(program)
        .args(["--edition", "2024", "--emit", "stdout"])
        .current_dir(file.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(source.as_bytes()).is_err()
    {
        return false;
    }
    match child.wait_with_output() {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => {
            output.stdout == source.as_bytes() || std::fs::write(file, output.stdout).is_ok()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rustfmt_installed() -> bool {
        Command::new("rustfmt").arg("--version").output().is_ok()
    }

    #[test]
    fn formats_only_the_files_it_was_given() {
        if !rustfmt_installed() {
            eprintln!("rustfmt isn't installed; skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let written = dir.path().join("written.rs");
        let untouched = dir.path().join("untouched.rs");
        let messy = "mod zeta;\nmod alpha;\nfn main(){let x=1;}\n";
        std::fs::write(&written, messy).unwrap();
        std::fs::write(&untouched, messy).unwrap();
        touched(&written);
        touched(&written);
        touched(&dir.path().join("schema.sql"));
        assert_eq!(format_touched(), 1);
        assert_eq!(
            std::fs::read_to_string(&written).unwrap(),
            "mod alpha;\nmod zeta;\nfn main() {\n    let x = 1;\n}\n"
        );
        // The app's other files are its own business.
        assert_eq!(std::fs::read_to_string(&untouched).unwrap(), messy);
        // Forgotten once formatted.
        assert_eq!(format_touched(), 0);
    }

    /// A file it can't read, a formatter that isn't installed or that fails,
    /// and code rustfmt refuses: the file is left as it was.
    #[test]
    fn files_are_left_alone_when_formatting_fails() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("broken.rs");
        std::fs::write(&file, "fn main( {").unwrap();
        assert!(!format_one(&dir.path().join("missing.rs")));
        assert!(!format_one_with("renox-no-such-formatter", &file));
        assert!(!format_one_with("false", &file));
        if rustfmt_installed() {
            assert!(!format_one(&file), "not Rust");
        }
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "fn main( {");
    }
}

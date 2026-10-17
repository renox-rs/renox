//! Where a template's files come from: a directory on disk or an in-memory
//! map (what `renox::embedded!()` provides in release builds).
//!
//! Reads are confined to the template root. A virtual path is normalised by
//! Typst (`..` that would climb above the root is an error, a leading `/`
//! means the root, not the file system), and a file on disk is only read when
//! its canonical path is still inside the canonical root, so a symlink cannot
//! lead out either.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst::syntax::VirtualPath;

/// The files a template can read.
#[derive(Clone, Debug)]
pub(crate) enum Sources {
    /// A template directory; read on every request (debug builds reload).
    Dir(PathBuf),
    /// Files held in memory, keyed by their path below the template root.
    Map(HashMap<String, Bytes>),
}

impl Sources {
    /// Sources read from the directory `root`.
    pub(crate) fn dir(root: impl Into<PathBuf>) -> Self {
        Sources::Dir(root.into())
    }

    /// Sources held in memory. A key like `"./invoice.typ"` or `"/a/../b.typ"`
    /// is normalised; a key that escapes the root is dropped.
    pub(crate) fn map<K, V>(files: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: AsRef<str>,
        V: AsRef<[u8]> + Send + Sync + 'static,
    {
        let files = files
            .into_iter()
            .filter_map(|(name, data)| {
                let path = VirtualPath::new(name.as_ref()).ok()?;
                Some((path.get_with_slash().to_owned(), Bytes::new(data)))
            })
            .collect();
        Sources::Map(files)
    }

    /// The bytes of the file at `path`, below the template root.
    pub(crate) fn read(&self, path: &VirtualPath) -> FileResult<Bytes> {
        match self {
            Sources::Map(files) => files
                .get(path.get_with_slash())
                .cloned()
                .ok_or_else(|| FileError::NotFound(path.get_without_slash().into())),
            Sources::Dir(root) => read_confined(root, path),
        }
    }
}

/// Reads `path` below `root`, refusing anything that resolves outside it.
fn read_confined(root: &Path, path: &VirtualPath) -> FileResult<Bytes> {
    let real = path.realize(root).map_err(FileError::Realize)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|e| FileError::from_io(e, root))?;
    let resolved = real
        .canonicalize()
        .map_err(|e| FileError::from_io(e, &real))?;
    if !resolved.starts_with(&canonical_root) {
        return Err(FileError::AccessDenied);
    }
    if resolved.is_dir() {
        return Err(FileError::IsDirectory);
    }
    let data = std::fs::read(&resolved).map_err(|e| FileError::from_io(e, &resolved))?;
    Ok(Bytes::new(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renox-pdf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vp(path: &str) -> VirtualPath {
        VirtualPath::new(path).unwrap()
    }

    #[test]
    fn map_reads_normalised_keys() {
        let sources = Sources::map([("./a/../main.typ", b"= Hi".to_vec())]);
        assert_eq!(sources.read(&vp("/main.typ")).unwrap().as_slice(), b"= Hi");
        assert!(matches!(
            sources.read(&vp("other.typ")),
            Err(FileError::NotFound(_))
        ));
    }

    #[test]
    fn map_drops_keys_that_escape() {
        let sources = Sources::map([("../secret.typ", b"x".to_vec())]);
        assert!(matches!(&sources, Sources::Map(m) if m.is_empty()));
    }

    #[test]
    fn dir_reads_inside_the_root() {
        let dir = temp_dir("inside");
        std::fs::write(dir.join("main.typ"), "= Hi").unwrap();
        let sources = Sources::dir(&dir);
        assert_eq!(sources.read(&vp("main.typ")).unwrap().as_slice(), b"= Hi");
        assert!(matches!(
            sources.read(&vp("missing.typ")),
            Err(FileError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dotdot_cannot_leave_the_root() {
        let dir = temp_dir("dotdot");
        std::fs::write(dir.join("secret.txt"), "secret").unwrap();
        let root = dir.join("templates");
        std::fs::create_dir_all(&root).unwrap();
        // Typst refuses to build the path at all.
        assert!(VirtualPath::new("../secret.txt").is_err());
        assert!(VirtualPath::new("a/../../secret.txt").is_err());
        assert!(Sources::dir(&root).read(&vp("secret.txt")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn absolute_paths_mean_the_template_root() {
        let dir = temp_dir("absolute");
        let root = dir.join("templates");
        std::fs::create_dir_all(&root).unwrap();
        // `/etc/passwd` is `<root>/etc/passwd`, which does not exist.
        let sources = Sources::dir(&root);
        assert!(matches!(
            sources.read(&vp("/etc/passwd")),
            Err(FileError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_leave_the_root() {
        let dir = temp_dir("symlink");
        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        let root = dir.join("templates");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link.typ")).unwrap();
        std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
        let sources = Sources::dir(&root);
        assert!(matches!(
            sources.read(&vp("link.typ")),
            Err(FileError::AccessDenied)
        ));
        assert!(matches!(
            sources.read(&vp("up/outside.txt")),
            Err(FileError::AccessDenied)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_directory_is_not_a_file() {
        let dir = temp_dir("isdir");
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        let sources = Sources::dir(&dir);
        assert!(matches!(
            sources.read(&vp("parts")),
            Err(FileError::IsDirectory)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }
}

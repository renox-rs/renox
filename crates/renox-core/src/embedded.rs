//! Views, translations and public files compiled into the binary with
//! `renox::embedded!()`, so a release build is a single file to deploy.

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use crate::Error;

/// Files from `resources/views`, `resources/lang` and `public`, as
/// `(relative path, contents)`. Built by `renox::embedded!()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Embedded {
    /// Templates from `resources/views`.
    pub views: &'static [(&'static str, &'static str)],
    /// Translation files from `resources/lang`.
    pub lang: &'static [(&'static str, &'static str)],
    /// Files from `public`, served at the site root.
    pub public: &'static [(&'static str, &'static [u8])],
}

/// A content type for a public file, from its extension.
pub(crate) fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("pdf") => "application/pdf",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

pub(crate) type PublicFiles = Arc<HashMap<&'static str, &'static [u8]>>;

pub(crate) fn public_map(files: &'static [(&'static str, &'static [u8])]) -> PublicFiles {
    Arc::new(files.iter().copied().collect())
}

/// Serves an embedded public file for the request path, or 404.
pub(crate) fn serve(files: &PublicFiles, uri: &Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match files.get(path) {
        Some(bytes) => (
            StatusCode::OK,
            [
                (CONTENT_TYPE, content_type(path)),
                (CACHE_CONTROL, cache_control(uri)),
            ],
            *bytes,
        )
            .into_response(),
        None => Error::NotFound.into_response(),
    }
}

/// Content versions for `asset()` URLs (`/app.css?v=1a2b3c4d`): a changed
/// file gets a new URL, so browsers may keep each version for a year.
pub(crate) enum AssetVersions {
    /// Compiled in: hashed once at boot.
    Embedded(HashMap<&'static str, String>),
    /// Read from `PUBLIC_PATH`: hashed when first asked for and again after
    /// the file changes (its modified time).
    Disk {
        root: std::path::PathBuf,
        seen: std::sync::Mutex<HashMap<String, (std::time::SystemTime, String)>>,
    },
}

fn short_hash(bytes: &[u8]) -> String {
    crate::webhook::sha256_hex(bytes)[..8].to_owned()
}

impl AssetVersions {
    pub(crate) fn new(
        public: &std::path::Path,
        embedded: Option<&'static [(&'static str, &'static [u8])]>,
    ) -> Self {
        match embedded {
            Some(files) => Self::Embedded(
                files
                    .iter()
                    .map(|(path, bytes)| (*path, short_hash(bytes)))
                    .collect(),
            ),
            None => Self::Disk {
                root: public.to_path_buf(),
                seen: std::sync::Mutex::default(),
            },
        }
    }

    /// The version of the public file at `path`, if there is one.
    pub(crate) fn version(&self, path: &str) -> Option<String> {
        let path = path.trim_start_matches('/');
        match self {
            Self::Embedded(files) => files.get(path).cloned(),
            Self::Disk { root, seen } => {
                let file = root.join(path);
                if !file.starts_with(root) || path.split('/').any(|part| part == "..") {
                    return None;
                }
                let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok()?;
                let mut seen = seen.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((at, hash)) = seen.get(path)
                    && *at == modified
                {
                    return Some(hash.clone());
                }
                let hash = short_hash(&std::fs::read(&file).ok()?);
                seen.insert(path.to_owned(), (modified, hash.clone()));
                Some(hash)
            }
        }
    }
}

/// Whether the request asks for a versioned file (`?v=…`), which can be
/// cached for good.
pub(crate) fn is_versioned(uri: &Uri) -> bool {
    uri.query()
        .is_some_and(|query| query.split('&').any(|pair| pair.starts_with("v=")))
}

/// `Cache-Control` for a public file: a year for versioned URLs.
pub(crate) fn cache_control(uri: &Uri) -> &'static str {
    if is_versioned(uri) {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_files_get_their_content_type() {
        for (path, expected) in [
            ("logo.PNG", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
            ("favicon.ico", "image/x-icon"),
            ("f.woff", "font/woff"),
            ("f.woff2", "font/woff2"),
            ("terms.pdf", "application/pdf"),
            ("sitemap.xml", "application/xml"),
            ("archive.zip", "application/octet-stream"),
            ("README", "application/octet-stream"),
        ] {
            assert_eq!(content_type(path), expected, "{path}");
        }
    }

    /// Versions of files on disk: read once and kept until the file
    /// changes; a path out of the public folder has none.
    #[test]
    fn disk_versions_are_cached_and_stay_in_the_public_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.css"), "a{}").unwrap();
        let versions = AssetVersions::new(dir.path(), None);
        let first = versions.version("/app.css").expect("a version");
        assert_eq!(versions.version("app.css"), Some(first));
        assert_eq!(versions.version("../secret.txt"), None);
        assert_eq!(versions.version("missing.css"), None);
    }
}

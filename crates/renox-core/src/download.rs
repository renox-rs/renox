//! File downloads: `Download`.

use std::path::Path;

use axum::body::{Body, Bytes};
use axum::http::header::{
    CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::Stream;

use crate::storage::Storage;
use crate::{Error, Result};

/// Files sent as downloads: bytes made in the handler (a PDF, a CSV),
/// a file on disk (streamed), a key in `Storage`, or any stream.
///
/// ```
/// # use renox::prelude::*;
/// use renox::Download;
///
/// async fn invoice(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Download> {
///     let pdf: Vec<u8> = b"%PDF-1.7 ...".to_vec(); // made by your PDF library
///     Ok(Download::bytes(format!("invoice-{id}.pdf"), "application/pdf", pdf).inline())
/// }
///
/// async fn export(State(state): State<AppState>) -> Result<Download> {
///     Download::from_storage(&state.storage, "exports/sales.csv", "sales.csv").await
/// }
///
/// async fn backup() -> Result<Download> {
///     Download::file("storage/backup.db", "backup.db").await // streamed, not read into memory
/// }
/// ```
pub struct Download {
    filename: String,
    content_type: String,
    body: Body,
    length: Option<u64>,
    inline: bool,
}

impl Download {
    /// `data` as a file named `filename`.
    pub fn bytes(
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: impl Into<Bytes>,
    ) -> Self {
        let data = data.into();
        Self {
            filename: filename.into(),
            content_type: content_type.into(),
            length: Some(data.len() as u64),
            body: Body::from(data),
            inline: false,
        }
    }

    /// A file on disk, streamed; its type comes from its extension. A
    /// missing file is a 404.
    pub async fn file(path: impl AsRef<Path>, filename: impl Into<String>) -> Result<Self> {
        let path = path.as_ref();
        let file = match tokio::fs::File::open(path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(Error::NotFound),
            Err(err) => {
                return Err(anyhow::Error::from(err)
                    .context(format!("could not open {}", path.display()))
                    .into());
            }
        };
        let length = file.metadata().await.ok().map(|m| m.len());
        let filename = filename.into();
        Ok(Self {
            content_type: guess(&filename, path),
            filename,
            length,
            body: Body::from_stream(tokio_util::io::ReaderStream::new(file)),
            inline: false,
        })
    }

    /// A file in `Storage` (local disk or S3). A missing key is a 404.
    pub async fn from_storage(
        storage: &Storage,
        key: &str,
        filename: impl Into<String>,
    ) -> Result<Self> {
        let bytes = storage.get(key).await?.ok_or(Error::NotFound)?;
        let filename = filename.into();
        let content_type = guess(&filename, Path::new(key));
        Ok(Self::bytes(filename, content_type, bytes))
    }

    /// Any stream of bytes, e.g. a CSV written row by row as the database
    /// hands them over.
    pub fn stream<S, E>(
        filename: impl Into<String>,
        content_type: impl Into<String>,
        stream: S,
    ) -> Self
    where
        S: Stream<Item = std::result::Result<Bytes, E>> + Send + 'static,
        E: Into<axum::BoxError>,
    {
        Self {
            filename: filename.into(),
            content_type: content_type.into(),
            length: None,
            body: Body::from_stream(stream),
            inline: false,
        }
    }

    /// Shown in the browser (a PDF, an image) instead of saved. HTML, XML and
    /// JavaScript are always downloads, so a file can't run as a page of
    /// the app.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }
}

/// The type for `filename`'s extension, else `path`'s, else binary.
fn guess(filename: &str, path: &Path) -> String {
    mime_guess::from_path(filename)
        .first()
        .or_else(|| mime_guess::from_path(path).first())
        .map_or_else(|| "application/octet-stream".to_owned(), |m| m.to_string())
}

/// `attachment; filename="plain.pdf"; filename*=UTF-8''%C3%BCnic%C3%B6de.pdf`
fn disposition(filename: &str, inline: bool) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    crate::routing::encode(&mut encoded, filename, false);
    let kind = if inline { "inline" } else { "attachment" };
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

impl IntoResponse for Download {
    fn into_response(self) -> Response {
        let active = {
            let t = self.content_type.to_ascii_lowercase();
            t.contains("html") || t.contains("xml") || t.contains("javascript")
        };
        let mut res = (StatusCode::OK, self.body).into_response();
        let headers = res.headers_mut();
        if let Ok(value) = HeaderValue::from_str(&self.content_type) {
            headers.insert(CONTENT_TYPE, value);
        }
        if let Ok(value) =
            HeaderValue::from_str(&disposition(&self.filename, self.inline && !active))
        {
            headers.insert(CONTENT_DISPOSITION, value);
        }
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        if let Some(length) = self.length {
            headers.insert(CONTENT_LENGTH, length.into());
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_safe_in_the_header() {
        assert_eq!(
            disposition("faktur \"Mei\".pdf", false),
            "attachment; filename=\"faktur _Mei_.pdf\"; filename*=UTF-8''faktur%20%22Mei%22.pdf"
        );
        assert!(
            disposition("laporan\r\nX: y.pdf", true)
                .starts_with("inline; filename=\"laporan__X: y.pdf\"")
        );
        assert!(disposition("ünï.pdf", false).ends_with("filename*=UTF-8''%C3%BCn%C3%AF.pdf"));
    }

    /// A missing file is a 404; a path that can't be opened for another
    /// reason (here a file used as a folder) is an error naming the path.
    #[tokio::test]
    async fn files_that_cant_be_opened() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Download::file(dir.path().join("gone.pdf"), "gone.pdf").await,
            Err(Error::NotFound)
        ));
        // (Windows reports this one as "not found".)
        #[cfg(unix)]
        {
            let file = dir.path().join("report.pdf");
            std::fs::write(&file, b"%PDF-").unwrap();
            let Err(err) = Download::file(file.join("inside.pdf"), "x.pdf").await else {
                panic!("opened a file inside a file");
            };
            assert!(
                format!("{err:?}").contains("could not open")
                    && format!("{err:?}").contains("report.pdf"),
                "{err:?}"
            );
        }
    }
}

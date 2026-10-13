//! Uploaded files. A form field of type `Upload` (or `Option<Upload>`)
//! receives the file from a `multipart/form-data` post through `Valid<T>`:
//!
//! ```
//! # use renox::prelude::*;
//! # use serde::Deserialize;
//! #[derive(Deserialize)]
//! struct ProductForm { name: String, photo: Option<Upload> }
//!
//! impl Validate for ProductForm {
//!     fn rules(&self, v: &mut Validator) {
//!         v.field("name", &self.name).required();
//!         v.field("photo", &self.photo).image().max(2048);     // KB
//!     }
//! }
//!
//! async fn store(State(state): State<AppState>, back: Back, Valid(form): Valid<ProductForm>) -> Result<Back> {
//!     if let Some(photo) = &form.photo {
//!         let key = photo.store_public(&state.storage, "products").await?;   // public/products/…jpg
//!         let url = state.storage.url(&key);
//! #       let _ = url;
//!     }
//!     Ok(back)
//! }
//! ```
//!
//! In the form: `<form method="post" enctype="multipart/form-data">`. The
//! request size limit is `UPLOAD_MAX_SIZE` (megabytes, default 10).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;

use axum::body::Bytes;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Result;
use crate::crypto::random_token;
use crate::storage::Storage;

/// A file posted in a multipart form.
#[derive(Clone)]
#[non_exhaustive]
pub struct Upload {
    file_name: String,
    content_type: String,
    bytes: Bytes,
}

impl fmt::Debug for Upload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Upload")
            .field("file_name", &self.file_name)
            .field("content_type", &self.content_type)
            .field("size", &self.bytes.len())
            .finish()
    }
}

/// Extensions browsers treat as documents or scripts.
const ACTIVE_EXTENSIONS: &[&str] = &[
    "html", "htm", "xhtml", "xht", "shtml", "mht", "mhtml", "xml", "xsl", "xslt", "js", "mjs",
    "cjs", "php", "phtml", "asp", "aspx", "jsp", "cgi", "pl", "py", "sh", "swf", "hta", "htc",
];

impl Upload {
    /// A file as a browser would send it, e.g. in tests:
    /// `Upload::new("photo.png", "image/png", bytes)`.
    pub fn new(
        file_name: impl Into<String>,
        content_type: impl Into<String>,
        bytes: impl Into<Bytes>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            content_type: content_type.into(),
            bytes: bytes.into(),
        }
    }

    /// The name the browser gave, e.g. `coffee photo.JPG`. Don't trust it for paths.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// The type the browser declared, e.g. `image/jpeg`. Don't trust it for security.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// The file's content.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    /// Size of the content in bytes.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// The lowercase extension of the original name, if it is a plain one.
    pub fn extension(&self) -> Option<String> {
        let (_, ext) = self.file_name.rsplit_once('.')?;
        let ext = ext.to_ascii_lowercase();
        (!ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
            .then_some(ext)
    }

    /// The type found in the file's first bytes, for the formats Renox knows.
    pub fn sniffed_type(&self) -> Option<&'static str> {
        let b = &self.bytes[..];
        if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some("image/png")
        } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some("image/jpeg")
        } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
            Some("image/gif")
        } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
            Some("image/webp")
        } else if b.starts_with(b"%PDF-") {
            Some("application/pdf")
        } else {
            None
        }
    }

    /// Whether the content really is a PNG, JPEG, GIF or WebP image.
    pub fn is_image(&self) -> bool {
        self.sniffed_type().is_some_and(|t| t.starts_with("image/"))
    }

    /// The image's width and height in pixels, read from its header (PNG,
    /// JPEG, GIF or WebP); `None` for anything else or a damaged header.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        image_dimensions(&self.bytes)
    }

    /// An extension to store the file under: from the content when it can be
    /// sniffed, otherwise from the original name, otherwise `bin`. Names a
    /// browser would run as a page or script (`.html`, `.js`, …) are stored
    /// as `.txt`, so a public upload can't become active content on the
    /// app's origin.
    fn safe_extension(&self) -> String {
        match self.sniffed_type() {
            Some("image/png") => "png".into(),
            Some("image/jpeg") => "jpg".into(),
            Some("image/gif") => "gif".into(),
            Some("image/webp") => "webp".into(),
            Some("application/pdf") => "pdf".into(),
            _ => match self.extension() {
                Some(ext) if ACTIVE_EXTENSIONS.contains(&ext.as_str()) => "txt".into(),
                Some(ext) => ext,
                None => "bin".into(),
            },
        }
    }

    fn key(&self, dir: &str) -> String {
        let dir = dir.trim_matches('/');
        let name = format!("{}.{}", &random_token()[..24], self.safe_extension());
        if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        }
    }

    /// Stores the file privately under `dir` with a random name; returns its key.
    pub async fn store(&self, storage: &Storage, dir: &str) -> Result<String> {
        let key = self.key(dir);
        storage.put(&key, self.bytes.clone()).await?;
        Ok(key)
    }

    /// Stores the file under `public/{dir}` so `storage.url(key)` can link to it.
    pub async fn store_public(&self, storage: &Storage, dir: &str) -> Result<String> {
        let key = self.key(&format!("public/{}", dir.trim_matches('/')));
        storage.put(&key, self.bytes.clone()).await?;
        Ok(key)
    }
}

/// Old input shows the file name; the bytes never go into the session.
impl Serialize for Upload {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.file_name)
    }
}

thread_local! {
    /// Files of the multipart form being deserialized on this thread.
    static UPLOADS: RefCell<HashMap<String, Upload>> = RefCell::new(HashMap::new());
}

impl<'de> Deserialize<'de> for Upload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let token = String::deserialize(deserializer)?;
        UPLOADS
            .with(|uploads| uploads.borrow().get(&token).cloned())
            .ok_or_else(|| D::Error::custom(NOT_A_FILE))
    }
}

pub(crate) const NOT_A_FILE: &str = "expected an uploaded file";

/// Placeholder put in the form data where a file was.
pub(crate) fn token(index: usize) -> String {
    format!("\u{1}renox-upload:{index}")
}

/// Runs `f` (a synchronous deserialization) with `uploads` resolvable by token.
pub(crate) fn with_uploads<R>(uploads: &HashMap<String, Upload>, f: impl FnOnce() -> R) -> R {
    UPLOADS.with(|cell| *cell.borrow_mut() = uploads.clone());
    let result = f();
    UPLOADS.with(|cell| cell.borrow_mut().clear());
    result
}

/// Width and height from a PNG, GIF, JPEG or WebP header.
fn image_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    let be16 = |i: usize| Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32);
    let le16 = |i: usize| Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32);
    let le24 = |i: usize| {
        let s = b.get(i..i + 3)?;
        Some(s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16)
    };
    let size = if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        if b.get(12..16)? != b"IHDR" {
            return None;
        }
        let width = u32::from_be_bytes(b.get(16..20)?.try_into().ok()?);
        let height = u32::from_be_bytes(b.get(20..24)?.try_into().ok()?);
        (width, height)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        (le16(6)?, le16(8)?)
    } else if b.starts_with(&[0xFF, 0xD8]) {
        // Walk the segments to the first frame header (SOF0–SOF15, not the
        // DHT, JPG and DAC markers that share the range).
        let mut i = 2;
        loop {
            while *b.get(i)? != 0xFF {
                i += 1;
            }
            while *b.get(i)? == 0xFF {
                i += 1;
            }
            let marker = *b.get(i)?;
            i += 1;
            if matches!(marker, 0xD0..=0xD9 | 0x01) {
                continue;
            }
            let length = be16(i)? as usize;
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                break (be16(i + 5)?, be16(i + 3)?);
            }
            i += length;
        }
    } else if b.len() >= 30 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        match &b[12..16] {
            b"VP8 " if b.get(23..26)? == [0x9D, 0x01, 0x2A] => {
                (le16(26)? & 0x3FFF, le16(28)? & 0x3FFF)
            }
            b"VP8L" if b[20] == 0x2F => {
                let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
            }
            b"VP8X" => (le24(24)? + 1, le24(27)? + 1),
            _ => return None,
        }
    } else {
        return None;
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_image_dimensions_from_headers() {
        // PNG: 3 × 2.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&3u32.to_be_bytes());
        png.extend_from_slice(&2u32.to_be_bytes());
        assert_eq!(image_dimensions(&png), Some((3, 2)));
        // GIF: 640 × 480.
        let gif = b"GIF89a\x80\x02\xe0\x01";
        assert_eq!(image_dimensions(gif), Some((640, 480)));
        // JPEG: an APP0 segment, then SOF0 with height 100 and width 200.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00,
            100, 0x00, 200,
        ];
        assert_eq!(image_dimensions(&jpeg), Some((200, 100)));
        // WebP (VP8X): 1920 × 1080.
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        webp.extend_from_slice(&[0x7F, 0x07, 0x00, 0x37, 0x04, 0x00]);
        assert_eq!(image_dimensions(&webp), Some((1920, 1080)));
        assert_eq!(image_dimensions(b"%PDF-1.7"), None);
        assert_eq!(image_dimensions(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(image_dimensions(&[0xFF, 0xD8, 0xFF]), None);
    }

    fn upload(name: &str, bytes: &'static [u8]) -> Upload {
        Upload::new(name, "application/octet-stream", Bytes::from_static(bytes))
    }

    #[test]
    fn sniffs_content_instead_of_trusting_names() {
        let png = upload("photo.txt", b"\x89PNG\r\n\x1a\nrest");
        assert!(png.is_image());
        assert_eq!(png.safe_extension(), "png");
        let fake = upload("virus.jpg", b"MZ\x90\x00");
        assert!(!fake.is_image());
        assert_eq!(fake.safe_extension(), "jpg");
        assert_eq!(upload("../../etc/passwd", b"x").extension(), None);
        assert_eq!(upload("no-extension", b"x").safe_extension(), "bin");
    }

    #[test]
    fn keys_are_random_and_contained() {
        let a = upload("a.PNG", b"x").key("/products/");
        assert!(a.starts_with("products/") && a.ends_with(".png"), "{a}");
        assert_ne!(a, upload("a.png", b"x").key("products"));
    }

    // #252: the accessors, JPEG and WebP headers the other tests don't use,
    // and what's kept of a file in old input.

    fn webp(chunk: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WEBP".to_vec();
        b.extend_from_slice(chunk);
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(body);
        b.resize(40, 0);
        b
    }

    #[test]
    fn webp_lossy_and_lossless_headers_and_unknown_chunks() {
        // VP8 (lossy): a key frame start code, then 14-bit width and height.
        let mut lossy = vec![0u8; 3];
        lossy.extend_from_slice(&[0x9D, 0x01, 0x2A]);
        lossy.extend_from_slice(&640u16.to_le_bytes());
        lossy.extend_from_slice(&480u16.to_le_bytes());
        let file = Upload::new("a.webp", "image/webp", webp(b"VP8 ", &lossy));
        assert_eq!(file.sniffed_type(), Some("image/webp"));
        assert_eq!(file.dimensions(), Some((640, 480)));
        // VP8L (lossless): 0x2F, then (width - 1) and (height - 1) in 14 bits each.
        let bits: u32 = (99) | (49 << 14);
        let mut lossless = vec![0x2F];
        lossless.extend_from_slice(&bits.to_le_bytes());
        let file = Upload::new("b.webp", "image/webp", webp(b"VP8L", &lossless));
        assert_eq!(file.dimensions(), Some((100, 50)));
        // A chunk Renox doesn't read: no size, but still a WebP.
        let file = Upload::new("c.webp", "image/webp", webp(b"ALPH", &[]));
        assert!(file.is_image());
        assert_eq!(file.dimensions(), None);
    }

    #[test]
    fn jpeg_headers_skip_fill_bytes_and_standalone_markers() {
        // SOI, a fill byte, an RST marker (no length), an APP0 segment, then
        // SOF0 with height 20 and width 30.
        let mut jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xFF, 0xD0, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00,
        ];
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x14, 0x00, 0x1E]);
        let file = Upload::new("photo.JPG", "image/jpeg", jpeg);
        assert_eq!(file.sniffed_type(), Some("image/jpeg"));
        assert_eq!(file.dimensions(), Some((30, 20)));
        assert!(
            file.key("photos").ends_with(".jpg"),
            "{}",
            file.key("photos")
        );
        // Cut off before any frame header: no size.
        let cut = Upload::new("cut.jpg", "image/jpeg", vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00]);
        assert_eq!(cut.dimensions(), None);
        // A PNG whose first chunk isn't IHDR: no size.
        let odd = Upload::new(
            "odd.png",
            "image/png",
            b"\x89PNG\r\n\x1a\n\0\0\0\x0dJUNK".to_vec(),
        );
        assert_eq!(odd.dimensions(), None);
    }

    #[test]
    fn accessors_debug_and_old_input() {
        let file = Upload::new("notes", "text/plain", b"hello".to_vec());
        assert_eq!(file.content_type(), "text/plain");
        assert_eq!(file.size(), 5);
        assert_eq!(file.extension(), None);
        // No extension and nothing sniffed: stored as .bin, with no folder.
        assert!(file.key("").ends_with(".bin") && !file.key("").contains('/'));
        // Debug shows the size, never the bytes.
        let shown = format!("{file:?}");
        assert!(
            shown.contains("size: 5") && !shown.contains("hello"),
            "{shown}"
        );
        // Old input keeps only the name.
        assert_eq!(serde_json::to_value(&file).unwrap(), "notes");
    }

    #[tokio::test]
    async fn store_keeps_the_file_private() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::Config {
            storage_path: dir.path().to_path_buf(),
            ..Default::default()
        };
        let storage = crate::storage::Storage::from_config(&config).unwrap();
        let file = Upload::new("a.txt", "text/plain", b"hi".to_vec());
        let key = file.store(&storage, "/notes/").await.unwrap();
        assert!(key.starts_with("notes/") && key.ends_with(".txt"), "{key}");
        assert_eq!(&storage.get(&key).await.unwrap().unwrap()[..], b"hi");
    }

    /// Stored names take the extension the content shows, whatever the
    /// file was called.
    #[test]
    fn sniffed_files_are_stored_under_their_real_extension() {
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.extend_from_slice(&[0; 20]);
        for (name, bytes, ext) in [
            ("a.png", b"GIF89a\x01\x00\x01\x00".to_vec(), "gif"),
            ("b.png", webp, "webp"),
            ("c.txt", b"%PDF-1.7\n".to_vec(), "pdf"),
        ] {
            let file = Upload::new(name, "application/octet-stream", bytes);
            let key = file.key("files");
            assert!(key.ends_with(&format!(".{ext}")), "{name}: {key}");
        }
        // A JPEG with stray bytes before a marker: they are skipped.
        let mut jpeg = vec![0xFF, 0xD8, 0x00, 0x13, 0xFF, 0xE0, 0x00, 0x02];
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x0A, 0x00, 0x0B]);
        let file = Upload::new("photo.jpg", "image/jpeg", jpeg);
        assert_eq!(file.dimensions(), Some((11, 10)));
    }
}

//! Image processing for uploads (feature `images`): the [`Images`] step
//! resizes JPEG, PNG and WebP images, applies their EXIF orientation, drops
//! their metadata and makes thumbnails. Pure Rust (the `image` crate); other
//! formats (GIF, HEIC, AVIF) pass through unchanged.

use std::io::Cursor;

use axum::body::Bytes;
use futures_util::future::BoxFuture;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use super::Upload;
use super::pipeline::{StepOutcome, UploadContext, UploadStep};
use crate::Result;

/// Pixels per side a decoder is allowed (a guard against decompression bombs).
const MAX_SIDE: u32 = 16_384;
/// Pixels in all (100 megapixels).
const MAX_PIXELS: u64 = 100_000_000;
/// JPEG quality of re-encoded images.
const JPEG_QUALITY: u8 = 85;

/// An [`UploadStep`] that processes JPEG, PNG and WebP images.
///
/// ```
/// # use renox::prelude::*;
/// use renox::upload::Pipeline;
/// use renox::upload::images::Images;
///
/// # let _ =
/// App::new().uploads(Pipeline::new().step(
///     Images::new()
///         .max_dimensions(2560, 2560)
///         .strip_metadata()
///         .thumbnail("thumb", 320, 320),
/// ))
/// # ;
/// ```
///
/// - `max_dimensions(w, h)` shrinks larger images to fit (keeping the
///   aspect ratio); smaller ones are never enlarged.
/// - The EXIF orientation is applied to the pixels whenever the image is
///   re-encoded, so a phone photo shown by a browser or a program that ignores
///   EXIF stands upright. Re-encoding drops all metadata (EXIF with its GPS
///   position, XMP, IPTC, comments): `strip_metadata()` re-encodes images
///   that would otherwise be left as they came.
/// - `thumbnail(name, w, h)` stores a smaller copy next to the original, in
///   the same format: `Upload::store` writes it at
///   [`Upload::derived_key`]`(key, name)`, e.g. `products/abc.thumb.jpg`.
/// - A file that claims to be an image but can't be decoded, or is absurdly
///   large (over 16,384 pixels a side or 100 megapixels), is refused.
///   Re-encoded JPEGs use quality 85, WebP is written lossless.
///
/// Files that aren't JPEG, PNG or WebP (by their content) are left alone;
/// add the [`image()`](crate::Validator) rule to refuse them.
#[derive(Clone, Debug, Default)]
pub struct Images {
    max: Option<(u32, u32)>,
    strip: bool,
    thumbnails: Vec<(String, u32, u32)>,
}

impl Images {
    /// A step that, as it is, only applies the EXIF orientation of images
    /// that have one.
    pub fn new() -> Self {
        Self::default()
    }

    /// Shrinks images larger than `width` x `height` pixels to fit.
    pub fn max_dimensions(mut self, width: u32, height: u32) -> Self {
        self.max = Some((width.max(1), height.max(1)));
        self
    }

    /// Re-encodes every image so no metadata is left (EXIF, GPS position,
    /// XMP, comments), even when it needs no resizing or rotating.
    pub fn strip_metadata(mut self) -> Self {
        self.strip = true;
        self
    }

    /// Also keeps a copy that fits `width` x `height`, stored next to the
    /// original under `name` (`[A-Za-z0-9_-]`, e.g. `thumb`).
    pub fn thumbnail(mut self, name: &str, width: u32, height: u32) -> Self {
        self.thumbnails
            .push((name.to_owned(), width.max(1), height.max(1)));
        self
    }
}

/// What processing an image made.
struct Processed {
    /// The re-encoded image, when it changed.
    main: Option<Vec<u8>>,
    thumbnails: Vec<(String, Vec<u8>)>,
    format: ImageFormat,
}

enum Failure {
    /// The image is not acceptable; the message is for the person.
    Refused(&'static str),
    /// Encoding failed.
    Internal(String),
}

fn transform(bytes: &[u8], config: &Images) -> std::result::Result<Processed, Failure> {
    const UNREADABLE: Failure = Failure::Refused("The image could not be read.");
    let format = match image::guess_format(bytes) {
        Ok(f @ (ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)) => f,
        _ => return Err(UNREADABLE),
    };
    // The header first: a bomb is refused before any decoder allocates for it.
    let too_large = |(w, h): (u32, u32)| {
        w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS
    };
    if super::image_dimensions(bytes).is_some_and(too_large) {
        return Err(Failure::Refused("The image is too large."));
    }
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(|_| UNREADABLE)?;
    if too_large(decoder.dimensions()) {
        return Err(Failure::Refused("The image is too large."));
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|_| UNREADABLE)?;
    let mut changed = config.strip;
    if orientation != Orientation::NoTransforms {
        image.apply_orientation(orientation);
        changed = true;
    }
    if let Some((w, h)) = config.max
        && (image.width() > w || image.height() > h)
    {
        image = image.resize(w, h, image::imageops::FilterType::Lanczos3);
        changed = true;
    }
    let main = if changed {
        Some(encode(&image, format)?)
    } else {
        None
    };
    let mut thumbnails = Vec::new();
    for (name, w, h) in &config.thumbnails {
        let small = if image.width() > *w || image.height() > *h {
            image.thumbnail(*w, *h)
        } else {
            image.clone()
        };
        thumbnails.push((name.clone(), encode(&small, format)?));
    }
    Ok(Processed {
        main,
        thumbnails,
        format,
    })
}

fn encode(image: &DynamicImage, format: ImageFormat) -> std::result::Result<Vec<u8>, Failure> {
    let internal = |err: image::ImageError| Failure::Internal(err.to_string());
    let mut out = Vec::new();
    match format {
        ImageFormat::Jpeg => {
            let encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
            // JPEG has no alpha channel.
            DynamicImage::ImageRgb8(image.to_rgb8())
                .write_with_encoder(encoder)
                .map_err(internal)?;
        }
        ImageFormat::WebP => {
            // The WebP encoder takes 8-bit channels.
            let flat = if image.color().has_alpha() {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            };
            flat.write_to(&mut Cursor::new(&mut out), format)
                .map_err(internal)?;
        }
        _ => image
            .write_to(&mut Cursor::new(&mut out), format)
            .map_err(internal)?,
    }
    Ok(out)
}

impl UploadStep for Images {
    fn process<'a>(
        &'a self,
        file: &'a mut Upload,
        _ctx: &'a UploadContext,
    ) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            if !matches!(
                file.sniffed_type(),
                Some("image/jpeg" | "image/png" | "image/webp")
            ) {
                return Ok(StepOutcome::Keep);
            }
            // Decoding takes CPU time: off the async workers.
            let bytes = file.bytes().clone();
            let config = self.clone();
            let outcome = tokio::task::spawn_blocking(move || transform(&bytes, &config))
                .await
                .map_err(anyhow::Error::from)?;
            let processed = match outcome {
                Ok(processed) => processed,
                Err(Failure::Refused(message)) => return Ok(StepOutcome::Reject(message.into())),
                Err(Failure::Internal(err)) => {
                    return Err(anyhow::anyhow!("could not process the image: {err}").into());
                }
            };
            if let Some(main) = processed.main {
                file.set_bytes(Bytes::from(main));
            }
            let content_type = processed.format.to_mime_type();
            for (name, bytes) in processed.thumbnails {
                file.derive(
                    &name,
                    Upload::new(file.file_name().to_owned(), content_type, bytes),
                );
            }
            Ok(StepOutcome::Keep)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    /// A `w` x `h` image: red on the left half, blue on the right.
    fn picture(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                Rgb([250, 0, 0])
            } else {
                Rgb([0, 0, 250])
            }
        })
    }

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        encode(&DynamicImage::ImageRgb8(picture(w, h)), ImageFormat::Jpeg)
            .ok()
            .unwrap()
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        encode(&DynamicImage::ImageRgb8(picture(w, h)), ImageFormat::Png)
            .ok()
            .unwrap()
    }

    /// `jpeg` with an EXIF block saying `orientation` right after the SOI.
    fn with_orientation(jpeg: &[u8], orientation: u8) -> Vec<u8> {
        let mut tiff = b"MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
        tiff.extend_from_slice(&[0, orientation, 0, 0, 0, 0, 0, 0]);
        let mut segment = b"Exif\0\0".to_vec();
        segment.extend_from_slice(&tiff);
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&((segment.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&segment);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    fn size(bytes: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory(bytes).unwrap();
        (image.width(), image.height())
    }

    /// The image after the step, the thumbnails, and whether it was re-encoded.
    type Ran = (Vec<u8>, Vec<(String, Vec<u8>)>, bool);

    fn run(step: &Images, bytes: Vec<u8>) -> std::result::Result<Ran, String> {
        let before = bytes.clone();
        match transform(&bytes, step) {
            Ok(done) => {
                let changed = done.main.is_some();
                Ok((done.main.unwrap_or(before), done.thumbnails, changed))
            }
            Err(Failure::Refused(message)) => Err(message.into()),
            Err(Failure::Internal(message)) => panic!("{message}"),
        }
    }

    #[test]
    fn shrinks_to_fit_and_never_enlarges() {
        let step = Images::new().max_dimensions(100, 100);
        let (bytes, _, changed) = run(&step, png(400, 200)).unwrap();
        assert!(changed);
        assert_eq!(size(&bytes), (100, 50));
        let (_, _, changed) = run(&step, png(80, 40)).unwrap();
        assert!(!changed, "an image that fits is left as it came");
    }

    #[test]
    fn applies_exif_orientation_and_drops_the_metadata() {
        // Orientation 6: stored sideways, shown rotated 90 degrees clockwise.
        let sideways = with_orientation(&jpeg(80, 40), 6);
        let (bytes, _, changed) = run(&Images::new(), sideways.clone()).unwrap();
        assert!(changed);
        assert_eq!(size(&bytes), (40, 80), "upright now");
        assert!(!bytes.windows(4).any(|w| w == b"Exif"), "EXIF is gone");
        // Without EXIF and without `strip_metadata` the file is untouched.
        let (_, _, changed) = run(&Images::new(), jpeg(80, 40)).unwrap();
        assert!(!changed);
        // Orientation 1 (as shown) needs no change either.
        let (_, _, changed) = run(&Images::new(), with_orientation(&jpeg(80, 40), 1)).unwrap();
        assert!(!changed);
        // `strip_metadata` re-encodes anyway.
        let (bytes, _, changed) = run(&Images::new().strip_metadata(), sideways).unwrap();
        assert!(changed && size(&bytes) == (40, 80));
        let tagged = with_orientation(&jpeg(80, 40), 1);
        let (stripped, _, changed) = run(&Images::new().strip_metadata(), tagged).unwrap();
        assert!(changed && !stripped.windows(4).any(|w| w == b"Exif"));
    }

    #[test]
    fn thumbnails_fit_their_box_in_the_same_format() {
        let step = Images::new()
            .thumbnail("thumb", 64, 64)
            .thumbnail("tiny", 8, 8);
        let (original, thumbs, changed) = run(&step, jpeg(256, 128)).unwrap();
        assert!(!changed);
        assert_eq!(size(&original), (256, 128));
        assert_eq!(thumbs.len(), 2);
        assert_eq!(thumbs[0].0, "thumb");
        assert_eq!(size(&thumbs[0].1), (64, 32));
        assert_eq!(size(&thumbs[1].1), (8, 4));
        assert!(thumbs[0].1.starts_with(&[0xFF, 0xD8]));
        // A small image gets a copy, not an enlargement.
        let (_, thumbs, _) = run(&step, png(20, 10)).unwrap();
        assert_eq!(size(&thumbs[0].1), (20, 10));
    }

    #[test]
    fn webp_and_png_round_trip() {
        let webp = encode(&DynamicImage::ImageRgb8(picture(60, 30)), ImageFormat::WebP)
            .ok()
            .unwrap();
        let (bytes, _, changed) = run(&Images::new().max_dimensions(30, 30), webp).unwrap();
        assert!(changed);
        assert_eq!(size(&bytes), (30, 15));
        assert_eq!(&bytes[8..12], b"WEBP");
    }

    #[test]
    fn refuses_damaged_and_enormous_images() {
        let mut broken = png(40, 40);
        broken.truncate(30);
        assert_eq!(
            run(&Images::new(), broken).unwrap_err(),
            "The image could not be read."
        );
        // A header that claims 20,000 x 20,000 pixels.
        let mut huge = png(40, 40);
        huge[16..20].copy_from_slice(&20_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&20_000u32.to_be_bytes());
        assert_eq!(
            run(&Images::new(), huge).unwrap_err(),
            "The image is too large."
        );
    }
}

//! Turn a pasted or dropped image into a small WebP attachment.
//!
//! The app accepts what a camera or clipboard produces (JPEG, PNG, GIF, WebP)
//! and stores one format: a still WebP with its longest side at most
//! [`MAX_SIDE`] pixels. Decoding and encoding are pure Rust, so the same path
//! runs in the browser, on a phone and on the desktop (ADR-0008).
//!
//! EXIF orientation is applied before resizing, so a portrait photo does not
//! arrive sideways. Animated GIFs become their first frame; the note format is
//! for still images. SVG is refused: it can carry script and the preview would
//! have to sanitise it.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader};
use thiserror::Error;
use webp_rust::{ImageBuffer, LossyEncodingConfig};

/// Longest side of a stored image, in pixels.
pub const MAX_SIDE: u32 = 1600;

/// Refuse a file larger than this before decoding it.
pub const MAX_INPUT_BYTES: usize = 25 * 1024 * 1024;

/// A format this build can turn into WebP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    /// JPEG, the usual camera output.
    Jpeg,
    /// PNG, screenshots and pasted images.
    Png,
    /// GIF. Only the first frame is kept.
    Gif,
    /// An existing WebP.
    Webp,
}

/// An image ready to be stored as an attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestedImage {
    /// WebP bytes.
    pub bytes: Vec<u8>,
    /// Always `image/webp`.
    pub mime: &'static str,
    /// Display name with the `.webp` suffix.
    pub name: String,
    /// Width after resizing and orientation.
    pub width: u32,
    /// Height after resizing and orientation.
    pub height: u32,
    /// What the user supplied.
    pub source: SourceFormat,
    /// Bytes before compression, for the status line and tests.
    pub source_bytes: usize,
    /// WebP quality that met the size target, or the lowest one tried.
    pub quality: u8,
}

/// Why an image could not become an attachment.
#[derive(Debug, Error)]
pub enum IngestError {
    /// The file is too large to decode on a phone.
    #[error("that image is {bytes} bytes; the limit is {limit}")]
    TooLarge {
        /// Supplied size.
        bytes: usize,
        /// Accepted size.
        limit: usize,
    },

    /// The bytes are not an image this build understands.
    #[error("this image could not be read: {0}")]
    Decode(#[from] image::ImageError),

    /// Reading the in-memory buffer failed.
    #[error("this image could not be read: {0}")]
    Io(#[from] std::io::Error),

    /// The image is in a format this build does not convert.
    #[error("{0} images are not supported yet; use PNG, JPEG, GIF or WebP")]
    Unsupported(String),

    /// The bytes are not an image at all.
    #[error("that file is not an image")]
    NotAnImage,

    /// The pure-Rust encoder rejected the pixels.
    #[error("this image could not be compressed: {0}")]
    Encode(#[from] webp_rust::EncoderError),

    /// Even the lowest quality stayed above the hard ceiling.
    #[error("the compressed image is still {bytes} bytes; try a smaller picture")]
    StillTooLarge {
        /// Compressed size.
        bytes: usize,
    },
}

/// Highest quality tried first, then two cheaper attempts if the result is
/// still above the target. Every attempt is a full encode, so keep the list
/// short on a phone.
const QUALITY_STEPS: [u8; 3] = [80, 68, 56];

/// Prefer an output no larger than this; keep going down the quality steps.
const TARGET_BYTES: usize = 400 * 1024;

/// Refuse an attachment larger than this. The server splits 1 MiB chunks, and
/// a D1 row may not exceed 2 MB.
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

/// Decode, orient, shrink and encode `input` as WebP.
///
/// `name` is the file name the user supplied; the result always ends in
/// `.webp` because that is what the bytes are.
pub fn ingest(name: &str, input: &[u8]) -> Result<IngestedImage, IngestError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(IngestError::TooLarge {
            bytes: input.len(),
            limit: MAX_INPUT_BYTES,
        });
    }

    let reader = ImageReader::new(Cursor::new(input)).with_guessed_format()?;
    let format = reader.format().ok_or(IngestError::NotAnImage)?;
    let format = source_format(format)?;
    let mut decoder = reader.into_decoder()?;
    // Some formats (notably JPEG) expose EXIF orientation only through the
    // decoder, so read it before the decoder is consumed.
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    if image.width() > MAX_SIDE || image.height() > MAX_SIDE {
        image = image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Lanczos3);
    }

    let width = image.width();
    let height = image.height();
    let rgba = image.to_rgba8();
    let buffer = ImageBuffer {
        width: width as usize,
        height: height as usize,
        rgba: rgba.into_raw(),
    };

    let mut last_size = 0;
    for quality in QUALITY_STEPS {
        let config = LossyEncodingConfig {
            quality: f32::from(quality),
            ..LossyEncodingConfig::default()
        };
        let bytes = webp_rust::encode_lossy_with_config(&buffer, &config, None)?;
        last_size = bytes.len();
        if bytes.len() <= TARGET_BYTES {
            return Ok(IngestedImage {
                name: webp_name(name),
                bytes,
                mime: "image/webp",
                width,
                height,
                source: format,
                source_bytes: input.len(),
                quality,
            });
        }
        if quality == *QUALITY_STEPS.last().unwrap_or(&quality) {
            if bytes.len() > MAX_OUTPUT_BYTES {
                return Err(IngestError::StillTooLarge { bytes: bytes.len() });
            }
            return Ok(IngestedImage {
                name: webp_name(name),
                bytes,
                mime: "image/webp",
                width,
                height,
                source: format,
                source_bytes: input.len(),
                quality,
            });
        }
    }

    // `QUALITY_STEPS` is not empty, so the loop always returns. Keep a typed
    // fallback in case that ever changes.
    Err(IngestError::StillTooLarge { bytes: last_size })
}

fn source_format(format: image::ImageFormat) -> Result<SourceFormat, IngestError> {
    match format {
        image::ImageFormat::Jpeg => Ok(SourceFormat::Jpeg),
        image::ImageFormat::Png => Ok(SourceFormat::Png),
        image::ImageFormat::Gif => Ok(SourceFormat::Gif),
        image::ImageFormat::WebP => Ok(SourceFormat::Webp),
        other => Err(IngestError::Unsupported(format!("{other:?}"))),
    }
}

fn webp_name(name: &str) -> String {
    notes_core::normalize_file_name(name, "image/webp")
}

#[cfg(test)]
mod tests {
    use super::{ingest, IngestError, SourceFormat, MAX_SIDE};
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    fn photo(width: u32, height: u32) -> Vec<u8> {
        let mut image = RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode png");
        bytes.into_inner()
    }

    #[test]
    fn a_large_png_becomes_a_small_webp() {
        let input = photo(1800, 900);
        let image = ingest("Camera/IMG 1.PNG", &input).expect("ingest");
        assert_eq!(image.mime, "image/webp");
        assert_eq!(image.name, "IMG 1.webp");
        assert_eq!(image.source, SourceFormat::Png);
        assert_eq!((image.width, image.height), (MAX_SIDE, 800));
        assert!(image.bytes.len() < 2 * 1024 * 1024);
        assert_eq!(&image.bytes[..4], b"RIFF");
        assert_eq!(&image.bytes[8..12], b"WEBP");
        assert!(image.quality <= 80);
        assert_eq!(image.source_bytes, input.len());
    }

    #[test]
    fn a_small_image_keeps_its_dimensions() {
        let input = photo(64, 48);
        let image = ingest("dot.png", &input).expect("ingest");
        assert_eq!((image.width, image.height), (64, 48));
        assert!(image.bytes.len() < input.len());
    }

    #[test]
    fn text_that_is_not_an_image_is_refused() {
        let error = ingest("notes.txt", b"hello").expect_err("not an image");
        assert!(matches!(error, IngestError::NotAnImage));
    }

    #[test]
    fn the_input_ceiling_is_checked_before_decoding() {
        let error = ingest("big.png", &vec![0; super::MAX_INPUT_BYTES + 1]).expect_err("too large");
        assert!(matches!(error, IngestError::TooLarge { .. }));
    }
}

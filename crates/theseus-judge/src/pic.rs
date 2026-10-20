//! Pictures for `onto.v1` (theseus-und4g; design `jev-ontology-growth.md`
//! section 5): what an attached image becomes before it leaves the process.
//!
//! Jev takes images as PNG or JPEG only, each side 280 to 1,568 px, and one
//! rejected image fails the whole request (`~/reports/theseus-jev-probe`), and
//! the owner's yes is for images *without* their metadata. So the code, not
//! the API, does all of it:
//!
//! - **Decode** (PNG or JPEG; any other format is skipped with its reason),
//!   then **rotate the pixels upright** by the EXIF orientation, which the API
//!   ignores.
//! - **Re-encode from the pixels.** Nothing of the original file is copied:
//!   EXIF, GPS, maker notes, comments, colour profiles and any trailing bytes
//!   do not survive, because only the decoded pixels are written.
//! - **Fit**: the long edge at most [`MAX_SIDE`] (1,024), each side at least
//!   [`MIN_SIDE`] (280, padded with white, never upscaled), the file at most
//!   [`MAX_BYTES`] (1 MB): PNG first, then JPEG at falling quality, then a
//!   smaller picture.
//! - **Skip with a row** what cannot be sent: over [`SKIP_BYTES`] (20 MB),
//!   a format that is not PNG or JPEG, an undecodable file, a picture past
//!   [`MAX_PER_MESSAGE`] (four a message), one too large to fit. The reason
//!   is the state's, so the judgment's record says what was left out.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::metadata::Orientation;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Rgb,
    RgbImage,
};
use sha2::{Digest, Sha256};

use crate::client::Asset;

/// The longest edge sent, in pixels.
pub const MAX_SIDE: u32 = 1024;
/// The shortest side Jev takes, in pixels (a smaller one is padded).
pub const MIN_SIDE: u32 = 280;
/// The most a re-encoded picture weighs.
pub const MAX_BYTES: usize = 1 << 20;
/// A file larger than this is skipped, undecoded.
pub const SKIP_BYTES: u64 = 20 * 1024 * 1024;
/// The pictures one message sends at most; the rest are skipped with a row.
pub const MAX_PER_MESSAGE: usize = 4;
/// A decoded picture past this many pixels is skipped (a decompression bomb
/// is a small file).
const MAX_PIXELS: u64 = 100_000_000;

/// What a picture was sent as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
}

impl Format {
    /// The `format` the wire names.
    pub fn wire(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpeg",
        }
    }
}

/// A picture ready to send: the re-encoded bytes and their size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub format: Format,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The size of the file it came from.
    pub from_bytes: u64,
}

impl Shown {
    /// The asset the state refers to by [`Shown::id`].
    pub fn asset(&self) -> Asset {
        use base64::Engine;
        Asset {
            id: self.id(),
            format: self.format.wire().to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(&self.bytes),
        }
    }

    /// A UUID-shaped id of the re-encoded bytes: the same picture is the
    /// same reference in every pack's state, so a batch still shares one.
    pub fn id(&self) -> String {
        let h = hex::encode(Sha256::digest(&self.bytes));
        format!(
            "{}-{}-{}-{}-{}",
            &h[0..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..32]
        )
    }
}

/// Why a picture was not sent. Its words are the state's, for the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The file is over [`SKIP_BYTES`].
    TooLarge { bytes: u64 },
    /// Not a PNG or a JPEG (a GIF, WebP or HEIC is not converted).
    Format(String),
    /// The file would not decode, or is past the pixel limits.
    Unreadable(String),
    /// It would not fit [`MAX_BYTES`] at any size tried.
    WontFit,
    /// A message's fifth picture or later.
    OverMessage,
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Skip::TooLarge { bytes } => write!(
                f,
                "skipped: {} MB is over the {} MB limit",
                bytes.div_ceil(1024 * 1024),
                SKIP_BYTES / (1024 * 1024)
            ),
            Skip::Format(what) => write!(f, "skipped: {what} is not a PNG or a JPEG"),
            Skip::Unreadable(why) => write!(f, "skipped: unreadable ({why})"),
            Skip::WontFit => write!(f, "skipped: it would not fit 1 MB"),
            Skip::OverMessage => write!(f, "skipped: past {MAX_PER_MESSAGE} pictures a message"),
        }
    }
}

/// One attachment's picture: its name and what became of it. Never
/// serialized: the bytes are the process's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub shown: Result<Shown, Skip>,
}

/// The pictures of one message, each prepared, the fifth and later skipped.
pub fn prepare_all(items: &[(String, Vec<u8>)]) -> Vec<Picture> {
    items
        .iter()
        .enumerate()
        .map(|(n, (name, raw))| Picture {
            name: name.clone(),
            shown: match n >= MAX_PER_MESSAGE {
                true => Err(Skip::OverMessage),
                false => prepare(raw),
            },
        })
        .collect()
}

/// One file's bytes, as Jev takes them: see the module.
pub fn prepare(raw: &[u8]) -> Result<Shown, Skip> {
    prepare_sized(raw, raw.len() as u64)
}

/// [`prepare`] for a file whose size on disk is `size` (the sender's own
/// figure, which may be over what was read).
pub fn prepare_sized(raw: &[u8], size: u64) -> Result<Shown, Skip> {
    if size > SKIP_BYTES || raw.len() as u64 > SKIP_BYTES {
        return Err(Skip::TooLarge { bytes: size });
    }
    let reader = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .map_err(|e| Skip::Unreadable(e.to_string()))?;
    match reader.format() {
        Some(ImageFormat::Png | ImageFormat::Jpeg) => {}
        Some(other) => return Err(Skip::Format(format!("{other:?}").to_lowercase())),
        None => return Err(Skip::Format("this file".into())),
    }
    // The decoder's own limits stand (an allocation of at most 512 MiB).
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| Skip::Unreadable(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(Skip::Unreadable(format!("{w}x{h} px")));
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img =
        DynamicImage::from_decoder(decoder).map_err(|e| Skip::Unreadable(e.to_string()))?;
    img.apply_orientation(orientation);
    let rgb = flatten(&img);
    let (format, bytes, rgb) = fit(rgb).ok_or(Skip::WontFit)?;
    Ok(Shown {
        format,
        bytes,
        width: rgb.width(),
        height: rgb.height(),
        from_bytes: size,
    })
}

/// The picture on white: transparency is not sent as colour, and a JPEG has
/// none.
fn flatten(img: &DynamicImage) -> RgbImage {
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = u32::from(p[3]);
        let blend = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    out
}

/// The picture at `side` on its long edge at most (never enlarged), padded
/// to [`MIN_SIDE`] on each side.
fn sized(rgb: &RgbImage, side: u32) -> RgbImage {
    let (w, h) = rgb.dimensions();
    let long = w.max(h);
    let scaled = match long > side {
        true => {
            let k = f64::from(side) / f64::from(long);
            let nw = ((f64::from(w) * k).round() as u32).max(1);
            let nh = ((f64::from(h) * k).round() as u32).max(1);
            imageops::resize(rgb, nw, nh, FilterType::Lanczos3)
        }
        false => rgb.clone(),
    };
    let (nw, nh) = scaled.dimensions();
    if nw >= MIN_SIDE && nh >= MIN_SIDE {
        return scaled;
    }
    let (cw, ch) = (nw.max(MIN_SIDE), nh.max(MIN_SIDE));
    let mut canvas = RgbImage::from_pixel(cw, ch, Rgb([255, 255, 255]));
    imageops::overlay(
        &mut canvas,
        &scaled,
        i64::from((cw - nw) / 2),
        i64::from((ch - nh) / 2),
    );
    canvas
}

/// The first of PNG, then JPEG at 85, 70 and 50, that weighs at most
/// [`MAX_BYTES`], at 1,024 px, then 768, 512 and 384 on the long edge.
fn fit(rgb: RgbImage) -> Option<(Format, Vec<u8>, RgbImage)> {
    for side in [MAX_SIDE, 768, 512, 384] {
        let pic = sized(&rgb, side);
        if let Some(png) = png(&pic).filter(|b| b.len() <= MAX_BYTES) {
            return Some((Format::Png, png, pic));
        }
        for q in [85, 70, 50] {
            if let Some(jpg) = jpeg(&pic, q).filter(|b| b.len() <= MAX_BYTES) {
                return Some((Format::Jpeg, jpg, pic));
            }
        }
    }
    None
}

fn png(rgb: &RgbImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Best, PngFilter::Adaptive)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(out)
}

fn jpeg(rgb: &RgbImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(out)
}

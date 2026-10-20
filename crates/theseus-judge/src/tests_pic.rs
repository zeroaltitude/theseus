//! The pictures of `onto.v1` (theseus-und4g): what an attached image becomes
//! before it leaves the process. Each test builds its file here, in code, so
//! the proof does not lean on a fixture nobody can read.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

use crate::pic::{self, Format, Skip};

/// A smooth picture: a gradient with a bar, which compresses well.
fn gradient(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        Rgb([
            (x * 255 / w.max(1)) as u8,
            (y * 255 / h.max(1)) as u8,
            if (x / 20) % 2 == 0 { 40 } else { 200 },
        ])
    })
}

/// A noisy one, which does not (a deterministic xorshift, no `rand`).
fn noise(w: u32, h: u32) -> RgbImage {
    let mut s = 0x2545_f491_4f6c_dd1du64;
    RgbImage::from_fn(w, h, |_, _| {
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 24) as u8
        };
        Rgb([next(), next(), next()])
    })
}

fn png_of(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .unwrap();
    out
}

fn jpeg_of(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 90)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgb8,
        )
        .unwrap();
    out
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// A JPEG's EXIF segment (APP1) with an orientation, a GPS-looking tag name
/// and a maker note, spliced in right after the start of image.
fn with_exif(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    let mut tiff: Vec<u8> = b"MM\x00\x2a\x00\x00\x00\x08".to_vec();
    // One entry: Orientation (0x0112), SHORT, one value.
    tiff.extend_from_slice(&[0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1]);
    tiff.extend_from_slice(&orientation.to_be_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&[0, 0, 0, 0]);
    tiff.extend_from_slice(b"GPSLatitude 33.4484 N SECRET-MAKER-NOTE");
    let mut seg: Vec<u8> = b"Exif\0\0".to_vec();
    seg.extend_from_slice(&tiff);
    let len = (seg.len() + 2) as u16;
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&seg);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A PNG with a text chunk and an eXIf chunk of metadata after its header.
fn png_with_metadata(png: &[u8]) -> Vec<u8> {
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        c.extend_from_slice(&body);
        c.extend_from_slice(&crc32(&body).to_be_bytes());
        c
    };
    // The signature (8 bytes) and IHDR (4 + 4 + 13 + 4) come first.
    let at = 8 + 25;
    let mut out = png[..at].to_vec();
    out.extend(chunk(
        b"tEXt",
        b"Comment\0GPSLatitude 33.4484 SECRET-MAKER-NOTE",
    ));
    out.extend(chunk(
        b"eXIf",
        b"MM\x00\x2a\x00\x00\x00\x08SECRET-MAKER-NOTE",
    ));
    out.extend_from_slice(&png[at..]);
    out
}

fn dims(bytes: &[u8]) -> (u32, u32) {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .unwrap()
        .into_dimensions()
        .unwrap()
}

/// A JPEG with EXIF and GPS leaves without any of it.
#[test]
fn a_jpeg_with_gps_leaves_without_it() {
    let raw = with_exif(&jpeg_of(&gradient(600, 400)), 1);
    assert!(has(&raw, b"Exif") && has(&raw, b"SECRET-MAKER-NOTE"));
    let shown = pic::prepare(&raw).unwrap();
    for needle in [
        &b"Exif"[..],
        b"GPS",
        b"SECRET-MAKER-NOTE",
        b"Comment",
        b"tEXt",
        b"eXIf",
        b"ICC_PROFILE",
    ] {
        assert!(
            !has(&shown.bytes, needle),
            "{} survived",
            String::from_utf8_lossy(needle)
        );
    }
    assert_eq!(dims(&shown.bytes), (600, 400));
}

/// A PNG with text and eXIf chunks leaves without them.
#[test]
fn a_png_with_gps_metadata_leaves_without_it() {
    let raw = png_with_metadata(&png_of(&gradient(500, 300)));
    assert!(has(&raw, b"SECRET-MAKER-NOTE"));
    // The file is still a PNG the decoder reads.
    assert_eq!(dims(&raw), (500, 300));
    let shown = pic::prepare(&raw).unwrap();
    assert_eq!(shown.format, Format::Png);
    for needle in [
        &b"GPS"[..],
        b"SECRET-MAKER-NOTE",
        b"tEXt",
        b"eXIf",
        b"Comment",
    ] {
        assert!(!has(&shown.bytes, needle));
    }
}

/// The API ignores the orientation, so the pixels are turned upright before
/// the metadata goes: a 400 x 300 photo marked "rotate 90" arrives 300 x 400.
#[test]
fn the_pixels_are_turned_upright_by_the_orientation() {
    let raw = with_exif(&jpeg_of(&gradient(400, 300)), 6);
    let shown = pic::prepare(&raw).unwrap();
    assert_eq!((shown.width, shown.height), (300, 400));
    assert_eq!(dims(&shown.bytes), (300, 400));
    let upright = pic::prepare(&with_exif(&jpeg_of(&gradient(400, 300)), 1)).unwrap();
    assert_eq!((upright.width, upright.height), (400, 300));
}

/// Each side at least 280 (padded, never enlarged), the long edge at most
/// 1,024 (shrunk with its proportions).
#[test]
fn the_size_floor_and_ceiling_hold() {
    let size = |w, h| {
        let s = pic::prepare(&png_of(&gradient(w, h))).unwrap();
        (s.width, s.height)
    };
    assert_eq!(size(2000, 1500), (1024, 768));
    assert_eq!(size(1500, 2000), (768, 1024));
    assert_eq!(size(100, 60), (280, 280), "a thumbnail is padded");
    assert_eq!(
        size(4000, 10),
        (1024, 280),
        "a strip is shrunk, then padded"
    );
    assert_eq!(size(300, 300), (300, 300), "a small one is not enlarged");
    assert_eq!(size(1024, 1024), (1024, 1024));
}

/// A picture that does not fit a megabyte as a PNG goes as a JPEG, and in
/// any case weighs at most a megabyte.
#[test]
fn a_heavy_picture_is_a_jpeg_under_a_megabyte() {
    let shown = pic::prepare(&png_of(&noise(1500, 1200))).unwrap();
    assert_eq!(shown.format, Format::Jpeg);
    assert!(shown.bytes.len() <= pic::MAX_BYTES, "{}", shown.bytes.len());
    assert!(shown.width.max(shown.height) <= pic::MAX_SIDE);
    assert!(shown.width.min(shown.height) >= pic::MIN_SIDE);
    // A smooth one stays a PNG.
    assert_eq!(
        pic::prepare(&png_of(&gradient(800, 600))).unwrap().format,
        Format::Png
    );
}

/// Transparency is not sent as colour: a clear pixel is white.
#[test]
fn transparency_is_flattened_onto_white() {
    let img = RgbaImage::from_pixel(300, 300, Rgba([10, 20, 30, 0]));
    let mut raw = Vec::new();
    img.write_to(&mut Cursor::new(&mut raw), ImageFormat::Png)
        .unwrap();
    let shown = pic::prepare(&raw).unwrap();
    let back = image::load_from_memory(&shown.bytes).unwrap().to_rgb8();
    assert_eq!(back.get_pixel(150, 150), &Rgb([255, 255, 255]));
}

/// A file over 20 MB is skipped, undecoded, and its row says so.
#[test]
fn a_25_mb_file_is_skipped_with_its_row() {
    let raw = vec![0u8; 25 << 20];
    let skip = pic::prepare(&raw).unwrap_err();
    assert_eq!(skip, Skip::TooLarge { bytes: 25 << 20 });
    assert_eq!(skip.to_string(), "skipped: 25 MB is over the 20 MB limit");
    // The sender's own size counts too, whatever was read.
    assert!(matches!(
        pic::prepare_sized(&png_of(&gradient(300, 300)), 30 << 20),
        Err(Skip::TooLarge { .. })
    ));
}

/// Only PNG and JPEG are sent; anything else is skipped with its format.
#[test]
fn other_formats_and_junk_are_skipped() {
    // The magic is what names a format; this GIF codec is not built in.
    let gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00;".to_vec();
    assert_eq!(pic::prepare(&gif).unwrap_err(), Skip::Format("gif".into()));
    assert!(matches!(
        pic::prepare(b"just some words, not a picture").unwrap_err(),
        Skip::Format(_)
    ));
    // A PNG cut short is unreadable, not a panic.
    let png = png_of(&gradient(300, 300));
    assert!(matches!(
        pic::prepare(&png[..png.len() / 2]).unwrap_err(),
        Skip::Unreadable(_)
    ));
}

/// A message sends at most four; the fifth and later are skipped.
#[test]
fn a_message_sends_four_pictures() {
    let raw = png_of(&gradient(300, 300));
    let items: Vec<(String, Vec<u8>)> =
        (0..6).map(|n| (format!("p{n}.png"), raw.clone())).collect();
    let all = pic::prepare_all(&items);
    assert_eq!(all.iter().filter(|p| p.shown.is_ok()).count(), 4);
    assert_eq!(all[4].shown, Err(Skip::OverMessage));
    assert_eq!(all[5].shown, Err(Skip::OverMessage));
}

/// The reference is the bytes' own: the same picture is the same id, a
/// different one is not, and it is UUID-shaped.
#[test]
fn a_picture_is_referenced_by_a_hash_of_its_bytes() {
    let a = pic::prepare(&png_of(&gradient(300, 300))).unwrap();
    let b = pic::prepare(&png_of(&gradient(300, 300))).unwrap();
    let c = pic::prepare(&png_of(&gradient(320, 300))).unwrap();
    assert_eq!(a.id(), b.id());
    assert_ne!(a.id(), c.id());
    let parts: Vec<usize> = a.id().split('-').map(str::len).collect();
    assert_eq!(parts, [8, 4, 4, 4, 12]);
    assert_eq!(a.asset().format, "png");
}

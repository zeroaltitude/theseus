//! Images a model can be shown (theseus-9g2): PNG, JPEG, GIF, and WebP,
//! known by their first bytes (never by a file name), with their dimensions,
//! and the limits a request may carry. Nothing here decodes pixels: the
//! header is enough to name the type, size the token estimate, and refuse
//! what the provider would refuse.

pub use theseus_protocol::MAX_IMAGE_BYTES;

/// The longest side the provider takes (Anthropic refuses an image over
/// 8,000 pixels on either side).
pub const MAX_IMAGE_SIDE: u32 = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    /// `image/png`, `image/jpeg`, `image/gif`, or `image/webp`.
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
}

impl ImageInfo {
    /// `PNG`, `JPEG`, `GIF`, `WebP`.
    pub fn kind(&self) -> &'static str {
        match self.media_type {
            "image/png" => "PNG",
            "image/jpeg" => "JPEG",
            "image/gif" => "GIF",
            _ => "WebP",
        }
    }
}

/// Why an image of this size would not be sent, if it would not.
pub fn refusal(bytes: u64, info: &ImageInfo) -> Option<String> {
    if bytes > MAX_IMAGE_BYTES {
        Some("an image over the 5 MiB limit".into())
    } else if info.width > MAX_IMAGE_SIDE || info.height > MAX_IMAGE_SIDE {
        Some(format!(
            "an image over {} pixels on a side ({}×{})",
            MAX_IMAGE_SIDE, info.width, info.height
        ))
    } else if info.width == 0 || info.height == 0 {
        Some("an image with no pixels".into())
    } else {
        None
    }
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}
fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}
fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn le24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 3)?;
    Some(s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16)
}

/// The type and dimensions of an image the models read, from its bytes;
/// `None` for anything else, a truncated header included.
pub fn sniff(b: &[u8]) -> Option<ImageInfo> {
    let info = |media_type, width, height| ImageInfo {
        media_type,
        width,
        height,
    };
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        // The IHDR chunk comes first: width and height at 16 and 20.
        if b.get(12..16)? != b"IHDR" {
            return None;
        }
        return Some(info("image/png", be32(b, 16)?, be32(b, 20)?));
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some(info("image/gif", le16(b, 6)?, le16(b, 8)?));
    }
    if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return match b.get(12..16)? {
            b"VP8 " => {
                // A key frame: its start code, then 14-bit width and height.
                if b.get(23..26)? != [0x9d, 0x01, 0x2a] {
                    return None;
                }
                Some(info(
                    "image/webp",
                    le16(b, 26)? & 0x3fff,
                    le16(b, 28)? & 0x3fff,
                ))
            }
            b"VP8L" => {
                if *b.get(20)? != 0x2f {
                    return None;
                }
                let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                Some(info(
                    "image/webp",
                    (bits & 0x3fff) + 1,
                    ((bits >> 14) & 0x3fff) + 1,
                ))
            }
            b"VP8X" => Some(info("image/webp", le24(b, 24)? + 1, le24(b, 27)? + 1)),
            _ => None,
        };
    }
    if b.starts_with(&[0xff, 0xd8, 0xff]) {
        return jpeg(b);
    }
    None
}

/// A JPEG's dimensions are in its first start-of-frame segment; walk the
/// segments to it.
fn jpeg(b: &[u8]) -> Option<ImageInfo> {
    let mut i = 2;
    loop {
        while *b.get(i)? != 0xff {
            i += 1;
        }
        while *b.get(i)? == 0xff {
            i += 1;
        }
        let marker = *b.get(i)?;
        i += 1;
        match marker {
            // Markers without a length.
            0x01 | 0xd0..=0xd8 => continue,
            // Start of scan or end of image before any frame: no dimensions.
            0xda | 0xd9 => return None,
            // Start of frame (not DHT, JPG, or DAC): precision, height, width.
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                return Some(ImageInfo {
                    media_type: "image/jpeg",
                    width: be16(b, i + 5)?,
                    height: be16(b, i + 3)?,
                });
            }
            _ => i += be16(b, i)? as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header for the given size, then `pad` zero bytes. Nothing in
    /// Theseus decodes pixels, so a header is a PNG as far as it can tell.
    fn png(width: u32, height: u32, pad: usize) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        v.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        v.resize(v.len() + pad, 0);
        v
    }

    #[test]
    fn the_four_types_are_known_by_their_bytes_with_their_sizes() {
        assert_eq!(
            sniff(&png(800, 600, 10)),
            Some(ImageInfo {
                media_type: "image/png",
                width: 800,
                height: 600
            })
        );
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0x20, 0x03, 0x58, 0x02, 0, 0]);
        assert_eq!(sniff(&gif).map(|i| (i.width, i.height)), Some((800, 600)));
        // JPEG: SOI, an APP0 segment to skip, then SOF0 with height then width.
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
        jpg.extend_from_slice(&[0; 14]);
        jpg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08, 0x02, 0x58, 0x03, 0x20, 0x03]);
        let j = sniff(&jpg).unwrap();
        assert_eq!((j.media_type, j.width, j.height), ("image/jpeg", 800, 600));
        // WebP, lossless: 14-bit width-1 and height-1 after the 0x2f signature.
        let bits: u32 = 799 | (599 << 14);
        let mut webp = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        webp.extend_from_slice(&bits.to_le_bytes());
        assert_eq!(sniff(&webp).map(|i| (i.width, i.height)), Some((800, 600)));
        // WebP, extended: 24-bit canvas width-1 and height-1.
        let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".to_vec();
        vp8x.extend_from_slice(&[0x1f, 0x03, 0x00, 0x57, 0x02, 0x00]);
        assert_eq!(sniff(&vp8x).map(|i| i.kind()), Some("WebP"));
        assert_eq!(sniff(&vp8x).map(|i| (i.width, i.height)), Some((800, 600)));
        // Not images, or cut short: nothing.
        assert_eq!(sniff(b"PK\x03\x04zip"), None);
        assert_eq!(sniff(b"hello"), None);
        assert_eq!(sniff(&png(800, 600, 0)[..18]), None);
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xda]), None);
    }

    #[test]
    fn what_the_provider_would_refuse_is_refused_with_the_reason() {
        let ok = sniff(&png(800, 600, 0)).unwrap();
        assert_eq!(refusal(1_000, &ok), None);
        assert_eq!(
            refusal(6 * 1024 * 1024, &ok).as_deref(),
            Some("an image over the 5 MiB limit")
        );
        let tall = sniff(&png(100, 9_000, 0)).unwrap();
        assert_eq!(
            refusal(1_000, &tall).as_deref(),
            Some("an image over 8000 pixels on a side (100×9000)")
        );
    }
}

//! Files that come with an operator's message (theseus-9g2): a Discord
//! attachment, or `theseus ask --attach`; and images a tool returns.
//!
//! The sender reads what it can: text as text, an image as its bytes, and
//! anything else is listed with the reason. The core keeps each one on the
//! user node: text capped at `[tools].max_read_bytes` on a character
//! boundary, an image stored once in the store's blobs with the node holding
//! its digest. The compiler renders each as its own block before the typed
//! text, under a header that names the file and who sent it. Theseus has no
//! provenance labels yet (§3.9), so the header is what marks the text as the
//! file's and not the operator's.
//!
//! An image renders as an image block only for a model whose catalog entry
//! has vision; any other model reads one line saying it was not shown. The
//! rendering depends on the node, the model, and the blob's bytes only, so
//! the same node renders to the same bytes every time.

use serde_json::{json, Value};
use theseus_tools::image;

use crate::blobs::Blobs;
use crate::narrative;
use crate::node::{Attachment, AttachmentContent};

/// The longest file name or type a header repeats.
const MAX_NAME_CHARS: usize = 200;

/// A name or type as a header shows it: one line, bounded.
fn clean(s: &str) -> String {
    let one_line: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let t = one_line.trim();
    if t.chars().count() > MAX_NAME_CHARS {
        format!("{}…", t.chars().take(MAX_NAME_CHARS).collect::<String>())
    } else {
        t.to_string()
    }
}

/// `5,012 bytes` under a mebibyte, `1.2 MB` from there.
pub fn size_words(n: u64) -> String {
    if n < 1024 * 1024 {
        narrative::count(n, "byte", "bytes")
    } else {
        narrative::bytes(n)
    }
}

/// The start of `s` that fits in `max` bytes, cut on a character boundary,
/// and whether anything was cut.
pub fn cut_to(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

fn from_words(author: Option<&str>) -> String {
    author
        .filter(|s| !s.is_empty())
        .map(|s| format!(" from {}", clean(s)))
        .unwrap_or_default()
}

/// The line that names an attachment wherever it is shown: the model's
/// context, the web UI, and `theseus history`. For example
/// `[Attachment message.txt from discord:eddie, 5,012 bytes]`, or
/// `[Image photo.png from discord:eddie, 1.2 MB, 1280×720]`.
pub fn header(a: &Attachment, author: Option<&str>) -> String {
    let from = from_words(author);
    let name = clean(&a.name);
    let size = size_words(a.size);
    match &a.content {
        AttachmentContent::Text { cut: false, .. } => format!("[Attachment {name}{from}, {size}]"),
        AttachmentContent::Text { text, cut: true } => format!(
            "[Attachment {name}{from}, {size}; cut to its first {}]",
            size_words(text.len() as u64)
        ),
        AttachmentContent::Image { width, height, .. } => {
            format!("[Image {name}{from}, {size}, {width}×{height}]")
        }
        AttachmentContent::NotRead { reason } => {
            let kind = if a.media_type.is_empty() {
                String::new()
            } else {
                format!(", {}", clean(&a.media_type))
            };
            format!("[Attachment {name}{from}{kind}, {size}: not read: {reason}]")
        }
    }
}

/// The one line an image becomes when it is not shown.
fn not_shown(a: &Attachment, author: Option<&str>, why: &str) -> String {
    format!(
        "[Image {}{}, {}: not shown, {why}]",
        clean(&a.name),
        from_words(author),
        size_words(a.size)
    )
}

/// How a request shows images: whether its model has vision, which model
/// (for the token estimate), and where the bytes are.
pub struct Media<'a> {
    pub vision: bool,
    pub model: &'a str,
    pub blobs: Option<&'a Blobs>,
}

impl Media<'_> {
    /// For a request that carries no images (tests, probes).
    pub fn none() -> Media<'static> {
        Media {
            vision: false,
            model: "",
            blobs: None,
        }
    }
}

/// What one image renders as: an image block and its estimated tokens, or
/// the line that says why it is not shown.
enum Shown {
    Block(Value, u64),
    Line(String),
}

fn show(a: &Attachment, author: Option<&str>, media: &Media) -> Shown {
    let AttachmentContent::Image {
        digest,
        width,
        height,
    } = &a.content
    else {
        return Shown::Line(header(a, author));
    };
    if !media.vision {
        return Shown::Line(not_shown(a, author, "this model has no vision"));
    }
    match media.blobs.and_then(|b| b.base64(digest)) {
        Some(data) => Shown::Block(
            json!({"type": "image", "source": {"type": "base64", "media_type": a.media_type, "data": &*data}}),
            crate::catalog::image_tokens(media.model, *width, *height),
        ),
        None => Shown::Line(not_shown(a, author, "its stored bytes are missing")),
    }
}

/// The blocks one attachment puts in a user message, before the typed
/// text; `tokens` gains what its images are estimated to cost.
pub fn blocks(a: &Attachment, author: Option<&str>, media: &Media, tokens: &mut u64) -> Vec<Value> {
    let text = |t: String| json!({"type": "text", "text": t});
    match &a.content {
        AttachmentContent::Text { text: body, .. } => {
            vec![text(format!("{}\n{body}", header(a, author)))]
        }
        AttachmentContent::NotRead { .. } => vec![text(header(a, author))],
        AttachmentContent::Image { .. } => match show(a, author, media) {
            Shown::Block(img, t) => {
                *tokens += t;
                vec![text(header(a, author)), img]
            }
            Shown::Line(line) => vec![text(line)],
        },
    }
}

/// A `tool_result`'s content when its tool returned an image: the text,
/// then the image block for a vision model; the text and the not-shown line
/// for any other.
pub fn tool_content(content: &str, img: &Attachment, media: &Media, tokens: &mut u64) -> Value {
    match show(img, None, media) {
        Shown::Block(block, t) => {
            *tokens += t;
            json!([{"type": "text", "text": content}, block])
        }
        Shown::Line(line) => Value::String(format!("{content}\n{line}")),
    }
}

/// What the model reads for one attachment as text (a text file or one not read).
pub fn for_model(a: &Attachment, author: Option<&str>) -> String {
    match &a.content {
        AttachmentContent::Text { text, .. } => format!("{}\n{text}", header(a, author)),
        _ => header(a, author),
    }
}

/// A message as people read it: the typed text, then one header line per
/// attachment (the web UI and the CLI show this, so they need no change).
pub fn display_text(text: &str, attachments: &[Attachment], author: Option<&str>) -> String {
    let mut out = text.to_string();
    for a in attachments {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&header(a, author));
    }
    out
}

/// An image's bytes as the node keeps them: checked against what the
/// provider takes and stored once in the blobs. `Err` is the reason it was
/// not kept.
pub fn store_image(bytes: &[u8], blobs: &Blobs) -> Result<(image::ImageInfo, String), String> {
    let info = image::sniff(bytes)
        .ok_or_else(|| "not an image the models read (PNG, JPEG, GIF, or WebP)".to_string())?;
    if let Some(why) = image::refusal(bytes.len() as u64, &info) {
        return Err(why);
    }
    let digest = blobs
        .put(bytes)
        .map_err(|e| format!("it could not be stored ({e})"))?;
    Ok((info, digest))
}

/// The node's attachments from the wire's, in order. Nothing here fails a
/// turn: whatever cannot be kept becomes a `not_read` with the reason.
pub fn from_wire(
    list: Vec<theseus_protocol::Attachment>,
    max_text: usize,
    blobs: &Blobs,
) -> Vec<Attachment> {
    list.into_iter()
        .map(|w| {
            let mut a = Attachment {
                name: w.name,
                media_type: w.media_type,
                size: w.size,
                content: AttachmentContent::NotRead {
                    reason: "it came without its content".into(),
                },
            };
            a.content = match (w.not_read, w.text, w.data) {
                (Some(reason), _, _) => AttachmentContent::NotRead {
                    reason: clean(&reason),
                },
                (None, Some(text), _) => {
                    let (kept, cut) = cut_to(&text, max_text);
                    AttachmentContent::Text {
                        text: kept.to_string(),
                        cut,
                    }
                }
                (None, None, Some(data)) => {
                    match crate::blobs::decode(&data)
                        .map_err(|_| "its data is not valid base64".to_string())
                        .and_then(|bytes| {
                            let n = bytes.len() as u64;
                            store_image(&bytes, blobs).map(|stored| (stored, n))
                        }) {
                        Ok(((info, digest), n)) => {
                            a.media_type = info.media_type.into();
                            a.size = n;
                            AttachmentContent::Image {
                                digest,
                                width: info.width,
                                height: info.height,
                            }
                        }
                        Err(reason) => AttachmentContent::NotRead { reason },
                    }
                }
                (None, None, None) => a.content,
            };
            a
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG header for the given size, then `pad` zero bytes. Nothing in
    /// Theseus decodes pixels, so a header is a PNG as far as it can tell.
    pub(crate) fn png(width: u32, height: u32, pad: usize) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        v.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        v.resize(v.len() + pad, 0);
        v
    }

    fn wire(
        name: &str,
        text: Option<&str>,
        not_read: Option<&str>,
    ) -> theseus_protocol::Attachment {
        theseus_protocol::Attachment {
            name: name.into(),
            media_type: "text/plain; charset=utf-8".into(),
            size: text.map_or(20 * 1024 * 1024, |t| t.len() as u64),
            text: text.map(str::to_string),
            data: None,
            not_read: not_read.map(str::to_string),
        }
    }

    fn image_wire(name: &str, bytes: &[u8]) -> theseus_protocol::Attachment {
        theseus_protocol::Attachment {
            name: name.into(),
            media_type: "image/png".into(),
            size: bytes.len() as u64,
            data: Some(crate::blobs::encode(bytes)),
            ..Default::default()
        }
    }

    #[test]
    fn text_is_cut_on_a_character_boundary_and_headers_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        // 'é' is two bytes, so a 5-byte cap cannot keep the third one whole.
        let got = from_wire(vec![wire("a.txt", Some("ééé"), None)], 5, &blobs);
        assert_eq!(
            got[0].content,
            AttachmentContent::Text {
                text: "éé".into(),
                cut: true
            }
        );
        assert_eq!(
            header(&got[0], Some("discord:eddie")),
            "[Attachment a.txt from discord:eddie, 6 bytes; cut to its first 4 bytes]"
        );
        let whole = from_wire(
            vec![wire("message.txt", Some(&"x".repeat(5_012)), None)],
            262_144,
            &blobs,
        );
        assert_eq!(
            header(&whole[0], Some("discord:eddie")),
            "[Attachment message.txt from discord:eddie, 5,012 bytes]"
        );
        assert!(
            for_model(&whole[0], None).starts_with("[Attachment message.txt, 5,012 bytes]\nxxx")
        );
    }

    #[test]
    fn a_file_not_read_is_listed_with_its_type_size_and_reason() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let mut w = wire(
            "dump.zip",
            None,
            Some("over the limit for text (262,144 bytes)"),
        );
        w.media_type = "application/zip".into();
        let got = from_wire(vec![w], 262_144, &blobs);
        assert_eq!(
            for_model(&got[0], Some("discord:eddie")),
            "[Attachment dump.zip from discord:eddie, application/zip, 20.0 MB: not read: over the limit for text (262,144 bytes)]"
        );
        assert_eq!(
            display_text("see attached", &got, Some("discord:eddie")),
            "see attached\n[Attachment dump.zip from discord:eddie, application/zip, 20.0 MB: not read: over the limit for text (262,144 bytes)]"
        );
        // A name cannot break the header's line.
        let odd = from_wire(vec![wire("a\nb].txt", Some("t"), None)], 10, &blobs);
        assert_eq!(header(&odd[0], None), "[Attachment a b].txt, 1 byte]");
    }

    #[test]
    fn an_image_is_stored_once_and_the_node_holds_its_digest() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let bytes = png(1280, 720, 1_000);
        let got = from_wire(
            vec![
                image_wire("shot.png", &bytes),
                image_wire("again.png", &bytes),
            ],
            262_144,
            &blobs,
        );
        let digest = crate::blobs::digest(&bytes);
        assert_eq!(
            got[0].content,
            AttachmentContent::Image {
                digest: digest.clone(),
                width: 1280,
                height: 720
            }
        );
        assert_eq!(got[1].content, got[0].content, "the same bytes, one blob");
        assert_eq!(std::fs::read_dir(blobs.dir()).unwrap().count(), 1);
        let stored = serde_json::to_string(&got[0]).unwrap();
        assert!(stored.len() < 300, "no bytes in the node: {stored}");
        assert_eq!(
            header(&got[0], Some("discord:eddie")),
            "[Image shot.png from discord:eddie, 1,033 bytes, 1280×720]"
        );

        // Rendered for a vision model: the header, then one image block, and
        // its tokens; for a model without vision, one line.
        let vision = Media {
            vision: true,
            model: "claude-haiku-4-5",
            blobs: Some(&blobs),
        };
        let mut tokens = 0;
        let b = blocks(&got[0], Some("discord:eddie"), &vision, &mut tokens);
        assert_eq!(b.len(), 2);
        assert_eq!(b[1]["type"], "image");
        assert_eq!(b[1]["source"]["media_type"], "image/png");
        assert_eq!(
            crate::blobs::decode(b[1]["source"]["data"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert!(tokens > 0 && tokens <= 1_568, "{tokens}");
        let again = blocks(&got[0], Some("discord:eddie"), &vision, &mut 0);
        assert_eq!(
            serde_json::to_vec(&again).unwrap(),
            serde_json::to_vec(&b).unwrap(),
            "the same node renders to the same bytes"
        );
        let blind = Media {
            vision: false,
            model: "glm-5.3",
            blobs: Some(&blobs),
        };
        let mut none = 0;
        assert_eq!(
            blocks(&got[0], Some("discord:eddie"), &blind, &mut none),
            vec![
                json!({"type": "text", "text": "[Image shot.png from discord:eddie, 1,033 bytes: not shown, this model has no vision]"})
            ]
        );
        assert_eq!(none, 0);
    }

    #[test]
    fn an_image_the_provider_would_refuse_is_listed_with_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let big = png(4000, 3000, 6 * 1024 * 1024);
        let mut garbled = image_wire("x.png", b"x");
        garbled.data = Some("not base64!".into());
        let got = from_wire(
            vec![
                image_wire("big.png", &big),
                image_wire("zip.png", b"PK\x03\x04 a zip in disguise"),
                garbled,
            ],
            262_144,
            &blobs,
        );
        let reasons: Vec<String> = got
            .iter()
            .map(|a| match &a.content {
                AttachmentContent::NotRead { reason } => reason.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            reasons,
            vec![
                "an image over the 5 MiB limit",
                "not an image the models read (PNG, JPEG, GIF, or WebP)",
                "its data is not valid base64",
            ]
        );
        assert!(!blobs.dir().exists(), "nothing refused was stored");
        assert_eq!(
            header(&got[0], None),
            "[Attachment big.png, image/png, 6.0 MB: not read: an image over the 5 MiB limit]"
        );
    }
}

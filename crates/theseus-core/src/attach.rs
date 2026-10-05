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
//! rendering depends on the node, the model, the blob's bytes, and the
//! session's list of images the provider refused, so the same node renders
//! to the same bytes every time until the provider refuses its image.
//!
//! A node is written once, so an image the provider rejects (corrupt pixel
//! data behind a valid header, a limit the sniffer does not check) would
//! fail every later request of its session the same way. A 400 that names
//! an image marks it not shown in the session record (theseus-0s4,
//! `refused`), and it renders as its line from then on.
//!
//! Any other file, up to `[tools] max_attachment_bytes`, is kept whole in
//! the blobs too (theseus-c9l6), a `File`. A PDF is read when it arrives,
//! once, in a capped child (`theseus_files::convert`): its pages counted,
//! its text page by page, and, when one request cannot carry it whole, its
//! first pages as PDFs of their own, every one a blob the node names. A
//! model that reads PDFs gets a `document` block (the whole file, or the
//! largest part the request still has room for, with a line that says what
//! was left out); any other model, and a PDF the provider refused, its text
//! by page. A request's PDFs are budgeted in the order they render, so a
//! later file never changes how an earlier one renders, and the prompt's
//! cached prefix holds.

use serde_json::{json, Value};
use theseus_files::kind::Kind;
use theseus_files::pdf;
use theseus_tools::image;

use crate::blobs::Blobs;
use crate::narrative;
use crate::node::{Attachment, AttachmentContent, FilePart};
use crate::session::NotShown;

/// The pages one request may carry, from the claude-api reference's
/// "Document & File Input" (read 2026-10-04): 600, and 100 on a model with a
/// window of 200K tokens. A PDF over one of them keeps its first pages as a
/// part of their own for it.
pub const PAGE_LIMITS: [u32; 2] = [100, 600];

/// The PDF bytes one request carries, all its documents together. The same
/// reference caps a request at 32 MB; base64 makes these 24 MB of it, which
/// leaves room for the images and the text.
pub const DOCUMENT_BUDGET_BYTES: u64 = 18 * 1024 * 1024;

/// The text of one PDF a model reads when it does not read the PDF itself.
pub const MAX_DOCUMENT_TEXT: usize = 256 * 1024;

/// The most pages a request carries for a model with this window.
pub fn page_limit(context_window: u64) -> u32 {
    if context_window <= 200_000 {
        PAGE_LIMITS[0]
    } else {
        PAGE_LIMITS[1]
    }
}

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
        AttachmentContent::File { pages, .. } if is_pdf(a) => match pages {
            Some(n) => format!("[PDF {name}{from}, {size}, {}]", pdf::count(*n)),
            None => format!("[PDF {name}{from}, {size}]"),
        },
        AttachmentContent::File { unread, .. } => {
            let kind = Kind::of_media_type(&a.media_type);
            let typed = if a.media_type.is_empty() {
                String::new()
            } else {
                format!(", {}", clean(&a.media_type))
            };
            match (kind, unread) {
                (_, Some(why)) => format!(
                    "[File {name}{from}{typed}, {size}: kept, not read: {why}; file_read with save puts it where proc_run can use it]"
                ),
                (Kind::Audio, None) => format!(
                    "[Audio {name}{from}{typed}, {size}: not transcribed yet; file_read gives its transcript (Deepgram, about $0.26 an hour of audio)]"
                ),
                (Kind::Video, None) => format!(
                    "[Video {name}{from}{typed}, {size}: not read yet; file_read gives its transcript and a strip of its frames]"
                ),
                (k, None) if k.has_text() => format!("[{} {name}{from}, {size}]", k.noun()),
                (_, None) => format!(
                    "[File {name}{from}{typed}, {size}: kept, not read; file_read with save puts it where proc_run can use it]"
                ),
            }
        }
    }
}

fn is_pdf(a: &Attachment) -> bool {
    a.media_type == PDF_TYPE
}

const PDF_TYPE: &str = "application/pdf";

/// The one line an image becomes when it is not shown.
fn not_shown(a: &Attachment, author: Option<&str>, why: &str) -> String {
    format!(
        "[Image {}{}, {}: not shown, {why}]",
        clean(&a.name),
        from_words(author),
        size_words(a.size)
    )
}

/// How a request shows images and PDFs: whether its model has vision and
/// reads PDFs, how many PDF pages one of its requests carries, which model
/// (for the token estimate), where the bytes are, and which images and PDFs
/// the provider refused in this session (theseus-0s4), which show as their
/// line or their text.
pub struct Media<'a> {
    pub vision: bool,
    /// The model reads a PDF as a `document` block (theseus-c9l6).
    pub pdf: bool,
    /// The most PDF pages one request carries ([`page_limit`]).
    pub pdf_pages: u32,
    pub model: &'a str,
    /// A refusal's fallback the request goes to (theseus-7gir.18), whose
    /// thinking goes back too.
    pub also: Option<&'a str>,
    pub blobs: Option<&'a Blobs>,
    pub hidden: &'a [NotShown],
}

impl Media<'_> {
    /// For a request that carries no images (tests, probes).
    pub fn none() -> Media<'static> {
        Media {
            vision: false,
            pdf: false,
            pdf_pages: 0,
            model: "",
            also: None,
            blobs: None,
            hidden: &[],
        }
    }
}

/// What a request's images and PDFs have used so far, in the order they
/// render: their estimated tokens, and the pages and bytes of the PDFs it
/// carries (theseus-c9l6). A PDF that no longer fits renders as its text.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Spend {
    pub tokens: u64,
    pub pages: u32,
    pub bytes: u64,
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
    if let Some(h) = media.hidden.iter().find(|h| h.digest == *digest) {
        let why = format!("the provider refused it ({})", h.why);
        return Shown::Line(not_shown(a, author, &why));
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
/// text; `spend` gains what its images and PDFs are estimated to cost.
pub fn blocks(
    a: &Attachment,
    author: Option<&str>,
    media: &Media,
    spend: &mut Spend,
) -> Vec<Value> {
    let text = |t: String| json!({"type": "text", "text": t});
    match &a.content {
        AttachmentContent::Text { text: body, .. } => {
            vec![text(format!("{}\n{body}", header(a, author)))]
        }
        AttachmentContent::NotRead { .. } => vec![text(header(a, author))],
        AttachmentContent::Image { .. } => match show(a, author, media) {
            Shown::Block(img, t) => {
                spend.tokens += t;
                vec![text(header(a, author)), img]
            }
            Shown::Line(line) => vec![text(line)],
        },
        AttachmentContent::File { .. } if is_pdf(a) => match document(a, author, media, spend) {
            (line, Some(block)) => vec![text(line), block],
            (line, None) => vec![text(line)],
        },
        AttachmentContent::File { .. } => doc_blocks(a, author, media, spend),
    }
}

/// The most images of one document's sections a request shows (a
/// notebook's outputs); the rest are named.
pub const MAX_DOC_IMAGES: usize = 8;

/// A kept file that is not a PDF: a document's line and its text by section
/// (every model reads it), then its sections' images for a model with
/// vision; a recording, a video, an archive's member, or any other file is
/// its line, which says how to read it.
fn doc_blocks(
    a: &Attachment,
    author: Option<&str>,
    media: &Media,
    spend: &mut Spend,
) -> Vec<Value> {
    let line = header(a, author);
    let AttachmentContent::File { text, unread, .. } = &a.content else {
        return vec![json!({"type": "text", "text": line})];
    };
    let kind = Kind::of_media_type(&a.media_type);
    if !kind.has_text() || unread.is_some() {
        return vec![json!({"type": "text", "text": line})];
    }
    let Some(texts) = text
        .as_deref()
        .and_then(|d| media.blobs.and_then(|b| b.texts(d)))
    else {
        let head = line.trim_end_matches(']');
        return vec![json!({"type": "text", "text": format!("{head}: its text is missing]")})];
    };
    let mut out = line;
    let mut images: Vec<&crate::blobs::SectionImage> = Vec::new();
    let mut named = 0usize;
    for (i, s) in texts.iter().enumerate() {
        let piece = format!("\n--- {} ---\n{}", s.label, s.text);
        if out.len() + piece.len() > MAX_DOCUMENT_TEXT {
            let (kept, _) = cut_to(&piece, MAX_DOCUMENT_TEXT.saturating_sub(out.len()));
            out.push_str(kept);
            out.push_str(&format!(
                "\n[cut at {} of its text, in section {} of {}: file_read with pages reads on]",
                narrative::bytes(MAX_DOCUMENT_TEXT as u64),
                i + 1,
                texts.len()
            ));
            break;
        }
        out.push_str(&piece);
        for img in &s.images {
            if media.vision && images.len() < MAX_DOC_IMAGES {
                images.push(img);
            } else {
                named += 1;
            }
        }
    }
    if named > 0 {
        out.push_str(&format!(
            "\n[{} of its images are not shown{}]",
            named,
            if media.vision {
                ""
            } else {
                ": this model has no vision"
            }
        ));
    }
    let mut blocks = vec![json!({"type": "text", "text": out})];
    for img in images {
        if let Some(data) = media.blobs.and_then(|b| b.base64(&img.digest)) {
            spend.tokens += crate::catalog::image_tokens(media.model, img.width, img.height);
            blocks.push(json!({"type": "image", "source": {"type": "base64", "media_type": img.media_type, "data": &*data}}));
        }
    }
    blocks
}

/// A `tool_result`'s content when its tool returned an image or a PDF's
/// pages: the text, then the image or document block for a model that reads
/// it; the text and the not-shown line, or the pages' text, for any other.
pub fn tool_content(content: &str, file: &Attachment, media: &Media, spend: &mut Spend) -> Value {
    if let AttachmentContent::File { .. } = file.content {
        if !is_pdf(file) {
            return Value::String(format!("{content}\n{}", header(file, None)));
        }
        return match document(file, None, media, spend) {
            (line, Some(block)) => {
                json!([{"type": "text", "text": format!("{content}\n{line}")}, block])
            }
            (line, None) => Value::String(format!("{content}\n{line}")),
        };
    }
    match show(file, None, media) {
        Shown::Block(block, t) => {
            spend.tokens += t;
            json!([{"type": "text", "text": content}, block])
        }
        Shown::Line(line) => Value::String(format!("{content}\n{line}")),
    }
}

/// What a kept file renders as: its line, and a `document` block when the
/// model reads PDFs, the provider has not refused it, and the request still
/// has room for it or for one of its parts; otherwise its line and its text
/// by page. The choice depends only on the node, the model, its blobs, and
/// what the PDFs before it in the request used, so a node renders the same
/// bytes in every request of its session.
fn document(
    a: &Attachment,
    author: Option<&str>,
    media: &Media,
    spend: &mut Spend,
) -> (String, Option<Value>) {
    let AttachmentContent::File {
        digest,
        pages,
        text,
        parts,
        unread,
    } = &a.content
    else {
        return (header(a, author), None);
    };
    let line = header(a, author);
    if !is_pdf(a) {
        return (line, None);
    }
    let refused = media
        .hidden
        .iter()
        .find(|h| h.digest == *digest || parts.iter().any(|p| p.digest == h.digest));
    let why_text = match (media.pdf, refused) {
        (false, _) => "this model reads no PDFs".to_string(),
        (true, Some(h)) => format!("the provider refused it as a document ({})", h.why),
        (true, None) => match fitting(a.size, *pages, parts, media, spend) {
            Fit::None => format!(
                "this request has no room left for it as a document ({} pages and {} a request)",
                media.pdf_pages,
                narrative::bytes(DOCUMENT_BUDGET_BYTES)
            ),
            fit => match shown(digest, *pages, parts, fit, media, spend, text.as_deref()) {
                Some((block, said)) => {
                    let line = match said {
                        Some(s) => format!("{}; {s}]", line.trim_end_matches(']')),
                        None => line,
                    };
                    return (line, Some(block));
                }
                None => "its stored bytes are missing".to_string(),
            },
        },
    };
    (
        as_text(
            &line,
            &why_text,
            *pages,
            text.as_deref(),
            unread.as_deref(),
            media,
        ),
        None,
    )
}

/// Which of a PDF's forms a request still has room for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fit {
    Whole,
    /// The part at this index.
    Part(usize),
    None,
}

fn fitting(size: u64, pages: Option<u32>, parts: &[FilePart], media: &Media, spend: &Spend) -> Fit {
    let room = |n: u32, bytes: u64| {
        spend.pages + n <= media.pdf_pages && spend.bytes + bytes <= DOCUMENT_BUDGET_BYTES
    };
    // A PDF whose pages could not be counted is sent by its bytes alone; the
    // provider says if it is too long, and it shows as its text from then on.
    if room(pages.unwrap_or(0), size) {
        return Fit::Whole;
    }
    match parts.iter().rposition(|p| room(p.last, p.bytes)) {
        Some(i) => Fit::Part(i),
        None => Fit::None,
    }
}

/// The document block for the form that fits, its pages and bytes added to
/// `spend`, and the words for what a part left out; `None` when its bytes
/// are missing.
fn shown(
    digest: &str,
    pages: Option<u32>,
    parts: &[FilePart],
    fit: Fit,
    media: &Media,
    spend: &mut Spend,
    text: Option<&str>,
) -> Option<(Value, Option<String>)> {
    let (sent, n, bytes, said) = match fit {
        Fit::Whole => (digest, pages.unwrap_or(0), None, None),
        Fit::Part(i) => {
            let p = &parts[i];
            let total = pages.unwrap_or(p.last);
            let said = format!(
                "pages 1–{} shown, as many as one request carries for this model; pages {}–{total} left out",
                p.last,
                p.last + 1
            );
            (p.digest.as_str(), p.last, Some(p.bytes), Some(said))
        }
        Fit::None => return None,
    };
    let data = media.blobs.and_then(|b| b.base64(sent))?;
    let bytes = bytes.unwrap_or(crate::blobs::decoded_len(&data));
    spend.pages += n;
    spend.bytes += bytes;
    let text_bytes = text
        .and_then(|d| media.blobs.and_then(|b| b.texts(d)))
        .map_or(0, |t| {
            t.iter()
                .take(n.max(1) as usize)
                .map(|s| s.text.len())
                .sum::<usize>()
        });
    spend.tokens += crate::catalog::pdf_tokens(media.model, n.max(1), text_bytes as u64);
    let block = json!({"type": "document", "source": {"type": "base64", "media_type": PDF_TYPE, "data": &*data}});
    Some((block, said))
}

/// A PDF as its text, page by page, under its line, which says why it is
/// not shown as a document; cut at [`MAX_DOCUMENT_TEXT`] with a line that
/// says where.
fn as_text(
    line: &str,
    why: &str,
    pages: Option<u32>,
    text: Option<&str>,
    unread: Option<&str>,
    media: &Media,
) -> String {
    let head = line.trim_end_matches(']');
    let texts = text.and_then(|d| media.blobs.and_then(|b| b.texts(d)));
    let Some(texts) = texts else {
        let not = match unread {
            Some(u) => format!("its text could not be read ({u})"),
            None => "its text is missing".to_string(),
        };
        return format!("{head}: not shown, {why}; {not}]");
    };
    let mut out = format!(
        "{head}: its text, page by page, since {why}; a page's pictures, charts, and scanned text are not in it]"
    );
    let total = pages.unwrap_or(texts.len() as u32);
    for (i, t) in texts.iter().enumerate() {
        let page = if t.text.trim().is_empty() {
            format!("\n--- page {} ---\n(no text on this page)", i + 1)
        } else {
            format!("\n--- page {} ---\n{}", i + 1, t.text)
        };
        if out.len() + page.len() > MAX_DOCUMENT_TEXT {
            let (kept, _) = cut_to(&page, MAX_DOCUMENT_TEXT.saturating_sub(out.len()));
            out.push_str(kept);
            out.push_str(&format!(
                "\n[cut at {} of its text, in page {} of {total}]",
                narrative::bytes(MAX_DOCUMENT_TEXT as u64),
                i + 1
            ));
            return out;
        }
        out.push_str(&page);
    }
    if (texts.len() as u32) < total {
        out.push_str(&format!(
            "\n[the text stops at page {} of {total}: the rest was over what one read keeps]",
            texts.len()
        ));
    }
    out
}

/// An image a provider's 400 named (theseus-0s4): its blob's digest, the
/// index of the request message it sat in, and what the provider said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub digest: String,
    pub message: usize,
    pub why: String,
}

/// The images a provider's 400 names in the request it refused, by the path
/// of their block: `messages.3.content.1.image.source.base64.data`, or, in a
/// tool result, `messages.3.content.0.content.1…`; a tool result named whole
/// gives its images. An error that names an image but no block (Anthropic's
/// own "Could not process image" names none) means the images the model has
/// not answered over yet, those after its last answer: every request it
/// answered carried the ones before. With no new image, it means the
/// request's only one, and nothing when it carries several. Each image is
/// known by its bytes, whose digest names its blob.
pub fn refused(messages: &[Value], error: &str) -> Vec<Refused> {
    let mut named: Vec<(usize, &Value)> = Vec::new();
    for path in block_paths(error) {
        let block = messages
            .get(path[0])
            .and_then(|m| m["content"].get(path[1]))
            .and_then(|b| match path.get(2) {
                Some(&k) => b["content"].get(k),
                None => Some(b),
            });
        if let Some(b) = block {
            named.extend(images_of(b).into_iter().map(|img| (path[0], img)));
        }
    }
    let mut unsure = false;
    let lower = error.to_ascii_lowercase();
    // A PDF the provider could not read is named like an image is
    // (theseus-c9l6): by its block's path, or by the word alone.
    let words = |b: &Value| match b["type"].as_str() {
        Some("document") => lower.contains("pdf") || lower.contains("document"),
        _ => lower.contains("image"),
    };
    if named.is_empty()
        && (lower.contains("image") || lower.contains("pdf") || lower.contains("document"))
    {
        let all: Vec<(usize, &Value)> = messages
            .iter()
            .enumerate()
            .flat_map(|(i, m)| {
                m["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(images_of)
                    .filter(|b| words(b))
                    .map(move |img| (i, img))
            })
            .collect();
        let answered = messages.iter().rposition(|m| m["role"] == "assistant");
        let new: Vec<(usize, &Value)> = all
            .iter()
            .copied()
            .filter(|&(i, _)| answered.is_none_or(|a| i > a))
            .collect();
        named = match (new.len(), all.len()) {
            (0, 1) => all,
            (0, _) => Vec::new(),
            _ => {
                unsure = true;
                new
            }
        };
    }
    let words = refusal_words(error);
    let mut out: Vec<Refused> = Vec::new();
    for (message, img) in named {
        let Some(digest) = digest_of(img) else {
            continue;
        };
        if !out.iter().any(|r| r.digest == digest) {
            out.push(Refused {
                digest,
                message,
                why: words.clone(),
            });
        }
    }
    // Several new images and no word of which: all of them go, and each
    // line says so.
    let n = out.len();
    if unsure && n > 1 {
        for r in &mut out {
            r.why = format!("{words}; it did not say which of the {n} new images");
        }
    }
    out
}

/// The digests of the images a request message carries, in its own blocks
/// and in its tool results (theseus-0s4): a mark hides every copy of an
/// image, not only the one a 400 named.
pub fn digests_in(message: &Value) -> Vec<String> {
    message["content"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(images_of)
        .filter_map(digest_of)
        .collect()
}

/// Image blocks and document blocks in `messages`, those inside tool results
/// included.
pub fn media_in(messages: &[Value]) -> (u64, u64) {
    fn count(blocks: &Value) -> (u64, u64) {
        blocks.as_array().map_or((0, 0), |bs| {
            bs.iter()
                .map(|b| match b.get("type").and_then(Value::as_str) {
                    Some("image") => (1, 0),
                    Some("document") => (0, 1),
                    Some("tool_result") => count(&b["content"]),
                    _ => (0, 0),
                })
                .fold((0, 0), |(i, d), (a, b)| (i + a, d + b))
        })
    }
    messages
        .iter()
        .map(|m| count(&m["content"]))
        .fold((0, 0), |(i, d), (a, b)| (i + a, d + b))
}

/// An image block's blob digest, from its bytes.
fn digest_of(img: &Value) -> Option<String> {
    img["source"]["data"]
        .as_str()
        .and_then(|d| crate::blobs::decode(d).ok())
        .map(|bytes| crate::blobs::digest(&bytes))
}

/// A block's images and PDFs (theseus-c9l6): itself when it is one, and a
/// tool result's.
fn images_of(b: &Value) -> Vec<&Value> {
    let media = |c: &Value| c["type"] == "image" || c["type"] == "document";
    match b["type"].as_str() {
        Some("image") | Some("document") => vec![b],
        Some("tool_result") => b["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| media(c))
            .collect(),
        _ => vec![],
    }
}

/// The block paths an error names: `messages.<i>.content.<j>`, and in a tool
/// result `….content.<k>`, as indices.
fn block_paths(error: &str) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    let mut rest = error;
    while let Some(at) = rest.find("messages.") {
        rest = &rest[at + "messages.".len()..];
        let mut path = Vec::new();
        let mut s = rest;
        loop {
            let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                break;
            }
            path.push(digits.parse().unwrap_or(usize::MAX));
            s = &s[digits.len()..];
            match s.strip_prefix(".content.") {
                Some(more) if path.len() < 3 => s = more,
                _ => break,
            }
        }
        if path.len() >= 2 {
            out.push(path);
        }
    }
    out
}

/// What the provider said of an image, without its block's path: the words
/// after it, on one line, bounded.
fn refusal_words(error: &str) -> String {
    let words = match error.rfind("messages.") {
        Some(at) => error[at..]
            .split_once(": ")
            .map_or(&error[at..], |(_, w)| w),
        None => error
            .split_once("invalid_request_error: ")
            .map_or(error, |(_, w)| w),
    };
    match clean(words) {
        w if w.is_empty() => "it was rejected".into(),
        w => w,
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

/// What a message's files may hold (theseus-c9l6): the text kept of a text
/// file (`[tools] max_read_bytes`), and the bytes of any other file kept
/// whole (`[tools] max_attachment_bytes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub max_text: usize,
    pub max_file: u64,
}

/// One file read for a model (theseus-c9l6): an attachment kept when it
/// arrived, or a PDF a tool read, for its `file.read` row.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRead {
    /// `attachment`, `fs.read`, or `http.fetch`.
    pub via: &'static str,
    pub name: String,
    pub media_type: String,
    pub bytes: u64,
    pub digest: String,
    pub pages: Option<u32>,
    /// Its first pages, kept as parts of their own.
    pub parts: usize,
    pub text_bytes: u64,
    /// Why its text was not read, when it was not.
    pub why: Option<String>,
    /// The conversion's time, and whether it ran in the capped child.
    pub ms: u64,
    pub capped: bool,
}

impl FileRead {
    /// `read`, `kept` (a recording, a video, or a type not read), or `unread`.
    pub fn outcome(&self) -> &'static str {
        match (&self.why, Kind::of_media_type(&self.media_type).has_text()) {
            (Some(_), _) => "unread",
            (None, true) => "read",
            (None, false) => "kept",
        }
    }
}

/// The node's attachments from the wire's, in order, and what was read of
/// each file kept. Nothing here fails a turn: whatever cannot be kept
/// becomes a `not_read` with the reason. A message with a file to keep
/// blocks on the disk and on the PDF's conversion: call it in a blocking
/// section.
pub fn from_wire(
    list: Vec<theseus_protocol::Attachment>,
    caps: Caps,
    blobs: &Blobs,
) -> (Vec<Attachment>, Vec<FileRead>) {
    let mut reads = Vec::new();
    let files = list
        .into_iter()
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
                    let (kept, cut) = cut_to(&text, caps.max_text);
                    AttachmentContent::Text {
                        text: kept.to_string(),
                        cut,
                    }
                }
                (None, None, Some(data)) => match crate::blobs::decode(&data) {
                    Err(_) => AttachmentContent::NotRead {
                        reason: "its data is not valid base64".into(),
                    },
                    Ok(bytes) => {
                        a.size = bytes.len() as u64;
                        let (content, read) = keep(&mut a, &bytes, caps.max_file, blobs);
                        reads.extend(read);
                        content
                    }
                },
                (None, None, None) => a.content,
            };
            a
        })
        .collect();
    (files, reads)
}

/// A file's bytes as the node keeps them: an image the models read as an
/// image, any other file up to `max_file` whole, a PDF read for each model.
/// Sets the attachment's type to the one its bytes say.
fn keep(
    a: &mut Attachment,
    bytes: &[u8],
    max_file: u64,
    blobs: &Blobs,
) -> (AttachmentContent, Option<FileRead>) {
    let refused = match image::sniff(bytes) {
        Some(_) => match store_image(bytes, blobs) {
            Ok((info, digest)) => {
                a.media_type = info.media_type.into();
                return (
                    AttachmentContent::Image {
                        digest,
                        width: info.width,
                        height: info.height,
                    },
                    None,
                );
            }
            Err(why) => Some(why),
        },
        None => None,
    };
    if bytes.len() as u64 > max_file {
        let reason = match refused {
            Some(why) => why,
            None => format!(
                "over the {} limit for files ([tools] max_attachment_bytes)",
                narrative::bytes(max_file)
            ),
        };
        return (AttachmentContent::NotRead { reason }, None);
    }
    let members = match bytes.starts_with(b"PK") {
        true => theseus_files::doc::zip_members(bytes),
        false => Vec::new(),
    };
    let kind = theseus_files::kind::sniff(bytes, &a.name, &members);
    match kind.media_type() {
        Some(t) => a.media_type = t.into(),
        None if matches!(kind, Kind::Audio | Kind::Video) => {
            if let Some(t) = theseus_files::kind::av_media_type(bytes, &a.name) {
                a.media_type = t.into();
            }
        }
        None => {}
    }
    match keep_file(bytes, blobs, "attachment", &a.name, &a.media_type) {
        Ok((mut content, read)) => {
            // An image the models would refuse is kept, and says why it
            // is not shown.
            if let (AttachmentContent::File { unread, .. }, Some(why)) = (&mut content, refused) {
                *unread = Some(why);
            }
            (content, Some(read))
        }
        Err(reason) => (AttachmentContent::NotRead { reason }, None),
    }
}

/// Keep a file whole in the blobs, and read it if it is a PDF: its pages,
/// its text page by page, and a part for each page limit it passes, each
/// part its first pages within [`DOCUMENT_BUDGET_BYTES`]. `Err` only when
/// it could not be stored; a PDF that could not be read is kept, and says
/// why.
pub fn keep_file(
    bytes: &[u8],
    blobs: &Blobs,
    via: &'static str,
    name: &str,
    media_type: &str,
) -> Result<(AttachmentContent, FileRead), String> {
    let digest = blobs
        .put(bytes)
        .map_err(|e| format!("it could not be stored ({e})"))?;
    let mut read = FileRead {
        via,
        name: clean(name),
        media_type: media_type.into(),
        bytes: bytes.len() as u64,
        digest: digest.clone(),
        pages: None,
        parts: 0,
        text_bytes: 0,
        why: None,
        ms: 0,
        capped: false,
    };
    let mut content = AttachmentContent::File {
        digest,
        pages: None,
        text: None,
        parts: Vec::new(),
        unread: None,
    };
    let kind = Kind::of_media_type(media_type);
    if kind != Kind::Pdf {
        if kind.has_text() {
            read_doc(bytes, kind, blobs, &mut content, &mut read);
        }
        return Ok((content, read));
    }
    let ask = pdf::Ask {
        text: true,
        ..pdf::Ask::default()
    };
    let (got, ran) = theseus_files::convert::pdf(bytes, &ask);
    (read.ms, read.capped) = (ran.ms, ran.capped);
    let AttachmentContent::File {
        pages,
        text,
        parts,
        unread,
        ..
    } = &mut content
    else {
        unreachable!("a file")
    };
    match got.and_then(|r| store_texts(&r, blobs).map(|t| (r, t))) {
        Err(why) => {
            read.why = Some(why.clone());
            *unread = Some(why);
        }
        Ok((r, (t, n))) => {
            (*pages, *text) = (Some(r.pages), Some(t));
            (read.pages, read.text_bytes) = (Some(r.pages), n);
            *parts = make_parts(bytes, r.pages, blobs, &mut read);
            read.parts = parts.len();
        }
    }
    Ok((content, read))
}

/// A document's sections, read in the capped child, as the blob a `File`
/// names, with its images stored as blobs of their own.
fn read_doc(
    bytes: &[u8],
    kind: Kind,
    blobs: &Blobs,
    content: &mut AttachmentContent,
    read: &mut FileRead,
) {
    let (got, ran) = theseus_files::convert::doc(bytes, kind);
    (read.ms, read.capped) = (ran.ms, ran.capped);
    let AttachmentContent::File { text, unread, .. } = content else {
        return;
    };
    let stored = got.map(|d| crate::blobs::Sections {
        sections: d
            .sections
            .into_iter()
            .map(|s| crate::blobs::Section {
                label: s.label,
                text: s.text,
                images: s
                    .images
                    .iter()
                    .filter_map(|p| {
                        let (info, digest) = store_image(&p.bytes, blobs).ok()?;
                        Some(crate::blobs::SectionImage {
                            digest,
                            media_type: info.media_type.into(),
                            width: info.width,
                            height: info.height,
                        })
                    })
                    .collect(),
            })
            .collect(),
        cut: d.cut,
    });
    let put = stored.and_then(|s| {
        let n = s.sections.iter().map(|x| x.text.len()).sum::<usize>() as u64;
        let json = serde_json::to_vec(&s).map_err(|e| e.to_string())?;
        blobs
            .put(&json)
            .map(|d| (d, n))
            .map_err(|e| format!("its text could not be stored ({e})"))
    });
    match put {
        Ok((d, n)) => {
            *text = Some(d);
            read.text_bytes = n;
        }
        Err(why) => {
            read.why = Some(why.clone());
            *unread = Some(why);
        }
    }
}

/// A read's texts, as the blob a `File` names, and their bytes.
fn store_texts(r: &pdf::Read, blobs: &Blobs) -> Result<(String, u64), String> {
    let json = serde_json::to_vec(&r.texts).map_err(|e| e.to_string())?;
    let n = r.texts.iter().map(String::len).sum::<usize>() as u64;
    blobs
        .put(&json)
        .map(|d| (d, n))
        .map_err(|e| format!("its text could not be stored ({e})"))
}

/// A part for each page limit a PDF passes, or for the byte budget: its
/// first pages, as many as the limit takes, then fewer until the part fits
/// [`DOCUMENT_BUDGET_BYTES`]. Fewest pages first; a part that could not be
/// made is left out, and the PDF shows as its text where it does not fit.
fn make_parts(bytes: &[u8], pages: u32, blobs: &Blobs, read: &mut FileRead) -> Vec<FilePart> {
    let mut out: Vec<FilePart> = Vec::new();
    let big = bytes.len() as u64 > DOCUMENT_BUDGET_BYTES;
    for limit in PAGE_LIMITS {
        if pages <= limit && !big {
            continue;
        }
        let mut last = pages.min(limit);
        for _ in 0..3 {
            if last == 0 || out.iter().any(|p| p.last == last) {
                break;
            }
            let ask = pdf::Ask {
                pages: Some(pdf::Pages::new(1, last)),
                part: true,
                text: false,
            };
            let (got, ran) = theseus_files::convert::pdf(bytes, &ask);
            read.ms += ran.ms;
            // A part of the whole file is the file itself.
            let part = match got {
                Ok(r) => r.part.unwrap_or_else(|| bytes.to_vec()),
                Err(_) => break,
            };
            let n = part.len() as u64;
            if n > DOCUMENT_BUDGET_BYTES {
                // Fewer pages, in proportion, and a tenth for slack.
                let fewer = (u64::from(last) * DOCUMENT_BUDGET_BYTES / n) * 9 / 10;
                last = (fewer as u32).min(last - 1);
                continue;
            }
            if let Ok(digest) = blobs.put(&part) {
                out.push(FilePart {
                    last,
                    digest,
                    bytes: n,
                });
            }
            break;
        }
    }
    out.sort_by_key(|p| p.last);
    out
}

/// A PDF a tool read (theseus-c9l6): the pages it read, as the result
/// node keeps them, a `File` whose bytes are those pages alone, with their
/// text. `name` says which pages of which file.
pub fn keep_pages(
    name: String,
    bytes: &[u8],
    read: &pdf::Read,
    via: &'static str,
    blobs: &Blobs,
) -> Result<(Attachment, FileRead), String> {
    let pages = read.range.map_or(read.pages, |r| r.count());
    let digest = blobs
        .put(bytes)
        .map_err(|e| format!("it could not be stored ({e})"))?;
    let (text, text_bytes) = match store_texts(read, blobs) {
        Ok((t, n)) => (Some(t), n),
        Err(_) => (None, 0),
    };
    let a = Attachment {
        name,
        media_type: PDF_TYPE.into(),
        size: bytes.len() as u64,
        content: AttachmentContent::File {
            digest: digest.clone(),
            pages: Some(pages),
            text,
            parts: Vec::new(),
            unread: None,
        },
    };
    let r = FileRead {
        via,
        name: a.name.clone(),
        media_type: PDF_TYPE.into(),
        bytes: a.size,
        digest,
        pages: Some(pages),
        parts: 0,
        text_bytes,
        why: None,
        ms: 0,
        capped: false,
    };
    Ok((a, r))
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
        let got = from_wire(vec![wire("a.txt", Some("ééé"), None)], caps(5), &blobs).0;
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
            caps(262_144),
            &blobs,
        )
        .0;
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
        let got = from_wire(vec![w], caps(262_144), &blobs).0;
        assert_eq!(
            for_model(&got[0], Some("discord:eddie")),
            "[Attachment dump.zip from discord:eddie, application/zip, 20.0 MB: not read: over the limit for text (262,144 bytes)]"
        );
        assert_eq!(
            display_text("see attached", &got, Some("discord:eddie")),
            "see attached\n[Attachment dump.zip from discord:eddie, application/zip, 20.0 MB: not read: over the limit for text (262,144 bytes)]"
        );
        // A name cannot break the header's line.
        let odd = from_wire(vec![wire("a\nb].txt", Some("t"), None)], caps(10), &blobs).0;
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
            caps(262_144),
            &blobs,
        )
        .0;
        let digest = crate::blobs::digest(&bytes);
        assert_eq!(
            got[0].content,
            AttachmentContent::Image {
                digest,
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
            pdf: true,
            pdf_pages: 100,
            model: "claude-haiku-4-5",
            also: None,
            blobs: Some(&blobs),
            hidden: &[],
        };
        let mut spend = Spend::default();
        let b = blocks(&got[0], Some("discord:eddie"), &vision, &mut spend);
        let tokens = spend.tokens;
        assert_eq!(b.len(), 2);
        assert_eq!(b[1]["type"], "image");
        assert_eq!(b[1]["source"]["media_type"], "image/png");
        assert_eq!(
            crate::blobs::decode(b[1]["source"]["data"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert!(tokens > 0 && tokens <= 1_568, "{tokens}");
        let again = blocks(
            &got[0],
            Some("discord:eddie"),
            &vision,
            &mut Spend::default(),
        );
        assert_eq!(
            serde_json::to_vec(&again).unwrap(),
            serde_json::to_vec(&b).unwrap(),
            "the same node renders to the same bytes"
        );
        let blind = Media {
            vision: false,
            pdf: false,
            pdf_pages: 0,
            model: "glm-5.3",
            also: None,
            blobs: Some(&blobs),
            hidden: &[],
        };
        let mut none = Spend::default();
        assert_eq!(
            blocks(&got[0], Some("discord:eddie"), &blind, &mut none),
            vec![
                json!({"type": "text", "text": "[Image shot.png from discord:eddie, 1,033 bytes: not shown, this model has no vision]"})
            ]
        );
        assert_eq!(none, Spend::default());
        // An image the provider refused in this session (theseus-0s4): its
        // line, for a vision model too, and no tokens.
        let refused = [NotShown {
            digest: crate::blobs::digest(&bytes),
            why: "Could not process image".into(),
            at_ms: 1,
        }];
        let hiding = Media {
            hidden: &refused,
            ..vision
        };
        let mut none = Spend::default();
        assert_eq!(
            blocks(&got[0], Some("discord:eddie"), &hiding, &mut none),
            vec![
                json!({"type": "text", "text": "[Image shot.png from discord:eddie, 1,033 bytes: not shown, the provider refused it (Could not process image)]"})
            ]
        );
        assert_eq!(none, Spend::default());
    }

    /// A request's messages: a user message with a header, an image, and
    /// text; the model's answer; and a user message with a tool result that
    /// carries a second image.
    fn refused_request(first: &[u8], second: &[u8]) -> Vec<Value> {
        let img = |b: &[u8]| json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": crate::blobs::encode(b)}});
        vec![
            json!({"role": "user", "content": [{"type": "text", "text": "[Image a.png]"}, img(first), {"type": "text", "text": "what is this?"}]}),
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "b.png"}}]}),
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "b.png"}, img(second)]}]}),
        ]
    }

    /// theseus-0s4: a 400 names an image by its block's path, in a message or
    /// in a tool result; the parser finds its blob by its bytes.
    #[test]
    fn a_400_that_names_an_images_block_finds_its_blob() {
        let (a, b) = (png(64, 64, 10), png(32, 32, 20));
        let msgs = refused_request(&a, &b);
        let got = refused(
            &msgs,
            "invalid_request_error: messages.0.content.1.image.source.base64.data: Could not process image",
        );
        assert_eq!(
            got,
            [Refused {
                digest: crate::blobs::digest(&a),
                message: 0,
                why: "Could not process image".into()
            }]
        );
        let inner = refused(
            &msgs,
            "invalid_request_error: messages.2.content.0.content.1.image.source.base64: Could not process image",
        );
        assert_eq!(inner.len(), 1);
        assert_eq!(
            (inner[0].digest.clone(), inner[0].message),
            (crate::blobs::digest(&b), 2)
        );
        // A tool result named whole gives its image.
        let whole = refused(&msgs, "messages.2.content.0: an image in it is too large");
        assert_eq!(whole[0].digest, crate::blobs::digest(&b));
        // A path to a block that is not an image, and an error that names no
        // image at all: nothing.
        assert!(refused(&msgs, "messages.0.content.2: text too long").is_empty());
        assert!(refused(&msgs, "invalid_request_error: max_tokens: too large").is_empty());
    }

    /// theseus-0s4: an error that names an image but no block means the
    /// request's only image, and nothing when it carries two.
    #[test]
    fn a_400_that_names_no_block_means_the_images_since_the_last_answer() {
        // Anthropic's own words for a PNG it could not decode (fb1b's live
        // check, 2026-10-01): no block path.
        const NO_PATH: &str = "invalid_request_error: Could not process image";
        let (a, b) = (png(64, 64, 10), png(32, 32, 20));
        // The only image, before any answer: it.
        let one = vec![refused_request(&a, &a)[0].clone()];
        let got = refused(&one, NO_PATH);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].digest, crate::blobs::digest(&a));
        assert_eq!(got[0].why, "Could not process image");
        // One image the model answered over, and a tool result's after it:
        // the new one alone.
        let two = refused_request(&a, &b);
        let got = refused(&two, NO_PATH);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(
            (got[0].digest.clone(), got[0].message),
            (crate::blobs::digest(&b), 2)
        );
        assert_eq!(got[0].why, "Could not process image");
        // Two new images in one message: both, and each line says so.
        let img = |x: &[u8]| json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": crate::blobs::encode(x)}});
        let first = vec![
            json!({"role": "user", "content": [img(&a), img(&b), {"type": "text", "text": "which?"}]}),
        ];
        let got = refused(&first, NO_PATH);
        assert_eq!(got.len(), 2, "{got:?}");
        for r in &got {
            assert_eq!(
                r.why,
                "Could not process image; it did not say which of the 2 new images"
            );
        }
        // Both answered over, and none new: nothing.
        let mut old = refused_request(&a, &b);
        old.push(
            json!({"role": "assistant", "content": [{"type": "text", "text": "Two charts."}]}),
        );
        old.push(json!({"role": "user", "content": [{"type": "text", "text": "Again?"}]}));
        assert!(refused(&old, NO_PATH).is_empty());
    }

    #[test]
    fn an_image_the_provider_would_refuse_is_kept_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let big = png(4000, 3000, 6 * 1024 * 1024);
        let mut garbled = image_wire("x.png", b"x");
        garbled.data = Some("not base64!".into());
        let (got, reads) = from_wire(
            vec![
                image_wire("big.png", &big),
                image_wire("zip.png", b"PK\x03\x04 a zip in disguise"),
                garbled,
            ],
            caps(262_144),
            &blobs,
        );
        // Since theseus-c9l6 a file the models do not read is kept whole,
        // and its line says why it is not read.
        assert_eq!(
            header(&got[0], None),
            "[File big.png, image/png, 6.0 MB: kept, not read: an image over the 5 MiB limit; file_read with save puts it where proc_run can use it]"
        );
        // A zip that is not one is kept, and says why it was not read.
        assert!(
            header(&got[1], None).starts_with(
                "[File zip.png, application/zip, 22 bytes: kept, not read: it could not be read as a zip"
            ),
            "{}",
            header(&got[1], None)
        );
        assert_eq!(
            got[2].content,
            AttachmentContent::NotRead {
                reason: "its data is not valid base64".into()
            }
        );
        assert_eq!(
            reads.iter().map(FileRead::outcome).collect::<Vec<_>>(),
            vec!["kept", "unread"]
        );
        assert_eq!(std::fs::read_dir(blobs.dir()).unwrap().count(), 2);
        // A file over the cap is listed, never stored.
        let tiny = Caps {
            max_text: 262_144,
            max_file: 10,
        };
        let (over, _) = from_wire(
            vec![image_wire("zip.png", b"PK\x03\x04 a zip in disguise")],
            tiny,
            &blobs,
        );
        assert_eq!(
            header(&over[0], None),
            "[Attachment zip.png, image/png, 22 bytes: not read: over the 10 bytes limit for files ([tools] max_attachment_bytes)]"
        );
    }

    pub(crate) fn caps(max_text: usize) -> Caps {
        Caps {
            max_text,
            max_file: theseus_files::MAX_FILE_BYTES,
        }
    }

    fn pdf_wire(name: &str, bytes: &[u8]) -> theseus_protocol::Attachment {
        theseus_protocol::Attachment {
            name: name.into(),
            media_type: "application/pdf".into(),
            size: bytes.len() as u64,
            data: Some(crate::blobs::encode(bytes)),
            ..Default::default()
        }
    }

    /// A model that reads PDFs (theseus-c9l6), with a request's page limit.
    fn claude<'a>(blobs: &'a Blobs, pages: u32, hidden: &'a [NotShown]) -> Media<'a> {
        Media {
            vision: true,
            pdf: true,
            pdf_pages: pages,
            model: "claude-sonnet-5-5",
            also: None,
            blobs: Some(blobs),
            hidden,
        }
    }

    fn glm(blobs: &Blobs) -> Media<'_> {
        Media {
            vision: false,
            pdf: false,
            pdf_pages: 0,
            model: "glm-5.3",
            also: None,
            blobs: Some(blobs),
            hidden: &[],
        }
    }

    fn harbour() -> Vec<u8> {
        theseus_files::pdf::sample(&[
            "The harbour master is Odile Varnack.",
            "Buoy B-2 is green.",
            "",
        ])
    }

    /// Eddie's case (theseus-c9l6): an attached PDF is kept whole, read
    /// once, and shown to a model that reads PDFs as a document block of its
    /// own bytes; the same node renders the same bytes every time.
    #[test]
    fn a_pdf_is_kept_read_and_shown_as_a_document() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let bytes = harbour();
        let (got, reads) = from_wire(vec![pdf_wire("orders.pdf", &bytes)], caps(262_144), &blobs);
        let AttachmentContent::File {
            digest,
            pages,
            text,
            parts,
            unread,
        } = &got[0].content
        else {
            panic!("{:?}", got[0].content)
        };
        assert_eq!(digest, &crate::blobs::digest(&bytes));
        assert_eq!((*pages, parts.len(), unread.as_deref()), (Some(3), 0, None));
        let texts = blobs.texts(text.as_deref().unwrap()).unwrap();
        assert_eq!(
            texts.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            vec![
                "The harbour master is Odile Varnack.",
                "Buoy B-2 is green.",
                ""
            ]
        );
        assert_eq!(texts[1].label, "page 2");
        assert_eq!(reads.len(), 1);
        assert_eq!(
            (reads[0].via, reads[0].outcome(), reads[0].pages),
            ("attachment", "read", Some(3))
        );
        let stored = serde_json::to_string(&got[0]).unwrap();
        assert!(stored.len() < 400, "no bytes in the node: {stored}");

        let media = claude(&blobs, 600, &[]);
        let mut spend = Spend::default();
        let b = blocks(&got[0], Some("discord:eddie"), &media, &mut spend);
        assert_eq!(b.len(), 2, "{b:?}");
        let size = bytes.len();
        assert_eq!(
            b[0]["text"],
            format!("[PDF orders.pdf from discord:eddie, {size} bytes, 3 pages]")
        );
        assert_eq!(b[1]["type"], "document");
        assert_eq!(b[1]["source"]["media_type"], "application/pdf");
        assert_eq!(
            crate::blobs::decode(b[1]["source"]["data"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert_eq!((spend.pages, spend.bytes), (3, size as u64));
        let text_bytes = "The harbour master is Odile Varnack.Buoy B-2 is green.".len() as u64;
        assert_eq!(
            spend.tokens,
            crate::catalog::pdf_tokens("claude-sonnet-5-5", 3, text_bytes)
        );
        let again = blocks(
            &got[0],
            Some("discord:eddie"),
            &media,
            &mut Spend::default(),
        );
        assert_eq!(
            serde_json::to_vec(&again).unwrap(),
            serde_json::to_vec(&b).unwrap(),
            "the same node renders to the same bytes"
        );
    }

    /// A model that reads no PDFs (GLM) reads the PDF's text, page by page,
    /// and is told what the text leaves out.
    #[test]
    fn a_pdf_is_its_text_by_page_for_a_model_that_reads_no_pdfs() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let bytes = harbour();
        let (got, _) = from_wire(vec![pdf_wire("orders.pdf", &bytes)], caps(262_144), &blobs);
        let mut spend = Spend::default();
        let b = blocks(&got[0], None, &glm(&blobs), &mut spend);
        assert_eq!(b.len(), 1);
        let size = bytes.len();
        assert_eq!(
            b[0]["text"],
            format!(
                "[PDF orders.pdf, {size} bytes, 3 pages: its text, page by page, since this model reads no PDFs; \
                 a page's pictures, charts, and scanned text are not in it]\n--- page 1 ---\nThe harbour master is \
                 Odile Varnack.\n--- page 2 ---\nBuoy B-2 is green.\n--- page 3 ---\n(no text on this page)"
            )
        );
        assert_eq!(spend, Spend::default(), "no document, no tokens of one");
        // As a tool's result: its text after the tool's.
        let tool = tool_content(
            "orders.pdf is a PDF of 3 pages.",
            &got[0],
            &glm(&blobs),
            &mut spend,
        );
        assert!(
            tool.as_str()
                .unwrap()
                .contains("--- page 2 ---\nBuoy B-2 is green."),
            "{tool}"
        );
        let native = tool_content(
            "orders.pdf is a PDF of 3 pages.",
            &got[0],
            &claude(&blobs, 600, &[]),
            &mut spend,
        );
        assert_eq!(native[1]["type"], "document", "{native}");
    }

    /// A request's PDFs are budgeted in the order they render: one the
    /// request has no room left for is its text, and how an earlier one
    /// renders never depends on a later one, so the cached prefix holds.
    #[test]
    fn pdfs_are_budgeted_in_render_order() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let first = harbour();
        let second = theseus_files::pdf::sample(&[
            "Moorings are free after six.",
            "Pilots board at the mark.",
            "Fees: none.",
        ]);
        let (got, _) = from_wire(
            vec![
                pdf_wire("orders.pdf", &first),
                pdf_wire("moorings.pdf", &second),
            ],
            caps(262_144),
            &blobs,
        );
        // A request that carries four pages: the first PDF fits, the second
        // does not.
        let media = claude(&blobs, 4, &[]);
        let mut spend = Spend::default();
        let a = blocks(&got[0], None, &media, &mut spend);
        let b = blocks(&got[1], None, &media, &mut spend);
        assert_eq!(a[1]["type"], "document");
        assert_eq!(b.len(), 1);
        let line = b[0]["text"].as_str().unwrap();
        assert!(
            line.contains("its text, page by page, since this request has no room left for it as a document (4 pages and 18.0 MB a request)"),
            "{line}"
        );
        assert!(
            line.contains("--- page 1 ---\nMoorings are free after six."),
            "{line}"
        );
        assert_eq!(spend.pages, 3);
        let alone = blocks(&got[0], None, &media, &mut Spend::default());
        assert_eq!(alone, a, "the first renders as it did alone");
    }

    /// A PDF over a page limit keeps its first pages as a part of their own
    /// (theseus-c9l6): a model whose requests carry fewer pages gets the
    /// part, with a line that says what was left out; one whose requests
    /// carry it all gets the whole file.
    #[test]
    fn a_pdf_over_a_page_limit_shows_its_first_pages_and_says_what_was_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let texts: Vec<String> = (1..=120).map(|n| format!("Log page {n}")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let bytes = theseus_files::pdf::sample(&refs);
        let (got, reads) = from_wire(vec![pdf_wire("log.pdf", &bytes)], caps(262_144), &blobs);
        let AttachmentContent::File { parts, .. } = &got[0].content else {
            panic!()
        };
        assert_eq!(parts.iter().map(|p| p.last).collect::<Vec<_>>(), vec![100]);
        assert_eq!(reads[0].parts, 1);
        let mut spend = Spend::default();
        let b = blocks(&got[0], None, &claude(&blobs, 100, &[]), &mut spend);
        assert_eq!(b[1]["type"], "document");
        let line = b[0]["text"].as_str().unwrap();
        assert!(
            line.ends_with("120 pages; pages 1–100 shown, as many as one request carries for this model; pages 101–120 left out]"),
            "{line}"
        );
        let part = crate::blobs::decode(b[1]["source"]["data"].as_str().unwrap()).unwrap();
        assert_eq!(
            theseus_files::pdf::read(&part, &Default::default())
                .unwrap()
                .pages,
            100
        );
        assert_eq!(spend.pages, 100);
        let whole = blocks(
            &got[0],
            None,
            &claude(&blobs, 600, &[]),
            &mut Spend::default(),
        );
        assert_eq!(
            crate::blobs::decode(whole[1]["source"]["data"].as_str().unwrap()).unwrap(),
            bytes
        );
    }

    /// A PDF the provider refused (theseus-0s4's rule, theseus-c9l6) shows as
    /// its text from then on; a 400 names it by its block's path, or by the
    /// word alone.
    #[test]
    fn a_pdf_the_provider_refused_is_its_text_from_then_on() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let bytes = harbour();
        let (got, _) = from_wire(vec![pdf_wire("orders.pdf", &bytes)], caps(262_144), &blobs);
        let b = blocks(
            &got[0],
            None,
            &claude(&blobs, 600, &[]),
            &mut Spend::default(),
        );
        let msgs = vec![
            json!({"role": "user", "content": [b[0].clone(), b[1].clone(), {"type": "text", "text": "who is the harbour master?"}]}),
        ];
        let named = refused(
            &msgs,
            "invalid_request_error: messages.0.content.1.document.source.base64.data: The PDF specified was not valid.",
        );
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].digest, crate::blobs::digest(&bytes));
        assert_eq!(named[0].why, "The PDF specified was not valid.");
        let unnamed = refused(&msgs, "invalid_request_error: Could not process PDF");
        assert_eq!(unnamed.len(), 1, "the request's only PDF");
        assert!(refused(&msgs, "invalid_request_error: Could not process image").is_empty());
        assert_eq!(digests_in(&msgs[0]), vec![crate::blobs::digest(&bytes)]);
        let hidden = [NotShown {
            digest: named[0].digest.clone(),
            why: named[0].why.clone(),
            at_ms: 1,
        }];
        let mut spend = Spend::default();
        let shown = blocks(&got[0], None, &claude(&blobs, 600, &hidden), &mut spend);
        assert_eq!(shown.len(), 1);
        assert!(
            shown[0]["text"].as_str().unwrap().contains(
                "since the provider refused it as a document (The PDF specified was not valid.)"
            ),
            "{shown:?}"
        );
        assert_eq!(spend, Spend::default());
    }

    /// A PDF that could not be read is kept, and says why; a model that reads
    /// PDFs still gets it whole, since the provider may read what the
    /// converter could not.
    #[test]
    fn a_pdf_that_could_not_be_read_is_kept_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let torn = b"%PDF-1.7 and nothing after it".to_vec();
        let (got, reads) = from_wire(vec![pdf_wire("torn.pdf", &torn)], caps(262_144), &blobs);
        assert_eq!(header(&got[0], None), "[PDF torn.pdf, 29 bytes]");
        assert_eq!(reads[0].outcome(), "unread");
        let glm_line = blocks(&got[0], None, &glm(&blobs), &mut Spend::default());
        let line = glm_line[0]["text"].as_str().unwrap();
        assert!(
            line.starts_with("[PDF torn.pdf, 29 bytes: not shown, this model reads no PDFs; its text could not be read (it could not be read as a PDF"),
            "{line}"
        );
        let native = blocks(
            &got[0],
            None,
            &claude(&blobs, 600, &[]),
            &mut Spend::default(),
        );
        assert_eq!(native[1]["type"], "document");
    }

    fn file_wire(name: &str, media_type: &str, bytes: &[u8]) -> theseus_protocol::Attachment {
        theseus_protocol::Attachment {
            name: name.into(),
            media_type: media_type.into(),
            size: bytes.len() as u64,
            data: Some(crate::blobs::encode(bytes)),
            ..Default::default()
        }
    }

    /// Join 2 (theseus-c9l6): a Word file, known by its members whatever its
    /// sender said, is read into its text when it arrives, and every model
    /// reads that text, a heading marked as one.
    #[test]
    fn a_word_document_is_read_when_it_arrives_and_every_model_reads_its_text() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let docx = theseus_files::doc::sample_zip(&[
            ("[Content_Types].xml", "<Types/>"),
            (
                "word/document.xml",
                r#"<w:document><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Harbour rules</w:t></w:r></w:p><w:p><w:r><w:t>Moorings are free after six.</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        ]);
        let (got, reads) = from_wire(
            vec![file_wire("rules.docx", "application/octet-stream", &docx)],
            caps(262_144),
            &blobs,
        );
        assert_eq!(
            got[0].media_type,
            theseus_files::kind::Kind::Docx.media_type().unwrap()
        );
        assert_eq!((reads[0].outcome(), reads[0].via), ("read", "attachment"));
        let size = docx.len();
        for media in [glm(&blobs), claude(&blobs, 600, &[])] {
            let b = blocks(
                &got[0],
                Some("discord:eddie"),
                &media,
                &mut Spend::default(),
            );
            assert_eq!(b.len(), 1);
            assert_eq!(
                b[0]["text"],
                format!("[Document rules.docx from discord:eddie, {size} bytes]\n--- text ---\n# Harbour rules\n\nMoorings are free after six.")
            );
        }
    }

    /// A notebook's cells are its sections, and its output images are shown
    /// to a model with vision, after its text, as images.
    #[test]
    fn a_notebooks_output_images_are_shown_to_a_model_with_vision() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let png = png(64, 32, 10);
        let nb = json!({"nbformat": 4, "metadata": {}, "cells": [
            {"cell_type": "code", "source": "plot()", "outputs": [
                {"output_type": "display_data", "data": {"text/plain": "<Figure>", "image/png": crate::blobs::encode(&png)}}]}]});
        let (got, _) = from_wire(
            vec![file_wire("survey.ipynb", "", nb.to_string().as_bytes())],
            caps(262_144),
            &blobs,
        );
        let mut spend = Spend::default();
        let b = blocks(&got[0], None, &claude(&blobs, 600, &[]), &mut spend);
        assert_eq!(b.len(), 2, "{b:?}");
        assert!(b[0]["text"]
            .as_str()
            .unwrap()
            .contains("--- cell 1 (code) ---\nplot()\n[output]\n<Figure>"));
        assert_eq!(b[1]["type"], "image");
        assert_eq!(
            crate::blobs::decode(b[1]["source"]["data"].as_str().unwrap()).unwrap(),
            png
        );
        assert!(spend.tokens > 0);
        let blind = blocks(&got[0], None, &glm(&blobs), &mut Spend::default());
        assert_eq!(blind.len(), 1);
        assert!(blind[0]["text"]
            .as_str()
            .unwrap()
            .ends_with("[1 of its images are not shown: this model has no vision]"));
    }

    /// A recording is kept and named with how to hear it: nothing is spent
    /// until the model asks (file.read). An archive is its list.
    #[test]
    fn a_recording_waits_to_be_asked_for_and_an_archive_is_its_list() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let ogg = b"OggS\x00\x02 a voice message, not really".to_vec();
        let zip = theseus_files::doc::sample_zip(&[("logs/tide.log", "high 06:12")]);
        let (got, reads) = from_wire(
            vec![
                file_wire("voice-message.ogg", "audio/ogg", &ogg),
                file_wire("logs.zip", "application/zip", &zip),
            ],
            caps(262_144),
            &blobs,
        );
        assert_eq!(
            reads.iter().map(FileRead::outcome).collect::<Vec<_>>(),
            vec!["kept", "read"]
        );
        let audio = blocks(
            &got[0],
            None,
            &claude(&blobs, 600, &[]),
            &mut Spend::default(),
        );
        assert_eq!(
            audio[0]["text"],
            format!("[Audio voice-message.ogg, audio/ogg, {} bytes: not transcribed yet; file_read gives its transcript (Deepgram, about $0.26 an hour of audio)]", ogg.len())
        );
        let list = blocks(&got[1], None, &glm(&blobs), &mut Spend::default());
        assert!(
            list[0]["text"]
                .as_str()
                .unwrap()
                .ends_with("--- contents: 1 files ---\nlogs/tide.log  (10 bytes)"),
            "{list:?}"
        );
    }
}

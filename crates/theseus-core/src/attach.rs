//! Files that come with an operator's message (theseus-9g2): a Discord
//! attachment, or `theseus ask --attach`.
//!
//! The sender reads what it can: text as text, anything it will not read
//! with the reason. The core keeps each one on the user node, text capped at
//! `[tools].max_read_bytes` on a character boundary, and the compiler renders
//! each as its own block before the typed text, under a header that names
//! the file and who sent it. Theseus has no provenance labels yet (§3.9), so
//! the header is what marks the text as the file's and not the operator's.

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

/// The line that names an attachment wherever it is shown: the model's
/// context, the web UI, and `theseus history`. For example
/// `[Attachment message.txt from discord:eddie, 5,012 bytes]`.
pub fn header(a: &Attachment, author: Option<&str>) -> String {
    let from = author
        .filter(|s| !s.is_empty())
        .map(|s| format!(" from {}", clean(s)))
        .unwrap_or_default();
    let name = clean(&a.name);
    let size = size_words(a.size);
    match &a.content {
        AttachmentContent::Text { cut: false, .. } => format!("[Attachment {name}{from}, {size}]"),
        AttachmentContent::Text { text, cut: true } => format!(
            "[Attachment {name}{from}, {size}; cut to its first {}]",
            size_words(text.len() as u64)
        ),
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

/// What the model reads for one attachment: the header, then the text.
pub fn for_model(a: &Attachment, author: Option<&str>) -> String {
    match &a.content {
        AttachmentContent::Text { text, .. } => format!("{}\n{text}", header(a, author)),
        AttachmentContent::NotRead { .. } => header(a, author),
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

/// The node's attachments from the wire's, in order. Nothing here fails a
/// turn: whatever cannot be kept becomes a `not_read` with the reason.
pub fn from_wire(list: Vec<theseus_protocol::Attachment>, max_text: usize) -> Vec<Attachment> {
    list.into_iter()
        .map(|w| {
            let content = match (w.not_read, w.text, w.data) {
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
                (None, None, Some(_)) => AttachmentContent::NotRead {
                    reason: "only text attachments are read".into(),
                },
                (None, None, None) => AttachmentContent::NotRead {
                    reason: "it came without its content".into(),
                },
            };
            Attachment {
                name: w.name,
                media_type: w.media_type,
                size: w.size,
                content,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn text_is_cut_on_a_character_boundary_and_headers_say_so() {
        // 'é' is two bytes, so a 5-byte cap cannot keep the third one whole.
        let got = from_wire(vec![wire("a.txt", Some("ééé"), None)], 5);
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
        let mut w = wire(
            "dump.zip",
            None,
            Some("over the limit for text (262,144 bytes)"),
        );
        w.media_type = "application/zip".into();
        let got = from_wire(vec![w], 262_144);
        assert_eq!(
            for_model(&got[0], Some("discord:eddie")),
            "[Attachment dump.zip from discord:eddie, application/zip, 20.0 MB: not read: over the limit for text (262,144 bytes)]"
        );
        assert_eq!(
            display_text("see attached", &got, Some("discord:eddie")),
            "see attached\n[Attachment dump.zip from discord:eddie, application/zip, 20.0 MB: not read: over the limit for text (262,144 bytes)]"
        );
        // A name cannot break the header's line.
        let odd = from_wire(vec![wire("a\nb].txt", Some("t"), None)], 10);
        assert_eq!(header(&odd[0], None), "[Attachment a b].txt, 1 byte]");
    }
}

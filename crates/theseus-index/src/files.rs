//! A kept file's read text, as the index takes it (theseus-lv2u).
//!
//! A file a person attaches is kept whole in the store's blobs
//! (`AttachmentContent::File`, store format 17), and what was read of it —
//! a PDF's pages, a document's sections — is a blob of its own that the node
//! names by digest. None of it is in the WAL, so until this module the index
//! saw a document's name and nothing of its words: ask about last week's PDF
//! in a new session and recall could not find it.
//!
//! **Where the text enters.** Here, at index time, read from the blob the
//! node names: the tender is given the store's directory
//! ([`crate::tender::Config::store_dir`]) and the blobs sit in it beside the
//! WAL. The other side — writing the read text into the node at attach time
//! — was refused: a node is a WAL frame that every recovery and every index
//! rebuild reads whole, and the blobs exist precisely so that a 20 MB
//! document does not go through it.
//!
//! **The cap.** [`MAX_BYTES`] of read text per file, the default of
//! `[tools] max_read_bytes`: what the harness itself already takes of one
//! file for the model. Sections are taken in order until the cap, the last
//! one cut on a character boundary; the rest is dropped. The drop is not
//! implicit: it is indexed as a chunk of its own ([`FileText::cut`]) saying
//! how much is held and how to read the rest, so a recall hit at the end of
//! a long document says it is the end of what the index holds, and
//! `index.status` counts the files that were cut.
//!
//! **The chunking.** Each section is chunked on its own ([`crate::chunk`],
//! 512 tokens), so no chunk spans two pages, and every chunk is headed
//! `[<file> · <section>]`. That head is what a recall hit cites: the hit's
//! node is the message the file came with, and its text says which file and
//! which page the passage is from.
//!
//! **Known limits of "once per digest".** The digest is counted across the
//! whole index, not per session, so a file's one copy belongs to the node
//! that carried it first, and two things follow:
//!
//! - *Erasure.* Erasing that node takes the text out even where a later node
//!   still carries the file, and a rebuild does not put it back.
//! - *Provenance.* A hit on the file's text cites the first node's message,
//!   and so that node's session, even for an asker who attached the same
//!   bytes in a session of their own and was never in the first one. Recall
//!   is across sessions by design (ask in a new session about last week's
//!   PDF), so the hit's text is not hidden; its citation is what can name a
//!   session the asker was not in. The place rule bounds that reach but
//!   does not remove it: recall drops a hit whose node sits in a place the
//!   asker may not draw on (`Place::may_draw_on`: a shared place draws only
//!   on its own place's sessions, a private place on any), so across shared
//!   places the dedup shows as a miss, never a citation; inside one place,
//!   or from a private place, the cited session can still be one the asker
//!   was never in. Scoping the dedup per session would cite the asker's own
//!   node but store each attached file once per session. That cost is
//!   measured (`AGENTS.md`: one capped file of varied prose, 132 chunks,
//!   411 KB of BM25 and 234 KB of vectors, about 645 KB), and the decision
//!   stands: the figure is per file per session-repeat, and what it buys is
//!   only a citation naming the asker's own session, for text recall already
//!   returns whole.

use std::path::Path;

use serde::Deserialize;

/// The most one text blob may hold, in bytes. Core's reader has no bound;
/// a blob of read text is a capped PDF's pages or a document's sections, far
/// under this, so a larger one is not what core wrote.
pub const MAX_BLOB: usize = 64 << 20;

/// Whether `d` is a blob's name: 64 lower-case hex digits, as
/// `theseus_core::blobs::digest` writes them. Restated here, not imported:
/// this crate does not depend on the core.
fn is_digest(d: &str) -> bool {
    d.len() == 64
        && d.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// A text blob's bytes, when `name` is a digest, the blob is within
/// [`MAX_BLOB`], and its bytes hash to its name: core's reader's two guards
/// plus the bound. `name` comes verbatim from a node, and `Path::join` with an
/// absolute component discards its base, so the shape is checked before it
/// is ever a path.
pub fn read_blob(dir: &Path, name: &str) -> Option<Vec<u8>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    if !is_digest(name) {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(dir.join(name))
        .ok()?
        .take(MAX_BLOB as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_BLOB {
        return None;
    }
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (hash == name).then_some(bytes)
}

/// The most of one file's read text the index takes. `[tools]
/// max_read_bytes`'s own default: the index never holds more of a file than
/// a `fs.read` would give the model at once.
pub const MAX_BYTES: usize = 256 << 10;

/// One piece of a file's read text: a PDF's page, a document's section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// The head every chunk of this piece carries, `[<file> · <section>]`.
    pub head: String,
    pub text: String,
}

/// What the index takes of one kept file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileText {
    /// The file's own bytes' digest: what "once per digest" counts.
    pub digest: String,
    pub name: String,
    pub pieces: Vec<Piece>,
    /// What the cap left out, indexed as a chunk of its own.
    pub cut: Option<String>,
}

impl FileText {
    /// Nothing to index: a file whose read text is empty.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty() && self.cut.is_none()
    }
}

#[derive(Deserialize)]
struct Section {
    #[serde(default)]
    label: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct Sections {
    sections: Vec<Section>,
}

/// A text blob's sections, in either of the shapes `theseus_core::blobs`
/// writes: a JSON list of strings (a PDF's pages, labelled `page N`), or
/// `{"sections": [{"label", "text"}, …]}` (a document's sections). `None`
/// for bytes in neither shape.
pub fn sections(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    if let Ok(pages) = serde_json::from_slice::<Vec<String>>(bytes) {
        return Some(
            pages
                .into_iter()
                .enumerate()
                .map(|(i, text)| (format!("page {}", i + 1), text))
                .collect(),
        );
    }
    serde_json::from_slice::<Sections>(bytes)
        .ok()
        .map(|s| s.sections.into_iter().map(|s| (s.label, s.text)).collect())
}

/// The head a chunk of this file's section carries.
fn head(name: &str, label: &str) -> String {
    if label.trim().is_empty() {
        format!("[{name}]")
    } else {
        format!("[{name} · {label}]")
    }
}

/// The longest prefix of `s` at most `max` bytes long, on a character
/// boundary.
fn cut_at(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// What the index takes of `sections`, under `max` bytes of text.
pub fn take(name: &str, digest: &str, sections: Vec<(String, String)>, max: usize) -> FileText {
    let total: usize = sections.iter().map(|(_, t)| t.len()).sum();
    let mut pieces = Vec::new();
    let mut taken = 0usize;
    let mut last = String::new();
    for (label, text) in sections {
        if taken >= max {
            break;
        }
        let text = cut_at(&text, max - taken);
        taken += text.len();
        last = label.clone();
        if text.trim().is_empty() {
            continue;
        }
        pieces.push(Piece {
            head: head(name, &label),
            text: text.to_string(),
        });
    }
    let cut = (taken < total).then(|| {
        let to = if last.trim().is_empty() {
            String::new()
        } else {
            format!(", to {last}")
        };
        format!(
            "[{name}: the index holds its first {taken} of {total} characters{to}; \
             the rest of it is not indexed. file_read gives the whole file.]"
        )
    });
    FileText {
        digest: digest.to_string(),
        name: name.to_string(),
        pieces,
        cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdfs_pages_and_a_documents_sections_both_read() {
        let pages = serde_json::to_vec(&vec!["one".to_string(), "two".to_string()]).unwrap();
        assert_eq!(
            sections(&pages).unwrap(),
            vec![
                ("page 1".to_string(), "one".to_string()),
                ("page 2".to_string(), "two".to_string()),
            ]
        );
        let doc = br#"{"sections":[{"label":"Heading","text":"body","images":[]}]}"#;
        assert_eq!(
            sections(doc).unwrap(),
            vec![("Heading".to_string(), "body".to_string())]
        );
        assert!(sections(b"not json").is_none());
    }

    #[test]
    fn every_chunk_of_a_section_cites_its_file_and_its_page() {
        let f = take(
            "harbour-notes.pdf",
            "d",
            vec![("page 1".into(), "the tide".into())],
            MAX_BYTES,
        );
        assert_eq!(f.pieces[0].head, "[harbour-notes.pdf · page 1]");
        assert!(f.cut.is_none());
        // A section with no label of its own still names the file.
        let f = take("notes.odt", "d", vec![(String::new(), "body".into())], 100);
        assert_eq!(f.pieces[0].head, "[notes.odt]");
    }

    #[test]
    fn the_cap_stops_the_text_and_says_so() {
        let long = "x".repeat(100);
        let f = take(
            "big.pdf",
            "d",
            vec![
                ("page 1".into(), long.clone()),
                ("page 2".into(), long.clone()),
                ("page 3".into(), long),
            ],
            150,
        );
        let held: usize = f.pieces.iter().map(|p| p.text.len()).sum();
        assert_eq!(held, 150, "it stops at the cap");
        assert_eq!(f.pieces.len(), 2, "page 2 is cut inside: {:?}", f.pieces);
        let cut = f.cut.expect("the cut is said");
        assert!(cut.contains("150 of 300 characters"), "{cut}");
        assert!(cut.contains("to page 2"), "{cut}");
        assert!(cut.contains("big.pdf"), "{cut}");
    }

    #[test]
    fn a_file_under_the_cap_is_whole_and_an_empty_one_gives_nothing() {
        let f = take(
            "a.pdf",
            "d",
            vec![("page 1".into(), "short".into())],
            MAX_BYTES,
        );
        assert_eq!(f.pieces.len(), 1);
        assert!(f.cut.is_none());
        assert!(!f.is_empty());
        let f = take(
            "b.pdf",
            "d",
            vec![("page 1".into(), "  ".into())],
            MAX_BYTES,
        );
        assert!(f.is_empty(), "nothing to index: {f:?}");
    }

    #[test]
    fn a_multi_byte_character_is_never_cut_through() {
        let text = "é".repeat(50);
        let f = take("w.pdf", "d", vec![("page 1".into(), text)], 25);
        let held = &f.pieces[0].text;
        assert!(held.len() <= 25 && held.chars().all(|c| c == 'é'));
        assert!(f.cut.is_some());
    }
}

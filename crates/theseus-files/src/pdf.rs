//! PDFs (theseus-c9l6): known by their first bytes, their pages counted,
//! each page's text extracted, and a range of pages cut out as a PDF of its
//! own. A page with no text layer (a scan) has empty text: a model that reads
//! PDFs sees the page's picture, and the text path says the page had none.
//!
//! This runs on bytes a stranger may have made, so it is called through
//! [`crate::convert`], in a child process with a time limit and a memory cap.
//! The streams it inflates are bounded here too, so a decompression bomb
//! fails with words before the child's cap kills it.

use lopdf::{Document, LoadOptions};
use serde::{Deserialize, Serialize};

/// The most bytes one stream may inflate to while a PDF is read: a page's
/// content, a font's map, an object stream.
const MAX_INFLATED: usize = 128 * 1024 * 1024;

/// The most text one read returns, all pages together; past it the pages'
/// texts are cut, and the read says so. A model is shown far less
/// (`[tools] max_read_bytes`); this bounds the child's answer.
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// A PDF's first bytes: `%PDF-`, which readers find within the first 1,024.
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|w| w == b"%PDF-")
}

/// Pages `first` to `last`, counted from 1, both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pages {
    pub first: u32,
    pub last: u32,
}

impl Pages {
    pub fn new(first: u32, last: u32) -> Self {
        Self { first, last }
    }

    pub fn count(&self) -> u32 {
        self.last.saturating_sub(self.first) + 1
    }

    /// A range as a tool takes it: `3`, `3-5`, or `3-` (to the end).
    pub fn parse(s: &str) -> Result<Pages, String> {
        let s = s.trim();
        let num = |t: &str| -> Result<u32, String> {
            match t.trim().parse::<u32>() {
                Ok(0) | Err(_) => Err(format!(
                    "`{s}` is not a page range: give `3`, `3-5`, or `3-`, pages counted from 1"
                )),
                Ok(n) => Ok(n),
            }
        };
        let (first, last) = match s.split_once('-') {
            Some((a, "")) => (num(a)?, u32::MAX),
            Some((a, b)) => (num(a)?, num(b)?),
            None => {
                let n = num(s)?;
                (n, n)
            }
        };
        if last < first {
            return Err(format!("`{s}` ends before it starts"));
        }
        Ok(Pages { first, last })
    }

    /// `page 3`, `pages 3–5`.
    pub fn words(&self) -> String {
        if self.first == self.last {
            format!("page {}", self.first)
        } else {
            format!("pages {}–{}", self.first, self.last)
        }
    }
}

/// What to make of a PDF.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ask {
    /// The pages to read; all of them when absent.
    #[serde(default)]
    pub pages: Option<Pages>,
    /// Cut the pages read out as a PDF of their own, when they are not the
    /// whole file.
    #[serde(default)]
    pub part: bool,
    /// Extract each page's text.
    #[serde(default)]
    pub text: bool,
}

/// What a read of a PDF gave.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Read {
    /// The whole file's pages.
    pub pages: u32,
    /// The pages read.
    pub range: Option<Pages>,
    /// Each page's text, in order, when text was asked for: empty for a
    /// page with no text layer, or one whose text could not be read.
    #[serde(default)]
    pub texts: Vec<String>,
    /// Pages whose text could not be read, and why (the first few).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<(u32, String)>,
    /// The texts were cut at [`MAX_TEXT_BYTES`]: the last page with text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_cut_at: Option<u32>,
    /// The pages read, as a PDF of their own (`Ask::part`), base64 on the
    /// wire.
    #[serde(default, with = "b64", skip_serializing_if = "Option::is_none")]
    pub part: Option<Vec<u8>>,
}

mod b64 {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        s.map(|s| {
            base64::engine::general_purpose::STANDARD
                .decode(s)
                .map_err(serde::de::Error::custom)
        })
        .transpose()
    }
}

/// Read a PDF as `ask` says. `Err` says why in words: not a PDF, a password,
/// damage the reader could not get past, or pages it does not have.
pub fn read(bytes: &[u8], ask: &Ask) -> Result<Read, String> {
    if !is_pdf(bytes) {
        return Err("it is not a PDF (no %PDF- header)".into());
    }
    let opts = LoadOptions {
        max_decompressed_size: Some(MAX_INFLATED),
        ..Default::default()
    };
    let mut doc = Document::load_mem_with_options(bytes, opts).map_err(|e| damaged(&e))?;
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return Err("it is encrypted with a password".into());
    }
    let numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    let pages = numbers.len() as u32;
    if pages == 0 {
        return Err("it has no pages".into());
    }
    let range = match ask.pages {
        None => Pages::new(1, pages),
        Some(p) if p.first > pages => {
            return Err(format!(
                "it has {} so {} is past its end",
                count(pages),
                p.words()
            ))
        }
        Some(p) => Pages::new(p.first, p.last.min(pages)),
    };
    let mut out = Read {
        pages,
        range: Some(range),
        ..Read::default()
    };
    if ask.text {
        let mut held = 0usize;
        for n in range.first..=range.last {
            if held >= MAX_TEXT_BYTES {
                out.text_cut_at = Some(n - 1);
                break;
            }
            let mut text = String::new();
            for chunk in doc.extract_text_chunks_with_limit(&[n], MAX_INFLATED) {
                match chunk {
                    Ok(t) => text.push_str(&t),
                    Err(e) if out.unreadable.len() < 8 => {
                        out.unreadable.push((n, short(&e.to_string())));
                    }
                    Err(_) => {}
                }
            }
            let text = tidy(&text);
            let room = MAX_TEXT_BYTES - held;
            let text = if text.len() > room {
                out.text_cut_at = Some(n);
                cut(&text, room).to_string()
            } else {
                text
            };
            held += text.len();
            out.texts.push(text);
        }
    }
    if ask.part && (range.first > 1 || range.last < pages) {
        let drop: Vec<u32> = numbers
            .iter()
            .copied()
            .filter(|n| *n < range.first || *n > range.last)
            .collect();
        doc.delete_pages(&drop);
        doc.prune_objects();
        doc.compress();
        let mut part = Vec::new();
        doc.save_to(&mut part)
            .map_err(|e| format!("its pages could not be cut out ({})", short(&e.to_string())))?;
        out.part = Some(part);
    }
    Ok(out)
}

/// `1 page`, `12 pages`.
pub fn count(n: u32) -> String {
    if n == 1 {
        "1 page".into()
    } else {
        format!("{n} pages")
    }
}

fn damaged(e: &lopdf::Error) -> String {
    format!("it could not be read as a PDF ({})", short(&e.to_string()))
}

/// An error's words, on one line, bounded.
fn short(s: &str) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect();
    one.trim().to_string()
}

/// A page's text as the extractor gives it, without the spaces it leaves at
/// the ends of lines and the blank lines at its ends.
fn tidy(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_string()
}

/// The start of `s` that fits in `max` bytes, cut on a character boundary.
fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// A small PDF with one page per item, each page holding that text on one
/// line in Helvetica, or no text at all for an empty item (a page like a
/// scan's, with nothing to extract). Tests build their PDFs with it, never
/// from a committed binary.
pub fn sample(pages: &[&str]) -> Vec<u8> {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = doc.add_object(dictionary! {"Font" => dictionary! {"F1" => font}});
    let mut kids = Vec::new();
    for text in pages {
        let mut ops = Vec::new();
        if !text.is_empty() {
            ops = vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 720.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ];
        }
        let content = Content { operations: ops }
            .encode()
            .expect("a sample's content encodes");
        let content = doc.add_object(Stream::new(dictionary! {}, content));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        });
        kids.push(Object::from(page));
    }
    let n = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => n, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog);
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("a sample saves");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(pages: Option<Pages>, part: bool, text: bool) -> Ask {
        Ask { pages, part, text }
    }

    #[test]
    fn a_pdf_is_known_by_its_header_and_its_pages_counted() {
        let pdf = sample(&["The tide turns at noon.", "Buoy B-2 is green.", ""]);
        assert!(is_pdf(&pdf));
        assert!(!is_pdf(b"PK\x03\x04 a zip"));
        let r = read(&pdf, &ask(None, false, true)).unwrap();
        assert_eq!(r.pages, 3);
        assert_eq!(r.range, Some(Pages::new(1, 3)));
        assert_eq!(
            r.texts,
            vec!["The tide turns at noon.", "Buoy B-2 is green.", ""],
            "the third page has no text layer"
        );
        assert!(r.part.is_none(), "the whole file is never cut");
        // No text asked for: none read.
        assert!(read(&pdf, &ask(None, false, false))
            .unwrap()
            .texts
            .is_empty());
    }

    #[test]
    fn a_range_is_cut_out_as_a_pdf_of_its_own() {
        let pdf = sample(&["one", "two", "three", "four"]);
        let r = read(&pdf, &ask(Some(Pages::new(2, 3)), true, true)).unwrap();
        assert_eq!(
            (r.pages, r.texts.clone()),
            (4, vec!["two".into(), "three".into()])
        );
        let part = r.part.expect("a part");
        let again = read(&part, &ask(None, false, true)).unwrap();
        assert_eq!(again.pages, 2);
        assert_eq!(again.texts, vec!["two", "three"]);
        // A range past the end is clamped; one that starts past it is refused.
        let tail = read(&pdf, &ask(Some(Pages::new(3, u32::MAX)), false, true)).unwrap();
        assert_eq!(tail.range, Some(Pages::new(3, 4)));
        assert_eq!(
            read(&pdf, &ask(Some(Pages::new(9, 9)), false, true)).unwrap_err(),
            "it has 4 pages so page 9 is past its end"
        );
    }

    #[test]
    fn page_ranges_parse_as_a_tool_takes_them() {
        assert_eq!(Pages::parse("3").unwrap(), Pages::new(3, 3));
        assert_eq!(Pages::parse(" 3-5 ").unwrap(), Pages::new(3, 5));
        assert_eq!(Pages::parse("7-").unwrap(), Pages::new(7, u32::MAX));
        assert!(Pages::parse("0").is_err());
        assert!(Pages::parse("5-3")
            .unwrap_err()
            .contains("ends before it starts"));
        assert!(Pages::parse("two").is_err());
        assert_eq!(Pages::new(3, 5).words(), "pages 3–5");
        assert_eq!(Pages::new(4, 4).words(), "page 4");
    }

    #[test]
    fn what_is_not_a_readable_pdf_says_why() {
        assert_eq!(
            read(b"hello", &Ask::default()).unwrap_err(),
            "it is not a PDF (no %PDF- header)"
        );
        let mut torn = sample(&["one"]);
        torn.truncate(40);
        assert!(
            read(&torn, &Ask::default())
                .unwrap_err()
                .starts_with("it could not be read as a PDF"),
            "a torn file"
        );
    }

    #[test]
    fn a_read_and_its_part_cross_the_wire() {
        let pdf = sample(&["one", "two"]);
        let r = read(&pdf, &ask(Some(Pages::new(2, 2)), true, true)).unwrap();
        let wire = serde_json::to_string(&r).unwrap();
        let back: Read = serde_json::from_str(&wire).unwrap();
        assert_eq!(back, r);
    }
}

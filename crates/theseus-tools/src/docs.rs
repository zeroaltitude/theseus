//! What `fs.read` reads besides text and images (theseus-c9l6): a PDF's
//! pages, and a document (Word, Excel, PowerPoint, OpenDocument, EPUB, RTF,
//! a Jupyter notebook) or an archive's list, each through theseus-files'
//! capped converter. `http.fetch` and `file.read` read a PDF's pages through
//! [`pdf_pages`] too.

use std::path::Path;

use serde_json::json;

use crate::fs::open_regular;
use crate::{Media, PdfData, ToolFailure, ToolOutput};

/// The most PDF pages one read returns, as Claude Code's Read does: a read
/// of more, or of a longer PDF without `pages`, returns this many and says
/// how to read on.
pub const PDF_PAGES_PER_READ: u32 = 20;

/// Whether a file's first bytes say PDF.
pub(crate) fn starts_as_pdf(path: &Path) -> bool {
    use std::io::Read as _;
    open_regular(path, true).is_ok_and(|(f, _)| {
        let mut head = Vec::with_capacity(1024);
        f.take(1024).read_to_end(&mut head).is_ok() && theseus_files::pdf::is_pdf(&head)
    })
}

/// A PDF's pages as a tool returns them (theseus-c9l6): the pages asked for
/// (`pages`), or the first [`PDF_PAGES_PER_READ`], read in the capped
/// converter. The model gets those pages cut out as a PDF of their own, with
/// their text for a model that reads no PDFs; the result says which pages of
/// how many, and how to read on with `again` (the tool's wire name). `what`
/// names the file in the result (`/w/report.pdf`, `It`), and `file` in the
/// line under it (`report.pdf`). A PDF that could not be read says why, and
/// gives no file.
pub fn pdf_pages(
    what: &str,
    file: String,
    bytes: Vec<u8>,
    pages: Option<&str>,
    again: &str,
) -> Result<(ToolOutput, Option<Media>), ToolFailure> {
    use theseus_files::pdf::{self, Pages};
    let asked = pages
        .map(Pages::parse)
        .transpose()
        .map_err(ToolFailure::new)?;
    let first = asked.map_or(1, |p| p.first);
    let last = asked.map_or(u32::MAX, |p| p.last);
    let range = Pages::new(
        first,
        last.min(first.saturating_add(PDF_PAGES_PER_READ - 1)),
    );
    let ask = pdf::Ask {
        pages: Some(range),
        part: true,
        text: true,
    };
    let (got, ran) = theseus_files::convert::pdf(&bytes, &ask);
    let size = bytes.len();
    let read = match got {
        Ok(r) => r,
        Err(why) => {
            return Ok((
                ToolOutput {
                    text: format!("{what} was not read as a PDF ({size} bytes): {why}."),
                    meta: json!({"bytes": size, "pdf": true, "not_read": why}),
                },
                None,
            ))
        }
    };
    let total = read.pages;
    let got = read.range.unwrap_or(Pages::new(1, total));
    let whole = got.first == 1 && got.last == total;
    let mut text = format!(
        "{what} is a PDF of {}, {size} bytes: {} shown.",
        pdf::count(total),
        if whole {
            "every page".to_string()
        } else {
            got.words()
        }
    );
    if got.last < total {
        let next = Pages::new(got.last + 1, (got.last + PDF_PAGES_PER_READ).min(total));
        text.push_str(&format!(
            " Pages {}–{total} are not shown: {again} with pages=\"{}-{}\" reads on.",
            got.last + 1,
            next.first,
            next.last
        ));
    }
    let name = match whole {
        true => file,
        false => format!("{file}, {} of {total}", got.words()),
    };
    let meta =
        json!({"bytes": size, "pdf": true, "pages": total, "from": got.first, "to": got.last});
    let mut read = read;
    let part = read.part.take().unwrap_or(bytes);
    Ok((
        ToolOutput { text, meta },
        Some(Media::Pdf(PdfData {
            name,
            bytes: part,
            read,
            ms: ran.ms,
            capped: ran.capped,
        })),
    ))
}

/// `fs.read` of a document (theseus-c9l6): Word, Excel, PowerPoint,
/// OpenDocument, EPUB, RTF, a Jupyter notebook, or an archive's list, read
/// into sections in the capped converter, `pages` counting sections, cut at
/// `max` bytes with words that say how to read on. `None` for any other file.
pub(crate) fn read_doc(
    path: &Path,
    bytes: &[u8],
    pages: Option<&str>,
    max: usize,
) -> Result<Option<ToolOutput>, ToolFailure> {
    use theseus_files::kind::{self, Kind};
    use theseus_files::pdf::Pages;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let members = match bytes.starts_with(b"PK") {
        true => theseus_files::doc::zip_members(bytes),
        false => Vec::new(),
    };
    let k = kind::sniff(bytes, &name, &members);
    if k == Kind::Pdf || !k.has_text() {
        return Ok(None);
    }
    let what = k.noun().to_ascii_lowercase();
    let (got, _) = theseus_files::convert::doc(bytes, k);
    let doc = match got {
        Ok(d) => d,
        Err(why) => {
            return Ok(Some(ToolOutput {
                text: format!(
                    "{} was not read as a {what} ({} bytes): {why}.",
                    path.display(),
                    bytes.len()
                ),
                meta: json!({"path": path, "bytes": bytes.len(), "kind": what, "not_read": why}),
            }))
        }
    };
    let total = doc.sections.len() as u32;
    let range = match pages {
        Some(p) => Pages::parse(p).map_err(ToolFailure::new)?,
        None => Pages::new(1, total.max(1)),
    };
    if range.first > total.max(1) {
        return Err(ToolFailure::new(format!(
            "{} has {total} sections, so section {} is past its end",
            path.display(),
            range.first
        )));
    }
    let mut text = format!(
        "{} is a {what} of {total} sections, {} bytes.",
        path.display(),
        bytes.len()
    );
    let mut shown = range.first - 1;
    for s in doc
        .sections
        .iter()
        .skip(range.first as usize - 1)
        .take(range.count() as usize)
    {
        let piece = format!("\n--- {} ---\n{}", s.label, s.text);
        if text.len() + piece.len() > max && shown >= range.first {
            break;
        }
        let room = max.saturating_sub(text.len());
        let mut end = room.min(piece.len());
        while !piece.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&piece[..end]);
        shown += 1;
        if !s.images.is_empty() {
            text.push_str(&format!(
                "\n[{} images in its outputs: attached to a message, they are shown]",
                s.images.len()
            ));
        }
    }
    let last = range.last.min(total);
    if shown < last {
        text.push_str(&format!(
            "\n[shown: sections {}–{shown}; fs_read with pages=\"{}-{last}\" reads on]",
            range.first,
            shown + 1
        ));
    }
    if let Some(cut) = &doc.cut {
        text.push_str(&format!("\n[{cut}]"));
    }
    Ok(Some(ToolOutput {
        text,
        meta: json!({"path": path, "bytes": bytes.len(), "kind": what, "sections": total, "from": range.first, "to": shown}),
    }))
}

/// `fs.read` of a PDF: [`pdf_pages`], its meta naming the path.
pub(crate) fn read_pdf(
    path: &Path,
    bytes: Vec<u8>,
    pages: Option<&str>,
) -> Result<(ToolOutput, Option<Media>), ToolFailure> {
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let (mut out, media) = pdf_pages(&path.display().to_string(), file, bytes, pages, "fs_read")?;
    out.meta["path"] = json!(path);
    Ok((out, media))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::Read;
    use crate::{Tool, ToolCtx};

    fn ctx(d: &tempfile::TempDir) -> ToolCtx {
        ToolCtx::for_tests(d.path())
    }

    /// `fs.read` of a document by path (theseus-c9l6): a workbook's sheets as
    /// tables, paged by `pages` (sections), and an archive's list.
    #[test]
    fn reading_a_document_returns_its_sections_and_an_archive_its_list() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(&d);
        let xlsx = theseus_files::doc::sample_zip(&[
            (
                "xl/workbook.xml",
                r#"<workbook><sheets><sheet name="Tides" r:id="rId1"/><sheet name="Fees" r:id="rId2"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>High</t></is></c><c r="B1"><v>6.2</v></c></row></sheetData></worksheet>"#,
            ),
            (
                "xl/worksheets/sheet2.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>none</t></is></c></row></sheetData></worksheet>"#,
            ),
        ]);
        std::fs::write(d.path().join("harbour.xlsx"), &xlsx).unwrap();
        let (out, media) = Read
            .run_with_media(&json!({"path": "harbour.xlsx", "pages": "2"}), &c)
            .unwrap();
        assert!(media.is_none());
        assert!(
            out.text.contains("is a spreadsheet of 2 sections"),
            "{}",
            out.text
        );
        assert!(
            out.text.ends_with("--- sheet Fees ---\nnone"),
            "{}",
            out.text
        );
        let (all, _) = Read
            .run_with_media(&json!({"path": "harbour.xlsx"}), &c)
            .unwrap();
        assert!(
            all.text.contains("--- sheet Tides ---\nHigh | 6.2"),
            "{}",
            all.text
        );
        let zip = theseus_files::doc::sample_zip(&[("a/b.txt", "x")]);
        std::fs::write(d.path().join("pack.zip"), &zip).unwrap();
        let (list, _) = Read
            .run_with_media(&json!({"path": "pack.zip"}), &c)
            .unwrap();
        assert!(
            list.text
                .ends_with("--- contents: 1 files ---\na/b.txt  (1 bytes)"),
            "{}",
            list.text
        );
    }

    /// `fs.read` of a PDF (theseus-c9l6): the pages asked for, cut out as a
    /// PDF of their own with their text; the first 20 when none are asked
    /// for; a range past the end, and pages of a file that is not a PDF,
    /// refused with words.
    #[test]
    fn reading_a_pdf_returns_its_pages_and_says_how_to_read_on() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(&d);
        let texts: Vec<String> = (1..=25).map(|n| format!("Ledger page {n}")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        std::fs::write(
            d.path().join("ledger.pdf"),
            theseus_files::pdf::sample(&refs),
        )
        .unwrap();
        let (out, media) = Read
            .run_with_media(&json!({"path": "ledger.pdf", "pages": "3-5"}), &c)
            .unwrap();
        assert!(
            out.text.contains("is a PDF of 25 pages") && out.text.contains("pages 3–5 shown."),
            "{}",
            out.text
        );
        assert!(
            out.text.contains("fs_read with pages=\"6-25\" reads on"),
            "{}",
            out.text
        );
        assert_eq!(
            (out.meta["pages"].as_u64(), out.meta["from"].as_u64()),
            (Some(25), Some(3))
        );
        let Some(Media::Pdf(p)) = media else {
            panic!("the pages")
        };
        assert_eq!(p.name, "ledger.pdf, pages 3–5 of 25");
        assert_eq!(
            p.read.texts,
            vec!["Ledger page 3", "Ledger page 4", "Ledger page 5"]
        );
        let part = theseus_files::pdf::read(&p.bytes, &Default::default()).unwrap();
        assert_eq!(part.pages, 3, "the part holds those pages alone");

        // No pages: the first 20, and how to read on.
        let (out, media) = Read
            .run_with_media(&json!({"path": "ledger.pdf"}), &c)
            .unwrap();
        assert!(out.text.contains("pages 1–20 shown"), "{}", out.text);
        let Some(Media::Pdf(p)) = media else {
            panic!("the pages")
        };
        assert_eq!(p.read.texts.len(), 20);

        // A short PDF whole: its own bytes, every page.
        std::fs::write(
            d.path().join("note.pdf"),
            theseus_files::pdf::sample(&["one", "two"]),
        )
        .unwrap();
        let (out, media) = Read
            .run_with_media(&json!({"path": "note.pdf"}), &c)
            .unwrap();
        assert!(out.text.ends_with("every page shown."), "{}", out.text);
        let Some(Media::Pdf(p)) = media else {
            panic!("the pages")
        };
        assert_eq!((p.name.as_str(), p.read.texts.len()), ("note.pdf", 2));

        let (out, media) = Read
            .run_with_media(&json!({"path": "note.pdf", "pages": "9"}), &c)
            .unwrap();
        assert!(media.is_none());
        assert!(
            out.text.ends_with(
                "was not read as a PDF (716 bytes): it has 2 pages so page 9 is past its end."
            ),
            "{}",
            out.text
        );
        std::fs::write(d.path().join("a.txt"), "plain").unwrap();
        let e = Read
            .run_with_media(&json!({"path": "a.txt", "pages": "1"}), &c)
            .unwrap_err();
        assert!(e.message.ends_with(
            "is not a PDF or a document; pages is for those (use offset/limit for lines)"
        ));
    }
}

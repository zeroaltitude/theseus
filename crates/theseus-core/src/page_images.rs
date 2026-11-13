//! A scanned page as a picture (theseus-w68l). A model that reads no PDFs
//! is given the document's extracted text page by page, and a scan has no
//! text to extract: those pages reached such a model as
//! `(no text on this page)`, which is every word the model ever saw of
//! them. A page with no text is drawn instead, once, when it arrives, and
//! kept beside the PDF's bytes; the request renders it as an image block
//! for a model with vision, and says the page is a picture for one
//! without.
//!
//! Nothing here renders: [`Draw`] is the seam, and the one a daemon passes
//! is [`POPPLER`] (theseus-files' `render`, an external renderer when the
//! machine has one). A test passes its own, so no test needs a renderer
//! installed.
//!
//! It is off unless `[tools] pdf_page_images` is set: drawing a page runs a
//! program, and anything that widens what Theseus reaches for is switched
//! off by default. A PDF whose pages all have text draws nothing whether
//! it is on or off.
//!
//! No store format moves for this: the pages are a *derived* blob,
//! `<blobs>/derived/<digest>.pages.json` (`Blobs::put_derived`, as a
//! recording's transcript is kept), named by the PDF's own digest, so no
//! stored record gains a field and a node written before this still reads.

use serde::{Deserialize, Serialize};

use crate::blobs::{Blobs, Texts};

/// One page kept as a picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageImage {
    /// The page it is, from 1.
    pub page: u32,
    /// Its blob.
    pub digest: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}

/// A PDF's pages kept as pictures, in page order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageImages {
    pub images: Vec<PageImage>,
}

impl PageImages {
    /// The picture of a page (from 1), when there is one.
    pub fn of(&self, page: u32) -> Option<&PageImage> {
        self.images.iter().find(|i| i.page == page)
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// What the derived blob is called: `<digest>.pages.json`.
pub const WHAT: &str = "pages.json";

/// The most pages of one PDF drawn, and so the most it can show. A scan
/// of a hundred pages would cost more in tiles than the whole request
/// carries; the rest keep their line, which says so.
pub const MAX_PAGES: usize = 8;

/// How one page is drawn: its PDF's bytes and the page, from 1, to a PNG
/// or to why there is none, in words. The daemon's is [`POPPLER`].
pub type Draw = fn(&[u8], u32) -> Result<Vec<u8>, String>;

/// The renderer a daemon draws with: theseus-files' external one, which is
/// absent on a machine with no renderer and says so.
pub const POPPLER: Draw = theseus_files::render::page;

/// The pages of a PDF that hold no text, from 1, at most [`MAX_PAGES`] of
/// them: the pages a scan leaves blank for a model that reads its text.
pub fn blank_pages(texts: &Texts) -> Vec<u32> {
    texts
        .iter()
        .enumerate()
        .filter(|(_, s)| s.text.trim().is_empty())
        .map(|(i, _)| i as u32 + 1)
        .take(MAX_PAGES)
        .collect()
}

/// What a PDF's pages were kept as, when any were; `None` for every PDF
/// that drew nothing, which is every one until the key is on.
pub fn of(blobs: Option<&Blobs>, digest: &str) -> Option<PageImages> {
    let raw = blobs?.derived(digest, WHAT)?;
    let kept: PageImages = serde_json::from_slice(&raw).ok()?;
    match kept.is_empty() {
        true => None,
        false => Some(kept),
    }
}

/// What one PDF's rendering did, for its `file.read` row and the words of
/// a failure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drawn {
    /// How many pages were drawn and kept.
    pub pages: usize,
    /// Why a page drew nothing, when one did not: the first such reason,
    /// in the renderer's own words.
    pub why: Option<String>,
}

impl Drawn {
    /// Keep the first reason a page drew nothing: every later page of the
    /// same file would say the same thing.
    fn said(&mut self, why: String) {
        if self.why.is_none() {
            self.why = Some(why);
        }
    }
}

/// Draw the pages of `bytes` that hold no text and keep them beside it.
///
/// `digest` is the PDF's own blob; `texts` its text by page, as the node's
/// text blob holds it. It draws nothing for a PDF whose every page has
/// text, nothing past [`MAX_PAGES`], and nothing at all when `draw` finds
/// no renderer — which it says, once, in `Drawn::why`.
///
/// It blocks on a child process and on the disk: call it where
/// `keep_file`'s own conversion is called, inside a blocking section.
pub fn keep(bytes: &[u8], digest: &str, texts: &Texts, blobs: &Blobs, draw: Draw) -> Drawn {
    let want = blank_pages(texts);
    let mut out = Drawn::default();
    if want.is_empty() {
        return out;
    }
    let mut kept = PageImages::default();
    for page in want {
        match draw(bytes, page) {
            Ok(png) => match crate::attach::store_image(&png, blobs) {
                Ok((info, digest)) => kept.images.push(PageImage {
                    page,
                    digest,
                    media_type: info.media_type.into(),
                    width: info.width,
                    height: info.height,
                }),
                Err(why) => out.said(why),
            },
            Err(why) => {
                // A renderer that is absent, or that cannot read this
                // file, says so once: every later page would say the same.
                out.said(why);
                break;
            }
        }
    }
    if kept.is_empty() {
        return out;
    }
    match serde_json::to_vec(&kept).map_err(|e| e.to_string()) {
        Ok(json) => match blobs.put_derived(digest, WHAT, &json) {
            Ok(()) => out.pages = kept.images.len(),
            Err(e) => out.said(format!("its pages could not be kept ({e})")),
        },
        Err(e) => out.said(e),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn texts(pages: &[&str]) -> Texts {
        Arc::new(
            pages
                .iter()
                .enumerate()
                .map(|(i, text)| crate::blobs::Section {
                    label: format!("page {}", i + 1),
                    text: (*text).to_string(),
                    images: Vec::new(),
                })
                .collect(),
        )
    }

    /// A stand-in renderer: one PNG per page, so a test needs no renderer
    /// on the machine.
    fn drew(_bytes: &[u8], page: u32) -> Result<Vec<u8>, String> {
        Ok(crate::attach::tests::png(40 + page, 30, page as usize))
    }

    fn drew_nothing(_bytes: &[u8], _page: u32) -> Result<Vec<u8>, String> {
        Err("no renderer is installed on this machine".to_string())
    }

    #[test]
    fn only_a_page_with_no_text_is_drawn_and_never_past_the_cap() {
        assert_eq!(blank_pages(&texts(&["a", "", "c", "  "])), vec![2, 4]);
        assert!(blank_pages(&texts(&["a", "b"])).is_empty());
        let many: Vec<&str> = vec![""; MAX_PAGES + 4];
        assert_eq!(blank_pages(&texts(&many)).len(), MAX_PAGES);
    }

    #[test]
    fn a_blank_page_is_kept_beside_its_pdf_and_read_back() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let blobs = Blobs::new(dir.path());
        let pdf = theseus_files::pdf::sample(&["the first page", ""]);
        let digest = blobs.put(&pdf).expect("stored");
        let drawn = keep(&pdf, &digest, &texts(&["the first page", ""]), &blobs, drew);
        assert_eq!(
            drawn,
            Drawn {
                pages: 1,
                why: None
            }
        );
        let kept = of(Some(&blobs), &digest).expect("its pages were kept");
        assert_eq!(kept.images.len(), 1, "one page, the blank one");
        let two = kept.of(2).expect("page 2");
        assert_eq!((two.page, two.media_type.as_str()), (2, "image/png"));
        assert_eq!((two.width, two.height), (42, 30));
        assert!(kept.of(1).is_none(), "the page with text drew nothing");
        // Its bytes are a blob of their own, readable as any image is.
        assert!(blobs.base64(&two.digest).is_some(), "its bytes are there");
    }

    #[test]
    fn a_pdf_with_text_on_every_page_draws_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let blobs = Blobs::new(dir.path());
        let pdf = theseus_files::pdf::sample(&["one", "two"]);
        let digest = blobs.put(&pdf).expect("stored");
        assert_eq!(
            keep(&pdf, &digest, &texts(&["one", "two"]), &blobs, drew),
            Drawn::default()
        );
        assert!(of(Some(&blobs), &digest).is_none(), "nothing was kept");
    }

    #[test]
    fn a_machine_with_no_renderer_is_said_and_keeps_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let blobs = Blobs::new(dir.path());
        let pdf = theseus_files::pdf::sample(&[""]);
        let digest = blobs.put(&pdf).expect("stored");
        let drawn = keep(&pdf, &digest, &texts(&[""]), &blobs, drew_nothing);
        assert_eq!(drawn.pages, 0);
        assert_eq!(
            drawn.why.as_deref(),
            Some("no renderer is installed on this machine")
        );
        assert!(of(Some(&blobs), &digest).is_none(), "nothing was kept");
    }
}

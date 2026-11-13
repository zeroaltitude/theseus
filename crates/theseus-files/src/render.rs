//! A PDF page as a picture, when a renderer is present (theseus-w68l): a
//! scan has no text to extract, so a model that reads no PDFs is given the
//! page itself. One page, one capped child, exactly as `media`'s tools run:
//! the renderer reads the PDF on its stdin and writes a PNG to its stdout,
//! so it needs no file of its own (`run_capped` forbids one).
//!
//! No renderer is vendored and no crate is added: poppler's `pdftocairo` is
//! used when the machine has it, and an absent one is said, never a failure
//! (`media`'s rule for ffmpeg and tesseract). The seam is this module's
//! `page`, so another renderer — a pure-Rust one, which would be a new
//! dependency and a licence entry — can take its place without the core
//! changing at all.

use std::process::Command;
use std::time::Duration;

use crate::convert::{self, Limits};
use crate::media;

/// The renderer this module runs: poppler's, which takes `- -` for a PDF on
/// stdin and a PNG on stdout. `pdftoppm`, its sibling, writes a file named
/// `-.png` instead and so cannot run under the caps.
pub const RENDERER: &str = "pdftocairo";

/// The long edge of a rendered page, in pixels. A vision model reads a
/// page's words at about this size and pays for its tiles: larger is dearer
/// and no clearer.
pub const LONG_EDGE_PX: u32 = 1100;

/// A page's render runs under its own caps: drawing a page is heavier than
/// a parse and lighter than video.
pub const RENDER_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(30),
    memory_bytes: 1024 * 1024 * 1024,
};

/// The most one page's PNG may be. A page of [`LONG_EDGE_PX`] is tens of
/// kilobytes; a megabyte is already a dense scan.
pub const MAX_PNG_BYTES: usize = 4 * 1024 * 1024;

/// PNG's first eight bytes.
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Whether this machine can render a page at all. A caller that renders
/// several pages asks once.
pub fn available() -> bool {
    media::which(RENDERER).is_some()
}

/// Why no renderer is here, in the words `media` uses for an absent tool.
pub fn absent() -> String {
    format!("{RENDERER} is not installed on this machine")
}

/// One page of `bytes` as a PNG (`page` is 1-based), or why there is none,
/// in words. The child gets the PDF on its stdin, writes the PNG to its
/// stdout, and is killed at [`RENDER_LIMITS`]' time.
pub fn page(bytes: &[u8], page: u32) -> Result<Vec<u8>, String> {
    let exe = media::which(RENDERER).ok_or_else(absent)?;
    let n = page.max(1).to_string();
    let mut cmd = Command::new(exe);
    cmd.args([
        "-png",
        "-singlefile",
        "-scale-to",
        &LONG_EDGE_PX.to_string(),
        "-f",
        &n,
        "-l",
        &n,
        "-",
        "-",
    ])
    .env_clear();
    let out = convert::run_capped(&mut cmd, bytes, RENDER_LIMITS)?;
    if !out.starts_with(PNG_MAGIC) {
        return Err(format!("{RENDERER} drew no picture of page {page}"));
    }
    if out.len() > MAX_PNG_BYTES {
        return Err(format!(
            "page {page} drew larger than its {} MiB limit",
            MAX_PNG_BYTES / (1024 * 1024)
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_renderer_is_said_not_failed() {
        assert_eq!(absent(), "pdftocairo is not installed on this machine");
        // The words are `media`'s own, so one reader of a missing tool is
        // enough.
        assert!(absent().ends_with("is not installed on this machine"));
    }

    #[test]
    fn a_page_with_no_text_still_draws_a_picture() {
        if !available() {
            // No renderer on this machine: the caller is told, and nothing
            // fails. `an_absent_renderer_is_said_not_failed` holds that.
            return;
        }
        let pdf = crate::pdf::sample(&["", "the second page"]);
        let png = page(&pdf, 1).expect("a page draws");
        assert!(png.starts_with(PNG_MAGIC), "a PNG");
        assert!(png.len() > 100, "not an empty picture: {} bytes", png.len());
        // Page 2 draws too, and the pages differ: the renderer is reading
        // the page it was asked for.
        let two = page(&pdf, 2).expect("a page draws");
        assert!(two.starts_with(PNG_MAGIC), "a PNG");
        assert_ne!(png, two, "each page draws itself");
    }

    #[test]
    fn bytes_that_are_no_pdf_are_refused_in_words() {
        if !available() {
            return;
        }
        let why = page(b"this is not a PDF at all", 1).expect_err("no picture");
        assert!(!why.is_empty(), "it says why");
    }
}

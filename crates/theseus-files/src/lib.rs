//! Files people give the model, read for it (theseus-c9l6): a PDF's pages
//! counted, its text extracted page by page, and a range of its pages cut out
//! as a PDF of its own (`pdf`); Word, Excel, PowerPoint, OpenDocument, EPUB,
//! RTF, and Jupyter notebooks read into sections (`doc`); archives listed and
//! one member read out (`archive`); what a file is (`kind`); and ffmpeg and
//! tesseract when they are present (`media`). Each conversion runs in a child
//! process with a time limit and a memory cap (`convert`), since a hostile
//! file is real.
//!
//! No store and no model here: the core keeps a file's bytes, and what was
//! made from them, in its blobs by digest, and renders them for each model
//! (`theseus-core`'s `attach`). `fs.read` (theseus-tools) and `http.fetch`
//! read PDFs through the same calls.

pub mod archive;
pub mod convert;
pub mod doc;
pub mod kind;
pub mod media;
pub mod pdf;
mod xml;

/// The largest file a surface accepts, and a tool reads, as a document: the
/// provider's limit on a request that carries a PDF (32 MB), and
/// `[tools] max_attachment_bytes`' default.
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

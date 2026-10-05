# theseus-files

Files people give the model, read for it (theseus-c9l6): a PDF's pages counted, its text extracted page by page,
and a range of its pages cut out as a PDF of its own; Word, Excel, PowerPoint, OpenDocument, EPUB, RTF, and Jupyter
notebooks read into sections; archives listed and a member read out; ffmpeg and tesseract when present. Every
conversion runs in a capped child process.

Key modules: `pdf.rs`, `doc.rs`, `archive.rs`, `kind.rs`, `media.rs`, `convert.rs`. Read by: theseus-core
(`attach.rs`, `file_read.rs`, `web/fetch.rs`), theseus-tools (`docs.rs`, `fs.read`), and theseusd (the converter's
role).

## What's here

- `src/pdf.rs`: `is_pdf` (`%PDF-` in the first 1,024 bytes), `Pages` (a range as a tool takes it: `3`, `3-5`,
  `3-`), and `read(bytes, &Ask) -> Read`: the page count, each page's text, and the pages asked for as a PDF of their
  own (`Ask::part`). Built on `lopdf` (MIT), default features off. `sample(&[text])` makes a small PDF for tests:
  tests build their PDFs with it, never from a committed binary.
- `src/kind.rs`: what a file is, from its first bytes and, for zips (Office, OpenDocument, EPUB), their members'
  names; the file's name breaks ties, never the type a sender said. Each kind's media type and noun.
- `src/doc.rs`: `read(bytes, kind) -> Doc`, sections with labels (`sheet Budget`, `slide 3: Tides`, `cell 4 (code)`,
  `chapter 2`) and a notebook's output images. Word keeps headings (`#`), lists, and tables; a sheet is a table
  (` | `), its trailing empty cells and rows dropped, at most 5,000 rows; a slide has its title and notes. Bounds: a
  zip member unpacks to 64 MiB at most, the text to 8 MiB. `sample_zip` builds test files. `src/xml.rs` is the small
  XML scanner it reads with (no dependency); `src/archive.rs` lists zips, tars, and gzipped tars as streams, and reads
  one member out only by a relative path that climbs nowhere (`safe_path`). Dependencies: `zip` (already in the
  lock; `deflate-flate2` only) and `flate2` (lopdf's too): no crate new to the workspace but lopdf's.
- `src/media.rs`: ffprobe (a recording's length), ffmpeg (a video's audio, a strip of its frames, a long recording cut
  to its cap), tesseract (OCR), each found on `PATH` or the usual places and run through `run_capped` with
  `MEDIA_LIMITS` (90 s, 2 GiB, two threads, an empty environment). An absent tool is said, never a failure.
- `src/convert.rs`: the one way to run a conversion. `convert::pdf(bytes, &ask)` returns what it made, or why it made
  nothing, in words, and how it ran (`Ran`: its time, and whether it was capped).
  - In a daemon, `theseusd` starts each conversion as a child of its own image in the `files-convert` role
    (`ROLE`), set once before serving by `use_child` with `Limits::DEFAULT`: 30 s of wall clock (then SIGKILL), 1 GiB
    of address space (`RLIMIT_AS`), CPU time just past the wall clock, no file it may write (`RLIMIT_FSIZE` 0), 64
    descriptors, an empty environment, and death with the daemon (`PR_SET_PDEATHSIG`). The child is spawned through
    the daemon's registry of children (`children::spawn`, `Kind::Owned`), so the reaper leaves its status to its
    waiter. The request (`Pdf`, `Doc`, `Extract`) and the file's bytes go on its stdin; its answer comes on stdout
    as JSON. `run_capped` is that runner, for any command: the system tools run through it too.
  - Anywhere else (tests, tools run outside a daemon), a conversion runs in the calling thread, without the caps.
  - Either way the caller blocks: call it from the CPU pool, a blocking task, or `theseus_store::blocking`.

## Invariants

- **A hostile file costs the child, never the daemon.** A decompression bomb fails first at `lopdf`'s bound on what
  one stream may inflate to (128 MiB), then at the child's address space; a slow file at the wall clock. A child that
  dies says which cap it met (`died`).
- **No store, no model.** The core keeps a file's bytes, and what was read of it (its text by page as a JSON blob,
  its parts), in the store's blobs by digest, and renders them for each model (`theseus-core/src/attach.rs`).
- **The text is the extractor's.** A page with no text layer (a scan) has empty text; the model that reads PDFs sees
  the page's picture, and the text path says the page had none. `MAX_TEXT_BYTES` (8 MiB) bounds one read's text.

## Tests

`cargo nextest run -p theseus-files`: the reader on `sample` PDFs (count, text, a range cut out and read again, a
range past the end, a torn file), the wire round trip, and the words for each cap. The child itself, with its caps,
is tested from theseusd (`crates/theseusd/tests/files.rs`), which can start the real binary in its role.

## Traps

- `use_child` is a `OnceLock`: the first call wins, for the process. A test that needs the child runs in a process of
  its own (nextest runs each test so).
- A debug build's child reads `THESEUS_FILES_PROBE` (`alloc:<bytes>`, `sleep:<ms>`), passed through from the parent,
  to prove the caps; a release build neither passes nor reads it.
- The child is `theseusd` itself, so it pays the binary's mappings against its address space: a debug binary's are
  hundreds of MiB of the 1 GiB.

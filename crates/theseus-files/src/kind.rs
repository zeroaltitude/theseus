//! What a file is (theseus-c9l6), from its first bytes and, for the formats
//! that are zip archives inside (Office, OpenDocument, EPUB), from the names
//! of their members; the file's name breaks a tie, never the type a sender
//! said. Each kind has the media type the node keeps.

/// A kind of file Theseus reads for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Pdf,
    Notebook,
    Docx,
    Xlsx,
    Pptx,
    Odt,
    Ods,
    Odp,
    Epub,
    Rtf,
    Zip,
    Tar,
    TarGz,
    Audio,
    Video,
    /// Anything else: kept, and saved at a path for `proc.run` on request.
    Other,
}

impl Kind {
    /// The media type the node keeps for it; `None` keeps the sender's.
    pub fn media_type(self) -> Option<&'static str> {
        Some(match self {
            Kind::Pdf => "application/pdf",
            Kind::Notebook => "application/x-ipynb+json",
            Kind::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Kind::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Kind::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Kind::Odt => "application/vnd.oasis.opendocument.text",
            Kind::Ods => "application/vnd.oasis.opendocument.spreadsheet",
            Kind::Odp => "application/vnd.oasis.opendocument.presentation",
            Kind::Epub => "application/epub+zip",
            Kind::Rtf => "application/rtf",
            Kind::Zip => "application/zip",
            Kind::Tar => "application/x-tar",
            Kind::TarGz => "application/gzip",
            Kind::Audio | Kind::Video | Kind::Other => return None,
        })
    }

    /// The word a file's line uses for it: `[Word document report.docx …]`.
    pub fn noun(self) -> &'static str {
        match self {
            Kind::Pdf => "PDF",
            Kind::Notebook => "Notebook",
            Kind::Docx | Kind::Odt => "Document",
            Kind::Xlsx | Kind::Ods => "Spreadsheet",
            Kind::Pptx | Kind::Odp => "Slides",
            Kind::Epub => "Book",
            Kind::Rtf => "Document",
            Kind::Zip | Kind::Tar | Kind::TarGz => "Archive",
            Kind::Audio => "Audio",
            Kind::Video => "Video",
            Kind::Other => "File",
        }
    }

    /// Its text is made when it arrives: every kind but audio, video, and
    /// what is not read at all.
    pub fn has_text(self) -> bool {
        !matches!(self, Kind::Audio | Kind::Video | Kind::Other)
    }

    /// The kind a media type the node kept names (`media_type`'s inverse),
    /// with audio and video by their family.
    pub fn of_media_type(t: &str) -> Kind {
        let all = [
            Kind::Pdf,
            Kind::Notebook,
            Kind::Docx,
            Kind::Xlsx,
            Kind::Pptx,
            Kind::Odt,
            Kind::Ods,
            Kind::Odp,
            Kind::Epub,
            Kind::Rtf,
            Kind::Zip,
            Kind::Tar,
            Kind::TarGz,
        ];
        if let Some(k) = all.into_iter().find(|k| k.media_type() == Some(t)) {
            return k;
        }
        if t.starts_with("audio/") {
            Kind::Audio
        } else if t.starts_with("video/") {
            Kind::Video
        } else {
            Kind::Other
        }
    }
}

fn ext(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The media type an audio or video file's bytes say, for a node whose
/// sender said none.
pub fn av_media_type(bytes: &[u8], name: &str) -> Option<&'static str> {
    let e = ext(name);
    if bytes.starts_with(b"ID3")
        || bytes.starts_with(&[0xff, 0xfb])
        || bytes.starts_with(&[0xff, 0xf3])
    {
        return Some("audio/mpeg");
    }
    if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Some("audio/wav");
    }
    if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"AVI " {
        return Some("video/x-msvideo");
    }
    if bytes.starts_with(b"OggS") {
        return Some(if e == "ogv" { "video/ogg" } else { "audio/ogg" });
    }
    if bytes.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some(match e.as_str() {
            "weba" => "audio/webm",
            "mkv" => "video/x-matroska",
            _ if e == "webm" => "video/webm",
            _ => "video/x-matroska",
        });
    }
    if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        return Some(match brand {
            b"M4A " | b"M4B " => "audio/mp4",
            b"qt  " => "video/quicktime",
            _ if matches!(e.as_str(), "m4a" | "m4b") => "audio/mp4",
            _ => "video/mp4",
        });
    }
    None
}

/// What `bytes`, named `name`, is. `members` lists a zip's member names
/// (empty for any other file); the caller reads them, since a zip's
/// directory is at its end.
pub fn sniff(bytes: &[u8], name: &str, members: &[String]) -> Kind {
    let e = ext(name);
    if crate::pdf::is_pdf(bytes) {
        return Kind::Pdf;
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        let has = |m: &str| members.iter().any(|x| x == m);
        let starts = |p: &str| members.iter().any(|x| x.starts_with(p));
        if has("word/document.xml") {
            return Kind::Docx;
        }
        if has("xl/workbook.xml") {
            return Kind::Xlsx;
        }
        if starts("ppt/slides/") {
            return Kind::Pptx;
        }
        if has("META-INF/container.xml") && (has("mimetype") || e == "epub") && !has("content.xml")
        {
            return Kind::Epub;
        }
        if has("content.xml") {
            return match e.as_str() {
                "ods" => Kind::Ods,
                "odp" => Kind::Odp,
                _ if starts("Object") && e != "odt" => Kind::Odt,
                _ => Kind::Odt,
            };
        }
        return Kind::Zip;
    }
    if bytes.starts_with(b"{\\rtf") {
        return Kind::Rtf;
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Kind::TarGz;
    }
    if bytes.len() > 262 && &bytes[257..262] == b"ustar" {
        return Kind::Tar;
    }
    if e == "ipynb" || (bytes.trim_ascii_start().starts_with(b"{") && looks_like_notebook(bytes)) {
        return Kind::Notebook;
    }
    match av_media_type(bytes, name) {
        Some(t) if t.starts_with("audio/") => Kind::Audio,
        Some(_) => Kind::Video,
        None => Kind::Other,
    }
}

fn looks_like_notebook(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let s = String::from_utf8_lossy(head);
    s.contains("\"cells\"") && s.contains("\"nbformat\"")
        || s.contains("\"nbformat\"") && s.contains("\"metadata\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_known_by_its_bytes_and_its_members() {
        let m = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(sniff(b"%PDF-1.7", "x.bin", &[]), Kind::Pdf);
        assert_eq!(
            sniff(
                b"PK\x03\x04...",
                "a.docx",
                &m(&["[Content_Types].xml", "word/document.xml"])
            ),
            Kind::Docx
        );
        assert_eq!(
            sniff(b"PK\x03\x04...", "a", &m(&["xl/workbook.xml"])),
            Kind::Xlsx
        );
        assert_eq!(
            sniff(b"PK\x03\x04...", "a", &m(&["ppt/slides/slide1.xml"])),
            Kind::Pptx
        );
        assert_eq!(
            sniff(
                b"PK\x03\x04...",
                "b.epub",
                &m(&["mimetype", "META-INF/container.xml", "OEBPS/c1.xhtml"])
            ),
            Kind::Epub
        );
        assert_eq!(
            sniff(b"PK\x03\x04...", "s.ods", &m(&["mimetype", "content.xml"])),
            Kind::Ods
        );
        assert_eq!(
            sniff(b"PK\x03\x04...", "t.odt", &m(&["mimetype", "content.xml"])),
            Kind::Odt
        );
        assert_eq!(
            sniff(b"PK\x03\x04...", "src.zip", &m(&["src/main.rs"])),
            Kind::Zip
        );
        assert_eq!(sniff(b"{\\rtf1\\ansi hello}", "x", &[]), Kind::Rtf);
        assert_eq!(sniff(&[0x1f, 0x8b, 8, 0], "x.tgz", &[]), Kind::TarGz);
        assert_eq!(
            sniff(
                b"{\n \"cells\": [], \"metadata\": {}, \"nbformat\": 4}",
                "n.json",
                &[]
            ),
            Kind::Notebook
        );
        assert_eq!(sniff(b"ID3\x04 more", "memo", &[]), Kind::Audio);
        assert_eq!(
            sniff(b"OggS\x00\x02", "voice-message.ogg", &[]),
            Kind::Audio
        );
        assert_eq!(
            sniff(b"\x00\x00\x00\x18ftypisom....", "clip.mp4", &[]),
            Kind::Video
        );
        assert_eq!(
            sniff(b"\x00\x00\x00\x18ftypM4A ....", "memo.m4a", &[]),
            Kind::Audio
        );
        assert_eq!(sniff(b"\x7fELF", "tool", &[]), Kind::Other);
        let mut tar = vec![0u8; 600];
        tar[257..262].copy_from_slice(b"ustar");
        assert_eq!(sniff(&tar, "a.tar", &[]), Kind::Tar);
        assert_eq!(Kind::of_media_type("audio/ogg"), Kind::Audio);
        assert_eq!(
            Kind::of_media_type(Kind::Docx.media_type().unwrap()),
            Kind::Docx
        );
    }
}

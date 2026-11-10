//! A file as `turn.submit` carries it (theseus-9g2, theseus-c9l6): the one
//! builder `theseus ask --attach` and the chat's `@image.png` share
//! (theseus-vjzt, phase 2), with its 32 MiB cap.

use anyhow::{anyhow, Context, Result};

/// The largest file `ask --attach` sends: the daemon's default
/// `[tools] max_attachment_bytes` (theseus-c9l6), which it applies again; it
/// caps text further, at `[tools].max_read_bytes`.
pub const MAX_ATTACH_BYTES: u64 = 32 * 1024 * 1024;

/// One `--attach` file as `turn.submit` carries it (theseus-9g2): a text
/// file's text, any other file's bytes for the daemon to keep and read (an
/// image, a PDF; theseus-c9l6), or the file listed with the reason it was
/// not read. A file that cannot be opened stops the command, before
/// anything is sent.
pub fn attachment_for(path: &std::path::Path) -> Result<theseus_protocol::Attachment> {
    attachment_labeled(path, "--attach")
}

/// [`attachment_for`] with the word its errors and its over-the-cap reason
/// use for the act (`--attach` for the CLI's flag; the chat's `@image`).
pub fn attachment_labeled(
    path: &std::path::Path,
    label: &str,
) -> Result<theseus_protocol::Attachment> {
    let meta = std::fs::metadata(path).with_context(|| format!("{label} {}", path.display()))?;
    if meta.is_dir() {
        return Err(anyhow!("{label} {}: is a directory", path.display()));
    }
    let mut a = theseus_protocol::Attachment {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        size: meta.len(),
        ..Default::default()
    };
    if meta.len() > MAX_ATTACH_BYTES {
        a.not_read = Some(format!("over the 32 MiB limit for {label}"));
        return Ok(a);
    }
    let bytes = std::fs::read(path).with_context(|| format!("{label} {}", path.display()))?;
    // A notebook or an RTF file is text the daemon reads as a document: its
    // bytes, whole.
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "ipynb" | "rtf") {
        use base64::Engine as _;
        a.data = Some(base64::engine::general_purpose::STANDARD.encode(&bytes));
        return Ok(a);
    }
    let bytes = match String::from_utf8(bytes) {
        Ok(text) if !text.as_bytes().iter().take(8192).any(|b| *b == 0) => {
            a.media_type = "text/plain".into();
            a.text = Some(text);
            return Ok(a);
        }
        Ok(text) => text.into_bytes(),
        Err(e) => e.into_bytes(),
    };
    // Not text: its bytes. The daemon reads the type from them, keeps an
    // image as an image and any other file whole, and reads a PDF.
    use base64::Engine as _;
    a.data = Some(base64::engine::general_purpose::STANDARD.encode(&bytes));
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_sends_a_text_files_text_and_lists_anything_else() {
        let dir = std::env::temp_dir().join(format!("theseus-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "The fact is 42.\n").unwrap();
        let a = attachment_for(&notes).unwrap();
        assert_eq!(
            (a.name.as_str(), a.media_type.as_str(), a.size),
            ("notes.txt", "text/plain", 16)
        );
        assert_eq!(a.text.as_deref(), Some("The fact is 42.\n"));
        assert!(a.not_read.is_none());

        // Not text: its bytes go, and the daemon keeps them only if they are an image.
        let shot = dir.join("shot.png");
        std::fs::write(&shot, b"\x89PNG\r\n\x1a\n").unwrap();
        let b = attachment_for(&shot).unwrap();
        assert_eq!(b.data.as_deref(), Some("iVBORw0KGgo="));
        assert!(b.text.is_none() && b.not_read.is_none());

        assert!(attachment_for(&dir.join("missing.txt")).is_err());
        assert!(attachment_for(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

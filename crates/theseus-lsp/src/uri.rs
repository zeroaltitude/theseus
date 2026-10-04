//! `file://` URIs, made from paths and read back.
//!
//! Servers spell one file's URI differently (an encoded `%3A`, a percent
//! escape in another case), so the client keys documents and diagnostics by
//! [`normalize`]d URIs: read as a path, then written again.

use std::path::{Path, PathBuf};

/// The `file://` URI of an absolute path; `None` for a relative one.
pub fn from_path(path: &Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(String::from)
}

/// The path a `file://` URI names; `None` for any other scheme.
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let u = url::Url::parse(uri).ok()?;
    if u.scheme() != "file" {
        return None;
    }
    u.to_file_path().ok()
}

/// One spelling of a URI: a file's as [`from_path`] writes it, any other as
/// given.
pub fn normalize(uri: &str) -> String {
    to_path(uri)
        .and_then(|p| from_path(&p))
        .unwrap_or_else(|| uri.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_uris_round_trip() {
        let p = Path::new("/srv/ledger app/src/räkna.py");
        let u = from_path(p).unwrap();
        assert_eq!(u, "file:///srv/ledger%20app/src/r%C3%A4kna.py");
        assert_eq!(to_path(&u).unwrap(), p);
        assert_eq!(from_path(Path::new("relative.py")), None);
        assert_eq!(to_path("untitled:Untitled-1"), None);
        assert_eq!(normalize("file:///srv/ledger%20app/src/r%c3%a4kna.py"), u);
        assert_eq!(normalize("untitled:Untitled-1"), "untitled:Untitled-1");
    }
}

//! A prompt file's change in the history (issue 23): what `config.history`
//! says of it, from the ledger row the daemon wrote and the copies it kept,
//! never from anything else. The text before a change is its kept copy, and
//! only if its digest is the one the row recorded; the text after it is the
//! next copy of the file that digest names, or the file as it is now.

use std::path::Path;

use serde_json::Value;

use crate::config_change::shown::Facts;
use crate::config_change::write;
use crate::config_copy::sha256;

/// One prompt file's change.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    /// The file's text before the change, when its kept copy is still the one
    /// the row recorded.
    pub before: Option<String>,
    /// And after it, when a later copy, or the file, still is.
    pub after: Option<String>,
    pub before_sha: String,
    pub after_sha: String,
    pub before_words: String,
    pub after_words: String,
    pub added: u64,
    pub removed: u64,
}

fn words(v: &Value) -> String {
    Facts {
        bytes: v["bytes"].as_u64().unwrap_or_default() as usize,
        lines: v["lines"].as_u64().unwrap_or_default() as usize,
        fingerprint: v["fingerprint"].as_str().unwrap_or_default().to_string(),
        unseen: 0,
    }
    .words()
}

/// The prompt file a `config.changed` row `d` records, or none for a change to
/// the settings. `later` are the numbers of the prompt files' copies kept
/// after it.
pub fn entry_of(d: &Value, state: &Path, later: &[u32]) -> Option<FileEntry> {
    let f = d.get("file").filter(|f| f.is_object())?;
    let n = d["previous_copy"].as_u64()? as u32;
    let sha = |k: &str| d[k].as_str().unwrap_or_default().to_string();
    let path = f["path"].as_str()?.to_string();
    let (before_sha, after_sha) = (sha("digest_before"), sha("digest_after"));
    let before = write::file_copy(state, n).filter(|t| sha256(t) == before_sha);
    let now = super::read_text(Path::new(&path)).ok();
    let after = later
        .iter()
        .filter(|c| **c > n)
        .take(write::KEEP)
        .filter_map(|c| write::file_copy(state, *c))
        .chain(now)
        .find(|t| sha256(t) == after_sha);
    Some(FileEntry {
        path,
        before,
        after,
        before_sha,
        after_sha,
        before_words: words(&f["before"]),
        after_words: words(&f["after"]),
        added: f["added"].as_u64().unwrap_or_default(),
        removed: f["removed"].as_u64().unwrap_or_default(),
    })
}

/// What history says of the change itself, after its heading.
pub fn lines(f: &FileEntry) -> String {
    let mut out = format!(
        "- the prompt file {}: {} → {}; {}, {}\n",
        crate::config_change::shown::escape_line(&f.path).0,
        f.before_words,
        f.after_words,
        crate::narrative::count(f.added, "line added", "lines added"),
        crate::narrative::count(f.removed, "line removed", "lines removed"),
    );
    if f.before.is_none() {
        out.push_str(
            "  (the text from before this change is no longer kept, so it cannot be undone)\n",
        );
    }
    out
}

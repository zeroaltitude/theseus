//! `files.restore`: the files a session's edits changed since a message, put
//! back, newest first, and only where a file still holds what the last edit
//! wrote. The restore's own changes are pre-images too, so a restore is
//! undone by a rewind to the position before it.

use std::collections::HashMap;

use anyhow::Result;
use theseus_protocol::RestoredFile;

use super::{blobs_in, counts, rows, Before, PreImage};
use crate::blobs::digest;
use crate::store::Store;

/// What a restore did (or, on a dry run, would do).
pub(crate) struct Restored {
    pub files: Vec<RestoredFile>,
    /// The restore's own pre-images: the caller writes them with its ledger
    /// row in one frame.
    pub undo: Vec<PreImage>,
}

fn skipped(path: &str, why: impl Into<String>) -> RestoredFile {
    RestoredFile {
        path: path.into(),
        outcome: "skipped".into(),
        why: why.into(),
    }
}

/// What a restore carries from file to file.
struct Run<'a> {
    session_id: &'a str,
    call: String,
    dry_run: bool,
    at_ms: u64,
    blobs: crate::blobs::Blobs,
}

/// The restore of `session_id`'s files to before the position `since`.
pub(crate) fn restore(
    store: &Store,
    session_id: &str,
    since: u64,
    dry_run: bool,
    at_ms: u64,
) -> Result<Restored> {
    let mut by_path: HashMap<String, Vec<(u64, PreImage)>> = HashMap::new();
    for (pos, p) in rows(store, session_id)? {
        if pos > since {
            by_path.entry(p.path.clone()).or_default().push((pos, p));
        }
    }
    // Newest change first.
    let mut paths: Vec<_> = by_path.into_iter().collect();
    paths.sort_by_key(|(_, v)| std::cmp::Reverse(v.last().map_or(0, |(p, _)| *p)));
    let run = Run {
        session_id,
        call: format!("restore_{at_ms}"),
        dry_run,
        at_ms,
        blobs: blobs_in(store.dir()),
    };
    let mut out = Restored {
        files: Vec::new(),
        undo: Vec::new(),
    };
    for (path, edits) in paths {
        let (shown, undo) = run.file(path, &edits);
        out.files.push(shown);
        out.undo.extend(undo);
    }
    Ok(out)
}

impl Run<'_> {
    /// One file: its earlier bytes back where it still holds what the last
    /// edit wrote, else why not.
    fn file(&self, path: String, edits: &[(u64, PreImage)]) -> (RestoredFile, Option<PreImage>) {
        let (first, last) = (&edits[0].1, &edits[edits.len() - 1].1);
        let p = std::path::Path::new(&path);
        let now = match std::fs::symlink_metadata(p) {
            Ok(m) if m.is_file() => std::fs::read(p).ok(),
            Ok(_) => {
                return (
                    skipped(&path, "changed since: it is not a regular file now"),
                    None,
                )
            }
            Err(_) => None,
        };
        // The file must still be what the last edit left.
        if now.as_deref().map(digest) != last.after {
            return (
                skipped(
                    &path,
                    "changed since: it no longer holds what the last edit wrote, so it was not restored",
                ),
                None,
            );
        }
        let target: Option<Vec<u8>> = match &first.before {
            Before::Kept { digest: d, .. } => match self.blobs.read(d) {
                Some(b) => Some(b),
                None => {
                    return (
                        skipped(&path, "its saved copy is gone, so it was not restored"),
                        None,
                    )
                }
            },
            Before::Absent => None,
            Before::TooBig { bytes } => {
                return (
                    skipped(
                        &path,
                        format!(
                            "it was {bytes} bytes before the edit, over 1 MiB: no copy was kept"
                        ),
                    ),
                    None,
                )
            }
            Before::Unread { why } => {
                return (skipped(&path, format!("no copy was kept ({why})")), None)
            }
        };
        let word =
            |done: &'static str, would: &'static str| if self.dry_run { would } else { done };
        // Already as it was before the message: nothing to write.
        if now.as_deref() == target.as_deref() {
            let shown = RestoredFile {
                path,
                outcome: word("restored", "would restore").into(),
                why: "already as it was".into(),
            };
            return (shown, None);
        }
        // The restore is undoable: keep what the file holds now first.
        let undo_before = match self.keep_now(&now) {
            Ok(b) => b,
            Err(why) => return (skipped(&path, why), None),
        };
        if !self.dry_run {
            let wrote = match &target {
                Some(bytes) => theseus_tools::fs::write_atomic(p, bytes, None),
                None => std::fs::remove_file(p),
            };
            if let Err(e) = wrote {
                return (skipped(&path, format!("could not be written: {e}")), None);
            }
        }
        let (added, removed) = counts(p, now.as_deref(), target.as_deref());
        let undo = PreImage {
            session_id: self.session_id.into(),
            call_id: self.call.clone(),
            tool: "files.restore".into(),
            path: path.clone(),
            before: undo_before,
            after: target.as_deref().map(digest),
            added,
            removed,
            at_ms: self.at_ms,
        };
        let outcome = match &target {
            Some(_) => word("restored", "would restore"),
            None => word("removed", "would remove"),
        };
        let shown = RestoredFile {
            path,
            outcome: outcome.into(),
            why: String::new(),
        };
        (shown, Some(undo))
    }

    /// What the file holds now, kept for the restore's undo (a dry run keeps
    /// nothing, only says what it would).
    fn keep_now(&self, now: &Option<Vec<u8>>) -> Result<Before, String> {
        match now {
            Some(b) if b.len() as u64 > super::MAX_KEPT => {
                Err("it is over 1 MiB now, so a restore could not be undone: not restored".into())
            }
            Some(b) if !self.dry_run => self
                .blobs
                .put(b)
                .map(|d| Before::Kept {
                    digest: d,
                    bytes: b.len() as u64,
                })
                .map_err(|e| format!("a copy for undo was not kept: {e}")),
            Some(b) => Ok(Before::Kept {
                digest: digest(b),
                bytes: b.len() as u64,
            }),
            None => Ok(Before::Absent),
        }
    }
}

/// The result in words, for every surface.
pub(crate) fn words(files: &[RestoredFile], commands: u32, dry_run: bool) -> String {
    let done: Vec<&str> = files
        .iter()
        .filter(|f| f.outcome != "skipped")
        .map(|f| f.path.as_str())
        .collect();
    let skipped = files.len() - done.len();
    let verb = if dry_run { "Would restore" } else { "Restored" };
    let mut s = match done.len() {
        0 => "No file was restored".to_string(),
        n => format!("{verb} {n} file{}", if n == 1 { "" } else { "s" }),
    };
    if skipped > 0 {
        s += &format!("; {skipped} skipped, with why");
    }
    if commands > 0 {
        s += &format!(
            ". {commands} command{} ran after that message: the files they changed are not restored",
            if commands == 1 { "" } else { "s" }
        );
    }
    s
}

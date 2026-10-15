//! Rewind (theseus-23rk, R20): put the files an edit tool changed, and the
//! conversation, back to before a chosen message.
//!
//! **Pre-images.** Before `fs.edit`, `fs.write` or `fs.patch` changes a file,
//! the file's prior bytes are kept as a blob (`Blobs::put`, synced before the
//! edit runs) and, once the edit has run, one `PRE_IMAGE` record per file
//! rides in the edit's own completion frame: the prior state (`Before`), the
//! digest of the bytes the tool wrote (`after`), and the lines it added and
//! removed. A file over `MAX_KEPT` (1 MiB) is noted, not kept. A crash after
//! the blob and before the edit leaves a blob nothing names; a crash after the
//! edit and before its frame leaves an edit with no record, which a restore
//! reads as a file changed by someone else and lists. Neither breaks a restore.
//!
//! **Restore** (`restore.rs`): for each file the session's edits changed
//! since a message, newest first, its first change's prior bytes go back, but
//! only where the file still holds what the last edit wrote. **Points**
//! (`points.rs`): the person's messages with the files changed since each,
//! for the picker. The conversation's half is `session.fork`, called by the
//! surface after the restore (`rpc/rewind.rs`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_store::{kinds, NewRecord};
use theseus_tools::{Tool, ToolCtx};

use crate::blobs::{digest, Blobs};

mod points;
mod restore;

pub(crate) use points::{commands_after, points};
pub(crate) use restore::{restore, words};

/// The most a kept file holds: a bigger one is noted, not kept.
pub const MAX_KEPT: u64 = 1 << 20;

/// The tools whose changes are kept and restored. A command's changes are
/// never known, so `proc.run` is not here.
pub const FILE_TOOLS: [&str; 3] = ["fs.edit", "fs.write", "fs.patch"];

/// The scope a session's pre-images are filed under.
pub fn scope_of(session_id: &str) -> String {
    format!("rewind:{session_id}")
}

/// What a file held before an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Before {
    /// Its bytes, in the blobs under this digest.
    Kept { digest: String, bytes: u64 },
    /// It did not exist: the edit made it.
    Absent,
    /// Over `MAX_KEPT`: not kept.
    TooBig { bytes: u64 },
    /// Not a regular file, or unreadable: not kept.
    Unread { why: String },
}

/// One file one edit changed, as the store holds it (`kinds::PRE_IMAGE`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreImage {
    pub session_id: String,
    /// The call (`tool_use` id), or the restore's own marker.
    pub call_id: String,
    /// `fs.edit`, `fs.write`, `fs.patch` or `files.restore`.
    pub tool: String,
    pub path: String,
    pub before: Before,
    /// The digest of the bytes the edit left; none when the file is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub added: u32,
    pub removed: u32,
    pub at_ms: u64,
}

impl PreImage {
    pub fn record(&self, n: usize) -> anyhow::Result<NewRecord> {
        let key = format!("pre_{}_{n}", self.call_id);
        Ok(
            NewRecord::json(kinds::PRE_IMAGE, Some(&key), self)?
                .scoped(&scope_of(&self.session_id)),
        )
    }
}

/// What a call needs to keep its files, owned so it can move to the core that
/// runs the tool.
pub struct Rewind {
    dir: PathBuf,
    session_id: String,
    call_id: String,
}

impl Rewind {
    pub fn of(tc: &crate::toolrun::TurnCtx<'_>, call_id: &str) -> Self {
        Self {
            dir: tc.store.dir().to_path_buf(),
            session_id: tc.session_id.to_string(),
            call_id: call_id.to_string(),
        }
    }

    pub fn hold(&self, tool: &dyn Tool, input: &Value, ctx: &ToolCtx) -> Held {
        hold(&blobs_in(&self.dir), tool, input, ctx)
    }

    pub fn settle(&self, held: Held, tool: &str) -> Vec<PreImage> {
        held.settle(
            &self.session_id,
            &self.call_id,
            tool,
            theseus_protocol::now_unix_ms(),
        )
    }
}

/// The records of some pre-images, for a frame.
pub fn records(all: &[PreImage]) -> anyhow::Result<Vec<NewRecord>> {
    all.iter().enumerate().map(|(n, p)| p.record(n)).collect()
}

/// The files a call is about to change, with what each held.
pub struct Held {
    files: Vec<(PathBuf, Before, Option<Vec<u8>>)>,
}

/// Whether a tool's changes are kept.
pub fn keeps(tool: &str) -> bool {
    FILE_TOOLS.contains(&tool)
}

/// What `path` holds now: its bytes when it is a regular file within the cap.
fn state_of(path: &Path, blobs: &Blobs) -> (Before, Option<Vec<u8>>) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Before::Absent, None),
        Err(e) => return (Before::Unread { why: e.to_string() }, None),
    };
    if !meta.is_file() {
        return (
            Before::Unread {
                why: "not a regular file".into(),
            },
            None,
        );
    }
    if meta.len() > MAX_KEPT {
        return (Before::TooBig { bytes: meta.len() }, None);
    }
    match std::fs::read(path) {
        Ok(bytes) => match blobs.put(&bytes) {
            Ok(d) => (
                Before::Kept {
                    digest: d,
                    bytes: bytes.len() as u64,
                },
                Some(bytes),
            ),
            Err(e) => (
                Before::Unread {
                    why: format!("not kept: {e}"),
                },
                None,
            ),
        },
        Err(e) => (Before::Unread { why: e.to_string() }, None),
    }
}

/// Keep what the files a call will write hold, before it runs. The blob is
/// durable when this returns, ahead of the edit. A call that plans no write
/// holds nothing. Never fails: a file it cannot keep is noted in its state.
pub fn hold(blobs: &Blobs, tool: &dyn Tool, input: &Value, ctx: &ToolCtx) -> Held {
    let mut files = Vec::new();
    if !keeps(tool.name()) {
        return Held { files };
    }
    let Ok(plan) = tool.plan(input, ctx) else {
        return Held { files };
    };
    for r in plan
        .resources
        .iter()
        .filter(|r| r.access == theseus_tools::Access::Write)
    {
        if files.iter().any(|(p, _, _): &(PathBuf, _, _)| *p == r.path) {
            continue;
        }
        let (before, bytes) = state_of(&r.path, blobs);
        files.push((r.path.clone(), before, bytes));
    }
    Held { files }
}

impl Held {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// After the call ran: a record for each file it changed (or made, or
    /// removed), with the digest of what it left and the lines it counted.
    pub fn settle(self, session_id: &str, call_id: &str, tool: &str, at_ms: u64) -> Vec<PreImage> {
        let mut out = Vec::new();
        for (path, before, old) in self.files {
            let now = std::fs::read(&path).ok();
            let after = now.as_deref().map(digest);
            let same = match (&before, &after) {
                (Before::Kept { digest: d, .. }, Some(a)) => d == a,
                (Before::Absent, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            let (added, removed) = counts(&path, old.as_deref(), now.as_deref());
            out.push(PreImage {
                session_id: session_id.into(),
                call_id: call_id.into(),
                tool: tool.into(),
                path: path.display().to_string(),
                before,
                after,
                added,
                removed,
                at_ms,
            });
        }
        out
    }
}

/// Lines added and removed between two texts; none counted for bytes that
/// are not text.
pub(crate) fn counts(path: &Path, old: Option<&[u8]>, new: Option<&[u8]>) -> (u32, u32) {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let text = |b: Option<&[u8]>| b.map(|b| std::str::from_utf8(b).ok().map(str::to_string));
    match (text(old), text(new)) {
        (Some(Some(o)), Some(Some(n))) => {
            theseus_protocol::diff::stat(&theseus_tools::change::file_diff(&name, Some(&o), &n))
        }
        (None, Some(Some(n))) => {
            theseus_protocol::diff::stat(&theseus_tools::change::file_diff(&name, None, &n))
        }
        (Some(Some(o)), None) => (0, o.lines().count() as u32),
        _ => (0, 0),
    }
}

/// A session's pre-images in the order they were written, as (position, row):
/// a fork's include its source's through the fork point first, so a rewind in
/// a fork reaches what the conversation it inherited changed.
pub(crate) fn rows(
    store: &crate::store::Store,
    session_id: &str,
) -> anyhow::Result<Vec<(u64, PreImage)>> {
    let mut chain: Vec<(String, u64)> = vec![(session_id.to_string(), u64::MAX)];
    let mut at = session_id.to_string();
    while chain.len() < 16 {
        let Some(rec) = store.get_session::<crate::session::SessionRecord>(&at)? else {
            break;
        };
        let Some(origin) = rec.forked_from else { break };
        chain.push((origin.session.clone(), origin.position));
        at = origin.session;
    }
    let mut out = Vec::new();
    for (sid, through) in chain.into_iter().rev() {
        for r in store.scope_after(&scope_of(&sid), 0)? {
            if r.kind == kinds::PRE_IMAGE && r.position <= through {
                out.push((r.position, r.decode::<PreImage>()?));
            }
        }
    }
    Ok(out)
}

/// The pre-images' blobs: their own directory beside the store's blobs
/// (`<store>/rewind/blobs/`), so no attachment's digest is ever shared with
/// one and an erase deletes only its own.
pub fn blobs_in(store_dir: &Path) -> Blobs {
    Blobs::new(&store_dir.join("rewind"))
}

/// Delete the blobs a session's own pre-images name (an erased session's go
/// with it), leaving any another session's pre-image also names. Returns how
/// many files went.
pub fn forget(store: &crate::store::Store, session_id: &str) -> anyhow::Result<usize> {
    use theseus_store::Store as _;
    let kept = |p: &PreImage| match &p.before {
        Before::Kept { digest, .. } => Some(digest.clone()),
        _ => None,
    };
    // Most sessions made no edit: one scope read says so.
    let mine = store.scope_after(&scope_of(session_id), 0)?;
    if mine.is_empty() {
        return Ok(0);
    }
    let others: std::collections::HashSet<String> = store
        .inner()
        .latest_of_kind(kinds::PRE_IMAGE)?
        .iter()
        .filter_map(|r| r.decode::<PreImage>().ok())
        .filter(|p| p.session_id != session_id)
        .filter_map(|p| kept(&p))
        .collect();
    let blobs = blobs_in(store.dir());
    let mut gone = 0;
    for r in mine {
        let Ok(p) = r.decode::<PreImage>() else {
            continue;
        };
        let Some(d) = kept(&p) else { continue };
        if !others.contains(&d) && std::fs::remove_file(blobs.path(&d)).is_ok() {
            gone += 1;
        }
    }
    Ok(gone)
}

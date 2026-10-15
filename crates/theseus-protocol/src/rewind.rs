//! Rewind (theseus-23rk, the killer-features plan's R20): put the files an
//! agent's own edit tools changed back to before a chosen message.
//!
//! `files.changes` lists the person's messages of a session, each with the
//! files the session's edits changed since it. `files.restore` puts those
//! files back, newest first, and only where a file still holds the bytes
//! Theseus's last edit wrote: anything else is listed and left alone. The
//! conversation's half is `session.fork` at the node before the message
//! (`fork.rs`); a surface calls the two in turn.

use serde::{Deserialize, Serialize};

/// `files.changes`' params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FilesChangesParams {
    pub session_id: String,
    /// Only this message (a node as `node.reach` names one): the point for it,
    /// wherever it is in the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub node: Option<String>,
}

/// One file the edits changed, with the lines they added and removed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FileChange {
    /// The file's path as the tool resolved it.
    pub path: String,
    pub added: u32,
    pub removed: u32,
    /// The file did not exist before the first of these edits.
    #[serde(default)]
    pub created: bool,
}

/// One message of the person and what the session's edits did since it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RewindPoint {
    pub node_id: String,
    /// Its WAL position: the edits after it are the ones a rewind takes back.
    pub position: u64,
    pub at_ms: u64,
    /// The message's first line, cut.
    pub text: String,
    /// The message whole (at most `MESSAGE_CAP` characters), to put back in
    /// an input after a rewind.
    #[serde(default)]
    pub message: String,
    /// The files the session's edit tools changed since it, one entry each.
    pub files: Vec<FileChange>,
    /// Programs the session ran since it (`proc.run` calls). What they
    /// changed is listed in words, never restored.
    pub commands: u32,
    /// The node before this message that a fork can end at; none for the
    /// session's first message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub fork_at: Option<String>,
}

/// `files.changes`' result: the person's messages, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FilesChangesResult {
    pub session_id: String,
    pub points: Vec<RewindPoint>,
    /// What a rewind does not take back, in words.
    pub note: String,
}

/// `files.restore`'s params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FilesRestoreParams {
    pub session_id: String,
    /// The message: a node (as `node.reach` names one) or `@<WAL position>`.
    /// The edits after it are taken back.
    pub since_node: String,
    /// Say what would be restored and write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// What became of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RestoredFile {
    pub path: String,
    /// `restored` (its earlier bytes put back), `removed` (the session
    /// created it), `would restore` or `would remove` on a dry run, or
    /// `skipped`.
    pub outcome: String,
    /// Why a file was skipped, or what a restore did, in words.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
}

/// `files.restore`'s result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FilesRestoreResult {
    pub session_id: String,
    pub since_node: String,
    pub since_position: u64,
    pub dry_run: bool,
    pub files: Vec<RestoredFile>,
    /// How many `proc.run` calls came after the message: their changes are
    /// listed here and never touched.
    pub commands: u32,
    /// The WAL position the restore's own pre-images start after: a rewind to
    /// `@<this>` undoes the restore.
    pub undo_since: u64,
    pub words: String,
}

/// The most of a message `RewindPoint.message` carries.
pub const MESSAGE_CAP: usize = 8_000;

/// What a rewind never restores: said by every surface.
pub const NOT_RESTORED: &str =
    "only files Theseus's own edit tools changed are restored, and only \
    where the file still holds what the last edit wrote; files that programs changed are listed, \
    never touched; files over 1 MiB are not kept";

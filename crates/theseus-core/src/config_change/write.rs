//! The daemon's own write of the config (issue 22, line 1 step 4), and the
//! copies it keeps. All of it is under `<state>/store/config-changes/`: the
//! floor (`toolrun/guarded.rs`) names the store directory, not the state
//! directory, so this is inside a path the floor holds and a tool's read or
//! write there waits for the owner, as one of the store's own files does. The
//! WAL lists only its `.seg` files, so a directory beside them is not read.
//!
//! - a **candidate** is kept by its digest while its question waits, so the
//!   yes writes exactly the text the card was built from;
//! - the config is written in one step: a temporary file in the same
//!   directory, synced, given the old file's permissions, renamed over it,
//!   and the directory synced;
//! - the text it replaces is kept first, as a numbered copy; the last
//!   [`KEEP`] are kept;
//! - a prompt file's change (issue 23) keeps its copies the same way, as
//!   `previous-NNNN.txt`, numbered from the same count as the settings'
//!   (so a change's number names one change whichever it was), the last
//!   [`KEEP`] of each kind; and while its question waits, the file as the
//!   card was built from it and the text it would become are kept by their
//!   digests ([`Kept`]), so the whole of both can be read before the yes.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::config_copy::sha256;

/// How many previous copies are kept.
pub const KEEP: usize = 10;

/// The directory of candidates, previous copies and the restart's marker:
/// inside the store directory, which the floor holds.
pub fn dir(state: &Path) -> PathBuf {
    crate::rpc::store_dir(state, false).join("config-changes")
}

fn candidate_path(state: &Path, sha: &str) -> PathBuf {
    dir(state).join(format!("candidate-{sha}.toml"))
}

/// A file made private (0600) in `dir`, all or nothing: written, synced and
/// renamed to `to`. `mode` overrides the permissions it ends with.
pub fn atomic(to: &Path, text: &str, mode: Option<u32>) -> std::io::Result<()> {
    let parent = to.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the path has no directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let done = f
        .write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .and_then(|()| match mode {
            Some(m) => std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(m)),
            None => Ok(()),
        })
        .and_then(|()| std::fs::rename(&tmp, to));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return done;
    }
    // The rename is durable once its directory is.
    std::fs::File::open(parent).and_then(|d| d.sync_all())
}

/// Keep the candidate under its digest.
pub fn keep_candidate(state: &Path, text: &str) -> std::io::Result<String> {
    let sha = sha256(text);
    atomic(&candidate_path(state, &sha), text, None)?;
    Ok(sha)
}

/// The candidate whose digest is `sha`, read back and checked against it.
pub fn read_candidate(state: &Path, sha: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(candidate_path(state, sha))
        .map_err(|e| format!("the candidate the card was built from cannot be read: {e}"))?;
    match sha256(&text) == sha {
        true => Ok(text),
        false => Err("the candidate kept for this card is not the one it was built from".into()),
    }
}

/// Forget a candidate once its question is settled.
pub fn drop_candidate(state: &Path, sha: &str) {
    let _ = std::fs::remove_file(candidate_path(state, sha));
}

/// The text of copy `n`, by its name: the number is one a ledger row recorded,
/// so no directory is listed to learn which copies exist.
pub fn copy(state: &Path, n: u32) -> Option<String> {
    std::fs::read_to_string(copy_path(state, n)).ok()
}

fn copy_path(state: &Path, n: u32) -> PathBuf {
    dir(state).join(format!("previous-{n:04}.toml"))
}

/// The previous copies, oldest first: their number and path.
pub fn previous(state: &Path) -> Vec<(u32, PathBuf)> {
    let mut out: Vec<(u32, PathBuf)> = std::fs::read_dir(dir(state))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let num = n
                .strip_prefix("previous-")?
                .strip_suffix(".toml")?
                .parse()
                .ok()?;
            Some((num, e.path()))
        })
        .collect();
    out.sort();
    out
}

/// The number the next kept copy takes: one past the newest of either kind.
fn next_number(state: &Path) -> u32 {
    let newest = |v: Vec<(u32, PathBuf)>| v.last().map_or(0, |(n, _)| *n);
    newest(previous(state)).max(newest(previous_files(state))) + 1
}

/// Keep `text`, the file about to be replaced, as the next numbered copy, and
/// forget all but the last [`KEEP`].
pub fn keep_previous(state: &Path, text: &str) -> std::io::Result<(u32, PathBuf)> {
    let next = next_number(state);
    let path = copy_path(state, next);
    atomic(&path, text, None)?;
    let all = previous(state);
    for (_, old) in all.iter().take(all.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(old);
    }
    Ok((next, path))
}

fn file_copy_path(state: &Path, n: u32) -> PathBuf {
    dir(state).join(format!("previous-{n:04}.txt"))
}

/// The text of a prompt file's copy `n`, by its number.
pub fn file_copy(state: &Path, n: u32) -> Option<String> {
    std::fs::read_to_string(file_copy_path(state, n)).ok()
}

/// The prompt files' previous copies, oldest first.
pub fn previous_files(state: &Path) -> Vec<(u32, PathBuf)> {
    let mut out: Vec<(u32, PathBuf)> = std::fs::read_dir(dir(state))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let num = n
                .strip_prefix("previous-")?
                .strip_suffix(".txt")?
                .parse()
                .ok()?;
            Some((num, e.path()))
        })
        .collect();
    out.sort();
    out
}

/// Keep `text`, a prompt file about to be replaced, as the next numbered copy,
/// and forget all but the last [`KEEP`] of the prompt files' copies.
pub fn keep_previous_file(state: &Path, text: &str) -> std::io::Result<(u32, PathBuf)> {
    let next = next_number(state);
    let path = file_copy_path(state, next);
    atomic(&path, text, None)?;
    let all = previous_files(state);
    for (_, old) in all.iter().take(all.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(old);
    }
    Ok((next, path))
}

/// A text kept while its question waits, by its digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// A prompt file as the card was built from it.
    FileBase,
    /// The text the prompt file would become.
    FileNew,
    /// The settings file as the card was built from it, kept when a long value
    /// is on the card.
    SettingsBase,
}

impl Kept {
    fn name(self) -> &'static str {
        match self {
            Kept::FileBase => "file-base",
            Kept::FileNew => "file-new",
            Kept::SettingsBase => "settings-base",
        }
    }
}

fn kept_path(state: &Path, kind: Kept, sha: &str) -> PathBuf {
    dir(state).join(format!("{}-{sha}.txt", kind.name()))
}

/// Keep `text` under its digest, and say the digest.
pub fn keep_text(state: &Path, kind: Kept, text: &str) -> std::io::Result<String> {
    let sha = sha256(text);
    atomic(&kept_path(state, kind, &sha), text, None)?;
    Ok(sha)
}

/// The text kept under `sha`, read back and checked against it.
pub fn read_kept(state: &Path, kind: Kept, sha: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(kept_path(state, kind, sha))
        .map_err(|e| format!("the text the card was built from cannot be read: {e}"))?;
    match sha256(&text) == sha {
        true => Ok(text),
        false => Err("the text kept for this card is not the one it was built from".into()),
    }
}

/// Forget a kept text once its question is settled.
pub fn drop_kept(state: &Path, kind: Kept, sha: &str) {
    let _ = std::fs::remove_file(kept_path(state, kind, sha));
}

/// Write `text` as the config at `file`: the old permissions kept, the new
/// text complete or not there at all.
pub fn write_config(file: &Path, text: &str) -> std::io::Result<()> {
    let mode = std::fs::metadata(file)
        .ok()
        .map(|m| m.permissions().mode() & 0o7777);
    atomic(file, text, mode)
}

#[cfg(test)]
mod tests;

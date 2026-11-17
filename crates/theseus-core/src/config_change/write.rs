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
//!   [`KEEP`] are kept.

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

/// Keep `text`, the file about to be replaced, as the next numbered copy, and
/// forget all but the last [`KEEP`].
pub fn keep_previous(state: &Path, text: &str) -> std::io::Result<(u32, PathBuf)> {
    let next = previous(state).last().map_or(1, |(n, _)| n + 1);
    let path = dir(state).join(format!("previous-{next:04}.toml"));
    atomic(&path, text, None)?;
    let all = previous(state);
    for (_, old) in all.iter().take(all.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(old);
    }
    Ok((next, path))
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

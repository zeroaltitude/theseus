//! One owner of the self loop's trees (issue 41, round 2). A socket daemon and a `--stdio` daemon open different
//! stores but share `<state>/self/trees`, and what each knows of the trees it opened is its own memory. So the trees
//! have one owner at a time: the process that holds an exclusive advisory lock (`flock`) on
//! `<state>/self/trees.lock`, from when it first takes it until the process ends (the kernel drops the lock with
//! the last descriptor, so a daemon that is killed holds nothing). Only the holder opens a phase tree
//! (`join::git::detached`) and only the holder sweeps (`join::guard::close_dead`). The lock file stays; it holds
//! nothing but the holder's pid, for a person who looks.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};

/// What a daemon that is not the owner says, once.
pub const NOT_OWNER: &str = "another daemon holds the self loop's trees";

/// The lock, held: dropping the last handle releases it.
#[derive(Debug)]
struct Held {
    _file: File,
    path: PathBuf,
}

/// A handle on the lock this process holds. Cloned into every `Place` of its daemon.
#[derive(Debug, Clone)]
pub struct TreesOwner(Arc<Held>);

impl TreesOwner {
    /// The lock file it holds.
    pub fn path(&self) -> &Path {
        &self.0.path
    }
}

/// The lock file of the trees under the self root `root` (`<state>/self`).
pub fn lock_path(root: &Path) -> PathBuf {
    root.join("trees.lock")
}

/// The locks this process holds, by their files: a second take in the same process shares the first (one daemon is
/// one process; a test's fixture and its core are one process too).
static HELD: Mutex<Option<HashMap<PathBuf, Weak<Held>>>> = Mutex::new(None);

/// Take the lock on the trees under `root`, now or not at all: why not when another process holds it, or the lock
/// cannot be made.
pub fn take(root: &Path) -> Result<TreesOwner, String> {
    std::fs::create_dir_all(root)
        .map_err(|e| format!("{} could not be made: {e}", root.display()))?;
    let path = lock_path(root);
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    let held = held.get_or_insert_with(HashMap::new);
    if let Some(h) = held.get(&path).and_then(Weak::upgrade) {
        return Ok(TreesOwner(h));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{} could not be opened: {e}", path.display()))?;
    // SAFETY: a plain advisory lock on a descriptor this function owns.
    let got = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if got != 0 {
        return Err(NOT_OWNER.into());
    }
    let _ = file.set_len(0);
    let _ = writeln!(file, "pid: {}", std::process::id());
    let h = Arc::new(Held {
        _file: file,
        path: path.clone(),
    });
    held.insert(path, Arc::downgrade(&h));
    Ok(TreesOwner(h))
}

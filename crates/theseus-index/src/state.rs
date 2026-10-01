//! The index directory, `<state>/index/` (mode 0700): a projection of the
//! store, outside its manifest, rebuildable from the WAL at any time.
//!
//! | File | Holds |
//! |---|---|
//! | `LOCK` | an exclusive `flock` while a tender runs: one tender per directory |
//! | `cursor.json` | where the index is in the WAL, written after each commit, and the versions it was built with |
//! | `places.json` | each session's place, learned from the core's META records |
//! | `bm25/` | the tantivy index |
//! | `sock` | the tender's socket, mode 0600 |

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use theseus_follow::Cursor;

/// Bumped when `cursor.json`'s layout changes.
pub const CURSOR_FORMAT: u32 = 1;

#[derive(Debug, Clone)]
pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }
    pub fn lock(&self) -> PathBuf {
        self.dir.join("LOCK")
    }
    pub fn cursor(&self) -> PathBuf {
        self.dir.join("cursor.json")
    }
    pub fn places(&self) -> PathBuf {
        self.dir.join("places.json")
    }
    pub fn bm25(&self) -> PathBuf {
        self.dir.join("bm25")
    }
    pub fn socket(&self) -> PathBuf {
        self.dir.join("sock")
    }

    /// Create the directory, mode 0700, and tighten an existing one.
    pub fn create(&self) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
    }
}

/// What `cursor.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub format: u32,
    /// `engine::SCHEMA_VERSION` and `extract::EXTRACTOR_VERSION` when the
    /// index was built: another build's index is rebuilt.
    pub schema: u32,
    pub extractor: u32,
    pub cursor: Cursor,
    pub saved_at_ms: u64,
}

/// A JSON file, or `None` when it is missing or does not parse (the index
/// is then rebuilt, which is always safe).
pub fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "index: unreadable, so ignored");
            None
        }
    }
}

/// Replace a JSON file: a temporary file, synced, renamed over it. A crash
/// leaves the old file or the new one.
pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(value)?)?;
        f.sync_data()?;
    }
    fs::rename(&tmp, path)
}

pub type Places = BTreeMap<String, String>;

/// One tender per index directory: an exclusive `flock` on `LOCK`, held
/// until the process ends (a killed tender's lock goes with it).
pub struct Lock {
    _file: File,
}

impl Lock {
    /// `None` when another process holds it.
    pub fn take(path: &Path) -> io::Result<Option<Lock>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        // SAFETY: a valid descriptor, and flags only.
        let r = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if r == 0 {
            return Ok(Some(Lock { _file: file }));
        }
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(e)
        }
    }
}

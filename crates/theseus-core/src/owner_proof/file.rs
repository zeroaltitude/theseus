//! The file the proof lives in: the secret, the last step a code was taken
//! for, and the wrong tries that count toward a lock. One file, mode 0600,
//! written by the daemon alone and floored (`toolrun::guarded`): no tool
//! reads or writes it and no L1 view shows it.
//!
//! The counters are in the file and not in memory so that a restart clears
//! nothing: a lock outlives the daemon that set it, and a code used once is
//! used for good.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::totp::{Secret, SECRET_BYTES};

/// The file's name in the state dir.
pub const FILE: &str = "owner-proof.json";

/// What is written, beside it, while it is written, and renamed over it: the
/// floor holds this name too, so a tool cannot read the secret in the moment
/// before the rename.
const NEW: &str = "owner-proof.json.new";

/// Both names the floor holds, over `state`.
pub fn floor_paths(state: &Path) -> [PathBuf; 2] {
    [state.join(FILE), state.join(NEW)]
}

/// What the file holds.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Held {
    /// The secret, as hex. The file is the only place it is kept.
    secret: String,
    pub enrolled_at_ms: u64,
    /// The last step a code was taken for: that step's code and every
    /// earlier one's is used.
    #[serde(default)]
    pub last_step: Option<u64>,
    /// When each wrong try still inside the window was made.
    #[serde(default)]
    pub wrong_ms: Vec<u64>,
    /// No check is made before this.
    #[serde(default)]
    pub locked_until_ms: u64,
    /// The places a good code covers for a while.
    #[serde(default)]
    pub windows: Vec<Window>,
}

/// A good code's cover over the place it came from: until `until_ms`, a
/// question answered from `place` needs no code. Held here, in the file the
/// daemon alone writes, so nothing a turn does opens, moves or lengthens one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub place: String,
    pub opened_at_ms: u64,
    pub until_ms: u64,
}

impl Held {
    pub fn new(secret: &Secret, now_ms: u64, last_step: u64) -> Self {
        Self {
            secret: hex::encode(secret.bytes()),
            enrolled_at_ms: now_ms,
            last_step: Some(last_step),
            wrong_ms: Vec::new(),
            locked_until_ms: 0,
            windows: Vec::new(),
        }
    }

    pub fn secret(&self) -> Result<Secret, Error> {
        let mut bytes = hex::decode(&self.secret).map_err(|_| Error::Unreadable)?;
        let out = <[u8; SECRET_BYTES]>::try_from(bytes.as_slice()).map_err(|_| Error::Unreadable);
        bytes.zeroize();
        out.map(Secret::from_bytes)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

/// Never prints the secret, the way [`Secret`] never does.
impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Held")
            .field("enrolled_at_ms", &self.enrolled_at_ms)
            .field("last_step", &self.last_step)
            .field("locked_until_ms", &self.locked_until_ms)
            .field("windows", &self.windows.len())
            .finish_non_exhaustive()
    }
}

/// Why the file could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// It is there and does not read, or others can read it: nothing is
    /// trusted from it.
    Unreadable,
}

/// The file at `path`: `None` when there is none, an error when it is there
/// and cannot be trusted (it does not parse, or its mode lets another user
/// read it).
pub(super) fn read(path: &Path) -> Result<Option<Held>, Error> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::Unreadable),
    };
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(Error::Unreadable);
    }
    let bytes = std::fs::read(path).map_err(|_| Error::Unreadable)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Error::Unreadable)
}

/// Write `held` whole: to a file of its own at mode 0600, synced, then
/// renamed over `path`, so a crash leaves the old file or the new one and
/// never half of either.
pub(super) fn write(path: &Path, held: &Held) -> std::io::Result<()> {
    let new = path.with_file_name(NEW);
    let mut bytes = serde_json::to_vec(held)?;
    let wrote = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&new)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        std::fs::rename(&new, path)
    })();
    bytes.zeroize();
    if wrote.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    wrote
}

/// Take the file away: the reset.
pub(super) fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

//! The owner's own instruction file, `<state_dir>/`[`USER_FILE`]: what a
//! compile carries of it is the version the owner last **accepted**, kept in
//! the store, and never the file on disk as such.
//!
//! **Three META families**, all JSON and read by key:
//!
//! - [`VERSION_PREFIX`]`<pid>.<seq>`: one [`Version`] per accept, never
//!   rewritten and never ended, so the record of what was carried only grows.
//!   `text` is what the reader carries under the file's header, or none for a
//!   file accepted as absent.
//! - [`NOW_PREFIX`]`<pid>`: the pointer, [`Now`], to the version that stands,
//!   written in the same frame as the version it names.
//! - [`NOTICED_PREFIX`]`<pid>`: [`Noticed`], the disk's digest the owner's
//!   ledger has already been told about, so a difference is one row however
//!   many turns it lasts. An accept ends it.
//!
//! `<pid>` is [`pid`]: the first 16 hex digits of the SHA-256 of the file's
//! canonical path, so a store moved to another state directory has no
//! accepted version there.
//!
//! **The compare is the reader's.** The disk's digest is the one
//! `ContextFiles` computes for the text it would carry (after its 64 KiB
//! cut), and the accepted version stores that same digest, so a byte past
//! the cut is not a difference -- it is never carried either way.
//!
//! **What a compile carries** ([`Look::carry`]): the accepted text, under
//! the same header as any context file, and while the disk differs one line
//! at the end of block 3 that names the difference and the accept. The
//! line's words come from integers and stored days, never from the file, so
//! they are the same bytes from one compile to the next. A store with no
//! accepted version carries no text at all, and a pointer the store cannot
//! read carries none either.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use theseus_store::{kinds, NewRecord};

use crate::context_files::{ContextFiles, ContextPath, Level};

use super::{Found, USER_FILE};

/// One accepted version, by `<pid>.<seq>`.
pub const VERSION_PREFIX: &str = "own_file.v.";
/// The pointer to the version that stands, by `<pid>`.
pub const NOW_PREFIX: &str = "own_file.now.";
/// The disk digest already ledgered as a difference, by `<pid>`.
pub const NOTICED_PREFIX: &str = "own_file.noticed.";

/// How a version came to be accepted: by the owner's act.
pub const HOW_ACCEPT: &str = "accept";
/// How a version came to be accepted: the daemon's first start with none.
pub const HOW_FIRST_START: &str = "first_start";

/// The file's id in the keys: the first 16 hex digits of the SHA-256 of its
/// path, with its directory made canonical (the file itself may be absent).
pub fn pid(path: &Path) -> String {
    let path = match (path.parent(), path.file_name()) {
        (Some(d), Some(n)) => {
            std::fs::canonicalize(d).map_or_else(|_| path.to_path_buf(), |d| d.join(n))
        }
        _ => path.to_path_buf(),
    };
    hex::encode(Sha256::digest(path.to_string_lossy().as_bytes()))[..16].to_string()
}

pub fn version_key(pid: &str, seq: u64) -> String {
    format!("{VERSION_PREFIX}{pid}.{seq:010}")
}

pub fn now_key(pid: &str) -> String {
    format!("{NOW_PREFIX}{pid}")
}

pub fn noticed_key(pid: &str) -> String {
    format!("{NOTICED_PREFIX}{pid}")
}

/// One accepted version of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub path: String,
    /// The text under the header, as the reader carries it (its cut note
    /// included); none: the file was accepted as absent.
    pub text: Option<String>,
    /// The reader's digest of it; none with no text.
    pub digest: Option<String>,
    pub bytes: u64,
    pub cut: bool,
    pub at_ms: u64,
    pub by: String,
    pub via: String,
    /// [`HOW_ACCEPT`] or [`HOW_FIRST_START`].
    pub how: String,
}

/// The pointer: which version stands, its digest, and when it was accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Now {
    pub seq: u64,
    pub digest: Option<String>,
    pub at_ms: u64,
}

/// The disk's digest the ledger was told of, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Noticed {
    pub digest: Option<String>,
    pub at_ms: u64,
}

impl Noticed {
    pub fn read(store: &crate::store::Store, pid: &str) -> Option<Self> {
        store.get_meta::<Self>(&noticed_key(pid)).ok().flatten()
    }

    pub fn record(&self, pid: &str) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(&noticed_key(pid)), self)
    }
}

/// What the store says is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accepted {
    /// No pointer: no version was ever accepted.
    None,
    /// The pointer could not be read. Nothing is carried, and nothing is
    /// accepted by itself on top of it.
    Unreadable,
    Some(Now),
}

/// The pointer of `pid`, one keyed read.
pub fn read_now(store: &crate::store::Store, pid: &str) -> Accepted {
    match store.get_meta::<Now>(&now_key(pid)) {
        Ok(None) => Accepted::None,
        Ok(Some(n)) => Accepted::Some(n),
        Err(_) => Accepted::Unreadable,
    }
}

/// The file on disk, as the reader reads it now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Disk {
    /// The regular file at the path; none when there is none.
    pub found: Option<Found>,
    /// The reader's digest; none with no file, or one it could not read.
    pub digest: Option<String>,
    /// The text under the header, as the reader carries it.
    pub text: Option<String>,
    pub bytes: u64,
    pub cut: bool,
}

impl Disk {
    /// The file at `path` through the reader. `found` is discovery's stat
    /// of it, so a compile stats it once.
    pub fn read(files: &ContextFiles, found: Option<Found>) -> Self {
        let Some(f) = found else {
            return Self::default();
        };
        let read = files.peek(&[ContextPath {
            path: f.shown.clone(),
            level: Level::User,
            public: false,
        }]);
        let Some(c) = read.into_iter().next() else {
            return Self {
                found: Some(f),
                ..Self::default()
            };
        };
        let text = c
            .file
            .digest
            .as_ref()
            .and_then(|_| c.section.split_once("\n\n"))
            .map(|(_, body)| body.to_string());
        Self {
            found: Some(f),
            digest: c.file.digest.clone(),
            text,
            bytes: c.file.bytes,
            cut: c.file.cut,
        }
    }

    /// The version that accepting what is on disk now would keep.
    pub fn version(&self, path: &Path, at_ms: u64, by: &str, via: &str, how: &str) -> Version {
        Version {
            path: path.display().to_string(),
            text: self.text.clone(),
            digest: self.digest.clone(),
            bytes: self.bytes,
            cut: self.cut,
            at_ms,
            by: by.to_string(),
            via: via.to_string(),
            how: how.to_string(),
        }
    }
}

/// The file's state: its path, what the store accepted, and the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub path: PathBuf,
    pub pid: String,
    pub accepted: Accepted,
    pub disk: Disk,
}

/// What a compile carries: the accepted text's section, and the line for
/// the end of block 3.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carry {
    pub section: Option<String>,
    pub line: Option<String>,
}

impl Look {
    /// The owner's own file under `cfg`'s state directory.
    pub fn of(
        cfg: &crate::config::Config,
        store: &crate::store::Store,
        files: &ContextFiles,
    ) -> Self {
        let path = cfg.state_dir().join(USER_FILE);
        let found = super::found(&path, 0);
        Self::with(store, files, path, found)
    }

    /// The file at `path`, with discovery's stat of it.
    pub fn with(
        store: &crate::store::Store,
        files: &ContextFiles,
        path: PathBuf,
        found: Option<Found>,
    ) -> Self {
        let pid = pid(&path);
        Self {
            accepted: read_now(store, &pid),
            disk: Disk::read(files, found),
            path,
            pid,
        }
    }

    /// Whether the disk differs from the accepted version. False with none
    /// accepted: there is nothing to differ from.
    pub fn differs(&self) -> bool {
        let Accepted::Some(n) = &self.accepted else {
            return false;
        };
        n.digest != self.disk.digest || (n.digest.is_none() && self.disk.found.is_some())
    }

    /// The pointer and version that accepting the disk now writes, with the
    /// end of the noticed digest, as records for one frame. Refused when the
    /// pointer cannot be read or the disk holds a file the reader cannot
    /// read.
    pub fn accept_records(&self, v: &Version) -> Result<Vec<NewRecord>> {
        let seq = match &self.accepted {
            Accepted::None => 1,
            Accepted::Some(n) => n.seq + 1,
            Accepted::Unreadable => bail!(
                "the accepted version of {} cannot be read from the store",
                self.path.display()
            ),
        };
        if self.disk.found.is_some() && self.disk.digest.is_none() {
            bail!("{} cannot be read", self.path.display());
        }
        let now = Now {
            seq,
            digest: v.digest.clone(),
            at_ms: v.at_ms,
        };
        Ok(vec![
            NewRecord::json(kinds::META, Some(&version_key(&self.pid, seq)), v)?,
            NewRecord::json(kinds::META, Some(&now_key(&self.pid)), &now)?,
            NewRecord::delete_meta(&noticed_key(&self.pid)),
        ])
    }

    /// The day the disk's present state was first ledgered, or `now_ms`'s
    /// when it has not been yet (the turn writes it with this compile's
    /// row, on the same day).
    pub fn since_ms(&self, store: &crate::store::Store, now_ms: u64) -> u64 {
        Noticed::read(store, &self.pid)
            .filter(|n| n.digest == self.disk.digest)
            .map_or(now_ms, |n| n.at_ms)
    }

    /// What a compile carries of the file (see the module's doc).
    pub fn carry(&self, store: &crate::store::Store, files: &ContextFiles, now_ms: u64) -> Carry {
        let shown = self.path.display().to_string();
        let size = || crate::narrative::thousands(self.disk.found.as_ref().map_or(0, |f| f.bytes));
        let accept = theseus_protocol::own_file::accept_command();
        let n = match &self.accepted {
            Accepted::None => {
                return Carry {
                    section: None,
                    line: self.disk.found.as_ref().map(|_| {
                        format!(
                            "The owner's own instruction file is at {shown} ({} B). No version \
                             of it has been accepted yet, so its text is not here. The owner \
                             accepts it with `{accept}` at a terminal.",
                            size()
                        )
                    }),
                };
            }
            Accepted::Unreadable => return self.unreadable(&shown),
            Accepted::Some(n) => n,
        };
        let section = match &n.digest {
            None => None,
            Some(_) => match accepted_text(store, files, &self.pid, n) {
                Some(text) => Some(format!(
                    "{}\n\n{text}",
                    crate::context_files::header(&shown, &Level::User)
                )),
                None => return self.unreadable(&shown),
            },
        };
        if !self.differs() {
            return Carry {
                section,
                line: None,
            };
        }
        let on = crate::owner_note::day(n.at_ms);
        let since = crate::owner_note::day(self.since_ms(store, now_ms));
        let line = match (&n.digest, &self.disk.found) {
            (Some(_), Some(_)) => format!(
                "The owner's own instruction file on disk differs from the version accepted on \
                 {on} (changed since {since}); the accepted version is what is above. The owner \
                 accepts the new one with `{accept}` at a terminal."
            ),
            (Some(_), None) => format!(
                "The owner's own instruction file is gone from disk (since {since}); the version \
                 accepted on {on} is what is above. The owner accepts its removal with \
                 `{accept}` at a terminal."
            ),
            (None, _) => format!(
                "The owner's own instruction file is at {shown} ({} B) and its text is not here: \
                 the version accepted on {on} is no file at all, and this one appeared since \
                 {since}. The owner accepts it with `{accept}` at a terminal.",
                size()
            ),
        };
        Carry {
            section,
            line: Some(line),
        }
    }

    /// Nothing carried, because the store did not answer for the accepted
    /// version; one line when there is a file to name.
    fn unreadable(&self, shown: &str) -> Carry {
        Carry {
            section: None,
            line: self.disk.found.as_ref().map(|f| {
                format!(
                    "The owner's own instruction file is at {shown} ({} B). Its accepted \
                     version could not be read from the store, so its text is not here.",
                    crate::narrative::thousands(f.bytes)
                )
            }),
        }
    }
}

/// The accepted version's text, read from the store once a daemon run: a
/// version record is never rewritten, so its key names one text for good.
/// None when the record cannot be read, or does not carry the digest the
/// pointer names.
fn accepted_text(
    store: &crate::store::Store,
    files: &ContextFiles,
    pid: &str,
    n: &Now,
) -> Option<std::sync::Arc<str>> {
    let key = version_key(pid, n.seq);
    files.accepted(&key, || {
        store
            .get_meta::<Version>(&key)
            .ok()
            .flatten()
            .filter(|v| v.digest == n.digest)
            .and_then(|v| v.text)
    })
}

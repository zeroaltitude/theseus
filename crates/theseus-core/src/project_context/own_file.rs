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
use std::sync::Arc;

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
    /// The SHA-256 of `text` exactly as stored (of nothing, for none): a
    /// read checks it before the text is carried ([`Version::holds`]).
    pub sum: String,
    pub bytes: u64,
    pub cut: bool,
    pub at_ms: u64,
    pub by: String,
    pub via: String,
    /// [`HOW_ACCEPT`] or [`HOW_FIRST_START`].
    pub how: String,
}

impl Version {
    /// Whether this record is the version `n` names, whole: the pointer's
    /// digest, text exactly when the digest says there is a file, and the
    /// body its sum was taken of.
    pub fn holds(&self, n: &Now) -> bool {
        self.digest == n.digest
            && self.text.is_some() == n.digest.is_some()
            && self.sum == sum(self.text.as_deref())
    }
}

/// [`Version::sum`] of `text`.
pub fn sum(text: Option<&str>) -> String {
    hex::encode(Sha256::digest(text.unwrap_or_default().as_bytes()))
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
    /// The first start found a file it could not read, and recorded that it
    /// had happened: a pointer to no version ([`Look::unread_record`]).
    /// Nothing is carried, and only the owner's accept sets a version.
    Unread,
    /// The pointer could not be read. Nothing is carried, and nothing is
    /// accepted on top of it: no number is guessed.
    Unreadable,
    /// The pointer reads soundly, and the version it names could not be
    /// read: missing, not decoding, or not holding the body its sum names.
    /// Nothing is carried and nothing is accepted by itself; the owner's
    /// accept keeps the next number, and the record is left as it is.
    Damaged(Now),
    /// An accept this daemon run wrote may be in the store without the
    /// store saying so ([`Missed::Unknown`]): nothing is carried, and every
    /// accept is refused until a restart reads the store again.
    Unknown,
    Some(Now),
}

/// What this daemon run could not confirm of the owner's own file: kept for
/// the run beside the accepted texts (`ContextFiles::own_missed`), never
/// stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missed {
    /// The first start found a file it could not read, and recorded that it
    /// had happened ([`Accepted::Unread`]).
    Read,
    /// The first start's accept was not written, and the start recorded
    /// that it had happened instead ([`Accepted::Unread`]).
    Write,
    /// An accept's frame the log kept and the index did not, or a first
    /// start's record whose durability barrier failed: it may be written.
    Unknown,
}

/// Why an accept is refused while [`Missed::Unknown`] stands.
#[derive(Debug)]
pub struct Unconfirmed;

impl std::fmt::Display for Unconfirmed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(UNCONFIRMED)
    }
}

impl std::error::Error for Unconfirmed {}

/// [`Unconfirmed`]'s words.
pub const UNCONFIRMED: &str = "an accept of your own instruction file may already be \
                               written, and this daemon cannot read it back until it is \
                               restarted: nothing was accepted; restart the daemon, then look \
                               again";

/// The number the version after `a` takes; none when the store cannot say.
pub fn seq_after(a: &Accepted) -> Option<u64> {
    match a {
        Accepted::None | Accepted::Unread => Some(1),
        Accepted::Some(n) => Some(n.seq + 1),
        Accepted::Damaged(n) => n.seq.checked_add(1),
        Accepted::Unreadable | Accepted::Unknown => None,
    }
}

/// One accept at a time in this daemon: an accept's second read of the
/// pointer and its append run under it (`Core::own_file_keep`).
pub fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Why an accept is refused when the pointer moved after it was read.
pub const CHANGED: &str = "the accepted version changed while you looked; run it again";

/// Why an accept is refused when a record stands under the number it would
/// take ([`used`]).
pub const USED: &str = "the store already holds a version under the number this accept would \
                        take, so nothing was accepted";

/// Whether a record stands under version `seq` of `pid`, whatever it holds:
/// an accept never writes a version's key twice.
pub fn used(store: &crate::store::Store, pid: &str, seq: u64) -> bool {
    !matches!(
        store.get_meta::<serde_json::Value>(&version_key(pid, seq)),
        Ok(None)
    )
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
            sum: sum(self.text.as_deref()),
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
    /// The accepted version's text, read and checked with its pointer; none
    /// for an absence accepted, and with nothing accepted.
    pub text: Option<Arc<str>>,
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
        let (accepted, text) = resolve(store, files, &pid);
        Self {
            accepted,
            text,
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
        let seq = self.next_seq()?;
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

    /// The pointer that records a first start that could not read the file:
    /// number 0, which no version takes, and no digest.
    pub fn unread_record(&self, at_ms: u64) -> Result<NewRecord> {
        let now = Now {
            seq: 0,
            digest: None,
            at_ms,
        };
        NewRecord::json(kinds::META, Some(&now_key(&self.pid)), &now)
    }

    /// The number the next accepted version takes.
    pub fn next_seq(&self) -> Result<u64> {
        match seq_after(&self.accepted) {
            Some(seq) => Ok(seq),
            None if self.accepted == Accepted::Unknown => bail!(Unconfirmed),
            None => bail!(
                "the accepted version of {} cannot be read from the store",
                self.path.display()
            ),
        }
    }

    /// `theseus health`'s line: while the disk differs from the accepted
    /// version, or while nothing of the file is carried for want of one.
    /// None otherwise. `missed`: what this daemon run's first start could
    /// not do, when it knows; a later run's store says only that the first
    /// start accepted nothing.
    pub fn health(
        &self,
        store: &crate::store::Store,
        now_ms: u64,
        missed: Option<Missed>,
    ) -> Option<String> {
        let accept = theseus_protocol::own_file::accept_command();
        match &self.accepted {
            Accepted::Some(_) if self.differs() => Some(format!(
                "Your own instruction file differs from the accepted version since {}: `{accept}`",
                crate::owner_note::day(self.since_ms(store, now_ms))
            )),
            Accepted::None if self.disk.found.is_some() => Some(format!(
                "Your own instruction file has no accepted version, so none of it is carried: \
                 `{accept}`"
            )),
            Accepted::Unread => Some(match missed {
                Some(Missed::Read) => format!(
                    "Your own instruction file could not be read at the first start, so no \
                     version is accepted and none of it is carried until you accept one: \
                     `{accept}`"
                ),
                Some(Missed::Write) => format!(
                    "Your own instruction file: its accept at the first start could not be \
                     written, so nothing was kept, no version is accepted and none of it is \
                     carried until you accept one: `{accept}`"
                ),
                _ => format!(
                    "Your own instruction file could not be read at the first start, or its \
                     accept could not be written, so no version is accepted and none of it is \
                     carried until you accept one: `{accept}`"
                ),
            }),
            Accepted::Unreadable | Accepted::Damaged(_) => Some(
                "Your own instruction file: its accepted version cannot be read from the store, \
                 so none of it is carried"
                    .to_string(),
            ),
            Accepted::Unknown => Some(
                "Your own instruction file: an accept of it may already be written that this \
                 daemon cannot read back, so none of it is carried and every accept is refused \
                 until the daemon is restarted"
                    .to_string(),
            ),
            _ => None,
        }
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
    pub fn carry(&self, store: &crate::store::Store, now_ms: u64) -> Carry {
        let shown = self.path.display().to_string();
        let size = || crate::narrative::thousands(self.disk.found.as_ref().map_or(0, |f| f.bytes));
        let accept = theseus_protocol::own_file::accept_command();
        let n = match &self.accepted {
            Accepted::None | Accepted::Unread | Accepted::Unknown => {
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
            Accepted::Unreadable | Accepted::Damaged(_) => return self.unreadable(&shown),
            Accepted::Some(n) => n,
        };
        let section = self.text.as_ref().map(|text| {
            format!(
                "{}\n\n{text}",
                crate::context_files::header(&shown, &Level::User)
            )
        });
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

    /// The accepted version's text, as the reader carried it: empty for an
    /// absence accepted, none when nothing is accepted or the store does not
    /// answer for it.
    pub fn accepted_text(&self) -> Option<String> {
        let Accepted::Some(_) = &self.accepted else {
            return None;
        };
        Some(self.text.as_deref().unwrap_or_default().to_string())
    }

    /// The manifest's record of the accepted version a compile carries: the
    /// path, the digest and the bytes of the text, at the user level. None
    /// when no text is carried (none accepted, an absence accepted, or a
    /// version the store does not answer for). The text's cut note, when
    /// it has one, is part of the stored text, as it is of the block.
    pub fn carried(&self) -> Option<crate::compiler::ContextFileRef> {
        let Accepted::Some(n) = &self.accepted else {
            return None;
        };
        let text = self.text.as_ref()?;
        Some(crate::compiler::ContextFileRef {
            path: self.path.display().to_string(),
            digest: n.digest.clone(),
            bytes: text.len() as u64,
            cut: false,
            missing: None,
            persona: None,
            level: theseus_protocol::ContextLevel::User,
            public: false,
            withheld: None,
        })
    }

    /// Nothing carried, because the store did not answer for the accepted
    /// version; one line, with the file or without it.
    fn unreadable(&self, shown: &str) -> Carry {
        let line = match &self.disk.found {
            Some(f) => format!(
                "The owner's own instruction file is at {shown} ({} B). Its accepted version \
                 could not be read from the store, so its text is not here.",
                crate::narrative::thousands(f.bytes)
            ),
            None => format!(
                "The owner's own instruction file is not on disk at {shown}, and its accepted \
                 version could not be read from the store, so its text is not here."
            ),
        };
        Carry {
            section: None,
            line: Some(line),
        }
    }
}

/// The pointer of `pid` and the version it names, read and checked as one
/// state: [`Accepted::Some`] only when the version record is there, decodes
/// and [`Version::holds`] what the pointer names, else
/// [`Accepted::Damaged`]; and the version's text, none for an absence.
/// A checked version is kept for the daemon run under its key and the
/// digest it was read for, so it is read from the store once. While this
/// run's mark stands ([`Missed::Unknown`]), nothing is read:
/// [`Accepted::Unknown`].
fn resolve(
    store: &crate::store::Store,
    files: &ContextFiles,
    pid: &str,
) -> (Accepted, Option<Arc<str>>) {
    if files.own_missed() == Some(Missed::Unknown) {
        return (Accepted::Unknown, None);
    }
    let n = match read_now(store, pid) {
        Accepted::Some(n) if n.seq == 0 && n.digest.is_none() => return (Accepted::Unread, None),
        Accepted::Some(n) if n.seq == 0 => return (Accepted::Unreadable, None),
        Accepted::Some(n) => n,
        other => return (other, None),
    };
    let key = version_key(pid, n.seq);
    let digest = n.digest.as_deref().unwrap_or("-");
    let text = files.accepted(&format!("{key}.{digest}"), || {
        let v = store.get_meta::<Version>(&key).ok().flatten()?;
        v.holds(&n).then(|| v.text.unwrap_or_default())
    });
    match text {
        Some(t) => {
            let text = n.digest.is_some().then_some(t);
            (Accepted::Some(n), text)
        }
        None => (Accepted::Damaged(n), None),
    }
}

/// Whether this store has ever accepted a version of an own file, under any
/// path: the first start's marker. A store moved to another state directory
/// has its pointer under the old path, so the first start there accepts
/// nothing by itself, and its compiles carry no text until the owner
/// accepts.
pub fn started(store: &crate::store::Store) -> Result<bool> {
    use theseus_store::Store as _;
    Ok(!store
        .inner()
        .latest_with_prefix(kinds::META, NOW_PREFIX)?
        .is_empty())
}

/// Whether this start is the store's first: no pointer for the file, and
/// none under any path. Not when the pointer, or the store's marker, cannot
/// be read (nothing is accepted on top of an error).
pub fn first_start_due(look: &Look, store: &crate::store::Store) -> bool {
    look.accepted == Accepted::None && matches!(started(store), Ok(false))
}

/// What the daemon's first start accepts, given its look at the file: what
/// is on disk, or its absence, when this start is the store's first
/// ([`first_start_due`]). None when it is not, and when the disk holds a
/// file the reader cannot read.
pub fn first_start(look: &Look, store: &crate::store::Store, at_ms: u64) -> Option<Version> {
    if !first_start_due(look, store) {
        return None;
    }
    if look.disk.found.is_some() && look.disk.digest.is_none() {
        return None;
    }
    Some(look.disk.version(
        &look.path,
        at_ms,
        FIRST_START_BY,
        FIRST_START_VIA,
        HOW_FIRST_START,
    ))
}

/// Who a first start's version names, and by what route.
pub const FIRST_START_BY: &str = "theseusd";
pub const FIRST_START_VIA: &str = "start";

/// `theseusd check`'s line about the file at `path`, read-only: what the
/// store at `store_dir` has accepted of it, read from the store's log
/// without opening the store (a running daemon holds it), and what the next
/// start will do.
pub fn check_line(store_dir: &Path, path: &Path) -> String {
    let disk = Disk::read(&ContextFiles::default(), super::found(path, 0));
    let shown = path.display();
    let accept = theseus_protocol::own_file::accept_command();
    let Ok((this, any, version)) = pointers_in_log(&store_dir.join("wal"), &pid(path)) else {
        return "Your own instruction file: the store's log could not be read, so this check \
                cannot say which version is accepted. A start accepts nothing by itself while \
                the store cannot say."
            .to_string();
    };
    match (this, disk.found.as_ref()) {
        (Some(n), _) if n.seq == 0 && n.digest.is_none() => format!(
            "Your own instruction file: at the first start it could not be read, or its accept \
             could not be written, so no version is accepted, none of it is carried, and a start \
             accepts nothing by itself: an accept is needed, `{accept}`."
        ),
        (Some(n), _) if !version.is_some_and(|v| v.holds(&n)) => {
            "Your own instruction file: its accepted version cannot be read from the store, so \
             none of it is carried, and a start accepts nothing by itself."
                .to_string()
        }
        (Some(n), _) => {
            let look = Look {
                path: path.to_path_buf(),
                pid: pid(path),
                accepted: Accepted::Some(n.clone()),
                text: None,
                disk: disk.clone(),
            };
            let on = crate::owner_note::day(n.at_ms);
            if look.differs() {
                format!(
                    "Your own instruction file: the version accepted on {on} is what is \
                     carried, and the file on disk differs: `{accept}` shows the change."
                )
            } else {
                format!(
                    "Your own instruction file: the version accepted on {on} is what is carried."
                )
            }
        }
        (None, _) if any => format!(
            "Your own instruction file: no version is accepted for {shown}, and this store \
             accepted one under another path, so a start accepts nothing by itself and none of \
             it is carried until `{accept}`."
        ),
        (None, Some(f)) => format!(
            "Your own instruction file: no version accepted yet. At the next start, the {}-byte \
             file on disk ({shown}) will be accepted, once.",
            crate::narrative::thousands(f.bytes)
        ),
        (None, None) => format!(
            "Your own instruction file: no version accepted yet, and there is no file at \
             {shown}. At the next start its absence will be accepted, once."
        ),
    }
}

/// The pointer of `pid` in the log at `wal`, whether the log holds a
/// pointer for any path, and the version record the pointer names when it
/// decodes, read from the start of the log. No log: none of them. A pointer
/// that does not decode is an error, never "none".
fn pointers_in_log(wal: &Path, pid: &str) -> Result<(Option<Now>, bool, Option<Version>)> {
    use theseus_follow::{Cursor, WalFollower};
    if !wal.is_dir() {
        return Ok((None, false, None));
    }
    let key = now_key(pid);
    let (mut this, mut any) = (None, false);
    let mut versions: std::collections::HashMap<String, Option<Version>> = Default::default();
    let mut f = WalFollower::open(wal, Cursor::start())?;
    loop {
        let b = f.read(8 << 20)?;
        if b.is_empty() {
            let version = this
                .as_ref()
                .and_then(|n: &Now| versions.remove(&version_key(pid, n.seq)))
                .flatten();
            return Ok((this, any, version));
        }
        for r in b.records {
            if let Some(k) = r.key.as_deref().filter(|k| k.starts_with(VERSION_PREFIX)) {
                let v = (r.kind == kinds::META).then(|| r.decode::<Version>().ok());
                versions.insert(k.to_string(), v.flatten());
                continue;
            }
            let Some(k) = r.key.as_deref().filter(|k| k.starts_with(NOW_PREFIX)) else {
                continue;
            };
            let set = r.kind == kinds::META;
            any |= set;
            if k == key {
                this = if set { Some(r.decode::<Now>()?) } else { None };
            }
        }
    }
}

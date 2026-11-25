//! The context files the settings name, `[context] files`: what a compile
//! carries of each is the version the owner last accepted, on the owner's
//! own file's records, reader and accept (`own_file`), one
//! [`own_file::Look`] per path. Nothing here is a second mechanism: the
//! records are `own_file`'s three META families under each file's own
//! `pid`, and the accept is `theseus own-file accept --file PATH`.
//!
//! - **The project layer** ([`hold`]): every reader of the context files
//!   (the turn's request, the census, `context explain`) goes through
//!   `TurnRunner::context_load` or `TurnRunner::context_peek`, which carry
//!   a file's accepted text under its usual header, and never the disk's
//!   when it differs. So the layer, which has its own cache breakpoint,
//!   changes only at an accept.
//! - **Block 3** ([`lines`]): one line for a file whose disk differs from its
//!   accepted version, or that is not carried for want of one, in the
//!   session's block, which has no breakpoint of its own: a difference
//!   rewrites only the conversation's cached entry, never a layer.
//! - **The first start** ([`first_start_due`]): a path with no pointer is
//!   accepted, once, at the start that first names it (`Core::held_first_start`);
//!   a pointer that stands, or one the store cannot read, is never accepted
//!   over by a start.
//! - **A file that cannot be read**, with none of its text accepted, keeps
//!   the reader's note that it is missing, as before.
//!
//! A file the settings no longer name is not read at all: its records stay,
//! and nothing of it is carried.

use std::path::{Path, PathBuf};

use crate::compiler::ContextFileRef;
use crate::context_files::{header, ContextFile, ContextFiles, ContextPath, Level, Unreadable};

use super::own_file::{self, Accepted, Disk, Look, Whose};

/// The paths `[context] files` names, expanded, each once, in order.
pub fn paths(cfg: &crate::config::Config) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for e in &cfg.context.files {
        let p = crate::config::expand(e.path());
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// `given` as one of [`paths`], by the settings' own expansion; none when
/// the settings do not name it.
pub fn named(cfg: &crate::config::Config, given: &str) -> Option<PathBuf> {
    let p = crate::config::expand(given.trim());
    paths(cfg).into_iter().find(|h| *h == p)
}

/// The context file at `path` as the own file's reader and records see it.
pub fn look(store: &crate::store::Store, files: &ContextFiles, path: &Path) -> Look {
    Look::with(store, files, path.to_path_buf(), super::found(path, 0))
}

/// Whether a start accepts what is on disk at `look`: no pointer for its
/// path. Not when the pointer stands, or cannot be read.
pub fn first_start_due(look: &Look) -> bool {
    look.accepted == Accepted::None
}

/// The reader's sections `read`, one for each of `paths` in order, as a
/// compile carries them: a file of the system level is its accepted text,
/// under its usual header; with none of its text accepted, a file the
/// reader could not read keeps the reader's note, and one it could read is
/// not carried ([`lines`] names it). A persona's files are the reader's.
pub fn hold(
    store: &crate::store::Store,
    files: &ContextFiles,
    read: Vec<ContextFile>,
    paths: &[ContextPath],
) -> Vec<ContextFile> {
    read.into_iter()
        .zip(paths)
        .filter_map(|(f, p)| match p.level {
            Level::System => held(&look(store, files, &crate::config::expand(&p.path)), f),
            _ => Some(f),
        })
        .collect()
}

/// What a compile carries of the file `look` read, given the reader's
/// section `f` of it.
fn held(look: &Look, f: ContextFile) -> Option<ContextFile> {
    let text = match (&look.accepted, &look.text) {
        (Accepted::Some(_), Some(text)) => text,
        _ => return f.file.digest.is_none().then_some(f),
    };
    let file = if look.differs() {
        ContextFileRef {
            level: f.file.level,
            public: f.file.public,
            ..look.carried()?
        }
    } else {
        f.file
    };
    Some(ContextFile {
        section: format!(
            "{}\n\n{text}",
            header(&look.path.display().to_string(), &Level::System)
        ),
        file,
    })
}

/// Block 3's lines about the context files: one for each whose disk is not
/// its accepted version, or that is not carried for want of one
/// (`Look::carry_as`).
pub fn lines(
    cfg: &crate::config::Config,
    store: &crate::store::Store,
    files: &ContextFiles,
    now_ms: u64,
) -> Vec<String> {
    paths(cfg)
        .iter()
        .filter_map(|p| {
            look(store, files, p)
                .carry_as(store, now_ms, Whose::Context)
                .line
        })
        .collect()
}

impl crate::turn::TurnRunner {
    /// The context files for `paths` as a compile carries them ([`hold`]),
    /// and the files this daemon run finds unreadable for the first time:
    /// the one way every reader of them reads them.
    pub(crate) fn context_load(
        &self,
        paths: &[ContextPath],
    ) -> (Vec<ContextFile>, Vec<Unreadable>) {
        let (read, unreadable) = self.context_files.load(paths);
        (
            hold(&self.store, &self.context_files, read, paths),
            unreadable,
        )
    }

    /// [`Self::context_load`], warning of nothing (`ContextFiles::peek`).
    pub(crate) fn context_peek(&self, paths: &[ContextPath]) -> Vec<ContextFile> {
        let read = self.context_files.peek(paths);
        hold(&self.store, &self.context_files, read, paths)
    }
}

/// `theseusd check`'s line about the context files at `paths`, read-only,
/// by count and never by content: what the store at `store_dir` has
/// accepted of them, read from its log, and what the next start will do.
/// None when the settings name none.
pub fn check_line(store_dir: &Path, paths: &[PathBuf]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let n = paths.len();
    let pids: Vec<String> = paths.iter().map(|p| own_file::pid(p)).collect();
    let Ok((each, _)) = own_file::pointers_of(&store_dir.join("wal"), &pids) else {
        return Some(format!(
            "Your context files ({n}): the store's log could not be read, so this check cannot \
             say which versions are accepted. A start accepts none of them by itself while the \
             store cannot say."
        ));
    };
    let (mut same, mut differs, mut due, mut wait, mut bad) = (0, 0, 0, 0, 0);
    for ((path, pid), (now, version)) in paths.iter().zip(pids).zip(each) {
        match now {
            None => due += 1,
            Some(now) if now.seq == 0 && now.digest.is_none() => wait += 1,
            Some(now) if !version.is_some_and(|v| v.holds(&now)) => bad += 1,
            Some(now) => {
                let look = Look {
                    path: path.clone(),
                    pid,
                    accepted: Accepted::Some(now),
                    text: None,
                    disk: Disk::read(&ContextFiles::default(), super::found(path, 0)),
                };
                if look.differs() {
                    differs += 1;
                } else {
                    same += 1;
                }
            }
        }
    }
    let accept = theseus_protocol::own_file::accept_command_for(Some("PATH"));
    let mut parts = vec![format!("Your context files: {n} named")];
    for (count, words) in [
        (same, "carried as accepted".to_string()),
        (
            differs,
            format!(
                "carried as accepted while the file on disk differs (`{accept}` shows the change)"
            ),
        ),
        (
            due,
            "with no version accepted yet, each of which the next start accepts, once".to_string(),
        ),
        (
            wait,
            format!("that a start could not read, which wait for `{accept}`"),
        ),
        (
            bad,
            "whose accepted version cannot be read from the store, so none of it is carried"
                .to_string(),
        ),
    ] {
        if count > 0 {
            parts.push(format!("{count} {words}"));
        }
    }
    Some(format!("{}.", parts.join("; ")))
}

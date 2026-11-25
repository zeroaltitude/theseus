//! The context files the settings name, `[context] files`: what a request
//! carries of each is the version the owner last accepted, on the owner's
//! own file's records, reader and accept (`own_file`), one
//! [`own_file::Look`] per configured spelling. Nothing here is a second
//! mechanism: the records are `own_file`'s three META families under each
//! spelling's own id ([`pid`]), and the accept is `theseus own-file accept
//! --file PATH`.
//!
//! - **Every automatic reader** ([`hold`]): the turn's request, the census,
//!   `context explain`, the persona view and the daemon's own messages go
//!   through `TurnRunner::context_load` or `TurnRunner::context_peek`. A
//!   path that is a configured spelling, or a symbolic link that resolves
//!   to the same path as one, is carried as that spelling's accepted text,
//!   under the reader's own header, whether the system level or a persona
//!   names it, and never as it stands on disk. The path is compared, not
//!   the file: a hard link under another name is read as it stands.
//! - **The binding is the configured spelling** ([`pid`]): when a link above
//!   it is pointed elsewhere, or a directory in it appears, the accepted
//!   text stays what is carried and the change is held like any other.
//! - **Block 3** ([`lines`]): one line for a file whose disk differs from its
//!   accepted version, or that is not carried for want of one, in the
//!   session's block, which has no breakpoint of its own. For a readable
//!   accepted file, the project layer, which has its own breakpoint, changes
//!   only at an accept. A file the reader cannot read is the reader's note,
//!   as before, so the layer changes when a file appears where an absence
//!   was accepted (the note goes, and the new text is not carried), and when
//!   one reason a file cannot be read becomes another.
//! - **The first start** ([`first_start_due`]): a spelling with no pointer
//!   is accepted, once, at the start that first names it
//!   (`Core::held_first_start`); a pointer that stands, or one the store
//!   cannot read, is never accepted over by a start.
//! - **A change Theseus is asked to make** ([`base_refusal`]) is asked and
//!   written only on a file whose disk is its accepted version.
//! - **A turn** ([`turn_refusal`]) is not run while an accepted version of
//!   any instruction file cannot be read from the store.
//!
//! A file the settings no longer name is not read at all: its records stay,
//! and nothing of it is carried.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

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

/// The configured spellings that resolve to the file at `real`, a
/// canonical path, in order.
pub fn spellings_of(cfg: &crate::config::Config, real: &Path) -> Vec<PathBuf> {
    paths(cfg)
        .into_iter()
        .filter(|p| std::fs::canonicalize(p).is_ok_and(|r| r == real))
        .collect()
}

/// The record id of the configured spelling `path`: the first 16 hex
/// digits of the SHA-256 of the spelling as the settings expand it, never of
/// what it resolves to.
pub fn pid(path: &Path) -> String {
    let named = format!("[context] files\n{}", path.display());
    hex::encode(Sha256::digest(named.as_bytes()))[..16].to_string()
}

/// The context file the configured spelling `path` names, as the own file's
/// reader and records see it.
pub fn look(store: &crate::store::Store, files: &ContextFiles, path: &Path) -> Look {
    Look::bound(
        store,
        files,
        path.to_path_buf(),
        pid(path),
        super::found(path, 0),
    )
}

/// Whether a start accepts what is on disk at `look`: no pointer for its
/// spelling. Not when the pointer stands, or cannot be read.
pub fn first_start_due(look: &Look) -> bool {
    look.accepted == Accepted::None
}

/// The reader's sections `read`, one for each of `paths` in order, as a
/// request carries them. A path that is a configured spelling, or resolves to
/// the same canonical path as one ([`spellings_of`]), whichever level reads
/// it, is that spelling's accepted
/// text under the reader's own header; with none of its text accepted, a
/// file the reader could not read keeps the reader's note, and one it could
/// read is not carried ([`lines`] names it). Any other file is the
/// reader's.
pub fn hold(
    cfg: &crate::config::Config,
    store: &crate::store::Store,
    files: &ContextFiles,
    read: Vec<ContextFile>,
    paths: &[ContextPath],
) -> Vec<ContextFile> {
    let named: Vec<(PathBuf, Option<PathBuf>)> = self::paths(cfg)
        .into_iter()
        .map(|p| {
            let real = std::fs::canonicalize(&p).ok();
            (p, real)
        })
        .collect();
    let binding = |given: &Path| -> Option<PathBuf> {
        if let Some((p, _)) = named.iter().find(|(p, _)| p == given) {
            return Some(p.clone());
        }
        let real = std::fs::canonicalize(given).ok()?;
        named
            .iter()
            .find(|(_, r)| r.as_ref() == Some(&real))
            .map(|(p, _)| p.clone())
    };
    read.into_iter()
        .zip(paths)
        .filter_map(|(f, p)| match binding(&crate::config::expand(&p.path)) {
            Some(spelling) => held(&look(store, files, &spelling), f, &p.level),
            None => Some(f),
        })
        .collect()
}

/// What a request carries of the file `look` read, given the reader's
/// section `f` of it at `level`.
fn held(look: &Look, f: ContextFile, level: &Level) -> Option<ContextFile> {
    let (n, text, (bytes, cut)) = match (&look.accepted, &look.text, look.size) {
        (Accepted::Some(n), Some(text), Some(size)) => (n, text, size),
        _ => return f.file.digest.is_none().then_some(f),
    };
    let file = if look.differs() || f.file.digest != n.digest {
        ContextFileRef {
            digest: n.digest.clone(),
            bytes,
            cut,
            missing: None,
            ..f.file
        }
    } else {
        f.file
    };
    Some(ContextFile {
        section: format!("{}\n\n{text}", header(&file.path, level)),
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

/// Why a change to the prompt file at `real` (canonical) may not be asked,
/// or written, on the base `text`: a configured spelling resolves to it and
/// `text` is not that spelling's accepted version, as the reader would read
/// it. None for a file no configured spelling names. A card shows the change
/// from its base, so a base that is not the accepted version would let a yes
/// accept what the card never showed as a change. A file whose accepted
/// version or base is longer than the reader carries is refused too: a
/// change to the part not carried would not be on the card, and a change
/// before it could bring that part into what is carried.
pub fn base_refusal(
    cfg: &crate::config::Config,
    store: &crate::store::Store,
    files: &ContextFiles,
    real: &Path,
    text: &str,
) -> Option<String> {
    spellings_of(cfg, real).into_iter().find_map(|spelling| {
        let look = look(store, files, &spelling);
        let base = Disk::written(look.disk.found.clone(), text);
        if base.cut || look.size.is_some_and(|(_, cut)| cut) {
            return Some(format!(
                "{} is longer than the {} bytes of it Theseus carries, so no change to it is \
                 asked: change it yourself at your terminal, then accept it with `{}`",
                spelling.display(),
                crate::narrative::thousands(crate::context_files::MAX_BYTES as u64),
                Whose::Context.accept(&spelling)
            ));
        }
        let accepted = matches!(&look.accepted,
            Accepted::Some(n) if n.digest.is_some() && n.digest == base.digest);
        (!accepted).then(|| {
            format!(
                "{} is not the version of it you accepted, so no change to it is asked: accept \
                 what is on disk first (`{}` at your terminal shows the change and asks), or put \
                 back the text you accepted",
                spelling.display(),
                Whose::Context.accept(&spelling)
            )
        })
    })
}

/// **The one place a turn is refused for its instruction files** (the
/// owner's ruling, 2026-11-24: fail closed). `Some(words)` while the store
/// cannot say which version of the owner's own instruction file, or of a
/// context file, is accepted: a version record that cannot be read
/// (`Accepted::Damaged`), a pointer that cannot be read
/// (`Accepted::Unreadable`), or an accept this run may have written and
/// cannot read back (`Accepted::Unknown`). A file with no version accepted
/// yet is not refused: it is named in block 3 and not carried. To let turns
/// run without the files instead, return `None` here.
pub fn turn_refusal(
    cfg: &crate::config::Config,
    store: &crate::store::Store,
    files: &ContextFiles,
) -> Option<String> {
    let own = Look::of(cfg, store, files);
    let held = paths(cfg).into_iter().map(|p| look(store, files, &p));
    let mut unknown = false;
    let mut said: Vec<String> = Vec::new();
    for (look, whose) in std::iter::once((own, Whose::Own)).chain(held.map(|l| (l, Whose::Context)))
    {
        let name = match whose {
            Whose::Own => "your own instruction file".to_string(),
            Whose::Context => format!("your context file {}", look.path.display()),
        };
        match look.accepted {
            Accepted::Unknown => unknown = true,
            Accepted::Damaged(_) => said.push(format!(
                "{name}: its accepted version cannot be read, and `{}` at your terminal shows the \
                 file on disk and keeps it as the next version",
                whose.accept(&look.path)
            )),
            Accepted::Unreadable => said.push(format!(
                "{name}: the record of which version is accepted cannot be read, and no accept \
                 can be written over it (`{}` shows it)",
                whose.show(&look.path)
            )),
            _ => {}
        }
    }
    if unknown {
        said.insert(
            0,
            "an accept may already be written that this daemon cannot read back: restart the \
             daemon, then `theseus own-file` shows where your own instruction file stands"
                .to_string(),
        );
    }
    let said = said.join("; ");
    let mut first = said.chars();
    let said: String = first
        .next()
        .map(|c| c.to_uppercase().chain(first).collect())
        .unwrap_or_default();
    (!said.is_empty()).then(|| {
        format!(
            "This turn was not run: your instruction files could not be read from the store as \
             you last accepted them, and Theseus runs no turn without them. {said}."
        )
    })
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
            hold(&self.cfg, &self.store, &self.context_files, read, paths),
            unreadable,
        )
    }

    /// [`Self::context_load`], warning of nothing (`ContextFiles::peek`).
    pub(crate) fn context_peek(&self, paths: &[ContextPath]) -> Vec<ContextFile> {
        let read = self.context_files.peek(paths);
        hold(&self.cfg, &self.store, &self.context_files, read, paths)
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
    let pids: Vec<String> = paths.iter().map(|p| pid(p)).collect();
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
                    size: None,
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
            format!(
                "that a start could not read, or whose accept it could not write, which wait for \
                 `{accept}`"
            ),
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

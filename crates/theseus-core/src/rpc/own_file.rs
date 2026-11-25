//! The owner's own instruction file at the daemon's level
//! (`project_context::own_file`), and the context files the settings name
//! on the same records (`project_context::held`): the first start's accept,
//! the line `theseus health` shows while a file on disk is not the accepted
//! one, and the owner's two methods: `own_file.show`, a read, and
//! `own_file.accept`, the owner's act at their own terminal. Each names a
//! context file by `file`, and the owner's own file without it.

use std::sync::Arc;

use anyhow::{bail, Result};
use serde_json::Value;
use theseus_protocol::own_file::{
    OwnFileAcceptParams, OwnFileAcceptResult, OwnFileShowParams, OwnFileShowResult, OwnFileState,
};
use theseus_protocol::{error_code, method};
use theseus_store::store::RetainedFrame;
use theseus_store::NewRecord;

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal, Surface};
use crate::fact::own_file::OwnFileAccepted;
use crate::project_context::held;
use crate::project_context::own_file::{self, Accepted, Look, Missed, Version, Whose};

impl Core {
    /// The first start's accept, once the socket answers: what is on disk,
    /// or its absence, when this store has never accepted a version
    /// (`own_file::first_start`). One frame: the version, its pointer and
    /// its `own_file.accepted` row; and one line in the daemon's log. Never
    /// again by itself: a later start finds the pointer and does nothing,
    /// and a pointer it cannot read is not "none".
    ///
    /// A first start that finds a file it cannot read, or cannot write its
    /// accept, still records that it happened ([`Accepted::Unread`]):
    /// nothing is carried, one line is logged, and only the owner's accept
    /// sets a version. When even that record cannot be written, the error
    /// is returned. An accept the store may hold without saying so
    /// ([`own_file::Unconfirmed`]) writes nothing more: no record that the
    /// start could not accept.
    ///
    /// Either record is said done only once it is durable: each passes the
    /// store's barrier (`Store::checkpoint`) before the start answers, and
    /// a barrier that fails is the same unknown outcome.
    pub fn own_file_first_start(&self) -> Result<Option<Version>> {
        let look = Look::of(&self.cfg, &self.store, &self.runner.context_files);
        if matches!(look.accepted, Accepted::Unreadable | Accepted::Damaged(_)) {
            // The decode's own words stay out of the log: they could quote
            // the record.
            tracing::warn!(path = %look.path.display(),
                "your own instruction file: its accepted version cannot be read from the store, so this start accepts nothing and none of it is carried");
            return Ok(None);
        }
        if !own_file::first_start_due(&look, &self.store) {
            return Ok(None);
        }
        let not_written = match self.own_file_first_accept(&look) {
            First::Kept(v) => return Ok(Some(v)),
            First::Unconfirmed => return Ok(None),
            First::Unread => None,
            First::NotWritten(e) => Some(e),
        };
        let missed = if not_written.is_some() {
            Missed::Write
        } else {
            Missed::Read
        };
        if !self.own_file_unread(&look, Some(missed))? {
            return Ok(None);
        }
        match not_written {
            None => tracing::warn!(path = %look.path.display(),
                "your own instruction file could not be read at this first start, so no version is accepted and none of it is carried until `theseus own-file accept`"),
            Some(e) => tracing::warn!(path = %look.path.display(), error = %e,
                "your own instruction file: this first start's accept could not be written, so nothing was kept, no version is accepted and none of it is carried until `theseus own-file accept`"),
        }
        Ok(None)
    }

    /// The first start's accept of what is on disk, with its line in the
    /// log when it is kept.
    fn own_file_first_accept(&self, look: &Look) -> First {
        let Some(v) = own_file::first_start(look, &self.store, theseus_protocol::now_unix_ms())
        else {
            return First::Unread;
        };
        match self.own_file_kept(look, &v, true, Whose::Own) {
            Ok(seq) => {
                tracing::info!(path = %look.path.display(), bytes = v.bytes, seq,
                    present = v.digest.is_some(),
                    "your own instruction file: what is on disk is accepted, once, at this first start; from now on a change to it waits for `theseus own-file accept`");
                First::Kept(v)
            }
            Err(e) if e.is::<own_file::Unconfirmed>() => First::Unconfirmed,
            Err(e) => First::NotWritten(e),
        }
    }

    /// The first start recorded as having happened with no version
    /// (`Look::unread_record`), under the accepts' lock, and made durable;
    /// `missed`, what the owner's own file's start could not do, is kept for
    /// the run (none for a context file). False, and nothing written, when
    /// a pointer stands by then; false too when the store cannot confirm the
    /// record ([`own_file::Unconfirmed`]).
    fn own_file_unread(&self, look: &Look, missed: Option<Missed>) -> Result<bool> {
        let _one = own_file::one_at_a_time();
        if own_file::read_now(&self.store, &look.pid) != Accepted::None {
            return Ok(false);
        }
        let at_ms = theseus_protocol::now_unix_ms();
        match self.own_file_settle(&[look.unread_record(at_ms)?], true) {
            Ok(()) => {
                if let Some(missed) = missed {
                    self.runner.context_files.set_own_missed(missed);
                }
                Ok(true)
            }
            Err(e) if e.is::<own_file::Unconfirmed>() => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// [`Core::own_file_first_start`], then [`Core::held_first_start`], off
    /// the runtime's workers: each reads its files and writes a frame for
    /// each. The owner's own file goes first, so a fresh store's first start
    /// is its own. The own file's error is the caller's; a context file's
    /// is one line in the log.
    pub fn own_file_after_serving(&self) -> Result<()> {
        theseus_store::blocking(|| {
            let own = self.own_file_first_start().map(|_| ());
            if let Err(e) = self.held_first_start() {
                tracing::error!(error = %e,
                    "a context file: the first start that names it could not be recorded, so none of it is carried and this run accepts nothing of it by itself; `theseus own-file accept --file` accepts a version");
            }
            own
        })
    }

    /// The first start of each context file the settings name
    /// (`held::first_start_due`): a path with no pointer is accepted, what
    /// is on disk or its absence, as the owner's own file's first start
    /// accepts it, in its own frame with its own row and one line in the
    /// log; a file the reader cannot read, or whose accept is not written,
    /// is recorded as started with no version. A pointer that stands, or
    /// that cannot be read, is never accepted over. How many were accepted.
    pub fn held_first_start(&self) -> Result<usize> {
        let mut kept = 0;
        for path in held::paths(&self.cfg) {
            let look = held::look(&self.store, &self.runner.context_files, &path);
            if !held::first_start_due(&look) {
                continue;
            }
            match self.held_first_accept(&look)? {
                Some(true) => kept += 1,
                Some(false) => {}
                None => break,
            }
        }
        Ok(kept)
    }

    /// One context file's first start ([`Core::held_first_start`]): true
    /// when what is on disk is accepted; false when the start is recorded
    /// with no version, or a pointer stood by then; none when the store may
    /// hold an accept it cannot read back ([`own_file::Unconfirmed`]).
    fn held_first_accept(&self, look: &Look) -> Result<Option<bool>> {
        let not_written = match own_file::first_version(look, theseus_protocol::now_unix_ms()) {
            Some(v) => match self.own_file_kept(look, &v, true, Whose::Context) {
                Ok(seq) => {
                    tracing::info!(path = %look.path.display(), bytes = v.bytes, seq,
                        present = v.digest.is_some(),
                        "a context file: what is on disk is accepted, once, at the first start that names it; from now on a change to it waits for `theseus own-file accept --file`");
                    return Ok(Some(true));
                }
                Err(e) if e.is::<own_file::Unconfirmed>() => return Ok(None),
                Err(e) => Some(e),
            },
            None => None,
        };
        if !self.own_file_unread(look, None)? {
            return Ok(Some(false));
        }
        match not_written {
            None => tracing::warn!(path = %look.path.display(),
                "a context file could not be read at the first start that names it, so no version is accepted and none of it is carried until `theseus own-file accept --file`"),
            Some(e) => tracing::warn!(path = %look.path.display(), error = %e,
                "a context file: its accept at the first start that names it could not be written, so no version is accepted and none of it is carried until `theseus own-file accept --file`"),
        }
        Ok(Some(false))
    }

    /// The context file at `path`, which the daemon itself has just written
    /// with `text` at the owner's yes (`write_prompt_file`): that yes is the
    /// accept, and the version kept is `text` as the reader reads it, never
    /// the file read back. Nothing for a path the settings do not name as a
    /// context file. An accept that is not kept is logged and the yes
    /// stands: the file then differs from its accepted version, and the
    /// owner is told so as for any other change.
    pub(crate) fn held_written(&self, path: &std::path::Path, text: &str, by: &str, via: &str) {
        let Some(path) = held::paths(&self.cfg)
            .into_iter()
            .find(|p| std::fs::canonicalize(p).is_ok_and(|real| real == path))
        else {
            return;
        };
        let mut look = held::look(&self.store, &self.runner.context_files, &path);
        look.disk = own_file::Disk::written(look.disk.found.clone(), text);
        if matches!(&look.accepted, Accepted::Some(_)) && !look.differs() {
            return;
        }
        let v = look.disk.version(
            &look.path,
            theseus_protocol::now_unix_ms(),
            by,
            via,
            own_file::HOW_ACCEPT,
        );
        if let Err(e) = self.own_file_kept(&look, &v, false, Whose::Context) {
            tracing::warn!(path = %look.path.display(), error = %e,
                "a context file written at the owner's yes: its accept was not kept, so the version accepted before is still what is carried");
        }
    }

    /// `v` accepted for the file `look` read: its records and its row, in
    /// one frame. The version's number. The pointer is read again under
    /// one lock with the append, and an accept whose pointer moved since
    /// `look` read it is refused ([`own_file::CHANGED`]), and so is a number
    /// a record already stands under ([`own_file::USED`]), so a version's
    /// key is never written twice.
    pub(crate) fn own_file_keep(&self, look: &Look, v: &Version) -> Result<u64> {
        self.own_file_kept(look, v, false, Whose::Own)
    }

    /// [`Core::own_file_keep`]; `durable`: the first start's, which passes
    /// the store's barrier before it answers (`own_file_settle`); `whose`:
    /// the row's words.
    fn own_file_kept(&self, look: &Look, v: &Version, durable: bool, whose: Whose) -> Result<u64> {
        let seq = look.next_seq()?;
        let mut records = look.accept_records(v)?;
        let path = look.path.display().to_string();
        let fact = OwnFileAccepted {
            path: &path,
            digest: v.digest.as_deref().unwrap_or_default(),
            seq,
            bytes: v.bytes,
            how: &v.how,
            by: &v.by,
            via: &v.via,
            whose,
        };
        let rec = self.rec(None);
        records.push(rec.row(&fact)?);
        {
            let _one = own_file::one_at_a_time();
            if self.runner.context_files.own_missed() == Some(Missed::Unknown) {
                bail!(own_file::Unconfirmed);
            }
            if own_file::seq_after(&own_file::read_now(&self.store, &look.pid)) != Some(seq) {
                bail!(own_file::CHANGED);
            }
            if own_file::used(&self.store, &look.pid, seq) {
                bail!(own_file::USED);
            }
            self.own_file_settle(&records, durable)?;
        }
        rec.announce(&fact);
        Ok(seq)
    }

    /// Append an accept's `records`, under the accepts' lock; `durable`:
    /// then pass the store's barrier (`Store::checkpoint`). A frame the
    /// store's log kept while its index did not (`RetainedFrame`), or a
    /// barrier that failed, may be written: this run's mark is set
    /// ([`Missed::Unknown`]), one line is logged, and the answer is
    /// [`own_file::Unconfirmed`], never "not written". Any other error is
    /// the append's.
    fn own_file_settle(&self, records: &[NewRecord], durable: bool) -> Result<()> {
        let e = match self.store.append(records) {
            Ok(_) if !durable => return Ok(()),
            Ok(_) => match self.store.checkpoint() {
                Ok(_) => return Ok(()),
                Err(e) => e,
            },
            Err(e) if e.is::<RetainedFrame>() => e,
            Err(e) => return Err(e),
        };
        self.runner.context_files.set_own_missed(Missed::Unknown);
        tracing::error!(error = %e,
            "your own instruction file: an accept of it may already be written that this run cannot read back, so none of it is carried and every accept is refused until the daemon is restarted");
        bail!(own_file::Unconfirmed)
    }

    /// `theseus health`'s line about the file (`Look::health`).
    pub(super) fn own_file_health(&self) -> Option<String> {
        Look::of(&self.cfg, &self.store, &self.runner.context_files).health(
            &self.store,
            theseus_protocol::now_unix_ms(),
            self.runner.context_files.own_missed(),
        )
    }

    /// `theseus health`'s lines about the context files the settings name,
    /// one for each that has one (`Look::health_as`).
    pub(super) fn held_health(&self) -> Vec<String> {
        let now_ms = theseus_protocol::now_unix_ms();
        held::paths(&self.cfg)
            .iter()
            .filter_map(|p| {
                held::look(&self.store, &self.runner.context_files, p).health_as(
                    &self.store,
                    now_ms,
                    None,
                    Whose::Context,
                )
            })
            .collect()
    }

    /// The file `file` names, as the show and the accept look at it: a
    /// context file the settings name, or the owner's own file without it.
    fn own_file_look(&self, file: Option<&str>) -> Result<(Look, Whose)> {
        let files = &self.runner.context_files;
        let Some(given) = file else {
            return Ok((Look::of(&self.cfg, &self.store, files), Whose::Own));
        };
        let Some(path) = held::named(&self.cfg, given) else {
            bail!(
                "{} is not a context file the settings name, so there is nothing of it to show \
                 or accept",
                given.trim()
            );
        };
        Ok((held::look(&self.store, files, &path), Whose::Context))
    }
}

/// The own file's methods, routed by the owner-in-person dispatch arm
/// (`undo_allow::rpc_owner_in_person`).
pub(super) const OWN: [&str; 2] = [method::OWN_FILE_SHOW, method::OWN_FILE_ACCEPT];

/// The most characters of changed lines the show carries whole; past it,
/// a summary that says it is one.
pub const ROOM: usize = 12_000;

impl Core {
    /// `own_file.show` (a read) and `own_file.accept` (the owner's act).
    pub(super) fn rpc_own_file(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name == method::OWN_FILE_SHOW {
            return route(params, |p: OwnFileShowParams| {
                self.own_file_show_of(p.file.as_deref())
                    .map_err(RpcFailure::invalid)
            });
        }
        route(params, |p: OwnFileAcceptParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            let whose = if p.file.is_some() {
                Whose::Context
            } else {
                Whose::Own
            };
            self.own_file_accept_of(p.file.as_deref(), &p.digest, who)
                .map_err(|e| refused(e, whose))
        })
    }

    /// What is accepted, what is on disk, and the change between them.
    pub fn own_file_show(&self) -> OwnFileShowResult {
        let look = Look::of(&self.cfg, &self.store, &self.runner.context_files);
        Self::own_file_shown(&look)
    }

    /// [`Core::own_file_show`] of `file`, a context file the settings name,
    /// or of the owner's own file without it.
    pub fn own_file_show_of(&self, file: Option<&str>) -> Result<OwnFileShowResult> {
        Ok(Self::own_file_shown(&self.own_file_look(file)?.0))
    }

    fn own_file_shown(look: &Look) -> OwnFileShowResult {
        let accepted = match &look.accepted {
            Accepted::Some(n) | Accepted::Damaged(n) => {
                Some(theseus_protocol::own_file::OwnFileAccepted {
                    seq: n.seq,
                    digest: n.digest.clone(),
                    at_ms: n.at_ms,
                    on: crate::owner_note::day(n.at_ms),
                })
            }
            _ => None,
        };
        let state = match (&look.accepted, look.differs()) {
            (Accepted::None | Accepted::Unread, _) => OwnFileState::NotAccepted,
            (Accepted::Unreadable, _) => OwnFileState::Unreadable,
            (Accepted::Damaged(_), _) => OwnFileState::Damaged,
            (Accepted::Unknown, _) => OwnFileState::Unconfirmed,
            (Accepted::Some(_), false) => OwnFileState::Same,
            (Accepted::Some(n), true) => match (&n.digest, &look.disk.found) {
                (Some(_), Some(_)) => OwnFileState::Differs,
                (Some(_), None) => OwnFileState::Gone,
                (None, _) => OwnFileState::New,
            },
        };
        let change = (!matches!(state, OwnFileState::Unreadable | OwnFileState::Unconfirmed))
            .then(|| {
                let before = look.accepted_text().unwrap_or_default();
                let after = look.disk.text.clone().unwrap_or_default();
                crate::config_change::shown::diff(&before, &after)
            })
            .filter(|d| !d.hunks.is_empty())
            .map(|d| {
                d.block(ROOM).unwrap_or_else(|| {
                    format!(
                        "A summary, not the change itself: {} line(s) added and {} removed, at \
                         {}, too long to show whole here. The file is {}.",
                        d.added,
                        d.removed,
                        d.places(),
                        look.path.display()
                    )
                })
            });
        OwnFileShowResult {
            path: look.path.display().to_string(),
            state,
            accepted,
            bytes: look.disk.found.as_ref().map(|f| f.bytes),
            digest: look.disk.digest.clone().unwrap_or_default(),
            change,
        }
    }

    /// Accept what is on disk, which must still be what the caller was
    /// shown: `digest` is the disk's digest from the show, empty for no
    /// file. The owner's, at their own terminal: `places::owner_in_person_of`
    /// and the CLI's surface, or one `approval.refused` row and the error.
    /// One frame: the version, the pointer, the end of the notice, and the
    /// `own_file.accepted` row. A disk that already holds the accepted
    /// version keeps nothing new.
    pub fn own_file_accept(
        &self,
        digest: &str,
        who: impl Into<Answerer>,
    ) -> Result<OwnFileAcceptResult> {
        self.own_file_accept_of(None, digest, who)
    }

    /// [`Core::own_file_accept`] of `file`, a context file the settings
    /// name, or of the owner's own file without it: the same judge, the same
    /// digest, the same frame.
    pub fn own_file_accept_of(
        &self,
        file: Option<&str>,
        digest: &str,
        who: impl Into<Answerer>,
    ) -> Result<OwnFileAcceptResult> {
        let who = who.into();
        let (look, whose) = self.own_file_look(file)?;
        let path = look.path.display().to_string();
        self.own_file_judge(&who, &path, whose)?;
        if look.disk.digest.as_deref().unwrap_or_default() != digest
            || (digest.is_empty() && look.disk.found.is_some())
        {
            bail!(
                "{path} changed after it was shown, so nothing was accepted: run `{}` again and \
                 read the change first",
                whose.accept(&look.path)
            );
        }
        if let Accepted::Some(n) = &look.accepted {
            if !look.differs() {
                return Ok(OwnFileAcceptResult {
                    path,
                    seq: n.seq,
                    kept: false,
                });
            }
        }
        let v = look.disk.version(
            &look.path,
            theseus_protocol::now_unix_ms(),
            &who.who(),
            &who.via(),
            own_file::HOW_ACCEPT,
        );
        let seq = match whose {
            Whose::Own => self.own_file_keep(&look, &v)?,
            Whose::Context => self.own_file_kept(&look, &v, false, whose)?,
        };
        Ok(OwnFileAcceptResult {
            path,
            seq,
            kept: true,
        })
    }

    /// `Err` when `who` may not accept: the owner in person
    /// (`places::owner_in_person_of`, whose rule is `project trust`'s), and
    /// at a terminal (the CLI's surface). The refusal is one
    /// `approval.refused` row.
    fn own_file_judge(&self, who: &Answerer, path: &str, whose: Whose) -> Result<()> {
        let (why, given, which) = match whose {
            Whose::Own => (
                "your own instruction file is carried in every prompt, so only you accept a \
                 version of it",
                "a version of your own instruction file is accepted",
                "your own instruction file",
            ),
            Whose::Context => (
                "a context file is carried in every prompt, so only you accept a version of it",
                "a version of a context file is accepted",
                "a context file",
            ),
        };
        let verdict =
            crate::places::owner_in_person_of(who, &self.runner.place_rule, &self.cfg, why, given)
                .and_then(|()| match who.surface {
                    Surface::Cli => Ok(()),
                    _ => Err(format!(
                "it came through {}, and a version of {which} is accepted at your own terminal \
                 alone: `{}`",
                who.via(),
                whose.accept(std::path::Path::new(path))
            )),
                });
        let Err(why) = verdict else {
            return Ok(());
        };
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why,
        };
        self.refused(
            Act::Trust {
                method: method::OWN_FILE_ACCEPT,
                what: &format!("The accept of {path}"),
            },
            who,
            &r,
        )?;
        Err(r.into())
    }
}

/// What became of the first start's accept.
enum First {
    Kept(Version),
    /// The disk holds a file the reader cannot read.
    Unread,
    /// The accept was not written: the append's error.
    NotWritten(anyhow::Error),
    /// The store may hold it ([`own_file::Unconfirmed`]).
    Unconfirmed,
}

/// A refusal as the wire's `REFUSED`, with its reason; any other error as
/// invalid.
fn refused(e: anyhow::Error, whose: Whose) -> RpcFailure {
    let which = match whose {
        Whose::Own => "your own instruction file",
        Whose::Context => "a context file",
    };
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "an accept of {which} from {} does not count: {}",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

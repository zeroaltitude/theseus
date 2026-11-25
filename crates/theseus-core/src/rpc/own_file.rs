//! The owner's own instruction file at the daemon's level
//! (`project_context::own_file`): the first start's accept, the line
//! `theseus health` shows while the file on disk is not the accepted one,
//! and the owner's two methods: `own_file.show`, a read, and
//! `own_file.accept`, the owner's act at their own terminal.

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
use crate::project_context::own_file::{self, Accepted, Look, Missed, Version};

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
        match self.own_file_first_accept(&look) {
            First::Kept(v) => return Ok(Some(v)),
            First::Unconfirmed => return Ok(None),
            First::Not => {}
        }
        if self.own_file_unread(&look)? {
            tracing::warn!(path = %look.path.display(),
                "your own instruction file could not be read at this first start, so no version is accepted and none of it is carried until `theseus own-file accept`");
        }
        Ok(None)
    }

    /// The first start's accept of what is on disk, with its line in the
    /// log. [`First::Not`] when the disk holds a file the reader cannot
    /// read, or the accept was not written.
    fn own_file_first_accept(&self, look: &Look) -> First {
        let Some(v) = own_file::first_start(look, &self.store, theseus_protocol::now_unix_ms())
        else {
            return First::Not;
        };
        match self.own_file_kept(look, &v, true) {
            Ok(seq) => {
                tracing::info!(path = %look.path.display(), bytes = v.bytes, seq,
                    present = v.digest.is_some(),
                    "your own instruction file: what is on disk is accepted, once, at this first start; from now on a change to it waits for `theseus own-file accept`");
                First::Kept(v)
            }
            Err(e) if e.is::<own_file::Unconfirmed>() => First::Unconfirmed,
            Err(e) => {
                tracing::warn!(path = %look.path.display(), error = %e,
                    "your own instruction file: the first start's accept was not written, and nothing of it is carried");
                First::Not
            }
        }
    }

    /// The first start recorded as having happened with no version
    /// (`Look::unread_record`), under the accepts' lock, and made durable.
    /// False, and nothing written, when a pointer stands by then; false too
    /// when the store cannot confirm the record ([`own_file::Unconfirmed`]).
    fn own_file_unread(&self, look: &Look) -> Result<bool> {
        let _one = own_file::one_at_a_time();
        if own_file::read_now(&self.store, &look.pid) != Accepted::None {
            return Ok(false);
        }
        let at_ms = theseus_protocol::now_unix_ms();
        match self.own_file_settle(&[look.unread_record(at_ms)?], true) {
            Ok(()) => Ok(true),
            Err(e) if e.is::<own_file::Unconfirmed>() => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// [`Core::own_file_first_start`] off the runtime's workers: it reads
    /// the file and writes one frame. Its error is the caller's.
    pub fn own_file_after_serving(&self) -> Result<()> {
        theseus_store::blocking(|| self.own_file_first_start()).map(|_| ())
    }

    /// `v` accepted for the file `look` read: its records and its row, in
    /// one frame. The version's number. The pointer is read again under
    /// one lock with the append, and an accept whose pointer moved since
    /// `look` read it is refused ([`own_file::CHANGED`]), and so is a number
    /// a record already stands under ([`own_file::USED`]), so a version's
    /// key is never written twice.
    pub(crate) fn own_file_keep(&self, look: &Look, v: &Version) -> Result<u64> {
        self.own_file_kept(look, v, false)
    }

    /// [`Core::own_file_keep`]; `durable`: the first start's, which passes
    /// the store's barrier before it answers (`own_file_settle`).
    fn own_file_kept(&self, look: &Look, v: &Version, durable: bool) -> Result<u64> {
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
        Look::of(&self.cfg, &self.store, &self.runner.context_files)
            .health(&self.store, theseus_protocol::now_unix_ms())
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
            return route(params, |_: OwnFileShowParams| Ok(self.own_file_show()));
        }
        route(params, |p: OwnFileAcceptParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.own_file_accept(&p.digest, who).map_err(refused)
        })
    }

    /// What is accepted, what is on disk, and the change between them.
    pub fn own_file_show(&self) -> OwnFileShowResult {
        let files = &self.runner.context_files;
        let look = Look::of(&self.cfg, &self.store, files);
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
        let who = who.into();
        let look = Look::of(&self.cfg, &self.store, &self.runner.context_files);
        let path = look.path.display().to_string();
        self.own_file_judge(&who, &path)?;
        if look.disk.digest.as_deref().unwrap_or_default() != digest
            || (digest.is_empty() && look.disk.found.is_some())
        {
            bail!(
                "{path} changed after it was shown, so nothing was accepted: run `{}` again and \
                 read the change first",
                theseus_protocol::own_file::accept_command()
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
        let seq = self.own_file_keep(&look, &v)?;
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
    fn own_file_judge(&self, who: &Answerer, path: &str) -> Result<()> {
        let verdict = crate::places::owner_in_person_of(
            who,
            &self.runner.place_rule,
            &self.cfg,
            "your own instruction file is carried in every prompt, so only you accept a version \
             of it",
            "a version of your own instruction file is accepted",
        )
        .and_then(|()| match who.surface {
            Surface::Cli => Ok(()),
            _ => Err(format!(
                "it came through {}, and a version of your own instruction file is accepted at \
                 your own terminal alone: `{}`",
                who.via(),
                theseus_protocol::own_file::accept_command()
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
    /// Not accepted: the disk holds a file the reader cannot read, or the
    /// accept was not written.
    Not,
    /// The store may hold it ([`own_file::Unconfirmed`]).
    Unconfirmed,
}

/// A refusal as the wire's `REFUSED`, with its reason; any other error as
/// invalid.
fn refused(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "an accept of your own instruction file from {} does not count: {}",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

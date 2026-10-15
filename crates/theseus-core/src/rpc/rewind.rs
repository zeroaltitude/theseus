//! `files.changes` and `files.restore` (theseus-23rk, R20): the messages of a
//! session with the files its edit tools changed since each, and the files
//! put back to before one. The conversation's half is `session.fork`, which
//! the surface calls at the node before the message (`RewindPoint.fork_at`).
//! `crate::rewind` holds what they stand on.

use serde_json::json;
use theseus_protocol::{
    error_code, FilesChangesParams, FilesChangesResult, FilesRestoreParams, FilesRestoreResult,
    LedgerKind,
};
use theseus_store::{kinds, NewRecord};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::ledger::LedgerRow;

/// The messages a picker lists.
const POINTS: usize = 40;

fn invalid(m: impl Into<String>) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, m)
}

impl Core {
    /// Whether this connection may see a session's files at all: a file path
    /// is private text, so only a surface that reads private text does.
    fn rewind_reader(&self, conn: &Conn<'_>, session_id: &str) -> Result<(), RpcFailure> {
        let rec = self.session(session_id)?;
        if rec.imported.is_some() {
            return Err(invalid(
                "an imported session is the owner's history, closed and read-only: it has no edits",
            ));
        }
        if !conn.surface.reads_private() {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "{} does not read private text, which a session's file paths are: rewind it \
                     from the CLI, the TUI or the web UI",
                    conn.surface.name()
                ),
            ));
        }
        Ok(())
    }

    /// `files.changes`.
    pub(super) fn files_changes(
        &self,
        p: FilesChangesParams,
        conn: Conn<'_>,
    ) -> Result<FilesChangesResult, RpcFailure> {
        self.rewind_reader(&conn, &p.session_id)?;
        let only = match &p.node {
            Some(n) => Some(self.since_position(&p.session_id, n)?.1),
            None => None,
        };
        let points = crate::rewind::points(&self.store, &p.session_id, (POINTS, only.as_deref()))?;
        if only.is_some() && points.is_empty() {
            return Err(invalid(
                "that node is not a message of yours: a rewind goes back to one of them",
            ));
        }
        Ok(FilesChangesResult {
            session_id: p.session_id,
            points,
            note: theseus_protocol::rewind::NOT_RESTORED.into(),
        })
    }

    /// The WAL position a message is at: a node, or `@<position>`.
    fn since_position(&self, session_id: &str, at: &str) -> Result<(u64, String), RpcFailure> {
        let at = at.trim();
        if let Some(n) = at.strip_prefix('@') {
            let pos = n
                .replace([',', '_'], "")
                .parse::<u64>()
                .map_err(|_| invalid(format!("{at:?} is not a position")))?;
            return Ok((pos, at.into()));
        }
        let found = match self.store.get_node(at)? {
            Some(f) => Some(f),
            None => match crate::reach::resolve(&self.store, at)? {
                crate::reach::Named::One(id) => self.store.get_node(&id)?,
                crate::reach::Named::Refused(m) | crate::reach::Named::Unknown(m) => {
                    return Err(invalid(m))
                }
            },
        };
        let Some((pos, node)) = found else {
            return Err(invalid(format!("no node {at}")));
        };
        if !crate::fork::in_transcript(&self.store, session_id, &node.session_id, pos)? {
            return Err(invalid(format!(
                "{} is not in the transcript of session {session_id}",
                node.id
            )));
        }
        Ok((pos, node.id))
    }

    /// `files.restore`.
    pub(super) fn files_restore(
        &self,
        p: FilesRestoreParams,
        conn: Conn<'_>,
    ) -> Result<FilesRestoreResult, RpcFailure> {
        self.rewind_reader(&conn, &p.session_id)?;
        let (since, node) = self.since_position(&p.session_id, &p.since_node)?;
        if !p.dry_run {
            // Files are the owner's: the owner, from a private place.
            let what = format!("the restore of files in {} to before {node}", p.session_id);
            self.judge_act(
                &conn.answerer(None, None),
                super::confirms::Act::Session {
                    method: theseus_protocol::method::FILES_RESTORE,
                    what: &what,
                },
            )
            .map_err(|e| match e.downcast::<crate::approval::Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "restoring files from {} does not count: {}. Nothing was changed.",
                        r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })?;
        }
        let now = theseus_protocol::now_unix_ms();
        let undo_since = self.store.last_position();
        let sid = p.session_id.clone();
        let (store, dry) = (self.store.clone(), p.dry_run);
        let done =
            theseus_store::blocking(|| crate::rewind::restore(&store, &sid, since, dry, now))?;
        let commands = crate::rewind::commands_after(&self.store, &p.session_id, since)?;
        let words = crate::rewind::words(&done.files, commands, p.dry_run);
        if !p.dry_run && !done.files.is_empty() {
            let row = LedgerRow::new(
                LedgerKind::FilesRestored,
                Some(&p.session_id),
                None,
                json!({
                    "since_node": node,
                    "since_position": since,
                    "files": done.files,
                    "commands": commands,
                    "undo_since": undo_since,
                }),
            );
            let mut frame = Vec::new();
            for (n, u) in done.undo.iter().enumerate() {
                frame.push(u.record(n)?);
            }
            frame.push(NewRecord::json(kinds::LEDGER, None, &row)?);
            self.store.append(&frame)?;
        }
        Ok(FilesRestoreResult {
            session_id: p.session_id,
            since_node: node,
            since_position: since,
            dry_run: p.dry_run,
            files: done.files,
            commands,
            undo_since,
            words,
        })
    }
}

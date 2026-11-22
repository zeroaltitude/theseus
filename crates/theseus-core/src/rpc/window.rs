//! `ledger.window` (theseus-m2tiu): the postmortem's read, one page of a time
//! window's rows.
//!
//! - **Read only, off every lock.** One page of the store's index by the
//!   postmortem's kinds (`theseus_protocol::POSTMORTEM_KINDS`), or by a
//!   session's tag when the window is narrowed; no frame, no session lock,
//!   no execution lock: it costs about its answer, never the history before
//!   its window.
//! - **Narrowed to a session and its tasks.** The scope is the session, and
//!   each task session below it as far down as tasks go, found from the
//!   kernel's task executions (`Kernel::tasks`).
//! - **Scrubbed.** Every string in a row's data passes the secrets'
//!   scrubber, as a tool's output does before history shows it, for every
//!   surface. Any surface may read the whole window (2026-11-22).

use std::collections::HashSet;

use serde_json::Value;
use theseus_protocol::{
    method, LedgerEntry, LedgerWindowParams, LedgerWindowResult, POSTMORTEM_KINDS,
};
use theseus_store::pages::{ledger_kind, ledger_session};
use theseus_store::{kinds, Page};

use super::server::{parse, reply, RpcFailure};
use super::Core;
use crate::ledger::LedgerRow;
use crate::scrub::Scrubber;
use crate::session::SessionRecord;

/// The default page, and the most one call returns.
const DEFAULT_LIMIT: usize = 500;
const MAX_LIMIT: usize = 1000;
/// Rows read from the index for one page of a narrowed window, at most.
const RAW: usize = 1000;
/// The daemon's own kinds: no session, and nothing of a session's text.
const DAEMON_KINDS: &[&str] = &[
    "server.started",
    "server.serving",
    "server.stopping",
    "server.crashed",
    "startup.step",
    "reconcile",
];

impl Core {
    pub(super) fn ledger_reads(&self, name: &str, params: Value) -> Result<Value, RpcFailure> {
        match name {
            method::LEDGER_WINDOW => reply(self.ledger_window(parse(params)?)?),
            _ => reply(self.ledger_tail(parse(params)?)?),
        }
    }

    pub(super) fn ledger_window(
        &self,
        p: LedgerWindowParams,
    ) -> Result<LedgerWindowResult, RpcFailure> {
        let named = p.session_id.as_deref().map(session_of);
        let scope = match &named {
            Some(sid) => {
                self.session_exists(sid)?;
                Some(self.session_and_tasks(sid)?)
            }
            None => None,
        };
        let limit = p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let tags: Vec<String> = match &scope {
            Some(s) => {
                let mut tags: Vec<String> = s.iter().map(|s| ledger_session(s)).collect();
                tags.extend(DAEMON_KINDS.iter().map(|k| ledger_kind(k)));
                tags
            }
            None => POSTMORTEM_KINDS.iter().map(|k| ledger_kind(k)).collect(),
        };
        let in_scope: Option<HashSet<&str>> = scope
            .as_ref()
            .map(|s| s.iter().map(String::as_str).collect());
        let mut after = p.after;
        let mut rows: Vec<LedgerEntry> = Vec::new();
        loop {
            let page = Page {
                kind: kinds::LEDGER,
                tags: tags.clone(),
                after: Some(after.unwrap_or(0)),
                before: None,
                since_ms: Some(p.since_ms),
                until_ms: p.until_ms,
                limit: RAW,
            };
            let Some(got) = self.store.ledger_page(&page)? else {
                return Ok(LedgerWindowResult {
                    building: true,
                    shared: false,
                    now_ms: theseus_protocol::now_unix_ms(),
                    ..Default::default()
                });
            };
            for r in &got.records {
                after = Some(r.position);
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                // The kind's clock may hold a row a little early: its own time decides.
                if row.at_unix_ms < p.since_ms || p.until_ms.is_some_and(|u| row.at_unix_ms > u) {
                    continue;
                }
                if !POSTMORTEM_KINDS.iter().any(|k| row.is_kind(k)) {
                    continue;
                }
                if let (Some(set), Some(sid)) = (&in_scope, row.session_id.as_deref()) {
                    if !set.contains(sid) {
                        continue;
                    }
                }
                rows.push(entry(r.position, row, &self.tools.scrubber));
                if rows.len() == limit {
                    // The rest of this raw page is read again from here.
                    return Ok(window_page(rows, after, scope));
                }
            }
            if !got.more {
                break;
            }
        }
        Ok(window_page(rows, None, scope))
    }

    /// `sid` and every task session below it, by the kernel's task
    /// executions: no turn's lock, a read by their terms.
    fn session_and_tasks(&self, sid: &str) -> Result<Vec<String>, RpcFailure> {
        let mut sessions = vec![sid.to_string()];
        let Some(SessionRecord {
            execution_id: Some(root),
            ..
        }) = self.store.get_session::<SessionRecord>(sid)?
        else {
            return Ok(sessions);
        };
        let tasks = self.kernel.tasks(None)?;
        let mut executions: HashSet<String> = HashSet::from([root]);
        loop {
            let before = executions.len();
            for e in &tasks {
                if e.parent.as_ref().is_some_and(|p| executions.contains(p))
                    && executions.insert(e.id.to_string())
                {
                    sessions.push(e.session_id.to_string());
                }
            }
            if executions.len() == before {
                return Ok(sessions);
            }
        }
    }
}

fn window_page(
    rows: Vec<LedgerEntry>,
    next: Option<u64>,
    scope: Option<Vec<String>>,
) -> LedgerWindowResult {
    LedgerWindowResult {
        rows,
        next,
        scope: scope.unwrap_or_default(),
        building: false,
        shared: false,
        now_ms: theseus_protocol::now_unix_ms(),
    }
}

/// A task's id names its session: `tsk_<tail>` is `ses_<tail>`.
fn session_of(id: &str) -> String {
    match id.strip_prefix("tsk_") {
        Some(tail) => format!("ses_{tail}"),
        None => id.to_string(),
    }
}

fn entry(position: u64, row: LedgerRow, scrubber: &Scrubber) -> LedgerEntry {
    LedgerEntry {
        position,
        at_unix_ms: row.at_unix_ms,
        kind: row.kind,
        session_id: row.session_id,
        turn_id: row.turn_id,
        data: scrubbed(row.data, scrubber),
    }
}

/// `v` with every string passed through the scrubber, keys kept.
fn scrubbed(v: Value, scrubber: &Scrubber) -> Value {
    match v {
        Value::String(s) => Value::String(scrubber.scrub(&s).0),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| scrubbed(x, scrubber)).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, x)| (k, scrubbed(x, scrubber)))
                .collect(),
        ),
        other => other,
    }
}

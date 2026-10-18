//! `work.digest` (theseus-qkoi3): what finished, failed, was answered,
//! cancelled or woke since a position, in a few lines.
//!
//! - **The ring when it reaches back, else the ledger.** The work board keeps
//!   its last 4,096 notices (`push/work.rs`); a window that starts inside them
//!   is read from memory, and one that starts before them (or before the
//!   board existed) is read from the ledger by kind (`task.ended`,
//!   `action.confirm_answered`, `execution.failed`, `wake.fired`,
//!   `tool.late_result`) over the whole window, a page of each kind at a time
//!   from the window's start, on the blocking pool, to at most `HARD_ROWS`
//!   (20,000): past that the lines reach the time the read got to and the
//!   result's `more` says so. The result says which source (`from_ledger`).
//!   Both make the same `Raw` lines and word them with the protocol's one
//!   function; a task that reported carries the first line of its report
//!   (`Core::report_text`, the one reader `task.list` uses too).
//! - **Seen stays the client's.** The daemon keeps no position per surface: a
//!   surface passes the `to` of its last digest, or a time.
//! - **The place rule** (`work.watch`'s): a connection that reads no private
//!   text must name conversations, sees those of a shared place alone, and
//!   every title it is given passes the scrubber and offers no answer.
//! - **No frame, no lock but the board's** for a moment, no timer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;
use theseus_kernel::{Execution, SessionKind};
use theseus_memory::recall::Place;
use theseus_protocol::notices::Why;
use theseus_protocol::work::{WorkKind, WorkRef};
use theseus_protocol::{
    digest_line_with, error_code, fold_digest, DigestKind, DigestLine, DigestMore, LedgerKind,
    WorkDigestParams, WorkDigestResult,
};
use theseus_store::kinds;
use theseus_store::pages::ledger_kind;
use theseus_store::Page;

use super::server::RpcFailure;
use super::Core;
use crate::approval::Surface;
use crate::ledger::LedgerRow;
use crate::push::{WorkDigestRead, WorkScope};
use crate::scrub::Scrubber;
use crate::session::SessionRecord;

/// The ledger rows one digest reads, at most, over every kind. A window that holds more is cut at
/// the time the read reached, and the result says so (`WorkDigestResult.more`).
pub const HARD_ROWS: usize = 20_000;

/// The ledger rows of a window, oldest first, and where the read stopped when it was cut.
type Window = (Vec<(u64, LedgerRow)>, Option<DigestMore>);

/// The rows of one kind read at a time.
const PAGE_ROWS: usize = 500;

/// The ledger kinds the fallback reads.
const KINDS: [LedgerKind; 5] = [
    LedgerKind::TaskEnded,
    LedgerKind::ActionConfirmAnswered,
    LedgerKind::ExecutionFailed,
    LedgerKind::WakeFired,
    LedgerKind::ToolLateResult,
];

/// One thing that happened, before it is worded and folded.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Raw {
    pub kind: DigestKind,
    pub at_ms: u64,
    pub work: WorkRef,
    /// The execution id of the root it happened under.
    pub root: String,
    /// For a task that reported: the first line of its report.
    pub report: Option<String>,
}

impl Core {
    /// `work.digest` for a connection from `surface`.
    pub(crate) async fn work_digest_for(
        self: &Arc<Self>,
        p: WorkDigestParams,
        surface: Surface,
    ) -> Result<WorkDigestResult, RpcFailure> {
        let (scope, named) = self.digest_scope(&p, surface)?;
        self.push.ensure(self).await?;
        let since = self.digest_since(&p)?;
        let core = Arc::clone(self);
        let since_ms = p.since_ms;
        tokio::task::spawn_blocking(move || {
            core.digest_blocking(since, since_ms, &scope, &named, false)
        })
        .await
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
    }

    /// The scope a digest is cut to, and the executions the caller named. The
    /// place rule is `work.watch`'s: a connection that reads no private text
    /// must name conversations and is given a shared place's alone; the
    /// Discord binding, in process, may name a private place's too (the owner's
    /// DM is where it speaks), and only what lies under the roots it names.
    fn digest_scope(
        &self,
        p: &WorkDigestParams,
        surface: Surface,
    ) -> Result<(WorkScope, HashSet<String>), RpcFailure> {
        let guarded = !surface.reads_private();
        let Some(roots) = p.roots.as_ref() else {
            if guarded {
                return Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!(
                        "{} reads no private text: name the conversations (roots) to digest",
                        surface.name()
                    ),
                ));
            }
            return Ok((WorkScope::default(), HashSet::new()));
        };
        let mut named = HashSet::new();
        for sid in roots {
            let Ok(Some(rec)) = self.store.get_session::<SessionRecord>(sid) else {
                continue;
            };
            let allowed = !guarded
                || match self.runner.place_of(sid) {
                    Place::Shared(_) => true,
                    _ => surface == Surface::Discord,
                };
            if let (true, Some(exe)) = (allowed, rec.execution_id) {
                named.insert(exe);
            }
        }
        // A connection that sees private text sees every root; what it names
        // only narrows (a task's own timeline lies under a root it does not
        // name). A guarded one is cut to what it named, even when that is
        // nothing.
        let scope = if guarded {
            WorkScope {
                roots: Some(named.clone()),
                guarded: true,
            }
        } else {
            WorkScope::default()
        };
        if !guarded && named.is_empty() {
            return Err(RpcFailure::new(
                error_code::NOT_FOUND,
                "none of those sessions is a conversation or task",
            ));
        }
        Ok((scope, named))
    }

    /// The position the digest starts after: the caller's, else the one before
    /// the first ledger row at or after the time it named.
    fn digest_since(&self, p: &WorkDigestParams) -> Result<u64, RpcFailure> {
        if let Some(s) = p.since {
            return Ok(s);
        }
        let Some(ms) = p.since_ms else { return Ok(0) };
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![],
            after: Some(0),
            before: None,
            since_ms: Some(ms),
            until_ms: None,
            limit: 1,
        };
        match self.store.ledger_page(&page)? {
            Some(out) => Ok(out.records.first().map_or_else(
                || self.store.last_position(),
                |r| r.position.saturating_sub(1),
            )),
            // The index's shape is built after serving: read from the start
            // and let the lines' own times cut the window.
            None => Ok(0),
        }
    }

    /// The digest, read from the ring when it reaches back to `since` and the
    /// ledger when it does not (`ledger_only`: a test's, to read both for one
    /// window).
    pub(crate) fn digest_blocking(
        &self,
        since: u64,
        since_ms: Option<u64>,
        scope: &WorkScope,
        named: &HashSet<String>,
        ledger_only: bool,
    ) -> Result<WorkDigestResult, RpcFailure> {
        let scrubber: &Scrubber = &self.tools.scrubber;
        let read = self.push.work_digest_read(since, scope, named, scrubber);
        let mut raws: Vec<Raw>;
        let mut more = None;
        let from_ledger = ledger_only || !read.complete;
        if !from_ledger {
            raws = ring_raws(&read);
            raws.extend(self.fired_raws(&read, scope, named, scrubber));
        } else {
            (raws, more) = self.ledger_raws(since, scope, named, &read, scrubber)?;
        }
        if let Some(ms) = since_ms {
            raws.retain(|r| r.at_ms >= ms);
        }
        self.with_reports(&mut raws, scope, scrubber);
        raws.sort_by(|a, b| (a.at_ms, &a.work.id).cmp(&(b.at_ms, &b.work.id)));
        let mut roots: HashMap<String, WorkRef> = HashMap::new();
        let entries = raws
            .into_iter()
            .map(|r| {
                let root = roots
                    .entry(r.root.clone())
                    .or_insert_with(|| self.root_ref(&r.root, &read, scope, scrubber))
                    .clone();
                (digest_line_with(r.kind, r.at_ms, r.work, r.report), root)
            })
            .collect();
        let lines: Vec<DigestLine> = fold_digest(entries);
        Ok(WorkDigestResult {
            from: since,
            to: read.position,
            lines,
            from_ledger,
            now: read.now,
            questions: read.questions,
            more,
        })
    }

    /// A root as a reference: its title from the board, else its session's.
    fn root_ref(
        &self,
        id: &str,
        read: &WorkDigestRead,
        scope: &WorkScope,
        scrubber: &Scrubber,
    ) -> WorkRef {
        let exec = self.kernel.execution(id).ok().flatten();
        let title = read
            .titles
            .get(id)
            .cloned()
            .or_else(|| exec.as_ref().map(|e| self.digest_title(&e.session_id)))
            .unwrap_or_default();
        WorkRef {
            id: id.to_string(),
            kind: WorkKind::Conversation,
            session_id: exec.map(|e| e.session_id),
            title: shown(scope, scrubber, title),
        }
    }

    fn digest_title(&self, session: &str) -> String {
        // A task session's name is its task's title (the store labels every
        // task `task`); a conversation's is its label or its first words.
        let task = crate::task_graph::get(&self.store, &crate::task_graph::of_session(session))
            .ok()
            .flatten()
            .map(|t| t.title)
            .filter(|t| !t.is_empty());
        if let Some(t) = task {
            return t;
        }
        match self.store.get_session::<SessionRecord>(session) {
            Ok(Some(s)) => s
                .label
                .filter(|l| !l.is_empty())
                .or(s.title.filter(|t| !t.is_empty()))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// The root of an execution: up its parents.
    fn root_exec(&self, mut e: Execution) -> String {
        for _ in 0..16 {
            let Some(p) = e.parent.clone() else { break };
            match self.kernel.execution(&p) {
                Ok(Some(up)) => e = up,
                _ => break,
            }
        }
        e.id
    }

    /// The ledger's rows of the digest's kinds after `since`, as lines, and where the read stopped
    /// when the window held more than `HARD_ROWS`.
    fn ledger_raws(
        &self,
        since: u64,
        scope: &WorkScope,
        named: &HashSet<String>,
        read: &WorkDigestRead,
        scrubber: &Scrubber,
    ) -> Result<(Vec<Raw>, Option<DigestMore>), RpcFailure> {
        let (rows, more) = self.digest_window(since, HARD_ROWS)?;
        let mut out = Vec::new();
        for (_, row) in rows {
            let Some(raw) = self.raw_of(&row, scope, scrubber) else {
                continue;
            };
            if self.in_scope(&raw, scope, named, read) {
                out.push(raw);
            }
        }
        Ok((out, more))
    }

    /// The first line of a reported task's report, for each such line: the one reader `task.list`
    /// uses, scrubbed as the title is. Only a task that reported is read, so a plain window reads
    /// nothing.
    fn with_reports(&self, raws: &mut [Raw], scope: &WorkScope, scrubber: &Scrubber) {
        for r in raws
            .iter_mut()
            .filter(|r| r.kind == DigestKind::Reported && r.work.kind == WorkKind::Task)
        {
            let session = r.work.session_id.clone().or_else(|| {
                self.kernel
                    .execution(&r.work.id)
                    .ok()
                    .flatten()
                    .map(|e| e.session_id)
            });
            let Some(session) = session else { continue };
            let rec = crate::task_graph::get(&self.store, &crate::task_graph::of_session(&session))
                .ok()
                .flatten();
            r.report = self
                .report_text(rec.as_ref(), None)
                .and_then(|t| super::task_history::first_line(&t))
                .map(|l| shown(scope, scrubber, l));
        }
    }

    /// The window's rows of the digest's kinds after `since`, oldest first, a page of each kind at a
    /// time. At `cap` (`HARD_ROWS`) rows the read stops: rows after the time the slowest kind reached are left
    /// out, so the lines are whole up to it, and `more` says when and what lists the rest.
    pub(crate) fn digest_window(&self, since: u64, cap: usize) -> Result<Window, RpcFailure> {
        struct Lane {
            tag: String,
            cursor: u64,
            at_ms: u64,
            more: bool,
        }
        let mut lanes: Vec<Lane> = KINDS
            .iter()
            .map(|k| Lane {
                tag: ledger_kind(k.as_str()),
                cursor: since,
                at_ms: 0,
                more: true,
            })
            .collect();
        let mut rows: Vec<(u64, LedgerRow)> = Vec::new();
        let mut read = 0usize;
        while read < cap && lanes.iter().any(|l| l.more) {
            for l in lanes.iter_mut().filter(|l| l.more) {
                let page = Page {
                    kind: kinds::LEDGER,
                    tags: vec![l.tag.clone()],
                    after: Some(l.cursor),
                    before: None,
                    since_ms: None,
                    until_ms: None,
                    limit: PAGE_ROWS,
                };
                // The index's shape is built after serving: a scan of the tail.
                let Some(out) = self.store.ledger_page(&page)? else {
                    return Ok((self.ledger_tail_window(since)?, None));
                };
                for r in out.records {
                    read += 1;
                    if let Ok(row) = r.decode::<LedgerRow>() {
                        l.at_ms = row.at_unix_ms;
                        rows.push((r.position, row));
                    }
                }
                l.cursor = out.last.unwrap_or(l.cursor);
                l.more = out.more;
            }
        }
        let stopped = lanes.iter().filter(|l| l.more).min_by_key(|l| l.cursor);
        let more = stopped.map(|l| {
            let (stop, at_ms) = (l.cursor, l.at_ms);
            rows.retain(|(p, _)| *p <= stop);
            DigestMore {
                since_ms: at_ms,
                command: format!(
                    "theseus tasks --ended --since {}",
                    age_arg(theseus_protocol::now_unix_ms().saturating_sub(at_ms))
                ),
            }
        });
        rows.sort_by_key(|(p, _)| *p);
        Ok((rows, more))
    }

    /// The last `HARD_ROWS` ledger rows of the digest's kinds after `since`, while the index's shape
    /// is still being built.
    fn ledger_tail_window(&self, since: u64) -> Result<Vec<(u64, LedgerRow)>, RpcFailure> {
        let tail: Vec<(u64, LedgerRow)> = self.store.ledger_tail(HARD_ROWS)?;
        Ok(tail
            .into_iter()
            .filter(|(p, r)| *p > since && KINDS.iter().any(|k| r.is_kind(k.as_str())))
            .collect())
    }

    /// Whether a line is one this connection may see and the caller asked for.
    fn in_scope(
        &self,
        raw: &Raw,
        scope: &WorkScope,
        named: &HashSet<String>,
        read: &WorkDigestRead,
    ) -> bool {
        let here = named.is_empty()
            || named.contains(&raw.root)
            || read.under.contains(&raw.work.id)
            || raw
                .work
                .session_id
                .as_deref()
                .is_some_and(|s| self.named_session(s, named));
        scope.sees(&raw.root) && here
    }

    /// The board's `wake.fired` rows as lines: a wake is told from its row
    /// on both paths, so it is told at one position (the ring's notice for
    /// it comes a frame earlier, when the due scan queues the turn, and
    /// would be told again from the ledger after a restart).
    fn fired_raws(
        &self,
        read: &WorkDigestRead,
        scope: &WorkScope,
        named: &HashSet<String>,
        scrubber: &Scrubber,
    ) -> Vec<Raw> {
        read.fired
            .iter()
            .filter_map(|(_, row)| self.raw_of(row, scope, scrubber))
            .filter(|raw| self.in_scope(raw, scope, named, read))
            .collect()
    }

    /// Whether `session`'s execution is one the caller named.
    fn named_session(&self, session: &str, named: &HashSet<String>) -> bool {
        match self.store.get_session::<SessionRecord>(session) {
            Ok(Some(s)) => s.execution_id.is_some_and(|e| named.contains(&e)),
            _ => false,
        }
    }

    /// A ledger row as a digest line, or none: not every row of these kinds
    /// is something the owner would want told.
    fn raw_of(&self, row: &LedgerRow, scope: &WorkScope, scrubber: &Scrubber) -> Option<Raw> {
        let d = Row {
            data: &row.data,
            at_ms: row.at_unix_ms,
            scope,
            scrubber,
        };
        if row.is_kind(LedgerKind::TaskEnded.as_str()) {
            self.task_ended(&d)
        } else if row.is_kind(LedgerKind::ActionConfirmAnswered.as_str()) {
            self.answered(&d)
        } else if row.is_kind(LedgerKind::ExecutionFailed.as_str()) {
            self.failed(&d)
        } else if row.is_kind(LedgerKind::WakeFired.as_str()) {
            self.woke(&d)
        } else if row.is_kind(LedgerKind::ToolLateResult.as_str()) {
            self.late_failure(&d)
        } else {
            None
        }
    }

    /// `task.ended`: a task reported, failed or was cancelled.
    fn task_ended(&self, d: &Row) -> Option<Raw> {
        let kind = match d.s("state")? {
            "complete" => DigestKind::Reported,
            "failed" | "budget_exhausted" => DigestKind::Failed,
            "cancelled" => DigestKind::Cancelled,
            _ => return None,
        };
        let task = self.kernel.execution(d.s("task")?).ok().flatten()?;
        let root = self.root_exec(task.clone());
        let title = d.show(self.digest_title(&task.session_id));
        Some(Raw {
            kind,
            at_ms: d.at_ms,
            work: WorkRef {
                id: task.id,
                kind: WorkKind::Task,
                session_id: Some(task.session_id),
                title,
            },
            root,
            report: None,
        })
    }

    /// `action.confirm_answered`: a question someone answered (an expiry is
    /// no one's answer).
    fn answered(&self, d: &Row) -> Option<Raw> {
        if d.s("by").is_none_or(|b| b == super::confirms::EXPIRY) {
            return None;
        }
        let id = d.s("correlation_id")?;
        let action = self.kernel.action(id).ok().flatten()?;
        let exec = self.kernel.execution(&action.execution_id).ok().flatten()?;
        Some(Raw {
            kind: DigestKind::Answered,
            at_ms: d.at_ms,
            work: WorkRef {
                id: id.to_string(),
                kind: WorkKind::Question,
                session_id: Some(action.session_id),
                title: d.show(action.tool),
            },
            root: self.root_exec(exec),
            report: None,
        })
    }

    /// `execution.failed`: a conversation failed (a task's failure is its
    /// `task.ended`).
    fn failed(&self, d: &Row) -> Option<Raw> {
        let exec = self.kernel.execution(d.s("execution_id")?).ok().flatten()?;
        if exec.kind == SessionKind::Task {
            return None;
        }
        let title = d.show(self.digest_title(&exec.session_id));
        Some(Raw {
            kind: DigestKind::Failed,
            at_ms: d.at_ms,
            work: WorkRef {
                id: exec.id.clone(),
                kind: WorkKind::Conversation,
                session_id: Some(exec.session_id.clone()),
                title,
            },
            root: exec.id,
            report: None,
        })
    }

    /// `wake.fired`: the work whose wake it was, as the board's notice names
    /// it.
    fn woke(&self, d: &Row) -> Option<Raw> {
        let exec = self.kernel.execution(d.s("execution_id")?).ok().flatten()?;
        let kind = if d.s("purpose") == Some("remind") {
            DigestKind::Reminder
        } else {
            DigestKind::Woke
        };
        let title = d.show(self.digest_title(&exec.session_id));
        let work_kind = if exec.kind == SessionKind::Task {
            WorkKind::Task
        } else {
            WorkKind::Conversation
        };
        Some(Raw {
            kind,
            at_ms: d.at_ms,
            work: WorkRef {
                id: exec.id.clone(),
                kind: work_kind,
                session_id: Some(exec.session_id.clone()),
                title,
            },
            root: self.root_exec(exec),
            report: None,
        })
    }

    /// `tool.late_result`: a job that failed (one that ended well is no news).
    fn late_failure(&self, d: &Row) -> Option<Raw> {
        if d.s("state")? != "failed" {
            return None;
        }
        let id = d.s("correlation_id")?;
        let action = self.kernel.action(id).ok().flatten()?;
        let exec = self.kernel.execution(&action.execution_id).ok().flatten()?;
        Some(Raw {
            kind: DigestKind::Failed,
            at_ms: d.at_ms,
            work: WorkRef {
                id: id.to_string(),
                kind: WorkKind::Job,
                session_id: Some(action.session_id),
                title: d.show(d.s("tool").unwrap_or_default().to_string()),
            },
            root: self.root_exec(exec),
            report: None,
        })
    }
}

/// A ledger row's data, and how its words are shown to this connection.
struct Row<'a> {
    data: &'a Value,
    at_ms: u64,
    scope: &'a WorkScope,
    scrubber: &'a Scrubber,
}

impl Row<'_> {
    fn s(&self, key: &str) -> Option<&str> {
        self.data.get(key).and_then(Value::as_str)
    }

    fn show(&self, title: String) -> String {
        shown(self.scope, self.scrubber, title)
    }
}

/// A title as the scope shows it: scrubbed when guarded.
fn shown(scope: &WorkScope, scrubber: &Scrubber, title: String) -> String {
    if scope.guarded {
        scrubber.scrub(&title).0
    } else {
        title
    }
}

/// The ring's notices and the board's cancels as `Raw` lines. A wake is no
/// notice's: `fired_raws` tells it from its row.
fn ring_raws(read: &WorkDigestRead) -> Vec<Raw> {
    let mut out = Vec::new();
    // A cancelled task's report is its cancel, told once.
    let cancelled: HashSet<&str> = read.cancelled.iter().map(|v| v.id.as_str()).collect();
    for n in &read.notices {
        if n.why == Why::Reported && cancelled.contains(n.work.id.as_str()) {
            continue;
        }
        let kind = match n.why {
            Why::Reported => DigestKind::Reported,
            Why::Failed => DigestKind::Failed,
            Why::Answered => DigestKind::Answered,
            // A job that failed (one that ended well is no news), never a root's rest.
            Why::Finished if n.work.kind == WorkKind::Job && n.line.starts_with('✗') => {
                DigestKind::Failed
            }
            _ => continue,
        };
        out.push(Raw {
            kind,
            at_ms: n.at_ms,
            work: n.work.clone(),
            root: n.root.id.clone(),
            report: None,
        });
    }
    for v in &read.cancelled {
        out.push(Raw {
            kind: DigestKind::Cancelled,
            at_ms: v.at_ms,
            work: v.work_ref(),
            root: v.root.clone(),
            report: None,
        });
    }
    out
}

/// An age for `--since`: whole hours (at least one), or days from two days up.
fn age_arg(ms: u64) -> String {
    let hours = ms.div_ceil(3_600_000).max(1);
    if hours >= 48 {
        format!("{}d", hours.div_ceil(24))
    } else {
        format!("{hours}h")
    }
}

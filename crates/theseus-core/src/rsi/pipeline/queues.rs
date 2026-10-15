//! The pipeline's queues, read from the record each time (theseus-pw1q.16):
//! the backlog's tasks and their order, the executions of their sessions,
//! and the self-join's rows. Nothing here writes.
//!
//! - **ready**: proposed items in the backlog's order now, each with the
//!   reviews that rejected it (its `review:` evidence) and the last one's
//!   findings, and the branch it was rejected on;
//! - **building**: items in progress with no green `self.branch.built` row
//!   since they started, whose session's turn runs, queues or waits on the
//!   owner, or that started within the grace (its turn not begun yet);
//! - **reviewing**: green branches not joined, held or reviewed at their
//!   head; **joining**: those whose review passed;
//! - **held**: out of the pipeline, the owner's: parked or keel-marked
//!   items (`waiting_human`), and branches a join stopped and holds;
//! - and the moves a tick makes: proposed items rejected twice (`parking`),
//!   builds whose turn ended with no green branch (`ended`), dropped items
//!   with a live build or trees (`dropped`), and joined ones (`joined`).

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use theseus_kernel::ExecState;
use theseus_protocol::tasks::{TaskRecord, TaskState};

use crate::rpc::Core;
use crate::rsi::join::rows_of;
use crate::rsi::tree::{self, Place};

/// One item, where it stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Entry {
    pub item: String,
    pub title: String,
    /// Its place in its queue, from 1.
    pub place: u32,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub session: Option<String>,
    pub execution: Option<String>,
    /// Its session's turn runs, queues or waits on the owner.
    pub live: bool,
    pub since_ms: Option<u64>,
    pub rejections: u32,
    pub why: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Queues {
    pub ready: Vec<Entry>,
    pub building: Vec<Entry>,
    pub reviewing: Vec<Entry>,
    pub joining: Vec<Entry>,
    pub held: Vec<Entry>,
    pub parking: Vec<Entry>,
    pub ended: Vec<Entry>,
    pub dropped: Vec<Entry>,
    pub joined: Vec<Entry>,
}

/// A green branch, as its row says it.
struct Green {
    item: String,
    branch: String,
    head: String,
    at_ms: u64,
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

/// The evidence that names an item's last start: its session, and when.
fn started(t: &TaskRecord) -> Option<(String, u64)> {
    t.evidence
        .iter()
        .rev()
        .find_map(|e| Some((e.identity.strip_prefix("session:")?.to_string(), e.at_ms)))
}

fn execution(t: &TaskRecord) -> Option<String> {
    t.evidence
        .iter()
        .rev()
        .find_map(|e| e.identity.strip_prefix("execution:").map(str::to_string))
        .filter(|e| !e.is_empty())
}

/// The reviews that rejected it, and the last one's note.
fn rejections(t: &TaskRecord) -> (u32, Option<String>, Option<(String, String)>) {
    let rejected: Vec<_> = t
        .evidence
        .iter()
        .filter(|e| e.identity.starts_with("review:"))
        .collect();
    let last = rejected.last();
    let at = last.and_then(|e| {
        let (b, h) = e.identity.strip_prefix("review:")?.rsplit_once('@')?;
        Some((b.to_string(), h.to_string()))
    });
    (rejected.len() as u32, last.and_then(|e| e.note.clone()), at)
}

fn entry(t: &TaskRecord) -> Entry {
    Entry {
        item: t.id.clone(),
        title: t.title.clone(),
        session: t.session.clone(),
        execution: execution(t),
        ..Entry::default()
    }
}

/// What the self-join's rows say: green branches, verdicts by head, joined
/// branches, and heads a join stopped and holds.
struct Seen {
    greens: Vec<Green>,
    verdicts: HashMap<(String, String), (String, String)>,
    joined: Vec<String>,
    stopped: HashMap<(String, String), String>,
}

impl Seen {
    fn read(store: &crate::store::Store) -> Result<Option<Seen>> {
        let Some(rows) = rows_of(
            store,
            &[
                "self.branch.built",
                "self.review.verdict",
                "self.joined",
                "self.join.stopped",
            ],
        )?
        else {
            return Ok(None);
        };
        let mut greens: Vec<Green> = Vec::new();
        let mut verdicts: HashMap<(String, String), (String, String)> = HashMap::new();
        let mut joined: Vec<String> = Vec::new();
        let mut stopped: HashMap<(String, String), String> = HashMap::new();
        for r in rows {
            let d = &r.data;
            match r.kind.as_str() {
                "self.branch.built" if d["gate"] == "green" && d["head"].is_string() => {
                    greens.push(Green {
                        item: s(d, "item"),
                        branch: s(d, "branch"),
                        head: s(d, "head"),
                        at_ms: r.at_unix_ms,
                    });
                }
                "self.review.verdict" => {
                    let why = d["reasons"][0].as_str().unwrap_or_default().to_string();
                    verdicts.insert((s(d, "branch"), s(d, "head")), (s(d, "verdict"), why));
                }
                "self.joined" => joined.push(s(d, "branch")),
                "self.join.stopped" if d["numbers"]["held"] == true => {
                    stopped.insert((s(d, "branch"), s(d, "head")), s(d, "why"));
                }
                _ => {}
            }
        }
        Ok(Some(Seen {
            greens,
            verdicts,
            joined,
            stopped,
        }))
    }
}

/// Reviewing and joining, and the held branches: each green branch's newest
/// head that has not joined.
fn branches(
    q: &mut Queues,
    greens: &[Green],
    verdicts: &HashMap<(String, String), (String, String)>,
    joined: &[String],
    stopped: &HashMap<(String, String), String>,
    by_id: &HashMap<&str, &TaskRecord>,
) {
    let mut newest: Vec<&Green> = Vec::new();
    for g in greens.iter().rev() {
        if !newest.iter().any(|n| n.branch == g.branch) {
            newest.push(g);
        }
    }
    newest.reverse();
    for g in newest {
        let title = by_id
            .get(g.item.as_str())
            .map_or_else(|| g.item.clone(), |t| t.title.clone());
        let e = Entry {
            item: g.item.clone(),
            title,
            branch: Some(g.branch.clone()),
            head: Some(g.head.clone()),
            since_ms: Some(g.at_ms),
            ..Entry::default()
        };
        if joined.contains(&g.branch) {
            q.joined.push(e);
            continue;
        }
        let key = (g.branch.clone(), g.head.clone());
        if let Some(why) = stopped.get(&key) {
            let mut e = e;
            e.why = Some(why.clone());
            e.place = q.held.len() as u32 + 1;
            q.held.push(e);
            continue;
        }
        match verdicts.get(&key).map(|(v, w)| (v.as_str(), w)) {
            None => {
                let mut e = e;
                e.place = q.reviewing.len() as u32 + 1;
                q.reviewing.push(e);
            }
            Some(("accept", _)) => {
                let mut e = e;
                e.place = q.joining.len() as u32 + 1;
                q.joining.push(e);
            }
            Some(("refused", why)) => {
                let mut e = e;
                e.why = Some(format!("no independent review could run: {why}"));
                e.place = q.held.len() as u32 + 1;
                q.held.push(e);
            }
            // A reject puts the item back to proposed: it is ready.
            Some(_) => {}
        }
    }
}

impl Core {
    /// Whether an execution's turn runs, queues, or waits on the owner.
    fn live(&self, exec: Option<&str>) -> bool {
        exec.and_then(|id| self.kernel.execution(id).ok().flatten())
            .is_some_and(|e| {
                matches!(
                    e.state,
                    ExecState::Running | ExecState::Queued | ExecState::Blocked
                )
            })
    }

    /// The queues now; none while the ledger's index is built.
    pub(crate) fn pipeline_queues(&self, place: &Place, grace: Duration) -> Result<Option<Queues>> {
        let Some(seen) = Seen::read(&self.store)? else {
            return Ok(None);
        };
        let Seen {
            greens,
            verdicts,
            joined,
            stopped,
        } = seen;
        let (tasks, order) = self.backlog_for_pipeline()?;
        let by_id: HashMap<&str, &TaskRecord> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
        let now = theseus_protocol::now_unix_ms();
        let mut q = Queues::default();
        // Ready, in the backlog's order; parking, the twice rejected.
        for id in &order {
            let Some(t) = by_id.get(id.as_str()) else {
                continue;
            };
            if t.state != TaskState::Proposed {
                continue;
            }
            let (n, why, at) = rejections(t);
            let mut e = entry(t);
            e.rejections = n;
            e.why = why;
            if let Some((b, h)) = at {
                // The rejected head as the row has it whole.
                let whole = greens
                    .iter()
                    .rev()
                    .find(|g| g.branch == b && g.head.starts_with(&h))
                    .map_or(h, |g| g.head.clone());
                e.branch = Some(b);
                e.head = Some(whole);
            }
            if n >= 2 {
                q.parking.push(e);
            } else {
                e.place = q.ready.len() as u32 + 1;
                q.ready.push(e);
            }
        }
        // Building, ended, dropped, and the owner's.
        for t in &tasks {
            match t.state {
                TaskState::InProgress => {
                    let Some((session, at)) = started(t) else {
                        continue;
                    };
                    let mut e = entry(t);
                    e.session = Some(session);
                    e.since_ms = Some(at);
                    e.live = self.live(e.execution.as_deref());
                    if greens.iter().any(|g| g.item == t.id && g.at_ms >= at) {
                        continue;
                    }
                    if e.live || now.saturating_sub(at) < grace.as_millis() as u64 {
                        e.place = q.building.len() as u32 + 1;
                        q.building.push(e);
                    } else {
                        q.ended.push(e);
                    }
                }
                TaskState::Abandoned => {
                    let mut e = entry(t);
                    e.session = started(t).map(|(s, _)| s).or(e.session);
                    e.live = self.live(e.execution.as_deref());
                    let trees = tree::find(place, &t.id).is_some()
                        || place.targets().join(tree::slug(&t.id)).exists();
                    if e.live || trees {
                        q.dropped.push(e);
                    }
                }
                TaskState::WaitingHuman => {
                    let mut e = entry(t);
                    e.why = Some(
                        t.evidence
                            .iter()
                            .rev()
                            .find(|v| v.identity.starts_with("parked:"))
                            .map_or("held at the keel: it waits for the owner's word", |_| {
                                "rejected twice: it waits for the owner's word"
                            })
                            .into(),
                    );
                    e.place = q.held.len() as u32 + 1;
                    q.held.push(e);
                }
                _ => {}
            }
        }
        branches(&mut q, &greens, &verdicts, &joined, &stopped, &by_id);
        Ok(Some(q))
    }
}

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
//! - and the moves a tick makes: proposed items rejected twice (`parking`: dropped with a notice,
//!   theseus-o1h3f.6),
//!   builds whose turn ended with no green branch (`ended`), dropped items
//!   with a live build or trees (`dropped`), and joined ones (`joined`);
//! - `stale`: in progress and in none of the queues, with no build behind it (issue 6,
//!   condition 8); a tick settles it as failed;
//! - and `closed`: failed or done items whose trees are still on disk, with
//!   no green branch waiting on a review, a join or the owner (theseus-czlzg),
//!   so a path nobody thought of (the backlog's sweep marking a build failed)
//!   leaves no tree behind.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use theseus_kernel::{ExecState, Wake};
use theseus_protocol::tasks::{TaskRecord, TaskState};

use crate::rpc::Core;
use crate::rsi::join::{rows_of, unproven_accept};
use crate::rsi::tree::{self, Place};

/// How long a finding filed from chat waits before its first build (issue 57). The tender ticks
/// at least every two minutes, so it starts at the first tick after.
pub(crate) const CHAT_HOLD: Duration = Duration::from_secs(60);

/// Whether `t` was filed from chat (`self.file`: its evidence `filed:<the owner>`) less than
/// [`CHAT_HOLD`] before `now`. An item built and rejected since is not held again.
pub(crate) fn chat_held(t: &TaskRecord, now: u64) -> bool {
    let chat = format!("filed:{}", crate::rsi::backlog::tool::OWNER);
    let hold = CHAT_HOLD.as_millis() as u64;
    t.evidence
        .iter()
        .any(|e| e.identity == chat && now < e.at_ms.saturating_add(hold))
}

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
    /// A build whose turn ended on a question to the owner and is waiting for the answer
    /// (`parked`): the question's id.
    pub parked_on: Option<String>,
    pub since_ms: Option<u64>,
    pub rejections: u32,
    pub why: Option<String>,
    /// Every rejecting review's note since the last retry, oldest first.
    pub findings: Vec<String>,
    /// A fix of its branch is under way (issue 36): where, and by which model.
    pub fixing: Option<String>,
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
    pub closed: Vec<Entry>,
    /// In progress, and in no queue above: nothing is building, reviewing or waiting on it.
    pub stale: Vec<Entry>,
    /// The backlog's tasks this was read from, which the withdrawal of an ended build's question
    /// reads again (`parked`).
    pub tasks: Vec<TaskRecord>,
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

/// The reviews that rejected it since the owner last retried it, their
/// notes, and the last one's branch and head.
fn rejections(t: &TaskRecord) -> (u32, Vec<String>, Option<(String, String)>) {
    let rejected: Vec<_> = t
        .evidence
        .iter()
        .rev()
        .take_while(|e| e.identity != "retried:owner")
        .filter(|e| e.identity.starts_with("review:"))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let last = rejected.last();
    let at = last.and_then(|e| {
        let (b, h) = e.identity.strip_prefix("review:")?.rsplit_once('@')?;
        Some((b.to_string(), h.to_string()))
    });
    let notes = rejected.iter().filter_map(|e| e.note.clone()).collect();
    (rejected.len() as u32, notes, at)
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
/// branches, heads a join stopped and holds, and heads a conflict with main
/// returned.
struct Seen {
    greens: Vec<Green>,
    verdicts: HashMap<(String, String), (String, String)>,
    joined: Vec<String>,
    stopped: HashMap<(String, String), String>,
    conflicted: Vec<(String, String)>,
    /// Each fix begun (issue 36), and whether it has ended.
    fixes: Vec<Fixing>,
    /// How many times each head's join gate has run because main moved under it.
    again: HashMap<(String, String), u64>,
}

/// A fix begun, as its rows say it.
struct Fixing {
    item: String,
    branch: String,
    point: String,
    fixer: String,
    at_ms: u64,
    ended: bool,
}

impl Seen {
    /// The fix under way of `branch` at `point`, in the view's words; none when there is none.
    fn fixing(&self, branch: &str, point: &str) -> Option<String> {
        let f = self
            .fixes
            .iter()
            .rev()
            .find(|f| f.branch == branch && f.point == point)?;
        (!f.ended).then(|| fixing_words(f))
    }

    /// The fix at the gate of `item`'s build that began at `since`, under way (issue 36).
    fn fixing_at_gate(&self, item: &str, since: u64) -> Option<String> {
        self.fixes
            .iter()
            .rev()
            .find(|f| f.item == item && f.point == "gate" && f.at_ms >= since)
            .filter(|f| !f.ended)
            .map(fixing_words)
    }
}

/// `at the gate, by fable (claude-fable-5-2)`.
fn fixing_words(f: &Fixing) -> String {
    let at = match f.point.as_str() {
        "gate" => "at the gate",
        _ => "at review",
    };
    match f.fixer.as_str() {
        "" => at.to_string(),
        by => format!("{at}, by {by}"),
    }
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
                "self.join.reopened",
                "self.join.again",
                "self.fix.started",
                "self.fix.refused",
                "self.fix.none",
                "self.fix.verdict",
            ],
        )?
        else {
            return Ok(None);
        };
        let mut fixes: Vec<Fixing> = Vec::new();
        let mut again: HashMap<(String, String), u64> = HashMap::new();
        let mut greens: Vec<Green> = Vec::new();
        let mut verdicts: HashMap<(String, String), (String, String)> = HashMap::new();
        let mut joined: Vec<String> = Vec::new();
        let mut stopped: HashMap<(String, String), String> = HashMap::new();
        let mut conflicted: Vec<(String, String)> = Vec::new();
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
                    let key = (s(d, "branch"), s(d, "head"));
                    // An accept from before the row said what was asked proves nothing: the head
                    // is reviewed again, so it waits in the reviewing queue, not the joining one.
                    match unproven_accept(&s(d, "verdict"), &s(d, "asked")) {
                        true => verdicts.remove(&key),
                        false => verdicts.insert(key, (s(d, "verdict"), why)),
                    };
                }
                "self.joined" => joined.push(s(d, "branch")),
                // The owner's retry closed a head already on main as joined by hand
                // (`join::joining`): it is done, held by no one.
                "self.join.stopped" if d["numbers"]["by_hand"] == true => {
                    joined.push(s(d, "branch"));
                }
                // A conflict with main is no one's hold and no queue's: this head
                // is never joined, and the item is built again (theseus-o1h3f.6).
                "self.join.stopped" if d["numbers"]["conflict"] == true => {
                    conflicted.push((s(d, "branch"), s(d, "head")));
                }
                "self.join.stopped" if d["numbers"]["held"] == true => {
                    stopped.insert((s(d, "branch"), s(d, "head")), s(d, "why"));
                }
                // The owner's retry: the head is held no more, as it stood before the hold. A
                // stop after it holds the head again.
                "self.join.reopened" => {
                    stopped.remove(&(s(d, "branch"), s(d, "head")));
                }
                "self.join.again" => {
                    let n = d["runs"].as_u64().unwrap_or(1);
                    let e = again.entry((s(d, "branch"), s(d, "head"))).or_default();
                    *e = (*e).max(n);
                }
                "self.fix.started" => fixes.push(Fixing {
                    item: s(d, "item"),
                    branch: s(d, "branch"),
                    point: s(d, "point"),
                    fixer: s(d, "fixer"),
                    at_ms: r.at_unix_ms,
                    ended: false,
                }),
                kind if kind.starts_with("self.fix.") => {
                    let (b, p) = (s(d, "branch"), s(d, "point"));
                    if let Some(f) = fixes
                        .iter_mut()
                        .rev()
                        .find(|f| f.branch == b && f.point == p)
                    {
                        f.ended = true;
                    }
                    // An accepted fix at review is the branch's new green head: reviewed afresh.
                    if crate::rsi::join::fix::accepted_at_review(d) {
                        greens.push(Green {
                            item: s(d, "item"),
                            branch: b,
                            head: s(d, "head"),
                            at_ms: r.at_unix_ms,
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(Some(Seen {
            greens,
            verdicts,
            joined,
            stopped,
            conflicted,
            fixes,
            again,
        }))
    }
}

/// Each branch's newest green row, oldest first.
fn newest(greens: &[Green]) -> Vec<&Green> {
    let mut newest: Vec<&Green> = Vec::new();
    for g in greens.iter().rev() {
        if !newest.iter().any(|n| n.branch == g.branch) {
            newest.push(g);
        }
    }
    newest.reverse();
    newest
}

/// Where an item's finished change stands with its join, for the owner's `self retry`.
pub(crate) enum Held {
    /// The ledger's index is still being built: nothing can be read yet.
    Index,
    /// No join holds a change of this item.
    No,
    /// A join holds this branch at this head, for the owner.
    Yes { branch: String, head: String },
}

/// Whether `g` is the change `t` holds now: the item is in progress, and the green row is of its
/// current build (written at or after its last `session:`, as the building queue reads it). A
/// head an item was sent back from (the sealed hold returns the item to its builder, and the
/// conflict path does) is the change of a build that is over: the item's new build is what a
/// join waits for, and that old head is never measured again.
fn is_its_change(t: &TaskRecord, g: &Green) -> bool {
    t.state == TaskState::InProgress && started(t).is_none_or(|(_, at)| g.at_ms >= at)
}

/// The change of `t` that a join stopped and holds for the owner, read from the rows exactly as
/// the queues read them: the item's newest green branch, if it is the item's change now
/// ([`is_its_change`]), has not joined, was not sent back for a conflict with main, and its head
/// has a held stop that no retry has lifted. A failed or dropped item's old branch is not its
/// change, an older branch of an item that has built again is not either, and neither is the head
/// of a sealed hold while its item is rebuilt.
pub(crate) fn join_held(store: &crate::store::Store, t: &TaskRecord) -> Result<Held> {
    if t.state != TaskState::InProgress {
        return Ok(Held::No);
    }
    let Some(seen) = Seen::read(store)? else {
        return Ok(Held::Index);
    };
    let Some(g) = seen.greens.iter().rev().find(|g| g.item == t.id) else {
        return Ok(Held::No);
    };
    let key = (g.branch.clone(), g.head.clone());
    let held = is_its_change(t, g)
        && !seen.joined.contains(&g.branch)
        && !seen.conflicted.contains(&key)
        && seen.stopped.contains_key(&key);
    Ok(match held {
        true => Held::Yes {
            branch: g.branch.clone(),
            head: g.head.clone(),
        },
        false => Held::No,
    })
}

/// Reviewing and joining, and the held branches: each green branch's newest
/// head that has not joined.
fn branches(q: &mut Queues, seen: &Seen, by_id: &HashMap<&str, &TaskRecord>) {
    let Seen {
        greens,
        verdicts,
        joined,
        stopped,
        conflicted,
        ..
    } = seen;
    for g in newest(greens) {
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
        if conflicted.contains(&key) {
            // Its item is on the backlog: this head joins nothing and holds
            // nobody (theseus-o1h3f.6).
            continue;
        }
        if let Some(why) = stopped.get(&key) {
            let mut e = e;
            // The command comes first: the owner's view cuts a long line from its end.
            e.why = Some(match by_id.get(g.item.as_str()) {
                Some(t) if is_its_change(t, g) => format!(
                    "To have Theseus check it again from the first check, run `{}`. It was held: {why}",
                    theseus_protocol::rsi::retry_command(&g.item)
                ),
                _ => why.clone(),
            });
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
                // A join main moved under, as it happens (issue 36, round 2).
                if let Some(n) = seen.again.get(&key) {
                    e.why = Some(format!(
                        "its gate has run {n} times: main moved under it before its push"
                    ));
                }
                e.place = q.joining.len() as u32 + 1;
                q.joining.push(e);
            }
            // A review that asked for a fix: reviewing while the fix is under way or not begun;
            // a fix that did not hold sent the item back, as a reject does (issue 36).
            Some(("fix", _)) => {
                let begun = seen
                    .fixes
                    .iter()
                    .any(|f| f.branch == g.branch && f.point == "review");
                if begun && seen.fixing(&g.branch, "review").is_none() {
                    continue;
                }
                let mut e = e;
                e.fixing = Some(
                    seen.fixing(&g.branch, "review")
                        .unwrap_or_else(|| "at review".into()),
                );
                e.place = q.reviewing.len() as u32 + 1;
                q.reviewing.push(e);
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

/// Failed or done items whose trees are still on disk, but for those whose
/// branch waits on a review, a join or the owner (or a keel mark).
fn closed(q: &mut Queues, tasks: &[TaskRecord], place: &Place) {
    let waiting: Vec<&str> = [&q.reviewing, &q.joining, &q.held, &q.joined]
        .into_iter()
        .flatten()
        .map(|e| e.item.as_str())
        .collect();
    let closed = tasks
        .iter()
        .filter(|t| matches!(t.state, TaskState::Failed | TaskState::Done))
        .filter(|t| !waiting.contains(&t.id.as_str()))
        .filter(|t| {
            let slug = tree::slug(&t.id);
            (tree::find(place, &t.id).is_some()
                || place.targets().join(&slug).exists()
                || place.builders().join(&slug).exists())
                && !place.trees().join(format!("{slug}.keel")).exists()
        })
        .map(entry)
        .collect();
    q.closed = closed;
}

/// In-progress items that no queue holds: an item shows as in progress for as long as its
/// record says so, so one nothing builds, reviews or waits on (its build began with no record
/// of a session, its branch conflicted with main and nothing took it back) would show as in
/// progress for ever.
fn stale(q: &mut Queues, tasks: &[TaskRecord]) {
    let queued: Vec<&str> = [
        &q.ready,
        &q.building,
        &q.reviewing,
        &q.joining,
        &q.held,
        &q.parking,
        &q.ended,
        &q.dropped,
        &q.joined,
        &q.closed,
    ]
    .into_iter()
    .flatten()
    .map(|e| e.item.as_str())
    .collect();
    q.stale = tasks
        .iter()
        .filter(|t| t.state == TaskState::InProgress)
        .filter(|t| !queued.contains(&t.id.as_str()))
        .map(entry)
        .collect();
}

impl Core {
    /// An item the owner set aside (`backlog::aside`): its held entry says so. Whether it still
    /// has a build or trees to stop and close.
    fn set_aside(&self, place: &Place, t: &TaskRecord, e: &mut Entry) -> bool {
        if !crate::rsi::backlog::aside::is_set_aside(t) {
            return false;
        }
        e.why = Some("set aside by the owner: nothing starts it until it is brought back".into());
        e.session = started(t).map(|(s, _)| s).or(e.session.take());
        e.live = self.live(e.execution.as_deref());
        e.live
            || tree::find(place, &t.id).is_some()
            || place.targets().join(tree::slug(&t.id)).exists()
    }

    /// Whether an execution's turn runs, queues, or waits on the owner, or waits on something
    /// that will wake it by itself: a job or call it dispatched (a test run past its call's time
    /// limit goes on as a background job), a child execution, or a due time. Such a build has not
    /// ended; the same reading as health's parked-task invariant (`parked::blocker`).
    fn live(&self, exec: Option<&str>) -> bool {
        exec.and_then(|id| self.kernel.execution(id).ok().flatten())
            .is_some_and(|e| match e.state {
                ExecState::Running | ExecState::Queued | ExecState::Blocked => true,
                ExecState::Waiting => match &e.wake {
                    Some(Wake::Actions { .. } | Wake::Execution { .. } | Wake::DueAt { .. }) => {
                        true
                    }
                    Some(Wake::Input) => !e.wakes.is_empty(),
                    _ => false,
                },
                _ => false,
            })
    }

    /// The queues now; none while the ledger's index is built.
    pub(crate) fn pipeline_queues(&self, place: &Place, grace: Duration) -> Result<Option<Queues>> {
        let Some(seen) = Seen::read(&self.store)? else {
            return Ok(None);
        };
        let (tasks, order) = self.backlog_for_pipeline()?;
        let by_id: HashMap<&str, &TaskRecord> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
        let now = theseus_protocol::now_unix_ms();
        let pending = self.kernel.pending_confirms().unwrap_or_default();
        let mut q = Queues::default();
        // Ready, in the backlog's order; parking, the twice rejected.
        for id in &order {
            let Some(t) = by_id.get(id.as_str()) else {
                continue;
            };
            if t.state != TaskState::Proposed {
                continue;
            }
            // A finding filed from chat waits before its first build (issue 57): its note,
            // written with it, is the owner's to read, and its Undo his to press, first.
            if chat_held(t, now) {
                continue;
            }
            let (n, findings, at) = rejections(t);
            let mut e = entry(t);
            e.rejections = n;
            e.why = findings.last().cloned();
            e.findings = findings;
            if let Some((b, h)) = at {
                // The rejected head as the row has it whole.
                let whole = seen
                    .greens
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
                    if seen.greens.iter().any(|g| g.item == t.id && g.at_ms >= at) {
                        continue;
                    }
                    // A turn that ended on a question to the owner is a build waiting on him, not
                    // one that ended with no branch (`parked`).
                    self.mark_parked(&mut e, &pending, place, &t.id);
                    // A gate still runs in its tree (`tree_lease`): it is building, whatever its
                    // execution says, and its end is not the build's end.
                    let gating = tree::find(place, &t.id).is_some_and(|tr| {
                        crate::rsi::tree_lease::held(&[&tr.dir, &tr.target]).is_some()
                    });
                    if e.live || gating || now.saturating_sub(at) < grace.as_millis() as u64 {
                        e.fixing = seen.fixing_at_gate(&t.id, at);
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
                    if self.set_aside(place, t, &mut e) {
                        // A build the owner set aside stops, as a dropped one does.
                        q.dropped.push(e.clone());
                    }
                    e.place = q.held.len() as u32 + 1;
                    q.held.push(e);
                }
                _ => {}
            }
        }
        branches(&mut q, &seen, &by_id);
        closed(&mut q, &tasks, place);
        stale(&mut q, &tasks);
        q.tasks = tasks;
        Ok(Some(q))
    }
}

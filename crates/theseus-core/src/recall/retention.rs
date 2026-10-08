//! FSRS-6 retention from the memory rows (M6 step 32a's wire-in; design
//! §2.7): each row the memory pass and the operator write becomes the
//! `AccessEvent` it is (`event_of`, pure), at the row's time and in its
//! position's order.
//!
//! | Row | Event | Review |
//! |---|---|---|
//! | `memory.labeled` (scope `memory:<session>`), by `durability` | first sight | Easy, Good, Hard, Again |
//! | `memory.used` (scope `recall:<session>`), `used` with its `outcome` | used | Good (`ok`), Hard (`unknown`), Again (`corrected`) |
//! | `memory.used`, not used | shown | none: exposure is no review |
//! | `memory.label` (scope `memory`) | labeled | Easy (`useful`, `should_have`), Again (`wrong`, `stale`) |
//!
//! `should_have` grades Easy: the operator says recall missed a node that
//! would have helped, the same vouching as `useful` (and §2.9's strongest
//! silver label), and Easy raises the node's stability, so that `+retention`
//! ranks it higher the next time: the remedy for a miss.
//!
//! **The projection** (`Projection`): each node's `Retention`, folded with
//! `Fsrs6` (`fsrs6-default`) from every arm's events (one projection, §5
//! question 9: usefulness belongs to the node), so a shadow turn's use counts
//! as a live one's. It is built after serving, on the blocking pool, only
//! when something reads it (`warm`: the config's arm, or the first search
//! that asks), by one walk of the three kinds' rows through the ledger's
//! index in position order, never on the start path; and kept current as
//! the memory pass's frames and the operator's labels are written
//! (`Memory::retention_written`). Each node keeps its events by position, so
//! a row the build and a write both bring counts once, and one that arrives
//! out of order folds the node again: the projection always equals a
//! rebuild, event for event. A recall before it is built ranks without it,
//! and its row says so.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use theseus_memory::{Access, AccessEvent, Durability, Fsrs6, Label, Outcome, Retention};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use super::Memory;
use crate::ledger::LedgerRow;
use crate::store::Store;

/// The ledger kinds a node's retention is folded from.
pub const KINDS: [LedgerKind; 3] = [
    LedgerKind::MemoryLabeled,
    LedgerKind::MemoryUsed,
    LedgerKind::MemoryLabel,
];

/// The node a memory row is about, and the event it is, at the row's time;
/// `None` for a row of another kind, or one that does not read.
pub fn event_of(row: &LedgerRow) -> Option<(String, AccessEvent)> {
    let d = &row.data;
    let node = d["node_id"].as_str().filter(|n| !n.is_empty())?;
    let access = match row.kind.as_str() {
        k if k == LedgerKind::MemoryLabeled.as_str() => {
            Access::FirstSight(durability(d["durability"].as_str()?)?)
        }
        k if k == LedgerKind::MemoryUsed.as_str() => match d["used"].as_bool()? {
            false => Access::Shown,
            // A used item's row is written once its outcome is known; one
            // without reads as neither gone on nor corrected.
            true => Access::Used(
                d["outcome"]
                    .as_str()
                    .map_or(Some(Outcome::Unknown), outcome)?,
            ),
        },
        k if k == LedgerKind::MemoryLabel.as_str() => Access::Labeled(label(d["label"].as_str()?)?),
        _ => return None,
    };
    Some((
        node.to_string(),
        AccessEvent {
            at_ms: row.at_unix_ms,
            access,
        },
    ))
}

fn durability(s: &str) -> Option<Durability> {
    Some(match s {
        "high" => Durability::High,
        "medium" => Durability::Medium,
        "low" => Durability::Low,
        "floor" => Durability::Floor,
        _ => return None,
    })
}

fn outcome(s: &str) -> Option<Outcome> {
    Some(match s {
        "ok" => Outcome::Ok,
        "unknown" => Outcome::Unknown,
        "corrected" => Outcome::Corrected,
        _ => return None,
    })
}

fn label(s: &str) -> Option<Label> {
    Some(match s {
        "useful" => Label::Useful,
        "should_have" => Label::ShouldHave,
        "wrong" => Label::Wrong,
        "stale" => Label::Stale,
        _ => return None,
    })
}

/// Where the projection stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Nothing has read it: no build, and writes are not followed.
    Unasked,
    /// Its walk of the record runs; writes are followed meanwhile.
    Building,
    Ready,
    /// The walk failed, and why: recall ranks without it.
    Failed(String),
}

impl Phase {
    /// The phase in a word, for a recall's row and health.
    pub fn word(&self) -> &str {
        match self {
            Phase::Unasked => "unbuilt",
            Phase::Building => "building",
            Phase::Ready => "ready",
            Phase::Failed(_) => "failed",
        }
    }
}

/// One node: its events by position, and their fold.
#[derive(Debug, Clone, Default)]
struct Node {
    events: BTreeMap<u64, AccessEvent>,
    retention: Option<Retention>,
}

/// Every node's retention, and where the projection stands.
#[derive(Debug)]
pub struct Projection {
    fsrs: Fsrs6,
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    phase: Phase,
    nodes: BTreeMap<String, Node>,
    events: u64,
}

impl Default for Projection {
    fn default() -> Self {
        Self::new(Fsrs6::default())
    }
}

/// What the projection holds, for health and a metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub phase: Phase,
    /// Nodes with a retention (a review), and the events folded.
    pub nodes: u64,
    pub events: u64,
}

impl Projection {
    pub fn new(fsrs: Fsrs6) -> Self {
        Self {
            fsrs,
            inner: Mutex::new(Inner {
                phase: Phase::Unasked,
                nodes: BTreeMap::new(),
                events: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn fsrs(&self) -> &Fsrs6 {
        &self.fsrs
    }

    pub fn phase(&self) -> Phase {
        self.lock().phase.clone()
    }

    pub fn shape(&self) -> Shape {
        let i = self.lock();
        Shape {
            phase: i.phase.clone(),
            nodes: i.nodes.values().filter(|n| n.retention.is_some()).count() as u64,
            events: i.events,
        }
    }

    /// Asked for the first time: it starts building (the caller walks the
    /// record). `false` once it was asked before.
    pub fn ask(&self) -> bool {
        let mut i = self.lock();
        if i.phase != Phase::Unasked {
            return false;
        }
        i.phase = Phase::Building;
        true
    }

    /// The walk is done, or failed.
    pub fn built(&self, outcome: Result<(), String>) {
        self.lock().phase = match outcome {
            Ok(()) => Phase::Ready,
            Err(why) => Phase::Failed(why),
        };
    }

    /// One row at its position: a memory row's event joins its node, once.
    /// Unasked, nothing is followed: the build reads it from the record.
    pub fn apply(&self, position: u64, row: &LedgerRow) {
        let Some((node, ev)) = event_of(row) else {
            return;
        };
        let mut i = self.lock();
        if i.phase == Phase::Unasked {
            return;
        }
        let n = i.nodes.entry(node).or_default();
        let last = n.events.keys().next_back().copied();
        if n.events.insert(position, ev).is_some() {
            // The build and a write both brought it: it counts once.
            return;
        }
        n.retention = match last {
            Some(l) if position < l => self.fsrs.fold(n.events.values()),
            _ => self.fsrs.step(n.retention, &ev),
        };
        i.events += 1;
    }

    /// A node's retention, when it has had a review.
    pub fn get(&self, node_id: &str) -> Option<Retention> {
        self.lock().nodes.get(node_id).and_then(|n| n.retention)
    }

    /// The retention of each of `ids` that has one.
    pub fn of<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, Retention> {
        let i = self.lock();
        ids.into_iter()
            .filter_map(|id| Some((id.to_string(), i.nodes.get(id)?.retention?)))
            .collect()
    }

    /// Every node's retention (tests: a rebuild against the incremental).
    pub fn all(&self) -> BTreeMap<String, Retention> {
        self.lock()
            .nodes
            .iter()
            .filter_map(|(id, n)| Some((id.clone(), n.retention?)))
            .collect()
    }
}

/// The rows a walk asks for in one page.
const PAGE: usize = 500;
/// How long a build waits for the ledger's index, built after a start,
/// before it asks again.
const INDEX_WAIT: Duration = Duration::from_secs(2);

/// Walk every memory row through the ledger's index, oldest first, into
/// `p`: one page of the three kinds at a time, so their rows interleave in
/// position order. `Ok(false)`: the index's shape is still being built
/// after the start; ask again.
pub fn walk(store: &Store, p: &Projection) -> anyhow::Result<bool> {
    let tags: Vec<String> = KINDS
        .iter()
        .flat_map(|k| crate::rpc::ledger_tags(Some(k.as_str()), None))
        .collect();
    let mut after = 0;
    loop {
        let page = theseus_store::Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: Some(after),
            before: None,
            since_ms: None,
            until_ms: None,
            limit: PAGE,
        };
        let Some(got) = store.ledger_page(&page)? else {
            return Ok(false);
        };
        for r in &got.records {
            if let Ok(row) = r.decode::<LedgerRow>() {
                p.apply(r.position, &row);
            }
        }
        match got.last {
            Some(last) if got.more => after = last,
            _ => return Ok(true),
        }
    }
}

/// Build `memory`'s projection once, after serving: a task that walks the
/// record on the blocking pool, and waits on tokio's timer while the
/// ledger's index is still being built. Asked again, it does nothing.
pub fn warm(memory: &Arc<Memory>, store: &Store) {
    if !memory.retention.ask() {
        return;
    }
    let Ok(rt) = tokio::runtime::Handle::try_current() else {
        memory
            .retention
            .built(Err("no runtime to build it on".into()));
        return;
    };
    let (memory, store) = (Arc::downgrade(memory), store.clone());
    rt.spawn(async move {
        loop {
            let (m, s) = (memory.clone(), store.clone());
            let walked = tokio::task::spawn_blocking(move || {
                let m = m.upgrade()?;
                Some(walk(&s, &m.retention))
            })
            .await;
            let Some(m) = memory.upgrade() else { return };
            match walked {
                Ok(Some(Ok(true))) => {
                    m.retention.built(Ok(()));
                    m.retention_measured();
                    let s = m.retention.shape();
                    tracing::info!(nodes = s.nodes, events = s.events, "memory: the retention projection is built");
                    return;
                }
                Ok(Some(Ok(false))) => {
                    drop(m);
                    tokio::time::sleep(INDEX_WAIT).await;
                }
                Ok(Some(Err(e))) => {
                    tracing::warn!(error = %format!("{e:#}"), "memory: the retention projection cannot be built; recall ranks without it");
                    m.retention.built(Err(format!("{e:#}")));
                    return;
                }
                Ok(None) => return,
                Err(e) => {
                    m.retention.built(Err(format!("its task ended: {e}")));
                    return;
                }
            }
        }
    });
}

impl Memory {
    /// The retention projection (32a).
    pub fn retention(&self) -> &Projection {
        &self.retention
    }

    /// Records just written at `positions` (a frame's): the memory rows
    /// among them join the projection.
    pub fn retention_written(&self, records: &[NewRecord], positions: &[u64]) {
        let mut any = false;
        for (r, &position) in records.iter().zip(positions) {
            if r.kind != kinds::LEDGER {
                continue;
            }
            if let Ok(row) = serde_json::from_slice::<LedgerRow>(&r.payload) {
                any |= event_of(&row).is_some();
                self.retention.apply(position, &row);
            }
        }
        if any {
            self.retention_measured();
        }
    }

    /// Measure the projection's size, where telemetry is built.
    pub fn export_to(&self, t: crate::telemetry::Telemetry) {
        let _ = self.telemetry.set(t);
        self.retention_measured();
    }

    fn retention_measured(&self) {
        if let Some(t) = self.telemetry.get() {
            let s = self.retention.shape();
            if s.phase != Phase::Unasked {
                t.record_retention(s.nodes);
            }
        }
    }

    /// Memory's line in health: the mode, the arm, and the projection.
    pub fn health(&self) -> theseus_protocol::memory::MemoryHealth {
        let s = self.retention.shape();
        theseus_protocol::memory::MemoryHealth {
            mode: self.cfg.mode.as_str().into(),
            arm: self.cfg.arm.as_str().into(),
            retention: s.phase.word().into(),
            why: match s.phase {
                Phase::Failed(why) => Some(why),
                _ => None,
            },
            nodes: s.nodes,
            events: s.events,
            adjacency: None,
            recalls: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use theseus_memory::Grade;

    use super::*;

    fn row(kind: LedgerKind, data: serde_json::Value) -> LedgerRow {
        LedgerRow {
            at_unix_ms: 1_700_000_000_000,
            ..LedgerRow::new(kind, Some("ses_wren"), None, data)
        }
    }

    /// Each row to its event, row by row, and each event to its review by
    /// §2.7's table; a row that does not read is no event.
    #[test]
    fn each_memory_row_is_its_event_and_review() {
        use Access::{FirstSight as Seen, Labeled, Shown, Used};
        use Durability::{Floor, High, Low, Medium};
        use Grade::{Again, Easy, Good, Hard};
        use LedgerKind::{MemoryLabel as L, MemoryLabeled as F, MemoryUsed as U};
        let table = [
            (F, r#"{"durability": "high"}"#, Some(Seen(High)), Some(Easy)),
            (
                F,
                r#"{"durability": "medium"}"#,
                Some(Seen(Medium)),
                Some(Good),
            ),
            (F, r#"{"durability": "low"}"#, Some(Seen(Low)), Some(Hard)),
            (
                F,
                r#"{"durability": "floor"}"#,
                Some(Seen(Floor)),
                Some(Again),
            ),
            (F, r#"{"durability": "eternal"}"#, None, None),
            (F, r#"{}"#, None, None),
            (U, r#"{"used": false, "outcome": null}"#, Some(Shown), None),
            (
                U,
                r#"{"used": true, "outcome": "ok"}"#,
                Some(Used(Outcome::Ok)),
                Some(Good),
            ),
            (
                U,
                r#"{"used": true, "outcome": "unknown"}"#,
                Some(Used(Outcome::Unknown)),
                Some(Hard),
            ),
            (
                U,
                r#"{"used": true, "outcome": "corrected"}"#,
                Some(Used(Outcome::Corrected)),
                Some(Again),
            ),
            (
                U,
                r#"{"used": true}"#,
                Some(Used(Outcome::Unknown)),
                Some(Hard),
            ),
            (U, r#"{"used": true, "outcome": "praised"}"#, None, None),
            (U, r#"{"outcome": "ok"}"#, None, None),
            (
                L,
                r#"{"label": "useful"}"#,
                Some(Labeled(Label::Useful)),
                Some(Easy),
            ),
            (
                L,
                r#"{"label": "should_have"}"#,
                Some(Labeled(Label::ShouldHave)),
                Some(Easy),
            ),
            (
                L,
                r#"{"label": "wrong"}"#,
                Some(Labeled(Label::Wrong)),
                Some(Again),
            ),
            (
                L,
                r#"{"label": "stale"}"#,
                Some(Labeled(Label::Stale)),
                Some(Again),
            ),
            (L, r#"{"label": "remember"}"#, None, None),
            (
                LedgerKind::MemoryGated,
                r#"{"decision": "store"}"#,
                None,
                None,
            ),
        ];
        for (kind, data, access, grade) in table {
            let mut data: serde_json::Value = serde_json::from_str(data).unwrap();
            data["node_id"] = json!("nod_wren1");
            let got = event_of(&row(kind, data.clone())).map(|(n, e)| (n, e.access));
            let want = access.map(|a| ("nod_wren1".to_string(), a));
            assert_eq!(got, want, "{kind:?} {data}");
            assert_eq!(access.and_then(Access::grade), grade, "{kind:?} {data}");
        }
        // The row's time is the event's, and a row with no node is none.
        let (_, ev) =
            event_of(&row(L, json!({"node_id": "nod_wren1", "label": "useful"}))).unwrap();
        assert_eq!(ev.at_ms, 1_700_000_000_000);
        assert_eq!(event_of(&row(L, json!({"label": "useful"}))), None);
        assert_eq!(
            event_of(&row(L, json!({"node_id": "", "label": "useful"}))),
            None
        );
    }
}

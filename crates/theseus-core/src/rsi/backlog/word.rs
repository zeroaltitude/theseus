//! The owner's word on the backlog: `self.backlog` (a read), `self.drop` and
//! `self.move` (the owner's, from any place, as the resume is). A drop
//! abandons the item and remembers its cause, so the cause is never filed
//! again; a move pins the item to a rank until it is moved again.

use anyhow::Result;
use theseus_protocol::rsi::{
    SelfBacklogItem, SelfBacklogParams, SelfBacklogResult, SelfDropParams, SelfDropResult,
    SelfMoveParams, SelfMoveResult,
};
use theseus_protocol::tasks::{TaskRecord, TaskState};

use super::state::State;
use super::{is_open, rank, Night, Work};
use crate::approval::Answerer;
use crate::rpc::{Act, Core};

/// An item the owner named that is not on the backlog, or cannot do what
/// was asked: the caller's mistake, not the daemon's.
#[derive(Debug)]
pub struct BadItem(pub String);

impl std::fmt::Display for BadItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadItem {}

/// The open items' ids in rank order, from their scores now.
fn open_order(w: &mut Work) -> Vec<String> {
    let open: Vec<_> = w
        .tasks
        .values()
        .filter(|t| is_open(t))
        .filter_map(|t| {
            let m = w.st.items.get(&t.id)?;
            Some((t.id.clone(), m.cause.clone(), rank::score(m, w.now), m.pin))
        })
        .collect();
    rank::order(&open)
}

impl Core {
    /// The backlog as the self pipeline reads it (theseus-pw1q.16): every
    /// item, and the open ones' ids in rank order now (the night's rule and
    /// the owner's pins), so a returned item takes its place at once.
    pub(crate) fn backlog_for_pipeline(&self) -> Result<(Vec<TaskRecord>, Vec<String>)> {
        let mut w = self.backlog_work()?;
        let order = open_order(&mut w);
        Ok((w.tasks.into_values().collect(), order))
    }

    pub(super) fn backlog_work(&self) -> Result<Work> {
        self.backlog_work_at(theseus_protocol::now_unix_ms())
    }

    /// `backlog_work` as of `now_ms`: the instant every score is read at.
    fn backlog_work_at(&self, now_ms: u64) -> Result<Work> {
        let st = State::read(&self.store)?;
        Ok(Work {
            tasks: self.backlog_tasks(&st)?,
            parent: None,
            changed: vec![],
            said: vec![],
            now: now_ms,
            st,
        })
    }

    /// The item the owner means, by its id or the end of it.
    fn backlog_item(w: &Work, name: &str) -> Result<String> {
        let all: Vec<_> = w.tasks.values().cloned().collect();
        task_graph_resolve(&all, name).map_err(|e| BadItem(e).into())
    }

    /// The ranked list: open and running items by rank, then (with `all`)
    /// the ones that left.
    pub fn self_backlog(&self, p: &SelfBacklogParams) -> Result<SelfBacklogResult> {
        self.self_backlog_at(p, theseus_protocol::now_unix_ms())
    }

    /// `self_backlog` with every score read at `now_ms`, so two reads at one
    /// instant agree to the milli-point (a score decays with the clock).
    pub fn self_backlog_at(&self, p: &SelfBacklogParams, now_ms: u64) -> Result<SelfBacklogResult> {
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut w = self.backlog_work_at(now_ms)?;
        let order = open_order(&mut w);
        let mut ids: Vec<String> = order.clone();
        let mut others: Vec<_> = w.tasks.values().filter(|t| !is_open(t)).collect();
        others.sort_by(|a, b| (b.created_at_ms, &b.id).cmp(&(a.created_at_ms, &a.id)));
        ids.extend(
            others
                .iter()
                .filter(|t| p.all || t.state == TaskState::InProgress)
                .map(|t| t.id.clone()),
        );
        let items = ids
            .iter()
            .filter_map(|id| {
                let (t, m) = (w.tasks.get(id)?, w.st.items.get(id)?);
                Some(SelfBacklogItem {
                    id: id.clone(),
                    rank: order
                        .iter()
                        .position(|o| o == id)
                        .map_or(0, |i| i as u32 + 1),
                    score: rank::score(m, w.now),
                    title: t.title.clone(),
                    objective: t.objective.clone(),
                    state: t.state.as_str().into(),
                    cause: m.cause.clone(),
                    keel: m.keel.clone(),
                    count: m.count,
                    evidence: t
                        .evidence
                        .iter()
                        .filter(|e| !e.identity.starts_with("cause:"))
                        .map(|e| match &e.note {
                            Some(n) => format!("{}: {n}", e.identity),
                            None => e.identity.clone(),
                        })
                        .collect(),
                    pinned: m.pin.is_some(),
                    session: t.session.clone(),
                    filed_ms: t.created_at_ms,
                })
            })
            .collect();
        Ok(SelfBacklogResult {
            items,
            dropped: w.st.dropped.iter().map(|d| d.cause.clone()).collect(),
            state: self.self_state(),
            max_open: self.cfg.self_improve.backlog_max_open,
            read_ms: w.st.read_ms,
        })
    }

    /// Drop an item: the owner's word. Its cause is never filed again.
    pub fn self_drop(&self, p: &SelfDropParams, who: &Answerer) -> Result<SelfDropResult> {
        let from_job = p
            .from_job
            .as_deref()
            .map(str::trim)
            .filter(|j| !j.is_empty());
        self.judge_act(
            who,
            Act::SelfBacklog {
                method: theseus_protocol::method::SELF_DROP,
                what: &p.item,
                from_job,
            },
        )?;
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut w = self.backlog_work()?;
        let id = Self::backlog_item(&w, &p.item)?;
        let (title, cause) = (w.tasks[&id].title.clone(), w.st.items[&id].cause.clone());
        if w.tasks[&id].state.is_closed() {
            return Ok(SelfDropResult {
                item: id,
                title,
                cause,
                changed: false,
            });
        }
        w.drop_item(&id, &who.who(), "dropped by the owner's word", true);
        self.backlog_write(&mut w)?;
        self.rsi.pipeline.kick();
        Ok(SelfDropResult {
            item: id,
            title,
            cause,
            changed: true,
        })
    }

    /// The owner's veto drops the item its change was built for, and the
    /// cause is never filed again (theseus-pw1q.10). False when the name finds
    /// no item (it left the backlog) or the item was dropped already. The
    /// caller has judged the act; this only writes.
    pub(crate) fn self_drop_vetoed(&self, item: &str, by: &str) -> Result<bool> {
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut w = self.backlog_work()?;
        let Ok(id) = Self::backlog_item(&w, item) else {
            return Ok(false);
        };
        if w.st.dropped.iter().any(|d| d.item == id) {
            return Ok(false);
        }
        w.drop_item(
            &id,
            by,
            "its change was vetoed and reverted by the owner",
            true,
        );
        self.backlog_write(&mut w)?;
        Ok(true)
    }

    /// Move an open item to a rank: the owner's word. It keeps the rank.
    pub fn self_move(&self, p: &SelfMoveParams, who: &Answerer) -> Result<SelfMoveResult> {
        let from_job = p
            .from_job
            .as_deref()
            .map(str::trim)
            .filter(|j| !j.is_empty());
        self.judge_act(
            who,
            Act::SelfBacklog {
                method: theseus_protocol::method::SELF_MOVE,
                what: &p.item,
                from_job,
            },
        )?;
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut w = self.backlog_work()?;
        let id = Self::backlog_item(&w, &p.item)?;
        if !is_open(&w.tasks[&id]) {
            return Err(BadItem(format!(
                "{id} is {}: only an item waiting its turn can move",
                w.tasks[&id].state.as_str()
            ))
            .into());
        }
        let was = open_order(&mut w)
            .iter()
            .position(|o| *o == id)
            .map_or(0, |i| i as u32 + 1);
        let want = p.rank.max(1);
        if let Some(m) = w.st.items.get_mut(&id) {
            m.pin = Some(want);
        }
        let order = open_order(&mut w);
        let mut night = Night::default();
        for (i, o) in order.iter().enumerate() {
            if let Some(m) = w.st.items.get_mut(o) {
                m.score = rank::score(m, w.now);
            }
            w.set_rank(o, i as u32 + 1, "owner", &mut night);
        }
        w.touch(&id);
        let rank = order
            .iter()
            .position(|o| *o == id)
            .map_or(0, |i| i as u32 + 1);
        self.backlog_write(&mut w)?;
        self.rsi.pipeline.kick();
        Ok(SelfMoveResult {
            item: id.clone(),
            title: w.tasks[&id].title.clone(),
            was,
            rank,
        })
    }
}

/// The task a name means among the backlog's: its id or the end of it.
fn task_graph_resolve(
    all: &[theseus_protocol::tasks::TaskRecord],
    name: &str,
) -> Result<String, String> {
    crate::task_graph::resolve(all, name).map(|t| t.id.clone())
}

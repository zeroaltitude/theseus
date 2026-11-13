//! The owner's word on the backlog: `self.backlog` (a read), `self.drop` and
//! `self.move` and `self.retry` and `self.file` (the owner's, from any place,
//! as the resume is). A drop abandons the item and remembers its cause, so
//! the cause is never filed again; a move pins the item to a rank until it is
//! moved again; a retry queues a failed item again; a file puts a person's own
//! finding on the backlog, which until theseus-cux0o only the nightly miner
//! could do ([`super::filed`] is the rule, and its doc the reasoning).

use anyhow::Result;
use theseus_protocol::rsi::{
    SelfBacklogItem, SelfBacklogParams, SelfBacklogResult, SelfDropParams, SelfDropResult,
    SelfFileParams, SelfFileResult, SelfMoveParams, SelfMoveResult, SelfRetryParams,
    SelfRetryResult,
};
use theseus_protocol::tasks::{TaskEvidence, TaskRecord, TaskState};

use super::state::State;
use super::{filed, is_open, rank, Night, Work};
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

    /// A chat session's filing (theseus-cux0o): the same write with no
    /// `judge_act`, because the caller has judged it. `super::tool` is the
    /// only caller, and its own wall is stricter than the place rule here --
    /// a private place the owner is alone in, never a task's -- while the
    /// gate has already asked for a person's yes at every posture
    /// (`Plan::authority`). The shape is `self_drop_vetoed`'s: the caller
    /// judges, this only writes.
    pub(crate) fn self_file_in_chat(
        &self,
        finding: &str,
        title: Option<&str>,
        by: &str,
    ) -> Result<SelfFileResult> {
        let finding = filed::finding_of(finding).map_err(BadItem)?.to_string();
        let title = filed::title_of(&finding, title);
        self.file_on_backlog(&finding, &title, by)
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

    /// Queue a failed item again: the owner's word. It is proposed with its
    /// session cleared and takes its place in the order, so the pipeline
    /// starts it on its next free slot. Any other state is refused, with
    /// words.
    pub fn self_retry(&self, p: &SelfRetryParams, who: &Answerer) -> Result<SelfRetryResult> {
        let from_job = p
            .from_job
            .as_deref()
            .map(str::trim)
            .filter(|j| !j.is_empty());
        self.judge_act(
            who,
            Act::SelfBacklog {
                method: theseus_protocol::method::SELF_RETRY,
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
        if w.tasks[&id].state != TaskState::Failed {
            return Err(BadItem(format!(
                "{id} is {}: only a failed item can be retried",
                w.tasks[&id].state.as_str()
            ))
            .into());
        }
        let now = w.now;
        if let Some(t) = w.tasks.get_mut(&id) {
            t.state = TaskState::Proposed;
            t.session = None;
            t.version += 1;
            t.updated_at_ms = now;
            t.evidence.push(TaskEvidence {
                node: None,
                identity: "retried:owner".into(),
                note: Some("retried by the owner".into()),
                by: who.who(),
                at_ms: now,
            });
        }
        // Ranked as the order now has it, so it shows ready at once.
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
        let title = w.tasks[&id].title.clone();
        self.backlog_write(&mut w)?;
        self.rsi.pipeline.kick();
        Ok(SelfRetryResult {
            item: id,
            title,
            rank,
        })
    }
}

impl Core {
    /// Put a person's own finding on the backlog (theseus-cux0o): the
    /// owner's word, judged and written exactly as `self.drop`,
    /// `self.move` and `self.retry` are.
    ///
    /// Until this verb the only writer was the night's miner, whose
    /// `Work::file` is private, takes a mined `Candidate` and a `Night`, and
    /// is reachable from nowhere else -- so a person who found a defect had
    /// no way to hand it to the loop, and the owner's own chosen shape for
    /// the self-build bar could not be run.
    ///
    /// **It is gated by nothing new, and it starts nothing by itself.** The
    /// item is written whatever `[self] mode` says, as a drop and a move are;
    /// what the mode gates is the loop that would pick it up
    /// (`backlog_night` and the pipeline both ask [`super::super::gate`]
    /// first), so with self-improvement off a filed item sits and nothing
    /// runs. That is the card's own sentence, and it is why no second switch
    /// is added here.
    ///
    /// **The mark is permanent and says who**: [`Work::file_person`] writes
    /// it into the fields a task already has, and the filer is `who.who()`,
    /// which `rpc/server.rs`'s `answerer` already resolves to
    /// `agent:<name>` for a process running under an agent -- so an agent's
    /// filing is marked as the agent's and is never passed off as the
    /// owner's, with no code of its own here.
    pub fn self_file(&self, p: &SelfFileParams, who: &Answerer) -> Result<SelfFileResult> {
        // Read before the act: a finding nobody could file is invalid input,
        // and the title is what the refusal line and the ledger should name
        // rather than the whole of a person's prose.
        let finding = filed::finding_of(&p.finding).map_err(BadItem)?.to_string();
        let title = filed::title_of(&finding, p.title.as_deref());
        let from_job = p
            .from_job
            .as_deref()
            .map(str::trim)
            .filter(|j| !j.is_empty());
        self.judge_act(
            who,
            Act::SelfBacklog {
                method: theseus_protocol::method::SELF_FILE,
                what: &title,
                from_job,
            },
        )?;
        self.file_on_backlog(&finding, &title, &who.who())
    }

    /// The write both filing routes share: one frame with the item, the
    /// record and the rows, under the backlog's own lock.
    fn file_on_backlog(
        &self,
        finding: &str,
        title: &str,
        filed_by: &str,
    ) -> Result<SelfFileResult> {
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut w = self.backlog_work()?;
        Self::ensure_parent(&mut w);
        let id = w.file_person(finding, title, filed_by);
        // Ranked as the order now has it, so it shows ready at once, exactly
        // as a retry does. Nothing is trimmed here: the night's own
        // `Work::rank` is what holds the backlog to `backlog_max_open`.
        let order = open_order(&mut w);
        let mut night = Night::default();
        for (i, o) in order.iter().enumerate() {
            if let Some(m) = w.st.items.get_mut(o) {
                m.score = rank::score(m, w.now);
            }
            w.set_rank(o, i as u32 + 1, filed_by, &mut night);
        }
        w.touch(&id);
        let rank = order
            .iter()
            .position(|o| *o == id)
            .map_or(0, |i| i as u32 + 1);
        let cause = filed::cause_of(&id);
        self.backlog_write(&mut w)?;
        self.rsi.pipeline.kick();
        Ok(SelfFileResult {
            item: id,
            title: title.to_string(),
            cause,
            rank,
            filed_by: filed_by.to_string(),
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

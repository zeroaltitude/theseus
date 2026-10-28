//! The self backlog (theseus-pw1q.7; the owner's decision of 2026-10-09, L7
//! at "Act, notice after"): Theseus files and ranks its own backlog every
//! night, with evidence, and starts the top items. The owner reorders or
//! drops an item with a word, and a dropped item stays dropped.
//!
//! **The night** ([`Core::backlog_night`], run by `backlog_after_serving`
//! an hour after consolidation, on the same nightly pattern, never a timer
//! of its own). It asks [`super::gate`] first and between its phases, and
//! with the mode off or the switch halted it reads nothing and writes
//! nothing. It reads the ledger since the last night (failed calls, failed
//! and refused turns, provider errors, invalid tool input, the owner's
//! routing corrections, Jev's wrong labels, turns past their budgets, spend
//! lines reached, refused acts), health's warnings, and the learning
//! loop's report ([`read`]).
//!
//! **Filing.** Each problem is a cause with a stable key (`turn.failed:
//! timeout`, `provider.error:overloaded:anthropic`, `health:disk`), and each
//! item is a task under one parent, "Self-improvement backlog", with
//! `origin.principal = "self"` and `by_model`. It is `proposed` (no session,
//! so nothing runs it on its own), or `waiting_human` when its cause is in a
//! keel class ([`keel`]). Its evidence entries name the rows. A cause that is
//! open already gets the night's new evidence and no new item; a cause the
//! owner dropped is never filed again. At most `[self] backlog_max_open`
//! items stay open: the lowest-ranked past it are dropped with a reason (and
//! may be filed again if they climb).
//!
//! **Ranking** ([`rank`]): a score from how often, what it costs the owner,
//! how recent and how much work, no model call. The titles and objectives are
//! the cause's own words; a night's one cheap phrasing call, which the plan
//! allows, is not made (it would be the first spend outside a turn, and the
//! budget's `reserve` is self-budget's).
//!
//! **Starting.** Up to `[self] backlog_starts` top unmarked items a night (unset:
//! the pipeline's free build slots, `[self] pipeline_builds` less the builds in
//! flight, theseus-awycv) move to running: a session whose execution's principal is the reserved
//! `"self"` (so the budget counts it, and it holds no owner handle), a
//! `self.backlog.started` row, and one line to the owner's DM. The first turn
//! tells the model to call `self.propose_change { item, issue, why }`, the
//! branch builder's tool; the turn runs from [`tender`], after the night has
//! written.
//!
//! **The owner's word** (`theseus self backlog | drop <item> | move <item>
//! <rank>`): `drop` and `move` are the owner's from any place, as the resume
//! is (`judge_act(Act::SelfBacklog)`, `places::owner_anywhere`), a refusal an
//! `approval.refused` row. A move pins the item to its rank until moved
//! again.
//!
//! What it keeps beside the tasks is one META record ([`state`]).

mod graduate;
mod keel;
mod rank;
mod read;
pub(crate) use read::plain_title;
pub(crate) mod start;
mod state;
mod tender;
mod word;

pub use read::{from_ledger, of_motif, Obs};
pub(crate) use start::{NoTree, Settled};
pub use word::BadItem;

use std::collections::BTreeMap;
use theseus_protocol::tasks::BECAUSE_MAX;

use anyhow::Result;
use theseus_protocol::tasks::{TaskEvidence, TaskOrigin, TaskRecord, TaskState};
use theseus_store::NewRecord;

use self::read::{Candidate, NIGHT_EVIDENCE};
use self::state::{Dropped, Meta, State};
use super::{gate, Gate};
use crate::fact::rsi_backlog::{BacklogDropped, BacklogFiled, BacklogRanked, Spoken};
use crate::rpc::Core;
use crate::task_graph;

/// The reserved principal of every self step's session.
pub const PRINCIPAL: &str = "self";
/// The parent task's title.
pub const PARENT_TITLE: &str = "Self-improvement backlog";
/// A night reads at most this far back (ms).
const MAX_WINDOW_MS: u64 = 2 * 86_400_000;
const DAY_MS: u64 = 86_400_000;

/// What a night did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Night {
    /// What the gate said: `allowed`, `halted` or `off`. Anything else but
    /// `allowed` means the night read and wrote nothing.
    pub gate: String,
    pub candidates: usize,
    pub filed: Vec<String>,
    /// Items that got more evidence.
    pub updated: Vec<String>,
    /// Candidates the owner's drops kept out.
    pub kept_out: usize,
    /// Open items dropped for the backlog's size.
    pub trimmed: Vec<String>,
    /// Candidates that would have ranked past the size: not filed.
    pub too_low: usize,
    pub ranked: usize,
    pub started: Vec<Started>,
    /// Items that did not start, with why: their tree could not be opened.
    pub held: Vec<(String, String)>,
    /// Open items waiting on the owner (keel marks), filed this night.
    pub waiting: Vec<String>,
}

/// An item that started: the session its work runs in.
#[derive(Debug, Clone, PartialEq)]
pub struct Started {
    pub item: String,
    pub title: String,
    pub session_id: String,
    /// The first turn's input.
    pub objective: String,
    /// A minted tool's writer: the placeholder name its pending mint is kept
    /// under (`rsi::mint::run`); `None` for a fix.
    pub mint: Option<String>,
}

/// An item (a task) is open on the backlog while it waits its turn.
fn is_open(t: &TaskRecord) -> bool {
    matches!(t.state, TaskState::Proposed | TaskState::WaitingHuman)
}

fn date(ms: u64) -> String {
    let l = crate::wake::local(ms);
    format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
}

/// A cause is seen often enough to file: noisy kinds need two sightings.
fn min_count(key: &str) -> usize {
    const NOISY: &[&str] = &[
        "tool.invalid_input",
        "route.corrected",
        "approval.refused",
        "loop.cap_reached",
        "turn.first_token",
    ];
    if NOISY.iter().any(|n| key.starts_with(n)) {
        2
    } else {
        1
    }
}

/// The objective a filed item carries: the problem, its evidence, what to do.
fn objective_of(c: &Candidate, item: &str) -> String {
    if let Some(g) = &c.graduate {
        return g.brief.replace("{item}", item);
    }
    if let Some(p) = &c.pattern {
        return p.brief.clone();
    }
    format!(
        "Fix the cause `{}`: {}.\n\nSeen {} times; the evidence is on this task. Find the root \
         cause, fix it with a test that fails without the fix, and call \
         self.propose_change {{ item: \"{item}\", issue: <the issue>, why: <one sentence> }}.",
        c.key,
        c.title,
        c.obs.len()
    )
}

fn evidence_of(o: &read::Obs) -> TaskEvidence {
    TaskEvidence {
        node: None,
        identity: o.id.clone(),
        note: Some(o.note.clone()),
        by: "self-backlog".into(),
        at_ms: o.at_ms,
    }
}

/// A night's working set: the backlog's tasks and record, what changed, and
/// what to say once the frame is written.
struct Work {
    st: State,
    tasks: BTreeMap<String, TaskRecord>,
    changed: Vec<String>,
    parent: Option<TaskRecord>,
    said: Vec<Spoken>,
    now: u64,
}

impl Work {
    fn touch(&mut self, id: &str) {
        if !self.changed.iter().any(|c| c == id) {
            self.changed.push(id.to_string());
        }
    }

    /// A candidate's new evidence onto the open item of its cause.
    fn add(&mut self, id: &str, c: &Candidate, night: &mut Night) {
        let Some(t) = self.tasks.get_mut(id) else {
            return;
        };
        let known: Vec<String> = t.evidence.iter().map(|e| e.identity.clone()).collect();
        let fresh: Vec<&read::Obs> = c.obs.iter().filter(|o| !known.contains(&o.id)).collect();
        if fresh.is_empty() {
            return;
        }
        let Some(m) = self.st.items.get_mut(id) else {
            return;
        };
        m.count += fresh.len() as u64;
        m.usd += fresh.iter().map(|o| o.usd).sum::<f64>();
        m.person |= fresh.iter().any(|o| o.person);
        m.task_failed |= fresh.iter().any(|o| o.task_failed);
        m.work = m.work.max(fresh.iter().map(|o| o.work).max().unwrap_or(1));
        if let Some(p) = fresh.iter().rev().find_map(|o| o.pattern.as_ref()) {
            m.pattern = Some(p.score.clone());
        }
        m.last_ms = m
            .last_ms
            .max(fresh.iter().map(|o| o.at_ms).max().unwrap_or(0));
        let added: Vec<String> = fresh
            .iter()
            .take(NIGHT_EVIDENCE)
            .map(|o| o.id.clone())
            .collect();
        t.evidence
            .extend(fresh.iter().take(NIGHT_EVIDENCE).map(|o| evidence_of(o)));
        t.version += 1;
        t.updated_at_ms = self.now;
        self.said.push(Spoken::Filed(BacklogFiled {
            item: id.into(),
            title: t.title.clone(),
            cause: m.cause.clone(),
            evidence: added,
            source: m.source.clone(),
            again: true,
            count: m.count,
            keel: m.keel.clone(),
        }));
        night.updated.push(id.to_string());
        self.touch(id);
    }

    /// A new item for a candidate.
    fn file(&mut self, c: &Candidate, night: &mut Night) -> String {
        let id = task_graph::id_from(&crate::new_id("x"));
        let keel = keel::class_of(&c.key);
        let mut t = task_graph::NewTask {
            id: id.clone(),
            title: &c.title,
            objective: objective_of(c, &id),
            acceptance: vec![
                "a test that fails without the fix passes with it".into(),
                "every gate passes".into(),
            ],
            parent: self.st.parent.clone(),
            deps: vec![],
            session: None,
            origin: TaskOrigin {
                session: "self:backlog".into(),
                principal: PRINCIPAL.into(),
                by_model: true,
                self_started: true,
                because: Some(c.title.chars().take(BECAUSE_MAX).collect()),
            },
            state: if keel.is_some() {
                TaskState::WaitingHuman
            } else {
                TaskState::Proposed
            },
        }
        .build(self.now);
        t.evidence.push(TaskEvidence {
            node: None,
            identity: format!("cause:{}", c.key),
            note: Some(c.title.clone()),
            by: "self-backlog".into(),
            at_ms: self.now,
        });
        t.evidence
            .extend(c.obs.iter().take(NIGHT_EVIDENCE).map(evidence_of));
        let meta = Meta {
            cause: c.key.clone(),
            source: c.source.clone(),
            count: c.obs.len() as u64,
            usd: c.usd(),
            person: c.person(),
            task_failed: c.task_failed(),
            work: c.work(),
            last_ms: c.last_ms(),
            keel: keel.map(str::to_string),
            score: 0,
            rank: 0,
            pin: None,
            pattern: c.pattern.as_ref().map(|p| p.score.clone()),
        };
        self.said.push(Spoken::Filed(BacklogFiled {
            item: id.clone(),
            title: c.title.clone(),
            cause: c.key.clone(),
            evidence: c
                .obs
                .iter()
                .take(NIGHT_EVIDENCE)
                .map(|o| o.id.clone())
                .collect(),
            source: c.source.clone(),
            again: false,
            count: meta.count,
            keel: meta.keel.clone(),
        }));
        if let Some(g) = &c.graduate {
            self.said
                .push(Spoken::Graduate(g.row_of(&id, meta.keel.clone())));
        }
        if keel.is_some() {
            night.waiting.push(id.clone());
        }
        self.st.items.insert(id.clone(), meta);
        self.tasks.insert(id.clone(), t);
        self.touch(&id);
        night.filed.push(id.clone());
        id
    }

    /// Score and order the open items; drop those past `max_open`; write the
    /// ranks that moved. Returns the ids dropped.
    fn rank(&mut self, max_open: usize, by: &str, night: &mut Night) {
        let open: Vec<(String, String, u64, Option<u32>)> = self
            .tasks
            .values()
            .filter(|t| is_open(t))
            .filter_map(|t| {
                let m = self.st.items.get_mut(&t.id)?;
                m.score = rank::score(m, self.now);
                Some((t.id.clone(), m.cause.clone(), m.score, m.pin))
            })
            .collect();
        let order = rank::order(&open);
        for (i, id) in order.iter().enumerate() {
            if i >= max_open {
                if night.filed.contains(id) {
                    // Filed tonight and ranked past the size: never filed.
                    self.forget(id);
                    night.filed.retain(|f| f != id);
                    night.waiting.retain(|f| f != id);
                    night.too_low += 1;
                } else {
                    self.drop_item(
                        id,
                        by,
                        "past the backlog's size: the lowest-ranked go first",
                        false,
                    );
                    night.trimmed.push(id.clone());
                }
                continue;
            }
            self.set_rank(id, i as u32 + 1, by, night);
        }
    }

    /// Take back an item made tonight, before its frame is written.
    fn forget(&mut self, id: &str) {
        self.tasks.remove(id);
        self.st.items.remove(id);
        self.changed.retain(|c| c != id);
        self.said.retain(|s| s.item() != id);
    }

    fn set_rank(&mut self, id: &str, rank: u32, by: &str, night: &mut Night) {
        let (Some(m), Some(t)) = (self.st.items.get_mut(id), self.tasks.get(id)) else {
            return;
        };
        if m.rank == rank {
            return;
        }
        self.said.push(Spoken::Ranked(BacklogRanked {
            item: id.into(),
            title: t.title.clone(),
            rank,
            score: m.score,
            was: (m.rank > 0).then_some(m.rank),
            by: by.into(),
        }));
        m.rank = rank;
        night.ranked += 1;
        self.touch(id);
    }

    /// An item leaves the backlog: abandoned, with its reason; the owner's
    /// drop also remembers the cause.
    fn drop_item(&mut self, id: &str, by: &str, reason: &str, remember: bool) {
        let (Some(m), Some(t)) = (self.st.items.get_mut(id), self.tasks.get_mut(id)) else {
            return;
        };
        t.state = TaskState::Abandoned;
        t.version += 1;
        t.updated_at_ms = self.now;
        t.evidence.push(TaskEvidence {
            node: None,
            identity: format!("dropped:{by}"),
            note: Some(reason.into()),
            by: by.into(),
            at_ms: self.now,
        });
        m.rank = 0;
        if remember {
            self.st.dropped.push(Dropped {
                cause: m.cause.clone(),
                item: id.into(),
                at_ms: self.now,
                by: by.into(),
            });
        }
        self.said.push(Spoken::Dropped(BacklogDropped {
            item: id.into(),
            title: t.title.clone(),
            cause: m.cause.clone(),
            by: by.into(),
            reason: reason.into(),
            remembered: remember,
        }));
        self.touch(id);
    }

    /// The frame: the parent when new, each changed task, the record, the rows.
    fn frame(&self) -> Result<Vec<NewRecord>> {
        let mut frame = Vec::new();
        if let Some(p) = &self.parent {
            frame.push(task_graph::record(p)?);
        }
        for id in &self.changed {
            if let Some(t) = self.tasks.get(id) {
                frame.push(task_graph::record(t)?);
            }
        }
        frame.push(self.st.record()?);
        for s in &self.said {
            frame.push(s.row()?);
        }
        Ok(frame)
    }
}

impl Core {
    /// The backlog's tasks, by id, under its parent.
    fn backlog_tasks(&self, st: &State) -> Result<BTreeMap<String, TaskRecord>> {
        let Some(parent) = &st.parent else {
            return Ok(BTreeMap::new());
        };
        Ok(task_graph::all(&self.store)?
            .into_iter()
            .filter(|t| t.parent.as_deref() == Some(parent.as_str()))
            .map(|mut t| {
                t.title = read::plain_title(&t.title);
                (t.id.clone(), t)
            })
            .collect())
    }

    /// Write a working set's frame, then say its rows.
    fn backlog_write(&self, w: &mut Work) -> Result<()> {
        if w.changed.is_empty() && w.parent.is_none() && w.said.is_empty() {
            return Ok(());
        }
        self.store.append(&w.frame()?)?;
        let rec = self.rec(None);
        for s in w.said.drain(..) {
            s.announce(&rec);
        }
        w.changed.clear();
        w.parent = None;
        Ok(())
    }

    /// The builds in flight, as the pipeline counts them: the length of its
    /// `building` queue, not the items marked in progress, so a build that went
    /// green or whose turn ended holds no slot. None while the ledger's index
    /// is built, when the pipeline starts nothing either.
    fn pipeline_building(&self) -> Result<Option<usize>> {
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            // No tree to build in: the start itself refuses.
            return Ok(Some(0));
        };
        let place = self.tools.self_pr.place(&repo);
        let queues =
            theseus_store::blocking(|| self.pipeline_queues(&place, super::pipeline::GRACE))?;
        Ok(queues.map(|q| q.building.len()))
    }

    /// One night of the backlog at `now_ms`: the module's doc.
    pub fn backlog_night(&self, now_ms: u64) -> Result<Night> {
        let g = gate(&self.self_ctx());
        let mut night = Night {
            gate: g.name().into(),
            ..Night::default()
        };
        if g != Gate::Allowed {
            return Ok(night);
        }
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cfg = &self.cfg.self_improve;
        let mut st = State::read(&self.store)?;
        let since = st
            .read_ms
            .unwrap_or(now_ms.saturating_sub(DAY_MS))
            .max(now_ms.saturating_sub(MAX_WINDOW_MS));
        let mut obs = read::from_ledger(&self.store, since, now_ms)?;
        obs.extend(read::from_health(&self.health(), now_ms, &date(now_ms)));
        obs.extend(self.graduate_obs(&st, now_ms));
        obs.extend(self.mine_obs(now_ms, &date(now_ms)));
        let cands = read::group(obs);
        night.candidates = cands.len();
        st.read_ms = Some(now_ms);
        let mut w = Work {
            tasks: self.backlog_tasks(&st)?,
            parent: None,
            changed: vec![],
            said: vec![],
            now: now_ms,
            st,
        };
        // An item whose newest build never had a gate run is unjudged: its
        // cause is not filed again as though the code had failed (theseus-otkwu).
        let unjudged = match super::join::rows_of(&self.store, &["self.branch.built"])? {
            Some(rows) => super::infra::unjudged_items(&rows),
            None => Default::default(),
        };
        Self::backlog_file(&mut w, &cands, &unjudged, &mut night);
        w.rank(cfg.backlog_max_open as usize, "night", &mut night);
        self.backlog_write(&mut w)?;
        if gate(&self.self_ctx()) != Gate::Allowed {
            return Ok(night);
        }
        self.backlog_sweep(&mut w);
        // While the pipeline runs, it starts the items (theseus-pw1q.16).
        let starts = match self.rsi.pipeline.runs() {
            true => 0,
            false => match cfg.backlog_starts {
                Some(_) => cfg.night_starts(0),
                None => {
                    // The sweep's endings are written first: the pipeline's
                    // queues read the record, and count the builds in flight
                    // as it does (theseus-awycv).
                    self.backlog_write(&mut w)?;
                    self.pipeline_building()?
                        .map_or(0, |building| cfg.night_starts(building))
                }
            },
        };
        self.backlog_start(&mut w, starts, &mut night, None)?;
        self.backlog_mint_start(&mut w, &mut night)?;
        self.backlog_write(&mut w)?;
        self.backlog_tell(&w, &night);
        Ok(night)
    }

    /// Phase one: the night's candidates onto the backlog.
    fn backlog_file(
        w: &mut Work,
        cands: &[Candidate],
        unjudged: &std::collections::HashSet<String>,
        night: &mut Night,
    ) {
        if w.st.parent.is_none() && !cands.is_empty() {
            let id = task_graph::id_from(&crate::new_id("x"));
            w.parent = Some(
                task_graph::NewTask {
                    id: id.clone(),
                    title: PARENT_TITLE,
                    objective: "The problems Theseus found in its own record, ranked, with the \
                                evidence for each."
                        .into(),
                    acceptance: vec![],
                    parent: None,
                    deps: vec![],
                    session: None,
                    origin: TaskOrigin {
                        session: "self:backlog".into(),
                        principal: PRINCIPAL.into(),
                        by_model: true,
                        self_started: true,
                        because: Some("holds the problems found in its own record".into()),
                    },
                    state: TaskState::Accepted,
                }
                .build(w.now),
            );
            w.st.parent = Some(id);
        }
        for c in cands {
            if w.st.is_dropped(&c.key) {
                night.kept_out += 1;
                continue;
            }
            // A closed item of this cause whose build was never judged: its
            // cause waits for a person (`self retry`), not for a fresh patch.
            if w.st.items.iter().any(|(id, m)| {
                m.cause == c.key
                    && unjudged.contains(id)
                    && w.tasks.get(id).is_some_and(|t| t.state.is_closed())
            }) {
                night.kept_out += 1;
                continue;
            }
            let existing =
                w.st.items
                    .iter()
                    .find(|(id, m)| {
                        m.cause == c.key && w.tasks.get(*id).is_some_and(|t| !t.state.is_closed())
                    })
                    .map(|(id, _)| id.clone());
            match existing {
                Some(id) => w.add(&id, c, night),
                None if c.obs.len() >= min_count(&c.key) => {
                    w.file(c, night);
                }
                None => {}
            }
        }
        // A parent made for a night that filed nothing is not written.
        if night.filed.is_empty() && w.st.items.is_empty() {
            w.parent = None;
            w.st.parent = None;
        }
    }
}

//! The work board's seed (theseus-hkx30): from what the execution board's
//! seed read (the executions that need you or work, the most recent, their
//! parents, and the actions not settled) and the task graph. It reads no
//! settled action and no ledger row: the jobs' commands and their usual
//! times come after serving (`warm`).
//!
//! The seed and a replay of the frames since the store began agree on what a
//! surface shows (`tests_work::seed_equals_replay`); what only a frame can
//! say (a wake that just fired, the loop a turn is in) is empty until one
//! says it.

use std::collections::HashMap;

use theseus_kernel::{Action, Execution};
use theseus_protocol::tasks::TaskRecord;
use theseus_store::Store as _;

use super::{Dirty, WorkBoard};
use crate::push::board::Board;
use crate::push::Frame;
use crate::Core;

/// The board, from the executions and actions the execution board read.
pub(in crate::push) fn seed(
    core: &Core,
    board: &Board,
    execs: &[(u64, Execution)],
    actions: &[(u64, Action)],
) -> WorkBoard {
    let mut w = WorkBoard {
        position: board.position,
        seeded_at: board.position,
        ..WorkBoard::default()
    };
    let mut d = Dirty::default();
    let tasks: Vec<TaskRecord> = crate::task_graph::all(&core.store).unwrap_or_default();
    let mut by_exec: HashMap<String, TaskRecord> = HashMap::new();
    let mut plan: Vec<TaskRecord> = Vec::new();
    for t in tasks {
        match crate::task_graph::execution_of(&t) {
            Some(e) => {
                by_exec.insert(e, t);
            }
            None => plan.push(t),
        }
    }
    for (pos, e) in execs {
        let Some(x) = board.entries.get(&e.id).and_then(|en| en.view.clone()) else {
            continue;
        };
        w.add_exec(core, *pos, e, x, by_exec.remove(&e.id), &mut d);
    }
    // The steps that hang from an execution on the board.
    let all: HashMap<String, TaskRecord> = plan.iter().map(|t| (t.id.clone(), t.clone())).collect();
    w.steps = all;
    let keep: Vec<String> = w
        .steps
        .values()
        .filter(|t| w.owner_exec(t).is_some())
        .map(|t| t.id.clone())
        .collect();
    w.steps.retain(|id, _| keep.contains(id));
    d.steps.extend(w.steps.keys().cloned());
    for (pos, a) in actions {
        if a.awaits_confirm() || super::is_job(a) {
            w.track(a, *pos);
            d.acts.insert(a.correlation_id.clone());
        } else if !a.state.is_settled() && a.tool != theseus_kernel::PROVIDER_TOOL {
            w.track(a, *pos);
        }
    }
    let f = Frame {
        at_ms: theseus_protocol::now_unix_ms(),
        committed: std::time::Instant::now(),
        position: board.position,
        records: Vec::new(),
        nodes: Vec::new(),
    };
    let _ = w.settle(&f, d);
    w.ring.clear();
    w
}

/// Jobs read for the warm-up: the newest settled `proc.run` jobs, whose
/// times become the usual ones.
const WARM_JOBS: usize = 1_000;

/// Records of the newest actions looked at to find them: a job's action is
/// written a few times (planned, authorized, dispatched, settled).
const WARM_ACTIONS: usize = 6_000;

/// The job_started rows read for the programs of those jobs.
const WARM_ROWS: usize = 1_500;

/// What the warm-up read: the usual times (program key, milliseconds), and
/// the commands of the jobs running (correlation id, argv).
pub(in crate::push) type Warm = (Vec<(String, u64)>, Vec<(String, Vec<String>)>);

/// What the warm-up reads, off every lock: the durations of the newest
/// settled jobs under their program's key (oldest first, so the newest are
/// the ones kept), and the commands of `open` jobs (correlation id,
/// session). Blocking. Nothing here is written, and a store whose index is
/// still being built answers nothing.
pub(in crate::push) fn read_warm(core: &Core, open: &[(String, String)]) -> Warm {
    use theseus_store::pages::ledger_kind_session;
    use theseus_store::{kinds, Page};
    let argv_of = |r: &crate::ledger::LedgerRow| -> Option<(String, Vec<String>)> {
        let corr = r.data["correlation_id"].as_str()?.to_string();
        let argv = r.data["argv"]
            .as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        Some((corr, argv))
    };
    let read = |tags: Vec<String>, limit: usize| -> Vec<(String, Vec<String>)> {
        let page = Page {
            kind: kinds::LEDGER,
            tags,
            limit,
            ..Page::default()
        };
        match core.store.ledger_page(&page) {
            Ok(Some(out)) => out
                .records
                .iter()
                .filter_map(|r| r.decode::<crate::ledger::LedgerRow>().ok())
                .filter(|r| r.kind == "tool.job_started")
                .filter_map(|r| argv_of(&r))
                .collect(),
            _ => Vec::new(),
        }
    };
    let started: HashMap<String, Vec<String>> = read(
        crate::rpc::ledger_tags(Some("tool.job_started"), None),
        WARM_ROWS,
    )
    .into_iter()
    .collect();
    let mut argvs = Vec::new();
    for (corr, session) in open {
        let argv = started.get(corr).cloned().or_else(|| {
            read(vec![ledger_kind_session("tool.job_started", session)], 100)
                .into_iter()
                .find(|(c, _)| c == corr)
                .map(|(_, a)| a)
        });
        if let Some(argv) = argv {
            argvs.push((corr.clone(), argv));
        }
    }
    let mut newest: HashMap<String, (u64, Action)> = HashMap::new();
    if let Ok(recs) = core.store.inner().tail_of_kind(kinds::ACTION, WARM_ACTIONS) {
        for r in recs {
            if let Ok(a) = r.decode::<Action>() {
                newest.insert(a.correlation_id.clone(), (r.position, a));
            }
        }
    }
    let mut settled: Vec<&Action> = newest
        .values()
        .map(|(_, a)| a)
        .filter(|a| super::is_job(a) && a.settled_at_ms.is_some())
        .filter(|a| {
            matches!(
                a.state,
                theseus_kernel::ActionState::Succeeded | theseus_kernel::ActionState::Failed
            )
        })
        .collect();
    settled.sort_by_key(|a| a.settled_at_ms);
    let from = settled.len().saturating_sub(WARM_JOBS);
    let runs = settled[from..]
        .iter()
        .filter_map(|a| {
            let key = super::eta::key_of(started.get(&a.correlation_id)?)?;
            Some((key, a.settled_at_ms?.saturating_sub(a.dispatched_at_ms?)))
        })
        .collect();
    (runs, argvs)
}

impl WorkBoard {
    /// The open jobs: correlation id and session.
    pub(in crate::push) fn open_jobs(&self) -> Vec<(String, String)> {
        self.acts
            .values()
            .filter(|a| super::is_job(&a.a) && !a.a.state.is_settled() && a.argv.is_empty())
            .map(|a| (a.a.correlation_id.clone(), a.a.session_id.clone()))
            .collect()
    }

    /// What the board learns after serving: jobs' usual times, and the
    /// commands of the jobs running now. A view it changes is changed in
    /// place, at its own position (a client that applies by the position
    /// rule would drop a second view at that position), and marked stale:
    /// the next frame sends it again, at its own position. A new watcher's
    /// snapshot has it at once.
    pub(in crate::push) fn warm(
        &mut self,
        runs: Vec<(String, u64)>,
        argvs: Vec<(String, Vec<String>)>,
    ) {
        for (key, ms) in runs {
            self.eta.note(&key, ms);
        }
        let mut ids: Vec<String> = Vec::new();
        for (corr, argv) in argvs {
            if let Some(ai) = self.acts.get_mut(&corr) {
                if ai.argv.is_empty() && !argv.is_empty() {
                    ai.argv = argv;
                    ids.push(corr);
                }
            }
        }
        // A running job's estimate may have just become known.
        ids.extend(
            self.acts
                .iter()
                .filter(|(_, a)| super::is_job(&a.a) && !a.a.state.is_settled())
                .map(|(c, _)| c.clone()),
        );
        for id in ids {
            let Some(new) = self.act_view(&id) else {
                continue;
            };
            let exec = new.parent.clone();
            self.refresh_silent(new);
            if let Some(e) = exec.and_then(|e| self.exec_view(&e)) {
                self.refresh_silent(e);
            }
        }
    }

    /// Replace a held view with a rebuilt one, keeping what the frame that
    /// made it said: its position, time, and state before.
    pub(super) fn refresh_silent(&mut self, mut new: theseus_protocol::work::WorkView) {
        let Some(old) = self.views.get(&new.id) else {
            return;
        };
        new.position = old.position;
        new.at_ms = old.at_ms;
        new.previous = old.previous;
        new.state_since_ms = old.state_since_ms;
        new.below = old.below;
        new.progress = old.progress.clone();
        new.attention = theseus_protocol::work::attention_of(&new);
        if super::settle::same_work(old, &new) {
            return;
        }
        self.stale.insert(new.id.clone());
        self.install(new);
    }
}

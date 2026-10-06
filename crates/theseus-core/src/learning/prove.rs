//! The prove's records (M5 L3, roadmap row 50; design §2.9, "The prove"):
//! one `theseus_judge::prove::TaskRecord` per finished task, built from the
//! ledger, for the generator to read. [`build`] is pure over the rows it is
//! handed; [`Core::prove_input`] reads them from the store, through the
//! index's pages, off the workers.
//!
//! A task is a kernel task: an execution a turn opened with `task.create`,
//! whose end is its `task.ended` row (in its parent's session). One record a
//! task, the first `task.ended` row naming it. The task graph's `task.closed`
//! makes no record and changes none: its report close is the same end said
//! again (`done` for `complete`, `failed` for `failed` or `budget_exhausted`),
//! and a close by hand closes a record, not an execution.
//!
//! Each field, as §2.9 and `theseus_judge::prove` define it, from what the
//! learning ledger already holds (its labels resolved by `labels::resolve`,
//! heaviest first, so nothing here re-derives a label's rule):
//!
//! - **`arm`**: the `pack_arm` (26a, `Judge::ask_mode`) of the `loop.v1`
//!   judgments whose context names the task's session. Arms are sticky per
//!   session (`learn::arm`), so they agree; a task with none is left out
//!   (`never_judged`), one judged only outside a canary (`all`, or before
//!   26a) is left out (`no_arm`), and one with both is left out
//!   (`both_arms`). Only `loop.v1`'s judgments count: another version's
//!   (a candidate in shadow) says nothing of this canary. A learned version
//!   in loop.v1's lineage (`loop.v101`) judges at the loop point in its
//!   place (`JudgeService::placed`), so a task it judged was judged, not by
//!   loop.v1: it is left out as `learned_version`, never `never_judged`
//!   (theseus-ag0t). So is a task judged by both: its stops were not all
//!   loop.v1's, and the version that judged its last one may not have been,
//!   so its outcome is not loop.v1's arm's alone.
//! - **`stops`**: each of those judgments in the task's arm, oldest first.
//!   Its `decision` is the one that acted in its arm: the control's is the
//!   baseline's (`decision` in its context: `no_tool_calls` stops); the
//!   canary's is Jev's by §2.8a's rule (continue when `work_state` acts on
//!   `progressing` or `announced_unfinished` acts true; else, and when Jev
//!   did not answer, the baseline's). Until 26b builds the nudge, the turn
//!   ends either way. `should_stop` is its resolved `work_state` label:
//!   `complete` is true; another class, or "not complete", false; a label
//!   that only rules out another class says nothing (`null`), as does none.
//! - **`false_completion`**: the task's last judgment with a `work_state`
//!   answer is where it was called complete (its acting decision stopped):
//!   its resolved `work_state` label says whether it was not (a continuation
//!   re-ask, a near-identical task, the audit, or the operator). `null` when
//!   its decision continued, or no label says.
//! - **`success`**: `false` when the execution ended `failed` or
//!   `budget_exhausted`, or the last judgment's resolved `work_state` says
//!   not complete (the system's `false_completion` rule, an operator's
//!   `wrong` on a `complete` lean or a class other than `complete`, an
//!   audit's); otherwise `true` once the 24 hours after the task's end
//!   (`system::FALSE_COMPLETION_MS`) have been read by a learning run (its
//!   `learning.last_run`), since that run writes the near-identical rule's
//!   label; `null` until then. No audit is required: where none ran, nothing
//!   says not done, and an audit's label counts like any other. An
//!   operator's `complete` (weight 1.0) outweighs the system's rule (0.5).
//! - **`spend_micros`**: the execution's own spend as its budget counts it
//!   (`budget.spent_micros`: a task opens no tasks, so it is all its own),
//!   plus the judge calls paid outside it (`budget: shadow`, the judge's own
//!   day budget, which today pays every judgment); **`judge_micros`**: the
//!   `cost_micros` of every `judge.call` whose context names its session,
//!   every pack's.
//! - **`turns`**: its session's `turn.ended` rows.
//! - **`nudges`**, **`unnecessary_nudges`**: 0. The canary's nudge is 26b's,
//!   and nothing records one yet.
//!
//! A cancelled task (a cancel, a `/stop`) is left out (`cancelled`): it was
//! stopped from outside, so it has no outcome of its own.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Context as _;
use serde_json::Value;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::prove::{ArmName, Stop, StopDecision, TaskRecord};
use theseus_judge::Outcome;
use theseus_kernel::ExecState;
use theseus_protocol::LedgerKind;

use super::labels::{resolve, Truth};
use super::system::FALSE_COMPLETION_MS;
use super::{LabelRow, Scope, Seen, LAST_RUN};
use crate::judge::LOOP_PACK;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// One finished task, as its `task.ended` row and its execution say.
#[derive(Debug, Clone, PartialEq)]
pub struct Ended {
    /// The execution's id (`task.ended`'s `task`).
    pub task: String,
    pub session: String,
    pub state: ExecState,
    /// When its `task.ended` row was written.
    pub ended_at_ms: u64,
    /// `budget.spent_micros`.
    pub spent_micros: u64,
}

/// One `judge.call`'s cost, under the session its context names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JudgeSpend {
    pub micros: u64,
    /// Paid by the judge's own budget (`shadow`), not the execution's.
    pub outside: bool,
}

/// What [`build`] reads: the rows, already read.
#[derive(Debug, Default)]
pub struct Input {
    pub tasks: Vec<Ended>,
    /// `judge:loop`'s scope: its judgments and their labels.
    pub loop_scope: Scope,
    /// The names of loop.v1's learned versions (its lineage), which judge
    /// in its place where the ladder placed them.
    pub learned: Vec<String>,
    /// Every `judge.call`'s cost, by the session its context names.
    pub judge_spend: HashMap<String, Vec<JudgeSpend>>,
    /// `turn.ended` rows, by session.
    pub turns: HashMap<String, u32>,
    /// The last learning run's time: the system labels are written up to it.
    pub settled_ms: Option<u64>,
}

/// The records, and the tasks left out, counted by reason.
#[derive(Debug, Default, PartialEq)]
pub struct Built {
    pub records: Vec<TaskRecord>,
    pub left_out: BTreeMap<String, u32>,
}

/// Why a task is left out.
pub const CANCELLED: &str = "cancelled";
pub const NEVER_JUDGED: &str = "never_judged";
pub const LEARNED_VERSION: &str = "learned_version";
pub const NO_ARM: &str = "no_arm";
pub const BOTH_ARMS: &str = "both_arms";
pub const UNREADABLE: &str = "unreadable";

/// The records, one a finished task, oldest end first.
pub fn build(input: &Input) -> Built {
    let mut out = Built::default();
    // `loop.v1`'s judgments by the session their context names, oldest
    // first.
    let mut by_session: HashMap<&str, Vec<&Seen>> = HashMap::new();
    // The sessions a learned version of loop.v1 judged.
    let mut learned: HashSet<&str> = HashSet::new();
    for seen in input.loop_scope.judgments.values().flatten() {
        let Some(s) = seen.context("session") else {
            continue;
        };
        if seen.judgment.pack == LOOP_PACK {
            by_session.entry(s).or_default().push(seen);
        } else if input.learned.contains(&seen.judgment.pack) {
            learned.insert(s);
        }
    }
    for v in by_session.values_mut() {
        v.sort_by_key(|s| (s.at_ms, s.position));
    }
    let mut tasks: Vec<&Ended> = input.tasks.iter().collect();
    tasks.sort_by(|a, b| (a.ended_at_ms, &a.task).cmp(&(b.ended_at_ms, &b.task)));
    for t in tasks {
        let judged = by_session
            .get(t.session.as_str())
            .map_or(&[][..], Vec::as_slice);
        match arm_of_task(t, judged, learned.contains(t.session.as_str())) {
            Ok(arm) => out.records.push(record(input, t, judged, arm)),
            Err(why) => count(&mut out, why),
        }
    }
    out
}

fn arm_of(s: &Seen) -> Option<ArmName> {
    match s.context("pack_arm") {
        Some("canary") => Some(ArmName::Canary),
        Some("control") => Some(ArmName::Control),
        _ => None,
    }
}

/// The task's arm, or why it is left out. `learned`: a learned version of
/// loop.v1 judged its session.
fn arm_of_task(t: &Ended, judged: &[&Seen], learned: bool) -> Result<ArmName, &'static str> {
    if t.state == ExecState::Cancelled {
        return Err(CANCELLED);
    }
    if learned {
        return Err(LEARNED_VERSION);
    }
    if judged.is_empty() {
        return Err(NEVER_JUDGED);
    }
    let arms: Vec<ArmName> = judged.iter().filter_map(|s| arm_of(s)).collect();
    match arms.first() {
        None => Err(NO_ARM),
        Some(a) if arms.iter().any(|b| b != a) => Err(BOTH_ARMS),
        Some(a) => Ok(*a),
    }
}

/// One task's record, in its arm.
fn record(input: &Input, t: &Ended, judged: &[&Seen], arm: ArmName) -> TaskRecord {
    let none: Vec<LabelRow> = Vec::new();
    let mine: Vec<&Seen> = judged
        .iter()
        .copied()
        .filter(|s| arm_of(s) == Some(arm))
        .collect();
    let labels_of = |s: &Seen| input.loop_scope.labels.get(&s.judgment.id).unwrap_or(&none);
    let stops: Vec<Stop> = mine
        .iter()
        .map(|s| Stop {
            decision: decision(s, arm),
            should_stop: work_state(s)
                .and_then(|a| resolve(labels_of(s), "work_state", a))
                .and_then(|(_, t)| should_stop(&t)),
        })
        .collect();
    // Where it was called complete: its last judgment with an answer.
    let last = mine
        .iter()
        .rev()
        .find_map(|s| work_state(s).map(|a| (s, a)));
    let done = last
        .and_then(|(s, a)| resolve(labels_of(s), "work_state", a).and_then(|(_, t)| done_of(&t)));
    let called_complete = last.is_some_and(|(s, _)| decision(s, arm) == StopDecision::Stop);
    let false_completion = if called_complete {
        done.map(|d| !d)
    } else {
        None
    };
    let failed = matches!(t.state, ExecState::Failed | ExecState::BudgetExhausted);
    let closed = input
        .settled_ms
        .is_some_and(|s| s >= t.ended_at_ms.saturating_add(FALSE_COMPLETION_MS));
    let success = if failed || done == Some(false) {
        Some(false)
    } else if closed {
        Some(true)
    } else {
        None
    };
    let spend = input
        .judge_spend
        .get(&t.session)
        .map_or(&[][..], Vec::as_slice);
    let judge_micros: u64 = spend.iter().map(|j| j.micros).sum();
    let outside: u64 = spend.iter().filter(|j| j.outside).map(|j| j.micros).sum();
    TaskRecord {
        task: t.task.clone(),
        arm,
        success,
        // A judgment paid inside the execution is in its spend already; the
        // max keeps the generator's rule (the judge's within the whole)
        // should a row say otherwise.
        spend_micros: t.spent_micros.saturating_add(outside).max(judge_micros),
        judge_micros,
        turns: input.turns.get(&t.session).copied().unwrap_or(0),
        nudges: 0,
        unnecessary_nudges: 0,
        false_completion,
        stops,
    }
}

fn count(out: &mut Built, why: &str) {
    *out.left_out.entry(why.to_string()).or_insert(0) += 1;
}

/// A judgment's `work_state` answer, when Jev answered it.
fn work_state(s: &Seen) -> Option<&AnswerRecord> {
    (s.judgment.outcome == Outcome::Answered)
        .then(|| {
            s.judgment
                .answers
                .iter()
                .find(|a| a.question == "work_state")
        })
        .flatten()
}

/// The decision that acted in `arm` at this judgment (§2.8a).
fn decision(s: &Seen, arm: ArmName) -> StopDecision {
    let baseline = match s.context("decision") {
        Some("continue") => StopDecision::Continue,
        _ => StopDecision::Stop,
    };
    if arm == ArmName::Control || s.judgment.outcome != Outcome::Answered {
        return baseline;
    }
    let a = |q: &str| s.judgment.answers.iter().find(|a| a.question == q);
    let progressing = a("work_state").is_some_and(|a| a.band.acts_on("progressing"));
    let unfinished = a("announced_unfinished").is_some_and(|a| a.band.acts_true());
    if progressing || unfinished {
        StopDecision::Continue
    } else {
        baseline
    }
}

/// Whether the work was complete, as a resolved `work_state` label says.
fn done_of(t: &Truth) -> Option<bool> {
    match t {
        Truth::Class(c) => Some(c == "complete"),
        Truth::NotClass(c) if c == "complete" => Some(false),
        _ => None,
    }
}

fn should_stop(t: &Truth) -> Option<bool> {
    done_of(t)
}

/// The records as the generator's input: JSON lines, one a record.
pub fn jsonl(records: &[TaskRecord]) -> String {
    let mut s = String::new();
    for r in records {
        s.push_str(&serde_json::to_string(r).unwrap_or_default());
        s.push('\n');
    }
    s
}

/// `classify.v1`, whose decision the prove compares with its baseline's.
pub const CLASSIFY_PACK: &str = "classify.v1";

/// Classification's decision quality (§2.9): `classify.v1`'s lean on
/// `should_promote` against the baseline's decision, the model's own
/// `task.create` in that turn (the system's `task_create` label, read by its
/// key, never derived again), over the judgments in `[since, until]` that an
/// operator or an audit labeled. The truth is their labels alone, resolved
/// heaviest first: the system's label is the baseline itself, so it cannot
/// also be the answer. A labeled judgment whose turn the system has not
/// labeled yet is counted, not compared. Below `min` compared, the shares
/// are not stated.
pub fn classify_quality(
    scope: &Scope,
    since_ms: Option<u64>,
    until_ms: Option<u64>,
    min: usize,
) -> theseus_protocol::judge_runs::ClassifyQuality {
    const Q: &str = "should_promote";
    let none: Vec<LabelRow> = Vec::new();
    let (mut labeled, mut compared, mut jev, mut base) = (0u32, 0u32, 0u32, 0u32);
    // The discordant pairs: Jev right and the baseline wrong, and the
    // reverse.
    let (mut only_jev, mut only_base) = (0u32, 0u32);
    let judgments = scope
        .judgments
        .get(CLASSIFY_PACK)
        .map_or(&[][..], Vec::as_slice);
    for s in judgments {
        if since_ms.is_some_and(|t| s.at_ms < t) || until_ms.is_some_and(|t| s.at_ms > t) {
            continue;
        }
        let Some(answer) = (s.judgment.outcome == Outcome::Answered)
            .then(|| s.judgment.answers.iter().find(|a| a.question == Q))
            .flatten()
        else {
            continue;
        };
        let labels = scope.labels.get(&s.judgment.id).unwrap_or(&none);
        let theirs: Vec<LabelRow> = labels
            .iter()
            .filter(|l| l.source != "system")
            .cloned()
            .collect();
        let Some((_, Truth::Bool(truth))) = resolve(&theirs, Q, answer) else {
            continue;
        };
        labeled += 1;
        let key = super::labels::system_key(&s.judgment.id, Q, "task_create");
        let Some(called) = labels
            .iter()
            .find(|l| l.id == key)
            .and_then(|l| l.label.as_bool())
        else {
            continue;
        };
        compared += 1;
        let lean = matches!(answer.band.top, theseus_judge::band::Top::Noul(true));
        jev += u32::from(lean == truth);
        base += u32::from(called == truth);
        only_jev += u32::from(lean == truth && called != truth);
        only_base += u32::from(called == truth && lean != truth);
    }
    let enough = compared > 0 && compared as usize >= min;
    let rate = |k: u32| enough.then(|| f64::from(k) / f64::from(compared));
    // McNemar's test on the discordant pairs, at 95%: a difference only
    // where the pairs that disagree lean one way past chance.
    let (b, c) = (f64::from(only_jev), f64::from(only_base));
    let z = if b + c > 0.0 {
        (b - c) / (b + c).sqrt()
    } else {
        0.0
    };
    let verdict = match enough {
        false => "insufficient",
        true if z > 1.96 => "jev_better",
        true if z < -1.96 => "baseline_better",
        true => "no_difference",
    };
    theseus_protocol::judge_runs::ClassifyQuality {
        pack: CLASSIFY_PACK.into(),
        question: Q.into(),
        labeled,
        compared,
        jev_right: jev,
        baseline_right: base,
        jev_rate: rate(jev),
        baseline_rate: rate(base),
        verdict: verdict.into(),
        insufficient: (!enough).then(|| format!("compared {compared} of {min}")),
    }
}

/// A page walk's size.
const PAGE: usize = 200;

impl Core {
    /// [`build`]'s input, read from the store: the `task.ended` rows whose
    /// time is in `[since_ms, until_ms]`, each one's execution, `judge:loop`'s
    /// scope, and each task session's `judge.call` and `turn.ended` rows,
    /// through the index's pages by kind and session. Blocking: call it on
    /// the blocking pool. `Err` while the index's shape is built after a
    /// start. Also returns the unreadable tasks' count.
    pub fn prove_input(
        &self,
        since_ms: Option<u64>,
        until_ms: Option<u64>,
    ) -> anyhow::Result<(Input, u32)> {
        let mut input = Input::default();
        let mut unreadable = 0;
        let mut seen = std::collections::HashSet::new();
        for row in self.ledger_rows(LedgerKind::TaskEnded, None, since_ms, until_ms)? {
            let Some(id) = row.data["task"].as_str() else {
                continue;
            };
            if !seen.insert(id.to_string()) {
                continue;
            }
            match self.kernel.execution(id) {
                Ok(Some(e)) => input.tasks.push(Ended {
                    task: e.id,
                    session: e.session_id,
                    state: e.state,
                    ended_at_ms: row.at_unix_ms,
                    spent_micros: e.budget.spent_micros,
                }),
                _ => unreadable += 1,
            }
        }
        input.loop_scope = super::read_scope(&self.store, "loop")?;
        input.learned = self
            .runner
            .judge
            .lineage()
            .names_of_root(&self.store, LOOP_PACK);
        for t in &input.tasks {
            let mut spend = Vec::new();
            for row in self.ledger_rows(LedgerKind::JudgeCall, Some(&t.session), None, None)? {
                let d = &row.data;
                if d["context"]["session"].as_str() != Some(t.session.as_str()) {
                    continue;
                }
                spend.push(JudgeSpend {
                    micros: d["cost_micros"].as_u64().unwrap_or(0),
                    outside: d["budget"].as_str() == Some("shadow"),
                });
            }
            input.judge_spend.insert(t.session.clone(), spend);
            let turns = self
                .ledger_rows(LedgerKind::TurnEnded, Some(&t.session), None, None)?
                .len();
            input
                .turns
                .insert(t.session.clone(), u32::try_from(turns).unwrap_or(u32::MAX));
        }
        input.settled_ms = self
            .store
            .get_meta::<Value>(LAST_RUN)?
            .and_then(|v| v["at_unix_ms"].as_u64());
        Ok((input, unreadable))
    }

    /// Every ledger row of `kind` (in `session`, when given) whose kind's
    /// time is in the window, oldest first, a page at a time.
    fn ledger_rows(
        &self,
        kind: LedgerKind,
        session: Option<&str>,
        since_ms: Option<u64>,
        until_ms: Option<u64>,
    ) -> anyhow::Result<Vec<LedgerRow>> {
        let tags = crate::rpc::ledger_tags(Some(kind.as_str()), session);
        let mut after = 0;
        let mut out = Vec::new();
        loop {
            let page = theseus_store::Page {
                kind: theseus_store::kinds::LEDGER,
                tags: tags.clone(),
                after: Some(after),
                before: None,
                since_ms,
                until_ms,
                limit: PAGE,
            };
            let got = self.store.ledger_page(&page)?.context(
                "the ledger's index is still being built after the start; try again in a minute",
            )?;
            out.extend(
                got.records
                    .iter()
                    .filter_map(|r| r.decode::<LedgerRow>().ok())
                    .filter(|r| r.kind == kind.as_str()),
            );
            match got.last {
                Some(last) if got.more => after = last,
                _ => return Ok(out),
            }
        }
    }
}

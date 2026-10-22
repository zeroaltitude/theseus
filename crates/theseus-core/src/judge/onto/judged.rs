//! The sweep asks Jev about its own categories (theseus-nzzz4), after the
//! deterministic plan and the kinds the `other` answers mint. Two questions,
//! each a pack of its own and off every turn's path:
//!
//! - **`same_thing.v1`**: two categories of one kind under one parent that
//!   look alike (`Ontology::merge_pairs`: both a machine's, nothing below them,
//!   no guidance, a shared word of four letters or more) are asked, at most
//!   [`PAIRS_MAX`] a run. Jev's yes at the act band (0.85) merges them as the
//!   sweep's write by the one survivor rule, which the deterministic merge
//!   uses (`Ontology::plan_pair_merge`).
//! - **`two_things.v1`**: a category a machine wrote, of a kind that nests,
//!   that [`SPLIT_AT`] sessions hold (`Ontology::split_candidates`), at most
//!   [`SPLITS_MAX`] a run, largest first. With a sample of [`SAMPLE`] titled
//!   sessions or fewer, at least [`SAMPLE_MIN`], Jev says whether they fall
//!   into two things, names each over the names mined from the titles, and
//!   says of each sampled session whether it is about the first. All at the
//!   act band and two names that differ, the sweep writes two children and
//!   moves the sampled sessions Jev placed (yes to the first, no to the
//!   second); a session answered below the act band, and every session not
//!   sampled, stays in the parent, where Jev's next filing offers the
//!   children as candidates. One write a split.
//!
//! Three more questions are `renamed.rs`'s (theseus-63tfk.1): a stale
//! category's name (`name_it.v1`), whether two names the `other` answers
//! mined are one kind (`same_kind.v1`), and the name of a kind about to be
//! minted (`name_it.v1`). They run from the same `Run`, so the money, the mark
//! and the not-again rule below hold for all five; the last two run before the
//! kind is minted (`Core::onto_sweep_night`).
//!
//! **Money.** `[ontology] sweep_usd_per_day` (1.0; 0 turns this part off and
//! the deterministic sweep still runs) caps the local day: a call whose worst
//! case would pass what is left is not begun, and the day's spend is the META
//! record [`MARK`], read back at a restart and written after each paid
//! question (theseus-23wd8), so a stop mid-run keeps what it spent and asked. Each call also passes the judge's
//! own day budget (`JudgeService::reserve`), as `people.v1`'s does; no judge
//! budget is raised.
//!
//! **Not asked again.** A pair or a category asked is not asked again for
//! [`AGAIN_MS`] (30 days), whatever the answer, so a "no" costs once a
//! month; the record is the same mark.
//!
//! A sampled session is one in a private place that a person wrote, never an
//! import's; its title is scrubbed as every state's strings are.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_judge::builders::{
    SameThingInput, SweepCategory, TwoThingsInput, FIRST_ASKED, SWEEP_SAMPLE as SAMPLE,
    SWEEP_TITLES as TITLES,
};
use theseus_judge::price::{micros_to_usd, usd_to_micros, Micros};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Judgment, Mode, Outcome, Urgency};
use theseus_ontology::judged::SPLIT_AT;
use theseus_ontology::sweep::fold;
use theseus_ontology::{CategoryId, Ontology, Origin, Step};

use super::sweep::Swept;
use crate::config::PackMode;
use crate::judge::{spend, JudgeService, ScrubWith};
use crate::places::PlaceClass;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The pack that asks whether two categories are one.
pub const SAME_PACK: &str = "same_thing.v1";
/// The pack that asks whether a category holds two things.
pub const SPLIT_PACK: &str = "two_things.v1";
/// The pack that asks whether two groups of `other` answers are one kind
/// (`renamed.rs`).
pub const KIND_PACK: &str = "same_kind.v1";
/// The pack that asks which mined name names a category or a kind.
pub const NAME_PACK: &str = "name_it.v1";
/// The META record of the day's spend and what was asked.
pub const MARK: &str = "onto.sweep.judged";
/// The pairs one run asks about at most.
pub const PAIRS_MAX: usize = 20;
/// The categories one run asks to split at most.
pub const SPLITS_MAX: usize = 3;
/// A split is asked about with this many titled sessions sampled at least.
pub const SAMPLE_MIN: usize = SPLIT_AT as usize / 2;
/// How long a question, once asked, is not asked again: 30 days.
pub const AGAIN_MS: u64 = 30 * 24 * 3_600_000;

/// The day's spend and what was asked when, in one META record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    /// The local day `spent_usd` belongs to.
    pub day: String,
    pub spent_usd: f64,
    /// `pair <a> <b>`, `split <category>`, `rename <category>`, `kind <a> <b>` or
    /// `kind-name <cluster>`, and when it was asked.
    #[serde(default)]
    pub asked: BTreeMap<String, u64>,
}

/// What one call came to.
pub(super) enum Asked {
    /// Answered, with what it cost.
    Answered(Box<Judgment>, Micros),
    /// Its worst case would pass what the day's cap leaves.
    Over(Micros),
    /// Not asked: the pack is off, the client was not built, or the judge's
    /// own day budget refused.
    Off,
}

impl JudgeService {
    /// One question of the sweep: `input` to `pack`, in one decision point,
    /// reserved on the judge's day budget and settled as `judge_people` does.
    /// `left` is what the sweep's own cap leaves.
    pub(super) async fn judge_sweep(
        &self,
        pack: &str,
        input: Input,
        mut context: Value,
        left: Micros,
    ) -> Asked {
        let Some(pack) = theseus_judge::pack::by_name(pack) else {
            return Asked::Off;
        };
        if self.mode_for(&pack.name(), "").mode == PackMode::Off {
            return Asked::Off;
        }
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let built = match self.built() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built; the sweep asked nothing");
                return Asked::Off;
            }
        };
        let scrub = ScrubWith(self.scrubber.clone());
        let Ok(state) = theseus_judge::prepare(&pack, &input, &scrub) else {
            return Asked::Off;
        };
        let blob = match theseus_store::blocking(|| {
            self.store.blobs().put(state.state.json.as_bytes())
        }) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "judge: the state's blob was not written; not judged");
                return Asked::Off;
            }
        };
        context["session"] = json!("");
        context["purpose"] = json!("onto sweep");
        context["baseline"] = json!("none");
        context["decision"] = json!("none");
        context["blob"] = json!(blob);
        context["on_path_ms"] = json!(0);
        let mode = self.ask_mode(&pack.name(), &mut context);
        let mut ask = Ask::new(pack, &state, mode, context);
        ask.id = Some(format!("jdg_{}", uuid::Uuid::now_v7().simple()));
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        if need > left {
            return Asked::Over(need);
        }
        if !self.reserve(&today, need) {
            return Asked::Off;
        }
        let judgments = built
            .judge
            .judge(DecisionPoint {
                asks: vec![ask],
                urgency: Urgency::Shadow,
            })
            .await;
        let Some(j) = judgments.into_iter().next() else {
            self.budget.settle(&today, need, 0, false, false);
            return Asked::Off;
        };
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        self.budget.settle(&today, need, spent, called, failed);
        Asked::Answered(Box::new(j), spent)
    }
}

/// Whether `j` is an answer the sweep may act on: the model the pack pins
/// answered, and the pack acts.
pub(super) fn acts(j: &Judgment) -> bool {
    j.actionable() && j.mode != Mode::Shadow
}

/// `id` as the sweep shows it, with the titles of `held`'s sessions.
pub(super) fn sweep_category(o: &Ontology, id: &CategoryId, held: &Held) -> Option<SweepCategory> {
    let c = o.category(id)?;
    Some(SweepCategory {
        name: c.name.clone(),
        path: o
            .path(id)
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(" › "),
        description: c.description.clone(),
        titles: held
            .sessions
            .iter()
            .take(TITLES)
            .map(|(_, t)| t.clone())
            .collect(),
    })
}

/// Whether a Noul question's answer is yes (or no) at the act band.
pub(super) fn noul(j: &Judgment, question: &str, yes: bool) -> bool {
    j.answer(question).is_some_and(|a| match yes {
        true => a.band.acts_true(),
        false => a.band.acts_false(),
    })
}

/// The mined name a Choice answered at the act band, by its option `n<k>`.
pub(super) fn named(j: &Judgment, question: &str, names: &[String]) -> Option<String> {
    let a = j.answer(question)?;
    names.iter().enumerate().find_map(|(i, n)| {
        let option = format!("n{}", i + 1);
        a.band.acts_on(&option).then(|| n.clone())
    })
}

/// One category's sessions for the sweep to show: titled, a person's, in a
/// private place, newest first.
pub(super) struct Held {
    pub(super) sessions: Vec<(String, String)>,
}

impl Core {
    /// A session's last activity, id and title when it is one the sweep may
    /// show: titled, a person's, in a private place.
    pub(super) fn titled(&self, session: &str) -> Option<(u64, String, String)> {
        let s = self.store.get_session::<SessionRecord>(session).ok()??;
        let title = s.title.clone().unwrap_or_default();
        let shown = !title.trim().is_empty()
            && s.task.is_none()
            && s.imported.is_none()
            && self.runner.class_of(&s.session_id) == PlaceClass::Private;
        shown.then_some((s.last_active_ms, s.session_id, title))
    }

    /// A store scan, off the runtime's worker (`theseus_store::blocking`).
    pub(super) fn held_by(&self, o: &Ontology, id: &CategoryId) -> Held {
        let mut found: Vec<(u64, String, String)> = Vec::new();
        theseus_store::blocking(|| {
            for l in o.lists().filter(|l| l.kind == id.kind()) {
                if !l.members.iter().any(|m| &m.category == id) {
                    continue;
                }
                if let Some(found_one) = self.titled(&l.session) {
                    found.push(found_one);
                }
            }
        });
        found.sort_by(|a, b| b.cmp(a));
        Held {
            sessions: found.into_iter().map(|(_, id, t)| (id, t)).collect(),
        }
    }

    /// The model's part of one run at `now_ms`: pairs, then splits, under
    /// `[ontology] sweep_usd_per_day`. Nothing with the cap at 0.
    pub async fn onto_sweep_judged(&self, now_ms: u64, out: &mut Swept) -> Result<()> {
        let Some(mut run) = self.judged_run(now_ms)? else {
            return Ok(());
        };
        let o = self.runner.ontology.snapshot(&self.store)?;
        let merges = run.pairs(&o).await;
        let splits = run.splits(&o).await;
        let renames = run.renames(&o).await;
        // Kept after each paid question too (`Run::paid`), so a stop mid-run
        // keeps what was spent and asked; this keeps a run that asked nothing.
        run.keep()?;
        out.spent += run.spent;
        // The writes, after every question: each is checked against the
        // ontology as it stands, so one write never builds on another's.
        self.onto_judged_writes(now_ms, merges, splits, renames, out);
        Ok(())
    }

    /// A run of the sweep's questions at `now_ms`: the day's mark and what the
    /// cap leaves. None with the cap at 0. Each run reads the mark again, so
    /// two in a night share one day's money.
    pub(super) fn judged_run(&self, now_ms: u64) -> Result<Option<Run<'_>>> {
        let cap_usd = self.runner.cfg.ontology.sweep_usd_per_day;
        if cap_usd <= 0.0 {
            return Ok(None);
        }
        let today = spend::local_day(now_ms);
        let mut mark: Mark =
            theseus_store::blocking(|| self.store.get_meta(MARK))?.unwrap_or_default();
        if mark.day != today {
            mark.day = today;
            mark.spent_usd = 0.0;
        }
        mark.asked
            .retain(|_, at| now_ms.saturating_sub(*at) < AGAIN_MS);
        let before = mark.spent_usd;
        let cap = usd_to_micros((cap_usd - before).max(0.0));
        Ok(Some(Run {
            core: self,
            now_ms,
            cap,
            spent: 0,
            before,
            mark,
        }))
    }

    /// The merges and splits the answers came to, each written as the sweep's.
    fn onto_judged_writes(
        &self,
        now_ms: u64,
        merges: Vec<(CategoryId, CategoryId)>,
        splits: Vec<Splitting>,
        renames: Vec<(CategoryId, String)>,
        out: &mut Swept,
    ) {
        let Ok(o) = self.runner.ontology.snapshot(&self.store) else {
            return;
        };
        let mut working: Ontology = (*o).clone();
        let base = out.writes.len() + out.failed as usize;
        // Each step's write id, fixed as it is planned: `sweep-<ms>-<n>-<word>`.
        let mut steps: Vec<(String, Step)> = Vec::new();
        let id = |steps: &[(String, Step)], word: &str| {
            format!("sweep-{now_ms}-{}-{word}", base + steps.len())
        };
        for (a, b) in merges {
            let write = id(&steps, "merge");
            if let Some(step) = working.plan_pair_merge(&a, &b) {
                steps.push((write, step));
            }
        }
        for s in splits {
            let write = id(&steps, "split");
            let moved = [s.first.as_slice(), s.second.as_slice()];
            let names = [s.names[0].as_str(), s.names[1].as_str()];
            if let Some(step) = working.plan_split(&s.parent, names, moved, &write) {
                steps.push((write, step));
            }
        }
        for (category, name) in renames {
            let write = id(&steps, "rename");
            if let Some(step) = working.plan_rename(&category, &name, &write) {
                steps.push((write, step));
            }
        }
        for (write, step) in steps {
            let records = step.records().to_vec();
            let done = theseus_store::blocking(|| {
                self.ontology_machine_write(Origin::Sweep, &write, records)
            });
            match (done, &step) {
                (Ok(()), Step::Split { .. }) => {
                    out.splits += 1;
                    out.writes.push(write);
                }
                (Ok(()), Step::Rename { .. }) => {
                    out.renamed += 1;
                    out.writes.push(write);
                }
                (Ok(()), _) => {
                    out.judged_merges += 1;
                    out.writes.push(write);
                }
                (Err(e), _) => {
                    out.failed += 1;
                    tracing::warn!(write, error = %format!("{e:#}"), "ontology: a judged sweep step was not written");
                }
            }
        }
    }
}

/// A split Jev answered for: the parent, the two names and the sessions it
/// placed in each.
struct Splitting {
    parent: CategoryId,
    names: [String; 2],
    first: Vec<String>,
    second: Vec<String>,
}

/// One run's state: the cap, what it has spent, what was asked.
pub(super) struct Run<'a> {
    pub(super) core: &'a Core,
    now_ms: u64,
    cap: Micros,
    pub(super) spent: Micros,
    /// What the day had spent before the run.
    before: f64,
    pub(super) mark: Mark,
}

impl Run<'_> {
    /// The day's mark as the run stands, written where a restart reads it.
    pub(super) fn keep(&mut self) -> Result<()> {
        self.mark.spent_usd = self.before + micros_to_usd(self.spent);
        theseus_store::blocking(|| self.core.store.put_meta(MARK, &self.mark))
    }

    /// A paid question's answer: its cost and its key go into the mark, and
    /// the mark is written at once (theseus-23wd8), so a stop before the run
    /// ends loses neither.
    pub(super) fn paid(&mut self, key: String, cost: Micros) {
        self.spent += cost;
        self.mark.asked.insert(key, self.now_ms);
        if let Err(e) = self.keep() {
            tracing::warn!(error = %format!("{e:#}"), "ontology: the sweep's day mark was not written");
        }
    }

    pub(super) fn left(&self) -> Micros {
        self.cap.saturating_sub(self.spent)
    }

    /// Whether the run goes on to another question.
    pub(super) fn going(&self) -> bool {
        !self.core.outbox.stopping()
    }

    /// The pairs Jev called one thing at the act band.
    async fn pairs(&mut self, o: &Ontology) -> Vec<(CategoryId, CategoryId)> {
        let mut yes = Vec::new();
        let mut asked = 0;
        for (a, b) in o.merge_pairs() {
            if asked >= PAIRS_MAX || !self.going() {
                break;
            }
            let key = format!("pair {a} {b}");
            if self.mark.asked.contains_key(&key) {
                continue;
            }
            let (Some(first), Some(second)) = (
                sweep_category(o, &a, &self.core.held_by(o, &a)),
                sweep_category(o, &b, &self.core.held_by(o, &b)),
            ) else {
                continue;
            };
            let input = SameThingInput {
                kind: a.kind().to_string(),
                first,
                second,
            };
            if asked > 0 {
                theseus_store::pressure::quiet(theseus_store::pressure::BOUND).await;
            }
            let context = json!({"a": a, "b": b});
            let judge = &self.core.runner.judge;
            match judge
                .judge_sweep(SAME_PACK, Input::SameThing(input), context, self.left())
                .await
            {
                Asked::Answered(j, cost) => {
                    asked += 1;
                    self.paid(key, cost);
                    if acts(&j) && noul(&j, "same_thing", true) {
                        yes.push((a, b));
                    }
                }
                Asked::Over(need) => {
                    tracing::info!(
                        need,
                        "ontology: the sweep's day cap would pass; no more pairs"
                    );
                    break;
                }
                Asked::Off => break,
            }
        }
        yes
    }

    /// The splits Jev answered for at the act band.
    async fn splits(&mut self, o: &Ontology) -> Vec<Splitting> {
        let mut out = Vec::new();
        let mut asked = 0;
        for parent in o.split_candidates() {
            if asked >= SPLITS_MAX || !self.going() {
                break;
            }
            let key = format!("split {parent}");
            if self.mark.asked.contains_key(&key) {
                continue;
            }
            // One scan of the category's sessions: the sample is its newest.
            let held = self.core.held_by(o, &parent);
            let Some(category) = sweep_category(o, &parent, &held) else {
                continue;
            };
            let mut sample = held.sessions;
            sample.truncate(SAMPLE);
            if sample.len() < SAMPLE_MIN {
                continue;
            }
            let input = TwoThingsInput {
                kind: parent.kind().to_string(),
                category: SweepCategory {
                    titles: Vec::new(),
                    ..category
                },
                sessions: sample,
            };
            let names = input.names();
            let sampled = input.sessions.clone();
            if asked > 0 {
                theseus_store::pressure::quiet(theseus_store::pressure::BOUND).await;
            }
            let context = json!({"category": parent});
            let judge = &self.core.runner.judge;
            match judge
                .judge_sweep(SPLIT_PACK, Input::TwoThings(input), context, self.left())
                .await
            {
                Asked::Answered(j, cost) => {
                    asked += 1;
                    self.paid(key, cost);
                    if let Some(s) = splitting(&j, &parent, &names, &sampled) {
                        out.push(s);
                    }
                }
                Asked::Over(need) => {
                    tracing::info!(
                        need,
                        "ontology: the sweep's day cap would pass; no more splits"
                    );
                    break;
                }
                Asked::Off => break,
            }
        }
        out
    }
}

/// What a `two_things.v1` judgment comes to: two different names and the
/// sessions placed, when every answer that decides is at the act band.
fn splitting(
    j: &Judgment,
    parent: &CategoryId,
    names: &[String],
    sampled: &[(String, String)],
) -> Option<Splitting> {
    if !acts(j) || !noul(j, "two_things", true) {
        return None;
    }
    let first = named(j, "first_name", names)?;
    let second = named(j, "second_name", names)?;
    if fold(&first) == fold(&second) {
        return None;
    }
    let placed = |def: &str, id: &str, yes: bool| {
        j.answers.iter().any(|a| {
            a.def == def
                && a.about.as_deref() == Some(id)
                && match yes {
                    true => a.band.acts_true(),
                    false => a.band.acts_false(),
                }
        })
    };
    let side = |yes: bool| -> Vec<String> {
        sampled
            .iter()
            .enumerate()
            .filter(|(i, (id, _))| {
                let def = match *i < FIRST_ASKED {
                    true => "in_first",
                    false => "in_first_more",
                };
                placed(def, id, yes)
            })
            .map(|(_, (id, _))| id.clone())
            .collect()
    };
    Some(Splitting {
        parent: parent.clone(),
        names: [first, second],
        first: side(true),
        second: side(false),
    })
}

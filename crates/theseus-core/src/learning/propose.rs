//! The learning loop (M5 25f; design §2.17): the owner's labels rewrite a
//! pack's text. Nightly after the report (`learning::tender`), or now as
//! the owner's act (`judge.learn`, `theseus judge learn <pack>`), for each
//! lineage the step may rewrite (every wired root but `lineage::LEFT_ALONE`):
//!
//! 1. **The errors.** The version standing in the root's place (its
//!    parent) is read with its labels: an error is an owner's label
//!    (`source: operator`) that says an answer was wrong, on a judgment in
//!    the train split, not read by an earlier proposal of the lineage. The
//!    split is the nightly rule (`propose::SplitRule::nightly`: interleaved
//!    below 200 labeled in the holdout window, then the window), or the
//!    owner's `--split` (train before it, holdout from it until now). Below
//!    `min_errors` new errors, nothing more happens and nothing is written.
//! 2. **The writer.** The profile `[judge.learn] writer_profile` reads up to
//!    `max_errors`, newest first (each state from its blob, Jev's answers
//!    and bands, the label and its note) with the pack file, and returns a
//!    pack file. Priced from the catalog and reserved before it is sent,
//!    inside `writer_limit_usd_per_day` (a proposal past it is skipped and
//!    written so). Only ever a candidate: `propose::candidate_from_reply`
//!    keeps every field but the text Jev reads, and `Pack::parse` loads it.
//! 3. **The check.** One 25d replay asks the candidate on the stored states
//!    of the labeled judgments of both splits; thresholds are re-fit in code
//!    from its train answers (`propose::refit`); both versions' holdout
//!    numbers per question and class, and the train errors it fixed.
//! 4. **The decision** (`propose::decide`), placed through 26a's own act
//!    (`Core::promote_learned`): live, canary, its parent's place in shadow,
//!    or a security pack's card; else held, with why.
//! 5. **Written**: the version's `pack.version` row (and its file), then the
//!    move, then the `judge.proposal` row, and a notice to the owner.
//!
//! One open proposal per lineage: a card not yet answered holds the next.
//! States hold strangers' text: the writer is told so, and its output is
//! only a candidate that the numbers place.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{bail, Context as _};
use serde_json::json;
use theseus_judge::band::{Band, Top};
use theseus_judge::client::Kind;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::learn::{self, Minimum, Window};
use theseus_judge::price::{micros_to_usd, usd_to_micros, Micros};
use theseus_judge::propose::{
    self, decide, ErrorCase, Evidence, Graded, QuestionNumbers, Side, SplitRule, Verdict,
};
use theseus_judge::{Judgment, Outcome, Pack, Thresholds};
use theseus_protocol::judge_runs::{
    JudgeLearnParams, JudgeProposal, JudgeReplayParams, LearnClass, LearnQuestion, LearnThreshold,
};
use theseus_protocol::LedgerKind;

use super::labels::{resolve, truth, Truth};
use super::report::{answer, graded};
use super::{read_scope, LabelRow, Seen};
use crate::fact;
use crate::judge::lineage::{self, Learned};
use crate::ledger::LedgerRow;
use crate::provider::ProviderRequest;
use crate::rpc::Core;

/// This thread's CPU time (`CLOCK_THREAD_CPUTIME_ID`): the nightly loop's
/// own work, which its pace is measured by.
fn thread_cpu() -> std::time::Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes the timespec it is given, nothing else.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// The share a learned canary starts at.
pub const CANARY_SHARE: f64 = 0.2;

/// The writer's output cap: a pack file is a few thousand tokens. The
/// profile's own cap would reserve past the day's limit (Opus 5.5's whole
/// output at its price is more than `writer_limit_usd_per_day`'s $2).
pub const WRITER_MAX_TOKENS: u32 = 8192;

/// The roots the loop rewrites: every wired version but those it leaves
/// alone.
pub fn roots() -> Vec<&'static str> {
    crate::judge::WIRED
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !lineage::LEFT_ALONE.contains(n))
        .collect()
}

/// `--split`: unix ms, an RFC 3339 time, a local day, or a duration ago.
pub fn parse_split(s: &str, now: u64) -> anyhow::Result<u64> {
    let s = s.trim();
    if let Ok(ms) = s.parse::<u64>() {
        return Ok(ms);
    }
    if let Ok(ms) = crate::wake::parse_at(s) {
        return Ok(ms);
    }
    if let Ok(ago) = crate::wake::parse_after(s) {
        return Ok(now.saturating_sub(ago));
    }
    super::backfill::day_start(s).map_err(|_| {
        anyhow::anyhow!(
            "--split is unix ms, an RFC 3339 time (2026-10-04T15:00:00-07:00), a local day \
             (2026-10-04), or a duration ago (2h), not {s:?}"
        )
    })
}

/// An answer's truth in words, for the writer: `kind: control`.
fn truth_words(q: &str, t: &Truth) -> String {
    match t {
        Truth::Bool(b) => format!("{q}: {b}"),
        Truth::Class(c) => format!("{q}: {c}"),
        Truth::NotClass(c) => format!("{q}: not {c}"),
        Truth::Level(n) => format!("{q}: level {n}"),
        Truth::NotLevel(n) => format!("{q}: not level {n}"),
    }
}

fn top_words(a: &AnswerRecord) -> String {
    let band = super::report::band_name(a.band.band);
    match &a.band.top {
        Top::Choice(c) => format!("{}: {c} ({band}, {:.2})", a.def, a.band.value),
        Top::Noul(b) => format!("{}: {b} ({band}, p {:.2})", a.def, a.band.value),
        Top::Level(n) => format!("{}: level {n} ({band}, {:.2})", a.def, a.band.value),
    }
}

/// One error the writer reads, before its state is read.
struct Error<'a> {
    label: &'a LabelRow,
    seen: &'a Seen,
    truth: Truth,
    question: String,
}

/// What a run gathered before the writer: the parent, its labeled
/// judgments on each side, their labels, and the errors as the writer reads
/// them.
struct Gathered {
    parent: Arc<Pack>,
    parent_name: String,
    parent_text: String,
    labels: HashMap<String, Vec<LabelRow>>,
    train: Vec<Seen>,
    holdout: Vec<Seen>,
    cases: Vec<ErrorCase>,
}

/// Whether an answer's lean was right by a label's truth. `report::graded`
/// gives a Noul's truth (its calibration pair), not whether its lean met it:
/// a Noul is right when its lean is the truth.
fn right(a: &AnswerRecord, t: &Truth) -> Option<bool> {
    let (_, x) = graded(a, t)?;
    Some(match a.band.top {
        Top::Noul(lean) => lean == x,
        _ => x,
    })
}

/// The owner's labels saying a train answer was wrong, in `train`'s order,
/// none an earlier proposal read (`used`).
fn train_errors<'a>(
    train: &[&'a Seen],
    labels: &'a HashMap<String, Vec<LabelRow>>,
    used: &BTreeSet<String>,
) -> Vec<Error<'a>> {
    let mut errors = Vec::new();
    for s in train {
        let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
        for a in s.judgment.answers.iter().filter(|a| a.about.is_none()) {
            let Some((l, t)) = resolve(ls, &a.def, a) else {
                continue;
            };
            let wrong = right(a, &t) == Some(false);
            if wrong && l.source == "operator" && !used.contains(&l.id) {
                errors.push(Error {
                    label: l,
                    seen: s,
                    truth: t,
                    question: a.def.clone(),
                });
            }
        }
    }
    errors
}

/// The split a run used, in its proposal.
fn record_split(prop: &mut JudgeProposal, rule: SplitRule) {
    match rule {
        SplitRule::Interleaved => prop.split = "interleaved".into(),
        SplitRule::Time { start_ms, end_ms } => {
            prop.split = "time".into();
            prop.split_start_ms = Some(start_ms);
            prop.split_end_ms = Some(end_ms);
        }
    }
}

/// A side's pairs per question: `(predicted, label)` for precision and
/// recall, and each answer graded for the re-fit.
#[derive(Default)]
struct Pairs {
    classes: BTreeMap<String, Vec<(String, String)>>,
    graded: BTreeMap<String, Vec<Graded>>,
    /// Whether each (judgment, question) answer was right.
    right: BTreeMap<(String, String), bool>,
}

impl Pairs {
    /// The answers wrong here and right in `then`: the errors it fixed.
    fn fixed_by(&self, then: &Pairs) -> u32 {
        self.right
            .iter()
            .filter(|(k, r)| !**r && then.right.get(*k) == Some(&true))
            .count() as u32
    }
}

/// What a label settles of an answer as a class pair: a Choice's top and
/// its class (`not:<c>` when it names only a wrong one), a Noul's lean and
/// its truth.
fn pair(a: &AnswerRecord, t: &Truth) -> Option<(String, String)> {
    let right = right(a, t)?;
    Some(match (&a.band.top, t) {
        (Top::Choice(top), Truth::Class(c)) => (top.clone(), c.clone()),
        (Top::Choice(top), _) => (top.clone(), format!("not:{top}")),
        (Top::Noul(lean), _) => (
            lean.to_string(),
            (if right { *lean } else { !lean }).to_string(),
        ),
        (Top::Level(_), _) => return None,
    })
}

fn strength(a: &AnswerRecord) -> f64 {
    match a.band.top {
        Top::Noul(_) => a.band.value.max(1.0 - a.band.value),
        _ => a.band.value,
    }
}

/// The pairs of `judgments` (each with its absolute labels, by the
/// parent's id) for the whole questions of `pack`.
fn pairs_of<'a>(
    pack: &Pack,
    judgments: impl Iterator<Item = (&'a str, &'a Judgment)>,
    labels: &HashMap<String, Vec<LabelRow>>,
) -> Pairs {
    let mut out = Pairs::default();
    let qs: Vec<String> = propose::whole_questions(pack)
        .map(|q| q.id.clone())
        .collect();
    for (id, j) in judgments {
        let ls = labels.get(id).map_or(&[][..], Vec::as_slice);
        for q in &qs {
            let Some(a) = j.answers.iter().find(|a| a.about.is_none() && &a.def == q) else {
                continue;
            };
            let Some(l) = ls
                .iter()
                .find(|l| l.question.as_deref() == Some(q.as_str()))
            else {
                continue;
            };
            let Some(t) = truth(&l.label, a) else {
                continue;
            };
            if let Some(p) = pair(a, &t) {
                out.classes.entry(q.clone()).or_default().push(p);
            }
            if let Some(right) = right(a, &t) {
                out.graded.entry(q.clone()).or_default().push(Graded {
                    strength: strength(a),
                    right,
                });
                out.right.insert((id.to_string(), q.clone()), right);
            }
        }
    }
    out
}

fn numbers(p: &Pairs) -> Vec<QuestionNumbers> {
    p.classes
        .iter()
        .map(|(q, pairs)| QuestionNumbers::of(q, pairs))
        .collect()
}

fn wire(n: &[QuestionNumbers]) -> Vec<LearnQuestion> {
    n.iter()
        .map(|q| {
            let (precision, recall) = q.macro_pr();
            LearnQuestion {
                question: q.question.clone(),
                labeled: q.labeled,
                classes: q
                    .classes
                    .iter()
                    .map(|(c, (p, r))| LearnClass {
                        class: c.clone(),
                        precision: *p,
                        recall: *r,
                    })
                    .collect(),
                precision,
                recall,
            }
        })
        .collect()
}

/// The candidate's text against its parent's, line by line: `-` the
/// parent's lines it lacks, `+` its own the parent lacks (comments and
/// blank lines aside).
pub fn diff(parent: &str, cand: &str) -> String {
    let lines = |t: &str| -> Vec<String> {
        t.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect()
    };
    let (a, b) = (lines(parent), lines(cand));
    let (sa, sb): (BTreeSet<&String>, BTreeSet<&String>) = (a.iter().collect(), b.iter().collect());
    let mut out: Vec<String> = a
        .iter()
        .filter(|l| !sb.contains(l))
        .map(|l| format!("- {l}"))
        .collect();
    out.extend(
        b.iter()
            .filter(|l| !sa.contains(l))
            .map(|l| format!("+ {l}")),
    );
    out.join("\n")
}

fn words(x: Option<f64>) -> String {
    x.map_or("n/a".into(), |v| format!("{v:.2}"))
}

impl Core {
    /// `judge.learn`: the owner's run now, from a private place
    /// (`judge_act(Act::JudgeRun)`), on a thread of its own at low priority.
    pub async fn judge_learn(
        self: &Arc<Self>,
        p: JudgeLearnParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> anyhow::Result<JudgeProposal> {
        let who = who.into();
        if !self.cfg.judge.enabled {
            bail!("the judge is off ([judge] enabled = false): nothing is learned");
        }
        let root = self.root_named(&p.pack)?;
        let now = theseus_protocol::now_unix_ms();
        let split = p
            .split
            .as_deref()
            .map(|s| parse_split(s, now))
            .transpose()?;
        let what = format!("learning loop for {root}");
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::JUDGE_LEARN,
                what: &what,
            },
        )?;
        let core = Arc::downgrade(self);
        let rt = tokio::runtime::Handle::current();
        let (who_s, via) = (who.who(), who.via());
        let rx = super::tender::on_low_thread(move || match core.upgrade() {
            Some(c) => c.learn_lineage(&rt, &root, split, now, "owner", &who_s, &via),
            None => Err(anyhow::anyhow!("the daemon stopped before the loop ran")),
        })?;
        rx.await.context("the loop's thread ended")?.1
    }

    /// The root a name gives: a version (its lineage's root) or an id (its
    /// one wired root; `security` has two, so it is named by version).
    fn root_named(&self, name: &str) -> anyhow::Result<String> {
        let name = name.trim();
        let root = if name.contains('.') {
            self.runner.judge.root_of(name)
        } else {
            let of: Vec<&str> = roots()
                .into_iter()
                .filter(|n| crate::judge::ladder::id_of(n) == name)
                .collect();
            match of.as_slice() {
                [one] => (*one).to_string(),
                [] => bail!("the learning loop rewrites no pack {name}"),
                _ => bail!(
                    "{name} has more than one wired version: name one ({})",
                    of.join(", ")
                ),
            }
        };
        if !roots().contains(&root.as_str()) {
            bail!(
                "the learning loop rewrites {}; not {root}",
                roots().join(", ")
            );
        }
        Ok(root)
    }

    /// The nightly loop, after the report: each lineage once, `pace` called
    /// after each with the work it took (the tender sleeps 19 times that and
    /// then waits while the machine is busy, as after each of the report's
    /// packs). The work is this thread's CPU time, not the wall clock's: a
    /// lineage's wall time is mostly its writer's and its replay's requests,
    /// which wait on the runtime, and 19 times a few minutes asleep would
    /// hold the core for an hour, past any stop. No lineage starts once a
    /// stop has begun. Errors are logged; the rest go on.
    pub fn learn_nightly(
        &self,
        rt: &tokio::runtime::Handle,
        now: u64,
        mut pace: impl FnMut(std::time::Duration),
    ) -> Vec<JudgeProposal> {
        if !self.cfg.judge.enabled || !self.cfg.judge.learn.enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        for root in roots() {
            if self.outbox.stopping() {
                break;
            }
            let cpu = thread_cpu();
            match self.learn_lineage(rt, root, None, now, "nightly", "system", "learning") {
                Ok(p) => out.push(p),
                Err(e) => {
                    tracing::warn!(root, error = %format!("{e:#}"), "learning: the loop did not run")
                }
            }
            pace(thread_cpu().saturating_sub(cpu));
        }
        out
    }

    /// The lineage's proposals, oldest first.
    fn proposals(&self, root: &str) -> anyhow::Result<Vec<(u64, JudgeProposal)>> {
        let mut out = Vec::new();
        for r in self.store.scope_after(&lineage::scope(root), 0)? {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.kind != LedgerKind::JudgeProposal.as_str() {
                continue;
            }
            if let Ok(p) = serde_json::from_value::<JudgeProposal>(row.data) {
                if p.root == root {
                    out.push((row.at_unix_ms, p));
                }
            }
        }
        Ok(out)
    }

    /// What the writer spent today, across every lineage.
    fn writer_spent_today(&self, now: u64) -> Micros {
        let today = crate::judge::spend::local_day(now);
        roots()
            .iter()
            .flat_map(|r| self.proposals(r).unwrap_or_default())
            .filter(|(at, _)| crate::judge::spend::local_day(*at) == today)
            .map(|(_, p)| usd_to_micros(p.writer_usd))
            .sum()
    }

    /// A version's file text: compiled in, or its row's.
    fn text_of(&self, name: &str) -> Option<String> {
        theseus_judge::pack::EMBEDDED
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| (*t).to_string())
            .or_else(|| {
                self.runner
                    .judge
                    .lineage()
                    .get(&self.store, name)
                    .map(|l| l.text)
            })
    }

    /// One lineage's run. Returns what happened; writes a proposal row
    /// whenever the writer was asked or would have been (a budget's skip).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn learn_lineage(
        &self,
        rt: &tokio::runtime::Handle,
        root: &str,
        split_at: Option<u64>,
        now: u64,
        trigger: &str,
        who: &str,
        via: &str,
    ) -> anyhow::Result<JudgeProposal> {
        let _rt = rt.enter();
        let mut prop = JudgeProposal {
            id: crate::new_id("prp"),
            root: root.to_string(),
            trigger: trigger.to_string(),
            ..JudgeProposal::default()
        };
        let Some(g) = self.gather(root, split_at, now, &mut prop)? else {
            return Ok(prop);
        };
        let Some((cand_text, cand)) = self.write_candidate(rt, &g, &mut prop, now, who, via)?
        else {
            return Ok(prop);
        };
        let (cand_text, cand, pn, cn) = self.check(rt, &g, cand_text, cand, &mut prop, who, via)?;
        self.place(root, &g, cand_text, cand, &pn, &cn, &mut prop, now)?;
        self.write_proposal(&prop, who, via)?;
        if let Err(e) = self
            .outbox
            .to_operator(None, json!({"kind": "notice", "text": prop.said}))
        {
            tracing::warn!(error = %format!("{e:#}"), "learning: the proposal's notice was not posted");
        }
        Ok(prop)
    }

    /// Whether the holdout is short of `min_holdout` labeled judgments: then
    /// nothing moves on no held-out evidence, and nothing is sent; the run
    /// ends as below `min_errors` does, and `prop` says why.
    fn holdout_short(prop: &mut JudgeProposal, holdout: usize, min: u32) -> bool {
        if holdout >= min as usize {
            return false;
        }
        prop.decision = "none".into();
        prop.why =
            format!("{holdout} labeled holdout judgments, under [judge.learn] min_holdout ({min})");
        true
    }

    /// The parent, its labeled judgments split, and the new errors as the
    /// writer reads them; none when the run ends here (`prop` says why).
    fn gather(
        &self,
        root: &str,
        split_at: Option<u64>,
        now: u64,
        prop: &mut JudgeProposal,
    ) -> anyhow::Result<Option<Gathered>> {
        let j = &self.runner.judge;
        let lc = &self.cfg.judge.learn;
        let parent_name = j.placed_read(root, "");
        let parent = j
            .pack(&parent_name)
            .with_context(|| format!("{parent_name} does not load"))?;
        prop.parent = parent_name.clone();
        // One open proposal per lineage: a card not yet answered.
        let earlier = self.proposals(root)?;
        if let Some(why) = self.open_card(&earlier) {
            prop.decision = "skipped".into();
            prop.why = why;
            return Ok(None);
        }
        let used: BTreeSet<String> = earlier
            .iter()
            .flat_map(|(_, p)| p.errors.iter().cloned())
            .collect();
        // The parent's labeled judgments, and the split.
        let mut scope = read_scope(&self.store, &parent.id)?;
        let seen: Vec<Seen> = scope
            .judgments
            .remove(&parent_name)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.judgment.outcome == Outcome::Answered)
            .collect();
        let labeled: Vec<&Seen> = seen
            .iter()
            .filter(|s| {
                let ls = scope
                    .labels
                    .get(&s.judgment.id)
                    .map_or(&[][..], Vec::as_slice);
                s.judgment
                    .answers
                    .iter()
                    .filter(|a| a.about.is_none())
                    .any(|a| resolve(ls, &a.def, a).is_some_and(|(_, t)| graded(a, &t).is_some()))
            })
            .collect();
        let window = Window::latest(super::local_midnight(now), self.cfg.judge.holdout_days);
        let rule = match split_at {
            Some(t) => SplitRule::Time {
                start_ms: t,
                end_ms: now.saturating_add(1),
            },
            None => SplitRule::nightly(
                labeled.iter().filter(|s| window.contains(s.at_ms)).count(),
                window,
            ),
        };
        record_split(prop, rule);
        let side = |s: &Seen| rule.side(&s.judgment.id, s.at_ms);
        let mut train: Vec<&Seen> = labeled
            .iter()
            .copied()
            .filter(|s| side(s) == Side::Train)
            .collect();
        let holdout: Vec<&Seen> = labeled
            .iter()
            .copied()
            .filter(|s| side(s) == Side::Holdout)
            .collect();
        prop.train = train.len() as u32;
        prop.holdout = holdout.len() as u32;
        // The errors: the owner's labels saying a train answer was wrong,
        // newest first, none an earlier proposal read.
        train.sort_by_key(|a| std::cmp::Reverse(a.at_ms));
        let mut errors = train_errors(&train, &scope.labels, &used);
        prop.new_errors = errors.len() as u32;
        if errors.len() < lc.min_errors as usize {
            prop.decision = "none".into();
            prop.why = format!(
                "{} new errors on the train split; it takes {}",
                errors.len(),
                lc.min_errors
            );
            return Ok(None);
        }
        if Self::holdout_short(prop, holdout.len(), lc.min_holdout) {
            return Ok(None);
        }
        errors.truncate(lc.max_errors as usize);
        prop.errors = errors.iter().map(|e| e.label.id.clone()).collect();
        prop.error_judgments = errors.iter().map(|e| e.seen.judgment.id.clone()).collect();
        prop.error_judgments.dedup();
        let cases: Vec<ErrorCase> = errors.iter().map(|e| self.error_case(e)).collect();
        drop(errors);
        Ok(Some(Gathered {
            parent,
            parent_text: self.text_of(&parent_name).unwrap_or_default(),
            parent_name,
            labels: scope.labels,
            train: train.into_iter().cloned().collect(),
            holdout: holdout.into_iter().cloned().collect(),
            cases,
        }))
    }

    /// The writer's candidate: priced inside the day's limit, its reply
    /// through `candidate_from_reply`; none when the run ends here (a skip
    /// or a refusal, written).
    fn write_candidate(
        &self,
        rt: &tokio::runtime::Handle,
        g: &Gathered,
        prop: &mut JudgeProposal,
        now: u64,
        who: &str,
        via: &str,
    ) -> anyhow::Result<Option<(String, Pack)>> {
        let lc = &self.cfg.judge.learn;
        let (parent, parent_name, parent_text, cases) =
            (&g.parent, &g.parent_name, &g.parent_text, &g.cases);
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&lc.writer_profile), None, None)
            .with_context(|| {
                format!(
                    "the writer's profile {:?} does not resolve",
                    lc.writer_profile
                )
            })?;
        prop.writer_model = Some(target.model.clone());
        let request = ProviderRequest {
            model: target.model.clone(),
            max_tokens: target.max_tokens.min(WRITER_MAX_TOKENS),
            system: vec![json!({"type": "text", "text": propose::WRITER_SYSTEM})],
            messages: vec![json!({"role": "user", "content": [{"type": "text",
                "text": propose::writer_message(parent_text, cases)}]})],
            tools: Vec::new(),
            thinking: None,
            output_config: None,
            cache_control: None,
            betas: Vec::new(),
            extra: Default::default(),
            image_tokens: 0,
        };
        let need = self.audit_reserve(&request).with_context(|| {
            format!(
                "{} has no price in the catalog: nothing is sent",
                target.model
            )
        })?;
        let limit = usd_to_micros(lc.writer_limit_usd_per_day);
        let spent = self.writer_spent_today(now);
        if spent.saturating_add(need) > limit {
            prop.decision = "skipped".into();
            prop.why = format!(
                "the writer would reserve {} with {} spent today, past [judge.learn] \
                 writer_limit_usd_per_day ({}); nothing was sent",
                crate::narrative::dollars(need),
                crate::narrative::dollars(spent),
                crate::narrative::dollars(limit)
            );
            // Its errors stay new for the next run.
            prop.errors.clear();
            prop.error_judgments.clear();
            prop.said = format!("The learning loop skipped {parent_name}: {}.", prop.why);
            self.write_proposal(prop, who, via)?;
            return Ok(None);
        }
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .with_context(|| format!("the provider {:?} is not configured", target.provider))?;
        // On the runtime, waited for here: a blocking pool thread the
        // request starts (a DNS lookup) is a worker's child, never the
        // nightly run's SCHED_IDLE thread's, whose policy it would keep
        // (theseus-bgg5).
        let reply = rt
            .block_on(rt.spawn(async move {
                let mut quiet = |_: crate::provider::Delta<'_>| {};
                provider.stream_message(&request, &mut quiet).await
            }))
            .unwrap_or_else(|e| Err(anyhow::anyhow!("the writer's request ended: {e}")));
        let reply = match reply {
            Ok(resp) => {
                prop.writer_usd = micros_to_usd(
                    self.runner
                        .catalog
                        .get(&resp.model)
                        .or_else(|| self.runner.catalog.get(&target.model))
                        .map_or(need, |e| e.cost_micros(&resp.usage)),
                );
                resp.text
            }
            Err(e) => {
                // A failed request may have been billed: booked at its
                // reservation, and its errors stay new.
                prop.writer_usd = micros_to_usd(need);
                prop.errors.clear();
                prop.error_judgments.clear();
                prop.decision = "skipped".into();
                prop.why = format!("the writer's request failed: {e:#}");
                prop.said = format!("The learning loop skipped {parent_name}: {}.", prop.why);
                self.write_proposal(prop, who, via)?;
                return Ok(None);
            }
        };
        // The candidate: text only, through the loader.
        let version = self.next_learned(&parent.id);
        let (cand_text, cand) = match propose::candidate_from_reply(parent, &reply, version) {
            Ok(c) => c,
            Err(why) => {
                prop.decision = "refused".into();
                prop.why = format!("the writer's file was refused: {why}");
                prop.said =
                    format!("The learning loop's rewrite of {parent_name} was refused: {why}.");
                self.write_proposal(prop, who, via)?;
                return Ok(None);
            }
        };
        Ok(Some((cand_text, cand)))
    }

    /// A proposal of the lineage still open, in words: a card waiting on
    /// the owner, or a learned canary still running.
    fn open_card(&self, earlier: &[(u64, JudgeProposal)]) -> Option<String> {
        let ladder = self.runner.judge.ladder();
        // A learned canary still running is open too: which version is the
        // parent depends on a session's arm until it goes live or back.
        let canary = earlier.iter().find_map(|(_, p)| {
            let v = p.version.as_deref()?;
            (ladder.rows_of(v).iter().any(|r| !r.declined)
                && ladder.standing(v).rung == crate::judge::ladder::Rung::Canary)
                .then(|| v.to_string())
        });
        if let Some(v) = canary {
            return Some(format!(
                "{v} is in its canary; one open proposal per lineage, until it goes live or back"
            ));
        }
        let (_, p) = earlier.iter().find(|(_, p)| {
            p.decision == Verdict::Card.as_str()
                && p.version
                    .as_deref()
                    .is_some_and(|v| ladder.rows_of(v).is_empty())
        })?;
        Some(format!(
            "{} waits on your card ({}); one open proposal per lineage",
            p.version.as_deref().unwrap_or_default(),
            p.question.as_deref().unwrap_or_default()
        ))
    }

    /// The next learned version of a pack id: past every compiled-in one
    /// and every learned one.
    fn next_learned(&self, id: &str) -> u32 {
        let mut taken: Vec<u32> = theseus_judge::pack::embedded()
            .as_ref()
            .map(|ps| {
                ps.iter()
                    .filter(|p| p.id == id)
                    .map(|p| p.version)
                    .collect()
            })
            .unwrap_or_default();
        taken.extend(
            self.runner
                .judge
                .lineage()
                .all(&self.store)
                .iter()
                .filter(|l| l.pack.id == id)
                .map(|l| l.pack.version),
        );
        propose::next_version(&taken)
    }

    /// The replay over both splits, the re-fit, and both versions' holdout
    /// numbers.
    #[allow(clippy::too_many_arguments)]
    fn check(
        &self,
        rt: &tokio::runtime::Handle,
        g: &Gathered,
        cand_text: String,
        cand: Pack,
        prop: &mut JudgeProposal,
        who: &str,
        via: &str,
    ) -> anyhow::Result<(String, Pack, Vec<QuestionNumbers>, Vec<QuestionNumbers>)> {
        let (parent, train, holdout) = (&g.parent, &g.train, &g.holdout);
        // The check: one replay over both splits' labeled judgments.
        let ids: Vec<String> = train
            .iter()
            .chain(holdout.iter())
            .map(|s| s.judgment.id.clone())
            .collect();
        let replay_params = JudgeReplayParams {
            pack_text: Some(cand_text.clone()),
            judgments: ids,
            ..JudgeReplayParams::default()
        };
        let (rr, by_id) = self.replay_with(
            rt,
            &replay_params,
            Arc::new(cand.clone()),
            cand_text.clone(),
            who,
            via,
        )?;
        prop.replay = Some(rr.id.clone());
        prop.replay_usd = rr.cost_usd;
        // The numbers, each side apart, graded by the parent's labels.
        let parent_seen: Vec<Seen> = train.iter().chain(holdout).cloned().collect();
        let abs = super::replay::absolute_labels(&parent_seen, &g.labels);
        let side_pairs = |set: &[Seen]| {
            let p = pairs_of(
                parent,
                set.iter().map(|s| (s.judgment.id.as_str(), &s.judgment)),
                &abs,
            );
            let c = pairs_of(
                parent,
                set.iter()
                    .filter_map(|s| Some((s.judgment.id.as_str(), by_id.get(&s.judgment.id)?))),
                &abs,
            );
            (p, c)
        };
        let (p_train, c_train) = side_pairs(train);
        let (p_hold, c_hold) = side_pairs(holdout);
        // Train errors fixed, and labeled answers on either side broken.
        prop.fixed = p_train.fixed_by(&c_train);
        prop.broken = c_train.fixed_by(&p_train) + c_hold.fixed_by(&p_hold);
        // Thresholds, re-fit in code from the candidate's train answers.
        let mut fit: BTreeMap<String, Thresholds> = BTreeMap::new();
        for q in propose::whole_questions(parent) {
            let none = Vec::new();
            let pt = p_train.graded.get(&q.id).unwrap_or(&none);
            let ct = c_train.graded.get(&q.id).unwrap_or(&none);
            let t = propose::refit(q.thresholds, pt, ct);
            prop.thresholds.push(LearnThreshold {
                question: q.id.clone(),
                act_was: q.thresholds.act,
                act: t.act,
                confirm: t.confirm,
                train: ct.len() as u32,
            });
            if t != q.thresholds {
                fit.insert(q.id.clone(), t);
            }
        }
        let (cand_text, cand) = match fit.is_empty() {
            true => (cand_text, cand),
            false => propose::with_thresholds(&cand_text, &fit)
                .map_err(anyhow::Error::msg)
                .context("the re-fit thresholds do not load")?,
        };
        prop.version = Some(cand.name());
        prop.sha256 = Some(cand.sha256.clone());
        prop.diff = diff(&g.parent_text, &cand_text);
        let (pn, cn) = (numbers(&p_hold), numbers(&c_hold));
        prop.parent_holdout = wire(&pn);
        prop.candidate_holdout = wire(&cn);
        Ok((cand_text, cand, pn, cn))
    }

    /// The decision, the version's row and file, and the move.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &self,
        root: &str,
        g: &Gathered,
        cand_text: String,
        cand: Pack,
        pn: &[QuestionNumbers],
        cn: &[QuestionNumbers],
        prop: &mut JudgeProposal,
        now: u64,
    ) -> anyhow::Result<()> {
        let j = &self.runner.judge;
        let lc = &self.cfg.judge.learn;
        let (parent, parent_name, holdout) = (&g.parent, &g.parent_name, &g.holdout);
        let parent_seen: Vec<Seen> = g.train.iter().chain(holdout).cloned().collect();
        let abs = super::replay::absolute_labels(&parent_seen, &g.labels);
        // The minimum: 25c's, on the holdout's labels.
        let deciding: Vec<String> = parent
            .questions
            .iter()
            .filter(|q| q.decides && q.per.is_none() && q.kind != Kind::Score)
            .map(|q| q.id.clone())
            .collect();
        prop.sufficient = Self::holdout_sufficient(parent, &deciding, pn, holdout, &abs);
        let standing = j.ladder().standing(parent_name);
        let evidence = Evidence {
            parent: pn,
            candidate: cn,
            deciding: &deciding,
            sufficient: prop.sufficient,
            fixed: prop.fixed,
            margin: lc.margin,
            parent_acts: standing.rung.acts(),
            security: parent.id == "security",
        };
        let (verdict, why) = decide(&evidence);
        prop.decision = verdict.as_str().into();
        prop.why = why;
        // The version: its row and its file, then the move.
        let learned = Learned {
            pack: Arc::new(cand),
            text: cand_text,
            parent: parent_name.to_string(),
            root: root.to_string(),
            proposal: prop.id.clone(),
            at_ms: now,
        };
        self.write_version(&learned)?;
        prop.said = Self::said(prop, &evidence);
        let numbers_words = prop.said.clone();
        let name = learned.name();
        let moved = match verdict {
            Verdict::Live => Some(("live", None)),
            Verdict::Canary => Some(("canary", Some(CANARY_SHARE))),
            Verdict::Shadow => Some(("shadow", None)),
            Verdict::Card if standing.rung.acts() => Some(("live", None)),
            Verdict::Card => Some(("shadow", None)),
            Verdict::Held => None,
        };
        if let Some((to, share)) = moved {
            match self.promote_learned(&name, to, share, &prop.id, &prop.why, &numbers_words) {
                Ok(r) => prop.question = r.question,
                Err(e) => {
                    prop.decision = Verdict::Held.as_str().into();
                    prop.why = format!("held: the ladder refused it: {e:#}");
                    prop.said = Self::said(prop, &evidence);
                }
            }
        }
        Ok(())
    }

    /// One error as the writer reads it: its state from its blob, the
    /// answers, the label, and its note.
    fn error_case(&self, e: &Error<'_>) -> ErrorCase {
        let j = &e.seen.judgment;
        let state = j.context["blob"]
            .as_str()
            .and_then(|d| self.store.blobs().base64(d))
            .and_then(|b| crate::blobs::decode(&b).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| "(its state's blob is missing)".into());
        let note = self
            .store
            .ledger_by_key(&e.label.id)
            .ok()
            .flatten()
            .and_then(|r| r.decode::<LedgerRow>().ok())
            .and_then(|r| r.data["note"].as_str().map(str::to_string))
            .unwrap_or_default();
        ErrorCase {
            judgment: j.id.clone(),
            state,
            answers: j
                .answers
                .iter()
                .filter(|a| a.about.is_none())
                .map(top_words)
                .collect(),
            label: truth_words(&e.question, &e.truth),
            note,
        }
    }

    /// 25c's minimum on the holdout: 200 labeled per deciding question, 30
    /// per acting class (`report::ACTING`).
    fn holdout_sufficient(
        parent: &Pack,
        deciding: &[String],
        pn: &[QuestionNumbers],
        holdout: &[Seen],
        abs: &HashMap<String, Vec<LabelRow>>,
    ) -> bool {
        let per_q: BTreeMap<String, usize> = pn
            .iter()
            .map(|q| (q.question.clone(), q.labeled as usize))
            .collect();
        let acting: Vec<(&str, &[&str])> = super::report::ACTING
            .iter()
            .filter(|(id, _, _)| *id == parent.id)
            .map(|(_, q, c)| (*q, *c))
            .collect();
        let mut per_c: BTreeMap<String, usize> = BTreeMap::new();
        let mut classes: Vec<&str> = Vec::new();
        for (q, cs) in &acting {
            classes.extend(cs.iter());
            for s in holdout {
                let Some(a) = answer(s, q) else { continue };
                let labeled = abs
                    .get(&s.judgment.id)
                    .is_some_and(|ls| ls.iter().any(|l| l.question.as_deref() == Some(q)));
                if let (true, Top::Choice(top), Band::Act) = (labeled, &a.band.top, a.band.band) {
                    if cs.contains(&top.as_str()) {
                        *per_c.entry(top.clone()).or_default() += 1;
                    }
                }
            }
        }
        let d: Vec<&str> = deciding.iter().map(String::as_str).collect();
        learn::sufficient(&per_q, &per_c, &d, &classes, Minimum::default()).is_ok()
    }

    /// The notice's sentence: "classify.v101 from 12 of your labels:
    /// holdout precision 0.80 → 0.86, recall 0.70 → 0.75; replacing
    /// classify.v1 in shadow".
    fn said(p: &JudgeProposal, e: &Evidence<'_>) -> String {
        let name = p.version.as_deref().unwrap_or("a candidate");
        let first = e
            .deciding
            .first()
            .cloned()
            .or_else(|| e.parent.first().map(|q| q.question.clone()));
        let pr = |side: &[QuestionNumbers]| {
            first
                .as_ref()
                .and_then(|q| side.iter().find(|x| &x.question == q))
                .map(QuestionNumbers::macro_pr)
                .unwrap_or((None, None))
        };
        let ((pp, prr), (cp, cr)) = (pr(e.parent), pr(e.candidate));
        let place = match p.decision.as_str() {
            "live" => format!("live in place of {}", p.parent),
            "canary" => format!("a {:.0}% canary beside {}", CANARY_SHARE * 100.0, p.parent),
            "shadow" => format!("replacing {} in shadow", p.parent),
            "card" => format!("waiting on your approval card in place of {}", p.parent),
            _ => p.why.clone(),
        };
        format!(
            "{name} from {} of your labels: holdout precision {} → {}, recall {} → {}{}; {place}",
            p.errors.len(),
            words(pp),
            words(cp),
            words(prr),
            words(cr),
            first.map(|q| format!(" ({q})")).unwrap_or_default(),
        )
    }

    /// The version's row and file; kept in the lineage at once.
    fn write_version(&self, l: &Learned) -> anyhow::Result<()> {
        let f = fact::judge_runs::PackVersioned { learned: l };
        let mut rec = fact::row(&f, None, None)?;
        rec.key = Some(lineage::key(&l.name()));
        self.store.append(&[rec.scoped(&lineage::scope(&l.root))])?;
        self.runner.judge.lineage().add(l.clone());
        if let Err(e) =
            lineage::write_files(&lineage::state_of(&self.store), std::slice::from_ref(l))
        {
            tracing::warn!(error = %format!("{e:#}"), "learning: the version's file was not written; its row holds it");
        }
        self.rec(None).announce(&f);
        Ok(())
    }

    fn write_proposal(&self, p: &JudgeProposal, who: &str, via: &str) -> anyhow::Result<()> {
        let f = fact::judge_runs::JudgeProposed {
            proposal: p,
            who,
            via,
        };
        let mut rec = fact::row(&f, None, None)?;
        rec.key = Some(p.id.clone());
        self.store.append(&[rec.scoped(&lineage::scope(&p.root))])?;
        self.rec(None).announce(&f);
        Ok(())
    }

    /// Every proposal of every lineage, newest first (the cockpit's
    /// versions view reads them through `pack.list`'s rows and this).
    pub fn learn_proposals(&self) -> Vec<JudgeProposal> {
        let mut all: Vec<(u64, JudgeProposal)> = roots()
            .iter()
            .flat_map(|r| self.proposals(r).unwrap_or_default())
            .collect();
        all.sort_by_key(|a| std::cmp::Reverse(a.0));
        all.into_iter().map(|(_, p)| p).collect()
    }
}

//! Replay (M5 25d; design §2.9): a candidate pack version asked the
//! incumbent's questions over a set of the incumbent's recorded judgments,
//! and both graded by the same labels, every number the learning report's.
//! The learning loop (theseus-0j2.12) calls it in-process to check a
//! proposed version on the errors it was written from and on the holdout.
//!
//! - **The candidate** is a version the build does not wire: an embedded
//!   one by name, or a pack file's text, parsed with `Pack::parse` (every
//!   loader rule). Same id as the incumbent; a version no row already holds
//!   under another sha256, so a name always means one text. It never acts.
//! - **The set**: a report's frozen holdout (`Holdout::judgments`), its train
//!   split (answered judgments before the window, never the holdout's), only
//!   the labeled ones the incumbent got wrong (`errors`), or ids. The labels
//!   are the report's frozen ones for its holdout, else today's.
//! - **The states**: each judgment's blob, sent as it was when the
//!   candidate's builder, its version, and its cap equal the judgment's; else
//!   rebuilt from its inputs (`rebuild`), or left out with the reason. A
//!   candidate that changes only thresholds makes no call: the stored
//!   answers are re-banded.
//! - **The calls** go through the judge's client and breaker, one state at a
//!   time on the run's low thread, each a `judge.call` row (`purpose:
//!   replay`, the run, the judgment it re-asks) scoped
//!   `judge.replay:<pack id>`, which the nightly report never reads. The run
//!   is a `judge.replay` row (`rpl_…`) in the same frame, with the
//!   candidate's text in a blob.
//! - **Spend**: priced and reserved as any judgment. The run's estimate is
//!   checked first against `[judge] replay_limit_usd`, and refused with the
//!   numbers; a run stops before a call would pass it. It never touches the
//!   shadow day budget (`judge::spend`), so a replay never pauses shadow
//!   judging.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{bail, Context as _};
use serde_json::{json, Value};
use theseus_judge::band::Top;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::learn::Window;
use theseus_judge::price::{micros_to_usd, usd_to_micros, Micros};
use theseus_judge::replay::{reband, stored_state, stored_state_differs, what_changes, Change};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Judgment, Mode, Outcome, Pack, Urgency};
use theseus_protocol::judge_runs::{
    JudgeReplayParams, JudgeReplayResult, ReplayClassFell, ReplayEval, ReplayEvalSide,
    ReplayJudgment, ReplayLeftOut,
};
use theseus_protocol::learning::{PackReport, QuestionReport};
use theseus_protocol::LedgerKind;
use theseus_store::NewRecord;

use super::labels::{resolve, truth, Truth};
use super::report::{answer, graded, pack_report_of};
use super::{read_scope, LabelRow, Seen};
use crate::fact;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// The scope of a pack's replays: its calls and its runs, apart from the
/// `judge:<pack id>` scope the report reads.
pub fn scope(pack: &str) -> String {
    format!("judge.replay:{}", pack.split('.').next().unwrap_or(pack))
}

/// A label's truth in absolute form: what it says of the question, whoever
/// answered it, so the candidate is graded by the same truth as the
/// incumbent.
fn absolute(t: &Truth) -> Value {
    match t {
        Truth::Bool(b) => json!(b),
        Truth::Class(c) => json!(c),
        Truth::NotClass(c) => json!({ "not": c }),
        Truth::Level(n) => json!(n),
        Truth::NotLevel(n) => json!({ "not": n }),
    }
}

/// Each judgment's counting label per question, as an absolute label on
/// that question (by the judgment's id), from `labels`.
pub fn absolute_labels(
    seen: &[Seen],
    labels: &HashMap<String, Vec<LabelRow>>,
) -> HashMap<String, Vec<LabelRow>> {
    let mut out: HashMap<String, Vec<LabelRow>> = HashMap::new();
    for s in seen {
        let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
        for a in s.judgment.answers.iter().filter(|a| a.about.is_none()) {
            if let Some((l, t)) = resolve(ls, &a.def, a) {
                out.entry(s.judgment.id.clone())
                    .or_default()
                    .push(LabelRow {
                        question: Some(a.def.clone()),
                        label: absolute(&t),
                        ..l.clone()
                    });
            }
        }
    }
    out
}

/// Whether a judgment's answer to `q` was right by its absolute label:
/// none when the label settles nothing of it.
fn right(labels: &[LabelRow], q: &str, a: &AnswerRecord) -> Option<bool> {
    let l = labels.iter().find(|l| l.question.as_deref() == Some(q))?;
    let t = truth(&l.label, a)?;
    graded(a, &t).map(|(_, r)| r)
}

/// Whether the incumbent got a labeled question wrong.
fn has_error(s: &Seen, labels: &HashMap<String, Vec<LabelRow>>) -> bool {
    let ls = labels.get(&s.judgment.id).map_or(&[][..], Vec::as_slice);
    s.judgment
        .answers
        .iter()
        .filter(|a| a.about.is_none())
        .any(|a| right(ls, &a.def, a) == Some(false))
}

/// A replay, read and checked: nothing sent.
struct Plan {
    candidate: Arc<Pack>,
    incumbent: Arc<Pack>,
    text: String,
    set: &'static str,
    report: Option<String>,
    labels_mode: &'static str,
    errors: bool,
    /// The incumbent's judgments in the set, oldest first.
    seen: Vec<Seen>,
    /// Their labels, absolute, by judgment.
    labels: HashMap<String, Vec<LabelRow>>,
    left_out: Vec<ReplayLeftOut>,
}

/// One judgment, ready: a call to make, or the stored answers re-banded.
enum Ready {
    Call { ask: Ask, how: &'static str },
    Rebanded(Box<Judgment>),
}

/// The run's calls: through the judge's client and breaker, one at a time,
/// each reserved inside the run's limit and settled at what it cost (its
/// reservation, when its usage is unknown).
pub(super) struct Caller<'a> {
    rt: &'a tokio::runtime::Handle,
    built: &'a crate::judge::Built,
    pub(super) limit: Micros,
    pub(super) spent: Micros,
    /// Every call made, answered or not: each is a row.
    pub(super) called: Vec<Judgment>,
}

impl<'a> Caller<'a> {
    pub(super) fn new(
        rt: &'a tokio::runtime::Handle,
        built: &'a crate::judge::Built,
        limit: Micros,
    ) -> Self {
        Self {
            rt,
            built,
            limit,
            spent: 0,
            called: Vec::new(),
        }
    }

    fn reserve(&self, a: &Ask) -> Option<Micros> {
        self.built.jev().reserve_micros(std::slice::from_ref(a))
    }

    /// What the asks would reserve; `Err` when a model is unpriced.
    pub(super) fn estimate<'b>(
        &self,
        asks: impl Iterator<Item = &'b Ask>,
    ) -> anyhow::Result<Micros> {
        let mut sum = 0;
        for a in asks {
            sum += self.reserve(a).with_context(|| {
                format!(
                    "{} has no price in the catalog: nothing is sent",
                    a.pack.jev_model
                )
            })?;
        }
        Ok(sum)
    }

    /// One call; none when it would pass the run's limit.
    pub(super) fn ask(&mut self, ask: Ask) -> Option<Judgment> {
        let need = self.reserve(&ask).unwrap_or(0);
        if self.spent + need > self.limit {
            return None;
        }
        let j = self
            .rt
            .block_on(self.built.jev().judge(DecisionPoint {
                asks: vec![ask],
                urgency: Urgency::Shadow,
            }))
            .pop()?;
        let unknown = matches!(
            j.outcome,
            Outcome::Failed {
                usage_unknown: true,
                ..
            }
        );
        self.spent += j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        self.called.push(j.clone());
        Some(j)
    }

    /// The planted-injection set, each case asked of both versions, and
    /// each expectation met or missed.
    fn eval(
        &mut self,
        plan: &Plan,
        asks: Vec<(theseus_judge::eval::Case, Ask, Ask)>,
    ) -> ReplayEval {
        let mut inc = ReplayEvalSide {
            pack: plan.incumbent.name(),
            ..Default::default()
        };
        let mut cand = ReplayEvalSide {
            pack: plan.candidate.name(),
            ..Default::default()
        };
        let cases = asks.len() as u32;
        for (case, a_inc, a_cand) in asks {
            for (side, ask, pack) in [
                (&mut inc, a_inc, &plan.incumbent),
                (&mut cand, a_cand, &plan.candidate),
            ] {
                match self.ask(ask) {
                    Some(j) if j.outcome == Outcome::Answered => {
                        for c in theseus_judge::eval::check(&case, pack, &j.answers) {
                            match c.met {
                                true => side.met += 1,
                                false => side.missed += 1,
                            }
                        }
                    }
                    _ => side.unanswered += 1,
                }
            }
        }
        ReplayEval {
            set: "security.v3".into(),
            cases,
            incumbent: inc,
            candidate: cand,
        }
    }
}

/// The candidate and its text, from the params.
pub fn candidate_of(p: &JudgeReplayParams) -> anyhow::Result<(Arc<Pack>, String)> {
    match (p.candidate.as_deref().map(str::trim), &p.pack_text) {
        (Some(name), None) => {
            let text = theseus_judge::pack::EMBEDDED
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, t)| (*t).to_string())
                .with_context(|| format!("this build embeds no pack {name}"))?;
            let pack = theseus_judge::pack::by_name(name)
                .with_context(|| format!("the embedded pack {name} does not load"))?;
            Ok((pack, text))
        }
        (None, Some(text)) => {
            let pack = Pack::parse(text)
                .map_err(|e| anyhow::anyhow!("the pack file does not load: {e}"))?;
            Ok((Arc::new(pack), text.clone()))
        }
        _ => bail!(
            "name one candidate: an embedded version (`loop.v2`) or a pack file (--pack-file)"
        ),
    }
}

impl Core {
    /// `judge.replay`: the owner's run, from a private place
    /// (`judge_act(Act::JudgeRun)`), on a thread of its own at low
    /// priority, off the start path and every turn's.
    pub async fn judge_replay(
        self: &Arc<Self>,
        p: JudgeReplayParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> anyhow::Result<JudgeReplayResult> {
        let who = who.into();
        if !self.cfg.judge.enabled {
            bail!("the judge is off ([judge] enabled = false): nothing is replayed");
        }
        let (candidate, text) = candidate_of(&p)?;
        let what = format!("replay of {}", candidate.name());
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::JUDGE_REPLAY,
                what: &what,
            },
        )?;
        let core = Arc::downgrade(self);
        let rt = tokio::runtime::Handle::current();
        let (who_s, via) = (who.who(), who.via());
        let rx = super::tender::on_low_thread(move || match core.upgrade() {
            Some(c) => c.replay_run(&rt, &p, candidate, text, &who_s, &via),
            None => Err(anyhow::anyhow!("the daemon stopped before the replay ran")),
        })?;
        rx.await.context("the replay's thread ended")?.1
    }

    /// The incumbent, its set, and the set's labels.
    fn replay_plan(
        &self,
        p: &JudgeReplayParams,
        candidate: Arc<Pack>,
        text: String,
    ) -> anyhow::Result<Plan> {
        let wired = crate::judge::WIRED
            .iter()
            .any(|(n, _)| *n == candidate.name());
        if wired {
            bail!(
                "{} is wired: a candidate is a version this build does not dispatch",
                candidate.name()
            );
        }
        if let Some(e) = theseus_judge::pack::by_name(&candidate.name()) {
            if e.sha256 != candidate.sha256 {
                bail!(
                    "{} is this build's embedded version, under another text (sha256 {}); give \
                     the candidate a version of its own",
                    candidate.name(),
                    e.sha256
                );
            }
        }
        // The report, when the set is its.
        let report: Option<(String, PackReport)> = match p.report.as_deref().map(str::trim) {
            Some(id) => {
                let r = self
                    .store
                    .ledger_by_key(id)?
                    .with_context(|| format!("no report is named {id}"))?;
                let row: LedgerRow = r.decode()?;
                if row.kind != LedgerKind::JudgeReport.as_str() {
                    bail!("{id} is not a learning report");
                }
                let pr: PackReport = serde_json::from_value(row.data["report"].clone())
                    .with_context(|| format!("{id}'s report does not read"))?;
                Some((id.to_string(), pr))
            }
            None => None,
        };
        let mut scope = read_scope(&self.store, &candidate.id)?;
        // The incumbent: the report's version, the judgments', or the wired
        // one of the candidate's id.
        let inc_name = match (&report, p.judgments.first()) {
            (Some((_, r)), _) => r.pack.clone(),
            (None, Some(id)) => scope
                .judgments
                .values()
                .flatten()
                .find(|s| s.judgment.id == *id)
                .map(|s| s.judgment.pack.clone())
                .with_context(|| format!("{id} is not a judgment of {}", candidate.id))?,
            (None, None) => bail!(
                "name the set: a report (--report rpt_<date>_<pack>) or judgments (--judgments)"
            ),
        };
        let incumbent = self
            .runner
            .judge
            .pack(&inc_name)
            .with_context(|| format!("this build has no pack {inc_name}"))?;
        if incumbent.id != candidate.id {
            bail!(
                "{} is not a version of {}: a candidate replays its own pack's judgments",
                candidate.name(),
                incumbent.id
            );
        }
        if incumbent.name() == candidate.name() {
            bail!("{} is the incumbent itself", candidate.name());
        }
        self.same_name_same_text(&candidate)?;
        let all: Vec<Seen> = scope.judgments.remove(&inc_name).unwrap_or_default();
        let (set, labels_mode, mut seen, labels) = select(
            p,
            report.as_ref().map(|(_, r)| r),
            all,
            scope.labels,
            &inc_name,
        )?;
        let mut left_out = Vec::new();
        seen.retain(|s| {
            let answered = s.judgment.outcome == Outcome::Answered;
            if !answered {
                left_out.push(ReplayLeftOut {
                    judgment: s.judgment.id.clone(),
                    reason: "the incumbent's call was not answered".into(),
                });
            }
            answered
        });
        let labels = absolute_labels(&seen, &labels);
        if p.errors {
            seen.retain(|s| has_error(s, &labels));
        }
        Ok(Plan {
            candidate,
            incumbent,
            text,
            set,
            report: report.map(|(id, _)| id),
            labels_mode,
            errors: p.errors,
            seen,
            labels,
            left_out,
        })
    }

    /// A name always means one text: no judgment or replay of this pack
    /// holds the candidate's name under another sha256.
    fn same_name_same_text(&self, c: &Pack) -> anyhow::Result<()> {
        let name = c.name();
        for sc in [crate::rpc::judge::scope_of(&c.id), scope(&c.id)] {
            for r in self.store.scope_after(&sc, 0)? {
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                let (held, sha) = if row.kind == LedgerKind::JudgeCall.as_str() {
                    (&row.data["pack"], &row.data["pack_sha256"])
                } else if row.kind == LedgerKind::JudgeReplay.as_str() {
                    (&row.data["candidate"], &row.data["candidate_sha256"])
                } else {
                    continue;
                };
                if held.as_str() == Some(name.as_str())
                    && sha.as_str().is_some_and(|s| s != c.sha256)
                {
                    bail!(
                        "{name} is already recorded under another text (sha256 {}); give the \
                         candidate a version of its own",
                        sha.as_str().unwrap_or_default()
                    );
                }
            }
        }
        Ok(())
    }

    /// One judgment's state for the candidate: the stored one, or one
    /// rebuilt from its inputs; `Err`: why neither.
    fn replay_state(
        &self,
        cand: &Pack,
        s: &Seen,
    ) -> Result<(theseus_judge::Prepared, String, &'static str), String> {
        let j = &s.judgment;
        // A learned candidate (25f) keeps its parent's questions and
        // builder: it is asked on the stored state without the builder's
        // items (a question only for them is not asked; a Choice drawing
        // options from them asks its own).
        let dynamic = !theseus_judge::propose::is_learned(cand.version)
            && cand
                .questions
                .iter()
                .any(|q| q.options_from.is_some() || q.per.is_some() || q.only_when.is_some());
        if stored_state_differs(cand, &j.state).is_none() && !dynamic {
            let digest = j.context["blob"]
                .as_str()
                .ok_or("its judgment names no state blob")?;
            let json = self
                .store
                .blobs()
                .base64(digest)
                .and_then(|b| crate::blobs::decode(&b).ok())
                .and_then(|b| String::from_utf8(b).ok())
                .ok_or_else(|| format!("its state's blob {digest} is missing"))?;
            let state = stored_state(json, &j.state)?;
            let prepared = theseus_judge::Prepared {
                state: Arc::new(state),
                dynamic: Default::default(),
            };
            return Ok((prepared, digest.to_string(), "stored"));
        }
        let why_not_stored = stored_state_differs(cand, &j.state).unwrap_or_else(|| {
            "the candidate's questions draw on the builder's items, which the record does not keep"
                .into()
        });
        if let Some(why) = super::rebuild::unrebuildable(cand.builder) {
            return Err(format!("{why_not_stored}, and it can't be rebuilt: {why}"));
        }
        let input = match cand.builder {
            theseus_judge::pack::Builder::Loop => self
                .rebuild_loop(&j.context)
                .map(Input::Loop)
                .map_err(|e| format!("{why_not_stored}, and it can't be rebuilt: {e}"))?,
            _ => unreachable!("only loop's input is rebuilt"),
        };
        let svc = &self.runner.judge;
        let prepared =
            theseus_judge::prepare(cand, &input, &svc.scrub()).map_err(|e| e.to_string())?;
        let blob = self
            .store
            .blobs()
            .put(prepared.state.json.as_bytes())
            .map_err(|e| format!("the rebuilt state's blob was not written: {e}"))?;
        Ok((prepared, blob, "rebuilt"))
    }

    /// The run, on its own thread: plan, estimate, call, grade, write.
    fn replay_run(
        &self,
        rt: &tokio::runtime::Handle,
        p: &JudgeReplayParams,
        candidate: Arc<Pack>,
        text: String,
        who: &str,
        via: &str,
    ) -> anyhow::Result<JudgeReplayResult> {
        self.replay_with(rt, p, candidate, text, who, via)
            .map(|(r, _)| r)
    }

    /// The same, with the candidate's answered judgment for each judgment
    /// of the set, by the incumbent's id: the learning loop (25f) grades
    /// each split apart.
    pub(crate) fn replay_with(
        &self,
        rt: &tokio::runtime::Handle,
        p: &JudgeReplayParams,
        candidate: Arc<Pack>,
        text: String,
        who: &str,
        via: &str,
    ) -> anyhow::Result<(JudgeReplayResult, BTreeMap<String, Judgment>)> {
        // The judge's first use builds its sink's task on the runtime.
        let _rt = rt.enter();
        let id = crate::new_id("rpl");
        let mut plan = self.replay_plan(p, candidate, text)?;
        let change = what_changes(&plan.incumbent, &plan.candidate);
        let built = self.runner.judge.jev()?;
        let ready = self.replay_ready(&mut plan, change, &id);
        // A security candidate's planted-injection set, beside the incumbent.
        let eval_asks = self.eval_asks(&plan, &id);
        let mut caller = Caller::new(rt, &built, usd_to_micros(self.cfg.judge.replay_limit_usd));
        let asks = ready
            .iter()
            .filter_map(|(_, r)| match r {
                Ready::Call { ask, .. } => Some(ask),
                Ready::Rebanded(_) => None,
            })
            .chain(eval_asks.iter().flat_map(|(_, a, b)| [a, b]));
        let estimate = caller.estimate(asks)?;
        if estimate > caller.limit {
            bail!(
                "the replay of {} over {} judgments would reserve {}, past [judge] \
                 replay_limit_usd ({}); nothing was sent",
                plan.candidate.name(),
                plan.seen.len(),
                crate::narrative::dollars(estimate),
                crate::narrative::dollars(caller.limit)
            );
        }
        // The calls, one at a time, inside the run's limit.
        let mut cand_of: BTreeMap<usize, (Judgment, &'static str)> = BTreeMap::new();
        for (i, r) in ready {
            let of = plan.seen[i].judgment.id.clone();
            let reason = match r {
                Ready::Rebanded(j) => {
                    cand_of.insert(i, (*j, "rebanded"));
                    continue;
                }
                Ready::Call { ask, how } => match caller.ask(ask) {
                    Some(j) if j.outcome == Outcome::Answered => {
                        cand_of.insert(i, (j, how));
                        continue;
                    }
                    Some(j) => format!(
                        "the candidate's call was not answered ({})",
                        outcome_word(&j.outcome)
                    ),
                    None => "the run reached [judge] replay_limit_usd".into(),
                },
            };
            plan.left_out.push(ReplayLeftOut {
                judgment: of,
                reason,
            });
        }
        let eval = (!eval_asks.is_empty()).then(|| caller.eval(&plan, eval_asks));
        let result = replay_result(&id, &plan, change, &cand_of, estimate, &caller, eval);
        self.write_replay(&plan, &result, &caller.called, who, via)?;
        let by_id = cand_of
            .into_iter()
            .map(|(i, (j, _))| (plan.seen[i].judgment.id.clone(), j))
            .collect();
        Ok((result, by_id))
    }

    /// Each judgment of the set, ready: its stored answers re-banded, or a
    /// call with its state; a state neither stored nor rebuilt is left out.
    fn replay_ready(&self, plan: &mut Plan, change: Change, run: &str) -> Vec<(usize, Ready)> {
        let cand = plan.candidate.clone();
        let mut ready = Vec::new();
        for (i, s) in plan.seen.iter().enumerate() {
            if change == Change::ThresholdsOnly {
                let mut j = s.judgment.clone();
                j.answers = reband(&j.answers, &cand);
                j.pack = cand.name();
                j.version = cand.version;
                j.pack_sha256 = cand.sha256.clone();
                ready.push((i, Ready::Rebanded(Box::new(j))));
                continue;
            }
            match self.replay_state(&cand, s) {
                Ok((prepared, blob, how)) => {
                    let c = &s.judgment.context;
                    let context = json!({
                        "purpose": "replay", "run": run, "rejudges": s.judgment.id, "blob": blob,
                        "state": how, "session": c.get("session"), "turn": c.get("turn"),
                        "class": c.get("class"), "on_path_ms": 0,
                    });
                    let mut ask = Ask::new(cand.clone(), &prepared, Mode::Shadow, context);
                    ask.id = Some(theseus_judge::new_id());
                    ready.push((i, Ready::Call { ask, how }));
                }
                Err(reason) => plan.left_out.push(ReplayLeftOut {
                    judgment: s.judgment.id.clone(),
                    reason,
                }),
            }
        }
        ready
    }

    /// The planted-injection set's asks for a security candidate: each
    /// case's state for the incumbent and for the candidate.
    fn eval_asks(&self, plan: &Plan, run: &str) -> Vec<(theseus_judge::eval::Case, Ask, Ask)> {
        if plan.candidate.id != "security" {
            return Vec::new();
        }
        let Ok(cases) = theseus_judge::eval::set("security.v3") else {
            return Vec::new();
        };
        let scrub = self.runner.judge.scrub();
        let ask_for = |pack: &Arc<Pack>, case: &theseus_judge::eval::Case| -> Option<Ask> {
            let input = match pack.builder {
                theseus_judge::pack::Builder::Security => Input::Security(case.input.clone()),
                theseus_judge::pack::Builder::Security2 => Input::Security2(case.input.clone()),
                _ => return None,
            };
            let prepared = theseus_judge::prepare(pack, &input, &scrub).ok()?;
            let context = json!({"purpose": "replay_eval", "run": run, "case": case.name});
            let mut ask = Ask::new(pack.clone(), &prepared, Mode::Shadow, context);
            ask.id = Some(theseus_judge::new_id());
            Some(ask)
        };
        cases
            .into_iter()
            .filter(|c| c.category != theseus_judge::eval::Category::Observe)
            .filter_map(|c| {
                let a = ask_for(&plan.incumbent, &c)?;
                let b = ask_for(&plan.candidate, &c)?;
                Some((c, a, b))
            })
            .collect()
    }

    /// The run's frame: each call's `judge.call` row and the `judge.replay`
    /// row, scoped `judge.replay:<pack id>`, with the candidate's text in a
    /// blob written first.
    fn write_replay(
        &self,
        plan: &Plan,
        r: &JudgeReplayResult,
        called: &[Judgment],
        who: &str,
        via: &str,
    ) -> anyhow::Result<()> {
        let sc = scope(&plan.candidate.id);
        let text_blob = self.store.blobs().put(plan.text.as_bytes())?;
        let mut records: Vec<NewRecord> = Vec::new();
        for j in called {
            let f = fact::judge::JudgeCall {
                judgment: j,
                budget: "replay",
            };
            let mut rec = fact::row(&f, None, None)?;
            rec.key = Some(j.id.clone());
            records.push(rec.scoped(&sc));
        }
        let f = fact::judge_runs::JudgeReplayed {
            result: r,
            text_blob: &text_blob,
            who,
            via,
        };
        let mut rec = fact::row(&f, None, None)?;
        rec.key = Some(r.id.clone());
        records.push(rec.scoped(&sc));
        self.store.append(&records)?;
        self.rec(None).announce(&f);
        Ok(())
    }
}

/// What a set's params select: its kind, whose labels grade it, the
/// incumbent's judgments in it, and their labels.
type Selected = (
    &'static str,
    &'static str,
    Vec<Seen>,
    HashMap<String, Vec<LabelRow>>,
);

/// The set: a report's frozen holdout (with its frozen labels), its train
/// split, or ids (each with today's labels).
fn select(
    p: &JudgeReplayParams,
    report: Option<&PackReport>,
    all: Vec<Seen>,
    labels: HashMap<String, Vec<LabelRow>>,
    inc_name: &str,
) -> anyhow::Result<Selected> {
    Ok(match (report, p.judgments.is_empty()) {
        (Some(r), true) => {
            let h = &r.holdout;
            match p.split.as_deref().unwrap_or("holdout") {
                "holdout" => {
                    let ids: BTreeSet<&String> = h.judgments.iter().collect();
                    let frozen: BTreeSet<&String> = h.labels.iter().collect();
                    let frozen_labels: HashMap<String, Vec<LabelRow>> = labels
                        .iter()
                        .map(|(j, ls)| {
                            let ls = ls.iter().filter(|l| frozen.contains(&l.id)).cloned();
                            (j.clone(), ls.collect())
                        })
                        .collect();
                    let seen = all
                        .into_iter()
                        .filter(|s| ids.contains(&s.judgment.id))
                        .collect();
                    ("holdout", "frozen", seen, frozen_labels)
                }
                "train" => {
                    let ids: BTreeSet<&String> = h.judgments.iter().collect();
                    let seen = all
                        .into_iter()
                        .filter(|s| {
                            s.at_ms < h.start_ms
                                && !ids.contains(&s.judgment.id)
                                && s.judgment.outcome == Outcome::Answered
                        })
                        .collect();
                    ("train", "today", seen, labels)
                }
                other => bail!("--split is holdout or train, not {other:?}"),
            }
        }
        (None, false) => {
            let ids: BTreeSet<&String> = p.judgments.iter().collect();
            let seen: Vec<Seen> = all
                .into_iter()
                .filter(|s| ids.contains(&s.judgment.id))
                .collect();
            if let Some(missing) = p
                .judgments
                .iter()
                .find(|id| !seen.iter().any(|s| s.judgment.id == **id))
            {
                bail!("{missing} is not a judgment of {inc_name}");
            }
            ("judgments", "today", seen, labels)
        }
        (Some(_), false) => bail!("name a report or judgments, not both"),
        (None, true) => unreachable!("the incumbent is named by one of them"),
    })
}

/// Both sides' numbers on the judgments the candidate answered, and what
/// it fixed and broke.
fn replay_result(
    id: &str,
    plan: &Plan,
    change: Change,
    cand_of: &BTreeMap<usize, (Judgment, &'static str)>,
    estimate: Micros,
    caller: &Caller<'_>,
    eval: Option<ReplayEval>,
) -> JudgeReplayResult {
    // Both sides on the same judgments: those the candidate answered.
    let inc_seen: Vec<Seen> = cand_of.keys().map(|i| plan.seen[*i].clone()).collect();
    let cand_seen: Vec<Seen> = cand_of
        .iter()
        .map(|(i, (j, _))| {
            let s = &plan.seen[*i];
            let mut j = j.clone();
            // Graded by the incumbent's labels, which name its id.
            j.id = s.judgment.id.clone();
            Seen {
                position: s.position,
                at_ms: s.at_ms,
                judgment: j,
            }
        })
        .collect();
    let all = Window {
        start_ms: 0,
        end_ms: u64::MAX,
    };
    let inc_name = plan.incumbent.name();
    let incumbent_report = pack_report_of(
        Some(&plan.incumbent),
        &inc_name,
        &inc_seen,
        &plan.labels,
        all,
    );
    let candidate_report = pack_report_of(
        Some(&plan.candidate),
        &plan.candidate.name(),
        &cand_seen,
        &plan.labels,
        all,
    );
    let mut per_judgment = Vec::new();
    let (mut fixed, mut broken, mut agree) = (0u32, 0u32, 0u32);
    for ((i, (j, how)), c) in cand_of.iter().zip(&cand_seen) {
        let s = &plan.seen[*i];
        let ls = plan
            .labels
            .get(&s.judgment.id)
            .map_or(&[][..], Vec::as_slice);
        let mut row = ReplayJudgment {
            judgment: s.judgment.id.clone(),
            replayed: (*how != "rebanded").then(|| j.id.clone()),
            state: (*how).to_string(),
            fixed: vec![],
            broken: vec![],
        };
        let mut same = true;
        for a in s.judgment.answers.iter().filter(|a| a.about.is_none()) {
            let Some(b) = answer(c, &a.def) else {
                continue;
            };
            same &= top_of(a) == top_of(b);
            match (right(ls, &a.def, a), right(ls, &a.def, b)) {
                (Some(false), Some(true)) => row.fixed.push(a.def.clone()),
                (Some(true), Some(false)) => row.broken.push(a.def.clone()),
                _ => {}
            }
        }
        agree += u32::from(same);
        fixed += row.fixed.len() as u32;
        broken += row.broken.len() as u32;
        per_judgment.push(row);
    }
    let fell = fell(&incumbent_report.questions, &candidate_report.questions);
    let count = |h: &str| cand_of.values().filter(|(_, x)| *x == h).count() as u32;
    let (stored, rebuilt, rebanded) = (count("stored"), count("rebuilt"), count("rebanded"));
    JudgeReplayResult {
        id: id.to_string(),
        candidate: plan.candidate.name(),
        candidate_sha256: plan.candidate.sha256.clone(),
        incumbent: inc_name,
        change: match change {
            Change::Asks => "asks",
            Change::ThresholdsOnly => "thresholds_only",
        }
        .into(),
        set: plan.set.into(),
        report: plan.report.clone(),
        errors: plan.errors,
        labels: plan.labels_mode.into(),
        judgments: (plan.seen.len()) as u32,
        called: stored + rebuilt,
        stored,
        rebuilt,
        rebanded,
        left_out: plan.left_out.clone(),
        estimate_usd: micros_to_usd(estimate),
        limit_usd: micros_to_usd(caller.limit),
        cost_usd: micros_to_usd(caller.spent),
        agreement: (!cand_of.is_empty()).then(|| f64::from(agree) / cand_of.len() as f64),
        incumbent_report,
        candidate_report,
        per_judgment,
        fixed,
        broken,
        fell,
        eval,
    }
}

fn top_of(a: &AnswerRecord) -> &Top {
    &a.band.top
}

fn outcome_word(o: &Outcome) -> String {
    match o {
        Outcome::Answered => "answered".into(),
        Outcome::Skipped { reason } => serde_json::to_value(reason)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "skipped".into()),
        Outcome::Failed { class, .. } => class.clone(),
    }
}

/// Per question and Choice class, whether the candidate's precision or
/// recall fell below the incumbent's.
pub fn fell(inc: &[QuestionReport], cand: &[QuestionReport]) -> Vec<ReplayClassFell> {
    let lower =
        |a: Option<f64>, b: Option<f64>| matches!((a, b), (Some(a), Some(b)) if b + 1e-9 < a);
    let mut out = Vec::new();
    for q in inc {
        let Some(c) = cand.iter().find(|c| c.question == q.question) else {
            continue;
        };
        for k in &q.classes {
            let Some(ck) = c.classes.iter().find(|x| x.class == k.class) else {
                continue;
            };
            let (pf, rf) = (lower(k.precision, ck.precision), lower(k.recall, ck.recall));
            if pf || rf {
                out.push(ReplayClassFell {
                    question: q.question.clone(),
                    class: k.class.clone(),
                    precision_fell: pf,
                    recall_fell: rf,
                });
            }
        }
    }
    out
}

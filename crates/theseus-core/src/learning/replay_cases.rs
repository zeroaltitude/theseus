//! `theseus judge replay --cases` (theseus-h66v4, row 1): a pack's labelled
//! cases, asked of Jev through the judge's client and graded against the
//! labels (`theseus_judge::route_cases`'s pure half). The cases are written
//! down (route.v10's 58, from the owner's labels and the design), not
//! recorded judgments, so no ledger judgment is read and no incumbent is
//! named: the run is the candidate beside its own labels.
//!
//! - **The candidate** is `candidate_of`'s: an embedded version by name, a pack
//!   file's text, or, when neither is given, the pack the cases file says it
//!   is written for. A wired version is refused as `replay` refuses it: a
//!   candidate is one this build does not dispatch.
//! - **The run** is [`replay`](super::replay)'s: each call a `judge.call` row
//!   (`purpose: replay`) scoped `judge.replay:<pack id>`, the run a
//!   `judge.replay` row in the same frame, priced and reserved against
//!   `[judge] replay_limit_usd` (refused with the numbers before any call),
//!   never the shadow day budget. A run cut short by the limit grades the
//!   cases it reached and leaves out every one after, each with the limit's
//!   words.
//! - **Spend and consent**: the cases are written, and a labelled message from
//!   the owner's own history was already sent to Jev when it was judged, so no
//!   consent key is needed, as `--onto`'s is, over recorded history. The run
//!   is the owner's, from a private place (`judge_act`).

use std::sync::Arc;

use anyhow::{bail, Context as _};
use serde_json::json;
use theseus_judge::price::{micros_to_usd, usd_to_micros};
use theseus_judge::route_cases::{self, CaseSet, Check, Graded};
use theseus_judge::{Judgment, Outcome};
use theseus_protocol::judge_runs::{
    JudgeReplayParams, JudgeReplayResult, ReplayCase, ReplayCases, ReplayCheck, ReplayLeftOut,
};

use super::replay::{candidate_of, Caller};
use crate::rpc::Core;

/// What `--cases` refuses, since its run would ignore it: a recorded set (a
/// report, ids, errors, a split) or `--onto`. `candidate` and `pack_text` it
/// honours.
pub(crate) fn ignored_with_cases(p: &JudgeReplayParams) -> Option<String> {
    let (flag, what) = if p.onto {
        ("--onto", "replays the import's labelled sessions")
    } else if p.report.is_some() {
        ("--report", "names a report whose holdout is the set")
    } else if p.split.is_some() {
        ("--split", "is for a report's holdout")
    } else if p.errors {
        (
            "--errors",
            "is for a report's judgments the incumbent got wrong",
        )
    } else if !p.judgments.is_empty() {
        ("--judgments", "names judgments by id")
    } else {
        return None;
    };
    Some(format!(
        "--cases replays the cases file's own labels; {flag} {what} and is not used with it. \
         Nothing was sent."
    ))
}

fn check(c: &Check) -> ReplayCheck {
    ReplayCheck {
        want: c.want.clone(),
        got: c.got.clone(),
        confidence: c.confidence,
        agrees: c.agrees,
        close: c.close,
        raw: c.raw.clone(),
        confirm: c.confirm,
        sub_confirm: c.sub_confirm,
        raw_agrees: c.raw_agrees,
        raw_close: c.raw_close,
    }
}

/// The wire report: each case, the totals, and the bar.
fn report(pack: &str, graded: &[Graded]) -> ReplayCases {
    let t = route_cases::totals(graded);
    ReplayCases {
        pack: pack.to_string(),
        cases: graded
            .iter()
            .map(|g| ReplayCase {
                id: g.id.clone(),
                must: g.must,
                answered: g.answered,
                model: check(&g.model),
                effort: check(&g.effort),
            })
            .collect(),
        answered: t.answered,
        model_ok: t.model_ok,
        effort_exact: t.effort_exact,
        effort_close: t.effort_close,
        must_total: t.must_total,
        must_ok: t.must_ok,
        must_under_confirm: t.must_under_confirm,
        raw_model_ok: t.raw_model_ok,
        raw_effort_exact: t.raw_effort_exact,
        raw_effort_close: t.raw_effort_close,
        held_elsewhere: route_cases::PINNED,
        bar: t.bar,
    }
}

impl Core {
    /// `judge.replay { cases }`: the owner's run, from a private place, on a
    /// learning thread as `judge_replay`'s are.
    pub(super) async fn cases_replay_run(
        self: &Arc<Self>,
        p: JudgeReplayParams,
        who: crate::approval::Answerer,
    ) -> anyhow::Result<JudgeReplayResult> {
        if let Some(why) = ignored_with_cases(&p) {
            bail!("{why}");
        }
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::JUDGE_REPLAY,
                what: "replay of a pack over a cases file",
            },
        )?;
        let core = Arc::downgrade(self);
        let rt = tokio::runtime::Handle::current();
        let (who_s, via) = (who.who(), who.via());
        let rx = super::tender::on_low_thread(move || match core.upgrade() {
            Some(c) => c.cases_replay(&rt, &p, &who_s, &via),
            None => Err(anyhow::anyhow!("the daemon stopped before the replay ran")),
        })?;
        rx.await.context("the replay's thread ended")?.1
    }

    /// The run itself: see the module's doc.
    pub(crate) fn cases_replay(
        &self,
        rt: &tokio::runtime::Handle,
        p: &JudgeReplayParams,
        who: &str,
        via: &str,
    ) -> anyhow::Result<JudgeReplayResult> {
        let _rt = rt.enter();
        let text = p.cases.as_deref().context("no cases file was sent")?;
        let mut named = p.clone();
        if named.candidate.is_none() && named.pack_text.is_none() {
            named.candidate = Some(route_cases::declared_pack(text).map_err(anyhow::Error::msg)?);
        }
        let (pack, pack_text) = candidate_of(&named)?;
        if crate::judge::WIRED.iter().any(|(n, _)| *n == pack.name()) {
            bail!(
                "{} is wired: a candidate is a version this build does not dispatch",
                pack.name()
            );
        }
        let set = CaseSet::parse(text, &pack).map_err(anyhow::Error::msg)?;
        let id = crate::new_id("rpl");
        let context = json!({"purpose": "replay", "run": id});
        let asks: Vec<_> = set
            .cases
            .iter()
            .map(|c| {
                let mut a = route_cases::ask(&pack, c, &context);
                a.id = Some(theseus_judge::new_id());
                a
            })
            .collect();
        let built = self.runner.judge.jev()?;
        let mut caller = Caller::new(rt, built, usd_to_micros(self.cfg.judge.replay_limit_usd))
            .with_ceiling(&self.runner);
        let estimate = caller.estimate(asks.iter())?;
        if estimate > caller.limit {
            bail!(
                "the replay of {} over {} cases would reserve {}, past [judge] replay_limit_usd \
                 ({}); nothing was sent",
                pack.name(),
                set.cases.len(),
                crate::narrative::dollars(estimate),
                crate::narrative::dollars(caller.limit)
            );
        }
        let mut judgments: Vec<Judgment> = Vec::new();
        for a in asks {
            match caller.ask(a) {
                Some(j) => judgments.push(j),
                None => break,
            }
        }
        let reached = judgments.len();
        let why = caller.left_reason();
        let left_out: Vec<ReplayLeftOut> = set.cases[reached..]
            .iter()
            .map(|c| ReplayLeftOut {
                judgment: c.id.clone(),
                reason: why.clone(),
            })
            .collect();
        let graded: Vec<Graded> = set
            .cases
            .iter()
            .enumerate()
            .map(|(i, c)| route_cases::grade(&pack, c, judgments.get(i)))
            .collect();
        let result = JudgeReplayResult {
            id,
            candidate: pack.name(),
            candidate_sha256: pack.sha256.clone(),
            incumbent: pack.name(),
            change: "asks".into(),
            set: "cases".into(),
            labels: "cases".into(),
            judgments: set.cases.len() as u32,
            called: judgments
                .iter()
                .filter(|j| j.outcome == Outcome::Answered)
                .count() as u32,
            left_out,
            estimate_usd: micros_to_usd(estimate),
            limit_usd: micros_to_usd(caller.limit),
            cost_usd: micros_to_usd(caller.spent),
            cases: Some(report(&pack.name(), &graded)),
            ..JudgeReplayResult::default()
        };
        self.write_onto_replay(&pack.id, &pack_text, &result, &caller.called, who, via)?;
        Ok(result)
    }
}

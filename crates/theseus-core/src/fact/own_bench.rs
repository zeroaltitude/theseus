//! Your own benchmark's facts (own-benchmark, theseus-pw1q.12): a replay of a
//! turn's first request (`turn.replayed`) and the nightly sampler's judged
//! replay (`route.replay.judged`). Each is a row and a sentence; neither
//! carries a reply's text.

use serde_json::{json, Value};
use theseus_protocol::NarrativePart::Model;
use theseus_protocol::{own_bench::TurnReplayResult, LedgerKind};

use super::{Fact, Say};
use crate::narrative;

/// A turn's first request, rebuilt and sent to another profile. Written by
/// `turn.replay` and by the sampler; it is the only thing a replay writes.
pub struct TurnReplayed<'a> {
    pub by: &'a str,
    pub result: &'a TurnReplayResult,
    /// The model the original ran on and its first loop's cost.
    pub from_model: &'a str,
    /// The effort it ran at; empty: the profile's own.
    pub from_effort: &'a str,
}

impl Fact for TurnReplayed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TurnReplayed);

    fn row(&self) -> Value {
        let r = self.result;
        json!({
            "by": self.by,
            "profile": r.profile,
            "model": r.replay.model,
            "effort": r.effort,
            "scope": r.scope,
            "request_digest": r.request_digest,
            "from_model": self.from_model,
            "from_cost_usd": r.original.cost_usd,
            "input_tokens": r.replay.input_tokens,
            "output_tokens": r.replay.output_tokens,
            "replay_cost_usd": r.replay.cost_usd,
            "cost_usd": r.cost_usd,
            "elapsed_ms": r.elapsed_ms,
            "tool_calls": r.replay.tool_calls.len(),
            "verdict": r.verdict.as_ref().map(|v| v.verdict.as_str()),
            "same_or_better": r.verdict.as_ref().map(|v| v.same_or_better),
            "from_effort": self.from_effort,
            "whole": r.whole.as_ref().map(|w| json!({
                "loops": w.loops, "capped": w.capped,
                "calls": w.calls.iter().map(|c| json!({"tool": c.tool, "how": c.how, "exit": c.exit})).collect::<Vec<_>>(),
                "stubs": w.calls.iter().filter(|c| c.how == "stub").map(|c| c.tool.as_str()).collect::<Vec<_>>(),
                "changed": w.changed, "original_changed": w.original_changed,
                "same": {"tests": w.same.tests, "files": w.same.files, "exits": w.same.exits},
                "caveats": w.caveats, "label": w.label,
            })),
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.result;
        say.line(
            Model,
            format!(
                "Replayed turn {}'s {} on {} ({}): {} out, {}.",
                narrative::short(&r.turn),
                if r.whole.is_some() {
                    "whole turn"
                } else {
                    "first request"
                },
                r.profile,
                r.replay.model,
                narrative::count(r.replay.output_tokens, "token", "tokens"),
                narrative::money(Some(r.cost_usd)),
            ),
        );
    }
}

/// The sampler's result for one turn, judged: what the cell was, where it
/// was replayed, what `compare.v1` said, and what it cost.
pub struct RouteReplayJudged<'a> {
    pub session: &'a str,
    pub turn: &'a str,
    pub from_model: &'a str,
    pub from_effort: &'a str,
    pub to_profile: &'a str,
    pub to_model: &'a str,
    pub to_effort: &'a str,
    pub verdict: Option<&'a str>,
    pub p: Option<f64>,
    pub same_or_better: Option<f64>,
    pub judgment: Option<&'a str>,
    /// Why there is no verdict.
    pub unjudged: Option<&'a str>,
    pub orig_usd: f64,
    pub replay_usd: f64,
    pub judge_usd: f64,
}

impl Fact for RouteReplayJudged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RouteReplayJudged);

    fn row(&self) -> Value {
        json!({
            "turn": self.turn,
            "from_model": self.from_model, "from_effort": self.from_effort,
            "to_profile": self.to_profile, "to_model": self.to_model, "to_effort": self.to_effort,
            "verdict": self.verdict, "p": self.p, "same_or_better": self.same_or_better,
            "judgment": self.judgment, "unjudged": self.unjudged,
            "orig_usd": self.orig_usd, "replay_usd": self.replay_usd, "judge_usd": self.judge_usd,
            "by": crate::own_bench::BY_SAMPLER,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "Replayed turn {} one rung down, {} to {}: {}.",
                narrative::short(self.turn),
                self.from_model,
                self.to_model,
                match (self.verdict, self.unjudged) {
                    (Some(v), _) => format!("compare.v1 says {v}"),
                    (None, Some(why)) => format!("not judged ({why})"),
                    (None, None) => "not judged".into(),
                }
            ),
        );
    }
}

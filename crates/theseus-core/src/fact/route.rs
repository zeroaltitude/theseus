//! Routing's fact (M5 25e; `crate::routing`): where a person's message's
//! turn ran, and why. Its row rides in the turn's next frame; its mark sits
//! in the turn's trace, under the first loop.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Model;

use super::{Fact, Say};
use crate::trace::Trace;

/// `route.decided`: the mode `route.v1` answered (if a verdict was read),
/// the profile the turn runs on and the one it came from, the wait after
/// the first compile, and the reason; whether the verdict missed the wait
/// (`late`: the turn ran on the session's last verdict or its base), and
/// when `route.v1`'s request came back, in ms after the turn's start, if it
/// had by then (theseus-ddbi). Since route.v3 (theseus-qe3v), the effort it
/// answered and its confidence, why the turn ran at its effort
/// (`effort_reason`), whether Jev's answer set it (`effort_applied`), and the
/// effort the request carries (`effort_ran`: Jev's, or the profile's own;
/// none for the model's default or a model that takes none).
pub struct RouteDecided<'a> {
    pub mode: Option<&'a str>,
    pub confidence: Option<f64>,
    pub judgment: Option<&'a str>,
    pub from: &'a str,
    pub profile: &'a str,
    pub reason: &'a str,
    pub detour: bool,
    pub switch: bool,
    pub est_tokens: u64,
    pub wait_ms: u64,
    pub late: bool,
    pub answered_ms: Option<u64>,
    /// `correction`: the owner's correction, or the layer's entry close to
    /// this message, placed the turn ahead of the verdict (theseus-q31l).
    pub source: Option<&'a str>,
    /// The owner's label (or correction) it followed.
    pub follows: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub effort_confidence: Option<f64>,
    pub effort_reason: Option<&'a str>,
    pub effort_applied: bool,
    pub effort_ran: Option<&'a str>,
    /// The effort the request carried when `sticky` held it at the previous
    /// request's (`effort_reason`): `effort_ran` then is Jev's level.
    pub effort_sent: Option<&'a str>,
    /// A running task's checkpoint (theseus-ypnxt.9): the row's `source` is
    /// `task checkpoint`, and this says why it was asked, where, and what a
    /// switch cost.
    pub checkpoint: Option<Checkpointed<'a>>,
    /// What the turn injects (theseus-d9je7): its tier and the sizes it gave;
    /// absent on a checkpoint's row.
    pub injection: Option<Value>,
    /// Where the profile runs, and its origin (theseus-a1wvq.11).
    pub ran: Ran<'a>,
    /// The data rule's part: absent on a checkpoint's row.
    pub data: Option<RouteData<'a>>,
    /// What route.v10 adds (theseus-h66v4): absent while another pack acts, so
    /// the row is byte for byte what it was.
    pub open: Option<RouteOpen>,
}

/// route.v10's part of a `route.decided` row: the acting pack, what Jev picked
/// and how sure, the first sentences of the block it picked, the context it
/// named, and the words that qualify the reason (`-P fable`,
/// `restricted_models=not_allowed`, `deepseek: no key`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteOpen {
    pub pack: String,
    pub pick: Option<String>,
    pub pick_confidence: Option<f64>,
    pub block: Option<String>,
    pub context: Option<String>,
    pub context_confidence: Option<f64>,
    pub detail: Option<String>,
}

/// The provider, model and origin of the profile the row's turn runs on: a
/// profile that does not resolve has no provider or model and origin `unknown`.
pub struct Ran<'a> {
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub origin: &'a str,
}

/// The data rule on a turn's route row. `gate` is an open word: why the rule
/// kept the turn off a restricted-origin profile, written only when it did.
pub struct RouteData<'a> {
    /// `allowed`, a flag's word, or `never` (`cn_models`): whether the session may
    /// use a model of restricted origin, as the turn found its labels.
    pub cn: &'a str,
    pub gate: Option<&'a str>,
    /// Why a class with `cn_draft` did not draft (theseus-a1wvq.13).
    pub draft_skip: Option<&'a str>,
}

/// What a task checkpoint's `route.decided` row adds to the turn's own.
pub struct Checkpointed<'a> {
    /// `failing`, `quiet`, `phase` or `report`.
    pub trigger: &'a str,
    /// The loop the verdict was applied at the boundary of.
    pub loop_index: u32,
    /// On a switch: the compile's estimate less the warm prefix on the new
    /// model, and what writing those tokens cold costs at its catalog prices.
    pub cold: Option<(u64, f64)>,
    /// What the checkpoint's judgment settled at, in dollars: charged to the task.
    pub cost_usd: f64,
}

impl Fact for RouteDecided<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RouteDecided);

    fn row(&self) -> Value {
        let mut row = json!({
            "mode": self.mode, "confidence": self.confidence, "judgment": self.judgment,
            "from": self.from, "profile": self.profile, "reason": self.reason,
            "detour": self.detour, "switch": self.switch, "est_tokens": self.est_tokens,
            "wait_ms": self.wait_ms, "late": self.late, "answered_ms": self.answered_ms,
            "effort": self.effort, "effort_confidence": self.effort_confidence,
            "effort_reason": self.effort_reason, "effort_applied": self.effort_applied,
            "effort_ran": self.effort_ran,
        });
        row["provider"] = json!(self.ran.provider);
        row["model"] = json!(self.ran.model);
        row["origin"] = json!(self.ran.origin);
        if let Some(d) = &self.data {
            row["cn"] = json!(d.cn);
            if let Some(g) = d.gate {
                row["data_gate"] = json!(g);
            }
            if let Some(w) = d.draft_skip {
                row["draft_skip"] = json!(w);
            }
        }
        if let Some(s) = self.effort_sent {
            row["effort_sent"] = json!(s);
        }
        if let Some(o) = &self.open {
            row["pack"] = json!(o.pack);
            for (k, v) in [
                ("pick", o.pick.as_ref().map(|s| json!(s))),
                ("pick_confidence", o.pick_confidence.map(|c| json!(c))),
                ("block", o.block.as_ref().map(|s| json!(s))),
                ("context", o.context.as_ref().map(|s| json!(s))),
                ("context_confidence", o.context_confidence.map(|c| json!(c))),
                ("detail", o.detail.as_ref().map(|s| json!(s))),
            ] {
                if let Some(v) = v {
                    row[k] = v;
                }
            }
        }
        if let Some(i) = &self.injection {
            row["injection"] = i.clone();
        }
        if let Some(s) = self.source {
            row["source"] = json!(s);
            if self.follows.is_some() {
                row["follows"] = json!(self.follows);
            }
        }
        if let Some(c) = &self.checkpoint {
            row["trigger"] = json!(c.trigger);
            row["loop"] = json!(c.loop_index);
            row["cost_usd"] = json!(c.cost_usd);
            if let Some((tokens, usd)) = c.cold {
                row["cold_tokens"] = json!(tokens);
                row["cache_usd"] = json!(usd);
            }
        }
        row
    }

    fn span(&self, trace: &mut Trace) {
        trace.mark("route", "mark", self.row());
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let mode = match self.source {
            Some(s) => match self.follows {
                Some(f) => format!(" ({s}, following {f})"),
                None => format!(" ({s})"),
            },
            None => self.mode.map_or(String::new(), |m| format!(" ({m})")),
        };
        let effort = match (self.effort_applied, self.effort_ran, self.effort_sent) {
            (true, Some(e), Some(s)) => format!(" Jev asked for effort {e}; it stays at {s}."),
            (true, Some(e), None) => format!(" Jev set its effort to {e}."),
            _ => String::new(),
        };
        let line = match (self.detour, self.switch) {
            (_, true) if self.checkpoint.is_some() => {
                let c = self.checkpoint.as_ref().expect("checked");
                let (source, what) = match self.source {
                    Some(s) if s.starts_with("turn checkpoint") => {
                        ("turn checkpoint", "conversation")
                    }
                    _ => ("task checkpoint", "task"),
                };
                format!(
                    "Routing ({source}: {}): the {what} moves from {} to {}.",
                    c.trigger, self.from, self.profile
                )
            }
            (true, _) => format!(
                "Routing{mode}: this message alone runs on {}; the session stays on {}.",
                self.profile, self.from
            ),
            (_, true) => format!(
                "Routing{mode}: the session moves from {} to {}.",
                self.from, self.profile
            ),
            _ => format!(
                "Routing{mode}: the turn stays on {} ({}).",
                self.profile, self.reason
            ),
        };
        let gate = match self.data.as_ref().and_then(|d| d.gate) {
            Some(g) => format!(" Kept off a restricted-origin model: {g}."),
            None => String::new(),
        };
        say.line(Model, format!("{line}{effort}{gate}"));
    }
}

/// `route.corrected` (theseus-q31l): the owner's correction of a turn's
/// routing, keyed `rcx_<id>` and scoped `route.corrections`, in one frame
/// with the owner's label on the turn's route judgment, when it wrote one.
/// Its provenance is the owner's own message, reaction, or press, by id. The
/// live layer is rebuilt from these rows after serving.
pub struct RouteCorrected<'a> {
    pub id: &'a str,
    /// The corrected turn, and its route judgment, when it had one.
    pub turn: &'a str,
    pub judgment: Option<&'a str>,
    /// The route pack version that judged it.
    pub pack: &'a str,
    pub session: &'a str,
    /// The verdict's mode, and where the turn ran.
    pub mode: Option<&'a str>,
    pub ran: &'a str,
    /// What the owner said (`fable`, `mode deep_coding`, `stronger`).
    pub to: &'a str,
    /// Where the session runs from its next turn, if it moves.
    pub profile: Option<&'a str>,
    /// Where a close message runs: the layer's entry, when it steers.
    pub steer: Value,
    /// The owner's label, when the mode was wrong.
    pub label: Option<&'a str>,
    pub rerun: bool,
    pub who: &'a str,
    /// `message`, `reaction`, `button`, `cockpit`, `cli`.
    pub via: &'a str,
    /// The message's node, the reaction, or the press, by id.
    pub provenance: &'a str,
    /// The corrected message's content words, which a close one shares.
    pub words: &'a [String],
    pub at_ms: u64,
}

impl Fact for RouteCorrected<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RouteCorrected);

    fn row(&self) -> Value {
        json!({
            "id": self.id, "turn": self.turn, "judgment": self.judgment, "pack": self.pack,
            "session": self.session, "mode": self.mode, "ran": self.ran, "to": self.to,
            "profile": self.profile, "steer": self.steer, "label": self.label,
            "rerun": self.rerun, "who": self.who, "via": self.via,
            "provenance": self.provenance, "words": self.words, "at_ms": self.at_ms,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let moved = match self.profile {
            Some(p) => format!("; the session runs on {p} from its next turn"),
            None => "; there is nowhere to move it".into(),
        };
        let label = match (self.label, self.mode) {
            (Some(l), Some(m)) => format!(
                ", labeling {} {} not {m} ({l})",
                self.pack,
                self.judgment.unwrap_or("?")
            ),
            _ => String::new(),
        };
        let again = if self.rerun {
            ", and answers it again there"
        } else {
            ""
        };
        say.line(
            Model,
            format!(
                "{} corrected turn {}'s routing (it ran on {}): {}{label}{moved}{again} (by {} {}).",
                self.who, self.turn, self.ran, self.to, self.via, self.provenance
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::narrative::Narrator;

    /// The line a checkpoint's switch narrates, from a source of `source`.
    fn moved(source: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
        let narrator = Narrator::new(true);
        let rec = super::super::Rec {
            narrator: &narrator,
            session: Some("ses_a"),
            turn: None,
            to: super::super::To::Nobody,
            store: &store,
        };
        rec.announce(&RouteDecided {
            mode: None,
            confidence: None,
            judgment: None,
            from: "sonnet",
            profile: "fable",
            reason: "switch",
            detour: false,
            switch: true,
            est_tokens: 0,
            wait_ms: 0,
            late: false,
            answered_ms: None,
            source: Some(source),
            follows: None,
            effort: None,
            effort_confidence: None,
            effort_reason: None,
            effort_applied: false,
            effort_ran: None,
            effort_sent: None,
            checkpoint: Some(Checkpointed {
                trigger: "failing",
                loop_index: 3,
                cold: None,
                cost_usd: 0.0,
            }),
            injection: None,
            ran: Ran {
                provider: None,
                model: None,
                origin: "unknown",
            },
            data: None,
            open: None,
        });
        let lines = narrator.tail();
        assert_eq!(lines.len(), 1);
        lines[0].text.clone()
    }

    /// A turn checkpoint moves the conversation, a task checkpoint the task (theseus-1qexl:
    /// before it, both said "the turn moves").
    #[test]
    fn a_checkpoints_move_names_what_moves() {
        assert_eq!(
            moved("turn checkpoint"),
            "Routing (turn checkpoint: failing): the conversation moves from sonnet to fable."
        );
        assert_eq!(
            moved("task checkpoint"),
            "Routing (task checkpoint: failing): the task moves from sonnet to fable."
        );
    }
}

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
/// had by then (theseus-ddbi).
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
}

impl Fact for RouteDecided<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RouteDecided);

    fn row(&self) -> Value {
        let mut row = json!({
            "mode": self.mode, "confidence": self.confidence, "judgment": self.judgment,
            "from": self.from, "profile": self.profile, "reason": self.reason,
            "detour": self.detour, "switch": self.switch, "est_tokens": self.est_tokens,
            "wait_ms": self.wait_ms, "late": self.late, "answered_ms": self.answered_ms,
        });
        if let Some(s) = self.source {
            row["source"] = json!(s);
            row["follows"] = json!(self.follows);
        }
        row
    }

    fn span(&self, trace: &mut Trace) {
        trace.mark("route", "mark", self.row());
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let mode = match self.source {
            Some(s) => format!(" ({s}, following {})", self.follows.unwrap_or("?")),
            None => self.mode.map_or(String::new(), |m| format!(" ({m})")),
        };
        let line = match (self.detour, self.switch) {
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
        say.line(Model, line);
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

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
    pub effort: Option<&'a str>,
    pub effort_confidence: Option<f64>,
    pub effort_reason: Option<&'a str>,
    pub effort_applied: bool,
    pub effort_ran: Option<&'a str>,
}

impl Fact for RouteDecided<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RouteDecided);

    fn row(&self) -> Value {
        json!({
            "mode": self.mode, "confidence": self.confidence, "judgment": self.judgment,
            "from": self.from, "profile": self.profile, "reason": self.reason,
            "detour": self.detour, "switch": self.switch, "est_tokens": self.est_tokens,
            "wait_ms": self.wait_ms, "late": self.late, "answered_ms": self.answered_ms,
            "effort": self.effort, "effort_confidence": self.effort_confidence,
            "effort_reason": self.effort_reason, "effort_applied": self.effort_applied,
            "effort_ran": self.effort_ran,
        })
    }

    fn span(&self, trace: &mut Trace) {
        trace.mark("route", "mark", self.row());
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let mode = self.mode.map_or(String::new(), |m| format!(" ({m})"));
        let effort = match (self.effort_applied, self.effort_ran) {
            (true, Some(e)) => format!(" Jev set its effort to {e}."),
            _ => String::new(),
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
        say.line(Model, format!("{line}{effort}"));
    }
}

//! A conversation's mode or plan changed (theseus-lyxy): one ledger row
//! (`session.mode`) with the mode and the steps as they stand, which a start
//! reads back, one notification to the session's watchers, and a line of the
//! narrative.

use serde_json::{json, Value};
use theseus_protocol::modes::{plan_line, PlanStep, SessionMode, SessionModeChanged};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, LedgerKind};

use super::{Fact, Say};

pub struct ModeChanged<'a> {
    pub session_id: &'a str,
    pub mode: SessionMode,
    /// The mode before, when the mode itself moved.
    pub previous: Option<SessionMode>,
    pub steps: &'a [PlanStep],
    pub by: &'a str,
}

impl Fact for ModeChanged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SessionMode);
    const METHOD: Option<&'static str> = Some(notify::SESSION_MODE);

    fn row(&self) -> Value {
        json!({"mode": self.mode, "previous": self.previous, "steps": self.steps, "by": self.by})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::SessionMode(SessionModeChanged {
            session_id: self.session_id.into(),
            mode: self.mode,
            steps: self.steps.to_vec(),
            by: self.by.into(),
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let plan = plan_line(self.steps).map_or(String::new(), |p| format!(", {p}"));
        say.line(
            Session,
            format!(
                "Mode: {} is {}{plan} ({}).",
                self.session_id,
                self.mode.name(),
                self.by
            ),
        );
    }
}

//! A conversation's mode or plan changed (theseus-lyxy).
//!
//! - `ModeChanged`: the mode moved. One ledger row (`session.mode`, the mode
//!   alone: a plan is the task graph's items, not a field of this row), which
//!   a start reads back, one notification to the session's watchers, and a
//!   line of the narrative.
//! - `PlanMoved`: a plan item of the conversation changed (a plan approved,
//!   a step marked, an item closed). Its row is the task graph's own
//!   (`task.*`), so it has none; it is the same `session.mode` notification,
//!   carrying the items as they stand, so every surface redraws its
//!   checklist from the one view.

use serde_json::{json, Value};
use theseus_protocol::modes::{plan_line, PlanItem, SessionMode, SessionModeChanged};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, LedgerKind};

use super::{Fact, Say};

pub struct ModeChanged<'a> {
    pub session_id: &'a str,
    pub mode: SessionMode,
    /// The mode before, when the mode itself moved.
    pub previous: Option<SessionMode>,
    pub plan: &'a [PlanItem],
    pub by: &'a str,
}

impl Fact for ModeChanged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SessionMode);
    const METHOD: Option<&'static str> = Some(notify::SESSION_MODE);

    fn row(&self) -> Value {
        json!({"mode": self.mode, "previous": self.previous, "by": self.by})
    }

    fn event(&self) -> Option<Event> {
        Some(event(self.session_id, self.mode, self.plan, self.by))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let plan = plan_line(self.plan).map_or(String::new(), |p| format!(", {p}"));
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

/// A plan item of the conversation changed: the notification alone.
pub struct PlanMoved<'a> {
    pub session_id: &'a str,
    pub mode: SessionMode,
    pub plan: &'a [PlanItem],
    pub by: &'a str,
}

impl Fact for PlanMoved<'_> {
    const METHOD: Option<&'static str> = Some(notify::SESSION_MODE);

    fn event(&self) -> Option<Event> {
        Some(event(self.session_id, self.mode, self.plan, self.by))
    }
}

fn event(session_id: &str, mode: SessionMode, plan: &[PlanItem], by: &str) -> Event {
    Event::SessionMode(SessionModeChanged {
        session_id: session_id.into(),
        mode,
        plan: plan.to_vec(),
        by: by.into(),
    })
}

//! Modes (theseus-lyxy): how much a conversation asks, for the conversation
//! alone. `ask` is the config's postures; `edits` lets the file tools through
//! under the session's directory; `plan` is read-only until the person
//! approves a plan; `auto` runs what only the posture asked, with a notice.
//! A mode never passes the floor, the approve lists, the place rule or L1's
//! own asks, and a shared place has none but `ask`.
//!
//! The plan a person approves is recorded on the session as `PlanStep`s
//! (`{text, state}`, the shape of a task graph's plan item: one notion of a
//! plan), and `plan_step` moves them. Both ride one notification,
//! `session.mode`, so every surface shows the mode and the checklist from the
//! same words, written here once.

use serde::{Deserialize, Serialize};

/// A conversation's mode. A new conversation starts in `ask`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    /// The config's postures, as before modes.
    #[default]
    Ask,
    /// File edits under the session's directory run with a notice.
    Edits,
    /// Read-only: writers and `proc.run` are refused, and `plan_present`
    /// puts a plan to the person.
    Plan,
    /// What only the tool's posture made wait runs with a notice.
    Auto,
}

impl SessionMode {
    /// Shift+Tab's order.
    pub const CYCLE: [SessionMode; 4] = [Self::Ask, Self::Edits, Self::Plan, Self::Auto];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Edits => "edits",
            Self::Plan => "plan",
            Self::Auto => "auto",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::CYCLE.into_iter().find(|m| m.name() == s)
    }

    /// The mode Shift+Tab moves to.
    pub fn next(self) -> Self {
        let i = Self::CYCLE.iter().position(|m| *m == self).unwrap_or(0);
        Self::CYCLE[(i + 1) % Self::CYCLE.len()]
    }

    /// The status line's words: `edits (shift+tab)`.
    pub fn status(self) -> String {
        format!("{} (shift+tab)", self.name())
    }

    /// What the mode does, in words every surface shows beside its name. The
    /// words `accepting edits` and `planning` are what F10's S5c and S12a read.
    pub fn effect(self) -> &'static str {
        match self {
            Self::Ask => "asking as the config says",
            Self::Edits => "accepting edits to files in this project, each with a notice",
            Self::Plan => "planning, read-only until you approve a plan",
            Self::Auto => {
                "running what only its posture made wait, with a notice; the floor still asks"
            }
        }
    }
}

/// Where a plan's step stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum PlanState {
    #[default]
    Todo,
    Doing,
    Done,
    Skipped,
}

impl PlanState {
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Todo, Self::Doing, Self::Done, Self::Skipped]
            .into_iter()
            .find(|s2| s2.name() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Done => "done",
            Self::Skipped => "skipped",
        }
    }

    /// The checklist's mark. They are the marks F10's S12c reads: `☐` a step
    /// not begun, `◼` one under way, `✔` one done, `⊘` one skipped.
    pub fn mark(self) -> char {
        match self {
            Self::Todo => '☐',
            Self::Doing => '◼',
            Self::Done => '✔',
            Self::Skipped => '⊘',
        }
    }
}

/// One step of the approved plan, shaped as a task graph's plan item.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PlanStep {
    pub text: String,
    pub state: PlanState,
}

impl PlanStep {
    /// The steps as a plan names them, none begun.
    pub fn todo(texts: &[String]) -> Vec<PlanStep> {
        texts
            .iter()
            .map(|t| PlanStep {
                text: t.clone(),
                state: PlanState::Todo,
            })
            .collect()
    }
}

/// `plan 2/5`: the steps done (or skipped) of all of them; none for no plan.
pub fn plan_line(steps: &[PlanStep]) -> Option<String> {
    if steps.is_empty() {
        return None;
    }
    let done = steps
        .iter()
        .filter(|s| matches!(s.state, PlanState::Done | PlanState::Skipped))
        .count();
    Some(format!("plan {done}/{}", steps.len()))
}

/// The checklist, a line a step: `✔ 1. add the function`.
pub fn checklist(steps: &[PlanStep]) -> Vec<String> {
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{} {}. {}", s.state.mark(), i + 1, s.text))
        .collect()
}

/// A plan as a question shows it: numbered, no marks.
pub fn plan_text(steps: &[String]) -> String {
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {s}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `session.mode`: with a `mode`, set it; without, read it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct SessionModeParams {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub mode: Option<SessionMode>,
}

/// What `session.mode` left.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct SessionModeResult {
    pub session_id: String,
    pub mode: SessionMode,
    /// The mode before, when this call changed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub previous: Option<SessionMode>,
    /// The recorded plan, none before one was approved.
    pub steps: Vec<PlanStep>,
}

impl SessionModeResult {
    /// The line the CLI prints: `edits: edits to files in this project run with a notice`.
    pub fn line(&self) -> String {
        let plan = plan_line(&self.steps).map_or(String::new(), |p| format!(" · {p}"));
        format!("{}: {}{plan}", self.mode.name(), self.mode.effect())
    }
}

/// `session.mode`'s notification: the mode and the plan after a change, to
/// the session's watchers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct SessionModeChanged {
    pub session_id: String,
    pub mode: SessionMode,
    pub steps: Vec<PlanStep>,
    /// Who or what changed it: a surface's label, `plan approved`, or
    /// `plan_step`.
    pub by: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_tab_cycles_ask_edits_plan_auto_and_round() {
        let mut m = SessionMode::Ask;
        let seen: Vec<_> = (0..5)
            .map(|_| {
                let was = m;
                m = m.next();
                was.name()
            })
            .collect();
        assert_eq!(seen, ["ask", "edits", "plan", "auto", "ask"]);
        assert_eq!(SessionMode::Edits.status(), "edits (shift+tab)");
    }

    #[test]
    fn a_plan_reads_as_a_line_and_a_checklist() {
        let mut steps = PlanStep::todo(&["add it".into(), "test it".into(), "run all".into()]);
        assert_eq!(plan_line(&steps).as_deref(), Some("plan 0/3"));
        steps[0].state = PlanState::Done;
        steps[1].state = PlanState::Doing;
        assert_eq!(plan_line(&steps).as_deref(), Some("plan 1/3"));
        assert_eq!(
            checklist(&steps),
            ["✔ 1. add it", "◼ 2. test it", "☐ 3. run all"]
        );
        assert_eq!(plan_line(&[]), None);
    }
}

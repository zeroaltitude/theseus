//! A check task's basis (M5 step 28a, theseus-vug.3): why a task that checks
//! another task's work is independent of it. `task.create { check_of }`
//! records it on the check's session, and a task's surfaces show it
//! (`task.list`, `theseus tasks`, the report's post, the cockpit's task view).

use serde::{Deserialize, Serialize};

/// What a check task was admitted, what it was kept from, and on what model.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCheck {
    /// The task it checks: its session's id, and how people name it.
    pub checked_task: String,
    pub checked_short: String,
    /// The checked task's report: its last message, in its own session,
    /// which the check reads as a claim, and when it was written.
    pub report_node: String,
    pub report_at_ms: u64,
    /// The sessions none of whose other nodes reach the check: the checked
    /// task's.
    pub excluded_sessions: Vec<String>,
    /// What the check reads after its brief, in order.
    pub admitted: Vec<CheckPiece>,
    /// What the check runs on: the call's `profile`, or its parent's.
    pub profile: String,
    pub provider: String,
    pub model: String,
    /// The spans of 12 words or more that its brief or its own pieces share
    /// with a node of the checked task's session. The check still runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlaps: Vec<CheckOverlap>,
    pub at_ms: u64,
}

/// One node the check was admitted.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckPiece {
    pub node_id: String,
    pub session_id: String,
    /// `objective` or `acceptance` (the checked task's), a role of the
    /// check's own pieces, or `claim` (the report).
    pub role: String,
    /// `checked`: a piece of the checked task's arrangement; `own`: a piece
    /// the check's call quoted; `claim`: the checked task's report.
    pub from: String,
}

/// A span the check's words share with the checked task's working.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOverlap {
    /// `brief`, or `piece N` (its index in the check's arrangement, from 0).
    pub source: String,
    /// The node of the checked task's session that holds it.
    pub node_id: String,
    /// Its length in words.
    pub words: u32,
    /// The span, as the source says it (cut at 200 characters).
    pub span: String,
}

impl TaskCheck {
    /// `🔍 check of task a1b2c3 · independent (excluded ses_…a1b2c3,
    /// glm-4.6)`, and `· overlap: 2 spans` when any span was flagged.
    pub fn line(&self) -> String {
        let excluded = self
            .excluded_sessions
            .iter()
            .map(|s| short_session(s))
            .collect::<Vec<_>>()
            .join(", ");
        let mut out = format!(
            "🔍 check of task {} · independent (excluded {excluded}, {})",
            self.checked_short, self.model
        );
        match self.overlaps.len() {
            0 => {}
            1 => out.push_str(" · overlap: 1 span"),
            n => out.push_str(&format!(" · overlap: {n} spans")),
        }
        out
    }
}

/// `ses_…a1b2c3`: a session by the end of its id.
fn short_session(id: &str) -> String {
    let tail: String = {
        let n = id.chars().count();
        id.chars().skip(n.saturating_sub(6)).collect()
    };
    match id.split_once('_') {
        Some((kind, _)) => format!("{kind}_…{tail}"),
        None => format!("…{tail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checks_line_names_what_it_excluded_and_its_model() {
        let mut c = TaskCheck {
            checked_short: "a1b2c3".into(),
            excluded_sessions: vec!["ses_0123456789abcdefa1b2c3".into()],
            model: "glm-4.6".into(),
            ..Default::default()
        };
        assert_eq!(
            c.line(),
            "🔍 check of task a1b2c3 · independent (excluded ses_…a1b2c3, glm-4.6)"
        );
        c.overlaps.push(CheckOverlap::default());
        assert!(c.line().ends_with(" · overlap: 1 span"));
        c.overlaps.push(CheckOverlap::default());
        assert!(c.line().ends_with(" · overlap: 2 spans"));
    }
}

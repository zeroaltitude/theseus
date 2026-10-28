//! The stop nudge (theseus-fh0u; the owner's decision D8): a turn that ended
//! with no tool calls whose last message announced work still to come
//! (`loop.v1`'s `announced_unfinished`, in its act band, live) goes on once
//! more after one line in the harness's own voice. The result says so in
//! `nudged`, and every surface shows its `line` as a quiet line above the
//! status line, never as the person's words. `session.nudge` turns it off,
//! or back on, for one session.

use serde::{Deserialize, Serialize};

use crate::TurnSubmitResult;

/// A turn the harness nudged once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnNudge {
    /// The `loop.v1` judgment whose verdict nudged it (`judge.get`).
    pub judgment: String,
    /// The deciding question that acted (`announced_unfinished`).
    pub by: String,
    /// Jev's probability for it.
    pub p: f64,
    /// The loop whose answer stopped.
    #[serde(rename = "loop")]
    pub loop_index: u32,
    /// The model's answer after the nudge made a tool call: false when it
    /// answered in words alone, as a model that was done does.
    #[serde(default)]
    pub acted: bool,
}

impl TurnNudge {
    /// The one wording every surface shows (`cockpit/src/lib/nudge.ts`
    /// mirrors it).
    pub fn line(&self) -> String {
        let then = match self.acted {
            true => "the model went on",
            false => "the model answered without acting",
        };
        format!("Nudged once: the reply said more work would follow; {then}.")
    }
}

impl TurnSubmitResult {
    /// The harness's own quiet lines about the turn, above its status line:
    /// a refusal's fallback (theseus-7gir.18), then a nudge, then the turn's files.
    pub fn harness_lines(&self) -> Vec<String> {
        let fell = self.fallback.as_ref().map(|f| f.line(&self.stop_reason));
        let nudged = self.nudged.as_ref().map(TurnNudge::line);
        fell.into_iter()
            .chain(nudged)
            .chain(self.files_line())
            .chain(self.raised.as_ref().map(crate::route::ToolsRaised::line))
            .collect()
    }
}

/// `session.nudge`: the stop nudge off (`on: false`) or back on for one
/// session. `[judge] stop_nudge` is the daemon's switch; this one only lowers
/// it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionNudgeParams {
    pub session_id: String,
    pub on: bool,
}

/// What `session.nudge` left: the session's own switch, and whether a turn
/// of it would be nudged by the daemon's (`[judge] stop_nudge` and the
/// judge on).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionNudgeResult {
    pub session_id: String,
    pub on: bool,
    pub daemon_on: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::TurnFallback;

    #[test]
    fn a_nudged_turn_says_so_after_its_fallback() {
        let mut r = TurnSubmitResult::default();
        assert!(r.harness_lines().is_empty());
        r.nudged = Some(TurnNudge {
            judgment: "jdg_1".into(),
            by: "announced_unfinished".into(),
            p: 0.97,
            loop_index: 0,
            acted: true,
        });
        assert_eq!(
            r.harness_lines(),
            ["Nudged once: the reply said more work would follow; the model went on."]
        );
        r.fallback = Some(TurnFallback {
            from: "claude-sonnet-5-5".into(),
            to: "claude-sonnet-5".into(),
            category: None,
            answered: true,
            ..Default::default()
        });
        r.nudged.as_mut().unwrap().acted = false;
        let lines = r.harness_lines();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("declined"), "{lines:?}");
        assert!(lines[1].ends_with("the model answered without acting."));
        // theseus-xlri5: the families that raised the request close the lines.
        r.raised = Some(crate::route::ToolsRaised {
            budget: 8_000,
            ceiling: 16_000,
            tokens: 9_612,
            families: vec![
                crate::route::RaisedFamily {
                    family: "aws".into(),
                    tokens: 1_012,
                    by: "named".into(),
                },
                crate::route::RaisedFamily {
                    family: "terminals".into(),
                    tokens: 1_264,
                    by: "jev".into(),
                },
            ],
        });
        assert_eq!(
            r.harness_lines().last().unwrap(),
            "Tools raised the request to 9,612 tokens (budget 8,000, ceiling 16,000): \
             aws +1,012 (named), terminals +1,264 (Jev)."
        );
        let v = serde_json::to_value(r.nudged.unwrap()).unwrap();
        assert_eq!(v["loop"], 0);
    }
}

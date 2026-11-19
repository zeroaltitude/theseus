//! A subagent's answer (issue 28): the words another model profile gave a turn that asked it
//! one question (`profile.ask`), as every place that draws a reply draws them.
//!
//! The owner's rule (2026-11-18, 23:50 MST): a subagent's answer comes back into the
//! conversation "as Theseus, with a subagent footer, and the appropriate model listed in the
//! model part of the footer". So it is a message of its own: the asked model's words as the
//! body, and a footer of a reply's shape whose model part names the profile and the model that
//! ACTUALLY answered (never the model of the turn that asked), that says it is a subagent's
//! answer and who asked, and whose cost and time are the sub-call's own.
//!
//! The answer rides in the call's result node as `meta.subagent`, so the record, the history,
//! the inspector and a live event all read one shape; [`SubagentAnswer::footer_bits`] is the
//! one place the footer's words are chosen, for Discord, the terminal, the terminal UI and the
//! cockpit alike.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The icon a subagent's message opens with, before its first word, in every place a reply is
/// drawn (the owner, 2026-11-19: "a special icon right before the message"). One constant; the
/// cockpit's `lib/subagent.ts` holds the same one, and a test here holds the two together. Not
/// one a reply's own lines use (💭 ✅ 🔔 🧠).
pub const SUBAGENT_ICON: &str = "🛰️";

/// One subagent's answer: its words, and who gave them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct SubagentAnswer {
    /// The profile that was asked, and the provider and the model that answered (a fallback can
    /// differ from the profile's own).
    pub profile: String,
    pub provider: String,
    pub model: String,
    /// The profile of the turn that asked: whose subagent this was.
    pub asked_by: String,
    /// Its words.
    pub text: String,
    /// The call's own cost and time, and the tokens it wrote.
    pub cost_usd: Option<f64>,
    pub elapsed_ms: u64,
    pub output_tokens: u64,
    /// The answer stopped at its cap.
    pub cut: bool,
    /// The call's id: the post's key, so a retry never posts it twice.
    pub call: String,
    /// How the work ended, for an agent that was handed work as a task (`profile.run`): `cancelled` or
    /// `failed`; empty for an answer and for work that was done.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub outcome: String,
}

/// What every surface says of an answer whose turn did not end well (`stopped` or `failed`): the
/// one place the words are chosen, as the footer's bits are. The cockpit's `lib/subagent.ts` holds
/// the same two, and a test here holds them together.
pub fn ended_words(ended: &str) -> &'static str {
    match ended {
        "stopped" => "the turn that asked was stopped",
        _ => "the turn that asked then failed",
    }
}

impl SubagentAnswer {
    /// The line a surface adds under an answer whose turn ended `ended` (`stopped` or `failed`):
    /// the model part, then the words.
    pub fn ended_line(&self, ended: &str) -> String {
        format!("{} · {} · {}", self.profile, self.model, ended_words(ended))
    }

    /// The answer a result node's `meta` holds, when it holds one.
    pub fn from_meta(meta: &Value) -> Option<Self> {
        serde_json::from_value(meta.get("subagent")?.clone()).ok()
    }

    /// Its words as the message opens: the icon, then the first word.
    pub fn opening(&self) -> String {
        format!("{SUBAGENT_ICON} {}", self.text.trim())
    }

    /// The footer's bits, in a reply's order: the model part first (`profile · model`, the ones
    /// that answered), then what it is and who asked, then the call's own cost and time.
    pub fn footer_bits(&self) -> Vec<String> {
        let mut bits = vec![self.profile.clone(), self.model.clone()];
        bits.push(match self.asked_by.is_empty() {
            true => "subagent answer".into(),
            false => format!("subagent answer, asked by {}", self.asked_by),
        });
        if self.cut {
            bits.push("cut at its cap".into());
        }
        if !self.outcome.is_empty() {
            bits.push(self.outcome.clone());
        }
        bits.extend(self.cost_usd.map(crate::cost::usd));
        bits.push(format!("{:.1} s", self.elapsed_ms as f64 / 1000.0));
        bits
    }

    /// The footer as one line of words.
    pub fn footer(&self) -> String {
        self.footer_bits().join(" · ")
    }

    /// The footer in the shape of the terminal's status line for a reply
    /// (`[sonnet → anthropic/claude-sonnet-5-5 · 2 loop(s) · …]`): the model part names the profile,
    /// provider and model that answered, then what it is and who asked, then the call's own cost
    /// and time.
    pub fn status_line(&self) -> String {
        let rest = self.footer_bits().split_off(2).join(" · ");
        format!(
            "[{} → {}/{} · {rest}]",
            self.profile, self.provider, self.model
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn answer() -> SubagentAnswer {
        SubagentAnswer {
            profile: "haiku".into(),
            provider: "anthropic".into(),
            model: "claude-haiku-5-5".into(),
            asked_by: "sonnet".into(),
            text: "Hello.".into(),
            cost_usd: Some(0.0031),
            elapsed_ms: 1_300,
            output_tokens: 12,
            cut: false,
            call: "act_1".into(),
            outcome: String::new(),
        }
    }

    /// The message opens with the icon before its first word, and the cockpit draws the same one.
    #[test]
    fn the_message_opens_with_the_icon_and_the_cockpit_holds_the_same_one() {
        assert_eq!(answer().opening(), "🛰️ Hello.");
        let ts = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../cockpit/src/lib/subagent.ts"
        ))
        .unwrap();
        assert!(
            ts.contains(&format!("SUBAGENT_ICON = '{SUBAGENT_ICON}'")),
            "cockpit/src/lib/subagent.ts holds another icon"
        );
        for used in ["💭", "✅", "🔔", "🧠"] {
            assert_ne!(SUBAGENT_ICON, used);
        }
        for ended in ["stopped", "failed"] {
            assert!(
                ts.contains(&format!("'{}'", ended_words(ended))),
                "cockpit/src/lib/subagent.ts says another of {ended}"
            );
        }
        assert_eq!(
            answer().ended_line("stopped"),
            "haiku · claude-haiku-5-5 · the turn that asked was stopped"
        );
    }

    /// The model part is the asked profile and the model that answered, the footer says it is a
    /// subagent's and who asked, and the cost and time are the call's own.
    #[test]
    fn the_footer_names_who_answered_and_who_asked() {
        assert_eq!(
            answer().footer(),
            "haiku · claude-haiku-5-5 · subagent answer, asked by sonnet · $0.0031 · 1.3 s"
        );
        let cut = SubagentAnswer {
            cut: true,
            asked_by: String::new(),
            cost_usd: None,
            ..answer()
        };
        assert_eq!(
            cut.footer(),
            "haiku · claude-haiku-5-5 · subagent answer · cut at its cap · 1.3 s"
        );
    }

    /// The terminal's footer names the profile, provider and model that answered, in the shape
    /// of a reply's status line.
    #[test]
    fn the_status_line_has_a_replys_shape() {
        assert_eq!(
            answer().status_line(),
            "[haiku → anthropic/claude-haiku-5-5 · subagent answer, asked by sonnet · $0.0031 · 1.3 s]"
        );
    }

    #[test]
    fn a_result_nodes_meta_gives_the_answer_back() {
        let meta = json!({"subagent": answer()});
        assert_eq!(SubagentAnswer::from_meta(&meta), Some(answer()));
        assert_eq!(SubagentAnswer::from_meta(&json!({"exit_code": 0})), None);
        assert_eq!(SubagentAnswer::from_meta(&json!(null)), None);
    }

    /// Work that was cancelled or failed says so in the footer, after what it is and who asked, and an
    /// answer or finished work says nothing more.
    #[test]
    fn an_outcome_that_is_not_done_is_in_the_footer() {
        let cancelled = SubagentAnswer {
            outcome: "cancelled".into(),
            ..answer()
        };
        assert_eq!(
            cancelled.footer(),
            "haiku · claude-haiku-5-5 · subagent answer, asked by sonnet · cancelled · $0.0031 · 1.3 s"
        );
        assert!(!answer().footer().contains("cancelled"));
        let wire = serde_json::to_value(answer()).unwrap();
        assert!(
            wire.get("outcome").is_none(),
            "an answer's wire shape is as it was: {wire}"
        );
    }
}

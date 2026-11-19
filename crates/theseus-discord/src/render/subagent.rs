//! A subagent's answer as a message of its own (issue 28): the asked model's words as the body
//! and a footer in a reply's shape, apart from `render.rs` for the shape budget's file ceiling.
//!
//! The footer's words are `SubagentAnswer::footer_bits`, the one place they are chosen, so the
//! terminal, the terminal UI and the cockpit say what this says: the model part names the profile
//! and the model that answered, never the turn's that asked.

use theseus_protocol::SubagentAnswer;

use super::{split_text, DISCORD_LIMIT, PART_LIMIT};

/// The message's parts, in order, under the call's own keys: its text, then the footer riding on
/// the last part when it fits, else standing alone.
pub fn subagent_parts(a: &SubagentAnswer) -> Vec<(String, String)> {
    let key = format!("subagent:{}", a.call);
    let mut out: Vec<(String, String)> = split_text(&a.opening(), PART_LIMIT)
        .into_iter()
        .enumerate()
        .map(|(i, part)| (format!("{key}:p{i}"), part))
        .collect();
    let footer = format!("-# {}", a.footer());
    match out.last_mut() {
        Some((_, c)) if c.len() + footer.len() < DISCORD_LIMIT => {
            c.push('\n');
            c.push_str(&footer);
        }
        _ => out.push((format!("{key}:footer"), footer)),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(text: &str) -> SubagentAnswer {
        SubagentAnswer {
            profile: "haiku".into(),
            provider: "anthropic".into(),
            model: "claude-haiku-5-5".into(),
            asked_by: "sonnet".into(),
            text: text.into(),
            cost_usd: Some(0.0031),
            elapsed_ms: 1_300,
            output_tokens: 12,
            cut: false,
            call: "act_1".into(),
        }
    }

    /// The words, then a footer of a reply's shape that names the model that answered and says
    /// it is a subagent's.
    #[test]
    fn the_footer_rides_the_last_part_and_names_the_model_that_answered() {
        let parts = subagent_parts(&answer("Hello."));
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, "subagent:act_1:p0");
        assert_eq!(
            parts[0].1,
            "🛰️ Hello.\n-# haiku · claude-haiku-5-5 · subagent answer, asked by sonnet · $0.0031 · 1.3 s"
        );
    }

    /// Words that fill a message leave the footer a message of its own, and every part is under
    /// Discord's limit.
    #[test]
    fn a_long_answer_is_split_and_its_footer_stands_alone_when_it_does_not_fit() {
        let long = "word ".repeat(1_000);
        let parts = subagent_parts(&answer(&long));
        assert!(parts.len() >= 3, "{}", parts.len());
        assert!(parts.iter().all(|(_, c)| c.len() < DISCORD_LIMIT));
        let all: String = parts.iter().map(|(_, c)| c.as_str()).collect();
        assert!(all.contains("-# haiku · claude-haiku-5-5 · subagent answer"));
        assert_eq!(
            parts.iter().filter(|(_, c)| c.contains("-# haiku")).count(),
            1
        );
    }
}

//! A question with options (`owner_ask`, theseus-j7cje) in the terminal: the
//! question, its options numbered, and how to answer by number.

use theseus_protocol::work::ask::AskInput;
use theseus_protocol::ConfirmRequest;

use super::{push, Line, Tag};

/// The question's lines, for an event and for `theseus answer`'s list.
pub(super) fn lines(out: &mut Vec<Line>, c: &ConfirmRequest) {
    let ask = AskInput::parse(&c.input).ok();
    let question = ask.as_ref().map_or("a question", |a| a.question.as_str());
    push(out, Tag::Ask, &format!("  ? {question}"));
    for (i, o) in ask.iter().flat_map(|a| a.options.iter()).enumerate() {
        let detail = o.detail.as_deref().map(|d| format!(" — {d}"));
        push(
            out,
            Tag::Plain,
            &format!("      {}. {}{}", i + 1, o.label, detail.unwrap_or_default()),
        );
    }
    let fallback = ask.map_or("no default".to_string(), |a| a.on_expiry());
    push(
        out,
        Tag::Dim,
        &format!(
            "      answer: theseus answer {} <number>   (none of them: other --note <words>) · if no one answers: {fallback}",
            c.correlation_id
        ),
    );
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The question, each option numbered, and the command that answers it by
    /// number, with what happens at expiry.
    #[test]
    fn a_question_with_options_lists_them_and_says_how_to_answer() {
        let c = ConfirmRequest {
            correlation_id: "act_00reef".into(),
            tool: "owner_ask".into(),
            input: json!({"question": "Which plan?", "default": 2,
                          "options": [{"label": "Careful"}, {"label": "Fast", "detail": "skips the check"}]}),
            ..Default::default()
        };
        let mut out = Vec::new();
        lines(&mut out, &c);
        let text: Vec<&str> = out.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text[0], "  ? Which plan?");
        assert_eq!(text[1], "      1. Careful");
        assert_eq!(text[2], "      2. Fast — skips the check");
        assert!(
            text[3].contains("theseus answer act_00reef <number>")
                && text[3].contains("default 2: Fast"),
            "{}",
            text[3]
        );
    }
}

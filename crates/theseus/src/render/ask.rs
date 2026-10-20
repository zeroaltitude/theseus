//! A question with options (`owner_ask`, theseus-j7cje) in the terminal: the
//! question, its options numbered, and how to answer by number.

use theseus_protocol::work::ask::AskInput;
use theseus_protocol::ConfirmRequest;

use super::{push, Line, Tag};

/// An approval in plain words (theseus-cudar): who wants to do what, then
/// the risk and the model's reason, and what Yes and No do. False when the
/// daemon sent none, and the caller says the gate's reason instead.
pub(super) fn plain_lines(out: &mut Vec<Line>, c: &ConfirmRequest) -> bool {
    let Some(p) = &c.plain else {
        return false;
    };
    let mut words = p.lines().into_iter();
    if let Some(first) = words.next() {
        push(out, Tag::Ask, &format!("  ? {first}"));
    }
    for l in words {
        push(out, Tag::Dim, &format!("      {l}"));
    }
    true
}

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

    /// `theseus answer` words an approval (theseus-cudar): who wants to do
    /// what, the risk and the model's reason, what Yes and No do, then how to
    /// answer; a daemon that sends no plain words keeps the gate's reason.
    #[test]
    fn an_approval_is_worded_for_the_answer_list() {
        let mut c = theseus_protocol::ConfirmRequest {
            correlation_id: "act_0000feed".into(),
            session_id: "ses_00000000harbour".into(),
            tool: "fs.read".into(),
            reason: "fs.read — approve (enforcement = approve)".into(),
            ..Default::default()
        };
        let old: Vec<String> = super::super::confirm_lines(&c)
            .iter()
            .map(|l| l.text.clone())
            .collect();
        assert!(
            old[0].contains("fs.read waits for you in ses_00000000harbour"),
            "{old:?}"
        );
        c.plain = Some(theseus_protocol::PlainWords {
            who: "Theseus's self-repair: fix the tides".into(),
            what: "read its own report from the 05:12 run".into(),
            why: "to see why the last run stopped".into(),
            held: String::new(),
            risk: "read-only".into(),
            yes: "runs this one call.".into(),
            no: "refuses it; the session is told and carries on without it.".into(),
        });
        let lines: Vec<String> = super::super::confirm_lines(&c)
            .iter()
            .map(|l| l.text.clone())
            .collect();
        let all = lines.join("\n");
        assert!(
            lines[0].contains(
                "self-repair: fix the tides wants to read its own report from the 05:12 run"
            ),
            "{all}"
        );
        for must in [
            "risk: read-only",
            "why: to see why",
            "Yes: runs this one call.",
            "No: refuses it",
        ] {
            assert!(all.contains(must), "{must} in {all}");
        }
        assert!(
            !all.contains("ses_00000000harbour") && !all.contains("enforcement"),
            "{all}"
        );
        assert!(all.contains("theseus answer act_0000feed approve"), "{all}");
    }
}

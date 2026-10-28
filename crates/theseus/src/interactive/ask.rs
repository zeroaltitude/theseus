//! A question with options (`owner_ask`) in a pane (theseus-ypnxt.3): the pane prints it numbered,
//! as the chat does, and a number answers it with `work.answer` (what `theseus answer ID N`
//! sends). A plain `y/N` approval has no such answer: the daemon refuses a yes that chooses
//! nothing, so the pane says the keys again and sends nothing. Words of the owner's own are the
//! question's `other`, with the words as the note. The question has no decline, so there is no `n`.

use serde_json::Value;
use theseus_protocol::work::ask::{choice_id, AskInput, OWNER_ASK};
use theseus_protocol::ConfirmRequest;

/// Whether the question is a question with options.
pub(super) fn is_ask(q: &ConfirmRequest) -> bool {
    q.tool == OWNER_ASK
}

/// How many options the question has.
fn count(q: &ConfirmRequest) -> usize {
    AskInput::parse(&q.input).map_or_else(
        |_| q.options.iter().filter(|o| o.id != "choice:other").count(),
        |a| a.options.len(),
    )
}

/// The question and its options, numbered from 1.
pub(super) fn prompt_lines(q: &ConfirmRequest) -> Vec<String> {
    let Ok(a) = AskInput::parse(&q.input) else {
        return Vec::new();
    };
    let mut out = vec![format!("  ? {}", a.question)];
    for (i, o) in a.options.iter().enumerate() {
        let detail = o.detail.as_deref().map(|d| format!(" — {d}"));
        out.push(format!(
            "      {}. {}{}",
            i + 1,
            o.label,
            detail.unwrap_or_default()
        ));
    }
    out
}

/// The keys, as the prompt's last line.
pub(super) fn keys_line(q: &ConfirmRequest) -> String {
    format!("choose? [1-{}, or your own words]", count(q))
}

/// What a line says to the question.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Said {
    /// Answer with this option, and these words.
    Send {
        option: String,
        note: Option<String>,
    },
    /// Not an answer to it: say the keys.
    Keys,
}

/// A line against the question: a number from 1 to the last option picks it (words after it are
/// the note); a bare `y`, `n` or `t`, a number out of range and an empty line are no answer; other
/// words are `other`, in the owner's own.
pub(super) fn said(q: &ConfirmRequest, line: &str) -> Said {
    let line = line.trim();
    let (head, rest) = match line.split_once(char::is_whitespace) {
        Some((h, r)) => (h, Some(r.trim().to_string()).filter(|r| !r.is_empty())),
        None => (line, None),
    };
    if !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()) {
        return match head.parse::<usize>() {
            Ok(n) if (1..=count(q)).contains(&n) => Said::Send {
                option: choice_id(n - 1),
                note: rest,
            },
            _ => Said::Keys,
        };
    }
    if matches!(
        line.to_ascii_lowercase().as_str(),
        "" | "y" | "yes" | "n" | "no"
    ) {
        return Said::Keys;
    }
    Said::Send {
        option: "choice:other".to_string(),
        note: Some(line.to_string()),
    }
}

/// The text of a turn's reply, from a `turn.submit` result.
pub(super) fn reply_of(result: Option<Value>) -> Option<String> {
    let out = result?.get("output")?.as_str()?.trim().to_string();
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn three() -> ConfirmRequest {
        ConfirmRequest {
            correlation_id: "act_00reef".into(),
            tool: OWNER_ASK.into(),
            input: json!({"question": "Which plan?", "options": [
                {"label": "Careful"}, {"label": "Fast", "detail": "skips the check"},
                {"label": "Skip it"}]}),
            ..Default::default()
        }
    }

    fn send(option: &str, note: Option<&str>) -> Said {
        Said::Send {
            option: option.into(),
            note: note.map(str::to_string),
        }
    }

    /// The prompt numbers the options from 1 and says the keys.
    #[test]
    fn the_prompt_numbers_the_options_and_says_the_keys() {
        let q = three();
        assert_eq!(
            prompt_lines(&q),
            [
                "  ? Which plan?",
                "      1. Careful",
                "      2. Fast — skips the check",
                "      3. Skip it"
            ]
        );
        assert_eq!(keys_line(&q), "choose? [1-3, or your own words]");
    }

    /// A number is the option at that place from 1, and words after it are its note.
    #[test]
    fn a_number_picks_the_option_at_its_place() {
        let q = three();
        assert_eq!(said(&q, "1"), send("choice:0", None));
        assert_eq!(said(&q, " 2 "), send("choice:1", None));
        assert_eq!(
            said(&q, "3 only if it is cheap"),
            send("choice:2", Some("only if it is cheap"))
        );
    }

    /// An approval's answers are no answer to it, and neither is a number past the last option.
    #[test]
    fn a_yes_or_a_number_out_of_range_sends_nothing() {
        let q = three();
        for l in [
            "y",
            "Yes",
            "n",
            "no",
            "",
            "0",
            "4",
            "7 of 9",
            "99999999999999999999999",
        ] {
            assert_eq!(said(&q, l), Said::Keys, "{l:?}");
        }
    }

    /// Words of the owner's own are `other`, with the words as the note.
    #[test]
    fn other_words_are_the_owners_own() {
        let q = three();
        assert_eq!(
            said(&q, "use the staging port"),
            send("choice:other", Some("use the staging port"))
        );
    }
}
